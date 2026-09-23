//! 前端页面生成（`rush gen pages`）：为 `rush gen entity` 生成的后端实体
//! 生成 rushwind-admin React 前端的 CRUD 页面组。
//!
//! 模板基准是仓内的 dict 实体页（ProTable 列表 + DrawerForm 编辑抽屉 +
//! React Query hooks + i18n 模块）。生成的 hooks 是**自包含**的：本地接口
//! 类型 + `requestApi` 原生调用（与上游生成的 TS 客户端同一条 axios 通道，
//! 复用其 token 注入与错误拦截），因此不依赖上游重新生成 TS 客户端。
//!
//! 路由注册是后端驱动的（菜单种子里的组件路径），因此页面文件放到
//! `pages/app/<group>/<plural>/` 即可被动态路由拾取，无需改前端路由；
//! 菜单种子由本命令一并手术插入 seed.rs（`seed_gen_menu_<name>`，按
//! path 幂等，增量生效——固定 rows 数组的 seed_menus 只对空库生效）。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::entity::{camel_of, is_snake, pascal_of, plural_of, FieldKind, FieldSpec};
use crate::{Error, Result};

/// `rush gen pages` 选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagesOptions {
    /// rushwind-admin 仓库根目录。
    pub repo_root: PathBuf,
    /// 实体名，snake_case 单数（须与 gen entity 一致）。
    pub name: String,
    /// 页面分组目录（pages/app/<group>/<plural>）；缺省 `system`。
    pub group: Option<String>,
    /// 路由前缀（须与 gen entity 一致）；缺省 `/admin/v1/<复数>`。
    pub route_prefix: Option<String>,
    /// 业务字段（与 gen entity 相同的语法）。
    pub fields: Vec<FieldSpec>,
    /// 唯一编码字段（ drawers 里作必填并进搜索列）。
    pub code_field: Option<String>,
    /// 目标前端栈；缺省 react。
    pub stack: PagesStack,
    /// 平台全局表（页面去 tenantId）；缺省继承规格，显式给出时覆盖。
    #[serde(default)]
    pub global: Option<bool>,
    /// 重生成模式：既有页面组按规格/显式参数覆盖。
    #[serde(default)]
    pub overwrite: bool,
    /// 只报告不落盘。
    pub dry_run: bool,
}

/// 目标前端栈。react 菜单走后端 seed；vben / element 是前端静态路由
/// 模块（菜单=路由条目），由各自的生成器写入。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PagesStack {
    #[default]
    React,
    Vben,
    Element,
}

impl PagesStack {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::React => "react",
            Self::Vben => "vben",
            Self::Element => "element",
        }
    }
}

/// 生成结果报告。
#[derive(Debug, Default, Serialize)]
pub struct PagesReport {
    pub created: Vec<PathBuf>,
    /// 覆盖模式（--regen）下被重写的既有页面文件。
    #[serde(default)]
    pub updated: Vec<PathBuf>,
    /// dry-run 覆盖模式下的变更预览。
    #[serde(default)]
    pub diffs: Vec<(String, String)>,
    /// 手术插入的既有文件（seed.rs / 路由模块 / 规格文件）。
    pub edited: Vec<PathBuf>,
    pub skipped: Vec<String>,
    pub notes: Vec<String>,
}

/// 派生好的命名全集。
pub(crate) struct PagesSpec {
    pub(crate) name: String,
    pub(crate) pascal: String,
    pub(crate) plural: String,
    pub(crate) group: String,
    pub(crate) route_prefix: String,
    pub(crate) fields: Vec<FieldSpec>,
    pub(crate) code_field: Option<String>,
    pub(crate) stack: PagesStack,
    pub(crate) global: bool,
    /// `--field` 缺省时从规格文件（`.rush/<name>.json`）载入的原文；
    /// 显式传字段时为 None。
    pub(crate) loaded_spec: Option<crate::spec::EntitySpecFile>,
}

