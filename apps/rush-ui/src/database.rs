//! 数据库直连面：DSN/表单 → MySQL / PostgreSQL / SQLite 元数据 introspection。
//! 约定与 Go 版一致：test 返回 `ConnectionResult`（message 给前端 toast），
//! import 返回 String（`''` 成功），tables/columns 返回 `CmdResult<Vec<_>>`。
//!
//! 阻塞的驱动调用全部经 `spawn_blocking` 落到工作线程，UI 线程不冻结。
//! 类型映射复用 `sqlimport` 的闭集规则（FieldKind 只有 6 种），列导入与
//! DDL 导入共享同一条「原始类型 → kind」通路。

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::sqlimport::{self, SqlTable};
use crate::{next_id, AppState, SpecFieldDto};

const CONNECT_TIMEOUT: u64 = 10;

// ==================== 类型 ====================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DBConfig {
    #[serde(rename = "type")]
    pub db_type: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl: bool,
    pub db_path: String,
    pub use_dsn: Option<bool>,
    pub dsn: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResult {
    pub success: bool,
    pub message: String,
    pub database: String,
    pub server_ver: String,
    pub duration: i64,
    pub tables: i64,
    pub connected: bool,
    pub error: Option<String>,
}

impl ConnectionResult {
    fn ok(database: String, server_ver: String, tables: i64, duration: i64) -> Self {
        ConnectionResult {
            success: true,
            message: "数据库连接成功".to_string(),
            database,
            server_ver,
            duration,
            tables,
            connected: true,
            error: None,
        }
    }

