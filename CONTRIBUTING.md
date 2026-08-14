# 贡献指南

感谢你愿意为 hancic 贡献！无论是一行文档、一个 bug 修复还是一个全新特性，都欢迎。

## 目录

- [开发环境](#开发环境)
- [代码规范](#代码规范)
- [工作流](#工作流)
- [测试](#测试)
- [提交信息规范](#提交信息规范)
- [文档](#文档)

## 开发环境

- **Rust**：版本由 `rust-toolchain.toml` 固定（当前 1.88.0），`rustup` 会自动按目录文件安装
- **Node.js**（仅 e2e 测试需要）：版本 22+
- 无需数据库服务：hancic 使用嵌入式 SQLite，首次启动自动建库并迁移

```bash
git clone https://github.com/Angryshark128/hancic-blog.git
cd hancic-blog
cp config.example.toml config.toml
cargo run -- config.toml
```

打开 <http://localhost:8090> 预览前台，`/admin/setup` 初始化后台密码。

## 代码规范

- 运行 `cargo clippy --all-targets -- -D warnings`，必须零告警（CI 以此为门槛）
- 遵循 `rustfmt` 风格；CI 不强制 `cargo fmt --check`，但提交前建议本地格式化
- 新增依赖需谨慎：优先复用现有依赖（见 `Cargo.toml`），避免为了小功能引入重型库
- 错误处理统一走 `crate::error::AppError`，链式 `?`，不 panic
- 数据库访问全部参数化（`sqlx` 绑定变量），禁止字符串拼接 SQL

## 工作流

1. Fork 仓库并创建特性分支：`git checkout -b feat/your-feature`
2. 小步提交，遵循下方提交信息规范
3. 运行测试与 clippy，确保全绿
4. 提交 Pull Request，描述改动动机与验证方式

### 分支命名建议

- 新特性：`feat/<简述>`
- 缺陷修复：`fix/<简述>`
- 文档：`docs/<简述>`

## 测试

```bash
# 全部测试（单元 + 集成）
cargo test

# 静态检查
cargo clippy --all-targets -- -D warnings

# 端到端（Playwright）
cd e2e && npm ci && npx playwright test
```

- 新增/修改业务逻辑时，尽量补充集成测试（`tests/` 下已有大量覆盖：`front_pages.rs`、`services_posts.rs`、`admin_taxonomy.rs` 等，可参考其写法）
- 涉及数据库 schema 变更时，迁移必须**幂等**（重复执行无副作用），并附带测试
- 前端模板改动若涉及 `data/themes/`（运行时主题），记得同步到 `themes/`（镜像内嵌源）——两者不一致会导致 Docker 部署后前台回退到旧模板

## 提交信息规范

采用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/) 风格：

```
<type>(<scope>): <描述>
```

常见类型：

| 类型 | 用途 |
|---|---|
| `feat` | 新特性 |
| `fix` | 缺陷修复 |
| `docs` | 文档 |
| `refactor` | 重构（不改变行为） |
| `test` | 测试 |
| `chore` | 构建/工具/依赖等杂项 |

示例：

```
feat: 文章列表支持按阅读数排序
fix(admin): 修复专栏卡片间距过近的问题
docs: 完善 README 快速开始章节
```

## 文档

- 用户可见行为的变化请同步更新 `README.md` 或对应 `docs/*.md`
- 主题相关改动请更新 `docs/theme-guide.md`
- 部署相关改动请更新 `docs/deploy-sh.md`

## 行为准则

请阅读并遵守 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。
