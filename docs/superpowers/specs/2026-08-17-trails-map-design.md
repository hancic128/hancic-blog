# 徒步轨迹展示页（Trails Map）设计

- 日期：2026-08-17
- 分支：`feat/trails`（独立开发，暂不合并 main）
- 状态：设计已确认

## 背景

用户是户外徒步爱好者，两步路平台上积累了大量徒步轨迹。希望在 hancic-blog 上新增一个轨迹展示页，收藏并展示这些徒步记录，包含地图与完整数据。**不确定最终是否上线**，因此放在独立分支 `feat/trails` 开发，main 保持稳定。

## 需求（已与用户确认）

1. **数据来源**：两步路导出的 GPX 文件（用户手动导出）。
2. **展示形式**：总览页（地图 + 轨迹列表）+ 单条详情页（完整轨迹地图 + 数据）。
3. **详情数据**：里程、累计爬升/下降、运动时长、均速、海拔范围（最高/最低）、起终点、日期、描述。
4. **管理方式**：后台「徒步轨迹管理」页——上传 GPX 自动解析入库、编辑名称/描述、删除。
5. **地图**：Leaflet 地图库，内置多个瓦片源可切换（地图源 + 样式）。
6. **分支**：独立 `feat/trails` 分支，不合并 main。

## 架构（方案 1：SQLite 元数据 + GPX 存盘 + 预解析抽稀）

- **数据库**：新增 `trails` 表存元数据与统计，含抽稀后的简化坐标。
- **文件**：GPX 原文件存 `data/trails/`（运行时目录，与 `data/uploads/` 同级，不进镜像）。
- **前台**：只读页面 `/trails`（总览）与 `/trails/{id}`（详情）。
- **后台**：`/admin/trails` 管理页（上传/列表/编辑/删除）。
- **解析**：Rust 侧用 `quick-xml` 轻量解析 GPX（不引入重依赖）。

## 数据模型

```sql
CREATE TABLE IF NOT EXISTS trails (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  name              TEXT NOT NULL,              -- 轨迹名称（GPX <name> 或上传时填写）
  description       TEXT NOT NULL DEFAULT '',   -- 描述（后台可编辑）
  file_path         TEXT NOT NULL,              -- GPX 相对路径（data/trails/xxx.gpx）
  started_at        TEXT,                       -- 开始时间（ISO，轨迹首点时间）
  distance_m        REAL,                       -- 里程（米）
  elevation_gain_m  REAL,                       -- 累计爬升（米）
  elevation_loss_m  REAL,                       -- 累计下降（米）
  moving_seconds    INTEGER,                    -- 运动时长（秒）
  avg_speed_kmh     REAL,                       -- 平均速度（km/h）
  max_elevation_m   REAL,                       -- 最高海拔
  min_elevation_m   REAL,                       -- 最低海拔
  start_lat         REAL, start_lon REAL,       -- 起点坐标
  end_lat           REAL,   end_lon   REAL,     -- 终点坐标
  simplified        TEXT NOT NULL DEFAULT '[]', -- 抽稀后坐标 JSON [[lat,lon],...]（总览地图用）
  point_count       INTEGER NOT NULL DEFAULT 0, -- 原始轨迹点数
  created_at        TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);
```

幂等创建（仿现有 `ensure_column_id` 模式，`db.rs` 的 init 链追加）。

## GPX 解析与抽稀

**解析**（`src/services/trails.rs`）：
- 用 `quick-xml` 解析 GPX：读取 `<trkseg>` 下的 `<trkpt>`（lat/lon/ele/time）。
- 容错：无 `<trkpt>` 或点过少（<2）视为无效轨迹，拒绝入库。

**统计计算**：
- 距离：相邻点 haversine 公式累加。
- 累计爬升/下降：相邻点海拔正差/负差累加（海拔缺失时跳过该段）。
- 运动时长：首尾时间戳差（若个别点缺时间，按可用时间估算）。
- 均速：`distance / moving_time`。
- 海拔范围：遍历取 max/min。
- 起终点：首点/末点坐标。
- 开始时间：首点时间。

**抽稀**（Douglas-Peucker）：
- 按距离阈值（约 30m）把几千点压到几百点，存入 `simplified`（JSON 数组）。
- 阈值常量便于调整；点数少（<阈值内）直接全保留。

## 后台管理（`/admin/trails`）

