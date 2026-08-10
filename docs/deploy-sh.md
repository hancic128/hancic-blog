# hancic 上海主机上线手册

> 目标：把 hancic（ghcr.io/angryshark128/hancic:latest）部署到上海主机，经北京 nginx 对外提供 `https://hancic.site/`，并从 Halo 迁移真实数据。全程保留 halo 可回滚，观察稳定后再停 halo。
> 配套脚本：`scripts/deploy-sh.sh`（在上海主机执行，含部署 / 回滚 / 状态子命令）。

---

## 0. 目标架构与端口约定

| 角色 | 主机 | 说明 |
|---|---|---|
| 上海（sh） | 172.81.241.149（4C8G，wjbd.cloud） | **无法访问 GitHub**；运行 hancic 容器；halo 保留 |
| 北京（bj） | 49.232.168.161（hancic.site） | **无法访问 GitHub**；docker my-nginx 反代根路径 |
| 硅谷（usa） | 170.106.103.36（wjbd.site） | 可访问 GitHub；作镜像中转（先例：server-monitor / my-nginx） |
| 本机 Mac | — | 可访问 GitHub（代理 127.0.0.1:7897）；gh CLI 已认证 |

**端口约定（关键）**：

- halo：上海 `8090`，**全程不动**（回滚保障）
- hancic：容器内监听 `8090`（config.toml 默认），宿主机映射 **`8091`**（与 halo 不冲突）
- 流量：`hancic.site` → 北京 my-nginx（`proxy_pass` 上海 `8091`）→ hancic

```
浏览器 → https://hancic.site/ → 北京 49.232.168.161:443 (my-nginx)
                              → proxy_pass http://172.81.241.149:8091 → hancic 容器(8090)
```

**部署顺序**：镜像拉取 → 预置数据目录 → 首启（/admin/setup + /admin/tokens）→ 迁移（/admin/migrate）→ 备份（/admin/backup）→ 北京 nginx 切 8091 → 验收 → 观察期 → 停 halo。

---

## 1. 前置条件

### 1.1 上海主机

```bash
# Docker 与 compose v2
docker version --format '{{.Server.Version}}'     # 预期：v20.10+ / 24.x
docker compose version                            # 预期：Docker Compose version v2.x

# 脚本依赖（健康检查用 curl 或 wget，二者有其一即可）
command -v curl wget
```

- 目录准备（脚本会自动创建，也可先建好）：
  - `/opt/hancic/` — compose 文件与脚本存放
  - `/data/hancic/` — 数据目录（绑定容器 `/data`），**属主必须是容器内 hancic 用户的 uid `1000`**
- **防火墙放行 `8091`**：上海安全组 / iptables 需放行来自北京（及验收用本机）到 `172.81.241.149:8091` 的 TCP。建议只放行北京出口 IP；`8090` 已有放行记录可参照。
- 获取脚本：本机 Mac 上传（或经 usa 中转）：
  ```bash
  # 本机执行（本机可访问公网）
  scp scripts/deploy-sh.sh root@172.81.241.149:/opt/hancic/
  ssh root@172.81.241.149 'chmod +x /opt/hancic/deploy-sh.sh && /opt/hancic/deploy-sh.sh -h'
  ```

### 1.2 SSH 免密

- **上海 → usa**（方案 B 脚本由上海发起，必须）：
  ```bash
  # 在上海主机执行
  ssh-keygen -t ed25519 -N '' -f ~/.ssh/id_ed25519   # 如无密钥
  ssh-copy-id root@170.106.103.36                    # usa 可访问 GitHub
  ssh -o BatchMode=yes root@170.106.103.36 'echo ok' # 预期输出：ok
  ```
- （可选）usa → 上海：手动中转版（§2.2）由 usa 发起时才需要。
- 本机 Mac → 上海：上传脚本 / 下载迁移包用，方法同上。

### 1.3 北京 nginx

- 确认 my-nginx 容器与配置文件挂载路径（以你现有部署为准，先例是反代根路径 → 上海 8090）：
  ```bash
  docker inspect my-nginx --format '{{json .Mounts}}'   # 找到 conf 挂载目录
  docker exec my-nginx nginx -v                          # 确认 nginx 可执行
  ```

### 1.4 镜像大小与既有先例

