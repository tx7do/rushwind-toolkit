//! gen pages 集成测试：最小 rushwind-admin 形状的仓（React 前端根 +
//! seed.rs），验证页面组落位、菜单种子的两处手术插入、幂等与 dry-run。

use std::fs;
use std::path::Path;

use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use rush_gen::pages::{self, PagesOptions};
use rush_gen::spec;
use tempfile::TempDir;

const SEED_RS: &str = r#"//! Boot seeds.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, Set};

use crate::state::AppState;

macro_rules! insert_seed {
    ($db:expr, $row:expr) => {
        $row.insert($db).await.map_err(|e| e.to_string())?;
    };
}

pub async fn run(state: &Arc<AppState>) -> Result<(), String> {
    seed_languages(state).await?;
    seed_menus(state).await?;
    seed_admin_user(state).await?;
    Ok(())
}

async fn seed_languages(state: &Arc<AppState>) -> Result<(), String> {
    Ok(())
}

async fn seed_menus(state: &Arc<AppState>) -> Result<(), String> {
    Ok(())
}

async fn seed_admin_user(state: &Arc<AppState>) -> Result<(), String> {
    Ok(())
}
"#;

fn scaffold_admin() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    let react = root.join("frontend/admin/react");
    fs::create_dir_all(react.join("src/pages/app")).unwrap();
    fs::write(react.join("package.json"), "{}").unwrap();

    let src = root.join("backend/services/admin-api/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("seed.rs"), SEED_RS).unwrap();

    (dir, root)
}

fn opts(root: &Path) -> PagesOptions {
    PagesOptions {
        repo_root: root.to_path_buf(),
        name: "widget".to_owned(),
        group: Some("system".to_owned()),
        route_prefix: Some("/admin/v1/widgets".to_owned()),
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
        stack: rush_gen::pages::PagesStack::React,
        global: None,
        overwrite: false,
        dry_run: false,
    }
}

#[test]
fn generates_pages_and_inserts_both_seed_halves() {
    let (_dir, root) = scaffold_admin();
    let seed_rs = root.join("backend/services/admin-api/src/seed.rs");

    let report = pages::generate_pages(&opts(&root)).unwrap();
    assert_eq!(report.created.len(), 7, "{report:#?}");
    assert_eq!(report.edited, vec![seed_rs.clone()], "{report:#?}");

    // 页面组落位
    assert!(root
        .join("frontend/admin/react/src/api/hooks/widget.ts")
        .exists());
    assert!(root
        .join("frontend/admin/react/src/pages/app/system/widgets/WidgetList.tsx")
        .exists());
    assert!(root
        .join("frontend/admin/react/src/locales/zh-CN/_modules/widget.json")
        .exists());

    // seed.rs：调用行紧跟 seed_menus（admin_user 之前），函数体在 seed_languages 前
    let seed = fs::read_to_string(&seed_rs).unwrap();
    let lines: Vec<&str> = seed.lines().collect();
    let menus_at = lines
        .iter()
        .position(|l| l.contains("seed_menus(state)"))
        .unwrap();
    let call_at = lines
        .iter()
        .position(|l| l.contains("seed_gen_menu_widget(state)"))
        .unwrap();
    let admin_at = lines
        .iter()
        .position(|l| l.contains("seed_admin_user(state)"))
        .unwrap();
    assert!(menus_at < call_at && call_at < admin_at, "{seed}");
    assert!(seed.contains("async fn seed_gen_menu_widget("));
    assert!(seed.contains("let menu_path = \"/system/widgets\";"));
    assert!(seed.contains("component: Set(Some(\"system/widgets/index\".into())"));
    let fn_pos = seed.find("async fn seed_gen_menu_widget").unwrap();
    let langs_pos = seed.find("async fn seed_languages").unwrap();
    assert!(fn_pos < langs_pos);

    // 第二个实体：调用行聚在既有 gen 调用之后，函数体继续前置堆叠
    let mut second = opts(&root);
    second.name = "gadget".to_owned();
    second.fields = vec![FieldSpec {
        name: "label".to_owned(),
        kind: FieldKind::String,
    }];
    second.code_field = None;
    let report = pages::generate_pages(&second).unwrap();
    assert_eq!(report.edited, vec![seed_rs.clone()]);
    let seed = fs::read_to_string(&seed_rs).unwrap();
    let lines: Vec<&str> = seed.lines().collect();
    let widget_at = lines
        .iter()
        .position(|l| l.contains("seed_gen_menu_widget(state)"))
        .unwrap();
    let gadget_at = lines
        .iter()
        .position(|l| l.contains("seed_gen_menu_gadget(state)"))
        .unwrap();
    assert!(widget_at < gadget_at, "{seed}");
}

