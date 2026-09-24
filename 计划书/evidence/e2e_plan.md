# 真实 E2E 验收计划（本地 127.0.0.1:47830）

目标：在真实本地运行二进制，用真实 HTTP 验证网关所有主链路，输出可审计结果。
前置：本地已编译 release 二进制（target/release/tokenharbor2api.exe），有可用上游 Cookie（data/tokens.json 或 config auth_tokens）。

## 前提检查
- 无有效凭证：记录「凭证缺失」并给出导入指引，标注该链路 not_run（禁止伪造成功）
- 有有效凭证：执行下述全部

## 用例矩阵

| # | 用例 | 命令/请求 | 验收 |
|---|---|---|---|
| 1 | healthz | GET /healthz | 200 + ok:true |
| 2 | /v1/models | GET /v1/models | 200，>=22 模型，含 th-rudder:free |
| 3 | OpenAI 非流式 | POST /v1/chat/completions {"messages":[...]} | 200，choices[0].message.content 非空 |
| 4 | OpenAI 流式 | POST stream:true | 2xx，body 含 data: 与 [DONE] |
| 5 | Anthropic 流式 | POST /v1/messages stream:true | 2xx，body 含 message_start 与 content_block_delta |
| 6 | /v1/responses | POST /v1/responses | 2xx，object=response |
| 7 | 多轮上下文 | 同 user 两次对话 | 第二次能复述第一次信息 |
| 8 | 429 冷却与恢复 | 并发 20（脚本 scripts/concurrency_th.py 本地） | 混合 200/429，Retry-After；冷却后恢复 200 |
| 9 | UI 登录 | POST /api/ui/login 错/对密码 | 错→401；对→200+set-cookie |
| 10 | 管理鉴权 | 无 key GET /api/tokens | 401 |
| 11 | 带 key GET /api/tokens | 200 + masked cookie |
| 12 | 凭证导入校验 | POST /api/tokens/import 残缺 cookie | 400 + 指引 |
| 13 | 凭证续期 | POST /api/tokens/refresh-all | 200 refreshed>=0（无凭证时 not_run） |
| 14 | 字符上限预检 | 单条 40k 字符 | 400「超过…32000」 |
| 15 | CORS | 带 Origin 白名单外 | 无 CORS 头 |
| 16 | 登录防爆 | 8 连发错误密码 | 第 2 次起 429 + Retry-After |

## 输出
- 结果逐条记录到 计划书/evidence/e2e_local_report.md（PASS/FAIL/not_run + 证据）
- 失败项：只允许按正稿「小改 bug 清单」修（严禁重构），修完重跑到绿