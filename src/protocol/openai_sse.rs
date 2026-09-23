//! OpenAI SSE 流：TokenHarbor SSE → OpenAI chat.completions SSE

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

pub struct SseResponse {
    pub body: Body,
}

impl IntoResponse for SseResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(self.body)
            .unwrap()
    }
}

pub fn openai_events(
    upstream: reqwest::Response,
    model: &str,
    created: i64,
    _session_id: &str,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(upstream.bytes_stream()));
    OpenAiTransform {
        reader: Box::pin(reader),
        model: model.to_string(),
        created,
        finished: false,
        done_sent: false,
        saw_content: false,
    }
}

struct OpenAiTransform {
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: String,
    created: i64,
    finished: bool,
    done_sent: bool,
    saw_content: bool,
}

impl Stream for OpenAiTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished && !self.done_sent {
            self.done_sent = true;
            return Poll::Ready(Some(Ok(openai_done(&self.model, self.created))));
        }
        if self.finished {
            return Poll::Ready(None);
        }
        loop {
            let mut line = String::new();
            let reader = &mut self.reader;
            let fut = reader.read_line(&mut line);
            tokio::pin!(fut);
            match fut.poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(0)) => {
                    self.finished = true;
                    if !self.saw_content {
                        let frame = openai_empty_error(&self.model, self.created);
                        self.done_sent = true;
                        return Poll::Ready(Some(Ok(frame)));
                    }
                    self.done_sent = true;
                    return Poll::Ready(Some(Ok(openai_done(&self.model, self.created))));
                }
                Poll::Ready(Ok(_)) => {
                    let line = line.trim_end_matches('\n').trim_end_matches('\r');
                    if line.is_empty() { continue; }
                    if let Some(data) = line.strip_prefix("data: ") {
                        match parse_sse_data(data) {
                            Some(evt) => {
                                match evt.event.as_str() {
                                    "thinking" => {
                                        let delta = evt.json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                        let frame = format!("data: {}\n\n", serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": { "role": "assistant", "reasoning_content": delta },
                                                "finish_reason": null
                                            }]
                                        }));
                                        return Poll::Ready(Some(Ok(frame)));
                                    }
                                    "chunk" => {
                                        let delta = evt.json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                        if !delta.is_empty() { self.saw_content = true; }
                                        let frame = format!("data: {}\n\n", serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": { "content": delta },
                                                "finish_reason": null
                                            }]
                                        }));
                                        return Poll::Ready(Some(Ok(frame)));
                                    }
                                    "citation" => {
                                        let url = evt.json.get("url").and_then(|v| v.as_str()).unwrap_or("");
                                        if !url.is_empty() {
                                            let frame = format!("data: {}\n\n", serde_json::json!({
                                                "id": format!("chatcmpl-{}", self.created),
                                                "object": "chat.completion.chunk",
                                                "created": self.created,
                                                "model": self.model,
                                                "choices": [{
                                                    "index": 0,
                                                    "delta": { "annotations": [{ "type": "url_citation", "url": url, "title": evt.json.get("title") }] },
                                                    "finish_reason": null
                                                }]
                                            }));
                                            return Poll::Ready(Some(Ok(frame)));
                                        }
                                        continue;
                                    }
                                    "tool_use" => {
                                        let name = evt.json.get("name").and_then(|v| v.as_str()).unwrap_or("web_search");
                                        let args = evt.json.get("arguments").cloned().unwrap_or(serde_json::json!({}));
                                        let idx = evt.json.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                        let frame = format!("data: {}\n\n", serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": { "tool_calls": [{ "index": idx, "id": format!("call_{}_{}", self.created, idx), "type": "function", "function": { "name": name, "arguments": serde_json::to_string(&args).unwrap_or_default() } }] },
                                                "finish_reason": null
                                            }]
                                        }));
                                        return Poll::Ready(Some(Ok(frame)));
                                    }
                                    "image_start" | "image_partial" | "image" | "file_start" | "file" | "file_failed" => {
                                        continue;
                                    }
                                    "error" => {
                                        let msg = evt.json.get("message").and_then(|v| v.as_str()).unwrap_or("上游流错误");
                                        self.finished = true;
                                        let frame = format!("data: {}\n\n", serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": { "content": format!("\n\n[上游错误: {msg}]") },
                                                "finish_reason": "stop"
                                            }]
                                        }));
                                        let done = openai_done(&self.model, self.created);
                                        self.done_sent = true;
                                        return Poll::Ready(Some(Ok(format!("{frame}{done}"))));
                                    }
                                    "done" => {
                                        self.finished = true;
                                        self.done_sent = true;
                                        return Poll::Ready(Some(Ok(openai_done(&self.model, self.created))));
                                    }
                                    _ => continue,
                                }
                            }
                            None => continue,
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.finished = true;
                    self.done_sent = true;
                    return Poll::Ready(Some(Ok(openai_done(&self.model, self.created))));
                }
            }
        }
    }
}

fn parse_sse_data(data: &str) -> Option<Event> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    Some(Event {
        event: json.get("event").and_then(|v| v.as_str()).unwrap_or("message").to_string(),
        json,
    })
}

struct Event {
    event: String,
    json: serde_json::Value,
}

pub fn openai_done(model: &str, created: i64) -> String {
    format!("data: {}\n\ndata: [DONE]\n\n", serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        "usage": null
    }))
}

pub fn openai_empty_error(model: &str, created: i64) -> String {
    format!("data: {}\n\ndata: [DONE]\n\n", serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": { "content": "\n\n[上游未返回内容]" }, "finish_reason": "stop" }]
    }))
}

pub fn openai_nonstream(content: &str, model: &str, created: i64, input_tokens: u64, output_tokens: u64) -> String {
    serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": input_tokens, "completion_tokens": output_tokens, "total_tokens": input_tokens + output_tokens }
    }).to_string()
}


