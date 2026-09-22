//! 下游接管（adopt）：把 rushwind-admin 快照从“上游镜像”转为下游自有仓。
//!
//! 三件事：
//!
//! 1. 以当前树为基线重建两个 MANIFEST（proto / react）——下游从此对
//!    契约与前端快照拥有完全修改权；
//! 2. 从 `.github/workflows/ci.yml` 剥离两个上游镜像门禁步（连同紧邻
//!    的注释块一并移除）——手改 proto/前端不再红 CI；
//! 3. 上游漂移基线 `react.UPSTREAM.sha256` 默认保留并提示，可用
//!    `prune_upstream_baseline` 删除。
//!
//! `sync-*.sh` 脚本本身保留不动（CI 已不再调用）：它们仍可用于在持有
//! 上游仓的机器上拉取上游更新，但下游日常应以 [`crate::manifest`] 的
//! rebuild 维护清单。

use std::fs;
use std::path::PathBuf;

use crate::manifest::Flavor;
use crate::{manifest, Result};

/// adopt 选项。
#[derive(Debug, Clone)]
pub struct AdoptOptions {
    /// rushwind-admin 仓库根目录。
    pub repo_root: PathBuf,
    /// 只报告不落盘。
    pub dry_run: bool,
    /// 保留 CI 门禁步（仅重建清单）。
    pub keep_gates: bool,
    /// 跳过 proto 契约面。
    pub skip_proto: bool,
    /// 跳过 react 前端面。
    pub skip_react: bool,
    /// 删除上游漂移基线 `react.UPSTREAM.sha256`。
    pub prune_upstream_baseline: bool,
    /// 保留 sync-*.sh 原脚本（默认机制性退役：改写为拒跑 stub）。
    pub keep_sync_scripts: bool,
}

/// 退役 stub：拒跑 + 指引 rush manifest rebuild。保留原脚本时不用。
const RETIRED_SCRIPT: &str = r#"#!/usr/bin/env bash
# 已由 rush adopt 机制性退役：本仓已从「上游镜像」切换为「下游自有仓」。
#
# 原脚本对下游是反向语义——sync 要求持有上游仓且会整树覆盖（rm -rf 后重拷），
# --check 会把合法的手改当篡改。清单维护请改用：
#
#   rush manifest proto --rebuild   # backend/api 契约面
#   rush manifest react --rebuild   # frontend/admin/react 快照面
#
echo "refusing: this repo has been adopted (downstream-owned); use 'rush manifest <proto|react> --rebuild'" >&2
exit 1
"#;

const RETIRED_MARKER: &str = "已由 rush adopt 机制性退役";

/// `react.UPSTREAM.sha256` 的处置结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UpstreamBaseline {
    /// 已按选项删除。
    Pruned,
    /// 保留（默认）。
    #[default]
    Kept,
    /// 本来就不存在。
    Absent,
}

/// adopt 结果报告。
#[derive(Debug, Default)]
pub struct AdoptReport {
    /// proto 清单条目数（重建或 dry-run 评估）。
    pub proto_entries: Option<usize>,
    /// react 清单条目数（重建或 dry-run 评估）。
    pub react_entries: Option<usize>,
    /// 从 CI 剥离的门禁步名称。
    pub removed_ci_steps: Vec<String>,
    /// CI 里本就没有门禁步（已接管过）。
    pub ci_gates_already_absent: bool,
    /// `.github/workflows/ci.yml` 不存在。
    pub ci_missing: bool,
    /// 已退役（改写为拒跑 stub）的 sync 脚本。
    pub retired_scripts: Vec<String>,
    pub upstream_baseline: UpstreamBaseline,
}

/// 执行 adopt。幂等可重入：门禁步已不在时仅重建清单并报告。
pub fn adopt(opts: &AdoptOptions) -> Result<AdoptReport> {
    let root = &opts.repo_root;
    let mut report = AdoptReport::default();

    if !opts.skip_proto {
        let flavor = Flavor::Proto;
        let tree = flavor.tree_path(root);
        let path = flavor.manifest_path(root);
        report.proto_entries = if opts.dry_run {
            Some(manifest::build_manifest(&tree, flavor)?.lines().count())
        } else {
            Some(manifest::rebuild(&tree, &path, flavor)?)
        };
    }

    if !opts.skip_react {
        let flavor = Flavor::React;
        let tree = flavor.tree_path(root);
        let path = flavor.manifest_path(root);
        report.react_entries = if opts.dry_run {
            Some(manifest::build_manifest(&tree, flavor)?.lines().count())
        } else {
            Some(manifest::rebuild(&tree, &path, flavor)?)
        };
    }

    let baseline = root.join("frontend/admin/react.UPSTREAM.sha256");
    report.upstream_baseline = if !baseline.exists() {
        UpstreamBaseline::Absent
    } else if opts.prune_upstream_baseline {
        if !opts.dry_run {
            fs::remove_file(&baseline)?;
        }
        UpstreamBaseline::Pruned
    } else {
        UpstreamBaseline::Kept
    };

    if !opts.keep_gates {
        let ci = root.join(".github/workflows/ci.yml");
        if !ci.is_file() {
            report.ci_missing = true;
        } else {
            let text = fs::read_to_string(&ci)?;
            let (stripped, removed) = strip_gate_steps(&text);
            if removed.is_empty() {
                report.ci_gates_already_absent = true;
            } else {
                report.removed_ci_steps = removed;
                if !opts.dry_run {
                    fs::write(&ci, &stripped)?;
                }
            }
        }
    }

    // sync 脚本机制性退役：改写为拒跑 stub（防误运行——sync 会整树覆盖，
    // --check 会把手改当篡改，对下游都是反向语义）。
    if !opts.keep_sync_scripts {
        for script in ["backend/api/sync-protos.sh", "frontend/admin/sync-react.sh"] {
            let path = root.join(script);
            if !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if text.contains(RETIRED_MARKER) {
                continue;
            }
            if !opts.dry_run {
                fs::write(&path, RETIRED_SCRIPT)?;
            }
            report.retired_scripts.push(script.to_owned());
        }
    }

    Ok(report)
}

