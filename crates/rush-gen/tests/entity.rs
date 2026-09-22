//! gen entity 集成测试：最小 rushwind-admin 形状的仓，验证五处外科手术
//! 编辑、新文件落位、幂等与 dry-run。

use std::fs;
use std::path::Path;

use rush_gen::entity::{self, EntityOptions, FieldKind, FieldSpec};
use tempfile::TempDir;

fn scaffold_admin() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    // proto 面：MANIFEST 在 api/ 下（manifest rebuild 由 gen 触发）
    fs::create_dir_all(root.join("backend/api/protos")).unwrap();
    fs::write(root.join("backend/api/MANIFEST.sha256"), "").unwrap();

    let src = root.join("backend/services/admin-api/src");
    fs::create_dir_all(src.join("data/repos")).unwrap();
    fs::create_dir_all(src.join("server")).unwrap();

    fs::write(
        src.join("data.rs"),
        "mod audit;\nmod misc;\nmod scope;\n\npub use audit::{\n    sys_api_audit_logs,\n};\npub use misc::{sys_configs};\npub use scope::Viewer;\n\npub mod repos;\n",
    )
    .unwrap();
    fs::write(
        src.join("migration.rs"),
        "fn migrations() {\n    EntityTables::new(\"m1\", backend)\n        .table::<data::sys_configs::Entity>()\n        .build();\n}\n",
    )
    .unwrap();
    fs::write(
        src.join("data/repos/mod.rs"),
        "macro_rules! repo_shell {\n    (tenant $name:ident, $entity:ident) => {};\n}\n\nmod config;\n\npub use config::ConfigRepo;\n",
    )
    .unwrap();
    fs::write(
        src.join("services.rs"),
        "mod config;\nmod dashboard;\n\npub use config::ConfigService;\npub use dashboard::DashboardService;\n",
    )
    .unwrap();
    fs::write(
        src.join("server/rest.rs"),
        "use crate::services::{\n    ApiService, ConfigService,\n    DashboardService,\n    FileService,\n};\n\n    mount_services!(\n        (mount_api_service, ApiService),\n        (\n            mount_dashboard_service,\n            DashboardService\n        ),\n        (mount_file_service, FileService),\n    );\n",
    )
    .unwrap();

    (dir, root)
}

fn opts(root: &Path) -> EntityOptions {
    EntityOptions {
        repo_root: root.to_path_buf(),
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
                name: "quantity".to_owned(),
                kind: FieldKind::Uint32,
            },
        ],
        dry_run: false,
        skip_manifest: true,
    }
}

#[test]
fn generates_files_and_edits_all_registration_points() {
    let (_dir, root) = scaffold_admin();
    let src = root.join("backend/services/admin-api/src");

    let report = entity::generate_entity(&opts(&root)).unwrap();
    assert_eq!(report.created.len(), 5, "{report:#?}");
    assert_eq!(report.edited.len(), 5, "{report:#?}");
    assert!(report.skipped.is_empty());

    // 新文件落位
    assert!(root
        .join("backend/api/protos/widget/service/v1/widget.proto")
        .exists());
    assert!(root
        .join("backend/api/protos/admin/service/v1/i_widget.proto")
        .exists());
    assert!(src.join("data/sys_widgets.rs").exists());
    assert!(src.join("data/repos/widget.rs").exists());
    assert!(src.join("services/widget.rs").exists());

    // data.rs：单行 pub mod 注册，按字节序落位（scope < sys_widgets < repos 尾部）
    let data_rs = fs::read_to_string(src.join("data.rs")).unwrap();
    assert!(data_rs.contains("pub mod sys_widgets;"));
    assert!(!data_rs.contains("pub use sys_widgets::sys_widgets;"));
    let mod_pos = data_rs.find("mod misc;").unwrap();
    let scope_pos = data_rs.find("mod scope;").unwrap();
    let repos_pos = data_rs.find("pub mod repos;").unwrap();
    let new_pos = data_rs.find("pub mod sys_widgets;").unwrap();
    assert!(
        mod_pos < scope_pos && scope_pos < repos_pos && repos_pos < new_pos,
        "字节序插入：{data_rs}"
    );

    // migration.rs：链尾插入
    let migration = fs::read_to_string(src.join("migration.rs")).unwrap();
    assert!(migration.contains(".table::<data::sys_widgets::Entity>()"));
    let table_pos = migration
        .find(".table::<data::sys_widgets::Entity>()")
        .unwrap();
    let build_pos = migration.find(".build();").unwrap();
    assert!(table_pos < build_pos);

    // repos/mod.rs 与 services.rs
    let repos = fs::read_to_string(src.join("data/repos/mod.rs")).unwrap();
    assert!(repos.contains("mod widget;"));
    assert!(repos.contains("pub use widget::WidgetRepo;"));
    let services = fs::read_to_string(src.join("services.rs")).unwrap();
    assert!(services.contains("mod widget;"));
    assert!(services.contains("pub use widget::WidgetService;"));
    let cfg_pos = services.find("pub use config::ConfigService;").unwrap();
    let dash_pos = services
        .find("pub use dashboard::DashboardService;")
        .unwrap();
    let widget_pos = services.find("pub use widget::WidgetService;").unwrap();
    assert!(cfg_pos < dash_pos && dash_pos < widget_pos);

    // rest.rs：导入块行内拼接 + mount 表字节序（file < widget），多行表项不被破坏
    let rest = fs::read_to_string(src.join("server/rest.rs")).unwrap();
    assert!(rest.contains("(mount_widget_service, WidgetService),"));
    let api_pos = rest.find("mount_api_service").unwrap();
    let file_pos = rest.find("mount_file_service").unwrap();
    let widget_pos = rest.find("mount_widget_service").unwrap();
    assert!(api_pos < file_pos && file_pos < widget_pos);
    assert!(rest.contains("mount_dashboard_service"), "多行表项保留");
    assert!(
        rest.contains("    WidgetService,\n};"),
        "导入块全块更小 → 关闭括号前新行：{rest}"
    );

    // 幂等：二次执行在“文件已存在”上失败（新实体名应换名重跑）
    assert!(entity::generate_entity(&opts(&root)).is_err());
}

#[test]
fn dry_run_writes_nothing() {
    let (_dir, root) = scaffold_admin();
    let before =
        fs::read_to_string(root.join("backend/services/admin-api/src/services.rs")).unwrap();

    let mut options = opts(&root);
    options.dry_run = true;
    let report = entity::generate_entity(&options).unwrap();

    assert_eq!(report.created.len(), 5);
    assert_eq!(report.edited.len(), 5);
    assert!(!root.join("backend/api/protos/widget").exists());
    assert!(!root
        .join("backend/services/admin-api/src/data/sys_widgets.rs")
        .exists());
    assert_eq!(
        fs::read_to_string(root.join("backend/services/admin-api/src/services.rs")).unwrap(),
        before
    );
}

#[test]
fn missing_anchor_files_fail_cleanly() {
    let dir = TempDir::new().unwrap();
    // 空仓：锚点文件缺失 → 明确报错，而不是写出一半
    let err = entity::generate_entity(&opts(dir.path())).unwrap_err();
    assert!(format!("{err}").contains("锚点"), "{err}");
    assert!(!dir.path().join("backend/api/protos/widget").exists());
}
