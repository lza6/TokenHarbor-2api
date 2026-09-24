# Changelog

## v0.2.5 (2026-09-24)

### 新增 — Web 面板门锁（防公网裸奔）

- `config.json` 新增 `ui_password`（空 = UI 不锁；设置后 `/` 与 `/ui` 需登录）
- `POST /api/ui/login`：常数时间密码校验 → 签发 HMAC-SHA256 签名 session cookie
- session cookie：`th_ui_session`，HttpOnly + SameSite=Lax + 7 天有效期
- 未登录访问 `/ui` → 返回暗色登录页（内嵌密码表单，回车提交）
- 新增 `src/ui_auth.rs`（issue_token / check_session / LOGIN_HTML）

### 真实 E2E 验证（2026-09-24，本地 47830）

- 未登录访问 /ui → 200 登录页（含密码框）
- 错误密码 → 401
- 正确密码 → 200 + set-cookie th_ui_session
- 带 cookie 访问 /ui → 200 主面板
- 22/22 测试通过，clippy -D warnings 全绿

## v0.2.4 (2026-09-24)

### 新增 — 凭证导入多格式自动识别 + 续期诊断

- 新增 `src/import_parse.rs`：粘贴即自动识别 7 种格式
  - 裸 Cookie 头 / `Cookie:` 前缀
  - curl `-b` / `curl -H 'Cookie: ...'`（含 Windows `^"` cmd 转义）
  - HAR 文件（entries[].request.cookies）
  - Netscape cookie jar（`#HttpOnly_` 域名行）
  - Chromium / Firefox JSON 导出（[{name,value,domain}]）
  - 无法识别 → 400 + 明确提示
- 导入响应新增诊断字段：
  - `refreshable`：cookie 是否含 refresh_token（能否自动续期）
  - `expires_at`：access_token 过期时刻
  - `hint`：续期能力明确提示（可续 / 需重登）
- `GET /api/tokens` 列表新增 `refreshable` + `expires_at`（UI 徽章数据源）
- Web UI：导入面板说明多格式 + 凭证列表"续期"徽章（可续期绿 / 需重登灰）+ 导入 toast 显示 hint

### 真实 E2E 验证（2026-09-24）

- 6 种格式（裸 / 前缀 / curl -b / curl -H / HAR / jar / JSON）全部 200 且自动识别
- 7 个不同值格式 → 7 个独立凭证（去重不误伤），垃圾/空输入 → 400 + 提示
- 真实 cookie HAR 导入 → `refreshable: true` + `expires_at` + 正确 hint
- refresh-all 触发 → `refresh_token_already_used` 正确诊断（refresh_token 一次性）
- 测试 22/22 通过（新增 8 个 import_parse 单测），clippy -D warnings 全绿

## v0.2.3 (2026-09-24)

### 新增 — /v1/responses 原生格式 + 多账号加权轮询

- `POST /v1/responses`：OpenAI Responses API 原生格式（非流式完整 response 对象 + 流式 SSE）
  - 流式事件链：`response.created` → `response.reasoning_summary_text.delta`（思考流）→ `response.output_text.delta`（逐字正文）→ `response.output_text.done` → `response.completed`（携带完整文本）
  - input 支持字符串 / 消息数组；支持 `input_image` 图片附件（data URL）→ 上游 attachments
  - 复用会话绑定 / 信号量 / 429 自动换会话 / 凭证轮换全部网关能力
- 多账号负载均衡：`pick()` 升级为加权轮询
  - 健康分优先；同健康分选 `last_used` 最旧（避免永远选第一个 → 多账号交替使用）
  - 跳过冷却中 / 健康分 < 0.3 的凭证

### 真实 E2E 验证（2026-09-24，浏览器真实 cookie）

- `/v1/responses` 非流式：200，完整 response 对象，模型真实回答
- `/v1/responses` 流式：27 事件，created/reasoning/delta/done/completed 全链
- completed 事件携带完整 UTF-8 文本（你好世界）
- `/v1/chat/completions`：200，真实回答
- `/v1/messages`（Anthropic）：200 `type=message`
- 多轮会话：同 user 上下文延续（记住名字→正确回答）
- 15 并发：15/15 成功，信号量排队正确
- 20 快速并发：15 成功 + 5 透传 429 + 日志确认自动换会话重试 6 次

