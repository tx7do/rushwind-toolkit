//! element 栈页面生成（`rush gen pages --stack element`）：为实体生成
//! vue-element 前端的页面链。模板基准是仓内最小的单实体页链
//! `pages/app/system/language`（index.vue 的 ProPage 配置式列表 +
//! <name>-drawer.vue 的 ProModal/ElForm 编辑抽屉）。
//!
//! 与 react 栈的结构差同 vben：菜单由**前端静态路由**驱动
//! （`router/routes/modules/app/<group>.ts`），不写后端 seed.rs；数据层
//! 生成**自包含 composables**（本地类型 + requestApi 直连，走
//! `@/core/transport/rest` 的同一条 axios 通道），不依赖上游重新生成
//! TS 客户端。
//!
//! 文案策略：字段标签用字段名占位（`pages.<name>.*` 命名空间留给人工本
//! 地化），框架性文案复用仓内已有的 `common.*` 通用键。element 栈文件
//! 不带 BOM、双引号 + 分号（仓内现状）。

use std::fs;
use std::path::PathBuf;

use super::pages::{PagesOptions, PagesReport, PagesSpec};
use crate::entity::{camel_of, pascal_of, FieldKind};
use crate::{Error, Result};

/// token 替换式模板渲染：@@KEY@@ → 值。
fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_owned();
    for (key, value) in vars {
        out = out.replace(&format!("@@{key}@@"), value);
    }
    out
}

pub(crate) fn generate(opts: &PagesOptions, spec: &PagesSpec) -> Result<PagesReport> {
    let app_root = opts.repo_root.join("frontend/admin/vue-element");
    if !app_root.join("package.json").is_file() {
        return Err(Error::InvalidInput(format!(
            "目标仓缺少 vue-element 前端：{}（frontend/admin/vue-element 不在位？）",
            app_root.display()
        )));
    }

    let page_dir = app_root
        .join("src/pages/app")
        .join(&spec.group)
        .join(&spec.plural);
    let composable = app_root.join(format!("src/api/composables/{}.ts", spec.name));
    let route_module = app_root.join(format!("src/router/routes/modules/app/{}.ts", spec.group));

    let files: Vec<(PathBuf, String)> = vec![
        (composable.clone(), composable_file(spec)),
        (page_dir.join("index.vue"), index_vue(spec)),
        (
            page_dir.join(format!("{}-drawer.vue", spec.name)),
            drawer_vue(spec),
        ),
        (
            app_root.join(format!("src/locales/zh-CN/pages/{}.json", spec.name)),
            locale_file(spec, true),
        ),
        (
            app_root.join(format!("src/locales/en-US/pages/{}.json", spec.name)),
            locale_file(spec, false),
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

    let component_path = format!("@/pages/app/{}/{}/index.vue", spec.group, spec.plural);
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
        route_text = route_module_new(spec);
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
            "路由条目已插入：{}（element 菜单由前端静态路由驱动，无需后端 seed）",
            route_module.display()
        ));
    }
    report.notes.push(
        "字段文案在 src/locales/{zh-CN,en-US}/pages/<name>.json——翻译直接改这两份".to_owned(),
    );
    report.notes.push(
        "生成后建议在 frontend/admin/vue-element 下运行仓内的 typecheck/lint 脚本".to_owned(),
    );
    Ok(report)
}

/// children 块内插入子路由条目（锚点缺失报错；幂等由调用方查组件路径）。
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
        path: "@@PLURAL@@",
        name: "@@PASCAL@@Management",
        meta: {
          icon: "lucide:circle",
          title: "@@PASCAL@@",
        },
        component: () => import("@/pages/app/@@GROUP@@/@@PLURAL@@/index.vue"),
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
        r#"import type { RouteRecordRaw } from "vue-router";
import { Layout } from "@/layouts";

