# Changelog

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