- 路由注册在 `src/admin/mod.rs`（仿 columns 模块）：`/admin/trails`、`/admin/trails/{id}/update`、`/admin/trails/{id}/delete`、`/admin/trails/upload`。
- 页面：上传 GPX（file input + 名称/描述可选填）、轨迹列表（名称/日期/里程/点数 + 编辑/删除）、编辑弹窗（改名称/描述）。
- 上传校验：文件扩展名 `.gpx`、解析成功、有有效轨迹点；失败回显错误提示。
- 删除：删除数据库记录 + GPX 文件（`data/trails/`）。

## 前台页面

### 总览页 `/trails`
- 地图（Leaflet）显示所有轨迹的 `simplified` 简化线（不同轨迹可用不同颜色区分）。
- 轨迹列表：卡片（名称/日期/里程/爬升），点击进详情。
- 布局：地图在上、列表在下（移动端友好）；桌面保持同构。

### 详情页 `/trails/{id}`
- 完整轨迹地图（加载原始 GPX 解析或存储的完整坐标——为性能，导入时同时存完整坐标 JSON 至 `data/trails/{id}.json`，详情页直接加载，避免每次解析 GPX）。
- 数据卡片：里程、累计爬升/下降、运动时长、均速、最高/最低海拔、起终点、日期。
- 描述文字。
- 不存在 → 404。

### 地图与瓦片切换
- Leaflet 1.9.x 本地化到 `assets/vendor/leaflet/`（与 Vditor 本地化一致，脚本零外网依赖；瓦片本身需外网）。
- 瓦片图层定义为前端常量，地图右上角 `L.control.layers` 切换：
  - 高德街道（国内快，默认）：`https://webrd01.is.autonavi.com/appmaptile?style=7&x={x}&y={y}&z={z}`
  - 高德卫星：`https://webst01.is.autonavi.com/appmaptile?style=6&x={x}&y={y}&z={z}`
  - 腾讯街道：`https://rt0.map.gtimg.com/tile?z={z}&x={x}&y={y}&styleid=3`
  - Carto（国外备选）：`https://basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}.png`
  - OSM（国外备选）：`https://tile.openstreetmap.org/{z}/{x}/{y}.png`
- 已实测：高德/腾讯瓦片在国内（本机与上海服务器）响应 <0.2s；OSM 被墙不可用（保留作为国外访问备选）。

### 导航入口
- 导航菜单加「徒步」项（type=link 指向 `/trails`，通过后台设置页导航配置添加，不动代码默认值；若需默认项可在 seed 追加）。

### 主题适配
- default / modern 双主题各新增 `trails.html`、`trail.html` 模板（复用 header/footer/layout 结构）。
- 样式追加到两主题 `style.css`（地图容器高度、数据卡片、列表卡片）。

## 错误处理

- 上传：非法 GPX / 无轨迹点 → 拒绝并提示，不入库。
- 详情页轨迹不存在 → 404。
- 地图瓦片源不可用 → Leaflet 多图层，切换其他源即可，不阻塞页面；地图容器有最小高度与空态提示。
- 后台/前台解析失败日志打点（`tracing::error!`）。

## 测试

- Rust 单元测试：GPX 解析（距离/爬升/时长计算正确性）、抽稀（点数压缩 + 端点保留）、`trails` CRUD。
- 集成测试：后台上传接口（合法/非法 GPX）、`/trails` 与 `/trails/{id}` 渲染（含 404）。
- E2E（Playwright）：后台上传 GPX → 前台总览显示轨迹列表与地图容器、详情页显示数据卡片、图层切换控件存在。

## 开发顺序

1. 后端：`db.rs` 加 `trails` 表 → `src/services/trails.rs`（GPX 解析 + 统计 + 抽稀 + CRUD）→ `src/admin/trails.rs` + 路由注册
2. 后台管理页模板 + JS（上传/列表/编辑/删除）
3. 前台：`src/web/front.rs` 加 `/trails`、`/trails/{id}` 路由 → 模板（双主题）+ Leaflet 本地化 + 瓦片图层切换
4. 导航入口（后台设置加「徒步」项）
5. 测试全绿（cargo test / clippy -D warnings / Playwright E2E）
6. 分支保留不合并；待用户决定是否上线再合入 main 并部署

## 分支策略

- 当前分支 `feat/trails`（从 main `865a8d6` 切出）。
- 全部开发与提交在此分支完成；不 push 到远程、不合并 main，除非用户最终确认上线。
