//! 差分台架编排（`rush testbed`）：把 testbed/README 的三步运行手册收敛成
//! 一条命令，外加离线的报告摘要。
//!
//! **容器纪律**：Go 参照侧跑在 docker 差分栈里；按 2026-09 会话指令，本模
//! 块绝不代启/触碰 docker——Go 端点不可达时直接报错并给出手动指令。本模块
//! 只负责：构建并拉起 Rust 侧（admin-api 桩服务）、以仓内默认参数执行回放
//! 器（admin-diff）、进程清理、报告摘要。
//!
//! 回放器接口（对齐 crates 侧手写参数解析）：`admin-diff --go <url> --rust
//! <url> [--corpus dir] [--exemptions file] [--out file] [--wait secs]`，
//! 报告为 JSONL：每行 `{id, class, kind, verdict, go, rust, detail?}`，
//! verdict ∈ Ok/Fail/Exempt/Pending/Unreachable，退出码 2 表示任一端不可达。

use std::collections::BTreeMap;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{Error, Result};

/// 差分回放与摘要的仓内默认相对路径（相对 backend/）。
pub const DEFAULT_GO: &str = "http://127.0.0.1:27788";
pub const DEFAULT_RUST: &str = "http://127.0.0.1:7788";
const REPLAYER: &str = "admin-diff";
const SERVER: &str = "admin-api";

/// `rush testbed run` 选项。
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// rushwind-admin 仓库根目录。
    pub repo_root: PathBuf,
    /// Go 参照侧端点。
    pub go: String,
    /// Rust 复刻侧端点。
    pub rust: String,
    /// 拉起 Rust 侧后等待就绪的秒数。
    pub wait_secs: u64,
    /// 回放后保留拉起的 admin-api 进程（默认回放完即收）。
    pub keep_server: bool,
    /// 跳过 cargo build（两侧二进制已就绪时）。
    pub skip_build: bool,
}

/// `rush testbed run` 结果。
#[derive(Debug, Default)]
pub struct RunReport {
    /// 本次是否拉起了 admin-api（false = 复用已在运行的实例）。
    pub spawned_server: bool,
    /// 回放器退出码（0 全绿；2 任一端不可达）。
    pub exit_code: i32,
    /// 报告文件路径。
    pub report_path: PathBuf,
    pub summary: Option<Summary>,
    pub notes: Vec<String>,
}

/// run 的执行期句柄：拉起的 server 子进程，drop 时兜底回收。
struct SpawnedServer(Option<std::process::Child>);

