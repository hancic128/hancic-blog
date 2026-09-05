#!/usr/bin/env bash
# hancic 本地测试→部署脚本（不走 CI，优先本地验证）：
#   1) 本机 cargo test（+可选 E2E）确认全绿
#   2) 打包源码 scp 上海
#   3) 上海 docker build（cache mount + rsproxy，首次全量约 25min，之后增量约 2min）
#   4) compose 重建容器（entrypoint 自动同步主题）
#   5) 健康检查
set -euo pipefail

SH_HOST="${HANCIC_SH_HOST:?请设置 HANCIC_SH_HOST（上海部署主机 IP/别名）}"
SH_DIR="/opt/hancic"
RUN_E2E="${RUN_E2E:-1}"

echo "==> [1/5] 本机测试"
cargo test 2>&1 | grep -cE "test result: ok" | xargs -I{} echo "    {} 组测试通过"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
if [ "$RUN_E2E" = "1" ]; then
  (cd e2e && npx playwright test 2>&1 | tail -1)
fi

echo "==> [2/5] 打包源码并上传上海"
PKG="/tmp/hancic-src-local.tar.gz"
# COPYFILE_DISABLE=1：macOS tar 不把 xattr 编码成 AppleDouble（._* 条目），
# 否则解包进构建上下文后会随 COPY . . 进镜像、被 entrypoint 同步进数据卷，
# Tera 会把 ._*.html 当模板导致主题加载失败（8-28 遗留 54 个 ._ 文件的教训）。
COPYFILE_DISABLE=1 tar czf "$PKG" --exclude=.git --exclude=.superpowers --exclude=target \
  --exclude=data --exclude=data-e2e --exclude="data*" --exclude=e2e \
  --exclude=.cargo -C "$(dirname "$0")/.." . 2>/dev/null
scp -o BatchMode=yes -o ConnectTimeout=10 "$PKG" "root@${SH_HOST}:${SH_DIR}/" >/dev/null

echo "==> [3/5] 上海构建镜像（cache mount 增量）"
ssh -o BatchMode=yes "root@${SH_HOST}" "cd ${SH_DIR}/src && tar xzf ../$(basename "$PKG") --overwrite 2>/dev/null || true && docker build --build-arg CARGO_SOURCE_INDEX='sparse+https://rsproxy.cn/index/' -t hancic:latest ."

echo "==> [4/5] 重建容器"
ssh -o BatchMode=yes "root@${SH_HOST}" "cd ${SH_DIR} && docker compose up -d --force-recreate hancic && sleep 8"

echo "==> [5/5] 健康检查"
ssh -o BatchMode=yes "root@${SH_HOST}" "curl -sf http://127.0.0.1:8090/api/health && echo ' 部署 OK'" \
  && echo "=== 部署完成: https://hancic.site ==="