#[test]
fn seed_already_present_is_reported_skipped() {
    let (_dir, root) = scaffold_admin();
    let seed_rs = root.join("backend/services/admin-api/src/seed.rs");

    // 预置两半标记：模拟既已生成过（部分形态也各自幂等）
    let seeded = fs::read_to_string(&seed_rs)
        .unwrap()
        .replacen(
            "    seed_admin_user(state).await?;",
            "    seed_gen_menu_widget(state).await?;\n    seed_admin_user(state).await?;",
            1,
        )
        .replacen(
            "async fn seed_languages",
            "async fn seed_gen_menu_widget(state: &Arc<AppState>) -> Result<(), String> {\n    Ok(())\n}\n\nasync fn seed_languages",
            1,
        );
    fs::write(&seed_rs, &seeded).unwrap();

    let report = pages::generate_pages(&opts(&root)).unwrap();
    assert!(report.edited.is_empty(), "{report:#?}");
    assert!(
        report.skipped.iter().any(|s| s.contains("菜单种子已在位")),
        "{report:#?}"
    );
    // seed.rs 保持不动
    assert_eq!(fs::read_to_string(&seed_rs).unwrap(), seeded);
}

#[test]
fn partial_seed_state_fills_only_the_missing_half() {
    let (_dir, root) = scaffold_admin();
    let seed_rs = root.join("backend/services/admin-api/src/seed.rs");
    // 只有函数体、没有调用行 → 只补调用行
    let partial = fs::read_to_string(&seed_rs)
        .unwrap()
        .replacen(
            "async fn seed_languages",
            "async fn seed_gen_menu_widget(state: &Arc<AppState>) -> Result<(), String> {\n    Ok(())\n}\n\nasync fn seed_languages",
            1,
        );
    fs::write(&seed_rs, &partial).unwrap();

    let report = pages::generate_pages(&opts(&root)).unwrap();
    assert_eq!(report.edited, vec![seed_rs.clone()]);
    let seed = fs::read_to_string(&seed_rs).unwrap();
    assert!(seed.contains("seed_gen_menu_widget(state).await?;"));
    assert_eq!(seed.matches("async fn seed_gen_menu_widget").count(), 1);
}

#[test]
fn dry_run_reports_seed_edit_without_writing() {
    let (_dir, root) = scaffold_admin();
    let seed_rs = root.join("backend/services/admin-api/src/seed.rs");
    let before = fs::read_to_string(&seed_rs).unwrap();

    let mut options = opts(&root);
    options.dry_run = true;
    let report = pages::generate_pages(&options).unwrap();

    assert_eq!(report.created.len(), 7);
    assert_eq!(report.edited, vec![seed_rs.clone()]);
    assert!(!root
        .join("frontend/admin/react/src/api/hooks/widget.ts")
        .exists());
    assert_eq!(fs::read_to_string(&seed_rs).unwrap(), before);
}

#[test]
fn missing_seed_anchor_fails_cleanly() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let react = root.join("frontend/admin/react");
    fs::create_dir_all(&react).unwrap();
    fs::write(react.join("package.json"), "{}").unwrap();
    let src = root.join("backend/services/admin-api/src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("seed.rs"), "完全不同的形状\n").unwrap();

    let err = pages::generate_pages(&opts(&root)).unwrap_err();
    assert!(format!("{err}").contains("锚点"), "{err}");
    assert!(!root
        .join("frontend/admin/react/src/api/hooks/widget.ts")
        .exists());
}

