//! 模型注册表测试：归一化 / 免费窗口 / 目录

use tokenharbor2api::models::{self, ModelRegistry};
use chrono::{DateTime, Utc};

#[tokio::test]
async fn normalize_free_surfaces() {
    let r = ModelRegistry::new();
    // 无 :free → 自动补免费 provider 前缀
    assert_eq!(r.normalize("qwen3.8-flash").await, "alibaba/qwen3.8-flash:free");
    assert_eq!(r.normalize("deepseek-v4.1-flash").await, "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free");
    assert_eq!(r.normalize("mimo-v2.6-flash:free").await, "xiaomi/mimo-v2.6-flash:free");
    // 兜底模型原样
    assert_eq!(r.normalize("th-rudder:free").await, "th-rudder:free");
    // 已带 provider 前缀放行
    assert_eq!(r.normalize("anthropic/claude-opus-5.5").await, "anthropic/claude-opus-5.5");
}

#[tokio::test]
async fn normalize_unknown_passthrough() {
    let r = ModelRegistry::new();
    assert_eq!(r.normalize("unknown/model-x").await, "unknown/model-x");
    assert_eq!(r.normalize("").await, "th-rudder:free");
}

#[tokio::test]
async fn free_window_now_available() {
    let r = ModelRegistry::new();
    // qwen3.8-flash:free 免费窗口到 2026-09-27，今天 2026-09-24 应可用
    assert!(r.available("alibaba/qwen3.8-flash:free").await);
    assert!(r.available("th-rudder:free").await);
}

#[tokio::test]
async fn free_window_expired_unavailable() {
    let r = ModelRegistry::new();
    // 构造 2026-09-30 时刻：qwen3.8-flash:free 窗口已过
    let later = DateTime::parse_from_rfc3339("2026-09-30T00:00:00+00:00").unwrap().with_timezone(&Utc);
    assert!(!r.available_at("alibaba/qwen3.8-flash:free", later).await);
    // th-rudder 常驻仍可用
    assert!(r.available_at("th-rudder:free", later).await);
}

#[tokio::test]
async fn resolve_falls_back_when_expired() {
    let r = ModelRegistry::new();
    // 未知模型 → 降级到第一条可用免费
    let resolved = r.resolve("does-not-exist").await;
    assert!(resolved.contains(":free") || resolved == "th-rudder:free");
}

#[test]
fn upstream_id_mapping() {
    assert_eq!(models::upstream_id("qwen3.8-flash", true), "alibaba/qwen3.8-flash:free");
    assert_eq!(models::upstream_id("deepseek-v4.1-flash", true), "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free");
    assert_eq!(models::upstream_id("th-rudder", false), "th-rudder/th-rudder");
    assert_eq!(models::upstream_id("claude-opus-5.5", false), "anthropic/claude-opus-5.5");
    assert_eq!(models::upstream_id("kimi-k3", false), "moonshotai/kimi-k3");
}

#[test]
fn catalog_has_expected_models() {
    let list = models::catalog();
    assert!(list.len() >= 22, "catalog len = {}", list.len());
    assert!(list.iter().any(|m| m.id == "th-rudder:free"));
    assert!(list.iter().any(|m| m.id == "alibaba/qwen3.8-flash:free"));
    assert!(list.iter().any(|m| m.id == "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free"));
    assert!(list.iter().any(|m| m.id == "xiaomi/mimo-v2.6-flash:free"));
    // 免费模型价格必须为 0
    for m in list.iter().filter(|m| m.is_free) {
        assert_eq!(m.price_in, 0.0);
        assert_eq!(m.price_out, 0.0);
    }
}
