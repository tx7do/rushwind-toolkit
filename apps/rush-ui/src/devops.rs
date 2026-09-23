//! 开发工具：`backend/services/*` 一览与 cargo 子进程托管。
//! `dev run` 起的是长驻进程，日志经 `dev-log` 事件逐行推给前端；
//! 监视线程轮询 try_wait，自然退出或被 stop 摘除后发 `dev-exit`；
//! 应用退出时统一 kill，避免孤儿 cargo 进程。

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use rush_gen::project::{self, NewOptions, StorageKind};

use crate::{AppState, CommandResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub name: String,
    /// src/main.rs 在位（可运行）。
    pub has_server: bool,
    /// 配置目录在位（rushwind-admin 用 assets/，兼容 config/）。
    pub has_config: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevLog {
    name: String,
    line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevExit {
    name: String,
    code: Option<i32>,
}

pub(crate) type Running = Arc<Mutex<HashMap<String, Child>>>;

pub(crate) fn project_root(state: &AppState) -> Option<PathBuf> {
    state
        .project
        .lock()
        .expect("project 锁中毒")
        .as_ref()
        .map(|p| PathBuf::from(&p.root))
}

/// cargo 命令的工作目录：rushwind-admin 的 workspace 在 `backend/` 下。
fn cargo_dir(root: &Path) -> PathBuf {
    crate::detect::cargo_workspace(root)
}

#[tauri::command]
pub fn get_dev_services(state: State<AppState>) -> Vec<ServiceInfo> {
    let Some(root) = project_root(&state) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("backend/services"))
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    entry.file_type().ok()?.is_dir().then(|| entry.path())
                })
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| ServiceInfo {
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            has_server: dir.join("src/main.rs").is_file(),
            has_config: dir.join("assets").is_dir() || dir.join("config").is_dir(),
        })
        .collect()
}

fn pipe_reader<R: std::io::Read + Send + 'static>(reader: R, app: AppHandle, name: String) {
    std::thread::spawn(move || {
        let mut buf = std::io::BufReader::new(reader);
        let mut line = String::new();
        while let Ok(n) = buf.read_line(&mut line) {
            if n == 0 {
                break;
            }
            let text = line.trim_end().to_string();
            if !text.is_empty() {
                let _ = app.emit("dev-log", &DevLog { name: name.clone(), line: text });
            }
            line.clear();
        }
    });
}

/// 轮询子进程退出。条目被 stop 摘除时静默退出（dev-exit 由 stop 发），
/// 自然退出时负责从 map 移除并发事件，两条路径不重复。
enum Watch {
    Waiting,
    Gone,
    Exited(Option<i32>),
}

fn watch_exit(app: AppHandle, running: Running, name: String) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let watch = {
            let mut guard = running.lock().expect("running 锁中毒");
            let exited = guard
                .get_mut(&name)
                .and_then(|child| child.try_wait().ok())
                .flatten();
            match exited {
                Some(status) => {
                    let code = status.code();
                    guard.remove(&name);
                    Watch::Exited(code)
                }
                None if guard.contains_key(&name) => Watch::Waiting,
                None => Watch::Gone,
            }
        };
        match watch {
            Watch::Waiting => continue,
            Watch::Gone => break,
            Watch::Exited(code) => {
                let _ = app.emit("dev-exit", &DevExit { name, code });
                break;
            }
        }
    });
}

#[tauri::command]
pub fn dev_run_service(app: AppHandle, state: State<AppState>, name: String) -> CommandResult {
    let name = name.trim().to_string();
    if name.is_empty() {
        return CommandResult::fail("服务名为空");
    }
    let Some(root) = project_root(&state) else {
        return CommandResult::fail("尚未打开项目");
    };
    let running = state.running.clone();
    if running.lock().expect("running 锁中毒").contains_key(&name) {
        return CommandResult::ok(format!("服务 {name} 已在运行"));
    }
    let mut child = match Command::new("cargo")
        .args(["run", "-p", &name])
        .current_dir(cargo_dir(&root))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return CommandResult::fail(format!("启动 cargo run -p {name} 失败：{e}")),
    };
    if let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) {
        pipe_reader(stdout, app.clone(), name.clone());
        pipe_reader(stderr, app.clone(), name.clone());
    }
    running
        .lock()
        .expect("running 锁中毒")
        .insert(name.clone(), child);
    watch_exit(app, running, name.clone());
    CommandResult::ok(format!("cargo run -p {name} 已启动（目录 {}）", root.display()))
}

/// stop 先原子摘出再 kill+wait：监视线程看到条目消失即退出，
/// dev-exit 由本命令发，不会双发。
#[tauri::command]
pub fn dev_stop_service(app: AppHandle, state: State<AppState>, name: String) -> CommandResult {
    let taken = {
        let running = state.running.clone();
        let mut running = running.lock().expect("running 锁中毒");
        running.remove(&name)
    };
    let Some(mut child) = taken else {
        return CommandResult::fail(format!("服务 {name} 未在运行"));
    };
    let _ = child.kill();
    let code = child.wait().ok().and_then(|status| status.code());
    let _ = app.emit("dev-exit", &DevExit { name: name.clone(), code });
    CommandResult::ok(format!("服务 {name} 已停止"))
}