const @@GROUP@@: RouteRecordRaw[] = [
  {
    path: "/@@GROUP@@",
    name: "@@GROUP_PASCAL@@",
    component: Layout,
    redirect: "/@@GROUP@@/@@PLURAL@@",
    meta: {
      order: 2100,
      icon: "lucide:circle",
      title: "@@GROUP_PASCAL@@",
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

// ---- composables 模板（双引号 + 分号风格） ----

fn helper_code(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(values) => {
                out.push_str(&format!("export const {camel}Options = [\n"));
                for (num, text) in &values.values {
                    out.push_str(&format!("  {{ label: \"{text}\", value: {num} }},\n"));
                }
                out.push_str("];\n\n");
                out.push_str(&format!(
                    "export function {camel}ToColor(value?: number) {{\n  return (({camel}Options.find((option) => option.value === value)?.value ?? 0)) === 0 ? \"info\" : \"success\";\n}}\n\n"
                ));
                out.push_str(&format!(
                    "export function {camel}ToLabel(value?: number) {{\n  return {camel}Options.find((option) => option.value === value)?.label ?? String(value ?? 0);\n}}\n\n"
                ));
            }
            FieldKind::Bool => {
                out.push_str(&format!(
                    "export function {camel}ToColor(value?: boolean) {{\n  return value ? \"success\" : \"info\";\n}}\n\n"
                ));
                out.push_str(&format!(
                    "export function {camel}ToLabel(value?: boolean) {{\n  return value ? \"启用\" : \"停用\";\n}}\n\n"
                ));
            }
            _ => {}
        }
    }
    out
}

fn rest_import(_spec: &PagesSpec) -> String {
    "import {
  makeUpdateMask,
  PaginationQuery,
  requestApi,
} from \"@/core/transport/rest\";"
        .to_owned()
}

fn composable_file(spec: &PagesSpec) -> String {
    render(
        r#"/**
 * @@PASCAL@@ 页面的数据层：rush gen pages 生成（自包含）。
 *
 * 本地类型 + requestApi 直连，与仓内生成的 TS 客户端走同一条 axios
 * 通道；上游客户端重生成本服务后可切 apiClient。布尔/枚举的选项与渲染
 * 助手也在此文件。
 */
import {
  useMutation,
  type UseMutationOptions,
  useQuery,
  type UseQueryOptions,
} from "@tanstack/vue-query";

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
    if (value !== undefined && value !== null && value !== "") {
      params.set(key, String(value));
    }
  }
  const qs = params.toString();
  return qs ? `?${qs}` : "";
}

export async function fetch@@PASCAL@@List(params: PaginationQuery) {
  return requestApi({
    path: `@@BASE@@${listQueryString(params)}`,
    method: "GET",
    body: null,
  }) as Promise<List@@PASCAL@@Response>;
}

export function use@@PASCAL@@List(
  query: PaginationQuery,
  options?: UseQueryOptions<List@@PASCAL@@Response, Error>,
) {
  return useQuery({
    queryKey: ["@@NAME@@List", query],
    queryFn: () => fetch@@PASCAL@@List(query),
    ...options,
  });
}

