//! MySQL 风格 DDL 解析：`CREATE TABLE` → 实体行（RushWind 的 SQL 数据源）。
//! 纯函数、零依赖、可单测；数据库直连（`database`）与 AI 后端落地（`ai`）
//! 共用这里的类型映射与表→实体名派生。
//!
//! 取舍：rush-gen 的 FieldKind 是闭集（string/i32/u32/bool/f64/enum），
//! 64 位整型与日期等都折叠到最接近的可用类型；MySQL enum 只有在全部
//! 取值本身就是 UPPER_SNAKE（RushWind 文本列枚举的存储形态）时才映射为
//! enum kind，否则保守映射为 string，避免把小写存储值改写成大写。

use rush_gen::entity::plural_of;

use crate::{GeneratorOption, SpecFieldDto};

/// 生成链自动维护的列，导入时跳过（业务字段才是实体行的内容）。
pub const INFRA_COLUMNS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "tenant_id",
    "version",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlTable {
    /// 物理表名。
    pub table: String,
    /// 派生实体名（snake_case 单数）。
    pub name: String,
    pub fields: Vec<SpecFieldDto>,
}

/// 一张表 → 一行生成器选项（proto_package 按缺省 `<name>.service.v1` 预填）。
pub fn to_option(id: i64, table: &SqlTable) -> GeneratorOption {
    GeneratorOption {
        id,
        table_name: table.name.clone(),
        service: String::new(),
        exclude: false,
        proto_package: format!("{}.service.v1", table.name),
        table: table.table.clone(),
        route_prefix: format!("/admin/v1/{}", plural_of(&table.name)),
        fields: table.fields.clone(),
        code_field: if table.fields.iter().any(|f| f.name == "code") {
            "code".to_string()
        } else {
            String::new()
        },
        global: false,
        auth_free: false,
    }
}

/// SQL 类型（information_schema 的 DATA_TYPE[+COLUMN_TYPE] 或 DDL 原文类型）
/// → FieldKind 规格串。None = 不支持的列（跳过并在报告里说明）。
pub fn sql_type_to_kind(raw_type: &str) -> Option<String> {
    let lowered = raw_type.to_lowercase();
    let base = lowered
        .split(['(', ' '])
        .next()
        .unwrap_or(&lowered)
        .trim();
    let unsigned = lowered.contains("unsigned");

    // enum('A','B') / enum('a','b')：全 UPPER_SNAKE 才走 enum kind。
    if base == "enum" {
        return match parse_mysql_enum(lowered.rfind('(').map(|_| raw_type)) {
            Some(kind) => Some(kind),
            None => Some("string".to_string()),
        };
    }

    let kind = match base {
        "char" | "varchar" | "nvarchar" | "nchar" | "character" | "bpchar" | "text" | "tinytext"
        | "mediumtext" | "longtext" | "clob" | "json" | "jsonb" | "uuid" | "inet" | "citext"
        | "name" => "string",
        "binary" | "varbinary" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "bytea" => {
            "string"
        }
        "date" | "datetime" | "timestamp" | "timestamptz" | "time" | "interval" | "year" => {
            "string"
        }
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "int2" | "int4" | "serial" => {
            if base == "tinyint" && lowered.starts_with("tinyint(1)") && !unsigned {
                // MySQL 布尔的惯例形态。
                "bool"
            } else if unsigned {
                "u32"
            } else {
                "i32"
            }
        }
        // rush-gen 无 64 位整型：bigint 折叠到 i32/u32（proto 面同限制）。
        "bigint" | "int8" | "bigserial" | "numeric" | "decimal" | "float" | "double"
        | "double precision" | "real" | "money" | "float4" | "float8" => "f64",
        "bool" | "boolean" | "bit" => "bool",
        _ => return None,
    };
    Some(kind.to_string())
}

/// `enum('ALPHA','BETA')` → `enum(0=ALPHA,1=BETA)`（仅当全部文本 UPPER_SNAKE）。
fn parse_mysql_enum(original: Option<&str>) -> Option<String> {
    let text = original?;
    let inner = text[text.find('(')? + 1..text.rfind(')')?].to_string();
    let mut parts = Vec::new();
    for (idx, item) in split_quoted_list(&inner).into_iter().enumerate() {
        let value = item.trim().trim_matches('\'').trim_matches('"').to_string();
        let ok = !value.is_empty()
            && value.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && value
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return None;
        }
        parts.push(format!("{idx}={value}"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("enum({})", parts.join(",")))
}

/// 按顶层逗号切分（尊重单/双引号与括号嵌套）。
fn split_quoted_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    current.push(c);
                }
                '(' => {
                    depth += 1;
                    current.push(c);
                }
                ')' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    out.push(std::mem::take(&mut current));
                }
                _ => current.push(c),
            },
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// 顶层逗号切分（括号/引号内不切）——列定义行的分隔。
fn split_top_level_commas(text: &str) -> Vec<String> {
    split_quoted_list(text)
}

