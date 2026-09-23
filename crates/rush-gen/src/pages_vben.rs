//! vben 栈页面生成（`rush gen pages --stack vben`）：为实体生成
//! vue-vben 前端的页面链。模板基准是仓内最小的单实体页链
//! `views/app/system/language`（index.vue 列表 + <name>-drawer.vue 编辑
//! 抽屉，vxe-grid + useVbenForm）。
//!
//! 与 react 栈的两个结构差：
//! * 菜单由**前端静态路由**驱动（`router/routes/modules/app/<group>.ts`
//!   的条目即菜单），不写后端 seed.rs——modules 目录被 import.meta.glob
//!   自动拾取，新分组整文件创建，既有分组向 children 手术插入；
//! * 数据层生成**自包含 composables**（`src/api/composables/<name>.ts`，
//!   本地类型 + requestApi 直连），与 react 栈同哲学——不依赖上游重新
//!   生成 TS 客户端（apiClient 里还没有生成实体的服务）。
//!
//! 文案策略：字段标签用字段名占位（`page.<name>.*` 命名空间留给人工本
//! 地化），框架性文案复用仓内已有的 `ui.*` 通用键。vben 的 .vue 与路由
//! 模块文件带 UTF-8 BOM（仓内现状），生成物保持一致。

use std::fs;
use std::path::PathBuf;

use super::pages::{PagesOptions, PagesReport, PagesSpec};
use crate::entity::{camel_of, pascal_of, FieldKind};
use crate::{Error, Result};

const BOM: &str = "\u{FEFF}";

/// token 替换式模板渲染：@@KEY@@ → 值（Vue/TS 模板花括号密集，不用
/// format! 以免转义吞掉模板本体）。
fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_owned();
    for (key, value) in vars {
        out = out.replace(&format!("@@{key}@@"), value);
    }
    out
}

