//! HTTP API：OpenAI/Anthropic 兼容端点 + TokenHarbor 上游桥接 + 凭证管理 + 面板

use crate::config::Config;
use crate::errors::ApiError;
use crate::models::{self, ModelRegistry};
use crate::protocol::anthropic_sse::AnthropicSseResponse;
use crate::protocol::openai_sse::SseResponse;
use crate::session::SessionMap;
use crate::upstream::{StreamRequest, UpstreamClient};
use crate::web_pool::WebCookiePool;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub clients: Arc<Vec<UpstreamClient>>,
    pub pool: Arc<WebCookiePool>,
    pub registry: Arc<ModelRegistry>,
    pub sessions: Arc<SessionMap>,
    pub api_keys: Arc<std::sync::RwLock<Vec<String>>>,
    pub semaphore: Arc<crate::semaphore::TieredSemaphore>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_dashboard))
        .route("/ui", get(handle_dashboard))
        .route("/healthz", get(handle_healthz))
        .route("/v1/models", get(handle_v1_models))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/responses", post(handle_responses))
        .route("/v1/messages", post(handle_claude_messages))
        .route("/api/tokens", get(handle_tokens_list))
        .route("/api/tokens/import", post(handle_tokens_import))
        .route("/api/tokens/login", post(handle_tokens_login))
        .route("/api/tokens/refresh-all", post(handle_tokens_refresh_all))
        .route("/api/tokens/delete", post(handle_tokens_delete))
        .route("/api/tokens/check", post(handle_tokens_check))
        .route("/api/ui/login", post(handle_ui_login))
        .route("/api/guide", get(handle_guide))
        .route("/api/config/api-key", post(handle_config_api_key))
        .route("/api/me/free-tier", get(handle_me_free_tier))
        .route("/api/me/chat-quotas", get(handle_me_quotas))
        .route(
            "/api/direct-chat/sessions",
            post(handle_upstream_create_session),
        )
        .route("/api/direct-chat/upload", post(handle_upstream_upload))
        .route("/v1/uploads", post(handle_v1_uploads))
        .with_state(state)
}

// ---------- 认证 ----------

fn check_api_key(
    cfg: &Config,
    api_keys: &std::sync::RwLock<Vec<String>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let keys = api_keys.read().map(|g| g.clone()).unwrap_or_default();
    if keys.is_empty() && cfg.api_keys.is_empty() {
        return Ok(()); // 未配置则不校验
    }
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    let x_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if keys.iter().any(|k| k == bearer || k == x_key) {
        return Ok(());
    }
    if cfg.api_keys.iter().any(|k| k == bearer || k == x_key) {
        return Ok(());
    }
    Err(ApiError::unauthorized(
        "无效的 API Key。请在面板生成 Key 或配置 config.json 的 api_keys",
    ))
}

/// 管理端点双认证：API Key 或 UI session cookie
/// （UI 登录后无需再带 API Key 就能管理凭证/配置）
fn check_admin_auth(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    // 1) API Key 校验（与 check_api_key 相同）
    if check_api_key(&state.cfg, &state.api_keys, headers).is_ok() {
        return Ok(());
    }
    // 2) UI session cookie 校验（仅当配置了 ui_password 时）
    if !state.cfg.ui_password.is_empty() {
        let ok = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .map(|ck| crate::ui_auth::check_session(ck, &state.cfg.ui_password))
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
    }
    Err(ApiError::unauthorized(
        "需要 API Key 或登录 Web 面板（/ui）",
    ))
}

// ---------- 面板 ----------

/// Web UI 登录页（内嵌最小密码表单）
async fn handle_dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // 未配置 ui_password → 不锁，直接放行
    if state.cfg.ui_password.is_empty() {
        let html = crate::web::INDEX_HTML.replace("__VERSION__", env!("CARGO_PKG_VERSION"));
        return Html(html).into_response();
    }
    // 已配置 → 校验 session cookie
    let ok = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|ck| crate::ui_auth::check_session(ck, &state.cfg.ui_password))
        .unwrap_or(false);
    if ok {
        let html = crate::web::INDEX_HTML.replace("__VERSION__", env!("CARGO_PKG_VERSION"));
        return Html(html).into_response();
    }
    // 未登录 → 返回登录页
    Response::builder()
        .status(200)
        .header("content-type", "text/html; charset=utf-8")
        .body(axum::body::Body::from(
            crate::ui_auth::LOGIN_HTML.to_string(),
        ))
        .unwrap()
}

