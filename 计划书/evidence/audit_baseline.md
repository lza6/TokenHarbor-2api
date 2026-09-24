# TokenHarbor2API — 审计基线（实测数据）

更新时间：2026-09-25（真实命令输出）
仓库：C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tokenharbor-2api（main = c5da765，干净）
版本：v0.2.8（Cargo.toml）

## 1. 构建与测试基线（本机 cargo 1.95.0 实测）

| 检查项 | 命令 | 结果 |
|---|---|---|
| 单元+集成测试 | cargo test --all-targets | **27/27 通过**（13 lib + 7 models_test + 7 pool_errors_test） |
| 行覆盖率 | cargo llvm-cov --lib --tests | **24.45%**（函数 27.94%，CI 门槛 20% 勉强过） |
| 编译器 | rustc 1.95.0 / cargo 1.95.0 | 可用 |
| 覆盖率工具 | cargo-llvm-cov 0.9.1 | 可用 |
| 静态检查 | clippy CI 门槛 -D warnings | 历史全绿（本地未在本次重跑，见正稿验证步骤） |

## 2. 按模块覆盖率（llvm-cov 实测）

| 模块 | 行覆盖 | 函数执行 |
|---|---|---|
| api.rs | 0.00%（1875 行全未测） | 0.00%（200 函数） |
| protocol/openai_sse.rs | 0.00% | 0.00% |
| protocol/anthropic_sse.rs | 0.00% | 0.00% |
| protocol/responses_sse.rs | 0.00% | 0.00% |
| protocol/stream.rs | 0.00% | 0.00% |
| refresh.rs | 0.00% | 0.00% |
| ui_auth.rs | 0.00% | 0.00% |
| ratelimit.rs | 0.00% | 0.00% |
| semaphore.rs | 0.00% | 0.00% |
| upstream.rs | 0.00% | 0.00% |
| main.rs | 0.00% | 0.00% |
| session.rs | 13.68% | 30.95% |
| config.rs | 48.28% | 52.63% |
| web_pool.rs | 55.77% | 53.62% |
| errors.rs | 73.24% | 76.92% |
| models.rs | 81.86% | 71.64% |
| import_parse.rs | 84.68% | 96.43% |
| redact.rs | 100% | 100% |
| **总计** | **24.45%** | **27.94%** |

结论：网关最核心的 HTTP/协议/续期/限流/并发/UI 认证路径全部 0% 覆盖，测试集中在纯函数（模型目录、导入解析、脱敏）。

## 3. 端点清单（build_router，api.rs L25-L60）

| 端点 | 认证 | 说明 |
|---|---|---|
| GET / | ui_password 判断 | 面板/登录页 |
| GET /ui | ui_password 判断 | 面板/登录页 |
| GET /healthz | 无 | 健康检查 |
| GET /v1/models | check_api_key | 模型列表 |
| POST /v1/chat/completions | check_api_key | OpenAI 聊天 |
| POST /v1/responses | check_api_key | Responses API |
| POST /v1/messages | check_api_key | Anthropic 聊天 |
| GET /api/tokens | check_admin_auth | 凭证列表 |
| POST /api/tokens/import | check_admin_auth | 凭证导入 |
| POST /api/tokens/login | check_admin_auth + LoginGuard | 邮箱密码登录 |
| POST /api/tokens/refresh-all | check_admin_auth | 手动续期 |
| POST /api/tokens/delete | check_admin_auth | 删除凭证 |
| POST /api/tokens/check | check_admin_auth | 凭证检查 |
| POST /api/ui/login | LoginGuard | UI 登录 |
| GET /api/guide | check_admin_auth | 接入信息 |
| POST /api/config/api-key | check_admin_auth | 运行时 Key 管理 |
| GET /api/me/free-tier | check_admin_auth | 免费额度直通 |
| GET /api/me/chat-quotas | check_admin_auth | 限额直通 |
| POST /api/direct-chat/sessions | check_admin_auth | 上游会话直通 |
| POST /api/direct-chat/upload | check_admin_auth | 上传预签名直通 |
| POST /v1/uploads | check_admin_auth | 裸 body 上传 |

## 4. 真实 E2E 旧证据（data/，2026-09-24/25 线上 52.141.3.10）

- data/stress429.txt：20 并发→ 15×200 + 5×429（冷却期透传 429）
- data/concurrency.txt：15/15 并发 200
- data/e2e_tools.json：/v1/responses 完整 response（qwen3.8-flash:free）
- data/multi_turn.txt：多轮记忆 OK
- workflow_status.md：v0.2.8 终局闭环审计矩阵（13 项需求全 ✅）

## 5. 已识别的关键风险/债务（供正稿引用）

1. 总覆盖率 24.45%，HTTP/协议/续期 0%（CI 门槛 20% 形同虚设）
2. /api/ui/login 无 CSRF 防护说明、无 rate-limit 全局中间件；LoginGuard 只有进程内存级
3. 会话映射 key = 客户端 user/thread_id，攻击者可注入任意 key 撑爆 200 会话上限（有上限，但无鉴权区分）
4. 单一凭证自动续期串行化；多凭证刷新是顺序 for 循环
5. ui_password 明文放 config.json；api_keys 运行时生成后不落盘（重启丢失）
6. /v1/uploads 用管理认证但 upstream 直通端点没有统一限流，费用/配额滥用面
7. Anthropic error 回退带 502 且错误文本直接拼接（部分未走 redact）
8. Web UI 单 HTML 内嵌，无构建无测试无 a11y 审计
9. CI 用法语注释（ci.yml/deploy.yml/DEVOPS.md），与 README 中文不一致
10. Cargo.lock/CHANGELOG 等历史版本产物重复段落（CHANGELOG v0.2.2/v0.1.0 重复）
11. docs/DEVOPS.md 声称 deployment 需要凭据未配置 — 与 workflow_status（已部署 Azure 52.141.3.10）不一致
12. 未配置 sqlite/telemetry 实际写入（sqlite_path/telemetry_path 存在但未见使用点）；token_saver 死配置（Config 有字段，代码未用）
13. 免费窗口硬编码 2026-09-27 到期（models.rs free_until），需动态刷新机制
14. request_timeout_sec=900 但 UpstreamClient::new 忽略 timeout 参数（_timeout），实际固定 read_timeout 300s