fn validate(opts: &PagesOptions) -> Result<PagesSpec> {
    // 字段来源二选一：显式 --field 优先；缺省读 gen entity 落的规格文件
    // ——字段清单从此只声明一遍（spec.rs 是唯一真相）。
    let (fields, code_field, route_prefix, loaded_spec) = if opts.fields.is_empty() {
        let file = crate::spec::load(&opts.repo_root, &opts.name)?;
        let fields = crate::spec::to_fields(&file)?;
        let code_field = opts.code_field.clone().or_else(|| file.code_field.clone());
        let route_prefix = opts
            .route_prefix
            .clone()
            .or_else(|| Some(file.route_prefix.clone()));
        (fields, code_field, route_prefix, Some(file))
    } else {
        (
            opts.fields.clone(),
            opts.code_field.clone(),
            opts.route_prefix.clone(),
            None,
        )
    };
    // 全局表：显式 --global 优先；否则继承规格；都没有则按租户实体。
    let global = opts
        .global
        .unwrap_or_else(|| loaded_spec.as_ref().is_some_and(|file| file.global));
    let mut seen = std::collections::BTreeSet::new();
    for field in &fields {
        if !seen.insert(field.name.clone()) {
            return Err(Error::InvalidInput(format!("字段重复：{}", field.name)));
        }
    }
    let code_field = match &code_field {
        Some(code_name) => {
            let field = fields
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
    let group = opts.group.clone().unwrap_or_else(|| "system".to_owned());
    // group 落三个面：页面目录、菜单 path 段、module 常量（大写化）——
    // 必须是合法 snake_case。
    if !is_snake(&group) {
        return Err(Error::InvalidInput(format!(
            "分组目录必须是 snake_case：{group}"
        )));
    }
    Ok(PagesSpec {
        name: opts.name.clone(),
        pascal: pascal_of(&opts.name),
        plural,
        stack: opts.stack,
        global,
        group,
        route_prefix: route_prefix
            .unwrap_or_else(|| format!("/admin/v1/{}", plural_of(&opts.name))),
        fields,
        code_field,
        loaded_spec,
    })
}

/// TS 字段类型（protojson 线上形状；枚举按 i32 数值传输）。
fn ts_type(kind: &FieldKind) -> &'static str {
    match kind {
        FieldKind::String => "string",
        FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Enum(_) => "number",
        FieldKind::Bool => "boolean",
        FieldKind::Float64 => "number",
    }
}

/// 生成页面组：按 --stack 分发到对应栈的生成器。新文件拒绝覆盖。
pub fn generate_pages(opts: &PagesOptions) -> Result<PagesReport> {
    let spec = validate(opts)?;
    let mut report = match spec.stack {
        PagesStack::React => generate_react(opts, &spec)?,
        PagesStack::Vben => crate::pages_vben::generate(opts, &spec)?,
        PagesStack::Element => crate::pages_element::generate(opts, &spec)?,
    };
    // 规格写回：group + stack 落档（字段真相归 gen entity，本命令只补
    // 页面视角的两个字段）；dry-run 不写。
    if !opts.dry_run {
        let spec_path = crate::spec::spec_path(&opts.repo_root, &spec.name);
        if spec_path.is_file() {
            let mut file = match &spec.loaded_spec {
                Some(file) => file.clone(),
                None => crate::spec::load(&opts.repo_root, &spec.name)?,
            };
            file.group = Some(spec.group.clone());
            file.stack = Some(spec.stack.as_str().to_owned());
            crate::spec::save(&file, &opts.repo_root)?;
            if !report.edited.contains(&spec_path) {
                report.edited.push(spec_path);
            }
        }
    }
    Ok(report)
}

fn generate_react(opts: &PagesOptions, spec: &PagesSpec) -> Result<PagesReport> {
    let react_root = opts.repo_root.join("frontend/admin/react");
    if !react_root.join("package.json").is_file() {
        return Err(Error::InvalidInput(format!(
            "目标仓缺少 React 前端：{}（frontend/admin/react 不在位？）",
            react_root.display()
        )));
    }
    let page_dir = react_root
        .join("src/pages/app")
        .join(&spec.group)
        .join(&spec.plural);
    let files: Vec<(PathBuf, String)> = vec![
        (
            react_root.join(format!("src/api/hooks/{}.ts", spec.name)),
            hooks_file(spec),
        ),
        (page_dir.join("constants.ts"), constants_file(spec)),
        (
            page_dir.join(format!("{}List.tsx", spec.pascal)),
            list_file(spec),
        ),
        (
            page_dir.join(format!("{}Drawer.tsx", spec.pascal)),
            drawer_file(spec),
        ),
        (page_dir.join("index.tsx"), index_file(spec)),
        (
            react_root.join(format!("src/locales/zh-CN/_modules/{}.json", spec.name)),
            locale_file(spec, true),
        ),
        (
            react_root.join(format!("src/locales/en-US/_modules/{}.json", spec.name)),
            locale_file(spec, false),
        ),
    ];
    for (path, _) in &files {
        if path.exists() && !opts.overwrite {
            return Err(Error::InvalidInput(format!(
                "文件已存在：{}（按规格重生成请加 --regen）",
                path.display()
            )));
        }
    }

    // ---- seed.rs 菜单种子的编辑计划（锚点在写入前全部校验） ----
    // 幂等标记：调用行与函数体各自查重，任一在位只补缺席的一半。
    let seed_rs = opts
        .repo_root
        .join("backend/services/admin-api/src/seed.rs");
    let fn_name = seed_menu_fn(&spec.name);
    let call_line = format!("    {fn_name}(state).await?;");
    let call_marker = format!("{fn_name}(state)");
    let fn_marker = format!("async fn {fn_name}");
    let mut seed_edits: Vec<String> = Vec::new();
    let mut seed_already = false;
    if seed_rs.is_file() {
        let mut text = fs::read_to_string(&seed_rs)?;
        let had_call = text.contains(&call_marker);
        let had_fn = text.contains(&fn_marker);
        if had_call && had_fn {
            seed_already = true;
        } else {
            if !text.contains("seed_menus(state).await?;") {
                return Err(Error::InvalidInput(format!(
                    "{} 中找不到 seed_menus 调用锚点（目标仓 seed.rs 不是 rushwind-admin 形状？）",
                    seed_rs.display()
                )));
            }
            if !text.contains("async fn seed_languages") {
                return Err(Error::InvalidInput(format!(
                    "{} 中找不到 seed_languages 函数锚点",
                    seed_rs.display()
                )));
            }
            if !had_call {
                text = insert_seed_call(&text, &call_line).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "{} 中找不到 seed_menus 调用锚点",
                        seed_rs.display()
                    ))
                })?;
            }
            if !had_fn {
                let function = seed_menu_function(spec);
                let edited = text.replacen(
                    "async fn seed_languages",
                    &format!("{function}\nasync fn seed_languages"),
                    1,
                );
                if edited == text {
                    return Err(Error::InvalidInput(format!(
                        "{} 中找不到 seed_languages 函数锚点",
                        seed_rs.display()
                    )));
                }
                text = edited;
            }
            seed_edits.push(text);
        }
    }

    let mut report = PagesReport::default();
    let spec_path = crate::spec::spec_path(&opts.repo_root, &spec.name);
    if opts.dry_run {
        report.created = files
            .iter()
            .filter(|(path, _)| !path.exists())
            .map(|(p, _)| p.clone())
            .collect();
        if opts.overwrite {
            for (path, content) in &files {
                if !path.exists() {
                    continue;
                }
                if let Ok(old) = fs::read_to_string(path) {
                    if let Some(diff) =
                        crate::textdiff::unified_ish(&path.display().to_string(), &old, content)
                    {
                        report.diffs.push((path.display().to_string(), diff));
                    }
                }
            }
        }
        if !seed_edits.is_empty() {
            report.edited.push(seed_rs.clone());
        } else if seed_already {
            report
                .skipped
                .push(format!("{}（菜单种子已在位）", seed_rs.display()));
        }
        if spec_path.is_file() {
            report.edited.push(spec_path.clone());
        }
        return Ok(report);
    }

    for (path, content) in &files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        if existed {
            if let Ok(old) = fs::read_to_string(path) {
                if old == *content {
                    continue;
                }
            }
            if !opts.overwrite {
                return Err(Error::InvalidInput(format!(
                    "文件已存在：{}（按规格重生成请加 --regen）",
                    path.display()
                )));
            }
            fs::write(path, content)?;
            report.updated.push(path.clone());
        } else {
            fs::write(path, content)?;
            report.created.push(path.clone());
        }
    }

    for text in &seed_edits {
        fs::write(&seed_rs, text)?;
    }
    if !seed_edits.is_empty() {
        report.edited.push(seed_rs.clone());
    } else if seed_already {
        report
            .skipped
            .push(format!("{}（菜单种子已在位）", seed_rs.display()));
    }

    if spec.loaded_spec.is_some() {
        report.notes.push(format!(
            "字段清单取自规格文件 .rush/{}.json（--field 可省）",
            spec.name
        ));
        if spec.loaded_spec.as_ref().is_some_and(|file| file.global) {
            report.notes.push(
                "规格为全局表（--global）：页面生成 v1 仍按租户实体形状输出（已知缺口）".to_owned(),
            );
        }
    }
    if seed_rs.is_file() {
        report.notes.push(format!(
            "菜单种子已写入 seed.rs（{fn_name}：按 path 幂等增量，父目录 /{} 缺失时自动补 CATALOG 行；标题用「{}」占位，文案请按需修改）",
            spec.group, spec.pascal
        ));
    } else {
        report
            .notes
            .push("目标仓没有 seed.rs，菜单种子未生成（页面放 pages/app/<group>/<plural>/ 后需手动配菜单）".to_owned());
    }
    report
        .notes
        .push("生成后建议在 frontend/admin/react 下运行 pnpm typecheck".to_owned());
    Ok(report)
}

