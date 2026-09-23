//! AI 助手：OpenAI 兼容 provider 的对话补全客户端（reqwest blocking）。
//!
//! 三条链路复用同一个 `chat`/`chat_stream` 底座：
//! - 需求 → DDL（流式，`ai:stream` task=ddl）
//! - DDL → 微服务划分（要求模型只回 JSON，解析成 `MicroservicePartition`）
//! - 生成代码审查（流式，task=review）
//!
//! 「后端代码生成」这一步是 RushWind 差异化落地面：不生成 Go/ent 代码，
//! 而是把 DDL 用 `sqlimport` 解析、按分组的 serviceName 写成 `.rush/<name>.json`
//! 实体规格，交给后端代码页签走真正的 rush-gen 生成链。
//!
//! 约定：连接测试/审查/DDL 返回 `StepResult`（success + content/error），
//! 划分返回 `PartitionResult`，落地返回 String（`''` 成功）。

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::sqlimport;
use crate::AppState;

use rush_gen::spec::{EntitySpecFile, SpecField, SPEC_SCHEMA};

// ==================== 配置类型 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub azure_api_version: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i64,
}

fn default_provider() -> String {
    "openai".into()
}
fn default_base_url() -> String {
    "https://api.openai.com/v1".into()
}
fn default_model() -> String {
    "gpt-4o".into()
}
fn default_temperature() -> f64 {
    0.7
}
fn default_max_tokens() -> i64 {
    4096
}