    fn err(message: impl Into<String>) -> Self {
        let message = message.into();
        ConnectionResult {
            success: false,
            error: Some(message.clone()),
            message,
            ..Default::default()
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TableInfo {
    pub table_name: String,
    pub table_type: String,
    pub table_engine: String,
    pub table_rows: i64,
    pub table_comment: String,
    pub table_columns: i64,
    pub table_indexes: i64,
    pub create_time: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub r#type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub default: String,
    pub comment: String,
    pub extra: String,
}

// ==================== DSN 解析 ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Mysql,
    Postgres,
    Sqlite,
}

/// 配置 → (引擎, 驱动可用的连接串, 库名展示)。DSN 优先；否则用表单字段拼装。
pub fn resolve_target(cfg: &DBConfig) -> Result<(Engine, String, String), String> {
    let dsn = cfg.dsn.clone().unwrap_or_default().trim().to_string();
    if matches!(cfg.use_dsn, Some(true)) && !dsn.is_empty() {
        return parse_dsn(&dsn);
    }
    if !dsn.is_empty() && cfg.host.trim().is_empty() && cfg.db_path.trim().is_empty() {
        // 兼容前端漏传 useDSN 的情况：只有 DSN 就用 DSN。
        return parse_dsn(&dsn);
    }
    build_from_fields(cfg)
}

fn parse_dsn(dsn: &str) -> Result<(Engine, String, String), String> {
    let Some((scheme, rest)) = dsn.split_once("://") else {
        return Err(format!(
            "无法识别的 DSN「{dsn}」：需要 scheme:// 前缀（mysql / postgresql / sqlite）"
        ));
    };
    match scheme.to_lowercase().as_str() {
        "mysql" | "mariadb" => Ok((Engine::Mysql, dsn.to_string(), url_database(rest))),
        "postgresql" | "postgres" | "pg" => {
            let mut target = url_database(rest);
            if target.is_empty() {
                target = "postgres".to_string();
            }
            Ok((Engine::Postgres, dsn.to_string(), target))
        }
        "sqlite" | "sqlite3" => {
            // sqlite:///abs/path、sqlite://./rel.db、sqlite://:memory:
            let path = match rest.strip_prefix('/') {
                Some(after_slash) if !after_slash.starts_with('/') => {
                    format!("/{after_slash}")
                }
                _ => rest.to_string(),
            };
            let label = path.rsplit('/').next().unwrap_or(&path).to_string();
            Ok((Engine::Sqlite, path, label))
        }
        other => Err(format!("暂不支持的数据库类型「{other}」")),
    }
}

/// `user:pass@host:3306/db?x=1` → `db`。
fn url_database(rest: &str) -> String {
    let authority = rest.rsplit('@').next().unwrap_or(rest);
    let path = authority.split_once('/').map(|(_, p)| p).unwrap_or("");
    path.split('?').next().unwrap_or("").to_string()
}

fn build_from_fields(cfg: &DBConfig) -> Result<(Engine, String, String), String> {
    let kind = cfg.db_type.to_lowercase();
    match kind.as_str() {
        "mysql" | "mariadb" => {
            let dsn = format!(
                "mysql://{}:{}@{}:{}/{}",
                percent_encode(&cfg.username),
                percent_encode(&cfg.password),
                cfg.host,
                if cfg.port == 0 { 3306 } else { cfg.port },
                cfg.database
            );
            Ok((Engine::Mysql, dsn, cfg.database.clone()))
        }
        "postgresql" | "postgres" => {
            let mut dsn = format!(
                "postgres://{}:{}@{}:{}/{}",
                percent_encode(&cfg.username),
                percent_encode(&cfg.password),
                cfg.host,
                if cfg.port == 0 { 5432 } else { cfg.port },
                cfg.database
            );
            dsn.push_str(if cfg.ssl { "?sslmode=require" } else { "?sslmode=prefer" });
            Ok((Engine::Postgres, dsn, cfg.database.clone()))
        }
        "sqlite" => {
            let path = if cfg.db_path.trim().is_empty() {
                cfg.database.clone()
            } else {
                cfg.db_path.clone()
            };
            if path.trim().is_empty() {
                return Err("SQLite 需要填写数据库文件路径".to_string());
            }
            let label = path.rsplit('/').next().unwrap_or(&path).to_string();
            Ok((Engine::Sqlite, path, label))
        }
        "oracle" => Err("暂不支持 Oracle，请把 schema 导出为 SQL 走「SQL 文件」导入".to_string()),
        "" => Err("未选择数据库类型".to_string()),
        other => Err(format!("暂不支持的数据库类型「{other}」")),
    }
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

// ==================== 阻塞驱动层 ====================

/// 一列：(列名, 用于类型映射的原始类型, 可空)。
type RawColumn = (String, String, bool);

#[derive(Debug, Default)]
struct TableEntry {
    name: String,
    engine: String,
    rows: i64,
    comment: String,
    columns: Vec<RawColumn>,
}

#[derive(Debug, Default)]
struct DbSnapshot {
    version: String,
    tables: Vec<TableEntry>,
}

fn err_text<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// (库名展示, 快照)。三个驱动共用一次全量抓取：introspection 场景表数量小，
/// 分命令各自抓取反而更容易出现前后不一致。
fn fetch_all(cfg: &DBConfig) -> Result<(String, DbSnapshot), String> {
    let (engine, target, label) = resolve_target(cfg)?;
    let snapshot = match engine {
        Engine::Mysql => fetch_mysql(&target),
        Engine::Postgres => fetch_postgres(&target),
        Engine::Sqlite => fetch_sqlite(&target),
    }?;
    Ok((label, snapshot))
}

fn fetch_mysql(dsn: &str) -> Result<DbSnapshot, String> {
    use mysql::prelude::Queryable;

    let base = mysql::Opts::from_url(dsn).map_err(err_text)?;
    let opts = mysql::OptsBuilder::from_opts(base)
        .read_timeout(Some(Duration::from_secs(CONNECT_TIMEOUT)))
        .write_timeout(Some(Duration::from_secs(CONNECT_TIMEOUT)));
    let mut conn = mysql::Conn::new(opts).map_err(err_text)?;
    let mut snapshot = DbSnapshot {
        version: conn
            .query_first("SELECT VERSION()")
            .unwrap_or_default()
            .unwrap_or_default(),
        ..Default::default()
    };

    let rows = conn
        .query_iter(
            "SELECT TABLE_NAME, IFNULL(ENGINE,''), CAST(IFNULL(TABLE_ROWS,0) AS SIGNED), IFNULL(TABLE_COMMENT,'') \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' \
             ORDER BY TABLE_NAME",
        )
        .map_err(err_text)?;
    for row in rows {
        let row = row.map_err(err_text)?;
        snapshot.tables.push(TableEntry {
            name: row.get::<String, usize>(0).unwrap_or_default(),
            engine: row.get::<String, usize>(1).unwrap_or_default(),
            rows: row.get::<i64, usize>(2).unwrap_or(0),
            comment: row.get::<String, usize>(3).unwrap_or_default(),
            columns: Vec::new(),
        });
    }

    let names: Vec<String> = snapshot.tables.iter().map(|t| t.name.clone()).collect();
    for (entry, name) in snapshot.tables.iter_mut().zip(names) {
        let result = conn
            .exec_iter(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE = 'YES' \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? \
                 ORDER BY ORDINAL_POSITION",
                (name,),
            )
            .map_err(err_text)?;
        for row in result {
            let row = row.map_err(err_text)?;
            entry.columns.push((
                row.get::<String, usize>(0).unwrap_or_default(),
                row.get::<String, usize>(1).unwrap_or_default(),
                row.get::<i64, usize>(2).unwrap_or(1) != 0,
            ));
        }
    }
    Ok(snapshot)
}

fn fetch_postgres(dsn: &str) -> Result<DbSnapshot, String> {
    let mut client = postgres::Client::connect(dsn, postgres::NoTls).map_err(err_text)?;
    let version = client
        .query_one("SHOW server_version", &[])
        .map_err(err_text)?
        .get::<_, &str>(0)
        .to_string();
    let mut snapshot = DbSnapshot {
        version,
        ..Default::default()
    };

    let names: Vec<String> = client
        .query(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
            &[],
        )
        .map_err(err_text)?
        .iter()
        .map(|row| row.get::<_, &str>(0).to_string())
        .collect();

    for name in names {
        let mut entry = TableEntry {
            name: name.clone(),
            engine: "postgres".to_string(),
            ..Default::default()
        };
        let rows = client
            .query(
                "SELECT column_name, data_type, is_nullable = 'YES' \
                 FROM information_schema.columns \
                 WHERE table_schema = 'public' AND table_name = $1 \
                 ORDER BY ordinal_position",
                &[&name],
            )
            .map_err(err_text)?;
        for row in rows.iter() {
            entry.columns.push((
                row.get::<_, &str>(0).to_string(),
                row.get::<_, &str>(1).to_string(),
                row.get::<_, bool>(2),
            ));
        }
        snapshot.tables.push(entry);
    }
    Ok(snapshot)
}

fn fetch_sqlite(path: &str) -> Result<DbSnapshot, String> {
    use rusqlite::OpenFlags;

    if path == ":memory:" || path.is_empty() {
        return Err("SQLite 需要文件路径（内存库无法 introspect）".to_string());
    }
    let conn = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(err_text)?;
    let version = format!(
        "sqlite {}",
        conn.query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))
            .unwrap_or_default()
    );
    let mut snapshot = DbSnapshot {
        version,
        ..Default::default()
    };

    let mut names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(err_text)?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(err_text)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err_text)?;
    names.retain(|name| name != "sqlite_sequence");