// ---- hooks 模板 ----

pub(crate) fn ts_interface(spec: &PagesSpec) -> String {
    let mut out = String::new();
    out.push_str("  id?: number;\n");
    if !spec.global_like() {
        out.push_str("  tenantId?: number;\n");
    }
    for field in &spec.fields {
        out.push_str(&format!(
            "  {}?: {};\n",
            camel_of(&field.name),
            ts_type(&field.kind)
        ));
    }
    out.push_str("  sortOrder?: number;\n");
    out.push_str("  createdAt?: string;\n");
    out.push_str("  updatedAt?: string;\n");
    out
}

impl PagesSpec {
    fn global_like(&self) -> bool {
        self.global
    }
    /// 传输路径（无前导斜杠，与生成 TS 客户端一致）。
    pub(crate) fn base_path(&self) -> String {
        self.route_prefix.trim_start_matches('/').to_owned()
    }
}

fn hooks_file(spec: &PagesSpec) -> String {
    format!(
        r#"//! {pascal} 页面的数据层：自包含类型 + requestApi 原生调用。
//! 与上游生成的 TS 客户端走同一条 axios 通道（token 注入、错误拦截）。

import {{
  useMutation,
  type UseMutationOptions,
  useQuery,
  type UseQueryOptions,
}} from '@tanstack/react-query';

import {{ makeUpdateMask, type PaginationQuery }} from '@/core/transport/rest';
import {{ requestApi }} from '@/core/transport/rest';

// 本地接口：字段形状与 {pascal} 的 proto 契约一致（protojson 线上形状）。
export interface {pascal} {{
{ts_interface}}}

export interface List{pascal}Response {{
  items?: {pascal}[];
  total?: number;
}}

function listQueryString(query: PaginationQuery) {{
  const raw = query.toRawParams();
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(raw)) {{
    if (value !== undefined && value !== null && value !== '') {{
      params.set(key, String(value));
    }}
  }}
  const qs = params.toString();
  return qs ? `?${{qs}}` : '';
}}

export async function fetch{pascal}List(params: PaginationQuery) {{
  return requestApi({{
    path: `{base_path}${{listQueryString(params)}}`,
    method: 'GET',
    body: null,
  }}) as Promise<List{pascal}Response>;
}}

export function use{pascal}List(
  query: PaginationQuery,
  options?: UseQueryOptions<List{pascal}Response, Error>,
) {{
  return useQuery({{
    queryKey: ['{name}List', query],
    queryFn: () => fetch{pascal}List(query),
    ...options,
  }});
}}

export function useCreate{pascal}(
  options?: UseMutationOptions<{{}}, Error, {{ data: Partial<{pascal}> }}>,
) {{
  return useMutation({{
    mutationFn: (input) =>
      requestApi({{
        path: `{base_path}`,
        method: 'POST',
        body: JSON.stringify({{ data: input.data }}),
      }}) as Promise<{{}}>,
    ...options,
  }});
}}

export function useUpdate{pascal}(
  options?: UseMutationOptions<
    {{}},
    Error,
    {{ id: number; values: Partial<{pascal}> }}
  >,
) {{
  return useMutation({{
    mutationFn: ({{ id, values }}) =>
      requestApi({{
        path: `{base_path}/${{id}}`,
        method: 'PUT',
        body: JSON.stringify({{
          id,
          data: values,
          updateMask: makeUpdateMask(Object.keys(values ?? {{}})),
        }}),
      }}) as Promise<{{}}>,
    ...options,
  }});
}}

export function useDelete{pascal}(
  options?: UseMutationOptions<{{}}, Error, {{ ids: number[] }}>,
) {{
  return useMutation({{
    mutationFn: (input) =>
      requestApi({{
        path: `{base_path}`,
        method: 'DELETE',
        body: JSON.stringify(input),
      }}) as Promise<{{}}>,
    ...options,
  }});
}}
"#,
        pascal = spec.pascal,
        name = spec.name,
        base_path = spec.base_path(),
        ts_interface = ts_interface(spec),
    )
}

