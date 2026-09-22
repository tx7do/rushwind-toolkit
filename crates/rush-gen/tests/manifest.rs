//! 清单算法测试：proto 面的 BOM 归一化与 sha256sum 布局，react 面的
//! 剪枝 + git 忽略语义，以及 check 的增删改识别。

use std::fs;
use std::process::Command;

use rush_gen::manifest::{self, Flavor};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.email=test@test", "-c", "user.name=test"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git 可执行");
    assert!(status.success(), "git {args:?} 失败");
}

#[test]
fn proto_manifest_strips_bom_sorts_and_uses_sha256sum_layout() {
    let tree = TempDir::new().unwrap().path().join("protos");
    fs::create_dir_all(tree.join("a/b")).unwrap();
    fs::write(tree.join("a/b/x.proto"), b"\xEF\xBB\xBFmessage X {}\n").unwrap();
    fs::write(tree.join("a/y.proto"), b"message Y {}\n").unwrap();
    fs::write(tree.join("a/note.txt"), b"non-proto is skipped").unwrap();

    let text = manifest::build_manifest(&tree, Flavor::Proto).unwrap();
    let entries = manifest::parse_manifest(&text).unwrap();

    assert_eq!(entries.len(), 2, "只收 *.proto：{text}");
    assert_eq!(entries[0].path, "a/b/x.proto", "按路径字节序排序");
    assert_eq!(
        entries[0].hash,
        sha256_hex(b"message X {}\n"),
        "BOM 剥除后哈希"
    );
    assert_eq!(entries[1].hash, sha256_hex(b"message Y {}\n"));
    assert!(text.ends_with('\n'));
    assert_eq!(entries[0].hash.len(), 64);
}

#[test]
fn proto_check_detects_add_modify_remove_and_rebuild_fixes() {
    let dir = TempDir::new().unwrap();
    let tree = dir.path().join("protos");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("one.proto"), b"message One {}\n").unwrap();

    let path = dir.path().join("MANIFEST.sha256");
    assert_eq!(manifest::rebuild(&tree, &path, Flavor::Proto).unwrap(), 1);
    assert!(manifest::check(&tree, &path, Flavor::Proto)
        .unwrap()
        .is_ok());

    fs::write(tree.join("two.proto"), b"message Two {}\n").unwrap();
    let report = manifest::check(&tree, &path, Flavor::Proto).unwrap();
    assert_eq!(report.added, vec!["two.proto".to_owned()]);
    manifest::rebuild(&tree, &path, Flavor::Proto).unwrap();
    assert!(manifest::check(&tree, &path, Flavor::Proto)
        .unwrap()
        .is_ok());

    fs::write(tree.join("one.proto"), b"message One { int32 v = 1; }\n").unwrap();
    let report = manifest::check(&tree, &path, Flavor::Proto).unwrap();
    assert_eq!(report.modified, vec!["one.proto".to_owned()]);

    fs::remove_file(tree.join("two.proto")).unwrap();
    let report = manifest::check(&tree, &path, Flavor::Proto).unwrap();
    assert_eq!(report.removed, vec!["two.proto".to_owned()]);

    manifest::rebuild(&tree, &path, Flavor::Proto).unwrap();
    assert!(manifest::check(&tree, &path, Flavor::Proto)
        .unwrap()
        .is_ok());
}

#[test]
fn react_manifest_prunes_dirs_and_respects_gitignore() {
    let dir = TempDir::new().unwrap();
    let tree = dir.path().join("react");
    for sub in [
        "node_modules/pkg",
        "dist",
        ".vite/deps",
        "ignored-dir",
        "src",
    ] {
        fs::create_dir_all(tree.join(sub)).unwrap();
    }
    fs::write(tree.join(".gitignore"), b"*.log\nignored-dir/\n.vite/\n").unwrap();
    fs::write(tree.join("keep.txt"), b"keep\n").unwrap();
    fs::write(tree.join("src/a.ts"), b"export {};\n").unwrap();
    fs::write(tree.join("skip.log"), b"ignored by rule").unwrap();
    fs::write(tree.join("ignored-dir/x"), b"ignored by rule").unwrap();
    fs::write(tree.join(".vite/deps/v.js"), b"ignored by rule").unwrap();
    fs::write(tree.join("node_modules/m.js"), b"pruned dir").unwrap();
    fs::write(tree.join("dist/b.js"), b"pruned dir").unwrap();
    git(&tree, &["init", "-q"]);
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "init"]);

    let text = manifest::build_manifest(&tree, Flavor::React).unwrap();
    let paths: Vec<&str> = text
        .lines()
        .map(|l| l.split("  ").nth(1).unwrap())
        .collect();

    assert!(paths.contains(&".gitignore"));
    assert!(paths.contains(&"keep.txt"));
    assert!(paths.contains(&"src/a.ts"));
    assert!(!paths.contains(&"node_modules/m.js"), "node_modules 被剪枝");
    assert!(!paths.contains(&"dist/b.js"), "dist 被剪枝");
    assert!(!paths.contains(&"skip.log"), "gitignore 规则命中");
    assert!(!paths.contains(&"ignored-dir/x"), "gitignore 规则命中");
    assert!(!paths.contains(&".vite/deps/v.js"), "gitignore 规则命中");

    // 与脚本同款校验循环：rebuild → check 一致
    let path = dir.path().join("react.MANIFEST.sha256");
    manifest::rebuild(&tree, &path, Flavor::React).unwrap();
    assert!(manifest::check(&tree, &path, Flavor::React)
        .unwrap()
        .is_ok());
}

#[test]
fn react_manifest_requires_a_git_work_tree() {
    let dir = TempDir::new().unwrap();
    let tree = dir.path().join("react");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("a.ts"), b"export {};\n").unwrap();

    let err = manifest::build_manifest(&tree, Flavor::React).unwrap_err();
    assert!(format!("{err}").contains("git 工作树"), "{err}");
}

#[test]
fn missing_trees_and_manifests_error_cleanly() {
    let dir = TempDir::new().unwrap();
    let tree = dir.path().join("protos");

    let err = manifest::build_manifest(&tree, Flavor::Proto).unwrap_err();
    assert!(format!("{err}").contains("不存在"), "{err}");

    let err = manifest::check(&tree, &dir.path().join("absent.sha256"), Flavor::Proto).unwrap_err();
    assert!(format!("{err}").contains("缺失"), "{err}");
}
