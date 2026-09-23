//! 生成物回滚（`rush gen undo <name>`）：按规格文件反向移除一个实体的
//! 全部产物——后端链（proto/实体/repo/service + 五处注册 + 免鉴权条目）
//! 与三栈页面（文件 + 菜单种子 + 静态路由条目）。
//!
//! 移除是插入的逆运算：单行注册按精确行删；导入块内拼词的行内恢复、
//! seed 函数块与路由 children 条目按括号配平定位块边界。全部操作幂等
//! ——找不到目标即视为已移除（skip），重复 undo 第二遍是纯 no-op。
//! 规格文件最后删除——它是"这个实体存在过"的凭证。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::entity::{pascal_of, plural_of};
use crate::{Error, Result};

/// `rush gen undo` 选项。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UndoOptions {
    /// rushwind-admin 仓库根目录。
    pub repo_root: PathBuf,
    /// 实体名，snake_case 单数。
    pub name: String,
    /// 只报告不落盘。
    pub dry_run: bool,
}

/// 回滚报告。
#[derive(Debug, Default, Serialize)]
pub struct UndoReport {
    pub removed_files: Vec<PathBuf>,
    pub removed_dirs: Vec<PathBuf>,
    pub edited_files: Vec<PathBuf>,
    pub skipped: Vec<String>,
    pub notes: Vec<String>,
}

/// 单行精确删除（行内容去首尾空白后等于目标行才删）。
fn remove_exact_line(text: &str, needle_trimmed: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == needle_trimmed)?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() - 1);
    out.extend_from_slice(&lines[..at]);
    out.extend_from_slice(&lines[at + 1..]);
    Some(out.join("\n"))
}

/// 删除包含 marker 的那一行。
fn remove_line_containing(text: &str, marker: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let at = lines.iter().position(|line| line.contains(marker))?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() - 1);
    out.extend_from_slice(&lines[..at]);
    out.extend_from_slice(&lines[at + 1..]);
    Some(out.join("\n"))
}

/// 从 `use crate::services::{` 块内摘掉一个导入名（行级或行内拼词还原）。
fn remove_use_token(text: &str, token: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("use crate::services::{"))?;
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        if line.trim_start().starts_with("};") {
            end = start + offset;
            break;
        }
    }
    // 单行形态：整个 "    Token," 行
    for index in start..=end {
        if lines[index].trim() == format!("{token},") {
            let mut out: Vec<&str> = Vec::with_capacity(lines.len() - 1);
            out.extend_from_slice(&lines[..index]);
            out.extend_from_slice(&lines[index + 1..]);
            return Some(out.join("\n"));
        }
    }
    // 行内拼词形态：还原 "Token, " 或 ", Token" 的拼接
    for index in start..=end {
        if lines[index].contains(&format!("{token}, ")) {
            let edited = lines[index].replacen(&format!("{token}, "), "", 1);
            let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
            out[index] = edited;
            return Some(out.join("\n"));
        }
        if lines[index].contains(&format!(", {token}")) {
            let edited = lines[index].replacen(&format!(", {token}"), "", 1);
            let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
            out[index] = edited;
            return Some(out.join("\n"));
        }
    }
    None
}

/// 删除包含 marker 的行所处的 `{ ... }` 块（含块_open 行本身）：向上找
/// 最近的 trim 后为 `{` 的行作为块头，向下按大括号配平找块尾。
fn remove_block_containing(text: &str, marker: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let marker_at = lines.iter().position(|line| line.contains(marker))?;
    let mut open = marker_at;
    while open > 0 && lines[open].trim() != "{" {
        open -= 1;
    }
    if lines[open].trim() != "{" {
        return None;
    }
    let mut balance: i64 = 0;
    let mut close = open;
    for (offset, line) in lines[open..].iter().enumerate() {
        balance += line.matches('{').count() as i64 - line.matches('}').count() as i64;
        close = open + offset;
        if balance <= 0 {
            break;
        }
    }
    // 吸收块尾同行或下一行的孤立逗号残留（",," 或 "},\n\n" 交给幂等粗略处理：
    // 只删块本身，逗号在块尾行内一并移除由 trim 完成）
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    out.extend_from_slice(&lines[..open]);
    out.extend_from_slice(&lines[close + 1..]);
    Some(out.join("\n"))
}

