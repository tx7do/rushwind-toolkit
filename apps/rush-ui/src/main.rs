//! rush-ui — RushWind 工具箱桌面壳。同进程直调 rush-gen 库：命令层只是
//! `库函数 + 错误串化`，没有 sidecar、没有 HTTP API；序列化形状与
//! rush-gen 的 Options/Report 结构体一一对应（serde derives 见各模块）。
//!
//! MVP 命令面：gen entity / gen pages / new / adopt / manifest 两个动作。
//! testbed run 涉及长进程与 docker 差分栈，暂不进 UI。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rush_gen::adopt::{self, AdoptOptions};
use rush_gen::entity::{self, EntityOptions};
use rush_gen::manifest::{self, CheckReport, Flavor};
use rush_gen::pages::{self, PagesOptions};
use rush_gen::project::{self, NewOptions, NewReport};

/// 库错误统一串化给前端（`{e:#}` 带完整 cause 链）。
type CmdResult<T> = Result<T, String>;

#[tauri::command]
fn gen_entity(opts: EntityOptions) -> CmdResult<entity::EntityReport> {
    entity::generate_entity(&opts).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn gen_pages(opts: PagesOptions) -> CmdResult<pages::PagesReport> {
    pages::generate_pages(&opts).map_err(|e| format!("{e:#}"))
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
    let repo = std::path::PathBuf::from(repo);
    let tree = flavor.tree_path(&repo);
    let path = flavor.manifest_path(&repo);
    manifest::check(&tree, &path, flavor).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn manifest_rebuild(repo: String, flavor: Flavor) -> CmdResult<usize> {
    let repo = std::path::PathBuf::from(repo);
    let tree = flavor.tree_path(&repo);
    let path = flavor.manifest_path(&repo);
    manifest::rebuild(&tree, &path, flavor).map_err(|e| format!("{e:#}"))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            gen_entity,
            gen_pages,
            new_project,
            adopt,
            manifest_check,
            manifest_rebuild
        ])
        .run(tauri::generate_context!())
        .expect("rush-ui 启动失败");
}