- 镜像约 **几十 MB（<100MB）**（T25 多阶段 alpine 构建，验收标准之一），传输与 load 均快。
- 先例（CI 部署矩阵）：镜像拉取经 usa 中转——与下方方案 B 一致。

---

## 2. 镜像拉取（两方案，默认方案 B）

> 技术前提：上海、北京**无法访问 GitHub**，`docker pull ghcr.io/...` 不可行；usa 可访问。`docker daemon` 的 `registry-mirrors` 只对 **Docker Hub** 生效，**对 ghcr.io 无效**——所以方案 A 的落地方式是「改镜像名拉取 + retag」或「推送到国内镜像仓库」，不是配置 daemon 镜像加速。

### 2.1 方案 A：国内镜像源（应急/备选）

脚本做法：从 `HANCIC_PULL_A_IMAGE` 指定的镜像名拉取，再 `docker tag` 成标准名，compose 无需改动。

```bash
# A1 推荐（长期稳定）：先推送到国内镜像仓库（阿里云 ACR / 腾讯云 TCR），再在上海直拉
#   在 usa（或本机）执行一次：
docker pull ghcr.io/angryshark128/hancic:latest
docker tag ghcr.io/angryshark128/hancic:latest registry.cn-shanghai.aliyuncs.com/<namespace>/hancic:latest
docker login registry.cn-shanghai.aliyuncs.com && docker push registry.cn-shanghai.aliyuncs.com/<namespace>/hancic:latest

#   上海执行：
HANCIC_PULL_A_IMAGE=registry.cn-shanghai.aliyuncs.com/<namespace>/hancic:latest \
  /opt/hancic/deploy-sh.sh deploy --pull-mode a

# A2 临时：ghcr 前缀代理（第三方，稳定性无保证，域可能失效，仅应急）
HANCIC_PULL_A_IMAGE=ghcr.nju.edu.cn/angryshark128/hancic:latest \
  /opt/hancic/deploy-sh.sh deploy --pull-mode a

# A3 直连试一把（最不稳，仅测试网络时用；不设 HANCIC_PULL_A_IMAGE 即为直连）
/opt/hancic/deploy-sh.sh deploy --pull-mode a
```

预期输出（A1/A2）：`[hancic] 方案 A：从 ... 拉取，retag 为 ghcr.io/...` → 拉取进度 → `[fake]` 无（真实为 `docker tag` 成功）→ `[hancic] 镜像就绪`。

### 2.2 方案 B：经 usa 中转（默认，优先）

**脚本版（上海发起，仅需上海→usa 免密）**：

```bash
/opt/hancic/deploy-sh.sh deploy --pull-mode b
# 等价：PULL_MODE=b /opt/hancic/deploy-sh.sh deploy
```

脚本内部执行：

```bash
ssh root@170.106.103.36 'docker pull ghcr.io/angryshark128/hancic:latest'   # usa 拉取（可访问 GitHub）
ssh root@170.106.103.36 'docker save ghcr.io/angryshark128/hancic:latest' | docker load  # 管道直传上海
```

预期输出：

```
[hancic] 方案 B：经 root@170.106.103.36 中转拉取 ghcr.io/angryshark128/hancic:latest
[hancic] usa 拉取完成，docker save 经 ssh 管道传回并 docker load（按镜像大小需数分钟）
Loaded image: ghcr.io/angryshark128/hancic:latest
[hancic] 镜像就绪：ghcr.io/angryshark128/hancic:latest
```

**手动版（在 usa 上执行，usa → 上海需免密）**：

```bash
# usa 上执行
docker pull ghcr.io/angryshark128/hancic:latest \
  && docker save ghcr.io/angryshark128/hancic:latest | ssh root@172.81.241.149 'docker load'
```

**校验（两种方案通用）**：

```bash
docker image inspect ghcr.io/angryshark128/hancic:latest >/dev/null && echo OK
docker images --format '{{.Repository}}:{{.Tag}}  {{.Size}}' ghcr.io/angryshark128/hancic
# 预期：ghcr.io/angryshark128/hancic:latest  <100MB（验收标准 1）
```

---

## 3. 数据目录预置

数据目录 `/data/hancic` 绑定容器 `/data`（compose 卷映射 `${HANCIC_DATA_DIR}:/data`）。脚本 `deploy` 会自动：

