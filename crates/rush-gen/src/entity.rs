//! 领域实体链生成（`rush gen entity`）：为一个标准 CRUD 实体生成
//! rushwind-admin 的后端全链。
//!
//! 模板基准是仓内最小的实体链 dict_type（消息面 proto + admin HTTP 注解面
//! proto + SeaORM 实体 + repo_shell 宏仓库 + Handlers trait 实现 + mount
//! 表项）。生成物与 `rushwind-gen-http` 在 build.rs 里生成的 trait/mount
//! 签名对齐——模板与本仓模板同源，trait 漂移应在本仓先暴露。
//!
//! 四个既有文件按行级手术插入（幂等，锚点缺失报错）：
//! `data.rs`（mod + pub use）、`migration.rs`（EntityTables 链）、
//! `data/repos/mod.rs`（mod + pub use）、`services.rs`（mod + pub use）、
//! `server/rest.rs`（mount_services! 表）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::manifest::Flavor;
use crate::{manifest, Error, Result};

/// 业务字段类型（`--field name:kind` 的 kind）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    /// proto `string`，实体非空 `String`。
    String,
    /// proto `int32`，实体可空 `Option<i32>`。
    Int32,
    /// proto `uint32`，实体可空 `Option<u32>`。
    Uint32,
    /// proto `bool`，实体可空 `Option<bool>`。
    Bool,
    /// proto `double`，实体可空 `Option<f64>`。
    Float64,
    /// 文本列枚举（仓内模式，见 data.rs "Enum columns carry the enum
    /// names as text"：proto enum → prost 按 i32 匹配 → 文本列）。
    Enum(EnumValues),
}

/// 枚举字段的取值集：`(数值, 文本)` 有序列表（必须含 0）+ 未识别回退文本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumValues {
    pub values: Vec<(i32, String)>,
    pub default: String,
}

fn is_upper_snake(s: &str) -> bool {
    let mut cs = s.chars();
    let first_ok = cs.next().is_some_and(|c| c.is_ascii_uppercase());
    first_ok
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

impl FieldKind {
    /// 解析 `--field name:kind` 的 kind 段。标量含常用别名；枚举语法为
    /// `enum(0=A,1=B@default=B)`（必须含 0 值项；@default 是未识别值的
    /// 回退文本，缺省取 0 值文本）。
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "string" => return Some(Self::String),
            "i32" | "int" => return Some(Self::Int32),
            "u32" | "uint" => return Some(Self::Uint32),
            "bool" => return Some(Self::Bool),
            "f64" | "float" => return Some(Self::Float64),
            _ => {}
        }
        let rest = kind.strip_prefix("enum(")?;
        let inner = rest.strip_suffix(')')?;
        let (inner, explicit_default) = match inner.split_once("@default=") {
            Some((head, text)) if is_upper_snake(text) => (head, Some(text.to_owned())),
            Some(_) => return None,
            None => (inner, None),
        };
        let mut values: Vec<(i32, String)> = Vec::new();
        for entry in inner.split(',') {
            let (num, text) = entry.split_once('=')?;
            let num: i32 = num.trim().parse().ok()?;
            let text = text.trim().to_owned();
            if !is_upper_snake(&text) || values.iter().any(|(seen, _)| *seen == num) {
                return None;
            }
            values.push((num, text));
        }
        if values.is_empty() || !values.iter().any(|(num, _)| *num == 0) {
            return None;
        }
        let zero_text = values
            .iter()
            .find(|(num, _)| *num == 0)
            .expect("已校验含 0")
            .1
            .clone();
        if values.iter().filter(|(_, text)| *text == zero_text).count() > 1 {
            return None;
        }
        let default = explicit_default.unwrap_or(zero_text);
        if !values.iter().any(|(_, text)| *text == default) {
            return None;
        }
        Some(Self::Enum(EnumValues { values, default }))
    }

    /// proto 字段类型（枚举字段的类型名 = 字段名的 Pascal 形）。
    fn proto_ty(kind: &Self, field_name: &str) -> String {
        match kind {
            Self::String => "string".to_owned(),
            Self::Int32 => "int32".to_owned(),
            Self::Uint32 => "uint32".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Float64 => "double".to_owned(),
            Self::Enum(_) => pascal_of(field_name),
        }
    }

    fn entity_ty(&self) -> &'static str {
        match self {
            Self::String | Self::Enum(_) => "String",
            Self::Int32 => "i32",
            Self::Uint32 => "u32",
            Self::Bool => "bool",
            Self::Float64 => "f64",
        }
    }

    /// 规格串：`FieldKind::parse` 的合法输入，往返恒等（@default 与
    /// 0 值文本相同时省略——那是 parse 的缺省语义）。
    pub fn spec_string(&self) -> String {
        match self {
            Self::String => "string".to_owned(),
            Self::Int32 => "i32".to_owned(),
            Self::Uint32 => "u32".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Float64 => "f64".to_owned(),
            Self::Enum(values) => {
                let mut out = String::from("enum(");
                for (index, (num, text)) in values.values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&format!("{num}={text}"));
                }
                let zero_text = values
                    .values
                    .iter()
                    .find(|(num, _)| *num == 0)
                    .map(|(_, text)| text);
                if Some(&values.default) != zero_text {
                    out.push_str(&format!("@default={}", values.default));
                }
                out.push(')');
                out
            }
        }
    }
}

/// 一个业务字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpec {
    pub name: String,
    pub kind: FieldKind,
}

/// `rush gen entity` 选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityOptions {
    /// rushwind-admin 仓库根目录。
    pub repo_root: PathBuf,
    /// 实体名，snake_case 单数（如 `widget`）。
    pub name: String,
    /// 表名；缺省 `sys_<复数>`。
    pub table: Option<String>,
    /// 消息面 package；缺省 `<name>.service.v1`。
    pub package: Option<String>,
    /// 路由前缀；缺省 `/admin/v1/<复数>`。
    pub route_prefix: Option<String>,
    /// 业务字段。
    pub fields: Vec<FieldSpec>,
    /// 唯一编码字段名（须在 fields 中且为 string）：启用 Get 的 code 臂与
    /// `/code/{code}` 附加路由（dict_type 同款）。
    pub code_field: Option<String>,
    /// 平台全局表：全链去租户（无 tenant 列/过滤，repo 走 global 臂）。
    pub global: bool,
    /// 生成后执行 `cargo check -p admin-api` 验证。
    pub check: bool,
    /// 只报告不落盘。
    pub dry_run: bool,
    /// 跳过 proto MANIFEST 重建。
    pub skip_manifest: bool,
}

/// 生成结果报告。
#[derive(Debug, Default, Serialize)]
pub struct EntityReport {
    pub created: Vec<PathBuf>,
    pub edited: Vec<PathBuf>,
    pub skipped: Vec<String>,
    pub manifest_entries: Option<usize>,
    /// `--check` 的结果：None 未请求；Some(true) 编译通过。
    pub check_passed: Option<bool>,
    pub notes: Vec<String>,
}

