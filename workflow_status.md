# workflow_status.md — TokenHarbor2API 终局闭环审计

更新时间: 2026-09-25

## 需求追踪矩阵

| # | 需求 | 模块/文件 | 状态 | 证据 |
|---|---|---|---|---|
| 1 | 复刻 Freebuff-2API（上游改为 TokenHarbor） | src/upstream.rs, api.rs | ✅ 闭环 | /v1/chat/completions + /v1/responses 真实对话 E2E |
| 2 | /v1/responses 原生格式 | src/protocol/responses_sse.rs | ✅ 闭环 | 流式 created→reasoning→delta→done→completed 全链 |
| 3 | 多账号负载均衡 | src/web_pool.rs pick() | ✅ 闭环 | 加权轮询（健康分优先 + 同分最旧） |
| 4 | 429 自动换会话 + 透传 | src/api.rs retried_429 | ✅ 闭环 | 日志 6 次换会话重试 + 透传 429 |
| 5 | 单账号并发控制 | src/semaphore.rs | ✅ 闭环 | 免费1/多3/付费3/多8 分层信号量 |
| 6 | 会话上限/自动清理 | src/session.rs | ✅ 闭环(v0.2.7加固) | MAX_SESSIONS=200 + 24h 清理 + 锁清理防 DoS |
| 7 | 多格式凭证导入 | src/import_parse.rs | ✅ 闭环 | 裸Cookie/curl/HAR/jar/JSON 7 格式 |
| 8 | 导入强校验（必须 refresh_token） | handle_tokens_import | ✅ 闭环 | 无 auth0 → 400 + 抓包指引；refreshable 诊断 |
| 9 | 自动续期 | src/refresh.rs + main.rs 50min | ✅ 闭环 | refresh-all 实测 refreshed:1 + expires_at 更新 |
| 10 | Web UI 门锁 | src/ui_auth.rs + handle_dashboard | ✅ 闭环 | 未登录→登录页；错密→401；HMAC session |
| 11 | 管理端点双认证 | check_admin_auth | ✅ 闭环(v0.2.7) | API key 或 UI session；空 key 放行已修 |
| 12 | 部署 Azure + systemd | docs/DEPLOY.md | ✅ 闭环 | 52.141.3.10 active + 开机自启 |
| 13 | CI/CD 流水线 | .github/workflows | ✅ 闭环 | build/quality/test/security/deploy |

## 审计发现与修复记录

| 级别 | 发现 | 修复 | 版本 |
|---|---|---|---|
| BLOCKER | 5 个 /api/tokens/* 端点无认证（可匿名增删凭证/中转密码） | 补 check_admin_auth | v0.2.7 |
| BLOCKER | check_admin_auth 空 key 放行（ui_password 非空也失效） | 重写独立认证逻辑 | v0.2.7 |
| MAJOR | 429 只降分不冷却（连续选中触发上游风控） | 429 进短冷却 60s 起 | v0.2.7 |
| MAJOR | session 锁 map 无界增长（DoS） | remove 清理锁 + remove_inner 防死锁 | v0.2.7 |
| MAJOR | /v1/models 用错认证组 | 改回 check_api_key | v0.2.7 |
| MAJOR | 错误信息过度暴露 / redact_logs 死配置 | 已知，建议后续接入 | 待办 |

## 服务器部署状态（52.141.3.10）

- v0.2.7 active + 开机自启
- 无认证管理端点 401 / 带 key 200 / /v1/models 无 key 401 ✅
- 凭证 43ecedde 已过期（浏览器与网关竞争消费 refresh_token）→ 需用户重新登录导入

## 阻塞项

1. TokenHarbor 新凭证（Google 验证码或邮箱密码）— 用户配合
