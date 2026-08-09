#!/bin/sh
# hancic 容器入口：
#   首次启动把内置 default 主题种子写入数据卷 /data（已存在则跳过，不覆盖用户改动），
#   然后原样透传 CMD（/app/hancic /app/config.toml）。
set -eu

if [ ! -f /data/themes/default/theme.toml ]; then
  echo "[entrypoint] 初始化内置主题 default → /data/themes/"
  mkdir -p /data/themes
  cp -r /app/themes/default /data/themes/
fi

exec "$@"