export function useCreate@@PASCAL@@(
  options?: UseMutationOptions<{}, Error, Record<string, any>>
) {
  return useMutation({
    mutationFn: (values) =>
      requestApi({
        path: "@@BASE@@",
        method: "POST",
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
  >
) {
  return useMutation({
    mutationFn: ({ id, values }) =>
      requestApi({
        path: `@@BASE@@/${id}`,
        method: "PUT",
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
  options?: UseMutationOptions<{}, Error, { ids: number[] }>
) {
  return useMutation({
    mutationFn: (input) =>
      requestApi({
        path: "@@BASE@@",
        method: "DELETE",
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

// ---- 列表页模板（index.vue，基准：language/index.vue 的 ProPage 配置） ----

/// 搜索字段：仅 string 字段进搜索（数值/布尔/枚举留给列渲染）。
fn search_fields(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        if field.kind != FieldKind::String {
            continue;
        }
        let camel = camel_of(&field.name);
        out.push_str(&render(
            r#"        {
          type: "input",
          label: $t("pages.@@NAME@@.@@CAMEL@@"),
          field: "@@CAMEL@@",
          attrs: { placeholder: $t("common.placeholder.input"), clearable: true },
        },
"#,
            &[("CAMEL", camel), ("NAME", spec.name.clone())],
        ));
    }
    out
}

/// 列定义：序号 + 业务字段全列（布尔/枚举走 slot）+ 标准尾列 + 操作列。
fn table_columns(spec: &PagesSpec) -> String {
    let mut out = String::from(
        r#"        { type: "index", label: $t("common.table.seq"), width: 60 },
"#,
    );
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let slot = matches!(field.kind, FieldKind::Bool | FieldKind::Enum(_));
        let slot_line = if slot {
            format!("        slotName: \"{camel}\",\n")
        } else {
            String::new()
        };
        out.push_str(&render(
            r#"        {
          prop: "@@CAMEL@@",
          label: $t("pages.@@NAME@@.@@CAMEL@@"),
          minWidth: 110,
@@SLOT@@        },
"#,
            &[
                ("CAMEL", camel),
                ("NAME", spec.name.clone()),
                ("SLOT", slot_line),
            ],
        ));
    }
    out.push_str(
        r#"        { prop: "sortOrder", label: $t("common.table.sortOrder"), width: 100, align: "center" },
        {
          prop: "createdAt",
          label: $t("common.table.createdAt"),
          minWidth: 160,
          cellType: "date",
          dateFormat: "YYYY-MM-DD HH:mm:ss",
        },
        {
          prop: "action",
          label: $t("common.table.action"),
          fixed: "right",
          width: 160,
          cellType: "tool",
          buttons: [
            { name: "edit", label: $t("common.button.edit"), icon: "lucide:pen-line" },
            {
              name: "delete",
              label: $t("common.button.delete"),
              icon: "lucide:trash-2",
              attrs: { type: "danger" },
            },
          ],
        },
"#,
    );
    out
}

/// 布尔/枚举列的模板 slot（ElTag 渲染）。
fn table_slots(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(_) => out.push_str(&render(
                r#"      <template #@@CAMEL@@="scope: any">
        <ElTag size="small" :type="@@CAMEL@@ToColor(scope.row.@@CAMEL@@)" effect="plain">
          {{ @@CAMEL@@ToLabel(scope.row.@@CAMEL@@) }}
        </ElTag>
      </template>
"#,
                &[("CAMEL", camel)],
            )),
            FieldKind::Bool => out.push_str(&render(
                r#"      <template #@@CAMEL@@="scope: any">
        <ElTag size="small" :type="@@CAMEL@@ToColor(scope.row.@@CAMEL@@)" effect="plain">
          {{ @@CAMEL@@ToLabel(scope.row.@@CAMEL@@) }}
        </ElTag>
      </template>
"#,
                &[("CAMEL", camel)],
            )),
            _ => {}
        }
    }
    out
}

/// index.vue 的 barrel 导入（createPagedExportAction 在 house 桶里）；
/// 布尔/枚举渲染助手在本实体的 composable 文件里，不走 barrel。
fn barrel_imports(_spec: &PagesSpec) -> String {
    "createPagedExportAction".to_owned()
}

/// 本实体 composable 文件的页面级导入（fetch/删除 + 渲染助手）。
fn composable_imports(spec: &PagesSpec) -> String {
    let mut names: Vec<String> = vec![
        format!("fetch{}List", spec.pascal),
        format!("useDelete{}", spec.pascal),
    ];
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        match &field.kind {
            FieldKind::Enum(_) => {
                names.push(format!("{camel}ToColor"));
                names.push(format!("{camel}ToLabel"));
            }
            FieldKind::Bool => {
                names.push(format!("{camel}ToColor"));
                names.push(format!("{camel}ToLabel"));
            }
            _ => {}
        }
    }
    if spec
        .fields
        .iter()
        .any(|field| matches!(field.kind, FieldKind::Bool))
    {
        names.push("boolToColor".to_owned());
        names.push("boolToLabel".to_owned());
    }
    names.sort();
    names.dedup();
    names.join(",\n  ")
}

fn index_vue(spec: &PagesSpec) -> String {
    render(
        r#"<template>
  <div class="app-container h-full flex flex-1 flex-col">
    <ProPage ref="pageRef" :config="pageConfig" @add="handleAdd" @edit="handleEdit">
@@TABLE_SLOTS@@    </ProPage>

    <!-- 新增/编辑抽屉 -->
    <@@PASCAL@@Drawer ref="drawerRef" @success="handleSuccess" />
  </div>
</template>

<script lang="ts" setup>
import { computed, ref } from "vue";
import { ElTag } from "element-plus";

import ProPage from "@/components/Pro/ProPage/index.vue";
import type { ProPageConfig } from "@/components/Pro/ProPage/types";
import @@PASCAL@@Drawer from "./@@NAME@@-drawer.vue";

import { @@BARREL_IMPORTS@@ } from "@/api/composables";
import {
@@COMPOSABLE_IMPORTS@@
} from "@/api/composables/@@NAME@@";
import { PaginationQuery } from "@/core/transport/rest";
import { $t } from "@/core/i18n";

const { mutateAsync: delete@@PASCAL@@ } = useDelete@@PASCAL@@();

const pageRef = ref();
const drawerRef = ref();

const pageConfig = computed<ProPageConfig>(() => ({
  skeleton: true,
  search: {
    grid: true,
    fields: [
@@SEARCH_FIELDS@@    ],
  },

  table: {
    listAction: async (query: any) => {
      const { page, pageSize, ...queryParams } = query;
      const result = await fetch@@PASCAL@@List(
        new PaginationQuery({
          paging: { page: page || 1, pageSize: pageSize || 10 },
          formValues: queryParams,
        })
      );
      return { items: result.items || [], total: result.total || 0 };
    },
    deleteAction: async (ids: string) => {
      await delete@@PASCAL@@({ ids: ids.split(",").map((item) => Number(item)) });
    },
    exportsAction: createPagedExportAction(fetch@@PASCAL@@List),
    toolbar: [],
    toolbarRight: ["add"],
    defaultToolbar: ["refresh", "exports", "filter"],
    tableAttrs: { border: true, stripe: false },
    columns: [
@@TABLE_COLUMNS@@    ],
  },
}));

function handleAdd() {
  drawerRef.value?.open();
}

function handleEdit(row: any) {
  drawerRef.value?.open(row);
}

function handleSuccess() {
  pageRef.value?.refresh();
}
</script>

<style lang="scss" scoped>
.app-container {
  padding: 20px;
  width: 100%;
  min-width: 0;
  flex-shrink: 0;
}
</style>
"#,
        &[
            ("PASCAL", spec.pascal.clone()),
            ("NAME", spec.name.clone()),
            ("TABLE_SLOTS", table_slots(spec)),
            ("BARREL_IMPORTS", barrel_imports(spec)),
            ("COMPOSABLE_IMPORTS", composable_imports(spec)),
            ("SEARCH_FIELDS", search_fields(spec)),
            ("TABLE_COLUMNS", table_columns(spec)),
        ],
    )
}

// ---- 抽屉模板（基准：language/language-drawer.vue 的 ProModal/ElForm） ----

fn form_data_decls(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let init = match &field.kind {
            FieldKind::String => "\"\"".to_owned(),
            FieldKind::Bool => "false".to_owned(),
            FieldKind::Enum(values) => values.default_num().to_string(),
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => "0".to_owned(),
        };
        out.push_str(&format!("  {camel}: {init},\n"));
    }
    out.push_str("  sortOrder: 1,\n");
    out
}

/// resetForm 里的逐字段赋值（statement 形态，与 open_fill 同款——
/// 不能复用对象字面量形态的 form_data_decls）。
fn form_reset_decls(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let init = match &field.kind {
            FieldKind::String => " \"\"".to_owned(),
            FieldKind::Bool => " false".to_owned(),
            FieldKind::Enum(values) => format!(" {}", values.default_num()),
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => " 0".to_owned(),
        };
        out.push_str(&format!("  formData.{camel} ={init};\n"));
    }
    out.push_str("  formData.sortOrder = 1;\n");
    out
}

fn form_rules(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let required = spec
            .code_field
            .as_deref()
            .map(|code| code == field.name)
            .unwrap_or(false);
        if !required {
            continue;
        }
        let camel = camel_of(&field.name);
        out.push_str(&render(
            r#"  @@CAMEL@@: [{ required: true, message: $t("common.validation.required"), trigger: "blur" }],
"#,
            &[("CAMEL", camel)],
        ));
    }
    out
}

fn form_items(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let label = camel.clone();
        let control = match &field.kind {
            FieldKind::Enum(_) => render(
                r#"        <ElSelect v-model="formData.@@CAMEL@@" :placeholder="$t('common.placeholder.input')" clearable>
          <ElOption
            v-for="option in @@CAMEL@@Options"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </ElSelect>
"#,
                &[("CAMEL", camel.clone())],
            ),
            FieldKind::Bool => render(
                r#"        <ElSwitch
          v-model="formData.@@CAMEL@@"
          :active-text="$t('common.status.enabled')"
          :inactive-text="$t('common.status.disabled')"
        />
"#,
                &[("CAMEL", camel.clone())],
            ),
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => render(
                r#"        <ElInputNumber v-model="formData.@@CAMEL@@" style="width: 100%" />
"#,
                &[("CAMEL", camel.clone())],
            ),
            FieldKind::String => render(
                r#"        <ElInput
          v-model="formData.@@CAMEL@@"
          :placeholder="$t('common.placeholder.input')"
          clearable
        />
"#,
                &[("CAMEL", camel.clone())],
            ),
        };
        out.push_str(&render(
            r#"      <ElFormItem :label="$t('pages.@@NAME@@.@@CAMEL@@')" prop="@@CAMEL@@">
@@CONTROL@@      </ElFormItem>

"#,
            &[
                ("CAMEL", camel),
                ("NAME", spec.name.clone()),
                ("CONTROL", control),
            ],
        ));
    }
    out.push_str(&render(
        r#"      <ElFormItem :label="$t('common.table.sortOrder')" prop="sortOrder">
        <ElInputNumber
          v-model="formData.sortOrder"
          :min="1"
          :placeholder="$t('common.placeholder.input')"
          style="width: 100%"
        />
      </ElFormItem>

"#,
        &[],
    ));
    out
}

/// 双语文案文件（element 是每实体一份，无需手术插入）：键结构与仓内
/// pages/*.json 对齐；字段标签以字段名占位，翻译在此文件上直接改。
fn locale_file(spec: &PagesSpec, zh: bool) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "moduleName".to_owned(),
        serde_json::Value::String(if zh {
            spec.name.clone()
        } else {
            spec.pascal.clone()
        }),
    );
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        map.insert(camel.clone(), serde_json::Value::String(camel));
    }
    map.insert(
        "sortOrder".to_owned(),
        serde_json::Value::String("sortOrder".to_owned()),
    );
    let (create, update) = if zh {
        (
            format!("新建{}", spec.pascal),
            format!("编辑{}", spec.pascal),
        )
    } else {
        (
            format!("Create {}", spec.pascal),
            format!("Edit {}", spec.pascal),
        )
    };
    map.insert(
        "button".to_owned(),
        serde_json::json!({ "create": create, "update": update }),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).expect("locale 合法") + "\n"
}