    for name in names {
        let mut entry = TableEntry {
            name: name.clone(),
            engine: "sqlite".to_string(),
            ..Default::default()
        };
        let mut stmt = conn
            .prepare("SELECT name, type, \"notnull\" FROM pragma_table_info(?)")
            .map_err(err_text)?;
        let rows = stmt
            .query_map([&name], |row| {
                Ok((
                    row.get::<_, String>(0).unwrap_or_default(),
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, i64>(2).map(|v| v == 0).unwrap_or(true),
                ))
            })
            .map_err(err_text)?;
        for row in rows {
            entry.columns.push(row.map_err(err_text)?);
        }
        snapshot.tables.push(entry);
    }
    Ok(snapshot)
}

// ==================== 快照 → 实体行 ====================

/// 抓取到的列 → rush-gen 规格字段（infra 列与不可映射类型跳过）。
fn columns_to_fields(columns: &[RawColumn]) -> Vec<SpecFieldDto> {
    columns
        .iter()
        .filter(|(name, _, _)| {
            let lowered = name.to_lowercase();
            !sqlimport::INFRA_COLUMNS.contains(&lowered.as_str())
        })
        .filter_map(|(name, raw_type, _)| {
            sqlimport::sql_type_to_kind(raw_type).map(|kind| SpecFieldDto {
                name: name.to_lowercase(),
                kind,
            })
        })
        .collect()
}