/// 删除 seed 函数块：向上吸收紧邻的 `///` 文档注释，向下按大括号配平
/// 到函数收口的行首 `}`。
fn remove_seed_function(text: &str, fn_name: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let marker = format!("async fn {fn_name}");
    let fn_at = lines.iter().position(|line| line.contains(&marker))?;
    let mut start = fn_at;
    while start > 0 && lines[start - 1].trim_start().starts_with("///") {
        start -= 1;
    }
    let mut balance: i64 = 0;
    let mut end = fn_at;
    for (offset, line) in lines[fn_at..].iter().enumerate() {
        balance += line.matches('{').count() as i64 - line.matches('}').count() as i64;
        end = fn_at + offset;
        if balance <= 0 {
            break;
        }
    }
    // 若块后紧跟一个空行，一并吸掉避免双空行
    let mut stop = end + 1;
    if stop < lines.len() && lines[stop].trim().is_empty() {
        stop += 1;
    }
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    out.extend_from_slice(&lines[..start]);
    out.extend_from_slice(&lines[stop..]);
    Some(out.join("\n"))
}

struct Ctx<'a> {
    dry_run: bool,
    report: &'a mut UndoReport,
}

fn remove_file(ctx: &mut Ctx, path: &Path) {
    if path.is_file() {
        if !ctx.dry_run {
            let _ = fs::remove_file(path);
        }
        ctx.report.removed_files.push(path.to_path_buf());
    } else {
        ctx.report
            .skipped
            .push(format!("{}（不存在）", path.display()));
    }
}

fn remove_dir(ctx: &mut Ctx, path: &Path) {
    if path.is_dir() {
        if !ctx.dry_run {
            let _ = fs::remove_dir_all(path);
        }
        ctx.report.removed_dirs.push(path.to_path_buf());
    }
}

/// 删除目录并向上清理随之变空的父目录（不越过 `stop_at`）——消息面
/// proto 的 package 目录是 `widget/service/v1` 三层嵌套，只删叶子会留
/// 空壳。
fn remove_dir_pruned(ctx: &mut Ctx, path: &Path, stop_at: &Path) {
    if !path.is_dir() {
        ctx.report
            .skipped
            .push(format!("{}（不存在）", path.display()));
        return;
    }
    if !ctx.dry_run {
        let _ = fs::remove_dir_all(path);
    }
    ctx.report.removed_dirs.push(path.to_path_buf());
    if ctx.dry_run {
        return;
    }
    let mut dir = path.parent();
    while let Some(current) = dir {
        if !current.starts_with(stop_at) || current == stop_at {
            break;
        }
        match fs::read_dir(current) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    break;
                }
                let _ = fs::remove_dir(current);
            }
            Err(_) => break,
        }
        dir = current.parent();
    }
}

fn edit_file(ctx: &mut Ctx, path: &Path, edit: impl FnOnce(&str) -> Option<String>, what: &str) {
    if !path.is_file() {
        ctx.report
            .skipped
            .push(format!("{}（不存在）", path.display()));
        return;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            ctx.report
                .notes
                .push(format!("{} 读取失败（{err}），请手动处理", path.display()));
            return;
        }
    };
    match edit(&text) {
        Some(edited) => {
            if !ctx.dry_run {
                if let Err(err) = fs::write(path, &edited) {
                    ctx.report
                        .notes
                        .push(format!("{} 写回失败（{err}），请手动处理", path.display()));
                    return;
                }
            }
            if !ctx.report.edited_files.contains(&path.to_path_buf()) {
                ctx.report.edited_files.push(path.to_path_buf());
            }
        }
        None => ctx
            .report
            .skipped
            .push(format!("{}（{what}：目标已在位或未找到）", path.display())),
    }
}

