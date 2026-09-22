//! rush-ui — RushWind 工具箱桌面壳。同进程直调 rush-gen 库：命令层是
//! `DTO 转换 + 库函数 + 错误串化`，没有 sidecar、没有 HTTP API。
//!
//! 字段经 DTO 传递（kind 存 `FieldKind::parse` 的合法输入串）——不让
//! 前端拼 serde 的 externally-tagged 枚举形状；非法 kind 在转换层报出
//! 可读错误。`probe_repo` 提供只读的仓库形状探测，供前端做选仓预检。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use rush_gen::adopt::{self, AdoptOptions};
use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use rush_gen::manifest::{self, CheckReport, Flavor};
use rush_gen::pages::{self, PagesOptions};
use rush_gen::project::{self, NewOptions, NewReport};

/// 库错误统一串化给前端（`{e:#}` 带完整 cause 链）。
type CmdResult<T> = Result<T, String>;

// ---- 字段 DTO：kind 用规格串（FieldKind::parse 的合法输入） ----

#[derive(Debug, Deserialize)]
struct FieldDto {
    name: String,
    kind: String,
}

fn convert_fields(fields: &[FieldDto]) -> Result<Vec<FieldSpec>, String> {
    fields
        .iter()
        .map(|field| {
            let kind = FieldKind::parse(&field.kind)
                .ok_or_else(|| format!("字段「{}」的类型非法：{}", field.name, field.kind))?;
            Ok(FieldSpec {
                name: field.name.clone(),
                kind,
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct EntityOptionsDto {
    repo_root: String,
    name: String,
    table: Option<String>,
    package: Option<String>,
    route_prefix: Option<String>,
    fields: Vec<FieldDto>,
    code_field: Option<String>,
    global: bool,
    check: bool,
    dry_run: bool,
    skip_manifest: bool,
}

impl EntityOptionsDto {
    fn into_core(self) -> Result<EntityOptions, String> {
        Ok(EntityOptions {
            repo_root: PathBuf::from(self.repo_root),
            name: self.name,
            table: self.table,
            package: self.package,
            route_prefix: self.route_prefix,
            fields: convert_fields(&self.fields)?,
            code_field: self.code_field,
            global: self.global,
            check: self.check,
            dry_run: self.dry_run,
            skip_manifest: self.skip_manifest,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PagesOptionsDto {
    repo_root: String,
    name: String,
    group: Option<String>,
    route_prefix: Option<String>,
    fields: Vec<FieldDto>,
    code_field: Option<String>,
    stack: rush_gen::pages::PagesStack,
    dry_run: bool,
}

impl PagesOptionsDto {
    fn into_core(self) -> Result<PagesOptions, String> {
        Ok(PagesOptions {
            repo_root: PathBuf::from(self.repo_root),
            name: self.name,
            group: self.group,
            route_prefix: self.route_prefix,
            fields: convert_fields(&self.fields)?,
            code_field: self.code_field,
            stack: self.stack,
            dry_run: self.dry_run,
        })
    }
}

// ---- 命令 ----

#[tauri::command]
fn gen_entity(opts: EntityOptionsDto) -> CmdResult<entity::EntityReport> {
    entity::generate_entity(&opts.into_core()?).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn gen_pages(opts: PagesOptionsDto) -> CmdResult<pages::PagesReport> {
    pages::generate_pages(&opts.into_core()?).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn new_project(opts: NewOptions) -> CmdResult<NewReport> {
    project::new_project(&opts).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn adopt(opts: AdoptOptions) -> CmdResult<adopt::AdoptReport> {
    adopt::adopt(&opts).map_err(|e| format!("{e:#}"))
}

/// manifest 校验：路径拼装与 CLI 同款（清单在 `<repo>/backend/api/` 与
/// `<repo>/frontend/admin/` 下）。
#[tauri::command]
fn manifest_check(repo: String, flavor: Flavor) -> CmdResult<CheckReport> {
    let repo = PathBuf::from(repo);
    let tree = flavor.tree_path(&repo);
    let path = flavor.manifest_path(&repo);
    manifest::check(&tree, &path, flavor).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn manifest_rebuild(repo: String, flavor: Flavor) -> CmdResult<usize> {
    let repo = PathBuf::from(repo);
    let tree = flavor.tree_path(&repo);
    let path = flavor.manifest_path(&repo);
    manifest::rebuild(&tree, &path, flavor).map_err(|e| format!("{e:#}"))
}

// ---- 仓库形状探测（只读，选仓预检） ----

#[derive(Debug, Default, Serialize)]
struct RepoProbe {
    repo_exists: bool,
    /// react 前端在位（frontend/admin/react/package.json）。
    react: bool,
    /// vue-vben 在位（frontend/admin/vue-vben/apps/admin/package.json）。
    vben: bool,
    /// vue-element 在位（frontend/admin/vue-element/package.json）。
    element: bool,
    /// 后端 seed.rs 在位（react 栈菜单种子的落点）。
    seed_rs: bool,
    /// 后端实体注册面在位（gen entity 的锚点文件抽查）。
    backend: bool,
    /// `.rush/` 下的实体规格名（gen entity 已落盘、gen pages 可直接继承）。
    specs: Vec<String>,
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

#[tauri::command]
fn probe_repo(repo: String) -> RepoProbe {
    let root = Path::new(&repo);
    let mut probe = RepoProbe {
        repo_exists: root.is_dir(),
        ..Default::default()
    };
    if !probe.repo_exists {
        return probe;
    }
    let react = root.join("frontend/admin/react");
    probe.react = is_file(&react.join("package.json"));
    probe.vben = is_file(&root.join("frontend/admin/vue-vben/apps/admin/package.json"));
    probe.element = is_file(&root.join("frontend/admin/vue-element/package.json"));
    probe.seed_rs = is_file(&root.join("backend/services/admin-api/src/seed.rs"));
    probe.backend = is_file(&root.join("backend/services/admin-api/src/data.rs"))
        && is_file(&root.join("backend/services/admin-api/src/server/rest.rs"));
    let spec_dir = root.join(".rush");
    if let Ok(entries) = std::fs::read_dir(&spec_dir) {
        let mut names: Vec<String> = entries
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        names.sort();
        probe.specs = names;
    }
    probe
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            gen_entity,
            gen_pages,
            new_project,
            adopt,
            manifest_check,
            manifest_rebuild,
            probe_repo
        ])
        .run(tauri::generate_context!())
        .expect("rush-ui 启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_dto_kinds_parse_to_core_specs() {
        let fields = vec![
            FieldDto {
                name: "code".into(),
                kind: "string".into(),
            },
            FieldDto {
                name: "state".into(),
                kind: "enum(0=OFF,1=ON@default=ON)".into(),
            },
            FieldDto {
                name: "quantity".into(),
                kind: "u32".into(),
            },
        ];
        let specs = convert_fields(&fields).unwrap();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].kind, FieldKind::String);
        assert_eq!(
            specs[1].kind,
            FieldKind::parse("enum(0=OFF,1=ON@default=ON)").unwrap()
        );
    }

    #[test]
    fn bad_field_kind_errors_in_chinese() {
        let fields = vec![FieldDto {
            name: "state".into(),
            kind: "enum(1=ON)".into(),
        }];
        let err = convert_fields(&fields).unwrap_err();
        assert!(err.contains("类型非法"), "{err}");
        assert!(err.contains("state"), "{err}");
    }
}
