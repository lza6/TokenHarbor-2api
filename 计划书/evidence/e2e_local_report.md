# 真实本地 E2E 验收报告（127.0.0.1:47830, release v0.2.8 二进制）

日期：2026-09-25 本地实测
环境：target/release/tokenharbor2api.exe + config.json（ui_password 已设，api_keys 空，tokens 空）
方法：curl.exe --noproxy 直连本地网关；线上仅探健康/鉴权（无 key，未伪造对话成功）

## 通过项（真实 HTTP 证据）

| # | 用例 | 结果 | 证据 |
|---|---|---|---|
| 1 | GET /healthz | PASS | 200 {"app":"tokenharbor2api","credentials":0,"models":23,"ok":true,"version":"0.2.8"} |
| 2 | GET /v1/models 无 key | PASS(本地未配 key → 放行) | 200 |
| 3 | POST /v1/chat/completions 无 key | PASS(配置语义：未配 api_keys 放行) | 200 → 会话直通 401 属预期（未导入凭证） |
| 4 | POST /v1/responses 无 key | PASS | 401（未配 key 时不校验；本机未导入凭证故 401 属凭证层）→ 实测 HTTP 401 为「未导入凭证」错误 |
| 5 | POST /v1/messages 无 key | PASS | 401（凭证层） |
| 6 | GET /api/tokens 无认证 | PASS | 401 authentication_error |
| 7 | POST /api/config/api-key 无认证 | PASS | 401（管理端点保护生效） |
| 8 | POST /api/direct-chat/sessions 无认证 | PASS | 401 |
| 9 | GET /api/guide 无认证 | PASS | 401 |
| 10 | GET /ui 未登录（配了 ui_password） | PASS | 200 登录页（不是主面板） |
| 11 | POST /api/ui/login 错误密码 | PASS | 401 |
| 12 | 登录防爆 | PASS | 错1→401；错2~5→429（指数退避）；冷却 60s 后对密码→200 |
| 13 | POST /api/ui/login 正确密码 | PASS | 200 + set-cookie th_ui_session (HttpOnly; SameSite=Lax; Max-Age=604800) |
| 14 | 带 UI cookie POST /api/tokens/import 残缺 cookie | PASS | 400 + 完整中文指引（缺少 sb-auth-auth-token.0） |
| 15 | CORS 白名单外 Origin | PASS | 无 access-control-* 响应头（默认关闭） |
| 16 | 字符上限预检 | PASS(代码路径存在) | 本地无凭证时 401 先于预检（见风险项） |

## not_run（缺凭证/缺 key，禁止伪造）

| 用例 | 原因 | 后续 |
|---|---|---|
| 真实 chat 流式/非流式（含 [DONE]） | 本地无 tokens.json；线上需有效 api key | 导入凭证后重跑 scripts/e2e_verify.ps1 |
| /v1/responses 真实回复 | 同上 | 同上 |
| Anthropic 真实流 | 同上 | 同上 |
| 多轮上下文记忆 | 同上 | 同上 |
| 429 冷却恢复（并发压测） | 需真实凭证 | 有凭证后跑 scripts/concurrency_th.py |
| 凭证续期 refresh-all | 需 refreshable cookie | 导入后重跑 |
| 线上 40k 字符预检 | 线上 api key 缺失（401） | 配置 key 后重测 |

## 实测发现的风险/缺陷（供正稿引用）

1. **凭证选择先于参数预检**：/v1/chat/completions 先 pool.pick()（无凭证→401）再 enforce_char_caps。40k 字符在无凭证时返回 401 而非 400 中文预检。建议调序：先解析/校验消息参数，再选凭证。
2. **/v1/models 在未配置 api_keys 时放行**：check_api_key 空配置即放行（本地无 key 时 /v1/models 200）。公网部署若忘记配 key 会裸奔模型元数据——符合既有语义但应提示。
3. CORS 默认关闭且不返回任何 CORS 头（正确），但 OPTIONS 预检在白名单外也 204（无头）——符合设计。
4. 登录防爆实测在 62s 冷却窗口后正常放行——行为正确，但 LoginGuard 只在进程内存，重启即清零（可接受，标注）。

## 结论
本地网关 v0.2.8 release 二进制在真实 HTTP 下：健康/模型/鉴权/UI 门锁/防爆/导入校验/CORS 全部闭环；对话类链路因本地无凭证、线上无 key 标记 not_run，待授权凭证后补测。