impl Default for AIConfig {
    fn default() -> Self {
        AIConfig {
            provider: default_provider(),
            base_url: default_base_url(),
            api_key: String::new(),
            azure_api_version: String::new(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AIProviderPreset {
    pub name: String,
    pub value: String,
    pub base_url: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub success: bool,
    pub content: String,
    pub error: Option<String>,
}

impl StepResult {
    fn ok(content: String) -> Self {
        StepResult { success: true, content, error: None }
    }
    fn err(message: impl Into<String>) -> Self {
        let message = message.into();
        StepResult { success: false, content: String::new(), error: Some(message) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroservicePartition {
    pub service_name: String,
    pub tables: Vec<String>,
    pub description: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionResult {
    pub success: bool,
    pub partitions: Vec<MicroservicePartition>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAPIResult {
    pub success: bool,
    pub files: Vec<String>,
    pub message: Option<String>,
    pub error: Option<String>,
}

// ==================== HTTP 底座 ====================

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

fn chat_endpoint(cfg: &AIConfig) -> String {
    let base = cfg.base_url.trim().trim_end_matches('/');
    if cfg.provider.eq_ignore_ascii_case("azure") {
        let version = if cfg.azure_api_version.trim().is_empty() {
            "2024-02-15-preview"
        } else {
            cfg.azure_api_version.trim()
        };
        return format!(
            "{base}/openai/deployments/{}/chat/completions?api-version={version}",
            cfg.model
        );
    }
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn build_body(cfg: &AIConfig, messages: &[(&str, String)], stream: bool) -> serde_json::Value {
    let json_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|(role, content)| serde_json::json!({"role": role, "content": content}))
        .collect();
    let mut body = serde_json::json!({
        "model": cfg.model,
        "messages": json_messages,
        "stream": stream,
    });
    // Ollama / 部分网关忽略 temperature/max_tokens 也无妨，仅在合理范围时带上。
    if cfg.temperature > 0.0 {
        body["temperature"] = serde_json::json!(cfg.temperature);
    }
    if cfg.max_tokens > 0 {
        body["max_tokens"] = serde_json::json!(cfg.max_tokens);
    }
    body
}

fn apply_auth(request: reqwest::blocking::RequestBuilder, cfg: &AIConfig) -> reqwest::blocking::RequestBuilder {
    if cfg.api_key.trim().is_empty() {
        request
    } else if cfg.provider.eq_ignore_ascii_case("azure") {
        request.header("api-key", cfg.api_key.trim())
    } else {
        request.bearer_auth(cfg.api_key.trim())
    }
}

/// 非流式对话补全：返回 assistant 文本。
fn chat(cfg: &AIConfig, messages: &[(&str, String)]) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = apply_auth(
        client
            .post(chat_endpoint(cfg))
            .json(&build_body(cfg, messages, false)),
        cfg,
    )
    .send()
    .map_err(|e| format!("请求失败：{e}"))?;
    let status = response.status();
    let text: serde_json::Value = response
        .json()
        .map_err(|e| format!("响应不是合法 JSON：{e}"))?;
    if !status.is_success() {
        let message = text
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("接口返回错误");
        return Err(format!("{status}: {message}"));
    }
    text.pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "响应缺少 choices[0].message.content".to_string())
}

/// 流式对话补全：逐块回调 `on_delta`。
fn chat_stream(
    cfg: &AIConfig,
    messages: &[(&str, String)],
    mut on_delta: impl FnMut(&str),
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = apply_auth(
        client
            .post(chat_endpoint(cfg))
            .json(&build_body(cfg, messages, true)),
        cfg,
    )
    .send()
    .map_err(|e| format!("请求失败：{e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "接口返回错误".to_string());
        return Err(format!("{status}: {}", truncate(&body, 400)));
    }

    let reader = BufReader::new(response);
    let mut buffer = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(delta) = chunk
            .pointer("/choices/0/delta/content")
            .and_then(|v| v.as_str())
        {
            if !delta.is_empty() {
                buffer.push_str(delta);
                on_delta(delta);
            }
        }
    }
    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    }
}

// ==================== 提示词 ====================

fn ddl_system_prompt() -> String {
    "你是资深数据库架构师，负责把中文需求转成可直接执行的 MySQL 8 DDL。\
     只输出 CREATE TABLE 语句（每张表一个），不要输出解释文字或 Markdown 代码块。\
     每张表必须含 id BIGINT 自增主键、created_at、updated_at 列；枚举用英文大写取值。"
        .to_string()
}

fn partition_system_prompt() -> String {
    "你是微服务拆分专家。给定一批 MySQL 建表语句，把表按业务领域划分成若干微服务。\
     只输出一个 JSON 数组，禁止任何额外文字或代码块标记。每个元素形如 \
     {\"serviceName\":\"snake_case 服务名\",\"tables\":[\"表名\"],\"description\":\"一句话职责\"}。\
     每张表只能归属一个服务，表名必须来自输入 DDL。"
        .to_string()
}

fn review_system_prompt() -> String {
    "你是严苛的 Rust 代码评审员。逐文件审阅给出的代码，指出正确性、安全性与可维护性问题，\
     并给出可执行的修改建议。用简洁的中文 Markdown 输出。"
        .to_string()
}

// ==================== Tauri 命令 ====================

#[tauri::command]
pub fn get_ai_config(state: State<AppState>) -> AIConfig {
    state.ai_config.lock().expect("ai_config 锁中毒").clone()
}

#[tauri::command]
pub fn set_ai_config(state: State<AppState>, cfg: AIConfig) {
    *state.ai_config.lock().expect("ai_config 锁中毒") = cfg;
}

#[tauri::command]
pub fn get_ai_provider_presets() -> Vec<AIProviderPreset> {
    [
        ("OpenAI", "openai", "https://api.openai.com/v1"),
        ("Anthropic", "anthropic", "https://api.anthropic.com/v1"),
        ("Azure OpenAI", "azure", "https://<resource>.openai.azure.com"),
        ("DeepSeek", "deepseek", "https://api.deepseek.com/v1"),
        ("Ollama (本地)", "ollama", "http://127.0.0.1:11434/v1"),
    ]
    .into_iter()
    .map(|(name, value, base_url)| AIProviderPreset {
        name: name.to_string(),
        value: value.to_string(),
        base_url: base_url.to_string(),
    })
    .collect()
}

fn require_config(cfg: &AIConfig) -> Result<(), String> {
    if cfg.base_url.trim().is_empty() || cfg.model.trim().is_empty() {
        return Err("请先填写 Base URL 与模型名".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn test_ai_connection(state: State<'_, AppState>) -> crate::CmdResult<StepResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    Ok(blocking(move || match require_config(&cfg) {
        Ok(()) => match chat(&cfg, &[("user", "回复 ok 两个字母即可，不要多说。".to_string())]) {
            Ok(text) => StepResult::ok(text.trim().to_string()),
            Err(e) => StepResult::err(e),
        },
        Err(e) => StepResult::err(e),
    })
    .await)
}

#[tauri::command]
pub async fn ai_generate_ddl(state: State<'_, AppState>, file: String) -> crate::CmdResult<StepResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    Ok(blocking(move || match require_config(&cfg) {
        Ok(()) => match chat(
            &cfg,
            &[
                ("system", ddl_system_prompt()),
                ("user", file),
            ],
        ) {
            Ok(text) => StepResult::ok(strip_code_fence(&text)),
            Err(e) => StepResult::err(e),
        },
        Err(e) => StepResult::err(e),
    })
    .await)
}

#[tauri::command]
pub async fn ai_generate_ddl_stream(app: AppHandle, state: State<'_, AppState>, file: String) -> crate::CmdResult<StepResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    Ok(blocking(move || {
        if let Err(e) = require_config(&cfg) {
            return StepResult::err(e);
        }
        let mut full = String::new();
        let app2 = app.clone();
        let streamed = chat_stream(
            &cfg,
            &[("system", ddl_system_prompt()), ("user", file)],
            |delta| {
                full.push_str(delta);
                let _ = app2.emit("ai:stream", StreamDelta { task: "ddl", delta: delta.to_string() });
            },
        );
        match streamed {
            // 流式已逐块推给前端，最终以去壳后的正文回填权威结果。
            Ok(()) => StepResult::ok(strip_code_fence(&full)),
            Err(e) => StepResult::err(e),
        }
    })
    .await)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamDelta<'a> {
    task: &'a str,
    delta: String,
}