pub(crate) fn generate(opts: &PagesOptions, spec: &PagesSpec) -> Result<PagesReport> {
    let app_root = opts.repo_root.join("frontend/admin/vue-vben/apps/admin");
    if !app_root.join("package.json").is_file() {
        return Err(Error::InvalidInput(format!(
            "目标仓缺少 vue-vben 前端：{}（frontend/admin/vue-vben/apps/admin 不在位？）",
            app_root.display()
        )));
    }

    let view_dir = app_root
        .join("src/views/app")
        .join(&spec.group)
        .join(&spec.plural);
    let composable = app_root.join(format!("src/api/composables/{}.ts", spec.name));
    let route_module = app_root.join(format!("src/router/routes/modules/app/{}.ts", spec.group));

    let files: Vec<(PathBuf, String)> = vec![
        (composable.clone(), composable_file(spec)),
        (
            view_dir.join("index.vue"),
            BOM.to_owned() + &index_vue(spec),
        ),
        (
            view_dir.join(format!("{}-drawer.vue", spec.name)),
            BOM.to_owned() + &drawer_vue(spec),
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

    // 路由模块：新分组整文件创建（glob 自动拾取），既有分组手术插入
    // children（写入前校验锚点，避免半套生成）。
    let component_path = format!("#/views/app/{}/{}/index.vue", spec.group, spec.plural);
    let mut route_created = false;
    let mut route_skipped = false;
    let mut route_text = String::new();
    if route_module.is_file() {
        let text = fs::read_to_string(&route_module)?;
        if text.contains(&component_path) {
            route_skipped = true;
        } else {
            route_text = insert_route_child(&text, &route_child_entry(spec))?;
        }
    } else {
        route_created = true;
        route_text = BOM.to_owned() + &route_module_new(spec);
    }

    let mut report = PagesReport::default();
    if opts.dry_run {
        report.created = files.iter().map(|(p, _)| p.clone()).collect();
        if route_created {
            report.created.push(route_module.clone());
        } else if !route_skipped {
            report.edited.push(route_module.clone());
        } else {
            report
                .skipped
                .push(format!("{}（路由条目已在位）", route_module.display()));
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
    if route_created {
        if let Some(parent) = route_module.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&route_module, &route_text)?;
        report.created.push(route_module.clone());
    } else if !route_skipped {
        fs::write(&route_module, &route_text)?;
        report.edited.push(route_module.clone());
    } else {
        report
            .skipped
            .push(format!("{}（路由条目已在位）", route_module.display()));
    }

    if route_created {
        report.notes.push(format!(
            "新路由模块已创建：{}（import.meta.glob 自动拾取，无需注册）",
            route_module.display()
        ));
    } else if !route_skipped {
        report.notes.push(format!(
            "路由条目已插入：{}（vben 菜单由前端静态路由驱动，无需后端 seed）",
            route_module.display()
        ));
    }
    report.notes.push(
        "字段标签用字段名占位：本地化时在 src/locales/langs/*/page.json 增补 page.<name> 命名空间并替换页面里的字面量"
            .to_owned(),
    );
    report
        .notes
        .push("生成后建议在 frontend/admin/vue-vben 下运行仓内的 typecheck/lint 脚本".to_owned());
    Ok(report)
}

/// children 块内插入子路由：`children: [` 之后第一个 `],`（4 空格缩进）
/// 前落位；组件路径串为幂等标记（调用方已查）。锚点缺失报错。
fn insert_route_child(text: &str, entry: &str) -> Result<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let children_at = lines
        .iter()
        .position(|line| line.contains("children: ["))
        .ok_or_else(|| Error::InvalidInput("路由模块中找不到 children: [ 锚点".to_owned()))?;
    let close_at = lines
        .iter()
        .skip(children_at)
        .position(|line| line.trim() == "],")
        .map(|at| children_at + at)
        .ok_or_else(|| Error::InvalidInput("路由模块中找不到 children 关闭锚点".to_owned()))?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + entry.lines().count());
    out.extend_from_slice(&lines[..close_at]);
    for line in entry.lines() {
        out.push(line);
    }
    out.extend_from_slice(&lines[close_at..]);
    Ok(out.join("\n"))
}

fn route_child_entry(spec: &PagesSpec) -> String {
    render(
        r#"      {
        path: '@@PLURAL@@',
        name: '@@PASCAL@@Management',
        meta: {
          icon: 'lucide:circle',
          title: '@@PASCAL@@',
        },
        component: () => import('#/views/app/@@GROUP@@/@@PLURAL@@/index.vue'),
      },
"#,
        &[
            ("PLURAL", spec.plural.clone()),
            ("PASCAL", spec.pascal.clone()),
            ("GROUP", spec.group.clone()),
        ],
    )
}

fn route_module_new(spec: &PagesSpec) -> String {
    render(
        r#"import type { RouteRecordRaw } from 'vue-router';

import { BasicLayout } from '#/layouts';

const @@GROUP@@: RouteRecordRaw[] = [
  {
    path: '/@@GROUP@@',
    name: '@@GROUP_PASCAL@@',
    component: BasicLayout,
    redirect: '/@@GROUP@@/@@PLURAL@@',
    meta: {
      order: 2100,
      icon: 'lucide:circle',
      title: '@@GROUP_PASCAL@@',
      keepAlive: true,
    },
    children: [
@@CHILD@@    ],
  },
];

export default @@GROUP@@;
"#,
        &[
            ("GROUP", spec.group.clone()),
            ("GROUP_PASCAL", pascal_of(&spec.group)),
            ("PLURAL", spec.plural.clone()),
            ("CHILD", route_child_entry(spec)),
        ],
    )
}

// ---- composables 模板 ----

