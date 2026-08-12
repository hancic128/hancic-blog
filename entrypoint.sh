#!/bin/sh
# hancic 容器入口：
#   每次启动用镜像内全部内置主题（default/modern 等）覆盖数据卷同名目录
#   （原子替换，避免旧主题残留；用户自定义主题请用其它目录名，勿改内置主题名），
#   然后透传 CMD。
set -eu

echo "[entrypoint] 同步内置主题 → /data/themes/"
mkdir -p /data/themes
for t in /app/themes/*/; do
  name=$(basename "$t")
  tmp="/data/themes/.$name.$$"
  rm -rf "$tmp"
  cp -r "$t" "$tmp"
  rm -rf "/data/themes/$name"
  mv "$tmp" "/data/themes/$name"
done

exec "$@"
