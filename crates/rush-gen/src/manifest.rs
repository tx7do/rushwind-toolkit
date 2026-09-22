//! 同步面清单：sha256 清单的构建、解析、校验与重建。
//!
//! 两个面、两套算法，均逐字对齐被替换的 shell 脚本：
//!
//! * [`Flavor::Proto`] 对齐 `backend/api/sync-protos.sh` 的
//!   `manifest_for`：树上全部 `*.proto`，哈希前剥除 UTF-8 BOM；
//! * [`Flavor::React`] 对齐 `frontend/admin/sync-react.sh` 的
//!   `hash_tree`：树上除 `node_modules`/`dist` 目录与 git 忽略路径外的
//!   全部文件，字节级哈希。要求目标树位于 git 工作树内（与脚本一致：
//!   宁可挂门也不放水）。
//!
//! 清单文本与 sha256sum 兼容：每行 `<hex>  <path>`，LF 结尾，路径为
//! 相对树根的正斜杠形式。与脚本唯一的刻意偏离：排序固定为路径字节序
//! （脚本的 `sort -z` 受环境 locale 影响），校验按内容映射比较，对行序
//! 不敏感，因此对既有清单做 `--check` 不受排序差异影响。

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::{Error, Result};

/// 清单覆盖的同步面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// proto 契约面（BOM 归一化哈希）。
    Proto,
    /// react 前端快照面（字节级哈希 + git 忽略语义）。
    React,
}

impl Flavor {
    /// 清单文件位置（相对仓库根）。proto 清单与 sync-protos.sh 同住
    /// `backend/api/`，不进 protos 树本体。
    pub fn manifest_path(self, repo_root: &Path) -> PathBuf {
        match self {
            Self::Proto => repo_root.join("backend/api/MANIFEST.sha256"),
            Self::React => repo_root.join("frontend/admin/react.MANIFEST.sha256"),
        }
    }

    /// 清单覆盖的树（相对仓库根）。
    pub fn tree_path(self, repo_root: &Path) -> PathBuf {
        match self {
            Self::Proto => repo_root.join("backend/api/protos"),
            Self::React => repo_root.join("frontend/admin/react"),
        }
    }
}

/// 一行清单：`<64 位十六进制>  <相对路径>`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub hash: String,
    pub path: String,
}

/// 解析清单文本（sha256sum 布局，路径可含空格）。
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 66
            || &bytes[64..66] != b"  "
            || !bytes[..64].iter().all(u8::is_ascii_hexdigit)
        {
            return Err(Error::BadManifestLine(line.to_owned()));
        }
        entries.push(Entry {
            hash: line[..64].to_owned(),
            path: line[66..].to_owned(),
        });
    }
    Ok(entries)
}

/// 校验结果：相对清单多出 / 缺失 / 改动的路径（均为排序后的相对路径）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