/// 每个枚举/布尔字段的选项与渲染助手（自包含，不依赖 #/api 的生成桶）。
fn helper_code(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(values) => {
                out.push_str(&format!("export const {camel}Options = [\n"));
                for (num, text) in &values.values {
                    out.push_str(&format!("  {{ label: '{text}', value: {num} }},\n"));
                }
                out.push_str("];\n\n");
                out.push_str(&format!(
                    "export function {camel}ToColor(value?: number) {{\n  return (({camel}Options.find((option) => option.value === value)?.value ?? 0)) === 0 ? 'default' : 'processing';\n}}\n\n"
                ));
                out.push_str(&format!(
                    "export function {camel}ToLabel(value?: number) {{\n  return {camel}Options.find((option) => option.value === value)?.label ?? String(value ?? 0);\n}}\n\n"
                ));
            }
            FieldKind::Bool => {
                out.push_str(&format!(
                    "export function {camel}ToColor(value?: boolean) {{\n  return value ? 'success' : 'default';\n}}\n\n"
                ));
                out.push_str(&format!(
                    "export function {camel}ToLabel(value?: boolean) {{\n  return value ? '启用' : '停用';\n}}\n\n"
                ));
            }
            _ => {}
        }
    }
    out
}

fn composable_file(spec: &PagesSpec) -> String {
    render(
        r#"/**
 * @@PASCAL@@ 页面的数据层：rush gen pages 生成（自包含）。
 *
 * 本地类型 + requestApi 直连，与仓内生成的 TS 客户端走同一条 axios
 * 通道（token 注入、错误拦截照常生效）；上游客户端重生成本服务后可切
 * apiClient。布尔/枚举的选项与渲染助手也在此文件。
 */
import {
  useMutation,
  type UseMutationOptions,
  useQuery,
  type UseQueryOptions,
} from '@tanstack/vue-query';

@@REST_IMPORT@@

// 本地接口：字段形状与 @@PASCAL@@ 的 proto 契约一致（protojson 线上形状）。
export interface @@PASCAL@@ {
@@TS_INTERFACE@@}

export interface List@@PASCAL@@Response {
  items?: @@PASCAL@@[];
  total?: number;
}

function listQueryString(query: PaginationQuery) {
  const raw = query.toRawParams();
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(raw)) {
    if (value !== undefined && value !== null && value !== '') {
      params.set(key, String(value));
    }
  }
  const qs = params.toString();
  return qs ? `?${qs}` : '';
}

export async function fetch@@PASCAL@@List(params: PaginationQuery) {
  return requestApi({
    path: `@@BASE@@${listQueryString(params)}`,
    method: 'GET',
    body: null,
  }) as Promise<List@@PASCAL@@Response>;
}

export function use@@PASCAL@@List(
  query: PaginationQuery,
  options?: UseQueryOptions<List@@PASCAL@@Response, Error>,
) {
  return useQuery({
    queryKey: ['@@NAME@@List', query],
    queryFn: () => fetch@@PASCAL@@List(query),
    ...options,
  });
}

export function useCreate@@PASCAL@@(
  options?: UseMutationOptions<{}, Error, Record<string, any>>,
) {
  return useMutation({
    mutationFn: (values) =>
      requestApi({
        path: '@@BASE@@',
        method: 'POST',
        body: JSON.stringify({ data: values }),
      }) as Promise<{}>,
    ...options,
  });
}

export function useUpdate@@PASCAL@@(
  options?: UseMutationOptions<
    {},
    Error,
    { id: number; values: Record<string, any> }
  >,
) {
  return useMutation({
    mutationFn: ({ id, values }) =>
      requestApi({
        path: `@@BASE@@/${id}`,
        method: 'PUT',
        body: JSON.stringify({
          id,
          data: values,
          updateMask: makeUpdateMask(Object.keys(values ?? {})),
        }),
      }) as Promise<{}>,
    ...options,
  });
}

export function useDelete@@PASCAL@@(
  options?: UseMutationOptions<{}, Error, { ids: number[] }>,
) {
  return useMutation({
    mutationFn: (input) =>
      requestApi({
        path: '@@BASE@@',
        method: 'DELETE',
        body: JSON.stringify(input),
      }) as Promise<{}>,
    ...options,
  });
}

@@HELPERS@@"#,
        &[
            ("PASCAL", spec.pascal.clone()),
            ("NAME", spec.name.clone()),
            ("BASE", spec.base_path()),
            ("REST_IMPORT", rest_import(spec)),
            ("TS_INTERFACE", super::pages::ts_interface(spec)),
            ("HELPERS", helper_code(spec)),
        ],
    )
}

