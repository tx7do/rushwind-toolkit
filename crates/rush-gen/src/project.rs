//! 项目脚手架（`rush new`）：从内嵌模板或外部模板目录生成一个可编译、
//! 可直接 `cargo run` 的 RushWind 服务项目。
//!
//! 内嵌模板自包含（rushwind git 依赖钉在与 rushwind-admin 一致的 rev），
//! 内存存储开箱即跑：一个 YAML 文档组装存储引擎、CRUD 边和 HTTP 服务器。
//! 外部模板（如 rushwind 仓的 examples/*）按目录整树拷贝——跳过 `.git`/
//! `target`——并按其 Cargo.toml 的包名做 token 重命名（含下划线变体），
//! 非 UTF-8 文件字节级原样拷贝。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::{Error, Result};

/// 模板钉住的 rushwind 框架 rev（与 rushwind-admin 的 workspace 依赖一致）。
pub const PINNED_RUSHWIND_REV: &str = "2bc3a96c742539a195b608b995be1c3bcc1545a3";

/// `rush new` 选项。
#[derive(Debug, Clone)]
pub struct NewOptions {
    /// 项目名（= crate 名 + 目录名）。
    pub name: String,
    /// 目标父目录，项目创建在 `<dest>/<name>`。
    pub dest: PathBuf,
    /// 外部模板目录；缺省用内嵌模板。
    pub template: Option<PathBuf>,
    /// 在项目目录里 `git init`。
    pub git: bool,
    /// 只报告不落盘。
    pub dry_run: bool,
}

/// 生成结果报告。
#[derive(Debug, Default)]
pub struct NewReport {
    pub project_dir: PathBuf,
    pub files: Vec<PathBuf>,
    /// 模板来源描述（"embedded" 或模板目录路径）。
    pub template_source: String,
    /// 外部模板被重命名的原包名。
    pub renamed_from: Option<String>,
    pub notes: Vec<String>,
}

/// 内嵌模板文件：(模板内相对路径, 目标相对路径, 内容)。
const EMBEDDED: [(&str, &str, &str); 5] = [
    (
        "Cargo.toml",
        "Cargo.toml",
        include_str!("../templates/new/Cargo.toml"),
    ),
    (
        "src_main.rs",
        "src/main.rs",
        include_str!("../templates/new/src_main.rs"),
    ),
    (
        "README.md",
        "README.md",
        include_str!("../templates/new/README.md"),
    ),
    (
        "gitignore",
        ".gitignore",
        include_str!("../templates/new/gitignore"),
    ),
    (
        "rustfmt.toml",
        "rustfmt.toml",
        include_str!("../templates/new/rustfmt.toml"),
    ),
];

/// 外部模板里做 token 替换的文本扩展名；其余文件字节级原样拷贝。
const TEXT_EXTENSIONS: [&str; 8] = ["toml", "rs", "md", "yaml", "yml", "json", "txt", "sh"];

fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let rest_ok = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !first_ok || !rest_ok {
        return Err(Error::InvalidInput(format!(
            "项目名须为小写字母开头的 [a-z0-9_-]：{name}"
        )));
    }
    Ok(())
}

/// 从模板 Cargo.toml 的 [package] 段提取包名。
fn template_package_name(template_dir: &Path) -> Result<String> {
    let text = fs::read_to_string(template_dir.join("Cargo.toml"))?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        return Ok(value.to_owned());
                    }
                }
            }
        }
    }
    Err(Error::InvalidInput(format!(
        "模板 {} 的 [package] name 缺失，无法推断重命名目标",
        template_dir.display()
    )))
}

fn render_embedded(name: &str) -> Vec<(String, Vec<u8>)> {
    EMBEDDED
        .iter()
        .map(|(_src, dst, content)| {
            let rendered = content
                .replace("@@NAME@@", name)
                .replace("@@REV@@", PINNED_RUSHWIND_REV);
            ((*dst).to_owned(), rendered.into_bytes())
        })
        .collect()
}

/// 外部模板拷贝时剪枝的目录（版本库与构建产物）。
const SKIPPED_TEMPLATE_DIRS: [&str; 2] = [".git", "target"];

fn is_skipped_dir(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir()
        && SKIPPED_TEMPLATE_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
}

/// 外部模板整树 → (相对路径, 内容)。跳过 .git 与 target；文本文件做
/// 包名（含下划线变体）token 替换，其余字节级原样。
fn render_external(
    template_dir: &Path,
    name: &str,
    old_name: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let old_underscore = old_name.replace('-', "_");
    let mut files = Vec::new();
    for entry in WalkDir::new(template_dir)
        .into_iter()
        .filter_entry(|e| !is_skipped_dir(e))
    {
        let entry = entry.map_err(|err| Error::Io(std::io::Error::other(err)))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(template_dir)
            .expect("walkdir 前缀必在")
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(entry.path())?;
        let extension = entry.path().extension().and_then(|e| e.to_str());
        let content = match extension {
            Some(ext) if TEXT_EXTENSIONS.contains(&ext) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                text.replace(old_name, name)
                    .replace(&old_underscore, &name.replace('-', "_"))
                    .into_bytes()
            }
            _ => bytes,
        };
        files.push((rel, content));
    }
    Ok(files)
}

