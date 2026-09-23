# rush-ui — RushWind 工具箱桌面壳

Tauri 2 桌面应用，与 gowind-uiapp（Wails）**同前端栈**：Vue 3 +
Ant Design Vue + Vite，同白底 + 左侧 Tabs + `#00b96b` 主题的页面骨架。
同进程直调 rush-gen 库（无 sidecar、无 HTTP API），命令层见 `src/main.rs`。

## 构建前提（Linux / Ubuntu 24.04）

```shell
sudo apt install libwebkit2gtk-4.1-dev pkg-config build-essential \
  libssl-dev librsvg2-dev libayatana-appindicator3-dev
```

Node 18+（vite 5 支持 18；ESLint 10 类工具链建议 Node 20+）。

## 跑起来

```shell
cd apps/rush-ui/frontend
npm install --registry=https://registry.npmmirror.com   # 首次
npm run build                                           # 产物出 frontend/dist

cd ..            # apps/rush-ui
cargo run        # tauri.conf.json 的 frontendDist 指向 frontend/dist
```

本 crate 已被 workspace `exclude`，主仓 build/test 不受影响。
开发热重载可装 tauri-cli：`cargo install tauri-cli --version '^2'` 后
`cargo tauri dev`（打包需在 tauri.conf.json 配置 bundle.icon 全尺寸集）。

## 命令面（直调 rush-gen 库）

| 页面 | 直调 | 说明 |
| --- | --- | --- |
| 实体链 | `rush_gen::entity::generate_entity` | 结构化字段编辑器（枚举取值集）、--regen/--auth-free/--global/--check |
| 页面生成 | `rush_gen::pages::generate_pages` | 三栈选择；字段缺省从 .rush/&lt;name&gt;.json 继承 |
| 回滚 | `rush_gen::undo::undo_entity` | 按规格反向移除全部生成物（dry-run 优先） |
| 脚手架 | `rush_gen::project::new_project` | memory / sqlite / postgres |
| 接管 | `rush_gen::adopt::adopt` | 快照仓接管 |
| 清单 | `rush_gen::manifest::check / rebuild` | proto / react 两面 |
| 环境体检 | `rush_gen::doctor::run_doctor` | 工具链 + 可选仓形状，页签顶部按钮即跑 |

## 约定

- 字段经 DTO 传递（kind 用规格串，main.rs 转换层落 FieldKind）；
  新增 Options 字段一律 `#[serde(default)]` 保持前端兼容。
- 仓库路径在 Header 输入（probe_repo 实时出芯片），多页签共享并持久化。
- 错误统一 `format!("{e:#}")` 传前端；报告渲染见 ReportView 组件。