/// Web UI 登录：POST /api/ui/login {password} → 设置 session cookie
async fn handle_ui_login(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if state.cfg.ui_password.is_empty() {
        return api_err_response(ApiError::bad_request("UI 未配置密码，无需登录"));
    }
    let pass = body
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // 常数时间比较
    let a = pass.as_bytes();
    let b = state.cfg.ui_password.as_bytes();
    let mut diff = (a.len() as u64) ^ (b.len() as u64);
    for i in 0..a.len().max(b.len()) {
        let av = if i < a.len() { a[i] as u64 } else { 0 };
        let bv = if i < b.len() { b[i] as u64 } else { 0 };
        diff |= av ^ bv;
    }
    if diff != 0 {
        return api_err_response(ApiError::unauthorized("密码错误"));
    }
    let token = crate::ui_auth::issue_token(&state.cfg.ui_password);
    Response::builder()
        .status(200)
        .header(
            "set-cookie",
            format!("th_ui_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800"),
        )
        .header("content-type", "application/json")
        .body(axum::body::Body::from(r#"{"ok":true}"#.to_string()))
        .unwrap()
}

async fn handle_healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let model_count = state.registry.all().await.len();
    let cred_count = state.pool.list().await.len();
    Json(
        json!({ "ok": true, "app": "tokenharbor2api", "version": env!("CARGO_PKG_VERSION"), "models": model_count, "credentials": cred_count }),
    )
}

// ---------- /v1/models ----------

async fn handle_v1_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let list = state.registry.all().await;
    let models = models::openai_models(&list);
    Json(json!({ "object": "list", "data": models })).into_response()
}

// ---------- OpenAI /v1/chat/completions ----------

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub web_search: Option<String>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
}

/// OpenAI Responses API 请求体（/v1/responses）
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    /// input 可以是字符串或消息数组
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub user: Option<String>,
}

