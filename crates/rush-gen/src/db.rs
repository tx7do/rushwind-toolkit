//! DB 内省 → 实体规格——rush-ui 长路线的第一块：连上部署库，把表结构
//! 反推成 `.rush/<name>.json` 规格，接进 `gen entity --regen` 的既有
//! 生命周期。
//!
//! 生成器的实体模型是"业务字段 + 标准尾段"：`gen entity` 的模板自己
//! 生成 id/sort_order/租户两列/审计三列/时间戳三列（entity.rs 的 tail
//! 表），所以内省的正确输出是**排除标准列后的业务字段清单**——撞上
//! 尾段名的列不进 fields。`tenant_id` 列的有无恰好推导 `global`（租户
//! 表 / 平台全局表），与模板的尾段裁剪规则一致。
//!
//! 类型映射表（信息有损处宁可拒译也不降级）：PG 的 `format_type` 串
//! 与 SQLite 的类型声明各走一张表；PG 原生 enum（`typtype='e'`）取
//! pg_enum 的标签按 sortorder 映成 0..n-1 的生成器枚举（数值语义有
//! 损，标签与顺序无损）；`bigint`/`numeric`/`json`/时间日期/数组在
//! FieldKind 词汇表之外，进 skipped 明细——调用方看得见每一个没被翻
//! 译的列。

use std::collections::{BTreeMap, BTreeSet};

use crate::entity::{plural_of, EnumValues, FieldKind, FieldSpec};
use crate::spec::{self, EntitySpecFile};
use crate::{Error, Result};

/// 生成器实体模型的固定尾段列——模板自生成，内省不进 fields。
const STANDARD_TAIL: [&str; 10] = [
    "id",
    "sort_order",
    "tenant_id",
    "tenant_name",
    "created_by",
    "updated_by",
    "deleted_by",
    "created_at",
    "updated_at",
    "deleted_at",
];

/// 内省忽略的系统表（迁移账本等）。
const SYSTEM_TABLE_PREFIXES: [&str; 2] = ["_sqlx_", "seaql_migrations"];

/// 一张表的规格化结果。
pub struct PulledEntity {
    pub spec: EntitySpecFile,
    /// 不可映射的列（列名, 数据库类型）——用户看得见每一个没被翻译的列。
    pub skipped: Vec<(String, String)>,
    /// 规格落盘路径——由调用方在保存后回填（dry-run 为 None）。
    pub written_to: Option<std::path::PathBuf>,
}

/// 一批表的拉取报告。
pub struct PullReport {
    pub entities: Vec<PulledEntity>,
    /// 被排除的表名（系统表）。
    pub excluded_tables: Vec<String>,
}

/// 从 PG `format_type` 串映射业务字段类型；`None` = 词汇表之外（进
/// skipped）。
pub fn map_pg_type(type_name: &str) -> Option<FieldKind> {
    // 剥 (n)/(precision,scale) 修饰与数组记法。
    let base = type_name.split('(').next().unwrap_or(type_name).trim();
    match base {
        "character varying" | "varchar" | "character" | "char" | "text" | "citext" | "uuid"
        | "name" => Some(FieldKind::String),
        "smallint" | "int2" | "integer" | "int" | "int4" => Some(FieldKind::Int32),
        "boolean" | "bool" => Some(FieldKind::Bool),
        "real" | "float4" | "double precision" | "float8" => Some(FieldKind::Float64),
        // bigint 的值域在词汇表外；numeric 带精度语义；json/时间日期/数组
        // 均无对应 FieldKind。
        _ => None,
    }
}

/// 从 SQLite 的类型声明（PRAGMA table_info 的 type 列）映射——SQLite
/// 是亲和性语义，按声明子串判别。
pub fn map_sqlite_type(decl: &str) -> Option<FieldKind> {
    let upper = decl.to_ascii_uppercase();
    if upper.contains("INT") {
        return Some(FieldKind::Int32);
    }
    if upper.contains("BOOL") {
        return Some(FieldKind::Bool);
    }
    if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
        return Some(FieldKind::String);
    }
    if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        return Some(FieldKind::Float64);
    }
    None
}