// ---- 常量模板 ----

fn constants_entries(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        match &field.kind {
            FieldKind::Bool => {
                out.push_str(&format!(
                    "export const {name}Options = (t: (key: string) => string) => [\n  {{ label: t('{name}_true'), value: true }},\n  {{ label: t('{name}_false'), value: false }},\n];\n\nexport const {name}Color = (value?: boolean) => (value ? 'green' : 'default');\n\nexport const {name}Label = (t: (key: string) => string, value?: boolean) => (value ? t('{name}_true') : t('{name}_false'));\n\n",
                    name = camel_of(&field.name)
                ));
            }
            FieldKind::Enum(values) => {
                let camel = camel_of(&field.name);
                out.push_str(&format!("export const {camel}Options = [\n"));
                for (num, text) in &values.values {
                    out.push_str(&format!("  {{ label: '{text}', value: {num} }},\n"));
                }
                out.push_str(&format!("];\n\nexport const {camel}Color = (value?: number) =>\n  ({camel}Options.find((option) => option.value === value)?.value ?? 0) === 0 ? 'default' : 'green';\n\nexport const {camel}Label = (value?: number) =>\n  {camel}Options.find((option) => option.value === value)?.label ?? String(value ?? 0);\n\n"));
            }
            _ => {}
        }
    }
    out
}

fn constants_file(spec: &PagesSpec) -> String {
    format!(
        r#"//! {pascal} 页面的取值集与渲染助手。
{entries}"#,
        pascal = spec.pascal,
        entries = constants_entries(spec),
    )
}

// ---- 表格列与表单控件（List / Drawer 共用字段遍历） ----

fn list_columns(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let label_key = &field.name;
        let searchable = matches!(field.kind, FieldKind::String);
        match &field.kind {
            FieldKind::Bool => {
                out.push_str(&format!(
                    "    {{\n      title: t('{label_key}'),\n      dataIndex: '{camel}',\n      width: 95,\n      valueType: 'select',\n      fieldProps: {{ options: {camel}Options(t) }},\n      render: (_, record) => (\n        <Tag color={{ {camel}Color(record.{camel}) }}>{{ {camel}Label(t, record.{camel}) }}</Tag>\n      ),\n    }},\n"
                ));
            }
            FieldKind::Enum(_) => {
                out.push_str(&format!(
                    "    {{\n      title: t('{label_key}'),\n      dataIndex: '{camel}',\n      width: 110,\n      valueType: 'select',\n      fieldProps: {{ options: {camel}Options }},\n      render: (_, record) => (\n        <Tag color={{ {camel}Color(record.{camel}) }}>{{ {camel}Label(record.{camel}) }}</Tag>\n      ),\n    }},\n"
                ));
            }
            _ => {
                let hide_search = if searchable {
                    ""
                } else {
                    "\n      hideInSearch: true,"
                };
                out.push_str(&format!(
                    "    {{\n      title: t('{label_key}'),\n      dataIndex: '{camel}',{hide_search}\n    }},\n"
                ));
            }
        }
    }
    out.push_str(
        "    {\n      title: t('sortOrder'),\n      dataIndex: 'sortOrder',\n      width: 95,\n      hideInSearch: true,\n    },\n    {\n      title: t('action'),\n      valueType: 'option',\n      width: 90,\n      render: (_, record) => [\n        <a\n          key=\"edit\"\n          onClick={() => {\n            setEditing(record);\n            setDrawerMode('edit');\n            setDrawerOpen(true);\n          }}\n        >\n          <EditOutlined />\n        </a>,\n        <Popconfirm\n          key=\"delete\"\n          title={t('deleteConfirmTitle')}\n          description={t('deleteConfirmDesc', { moduleName: t('moduleName') })}\n          onConfirm={() => record.id && deleteMutation.mutate({ ids: [record.id] })}\n          okText={t('common:button.ok')}\n          cancelText={t('common:button.cancel')}\n        >\n          <a style={{ color: 'var(--ant-color-error)' }}><DeleteOutlined /></a>\n        </Popconfirm>,\n      ],\n    },\n",
    );
    out
}

/// Drawer 实际用到的 pro-components 组件（按字段类型按需导入——生成物里
/// 一个未用导入就会挂下游 typecheck）。
fn drawer_components(spec: &PagesSpec) -> String {
    let mut parts = vec!["  DrawerForm,".to_owned()];
    parts.push("  ProFormDigit,".to_owned()); // sortOrder 恒在
    let mut has_text = false;
    let mut has_radio = false;
    let mut has_select = false;
    for field in &spec.fields {
        match &field.kind {
            FieldKind::String => has_text = true,
            FieldKind::Bool => has_radio = true,
            FieldKind::Enum(_) => has_select = true,
            _ => {}
        }
    }
    if has_radio {
        parts.push("  ProFormRadio,".to_owned());
    }
    if has_select {
        parts.push("  ProFormSelect,".to_owned());
    }
    if has_text {
        parts.push("  ProFormText,".to_owned());
    }
    let mut out = parts.join("\n");
    out.push('\n');
    out
}