```bash
mkdir -p /data/hancic
chown -R 1000:1000 /data/hancic        # 容器内 hancic 用户 uid=1000（alpine adduser -D 首个用户）
# config.toml 不存在时写入默认种子（data_dir="/data"、port=8090、压缩开、各上传上限同 config.example.toml）
```

- `config.toml`：**已存在则不覆盖**。自定义站点名/描述/上传上限就改这个文件后再部署。
- `themes/default`：**entrypoint 首启自动播种**（镜像内 `/app/themes/default` → `/data/themes/default`，见 `entrypoint.sh`：`theme.toml` 缺失才写入，不覆盖用户改动）。可选提前预置：
  ```bash
  docker run --rm --entrypoint sh -v /data/hancic:/data \
    ghcr.io/angryshark128/hancic:latest \
    -c 'mkdir -p /data/themes && cp -r /app/themes/default /data/themes/'
  ```
- `ip2region.xdb`：**内嵌在二进制**（`include_bytes!`），首启自动写出，无需手动拷贝。
- `uploads/`：上传时自动创建，无需预建。

> ⚠️ 首次全新部署无历史数据。若之前用仓库 `docker-compose.yaml`（named volume `hancic-data`）跑过：先 `docker compose down` 保留卷，再用 `docker run --rm -v hancic-data:/src -v /data/hancic:/dst alpine cp -a /src/. /dst/` 拷出数据（或直接绑定 `/data/hancic` 全新开始）。

---

## 4. 首启部署

```bash
# 上海主机
cd /opt/hancic
./deploy-sh.sh deploy --pull-mode b        # 默认 b；或按 §2 选 a

# 预期（截取关键行）：
#   [hancic] 已写入默认 config.toml（data_dir=/data, 端口 8090）
#   [hancic] 当前运行镜像 ... 已标记为 ghcr.io/angryshark128/hancic:prev（回滚用）  ← 首启时无此行
#   [hancic] 镜像就绪：ghcr.io/angryshark128/hancic:latest
#   [hancic] 健康检查通过：http://127.0.0.1:8091/api/health
#   [hancic] 部署成功 ✅
```

验证：

```bash
docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
# 预期：hancic  Up（healthy）  0.0.0.0:8091->8090/tcp

curl -s http://127.0.0.1:8091/api/health
# 预期：{"data":{"status":"ok"}}

docker exec hancic id -u                    # 预期：1000（bind 属主校验）
docker exec hancic ls /data/themes/default/theme.toml   # 预期：文件存在（entrypoint 已播种主题）
```

### 4.1 设置密码（首启）

浏览器打开 `http://172.81.241.149:8091/admin/setup`（或经 nginx 切完后用 `https://hancic.site/admin/setup`）：

- 设置管理员密码（**≥8 位**，见 `auth::PASSWORD_MIN_LEN = 8`）
- 提交后自动登录并跳转 `/admin`

> 仅当数据库无密码时 `/admin/setup` 可访问；设置后访问 `/admin/setup` 会重定向到 `/admin/login`（正常）。

### 4.2 生成部署 Token（agent 用）

后台 → Token 管理（`/admin/tokens`）→ 新建：

- 明文形如 **`hc_` 开头，共 46 字符**，**仅显示一次**（created 页，5 分钟内有效展示），立即保存到本机 `~/.hancic/token` 或密码管理器
- 库中只存 sha256 哈希；吊销后需重新生成
- 后续 agent 发文章、脚本调 API 均用该 Token（Bearer），接口约定见 `docs/ai-integration.md`

---

## 5. Halo 导出 zip

### 5.1 安装导出插件

1. Halo 后台（上海 `http://172.81.241.149:8090/admin`，halo 不动）→ 插件市场
2. 搜索安装 **Export MD**（`halo-sigs/plugin-export-md`，官方插件）→ 启用
3. 按插件说明执行导出 → 得到 Markdown zip（文章 + 页面 + 图片资源）

### 5.2 zip 结构预期（hancic 迁移服务读取契约）

`src/services/migrate.rs` 会遍历 zip 内**任意层级**的 `*.md`：

- 每个 `.md` 需带 front-matter（`---` 块）：`title` / `date` / `categories` / `tags` / `slug` / `type`
- 正文图片引用 `![alt](路径)`：zip 内相对路径会解包落库；`http(s)://` 外部图片在勾选「下载图片」时经 reqwest 下载（单张 30s 超时，失败记入报告不中断）
- 无 front-matter：标题取正文首个 `# `，slug 取文件名
- `type: page` 导入为页面，其余（含 `moment`）**一律导入为文章**——**Halo 瞬间（说说）不会自动成为说说**，需手动补录（§6.4）