/// 派生好的命名全集。
struct EntitySpec {
    name: String,
    pascal: String,
    table: String,
    package: String,
    pkg_mod: String,
    proto_dir: String,
    route_prefix: String,
    fields: Vec<FieldSpec>,
    code_field: Option<String>,
    global: bool,
}

pub fn pascal_of(snake: &str) -> String {
    snake
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut cs = part.chars();
            match cs.next() {
                Some(first) => first.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

pub fn plural_of(snake: &str) -> String {
    if snake.ends_with('s')
        || snake.ends_with('x')
        || snake.ends_with('z')
        || snake.ends_with("ch")
        || snake.ends_with("sh")
    {
        format!("{snake}es")
    } else if snake.ends_with('y')
        && snake
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| !"aeiou".contains(c))
    {
        format!("{}ies", &snake[..snake.len() - 1])
    } else {
        format!("{snake}s")
    }
}

pub fn camel_of(snake: &str) -> String {
    let pascal = pascal_of(snake);
    let mut cs = pascal.chars();
    match cs.next() {
        Some(first) => first.to_lowercase().collect::<String>() + cs.as_str(),
        None => pascal,
    }
}

/// snake_case 校验（生成器各入口共用的输入约束）。
pub fn is_snake(s: &str) -> bool {
    let mut cs = s.chars();
    let first_ok = cs.next().is_some_and(|c| c.is_ascii_lowercase());
    first_ok
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn validate(opts: &EntityOptions) -> Result<EntitySpec> {
    if !is_snake(&opts.name) {
        return Err(Error::InvalidInput(format!(
            "实体名必须是 snake_case：{}",
            opts.name
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for field in &opts.fields {
        if !is_snake(&field.name) {
            return Err(Error::InvalidInput(format!(
                "字段名必须是 snake_case：{}",
                field.name
            )));
        }
        if !seen.insert(field.name.clone()) {
            return Err(Error::InvalidInput(format!("字段重复：{}", field.name)));
        }
    }
    let code_field = match &opts.code_field {
        Some(code_name) => {
            let field = opts
                .fields
                .iter()
                .find(|field| &field.name == code_name)
                .ok_or_else(|| {
                    Error::InvalidInput(format!("--code-field 不在字段列表中：{code_name}"))
                })?;
            if field.kind != FieldKind::String {
                return Err(Error::InvalidInput(format!(
                    "--code-field 必须是 string 类型：{code_name}"
                )));
            }
            Some(code_name.clone())
        }
        None => None,
    };
    let plural = plural_of(&opts.name);
    let table = match &opts.table {
        Some(t) if !t.is_empty() => t.clone(),
        _ => format!("sys_{plural}"),
    };
    let is_snake_table = table.split('_').all(|part| {
        part.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    if !is_snake_table || table.is_empty() {
        return Err(Error::InvalidInput(format!("表名非法：{table}")));
    }
    let package = match &opts.package {
        Some(p) if !p.is_empty() => p.clone(),
        _ => format!("{}.service.v1", opts.name),
    };
    let pkg_mod = package.replace('.', "::");
    let proto_dir = package.replace('.', "/");
    let route_prefix = match &opts.route_prefix {
        Some(r) if !r.is_empty() => r.trim_end_matches('/').to_owned(),
        _ => format!("/admin/v1/{plural}"),
    };
    Ok(EntitySpec {
        pascal: pascal_of(&opts.name),
        name: opts.name.clone(),
        table,
        package,
        pkg_mod,
        proto_dir,
        route_prefix,
        fields: opts.fields.clone(),
        code_field,
        global: opts.global,
    })
}

/// 生成实体链。新文件拒绝覆盖；既有文件的手术插入幂等。
pub fn generate_entity(opts: &EntityOptions) -> Result<EntityReport> {
    let spec = validate(opts)?;
    let root = &opts.repo_root;
    let mut report = EntityReport::default();

    // ---- 新文件（拒绝覆盖） ----
    let files: Vec<(PathBuf, String)> = vec![
        (
            root.join(format!(
                "backend/api/protos/{}/{}.proto",
                spec.proto_dir, spec.name
            )),
            message_proto(&spec),
        ),
        (
            root.join(format!(
                "backend/api/protos/admin/service/v1/i_{}.proto",
                spec.name
            )),
            admin_proto(&spec),
        ),
        (
            root.join(format!(
                "backend/services/admin-api/src/data/{}.rs",
                spec.table
            )),
            entity_file(&spec),
        ),
        (
            root.join(format!(
                "backend/services/admin-api/src/data/repos/{}.rs",
                spec.name
            )),
            repo_file(&spec),
        ),
        (
            root.join(format!(
                "backend/services/admin-api/src/services/{}.rs",
                spec.name
            )),
            service_file(&spec),
        ),
    ];
    for (path, _) in &files {
        if path.exists() {
            return Err(Error::InvalidInput(format!(
                "文件已存在：{}",
                path.display()
            )));
        }
    }

    // ---- 既有文件的编辑计划（锚点在写入前全部校验，避免半套生成） ----
    let edits: Vec<(PathBuf, String, String)> = vec![
        (
            root.join("backend/services/admin-api/src/data.rs"),
            format!("pub mod {t};", t = spec.table),
            String::new(),
        ),
        (
            root.join("backend/services/admin-api/src/migration.rs"),
            format!("            .table::<data::{}::Entity>()", spec.table),
            String::new(),
        ),
        (
            root.join("backend/services/admin-api/src/data/repos/mod.rs"),
            format!("mod {};", spec.name),
            format!("pub use {}::{}Repo;", spec.name, spec.pascal),
        ),
        (
            root.join("backend/services/admin-api/src/services.rs"),
            format!("mod {};", spec.name),
            format!("pub use {}::{}Service;", spec.name, spec.pascal),
        ),
        (
            root.join("backend/services/admin-api/src/server/rest.rs"),
            format!(
                "        (mount_{}_service, {}Service),",
                spec.name, spec.pascal
            ),
            String::new(),
        ),
        (
            root.join("backend/services/admin-api/src/server/rest.rs"),
            format!("{}Service,", spec.pascal),
            String::new(),
        ),
    ];
    for (path, _, _) in &edits {
        if !path.is_file() {
            return Err(Error::InvalidInput(format!(
                "编辑锚点文件缺失：{}（目标仓不是 rushwind-admin 形状？）",
                path.display()
            )));
        }
    }

    if opts.dry_run {
        report.created = files.iter().map(|(p, _)| p.clone()).collect();
        report
            .created
            .push(crate::spec::spec_path(root, &spec.name));
        let mut edited: Vec<PathBuf> = Vec::new();
        for (path, _, _) in &edits {
            if !edited.contains(path) {
                edited.push(path.clone());
            }
        }
        report.edited = edited;
        if !opts.skip_manifest {
            let tree = Flavor::Proto.tree_path(root);
            report.manifest_entries = Some(
                manifest::build_manifest(&tree, Flavor::Proto)?
                    .lines()
                    .count(),
            );
        }
        return Ok(report);
    }

    for (path, content) in &files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        report.created.push(path.clone());
    }

    // data.rs：pub mod 行（实体文件平铺，无包装模块）
    let data_rs = &edits[0];
    apply_sorted_edit(
        &mut report,
        &data_rs.0,
        &data_rs.1,
        spec.table.as_str(),
        |line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("mod ") || trimmed.starts_with("pub mod ")
        },
        mod_key,
    )?;

    // migration.rs：EntityTables 链尾（.build() 前）插入
    let migration_rs = &edits[1];
    apply_contains_edit(&mut report, &migration_rs.0, &migration_rs.1, |text| {
        insert_before_line(text, ".build();", &migration_rs.1)
    })?;

    // repos/mod.rs：mod 行 + pub use 行
    let repos_mod = &edits[2];
    apply_sorted_edit(
        &mut report,
        &repos_mod.0,
        &repos_mod.1,
        spec.name.as_str(),
        |line| line.trim_start().starts_with("mod "),
        mod_key,
    )?;
    apply_sorted_edit(
        &mut report,
        &repos_mod.0,
        &repos_mod.2,
        spec.name.as_str(),
        |line| line.trim_start().starts_with("pub use "),
        pub_use_key,
    )?;

    // services.rs：mod 行 + pub use 行
    let services_rs = &edits[3];
    apply_sorted_edit(
        &mut report,
        &services_rs.0,
        &services_rs.1,
        spec.name.as_str(),
        |line| line.trim_start().starts_with("mod "),
        mod_key,
    )?;
    apply_sorted_edit(
        &mut report,
        &services_rs.0,
        &services_rs.2,
        spec.name.as_str(),
        |line| line.trim_start().starts_with("pub use "),
        pub_use_key,
    )?;

    // rest.rs：先插入 use 导入（守卫标记不与 mount 行冲突），再插 mount 表
    let rest_rs = &edits[4];
    apply_contains_edit(&mut report, &rest_rs.0, &edits[5].1, |text| {
        insert_service_import(text, &format!("{}Service", spec.pascal))
    })?;
    apply_contains_edit(&mut report, &rest_rs.0, &rest_rs.1, |text| {
        insert_mount_entry(text, &spec.name, &spec.pascal)
    })?;

    // 规格文件：字段清单的唯一真相（gen pages 免重输 --field、UI 表单
    // 回填都以它为数据源）。全链生成成功后才落盘。
    let spec_file = crate::spec::from_parts(
        &spec.name,
        &spec.table,
        &spec.package,
        &spec.route_prefix,
        &spec.fields,
        spec.code_field.as_deref(),
        spec.global,
    );
    let spec_path = crate::spec::save(&spec_file, root)?;
    report.created.push(spec_path);

    if !opts.skip_manifest {
        let tree = Flavor::Proto.tree_path(root);
        let path = Flavor::Proto.manifest_path(root);
        report.manifest_entries = Some(manifest::rebuild(&tree, &path, Flavor::Proto)?);
    }

    if opts.check {
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["check", "-q", "-p", "admin-api"])
            .current_dir(root.join("backend"))
            .status();
        match status {
            Ok(status) if status.success() => report.check_passed = Some(true),
            Ok(status) => {
                report.check_passed = Some(false);
                report.notes.push(format!(
                    "--check：cargo check 退出码 {:?}——生成物未通过编译，请核对错误输出",
                    status.code()
                ));
            }
            Err(err) => {
                report.check_passed = Some(false);
                report
                    .notes
                    .push(format!("--check：cargo 不可用，跳过编译验证（{err}）"));
            }
        }
    }

    report.notes.push(
        "已有数据的库：init 迁移（m20250915_000001_init）不会重放，新表需手工建表或在 migration.rs 追加独立迁移条目".to_owned(),
    );
    report.notes.push(
        "三套前端页面未生成：React 面用 rush gen pages（菜单种子随其一并写入 seed.rs）".to_owned(),
    );
    report
        .notes
        .push("生成后建议在 backend/ 下运行 cargo fmt（导入折行与长行由 rustfmt 归位）".to_owned());
    report.notes.push(
        "新路由默认走鉴权门；如需免鉴权，把服务路由加入 crates/proto/src/auth_free.rs".to_owned(),
    );
    report
        .notes
        .push("testbed corpus/exemptions 未更新（如需差分覆盖请手动补充）".to_owned());
    Ok(report)
}

fn pub_use_key(line: &str) -> Option<String> {
    line.strip_prefix("pub use ")?
        .split("::")
        .next()
        .map(str::to_owned)
}

/// `mod x;` / `pub mod x;` 的排序键。
fn mod_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("pub mod ")
        .or(trimmed.strip_prefix("mod "))?;
    rest.strip_suffix(';').map(str::to_owned)
}

fn apply_sorted_edit(
    report: &mut EntityReport,
    path: &Path,
    new_line: &str,
    new_key: &str,
    line_matches: impl Fn(&str) -> bool,
    key_of: impl Fn(&str) -> Option<String>,
) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let already = text.lines().any(|line| line.trim() == new_line);
    if already {
        report
            .skipped
            .push(format!("{}（已存在该行）", path.display()));
        return Ok(());
    }
    let edited =
        insert_sorted(&text, line_matches, key_of, new_line, new_key).ok_or_else(|| {
            Error::InvalidInput(format!(
                "{} 中找不到锚点行（新行：{new_line}）",
                path.display()
            ))
        })?;
    fs::write(path, &edited)?;
    if !report.edited.contains(&path.to_path_buf()) {
        report.edited.push(path.to_path_buf());
    }
    Ok(())
}

fn apply_contains_edit(
    report: &mut EntityReport,
    path: &Path,
    marker: &str,
    edit: impl Fn(&str) -> Option<String>,
) -> Result<()> {
    let text = fs::read_to_string(path)?;
    if text.contains(marker) {
        report
            .skipped
            .push(format!("{}（已存在该行）", path.display()));
        return Ok(());
    }
    let edited = edit(&text)
        .ok_or_else(|| Error::InvalidInput(format!("{} 中找不到插入锚点", path.display())))?;
    fs::write(path, &edited)?;
    if !report.edited.contains(&path.to_path_buf()) {
        report.edited.push(path.to_path_buf());
    }
    Ok(())
}

/// 在匹配 `match_prefix` 的行中按 key 升序找第一个更大的行插入其前；
/// 全部更小则插到最后一行匹配行之后。返回 None 表示没有锚点行。
fn insert_sorted(
    text: &str,
    line_matches: impl Fn(&str) -> bool,
    key_of: impl Fn(&str) -> Option<String>,
    new_line: &str,
    new_key: &str,
) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let anchors: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line_matches(line))
        .filter_map(|(index, line)| key_of(line).map(|key| (index, key)))
        .collect();
    if anchors.is_empty() {
        return None;
    }
    let insert_at = anchors
        .iter()
        .find(|(_, key)| key.as_str() > new_key)
        .map(|(index, _)| *index)
        .unwrap_or_else(|| anchors.last().expect("非空").0 + 1);
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..insert_at]);
    out.push(new_line);
    out.extend_from_slice(&lines[insert_at..]);
    Some(out.join("\n"))
}