fn drawer_controls(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let label = &field.name;
        let required = spec
            .code_field
            .as_deref()
            .map(|code| code == field.name)
            .unwrap_or(false);
        let required_rule = if required {
            format!(
                "rules={{[{{ required: true, message: t('required{}') }}]}}",
                pascal_of(&field.name)
            )
        } else {
            String::new()
        };
        match &field.kind {
            FieldKind::Enum(values) => {
                out.push_str(&format!(
                    "      <ProFormSelect\n        name=\"{camel}\"\n        label={{t('{label}')}}\n        options={{ {camel}Options }}\n        {required_rule}\n      />\n\n",
                    required_rule = required_rule,
                ));
                let _ = values;
            }
            FieldKind::Bool => {
                out.push_str(&format!(
                    "      <ProFormRadio.Group\n        name=\"{camel}\"\n        label={{t('{label}')}}\n        options={{ {camel}Options(t) }}\n        fieldProps={{{{ optionType: 'button', buttonStyle: 'solid' }}}}\n      />\n\n"
                ));
            }
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => {
                out.push_str(&format!(
                    "      <ProFormDigit\n        name=\"{camel}\"\n        label={{t('{label}')}}\n        {required_rule}\n        fieldProps={{{{ precision: 0 }}}}\n      />\n\n"
                ));
            }
            FieldKind::String => {
                out.push_str(&format!(
                    "      <ProFormText\n        name=\"{camel}\"\n        label={{t('{label}')}}\n        placeholder={{t('{label}Placeholder')}}\n        {required_rule}\n        fieldProps={{{{ allowClear: true }}}}\n      />\n\n"
                ));
            }
        }
    }
    out.push_str(
        "      <ProFormDigit\n        name=\"sortOrder\"\n        label={t('sortOrder')}\n        fieldProps={{ precision: 0, min: 0 }}\n      />\n\n",
    );
    out
}

// ---- 页面模板 ----

fn list_file(spec: &PagesSpec) -> String {
    format!(
        r#"import {{ useRef, useState }} from 'react';
import type {{ ProColumns, ActionType }} from '@ant-design/pro-components';
import {{ ProTable }} from '@ant-design/pro-components';
import {{ Button, Popconfirm, Tag, App }} from 'antd';
import {{ EditOutlined, DeleteOutlined, PlusOutlined }} from '@ant-design/icons';
import {{ useQueryClient }} from '@tanstack/react-query';
import {{ useTranslation }} from 'react-i18next';

import {{ PaginationQuery }} from '@/core/transport/rest';
import {{ TABLE }} from '@/config/constants';
import {{ useProTableScrollY }} from '@/hooks/useProTableScrollY';
import TableExportButton from '@/components/common/TableExportButton';
import {{
  fetch{pascal}List,
  useDelete{pascal},
}} from '@/api/hooks/{name}';
import {{
{constants_imports}}} from './constants';
import {pascal}Drawer from './{pascal}Drawer';

/**
 * {pascal} 列表页
 */
const {pascal}List: React.FC = () => {{
  const {{ t }} = useTranslation('{name}');
  const actionRef = useRef<ActionType>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const tableScrollY = useProTableScrollY(containerRef);
  const queryClient = useQueryClient();
  const {{ message }} = App.useApp();

  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerMode, setDrawerMode] = useState<'create' | 'edit'>('create');
  const [editing, setEditing] = useState<any>(null);

  const deleteMutation = useDelete{pascal}({{
    onSuccess: () => {{
      message.success(t('deleteSuccess'));
      actionRef.current?.reload();
      queryClient.invalidateQueries({{ queryKey: ['{name}List'] }});
    }},
    onError: (error: Error) => {{
      message.error(error.message || t('deleteFailed'));
    }},
  }});

  const columns: ProColumns<any>[] = [
{columns}  ];

  return (
    <>
      <div ref={{containerRef}} className="page-container-content" style={{{{ padding: '0 8px', height: '100%' }}}}>
        <ProTable<any>
          actionRef={{actionRef}}
          columns={{columns}}
          headerTitle={{false}}
          request={{async (params) => {{
            try {{
              const query = new PaginationQuery({{
                paging: {{
                  page: params.current || 1,
                  pageSize: params.pageSize || TABLE.DEFAULT_PAGE_SIZE,
                }},
                formValues: {{
                  ...Object.fromEntries(
                    Object.entries(params).filter(
                      ([key]) => !['current', 'pageSize'].includes(key),
                    ),
                  ),
                }},
              }});

              const response = await fetch{pascal}List(query);

              return {{
                data: response.items || [],
                total: response.total || 0,
                success: true,
              }};
            }} catch (error: any) {{
              message.error(error.message || t('fetchFailed'));
              return {{ data: [], total: 0, success: false }};
            }}
          }}}}
          rowKey="id"
          search={{{{
            labelWidth: 'auto',
            defaultCollapsed: false,
          }}}}
          pagination={{{{
            defaultPageSize: TABLE.DEFAULT_PAGE_SIZE,
            showSizeChanger: true,
            showQuickJumper: true,
          }}}}
          toolBarRender={{() => [
            <TableExportButton key="export" fetcher={{fetch{pascal}List}} columns={{columns}} filename="{plural}" />,
            <Button
              key="create"
              type="primary"
              icon={{<PlusOutlined />}}
              size="small"
              onClick={{() => {{
                setEditing(null);
                setDrawerMode('create');
                setDrawerOpen(true);
              }}}}
            >
              {{t('create')}}
            </Button>,
          ]}}
          options={{{{
            density: true,
            fullScreen: true,
            setting: true,
            reload: true,
          }}}}
          size="small"
          bordered
          cardBordered={{false}}
          scroll={{{{ y: tableScrollY }}}}
        />
      </div>

      <{pascal}Drawer
        open={{drawerOpen}}
        mode={{drawerMode}}
        data={{editing}}
        onClose={{() => {{
          setDrawerOpen(false);
          setEditing(null);
        }}}}
        onSuccess={{() => {{
          actionRef.current?.reload();
        }}}}
      />
    </>
  );
}};

export default {pascal}List;
"#,
        pascal = spec.pascal,
        name = spec.name,
        plural = spec.plural,
        columns = list_columns(spec),
        constants_imports = constants_imports(spec),
    )
}

fn constants_imports(spec: &PagesSpec) -> String {
    constants_imports_for(spec, false)
}

