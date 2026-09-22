//! 实体规格文件（`.rush/<name>.json`）：字段清单的唯一真相，一处声明、
//! 多处消费——`gen entity` 落盘，`gen pages` 缺 `--field` 时读取，未来
//! Tauri UI 的表单回填也以它为数据源。
//!
//! 文件放仓库根 `.rush/`（在 proto/react 两个同步面之外，不进清单）；
//! 应当提交进仓，作为重生成与页面生成的可追溯输入。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entity::{FieldKind, FieldSpec};
use crate::{Error, Result};

/// 规格文件结构版本（破坏性变更时递增）。
pub const SPEC_SCHEMA: u32 = 1;

/// 一个业务字段的规格化形态：kind 存 `FieldKind::parse` 的合法输入串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecField {
    pub name: String,
    pub kind: String,
}

/// 一个实体链的完整规格（解析后的值，不是原始 CLI 选项——缺省项落定，
/// 表单回填拿到的是完整描述）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySpecFile {
    pub schema: u32,
    /// 实体名，snake_case 单数。
    pub name: String,
    /// 表名（缺省展开后）。
    pub table: String,
    /// 消息面 package（缺省展开后）。
    pub package: String,
    /// 路由前缀（缺省展开后）。
    pub route_prefix: String,
    pub fields: Vec<SpecField>,
    pub code_field: Option<String>,
    /// 平台全局表。
    pub global: bool,
    /// 页面分组目录——`gen pages` 写回。
    pub group: Option<String>,
}

/// 规格 JSON 的序列化错误统一走输入非法通道。
fn serde_err(context: &str, err: serde_json::Error) -> Error {
    Error::InvalidInput(format!("{context}：{err}"))
}

/// 规格 串 ↔ FieldKind 的往返：kind 串必须是 `FieldKind::parse` 的
/// 合法输入（同一套语法，不发明第二种记法）。
pub fn to_fields(file: &EntitySpecFile) -> Result<Vec<FieldSpec>> {
    file.fields
        .iter()
        .map(|field| {
            let kind = FieldKind::parse(&field.kind).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "规格文件字段 kind 非法：{} = {}",
                    field.name, field.kind
                ))
            })?;
            Ok(FieldSpec {
                name: field.name.clone(),
                kind,
            })
        })
        .collect()
}

/// 从已解析的生成参数组装规格（gen entity 调用）。
pub fn from_parts(
    name: &str,
    table: &str,
    package: &str,
    route_prefix: &str,
    fields: &[FieldSpec],
    code_field: Option<&str>,
    global: bool,
) -> EntitySpecFile {
    EntitySpecFile {
        schema: SPEC_SCHEMA,
        name: name.to_owned(),
        table: table.to_owned(),
        package: package.to_owned(),
        route_prefix: route_prefix.to_owned(),
        fields: fields
            .iter()
            .map(|field| SpecField {
                name: field.name.clone(),
                kind: field.kind.spec_string(),
            })
            .collect(),
        code_field: code_field.map(str::to_owned),
        global,
        group: None,
    }
}

pub fn spec_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".rush")
}

pub fn spec_path(repo_root: &Path, name: &str) -> PathBuf {
    spec_dir(repo_root).join(format!("{name}.json"))
}

/// 落盘（目录不存在则创建）。返回写入路径。
pub fn save(file: &EntitySpecFile, repo_root: &Path) -> Result<PathBuf> {
    let path = spec_path(repo_root, &file.name);
    fs::create_dir_all(spec_dir(repo_root))?;
    let mut json =
        serde_json::to_string_pretty(file).map_err(|err| serde_err("规格文件序列化失败", err))?;
    json.push('\n');
    fs::write(&path, json)?;
    Ok(path)
}

