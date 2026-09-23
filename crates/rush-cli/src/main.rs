//! `rush` — RushWind 生态工具箱 CLI。

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use rush_gen::adopt::{self, AdoptOptions, UpstreamBaseline};
use rush_gen::doctor;
use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use rush_gen::manifest::{self, CheckReport, Flavor};
use rush_gen::pages::{self, PagesOptions};
use rush_gen::project::{self, NewOptions, StorageKind};
use rush_gen::testbed::{self, RunOptions};
use rush_gen::undo;

/// rush — RushWind 生态工具箱
#[derive(Debug, Parser)]
#[command(
    name = "rush",
    version,
    propagate_version = true,
    about = "RushWind 生态工具箱：接管快照仓、维护契约清单、生成实体链与项目脚手架"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// 所有报告以 JSON 输出（机器可读；报告结构体全部可序列化）
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// 把 rushwind-admin 快照从“上游镜像”接管为下游自有仓：重建双 MANIFEST
    /// 基线 + 剥离 CI 同步门禁步
    Adopt {
        /// rushwind-admin 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 只报告将要发生的变更，不写任何文件
        #[arg(long)]
        dry_run: bool,
        /// 保留 CI 门禁步，只重建清单
        #[arg(long)]
        keep_gates: bool,
        /// 跳过 proto 契约面
        #[arg(long)]
        skip_proto: bool,
        /// 跳过 react 前端面
        #[arg(long)]
        skip_react: bool,
        /// 顺带删除上游漂移基线 react.UPSTREAM.sha256
        #[arg(long)]
        prune_upstream_baseline: bool,
        /// 保留 sync-*.sh 原脚本（默认机制性退役为拒跑 stub）
        #[arg(long)]
        keep_sync_scripts: bool,
    },
    /// 校验或重建某个同步面的 sha256 清单（默认校验）
    Manifest {
        /// 同步面
        #[arg(value_enum)]
        flavor: FlavorArg,
        /// 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 以当前树为基线重建清单
        #[arg(long)]
        rebuild: bool,
    },
    /// 从模板创建一个新的 RushWind 服务项目（内嵌模板开箱即跑，
    /// `--template` 可指定任意外部模板目录并按其包名重命名）
    New {
        /// 项目名（= crate 名 + 目录名）
        #[arg(value_name = "NAME")]
        name: String,
        /// 目标父目录
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// 内嵌模板的存储变体（--template 给出时忽略）
        #[arg(long, value_enum, default_value_t = StorageArg::Memory)]
        storage: StorageArg,
        /// 外部模板目录（缺省用内嵌模板）
        #[arg(long)]
        template: Option<PathBuf>,
        /// 不执行 git init
        #[arg(long)]
        no_git: bool,
        /// 只报告不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 差分台架编排（容器纪律：绝不代启 docker，Go 侧栈请按 testbed/README 手动启动）
    Testbed {
        #[command(subcommand)]
        action: TestbedAction,
    },
    /// 生成代码
    Gen {
        #[command(subcommand)]
        target: GenTarget,
    },
    /// 环境体检：工具箱全链的外部依赖一次探明（✓/⚠/✗ + 修复提示）
    Doctor {
        /// 目标仓根目录（给出时追加仓形状检查）
        #[arg(long)]
        repo: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum TestbedAction {
    /// 构建并拉起 Rust 侧，以仓内默认参数执行 admin-diff 回放，随后摘要报告。
    /// Go 侧 docker 栈不可达时直接报错（不代启）。
    Run {
        /// rushwind-admin 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Go 参照侧端点
        #[arg(long, default_value = testbed::DEFAULT_GO)]
        go: String,
        /// Rust 复刻侧端点
        #[arg(long, default_value = testbed::DEFAULT_RUST)]
        rust: String,
        /// 拉起 Rust 侧后的就绪等待秒数
        #[arg(long, default_value_t = 60)]
        wait: u64,
        /// 回放后保留拉起的 admin-api 进程
        #[arg(long)]
        keep_server: bool,
        /// 跳过 cargo build（二进制已就绪时）
        #[arg(long)]
        skip_build: bool,
    },
    /// 摘要一份 JSONL 报告（verdict 直方图 + class 矩阵 + Fail/Unreachable 清单）
    Report {
        /// rushwind-admin 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 报告文件（缺省 backend/testbed/reports/report.jsonl）
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum GenTarget {
    /// 生成前端 CRUD 页面组（--stack 选 react|vben|element；自包含类型走
    /// requestApi，不依赖上游 TS 客户端；字段语法与 gen entity 一致。react
    /// 菜单走后端 seed，vben/element 写前端静态路由模块）
    Pages {
        /// 目标前端栈
        #[arg(long, value_enum, default_value_t = StackArg::React)]
        stack: StackArg,
        /// 实体名，snake_case 单数（须与 gen entity 一致）
        #[arg(value_name = "NAME")]
        name: String,
        /// 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 页面分组目录（pages/app/<group>/<plural>，缺省 system）
        #[arg(long)]
        group: Option<String>,
        /// 路由前缀（须与 gen entity 一致，缺省 /admin/v1/<复数>）
        #[arg(long)]
        route_prefix: Option<String>,
        /// 业务字段，语法同 gen entity --field；可重复。缺省时读取
        /// .rush/<name>.json 规格文件（gen entity 落盘的字段真相）
        #[arg(long = "field", value_name = "NAME:KIND")]
        fields: Vec<String>,
        /// 唯一编码字段（抽屉必填 + 搜索列）；缺省继承规格文件
        #[arg(long, value_name = "FIELD")]
        code_field: Option<String>,
        /// 平台全局表（页面去 tenantId；缺省继承规格）
        #[arg(long)]
        global: bool,
        /// 重生成：既有页面组覆盖
        #[arg(long)]
        regen: bool,
        /// 只报告不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 回滚一个实体的全部生成物：后端链 + 三栈页面 + 菜单/路由登记 +
    /// 规格文件（按 .rush/<name>.json 反向移除；数据库表不动）。幂等。
    Undo {
        /// 实体名
        #[arg(value_name = "NAME")]
        name: String,
        /// 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 只报告不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 生成一个标准 CRUD 实体的后端全链：消息面 proto + admin HTTP 注解面
    /// proto + SeaORM 实体 + repo（repo_shell! 宏）+ Handlers trait 实现 +
    /// data/migration/repos/services/mount 五处注册 + proto MANIFEST 重建，
    /// 并把字段清单落成 .rush/<name>.json 规格文件（gen pages 的默认输入）。
    /// 模板基准：rushwind-admin 的 dict_type 实体链。
    Entity {
        /// 实体名，snake_case 单数（如 widget）
        #[arg(value_name = "NAME")]
        name: String,
        /// 仓库根目录
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// 表名（缺省 sys_<复数>）
        #[arg(long)]
        table: Option<String>,
        /// 消息面 package（缺省 <name>.service.v1）
        #[arg(long)]
        package: Option<String>,
        /// 路由前缀（缺省 /admin/v1/<复数>）
        #[arg(long)]
        route_prefix: Option<String>,
        /// 业务字段，格式 name:kind，kind ∈ string|i32|u32|bool|f64 或
        /// enum(0=A,1=B@default=B)；可重复
        #[arg(long = "field", value_name = "NAME:KIND")]
        fields: Vec<String>,
        /// 唯一编码字段（须为 string 字段）：启用 Get 的 code 臂与
        /// /code/{code} 附加路由
        #[arg(long, value_name = "FIELD")]
        code_field: Option<String>,
        /// 平台全局表：全链去租户
        #[arg(long)]
        global: bool,
        /// 生成后执行 cargo check -p admin-api 验证
        #[arg(long)]
        check: bool,
        /// 只报告不落盘
        #[arg(long)]
        dry_run: bool,
        /// 跳过 proto MANIFEST 重建
        #[arg(long)]
        skip_manifest: bool,
        /// 重生成：既有生成物按规格覆盖（--field 缺省时整体读 .rush/<name>.json）
        #[arg(long)]
        regen: bool,
        /// 把本服务全部方法登记进免鉴权白名单（auth_free.rs）
        #[arg(long)]
        auth_free: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FlavorArg {
    /// proto 契约面（backend/api/protos）
    Proto,
    /// react 前端快照面（frontend/admin/react）
    React,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StackArg {
    /// React 栈（frontend/admin/react，菜单走后端 seed）
    React,
    /// vue-vben 栈（views + 静态路由模块）
    Vben,
    /// vue-element 栈（pages + 静态路由模块）
    Element,
}

impl From<StackArg> for rush_gen::pages::PagesStack {
    fn from(value: StackArg) -> Self {
        match value {
            StackArg::React => Self::React,
            StackArg::Vben => Self::Vben,
            StackArg::Element => Self::Element,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StorageArg {
    /// 内存存储（开箱即跑）
    Memory,
    /// PostgreSQL（SeaORM 动态仓库）
    Postgres,
    /// 内嵌 SQLite（单连接 :memory:，开箱即跑）
    Sqlite,
}

impl From<StorageArg> for StorageKind {
    fn from(value: StorageArg) -> Self {
        match value {
            StorageArg::Memory => Self::Memory,
            StorageArg::Postgres => Self::Postgres,
            StorageArg::Sqlite => Self::Sqlite,
        }
    }
}

impl From<FlavorArg> for Flavor {
    fn from(value: FlavorArg) -> Self {
        match value {
            FlavorArg::Proto => Self::Proto,
            FlavorArg::React => Self::React,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    match cli.command {
        Commands::Adopt {
            repo,
            dry_run,
            keep_gates,
            skip_proto,
            skip_react,
            prune_upstream_baseline,
            keep_sync_scripts,
        } => {
            if skip_proto && skip_react && keep_gates {
                bail!("--skip-proto + --skip-react + --keep-gates 没有任何可执行的动作");
            }
            let opts = AdoptOptions {
                repo_root: repo,
                dry_run,
                keep_gates,
                skip_proto,
                skip_react,
                prune_upstream_baseline,
                keep_sync_scripts,
            };
            let report = adopt::adopt(&opts).context("adopt 失败")?;
            render_adopt(&report, dry_run, json);
            Ok(())
        }
        Commands::Manifest {
            flavor,
            repo,
            rebuild,
        } => {
            let flavor = Flavor::from(flavor);
            let tree = flavor.tree_path(&repo);
            let path = flavor.manifest_path(&repo);
            if rebuild {
                let count = manifest::rebuild(&tree, &path, flavor).context("清单重建失败")?;
                if json {
                    println!(
                        r#"{{ "rebuilt": true, "entries": {count}, "path": "{}" }}"#,
                        path.display()
                    );
                } else {
                    println!("已重建 {}（{count} 条）", path.display());
                }
            } else {
                match manifest::check(&tree, &path, flavor).context("清单校验失败")? {
                    report if report.is_ok() => {
                        if json {
                            println!(r#"{{ "ok": true }}"#);
                        } else {
                            println!("OK: {} 与 {} 一致", tree.display(), path.display());
                        }
                    }
                    report => {
                        render_check(&report, json);
                        std::process::exit(1);
                    }
                }
            }
            Ok(())
        }
        Commands::New {
            name,
            dir,
            storage,
            template,
            no_git,
            dry_run,
        } => {
            let opts = NewOptions {
                name,
                dest: dir,
                storage: storage.into(),
                template,
                git: !no_git,
                dry_run,
            };
            let report = project::new_project(&opts).context("new 失败")?;
            render_new(&report, dry_run, json);
            Ok(())
        }
        Commands::Testbed { action } => match action {
            TestbedAction::Run {
                repo,
                go,
                rust,
                wait,
                keep_server,
                skip_build,
            } => {
                let opts = RunOptions {
                    repo_root: repo,
                    go,
                    rust,
                    wait_secs: wait,
                    keep_server,
                    skip_build,
                };
                let report = testbed::run_rig(&opts).context("testbed run 失败")?;
                if report.spawned_server {
                    println!(
                        "已拉起 Rust 侧 admin-api（回放{}）",
                        if keep_server {
                            "后保留"
                        } else {
                            "后回收"
                        }
                    );
                }
                if let Some(summary) = &report.summary {
                    println!();
                    render_summary(summary, json);
                }
                for note in &report.notes {
                    println!("注意：{note}");
                }
                if report.exit_code != 0 {
                    std::process::exit(report.exit_code);
                }
                Ok(())
            }
            TestbedAction::Report { repo, file } => {
                let path =
                    file.unwrap_or_else(|| repo.join("backend/testbed/reports/report.jsonl"));
                let summary = testbed::summarize(&path).context("报告摘要失败")?;
                if !json {
                    println!("报告：{}", path.display());
                }
                render_summary(&summary, json);
                Ok(())
            }
        },
        Commands::Gen {
            target:
                GenTarget::Pages {
                    name,
                    repo,
                    stack,
                    group,
                    route_prefix,
                    fields,
                    code_field,
                    global,
                    regen,
                    dry_run,
                },
        } => {
            let mut parsed = Vec::with_capacity(fields.len());
            for spec in &fields {
                let Some((fname, kind)) = spec.rsplit_once(':') else {
                    bail!("字段格式应为 name:kind（如 code:string）：{spec}");
                };
                let Some(kind) = FieldKind::parse(kind) else {
                    bail!("未知字段类型 {kind}（支持 string|i32|u32|bool|f64 或 enum(0=A,1=B@default=B)）：{spec}");
                };
                parsed.push(FieldSpec {
                    name: fname.to_owned(),
                    kind,
                });
            }
            let opts = PagesOptions {
                repo_root: repo,
                name,
                group,
                route_prefix,
                fields: parsed,
                code_field,
                stack: stack.into(),
                global: if global { Some(true) } else { None },
                overwrite: regen,
                dry_run,
            };
            let report = pages::generate_pages(&opts).context("gen pages 失败")?;
            render_pages(&report, dry_run, json);
            Ok(())
        }
        Commands::Gen {
            target:
                GenTarget::Undo {
                    name,
                    repo,
                    dry_run,
                },
        } => {
            let opts = undo::UndoOptions {
                repo_root: repo,
                name,
                dry_run,
            };
            let report = undo::undo_entity(&opts).context("gen undo 失败")?;
            render_undo(&report, dry_run, json);
            Ok(())
        }
        Commands::Gen {
            target:
                GenTarget::Entity {
                    name,
                    repo,
                    table,
                    package,
                    route_prefix,
                    fields,
                    code_field,
                    global,
                    check,
                    dry_run,
                    skip_manifest,
                    regen,
                    auth_free,
                },
        } => {
            let mut parsed = Vec::with_capacity(fields.len());
            for spec in &fields {
                let Some((fname, kind)) = spec.rsplit_once(':') else {
                    bail!("字段格式应为 name:kind（如 code:string 或 status:enum(0=A,1=B@default=B)）：{spec}");
                };
                let Some(kind) = FieldKind::parse(kind) else {
                    bail!("未知字段类型 {kind}（支持 string|i32|u32|bool|f64 或 enum(0=A,1=B@default=B)）：{spec}");
                };
                parsed.push(FieldSpec {
                    name: fname.to_owned(),
                    kind,
                });
            }
            let opts = EntityOptions {
                repo_root: repo,
                name,
                table,
                package,
                route_prefix,
                fields: parsed,
                code_field,
                global,
                check,
                dry_run,
                skip_manifest,
                overwrite: regen,
                auth_free,
            };
            let report = entity::generate_entity(&opts).context("gen entity 失败")?;
            render_gen(&report, dry_run, json);
            Ok(())
        }
        Commands::Doctor { repo } => {
            let report = doctor::run_doctor(repo.as_deref());
            render_doctor(&report, json);
            if report.has_failures() {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn render_pages(report: &pages::PagesReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    let tag = if dry_run { "[dry-run] " } else { "" };
    if !report.created.is_empty() {
        println!("{tag}新建文件：");
        for path in &report.created {
            println!("  + {}", path.display());
        }
    }
    if !report.edited.is_empty() {
        println!("{tag}编辑文件：");
        for path in &report.edited {
            println!("  ~ {}", path.display());
        }
    }
    for item in &report.skipped {
        println!("  = 跳过 {item}");
    }
    if !report.notes.is_empty() {
        println!();
        for note in &report.notes {
            println!("注意：{note}");
        }
    }
}

fn render_undo(report: &undo::UndoReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    let tag = if dry_run { "[dry-run] " } else { "" };
    if !report.removed_files.is_empty() {
        println!("{tag}删除文件：");
        for path in &report.removed_files {
            println!("  - {}", path.display());
        }
    }
    if !report.removed_dirs.is_empty() {
        println!("{tag}删除目录：");
        for path in &report.removed_dirs {
            println!("  - {}/", path.display());
        }
    }
    if !report.edited_files.is_empty() {
        println!("{tag}还原登记：");
        for path in &report.edited_files {
            println!("  ~ {}", path.display());
        }
    }
    for item in &report.skipped {
        println!("  = 跳过 {item}");
    }
    if !report.notes.is_empty() {
        println!();
        for note in &report.notes {
            println!("注意：{note}");
        }
    }
}

fn render_doctor(report: &doctor::DoctorReport, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    for check in &report.checks {
        let mark = match check.status {
            doctor::Status::Ok => "✓",
            doctor::Status::Warn => "⚠",
            doctor::Status::Fail => "✗",
        };
        println!("{mark} {:<16} {}", check.name, check.detail);
        if let Some(hint) = &check.hint {
            println!("                 ↳ {hint}");
        }
    }
}

fn render_summary(summary: &testbed::Summary, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(summary).expect("摘要序列化")
        );
        return;
    }
    println!(
        "总计 {} 案例：Ok {} / Fail {} / Exempt {} / Pending {} / Unreachable {}",
        summary.total,
        summary.count("Ok"),
        summary.count("Fail"),
        summary.count("Exempt"),
        summary.count("Pending"),
        summary.count("Unreachable")
    );
    if !summary.by_class.is_empty() {
        println!("按 class：");
        for (class, verdicts) in &summary.by_class {
            let parts: Vec<String> = verdicts
                .iter()
                .map(|(verdict, count)| format!("{verdict} {count}"))
                .collect();
            println!("  {class}: {}", parts.join(", "));
        }
    }
    if !summary.fails.is_empty() {
        println!("Fail（超出豁免的分歧）：");
        for entry in &summary.fails {
            match &entry.detail {
                Some(detail) => println!("  ! {} [{}] {}", entry.id, entry.class, detail),
                None => println!("  ! {} [{}]", entry.id, entry.class),
            }
        }
    }
    if !summary.unreachable.is_empty() {
        println!("Unreachable：{}", summary.unreachable.join(", "));
    }
}

fn render_new(report: &project::NewReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    let tag = if dry_run { "[dry-run] " } else { "" };
    println!(
        "{tag}项目目录：{}（模板：{}）",
        report.project_dir.display(),
        report.template_source
    );
    if let Some(old) = &report.renamed_from {
        println!("{tag}重命名自包名：{old}");
    }
    for path in &report.files {
        println!("  + {}", path.display());
    }
    for note in &report.notes {
        println!("注意：{note}");
    }
}

fn render_gen(report: &entity::EntityReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    let tag = if dry_run { "[dry-run] " } else { "" };
    if !report.created.is_empty() {
        println!("{tag}新建文件：");
        for path in &report.created {
            println!("  + {}", path.display());
        }
    }
    if !report.updated.is_empty() {
        println!("{tag}覆盖文件：");
        for path in &report.updated {
            println!("  ↻ {}", path.display());
        }
    }
    if !report.edited.is_empty() {
        println!("{tag}编辑文件：");
        for path in &report.edited {
            println!("  ~ {}", path.display());
        }
    }
    for (path, diff) in &report.diffs {
        println!("{tag}diff {path}：");
        println!("{}", diff.trim_end());
    }
    for item in &report.skipped {
        println!("  = 跳过 {item}");
    }
    if let Some(count) = report.manifest_entries {
        println!("{tag}proto MANIFEST：{count} 条");
    }
    match report.check_passed {
        Some(true) => println!("{tag}cargo check -p admin-api：通过"),
        Some(false) => println!("{tag}cargo check -p admin-api：未通过（见注意）"),
        None => {}
    }
    if !report.notes.is_empty() {
        println!();
        for note in &report.notes {
            println!("注意：{note}");
        }
    }
}

fn render_adopt(report: &adopt::AdoptReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    let tag = if dry_run { "[dry-run] " } else { "" };
    if let Some(count) = report.proto_entries {
        println!("{tag}proto MANIFEST：以当前树为基线重建（{count} 条）");
    }
    if let Some(count) = report.react_entries {
        println!("{tag}react MANIFEST：以当前树为基线重建（{count} 条）");
    }
    match report.upstream_baseline {
        UpstreamBaseline::Pruned => println!("{tag}已删除 react.UPSTREAM.sha256"),
        UpstreamBaseline::Kept => {
            println!("{tag}react.UPSTREAM.sha256 保留（可用 --prune-upstream-baseline 删除）")
        }
        UpstreamBaseline::Absent => {}
    }
    if report.ci_missing {
        println!("未找到 .github/workflows/ci.yml，跳过门禁剥离");
    } else if !report.removed_ci_steps.is_empty() {
        for name in &report.removed_ci_steps {
            println!("{tag}已剥离 CI 门禁步：{name}");
        }
    } else if report.ci_gates_already_absent {
        println!("CI 中未发现同步门禁步（可能已接管过）");
    }
    if !report.retired_scripts.is_empty() {
        for script in &report.retired_scripts {
            println!("{tag}已退役 sync 脚本（改写为拒跑 stub）：{script}");
        }
    }
    println!();
    println!("提示：sync-*.sh 已不再被 CI 调用。下游日常请用 `rush manifest <proto|react> --rebuild` 重建清单；");
    println!("切勿再运行 sync 脚本——sync 要求持有上游仓且会整树覆盖，--check 会把手改当篡改。");
}

fn render_check(report: &CheckReport, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("报告序列化")
        );
        return;
    }
    for path in &report.added {
        println!("  + {path}（树上有，清单没有）");
    }
    for path in &report.removed {
        println!("  - {path}（清单有，树上没有）");
    }
    for path in &report.modified {
        println!("  ~ {path}（内容与清单不符）");
    }
    eprintln!(
        "FAIL: 树与清单不一致（新增 {} / 删除 {} / 改动 {}）——以当前树为基线请执行 --rebuild",
        report.added.len(),
        report.removed.len(),
        report.modified.len()
    );
}