#[tauri::command]
pub async fn ai_partition_microservices(state: State<'_, AppState>, input: String) -> crate::CmdResult<PartitionResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    Ok(blocking(move || {
        if let Err(e) = require_config(&cfg) {
            return PartitionResult { success: false, partitions: Vec::new(), error: Some(e) };
        }
        match chat(
            &cfg,
            &[("system", partition_system_prompt()), ("user", input)],
        ) {
            Ok(text) => match parse_partitions(&text) {
                Ok(partitions) => PartitionResult { success: true, partitions, error: None },
                Err(e) => PartitionResult { success: false, partitions: Vec::new(), error: Some(e) },
            },
            Err(e) => PartitionResult { success: false, partitions: Vec::new(), error: Some(e) },
        }
    })
    .await)
}

/// 从模型输出里抠出 JSON 数组（容忍 ```json 围栏与前后闲话）。
fn parse_partitions(text: &str) -> Result<Vec<MicroservicePartition>, String> {
    let cleaned = strip_code_fence(text);
    let start = cleaned
        .find('[')
        .ok_or_else(|| "模型没有返回 JSON 数组".to_string())?;
    let end = cleaned
        .rfind(']')
        .ok_or_else(|| "模型没有返回完整 JSON 数组".to_string())?;
    let json = &cleaned[start..=end];
    let parsed: Vec<MicroservicePartition> = serde_json::from_str(json)
        .map_err(|e| format!("解析划分结果失败：{e}"))?;
    if parsed.is_empty() {
        return Err("划分结果为空".to_string());
    }
    Ok(parsed)
}