impl SpawnedServer {
    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 执行差分台架。容器纪律：绝不触碰 docker。
pub fn run_rig(opts: &RunOptions) -> Result<RunReport> {
    let backend = opts.repo_root.join("backend");
    let corpus = backend.join("testbed/corpus");
    let exemptions = backend.join("testbed/exemptions.json");
    let report_path = backend.join("testbed/reports/report.jsonl");
    for (label, path) in [
        ("corpus 目录", &corpus),
        ("豁免集", &exemptions),
        ("回放器 crate", &backend.join("testbed/admin-diff")),
        ("服务 crate", &backend.join("services/admin-api")),
    ] {
        if !path.exists() {
            return Err(Error::InvalidInput(format!(
                "{label}缺失：{}（目标仓不是 rushwind-admin 形状？）",
                path.display()
            )));
        }
    }

    let mut report = RunReport {
        report_path: report_path.clone(),
        ..Default::default()
    };

    // Go 侧先行检查：不可达就停，绝不代启 docker。
    if !endpoint_up(&opts.go) {
        return Err(Error::InvalidInput(format!(
            "Go 参照侧不可达：{}。按容器纪律（2026-09 会话指令），rush 不会代启 docker 差分栈——请手动执行：\n  cd backend/testbed && docker compose up -d --build\n栈就绪后重试 rush testbed run。",
            opts.go
        )));
    }

    // Rust 侧：已在跑就复用，否则构建并拉起。
    let mut server = SpawnedServer(None);
    if endpoint_up(&opts.rust) {
        report
            .notes
            .push(format!("复用已在运行的 Rust 侧：{}", opts.rust));
    } else {
        if !opts.skip_build {
            let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args(["build", "-q", "-p", SERVER, "-p", REPLAYER])
                .current_dir(&backend)
                .status()
                .map_err(|err| Error::InvalidInput(format!("cargo 不可用：{err}")))?;
            if !status.success() {
                return Err(Error::InvalidInput(
                    "cargo build 失败（admin-api / admin-diff）".into(),
                ));
            }
        }
        let bin = backend.join("target/debug").join(bin_name(SERVER));
        let child = Command::new(&bin)
            .current_dir(&backend)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| Error::InvalidInput(format!("拉起 {} 失败：{err}", bin.display())))?;
        server.0 = Some(child);
        report.spawned_server = true;
        let deadline = Instant::now() + Duration::from_secs(opts.wait_secs);
        loop {
            if endpoint_up(&opts.rust) {
                break;
            }
            if Instant::now() >= deadline {
                server.stop();
                return Err(Error::InvalidInput(format!(
                    "Rust 侧 {} 秒内未就绪：{}（查看 admin-api 日志）",
                    opts.wait_secs, opts.rust
                )));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    // 回放：以仓内默认参数显式传参，报告写入 testbed/reports/。
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let replayer = backend.join("target/debug").join(bin_name(REPLAYER));
    let exit = Command::new(&replayer)
        .args([
            "--go",
            &opts.go,
            "--rust",
            &opts.rust,
            "--corpus",
            "testbed/corpus",
            "--exemptions",
            "testbed/exemptions.json",
            "--out",
            "testbed/reports/report.jsonl",
        ])
        .current_dir(&backend)
        .status()
        .map_err(|err| Error::InvalidInput(format!("执行 {} 失败：{err}", replayer.display())))?;
    report.exit_code = exit.code().unwrap_or(-1);
    if !opts.keep_server {
        server.stop();
    } else if report.spawned_server {
        report
            .notes
            .push("--keep-server：admin-api 保持运行，请自行回收".to_owned());
    }

    if report_path.is_file() {
        report.summary = Some(summarize(&report_path)?);
    }
    Ok(report)
}

fn bin_name(crate_name: &str) -> String {
    if cfg!(windows) {
        format!("{crate_name}.exe")
    } else {
        crate_name.to_owned()
    }
}

/// 端点可达性探针：TCP 连接成功即视为就绪（真正的逐案例判定由 admin-diff
/// 负责，这里只做编排层的预检）。仅支持 http://。
pub fn endpoint_up(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host_port = rest.split('/').next().unwrap_or_default();
    if host_port.is_empty() {
        return false;
    }
    use std::net::ToSocketAddrs;
    let Ok(addrs) = host_port.to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok() {
            return true;
        }
    }
    false
}

// ---- 报告摘要 ----

/// 一行报告（只取摘要关心的字段）。
#[derive(Debug, Deserialize)]
pub struct ReportEntry {
    pub id: String,
    pub class: String,
    pub kind: String,
    pub verdict: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// 报告摘要：verdict 直方图 + class×verdict 矩阵 + Fail/Unreachable 清单。
#[derive(Debug, Default)]
pub struct Summary {
    pub total: usize,
    pub by_verdict: BTreeMap<String, usize>,
    /// class → (verdict → 计数)。
    pub by_class: BTreeMap<String, BTreeMap<String, usize>>,
    pub fails: Vec<ReportEntry>,
    pub unreachable: Vec<String>,
}

impl Summary {
    pub fn count(&self, verdict: &str) -> usize {
        self.by_verdict.get(verdict).copied().unwrap_or(0)
    }
}

/// 解析 JSONL 报告并汇总。空行跳过；坏行报错（宁可挂门不放水）。
pub fn summarize(path: &Path) -> Result<Summary> {
    let text = fs::read_to_string(path)
        .map_err(|err| Error::InvalidInput(format!("报告不可读：{}（{err}）", path.display())))?;
    let mut summary = Summary::default();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: ReportEntry = serde_json::from_str(line).map_err(|err| {
            Error::InvalidInput(format!(
                "{} 第 {} 行不是合法报告条目：{err}",
                path.display(),
                index + 1
            ))
        })?;
        summary.total += 1;
        *summary.by_verdict.entry(entry.verdict.clone()).or_default() += 1;
        *summary
            .by_class
            .entry(entry.class.clone())
            .or_default()
            .entry(entry.verdict.clone())
            .or_default() += 1;
        match entry.verdict.as_str() {
            "Fail" => summary.fails.push(entry),
            "Unreachable" => summary.unreachable.push(entry.id),
            _ => {}
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_up_detects_listener_and_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(endpoint_up(&format!("http://{addr}")));
        assert!(!endpoint_up("http://127.0.0.1:9")); // discard 特权端口，通常关闭
        assert!(!endpoint_up("https://127.0.0.1:80"), "仅支持 http://");
        assert!(!endpoint_up("not-a-url"));
    }

    #[test]
    fn summarize_counts_verdicts_classes_and_collects_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("report.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"id":"r1","class":"sweep-gated","kind":"sweep-gated","verdict":"Ok","go":{"reachable":true,"status":401,"len":67},"rust":{"reachable":true,"status":401,"len":67}}"#,
                "\n",
                r#"{"id":"r2","class":"sweep-gated","kind":"sweep-gated","verdict":"Fail","go":{"reachable":true,"status":401,"len":67},"rust":{"reachable":true,"status":500,"len":9},"detail":"envelope mismatch"}"#,
                "\n",
                r#"{"id":"h1","class":"head-on-get","kind":"head","verdict":"Exempt","go":{"reachable":true,"status":405,"len":0},"rust":{"reachable":true,"status":200,"len":0}}"#,
                "\n",
                r#"{"id":"p1","class":"sweep-public","kind":"sweep-public","verdict":"Pending","go":{"reachable":true,"status":200,"len":3},"rust":{"reachable":true,"status":500,"len":2}}"#,
                "\n",
                r#"{"id":"g1","class":"gate-invalid","kind":"curated","verdict":"Ok","go":{"reachable":true,"status":401,"len":67},"rust":{"reachable":true,"status":401,"len":67}}"#,
                "\n",
                "\n",
            ),
        )
        .unwrap();

        let summary = summarize(&path).unwrap();
        assert_eq!(summary.total, 5);
        assert_eq!(summary.count("Ok"), 2);
        assert_eq!(summary.count("Fail"), 1);
        assert_eq!(summary.count("Exempt"), 1);
        assert_eq!(summary.count("Pending"), 1);
        assert_eq!(summary.count("Unreachable"), 0);
        assert_eq!(
            summary.by_class.get("sweep-gated").unwrap().get("Ok"),
            Some(&1)
        );
        assert_eq!(summary.fails.len(), 1);
        assert_eq!(summary.fails[0].id, "r2");
        assert_eq!(
            summary.fails[0].detail.as_deref(),
            Some("envelope mismatch")
        );
        assert!(summary.unreachable.is_empty());
    }

    #[test]
    fn summarize_collects_unreachable_and_rejects_bad_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("report.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"id":"x1","class":"sweep-gated","kind":"sweep-gated","verdict":"Unreachable","go":{"reachable":false,"status":null,"len":null},"rust":{"reachable":true,"status":401,"len":67}}"#,
                "\n",
            ),
        )
        .unwrap();
        let summary = summarize(&path).unwrap();
        assert_eq!(summary.unreachable, vec!["x1".to_owned()]);

        fs::write(&path, "not json\n").unwrap();
        let err = summarize(&path).unwrap_err();
        assert!(format!("{err}").contains("不是合法报告条目"), "{err}");
    }
}