/// 从 Responses input 提取文本内容（支持 string / 消息数组）
fn responses_input_text(input: &serde_json::Value) -> String {
    match input {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for item in arr {
                // 消息对象：{"role":"user","content":[...]}
                if let Some(content) = item.get("content") {
                    match content {
                        serde_json::Value::String(t) => out.push_str(t),
                        serde_json::Value::Array(parts) => {
                            for part in parts {
                                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                    out.push_str(t);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// 从 Responses input 提取附件（图片）
fn responses_attachments(input: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut atts = Vec::new();
    if let serde_json::Value::Array(arr) = input {
        for item in arr {
            if let Some(serde_json::Value::Array(parts)) = item.get("content") {
                for part in parts {
                    if part.get("type").and_then(|v| v.as_str()) == Some("input_image") {
                        if let Some(url) = part.get("image_url").and_then(|v| v.as_str()) {
                            if url.starts_with("data:image/") {
                                let (mime, b64) = split_data_url(url);
                                atts.push(serde_json::json!({
                                    "kind": "image",
                                    "mime": mime,
                                    "name": format!("image-{}.{}", atts.len() + 1, mime_ext(&mime)),
                                    "bytes": b64.len() as u64,
                                    "data_url": url
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    atts
}

/// OpenAI Responses API handler（/v1/responses）
async fn handle_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResponsesRequest>,
) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let created = chrono::Utc::now().timestamp();
    let response_id = format!(
        "resp_{}_{}",
        created,
        uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(12)
            .collect::<String>()
    );
    let model = state.registry.resolve(&body.model).await;

    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized(
            "未导入 TokenHarbor Cookie 凭证。请 POST /api/tokens/import 粘贴 sb-auth-auth-token Cookie",
        ));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };

    let is_free = model.contains(":free");
    let _permit = state.semaphore.acquire(is_free, false).await;

    let content = responses_input_text(&body.input);
    if content.trim().is_empty() && responses_attachments(&body.input).is_empty() {
        return api_err_response(ApiError::bad_request("input 内容为空"));
    }

    let thread_key = body
        .user
        .clone()
        .unwrap_or_else(|| "default-thread".to_string());
    let initial_binding = match state
        .sessions
        .ensure(&thread_key, &client, &model, Some(&cred.cookie))
        .await
    {
        Ok(b) => b,
        Err(e) => {
            state.pool.record_failure(&cred.id, 401).await;
            return api_err_response(ApiError::upstream(format!("创建上游会话失败: {e}")));
        }
    };
    let mut binding = initial_binding;
    let mut retried_429 = false;
    let mut attempt = 0usize;
    let resp_result: Result<reqwest::Response, anyhow::Error> = loop {
        attempt += 1;
        let atts = responses_attachments(&body.input);
        let stream_req = crate::upstream::StreamRequest {
            session_id: binding.upstream_id.clone(),
            content: content.clone(),
            model: model.clone(),
            attachments: if atts.is_empty() { None } else { Some(atts) },
            web_search: Some("auto".into()),
            tz: Some(crate::upstream::UpstreamClient::timezone()),
            use_kb: None,
            rewind_to: None,
        };
        match client.stream(&stream_req, Some(&cred.cookie)).await {
            Ok(up) => break Ok(up),
            Err(e) if crate::upstream::UpstreamClient::is_rate_limited(&e) && !retried_429 => {
                tracing::warn!("上游 429 会话限流，切换新会话重试 (attempt {attempt})");
                retried_429 = true;
                let _ = state.sessions.remove(&thread_key).await;
                match state
                    .sessions
                    .ensure(&thread_key, &client, &model, Some(&cred.cookie))
                    .await
                {
                    Ok(new_binding) => binding = new_binding,
                    Err(e2) => break Err(anyhow::anyhow!("429 换会话失败: {e2}")),
                }
                continue;
            }
            Err(e) => break Err(e),
        }
    };

    match resp_result {
        Ok(up) => {
            state.pool.record_success(&cred.id).await;
            state.sessions.touch(&thread_key, 2).await;
            if body.stream {
                let s = crate::protocol::responses_sse::responses_events(up, &model, &response_id);
                let body = axum::body::Body::from_stream(s);
                crate::protocol::responses_sse::ResponsesSseResponse { body }.into_response()
            } else {
                let text = collect_nonstream_text(up).await;
                let body = crate::protocol::responses_sse::responses_nonstream(
                    &text,
                    &model,
                    &response_id,
                );
                Response::builder()
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(text_body(&body)))
                    .unwrap()
            }
        }
        Err(e) => {
            if crate::upstream::UpstreamClient::is_rate_limited(&e) {
                state.pool.record_failure(&cred.id, 429).await;
                api_err_response(ApiError::rate_limited(format!("上游限流，请稍后重试: {e}")))
            } else {
                state.pool.record_failure(&cred.id, 502).await;
                api_err_response(ApiError::upstream(format!("上游对话失败: {e}")))
            }
        }
    }
}

fn message_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
            out
        }
        serde_json::Value::Null => String::new(),
        _ => content.to_string(),
    }
}

/// 从 OpenAI 消息中提取附件（image_url data 直传）
fn extract_attachments(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut atts = Vec::new();
    for m in messages {
        if let serde_json::Value::Array(arr) = &m.content {
            for part in arr {
                if let Some(ty) = part.get("type").and_then(|v| v.as_str()) {
                    if ty == "image_url" {
                        let url = part
                            .get("image_url")
                            .and_then(|v| v.get("url"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if url.starts_with("data:image/") {
                            let (mime, b64) = split_data_url(url);
                            atts.push(json!({
                                "kind": "image",
                                "mime": mime,
                                "name": format!("image-{}.{}", atts.len() + 1, mime_ext(&mime)),
                                "bytes": b64.len() as u64,
                                "data_url": url
                            }));
                        }
                    }
                }
            }
        }
    }
    atts
}

fn split_data_url(url: &str) -> (String, String) {
    let rest = url.strip_prefix("data:").unwrap_or(url);
    let (mime, rest) = rest.split_once(',').unwrap_or(("image/png", rest));
    let mime = mime.split(';').next().unwrap_or("image/png").to_string();
    (mime, rest.to_string())
}

fn mime_ext(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "audio/webm" => "webm",
        "audio/mpeg" => "mp3",
        "video/mp4" => "mp4",
        _ => "bin",
    }
}

async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChatRequest>,
) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let created = chrono::Utc::now().timestamp();
    let model = state.registry.resolve(&body.model).await;

    // 选凭证
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入 TokenHarbor Cookie 凭证。请 POST /api/tokens/import 粘贴 sb-auth-auth-token Cookie"));
    };
    // 选客户端
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };

    // 并发限流：免费模型单会话桶（默认 1），防上游风控
    let is_free = model.contains(":free");
    let _permit = state.semaphore.acquire(is_free, false).await;

    // 提取最终消息内容（上游 session 保留历史，只发最后一条 user）
    let last_user = body.messages.iter().rev().find(|m| m.role == "user");
    let content = last_user
        .map(|m| message_text(&m.content))
        .unwrap_or_default();
    if content.trim().is_empty() && extract_attachments(&body.messages).is_empty() {
        return api_err_response(ApiError::bad_request("消息内容为空"));
    }

    // 会话绑定：下游线程 id 或 user 字段（有 user 用 user；否则 default-thread 保持上下文）
    let thread_key = body
        .user
        .clone()
        .unwrap_or_else(|| "default-thread".to_string());
    let initial_binding = match state
        .sessions
        .ensure(&thread_key, &client, &model, Some(&cred.cookie))
        .await
    {
        Ok(b) => b,
        Err(e) => {
            state.pool.record_failure(&cred.id, 401).await;
            return api_err_response(ApiError::upstream(format!("创建上游会话失败: {e}")));
        }
    };
    let mut binding = initial_binding;
    let mut retried_429 = false;
    let mut attempt = 0usize;
    let resp_result: Result<reqwest::Response, anyhow::Error> = loop {
        attempt += 1;
        let atts = extract_attachments(&body.messages);
        let stream_req = crate::upstream::StreamRequest {
            session_id: binding.upstream_id.clone(),
            content: content.clone(),
            model: model.clone(),
            attachments: if atts.is_empty() { None } else { Some(atts) },
            web_search: Some(body.web_search.clone().unwrap_or_else(|| "auto".into())),
            tz: Some(crate::upstream::UpstreamClient::timezone()),
            use_kb: None,
            rewind_to: None,
        };
        match client.stream(&stream_req, Some(&cred.cookie)).await {
            Ok(up) => break Ok(up),
            Err(e) if crate::upstream::UpstreamClient::is_rate_limited(&e) && !retried_429 => {
                // 上游对单会话有连续消息速率限制（~10 条/窗口）：换新会话重试一次绕开
                tracing::warn!("上游 429 会话限流，切换新会话重试 (attempt {attempt})");
                retried_429 = true;
                let _ = state.sessions.remove(&thread_key).await;
                match state
                    .sessions
                    .ensure(&thread_key, &client, &model, Some(&cred.cookie))
                    .await
                {
                    Ok(new_binding) => binding = new_binding,
                    Err(e2) => break Err(anyhow::anyhow!("429 换会话失败: {e2}")),
                }
                continue;
            }
            Err(e) => break Err(e),
        }
    };

    match resp_result {
        Ok(up) => {
            state.pool.record_success(&cred.id).await;
            state.sessions.touch(&thread_key, 2).await;
            if body.stream {
                let s = crate::protocol::openai_sse::openai_events(
                    up,
                    &model,
                    created,
                    &binding.upstream_id,
                );
                let body = axum::body::Body::from_stream(s);
                SseResponse { body }.into_response()
            } else {
                let text = collect_nonstream_text(up).await;
                let body =
                    crate::protocol::openai_sse::openai_nonstream(&text, &model, created, 0, 0);
                Response::builder()
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(text_body(&body)))
                    .unwrap()
            }
        }
        Err(e) => {
            if crate::upstream::UpstreamClient::is_rate_limited(&e) {
                // 429 透传为 429（客户端正确退避），并记录凭证冷却
                state.pool.record_failure(&cred.id, 429).await;
                api_err_response(ApiError::rate_limited(format!("上游限流，请稍后重试: {e}")))
            } else {
                state.pool.record_failure(&cred.id, 502).await;
                api_err_response(ApiError::upstream(format!("上游对话失败: {e}")))
            }
        }
    }
}

fn text_body(s: &str) -> String {
    s.to_string()
}

/// 非流式：收集上游所有 chunk，拼成完整文本（追踪 event: 行）
async fn collect_nonstream_text(up: reqwest::Response) -> String {
    let reader = crate::protocol::stream::reader_with_bytes(up.bytes_stream());
    let mut reader = tokio::io::BufReader::new(reader);
    let mut line = String::new();
    let mut out = String::new();
    let mut pending_event = String::new();
    loop {
        line.clear();
        use tokio::io::AsyncBufReadExt;
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let t = line.trim();
        if let Some(ev) = t.strip_prefix("event: ") {
            pending_event = ev.trim().to_string();
            continue;
        }
        if let Some(data) = t.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                let evt = if pending_event.is_empty() {
                    v.get("event").and_then(|e| e.as_str()).unwrap_or("")
                } else {
                    pending_event.as_str()
                };
                match evt {
                    "chunk" => {
                        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                            out.push_str(d);
                        }
                    }
                    "done" => break,
                    "error" => break,
                    _ => {}
                }
            }
        }
    }
    out
}

// ---------- Anthropic /v1/messages ----------

#[derive(Debug, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub system: Option<serde_json::Value>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    #[serde(default)]
    pub content: serde_json::Value,
}

async fn handle_claude_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnthropicRequest>,
) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response_anthropic(e);
    }
    let model = state.registry.resolve(&body.model).await;

    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response_anthropic(ApiError::unauthorized(
            "未导入 TokenHarbor Cookie 凭证",
        ));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response_anthropic(ApiError::internal("客户端未初始化"));
    };

    // 并发限流（免费单会话桶）
    let is_free = model.contains(":free");
    let _permit = state.semaphore.acquire(is_free, false).await;

    // Anthropic content 可能是字符串或数组；取最后一条 user
    let last_user = body.messages.iter().rev().find(|m| m.role == "user");
    let content = last_user
        .map(|m| anthropic_text(&m.content))
        .unwrap_or_default();
    if content.trim().is_empty() {
        return api_err_response_anthropic(ApiError::bad_request("消息内容为空"));
    }

    let thread_key = body
        .metadata
        .as_ref()
        .and_then(|m| m.get("thread_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "default-thread".to_string());
    let initial_binding = match state
        .sessions
        .ensure(&thread_key, &client, &model, Some(&cred.cookie))
        .await
    {
        Ok(b) => b,
        Err(e) => {
            state.pool.record_failure(&cred.id, 401).await;
            return api_err_response_anthropic(ApiError::upstream(format!(
                "创建上游会话失败: {e}"
            )));
        }
    };
    let mut binding = initial_binding;
    let mut retried_429 = false;
    let mut attempt = 0usize;
    let resp_result: Result<reqwest::Response, anyhow::Error> = loop {
        attempt += 1;
        let atts = anthropic_attachments(&body.messages);
        let stream_req = StreamRequest {
            session_id: binding.upstream_id.clone(),
            content: content.clone(),
            model: model.clone(),
            attachments: if atts.is_empty() { None } else { Some(atts) },
            web_search: Some("auto".into()),
            tz: Some(UpstreamClient::timezone()),
            use_kb: None,
            rewind_to: None,
        };
        match client.stream(&stream_req, Some(&cred.cookie)).await {
            Ok(up) => break Ok(up),
            Err(e) if crate::upstream::UpstreamClient::is_rate_limited(&e) && !retried_429 => {
                tracing::warn!("上游 429 会话限流，切换新会话重试 (attempt {attempt})");
                retried_429 = true;
                let _ = state.sessions.remove(&thread_key).await;
                match state
                    .sessions
                    .ensure(&thread_key, &client, &model, Some(&cred.cookie))
                    .await
                {
                    Ok(new_binding) => binding = new_binding,
                    Err(e2) => break Err(anyhow::anyhow!("429 换会话失败: {e2}")),
                }
                continue;
            }
            Err(e) => break Err(e),
        }
    };

    match resp_result {
        Ok(up) => {
            state.pool.record_success(&cred.id).await;
            state.sessions.touch(&thread_key, 2).await;
            if body.stream {
                let body = axum::body::Body::from_stream(
                    crate::protocol::anthropic_sse::anthropic_events(
                        up,
                        &model,
                        &binding.upstream_id,
                    ),
                );
                AnthropicSseResponse { body }.into_response()
            } else {
                let text = collect_nonstream_text(up).await;
                let resp = json!({
                    "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                    "type": "message",
                    "role": "assistant",
                    "model": model,
                    "content": [{ "type": "text", "text": text }],
                    "stop_reason": "end_turn",
                    "stop_sequence": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                });
                Json(resp).into_response()
            }
        }
        Err(e) => {
            if crate::upstream::UpstreamClient::is_rate_limited(&e) {
                state.pool.record_failure(&cred.id, 429).await;
                api_err_response_anthropic(ApiError::rate_limited(format!(
                    "上游限流，请稍后重试: {e}"
                )))
            } else {
                state.pool.record_failure(&cred.id, 502).await;
                api_err_response_anthropic(ApiError::upstream(format!("上游对话失败: {e}")))
            }
        }
    }
}

fn anthropic_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn anthropic_attachments(messages: &[AnthropicMessage]) -> Vec<serde_json::Value> {
    let mut atts = Vec::new();
    for m in messages {
        if let serde_json::Value::Array(arr) = &m.content {
            for part in arr {
                if part.get("type").and_then(|v| v.as_str()) == Some("image") {
                    if let Some(src) = part.get("source") {
                        if let Some(data) = src.get("data").and_then(|v| v.as_str()) {
                            let media_type = src
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/png");
                            let url = format!("data:{media_type};base64,{data}");
                            atts.push(json!({
                                "kind": "image",
                                "mime": media_type,
                                "name": format!("image-{}.{}", atts.len() + 1, mime_ext(media_type)),
                                "bytes": data.len() as u64,
                                "data_url": url
                            }));
                        }
                    }
                }
            }
        }
    }
    atts
}

// ---------- 凭证管理 ----------

#[derive(Debug, Deserialize)]
pub struct TokenImportRequest {
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// 允许导入不含 refresh_token 的"一次性"凭证（默认 false → 必须可续期才接受）
    #[serde(default)]
    pub allow_partial: bool,
}

async fn handle_tokens_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let creds = state.pool.list().await;
    let states = state.pool.states().await;
    let items: Vec<serde_json::Value> = creds
        .iter()
        .map(|c| {
            let st = states.get(&c.id).cloned().unwrap_or_default();
            json!({
                "id": c.id,
                "label": c.label,
                "source": c.source,
                "created_at": c.created_at,
                "note": c.note,
                "cookie_masked": mask_cookie(&c.cookie),
                "health": st.health,
                "failures": st.failures,
                "refreshable": crate::refresh::refresh_token_from_cookie(&c.cookie).is_some(),
                "expires_at": crate::refresh::expires_at_from_cookie(&c.cookie),
            })
        })
        .collect();
    Json(json!({ "ok": true, "tokens": items })).into_response()
}