/// 自包含 composables 的传输层导入：vben 别名 #/transport/rest。只导
/// 传输三件套——布尔/枚举助手是本文件的本地导出，混进导入会撞出
/// TS2395 合并声明冲突。
fn rest_import(_spec: &PagesSpec) -> String {
    "import { makeUpdateMask, PaginationQuery, requestApi } from '#/transport/rest';".to_owned()
}

// ---- 列表页模板（index.vue，基准：language/index.vue） ----

/// 搜索 schema：仅 string 字段进搜索（数值/布尔/枚举留给列渲染）。
fn search_schema(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        if field.kind != FieldKind::String {
            continue;
        }
        let camel = camel_of(&field.name);
        out.push_str(&render(
            r#"    {
      component: 'Input',
      fieldName: '@@CAMEL@@',
      label: '@@CAMEL@@',
      componentProps: {
        placeholder: $t('ui.placeholder.input'),
        allowClear: true,
      },
    },
"#,
            &[("CAMEL", camel)],
        ));
    }
    out
}

/// 列定义：业务字段全列（布尔/枚举走 slot），标准尾列 + 操作列。
fn grid_columns(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let slot = matches!(field.kind, FieldKind::Bool | FieldKind::Enum(_))
            .then(|| format!("      slots: {{ default: '{camel}' }},\n"));
        out.push_str(&render(
            r#"    {
      title: '@@CAMEL@@',
      field: '@@CAMEL@@',
@@SLOT@@      minWidth: 110,
    },
"#,
            &[("CAMEL", camel), ("SLOT", slot.unwrap_or_default())],
        ));
    }
    out.push_str(
        r#"    {
      title: $t('ui.table.sortOrder'),
      field: 'sortOrder',
      minWidth: 90,
    },
    {
      title: $t('ui.table.createdAt'),
      field: 'createdAt',
      formatter: 'formatDateTime',
      minWidth: 140,
    },
    {
      title: $t('ui.table.action'),
      field: 'action',
      fixed: 'right',
      slots: { default: 'action' },
      minWidth: 90,
    },
"#,
    );
    out
}

/// 布尔/枚举列的模板 slot（Tag 渲染）。
fn grid_slots(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(_) => out.push_str(&render(
                r#"      <template #@@CAMEL@@="{ row }">
        <a-tag :color="@@CAMEL@@ToColor(row.@@CAMEL@@)">
          {{ @@CAMEL@@ToLabel(row.@@CAMEL@@) }}
        </a-tag>
      </template>
"#,
                &[("CAMEL", camel)],
            )),
            FieldKind::Bool => out.push_str(&render(
                r#"      <template #@@CAMEL@@="{ row }">
        <a-tag :color="@@CAMEL@@ToColor(row.@@CAMEL@@)">
          {{ @@CAMEL@@ToLabel(row.@@CAMEL@@) }}
        </a-tag>
      </template>
"#,
                &[("CAMEL", camel)],
            )),
            _ => {}
        }
    }
    out
}

/// index.vue 里从 composable 导入的名字清单（未用导入会挂下游 lint，
/// 按字段形状裁剪）。
fn index_imports(spec: &PagesSpec) -> String {
    let mut names: Vec<String> = vec![format!("fetch{}List", spec.pascal)];
    let mut has_bool = false;
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(_) => {
                names.push(format!("{camel}ToColor"));
                names.push(format!("{camel}ToLabel"));
            }
            FieldKind::Bool => {
                has_bool = true;
                names.push(format!("{camel}ToColor"));
                names.push(format!("{camel}ToLabel"));
            }
            _ => {}
        }
    }
    if has_bool {
        names.push("boolToColor".to_owned());
        names.push("boolToLabel".to_owned());
    }
    names.push(format!("useDelete{}", spec.pascal));
    let mut sorted: Vec<String> = names;
    sorted.sort();
    sorted.join(",\n  ")
}

