//! gen pages 集成测试：最小 rushwind-admin 形状的仓（React 前端根 +
//! seed.rs），验证页面组落位、菜单种子的两处手术插入、幂等与 dry-run。

use std::fs;
use std::path::Path;

use rush_gen::entity::{FieldKind, FieldSpec};
use rush_gen::pages::{self, PagesOptions};
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