/// 生成新项目。
pub fn new_project(opts: &NewOptions) -> Result<NewReport> {
    validate_name(&opts.name)?;
    let project_dir = opts.dest.join(&opts.name);
    if project_dir.exists() {
        return Err(Error::InvalidInput(format!(
            "目标目录已存在：{}",
            project_dir.display()
        )));
    }

    let (files, template_source, renamed_from) = match &opts.template {
        None => (render_embedded(&opts.name), "embedded".to_owned(), None),
        Some(dir) => {
            if !dir.join("Cargo.toml").is_file() {
                return Err(Error::InvalidInput(format!(
                    "模板目录缺少 Cargo.toml：{}",
                    dir.display()
                )));
            }
            let old_name = template_package_name(dir)?;
            let files = render_external(dir, &opts.name, &old_name)?;
            (files, dir.display().to_string(), Some(old_name))
        }
    };
    if files.is_empty() {
        return Err(Error::InvalidInput("模板不含任何文件".to_owned()));
    }

    let mut report = NewReport {
        project_dir: project_dir.clone(),
        template_source,
        renamed_from,
        ..Default::default()
    };

    if opts.dry_run {
        report.files = files.iter().map(|(rel, _)| project_dir.join(rel)).collect();
        return Ok(report);
    }

    for (rel, content) in &files {
        let path = project_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        report.files.push(path);
    }

    if opts.git {
        match Command::new("git")
            .args(["init", "-q"])
            .current_dir(&project_dir)
            .status()
        {
            Ok(status) if status.success() => {}
            _ => report
                .notes
                .push("git init 失败（git 不可用？）——可稍后手动执行".to_owned()),
        }
    }

    report
        .notes
        .push("开箱即跑：cargo run 后按提示 curl /health、/wired 与 /items".to_owned());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_template_renders_with_name_and_rev() {
        let files = render_embedded("myapp");
        assert_eq!(files.len(), 5);
        let cargo = files
            .iter()
            .find(|(rel, _)| rel == "Cargo.toml")
            .expect("Cargo.toml 在模板里");
        let text = String::from_utf8_lossy(&cargo.1);
        assert!(text.contains("name = \"myapp\""));
        assert!(text.contains(PINNED_RUSHWIND_REV));
        assert!(!text.contains("@@NAME@@"));
        let main = files.iter().find(|(rel, _)| rel == "src/main.rs").unwrap();
        let text = String::from_utf8_lossy(&main.1);
        assert!(text.contains("[myapp]"));
        assert!(!text.contains("@@"));
    }

    #[test]
    fn rejects_bad_names_and_existing_dirs() {
        assert!(validate_name("MyApp").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("1app").is_err());
        assert!(validate_name("my_app-2").is_ok());
    }

    #[test]
    fn external_template_renames_and_passes_binary_through() {
        let dir = tempfile::TempDir::new().unwrap();
        let template = dir.path().join("tpl");
        fs::create_dir_all(template.join("src")).unwrap();
        fs::create_dir_all(template.join(".git")).unwrap();
        fs::write(
            template.join("Cargo.toml"),
            "[package]\nname = \"demo-app\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            template.join("src/main.rs"),
            "fn main() { println!(\"demo-app/demo_app\"); }\n",
        )
        .unwrap();
        fs::write(template.join(".git/config"), "[core]\n").unwrap();
        fs::write(template.join("logo.bin"), [0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let files = render_external(&template, "my-app", "demo-app").unwrap();
        assert_eq!(files.len(), 3, ".git 被跳过");
        let main = files.iter().find(|(rel, _)| rel == "src/main.rs").unwrap();
        let text = String::from_utf8_lossy(&main.1);
        assert!(
            text.contains("my-app/my_app"),
            "连字符名与下划线变体都替换：{text}"
        );
        assert!(!text.contains("demo"));
        let bin = files.iter().find(|(rel, _)| rel == "logo.bin").unwrap();
        assert_eq!(bin.1, vec![0xFF, 0xFE, 0x00, 0x01], "非文本文件字节级原样");
    }

    #[test]
    fn new_project_writes_tree_and_inits_git() {
        let dir = tempfile::TempDir::new().unwrap();
        let opts = NewOptions {
            name: "myapp".to_owned(),
            dest: dir.path().to_path_buf(),
            template: None,
            git: true,
            dry_run: false,
        };
        let report = new_project(&opts).unwrap();
        assert_eq!(report.files.len(), 5);
        assert!(dir.path().join("myapp/Cargo.toml").exists());
        assert!(dir.path().join("myapp/src/main.rs").exists());
        assert!(dir.path().join("myapp/.gitignore").exists());
        assert!(dir.path().join("myapp/.git").is_dir(), "git init 已执行");

        // 目录已存在 → 明确报错
        assert!(new_project(&opts).is_err());
    }

    #[test]
    fn external_template_infers_package_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let template = dir.path().join("tpl");
        fs::create_dir_all(&template).unwrap();
        fs::write(
            template.join("Cargo.toml"),
            "[package]\nname = \"demo-app\"\n\n[dependencies]\nx = \"1\"\n",
        )
        .unwrap();
        assert_eq!(template_package_name(&template).unwrap(), "demo-app");

        let no_name = tempfile::TempDir::new().unwrap();
        fs::write(
            no_name.path().join("Cargo.toml"),
            "[package]\nedition = \"2021\"\n",
        )
        .unwrap();
        assert!(template_package_name(no_name.path()).is_err());
    }
}
