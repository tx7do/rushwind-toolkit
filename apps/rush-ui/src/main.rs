//! rush-ui — RushWind 工具箱桌面壳。同进程直调 rush-gen 库：命令层是
//! `DTO 转换 + 库函数 + 错误串化`，没有 sidecar、没有 HTTP API。
//!
//! 字段经 DTO 传递（kind 存 `FieldKind::parse` 的合法输入串）——不让
//! 前端拼 serde 的 externally-tagged 枚举形状；非法 kind 在转换层报出
//! 可读错误。UI 复刻自 gowind-uiapp（Wails/Go），命令名与
//! `frontend/src/bridge/App.ts` 一一对应：项目缓存、生成器选项表与
//! cargo 子进程是 RushWind 的真实面（`detect`/`gencode`/`devops`），
//! 数据库/配置中心/AI 是占位面（`placeholders`），错误串保持中文。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod configexport;
mod database;
mod detect;
mod devops;
mod gencode;
mod placeholders;
mod sqlimport;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use rush_gen::adopt::{self, AdoptOptions};
use rush_gen::doctor;
use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use rush_gen::project::{self, NewOptions, NewReport, StorageKind};
use rush_gen::spec::{self, EntitySpecFile, SpecField};
use rush_gen::undo;
use rush_gen::manifest::{self, CheckReport, Flavor};
use rush_gen::pages::{self, PagesOptions, PagesReport};

use ai::AIConfig;
use database::DBConfig;
use detect::{OpenProjectResult, ProjectInfo};
use devops::Running;

/// 库错误统一串化给前端（`{e:#}` 带完整 cause 链）。
pub type CmdResult<T> = Result<T, String>;

/// 与 Go 版 `main.CommandResult` 同形的通用执行结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub dir: Option<String>,
}

impl CommandResult {
    pub(crate) fn ok(output: impl Into<String>) -> Self {
        CommandResult { success: true, output: output.into(), error: None, dir: None }
    }
    pub(crate) fn fail(error: impl Into<String>) -> Self {
        CommandResult {
            success: false,
            output: String::new(),
            error: Some(error.into()),
            dir: None,
        }
    }
}

/// 阻塞任务（HTTP/驱动调用）统一落工作线程；闭包 panic 时返回 Default
/// 而不是炸掉 IPC 线程。
pub(crate) async fn blocking_io<T, F>(f: F) -> T
where
    T: Default + Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(value) => value,
        Err(e) => {
            eprintln!("阻塞任务执行失败：{e}");
            T::default()
        }
    }
}

// ==================== 托管状态 ====================

pub struct AppState {
    /// 当前项目（open_project 缓存；换项目即重置）。
    pub project: Mutex<Option<ProjectInfo>>,
    /// 后端生成页的实体选项表。
    pub options: Mutex<Vec<GeneratorOption>>,
    pub db_config: Mutex<DBConfig>,
    pub ai_config: Mutex<AIConfig>,
    /// dev run 托管的 cargo 子进程。
    pub running: Running,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            project: Mutex::new(None),
            options: Mutex::new(Vec::new()),
            db_config: Mutex::new(DBConfig::default()),
            ai_config: Mutex::new(AIConfig::default()),
            running: Running::default(),
        }
    }
}

// ==================== 生成器选项 ====================

/// 规格字段（与 `.rush/*.json` 的 fields 同形）。
pub type SpecFieldDto = SpecField;

/// 一行 = 一条实体链。camelCase 序列化对齐 gowind-uiapp 的
/// `generator.Option`，rushwind 差异化字段（table/routePrefix/…）可选。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneratorOption {
    pub id: i64,
    /// 实体名（snake_case 单数）；沿用 Go 版的 tableName 字段名。
    pub table_name: String,
    pub service: String,
    pub exclude: bool,
    pub proto_package: String,
    // ---- rushwind 差异化字段 ----
    pub table: String,
    pub route_prefix: String,
    pub fields: Vec<SpecFieldDto>,
    pub code_field: String,
    pub global: bool,
    pub auth_free: bool,
}

impl GeneratorOption {
    fn from_spec(id: i64, file: &EntitySpecFile) -> Self {
        GeneratorOption {
            id,
            table_name: file.name.clone(),
            service: String::new(),
            exclude: false,
            proto_package: file.package.clone(),
            table: file.table.clone(),
            route_prefix: file.route_prefix.clone(),
            fields: file.fields.clone(),
            code_field: file.code_field.clone().unwrap_or_default(),
            global: file.global,
            auth_free: false,
        }
    }
}

