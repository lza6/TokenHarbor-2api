# TokenHarbor 上游协议逆向笔记

> 数据来源：抓包 `数据包.txt`（HAR）+ 站点 JS chunk（`0snqa017fr~7m.js` 等）+ `/models` RSC 实时快照。
> 更新：2026-09-23。

## 1. 认证

- Cookie：`sb-auth-auth-token.0` / `sb-auth-auth-token.1`（Supabase JWT，值形如 `base64-...`，内含 access_token + refresh_token + user 对象）、`th_sid`（会话 id）、`th_attr*`（归因）。
- 请求头：`origin: https://tokenharbor.ai`、`referer: https://tokenharbor.ai/chat`、`user-agent: Chrome 151`。
- 免费模型需先同意 `/api/me/privacy` 的 free_models_enabled（默认开）。

## 2. 对话流

```
POST /api/direct-chat/stream
Content-Type: application/json
{
  "sessionId": "8cbe7f89-4ace-4bab-b2a0-2ae9f0963231",
  "content": "你好",
  "model": "vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free",
  "webSearch": "auto",
  "tz": "Asia/Shanghai"
}
```

响应：`text/event-stream; charset=utf-8`。事件：

| 事件 | data 字段 | 含义 |
|------|-----------|------|
| `thinking` | `{delta}` | 思考片段（reasoning） |
| `chunk` | `{delta}` | 正文增量 |
| `tool_use` | `{name?, arguments?, index?}` | 工具调用（web_search 等） |
| `citation` | `{url, title}` | 联网搜索引用 |
| `image_start` | `{width,height,steps}` | 图片生成开始 |
| `image_partial` | `{data_url,index}` | 图片生成中间态 |
| `image` | `{url,mime}` | 图片完成 |
| `file_start` | `{format}` | 文件生成开始 |
| `file` | `{storage_path,mime,name,bytes}` | 文件完成 |
| `file_failed` | `{reason,limit?}` | 文件生成失败 |
| `done` | `{messageId,model,searchCount,usage:{inputTokens,outputTokens},billing}` | 结束 |
| `error` | `{message}` | 错误 |

请求体可选字段：

- `attachments`: `[{kind, mime, name, bytes, data_url? | storage_path?}]`
- `useKb`: `false`（关闭知识库）
- `rewindTo`: `{messageId, mode:"edit"|"regenerate"}`（编辑/重生成）

## 3. 会话

```
POST /api/direct-chat/sessions        body {model, temporary} -> {session:{id,...}}
GET  /api/direct-chat/sessions/{id}   -> {session, messages[]}
PATCH /api/direct-chat/sessions/{id}  body {temporary:false} 等
DELETE /api/direct-chat/sessions/{id}[?stale=1]
POST /api/direct-chat/sessions/{id}/share       创建分享链接
DELETE /api/direct-chat/sessions/{id}/share     撤销
PATCH /api/direct-chat/messages/{id}/vote       body {vote:1|-1}
```

会话上限 200（`too_many_sessions` 409），temporary 会话可 PATCH 落库。

## 4. 上传

```
POST /api/direct-chat/upload   body {kind,name,mime,bytes} -> {ok,path,token}
PUT https://auth.tokenharbor.ai/storage/v1/object/upload/sign/chat-uploads/{path}?token={token}
```

- 小图（≤1MB）：浏览器直接 base64 进 `attachments[].data_url`，不走上传。
- 限制：图片/音频 10MB，视频 100MB，文档(PDF/txt/md/csv) 4MB，每条消息最多 4 个附件。

## 5. 语音

```
POST /api/direct-chat/transcribe  FormData: audio=<clip>, mime=<type>
-> {ok, text}
```

## 6. 额度

- `GET /api/me/free-tier` → `{ok, activated, model_labels, used_pct, exhausted, plan?, reset_at}`
- `GET /api/me/chat-quotas` → `{ok, voice:{used,limit}, images:{used,limit}, searches?, files?, text:{unlimited}}`
- `POST /api/me/privacy` → `{free_models_enabled:true}`
- 匿名端 `GET /api/public/tokens-served` → `{freeTokens,totalTokens,freeRequests,asOf}`

## 7. 模型 id 规则

- surface（页面展示）：`qwen3.8-flash`、`deepseek-v4.1-flash`、`claude-opus-5.5`…
- 付费完整 id：`{provider}/{surface}`（如 `alibaba/qwen3.8-flash`、`vercel-ai-gateway/deepseek/deepseek-v4.1-flash`）
- 免费完整 id：`{provider}/{surface}:free`
- 兜底：`th-rudder:free`（站点 DEFAULT_CHAT_MODEL）

provider 映射（familyFromModelId）：anthropic→anthropic, openai→openai, google→google, deepseek→deepseek, qwen→alibaba, z-ai→z-ai, meta-llama→meta, x-ai→xai, moonshotai→kimi, xiaomi→xiaomi, vercel-ai-gateway→vercel-ai-gateway（deepseek 子路径 `vercel-ai-gateway/deepseek/`）。

免费模型（2026-09-23 快照）：

| surface | 免费 id | 免费窗口 |
|---------|---------|----------|
| mimo-v2.6-flash | xiaomi/mimo-v2.6-flash:free | 常驻 |
| qwen3.8-flash | alibaba/qwen3.8-flash:free | 2026-09-27T13:00:00Z |
| deepseek-v4.1-flash | vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free | 常驻 |
| th-rudder | th-rudder:free | 常驻 |

## 8. 模型能力字段

`/models` RSC rows 字段：`surface, label, family, tier(frontier|value), aaRank, intelligenceIndex, priceIn, priceOut, listIn, listOut, isFree, promo, limited, livePrice, inputModalities[], outputModalities[], speedBuild{same fields}, freeRows[], availableAt, freeUntil, serverNow`。