fn mask_cookie(cookie: &str) -> String {
    if cookie.len() <= 12 {
        return format!("{}***", &cookie[..cookie.len().min(4)]);
    }
    format!("{}...{}", &cookie[..6], &cookie[cookie.len() - 6..])
}

async fn handle_tokens_import(
    State(state): State<AppState>,
    Json(body): Json<TokenImportRequest>,
) -> Response {
    let raw = body.cookie.unwrap_or_default();
    if raw.trim().is_empty() {
        return api_err_response(ApiError::bad_request(
            "cookie 不能为空：支持裸 Cookie 头 / curl -b / curl -H / HAR / cookie jar / JSON 导出",
        ));
    }
    // 自动识别多种格式
    let cookie = match crate::import_parse::extract_cookie(&raw) {
        Some(c) if !c.is_empty() => c,
        _ => {
            return api_err_response(ApiError::bad_request(
                "无法从输入识别 Cookie：请粘贴 sb-auth-auth-token 等完整 Cookie 行、curl -b 命令、HAR 文件或 cookie jar（需包含登录后的 API 请求）",
            ));
        }
    };
    // 校验：必须含 sb-auth-auth-token.0 且能提取 refresh_token（否则无法自动续期）
    let analysis = crate::import_parse::analyze_cookie(&cookie);
    if !analysis.has_auth0 {
        return api_err_response(ApiError::bad_request(format!(
            "导入的 HAR/Cookie 缺少登录凭证 sb-auth-auth-token.0（当前仅识别到 {} 个 cookie 对）。\n\
             请重新在浏览器登录 tokenharbor.ai → F12 → Network → 勾选 Preserve log → 打开/刷新 Dashboard 或 Chat 页 → 点任意 API 请求（如 /api/me/free-tier、/api/direct-chat/sessions，URL 含 api 的请求）→ 右键 Export as HAR（含响应与请求头）→ 粘贴此 HAR。\n\
             仅含静态资源(js/css/图片)的 HAR 不含登录 Cookie，无法导入。",
            analysis.pair_count
        )));
    }
    if !analysis.refreshable && !body.allow_partial {
        return api_err_response(ApiError::bad_request(
            "导入的 Cookie 缺少 refresh_token（sb-auth-auth-token.0 内无有效 refresh_token），无法自动续期，过期后需重新登录。\n\
            检查：1) 该 Cookie 是否已过期（refresh_token 一次性，被浏览器/网关用过后即失效）；2) 是否从登录后的 API 请求抓取。\n\
            如需临时使用（不自动续期），传 allow_partial=true 强制导入。",
        ));
    }
    let cred = state.pool.add_raw(cookie).await;
    let refreshable = analysis.refreshable;
    let expires_at = analysis.expires_at;
    Json(json!({
        "ok": true,
        "id": cred.id,
        "label": cred.label,
        "refreshable": refreshable,
        "expires_at": expires_at,
        "has_auth0": analysis.has_auth0,
        "has_auth1": analysis.has_auth1,
        "has_th_sid": analysis.has_th_sid,
        "pair_count": analysis.pair_count,
        "hint": if refreshable {
            "导入成功：凭证包含 refresh_token，网关每 50 分钟自动续期，无需重新登录"
        } else {
            "警告：该凭证不含 refresh_token，无法自动续期；过期后需重新登录导入新 Cookie（或改用 /api/tokens/login 邮箱密码）"
        },
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct TokenLoginRequest {
    pub email: String,
    pub password: String,
}

async fn handle_tokens_login(
    State(state): State<AppState>,
    Json(body): Json<TokenLoginRequest>,
) -> Response {
    if body.email.trim().is_empty() || body.password.is_empty() {
        return api_err_response(ApiError::bad_request("email/password 不能为空"));
    }
    let proxy = if state.cfg.http_proxy.is_empty() {
        None
    } else {
        Some(state.cfg.http_proxy.clone())
    };
    match state
        .pool
        .login_email(&body.email, &body.password, proxy.as_deref())
        .await
    {
        Ok(cred) => {
            state.pool.record_success(&cred.id).await;
            Json(json!({ "ok": true, "id": cred.id, "label": cred.label })).into_response()
        }
        Err(e) => api_err_response(ApiError::upstream(format!("登录失败: {e}"))),
    }
}

async fn handle_tokens_refresh_all(State(state): State<AppState>) -> Response {
    let proxy = if state.cfg.http_proxy.is_empty() {
        None
    } else {
        Some(state.cfg.http_proxy.clone())
    };
    let n = state.pool.refresh_creds(proxy.as_deref()).await;
    Json(json!({ "ok": true, "refreshed": n })).into_response()
}

async fn handle_tokens_delete(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return api_err_response(ApiError::bad_request("缺少 id"));
    }
    let ok = state.pool.delete(&id).await;
    Json(json!({ "ok": ok })).into_response()
}

async fn handle_tokens_check(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let Some(cred) = state.pool.get(&id).await else {
        return api_err_response(ApiError::not_found("凭证不存在"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client.free_tier(Some(&cred.cookie)).await {
        Ok(v) => {
            state.pool.record_success(&id).await;
            Json(json!({ "ok": true, "free_tier": v })).into_response()
        }
        Err(e) => {
            state.pool.record_failure(&id, 401).await;
            api_err_response(ApiError::upstream(format!("凭证检查失败: {e}")))
        }
    }
}

// ---------- 面板/配置 ----------

async fn handle_guide(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let models = state.registry.all().await;
    let model_ids: Vec<String> = models
        .iter()
        .filter(|m| m.is_free)
        .map(|m| m.id.clone())
        .collect();
    Json(json!({
        "listen_addr": state.cfg.listen_addr,
        "api_keys_configured": !state.cfg.api_keys.is_empty() || !state.api_keys.read().map(|g| g.is_empty()).unwrap_or(true),
        "models": model_ids,
        "credential_count": state.pool.list().await.len(),
        "base_url": "http://127.0.0.1:47830/v1"
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyAction {
    pub action: String,
    #[serde(default)]
    pub key: Option<String>,
}

async fn handle_config_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApiKeyAction>,
) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let Ok(mut keys) = state.api_keys.write() else {
        return api_err_response(ApiError::internal("锁错误"));
    };
    match body.action.as_str() {
        "generate" => {
            let key = format!("sk-th-{}", uuid::Uuid::new_v4().simple());
            keys.push(key.clone());
            Json(json!({ "ok": true, "key": key })).into_response()
        }
        "set" => {
            let k = body.key.clone().unwrap_or_default().trim().to_string();
            if k.is_empty() {
                return api_err_response(ApiError::bad_request("缺少 key"));
            }
            if !keys.contains(&k) {
                keys.push(k.clone());
            }
            Json(json!({ "ok": true, "key": k })).into_response()
        }
        "clear" => {
            keys.clear();
            Json(json!({ "ok": true })).into_response()
        }
        _ => api_err_response(ApiError::bad_request("action 必须为 generate/set/clear")),
    }
}

// ---------- 上游直通（面板需要） ----------

async fn handle_me_free_tier(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入凭证"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client.free_tier(Some(&cred.cookie)).await {
        Ok(v) => Json(json!({ "ok": true, "free_tier": v })).into_response(),
        Err(e) => api_err_response(ApiError::upstream(format!("查询失败: {e}"))),
    }
}

async fn handle_me_quotas(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入凭证"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client.chat_quotas(Some(&cred.cookie)).await {
        Ok(v) => Json(json!({ "ok": true, "quotas": v })).into_response(),
        Err(e) => api_err_response(ApiError::upstream(format!("查询失败: {e}"))),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpstreamSessionReq {
    pub model: String,
    #[serde(default)]
    pub temporary: bool,
}

async fn handle_upstream_create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpstreamSessionReq>,
) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入凭证"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client
        .create_session(&body.model, body.temporary, Some(&cred.cookie))
        .await
    {
        Ok(id) => Json(
            json!({ "session": { "id": id, "model": body.model, "is_temporary": body.temporary } }),
        )
        .into_response(),
        Err(e) => api_err_response(ApiError::upstream(format!("创建会话失败: {e}"))),
    }
}

#[derive(Debug, Deserialize)]
pub struct UploadRequest {
    pub kind: String,
    pub name: String,
    pub mime: String,
    pub bytes: u64,
}

async fn handle_upstream_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UploadRequest>,
) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入凭证"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client
        .prepare_upload(
            &body.kind,
            &body.name,
            &body.mime,
            body.bytes,
            Some(&cred.cookie),
        )
        .await
    {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => api_err_response(ApiError::upstream(format!("上传准备失败: {e}"))),
    }
}

/// /v1/uploads：裸 body 上传换取 storagePath（站点 /api/direct-chat/upload 兼容）
async fn handle_v1_uploads(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(e) = check_admin_auth(&state, &headers) {
        return api_err_response(e);
    }
    let name = headers
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("upload.bin")
        .to_string();
    let mime = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let kind = if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if mime.starts_with("video/") {
        "video"
    } else {
        "file"
    };
    let Some(cred) = state.pool.pick(None).await else {
        return api_err_response(ApiError::unauthorized("未导入凭证"));
    };
    let Some(client) = state.clients.first().cloned() else {
        return api_err_response(ApiError::internal("客户端未初始化"));
    };
    match client
        .prepare_upload(kind, &name, &mime, body.len() as u64, Some(&cred.cookie))
        .await
    {
        Ok(resp) => {
            Json(json!({ "ok": true, "path": resp.path, "token": resp.token, "bytes": body.len() }))
                .into_response()
        }
        Err(e) => api_err_response(ApiError::upstream(format!("上传准备失败: {e}"))),
    }
}

// ---------- 错误响应 ----------

fn api_err_response(e: ApiError) -> Response {
    let status = e.status();
    (status, e.openai_json()).into_response()
}

fn api_err_response_anthropic(e: ApiError) -> Response {
    let status = e.status();
    (status, e.anthropic_json()).into_response()
}
