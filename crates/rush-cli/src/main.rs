//! `rush` — RushWind 生态工具箱 CLI。

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use rush_gen::adopt::{self, AdoptOptions, UpstreamBaseline};
use rush_gen::manifest::{self, CheckReport, Flavor};

/// rush — RushWind 生态工具箱
#[derive(Debug, Parser)]
#[command(
    name = "rush",
    version,
    propagate_version = true,
    about = "RushWind 生态工具箱：接管快照仓、维护契约清单（生成能力按路线图演进）"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    /// [规划中] 从模板创建新项目
    New {
        /// 项目名
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// [规划中] 生成领域实体链
    Gen {
        #[command(subcommand)]
        target: GenTarget,
    },
}

#[derive(Debug, Subcommand)]
enum GenTarget {
    /// 生成一个领域实体的后端全链（proto 模板 / SeaORM 实体 / migration /
    /// repo / service 骨架）
    Entity {
        /// 实体名（snake_case）
        #[arg(value_name = "NAME")]
        name: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FlavorArg {
    /// proto 契约面（backend/api/protos）
    Proto,
    /// react 前端快照面（frontend/admin/react）
    React,
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
    match cli.command {
        Commands::Adopt {
            repo,
            dry_run,
            keep_gates,
            skip_proto,
            skip_react,
            prune_upstream_baseline,
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
            };
            let report = adopt::adopt(&opts).context("adopt 失败")?;
            render_adopt(&report, dry_run);
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
                println!("已重建 {}（{count} 条）", path.display());
            } else {
                match manifest::check(&tree, &path, flavor).context("清单校验失败")? {
                    report if report.is_ok() => {
                        println!("OK: {} 与 {} 一致", tree.display(), path.display());
                    }
                    report => {
                        render_check(&report);
                        std::process::exit(1);
                    }
                }
            }
            Ok(())
        }
        Commands::New { .. } | Commands::Gen { .. } => {
            bail!("该命令尚未实现（路线图见 README）；当前可用：rush adopt / rush manifest");
        }
    }
}

fn render_adopt(report: &adopt::AdoptReport, dry_run: bool) {
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
    println!();
    println!("提示：sync-*.sh 已不再被 CI 调用。下游日常请用 `rush manifest <proto|react> --rebuild` 重建清单；");
    println!("切勿再运行 sync 脚本——sync 要求持有上游仓且会整树覆盖，--check 会把手改当篡改。");
}

fn render_check(report: &CheckReport) {
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