### 5.3 下载到上海主机

- 若导出下载走 halo 后台（上海 8090），直接在上海主机下载：
  ```bash
  # 示例：halo 后台导出页给出的 zip 直链（以实际 URL 为准）
  curl -fsSL -o /root/halo-export-$(date +%F).zip '<导出直链>'
  ls -lh /root/halo-export-*.zip
  ```
- 或导出到本机后 scp 上传上海：`scp halo-export-*.zip root@172.81.241.149:/root/`

---

## 6. 数据迁移（/admin/migrate）

### 6.1 导入

1. 后台 → 迁移导入（`/admin/migrate`）
2. 上传 §5 的 zip（上限 500MB）
3. **勾选「下载图片」**（外部 http(s) 图片才会下载落库；zip 内相对路径图片无论是否勾选都会解包）
4. 执行 → 报告页

### 6.2 核对报告（验收标准 4 的核心）

报告字段对照：

| 报告字段 | 含义 | 与 Halo 对账方式 |
|---|---|---|
| `posts_created` | 新建文章/页面数 | 与 Halo 后台文章 + 页面总数核对 |
| `posts_skipped` | 跳过（空文件/非 UTF-8/front-matter 损坏） | 应尽量为 0，看 `failures` 明细 |
| `categories` | 分类数 | 与 Halo 分类数核对 |
| `tags` | 标签数 | 与 Halo 标签数核对 |
| `images_downloaded` / `images_failed` | 图片下载成功/失败 | `images_failed` 应尽量为 0 |
| `failures` | 单篇/单图失败明细 | 逐条评估：能修则修，不能修记录留档 |

### 6.3 抽查

- 打开 **3 篇长文**（后台文章列表 → 前台预览），确认图片可显示、排版正常
- 分类/标签页抽查：`/category/<slug>`、`/tag/<slug>` 均有内容
- 站内搜索抽一个关键词，能搜到迁移文章

### 6.4 说说（Halo 瞬间）

- 迁移服务**不**支持瞬间导入（front-matter `type` 只有 `page` 会映射为页面，`moment` 会变成普通文章）——**手动补录**：
- 后台 → 说说（`/admin/moments`）→ 逐条或批量发布；带图说说先经上传（粘贴/拖拽/API）再引用

### 6.5 全量备份留档（上线前必须）

后台 → 备份（`/admin/backup`）→ 全量导出：

- zip 内含：`config.toml` + DB 快照（`VACUUM INTO` 一致性快照）+ `uploads/` + `themes/` + `meta.json`
- 下载到上海主机与本机各存一份，记录文件名与字节大小
- 该备份可用于：回滚后重导 / 换机迁移 / 灾难恢复

---

## 7. 北京 nginx 切换

### 7.1 修改反代

北京主机（49.232.168.161），编辑 my-nginx 挂载的 conf（先例为反代根路径，路径以 `docker inspect my-nginx` 挂载信息为准，形如 `/etc/nginx/conf.d/default.conf`）：

```nginx
# hancic.site server 块内，根路径 location：
location / {
    proxy_pass http://172.81.241.149:8091;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}
```

> `X-Real-IP` **必传**：hancic 统计服务优先读 `x-real-ip`（其次 `x-forwarded-for` 首段）来记访问地区（`src/web/front.rs`）；缺失则所有访问都被记为「本地」，验收标准 6 无法通过。

### 7.2 校验并重载

```bash
docker exec my-nginx nginx -t                # 预期：syntax is ok / test is successful
docker exec my-nginx nginx -s reload         # 或 docker restart my-nginx
```

### 7.3 切换前后验证

```bash
# 北京主机（或任意可访问公网处）
curl -sI https://hancic.site/                # 预期：HTTP/1.1 200
curl -s http://172.81.241.149:8091/api/health   # 北京 → 上海 8091 直连（确认防火墙放行）
# 带 Host 头验证反代正确：
curl -s -H 'Host: hancic.site' http://172.81.241.149:8091/ | head -5   # 预期：页面 HTML
```

---

## 8. 验收清单（设计 §17 七条，逐条映射）

