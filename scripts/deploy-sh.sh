#!/usr/bin/env bash
# =============================================================================
# deploy-sh.sh —— 上海主机 hancic 部署 / 回滚脚本
#
# 在部署目标（上海主机）上执行。本脚本只做本地容器编排与镜像
# 拉取，不触碰 halo（保留 8090 可回滚），也不连接生产环境外部服务。
#
# 用法：
#   ./deploy-sh.sh deploy  [--pull-mode a|b] [--auto-rollback]  部署（拉取 + 起服务 + 健康轮询）
#   ./deploy-sh.sh rollback                                     回退到上一版镜像（:prev，别名 --rollback）
#   ./deploy-sh.sh status                                       容器状态 + 内存 + 健康检查
#   ./deploy-sh.sh health                                       健康检查单次探测
#
# 镜像拉取两方案（详见 docs/deploy-sh.md §2，默认方案 B，优先）：
#   方案 A：从国内镜像源拉取（HANCIC_PULL_A_IMAGE 指定实际拉取名，自动 retag）
#   方案 B：经 usa 中转机（HANCIC_RELAY）docker pull + docker save | ssh docker load
#
# 环境变量（均可覆盖默认值）：
#   HANCIC_IMAGE            镜像全名（默认 ghcr.io/angryshark128/hancic:latest）
#   HANCIC_PULL_MODE        拉取方案 a|b（默认 b，等价于 --pull-mode）
#   HANCIC_PULL_A_IMAGE     方案 A 实际拉取的镜像名（如国内镜像源地址）
#   HANCIC_RELAY            usa 中转机 ssh 目标（必填，方案 B 用：root@<USA_IP>）
#   HANCIC_PORT             宿主机映射端口（默认 8091；halo 保留 8090 不动）
#   HANCIC_CONTAINER_PORT   容器内监听端口（默认 8090，须与 config.toml 一致）
#   HANCIC_DATA_DIR         宿主机数据目录（默认 /data/hancic，绑定容器 /data）
#   HANCIC_DEPLOY_DIR       compose 文件目录（默认 /opt/hancic）
#   HANCIC_UID              bind 目录属主 uid（默认 1000，镜像内 hancic 用户）
#   HEALTH_RETRIES          健康轮询次数（默认 30）
#   HEALTH_INTERVAL         健康轮询间隔秒（默认 5）
# =============================================================================
set -euo pipefail

# ---- 默认值（可被环境变量覆盖） ----
HANCIC_IMAGE="${HANCIC_IMAGE:-ghcr.io/angryshark128/hancic:latest}"
HANCIC_PULL_MODE="${HANCIC_PULL_MODE:-b}"
HANCIC_RELAY="${HANCIC_RELAY:-}"
HANCIC_PORT="${HANCIC_PORT:-8091}"
HANCIC_CONTAINER_PORT="${HANCIC_CONTAINER_PORT:-8090}"
HANCIC_DATA_DIR="${HANCIC_DATA_DIR:-/data/hancic}"
HANCIC_DEPLOY_DIR="${HANCIC_DEPLOY_DIR:-/opt/hancic}"
HANCIC_UID="${HANCIC_UID:-1000}"
HEALTH_RETRIES="${HEALTH_RETRIES:-30}"
HEALTH_INTERVAL="${HEALTH_INTERVAL:-5}"

COMPOSE_FILE="${HANCIC_DEPLOY_DIR}/docker-compose.yaml"
HEALTH_URL="http://127.0.0.1:${HANCIC_PORT}/api/health"

log()  { echo "[hancic] $*"; }
die()  { echo "[hancic] 错误：$*" >&2; exit 1; }

usage() {
  sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
}

# 上一版镜像标签：ghcr.io/angryshark128/hancic:latest -> ...:prev
prev_tag() {
  local img="$1"
  if [[ "$img" == *:* ]]; then
    echo "${img%:*}:prev"
  else
    echo "${img}:prev"
  fi
}

# docker compose 兼容（v2 优先，其次 docker-compose）
compose_cmd() {
  if docker compose version >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" "$@"
  elif command -v docker-compose >/dev/null 2>&1; then
    docker-compose -f "$COMPOSE_FILE" "$@"
  else
    die "需要 docker compose v2 或 docker-compose"
  fi
}

# 健康探测：优先 curl，其次 wget
http_ok() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsS -m 5 "$1" >/dev/null 2>&1
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- -T 5 "$1" >/dev/null 2>&1
  else
    die "需要 curl 或 wget"
  fi
}

