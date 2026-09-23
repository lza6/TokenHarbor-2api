# 架构说明

## 模块

```
src/
├── main.rs          # 入口：加载配置、构建客户端池、启动 axum
├── lib.rs           # 模块声明
├── api.rs           # axum 路由 + OpenAI/Anthropic 桥接 + 凭证管理
├── models.rs        # 模型目录（23 个）+ 归一化 + 免费窗口感知
├── upstream.rs      # TokenHarbor HTTP 客户端（会话/流/上传/转写/额度）
├── web_pool.rs      # Cookie 凭证池（健康分/冷却/轮询）
├── session.rs       # 会话映射（下游线程 ↔ 上游 session）
├── errors.rs        # OpenAI/Anthropic 兼容错误
├── web.rs           # 内置控制面板（单 HTML）
└── protocol/
    ├── openai_sse.rs    # 上游 SSE → OpenAI SSE
    ├── anthropic_sse.rs # 上游 SSE → Anthropic SSE
    └── stream.rs        # reqwest bytes → tokio AsyncRead 适配
tests/
├── models_test.rs       # 归一化/免费窗口/目录
└── pool_errors_test.rs  # 凭证池/错误形状/会话映射
```

## 请求路径

```
客户端 → /v1/chat/completions
  → check_api_key（api_keys 未配置则放行）
  → registry.resolve(model)（归一化 + 免费窗口降级）
  → pool.pick()（选健康凭证）
  → sessions.ensure()（创建/复用上游 session）
  → upstream.stream()（POST /api/direct-chat/stream）
  → openai_sse 转换 → SSE 回客户端
```

## 凭证池

- `add_raw`：Cookie 值去重入库（先读后写，避免写锁内二次读锁死锁）
- `pick`：健康分最高 + 不在冷却期
- `record_failure`：401/403 冷却 600s×failures（封顶 6 次）
- `record_success`：健康分回升

## 会话映射

- key：OpenAI `user` 字段或 Anthropic `metadata.thread_id`
- 同模型复用上游 session（多轮上下文连续）
- 换模型自动重建上游 session
- 超时（未实现清理任务）可后续补

## SSE 转换

上游 `thinking/chunk/citation/tool_use/done` → 目标协议对应事件。图片/文件生成事件透传为附注。

## 测试

`cargo test`：14 个测试（7 模型 + 7 池/错误/会话）。覆盖归一化、免费窗口、凭证去重/冷却、错误形状、会话复用。
