//! 项目形状探测（RushWind 仓库）：Cargo.toml 在工作区根，服务在
//! `backend/services/*`，前端栈在 `frontend/admin/{react,vue-vben,vue-element}`，
//! 实体规格在 `.rush/*.json`。字段序列化保持 PascalCase，与 Go 版
//! gowind-uiapp 的 `detect.ProjectInfo` 对齐，前端页面可原样复刻。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProjectInfo {
    pub root: String,
    /// workspace/包名（Go 版为 go module 路径）。
    pub mod_path: String,
    pub version: String,
    pub services: Vec<String>,
    pub has_api: bool,
    pub has_react: bool,
    pub has_vben: bool,
    pub has_element: bool,
    pub specs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModuleCandidate {
    pub dir: String,
    pub mod_path: String,
    pub rel_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct OpenProjectResult {
    /// `opened`：已识别为项目根；`choose`：目录下有候选子模块待用户确认。
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<ModuleCandidate>>,
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

/// 极简 Cargo.toml 扫描：`[workspace.package]` 优先，退回 `[package]`。
/// 只为取 name/version 两个展示字段，不引 toml 依赖、不校验语法。
pub fn cargo_name_version(text: &str) -> (String, String) {
    let mut ws: (Option<String>, Option<String>) = (None, None);
    let mut pkg: (Option<String>, Option<String>) = (None, None);
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key != "name" && key != "version" {
            continue;
        }
        let value = value.split('#').next().unwrap_or("").trim().trim_matches('"');
        let slot = match section.as_str() {
            "workspace.package" => &mut ws,
            "package" => &mut pkg,
            _ => continue,
        };
        match key {
            "name" if slot.0.is_none() => slot.0 = Some(value.to_string()),
            "version" if slot.1.is_none() => slot.1 = Some(value.to_string()),
            _ => {}
        }
    }
    (
        ws.0.or(pkg.0).unwrap_or_default(),
        ws.1.or(pkg.1).unwrap_or_else(|| "0.0.0".to_string()),
    )
}

pub fn spec_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join(".rush"))
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let name = entry.ok()?.file_name().into_string().ok()?;
                    name.strip_suffix(".json").map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn service_dirs(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("backend/services"))
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    entry.path().is_dir().then(|| entry.file_name())?.into_string().ok()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// 目录是项目根 ⇔ 根上有 Cargo.toml（toolkit 形状）或
/// `backend/Cargo.toml` 在位（rushwind-admin 形状：workspace 在 backend/ 下）。
pub fn detect_project(root: &Path) -> Option<ProjectInfo> {
    let cargo = manifest_for_root(root)?;
    let text = fs::read_to_string(&cargo).unwrap_or_default();
    let (mod_path, version) = cargo_name_version(&text);
    Some(ProjectInfo {
        root: root.to_string_lossy().into_owned(),
        mod_path,
        version,
        services: service_dirs(root),
        has_api: root.join("backend/api").is_dir(),
        has_react: is_file(&root.join("frontend/admin/react/package.json")),
        has_vben: is_file(&root.join("frontend/admin/vue-vben/apps/admin/package.json")),
        has_element: is_file(&root.join("frontend/admin/vue-element/package.json")),
        specs: spec_names(root),
    })
}

/// 根级清单：优先根上的 Cargo.toml，退回 `backend/Cargo.toml`。
pub fn manifest_for_root(root: &Path) -> Option<PathBuf> {
    let direct = root.join("Cargo.toml");
    if is_file(&direct) {
        return Some(direct);
    }
    let backend = root.join("backend/Cargo.toml");
    is_file(&backend).then_some(backend)
}

/// cargo 命令的工作目录：admin 形状仓库的 workspace 在 `backend/` 下。
pub fn cargo_workspace(root: &Path) -> PathBuf {
    let backend = root.join("backend");
    if is_file(&backend.join("Cargo.toml")) {
        backend
    } else {
        root.to_path_buf()
    }
}

/// 一级子目录里的 Cargo 包（Go 版 go.mod 候选的同款交互）。
pub fn find_candidates(root: &Path) -> Vec<ModuleCandidate> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<ModuleCandidate> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let dir = entry.path();
            if !dir.is_dir() {
                return None;
            }
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                return None;
            }
            let cargo = dir.join("Cargo.toml");
            if !is_file(&cargo) {
                return None;
            }
            let text = fs::read_to_string(&cargo).unwrap_or_default();
            let (mod_path, _) = cargo_name_version(&text);
            Some(ModuleCandidate {
                dir: dir.to_string_lossy().into_owned(),
                mod_path,
                rel_path: name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_package_wins_over_package() {
        let text = "[workspace]\nmembers = [\"apps/demo\"]\n\n[workspace.package]\nname = \"rushwind-admin\"\nversion = \"0.2.1\"\n";
        assert_eq!(
            cargo_name_version(text),
            ("rushwind-admin".to_string(), "0.2.1".to_string())
        );
    }

    #[test]
    fn falls_back_to_package_and_skips_inline_comments() {
        let text = "[package]\nname = \"demo\"  # crate 名\nversion = \"1.0.0\"\n";
        assert_eq!(
            cargo_name_version(text),
            ("demo".to_string(), "1.0.0".to_string())
        );
    }
}
