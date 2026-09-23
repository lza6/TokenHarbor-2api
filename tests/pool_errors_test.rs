//! 凭证池 / 错误形状 / 会话映射测试

use tokenharbor2api::config::Config;
use tokenharbor2api::errors::ApiError;
use tokenharbor2api::session::SessionMap;
use tokenharbor2api::web_pool::WebCookiePool;

#[tokio::test]
async fn pool_add_dedupe_and_pick() {
    let cfg = Config::default();
    let pool = WebCookiePool::new();
    pool.load(&cfg).await;
    let c1 = pool.add_raw("sb-auth-auth-token.0=abc; th_sid=1".to_string()).await;
    let c2 = pool.add_raw("sb-auth-auth-token.0=abc; th_sid=1".to_string()).await; // 同值去重
    assert_eq!(c1.id, c2.id);
    let c3 = pool.add_raw("sb-auth-auth-token.0=xyz; th_sid=2".to_string()).await;
    assert_ne!(c1.id, c3.id);
    assert_eq!(pool.list().await.len(), 2);

    // pick 应返回某个
    let picked = pool.pick(None).await;
    assert!(picked.is_some());
    // 排除 c3 后应返回 c1
    let picked2 = pool.pick(Some(&c3.id)).await;
    assert!(picked2.is_some());
    assert_eq!(picked2.unwrap().id, c1.id);
}

#[tokio::test]
async fn pool_failure_cooldown() {
    let cfg = Config::default();
    let pool = WebCookiePool::new();
    pool.load(&cfg).await;
    let c1 = pool.add_raw("sb-auth-auth-token.0=abc; th_sid=1".to_string()).await;
    let c2 = pool.add_raw("sb-auth-auth-token.0=xyz; th_sid=2".to_string()).await;
    pool.record_failure(&c1.id, 401).await;
    // c1 进入冷却，pick 应返回 c2
    let picked = pool.pick(None).await;
    assert!(picked.is_some());
    assert_eq!(picked.unwrap().id, c2.id);
    let st = pool.states().await;
    assert!(st.get(&c1.id).unwrap().cooling_until.is_some());
    assert!(st.get(&c1.id).unwrap().health < 1.0);
}

#[tokio::test]
async fn pool_success_recovers() {
    let cfg = Config::default();
    let pool = WebCookiePool::new();
    pool.load(&cfg).await;
    let c1 = pool.add_raw("sb-auth-auth-token.0=abc; th_sid=1".to_string()).await;
    pool.record_failure(&c1.id, 403).await;
    pool.record_success(&c1.id).await;
    let st = pool.states().await;
    let s = st.get(&c1.id).unwrap();
    assert!(s.cooling_until.is_none());
    assert!(s.health > 0.9);
}

#[test]
fn error_openai_shape() {
    let e = ApiError::unauthorized("bad key");
    let v = e.openai_json().0;
    assert_eq!(v["error"]["type"], "authentication_error");
    assert_eq!(v["error"]["message"], "bad key");
}

#[test]
fn error_anthropic_shape() {
    let e = ApiError::bad_request("empty");
    let v = e.anthropic_json().0;
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(v["error"]["message"], "empty");
}

#[test]
fn error_status_mapping() {
    use axum::http::StatusCode;
    assert_eq!(ApiError::bad_request("x").status(), StatusCode::BAD_REQUEST);
    assert_eq!(ApiError::unauthorized("x").status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ApiError::upstream("x").status(), StatusCode::BAD_GATEWAY);
    assert_eq!(ApiError::rate_limited("x").status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(ApiError::not_found("x").status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_map_reuse_and_rebuild() {
    let map = SessionMap::new();
    let b1 = tokenharbor2api::session::SessionBinding {
        upstream_id: "up-1".into(),
        model: "alibaba/qwen3.8-flash:free".into(),
        created_at: "2026-09-24T00:00:00+00:00".into(),
        last_active: "2026-09-24T00:00:00+00:00".into(),
        message_count: 0,
    };
    map.insert("thread-a", b1.clone()).await;
    let got = map.get("thread-a").await.unwrap();
    assert_eq!(got.upstream_id, "up-1");
    assert_eq!(got.model, "alibaba/qwen3.8-flash:free");

    // 同模型不同 key → 不同绑定
    let b2 = tokenharbor2api::session::SessionBinding {
        upstream_id: "up-2".into(),
        model: "th-rudder:free".into(),
        created_at: "2026-09-24T00:00:00+00:00".into(),
        last_active: "2026-09-24T00:00:00+00:00".into(),
        message_count: 0,
    };
    map.insert("thread-b", b2).await;
    assert_eq!(map.len().await, 2);

    // remove
    map.remove("thread-a").await;
    assert_eq!(map.len().await, 1);
    assert!(map.get("thread-a").await.is_none());
}

