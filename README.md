# TokenHarbor2API

> Rust(axum) 版。English: [README_en.md](README_en.md) ｜ 上游协议逆向笔记: [docs/PROTOCOL.md](docs/PROTOCOL.md)

TokenHarbor2API 把 [TokenHarbor](https://tokenharbor.ai) 免费层的模型逆向为 **OpenAI 兼容**与 **Anthropic 兼容** 的本地 API 网关。单二进制、零外部依赖，可在任意 OpenAI/Claude 客户端（Claude Code、Codex、Cursor、LobeChat、NextChat 等）中使用 TokenHarbor 免费模型。

---

## 一、快速开始（3 步）

### 第 1 步：编译

```bash
# Windows
build.bat

# 或手动
cargo build --release
./target/release/tokenharbor2api --config config.json
```

默认监听 `http://127.0.0.1:47830`。

### 第 2 步：登录账号（三选一）

**方式 A：邮箱密码登录（推荐，自动入库）**

```bash
curl -X POST http://127.0.0.1:47830/api/tokens/login \
  -H "Content-Type: application/json" \
  -d '{"email":"you@example.com","password":"你的密码"}'
```

网关直接调用 TokenHarbor 的 Supabase 认证接口（`auth.tokenharbor.ai/auth/v1/token?grant_type=password`）登录并入库。

**方式 B：浏览器登录 + 导入 Cookie**

1. 浏览器打开 tokenharbor.ai → 登录（Google / GitHub / 邮箱）
2. F12 → Network → 任意请求 → 复制完整 `cookie:` 整行（含 `sb-auth-auth-token.0` / `sb-auth-auth-token.1` / `th_sid`）
3. 调 `/api/tokens/import` 粘贴导入

**方式 C：启动自动续期（已验证闭环）**

网关启动时 + 每 50 分钟自动用 refresh_token 换新 access_token（`POST /api/tokens/refresh-all` 可手动触发）。续期后仅保留 `token.0`（移除 `token.1`，上游实测带 token.1 的续期 cookie 会 401），**真实 E2E 已验证续期后对话正常**，无需重复登录。

### 第 3 步：接入客户端

**Claude Code**（Anthropic 协议）

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:47830
export ANTHROPIC_API_KEY=sk-local   # 未配置 api_keys 时随意填；面板可一键生成真 Key
```

**Cursor / Continue / 任意 OpenAI 兼容客户端**

```
Base URL: http://127.0.0.1:47830/v1
API Key:  sk-local（或面板生成）
模型:     从 /v1/models 里选
```

**OpenAI SDK（Python）**

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:47830/v1", api_key="sk-local")
resp = client.chat.completions.create(
    model="alibaba/qwen3.8-flash:free",
    messages=[{"role": "user", "content": "你好"}],
)
print(resp.choices[0].message.content)
```

---

## 二、API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 聊天（流式 / 非流式） |
| `/v1/messages` | POST | Claude 聊天（双向转换，流式为 Anthropic 事件流） |
| `/v1/models` | GET | 可用模型列表 |
| `/v1/uploads` | POST | 上传文件换取 storagePath（裸 body + `x-file-name` 头） |
| `/healthz` | GET | 健康检查 |
| `/api/tokens/import` | POST | 导入 Cookie（`{"cookie": "..."}`），同值自动去重 |
| `/api/tokens/login` | POST | 邮箱密码登录（`{"email","password"}`），自动入库 |
| `/api/tokens/refresh-all` | POST | 手动触发所有凭证续期 |
| `/api/tokens` | GET | 已导入凭证（掩码 / 健康分 / 冷却状态） |
| `/api/tokens/check` | POST | 凭证有效性检查（拉取 /api/me/free-tier） |
| `/api/tokens/delete` | POST | 删除凭证 `{id}` |
| `/api/guide` | GET | 接入信息（地址 / Key 状态 / 模型数） |
| `/api/config/api-key` | POST | 运行时生成 / 设置 / 清除下游 API Key |
| `/api/me/free-tier` | GET | 免费额度（上游直通） |
| `/api/me/chat-quotas` | GET | 每日限额（上游直通） |
| `/api/direct-chat/sessions` | POST | 创建上游会话（直通） |
| `/api/direct-chat/upload` | POST | 上游上传预签名（直通） |
| `/ui` | GET | 内置控制面板 |

---

## 三、模型目录

网关内置 **23 个模型**（18 付费 + 4 免费 + 1 兜底）。`/v1/models` 返回完整列表。

### 免费模型（0 美元）

| 模型 ID | 名称 | 家族 | 输入模态 | 免费窗口 |
|---------|------|------|----------|----------|
| `th-rudder:free` | TH-Rudder | tokenharbor | text, image | 常驻（默认） |
| `alibaba/qwen3.8-flash:free` | Qwen3.8 Flash | qwen | text, image, video | 至 2026-09-27 |
| `vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free` | DeepSeek V4.1 Flash | deepseek | text, image | 常驻 |
| `xiaomi/mimo-v2.6-flash:free` | MiMo V2.6 Flash | xiaomi | text, image, audio, video | 常驻 |

### 付费模型（按量计费，需 TokenHarbor 余额）

| surface | 家族 | 档位 | $ in / 1M | $ out / 1M |
|---------|------|------|-----------|-----------|
| `claude-opus-5.5` | anthropic | frontier | 4.00 | 20.00 |
| `claude-fable-5.1` | anthropic | frontier | 10.00 | 50.00 |
| `gpt-6-astra` | openai | frontier | 10.00 | 50.00 |
| `gpt-6-sol` | openai | frontier | 2.00 | 10.00 |
| `gpt-5.6-terra` | openai | frontier | 2.00 | 12.00 |
| `gpt-6-luna` | openai | value | 0.10 | 0.50 |
| `muse-spark-1-3` | vercel-ai-gateway | frontier | 1.25 | 4.25 |
| `grok-4.7` | x-ai | frontier | 2.00 | 6.00 |
| `qwen3.8-max` | alibaba | frontier | 2.00 | 6.00 |
| `qwen3.8-27b` | alibaba | value | 0.35 | 2.10 |
| `glm-5.3` | z-ai | frontier | 1.40 | 4.40 |
| `glm-5.3-flash` | z-ai | value | 0.15 | 0.50 |
| `kimi-k3` | moonshotai | frontier | 3.00 | 15.00 |
| `mimo-v2.6-pro` | xiaomi | value | 0.435 | 0.87 |
| `gemini-3.8-flash` | google | value | 0.75 | 3.75 |
| `deepseek-v4.1-flash` | deepseek | value | 0.30 | 1.20 |

> 付费模型 id 形如 `anthropic/claude-opus-5.5`（带 provider 前缀）。免费模型自动优先：请求 `qwen3.8-flash` 会归一化到 `alibaba/qwen3.8-flash:free`。

---

## 四、上游协议（逆向摘要）

TokenHarbor 站点（Next.js + Supabase + Vercel）核心端点：

- `POST /api/direct-chat/stream` — SSE 对话流
  - 请求体：`{sessionId, content, model, attachments?, webSearch, tz, useKb?, rewindTo?}`
  - SSE 事件：`thinking` / `chunk` / `tool_use` / `citation` / `image_start` / `image_partial` / `image` / `file_start` / `file` / `file_failed` / `done` / `error`
- `POST/GET/PATCH/DELETE /api/direct-chat/sessions[/{id}]` — 会话管理
- `POST /api/direct-chat/upload` — 大文件预签名（小图 base64 直传，≤1MB 不进上传接口）
- `POST /api/direct-chat/transcribe` — 语音转文字
- `GET /api/me/free-tier`、`GET /api/me/chat-quotas` — 免费额度 / 每日限额

完整字段说明见 [docs/PROTOCOL.md](docs/PROTOCOL.md)。

网关实现的转换：

- **OpenAI 流**：`thinking` → `delta.reasoning_content`；`chunk` → `delta.content`；`citation` → `annotations.url_citation`；`tool_use` → `delta.tool_calls`；`done` → `finish_reason: stop` + `[DONE]`
- **Anthropic 流**：`thinking` → `content_block_delta thinking_delta`；`chunk` → `content_block_delta text_delta`；`tool_use` → `content_block_start tool_use`

---

## 五、能力说明

| 能力 | 支持 | 说明 |
|------|------|------|
| 工具调用（tools） | ✅ | 上游 tool_use 事件 → OpenAI tool_calls / Anthropic tool_use 块 |
| 联网搜索（webSearch） | ✅ | 请求体 `webSearch: auto/on/off`，citation 事件回传 |
| 知识库（KB） | ✅ | `useKb` 字段（面板「项目文档」上游直通） |
| 参考图 / 多模态 | ✅ | 小图 base64 直传；大图走上传预签名 |
| 语音转写 | ✅ | `/api/direct-chat/transcribe` 直通 |
| 免费窗口感知 | ✅ | qwen3.8 免费至 2026-09-27，过期自动降级 |
| 凭证池 | ✅ | 健康分 / 401 冷却 / 轮询换号 |
| 自动续期 | ✅ | refresh_token 换新 access_token（启动 + 定时 + 手动） |
| 邮箱登录 | ✅ | Supabase password grant 直接登录入库 |
| 会话复用 | ✅ | 每线程绑定上游 session，多轮上下文连续 |

---

## 六、配置（config.json）

```jsonc
{
  "listen_addr": "127.0.0.1:47830",
  "upstream_base_url": "https://tokenharbor.ai",
  "auth_tokens": ["<完整 Cookie>"],
  "api_keys": [],
  "http_proxy": "",
  "default_model": "th-rudder:free",
  "fallback_models": ["alibaba/qwen3.8-flash:free", "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free", "xiaomi/mimo-v2.6-flash:free"],
  "token_saver": false,
  "sqlite_path": "data/tokenharbor2api.sqlite",
  "tokens_path": "data/tokens.json",
  "skip_upstream_check": true,
  "redact_logs": true
}
```

环境变量优先：`LISTEN_ADDR` / `AUTH_TOKENS` / `API_KEYS` / `HTTP_PROXY` / `UPSTREAM_BASE_URL`。

---

## 七、免责声明

本项目与 OpenAI、TokenHarbor 无官方关联，相关商标版权归各自所有者。
所有内容仅供交流、实验与学习使用，按「原样（As-Is）」提供，使用者自行承担风险。

## 八、开源协议

MIT
