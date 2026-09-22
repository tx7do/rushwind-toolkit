# rushwind-toolkit

RushWind 生态工具箱：[`rush` CLI](#命令) + [`rush-gen` 核心库](#仓库结构)。

为 [RushWind](https://github.com/tx7do/rushwind)（Rust 微服务框架）与
[rushwind-admin](https://github.com/tx7do/rushwind-admin)（契约驱动的管理后台脚手架）
覆盖开发链里真实存在的缺口。

定位：[go-wind-toolkit](https://github.com/tx7do/go-wind-toolkit) 服务 Go/Kratos 侧，
本仓服务 Rust 侧——不做全家桶（proto 生成走 `build.rs` 的 rushwind-gen-http、前端
页面生成复用 gowind frontendgen、watch/交叉编译交给 cargo 生态），只做 Rust 侧
没有对应物的那部分。

## 安装

```shell
cargo install --git https://github.com/tx7do/rushwind-toolkit rush-cli
```

## 命令

| 命令 | 状态 | 说明 |
|---|---|---|
| `rush adopt` | ✅ | 把 rushwind-admin 快照从“上游镜像”接管为下游自有仓 |
| `rush manifest` | ✅ | 校验 / 重建 proto 与 react 两个同步面的 sha256 清单 |
| `rush gen entity` | 规划中 | 领域实体后端全链生成 |
| `rush new` | 规划中 | 从模板创建新项目 |
| `rush testbed` | 规划中 | admin-diff 差分回归台架的 sweep/fixture 包装 |

### `rush adopt` —— 下游接管（二次开发第一步）

背景：rushwind-admin 的 proto 契约与 react 前端是从 go-wind-admin 同步来的镜像，
由 sha256 清单 + CI 门禁保护（语义是“防手改”）。这套机制对维护者是对的，对下游
二次开发是反的——新增自己的 proto、修改自己的前端页面必然红 CI，而重建清单的
`sync` 模式又要求持有上游仓（Windows 侧 `D:\GoProject\go-wind-admin`）。

`rush adopt` 把仓库从镜像模式切到自有模式：

1. 以**当前树**为基线重建 `backend/api/MANIFEST.sha256` 与
   `frontend/admin/react.MANIFEST.sha256`；
2. 从 `.github/workflows/ci.yml` 剥离两个同步门禁步（连同紧邻的注释块）；
3. 上游漂移基线 `react.UPSTREAM.sha256` 默认保留并提示，`--prune-upstream-baseline`
   删除。

幂等可重入；`--dry-run` 全程预览不落盘；`--keep-gates` 只重建清单不动 CI
（仍想跟踪上游的维护者模式）。

```shell
rush adopt --dry-run   # 预览
rush adopt             # 接管
```

### `rush manifest` —— 清单维护

接管后新增/修改了 proto 或前端文件，重建基线让 CI 保持绿灯：

```shell
rush manifest proto --rebuild   # backend/api/protos 面
rush manifest react --rebuild   # frontend/admin/react 面
rush manifest proto             # 默认只校验（只读）
```

清单算法与被替换的 shell 脚本逐字对齐：proto 面哈希前剥 UTF-8 BOM；react 面
排除 `node_modules`/`dist` 目录与 git 忽略路径（`git check-ignore` 同款语义）、
字节级哈希，要求在 git 工作树内执行。校验按内容映射比较，对行序不敏感，
对未接管的仓库做只读校验同样准确。

## 仓库结构

```
rushwind-toolkit/
├── crates/
│   ├── rush-cli/     # `rush` 可执行入口（clap 命令面）
│   └── rush-gen/     # 核心库：清单双算法 + adopt；规划中的生成器也住这里
└── ...
```

核心库与 CLI 分离：将来的桌面壳（若做，倾向 Tauri）直接复用 `rush-gen`，
不经过 CLI。

## 路线图

- [x] `rush adopt` / `rush manifest`
- [ ] `rush gen entity`：领域实体后端全链生成——proto 模板（带正确的 http
      注解）+ SeaORM 实体 + migration 段 + repo + service 骨架（对齐
      rushwind-gen-http 生成的 trait 签名）+ mount 表注册 + testbed 占位；
      实体定义可借力 sea-orm-cli
- [ ] `rush new`：从模板起项目（rushwind examples + rushwind-bootstrap
      YAML 组装的模板化）
- [ ] `rush testbed`：差分回归台架的 fixture 重建 / 全量路由 sweep 包装
- [ ] 前端页面生成：与 go-wind-toolkit 的 frontendgen（Vben / Element /
      React 三栈）做文档级集成，而非在 Rust 侧重写

## 相关仓库

- [rushwind](https://github.com/tx7do/rushwind) — Rust 微服务框架本体
- [rushwind-admin](https://github.com/tx7do/rushwind-admin) — 契约驱动的管理后台脚手架
- [go-wind-toolkit](https://github.com/tx7do/go-wind-toolkit) — Go/Kratos 侧工具箱