/// 在第一个含 `needle` 的行之前插入 `line`（保持该行原有缩进推断交给调用方）。
fn insert_before_line(text: &str, needle: &str, line: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let at = lines.iter().position(|l| l.contains(needle))?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..at]);
    out.push(line);
    out.extend_from_slice(&lines[at..]);
    Some(out.join("\n"))
}

/// mount_services! 表的字母序插入。多行表项的首行是孤立的 `(`，其
/// mount 标识在下一行——提取时向右看一行，插入时落在 `(` 行之前。
fn insert_mount_entry(text: &str, name: &str, pascal: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let new_entry = format!("        (mount_{name}_service, {pascal}Service),");
    let start = lines
        .iter()
        .position(|line| line.contains("mount_services!"))?;
    let mut end = start + 1;
    let mut insert_at = None;
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        if line.trim_start().starts_with(");") {
            end = cursor;
            break;
        }
        let key = extract_mount_id(line).or_else(|| {
            lines
                .get(cursor + 1)
                .and_then(|next| extract_mount_id(next))
        });
        if let Some(key) = key {
            let target = format!("mount_{name}_service");
            if key.as_str() > target.as_str() {
                insert_at = Some(cursor);
                break;
            }
        }
        cursor += 1;
    }
    let insert_at = insert_at.unwrap_or(end);
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..insert_at]);
    out.push(&new_entry);
    out.extend_from_slice(&lines[insert_at..]);
    Some(out.join("\n"))
}