fn snapshot_to_sql_tables(snapshot: &DbSnapshot) -> Vec<SqlTable> {
    snapshot
        .tables
        .iter()
        .filter_map(|entry| {
            let fields = columns_to_fields(&entry.columns);
            if fields.is_empty() {
                return None;
            }
            let table = entry.name.to_lowercase();
            Some(SqlTable {
                name: sqlimport::entity_name_of(&table),
                table,
                fields,
            })
        })
        .collect()
}

// ==================== Tauri 命令 ====================

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("任务执行失败：{e}"))?
}

#[tauri::command]
pub fn get_db_config(state: State<AppState>) -> DBConfig {
    state.db_config.lock().expect("db_config 锁中毒").clone()
}

#[tauri::command]
pub fn set_db_config(state: State<AppState>, cfg: DBConfig) {
    *state.db_config.lock().expect("db_config 锁中毒") = cfg;
}

/// async 命令必须返回 `Result`（Tauri v2 约束）；Go 版契约里错误是
/// 载荷的一部分（toast 文案），所以这些命令始终返回 `Ok`。
#[tauri::command]
pub async fn test_database_connection(cfg: DBConfig) -> crate::CmdResult<ConnectionResult> {
    let started = Instant::now();
    Ok(match blocking(move || fetch_all(&cfg)).await {
        Ok((label, snapshot)) => ConnectionResult::ok(
            label,
            snapshot.version,
            snapshot.tables.len() as i64,
            started.elapsed().as_millis() as i64,
        ),
        Err(e) => ConnectionResult::err(e),
    })
}

#[tauri::command]
pub async fn get_database_tables(cfg: DBConfig) -> crate::CmdResult<Vec<TableInfo>> {
    let (_, snapshot) = blocking(move || fetch_all(&cfg)).await?;
    Ok(snapshot
        .tables
        .iter()
        .map(|entry| TableInfo {
            table_name: entry.name.clone(),
            table_type: "BASE TABLE".to_string(),
            table_engine: entry.engine.clone(),
            table_rows: entry.rows,
            table_comment: entry.comment.clone(),
            table_columns: entry.columns.len() as i64,
            table_indexes: 0,
            create_time: String::new(),
        })
        .collect())
}

#[tauri::command]
pub async fn get_table_columns(cfg: DBConfig, table: String) -> crate::CmdResult<Vec<ColumnInfo>> {
    let entry = blocking(move || fetch_all(&cfg))
        .await?
        .1
        .tables
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(&table))
        .ok_or_else(|| format!("找不到表「{table}」"))?;
    Ok(entry
        .columns
        .iter()
        .map(|(name, raw_type, nullable)| ColumnInfo {
            name: name.clone(),
            r#type: raw_type.clone(),
            nullable: *nullable,
            primary_key: false,
            default: String::new(),
            comment: String::new(),
            extra: String::new(),
        })
        .collect())
}