/// gen entity 的最小前置仓（proto 面 + 注册锚点），让规格文件真实落盘。
fn scaffold_for_entity(root: &Path) {
    fs::create_dir_all(root.join("backend/api/protos")).unwrap();
    fs::write(root.join("backend/api/MANIFEST.sha256"), "").unwrap();
    let src = root.join("backend/services/admin-api/src");
    fs::create_dir_all(src.join("data/repos")).unwrap();
    fs::create_dir_all(src.join("server")).unwrap();
    fs::write(src.join("data.rs"), "mod misc;\n").unwrap();
    fs::write(
        src.join("migration.rs"),
        "fn migrations() {\n    EntityTables::new(\"m1\", backend)\n        .table::<data::sys_configs::Entity>()\n        .build();\n}\n",
    )
    .unwrap();
    fs::write(
        src.join("data/repos/mod.rs"),
        "mod config;\n\npub use config::ConfigRepo;\n",
    )
    .unwrap();
    fs::write(
        src.join("services.rs"),
        "mod config;\n\npub use config::ConfigService;\n",
    )
    .unwrap();
    fs::write(
        src.join("server/rest.rs"),
        "use crate::services::{\n    ConfigService,\n};\n\n    mount_services!(\n        (mount_config_service, ConfigService),\n    );\n",
    )
    .unwrap();
}

#[test]
fn pages_without_fields_read_the_entity_spec() {
    let (_dir, root) = scaffold_admin();
    scaffold_for_entity(&root);

    // gen entity 落规格（显式字段）
    let entity_opts = EntityOptions {
        repo_root: root.clone(),
        name: "widget".to_owned(),
        table: None,
        package: None,
        route_prefix: None,
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
        global: false,
        check: false,
        dry_run: false,
        skip_manifest: true,
        overwrite: false,
        auth_free: false,
    };
    entity::generate_entity(&entity_opts).unwrap();
    assert!(root.join(".rush/widget.json").exists());

    // gen pages 不传 --field：字段/code_field/路由前缀全部继承规格
    let mut page_opts = opts(&root);
    page_opts.fields.clear();
    page_opts.code_field = None;
    page_opts.route_prefix = None;
    let report = pages::generate_pages(&page_opts).unwrap();

    let hooks =
        fs::read_to_string(root.join("frontend/admin/react/src/api/hooks/widget.ts")).unwrap();
    assert!(hooks.contains("export interface Widget {"), "{hooks}");
    assert!(hooks.contains("state?: number;"), "枚举字段继承自规格");
    assert!(
        hooks.contains("path: `admin/v1/widgets"),
        "路由前缀继承自规格"
    );
    let drawer = fs::read_to_string(
        root.join("frontend/admin/react/src/pages/app/system/widgets/WidgetDrawer.tsx"),
    )
    .unwrap();
    assert!(
        drawer.contains("requiredCode") && drawer.contains("name=\"code\""),
        "code_field 继承自规格：{drawer}"
    );
    assert!(
        report.notes.iter().any(|n| n.contains("规格文件")),
        "{report:#?}"
    );
    // group 写回规格
    let file = spec::load(&root, "widget").unwrap();
    assert_eq!(file.group.as_deref(), Some("system"));
    assert!(report.edited.contains(&spec::spec_path(&root, "widget")));

    // 字段与规格重复显式给出（字段重复在两源合并后仍是非法）
    let mut dup = opts(&root);
    dup.fields.push(FieldSpec {
        name: "code".to_owned(),
        kind: FieldKind::String,
    });
    let err = pages::generate_pages(&dup).unwrap_err();
    assert!(format!("{err}").contains("字段重复"), "{err}");
}

#[test]
fn pages_without_fields_and_without_spec_fail_cleanly() {
    let (_dir, root) = scaffold_admin();
    let mut page_opts = opts(&root);
    page_opts.fields.clear();
    let err = pages::generate_pages(&page_opts).unwrap_err();
    assert!(format!("{err}").contains("规格文件缺失"), "{err}");
    assert!(!root
        .join("frontend/admin/react/src/api/hooks/widget.ts")
        .exists());
}

// ---- vben / element 栈的集成测试 ----

fn vben_opts(root: &Path) -> PagesOptions {
    let mut opts = opts(root);
    opts.stack = rush_gen::pages::PagesStack::Vben;
    opts
}

fn element_opts(root: &Path) -> PagesOptions {
    let mut opts = opts(root);
    opts.stack = rush_gen::pages::PagesStack::Element;
    opts
}

fn scaffold_vben(root: &Path) {
    let app = root.join("frontend/admin/vue-vben/apps/admin");
    fs::create_dir_all(app.join("src/views/app")).unwrap();
    fs::write(app.join("package.json"), "{}").unwrap();
}

