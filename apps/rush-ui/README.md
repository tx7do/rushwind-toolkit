# rush-ui — RushWind 工具箱桌面壳

Tauri 2 桌面应用，**同进程直调 rush-gen 库**：没有 sidecar、没有 HTTP
API，命令层就是 `库函数 + 错误串化`（见 `src/main.rs`），序列化形状与
rush-gen 的 Options/Report 结构体一一对应。

## 构建前提（Linux / Ubuntu 24.04）

Tauri 的 webkit2gtk 需要系统开发包（一次即可）：

```shell
sudo apt install libwebkit2gtk-4.1-dev pkg-config build-essential \
  libssl-dev librsvg2-dev libayatana-appindicator3-dev
```

## 跑起来

本 crate 已被 workspace `exclude`（不拖累主仓 build/test），独立构建：

```shell
cd apps/rush-ui
cargo run          # MVP 直接跑；静态前端在 web/，无需 node 构建步骤
```

日常开发想要热重载 / 打包再装 CLI：

```shell
cargo install tauri-cli --version '^2'
cargo tauri dev    # 热重载
cargo tauri build  # 打包（需先在 tauri.conf.json 配 bundle.icon）
```

## MVP 命令面

| 页签 | 直调 | 说明 |
| --- | --- | --- |
| gen entity | `rush_gen::entity::generate_entity` | 字段编辑器（name:kind 逐行）、code-field、global、dry-run |
| gen pages | `rush_gen::pages::generate_pages` | 字段留空 = 从 `.rush/<name>.json` 规格继承（推荐） |
| new | `rush_gen::project::new_project` | 内存 / Postgres 变体 |
| adopt | `rush_gen::adopt::adopt` | 接管快照仓 |
| manifest | `rush_gen::manifest::check / rebuild` | proto / react 两面 |

testbed run 涉及长进程与 docker 差分栈，刻意不进 MVP。

## 状态与约定

- 前端参数键 = Rust 结构体字段名（snake_case，serde 默认形态），不设别名层。
- 报告面板直接渲染 Report 的 JSON；`created/edited/skipped/files` 计数进状态行。
- Rust 命令层的错误统一 `format!("{e:#}")`（完整 cause 链）传给前端。