/// PG 原生 enum 的标签集 → 生成器枚举（labels 按 sortorder 映
/// 0..n-1）；列缺省 `'LABEL'::typename` 剥出缺省文本，不在集合内则回
/// 退 0 值文本。
pub fn pg_enum_to_kind(labels: &[String], column_default: Option<&str>) -> Option<FieldKind> {
    if labels.is_empty() {
        return None;
    }
    let values: Vec<(i32, String)> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| (i as i32, label.clone()))
        .collect();
    let default = column_default
        .and_then(|d| d.split('\'').nth(1).map(str::to_owned))
        .filter(|text| values.iter().any(|(_, v)| v == text))
        .unwrap_or_else(|| values[0].1.clone());
    Some(FieldKind::Enum(EnumValues { values, default }))
}

/// 表名 → 实体名：剥 `sys_` 前缀，复数收单数（`ies`→`y`、去尾 `s`，
/// `ss`/`us` 例外，其余原样——保守，宁可不翻不错翻）。
pub fn entity_name_of(table: &str) -> String {
    let stripped = table.strip_prefix("sys_").unwrap_or(table);
    if let Some(stem) = stripped.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if stripped.ends_with('s') && !stripped.ends_with("ss") && !stripped.ends_with("us") {
        return stripped[..stripped.len() - 1].to_string();
    }
    stripped.to_string()
}

/// 一张表组装规格（纯函数，映射与裁剪语义都在这里，双 dialect 复用）。
/// `columns` 按 ordinal 序：(列名, 翻译结果——kind 或原类型串)。
pub fn build_spec(
    table: &str,
    columns: Vec<(String, std::result::Result<FieldKind, String>)>,
) -> Result<PulledEntity> {
    let name = entity_name_of(table);
    if name.is_empty() {
        return Err(Error::InvalidInput(format!("表名反推不出实体名：{table}")));
    }
    let global = !columns.iter().any(|(col, _)| col == "tenant_id");

    let mut fields = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = BTreeSet::new();
    for (column, mapped) in columns {
        if STANDARD_TAIL.contains(&column.as_str()) {
            continue;
        }
        match mapped {
            Ok(kind) => {
                if seen.insert(column.clone()) {
                    fields.push(FieldSpec { name: column, kind });
                }
            }
            Err(raw) => skipped.push((column, raw)),
        }
    }
    if fields.is_empty() {
        return Err(Error::InvalidInput(format!(
            "表 {table} 反推不出业务字段（全部是标准尾段或不可映射列）"
        )));
    }

    // 缺省展开与 gen entity 一致：package = <name>.service.v1，
    // route_prefix = /admin/v1/<复数>。
    let spec = spec::from_parts(
        &name,
        table,
        &format!("{name}.service.v1"),
        &format!("/admin/v1/{}", plural_of(&name)),
        &fields,
        None,
        global,
    );
    Ok(PulledEntity {
        spec,
        skipped,
        written_to: None,
    })
}

/// 连接 PG，内省 `schema` 下的基表（系统表排除；`only` 非空时取交
/// 集），返回拉取报告。
pub async fn pull_postgres(dsn: &str, schema: &str, only: &BTreeSet<String>) -> Result<PullReport> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(dsn)
        .await
        .map_err(|e| Error::InvalidInput(format!("连接失败：{e}")))?;

    // 表清单（基表；系统表排除）。
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' ORDER BY table_name",
    )
    .bind(schema)
    .fetch_all(&pool)
    .await
    .map_err(|e| Error::InvalidInput(format!("内省表清单失败：{e}")))?;

    let mut excluded = Vec::new();
    let mut tables = Vec::new();
    for (table,) in rows {
        if SYSTEM_TABLE_PREFIXES.iter().any(|p| table.starts_with(p)) {
            excluded.push(table);
            continue;
        }
        if !only.is_empty() && !only.contains(&table) {
            continue;
        }
        tables.push(table);
    }

    // 全 schema 的原生 enum 标签集（按类型名）。
    let mut enums: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let enum_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT t.typname, e.enumlabel FROM pg_type t \
         JOIN pg_enum e ON e.enumtypid = t.oid \
         JOIN pg_namespace n ON n.oid = t.typnamespace \
         WHERE n.nspname = $1 ORDER BY t.typname, e.enumsortorder",
    )
    .bind(schema)
    .fetch_all(&pool)
    .await
    .map_err(|e| Error::InvalidInput(format!("内省枚举失败：{e}")))?;
    for (typname, label) in enum_rows {
        enums.entry(typname).or_default().push(label);
    }

    let mut entities = Vec::new();
    for table in &tables {
        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT a.attname AS column_name, \
                    pg_catalog.format_type(a.atttypid, a.atttypmod) AS full_type, \
                    COALESCE(t.typtype, 'b') AS typtype, \
                    pg_get_expr(d.adbin, d.adrelid) AS column_default \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef d ON a.attrelid = d.adrelid AND a.attnum = d.adnum \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| Error::InvalidInput(format!("内省列失败（{table}）：{e}")))?;

        let columns = rows
            .into_iter()
            .map(|(name, full_type, typtype, default)| {
                let mapped = if typtype == "e" {
                    let labels: &[String] = enums
                        .get(&udt_base(&full_type))
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    pg_enum_to_kind(labels, default.as_deref()).ok_or_else(|| full_type.clone())
                } else {
                    map_pg_type(&full_type).ok_or_else(|| full_type.clone())
                };
                (name, mapped)
            })
            .collect();
        entities.push(build_spec(table, columns)?);
    }

    Ok(PullReport {
        entities,
        excluded_tables: excluded,
    })
}