## v0.2.2 (2026-09-24)
## v0.2.2 (2026-09-24)

### 新增 — Pipeline DevOps complète

- ci.yml : 6 jobs (Build Windows / Qualité fmt+clippy / Tests coverage≥20% / Sécurité audit+gitleaks+SAST / Release on tag / Notifications Slack)
- deploy.yml : staging auto (develop) + production approbation manuelle (main/tag, GitHub Environments) + rollback (workflow_dispatch)
- docker.yml : GHCR multi-arch (amd64+arm64) sur main/develop/tag
- .gitleaks.toml : exclusions scan secrets
- docs/DEVOPS.md : schéma ASCII, stratégie branches, secrets, troubleshooting

### Vérification locale (2026-09-24)

- cargo fmt --check : passe
- cargo clippy --all-targets -- -D warnings : passe (zéro lint)
- cargo test --lib --tests : 14/14 OK
- cargo llvm-cov : couverture 20.40% (models 81%, errors 66%, web_pool 53%)
- cargo audit : 243 crates, 0 vulnérabilité

### Note CI

- Les workflows sont poussés et déclenchés ; l''exécution runner est bloquée par la limite de facturation GitHub du compte (billing), pas par la configuration.

## v0.2.1 (2026-09-24)

### Production hardening

- 429 rate-limit auto-switch session + transmission 429 correcte
- Refresh failure ne refroidit plus les credentials par erreur
- Sémaphore de concurrence + mutex de création de session
- Limite 200 sessions + nettoyage automatique 24h
- Fenêtres de contexte réelles (DeepSeek 1M/384K, Qwen 256K...)

## v0.2.0 (2026-09-24)

### 新增

- CI/CD：GitHub Actions（build-release.yml：Windows 构建 + 测试 + clippy + Release 门禁；docker.yml：GHCR 镜像）
- Docker 部署：docker/Dockerfile（多阶段构建 + HEALTHCHECK）+ .dockerignore
- 自动续期闭环验证：续期后移除 tok0+tok1 只保留 th_*+新 tok0，真实 E2E 续期后对话正常
- 上传链路验证：/api/direct-chat/upload 预签名真实 E2E 通过
- scripts/e2e_verify.ps1 一键真实 E2E 验证脚本

### 修复（clippy 门禁）

- needless-return / map-identity / manual-strip / doc-lazy-continuation / len-without-is-empty

## v0.1.0 (2026-09-24)

首个可运行版本。上游 TokenHarbor 协议逆向自抓包数据包 + 站点 JS chunk + /models RSC 实时快照。

## v0.1.0 (2026-09-24)

首个可运行版本。上游 TokenHarbor 协议逆向自抓包数据包 + 站点 JS chunk + /models RSC 实时快照。

### 新增

- OpenAI 兼容 `/v1/chat/completions`（流式/非流式）与 Anthropic 兼容 `/v1/messages`
- `/v1/models` 模型列表：23 个模型（18 付费 + 4 免费 + 1 兜底），含免费窗口感知
- 上游 SSE 双向转换：thinking/chunk/citation/tool_use/image/file/done/error
- 凭证池：Cookie 导入/去重/健康分/401 冷却/轮询换号
- 会话复用：每线程绑定上游 session，多轮上下文连续
- 内置控制面板 `/ui`：总览/凭证/模型/接入指南
- 凭证检查 `/api/tokens/check`、上游直通 `/api/me/free-tier`、`/api/me/chat-quotas`
- 上传预签名 `/api/direct-chat/upload`、语音转写 `/api/direct-chat/transcribe`

### 修复

- normalize 裸 surface 优先免费路线（qwen3.8-flash → alibaba/qwen3.8-flash:free）
- 凭证池写锁自死锁（add_raw/delete 内 save 二次读锁）→ 先释放锁再保存
- 上游错误消息透传 HTTP 状态码