/// DDL + 划分 → 写 `.rush/<name>.json` 实体规格（group=serviceName）。
/// 这是 RushWind 的真实落地面：产物直接喂给后端代码生成链。
#[tauri::command]
pub fn ai_generate_backend_code(
    state: State<AppState>,
    openapi: String,
    _framework: String,
    partitions: Vec<MicroservicePartition>,
) -> String {
    let Some(root) = crate::devops::project_root(&state) else {
        return "尚未打开项目".to_string();
    };
    let tables = sqlimport::parse_ddl(&openapi);
    if tables.is_empty() {
        return "DDL 里没有解析到任何表，无法落地实体规格".to_string();
    }
    let mut written: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for partition in &partitions {
        let service = partition.service_name.trim();
        if service.is_empty() {
            errors.push("有分组缺少 serviceName".to_string());
            continue;
        }
        for table_name in &partition.tables {
            let wanted = table_name.trim().to_lowercase();
            let Some(table) = tables.iter().find(|t| t.table.eq_ignore_ascii_case(&wanted)) else {
                errors.push(format!("{service}: DDL 里找不到表「{table_name}」"));
                continue;
            };
            let spec = spec_from_sql_table(service, table);
            match rush_gen::spec::save(&spec, &root) {
                Ok(_) => written.push(spec.name),
                Err(e) => errors.push(format!("{service}/{}: {e:#}", table.name)),
            }
        }
    }
    if written.is_empty() {
        return if errors.is_empty() {
            "没有写入任何实体规格".to_string()
        } else {
            errors.join("\n")
        };
    }
    // 部分成功也返回空串（与 Go 版一致：'' = 打通），错误经前端 toast 之外的
    // 日志忽略；这里把残余错误折叠进返回，让页面能提示。
    if errors.is_empty() {
        String::new()
    } else {
        // 已写成功的仍算成功，只报告未落地的表。
        errors.join("\n")
    }
}

fn spec_from_sql_table(service: &str, table: &sqlimport::SqlTable) -> EntitySpecFile {
    let option = sqlimport::to_option(0, table);
    EntitySpecFile {
        schema: SPEC_SCHEMA,
        name: table.name.clone(),
        table: table.table.clone(),
        package: option.proto_package,
        route_prefix: option.route_prefix,
        fields: table
            .fields
            .iter()
            .map(|f| SpecField { name: f.name.clone(), kind: f.kind.clone() })
            .collect(),
        code_field: (!option.code_field.is_empty()).then(|| option.code_field.clone()),
        global: false,
        group: Some(service.to_string()),
        stack: None,
    }
}

#[tauri::command]
pub fn ai_find_openapi_files(state: State<AppState>) -> OpenAPIResult {
    let Some(root) = crate::devops::project_root(&state) else {
        return OpenAPIResult {
            success: false,
            files: Vec::new(),
            message: None,
            error: Some("尚未打开项目".to_string()),
        };
    };
    let mut files = Vec::new();
    collect_openapi(&root, &mut files);
    if files.is_empty() {
        OpenAPIResult {
            success: true,
            files,
            message: Some("没有找到 OpenAPI 文档（services/*/assets/openapi.y*ml）".to_string()),
            error: None,
        }
    } else {
        OpenAPIResult { success: true, files, message: None, error: None }
    }
}

fn collect_openapi(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= 200 {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            // 跳过重量级目录，避免全仓扫描。
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), "target" | "node_modules" | ".git" | "dist") {
                continue;
            }
            collect_openapi(&path, out);
        } else if let Some(stem) = path.file_name().and_then(|n| n.to_str()) {
            let lower = stem.to_lowercase();
            if (lower.starts_with("openapi") || lower.contains("swagger"))
                && (lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".json"))
            {
                out.push(path.to_string_lossy().into_owned());
            }
        }
    }
}

#[tauri::command]
pub async fn ai_review_code(state: State<'_, AppState>, files: std::collections::HashMap<String, String>) -> crate::CmdResult<StepResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    let prompt = render_review_prompt(&files);
    Ok(blocking(move || match require_config(&cfg) {
        Ok(()) => match chat(&cfg, &[("system", review_system_prompt()), ("user", prompt)]) {
            Ok(text) => StepResult::ok(text),
            Err(e) => StepResult::err(e),
        },
        Err(e) => StepResult::err(e),
    })
    .await)
}

