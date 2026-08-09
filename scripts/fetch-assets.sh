#!/usr/bin/env bash
# 下载前端资产（Vditor、Chart.js、ip2region）到 assets/，全部提交入库。
#
# - Vditor：官方 GitHub release 已不再附带 tar 资产（Vditor/vditor 仓库迁移至
#   Vanessa219/vditor 后，新版本 release 的 assets 为空），改从 npm 打包下载；
#   npm 包内产物名已从 vditor.min.js/css 更名为 index.min.js/index.css，
#   落盘时沿用约定文件名，供 T13 编辑器引用。
# - Chart.js：GitHub release 单文件，失败回退 jsdelivr CDN。
# - ip2region：GitHub raw → gitee 镜像逐源尝试（旧名 ip2region.xdb 已 404，
#   按 新名 ip2region_v4.xdb 再试）。
# 直连失败时自动经本地代理（默认 127.0.0.1:7897，可设 PROXY 覆盖）重试。
set -euo pipefail

cd "$(dirname "$0")/.."
PROXY="${PROXY:-http://127.0.0.1:7897}"
mkdir -p assets/vendor

TMP="$(mktemp -d "${TMPDIR:-/tmp}/hancic-assets.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# $1=url  $2=目标文件  $3=代理(可空)
download() {
  local url="$1" dst="$2" proxy="$3"
  if [ -n "$proxy" ]; then
    curl -fsSL --http1.1 --connect-timeout 15 --max-time 120 -x "$proxy" -o "$dst" "$url"
  else
    curl -fsSL --http1.1 --connect-timeout 15 --max-time 120 -o "$dst" "$url"
  fi
}

# 多源回退下载：$1=目标文件，其后为 URL 列表；每源先直连后代理，成功即返回 0。
# 先写临时文件，成功后才 mv 到目标，避免失败时截断已入库的资产。
fetch() {
  local dst="$1"; shift
  local part="$TMP/$(basename "$dst").part"
  rm -f "$part"
  for url in "$@"; do
    if download "$url" "$part" ""; then
      mv "$part" "$dst"
      echo "OK: $(basename "$dst") ← $url"
      return 0
    fi
    if [ -n "$PROXY" ] && download "$url" "$part" "$PROXY"; then
      mv "$part" "$dst"
      # 注意：macOS bash 3.2 对「$var 紧跟多字节字符」有解析 bug，
      # 变量一律用 ${var} 花括号形式（见 set -u 下的 unbound variable 误报）。
      echo "OK: $(basename "$dst") ← ${url}（经代理 ${PROXY}）"
      return 0
    fi
  done
  rm -f "$part"
  echo "!! 下载失败: $(basename "$dst")" >&2
  return 1
}

fail() {
  echo "!! $*" >&2
  exit 1
}

# ---- Vditor ----
VDITOR_VERSION="3.11.2"   # npm 包 latest（2026-08 实测；升级时改这里并重新入库）
fetch "$TMP/vditor.tgz" \
  "https://registry.npmjs.org/vditor/-/vditor-${VDITOR_VERSION}.tgz" \
  || fail "Vditor npm 包下载失败"
tar -xzf "$TMP/vditor.tgz" -C "$TMP" package/dist/index.min.js package/dist/index.css
[ -s "$TMP/package/dist/index.min.js" ] && [ -s "$TMP/package/dist/index.css" ] \
  || fail "Vditor 包结构异常：缺少 dist/index.min.js / dist/index.css"
mv "$TMP/package/dist/index.min.js" assets/vendor/vditor.min.js
mv "$TMP/package/dist/index.css" assets/vendor/vditor.min.css
echo "OK: vditor.min.js / vditor.min.css（Vditor ${VDITOR_VERSION}）"

# ---- Chart.js ----
fetch assets/vendor/chart.umd.min.js \
  "https://github.com/chartjs/Chart.js/releases/latest/download/chart.umd.min.js" \
  "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js" \
  || fail "Chart.js 下载失败"

# ---- ip2region ----
fetch assets/ip2region.xdb \
  "https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region.xdb" \
  "https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region_v4.xdb" \
  "https://gitee.com/lionsoul/ip2region/raw/master/data/ip2region.xdb" \
  "https://gitee.com/lionsoul/ip2region/raw/master/data/ip2region_v4.xdb" \
  || fail "ip2region 下载失败"
if [ "$(wc -c < assets/ip2region.xdb)" -lt 1048576 ]; then
  fail "ip2region.xdb 过小（可能下载到错误页）"
fi

echo "assets 就绪"