| # | 验收标准 | 操作命令 / 步骤 | 预期结果 | 对应手册 |
|---|---|---|---|---|
| 1 | 空闲内存 ≤100MB / 镜像 ≤100MB | `docker stats --no-stream hancic`；`docker images --format '{{.Size}}' ghcr.io/angryshark128/hancic` | 空闲内存 <50MB（达标线 ≤100MB）；镜像 <100MB | §2、§4 |
| 2 | 375px 移动端：带图说说、带图文章 | 浏览器 DevTools 设备模拟（iPhone SE 375×667）或真机：后台 `/admin/moments` 发带图说说；`/admin/posts/new` 写带图文章并发布（T24 已自动化覆盖，此处真机抽查） | 发图/上传/发布全流程可用，布局无横向溢出 | §4 |
| 3 | 桌面粘贴截图自动上传 | 编辑器（Vditor）内粘贴截图 | 自动上传成功，附件库 `/admin/attachments` 出现新附件 | §4 |
| 4 | Halo zip 导入数量一致、图片可用 | §6 报告字段与 Halo 对账 + 抽查 3 篇长文 | `posts_created`=Halo 文章+页面数；分类/标签数一致；`images_failed`≈0；图片可显示 | §6 |
| 5 | agent 凭 Token 发文章 | 见下方 §8.1 curl 实测 | `201` + `data.id`，前台可读 | §4.2 |
| 6 | 地区统计正确、图片体积显著变小 | 后台 `/admin/stats/regions` 核对北京/上海等访问来源；`docker exec hancic du -h /data/uploads/image/` 对比原图 | 地区分布正确（依赖 §7.1 的 `X-Real-IP`）；产物显著小于原图（image_compress=true，max_edge=2000，quality=85） | §7、§6 |
| 7 | hancic.site 文章/说说/导航可用 | 逐条抽查 URL（§8.2）+ 站内搜索 | 全部 `200`，内容与功能等价既有博客 | §7 |

### 8.1 验收 5：agent 用 Token 发文章（curl 实测）

```bash
TOKEN="hc_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"   # 来自 §4.2，保存的 46 字符明文
curl -s -X POST https://hancic.site/api/posts \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"title":"验收：agent 凭 Token 发布","content_md":"# 验收\n\n由 agent 经 REST API 创建并发布。","status":"published"}'
# 预期：{"data":{"id":N,"slug":"...","status":"published","published_at":"...",...}}（201）
```

- 响应含 `data.id` 即成功；`401` 检查 Token 是否完整/未吊销（接口约定已记入 `docs/ai-integration.md`）

### 8.2 验收 7：URL 抽查清单

```bash
for u in / /moments /about /search "/search?q=关键词" /post/<一篇长文slug> /category/<slug> /tag/<slug>; do
  printf '%-30s ' "$u"; curl -s -o /dev/null -w '%{http_code}\n' "https://hancic.site$u"
done
# 预期：全部 200
```

---

## 9. 回滚

对外流量与本地容器**两步独立**，可分别回滚：

### 9.1 立即恢复对外（30 秒级）

北京 nginx：`proxy_pass` 改回 `http://172.81.241.149:8090`（halo），`nginx -t && nginx -s reload`。

### 9.2 容器回退到上一版镜像

```bash
# 上海主机
/opt/hancic/deploy-sh.sh rollback        # --rollback 为同义别名
# 预期：使用上一版镜像 ghcr.io/angryshark128/hancic:prev 重启服务 → 健康检查通过 → 回退成功
```

- `:prev` 标记在每次 `deploy` 成功拉取前自动打在当前运行镜像上；首次部署无 `:prev`，rollback 会报错提示
- 容器回退不影响数据（`/data/hancic` 卷不动）

### 9.3 数据层面回滚

迁移后发现问题需回到迁移前状态：用 §6.5 的备份 zip 在后台 `/admin/backup/restore` 恢复（当前数据目录会改名保留，解包替换）。

---

## 10. 观察期与停 halo

- 切流后**观察 1–2 周**：关注访问量、`/admin/stats`、容器日志
  ```bash
  docker logs --tail=50 -f hancic
  docker stats --no-stream hancic                 # 空闲内存是否稳定 <50MB
  ```
- 确认无问题后停 halo（**只停不删**，保留容器可再启动）：
  ```bash
  docker ps --format '{{.Names}}' | grep halo      # 确认 halo 容器名
  docker stop <halo容器名>                          # 或 cd /root/Hancic/halo-blog/halo-migration && docker compose stop
  ```
