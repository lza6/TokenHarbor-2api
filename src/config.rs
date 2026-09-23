//! 配置解析（config.json + 环境变量覆盖）

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 监听地址
    #[serde(default = "default_listen")]
    pub listen_addr: String,
    /// 上游地址
    #[serde(default = "default_upstream")]
    pub upstream_base_url: String,
    /// 上游认证 Cookie（sb-auth-auth-token 等；可留空用面板/导入）
    #[serde(default)]
    pub auth_tokens: Vec<String>,
    /// 下游 API Key（配置后客户端必须带）
    #[serde(default)]
    pub api_keys: Vec<String>,
    /// HTTP 代理
    #[serde(default)]
    pub http_proxy: String,
    /// 轮询健康检查周期（秒）
    #[serde(default = "default_rotation")]
    pub rotation_interval_sec: u64,
    /// 请求超时（秒）
    #[serde(default = "default_timeout")]
    pub request_timeout_sec: u64,
    /// 会话保活周期（秒）
    #[serde(default = "default_keepalive")]
    pub session_keepalive_sec: u64,
    /// 默认模型（th-rudder 免费）
    #[serde(default = "default_model")]
    pub default_model: String,
    /// 降级链（付费/免费都可用时优先免费）
    #[serde(default = "default_fallbacks")]
    pub fallback_models: Vec<String>,
    /// token 节省（压缩超长 tool_result）
    #[serde(default)]
    pub token_saver: bool,
    #[serde(default = "default_sqlite")]
    pub sqlite_path: String,
    #[serde(default = "default_tokens")]
    pub tokens_path: String,
    #[serde(default = "default_telemetry")]
    pub telemetry_path: String,
    #[serde(default)]
    pub web_dir: String,
    /// 跳过上游健康检查（本地开发/代理环境）
    #[serde(default = "default_true")]
    pub skip_upstream_check: bool,
    /// 日志脱敏
    #[serde(default = "default_true")]
    pub redact_logs: bool,
    /// 每账号并发槽（免费/付费/多会话）
    #[serde(default = "default_conc")]
    pub concurrency_free_slots: usize,
    #[serde(default = "default_conc3")]
    pub concurrency_free_multi: usize,
    #[serde(default = "default_conc3")]
    pub concurrency_sub_slots: usize,
    #[serde(default = "default_conc8")]
    pub concurrency_sub_multi: usize,
}

fn default_listen() -> String { "127.0.0.1:47830".into() }
fn default_upstream() -> String { "https://tokenharbor.ai".into() }
fn default_rotation() -> u64 { 21600 }
fn default_timeout() -> u64 { 900 }
fn default_keepalive() -> u64 { 45 }
fn default_model() -> String { "th-rudder:free".into() }
fn default_fallbacks() -> Vec<String> {
    vec![
        "qwen3.8-flash:free".into(),
        "deepseek-v4.1-flash:free".into(),
        "mimo-v2.6-flash:free".into(),
    ]
}
fn default_sqlite() -> String { "data/tokenharbor2api.sqlite".into() }
fn default_tokens() -> String { "data/tokens.json".into() }
fn default_telemetry() -> String { "data/telemetry.sqlite".into() }
fn default_true() -> bool { true }
fn default_conc() -> usize { 1 }
fn default_conc3() -> usize { 3 }
fn default_conc8() -> usize { 8 }

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: default_listen(),
            upstream_base_url: default_upstream(),
            auth_tokens: vec![],
            api_keys: vec![],
            http_proxy: String::new(),
            rotation_interval_sec: default_rotation(),
            request_timeout_sec: default_timeout(),
            session_keepalive_sec: default_keepalive(),
            default_model: default_model(),
            fallback_models: default_fallbacks(),
            token_saver: false,
            sqlite_path: default_sqlite(),
            tokens_path: default_tokens(),
            telemetry_path: default_telemetry(),
            web_dir: String::new(),
            skip_upstream_check: default_true(),
            redact_logs: default_true(),
            concurrency_free_slots: default_conc(),
            concurrency_free_multi: default_conc3(),
            concurrency_sub_slots: default_conc3(),
            concurrency_sub_multi: default_conc8(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&std::path::Path>) -> Result<Self> {
        let mut cfg: Config = if let Some(p) = path {
            if p.exists() {
                let raw = std::fs::read_to_string(p)
                    .with_context(|| format!("读取配置文件失败: {}", p.display()))?;
                serde_json::from_str(&raw)
                    .with_context(|| format!("解析配置文件失败: {}", p.display()))?
            } else {
                Config::default()
            }
        } else {
            Config::default()
        };
        // 环境变量覆盖
        if let Ok(v) = std::env::var("LISTEN_ADDR") { cfg.listen_addr = v; }
        if let Ok(v) = std::env::var("UPSTREAM_BASE_URL") { cfg.upstream_base_url = v; }
        if let Ok(v) = std::env::var("AUTH_TOKENS") {
            cfg.auth_tokens = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        if let Ok(v) = std::env::var("API_KEYS") {
            cfg.api_keys = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        if let Ok(v) = std::env::var("HTTP_PROXY") { cfg.http_proxy = v; }
        Ok(cfg)
    }

    pub fn resolve_config_path() -> Option<std::path::PathBuf> {
        #[cfg(windows)]
        {
            if let Ok(p) = std::env::var("APPDATA") {
                let cand = PathBuf::from(&p).join("tokenharbor2api").join("config.json");
                if cand.exists() { return Some(cand); }
            }
        }
        let local = PathBuf::from("config.json");
        if local.exists() { Some(local) } else { None }
    }
}