/// 读取规格；缺失时给出可行动的报错（先跑 gen entity 或显式传 --field）。
pub fn load(repo_root: &Path, name: &str) -> Result<EntitySpecFile> {
    let path = spec_path(repo_root, name);
    let text = fs::read_to_string(&path).map_err(|_| {
        Error::InvalidInput(format!(
            "规格文件缺失：{}（先运行 rush gen entity {}，或显式传 --field）",
            path.display(),
            name
        ))
    })?;
    let file: EntitySpecFile =
        serde_json::from_str(&text).map_err(|err| serde_err("规格文件解析失败", err))?;
    if file.schema != SPEC_SCHEMA {
        return Err(Error::InvalidInput(format!(
            "规格文件 schema 版本不支持：{}（文件 {}，支持 {SPEC_SCHEMA}）",
            file.schema,
            path.display()
        )));
    }
    if file.name != name {
        return Err(Error::InvalidInput(format!(
            "规格文件实体名不匹配：文件内 {} ≠ 请求 {name}（{}）",
            file.name,
            path.display()
        )));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(kind: &str) -> String {
        FieldKind::parse(kind).expect("kind 合法").spec_string()
    }

    #[test]
    fn kind_spec_strings_round_trip() {
        assert_eq!(round_trip("string"), "string");
        assert_eq!(round_trip("int"), "i32");
        assert_eq!(round_trip("uint"), "u32");
        assert_eq!(round_trip("bool"), "bool");
        assert_eq!(round_trip("float"), "f64");
        // 缺省 @default == 0 值文本 → 规格串省略 @default
        assert_eq!(round_trip("enum(0=OFF,1=ON)"), "enum(0=OFF,1=ON)");
        // @default 偏离 0 值 → 保留
        assert_eq!(
            round_trip("enum(0=DISABLED,1=NORMAL,2=PENDING@default=NORMAL)"),
            "enum(0=DISABLED,1=NORMAL,2=PENDING@default=NORMAL)"
        );
        // 全程往返：parse(spec_string(x)) == x
        let kinds = [
            "string",
            "i32",
            "u32",
            "bool",
            "f64",
            "enum(0=OFF,1=ON@default=ON)",
            "enum(0=A,1=B,2=C)",
        ];
        for kind in kinds {
            let parsed = FieldKind::parse(kind).unwrap();
            let spec = parsed.spec_string();
            let again = FieldKind::parse(&spec).unwrap();
            assert_eq!(parsed, again, "{kind} 往返失配：{spec}");
        }
        assert!(FieldKind::parse("enum()").is_none());
    }

    #[test]
    fn spec_file_round_trips_through_json() {
        let file = EntitySpecFile {
            schema: SPEC_SCHEMA,
            name: "widget".into(),
            table: "sys_widgets".into(),
            package: "widget.service.v1".into(),
            route_prefix: "/admin/v1/widgets".into(),
            fields: vec![
                SpecField {
                    name: "code".into(),
                    kind: "string".into(),
                },
                SpecField {
                    name: "state".into(),
                    kind: "enum(0=OFF,1=ON)".into(),
                },
            ],
            code_field: Some("code".into()),
            global: false,
            group: Some("system".into()),
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let back: EntitySpecFile = serde_json::from_str(&json).unwrap();
        assert_eq!(file, back);
    }

    #[test]
    fn load_rejects_missing_and_mismatched_specs() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path(), "widget").unwrap_err();
        assert!(format!("{err}").contains("规格文件缺失"), "{err}");

        let mut file = from_parts(
            "widget",
            "sys_widgets",
            "widget.service.v1",
            "/admin/v1/widgets",
            &[FieldSpec {
                name: "code".into(),
                kind: FieldKind::String,
            }],
            Some("code"),
            false,
        );
        save(&file, dir.path()).unwrap();
        assert!(load(dir.path(), "widget").is_ok());

        file.name = "other".into();
        save(&file, dir.path()).unwrap();
        // save 按 file.name 定路径：把篡改后的文件挪回 widget.json 模拟错配
        fs::rename(
            spec_path(dir.path(), "other"),
            spec_path(dir.path(), "widget"),
        )
        .unwrap();
        let err = load(dir.path(), "widget").unwrap_err();
        assert!(format!("{err}").contains("不匹配"), "{err}");

        file.schema = 99;
        file.name = "widget".into();
        save(&file, dir.path()).unwrap();
        let err = load(dir.path(), "widget").unwrap_err();
        assert!(format!("{err}").contains("schema"), "{err}");
    }
}
