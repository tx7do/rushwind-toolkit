//! 远程配置中心导出：把服务的本地配置文件（`backend/services/<svc>/{assets,config}/*`）
//! 经 HTTP 推到 Consul / etcd / Nacos。
//!
//! 键位约定（三种中心共用同一条路径）：
//! `[env/]projectName/service/file`，Nacos 映射为 dataId=文件名、group=分组、
//! tenant=namespaceId。端点缺省协议时补 `http://`；给了 CA 证书就用它校验，
//! 否则放行自签证书（开发环境的 Consul/etcd 基本都是自签）。

use std::path::PathBuf;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteConfig {
    pub r#type: String,
    pub endpoint: String,
    pub project_name: String,
    pub group: String,
    pub env: String,
    pub namespace_id: String,
    pub username: String,
    pub password: String,
    pub ca_cert_pem: String,
    pub client_cert_pem: String,
    pub client_key_pem: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub success: bool,
    pub error: String,
    pub service: String,
    pub files_count: i64,
}

impl ExportResult {
    fn ok(service: String, files_count: i64) -> Self {
        ExportResult { success: true, error: String::new(), service, files_count }
    }
    fn fail(service: String, error: impl Into<String>) -> Self {
        ExportResult { success: false, error: error.into(), service, files_count: 0 }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigServiceInfo {
    pub name: String,
    pub config_files: Vec<String>,
    pub config_folder: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigType {
    pub label: String,
    pub value: String,
}

fn is_config_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["toml", "yaml", "yml", "json"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// 服务的配置目录：rushwind-admin 形态在 `assets/`，兼容旧式 `config/`。
pub fn config_dir_of(svc_dir: &std::path::Path) -> Option<PathBuf> {
    ["assets", "config"]
        .iter()
        .map(|folder| svc_dir.join(folder))
        .find(|folder| folder.is_dir())
}

/// 扫描项目里的可导出服务（含配置文件清单）。
pub fn scan_config_services(root: &std::path::Path) -> Vec<ConfigServiceInfo> {
    let services_dir = root.join("backend/services");
    let Ok(entries) = std::fs::read_dir(&services_dir) else {
        return Vec::new();
    };
    let mut out: Vec<ConfigServiceInfo> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let svc_dir = entry.path();
            if !svc_dir.is_dir() {
                return None;
            }
            let name = entry.file_name().into_string().ok()?;
            let folder = config_dir_of(&svc_dir)?;
            let files: Vec<String> = std::fs::read_dir(&folder)
                .map(|files| {
                    files
                        .filter_map(|file| {
                            let file = file.ok()?;
                            if !file.path().is_file() {
                                return None;
                            }
                            let name = file.file_name().into_string().ok()?;
                            is_config_file(&name).then_some(name)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if files.is_empty() {
                return None;
            }
            Some(ConfigServiceInfo {
                name,
                config_files: files,
                config_folder: folder.to_string_lossy().into_owned(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// `[env/]projectName/service/file`。
fn key_path(cfg: &RemoteConfig, service: &str, file: &str) -> String {
    [cfg.env.as_str(), cfg.project_name.as_str(), service, file]
        .into_iter()
        .map(|part| part.trim().trim_matches('/'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_endpoint(cfg: &RemoteConfig) -> Result<String, String> {
    let endpoint = cfg.endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err("请先填写配置中心地址".to_string());
    }
    Ok(if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("http://{endpoint}")
    })
}

fn build_client(cfg: &RemoteConfig) -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20));
    if !cfg.ca_cert_pem.trim().is_empty() {
        let cert = reqwest::Certificate::from_pem(cfg.ca_cert_pem.as_bytes())
            .map_err(|e| format!("CA 证书解析失败：{e}"))?;
        builder = builder.add_root_certificate(cert);
    } else {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if !cfg.client_key_pem.trim().is_empty() {
        let pem = format!("{}\n{}", cfg.client_cert_pem, cfg.client_key_pem);
        let identity = reqwest::Identity::from_pem(pem.as_bytes())
            .map_err(|e| format!("客户端证书无效：{e}"))?;
        builder = builder.identity(identity);
    }
    builder.build().map_err(|e| e.to_string())
}

/// 单个服务的全量导出。
fn export_service(
    cfg: &RemoteConfig,
    client: &reqwest::blocking::Client,
    info: &ConfigServiceInfo,
) -> Result<i64, String> {
    let folder = PathBuf::from(&info.config_folder);
    let mut count = 0i64;
    for file in &info.config_files {
        let content = std::fs::read(folder.join(file))
            .map_err(|e| format!("读取 {file} 失败：{e}"))?;
        match cfg.r#type.to_lowercase().as_str() {
            "consul" => {
                let url = format!("{}/v1/kv/{}", normalize_endpoint(cfg)?, key_path(cfg, &info.name, file));
                let response = client
                    .put(&url)
                    .body(content)
                    .send()
                    .map_err(|e| format!("Consul 请求失败：{e}"))?;
                // Consul 成功返回 "true"；其余原样回显错误。
                if !response.status().is_success() || response.text().unwrap_or_default().trim() != "true" {
                    return Err(format!("Consul 写入 {file} 失败（检查地址/token/KV 开关）"));
                }
            }
            "etcd" => {
                let key = format!("/{}", key_path(cfg, &info.name, file));
                let body = serde_json::json!({
                    "key": base64::engine::general_purpose::STANDARD.encode(&key),
                    "value": base64::engine::general_purpose::STANDARD.encode(&content),
                });
                let response = client
                    .post(format!("{}/v3/kv/put", normalize_endpoint(cfg)?))
                    .json(&body)
                    .send()
                    .map_err(|e| format!("etcd 请求失败：{e}"))?;
                if !response.status().is_success() {
                    let text = response.text().unwrap_or_default();
                    return Err(format!("etcd 写入 {file} 失败：{}", text.chars().take(200).collect::<String>()));
                }
            }
            "nacos" => {
                let endpoint = normalize_endpoint(cfg)?;
                let token = nacos_login(client, cfg, &endpoint)?;
                let group = if cfg.group.trim().is_empty() {
                    cfg.project_name.trim()
                } else {
                    cfg.group.trim()
                };
                let mut form: Vec<(String, String)> = vec![
                    ("dataId".to_string(), format!("{}/{}", info.name, file)),
                    ("group".to_string(), group.to_string()),
                    ("content".to_string(), String::from_utf8_lossy(&content).into_owned()),
                    ("type".to_string(), nacos_data_type(file)),
                ];
                if !cfg.namespace_id.trim().is_empty() {
                    form.push(("tenant".to_string(), cfg.namespace_id.trim().to_string()));
                }
                if let Some(token) = &token {
                    form.push(("accessToken".to_string(), token.clone()));
                }
                let response = client
                    .post(format!("{endpoint}/nacos/v1/cs/configs"))
                    .form(&form)
                    .send()
                    .map_err(|e| format!("Nacos 请求失败：{e}"))?;
                if !response.status().is_success() || response.text().unwrap_or_default().trim() != "true" {
                    return Err(format!("Nacos 写入 {file} 失败（检查命名空间/分组/权限）"));
                }
            }
            other => return Err(format!("暂不支持的配置中心类型「{other}」")),
        }
        count += 1;
    }
    Ok(count)
}

fn nacos_data_type(file: &str) -> String {
    if file.ends_with(".yaml") || file.ends_with(".yml") {
        "yaml".to_string()
    } else if file.ends_with(".json") {
        "json".to_string()
    } else {
        "text".to_string()
    }
}

fn nacos_login(
    client: &reqwest::blocking::Client,
    cfg: &RemoteConfig,
    endpoint: &str,
) -> Result<Option<String>, String> {
    if cfg.username.trim().is_empty() {
        return Ok(None);
    }
    let response: serde_json::Value = client
        .post(format!("{endpoint}/nacos/v1/auth/login"))
        .form(&[
            ("username", cfg.username.trim()),
            ("password", cfg.password.as_str()),
        ])
        .send()
        .map_err(|e| format!("Nacos 登录请求失败：{e}"))?
        .json()
        .map_err(|e| format!("Nacos 登录响应无效：{e}"))?;
    response
        .get("accessToken")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .map(Some)
        .ok_or_else(|| "Nacos 登录失败：响应里没有 accessToken".to_string())
}

// ==================== Tauri 命令 ====================

#[tauri::command]
pub fn get_remote_config_types() -> Vec<ConfigType> {
    ["Consul", "etcd", "Nacos"]
        .into_iter()
        .map(|label| ConfigType {
            label: label.to_string(),
            value: label.to_lowercase(),
        })
        .collect()
}

#[tauri::command]
pub fn get_config_services(state: State<AppState>) -> Vec<ConfigServiceInfo> {
    let Some(root) = crate::devops::project_root(&state) else {
        return Vec::new();
    };
    scan_config_services(&root)
}

#[tauri::command]
pub async fn export_one_service_config(
    state: State<'_, AppState>,
    cfg: RemoteConfig,
    service: String,
) -> crate::CmdResult<ExportResult> {
    let Some(root) = crate::devops::project_root(&state) else {
        return Ok(ExportResult::fail(service, "尚未打开项目"));
    };
    let Some(info) = scan_config_services(&root)
        .into_iter()
        .find(|info| info.name == service)
    else {
        return Ok(ExportResult::fail(
            service,
            "没有找到该服务的配置目录（backend/services/<svc>/assets|config）",
        ));
    };
    Ok(crate::blocking_io(move || {
        let client = match build_client(&cfg) {
            Ok(client) => client,
            Err(e) => return ExportResult::fail(info.name.clone(), e),
        };
        match export_service(&cfg, &client, &info) {
            Ok(count) => ExportResult::ok(info.name, count),
            Err(e) => ExportResult::fail(info.name, e),
        }
    })
    .await)
}

#[tauri::command]
pub async fn export_config_to_remote(
    state: State<'_, AppState>,
    cfg: RemoteConfig,
) -> crate::CmdResult<ExportResult> {
    let Some(root) = crate::devops::project_root(&state) else {
        return Ok(ExportResult::fail(String::new(), "尚未打开项目"));
    };
    let services = scan_config_services(&root);
    if services.is_empty() {
        return Ok(ExportResult::fail(
            String::new(),
            "没有找到任何带配置文件的服务",
        ));
    }
    Ok(crate::blocking_io(move || {
        let client = match build_client(&cfg) {
            Ok(client) => client,
            Err(e) => return ExportResult::fail(String::new(), e),
        };
        let mut total = 0i64;
        let mut errors: Vec<String> = Vec::new();
        for info in &services {
            match export_service(&cfg, &client, info) {
                Ok(count) => total += count,
                Err(e) => errors.push(format!("{}: {e}", info.name)),
            }
        }
        if errors.is_empty() {
            ExportResult::ok(String::new(), total)
        } else {
            ExportResult {
                success: false,
                error: errors.join("\n"),
                service: String::new(),
                files_count: total,
            }
        }
    })
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_path_skips_empty_segments() {
        let cfg = RemoteConfig {
            endpoint: "127.0.0.1:8500".into(),
            project_name: "rushwind".into(),
            ..Default::default()
        };
        assert_eq!(key_path(&cfg, "admin-api", "server.yaml"), "rushwind/admin-api/server.yaml");

        let cfg = RemoteConfig {
            endpoint: "127.0.0.1:8500".into(),
            project_name: "rushwind".into(),
            env: "prod".into(),
            ..Default::default()
        };
        assert_eq!(key_path(&cfg, "admin-api", "data.yaml"), "prod/rushwind/admin-api/data.yaml");
    }

    #[test]
    fn endpoint_defaults_to_http_and_strips_trailing_slash() {
        let cfg = RemoteConfig { endpoint: " 127.0.0.1:8500/ ".into(), ..Default::default() };
        assert_eq!(normalize_endpoint(&cfg).unwrap(), "http://127.0.0.1:8500");

        let cfg = RemoteConfig { endpoint: "https://consul.local".into(), ..Default::default() };
        assert_eq!(normalize_endpoint(&cfg).unwrap(), "https://consul.local");

        let cfg = RemoteConfig::default();
        assert!(normalize_endpoint(&cfg).is_err());
    }

    #[test]
    fn config_dir_prefers_assets_over_config() {
        let dir = std::env::temp_dir().join(format!(
            "rush-ui-cfgtest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        assert_eq!(config_dir_of(&dir).unwrap(), dir.join("assets"));
        std::fs::remove_dir_all(dir.join("assets")).unwrap();
        assert_eq!(config_dir_of(&dir).unwrap(), dir.join("config"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_finds_services_with_config_files() {
        let root = std::env::temp_dir().join(format!(
            "rush-ui-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let svc = root.join("backend/services/admin-api");
        std::fs::create_dir_all(svc.join("assets")).unwrap();
        std::fs::write(svc.join("assets/server.yaml"), "a: 1").unwrap();
        std::fs::write(svc.join("assets/notes.txt"), "ignored").unwrap();
        std::fs::create_dir_all(root.join("backend/services/empty-svc/assets")).unwrap();

        let services = scan_config_services(&root);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "admin-api");
        assert_eq!(services[0].config_files, vec!["server.yaml"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