/// 反引号/双引号包住的标识符去壳。
fn unquote_ident(raw: &str) -> String {
    raw.trim().trim_matches('`').trim_matches('"').to_string()
}

/// 表名 → 实体名：去 `sys_`/`t_`/`tbl_` 前缀 + 简单去复数。
pub fn entity_name_of(table: &str) -> String {
    let stripped = table
        .strip_prefix("sys_")
        .or(table.strip_prefix("t_"))
        .or(table.strip_prefix("tbl_"))
        .unwrap_or(table);
    singularize(stripped)
}

fn singularize(word: &str) -> String {
    if word.ends_with("ies") && word.len() > 3 {
        return format!("{}y", &word[..word.len() - 3]);
    }
    for suffix in ["xes", "zes", "ches", "shes"] {
        if word.ends_with(suffix) && word.len() > suffix.len() {
            return word[..word.len() - 2].to_string();
        }
    }
    if word.ends_with("ss") || !word.ends_with('s') || word.len() <= 2 {
        return word.to_string();
    }
    word[..word.len() - 1].to_string()
}

fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut iter = sql.chars().peekable();
    let mut in_line = false;
    let mut in_block = false;
    let mut quote: Option<char> = None;
    while let Some(c) = iter.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(next) = iter.next() {
                    out.push(next);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if in_line {
            if c == '\n' {
                in_line = false;
                out.push(c);
            }
            continue;
        }
        if in_block {
            if c == '*' && iter.peek() == Some(&'/') {
                iter.next();
                in_block = false;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '-' if iter.peek() == Some(&'-') => {
                in_line = true;
                continue;
            }
            '/' if iter.peek() == Some(&'*') => {
                iter.next();
                in_block = true;
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// 解析一段 MySQL DDL，提取所有 CREATE TABLE。约束行与未支持列被跳过。
pub fn parse_ddl(sql: &str) -> Vec<SqlTable> {
    let cleaned = strip_comments(sql);
    let lower = cleaned.to_lowercase();
    let mut tables = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("create table") {
        let after_kw = from + rel + "create table".len();
        from = after_kw;
        let mut rest = cleaned[after_kw..].trim_start();
        // `CREATE TABLE IF NOT EXISTS`：关键字在表名之前。
        if rest.len() >= 13 && rest[..13].eq_ignore_ascii_case("if not exists") {
            if let Some(b) = rest.as_bytes().get(13) {
                if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                    rest = rest[13..].trim_start();
                }
            }
        }
        let Some((name, tail)) = read_ident(rest) else {
            continue;
        };
        let head = tail.trim_start();
        let Some(open_rel) = head.find('(') else {
            continue;
        };
        let between = head[..open_rel].trim();
        if !between.is_empty() {
            continue;
        }
        let paren_in_rest = head.as_ptr() as usize - rest.as_ptr() as usize + open_rel;
        let Some(body) = balanced_parens(rest, paren_in_rest) else {
            continue;
        };
        // 半限定名 `db`.`table` 取最后一段。
        let table = unquote_ident(name.split('.').next_back().unwrap_or(&name));
        if let Some(table) = parse_table_body(&table, &body) {
            tables.push(table);
        }
    }
    tables
}
fn read_ident(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut idx = 0usize;
    let mut ident = String::new();
    let mut has = false;
    while idx < bytes.len() {
        let c = text[idx..].chars().next()?;
        match c {
            '`' | '"' if !has || ident.ends_with('.') => {
                let close = text[idx + c.len_utf8()..].find(c)?;
                ident.push_str(&text[idx + c.len_utf8()..idx + c.len_utf8() + close]);
                idx += c.len_utf8() * 2 + close;
                has = true;
            }
            '.' if has => {
                ident.push('.');
                idx += 1;
            }
            _ if c.is_alphanumeric() || c == '_' => {
                ident.push(c);
                idx += c.len_utf8();
                has = true;
            }
            _ if c.is_whitespace() && has => {
                return Some((ident, &text[idx..]));
            }
            _ => break,
        }
    }
    has.then(|| (ident, &text[idx..]))
}

fn balanced_parens(text: &str, open_at: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.get(open_at) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (rel, c) in text[open_at..].char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[open_at + 1..open_at + rel].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_table_body(table: &str, body: &str) -> Option<SqlTable> {
    let mut fields = Vec::new();
    for part in split_top_level_commas(body) {
        let line = part.trim().to_string();
        let lower_line = line.to_lowercase();
        if line.is_empty()
            || starts_with_keyword(
                &lower_line,
                &[
                    "primary key",
                    "unique",
                    "key",
                    "index",
                    "constraint",
                    "foreign key",
                    "fulltext",
                    "spatial",
                    "check",
                ],
            )
        {
            continue;
        }
        let Some((column, after)) = read_ident(&line) else {
            continue;
        };
        let column = column.to_lowercase();
        if column.is_empty() || INFRA_COLUMNS.contains(&column.as_str()) {
            continue;
        }
        let type_text = after.trim_start();
        let Some(type_end) = first_break(type_text) else {
            continue;
        };
        let raw_type = &type_text[..type_end];
        // `int unsigned` 的修饰词在类型段之后；补回给映射器。
        let rest = type_text[type_end..].to_lowercase();
        let composed = if rest.contains("unsigned") {
            format!("{raw_type} unsigned")
        } else {
            raw_type.to_string()
        };
        let kind = match sql_type_to_kind(&composed) {
            Some(kind) => kind,
            None => continue,
        };
        fields.push(SpecFieldDto {
            name: column,
            kind,
        });
    }
    if fields.is_empty() {
        return None;
    }
    Some(SqlTable {
        table: table.to_lowercase(),
        name: entity_name_of(&table.to_lowercase()),
        fields,
    })
}

fn starts_with_keyword(lower: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| {
        lower.starts_with(kw)
            && matches!(
                lower[kw.len()..].chars().next(),
                None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('(')
            )
    })
}

/// 类型段的结束位置：空白或未参与类型的括号外字符前。
fn first_break(type_text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, c) in type_text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth > 0 => {}
            ',' | ' ' | '\t' | '\n' | '\r' => return Some(idx),
            _ => {}
        }
    }
    (!type_text.is_empty()).then_some(type_text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = r#"
-- 演示表
CREATE TABLE IF NOT EXISTS `sys_dict_type` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `code` varchar(64) NOT NULL COMMENT '字典编码',
  `name` varchar(128) NOT NULL,
  `status` enum('DISABLED','ENABLED') NOT NULL DEFAULT 'DISABLED',
  `level` int unsigned DEFAULT 1,
  `weight` decimal(10,2) DEFAULT 0,
  `locked` tinyint(1) DEFAULT 0,
  `state` int DEFAULT 0 COMMENT '状态',
  `remark` varchar(64) CHARACTER SET utf8mb4 DEFAULT '',
  `created_at` datetime NOT NULL,
  `updated_at` datetime NOT NULL,
  `tenant_id` bigint DEFAULT 0,
  PRIMARY KEY (`id`),
  UNIQUE KEY `idx_code` (`code`),
  KEY `idx_state` (`state`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
"#;

    #[test]
    fn parses_columns_and_skips_infra_and_constraints() {
        let tables = parse_ddl(DEMO);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.table, "sys_dict_type");
        assert_eq!(t.name, "dict_type");
        let kinds: Vec<(&str, &str)> = t
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("code", "string"),
                ("name", "string"),
                ("status", "enum(0=DISABLED,1=ENABLED)"),
                ("level", "u32"),
                ("weight", "f64"),
                ("locked", "bool"),
                ("state", "i32"),
                ("remark", "string"),
            ]
        );
    }

    #[test]
    fn lowercase_mysql_enum_falls_back_to_string() {
        let sql = "CREATE TABLE t_widgets (id bigint, kind enum('normal','off'));";
        let tables = parse_ddl(sql);
        assert_eq!(tables.len(), 1);
        let kind = &tables[0].fields[0];
        assert_eq!(kind.name, "kind");
        assert_eq!(kind.kind, "string");
        assert_eq!(tables[0].name, "widget");
    }

    #[test]
    fn multiple_tables_and_comment_stripping() {
        let sql = "/* a */ CREATE TABLE users (name varchar(10)); # not supported marker\nCREATE TABLE categories (title text);";
        let tables = parse_ddl(sql);
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["user", "category"]);
    }

    #[test]
    fn option_row_fills_defaults_from_table() {
        let tables = parse_ddl(DEMO);
        let row = to_option(7, &tables[0]);
        assert_eq!(row.proto_package, "dict_type.service.v1");
        assert_eq!(row.route_prefix, "/admin/v1/dict_types");
        assert_eq!(row.code_field, "code");
        assert_eq!(row.table, "sys_dict_type");
    }
}