fn extract_mount_id(line: &str) -> Option<String> {
    let byte = line.find("mount_")?;
    let rest = &line[byte..];
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    (id.ends_with("_service")).then_some(id)
}

/// `use crate::services::{…}` 导入块的字母序插入。rustfmt 折行后一行
/// 携带多个导入名，需要在行内定位第一个更大的名字做词级拼接；全块
/// 更小则作为新行插到关闭括号前。
fn insert_service_import(text: &str, service: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("use crate::services::{"))?;
    if lines[start].contains('}') {
        // 单行形式不做行内词级插入（真实文件为 rustfmt 折行形态）
        return None;
    }
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        if line.trim_start().starts_with("};") {
            break;
        }
        if let Some(at) = first_greater_import_byte(line, service) {
            let spliced = format!("{}{}, {}", &line[..at], service, &line[at..]);
            let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
            out.extend_from_slice(&lines[..cursor]);
            out.push(&spliced);
            out.extend_from_slice(&lines[cursor + 1..]);
            return Some(out.join("\n"));
        }
        cursor += 1;
    }
    let new_line = format!("    {service},");
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..cursor]);
    out.push(&new_line);
    out.extend_from_slice(&lines[cursor..]);
    Some(out.join("\n"))
}

/// 行内按逗号分词，返回第一个（字符串序）大于 `service` 的导入名的
/// 字节偏移。
fn first_greater_import_byte(line: &str, service: &str) -> Option<usize> {
    let mut byte = 0;
    for token in line.split(',') {
        let name = token.trim();
        if !name.is_empty() && name > service {
            return Some(line[byte..].find(name)? + byte);
        }
        byte += token.len() + 1; // '+1' 为分隔逗号
    }
    None
}

// ---- 模板 ----

fn proto_field_row(ty: &str, name: &str, num: u32) -> String {
    format!(
        "  optional {ty} {name} = {num} [\n    json_name = \"{json}\",\n    (gnostic.openapi.v3.property) = {{description: \"{name}\"}}\n  ]; // {name}\n",
        ty = ty,
        name = name,
        num = num,
        json = camel_of(name),
    )
}

fn proto_field(field: &FieldSpec, num: u32) -> String {
    proto_field_row(
        &FieldKind::proto_ty(&field.kind, &field.name),
        &field.name,
        num,
    )
}

/// 消息体内的嵌套 enum 块（每个枚举字段一个，置于字段声明之前）。
fn proto_enums(spec: &EntitySpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        if let FieldKind::Enum(values) = &field.kind {
            out.push_str(&format!(
                "  // {} 取值集（文本列枚举：{}）\n  enum {} {{\n",
                field.name,
                values.default,
                pascal_of(&field.name)
            ));
            for (num, text) in &values.values {
                out.push_str(&format!("    {text} = {num};\n"));
            }
            out.push_str("  }\n\n");
        }
    }
    out
}

