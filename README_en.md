# TokenHarbor2API

Reverse-engineered **OpenAI/Anthropic-compatible API gateway** for [TokenHarbor](https://tokenharbor.ai) free-tier models. Single Rust binary, zero external deps.

## Quick Start

```bash
cargo build --release
./target/release/tokenharbor2api --config config.json
# default: http://127.0.0.1:47830
```

Import your TokenHarbor cookie (`sb-auth-auth-token.0/1`, `th_sid`) via the panel `/ui` or:

```bash
curl -X POST http://127.0.0.1:47830/api/tokens/import \
  -H "Content-Type: application/json" \
  -d '{"cookie":"sb-auth-auth-token.0=...; th_sid=..."}'
```

## Clients

**Claude Code**: `ANTHROPIC_BASE_URL=http://127.0.0.1:47830`, key any.
**OpenAI-compatible** (Cursor/LobeChat/SDK): `Base URL http://127.0.0.1:47830/v1`.

## Models

23 built-in models: 18 paid + 4 free (`th-rudder:free`, `alibaba/qwen3.8-flash:free`, `vercel-ai-gateway/deepseek/deepseek-v4.1-flash:free`, `xiaomi/mimo-v2.6-flash:free`). Full list at `/v1/models`.

## Endpoints

`/v1/chat/completions`, `/v1/messages`, `/v1/models`, `/v1/uploads`, `/healthz`, `/api/tokens*`, `/api/guide`, `/api/config/api-key`, `/ui`.

## Features

Tool calls, web search (citation), knowledge base, image/video/audio upload, voice transcription, free-window awareness with fallback, credential pool with health/cooldown, session reuse, streaming SSE for both OpenAI & Anthropic.

## License

MIT. Not affiliated with TokenHarbor.
