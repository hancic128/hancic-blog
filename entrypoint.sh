#!/bin/sh
# hancic 容器入口：
#   首次启动把内置 default 主题种子写入数据卷 /data（已存在则跳过，不覆盖用户改动），
#   然后原样透传 CMD（/app/hancic /app/config.toml）。
set -eu

if [ ! -f /data/themes/default/theme.toml ]; then
  echo "[entrypoint] 初始化内置主题 default → /data/themes/"
  mkdir -p /data/themes
  # 先写临时目录再 mv（同文件系统内原子）：中断不会留下半套主题；
  # 仅当 theme.toml 缺失才进入本分支，此时旧目录必为残留/损坏种子，可安全移除。
  tmp="/data/themes/.default.$$"
  rm -rf "$tmp" /data/themes/default
  cp -r /app/themes/default "$tmp"
  mv "$tmp" /data/themes/default
fi

exec "$@"