fn message_proto(spec: &EntitySpec) -> String {
    let mut fields = String::new();
    fields.push_str(
        "  optional uint32 id = 1 [\n    json_name = \"id\",\n    (gnostic.openapi.v3.property) = {description: \"ID\"}\n  ]; // ID\n\n",
    );
    let mut num = 2u32;
    for field in &spec.fields {
        fields.push_str(&proto_field(field, num));
        fields.push('\n');
        num += 1;
    }
    // 标准尾段字段：None 表示 google.protobuf.Timestamp（不用 FieldKind 表达）；
    // 全局表无租户两列。
    let tail: [(&str, Option<FieldKind>); 9] = [
        ("sort_order", Some(FieldKind::Uint32)),
        ("tenant_id", Some(FieldKind::Uint32)),
        ("tenant_name", Some(FieldKind::String)),
        ("created_by", Some(FieldKind::Uint32)),
        ("updated_by", Some(FieldKind::Uint32)),
        ("deleted_by", Some(FieldKind::Uint32)),
        ("created_at", None),
        ("updated_at", None),
        ("deleted_at", None),
    ];
    for (name, kind) in tail {
        if spec.global && (name == "tenant_id" || name == "tenant_name") {
            continue;
        }
        let block = match kind {
            Some(kind) => proto_field_row(&FieldKind::proto_ty(&kind, name), name, num),
            None => format!(
                "  optional google.protobuf.Timestamp {name} = {num} [json_name = \"{json}\", (gnostic.openapi.v3.property) = {{description: \"{name}\"}}]; // {name}\n",
                name = name,
                num = num,
                json = camel_of(name),
            ),
        };
        fields.push_str(&block);
        fields.push('\n');
        num += 1;
    }
    let code_arm = if spec.code_field.is_some() {
        "    string code = 2;\n"
    } else {
        ""
    };

    format!(
        r#"syntax = "proto3";

package {package};

import "gnostic/openapi/v3/annotations.proto";

import "google/protobuf/empty.proto";
import "google/protobuf/timestamp.proto";
import "google/protobuf/field_mask.proto";

import "pagination/v1/pagination.proto";

// {pascal} 服务（消息面——无 HTTP 注解；admin 面见 admin/service/v1/i_{name}.proto）
service {pascal}Service {{
  rpc List (pagination.PagingRequest) returns (List{pascal}Response) {{}}

  rpc Count (pagination.PagingRequest) returns (Count{pascal}Response) {{}}

  rpc Get (Get{pascal}Request) returns ({pascal}) {{}}

  rpc Create (Create{pascal}Request) returns (google.protobuf.Empty) {{}}

  rpc Update (Update{pascal}Request) returns (google.protobuf.Empty) {{}}

  rpc Delete (Delete{pascal}Request) returns (google.protobuf.Empty) {{}}
}}

// {pascal}
message {pascal} {{
{enums}{fields}}}

// 查询{pascal}列表 - 回应
message List{pascal}Response {{
  repeated {pascal} items = 1;
  uint64 total = 2;
}}

// 查询{pascal}详情 - 请求
message Get{pascal}Request {{
  oneof query_by {{
    uint32 id = 1;
{code_arm}  }}

  optional google.protobuf.FieldMask view_mask = 100 [
    json_name = "viewMask",
    (gnostic.openapi.v3.property) = {{
      description: "视图字段过滤器，用于控制返回的字段"
    }}
  ]; // 视图字段过滤器，用于控制返回的字段
}}

// 创建{pascal} - 请求
message Create{pascal}Request {{
  {pascal} data = 1;
}}

// 更新{pascal} - 请求
message Update{pascal}Request {{
  uint32 id = 1;

  {pascal} data = 2;

  google.protobuf.FieldMask update_mask = 3 [
    (gnostic.openapi.v3.property) = {{
      description: "要更新的字段列表"
    }},
    json_name = "updateMask"
  ]; // 要更新的字段列表

  optional bool allow_missing = 4 [
    (gnostic.openapi.v3.property) = {{description: "资源不存在时是否新增"}},
    json_name = "allowMissing"
  ]; // 资源不存在时是否新增
}}

// 批量删除{pascal} - 请求
message Delete{pascal}Request {{
  repeated uint32 ids = 1;  // 要删除的ID列表
}}

message Count{pascal}Response {{
  uint64 count = 1;  // {pascal}数量
}}
"#,
        package = spec.package,
        pascal = spec.pascal,
        name = spec.name,
        enums = proto_enums(spec),
        fields = fields,
        code_arm = code_arm,
    )
}

fn admin_proto(spec: &EntitySpec) -> String {
    let get_bindings = if spec.code_field.is_some() {
        format!(
            "      additional_bindings {{\n        get: \"{}/code/{{code}}\"\n      }}\n",
            spec.route_prefix
        )
    } else {
        String::new()
    };
    format!(
        r#"syntax = "proto3";

package admin.service.v1;

import "google/api/annotations.proto";
import "google/protobuf/empty.proto";

import "pagination/v1/pagination.proto";
import "{proto_dir}/{name}.proto";

// {pascal} 管理服务
service {pascal}Service {{
  // 分页查询{pascal}列表
  rpc List (pagination.PagingRequest) returns ({pkg}.List{pascal}Response) {{
    option (google.api.http) = {{
      get: "{route_prefix}"
    }};
  }}

  // 查询{pascal}详情
  rpc Get ({pkg}.Get{pascal}Request) returns ({pkg}.{pascal}) {{
    option (google.api.http) = {{
      get: "{route_prefix}/{{id}}"
{get_bindings}    }};
  }}

  // 创建{pascal}
  rpc Create ({pkg}.Create{pascal}Request) returns (google.protobuf.Empty) {{
    option (google.api.http) = {{
      post: "{route_prefix}"
      body: "*"
    }};
  }}

  // 更新{pascal}
  rpc Update ({pkg}.Update{pascal}Request) returns (google.protobuf.Empty) {{
    option (google.api.http) = {{
      put: "{route_prefix}/{{id}}"
      body: "*"
    }};
  }}

  // 删除{pascal}
  rpc Delete ({pkg}.Delete{pascal}Request) returns (google.protobuf.Empty) {{
    option (google.api.http) = {{
      delete: "{route_prefix}"
    }};
  }}
}}
"#,
        proto_dir = spec.proto_dir,
        name = spec.name,
        pascal = spec.pascal,
        pkg = spec.package,
        route_prefix = spec.route_prefix,
        get_bindings = get_bindings,
    )
}

fn entity_fields(spec: &EntitySpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        // code 列带 unique：建表派生（EntityTables）读 #[sea_orm(unique)]
        // 生成 UNIQUE 约束，code 臂查询的唯一性前提落在 schema 上，而非
        // 仅靠运行时约定。
        if spec
            .code_field
            .as_deref()
            .map(|code| code == field.name)
            .unwrap_or(false)
        {
            out.push_str("        #[sea_orm(unique)]\n");
        }
        if let FieldKind::Enum(values) = &field.kind {
            let texts: Vec<String> = values.values.iter().map(|(_, text)| text.clone()).collect();
            out.push_str(&format!(
                "    /// enum({}) default {}.\n",
                texts.join(","),
                values.default
            ));
        }
        if field.kind == FieldKind::String {
            out.push_str(&format!("        pub {}: String,\n", field.name));
        } else {
            out.push_str(&format!(
                "        pub {}: Option<{}>,\n",
                field.name,
                field.kind.entity_ty()
            ));
        }
    }
    out
}

fn entity_file(spec: &EntitySpec) -> String {
    let tenant_line = if spec.global {
        ""
    } else {
        "    pub tenant_id: Option<u32>,\n"
    };
    format!(
        r#"//! {table} — 由 rush gen entity 生成的实体模块。

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "{table}")]
pub struct Model {{
    #[sea_orm(primary_key)]
    pub id: u32,
{tenant_line}{business}    #[sea_orm(default_value = 0)]
    pub sort_order: Option<u32>,
    pub created_by: Option<u32>,
    pub updated_by: Option<u32>,
    pub deleted_by: Option<u32>,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<chrono::NaiveDateTime>,
}}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {{}}

impl ActiveModelBehavior for ActiveModel {{}}
"#,
        table = spec.table,
        tenant_line = tenant_line,
        business = entity_fields(spec),
    )
}