/// `format_type` 的串 → 用户自定义类型名（剥 array 记法与修饰）。
fn udt_base(full_type: &str) -> String {
    full_type
        .trim_start_matches('_')
        .split('(')
        .next()
        .unwrap_or(full_type)
        .trim()
        .to_string()
}

/// 连接 SQLite（path 或 `sqlite::memory:`），内省全部用户表。
pub async fn pull_sqlite(dsn: &str) -> Result<PullReport> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(2)
        .connect(dsn)
        .await
        .map_err(|e| Error::InvalidInput(format!("连接失败：{e}")))?;

    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .fetch_all(&pool)
            .await
            .map_err(|e| Error::InvalidInput(format!("内省表清单失败：{e}")))?;

    let mut excluded = Vec::new();
    let mut entities = Vec::new();
    for (table,) in rows {
        if SYSTEM_TABLE_PREFIXES.iter().any(|p| table.starts_with(p)) {
            excluded.push(table);
            continue;
        }
        // 标识符白名单：PRAGMA 不支持绑定参数，表名来自上一查询的内省
        // 结果，仍只放行安全的名字。
        let safe = table
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !table.is_empty();
        if !safe {
            excluded.push(table);
            continue;
        }
        let rows: Vec<(String, String)> = sqlx::query_as(&format!(
            "SELECT name, type FROM pragma_table_info('{table}')"
        ))
        .fetch_all(&pool)
        .await
        .map_err(|e| Error::InvalidInput(format!("内省列失败（{table}）：{e}")))?;
        let columns = rows
            .into_iter()
            .map(|(name, decl)| {
                let mapped = map_sqlite_type(&decl).ok_or_else(|| decl.clone());
                (name, mapped)
            })
            .collect();
        entities.push(build_spec(&table, columns)?);
    }

    Ok(PullReport {
        entities,
        excluded_tables: excluded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_type_table_translates_the_scalar_vocabulary() {
        assert_eq!(map_pg_type("character varying"), Some(FieldKind::String));
        assert_eq!(
            map_pg_type("character varying(255)"),
            Some(FieldKind::String)
        );
        assert_eq!(map_pg_type("text"), Some(FieldKind::String));
        assert_eq!(map_pg_type("uuid"), Some(FieldKind::String));
        assert_eq!(map_pg_type("integer"), Some(FieldKind::Int32));
        assert_eq!(map_pg_type("smallint"), Some(FieldKind::Int32));
        assert_eq!(map_pg_type("boolean"), Some(FieldKind::Bool));
        assert_eq!(map_pg_type("double precision"), Some(FieldKind::Float64));
        assert_eq!(map_pg_type("real"), Some(FieldKind::Float64));
        // 信息有损处拒译。
        assert_eq!(map_pg_type("bigint"), None);
        assert_eq!(map_pg_type("numeric(10,2)"), None);
        assert_eq!(map_pg_type("timestamp with time zone"), None);
        assert_eq!(map_pg_type("jsonb"), None);
        assert_eq!(map_pg_type("integer[]"), None);
    }

    #[test]
    fn sqlite_declarations_translate_by_affinity() {
        assert_eq!(map_sqlite_type("INTEGER"), Some(FieldKind::Int32));
        assert_eq!(map_sqlite_type("TEXT"), Some(FieldKind::String));
        assert_eq!(map_sqlite_type("VARCHAR(64)"), Some(FieldKind::String));
        assert_eq!(map_sqlite_type("BOOLEAN"), Some(FieldKind::Bool));
        assert_eq!(map_sqlite_type("REAL"), Some(FieldKind::Float64));
        assert_eq!(map_sqlite_type("BLOB"), None);
    }

    #[test]
    fn pg_enum_labels_become_zero_based_generator_enums() {
        let kind = pg_enum_to_kind(
            &["OFF".into(), "ON".into(), "PENDING".into()],
            Some("'ON'::widget_state"),
        )
        .unwrap();
        match kind {
            FieldKind::Enum(values) => {
                assert_eq!(
                    values.values,
                    vec![(0, "OFF".into()), (1, "ON".into()), (2, "PENDING".into())]
                );
                assert_eq!(values.default, "ON");
            }
            other => panic!("{other:?}"),
        }
        // 缺省不在集合内 → 回退 0 值文本。
        let kind = pg_enum_to_kind(&["OFF".into(), "ON".into()], Some("'JUNK'::t")).unwrap();
        match kind {
            FieldKind::Enum(values) => assert_eq!(values.default, "OFF"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn entity_name_strips_prefix_and_singularizes() {
        assert_eq!(entity_name_of("sys_widgets"), "widget");
        assert_eq!(entity_name_of("sys_dict_entries"), "dict_entry");
        assert_eq!(entity_name_of("sys_statuses"), "statuse"); // ss 保守不翻
        assert_eq!(entity_name_of("plugin_configs"), "plugin_config");
        assert_eq!(entity_name_of("sys_bus"), "bus");
    }

    #[test]
    fn build_spec_excludes_the_standard_tail_and_derives_global() {
        let columns = vec![
            (
                "id".into(),
                Ok(FieldKind::Int32) as std::result::Result<FieldKind, String>,
            ),
            ("tenant_id".into(), Ok(FieldKind::Int32)),
            ("code".into(), Ok(FieldKind::String)),
            (
                "state".into(),
                Ok(FieldKind::Enum(EnumValues {
                    values: vec![(0, "OFF".into()), (1, "ON".into())],
                    default: "OFF".into(),
                })),
            ),
            ("payload".into(), Err("jsonb".into())),
            ("created_at".into(), Ok(FieldKind::String)),
        ];
        let pulled = build_spec("sys_widgets", columns).expect("spec builds");
        let names: Vec<&str> = pulled.spec.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["code", "state"]);
        assert!(!pulled.spec.global);
        assert_eq!(
            pulled.skipped,
            vec![("payload".to_string(), "jsonb".to_string())]
        );
        assert_eq!(pulled.spec.name, "widget");
        assert_eq!(pulled.spec.table, "sys_widgets");
        // 缺省展开与 gen entity 一致。
        assert_eq!(pulled.spec.package, "widget.service.v1");
        assert_eq!(pulled.spec.route_prefix, "/admin/v1/widgets");
    }

    #[test]
    fn build_spec_without_tenant_column_is_global() {
        let columns = vec![(
            "code".into(),
            Ok(FieldKind::String) as std::result::Result<FieldKind, String>,
        )];
        let pulled = build_spec("sys_platforms", columns).unwrap();
        assert!(pulled.spec.global);
    }

    #[tokio::test]
    async fn sqlite_end_to_end_pulls_specs() {
        let dir = tempfile::tempdir().unwrap();
        let dsn = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("widgets.db").display()
        );
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE sys_widgets (
                id INTEGER PRIMARY KEY,
                tenant_id INTEGER NOT NULL,
                code TEXT NOT NULL,
                state TEXT NOT NULL,
                ratio REAL,
                raw BLOB,
                created_at TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE _sqlx_migrations (version TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let report = pull_sqlite(&dsn).await.unwrap();
        assert_eq!(report.excluded_tables, vec!["_sqlx_migrations"]);
        assert_eq!(report.entities.len(), 1);
        let pulled = &report.entities[0];
        assert_eq!(pulled.spec.name, "widget");
        assert_eq!(pulled.spec.table, "sys_widgets");
        let names: Vec<&str> = pulled.spec.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["code", "state", "ratio"]);
        assert_eq!(
            pulled.skipped,
            vec![("raw".to_string(), "BLOB".to_string())]
        );
    }
}
