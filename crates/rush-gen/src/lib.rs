//! rush-gen — RushWind 生态工具箱核心库。
//!
//! * [`manifest`]：rushwind-admin 两个同步面（proto 契约 / react 前端
//!   快照）的 sha256 清单构建、解析、校验与重建，算法逐字对齐被替换的
//!   shell 脚本；
//! * [`adopt`]：下游接管——以当前树为基线重建清单，并从 CI 剥离上游
//!   镜像门禁步；
//! * [`spec`]：实体规格文件（`.rush/<name>.json`）——字段清单的唯一
//!   真相，gen entity 落盘、gen pages 读取，UI 表单回填的数据源。

pub mod adopt;
pub mod entity;
pub mod manifest;
pub mod pages;
pub mod project;
pub mod spec;
pub mod testbed;

use std::io;
use std::path::PathBuf;

/// 工具箱核心库错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("输入非法: {0}")]
    InvalidInput(String),
    #[error("清单树不存在: {0}")]
    TreeMissing(PathBuf),
    #[error("清单文件缺失: {0}（先执行 rebuild）")]
    ManifestMissing(PathBuf),
    #[error("{0} 不在 git 工作树内（react 清单要求 git 忽略语义，与 sync-react.sh 一致）")]
    NotGitWorkTree(PathBuf),
    #[error("git check-ignore 执行失败: {0}")]
    GitCheckIgnore(String),
    #[error("清单行格式非法: {0}")]
    BadManifestLine(String),
}

/// 库层 Result 别名。
pub type Result<T> = std::result::Result<T, Error>;