fn scaffold_element(root: &Path) {
    let app = root.join("frontend/admin/vue-element");
    fs::create_dir_all(app.join("src/pages/app")).unwrap();
    fs::write(app.join("package.json"), "{}").unwrap();
}

#[test]
fn vben_pages_create_route_module_and_files() {
    let (_dir, root) = scaffold_admin();
    scaffold_vben(&root);
    let module =
        root.join("frontend/admin/vue-vben/apps/admin/src/router/routes/modules/app/system.ts");

    let report = pages::generate_pages(&vben_opts(&root)).unwrap();
    let app = root.join("frontend/admin/vue-vben/apps/admin");
    assert_eq!(report.created.len(), 4, "{report:#?}");
    assert!(app.join("src/api/composables/widget.ts").exists());
    assert!(app.join("src/views/app/system/widgets/index.vue").exists());
    assert!(app
        .join("src/views/app/system/widgets/widget-drawer.vue")
        .exists());
    assert!(module.exists(), "新分组整文件创建");

    // 路由模块内容与 BOM
    let module_text = fs::read_to_string(&module).unwrap();
    assert!(module_text.starts_with('\u{FEFF}'), "vben 模块带 BOM");
    assert!(module_text.contains("#/views/app/system/widgets/index.vue"));
    assert!(module_text.contains("name: 'WidgetManagement',"));

    // 幂等：路由条目已在位；但页面文件已存在 → 拒绝重跑（新实体换名）
    assert!(pages::generate_pages(&vben_opts(&root)).is_err());
}

#[test]
fn vben_pages_insert_child_into_existing_module() {
    let (_dir, root) = scaffold_admin();
    scaffold_vben(&root);
    let module =
        root.join("frontend/admin/vue-vben/apps/admin/src/router/routes/modules/app/system.ts");
    fs::create_dir_all(module.parent().unwrap()).unwrap();
    fs::write(
        &module,
        "import type { RouteRecordRaw } from 'vue-router';\n\nimport { BasicLayout } from '#/layouts';\n\nconst system: RouteRecordRaw[] = [\n  {\n    path: '/system',\n    children: [\n      {\n        path: 'user',\n        component: () => import('#/views/app/system/user/index.vue'),\n      },\n    ],\n  },\n];\n",
    )
    .unwrap();

    let report = pages::generate_pages(&vben_opts(&root)).unwrap();
    assert_eq!(report.edited, vec![module.clone()], "{report:#?}");
    let text = fs::read_to_string(&module).unwrap();
    let user_at = text.find("path: 'user',").unwrap();
    let widget_at = text.find("path: 'widgets',").unwrap();
    assert!(user_at < widget_at, "插到既有 children 尾部：{text}");
    // BOM 不被破坏（原文无 BOM 则不添加）
    assert!(text.starts_with("import type"));
}

#[test]
fn element_pages_create_route_module_and_files() {
    let (_dir, root) = scaffold_admin();
    scaffold_element(&root);
    let module = root.join("frontend/admin/vue-element/src/router/routes/modules/app/system.ts");

    let report = pages::generate_pages(&element_opts(&root)).unwrap();
    let app = root.join("frontend/admin/vue-element");
    assert_eq!(report.created.len(), 4, "{report:#?}");
    assert!(app.join("src/api/composables/widget.ts").exists());
    let index = fs::read_to_string(app.join("src/pages/app/system/widgets/index.vue")).unwrap();
    assert!(index.contains("ProPage ref=\"pageRef\""));
    assert!(module.exists());

    let module_text = fs::read_to_string(&module).unwrap();
    assert!(!module_text.starts_with('\u{FEFF}'), "element 模块不带 BOM");
    assert!(module_text.contains("@/pages/app/system/widgets/index.vue"));

    let drawer =
        fs::read_to_string(app.join("src/pages/app/system/widgets/widget-drawer.vue")).unwrap();
    assert!(drawer.contains("formData.code = row.code ?? \"\";"));
    assert!(pages::generate_pages(&element_opts(&root)).is_err());
}

#[test]
fn vben_and_element_require_their_frontend_roots() {
    let (_dir, root) = scaffold_admin();
    let err = pages::generate_pages(&vben_opts(&root)).unwrap_err();
    assert!(format!("{err}").contains("vue-vben"), "{err}");

    let err = pages::generate_pages(&element_opts(&root)).unwrap_err();
    assert!(format!("{err}").contains("vue-element"), "{err}");
}