# 生成 compose 文件（每次部署/回滚按当前参数重写，勿手改）
write_compose() {
  local image="$1"
  mkdir -p "$HANCIC_DEPLOY_DIR"
  cat > "$COMPOSE_FILE" <<EOF
# 由 deploy-sh.sh 生成，勿手改（每次部署/回滚会按当前参数覆盖）
services:
  hancic:
    image: ${image}
    container_name: hancic
    restart: unless-stopped
    ports:
      - "${HANCIC_PORT}:${HANCIC_CONTAINER_PORT}"
    volumes:
      - ${HANCIC_DATA_DIR}:/data
    environment:
      - RUST_LOG=info
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://127.0.0.1:${HANCIC_CONTAINER_PORT}/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 15s
EOF
  log "compose 文件已写入 ${COMPOSE_FILE}（image=${image}, 端口 ${HANCIC_PORT}->${HANCIC_CONTAINER_PORT}）"
}

# 数据目录预置：目录 + config.toml 种子（已存在不覆盖）+ bind 属主
prepare_data_dir() {
  mkdir -p "$HANCIC_DATA_DIR"
  if [ -f "$HANCIC_DATA_DIR/config.toml" ]; then
    log "使用已有 config.toml（不覆盖）：${HANCIC_DATA_DIR}/config.toml"
  else
    cat > "$HANCIC_DATA_DIR/config.toml" <<EOF
host = "0.0.0.0"
port = ${HANCIC_CONTAINER_PORT}
data_dir = "/data"
site_name = "寒蝉 Hancic"
site_desc = "记录与思考"
active_theme = "default"
image_compress = true
image_max_edge = 2000
image_quality = 85
upload_max_image = 10485760
upload_max_video = 104857600
upload_max_file = 52428800
EOF
    log "已写入默认 config.toml（data_dir=/data, 端口 ${HANCIC_CONTAINER_PORT}）"
  fi
  if ! chown -R "${HANCIC_UID}:${HANCIC_UID}" "$HANCIC_DATA_DIR" 2>/dev/null; then
    log "警告：chown 失败（需要 root）。请手工执行：chown -R ${HANCIC_UID}:${HANCIC_UID} ${HANCIC_DATA_DIR}"
  fi
  log "数据目录就绪：${HANCIC_DATA_DIR}（属主 uid=${HANCIC_UID}，容器内 hancic 用户）"
}

# 镜像拉取（方案 A / 方案 B）
pull_image() {
  local mode="$1"
  case "$mode" in
    a|A)
      if [ -n "${HANCIC_PULL_A_IMAGE:-}" ]; then
        log "方案 A：从 ${HANCIC_PULL_A_IMAGE} 拉取，retag 为 ${HANCIC_IMAGE}"
        docker pull "$HANCIC_PULL_A_IMAGE"
        docker tag "$HANCIC_PULL_A_IMAGE" "$HANCIC_IMAGE"
      else
        log "方案 A（未设 HANCIC_PULL_A_IMAGE）：直接 docker pull ${HANCIC_IMAGE}"
        log "  提示：ghcr 国内直连不稳定；失败请设 HANCIC_PULL_A_IMAGE 或改用方案 B"
        docker pull "$HANCIC_IMAGE"
      fi
      ;;
    b|B)
      [ -n "$HANCIC_RELAY" ] || die "方案 B 需要 HANCIC_RELAY（usa 中转机 ssh 目标，如 root@<USA_IP>）"
      log "方案 B：经 ${HANCIC_RELAY} 中转拉取 ${HANCIC_IMAGE}"
      ssh -o BatchMode=yes -o ConnectTimeout=10 "$HANCIC_RELAY" "docker pull $HANCIC_IMAGE"
      log "usa 拉取完成，docker save 经 ssh 管道传回并 docker load（按镜像大小需数分钟）"
      ssh -o BatchMode=yes -o ConnectTimeout=10 "$HANCIC_RELAY" "docker save $HANCIC_IMAGE" | docker load
      ;;
    *)
      die "未知拉取方案：${mode}（可选 a|b）"
      ;;
  esac
  docker image inspect "$HANCIC_IMAGE" >/dev/null \
    && log "镜像就绪：${HANCIC_IMAGE}"
}

# 健康检查轮询（到点即成功）
wait_health() {
  local i
  for i in $(seq 1 "$HEALTH_RETRIES"); do
    if http_ok "$HEALTH_URL"; then
      log "健康检查通过：${HEALTH_URL}"
      return 0
    fi
    log "健康检查等待中（${i}/${HEALTH_RETRIES}，每 ${HEALTH_INTERVAL}s）..."
    sleep "$HEALTH_INTERVAL"
  done
  return 1
}

# ---------- 子命令 ----------

