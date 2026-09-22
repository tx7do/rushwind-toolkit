//! adopt 集成测试：清单基线重建 + CI 门禁剥离，覆盖 dry-run、幂等与
//! keep-gates 三种模式。

use std::fs;
use std::path::Path;
use std::process::Command;

use rush_gen::adopt::{self, AdoptOptions, UpstreamBaseline};
use tempfile::TempDir;

const CI_FIXTURE: &str = include_str!("fixtures/ci.yml");

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.email=test@test", "-c", "user.name=test"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git 可执行");
    assert!(status.success(), "git {args:?} 失败");
}

/// 搭一个最小 rushwind-admin 形状的仓库：proto 面 + react 面（git 工作树）
/// + 真实形状的 CI。
fn scaffold_repo() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();

    let protos = root.join("backend/api/protos/admin/v1");
    fs::create_dir_all(&protos).unwrap();
    fs::write(protos.join("user.proto"), b"message User {}\n").unwrap();

    let react = root.join("frontend/admin/react");
    fs::create_dir_all(&react).unwrap();
    fs::write(react.join("main.tsx"), b"export {};\n").unwrap();
    // git 仓库建在仓库根（与真实布局一致：react/ 只是子目录，.git 不在树内）
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);

    let workflows = root.join(".github/workflows");
    fs::create_dir_all(&workflows).unwrap();
    fs::write(workflows.join("ci.yml"), CI_FIXTURE).unwrap();

    // sync 脚本（退役对象）
    fs::write(
        root.join("backend/api/sync-protos.sh"),
        b"#!/usr/bin/env bash\n# original proto sync\n",
    )
    .unwrap();
    fs::write(
        root.join("frontend/admin/sync-react.sh"),
        b"#!/usr/bin/env bash\n# original react sync\n",
    )
    .unwrap();

    (dir, root)
}

fn opts(root: &Path) -> AdoptOptions {
    AdoptOptions {
        repo_root: root.to_path_buf(),
        dry_run: false,
        keep_gates: false,
        skip_proto: false,
        skip_react: false,
        prune_upstream_baseline: false,
        keep_sync_scripts: false,
    }
}

#[test]
fn adopt_rebuilds_manifests_and_strips_ci_gates() {
    let (_dir, root) = scaffold_repo();

    let report = adopt::adopt(&opts(&root)).unwrap();
    assert_eq!(report.proto_entries, Some(1));
    assert_eq!(report.react_entries, Some(1));
    assert_eq!(report.upstream_baseline, UpstreamBaseline::Absent);
    assert_eq!(report.removed_ci_steps.len(), 2);

    let manifest = fs::read_to_string(root.join("backend/api/MANIFEST.sha256")).unwrap();
    assert_eq!(manifest.lines().count(), 1);
    assert!(manifest.ends_with('\n'));

    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
    assert!(!ci.contains("sync-protos"));
    assert!(!ci.contains("sync-react"));
    assert!(ci.contains("- name: Format"));

    // sync 脚本机制性退役：改写为拒跑 stub
    assert_eq!(report.retired_scripts.len(), 2);
    for script in ["backend/api/sync-protos.sh", "frontend/admin/sync-react.sh"] {
        let text = fs::read_to_string(root.join(script)).unwrap();
        assert!(text.contains("refusing"), "{script} 应为拒跑 stub");
        assert!(text.contains("rush manifest"));
    }

    // 幂等：二次执行不再有门禁可剥，清单重建结果一致，脚本已是 stub 不再改写
    let report2 = adopt::adopt(&opts(&root)).unwrap();
    assert!(report2.removed_ci_steps.is_empty());
    assert!(report2.ci_gates_already_absent);
    assert!(report2.retired_scripts.is_empty());
}

#[test]
fn adopt_dry_run_writes_nothing() {
    let (_dir, root) = scaffold_repo();
    let ci_before = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();

    let mut options = opts(&root);
    options.dry_run = true;
    let report = adopt::adopt(&options).unwrap();

    assert_eq!(report.proto_entries, Some(1));
    assert_eq!(report.removed_ci_steps.len(), 2, "报告里仍列出将要剥离的步");
    assert_eq!(
        report.retired_scripts.len(),
        2,
        "报告里仍列出将要退役的脚本"
    );
    assert!(!root.join("backend/api/MANIFEST.sha256").exists());
    assert!(!root.join("frontend/admin/react.MANIFEST.sha256").exists());
    assert_eq!(
        fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap(),
        ci_before
    );
    assert!(
        fs::read_to_string(root.join("backend/api/sync-protos.sh"))
            .unwrap()
            .contains("original proto sync"),
        "dry-run 不改写脚本"
    );
}

#[test]
fn adopt_keep_gates_only_rebuilds_manifests() {
    let (_dir, root) = scaffold_repo();
    let ci_before = fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();

    let mut options = opts(&root);
    options.keep_gates = true;
    let report = adopt::adopt(&options).unwrap();

    assert_eq!(report.proto_entries, Some(1));
    assert!(root.join("backend/api/MANIFEST.sha256").exists());
    assert_eq!(
        fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap(),
        ci_before
    );
}

#[test]
fn adopt_keep_sync_scripts_leaves_them_alone() {
    let (_dir, root) = scaffold_repo();

    let mut options = opts(&root);
    options.keep_sync_scripts = true;
    let report = adopt::adopt(&options).unwrap();

    assert!(report.retired_scripts.is_empty());
    assert!(
        fs::read_to_string(root.join("backend/api/sync-protos.sh"))
            .unwrap()
            .contains("original proto sync"),
        "保留模式下脚本原样"
    );
}

#[test]
fn adopt_prunes_upstream_baseline_on_request() {
    let (_dir, root) = scaffold_repo();
    fs::write(
        root.join("frontend/admin/react.UPSTREAM.sha256"),
        b"baseline\n",
    )
    .unwrap();

    let mut options = opts(&root);
    options.prune_upstream_baseline = true;
    let report = adopt::adopt(&options).unwrap();

    assert_eq!(report.upstream_baseline, UpstreamBaseline::Pruned);
    assert!(!root.join("frontend/admin/react.UPSTREAM.sha256").exists());
}
