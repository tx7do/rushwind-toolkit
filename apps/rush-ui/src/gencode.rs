//! 一键批量生成：把生成器选项表逐行翻译成 rush-gen 的调用。
//! 返回串沿用 Go 版约定——`''` 为全部成功，非空为按实体聚合的错误串，
//! 前端统一 `message.error(t('...Failed', {msg}))`。

use std::path::PathBuf;

use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use rush_gen::pages::{self, PagesOptions, PagesStack};
use tauri::State;

use crate::{devops::project_root, AppState, GeneratorOption};

fn trim_some(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn has_flag(servers: &[String], flag: &str) -> bool {
    servers.iter().any(|s| s == flag)
}

fn to_field_specs(row: &GeneratorOption) -> Result<Vec<FieldSpec>, String> {
    row.fields
        .iter()
        .map(|field| {
            let kind = FieldKind::parse(&field.kind)
                .ok_or_else(|| format!("字段「{}」的类型非法：{}", field.name, field.kind))?;
            Ok(FieldSpec { name: field.name.clone(), kind })
        })
        .collect()
}

fn collect_rows(state: &AppState) -> Result<(PathBuf, Vec<GeneratorOption>), String> {
    let root = project_root(state).ok_or_else(|| "尚未打开项目".to_string())?;
    let rows: Vec<GeneratorOption> = state
        .options
        .lock()
        .expect("options 锁中毒")
        .iter()
        .filter(|row| !row.exclude)
        .cloned()
        .collect();
    if rows.is_empty() {
        return Err("没有可生成的实体（列表为空或全部被排除）".to_string());
    }
    Ok((root, rows))
}

/// proto 消息面 package 策略：custom 用行内值，by-service 按服务名派生，
/// per-table 交给 rush-gen 的缺省 `<name>.service.v1`。
fn package_of(strategy: &str, row: &GeneratorOption, name: &str) -> Option<String> {
    match strategy {
        "custom" => trim_some(&row.proto_package),
        "by-service" => Some(format!(
            "{}.service.v1",
            trim_some(&row.service).unwrap_or_else(|| name.to_string())
        )),
        _ => None,
    }
}

#[tauri::command]
pub fn generate_grpc_code(state: State<AppState>, strategy: String, servers: Vec<String>) -> String {
    let (root, rows) = match collect_rows(&state) {
        Ok(pair) => pair,
        Err(e) => return e,
    };
    let mut errors: Vec<String> = Vec::new();
    for row in rows {
        let Some(name) = trim_some(&row.table_name) else {
            errors.push("存在实体名为空的行".to_string());
            continue;
        };
        let opts = match to_field_specs(&row) {
            Ok(fields) => EntityOptions {
                repo_root: root.clone(),
                name: name.clone(),
                table: trim_some(&row.table),
                package: package_of(&strategy, &row, &name),
                route_prefix: trim_some(&row.route_prefix),
                fields,
                code_field: trim_some(&row.code_field),
                global: row.global,
                check: has_flag(&servers, "check"),
                dry_run: has_flag(&servers, "dry_run"),
                skip_manifest: false,
                // UI 重复生成即重生成：--regen 语义，幂等由手术插入保证。
                overwrite: true,
                auth_free: row.auth_free,
            },
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        if let Err(e) = entity::generate_entity(&opts) {
            errors.push(format!("{name}: {e:#}"));
        }
    }
    errors.join("\n")
}

fn stack_of(stack: &str) -> PagesStack {
    match stack.trim() {
        "vben" => PagesStack::Vben,
        "element" => PagesStack::Element,
        _ => PagesStack::React,
    }
}

#[tauri::command]
pub fn generate_rest_code(
    state: State<AppState>,
    stack: String,
    group: String,
    servers: Vec<String>,
) -> String {
    let (root, rows) = match collect_rows(&state) {
        Ok(pair) => pair,
        Err(e) => return e,
    };
    let group = trim_some(&group);
    let stack = stack_of(&stack);
    let mut errors: Vec<String> = Vec::new();
    for row in rows {
        let Some(name) = trim_some(&row.table_name) else {
            errors.push("存在实体名为空的行".to_string());
            continue;
        };
        // 字段留空由 rush-gen 回退到 .rush 规格（与 CLI 缺 --field 同款）。
        let opts = match to_field_specs(&row) {
            Ok(fields) => PagesOptions {
                repo_root: root.clone(),
                name: name.clone(),
                group: group.clone(),
                route_prefix: trim_some(&row.route_prefix),
                fields,
                code_field: trim_some(&row.code_field),
                stack,
                global: None,
                overwrite: has_flag(&servers, "overwrite"),
                dry_run: has_flag(&servers, "dry_run"),
            },
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        if let Err(e) = pages::generate_pages(&opts) {
            errors.push(format!("{name}: {e:#}"));
        }
    }
    errors.join("\n")
}
