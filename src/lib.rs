//! TokenHarbor2API — TokenHarbor 免费模型 OpenAI/Anthropic 兼容 API 网关（Rust axum 版）
//!
//! 上游：https://tokenharbor.ai
//! 协议逆向来源：抓包数据包（数据包.txt）+ 站点 JS chunk + /models RSC 实时快照
//!
//! 支持的标准化端点：
//! - POST /v1/chat/completions   OpenAI 聊天（流式/非流式）
//! - POST /v1/messages           Claude 聊天（双向转换）
//! - GET  /v1/models             模型列表（付费/免费/能力元数据）
//! - GET  /healthz               健康检查
//! - /api/*                      TokenHarbor 上游直通端点（会话/上传/配额/知识库/语音）
//! - /ui                         内嵌控制面板

pub mod api;
pub mod config;
pub mod errors;
pub mod models;
pub mod refresh;
pub mod protocol;
pub mod upstream;
pub mod web;
pub mod web_pool;
pub mod session;

pub const APP_NAME: &str = "tokenharbor2api";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

