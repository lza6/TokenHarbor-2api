//! 模型注册表：TokenHarbor 模型目录 + 能力元数据
//!
//! 数据来源（2026-09-23 实时抓取 https://tokenharbor.ai/models RSC）：
//! - 18 个付费 surface（priceIn/priceOut per 1M tokens, USD）
//! - 3 个免费 route（mimo-v2.6-flash:free / qwen3.8-flash:free / deepseek-v4.1-flash:free）
//! - 1 个默认免费 th-rudder:free（站点常量 DEFAULT_CHAT_MODEL）
//!
//! 上游请求模型 id 形态：
//! - 付费：`{provider}/{surface}`（如 `alibaba/qwen3.8-max`、`vercel-ai-gateway/deepseek/deepseek-v4.1-flash`）
//! - 免费：`{provider}/{surface}:free`
//! - 兜底：`th-rudder:free`（Chat 站点默认，走 /api/direct-chat/stream）

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 上游请求 id（surface 前缀规范化）
pub const TH_BASE: &str = "https://tokenharbor.ai";
pub const DEFAULT_MODEL: &str = "th-rudder:free";

/// 上游 provider 前缀（surface → provider 映射，来自站点 familyFromModelId / 真实抓包）
pub const SURFACE_PROVIDER: &[(&str, &str)] = &[
    ("claude-opus-5.5", "anthropic"),
    ("claude-fable-5.1", "anthropic"),
    ("gpt-6-astra", "openai"),
    ("gpt-6-sol", "openai"),
    ("gpt-5.6-terra", "openai"),
    ("gpt-6-luna", "openai"),
    ("muse-spark-1-3", "vercel-ai-gateway"),
    ("grok-4.7", "x-ai"),
    ("qwen3.8-max", "alibaba"),
    ("qwen3.8-flash", "alibaba"),
    ("qwen3.8-27b", "alibaba"),
    ("glm-5.3", "z-ai"),
    ("glm-5.3-flash", "z-ai"),
    ("glm-5.3-flashx", "z-ai"),
    ("kimi-k3", "moonshotai"),
    ("mimo-v2.6-flash", "xiaomi"),
    ("mimo-v2.6-pro", "xiaomi"),
    ("gemini-3.8-flash", "google"),
    ("deepseek-v4.1-flash", "vercel-ai-gateway/deepseek"),
    ("th-rudder", "th-rudder"),
];

/// 免费 route：provider/surface:free
pub const FREE_MODELS: &[&str] = &[
    "xiaomi/mimo-v2.6-flash:free",
    "alibaba/qwen3.8-flash:free",
    "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMeta {
    /// 上游请求 id
    pub id: String,
    /// 展示名
    pub label: String,
    /// 家族
    pub family: String,
    /// frontier / value
    pub tier: String,
    /// 每 1M token 输入价（USD；免费为 0）
    pub price_in: f64,
    /// 每 1M token 输出价（USD；免费为 0）
    pub price_out: f64,
    /// 是否免费路线
    pub is_free: bool,
    /// 输入模态
    pub input_modalities: Vec<String>,
    /// 输出模态
    pub output_modalities: Vec<String>,
    /// 支持视觉
    pub vision: bool,
    /// 支持联网搜索
    pub web_search: bool,
    /// 支持知识库
    pub kb: bool,
    /// 支持工具调用
    pub tools: bool,
    /// 支持思考档位
    pub reasoning: bool,
    /// 免费窗口结束（None=常驻）
    pub free_until: Option<String>,
    /// 上下文窗口（tokens，来自官方模型文档）
    pub context_window: i64,
    /// 最大输出 tokens（官方文档）
    pub max_output: i64,
}

