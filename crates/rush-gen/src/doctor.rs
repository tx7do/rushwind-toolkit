//! 环境体检（`rush doctor`）：工具箱全链路的外部依赖一次探明——每项给
//! ✓/⚠/✗ 与修复提示。起因是实弹中反复被环境咬：spawn 环境缺 cargo、
//! protoc→buf 切换、Node 18 跑不动 ESLint 10、Docker VM 内存不足。
//! 只读探测，不改任何东西。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    fn push(
        &mut self,
        name: &str,
        status: Status,
        detail: impl Into<String>,
        hint: Option<String>,
    ) {
        self.checks.push(Check {
            name: name.to_owned(),
            status,
            detail: detail.into(),
            hint,
        });
    }

    /// 有任何 Fail 才算失败（Warn 放行）——供 CLI 决定退出码。
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|check| check.status == Status::Fail)
    }
}

fn command_version(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .env("PATH", crate::entity::augmented_path())
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!("退出码 {:?}", output.status.code()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or("").trim().to_owned();
    Ok(if first.is_empty() {
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned()
    } else {
        first
    })
}

fn major_version(version_line: &str) -> Option<u64> {
    // 从 "v18.20.8" / "go1.27.1" / "1.89.0" 里取首个数字串的主版本
    let digits: String = version_line
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    digits.split('.').next()?.parse().ok()
}

/// 工具箱全链的环境体检。`repo` 给出时追加仓形状检查（锚点/清单/规格）。
pub fn run_doctor(repo: Option<&Path>) -> DoctorReport {
    let mut report = DoctorReport::default();

    // ---- Rust 工具链 ----
    match command_version("cargo", &["--version"]) {
        Ok(version) => report.push("cargo", Status::Ok, version, None),
        Err(err) => report.push(
            "cargo",
            Status::Fail,
            format!("不可用（{err}）"),
            Some(
                "安装 rustup（https://rustup.rs）；GUI spawn 场景确认 ~/.cargo/bin 在 PATH"
                    .to_owned(),
            ),
        ),
    }
    match command_version("rustfmt", &["--version"]) {
        Ok(version) => report.push("rustfmt", Status::Ok, version, None),
        Err(err) => report.push(
            "rustfmt",
            Status::Warn,
            format!("不可用（{err}）"),
            Some("rustup component add rustfmt——生成物的就地格式化依赖它".to_owned()),
        ),
    }

    // ---- proto 契约面工具（buf 为现行 prereq，protoc 为旧链兼容） ----
    match command_version("buf", &["--version"]) {
        Ok(version) => report.push("buf", Status::Ok, version, None),
        Err(err) => report.push(
            "buf",
            Status::Warn,
            format!("不可用（{err}）"),
            Some(
                "rushwind-admin 后端已切 buf 契约面：参照其 README 安装（cargo/binary 均可）"
                    .to_owned(),
            ),
        ),
    }
    match command_version("protoc", &["--version"]) {
        Ok(version) => report.push("protoc", Status::Ok, format!("{version}（旧链兼容）"), None),
        Err(_) => report.push(
            "protoc",
            Status::Warn,
            "不可用（若目标仓仍是 protoc 链则为必需）",
            Some("apt install protobuf-compiler 或参照目标仓 README".to_owned()),
        ),
    }

    // ---- 前端工具 ----
    match command_version("node", &["--version"]) {
        Ok(version) => {
            let major = major_version(&version).unwrap_or(0);
            if major >= 20 {
                report.push("node", Status::Ok, version, None);
            } else {
                report.push(
                    "node",
                    Status::Warn,
                    version,
                    Some("Node 20+ 建议：ESLint 10 等前端工具链要求（Node 18 下 vue-tsc 可用但 eslint 不可用）".to_owned()),
                );
            }
        }
        Err(err) => report.push(
            "node",
            Status::Fail,
            format!("不可用（{err}）"),
            Some("安装 Node 20+（nvm 推荐）".to_owned()),
        ),
    }
    match command_version("pnpm", &["--version"]) {
        Ok(version) => report.push("pnpm", Status::Ok, version, None),
        Err(_) => report.push(
            "pnpm",
            Status::Warn,
            "不可用（npm 亦可，脚本按目标仓 package.json 为准）",
            Some("npm i -g pnpm 或 corepack enable".to_owned()),
        ),
    }

    // ---- 版本控制 / 容器 ----
    match command_version("git", &["--version"]) {
        Ok(version) => report.push("git", Status::Ok, version, None),
        Err(err) => report.push("git", Status::Fail, format!("不可用（{err}）"), None),
    }
    match command_version("docker", &["--version"]) {
        Ok(version) => {
            report.push("docker", Status::Ok, version, None);
            match command_version("docker", &["compose", "version", "--short"]) {
                Ok(v2) => report.push("docker compose", Status::Ok, v2, None),
                Err(err) => report.push(
                    "docker compose",
                    Status::Warn,
                    format!("不可用（{err}）"),
                    Some("testbed 差分栈依赖 compose v2".to_owned()),
                ),
            }
            // daemon 可达性（连不上 = 只有 CLI 没有引擎）
            let daemon = Command::new("docker")
                .args(["info", "--format", "{{.ServerVersion}}"])
                .env("PATH", crate::entity::augmented_path())
                .output();
            match daemon {
                Ok(out) if out.status.success() => {
                    let version = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                    report.push("docker daemon", Status::Ok, version, None);
                }
                _ => report.push(
                    "docker daemon",
                    Status::Fail,
                    "不可达",
                    Some("Docker Desktop 未运行时：systemctl --user start docker-desktop；原生 dockerd 则 sudo systemctl start docker".to_owned()),
                ),
            }
        }
        Err(err) => report.push(
            "docker",
            Status::Warn,
            format!("不可用（{err}）"),
            Some("仅 testbed 差分需要；其余功能不受影响".to_owned()),
        ),
    }

    // ---- spawn PATH 增补位存在性 ----
    let home = std::env::var("HOME").unwrap_or_default();
    let path_env = std::env::var("PATH").unwrap_or_default();
    let cargo_bin = PathBuf::from(format!("{home}/.cargo/bin"));
    if cargo_bin.is_dir() && !path_env.split(':').any(|p| p == cargo_bin.as_path()) {
        report.push(
            "PATH 增补位",
            Status::Warn,
            format!(
                "{} 不在 PATH（本工具 spawn 子进程时会自动增补）",
                cargo_bin.display()
            ),
            None,
        );
    } else {
        report.push(
            "PATH 增补位",
            Status::Ok,
            "~/.cargo/bin、~/.local/bin 可用",
            None,
        );
    }

    // ---- 仓形状（可选） ----
    if let Some(repo) = repo {
        if !repo.is_dir() {
            report.push(
                "目标仓",
                Status::Fail,
                format!("{} 不存在", repo.display()),
                None,
            );
            return report;
        }
        let has = |rel: &str| repo.join(rel).is_file();
        report.push(
            "仓·后端锚点",
            if has("backend/services/admin-api/src/data.rs")
                && has("backend/services/admin-api/src/server/rest.rs")
                && has("backend/services/admin-api/src/seed.rs")
            {
                Status::Ok
            } else {
                Status::Fail
            },
            "data.rs / rest.rs / seed.rs".to_owned(),
            None,
        );
        for (name, rel) in [
            ("仓·react", "frontend/admin/react/package.json"),
            ("仓·vben", "frontend/admin/vue-vben/apps/admin/package.json"),
            ("仓·element", "frontend/admin/vue-element/package.json"),
        ] {
            report.push(
                name,
                if has(rel) { Status::Ok } else { Status::Warn },
                rel.to_owned(),
                None,
            );
        }
        let spec_dir = repo.join(".rush");
        let specs = std::fs::read_dir(&spec_dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
                    .count()
            })
            .unwrap_or(0);
        report.push(
            "仓·实体规格",
            Status::Ok,
            format!("{specs} 个（.rush/）"),
            None,
        );
        let manifest = repo.join("backend/api/MANIFEST.sha256");
        if manifest.is_file() {
            match crate::manifest::check(
                &crate::manifest::Flavor::Proto.tree_path(repo),
                &crate::manifest::Flavor::Proto.manifest_path(repo),
                crate::manifest::Flavor::Proto,
            ) {
                Ok(check) if check.is_ok() => {
                    report.push("仓·proto 清单", Status::Ok, "与树一致".to_owned(), None)
                }
                Ok(check) => report.push(
                    "仓·proto 清单",
                    Status::Warn,
                    format!(
                        "不一致（新增 {} / 删除 {} / 改动 {}）",
                        check.added.len(),
                        check.removed.len(),
                        check.modified.len()
                    ),
                    Some("rush manifest proto --repo <路径> --rebuild".to_owned()),
                ),
                Err(err) => report.push(
                    "仓·proto 清单",
                    Status::Warn,
                    format!("校验失败（{err:#}）"),
                    None,
                ),
            }
        }
    }

    report
}
