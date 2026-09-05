# hancic 管理后台设计令牌（映射自 Trilium「前端约束规范」02/03）

> 本文件是后台样式重构（refactor/admin-ui）的令牌唯一依据。规范源：
> Trilium 前端约束规范 book —— 01 总则 / 02 设计令牌 / 03 主题系统 / 10 禁止清单。
> 技术适配裁定：规范技术栈为 React+Tailwind；hancic 后台为 SSR + 原生 CSS 变量，
> 故「令牌」以 CSS 变量承载（`admin.css` 顶部定义区），不引入构建链。

## 变量分层

| 层 | 变量 | 说明 |
|---|---|---|
| 品牌色 brand | `--brand-50/100/500/600/700/900` | 唯一主色来源，随 `html[data-accent]` 切换（默认 indigo） |
| 中性面 surface | `--surface-0..4` | 背景/边框；随 `data-mode` 取明暗两套值 |
| 文字 ink | `--ink-900/700/500/400` | 文字层级；随 `data-mode` 取值 |
| 语义色 | `--rose-*` `--amber-*`（危险/警告）+ emerald 复用 brand | 状态/危险操作专用 |
| 间距 | `--space-1/2/3/4/6/8/12/16` | 4px 网格（4/8/12/16/24/32/48/64） |
| 圆角 | `--radius-sm/md/lg/xl` | 4/6/8/12px |
| 字号 | `--text-xs/sm/base/lg/xl` | 12/14/16/18/20px |
| 阴影 | `--shadow-sm/md/lg` | 3 档；禁彩色阴影（`--shadow-accent` 为 legacy，将删） |

## 主题驱动链

```
html[data-accent=indigo|emerald|rose|amber|slate] 覆盖 --brand-*（6 级）
  → --accent = var(--brand-500)   （[data-mode=light] 下 = var(--brand-600)，深浅自适应）
  → --accent-2/--accent-soft/--accent-glow 由 color-mix() 派生
  → --side-active-bg/--side-indicator/--row-hover-bg/--stat-tint 全部经 accent 派生
```

任何"主色/选中/焦点"表现只允许引用 `--accent` 或 `--brand-*`，禁止直写色值。

## 旧变量别名（兼容层，迁移完成后删除；勿新增引用）

| 旧 | 现映射 | 旧 | 现映射 |
|---|---|---|---|
| `--bg` | `var(--surface-2)` | `--text` | `var(--ink-900)` |
| `--panel` | `var(--surface-0)` | `--muted` | `var(--ink-500)` |
| `--panel-2` | `var(--surface-1)` | `--border` | `var(--surface-3)` |
| `--accent` | brand-500/600 | `--danger` | `var(--rose-*)` |
| `--shadow` | `var(--shadow-md)` | `--warn` | `var(--amber-*)` |
| `--shadow-hover` | `var(--shadow-lg)` | `--stat-tint` | `var(--accent-soft)` |

## 主题名（localStorage: `admin-accent`）

规范 5 主题：`indigo`(默认) / `emerald` / `rose` / `amber` / `slate`。
旧值自动迁移：`pink→rose` `blue→indigo` `green→emerald` `purple→indigo` `orange→amber`。
明暗键：`admin-mode`（`dark` 默认 / `light`），与规范 `colorScheme` 语义一致但沿用旧键。

## 使用规则（禁止清单节选，防回潮）

- 新增样式一律引用令牌；禁止裸 hex、裸 px 间距/字号、`!important`、emoji 图标
- 按钮只用 4 变体语义（primary/secondary/ghost/danger），主按钮**禁渐变**（见 06-7.1）
- 语义色只用于状态标签/危险操作，不用于导航/链接主色
- 明暗由 `data-mode` 变量驱动，暗色适配不要用 `.dark` 独立样式文件