impl CheckReport {
    /// 树与清单是否完全一致。
    pub fn is_ok(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

/// 以当前树为基准校验清单（内容映射比较，对行序不敏感）。
pub fn check(tree: &Path, manifest_path: &Path, flavor: Flavor) -> Result<CheckReport> {
    let text = match fs::read_to_string(manifest_path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(Error::ManifestMissing(manifest_path.to_path_buf()));
        }
        Err(err) => return Err(err.into()),
    };
    let expected = entry_map(parse_manifest(&text)?);
    let actual = entry_map(parse_manifest(&build_manifest(tree, flavor)?)?);
    Ok(diff(&expected, &actual))
}

/// 以当前树为基准重建清单文件，返回清单行数。
pub fn rebuild(tree: &Path, manifest_path: &Path, flavor: Flavor) -> Result<usize> {
    let text = build_manifest(tree, flavor)?;
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(manifest_path, &text)?;
    Ok(text.lines().count())
}

/// 构建清单文本（不落盘）。
pub fn build_manifest(tree: &Path, flavor: Flavor) -> Result<String> {
    if !tree.is_dir() {
        return Err(Error::TreeMissing(tree.to_path_buf()));
    }
    let mut map = BTreeMap::new();
    match flavor {
        Flavor::Proto => collect_proto(tree, &mut map)?,
        Flavor::React => collect_react(tree, &mut map)?,
    }
    Ok(render(&map))
}

fn collect_proto(tree: &Path, map: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in WalkDir::new(tree) {
        let entry = entry.map_err(walk_err)?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().map_or(true, |ext| ext != "proto") {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        let rel = rel_path(tree, entry.path());
        map.insert(rel, sha256_hex(strip_bom(&bytes)));
    }
    Ok(())
}

/// react 面的目录剪枝（对齐 hash_tree 的 find 剪枝；git 忽略路径另由
/// `git check-ignore` 过滤——两道过滤语义不同，缺一不可）。
const REACT_PRUNED_DIRS: [&str; 2] = ["node_modules", "dist"];

fn collect_react(tree: &Path, map: &mut BTreeMap<String, String>) -> Result<()> {
    ensure_git_work_tree(tree)?;
    let mut candidates: Vec<String> = Vec::new();
    for entry in WalkDir::new(tree)
        .into_iter()
        .filter_entry(|e| !is_pruned(e))
    {
        let entry = entry.map_err(walk_err)?;
        if !entry.file_type().is_file() {
            continue;
        }
        candidates.push(rel_path(tree, entry.path()));
    }
    let ignored = git_ignored(tree, &candidates)?;
    for rel in candidates {
        if ignored.contains(&rel) {
            continue;
        }
        let bytes = fs::read(tree.join(&rel))?;
        map.insert(rel, sha256_hex(&bytes));
    }
    Ok(())
}

fn is_pruned(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && REACT_PRUNED_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
}

fn ensure_git_work_tree(tree: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(tree)
        .output()
        .map_err(|err| Error::GitCheckIgnore(format!("spawn git: {err}")))?;
    if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" {
        Ok(())
    } else {
        Err(Error::NotGitWorkTree(tree.to_path_buf()))
    }
}

/// 与脚本同款的 `git check-ignore --stdin -z` 过滤：输入候选路径，返回
/// 被忽略集合。退出码 1（无忽略项）与 0（有）都算正常，其余视为错误
/// （脚本层是 `|| true` 静默放行；这里选择硬失败，宁挂门不放水）。
fn git_ignored(tree: &Path, candidates: &[String]) -> Result<HashSet<String>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    let mut child = Command::new("git")
        .args(["check-ignore", "--stdin", "-z"])
        .current_dir(tree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| Error::GitCheckIgnore(format!("spawn git: {err}")))?;
    let mut stdin = child.stdin.take().expect("stdin 已声明为 piped");
    let paths = candidates.to_vec();
    let writer = std::thread::spawn(move || {
        for path in &paths {
            let _ = stdin.write_all(path.as_bytes());
            let _ = stdin.write_all(b"\0");
        }
    });
    let mut output = String::new();
    child
        .stdout
        .take()
        .expect("stdout 已声明为 piped")
        .read_to_string(&mut output)?;
    let status = child.wait()?;
    if writer.join().is_err() {
        return Err(Error::GitCheckIgnore("stdin 写入线程异常退出".to_owned()));
    }
    match status.code() {
        Some(0) | Some(1) => {}
        other => return Err(Error::GitCheckIgnore(format!("exit {other:?}"))),
    }
    Ok(output
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

fn diff(expected: &BTreeMap<String, String>, actual: &BTreeMap<String, String>) -> CheckReport {
    let mut report = CheckReport::default();
    for (path, hash) in actual {
        match expected.get(path) {
            None => report.added.push(path.clone()),
            Some(want) if want != hash => report.modified.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in expected.keys() {
        if !actual.contains_key(path) {
            report.removed.push(path.clone());
        }
    }
    report
}

fn entry_map(entries: Vec<Entry>) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .map(|entry| (entry.path, entry.hash))
        .collect()
}

fn render(map: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for (path, hash) in map {
        text.push_str(hash);
        text.push_str("  ");
        text.push_str(path);
        text.push('\n');
    }
    text
}

fn rel_path(tree: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(tree).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn walk_err(err: walkdir::Error) -> Error {
    Error::Io(io::Error::other(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256sum_layout_and_rejects_garbage() {
        let entries = parse_manifest("abc def\n").unwrap_err();
        assert!(matches!(entries, Error::BadManifestLine(_)));

        let ok = parse_manifest(&format!("{}  a/b.proto\n", "0".repeat(64))).unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].path, "a/b.proto");
    }

    #[test]
    fn flavor_paths_match_admin_layout() {
        let root = Path::new("/repo");
        assert_eq!(
            Flavor::Proto.manifest_path(root),
            PathBuf::from("/repo/backend/api/MANIFEST.sha256")
        );
        assert_eq!(
            Flavor::React.tree_path(root),
            PathBuf::from("/repo/frontend/admin/react")
        );
    }

    #[test]
    fn bom_is_stripped_before_hashing() {
        assert_eq!(strip_bom(b"\xEF\xBB\xBFhello"), b"hello");
        assert_eq!(strip_bom(b"hello"), b"hello");
    }
}
