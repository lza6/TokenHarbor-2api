# Changelog

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
