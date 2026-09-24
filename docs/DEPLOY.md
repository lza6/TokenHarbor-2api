# TokenHarbor2API 生产部署指南

## 当前生产实例（Azure）

| 项目 | 值 |
|---|---|
| 服务器 | 52.141.3.10 (Ubuntu 22.04 x86_64) |
| 服务 | systemd: `tokenharbor2api.service`（开机自启 + 崩溃自动重启） |
| 监听 | 0.0.0.0:47830 |
| 版本 | v0.2.6 |

## Web UI 控制台

```
地址: http://52.141.3.10:47830/ui
密码: 见服务器 /opt/tokenharbor/app/config.json 的 ui_password
```

登录后可在"凭证"页一键粘贴导入 Cookie / curl / HAR，并查看每个凭证的续期状态（可续期/需重登）。

## API 接入

```
Base URL:  http://52.141.3.10:47830/v1
API Key:   见服务器 /opt/tokenharbor/app/config.json 的 api_keys
```

| 端点 | 说明 |
|---|---|
| `GET  /v1/models` | 模型列表（23 个，含免费/付费） |
| `POST /v1/chat/completions` | OpenAI 兼容聊天（流式/非流式） |
| `POST /v1/responses` | OpenAI Responses 原生格式 |
| `POST /v1/messages` | Anthropic 兼容 |
| `POST /api/tokens/import` | 导入凭证（裸 Cookie/curl/HAR/jar/JSON，自动识别+强校验） |
| `POST /api/tokens/login` | 邮箱密码登录（Supabase password grant，自动入库+续期） |
| `POST /api/tokens/refresh-all` | 手动触发全部凭证续期 |

### 请求示例

```bash
curl http://52.141.3.10:47830/v1/chat/completions \
  -H "Authorization: Bearer <API_KEY>" \
  -H "Content-Type: application/json" \
  -d '{"model":"alibaba/qwen3.8-flash:free","messages":[{"role":"user","content":"你好"}]}'
```

## 安全

- Web UI 有密码锁（`ui_password`），未登录只显示登录页
- API 必须带 `Authorization: Bearer <key>` 或 `x-api-key`（无 key 返回 401）
- 管理端点双认证：API Key 或 Web UI session 均可
- Cookie 凭证以掩码形式展示，日志脱敏（`redact_logs: true`）

## 自动续期原理（重要）

TokenHarbor 使用 Supabase 认证：
- cookie `sb-auth-auth-token.0` 内含 `refresh_token`（1 小时 access_token + 一次性 refresh_token）
- 网关启动时 + 每 50 分钟自动调 Supabase `/auth/v1/token?grant_type=refresh_token` 换新 token 并写回 cookie
- **导入后请关闭 tokenharbor.ai 浏览器标签**：浏览器前端也会自动刷新 token，会和网关竞争消费 refresh_token，导致网关刷新报 `refresh_token_already_used`
- 若 refresh_token 已失效（被用过），access_token 到期后需重新登录导入新 cookie

## 运维命令

```bash
# 状态
systemctl status tokenharbor2api
# 日志
journalctl -u tokenharbor2api -f
# 重启
systemctl restart tokenharbor2api
# 更新（替换二进制后）
cp /opt/tokenharbor/app/tokenharbor2api.new /opt/tokenharbor/app/tokenharbor2api
systemctl restart tokenharbor2api
```

## 更新部署

1. 本地 `git archive --format=tar.gz -o ../tokenharbor-src.tar.gz HEAD`
2. SFTP 上传到服务器 `/opt/tokenharbor/src026.tar.gz`
3. 服务器解压 + `cargo build --release`（需 rust 工具链 + 4G swap）
4. 替换二进制 + `systemctl restart tokenharbor2api`
