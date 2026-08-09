#!/bin/sh
# hancic 容器入口：
#   每次启动用镜像内内置 default 主题覆盖数据卷（原子替换，避免旧主题残留；
#   用户自定义主题请复制到 themes/ 新目录，勿改 default），然后透传 CMD。
set -eu

echo "[entrypoint] 同步内置主题 default → /data/themes/"
mkdir -p /data/themes
tmp="/data/themes/.default.$$"
rm -rf "$tmp"
cp -r /app/themes/default "$tmp"
rm -rf /data/themes/default
mv "$tmp" /data/themes/default

exec "$@"