/// 权威底座（与实时快照一致）
pub fn catalog() -> Vec<ModelMeta> {
    let mut v = Vec::new();
    macro_rules! m {
        ($surface:expr, $label:expr, $family:expr, $tier:expr, $pi:expr, $po:expr, $free:expr, $inmods:expr, $outmods:expr, $vision:expr, $tools:expr, $ctx:expr, $mo:expr) => {
            v.push(ModelMeta {
                id: upstream_id($surface, $free),
                label: $label.into(),
                family: $family.into(),
                tier: $tier.into(),
                price_in: $pi,
                price_out: $po,
                is_free: $free,
                input_modalities: $inmods.iter().map(|s| s.to_string()).collect(),
                output_modalities: $outmods.iter().map(|s| s.to_string()).collect(),
                vision: $vision,
                web_search: true,
                kb: true,
                tools: true,
                reasoning: true,
                free_until: if $free { free_until($surface).map(|s| s.to_string()) } else { None },
                context_window: $ctx,
                max_output: $mo,
            });
        };
    }
    // 付费
    m!("claude-opus-5.5", "Claude Opus 5.5", "anthropic", "frontier", 4.0, 20.0, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("claude-fable-5.1", "Claude Fable 5.1", "anthropic", "frontier", 10.0, 50.0, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("gpt-6-astra", "GPT-6 Astra", "openai", "frontier", 10.0, 50.0, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("muse-spark-1-3", "Muse Spark 1.3", "vercel-ai-gateway", "frontier", 1.25, 4.25, false, &["text","image","file"], &["text"], true, true, 200000, 32000);
    m!("gpt-6-sol", "GPT-6 Sol", "openai", "frontier", 2.0, 10.0, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("grok-4.7", "Grok 4.7", "x-ai", "frontier", 2.0, 6.0, false, &["text","image","file"], &["text"], true, true, 200000, 32000);
    m!("qwen3.8-max", "Qwen3.8 Max", "qwen", "frontier", 2.0, 6.0, false, &["text","image","video"], &["text"], true, true, 256000, 64000);
    m!("glm-5.3", "GLM-5.3", "z-ai", "frontier", 1.4, 4.4, false, &["text"], &["text"], false, true, 200000, 32000);
    m!("kimi-k3", "Kimi K3", "kimi", "frontier", 3.0, 15.0, false, &["text","image","video"], &["text"], true, true, 256000, 32000);
    m!("gpt-5.6-terra", "GPT-5.6 Terra", "openai", "frontier", 2.0, 12.0, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("mimo-v2.6-flash", "MiMo V2.6 Flash", "xiaomi", "value", 0.14, 0.28, false, &["text","image","audio","video"], &["text"], true, true, 200000, 32000);
    m!("mimo-v2.6-pro", "MiMo V2.6 Pro", "xiaomi", "value", 0.435, 0.87, false, &["text","image","audio","video"], &["text"], true, true, 200000, 32000);
    m!("glm-5.3-flash", "GLM 5.3 Flash", "z-ai", "value", 0.15, 0.5, false, &["text","image","file"], &["text"], true, true, 200000, 32000);
    m!("glm-5.3-flashx", "GLM 5.3 FlashX", "z-ai", "value", 0.15, 0.5, false, &["text","image","file"], &["text"], true, true, 200000, 32000);
    m!("gemini-3.8-flash", "Gemini 3.8 Flash", "google", "value", 0.75, 3.75, false, &["text","image","audio","file"], &["text"], true, true, 1000000, 64000);
    m!("qwen3.8-flash", "Qwen3.8 Flash", "qwen", "value", 0.15, 0.47, false, &["text","image","video"], &["text"], true, true, 256000, 64000);
    m!("deepseek-v4.1-flash", "DeepSeek V4.1 Flash", "deepseek", "value", 0.3, 1.2, false, &["text","image"], &["text"], true, true, 1000000, 384000);
    m!("gpt-6-luna", "GPT-6 Luna", "openai", "value", 0.1, 0.5, false, &["text","image","file"], &["text"], true, true, 200000, 64000);
    m!("qwen3.8-27b", "Qwen3.8 27B", "qwen", "value", 0.35, 2.1, false, &["text","image","video"], &["text"], true, true, 256000, 64000);
    // 免费
    m!("mimo-v2.6-flash", "MiMo V2.6 Flash", "xiaomi", "value", 0.0, 0.0, true, &["text","image","audio","video"], &["text"], true, true, 200000, 32000);
    m!("qwen3.8-flash", "Qwen3.8 Flash", "qwen", "value", 0.0, 0.0, true, &["text","image","video"], &["text"], true, true, 256000, 64000);
    m!("deepseek-v4.1-flash", "DeepSeek V4.1 Flash", "deepseek", "value", 0.0, 0.0, true, &["text","image"], &["text"], true, true, 1000000, 384000);
    // 兜底 Rudder（站点默认免费 chat）
    v.push(ModelMeta {
        id: "th-rudder:free".into(),
        label: "TH-Rudder".into(),
        family: "tokenharbor".into(),
        tier: "value".into(),
        price_in: 0.0,
        price_out: 0.0,
        is_free: true,
        input_modalities: vec!["text".into(), "image".into()],
        output_modalities: vec!["text".into()],
        vision: true,
        web_search: true,
        kb: true,
        tools: true,
        reasoning: true,
        free_until: None,
        context_window: 200_000,
        max_output: 64_000,
    });
    v
}

/// 免费窗口（站点快照 freeUntil；过期后网关自动标记不可用并走降级链）
fn free_until(surface: &str) -> Option<&'static str> {
    match surface {
        "qwen3.8-flash" => Some("2026-09-27T13:00:00+00:00"),
        _ => None,
    }
}

/// surface → 上游完整 id（付费/免费）
pub fn upstream_id(surface: &str, is_free: bool) -> String {
    let surface = surface.trim();
    let surface = surface.strip_suffix(":free").unwrap_or(surface);
    for (s, p) in SURFACE_PROVIDER {
        if *s == surface {
            let p = if *p == "th-rudder" { "th-rudder".to_string() } else { p.to_string() };
            return if is_free { format!("{p}/{surface}:free") } else { format!("{p}/{surface}") };
        }
    }
    if is_free { format!("{surface}:free") } else { surface.to_string() }
}

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    inner: Arc<RwLock<Vec<ModelMeta>>>,
}

impl Default for ModelRegistry {
    fn default() -> Self { Self::new() }
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(catalog())) }
    }

    pub async fn all(&self) -> Vec<ModelMeta> {
        self.inner.read().await.clone()
    }

    pub async fn meta(&self, id: &str) -> Option<ModelMeta> {
        let list = self.inner.read().await;
        list.iter().find(|m| m.id == id).cloned()
    }

    pub async fn has_model(&self, id: &str) -> bool {
        self.inner.read().await.iter().any(|m| m.id == id)
    }

    /// 模型当前是否可用（免费窗口到期视为不可用；付费默认可用）
    pub async fn available(&self, id: &str) -> bool {
        self.available_at(id, Utc::now()).await
    }

    /// 指定时刻的可用性（免费窗口在 until 之后视为不可用）
    pub async fn available_at(&self, id: &str, now: DateTime<Utc>) -> bool {
        let Some(meta) = self.meta(id).await else { return false };
        if meta.is_free {
            if let Some(until) = &meta.free_until {
                if let Ok(until_dt) = DateTime::parse_from_rfc3339(until) {
                    if now > until_dt.with_timezone(&Utc) {
                        return false;
                    }
                }
            }
            return true;
        }
        true
    }

    /// 已注册的免费模型（用户请求 free route / 降级）
    pub async fn free_ids(&self) -> Vec<String> {
        let list = self.inner.read().await;
        list.iter().filter(|m| m.is_free).map(|m| m.id.clone()).collect()
    }



    /// 归一化请求 id：
    /// - qwen3.8-flash -> alibaba/qwen3.8-flash:free（裸 surface 优先免费路线）
    /// - anthropic/claude-opus-5.5 -> 原样（带 provider 前缀付费 id）
    /// - th-rudder:free -> 原样
    /// - 未知 -> 原样（上游给可读错误）
    pub async fn normalize(&self, requested: &str) -> String {
        let r = requested.trim();
        if r.is_empty() { return DEFAULT_MODEL.to_string(); }
        // 已在目录中（含 th-rudder:free、带 provider 前缀的付费 id）
        if self.has_model(r).await { return r.to_string(); }
        // 显式 :free 后缀
        let (surface, want_free) = if let Some(s) = r.strip_suffix(":free") {
            (s, true)
        } else { (r, false) };
        if surface.contains('/') {
            // 带 provider 前缀的完整 id 直接放行
            return r.to_string();
        }
        // 裸 surface：先试免费路线，再试付费
        let free_id = upstream_id(surface, true);
        if self.has_model(&free_id).await { return free_id; }
        let paid_id = upstream_id(surface, want_free);
        if self.has_model(&paid_id).await { return paid_id; }
        r.to_string()
    }

    /// 降级链：请求模型不可用时取第一条可用免费模型
    pub async fn resolve(&self, requested: &str) -> String {
        let norm = self.normalize(requested).await;
        if self.available(&norm).await { return norm; }
        for m in self.free_ids().await {
            if self.available(&m).await { return m; }
        }
        DEFAULT_MODEL.to_string()
    }
}