/// `for_drawer`：抽屉只用选项集（标签与颜色渲染属于列表）。
fn constants_imports_for(spec: &PagesSpec, for_drawer: bool) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        match &field.kind {
            FieldKind::Bool | FieldKind::Enum(_) => {
                out.push_str(&format!("  {}Options,\n", camel_of(&field.name)));
                if !for_drawer {
                    out.push_str(&format!(
                        "  {}Color,\n  {}Label,\n",
                        camel_of(&field.name),
                        camel_of(&field.name)
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

fn drawer_file(spec: &PagesSpec) -> String {
    let mut set_fields = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        set_fields.push_str(&format!("          {camel}: data.{camel},\n"));
    }
    format!(
        r#"import {{ useEffect, useRef, useState }} from 'react';
import type {{ ProFormInstance }} from '@ant-design/pro-components';
import {{
{drawer_components}}} from '@ant-design/pro-components';
import {{ App }} from 'antd';
import {{ useQueryClient }} from '@tanstack/react-query';
import {{ useTranslation }} from 'react-i18next';

import {{ useCreate{pascal}, useUpdate{pascal} }} from '@/api/hooks/{name}';
import {{
{drawer_constants_imports}}} from './constants';

interface {pascal}DrawerProps {{
  open: boolean;
  mode: 'create' | 'edit';
  data?: any;
  onClose: () => void;
  onSuccess: () => void;
}}

/**
 * {pascal} 编辑/创建抽屉
 */
const {pascal}Drawer: React.FC<{pascal}DrawerProps> = ({{
  open,
  mode,
  data,
  onClose,
  onSuccess,
}}) => {{
  const {{ t }} = useTranslation('{name}');
  const formRef = useRef<ProFormInstance>(null);
  const queryClient = useQueryClient();
  const {{ message }} = App.useApp();

  const [confirmLoading, setConfirmLoading] = useState(false);

  // 编辑模式下设置表单值（destroyOnHidden 时需延迟赋值）
  useEffect(() => {{
    if (open && mode === 'edit' && data) {{
      setTimeout(() => {{
        formRef.current?.setFieldsValue({{
{set_fields}        }});
      }}, 0);
    }}
  }}, [open, mode, data]);

  const createMutation = useCreate{pascal}({{
    onSuccess: () => {{
      message.success(t('createSuccess'));
      queryClient.invalidateQueries({{ queryKey: ['{name}List'] }});
      onSuccess();
      onClose();
    }},
    onError: (error: Error) => {{
      message.error(error.message || t('createFailed'));
    }},
  }});

  const updateMutation = useUpdate{pascal}({{
    onSuccess: () => {{
      message.success(t('updateSuccess'));
      queryClient.invalidateQueries({{ queryKey: ['{name}List'] }});
      onSuccess();
      onClose();
    }},
    onError: (error: Error) => {{
      message.error(error.message || t('updateFailed'));
    }},
  }});

  // 提交表单
  const handleSubmit = async (values: Record<string, any>) => {{
    try {{
      setConfirmLoading(true);
      if (mode === 'edit' && data?.id) {{
        await updateMutation.mutateAsync({{ id: data.id, values }});
      }} else {{
        await createMutation.mutateAsync({{ data: values }});
      }}
      return true;
    }} catch {{
      return false;
    }} finally {{
      setConfirmLoading(false);
    }}
  }};

  return (
    <DrawerForm
      formRef={{formRef}}
      title={{mode === 'create' ? t('create') : t('edit')}}
      open={{open}}
      onOpenChange={{(visible) => {{
        if (!visible) {{
          formRef.current?.resetFields();
          onClose();
        }}
      }}}}
      initialValues={{{{ sortOrder: 1 }}}}
      onFinish={{handleSubmit}}
      submitter={{{{
        searchConfig: {{
          submitText: t('common:button.submit'),
          resetText: t('common:button.cancel'),
        }},
        submitButtonProps: {{
          loading: confirmLoading || createMutation.isPending || updateMutation.isPending,
        }},
        resetButtonProps: {{ onClick: onClose }},
      }}}}
      drawerProps={{{{ destroyOnHidden: true, onClose, placement: 'left', size: 600 }}}}
    >
{controls}    </DrawerForm>
  );
}};

export default {pascal}Drawer;
"#,
        pascal = spec.pascal,
        name = spec.name,
        drawer_components = drawer_components(spec),
        drawer_constants_imports = constants_imports_for(spec, true),
        set_fields = set_fields,
        controls = drawer_controls(spec),
    )
}

fn index_file(spec: &PagesSpec) -> String {
    format!(
        r#"import ContentContainer from '@/layouts/components/PageContainer/ContentContainer';
import {pascal}List from './{pascal}List';

/**
 * {pascal} 管理页面
 */
const {pascal}Management = () => (
  <ContentContainer heightMode="fixed" padding="16px" bottomMargin={{0}}>
    <{pascal}List />
  </ContentContainer>
);

export default {pascal}Management;
"#,
        pascal = spec.pascal,
    )
}

// ---- 菜单种子模板（seed.rs 手术插入） ----

/// 生成的菜单种子函数名（`<name>` 为实体 snake 名）。
fn seed_menu_fn(name: &str) -> String {
    format!("seed_gen_menu_{name}")
}

/// 菜单种子函数体：按 path 幂等（同路径菜单行已存在即跳过），父目录
/// `/<group>` 缺失时先补一行 CATALOG。不能挂进固定 rows 数组——那个
/// 数组所在的 seed_menus 对非空菜单表整体早退，增量菜单只能独立成
/// 函数才能在既有库上生效。
fn seed_menu_function(spec: &PagesSpec) -> String {
    let module = spec.group.to_uppercase();
    let group = &spec.group;
    let menu_path = format!("/{}/{}", spec.group, spec.plural);
    let component = format!("{}/index", menu_path.trim_start_matches('/'));
    format!(
        r#"/// Generated by `rush gen pages` — {pascal} 管理菜单。按 path 幂等：
/// 同路径菜单行已存在即跳过；父目录 /{group} 缺失时先补 CATALOG 行。
async fn {fn_name}(state: &Arc<AppState>) -> Result<(), String> {{
    use crate::data::sys_menus as menus;

    let menu_path = "{menu_path}";
    let exists = menus::Entity::find()
        .filter(menus::Column::Path.eq(menu_path))
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .is_some();
    if exists {{
        return Ok(());
    }}

    let parent_id = match menus::Entity::find()
        .filter(menus::Column::Path.eq("/{group}"))
        .one(&state.db)
        .await
        .map_err(|e| e.to_string())?
    {{
        Some(catalog) => Some(catalog.id),
        None => {{
            let catalog = menus::ActiveModel {{
                type_column: Set(Some("CATALOG".into())),
                path: Set(Some("/{group}".into())),
                name: Set("{group}".into()),
                component: Set(Some("{group}".into())),
                module: Set(Some("{module}".into())),
                meta: Set(Some(serde_json::json!({{
                    "title": "{group}", "icon": "lucide:circle", "order": 90,
                }}))),
                status: Set(Some("ON".into())),
                created_at: Set(Some(crate::data::now())),
                updated_at: Set(Some(crate::data::now())),
                ..Default::default()
            }}
            .insert(&state.db)
            .await
            .map_err(|e| e.to_string())?;
            Some(catalog.id)
        }}
    }};

    insert_seed!(
        &state.db,
        menus::ActiveModel {{
            parent_id: Set(parent_id),
            type_column: Set(Some("MENU".into())),
            path: Set(Some(menu_path.into())),
            name: Set("{plural}".into()),
            component: Set(Some("{component}".into())),
            module: Set(Some("{module}".into())),
            meta: Set(Some(serde_json::json!({{
                "title": "{pascal}", "icon": "lucide:circle", "order": 99,
            }}))),
            status: Set(Some("ON".into())),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }}
    );
    Ok(())
}}
"#,
        fn_name = seed_menu_fn(&spec.name),
        pascal = spec.pascal,
        plural = spec.plural,
        group = group,
        module = module,
        menu_path = menu_path,
        component = component,
    )
}

/// 在 run() 中插入调用行：已有 gen 菜单调用则接在最后一个之后（多次
/// 生成聚成一组），否则紧跟 seed_menus 行。锚点缺失返回 None。
fn insert_seed_call(text: &str, call: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let anchor = lines
        .iter()
        .rposition(|line| line.contains("seed_gen_menu_"))
        .or_else(|| {
            lines
                .iter()
                .position(|line| line.contains("seed_menus(state).await?;"))
        })?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend_from_slice(&lines[..=anchor]);
    out.push(call);
    out.extend_from_slice(&lines[anchor + 1..]);
    Some(out.join("\n"))
}

/// locale JSON（serde_json 生成保证合法）。zh 与 en 的键集一致。
fn locale_file(spec: &PagesSpec, zh: bool) -> String {
    let mut map = serde_json::Map::new();

    let (create, edit) = if zh {
        ("新建", "编辑")
    } else {
        ("Create", "Edit")
    };
    map.insert("moduleName".into(), module_name(spec, zh).into());
    map.insert("create".into(), create.into());
    map.insert("edit".into(), edit.into());
    map.insert("action".into(), if zh { "操作" } else { "Actions" }.into());
    map.insert(
        "sortOrder".into(),
        if zh { "排序号" } else { "Sort Order" }.into(),
    );
    for key in [
        "deleteSuccess",
        "createSuccess",
        "updateSuccess",
        "fetchFailed",
        "deleteFailed",
        "createFailed",
        "updateFailed",
    ] {
        map.insert(
            key.into(),
            serde_json::Value::String(locale_common(key, zh)),
        );
    }
    map.insert(
        "deleteConfirmTitle".into(),
        if zh {
            "确认删除"
        } else {
            "Confirm deletion"
        }
        .into(),
    );
    map.insert(
        "deleteConfirmDesc".into(),
        if zh {
            "确定删除该{moduleName}吗？此操作不可恢复。"
        } else {
            "Delete this {moduleName}? This cannot be undone."
        }
        .into(),
    );
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        map.insert(camel.clone(), field_label(&field.name, zh).into());
        map.insert(
            format!("{camel}Placeholder"),
            if zh {
                format!("请输入{}", field_label(&field.name, zh))
            } else {
                format!("Enter {}", field_label(&field.name, false))
            }
            .into(),
        );
        if spec
            .code_field
            .as_deref()
            .map(|code| code == field.name)
            .unwrap_or(false)
        {
            map.insert(
                format!("required{}", pascal_of(&field.name)),
                if zh {
                    format!("请输入{}", field_label(&field.name, zh))
                } else {
                    format!("{} is required", field_label(&field.name, false))
                }
                .into(),
            );
        }
        if let FieldKind::Enum(values) = &field.kind {
            for (num, text) in &values.values {
                map.insert(format!("{camel}_{num}"), text.clone().into());
            }
        }
        if field.kind == FieldKind::Bool {
            map.insert(
                format!("{camel}_true"),
                if zh { "启用" } else { "Enabled" }.into(),
            );
            map.insert(
                format!("{camel}_false"),
                if zh { "停用" } else { "Disabled" }.into(),
            );
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).expect("locale map 合法") + "\n"
}

fn module_name(spec: &PagesSpec, zh: bool) -> String {
    if zh {
        spec.name.clone()
    } else {
        spec.pascal.clone()
    }
}

fn field_label(name: &str, zh: bool) -> String {
    let _ = zh;
    camel_of(name)
}

fn locale_common(key: &str, zh: bool) -> String {
    let text: &str = match (key, zh) {
        ("deleteSuccess", true) => "删除成功",
        ("deleteSuccess", false) => "Deleted",
        ("createSuccess", true) => "创建成功",
        ("createSuccess", false) => "Created",
        ("updateSuccess", true) => "更新成功",
        ("updateSuccess", false) => "Updated",
        ("fetchFailed", true) => "加载失败",
        ("fetchFailed", false) => "Failed to load",
        ("deleteFailed", true) => "删除失败",
        ("deleteFailed", false) => "Delete failed",
        ("createFailed", true) => "创建失败",
        ("createFailed", false) => "Create failed",
        ("updateFailed", true) => "更新失败",
        ("updateFailed", false) => "Update failed",
        _ => key,
    };
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> PagesSpec {
        PagesSpec {
            name: "widget".to_owned(),
            pascal: "Widget".to_owned(),
            plural: "widgets".to_owned(),
            group: "system".to_owned(),
            route_prefix: "/admin/v1/widgets".to_owned(),
            fields: vec![
                FieldSpec {
                    name: "code".to_owned(),
                    kind: FieldKind::String,
                },
                FieldSpec {
                    name: "state".to_owned(),
                    kind: FieldKind::parse("enum(0=OFF,1=ON)").unwrap(),
                },
            ],
            code_field: Some("code".to_owned()),
            stack: PagesStack::React,
            global: false,
            loaded_spec: None,
        }
    }

    #[test]
    fn hooks_are_self_contained_and_hit_the_right_paths() {
        let hooks = hooks_file(&spec());
        assert!(hooks.contains("path: `admin/v1/widgets${listQueryString(params)}`"));
        assert!(hooks.contains("path: `admin/v1/widgets/${id}`"));
        assert!(hooks.contains("makeUpdateMask(Object.keys(values ?? {}))"));
        assert!(hooks.contains("export interface Widget {"));
        assert!(hooks.contains("state?: number;"), "枚举线上形状是 i32 数值");
    }

    #[test]
    fn pages_follow_the_house_marks() {
        let list = list_file(&spec());
        assert!(list.contains("useTranslation('widget')"));
        assert!(list.contains("fetchWidgetList(query)"));
        assert!(list.contains("rowKey=\"id\""));
        assert!(list.contains("deleteMutation.mutate({ ids: [record.id] })"));

        let drawer = drawer_file(&spec());
        assert!(drawer.contains("useCreateWidget"));
        assert!(drawer.contains("ProFormSelect"));
        assert!(drawer.contains("name=\"code\""));

        let index = index_file(&spec());
        assert!(index.contains("WidgetList"));

        let constants = constants_file(&spec());
        assert!(constants.contains("stateOptions"));
    }

    #[test]
    fn locales_are_valid_json_with_matching_keys() {
        let zh = locale_file(&spec(), true);
        let en = locale_file(&spec(), false);
        let zh_map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&zh).unwrap();
        let en_map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&en).unwrap();
        assert_eq!(
            zh_map.keys().collect::<Vec<_>>(),
            en_map.keys().collect::<Vec<_>>()
        );
        assert!(zh_map.contains_key("moduleName"));
        assert!(zh_map.contains_key("state_0"));
    }

    #[test]
    fn seed_menu_template_is_idempotent_by_path() {
        let function = seed_menu_function(&spec());
        assert!(function.contains(
            "async fn seed_gen_menu_widget(state: &Arc<AppState>) -> Result<(), String>"
        ));
        assert!(function.contains("let menu_path = \"/system/widgets\";"));
        assert!(function.contains("component: Set(Some(\"system/widgets/index\".into())"));
        assert!(function.contains("module: Set(Some(\"SYSTEM\".into())"));
        assert!(
            function.contains("menus::Column::Path.eq(menu_path)"),
            "按 path 查重"
        );
        assert!(
            function.contains("menus::Column::Path.eq(\"/system\")"),
            "父目录查找"
        );
        assert!(
            function.contains("Some(catalog.id)"),
            "父目录缺失时补 CATALOG 并取其 id"
        );
        assert!(
            function.contains("insert_seed!("),
            "菜单行走仓内 insert_seed 宏"
        );
    }

    #[test]
    fn seed_call_lands_after_seed_menus_and_groups_gen_calls() {
        let seed = "pub async fn run(state: &Arc<AppState>) -> Result<(), String> {\n    seed_languages(state).await?;\n    seed_menus(state).await?;\n    seed_admin_user(state).await?;\n    Ok(())\n}\n";
        let call = "    seed_gen_menu_widget(state).await?;";
        let out = insert_seed_call(seed, call).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let menus_at = lines
            .iter()
            .position(|l| l.contains("seed_menus(state)"))
            .unwrap();
        let call_at = lines
            .iter()
            .position(|l| l.contains("seed_gen_menu_widget"))
            .unwrap();
        let admin_at = lines
            .iter()
            .position(|l| l.contains("seed_admin_user(state)"))
            .unwrap();
        assert!(
            menus_at < call_at && call_at < admin_at,
            "紧跟 seed_menus：{out}"
        );

        // 第二个实体接在既有 gen 调用之后（聚成一组）
        let out = insert_seed_call(&out, "    seed_gen_menu_gadget(state).await?;").unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let widget_at = lines
            .iter()
            .position(|l| l.contains("seed_gen_menu_widget"))
            .unwrap();
        let gadget_at = lines
            .iter()
            .position(|l| l.contains("seed_gen_menu_gadget"))
            .unwrap();
        assert!(widget_at < gadget_at);

        assert!(insert_seed_call("no anchors here", call).is_none());
    }

    #[test]
    fn group_must_be_snake_case() {
        let base = || PagesOptions {
            repo_root: PathBuf::from("/tmp/nowhere"),
            name: "widget".to_owned(),
            group: None,
            route_prefix: None,
            fields: vec![FieldSpec {
                name: "code".to_owned(),
                kind: FieldKind::String,
            }],
            code_field: None,
            stack: PagesStack::React,
            global: None,
            overwrite: false,
            dry_run: true,
        };
        assert!(validate(&base()).is_ok(), "缺省 group = system");
        assert!(validate(&PagesOptions {
            group: Some("myGroup".to_owned()),
            ..base()
        })
        .is_err());
        assert!(validate(&PagesOptions {
            group: Some("my_group".to_owned()),
            ..base()
        })
        .is_ok());
    }
}