fn next_id(options: &[GeneratorOption]) -> i64 {
    options.iter().map(|option| option.id).max().unwrap_or(0) + 1
}

#[tauri::command]
fn get_generator_options(state: State<AppState>) -> Vec<GeneratorOption> {
    state.options.lock().expect("options 锁中毒").clone()
}

#[tauri::command]
fn set_generator_option(state: State<AppState>, options: Vec<GeneratorOption>) {
    *state.options.lock().expect("options 锁中毒") = options;
}

#[tauri::command]
fn edit_generator_option(state: State<AppState>, option: GeneratorOption) {
    let mut options = state.options.lock().expect("options 锁中毒");
    match options.iter_mut().find(|row| row.id == option.id) {
        Some(row) => *row = option,
        None => options.push(option),
    }
}

/// 从 `.rush/<name>.json` 规格导入实体行（RushWind 的「数据源」）。
/// 返回串沿用 Go 版约定：`''` 成功，非空为聚合错误。
#[tauri::command]
fn import_spec_tables(app: AppHandle, state: State<AppState>, names: Vec<String>) -> String {
    let Some(root) = devops::project_root(&state) else {
        return "尚未打开项目".to_string();
    };
    let mut errors: Vec<String> = Vec::new();
    {
        let mut options = state.options.lock().expect("options 锁中毒");
        for name in &names {
            match spec::load(&root, name) {
                Ok(file) => {
                    if let Some(row) =
                        options.iter_mut().find(|row| row.table_name == file.name)
                    {
                        *row = GeneratorOption { id: row.id, ..GeneratorOption::from_spec(0, &file) };
                    } else {
                        let id = next_id(&options);
                        options.push(GeneratorOption::from_spec(id, &file));
                    }
                }
                Err(e) => errors.push(format!("{name}: {e:#}")),
            }
        }
    }
    if errors.len() < names.len() {
        let _ = app.emit("table-imported", ());
    }
    errors.join("\n")
}

// ==================== 字段 DTO ====================

#[derive(Debug, Deserialize)]
pub struct FieldDto {
    pub name: String,
    pub kind: String,
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
pub struct EntityOptionsDto {
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
    overwrite: bool,
    auth_free: bool,
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
            overwrite: self.overwrite,
            auth_free: self.auth_free,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PagesOptionsDto {
    repo_root: String,
    name: String,
    group: Option<String>,
    route_prefix: Option<String>,
    fields: Vec<FieldDto>,
    code_field: Option<String>,
    stack: pages::PagesStack,
    global: Option<bool>,
    overwrite: bool,
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
            global: self.global,
            overwrite: self.overwrite,
            dry_run: self.dry_run,
        })
    }
}

// ==================== 项目命令 ====================

#[tauri::command]
fn open_project(app: AppHandle, state: State<AppState>, path: String) -> CmdResult<OpenProjectResult> {
    let root = PathBuf::from(path.trim());
    if root.as_os_str().is_empty() || !root.is_dir() {
        return Err(format!("目录不存在：{}", root.display()));
    }
    if let Some(info) = detect::detect_project(&root) {
        {
            let mut project = state.project.lock().expect("project 锁中毒");
            *project = Some(info.clone());
            // 换项目：上一个项目的选项表必须作废。
            state.options.lock().expect("options 锁中毒").clear();
        }
        let _ = app.emit("project-opened", &info);
        return Ok(OpenProjectResult {
            status: "opened".to_string(),
            project: Some(info),
            candidates: None,
        });
    }
    let candidates = detect::find_candidates(&root);
    if !candidates.is_empty() {
        return Ok(OpenProjectResult {
            status: "choose".to_string(),
            project: None,
            candidates: Some(candidates),
        });
    }
    Err(format!("不是 RushWind 项目根（未找到 Cargo.toml）：{}", root.display()))
}