fn repo_file(spec: &EntitySpec) -> String {
    let (shell_arm, viewer_import, delete_body) = if spec.global {
        (
            "global",
            "",
            "        let delete =\n            entity::Entity::delete_many()\n                .filter(entity::Column::Id.is_in(ids.to_vec()));\n        delete.exec(self.db).await.map_err(db_err)?;\n        Ok(())\n",
        )
    } else {
        (
            "tenant",
            "use crate::data::Viewer;\n",
            "        let mut delete = entity::Entity::delete_many()\n            .filter(entity::Column::Id.is_in(ids.to_vec()));\n        if let Some(tid) = self.viewer.tenant_scope() {\n            delete = delete.filter(entity::Column::TenantId.eq(tid));\n        }\n        delete.exec(self.db).await.map_err(db_err)?;\n        Ok(())\n",
        )
    };
    format!(
        r#"//! {pascal}Repo — queries with
//! all predicates owned here (never ad-hoc in services).

use sea_orm::sea_query::Condition;
use sea_orm::{{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter}};

use crate::data::{table} as entity;
{viewer_import}use crate::state::{{db_err, StatusError}};

repo_shell!({shell_arm} {pascal}Repo, entity);

impl<'a> {pascal}Repo<'a> {{
    /// Delete = BatchDelete semantics.
    pub async fn delete_batch(&self, ids: &[u32]) -> Result<(), StatusError> {{
{delete_body}    }}
}}
"#,
        pascal = spec.pascal,
        table = spec.table,
        shell_arm = shell_arm,
        viewer_import = viewer_import,
        delete_body = delete_body,
    )
}

/// 每个枚举字段一对转换函数（proto i32 ↔ 文本列），置于 mapper 之前。
fn enum_helpers(spec: &EntitySpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        if let FieldKind::Enum(values) = &field.kind {
            let snake = field.name.clone();
            out.push_str(&format!(
                "fn {snake}_to_text(v: i32) -> String {{\n    match v {{\n"
            ));
            for (num, text) in &values.values {
                out.push_str(&format!("        {num} => \"{text}\".to_string(),\n"));
            }
            out.push_str(&format!(
                "        _ => \"{}\".to_string(),\n    }}\n}}\n\n",
                values.default
            ));
            out.push_str(&format!(
                "fn {snake}_from_text(value: Option<&str>) -> i32 {{\n    match value {{\n"
            ));
            for (num, text) in &values.values {
                if *num != 0 {
                    out.push_str(&format!("        Some(\"{text}\") => {num},\n"));
                }
            }
            out.push_str("        _ => 0,\n    }\n}\n\n");
        }
    }
    out
}

fn mapper_fields(spec: &EntitySpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        match &field.kind {
            FieldKind::String => {
                out.push_str(&format!(
                    "        {}: Some(r.{}),\n",
                    field.name, field.name
                ));
            }
            FieldKind::Enum(_) => {
                out.push_str(&format!(
                    "        {}: Some({}_from_text(r.{}.as_deref())),\n",
                    field.name, field.name, field.name
                ));
            }
            _ => {
                out.push_str(&format!("        {}: r.{},\n", field.name, field.name));
            }
        }
    }
    out
}

fn create_fields(spec: &EntitySpec) -> String {
    let mut out = String::new();
    if !spec.global {
        out.push_str("            tenant_id: Set(Some(payload.tenant_id)),\n");
    }
    for field in &spec.fields {
        match &field.kind {
            FieldKind::String => out.push_str(&format!(
                "            {}: Set(data.{}.unwrap_or_default()),\n",
                field.name, field.name
            )),
            FieldKind::Enum(_) => out.push_str(&format!(
                "            {}: Set(Some({}_to_text(data.{}.unwrap_or(0)))),\n",
                field.name, field.name, field.name
            )),
            _ => out.push_str(&format!(
                "            {}: Set(data.{}),\n",
                field.name, field.name
            )),
        }
    }
    out
}

fn update_fields(spec: &EntitySpec) -> String {
    spec.fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::String => format!(
                "            if let Some(v) = &data.{} {{\n                a.{} = Set(v.clone());\n            }}\n",
                field.name, field.name
            ),
            FieldKind::Enum(_) => format!(
                "            if let Some(v) = data.{} {{\n                a.{} = Set(Some({}_to_text(v)));\n            }}\n",
                field.name, field.name, field.name
            ),
            _ => format!(
                "            if let Some(v) = data.{} {{\n                a.{} = Set(Some(v));\n            }}\n",
                field.name, field.name
            ),
        })
        .collect()
}

fn service_file(spec: &EntitySpec) -> String {
    let has_code = spec.code_field.is_some();
    // sea_orm 导入按实际使用面裁剪（全局表无租户过滤时不引 ColumnTrait/QueryFilter）
    let sea_imports = if has_code {
        "use sea_orm::sea_query::Condition;\nuse sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};"
    } else if spec.global {
        "use sea_orm::{ActiveModelTrait, EntityTrait, Set};"
    } else {
        "use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};"
    };
    let data_import = if spec.global {
        format!("use crate::data::{};", spec.table)
    } else {
        format!("use crate::data::{{Viewer, {}}};", spec.table)
    };
    let state_imports = if has_code {
        "db_err, not_found, operator_of, status_error, tenant_of, AppState, StatusError"
    } else {
        "db_err, not_found, operator_of, status_error, AppState, StatusError"
    };
    let viewer_arg = if spec.global {
        ""
    } else {
        ", Viewer::from_ctx(&ctx)"
    };
    let tenant_row = if spec.global {
        ""
    } else {
        "        tenant_id: r.tenant_id,\n"
    };
    let list_ctx = if spec.global {
        "        let _ = ctx;\n"
    } else {
        ""
    };
    let delete_ctx = list_ctx;
    let tenant_name_row = if spec.global {
        ""
    } else {
        "        tenant_name: None,\n"
    };
    let update_filter = if spec.global {
        String::new()
    } else {
        format!(
            "            .filter({}::Column::TenantId.eq(payload.tenant_id))\n",
            spec.table
        )
    };
    let code_column = spec
        .code_field
        .as_deref()
        .map(pascal_of)
        .unwrap_or_default();
    let tenant_add = if spec.global {
        String::new()
    } else {
        format!(
            "                            .add({}::Column::TenantId.eq(tenant))\n",
            spec.table
        )
    };
    let get_body = if has_code {
        format!(
            r#"        let tenant = tenant_of(&ctx);
        let id = match req.query_by {{
            Some(proto::proto::{pkg_mod}::get_{name}_request::QueryBy::Id(id)) => id,
            Some(proto::proto::{pkg_mod}::get_{name}_request::QueryBy::Code(code)) => {{
                // Code 臂：租户内唯一编码换 id（dict_type 同款）。
                {table}::Entity::find()
                    .filter(
                        Condition::all()
{tenant_add}                            .add({table}::Column::{code_column}.eq(code)),
                    )
                    .one(&self.state.db)
                    .await
                    .map_err(db_err)?
                    .map(|r| r.id)
                    .ok_or_else(|| not_found("{name}"))?
            }}
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        }};
        let row = {table}::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("{name}"))?;
        Ok({name}_proto(row))
    }}
