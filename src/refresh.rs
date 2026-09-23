//! Supabase 会话刷新：Cookie 自动续期
//!
//! TokenHarbor 使用 Supabase 认证（auth.tokenharbor.ai / ref=isbnzmwjmtiumipesgmmg）。
//! Cookie `sb-auth-auth-token.0` 值是 `base64-<json>`，内含：
//!   access_token / refresh_token / expires_at / user 等。
//! access_token 1 小时过期；refresh_token 一次性，可换新 access+refresh。
//!
//! 本模块实现在到期前自动调用官方刷新端点，把新 token 写回 Cookie 字符串并落盘，
//! 用户只需登录一次、导入一次，网关长期自续。

use anyhow::{anyhow, Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

// Supabase 项目配置（anon key 是浏览器公开常量，非机密）
pub const SUPABASE_AUTH_URL: &str = "https://auth.tokenharbor.ai";
pub const SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImlzYm56bXdqbXRpdWlwZXNnbW1nIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzY3NjU1MzYsImV4cCI6MjA5MjM0MTUzNn0.CodUcchio6jNW_k68vaAb--LshBQXK51tZ6VTxNSz_A";

/// 从 Cookie 字符串中解码 sb-auth-auth-token.0 的 base64 原始字节
/// （Supabase 值可能被浏览器/导出截断，但 refresh_token 在 JSON 前段，正则即可提取）
fn decode_auth0(cookie: &str) -> Option<Vec<u8>> {
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("sb-auth-auth-token.0=") {
            let b64 = rest
                .strip_prefix("base64-")
                .map(|s| s.to_string())
                .unwrap_or_else(|| rest.to_string());
            let b64 = b64.trim().trim_matches('"').to_string();
            // 修正 base64 长度（偶发截断：尝试去掉 1-3 尾字符）
            for cut in 0..=3 {
                let candidate = if cut == 0 { b64.clone() } else { b64[..b64.len() - cut].to_string() };
                if let Ok(decoded) = URL_SAFE_NO_PAD.decode(candidate.as_bytes()) {
                    return Some(decoded);
                }
            }
        }
    }
    None
}

/// 从 Cookie 提取 refresh_token（正则扫描解码字节，不要求 JSON 完整）
pub fn refresh_token_from_cookie(cookie: &str) -> Option<String> {
    let bytes = decode_auth0(cookie)?;
    let text = String::from_utf8_lossy(&bytes);
    let re = regex::Regex::new(r#""refresh_token":"([^"]+)""#).ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// 从 Cookie 提取 access_token 过期时刻（epoch 秒）
pub fn expires_at_from_cookie(cookie: &str) -> Option<i64> {
    let bytes = decode_auth0(cookie)?;
    let text = String::from_utf8_lossy(&bytes);
    let re = regex::Regex::new(r#""expires_at":(\d+)"#).ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

/// Supabase 刷新响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResponse {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub user: Option<serde_json::Value>,
}

/// 邮箱密码登录（Supabase /auth/v1/token?grant_type=password）
pub async fn email_login(
    email: &str,
    password: &str,
    proxy: Option<&str>,
) -> Result<RefreshResponse> {
    let url = format!("{SUPABASE_AUTH_URL}/auth/v1/token?grant_type=password");
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(20));
    if let Some(p) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(p)?);
    }
    let client = builder.build()
        .context("构造登录客户端失败")?;
    let resp = client
        .post(&url)
        .header("apikey", SUPABASE_ANON_KEY)
        .header("authorization", format!("Bearer {}", SUPABASE_ANON_KEY))
        .header("content-type", "application/json")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .context("登录请求失败")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // 401/400 带错误详情
        let detail = truncate(&text, 300);
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::BAD_REQUEST {
            return Err(anyhow!("登录失败: {detail}"));
        }
        return Err(anyhow!("登录请求 HTTP {status}: {detail}"));
    }
    let parsed: RefreshResponse = serde_json::from_str(&text)
        .with_context(|| format!("解析登录响应失败: {}", truncate(&text, 300)))?;
    if parsed.access_token.is_empty() || parsed.refresh_token.is_empty() {
        return Err(anyhow!("登录响应缺少 token: {}", truncate(&text, 300)));
    }
    Ok(parsed)
}

/// 用 refresh_token 换新会话
pub async fn refresh_session(
    refresh_token: &str,
    proxy: Option<&str>,
) -> Result<RefreshResponse> {
    let url = format!("{SUPABASE_AUTH_URL}/auth/v1/token?grant_type=refresh_token");
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(20));
    if let Some(p) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(p)?);
    }
    let client = builder.build()
        .context("构造刷新客户端失败")?;
    let resp = client
        .post(&url)
        .header("apikey", SUPABASE_ANON_KEY)
        .header("authorization", format!("Bearer {}", SUPABASE_ANON_KEY))
        .header("content-type", "application/json")
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .context("刷新请求失败")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Supabase 刷新失败 HTTP {status}: {text}"));
    }
    let parsed: RefreshResponse = serde_json::from_str(&text)
        .with_context(|| format!("解析刷新响应失败: {}", truncate(&text, 300)))?;
    if parsed.access_token.is_empty() || parsed.refresh_token.is_empty() {
        return Err(anyhow!("刷新响应缺少 token: {}", truncate(&text, 300)));
    }
    Ok(parsed)
}

/// 用新会话组装 Cookie 字符串（保留原 cookie 其它字段，替换 tok0/tok1）
pub fn rebuild_cookie(old_cookie: &str, session: &RefreshResponse) -> String {
    // 新 tok0 = base64-<json{access_token, token_type, expires_in, expires_at, refresh_token, user}>
    let new_auth0 = serde_json::json!({
        "access_token": session.access_token,
        "token_type": session.token_type.clone().unwrap_or_else(|| "bearer".into()),
        "expires_in": session.expires_in.unwrap_or(3600),
        "expires_at": session.expires_at.unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600),
        "refresh_token": session.refresh_token,
        "user": session.user.clone().unwrap_or(serde_json::json!({})),
    });
    let tok0 = format!("base64-{}", encode_json(&new_auth0));
    let mut parts: Vec<String> = Vec::new();
    for part in old_cookie.split(';') {
        let part = part.trim();
        // 续期后移除 tok0 + tok1（上游实测：带 tok1 的续期 cookie 401，仅 tok0 + th_* 201）
        if part.starts_with("sb-auth-auth-token.0=") || part.starts_with("sb-auth-auth-token.1=") {
            continue;
        }
        if !part.is_empty() {
            parts.push(part.to_string());
        }
    }
    parts.push(format!("sb-auth-auth-token.0={tok0}"));
    parts.join("; ")
}

fn encode_json(v: &serde_json::Value) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap_or_default())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}...", &s[..n]) }
}