#[tauri::command]
fn get_project_info(state: State<AppState>) -> Option<ProjectInfo> {
    state.project.lock().expect("project 锁中毒").clone()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateProjectOptions {
    pub name: String,
    #[allow(dead_code)]
    pub module: String,
    #[allow(dead_code)]
    pub repo_url: String,
    #[allow(dead_code)]
    pub branch: String,
    pub parent_dir: String,
    /// Memory / Sqlite / Postgres（缺省 Memory）。
    pub storage: Option<String>,
}

#[tauri::command]
fn create_project(opts: CreateProjectOptions) -> CommandResult {
    let name = opts.name.trim().to_string();
    if name.is_empty() {
        return CommandResult::fail("项目名不能为空");
    }
    if opts.parent_dir.trim().is_empty() {
        return CommandResult::fail("请选择父目录");
    }
    let storage = match opts.storage.as_deref() {
        Some("Sqlite") => StorageKind::Sqlite,
        Some("Postgres") => StorageKind::Postgres,
        _ => StorageKind::Memory,
    };
    let new_opts = NewOptions {
        name,
        dest: PathBuf::from(opts.parent_dir.trim()),
        storage,
        template: None,
        git: true,
        dry_run: false,
    };
    match project::new_project(&new_opts) {
        Ok(report) => CommandResult {
            success: true,
            output: format!(
                "已在 {} 生成 {} 个文件",
                report.project_dir.display(),
                report.files.len()
            ),
            error: None,
            dir: Some(report.project_dir.to_string_lossy().into_owned()),
        },
        Err(e) => CommandResult::fail(format!("{e:#}")),
    }
}

/// 清场：项目缓存、选项表与运行中的服务全部丢弃（Go 版 CleanConfig
/// 的对应面；前端 store 监听 config-cleaned 事件重置全局项目状态）。
#[tauri::command]
fn clean_config(app: AppHandle, state: State<AppState>) {
    {
        let mut project = state.project.lock().expect("project 锁中毒");
        *project = None;
        state.options.lock().expect("options 锁中毒").clear();
    }
    devops::kill_all(&state.running);
    let _ = app.emit("config-cleaned", ());
}

// ==================== 生成命令（单实体，直连 rush-gen） ====================

#[tauri::command]
fn gen_entity(opts: EntityOptionsDto) -> CmdResult<entity::EntityReport> {
    entity::generate_entity(&opts.into_core()?).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn gen_pages(opts: PagesOptionsDto) -> CmdResult<PagesReport> {
    pages::generate_pages(&opts.into_core()?).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn spec_load(repo: String, name: String) -> CmdResult<EntitySpecFile> {
    spec::load(Path::new(&repo), &name).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn spec_save(repo: String, spec: EntitySpecFile) -> CmdResult<()> {
    spec::save(&spec, Path::new(&repo)).map(|_| ()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn gen_undo(opts: undo::UndoOptions) -> CmdResult<undo::UndoReport> {
    undo::undo_entity(&opts).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn doctor(repo: Option<String>) -> doctor::DoctorReport {
    doctor::run_doctor(repo.as_deref().map(Path::new))
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

// ==================== 仓库形状探测（只读，选仓预检） ====================

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
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
    probe.specs = detect::spec_names(root);
    probe
}

// ==================== 入口 ====================

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            get_project_info,
            create_project,
            clean_config,
            probe_repo,
            get_generator_options,
            set_generator_option,
            edit_generator_option,
            import_spec_tables,
            gen_entity,
            gen_pages,
            spec_load,
            spec_save,
            gen_undo,
            doctor,
            new_project,
            adopt,
            manifest_check,
            manifest_rebuild,
            gencode::generate_grpc_code,
            gencode::generate_rest_code,
            devops::get_dev_services,
            devops::dev_run_service,
            devops::dev_stop_service,
            devops::dev_cargo_check,
            devops::add_service,
            database::get_db_config,
            database::set_db_config,
            database::test_database_connection,
            database::get_database_tables,
            database::get_table_columns,
            database::import_database_tables,
            placeholders::import_sql_tables,
            placeholders::import_go_schema_tables,
            configexport::get_remote_config_types,
            configexport::get_config_services,
            configexport::export_config_to_remote,
            configexport::export_one_service_config,
            ai::get_ai_config,
            ai::set_ai_config,
            ai::get_ai_provider_presets,
            ai::test_ai_connection,
            ai::ai_generate_ddl,
            ai::ai_generate_ddl_stream,
            ai::ai_partition_microservices,
            ai::ai_generate_backend_code,
            ai::ai_find_openapi_files,
            ai::ai_review_code,
            ai::ai_review_code_stream
        ])
        .build(tauri::generate_context!())
        .expect("rush-ui 启动失败")
        .run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = handle.try_state::<AppState>() {
                    devops::kill_all(&state.running);
                }
            }
        });
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