run_deploy() {
  local mode="$1" auto_rollback="$2" prev cur
  prev="$(prev_tag "$HANCIC_IMAGE")"

  prepare_data_dir

  # 部署前把当前运行镜像标记为 :prev（仅当 hancic 容器存在且曾启动过）
  if docker ps -a --format '{{.Names}}' | grep -qx 'hancic'; then
    if docker ps --format '{{.Names}}' | grep -qx 'hancic'; then
      cur="$(docker inspect -f '{{.Image}}' hancic)"
      docker tag "$cur" "$prev" 2>/dev/null || true
      log "当前运行镜像 ${cur} 已标记为 ${prev}（回滚用）"
    else
      log "容器 hancic 存在但未运行，跳过 :prev 标记"
    fi
  else
    log "首次部署（无既有 hancic 容器），无 :prev 标记"
  fi

  write_compose "$HANCIC_IMAGE"
  pull_image "$mode"

  log "启动服务：docker compose -f ${COMPOSE_FILE} up -d"
  compose_cmd up -d

  if wait_health; then
    log "部署成功 ✅  对外地址 http://127.0.0.1:${HANCIC_PORT}/api/health"
    log "下一步："
    log "  1) 浏览器打开 /admin/setup 设置密码（≥8 位）"
    log "  2) /admin/tokens 生成部署 Token（hc_ 开头，仅显示一次，立即保存）"
    log "  3) /admin/migrate 导入 Halo zip（勾选下载图片）→ /admin/backup 全量备份留档"
    log "  4) 北京 nginx proxy_pass 改指 ${HANCIC_PORT}（详见 docs/deploy-sh.md §7）"
  else
    log "部署失败 ❌：健康检查 ${HEALTH_RETRIES}x${HEALTH_INTERVAL}s 内未通过"
    log "诊断：docker compose -f ${COMPOSE_FILE} logs --tail=100 hancic"
    if [ "$auto_rollback" = true ]; then
      log "--auto-rollback 开启，自动回退到 ${prev}..."
      run_rollback || true
    else
      log "回滚：./deploy-sh.sh rollback（切回 ${prev}；对外流量则先改北京 nginx 指回 8090）"
    fi
    exit 1
  fi
}

run_rollback() {
  local prev
  prev="$(prev_tag "$HANCIC_IMAGE")"
  if ! docker image inspect "$prev" >/dev/null 2>&1; then
    die "未找到 ${prev}（尚无成功部署过的前版镜像），无法回退"
  fi
  log "使用上一版镜像 ${prev} 重启服务"
  write_compose "$prev"
  compose_cmd up -d
  if wait_health; then
    log "回退成功 ✅ 已运行 ${prev}（对外地址 http://127.0.0.1:${HANCIC_PORT}）"
  else
    log "回退后健康检查失败 ❌ 请检查：docker compose -f ${COMPOSE_FILE} logs --tail=100 hancic"
    exit 1
  fi
}

run_status() {
  compose_cmd ps || true
  echo "---"
  docker stats --no-stream --format "table {{.Name}}\t{{.MemUsage}}\t{{.CPUPerc}}" hancic 2>/dev/null || true
  if http_ok "$HEALTH_URL"; then
    log "健康检查：OK"
  else
    log "健康检查：FAIL"
  fi
}

run_health() {
  if http_ok "$HEALTH_URL"; then
    log "健康检查：OK（${HEALTH_URL}）"
  else
    log "健康检查：FAIL（${HEALTH_URL}）"
    exit 1
  fi
}

# ---------- 入口 ----------

main() {
  local cmd="${1:-}" pull_mode="" auto_rollback=false
  # 首参允许直接是全局旗标（-h/--help / --rollback）
  case "$cmd" in
    -h|--help)  usage; exit 0 ;;
    --rollback) run_rollback; exit 0 ;;
    -*)         die "未知子命令：${cmd}（可用 deploy|rollback|status|health）" ;;
    "")         usage; exit 1 ;;
  esac
  shift || true

  while [ $# -gt 0 ]; do
    case "$1" in
      --pull-mode)        [ $# -ge 2 ] || die "--pull-mode 需要参数 a|b"; pull_mode="$2"; shift 2 ;;
      --pull-mode=*)      pull_mode="${1#*=}"; shift ;;
      --auto-rollback)    auto_rollback=true; shift ;;
      -h|--help)          usage; exit 0 ;;
      *)                  die "未知参数：$1（-h 查看用法）" ;;
    esac
  done

  # --pull-mode 优先于环境变量 HANCIC_PULL_MODE
  pull_mode="${pull_mode:-$HANCIC_PULL_MODE}"

  case "$cmd" in
    deploy)   run_deploy "$pull_mode" "$auto_rollback" ;;
    rollback) run_rollback ;;
    status)   run_status ;;
    health)   run_health ;;
    *)        die "未知子命令：${cmd}（可用 deploy|rollback|status|health）" ;;
  esac
}

main "$@"
