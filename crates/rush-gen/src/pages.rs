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
//! 菜单种子是后端的手动步骤（见 gen entity 的提示）。

use std::fs;
use std::path::PathBuf;

use crate::entity::{camel_of, pascal_of, plural_of, FieldKind, FieldSpec};
use crate::{Error, Result};

/// `rush gen pages` 选项。
#[derive(Debug, Clone)]
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
    /// 只报告不落盘。
    pub dry_run: bool,
}

/// 生成结果报告。
#[derive(Debug, Default)]
pub struct PagesReport {
    pub created: Vec<PathBuf>,
    pub skipped: Vec<String>,
    pub notes: Vec<String>,
}

/// 派生好的命名全集。
struct PagesSpec {
    name: String,
    pascal: String,
    plural: String,
    group: String,
    route_prefix: String,
    fields: Vec<FieldSpec>,
    code_field: Option<String>,
}

fn validate(opts: &PagesOptions) -> Result<PagesSpec> {
    let mut seen = std::collections::BTreeSet::new();
    for field in &opts.fields {
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
    Ok(PagesSpec {
        name: opts.name.clone(),
        pascal: pascal_of(&opts.name),
        plural,
        group: opts.group.clone().unwrap_or_else(|| "system".to_owned()),
        route_prefix: opts
            .route_prefix
            .clone()
            .unwrap_or_else(|| format!("/admin/v1/{}", plural_of(&opts.name))),
        fields: opts.fields.clone(),
        code_field,
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

/// 生成页面组。新文件拒绝覆盖。
pub fn generate_pages(opts: &PagesOptions) -> Result<PagesReport> {
    let spec = validate(opts)?;
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
            hooks_file(&spec),
        ),
        (page_dir.join("constants.ts"), constants_file(&spec)),
        (
            page_dir.join(format!("{}List.tsx", spec.pascal)),
            list_file(&spec),
        ),
        (
            page_dir.join(format!("{}Drawer.tsx", spec.pascal)),
            drawer_file(&spec),
        ),
        (page_dir.join("index.tsx"), index_file(&spec)),
        (
            react_root.join(format!("src/locales/zh-CN/_modules/{}.json", spec.name)),
            locale_file(&spec, true),
        ),
        (
            react_root.join(format!("src/locales/en-US/_modules/{}.json", spec.name)),
            locale_file(&spec, false),
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

    let mut report = PagesReport::default();
    if opts.dry_run {
        report.created = files.iter().map(|(p, _)| p.clone()).collect();
        return Ok(report);
    }

    for (path, content) in &files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        report.created.push(path.clone());
    }

    report
        .notes
        .push("菜单是后端种子驱动的：在 seed.rs 加菜单项（组件路径 app/<group>/<plural>/index）后页面才会出现在导航".to_owned());
    report
        .notes
        .push("生成后建议在 frontend/admin/react 下运行 pnpm typecheck".to_owned());
    Ok(report)
}

// ---- hooks 模板 ----

fn ts_interface(spec: &PagesSpec) -> String {
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
        false // v1: 页面生成仅面向租户实体（与 gen entity 的 --global 互为手动步骤）
    }
    /// 传输路径（无前导斜杠，与生成 TS 客户端一致）。
    fn base_path(&self) -> String {
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
}