/// OpenAI /v1/models 形状（含上下文窗口，供客户端展示）
#[derive(Serialize, Deserialize)]
pub struct OpenAIModelObject {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
    pub context_window: i64,
    pub max_output_tokens: i64,
}

/// Anthropic /v1/models 形状
#[derive(Serialize, Deserialize)]
pub struct AnthropicModelObject {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub context_window: i64,
}

pub fn openai_models(list: &[ModelMeta]) -> Vec<OpenAIModelObject> {
    list.iter()
        .map(|m| OpenAIModelObject {
            id: m.id.clone(),
            object: "model".into(),
            created: 0,
            owned_by: m.family.clone(),
            context_window: m.context_window,
            max_output_tokens: m.max_output,
        })
        .collect()
}

pub fn anthropic_models(list: &[ModelMeta]) -> Vec<AnthropicModelObject> {
    list.iter()
        .map(|m| AnthropicModelObject {
            id: m.id.clone(),
            name: m.label.clone(),
            created: 0,
            input_modalities: m.input_modalities.clone(),
            output_modalities: m.output_modalities.clone(),
            context_window: m.context_window,
        })
        .collect()
}

/// 模型能力检索（面板/工具调用判定）
pub fn supports_tools(meta: &ModelMeta) -> bool { meta.tools }
pub fn supports_vision(meta: &ModelMeta) -> bool { meta.vision }
pub fn supports_search(meta: &ModelMeta) -> bool { meta.web_search }