fn index_vue(spec: &PagesSpec) -> String {
    render(
        r#"<script lang="ts" setup>
import type { VxeGridProps } from '#/adapter/vxe-table';

import { h } from 'vue';

import { Page, useVbenDrawer, type VbenFormProps } from '@vben/common-ui';
import { LucideFilePenLine, LucideTrash2 } from '@vben/icons';

import { notification } from 'ant-design-vue';

import { useVbenVxeGrid } from '#/adapter/vxe-table';
import TableExportButton from '#/components/TableExportButton.vue';
import {
@@INDEX_IMPORTS@@
} from '#/api/composables/@@NAME@@';
import { PaginationQuery } from '#/transport/rest';
import { $t } from '#/locales';

import @@PASCAL@@Drawer from './@@NAME@@-drawer.vue';

const { mutateAsync: delete@@PASCAL@@ } = useDelete@@PASCAL@@();

const formOptions: VbenFormProps = {
  collapsed: false,
  showCollapseButton: false,
  submitOnEnter: true,
  schema: [
@@SEARCH_SCHEMA@@  ],
};

const gridOptions: VxeGridProps = {
  toolbarConfig: {
    custom: true,
    export: true,
    refresh: true,
    zoom: true,
  },
  height: 'auto',
  exportConfig: {},
  pagerConfig: {},
  rowConfig: {
    isHover: true,
  },
  stripe: true,

  proxyConfig: {
    ajax: {
      query: async ({ page }, formValues) => {
        return await fetch@@PASCAL@@List(
          new PaginationQuery({
            paging: {
              page: page.currentPage,
              pageSize: page.pageSize,
            },
            formValues,
          }),
        );
      },
    },
  },

  columns: [
@@COLUMNS@@  ],
};

const exportFetcher = (page: number, pageSize: number) =>
  fetch@@PASCAL@@List(new PaginationQuery({ paging: { page, pageSize } }));

const [Grid, gridApi] = useVbenVxeGrid({ gridOptions, formOptions });

const [Drawer, drawerApi] = useVbenDrawer({
  connectedComponent: @@PASCAL@@Drawer,

  onOpenChange(isOpen: boolean) {
    if (!isOpen) {
      gridApi.reload();
    }
  },
});

function openDrawer(create: boolean, row?: any) {
  drawerApi.setData({
    create,
    row,
  });

  drawerApi.open();
}

function handleCreate() {
  openDrawer(true);
}

function handleEdit(row: any) {
  openDrawer(false, row);
}

async function handleDelete(row: any) {
  try {
    await delete@@PASCAL@@({ ids: [row.id] });

    notification.success({
      message: $t('ui.notification.delete_success'),
    });

    await gridApi.reload();
  } catch {
    notification.error({
      message: $t('ui.notification.delete_failed'),
    });
  }
}
</script>

<template>
  <Page auto-content-height>
    <Grid :table-title="'@@PASCAL@@'">
      <template #toolbar-tools>
        <a-button type="primary" class="mr-2" @click="handleCreate">
          新建
        </a-button>
        <TableExportButton :fetcher="exportFetcher" :columns="gridOptions.columns" filename="@@PLURAL@@" />
      </template>
@@GRID_SLOTS@@      <template #action="{ row }">
        <a-button
          type="link"
          :icon="h(LucideFilePenLine)"
          @click.stop="handleEdit(row)"
        />
        <a-popconfirm
          :cancel-text="$t('ui.button.cancel')"
          :ok-text="$t('ui.button.ok')"
          :title="$t('ui.text.do_you_want_delete', { moduleName: '@@PASCAL@@' })"
          @confirm="handleDelete(row)"
        >
          <a-button danger type="link" :icon="h(LucideTrash2)" />
        </a-popconfirm>
      </template>
    </Grid>
    <Drawer />
  </Page>
</template>
"#,
        &[
            ("PASCAL", spec.pascal.clone()),
            ("NAME", spec.name.clone()),
            ("PLURAL", spec.plural.clone()),
            ("INDEX_IMPORTS", index_imports(spec)),
            ("SEARCH_SCHEMA", search_schema(spec)),
            ("COLUMNS", grid_columns(spec)),
            ("GRID_SLOTS", grid_slots(spec)),
        ],
    )
}