"#,
            pkg_mod = spec.pkg_mod,
            name = spec.name,
            table = spec.table,
            tenant_add = tenant_add,
            code_column = code_column,
        )
    } else {
        format!(
            r#"        let _ = ctx;
        let id = match req.query_by {{
            Some(proto::proto::{pkg_mod}::get_{name}_request::QueryBy::Id(id)) => id,
            None => return Err(status_error("BAD_REQUEST", "query_by required")),
        }};
        let row = {table}::Entity::find_by_id(id)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("{name}"))?;
        Ok({name}_proto(row))
    }}
"#,
            pkg_mod = spec.pkg_mod,
            name = spec.name,
            table = spec.table,
        )
    };

    format!(
        r#"//! {pascal}Service — service layer for module

use std::sync::Arc;

{sea_imports}

use crate::data::repos::{pascal}Repo;
{data_import}
use crate::state::{{
    {state_imports},
}};
use pbjson_types::Empty;
use proto::proto::{pkg_mod}::{{
    Create{pascal}Request, Delete{pascal}Request, {pascal}, Get{pascal}Request,
    List{pascal}Response, Update{pascal}Request,
}};
use proto::proto::pagination::PagingRequest;

{helpers}fn {name}_proto(r: {table}::Model) -> {pascal} {{
    {pascal} {{
        id: Some(r.id),
{tenant_row}{mapper}        sort_order: r.sort_order,
        created_by: r.created_by,
        updated_by: r.updated_by,
        deleted_by: r.deleted_by,
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
{tenant_name_row}    }}
}}

pub struct {pascal}Service {{
    pub state: Arc<AppState>,
}}

#[async_trait::async_trait]
impl proto::gen::services::{pascal}ServiceHandlers for {pascal}Service {{
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<List{pascal}Response, StatusError> {{
{list_ctx}        // Listing rides the repo: tenancy predicates live in the data layer.
        let repo = {pascal}Repo::new(&self.state.db{viewer_arg});
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(List{pascal}Response {{
            items: rows.into_iter().map({name}_proto).collect(),
            total,
        }})
    }}

    async fn get(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: Get{pascal}Request,
    ) -> Result<{pascal}, StatusError> {{
{get_body}
    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: Create{pascal}Request,
    ) -> Result<Empty, StatusError> {{
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        {table}::ActiveModel {{
{create}            sort_order: Set(Some(0)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }}
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {{}})
    }}

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: Update{pascal}Request,
    ) -> Result<Empty, StatusError> {{
        let payload = operator_of(&ctx)?;
        let row = {table}::Entity::find_by_id(req.id)
{update_filter}            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("{name}"))?;
        let mut a: {table}::ActiveModel = row.into();
        if let Some(data) = &req.data {{
{update}        }}
        a.updated_by = Set(Some(payload.user_id));
        a.updated_at = Set(Some(crate::data::now()));
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {{}})
    }}

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: Delete{pascal}Request,
    ) -> Result<Empty, StatusError> {{
{delete_ctx}        let _ = ctx;
        let repo = {pascal}Repo::new(&self.state.db{viewer_arg});
        repo.delete_batch(&req.ids).await?;
        Ok(Empty {{}})
    }}
}}
"#,
        pascal = spec.pascal,
        name = spec.name,
        table = spec.table,
        pkg_mod = spec.pkg_mod,
        sea_imports = sea_imports,
        data_import = data_import,
        state_imports = state_imports,
        helpers = enum_helpers(spec),
        tenant_row = tenant_row,
        list_ctx = list_ctx,
        delete_ctx = delete_ctx,
        mapper = mapper_fields(spec),
        tenant_name_row = tenant_name_row,
        viewer_arg = viewer_arg,
        get_body = get_body,
        create = create_fields(spec),
        update_filter = update_filter,
        update = update_fields(spec),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_helpers() {
        assert_eq!(pascal_of("dict_type"), "DictType");
        assert_eq!(pascal_of("widget"), "Widget");
        assert_eq!(plural_of("widget"), "widgets");
        assert_eq!(plural_of("category"), "categories");
        assert_eq!(plural_of("box"), "boxes");
        assert_eq!(plural_of("dict_type"), "dict_types");
        assert_eq!(camel_of("type_code"), "typeCode");
        assert!(!is_snake("Widget"));
        assert!(is_snake("widget"));
        assert!(!is_snake(""));
    }

    #[test]
    fn sorted_insert_places_lines_alphabetically() {
        let text = "mod audit;\nmod misc;\nmod scope;\n";
        let out = insert_sorted(
            text,
            |l| l.trim_start().starts_with("mod "),
            mod_key,
            "mod rbac;",
            "rbac",
        )
        .unwrap();
        assert_eq!(out, "mod audit;\nmod misc;\nmod rbac;\nmod scope;\n");

        // 全部更小 → 追加到最后一个锚点之后
        let out = insert_sorted(
            text,
            |l| l.trim_start().starts_with("mod "),
            mod_key,
            "mod zeta;",
            "zeta",
        )
        .unwrap();
        assert_eq!(out, "mod audit;\nmod misc;\nmod scope;\nmod zeta;\n");

        // pub mod 行参与同一字节序（repos < sys_widgets，'e' < 'y' → 追加在后）
        let text = "mod audit;\npub mod repos;\n";
        let out = insert_sorted(
            text,
            |l| {
                let t = l.trim_start();
                t.starts_with("mod ") || t.starts_with("pub mod ")
            },
            mod_key,
            "pub mod sys_widgets;",
            "sys_widgets",
        )
        .unwrap();
        assert_eq!(out, "mod audit;\npub mod repos;\npub mod sys_widgets;\n");
    }

    #[test]
    fn mount_entry_inserts_before_greater_multi_line_aware() {
        let text = "    mount_services!(\n        (mount_api_service, ApiService),\n        (\n            mount_data_access_audit_log_service,\n            DataAccessAuditLogService\n        ),\n        (mount_file_service, FileService),\n    );\n";
        let out = insert_mount_entry(text, "dict_type", "DictType").unwrap();
        assert!(out.contains("        (mount_dict_type_service, DictTypeService),"));
        let lines: Vec<&str> = out.lines().collect();
        let dict_at = lines
            .iter()
            .position(|l| l.contains("mount_dict_type"))
            .unwrap();
        let file_at = lines
            .iter()
            .position(|l| l.contains("mount_file_service"))
            .unwrap();
        assert!(dict_at < file_at, "字母序：dict_type 在 file 之前");

        // 幂等语义由调用方的 contains 守卫保证；此处验证无锚点场景
        assert!(insert_mount_entry("no macro here", "x", "X").is_none());
    }

    #[test]
    fn service_import_splices_into_wrapped_use_block() {
        let text = "use crate::services::{\n    AccessKeyService, ApiService,\n    ConfigService, DashboardService,\n    FileService,\n};\n";
        // Delta 落在 Dashboard 与 File 之间 → 拼进 File 行首
        let out = insert_service_import(text, "DeltaService").unwrap();
        assert!(
            out.contains("    DeltaService, FileService,"),
            "行内词级拼接：{out}"
        );

        // Gizmo 大于全块（G > F）→ 关闭括号前作为新行插入
        let out = insert_service_import(text, "GizmoService").unwrap();
        assert!(out.contains("    GizmoService,\n};"), "{out}");
    }

    #[test]
    fn templates_carry_the_load_bearing_marks() {
        let spec = EntitySpec {
            name: "widget".to_owned(),
            pascal: "Widget".to_owned(),
            table: "sys_widgets".to_owned(),
            package: "widget.service.v1".to_owned(),
            pkg_mod: "widget::service::v1".to_owned(),
            proto_dir: "widget/service/v1".to_owned(),
            route_prefix: "/admin/v1/widgets".to_owned(),
            fields: vec![
                FieldSpec {
                    name: "code".to_owned(),
                    kind: FieldKind::String,
                },
                FieldSpec {
                    name: "quantity".to_owned(),
                    kind: FieldKind::Uint32,
                },
            ],
            code_field: Some("code".to_owned()),
            global: false,
        };
        let admin = admin_proto(&spec);
        assert!(admin.contains("get: \"/admin/v1/widgets\""));
        assert!(admin.contains("import \"widget/service/v1/widget.proto\";"));
        assert!(
            admin.contains(
                "additional_bindings {\n        get: \"/admin/v1/widgets/code/{code}\"\n      }"
            ),
            "code 臂附加路由：{admin}"
        );

        let svc = service_file(&spec);
        assert!(svc.contains("impl proto::gen::services::WidgetServiceHandlers for WidgetService"));
        assert!(svc.contains("widget::service::v1::get_widget_request::QueryBy::Id(id)"));
        assert!(svc.contains("QueryBy::Code(code)"), "code 臂查询分支");
        assert!(svc.contains("Column::Code.eq(code)"));
        assert!(svc.contains("code: Set(data.code.unwrap_or_default()),"));
        assert!(svc.contains("quantity: Set(data.quantity),"));
        assert!(svc.contains(
            "if let Some(v) = data.quantity {\n                a.quantity = Set(Some(v));\n            }"
        ));

        let entity = entity_file(&spec);
        assert!(entity.contains("#[sea_orm(table_name = \"sys_widgets\")]"));
        assert!(
            entity.contains("#[sea_orm(unique)]\n        pub code: String,"),
            "code 列带 unique 标注：{entity}"
        );
        assert!(entity.contains("pub quantity: Option<u32>,"));
        assert!(entity.contains("pub tenant_id: Option<u32>,"));
        assert_eq!(
            entity.matches("#[sea_orm(unique)]").count(),
            1,
            "只有 code 列带 unique"
        );

        let msg = message_proto(&spec);
        assert!(msg.contains("optional string code = 2 ["));
        assert!(msg.contains("optional uint32 quantity = 3 ["));
    }

    #[test]
    fn enum_kind_parses_and_validates() {
        let ok = FieldKind::parse("enum(0=DISABLED,1=NORMAL,2=PENDING@default=NORMAL)");
        let Some(FieldKind::Enum(values)) = ok else {
            panic!("合法枚举语法应可解析");
        };
        assert_eq!(values.values.len(), 3);
        assert_eq!(values.default, "NORMAL");

        // 缺省 @default → 取 0 值文本
        let Some(FieldKind::Enum(values)) = FieldKind::parse("enum(0=OFF,1=ON)") else {
            panic!("无 @default 也应可解析");
        };
        assert_eq!(values.default, "OFF");

        assert!(FieldKind::parse("enum()").is_none(), "空取值集");
        assert!(FieldKind::parse("enum(1=ON)").is_none(), "必须含 0 值项");
        assert!(FieldKind::parse("enum(0=on)").is_none(), "文本须大写下划线");
        assert!(FieldKind::parse("enum(0=A,0=B)").is_none(), "数值重复");
        assert!(
            FieldKind::parse("enum(0=A@default=Z)").is_none(),
            "@default 须在取值集内"
        );
        assert!(FieldKind::parse("enum(0=A@default=nope)").is_none());
    }

    #[test]
    fn enum_and_global_templates_drop_or_add_the_right_marks() {
        let enum_kind = FieldKind::parse("enum(0=DISABLED,1=ENABLED@default=ENABLED)").unwrap();
        let spec = EntitySpec {
            name: "gadget".to_owned(),
            pascal: "Gadget".to_owned(),
            table: "sys_gadgets".to_owned(),
            package: "gadget.service.v1".to_owned(),
            pkg_mod: "gadget::service::v1".to_owned(),
            proto_dir: "gadget/service/v1".to_owned(),
            route_prefix: "/admin/v1/gadgets".to_owned(),
            fields: vec![FieldSpec {
                name: "state".to_owned(),
                kind: enum_kind,
            }],
            code_field: None,
            global: true,
        };

        let msg = message_proto(&spec);
        assert!(
            msg.contains("enum State {\n    DISABLED = 0;\n    ENABLED = 1;\n  }"),
            "嵌套 enum 块：{msg}"
        );
        assert!(msg.contains("optional State state = 2 ["));
        assert!(!msg.contains("tenant_id"), "全局表消息无租户列");
        assert!(!msg.contains("tenant_name"));

        let entity = entity_file(&spec);
        assert!(entity.contains("/// enum(DISABLED,ENABLED) default ENABLED."));
        assert!(entity.contains("pub state: Option<String>,"));
        assert!(!entity.contains("pub tenant_id"), "全局表实体无租户列");
        assert!(
            !entity.contains("#[sea_orm(unique)]"),
            "无 code 字段时不带 unique 标注"
        );

        let repo = repo_file(&spec);
        assert!(repo.contains("repo_shell!(global GadgetRepo, entity)"));
        assert!(!repo.contains("Viewer"), "全局 repo 无 viewer");

        let svc = service_file(&spec);
        assert!(svc.contains("fn state_to_text(v: i32) -> String"));
        assert!(svc.contains("1 => \"ENABLED\".to_string(),"));
        assert!(
            svc.contains("_ => \"ENABLED\".to_string(),"),
            "未识别回退 = @default"
        );
        assert!(svc.contains("fn state_from_text(value: Option<&str>) -> i32"));
        assert!(svc.contains("state: Some(state_from_text(r.state.as_deref())),"));
        assert!(svc.contains("state: Set(Some(state_to_text(data.state.unwrap_or(0)))),"));
        assert!(!svc.contains("tenant_id: Set("), "全局 create 无租户");
        assert!(!svc.contains("Column::TenantId"), "全局 update 无租户过滤");
        assert!(
            svc.contains("Repo::new(&self.state.db)"),
            "全局 repo 不带 viewer"
        );
        assert!(
            !svc.contains("ColumnTrait, EntityTrait, QueryFilter, Set"),
            "全局表按需裁剪 sea_orm 导入"
        );
        assert!(svc.contains("use sea_orm::{ActiveModelTrait, EntityTrait, Set};"));
    }
}