/// 回滚一个实体的全部生成物。规格文件缺失时报错（没有规格就没有可靠
/// 的回滚清单——这正是规格要落盘的原因）。
pub fn undo_entity(opts: &UndoOptions) -> Result<UndoReport> {
    if !crate::entity::is_snake(&opts.name) {
        return Err(Error::InvalidInput(format!(
            "实体名必须是 snake_case：{}",
            opts.name
        )));
    }
    let spec = crate::spec::load(&opts.repo_root, &opts.name)?;
    let pascal = pascal_of(&opts.name);
    let plural = plural_of(&opts.name);
    let table = spec.table.clone();
    let group = spec.group.clone().unwrap_or_else(|| "system".to_owned());
    let root = opts.repo_root.clone();

    let mut report = UndoReport::default();
    let mut ctx = Ctx {
        dry_run: opts.dry_run,
        report: &mut report,
    };

    // ---- 后端链：生成文件 ----
    let src = root.join("backend/services/admin-api/src");
    remove_file(
        &mut ctx,
        &root.join(format!(
            "backend/api/protos/admin/service/v1/i_{}.proto",
            opts.name
        )),
    );
    // 消息面 proto 目录（package 路径 = spec.package 的 '.' → '/'）
    let proto_dir = spec.package.replace('.', "/");
    remove_dir_pruned(
        &mut ctx,
        &root.join(format!("backend/api/protos/{proto_dir}")),
        &root.join("backend/api/protos"),
    );
    remove_file(&mut ctx, &src.join(format!("data/{}.rs", table)));
    remove_file(&mut ctx, &src.join(format!("data/repos/{}.rs", opts.name)));
    remove_file(&mut ctx, &src.join(format!("services/{}.rs", opts.name)));

    // ---- 后端链：五处注册 ----
    edit_file(
        &mut ctx,
        &src.join("data.rs"),
        |text| remove_exact_line(text, &format!("pub mod {table};")),
        "data.rs 注册行",
    );
    edit_file(
        &mut ctx,
        &src.join("migration.rs"),
        |text| remove_line_containing(text, &format!(".table::<data::{table}::Entity>()")),
        "migration 表项",
    );
    edit_file(
        &mut ctx,
        &src.join("data/repos/mod.rs"),
        |text| {
            let text = remove_exact_line(text, &format!("mod {};", opts.name))
                .or_else(|| remove_exact_line(text, &format!("pub mod {};", opts.name)))?;
            remove_exact_line(&text, &format!("pub use {}::{pascal}Repo;", opts.name))
        },
        "repos 注册",
    );
    edit_file(
        &mut ctx,
        &src.join("services.rs"),
        |text| {
            let text = remove_exact_line(text, &format!("mod {};", opts.name))?;
            remove_exact_line(&text, &format!("pub use {}::{pascal}Service;", opts.name))
        },
        "services 注册",
    );
    let rest_rs = src.join("server/rest.rs");
    edit_file(
        &mut ctx,
        &rest_rs,
        |text| {
            let text = remove_line_containing(text, &format!("(mount_{}_service,", opts.name))?;
            remove_use_token(&text, &format!("{pascal}Service"))
        },
        "rest 挂载/导入",
    );

    // ---- 免鉴权条目（--auth-free 生成过才存在；没有就跳过） ----
    let auth_free_rs = root.join("backend/crates/proto/src/auth_free.rs");
    if auth_free_rs.is_file() {
        edit_file(
            &mut ctx,
            &auth_free_rs,
            |text| {
                let fq = format!("{}.{}Service", spec.package, pascal);
                let region_start = text.find("pub const AUTH_FREE")?;
                let region = &text[region_start..];
                if !region.contains(&format!("\"{fq}\"")) {
                    return None;
                }
                // 逐对移除该服务的全部条目（单行形态）
                let lines: Vec<&str> = text.split('\n').collect();
                let kept: Vec<&str> = lines
                    .into_iter()
                    .filter(|line| {
                        !(line.contains(&format!("\"{fq}\""))
                            && line.contains('(')
                            && line.trim_end().ends_with("),"))
                    })
                    .collect();
                Some(kept.join("\n"))
            },
            "auth_free 条目",
        );
    }

    // ---- seed 菜单（react 栈；无则跳过） ----
    let seed_rs = src.join("seed.rs");
    if seed_rs.is_file() {
        let fn_name = format!("seed_gen_menu_{}", opts.name);
        edit_file(
            &mut ctx,
            &seed_rs,
            |text| {
                // needle 不带缩进：remove_exact_line 按 trim 后比较
                let text = remove_exact_line(text, &format!("{fn_name}(state).await?;"))
                    .unwrap_or_else(|| text.to_owned());
                remove_seed_function(&text, &fn_name)
            },
            "seed 菜单",
        );
    }

    // ---- react 页面 ----
    let react = root.join("frontend/admin/react");
    if react.join("package.json").is_file() {
        remove_file(
            &mut ctx,
            &react.join(format!("src/api/hooks/{}.ts", opts.name)),
        );
        remove_dir(
            &mut ctx,
            &react.join(format!("src/pages/app/{group}/{plural}")),
        );
        for locale in ["zh-CN", "en-US"] {
            remove_file(
                &mut ctx,
                &react.join(format!("src/locales/{locale}/_modules/{}.json", opts.name)),
            );
        }
    }

    // ---- vben 页面 ----
    let vben = root.join("frontend/admin/vue-vben/apps/admin");
    if vben.join("package.json").is_file() {
        remove_file(
            &mut ctx,
            &vben.join(format!("src/api/composables/{}.ts", opts.name)),
        );
        remove_dir(
            &mut ctx,
            &vben.join(format!("src/views/app/{group}/{plural}")),
        );
        for lang in ["zh-CN", "en-US"] {
            let page_json = vben.join(format!("src/locales/langs/{lang}/page.json"));
            if page_json.is_file() {
                edit_file(
                    &mut ctx,
                    &page_json,
                    |text| {
                        let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
                        value.as_object_mut()?.remove(&opts.name)?;
                        let mut out = serde_json::to_string_pretty(&value).ok()?;
                        out.push('\n');
                        Some(out)
                    },
                    "vben 文案键",
                );
            }
        }
        let route_module = vben.join(format!("src/router/routes/modules/app/{group}.ts"));
        let marker = format!("#/views/app/{group}/{plural}/index.vue");
        if route_module.is_file() {
            edit_file(
                &mut ctx,
                &route_module,
                |text| {
                    if !text.contains(&marker) {
                        return None;
                    }
                    remove_block_containing(text, &marker)
                },
                "vben 路由条目",
            );
        }
    }

    // ---- element 页面 ----
    let element = root.join("frontend/admin/vue-element");
    if element.join("package.json").is_file() {
        remove_file(
            &mut ctx,
            &element.join(format!("src/api/composables/{}.ts", opts.name)),
        );
        remove_dir(
            &mut ctx,
            &element.join(format!("src/pages/app/{group}/{plural}")),
        );
        for lang in ["zh-CN", "en-US"] {
            remove_file(
                &mut ctx,
                &element.join(format!("src/locales/{lang}/pages/{}.json", opts.name)),
            );
        }
        let route_module = element.join(format!("src/router/routes/modules/app/{group}.ts"));
        let marker = format!("@/pages/app/{group}/{plural}/index.vue");
        if route_module.is_file() {
            edit_file(
                &mut ctx,
                &route_module,
                |text| {
                    if !text.contains(&marker) {
                        return None;
                    }
                    remove_block_containing(text, &marker)
                },
                "element 路由条目",
            );
        }
    }

    // ---- 规格文件（最后删除：它是回滚清单本身） ----
    remove_file(&mut ctx, &crate::spec::spec_path(&root, &opts.name));

    // ---- proto 清单重建（生成物已移除，清单必须跟上） ----
    let manifest_path = crate::manifest::Flavor::Proto.manifest_path(&root);
    if manifest_path.is_file() && !opts.dry_run {
        match crate::manifest::rebuild(
            &crate::manifest::Flavor::Proto.tree_path(&root),
            &manifest_path,
            crate::manifest::Flavor::Proto,
        ) {
            Ok(_) => {
                if !ctx.report.edited_files.contains(&manifest_path) {
                    ctx.report.edited_files.push(manifest_path);
                }
            }
            Err(err) => ctx.report.notes.push(format!(
                "proto 清单重建失败（{err:#}）：请手动 rush manifest proto --rebuild"
            )),
        }
    }

    ctx.report.notes.push(format!(
        "已回滚实体「{}」；数据库表 {} 不受影响（如有数据需手动清理）",
        opts.name, table
    ));
    Ok(report)
}