/// CI 门禁步的识别标记：step 块内同时含脚本名与 `--check`。
const GATE_SCRIPTS: [&str; 2] = ["sync-protos.sh", "sync-react.sh"];

/// 从 CI YAML 文本中剥离上游镜像门禁步（连同其紧邻的注释块）。
///
/// 返回剥离后的文本与被剥离的 step 名称。幂等：无门禁步时原样返回。
/// 行级手术，保留原文件其余字节（CRLF 行尾的 `\r` 留在行内不动，同样
/// 保真）。
pub fn strip_gate_steps(yaml: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = yaml.split('\n').collect();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut names = Vec::new();

    for marker in marker_lines(&lines) {
        let Some(head) = step_head(&lines, marker) else {
            continue;
        };
        if ranges
            .iter()
            .any(|&(start, end)| head >= start && head < end)
        {
            continue; // 同一 step 的第二个标记行
        }
        let start = comment_start(&lines, head);
        let mut end = body_end(&lines, head);
        if start > 0
            && lines[start - 1].trim().is_empty()
            && end + 1 < lines.len()
            && lines[end + 1].trim().is_empty()
        {
            end += 1; // 吞掉一个尾随空行，避免留下双空行
        }
        ranges.push((start, end + 1));
        names.push(step_name(lines[head]));
    }

    if ranges.is_empty() {
        return (yaml.to_owned(), names);
    }
    ranges.sort_unstable();
    let mut keep: Vec<&str> = Vec::with_capacity(lines.len());
    let mut cursor = 0;
    for (start, end) in &ranges {
        keep.extend_from_slice(&lines[cursor..*start]);
        cursor = *end;
    }
    keep.extend_from_slice(&lines[cursor..]);
    (keep.join("\n"), names)
}

/// 含门禁标记的行号（step 的 run 行同时含脚本名与 `--check`）。
fn marker_lines(lines: &[&str]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            GATE_SCRIPTS.iter().any(|script| line.contains(script)) && line.contains("--check")
        })
        .map(|(index, _)| index)
        .collect()
}

/// 标记行所属 step 的首行（向上最近的 `- ` 列表项）。
fn step_head(lines: &[&str], marker: usize) -> Option<usize> {
    (0..=marker).rev().find(|&index| is_list_item(lines[index]))
}

fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("- ") || trimmed == "-"
}

/// step 首行之上连续注释块的起点。
fn comment_start(lines: &[&str], head: usize) -> usize {
    let mut start = head;
    while start > 0 && lines[start - 1].trim_start().starts_with('#') {
        start -= 1;
    }
    start
}

/// step 体末行（首行之下缩进更深且非空的连续行）。
fn body_end(lines: &[&str], head: usize) -> usize {
    let indent = indent_of(lines[head]);
    let mut end = head;
    while end + 1 < lines.len() {
        let next = lines[end + 1];
        if next.trim().is_empty() || indent_of(next) <= indent {
            break;
        }
        end += 1;
    }
    end
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn step_name(line: &str) -> String {
    line.trim_start()
        .strip_prefix("- ")
        .and_then(|rest| rest.trim().strip_prefix("name:"))
        .map(str::trim)
        .unwrap_or("<unknown>")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/ci.yml");

    #[test]
    fn strips_both_gate_steps_with_their_comments() {
        let (stripped, names) = strip_gate_steps(FIXTURE);
        assert_eq!(
            names,
            vec![
                "Proto contract gate (MANIFEST consistency)",
                "Frontend snapshot gate (MANIFEST consistency)",
            ]
        );
        assert!(!stripped.contains("sync-protos"));
        assert!(!stripped.contains("sync-react"));
        assert!(!stripped.contains("anti-tamper"));
        // 其余 step 与结构完好
        assert!(stripped.contains("- name: Install protoc (ubuntu)"));
        assert!(stripped.contains("- name: Install protoc (windows)"));
        assert!(stripped.contains("- name: Format"));
        assert!(stripped.contains("cargo test --workspace"));
        assert!(stripped.ends_with("run: cargo test --workspace\n"));
        // 不留下双空行
        assert!(!stripped.contains("\n\n\n"));
    }

    #[test]
    fn stripping_is_idempotent() {
        let (once, _) = strip_gate_steps(FIXTURE);
        let (twice, names) = strip_gate_steps(&once);
        assert!(names.is_empty());
        assert_eq!(once, twice);
    }
}