- 停 halo 后再观察几天；彻底稳定后可考虑：把 halo 数据目录整体备份归档、释放 8090 端口给 hancic（非必须，可保持 8091）。

---

## 11. 常见问题

| 现象 | 排查 / 解决 |
|---|---|
| 方案 B `ssh` 失败（Permission denied / timeout） | 上海→usa 免密未配（§1.2）；`BatchMode=yes` 下不会等密码，直接失败，先 `ssh-copy-id` |
| 方案 A 拉取 404 / 超时 | 代理域失效或 ACR 命名空间不对；换 A1（推送国内仓库）或回方案 B |
| 部署后 health 失败 | `docker compose -f /opt/hancic/docker-compose.yaml logs --tail=100 hancic`；常见：8091 被占用、`/data/hancic` 属主非 1000（容器内写不进去）、config.toml 端口与 `HANCIC_CONTAINER_PORT` 不一致 |
| 迁移图片 404 / 显示不出 | 报告 `images_failed` 明细；未勾「下载图片」导致外部图仍为原 URL；zip 内相对路径需与正文引用一致 |
| 统计地区全是「本地」 | 北京 nginx 未传 `X-Real-IP`（§7.1），或直接访问了 8091 未走反代 |
| `hancic.site` 打不开但 8091 通 | 北京 nginx conf 未 reload；或上海防火墙未放行 8091 |
| 之前用 named volume 跑过，数据混乱 | §3 提示的卷拷贝/全新开始，二选一，别混用 |
| rollback 报「未找到 :prev」 | 从未成功 deploy 过（首次部署）；或镜像 tag 被手动清理 |

---

## 附录：实际部署记录（2026-08-09，首日上线）

### 实际执行路径（与上文方案的差异）

1. **镜像获取**：ghcr private 包需 `read:packages` PAT，本机 gh OAuth token 无此权限 → 未用 usa 中转，改为**源码直传上海本地 `docker build`**（`--build-arg CARGO_SOURCE_INDEX="sparse+https://rsproxy.cn/index/"`，4C8G 约 25 分钟，镜像 69.6MB）。
2. **端口**：原计划 hancic 占 8091 保留 halo——但**上海腾讯云安全组（控制台层）只放行了 8090**，8091 从公网不可达（主机内 firewalld/iptables 均无拦截）。改为 **hancic 直接占 8090，halo `docker stop` 保留可回滚**（回滚：`docker stop hancic && docker start halo`，nginx 无需改——都走 8090）。
3. **数据迁移**：halo-plugin-export-md 与 halo 2.24 不兼容（未加载）；H2 数据库有密码 → 改走**前台爬虫**（`~/Project/hancic-migrate-data/crawl.py`）：33 篇文章（front-matter md + 图片下载）→ `/admin/migrate` 导入（0 图片失败）→ 11 条说说经 `POST /api/moments`（import-moments.py）导入。
4. **北京 nginx 配置**：宿主 `sed -i` 修改不生效——**bind mount 单文件 inode 变化后容器内仍是旧文件**（需 `docker compose up -d --force-recreate` 重建容器重新挂载；容器内直接改报 Resource busy/Read-only）。

### 回滚（若需切回 halo）

```bash
# 上海
docker stop hancic && docker start halo
# 北京：无需改 nginx（都走 8090），但 my-nginx 容器内若配置被改过需重建
```

### 待办
- `/about` 关于页：迁移不含独立页面，需在后台新建（文章 → 类型=页面 → slug=about）
- halo 数据保留在 `/root/Hancic/halo-blog/halo-migration/halo_data`（观察稳定后可归档）

## 附录：subpath 部署（2026-08-09 新增）

站点可部署在子路径（如 `https://example.com/blog/`），应用层 `base_path` 配置即可：

1. `config.toml` 设置 `base_path = "/blog"`（页面链接/静态资源/后台重定向自动带前缀）。
2. nginx 反代剥前缀转发（URL 与内部路径都去掉 `/blog`）：

```nginx
location /blog/ {
    proxy_pass http://127.0.0.1:8090/;          # 尾斜杠：剥掉 /blog 前缀
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

3. 后台入口变为 `https://example.com/blog/admin/`，登录/跳转均自动带前缀。
4. 不配置 `base_path`（空）时行为与之前完全一致（根路径部署）。