fn drawer_select_imports(spec: &PagesSpec) -> String {
    let mut names: Vec<String> = Vec::new();
    for field in &spec.fields {
        if let FieldKind::Enum(_) = &field.kind {
            names.push(format!("{}Options", camel_of(&field.name)));
        }
    }
    names.sort();
    if names.is_empty() {
        String::new()
    } else {
        format!(
            "import {{\n  {},\n}} from \"@/api/composables/{}\";\n",
            names.join(",\n  "),
            spec.name
        )
    }
}

fn drawer_vue(spec: &PagesSpec) -> String {
    let select_imports = drawer_select_imports(spec);
    render(
        r#"<template>
  <ProModal
    v-model:visible="visible"
    :title="title"
    :config="{ component: 'drawer', drawer: { size: DRAWER_WIDTH, closeOnClickModal: false } }"
  >
    <ElForm
      ref="formRef"
      :model="formData"
      :rules="formRules"
      label-width="120px"
      class="drawer-form"
    >
      <!-- 基本信息 -->
      <ElDivider content-position="left">{{ $t("common.section.basic") }}</ElDivider>

@@FORM_ITEMS@@    </ElForm>

    <template #footer>
      <div class="drawer-footer">
        <ElButton @click="handleClose">{{ $t("common.button.cancel") }}</ElButton>
        <ElButton type="primary" :loading="submitLoading" @click="handleSubmit">
          {{ $t("common.button.confirm") }}
        </ElButton>
      </div>
    </template>
  </ProModal>
</template>

<script lang="ts" setup>
import { computed, reactive, ref, watch } from "vue";
import { ElMessage } from "element-plus";

import {
  useCreate@@PASCAL@@,
  useUpdate@@PASCAL@@,
} from "@/api/composables/@@NAME@@";
@@SELECT_IMPORTS@@import { $t } from "@/core/i18n";
import { DRAWER_WIDTH } from "@/constants";
import ProModal from "@/components/Pro/ProModal/index.vue";

const emit = defineEmits<{
  success: [];
}>();

const { mutateAsync: create@@PASCAL@@ } = useCreate@@PASCAL@@();
const { mutateAsync: update@@PASCAL@@ } = useUpdate@@PASCAL@@();

const visible = ref(false);
const submitLoading = ref(false);
const isCreate = ref(true);
const currentId = ref<number>();
const formRef = ref();

// 表单数据
const formData = reactive({
@@FORM_DATA@@});

// 表单验证规则
const formRules = {
@@FORM_RULES@@};

// 标题
const title = computed(() =>
  isCreate.value
    ? $t("pages.@@NAME@@.button.create")
    : $t("pages.@@NAME@@.button.update")
);

// 打开抽屉
function open(row?: any) {
  visible.value = true;

  if (row) {
    // 编辑模式
    isCreate.value = false;
    currentId.value = row.id;
    // 仅回填表单声明的字段，避免把 id/createdAt 等不可变字段灌入 formData
@@OPEN_FILL@@  } else {
    // 创建模式
    isCreate.value = true;
    currentId.value = undefined;
    resetForm();
  }
}

// 关闭抽屉
function handleClose() {
  visible.value = false;
  resetForm();
}

// 重置表单
function resetForm() {
@@FORM_RESET@@
  formRef.value?.clearValidate();
}

// 提交表单
async function handleSubmit() {
  if (!formRef.value) return;

  try {
    await formRef.value.validate();
    submitLoading.value = true;

    const values = { ...formData };

    if (isCreate.value) {
      await create@@PASCAL@@(values);
      ElMessage.success($t("common.notification.createSuccess"));
    } else {
      await update@@PASCAL@@({ id: currentId.value!, values });
      ElMessage.success($t("common.notification.updateSuccess"));
    }

    emit("success");
    handleClose();
  } catch (error) {
    if (error !== false) {
      // 不是验证错误
      ElMessage.error(
        isCreate.value
          ? $t("common.notification.createFailed")
          : $t("common.notification.updateFailed")
      );
    }
  } finally {
    submitLoading.value = false;
  }
}

// ProModal 关闭时自动重置表单
watch(visible, (val) => {
  if (!val) resetForm();
});

// 暴露方法给父组件
defineExpose({
  open,
});
</script>

<style lang="scss" scoped>
.drawer-form {
  padding-right: 10px;
}

.drawer-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
"#,
        &[
            ("PASCAL", spec.pascal.clone()),
            ("NAME", spec.name.clone()),
            ("SELECT_IMPORTS", select_imports),
            ("FORM_ITEMS", form_items(spec)),
            ("FORM_DATA", form_data_decls(spec)),
            ("FORM_RULES", form_rules(spec)),
            ("FORM_RESET", form_reset_decls(spec)),
            ("OPEN_FILL", open_fill(spec)),
        ],
    )
}