/// 整库导入：抓全部表列 → 实体行，整表替换当前选项（新数据源语义）。
/// 返回串沿用 Go 版约定：`''` 成功。
#[tauri::command]
pub async fn import_database_tables(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: DBConfig,
) -> crate::CmdResult<String> {
    let tables = match blocking(move || fetch_all(&cfg)).await {
        Ok((_, snapshot)) => snapshot_to_sql_tables(&snapshot),
        Err(e) => return Ok(e),
    };
    if tables.is_empty() {
        return Ok("没有可导入的表（库为空，或所有表都只有基础设施列）".to_string());
    }
    {
        let mut options = state.options.lock().expect("options 锁中毒");
        options.clear();
        for table in &tables {
            let id = next_id(&options);
            options.push(sqlimport::to_option(id, table));
        }
    }
    let _ = app.emit("table-imported", ());
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsn_cfg(dsn: &str) -> DBConfig {
        DBConfig {
            use_dsn: Some(true),
            dsn: Some(dsn.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn parses_mysql_and_postgres_dsns() {
        let (engine, target, db) =
            resolve_target(&dsn_cfg("mysql://root:p%40ss@127.0.0.1:3306/rushwind?charset=utf8mb4"))
                .unwrap();
        assert_eq!(engine, Engine::Mysql);
        assert_eq!(db, "rushwind");
        assert!(target.starts_with("mysql://"));

        let (engine, _, db) =
            resolve_target(&dsn_cfg("postgresql://u:p@localhost:5432/admin")).unwrap();
        assert_eq!(engine, Engine::Postgres);
        assert_eq!(db, "admin");
    }

    #[test]
    fn parses_sqlite_dsn_paths() {
        let (engine, path, label) = resolve_target(&dsn_cfg("sqlite:///home/x/app.db")).unwrap();
        assert_eq!(engine, Engine::Sqlite);
        assert_eq!(path, "/home/x/app.db");
        assert_eq!(label, "app.db");

        let (_, path, _) = resolve_target(&dsn_cfg("sqlite://./relative.db")).unwrap();
        assert_eq!(path, "./relative.db");
    }

    #[test]
    fn unknown_dsn_scheme_and_oracle_are_chinese_errors() {
        let err = resolve_target(&dsn_cfg("oracle://u:p@h/db")).unwrap_err();
        assert!(err.contains("暂不支持"), "{err}");
        let err = resolve_target(&DBConfig {
            db_type: "oracle".to_string(),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.contains("Oracle"), "{err}");
    }

    #[test]
    fn builds_dsn_from_fields_with_percent_encoding() {
        let cfg = DBConfig {
            db_type: "mysql".to_string(),
            host: "127.0.0.1".to_string(),
            port: 13306,
            database: "demo".to_string(),
            username: "root".to_string(),
            password: "p@ss:w/rd#1".to_string(),
            ..Default::default()
        };
        let (engine, dsn, db) = resolve_target(&cfg).unwrap();
        assert_eq!(engine, Engine::Mysql);
        assert_eq!(db, "demo");
        assert_eq!(dsn, "mysql://root:p%40ss%3Aw%2Frd%231@127.0.0.1:13306/demo");
    }

    #[test]
    fn sqlite_snapshot_maps_columns_to_kinds() {
        let dir = std::env::temp_dir().join(format!(
            "rush-ui-dbtest-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meta.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sys_dict_type (
                    id INTEGER PRIMARY KEY,
                    code VARCHAR(64) NOT NULL,
                    status TEXT,
                    level INTEGER,
                    weight REAL,
                    locked BOOLEAN,
                    created_at TEXT
                 );
                 CREATE TABLE t_audit_logs (
                    id INTEGER PRIMARY KEY,
                    payload BLOB
                 );
                 CREATE TABLE empty_infra_only (
                    id INTEGER, created_at TEXT, updated_at TEXT
                 );",
            )
            .unwrap();
        }

        let (engine, target, label) = resolve_target(&dsn_cfg(
            format!("sqlite://{}", path.to_string_lossy()).as_str(),
        ))
        .unwrap();
        assert_eq!(engine, Engine::Sqlite);
        assert_eq!(label, "meta.db");

        let snapshot = fetch_sqlite(&target).unwrap();
        let tables = snapshot_to_sql_tables(&snapshot);
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        // empty_infra_only 只有 infra 列 → 被丢弃；t_audit_logs → audit_log。
        assert_eq!(names, vec!["dict_type", "audit_log"]);

        let dict = tables.iter().find(|t| t.name == "dict_type").unwrap();
        let kinds: Vec<(&str, &str)> = dict
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("code", "string"),
                ("status", "string"),
                ("level", "i32"),
                ("weight", "f64"),
                ("locked", "bool"),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }
}
