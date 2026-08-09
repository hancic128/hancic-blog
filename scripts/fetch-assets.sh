#!/usr/bin/env bash
# 下载 ip2region 数据资产（IPv4 库，约 11MB）到 assets/ip2region.xdb，提交入库。
#
# 上游 master 分支已把文件重命名为 data/ip2region_v4.xdb，旧名 ip2region.xdb
# 已 404，因此按 旧名→新名 依次尝试 GitHub 与 gitee 镜像；
# 直连失败时自动经本地代理（默认 127.0.0.1:7897，可设 PROXY 覆盖）重试。
set -euo pipefail

cd "$(dirname "$0")/.."
DEST="assets/ip2region.xdb"
PROXY="${PROXY:-http://127.0.0.1:7897}"
MIN_BYTES=1048576  # 1MB

mkdir -p assets

URLS=(
  "https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region.xdb"
  "https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region_v4.xdb"
  "https://gitee.com/lionsoul/ip2region/raw/master/data/ip2region.xdb"
  "https://gitee.com/lionsoul/ip2region/raw/master/data/ip2region_v4.xdb"
)

# $1=url  $2=proxy(可空)
download() {
  local url="$1" proxy="$2"
  if [ -n "$proxy" ]; then
    curl -fL --http1.1 --connect-timeout 15 --max-time 120 -x "$proxy" -o "$DEST.tmp" "$url"
  else
    curl -fL --http1.1 --connect-timeout 15 --max-time 120 -o "$DEST.tmp" "$url"
  fi
}

# 校验下载内容并把临时文件落到最终位置；失败清理 .tmp 并返回非 0。
verify() {
  if [ ! -s "$DEST.tmp" ]; then
    echo "   下载为空，丢弃" >&2
    rm -f "$DEST.tmp"
    return 1
  fi
  local size
  size=$(wc -c < "$DEST.tmp")
  if [ "$size" -lt "$MIN_BYTES" ]; then
    echo "   文件过小（$size 字节 < ${MIN_BYTES}），丢弃" >&2
    rm -f "$DEST.tmp"
    return 1
  fi
  mv "$DEST.tmp" "$DEST"
  echo "OK: ${DEST}（${size} 字节）"
  return 0
}

for url in "${URLS[@]}"; do
  echo "==> 尝试: $url"
  if download "$url" "" && verify; then exit 0; fi
  if [ -n "$PROXY" ]; then
    echo "   直连失败，经代理 $PROXY 重试"
    if download "$url" "$PROXY" && verify; then exit 0; fi
  fi
done

echo "!! 全部源下载失败" >&2
exit 1