// ---- 抽屉模板（基准：language/language-drawer.vue） ----

fn drawer_form_schema(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let required = spec
            .code_field
            .as_deref()
            .map(|code| code == field.name)
            .unwrap_or(false);
        let rules = if required {
            "      rules: 'required',\n"
        } else {
            ""
        };
        let component_block = match &field.kind {
            FieldKind::Enum(_) => render(
                r#"    {
      component: 'Select',
      fieldName: '@@CAMEL@@',
      label: '@@CAMEL@@',
@@RULES@@      componentProps: {
        options: @@CAMEL@@Options,
      },
    },
"#,
                &[("CAMEL", camel.clone()), ("RULES", rules.to_owned())],
            ),
            FieldKind::Bool => render(
                r#"    {
      component: 'Switch',
      fieldName: '@@CAMEL@@',
      label: '@@CAMEL@@',
      componentProps: {
        class: 'w-auto',
      },
    },
"#,
                &[("CAMEL", camel.clone())],
            ),
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => render(
                r#"    {
      component: 'InputNumber',
      fieldName: '@@CAMEL@@',
      label: '@@CAMEL@@',
@@RULES@@    },
"#,
                &[("CAMEL", camel.clone()), ("RULES", rules.to_owned())],
            ),
            FieldKind::String => render(
                r#"    {
      component: 'Input',
      fieldName: '@@CAMEL@@',
      label: '@@CAMEL@@',
@@RULES@@      componentProps: {
        placeholder: $t('ui.placeholder.input'),
        allowClear: true,
      },
    },
"#,
                &[("CAMEL", camel.clone()), ("RULES", rules.to_owned())],
            ),
        };
        out.push_str(&component_block);
    }
    out.push_str(
        r#"    {
      component: 'InputNumber',
      fieldName: 'sortOrder',
      defaultValue: 1,
      label: $t('ui.table.sortOrder'),
    },
"#,
    );
    out
}

/// drawer 里从 composable 导入的名字（枚举 Select 的 options）。
fn drawer_imports(spec: &PagesSpec) -> String {
    let mut names: Vec<String> = vec![
        format!("useCreate{}", spec.pascal),
        format!("useUpdate{}", spec.pascal),
    ];
    for field in &spec.fields {
        if let FieldKind::Enum(_) = &field.kind {
            names.push(format!("{}Options", camel_of(&field.name)));
        }
    }
    names.sort();
    names.join(",\n  ")
}

