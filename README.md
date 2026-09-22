<div align="center">

# rushwind-toolkit

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.81+-DEA584?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/tx7do/rushwind-toolkit/actions/workflows/ci.yml/badge.svg)](https://github.com/tx7do/rushwind-toolkit/actions/workflows/ci.yml)

**中文** | [English](./README.en-US.md) | [日本語](./README.ja-JP.md)

</div>

---

RushWind 生态工具箱：[`rush` CLI](#命令) + [`rush-gen` 核心库](#仓库结构)。

为 [RushWind](https://github.com/tx7do/rushwind)（Rust 微服务框架）与
[rushwind-admin](https://github.com/tx7do/rushwind-admin)（契约驱动的管理后台脚手架）
覆盖开发链里真实存在的缺口。

定位：只做 RushWind 侧没有对应物的那部分，不做全家桶——契约与路由生成已由
`build.rs` 里的 rushwind-gen-http 覆盖（proto 进来，路由表、错误映射、服务
trait 出去，零手写），watch 与交叉编译交给 cargo 生态，前端页面生成不在本仓
范围。

## 安装

```shell
cargo install --git https://github.com/tx7do/rushwind-toolkit rush-cli
```

## 命令

| 命令 | 状态 | 说明 |
|---|---|---|
| `rush adopt` | ✅ | 把 rushwind-admin 快照从“上游镜像”接管为下游自有仓 |
| `rush manifest` | ✅ | 校验 / 重建 proto 与 react 两个同步面的 sha256 清单 |
| `rush gen entity` | ✅ | 领域实体后端全链生成 |
| `rush new` | ✅ | 从模板创建可编译、可直接 cargo run 的新项目（memory / postgres 双变体） |
| `rush gen pages` | ✅ | React 前端 CRUD 页面组生成（自包含类型，不依赖上游 TS 客户端） |
| `rush testbed` | ✅ | 差分台架编排与报告摘要（绝不代启 docker） |

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
   删除；
4. 两个 sync 脚本**机制性退役**：改写为拒跑 stub（sync 会整树覆盖、--check
   会把手改当篡改，对下游都是反向语义），`--keep-sync-scripts` 保留原脚本。

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

### `rush gen entity` —— 领域实体后端全链生成

为一个标准 CRUD 实体生成 rushwind-admin 的整条后端链（模板基准：仓内最小的
dict_type 实体链），一次命令拿到可编译的骨架：

```shell
# 标准 CRUD 实体
rush gen entity widget \
  --field code:string --field quantity:u32 --field is_active:bool

# 带唯一编码（Get 获得 code 臂与 /widgets/code/{code} 附加路由）
rush gen entity widget --field code:string --code-field code

# 平台全局表（全链去租户）
rush gen entity setting --global --field key:string

# 枚举字段（proto enum → 文本列，仓内标准模式）
rush gen entity task --field state:enum(0=DRAFT,1=ACTIVE@default=ACTIVE)

# 生成后自动 cargo check 验证
rush gen entity widget --field code:string --code-field code --check
```

字段模型：`string|i32|u32|bool|f64|enum(0=A,1=B@default=B)`。`--code-field`
要求该字段为 string。`--global` 同时裁剪消息、实体、repo（global 臂）与
service 的租户面。枚举按仓内标准模式生成：消息内嵌 enum、SeaORM 文本列、
i32↔文本转换函数、`@default` 为未识别值回退。

生成物：

- 消息面 proto：`backend/api/protos/widget/service/v1/widget.proto`
- admin HTTP 注解面 proto：`backend/api/protos/admin/service/v1/i_widget.proto`
  （List/Get/Create/Update/Delete 五条路由，前缀缺省 `/admin/v1/widgets`）
- SeaORM 实体：`backend/services/admin-api/src/data/sys_widgets.rs`
- repo（`repo_shell!` 宏 + 租户作用域批量删除）：
  `backend/services/admin-api/src/data/repos/widget.rs`
- service 实现（对齐 rushwind-gen-http 生成的 Handlers trait）：
  `backend/services/admin-api/src/services/widget.rs`
- 五处注册（行级手术插入，幂等）：`data.rs` / `migration.rs` /
  `data/repos/mod.rs` / `services.rs` / `server/rest.rs`（use 导入块 +
  mount 表）
- proto MANIFEST 自动重建（`--skip-manifest` 跳过）

标准列（id / sort_order / 审计人与时间戳；租户表含 tenant_id）自动带上。
生成后 `cargo check` 即绿（在真实 rushwind-admin 上以三个实体验证过：code
臂、全局+枚举、三者组合，零警告），建议顺手 `cargo fmt`。

`--dry-run` 预览全部动作不落盘。已部署库的说明与 seed / testbed 等后续
手动步骤见命令输出的提示。

### `rush gen pages` —— React 前端 CRUD 页面组

为 `rush gen entity` 生成的实体配套生成 React 前端页面组（模板基准：仓内
dict 页面模式）：

```shell
rush gen pages widget \
  --field code:string --field label:string \
  --field "state:enum(0=OFF,1=ON@default=ON)" --code-field code
```

生成物（都在 `frontend/admin/react/` 下）：

- `src/api/hooks/<entity>.ts` —— **自包含类型 + requestApi 原生调用**：不
  依赖上游重新生成 TS 客户端，与生成客户端走同一条 axios 通道（token 注
  入、错误拦截照常生效）
- `src/pages/app/<group>/<plural>/` —— ProTable 列表 + DrawerForm 编辑抽
  屉 + constants + index（字段驱动列与表单控件；枚举/布尔渲染 Tag）
- `src/locales/zh-CN|en-US/_modules/<entity>.json` —— 双语文案骨架

路由注册是后端菜单种子驱动的：页面文件放到位即被动态路由拾取，导航出现
需在 seed.rs 加菜单项（生成器的提示会带上）。字段语法与 gen entity 完全
一致，两个命令的 `--field`/`--code-field`/`--route-prefix` 保持相同取值
即可对齐。

`--group` 指定页面分组目录（缺省 `system`）；`--dry-run` 预览。生成后建
议 `pnpm typecheck`（已在真实 rushwind-admin 前端上验证通过）。

### `rush new` —— 新项目脚手架

```shell
rush new my-server && cd my-server && cargo run
```

内嵌模板自包含：rushwind git 依赖钉在与 rushwind-admin 一致的 rev，一个
YAML 文档组装存储、自动 CRUD 边（`/items`）和 HTTP 服务器——`cargo run`
后按提示 `curl /health`、`/wired`、`/items` 即可体验全链。默认 `git
init`（`--no-git` 跳过），`--dir` 指定目标父目录。

存储变体 `--storage memory|postgres`（缺省 memory）：postgres 变体走
SeaORM 动态仓库（`SeaRepo::connect` + 启动建表），DSN 在
`storage.settings.url`。两个变体都做过实跑验证（postgres 变体在独立
Postgres 容器上验证了 CRUD 与落库持久化）。

`--template <dir>` 可换成任意外部模板（如 rushwind 仓的
`examples/bootstrap-demo`）：整树拷贝（跳过 `.git`/`target`），并按模板
Cargo.toml 的包名做 token 重命名（含下划线变体）；非文本文件字节级原样。

### `rush testbed` —— 差分台架编排与报告摘要

把 `backend/testbed/README` 的三步运行手册收敛成命令。**容器纪律**：Go 参照
侧跑在 docker 差分栈里，rush 绝不代启——Go 端点不可达时直接报错并给出手动
指令（`cd backend/testbed && docker compose up -d --build`）。

```shell
rush testbed run     # 构建 → 拉起 Rust 侧（:7788）→ 执行 admin-diff 回放 → 摘要报告
rush testbed report  # 离线摘要一份 JSONL 报告（缺省 backend/testbed/reports/report.jsonl）
```

`run` 的编排：预检语料/豁免集/两侧 crate 在位 → Go 端点预检（不可达即停）→
Rust 端点不可达则 `cargo build` 并拉起 admin-api、等待就绪 → 以仓内默认参数
执行回放器（`--go/--rust/--corpus/--exemptions/--out`）→ 回收拉起的进程
（`--keep-server` 保留）→ 透传回放器退出码并摘要报告。

`report` 的摘要：verdict 直方图（Ok/Fail/Exempt/Pending/Unreachable）、
class×verdict 矩阵、超豁免分歧（Fail）清单附分歧证据、不可达案例清单——
评审差分报告不用再肉眼扫 JSONL。

## 仓库结构

```
rushwind-toolkit/
├── crates/
│   ├── rush-cli/     # `rush` 可执行入口（clap 命令面）
│   └── rush-gen/     # 核心库：清单双算法 + adopt + 实体链生成 + 项目脚手架
└── ...
```

核心库与 CLI 分离：将来的桌面壳（若做，倾向 Tauri）直接复用 `rush-gen`，
不经过 CLI。

## 路线图

- [x] `rush adopt` / `rush manifest`
- [x] `rush gen entity`：领域实体后端全链生成（proto 双面 + SeaORM 实体 +
      repo + Handlers trait 实现 + 六处注册 + 清单重建）
- [x] `rush new`：新项目脚手架（内嵌自包含模板 + 外部模板整树拷贝与包名
      重命名）
- [x] `rush testbed`：差分台架编排（构建/拉起/回放/回收，容器纪律：不碰
      docker）+ JSONL 报告离线摘要
- [ ] 前端页面生成：Vben / Element / React 三栈的页面脚手架（优先对接
      既有前端生成链，而非在 Rust 侧重写）

## 相关仓库

- [rushwind](https://github.com/tx7do/rushwind) — Rust 微服务框架本体
- [rushwind-admin](https://github.com/tx7do/rushwind-admin) — 契约驱动的管理后台脚手架