/// open(row) 里的逐字段回填行。
fn open_fill(spec: &PagesSpec) -> String {
    let mut out = String::new();
    for field in &spec.fields {
        let camel = camel_of(&field.name);
        let fallback = match &field.kind {
            FieldKind::String => " ?? \"\"".to_owned(),
            FieldKind::Bool => " ?? false".to_owned(),
            FieldKind::Enum(values) => format!(" ?? {}", values.default_num()),
            FieldKind::Int32 | FieldKind::Uint32 | FieldKind::Float64 => " ?? 0".to_owned(),
        };
        out.push_str(&format!("    formData.{camel} = row.{camel}{fallback};\n"));
    }
    out.push_str("    formData.sortOrder = row.sortOrder ?? 1;\n");
    out
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
                    kind: FieldKind::parse("enum(0=OFF,1=ON@default=ON)").unwrap(),
                },
            ],
            code_field: Some("code".to_owned()),
            stack: crate::pages::PagesStack::Element,
            global: false,
            loaded_spec: None,
        }
    }

    #[test]
    fn composable_is_self_contained_element_style() {
        let file = composable_file(&spec());
        assert!(file.contains("export interface Widget {"));
        assert!(
            file.contains("} from \"@/core/transport/rest\";"),
            "element 传输导入：{file}"
        );
        assert!(file.contains("export const stateOptions = ["));
        assert!(
            file.contains("export function stateToColor(value?: number)"),
            "枚举缺省 ON（非 0 值）也生成渲染助手"
        );
        assert!(file.contains("path: `admin/v1/widgets${listQueryString(params)}`"));
    }

    #[test]
    fn index_and_drawer_carry_the_propage_marks() {
        let index = index_vue(&spec());
        assert!(index.contains("ProPage ref=\"pageRef\""));
        assert!(
            index.contains("deleteWidget({ ids: ids.split(\",\").map((item) => Number(item)) })")
        );
        assert!(
            index.contains("<template #state=\"scope: any\">"),
            "枚举列 slot：{index}"
        );
        assert!(index.contains("stateToColor(scope.row.state)"));
        assert!(index.contains("createPagedExportAction(fetchWidgetList)"));

        let drawer = drawer_vue(&spec());
        assert!(drawer.contains("ProModal"));
        assert!(drawer.contains("<ElSelect v-model=\"formData.state\""));
        assert!(drawer.contains("stateOptions"));
        assert!(drawer.contains("formData.code = row.code ?? \"\";"));
        assert!(
            drawer.contains("code: [{ required: true, message: $t(\"common.validation.required\")"),
            "code_field 必填规则：{drawer}"
        );
        // 缺省值取枚举的 default 数值（ON=1 而非 0）
        assert!(drawer.contains("state: 1,"));
        assert!(drawer.contains("formData.state = row.state ?? 1;"));
    }

    #[test]
    fn route_module_new_and_child_entry() {
        let module = route_module_new(&spec());
        assert!(module.contains("import { Layout } from \"@/layouts\";"));
        assert!(module.contains("const system: RouteRecordRaw[] = ["));
        assert!(
            module.contains("component: () => import(\"@/pages/app/system/widgets/index.vue\"),")
        );

        let child = route_child_entry(&spec());
        assert!(child.contains("name: \"WidgetManagement\","));

        let existing = "import type { RouteRecordRaw } from \"vue-router\";\nconst system: RouteRecordRaw[] = [\n  {\n    children: [\n      {\n        path: \"dict\",\n      },\n    ],\n  },\n];\n";
        let out = insert_route_child(existing, &child).unwrap();
        let dict_at = out.find("path: \"dict\",").unwrap();
        let widget_at = out.find("path: \"widgets\",").unwrap();
        let close_at = out.find("    ],").unwrap();
        assert!(dict_at < widget_at && widget_at < close_at);

        let err = insert_route_child("没有锚点", &child).unwrap_err();
        assert!(format!("{err}").contains("锚点"));
    }
}