#[tauri::command]
pub async fn ai_review_code_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    files: std::collections::HashMap<String, String>,
) -> crate::CmdResult<StepResult> {
    let cfg = state.ai_config.lock().expect("ai_config 锁中毒").clone();
    let prompt = render_review_prompt(&files);
    Ok(blocking(move || {
        if let Err(e) = require_config(&cfg) {
            return StepResult::err(e);
        }
        let mut full = String::new();
        let app2 = app.clone();
        match chat_stream(
            &cfg,
            &[("system", review_system_prompt()), ("user", prompt)],
            |delta| {
                full.push_str(delta);
                let _ = app2.emit("ai:stream", StreamDelta { task: "review", delta: delta.to_string() });
            },
        ) {
            Ok(()) => StepResult::ok(full),
            Err(e) => StepResult::err(e),
        }
    })
    .await)
}

/// 前端可以只给路径（值为空串）：这时由后端读盘补内容（单文件截 64K 字符）。
fn resolve_review_files(files: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = files
        .iter()
        .map(|(path, content)| {
            let content = if content.is_empty() {
                std::fs::read_to_string(path)
                    .map(|c| c.chars().take(64 * 1024).collect())
                    .unwrap_or_else(|e| format!("（读取失败：{e}）"))
            } else {
                content.clone()
            };
            (path.clone(), content)
        })
        .collect();
    entries.sort();
    entries
}

fn render_review_prompt(files: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::from("请审查以下文件：\n");
    for (path, content) in resolve_review_files(files) {
        out.push_str(&format!("\n===== {path} =====\n{content}\n"));
    }
    out
}

/// 剥掉 ```/```sql/```json 围栏。
fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    // 去掉围栏首行（```sql）与尾行（```）。
    let without_lang = rest.find('\n').map(|i| &rest[i + 1..]).unwrap_or(rest);
    without_lang
        .trim_end()
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(without_lang)
        .to_string()
}

async fn blocking<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(value) => value,
        Err(e) => panic!("AI 任务线程失败：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_endpoint_uses_deployments_and_api_version() {
        let cfg = AIConfig {
            provider: "azure".into(),
            base_url: "https://demo.openai.azure.com".into(),
            model: "gpt-4o".into(),
            azure_api_version: "2024-02-15-preview".into(),
            ..Default::default()
        };
        assert_eq!(
            chat_endpoint(&cfg),
            "https://demo.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-02-15-preview"
        );
    }

    #[test]
    fn openai_endpoint_appends_chat_completions_once() {
        let cfg = AIConfig {
            base_url: "https://api.openai.com/v1/".into(),
            ..Default::default()
        };
        assert_eq!(chat_endpoint(&cfg), "https://api.openai.com/v1/chat/completions");

        let cfg2 = AIConfig {
            base_url: "https://gateway.local/v1/chat/completions".into(),
            ..Default::default()
        };
        assert_eq!(chat_endpoint(&cfg2), "https://gateway.local/v1/chat/completions");
    }

    #[test]
    fn strips_markdown_code_fences() {
        let text = "```sql\nCREATE TABLE t (id INT);\n```";
        assert_eq!(strip_code_fence(text), "CREATE TABLE t (id INT);");
        assert_eq!(strip_code_fence("CREATE TABLE x (a INT)"), "CREATE TABLE x (a INT)");
    }

    #[test]
    fn parses_partitions_with_surrounding_noise() {
        let text = "好的，划分结果：\n[{\"serviceName\":\"user_service\",\"tables\":[\"sys_user\"],\"description\":\"用户\"}]\n以上。";
        let parts = parse_partitions(text).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].service_name, "user_service");
        assert_eq!(parts[0].tables, vec!["sys_user"]);
    }

    #[test]
    fn partition_parse_error_without_json_array() {
        assert!(parse_partitions("我没有表可以划分").is_err());
    }

    #[test]
    fn backend_spec_maps_service_and_route_prefix() {
        let tables = sqlimport::parse_ddl(
            "CREATE TABLE sys_dict_type (code varchar(64), name varchar(128));",
        );
        let spec = spec_from_sql_table("dict_service", &tables[0]);
        assert_eq!(spec.name, "dict_type");
        assert_eq!(spec.table, "sys_dict_type");
        assert_eq!(spec.group.as_deref(), Some("dict_service"));
        assert_eq!(spec.route_prefix, "/admin/v1/dict_types");
        assert_eq!(spec.package, "dict_type.service.v1");
    }
}