fn drawer_vue(spec: &PagesSpec) -> String {
    render(
        r#"<script lang="ts" setup>
import { computed, ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { notification } from 'ant-design-vue';

import { useVbenForm } from '#/adapter/form';
import {
@@DRAWER_IMPORTS@@
} from '#/api/composables/@@NAME@@';

const { mutateAsync: create@@PASCAL@@ } = useCreate@@PASCAL@@();
const { mutateAsync: update@@PASCAL@@ } = useUpdate@@PASCAL@@();

const data = ref();

const getTitle = computed(() =>
  data.value?.create ? '新建 @@PASCAL@@' : '编辑 @@PASCAL@@',
);

const [BaseForm, baseFormApi] = useVbenForm({
  showDefaultActions: false,
  commonConfig: {
    componentProps: {
      class: 'w-full',
    },
  },
  schema: [
@@FORM_SCHEMA@@  ],
});

const [Drawer, drawerApi] = useVbenDrawer({
  onCancel() {
    drawerApi.close();
  },

  async onConfirm() {
    const validate = await baseFormApi.validate();
    if (!validate.valid) {
      return;
    }

    setLoading(true);

    const values = await baseFormApi.getValues();

    try {
      await (data.value?.create
        ? create@@PASCAL@@(values)
        : update@@PASCAL@@({ id: data.value.row.id, values }));

      notification.success({
        message: data.value?.create
          ? $t('ui.notification.create_success')
          : $t('ui.notification.update_success'),
      });
    } catch {
      notification.error({
        message: data.value?.create
          ? $t('ui.notification.create_failed')
          : $t('ui.notification.update_failed'),
      });
    } finally {
      drawerApi.close();
      setLoading(false);
    }
  },

  onOpenChange(isOpen: boolean) {
    if (isOpen) {
      data.value = drawerApi.getData<Record<string, any>>();

      if (data.value.row !== undefined) {
        baseFormApi.setValues(data.value?.row);
      }

      setLoading(false);
    }
  },
});

function setLoading(loading: boolean) {
  drawerApi.setState({ confirmLoading: loading });
}
</script>

<template>
  <Drawer :title="getTitle">
    <BaseForm />
  </Drawer>
</template>
"#,
        &[
            ("PASCAL", spec.pascal.clone()),
            ("NAME", spec.name.clone()),
            ("DRAWER_IMPORTS", drawer_imports(spec)),
            ("FORM_SCHEMA", drawer_form_schema(spec)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::FieldSpec;

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
                FieldSpec {
                    name: "is_active".to_owned(),
                    kind: FieldKind::Bool,
                },
            ],
            code_field: Some("code".to_owned()),
            stack: crate::pages::PagesStack::Vben,
            global: false,
            loaded_spec: None,
        }
    }

    #[test]
    fn composable_is_self_contained_with_helpers() {
        let file = composable_file(&spec());
        assert!(file.contains("export interface Widget {"));
        assert!(file.contains("state?: number;"), "枚举线上形状是 i32 数值");
        assert!(
            file.contains("} from '#/transport/rest';"),
            "vben 别名导入：{file}"
        );
        assert!(file.contains("export const stateOptions = ["));
        assert!(file.contains("export function isActiveToColor(value?: boolean)"));
        assert!(file.contains("path: `admin/v1/widgets${listQueryString(params)}`"));
        assert!(file.contains("updateMask: makeUpdateMask(Object.keys(values ?? {}))"));
    }

    #[test]
    fn index_and_drawer_carry_the_house_marks() {
        let index = index_vue(&spec());
        assert!(index.starts_with("<script lang=\"ts\" setup>"));
        assert!(index.contains("useVbenVxeGrid({ gridOptions, formOptions })"));
        assert!(index.contains("deleteWidget({ ids: [row.id] })"));
        assert!(
            index.contains("<template #state=\"{ row }\">"),
            "枚举列 slot：{index}"
        );
        assert!(index.contains("stateToColor(row.state)"));

        let drawer = drawer_vue(&spec());
        assert!(!drawer.contains("connectedComponent"), "连接在 index 侧");
        assert!(drawer.contains("component: 'Select'"), "枚举表单控件");
        assert!(
            drawer.contains("fieldName: 'code',\n      label: 'code',\n      rules: 'required',")
        );
        assert!(drawer.contains("component: 'Switch'"), "布尔表单控件");
    }

    #[test]
    fn route_module_new_and_child_entry() {
        let module = route_module_new(&spec());
        assert!(module.starts_with("import type { RouteRecordRaw }"));
        assert!(module.contains("import { BasicLayout } from '#/layouts';"));
        assert!(module.contains("const system: RouteRecordRaw[] = ["));
        assert!(module.contains("component: () => import('#/views/app/system/widgets/index.vue'),"));

        let child = route_child_entry(&spec());
        assert!(child.contains("name: 'WidgetManagement',"));

        // 既有模块：插入 children 块，保留原文
        let existing = "import type { RouteRecordRaw } from 'vue-router';\n\nimport { BasicLayout } from '#/layouts';\n\nconst system: RouteRecordRaw[] = [\n  {\n    path: '/system',\n    children: [\n      {\n        path: 'user',\n      },\n    ],\n  },\n];\n";
        let out = insert_route_child(existing, &child).unwrap();
        assert!(out.contains("path: 'widgets',"));
        let user_at = out.find("path: 'user',").unwrap();
        let widget_at = out.find("path: 'widgets',").unwrap();
        let close_at = out.find("    ],").unwrap();
        assert!(user_at < widget_at && widget_at < close_at);

        let err = insert_route_child("没有锚点", &child).unwrap_err();
        assert!(format!("{err}").contains("锚点"));
    }
}
