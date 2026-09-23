//! 剩余占位面：SQL/DDL 导入的真实入口，以及 Go schema 导入的占位。
//! 约定与 Go 版一致——导入命令返回 `String`，`''` 表示成功，非空为中文错误，
//! 前端直接 `message.error` 展示。配置中心导出在 `configexport`、数据库直连在
//! `database`、SQL 解析在 `sqlimport`、AI 助手在 `ai`。

use tauri::{AppHandle, Emitter, State};

use crate::AppState;

/// MySQL 风格 DDL → 实体行。整表替换当前选项（新数据源语义），
/// 与 Go 版 `ImportSqlTables` 一样返回 `''` 表示成功。
#[tauri::command]
pub fn import_sql_tables(app: AppHandle, state: State<AppState>, sql: String) -> String {
    let tables = crate::sqlimport::parse_ddl(&sql);
    if tables.is_empty() {
        return "没有解析到任何 CREATE TABLE（仅支持 MySQL 风格 DDL）".to_string();
    }
    {
        let mut options = state.options.lock().expect("options 锁中毒");
        options.clear();
        for table in &tables {
            let id = crate::next_id(&options);
            options.push(crate::sqlimport::to_option(id, table));
        }
    }
    let _ = app.emit("table-imported", ());
    String::new()
}

#[tauri::command]
pub fn import_go_schema_tables(_dir: String, _service: String) -> String {
    "占位：RushWind 无 Go schema 导入面（.rush 实体规格请用「实体规格」导入）".to_string()
}