/// 退出时兜底 kill 全部托管子进程。
pub(crate) fn kill_all(running: &Running) {
    if let Ok(mut running) = running.lock() {
        for (_, mut child) in running.drain() {
            let _ = child.kill();
        }
    }
}

#[tauri::command]
pub fn dev_cargo_check(state: State<AppState>, scope: String) -> CommandResult {
    let Some(root) = project_root(&state) else {
        return CommandResult::fail("尚未打开项目");
    };
    let scope = scope.trim().to_string();
    let mut cargo = Command::new("cargo");
    if scope.is_empty() {
        cargo.args(["check", "--workspace"]);
    } else {
        cargo.args(["check", "-p", &scope]);
    }
    match cargo.current_dir(cargo_dir(&root)).output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            CommandResult {
                success: output.status.success(),
                output: text,
                error: None,
                dir: None,
            }
        }
        Err(e) => CommandResult::fail(format!("执行 cargo check 失败：{e}")),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddServiceOptions {
    pub service_name: String,
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub db_clients: Vec<String>,
}

/// 从勾选的 dbClients 推断内嵌模板的存储变体：RushWind 用一份 YAML 组装
/// 存储引擎，这里只区分「带真 SQL 面的 SeaORM（sqlite/postgres）」与「内存」。
/// 优先 postgres，其次 sqlite，都无则回落到开箱即跑的 memory。
fn storage_from_db_clients(db_clients: &[String]) -> StorageKind {
    let has = |needle: &str| {
        db_clients
            .iter()
            .any(|c| c.to_lowercase().contains(needle))
    };
    if has("postgres") {
        StorageKind::Postgres
    } else if has("sqlite") {
        StorageKind::Sqlite
    } else {
        StorageKind::Memory
    }
}

/// 向已打开项目添加新服务：复用 `rush new` 的内嵌模板，落到
/// `backend/services/<name>/`。rushwind-admin 的 workspace 用
/// `members = ["services/*"]` 通配，新目录会自动成为成员，无需改根
/// Cargo.toml。服务自带 `src/main.rs`，因此 `cargo run -p <name>` 即可跑。
#[tauri::command]
pub fn add_service(state: State<AppState>, opts: AddServiceOptions) -> CommandResult {
    let Some(root) = project_root(&state) else {
        return CommandResult::fail("尚未打开项目，请先在顶栏选择一个 RushWind 项目");
    };
    let name = opts.service_name.trim().to_string();
    if name.is_empty() {
        return CommandResult::fail("服务名不能为空");
    }
    let services_dir = root.join("backend/services");
    if !services_dir.is_dir() {
        return CommandResult::fail(format!(
            "没有找到服务目录：{}（请确认打开的是 RushWind 项目根）",
            services_dir.display()
        ));
    }
    let target = services_dir.join(&name);
    if target.exists() {
        return CommandResult::fail(format!("服务目录已存在：{}", target.display()));
    }

    let new_opts = NewOptions {
        name: name.clone(),
        dest: services_dir,
        storage: storage_from_db_clients(&opts.db_clients),
        template: None,
        git: false,
        dry_run: false,
    };
    match project::new_project(&new_opts) {
        Ok(report) => {
            let mut output = format!(
                "已生成服务 {name}（{} 个文件，模板 {}）\n",
                report.files.len(),
                report.template_source
            );
            // servers/dbClients 在 RushWind 里只是提示：组装由服务自己的 YAML 决定。
            if !opts.servers.is_empty() || !opts.db_clients.is_empty() {
                output.push_str(&format!(
                    "勾选的 server/dbClient：{} / {}（RushWind 由服务内的 YAML 组装，可在 backend/services/{name}/src/main.rs 的 CONFIG 里调整）\n",
                    if opts.servers.is_empty() { "—".to_string() } else { opts.servers.join(", ") },
                    if opts.db_clients.is_empty() { "—".to_string() } else { opts.db_clients.join(", ") },
                ));
            }
            output.push_str(&format!(
                "workspace 通过 services/* 通配自动纳入，可直接 cargo run -p {name}。"
            ));
            CommandResult {
                success: true,
                output,
                error: None,
                dir: Some(report.project_dir.to_string_lossy().into_owned()),
            }
        }
        Err(e) => CommandResult::fail(format!("服务脚手架失败：{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_clients_pick_the_storage_variant() {
        assert_eq!(storage_from_db_clients(&[]), StorageKind::Memory);
        assert_eq!(storage_from_db_clients(&["sqlx".into()]), StorageKind::Memory);
        assert_eq!(storage_from_db_clients(&["SeaORM".into()]), StorageKind::Memory);
        assert_eq!(
            storage_from_db_clients(&["sqlite".into(), "redis".into()]),
            StorageKind::Sqlite
        );
        // postgres 优先：勾选了 postgres 就走真库模板。
        assert_eq!(
            storage_from_db_clients(&["sqlite".into(), "postgres".into()]),
            StorageKind::Postgres
        );
    }
}
