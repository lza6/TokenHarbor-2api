//! 上游 TokenHarbor HTTP 客户端
//!
//! 逆向自抓包数据包 + 站点 JS（0snqa017fr~7m.js）：
//! - POST /api/direct-chat/sessions           创建会话 `{model, temporary}`
//! - GET  /api/direct-chat/sessions/{id}      读取会话（含 messages）
//! - PATCH /api/direct-chat/sessions/{id}     更新（temporary=false / 系统参数）
//! - POST /api/direct-chat/stream             SSE 对话流
//! - POST /api/direct-chat/upload             换取 Supabase 签名上传
//! - POST /api/direct-chat/transcribe         语音转文字（FormData）
//! - GET  /api/me/free-tier                   免费额度
//! - GET  /api/me/chat-quotas                 每日限额
//!
//! 认证：Cookie（sb-auth-auth-token.0/.1 + th_sid + th_attr*），origin/referer 必须一致。

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub model: String,
    #[serde(default)]
    pub temporary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub is_temporary: Option<bool>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    #[serde(default)]
    pub session: Option<SessionInfo>,
    #[serde(default)]
    pub messages: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub content: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_kb: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewind_to: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub ok: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResponse {
    pub ok: bool,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpstreamClient {
    base_url: String,
    http: reqwest::Client,
    #[allow(dead_code)]
    cookie: Option<String>,
    #[allow(dead_code)]
    proxy: Option<String>,
}

impl UpstreamClient {
    pub fn new(
        base_url: String,
        cookie: Option<String>,
        proxy: Option<String>,
        _timeout: Duration,
    ) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(300))
            .pool_idle_timeout(Duration::from_secs(90))
            // 手工管理 Cookie header（凭证池按账号轮换），关闭 reqwest jar 以免干扰
            .cookie_store(false)
            .user_agent(DESKTOP_UA)
            .default_headers(default_headers(&base_url));
        if let Some(p) = &proxy {
            builder = builder.proxy(reqwest::Proxy::all(p)?);
        }
        let http = builder.build()?;
        let base_url = base_url.trim_end_matches('/').to_string();
        Ok(Self {
            base_url,
            http,
            cookie,
            proxy,
        })
    }

    fn cookie_header(&self, extra_cookie: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        let c = match (self.cookie.as_deref(), extra_cookie) {
            (Some(a), Some(b)) => format!("{a}; {b}"),
            (Some(a), None) => a.to_string(),
            (None, Some(b)) => b.to_string(),
            (None, None) => String::new(),
        };
        if !c.is_empty() {
            if let Ok(v) = HeaderValue::from_str(&c) {
                h.insert("cookie", v);
            }
        }
        h
    }

    /// 创建会话（返回 session id）
    pub async fn create_session(
        &self,
        model: &str,
        temporary: bool,
        cookie: Option<&str>,
    ) -> Result<String> {
        let url = format!("{}/api/direct-chat/sessions", self.base_url);
        let body = SessionRequest {
            model: model.to_string(),
            temporary,
        };
        let resp = self
            .http
            .post(&url)
            .headers(self.cookie_header(cookie))
            .json(&body)
            .send()
            .await
            .context("创建会话失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "创建会话失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        let parsed: SessionResponse = serde_json::from_str(&text)
            .with_context(|| format!("解析会话响应失败: {}", truncate(&text, 300)))?;
        Ok(parsed.session.id)
    }

    /// 读取会话（含历史消息）
    pub async fn get_session(&self, id: &str, cookie: Option<&str>) -> Result<SessionDetail> {
        let url = format!("{}/api/direct-chat/sessions/{id}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .headers(self.cookie_header(cookie))
            .send()
            .await
            .context("读取会话失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "读取会话失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        serde_json::from_str(&text).with_context(|| "解析会话详情失败".to_string())
    }

    /// 更新会话（把临时会话落库 / 改系统参数）
    pub async fn patch_session(
        &self,
        id: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
    ) -> Result<()> {
        let url = format!("{}/api/direct-chat/sessions/{id}", self.base_url);
        let resp = self
            .http
            .patch(&url)
            .headers(self.cookie_header(cookie))
            .json(&body)
            .send()
            .await
            .context("更新会话失败")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "更新会话失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        Ok(())
    }

    /// 对话流：返回响应体（SSE），由上层逐行转换
    pub async fn stream(
        &self,
        req: &StreamRequest,
        cookie: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/api/direct-chat/stream", self.base_url);
        let resp = self
            .http
            .post(&url)
            .headers(self.cookie_header(cookie))
            .json(req)
            .send()
            .await
            .context("上游对话流请求失败")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "上游对话流失败 HTTP {status}: {}",
                truncate(&text, 400)
            ));
        }
        Ok(resp)
    }

    /// 判断错误是否是上游 429 限流（会话速率限制，换新 session 可绕开）
    pub fn is_rate_limited(err: &anyhow::Error) -> bool {
        let msg = format!("{err:#}");
        msg.contains("429") || msg.contains("rate_limited") || msg.contains("too many")
    }

    /// 上传预签名（大文件走 Supabase；小图直接 base64 不发这里）
    pub async fn prepare_upload(
        &self,
        kind: &str,
        name: &str,
        mime: &str,
        bytes: u64,
        cookie: Option<&str>,
    ) -> Result<UploadResponse> {
        let url = format!("{}/api/direct-chat/upload", self.base_url);
        let body = serde_json::json!({ "kind": kind, "name": name, "mime": mime, "bytes": bytes });
        let resp = self
            .http
            .post(&url)
            .headers(self.cookie_header(cookie))
            .json(&body)
            .send()
            .await
            .context("上传准备失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "上传准备失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        serde_json::from_str(&text).with_context(|| "解析上传响应失败".to_string())
    }

    /// 语音转文字
    pub async fn transcribe(
        &self,
        audio: Vec<u8>,
        mime: String,
        cookie: Option<&str>,
    ) -> Result<TranscribeResponse> {
        let url = format!("{}/api/direct-chat/transcribe", self.base_url);
        let mime = if mime.is_empty() {
            "audio/webm".to_string()
        } else {
            mime
        };
        let part = reqwest::multipart::Part::bytes(audio)
            .file_name("clip")
            .mime_str(&mime)
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(Vec::new()).file_name("clip"));
        let form = reqwest::multipart::Form::new()
            .part("audio", part)
            .text("mime", mime);
        let resp = self
            .http
            .post(&url)
            .headers(self.cookie_header(cookie))
            .multipart(form)
            .send()
            .await
            .context("语音转文字失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "语音转文字失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        serde_json::from_str(&text).with_context(|| "解析转写响应失败".to_string())
    }

    /// 免费额度
    pub async fn free_tier(&self, cookie: Option<&str>) -> Result<serde_json::Value> {
        let url = format!("{}/api/me/free-tier", self.base_url);
        let resp = self
            .http
            .get(&url)
            .headers(self.cookie_header(cookie))
            .send()
            .await
            .context("查询免费额度失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "查询免费额度失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        serde_json::from_str(&text).with_context(|| "解析免费额度失败".to_string())
    }

    /// 每日限额
    pub async fn chat_quotas(&self, cookie: Option<&str>) -> Result<serde_json::Value> {
        let url = format!("{}/api/me/chat-quotas", self.base_url);
        let resp = self
            .http
            .get(&url)
            .headers(self.cookie_header(cookie))
            .send()
            .await
            .context("查询每日限额失败")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "查询每日限额失败 HTTP {status}: {}",
                truncate(&text, 300)
            ));
        }
        serde_json::from_str(&text).with_context(|| "解析每日限额失败".to_string())
    }

    /// 健康检查：可匿名访问的公共端点
    pub async fn check_health(&self) -> Result<()> {
        let url = format!("{}/api/public/tokens-served", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("上游健康检查失败")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("上游健康检查 HTTP {}", resp.status()))
        }
    }

    pub fn timezone() -> String {
        let now = Utc::now();
        format!("UTC{}", now.format("%z"))
    }
}

fn default_headers(base_url: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_static(DESKTOP_UA));
    h.insert("accept", HeaderValue::from_static("*/*"));
    h.insert(
        "accept-language",
        HeaderValue::from_static("zh-CN,zh;q=0.9"),
    );
    h.insert(
        "origin",
        HeaderValue::from_str(base_url)
            .unwrap_or(HeaderValue::from_static("https://tokenharbor.ai")),
    );
    h.insert(
        REFERER,
        HeaderValue::from_str(&format!("{base_url}/chat"))
            .unwrap_or(HeaderValue::from_static("https://tokenharbor.ai/chat")),
    );
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}
