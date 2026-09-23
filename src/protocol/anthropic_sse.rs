//! Anthropic SSE：TokenHarbor SSE → Claude messages SSE

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

pub struct AnthropicSseResponse {
    pub body: Body,
}

impl IntoResponse for AnthropicSseResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(self.body)
            .unwrap()
    }
}

pub fn anthropic_events(
    upstream: reqwest::Response,
    model: &str,
    session_id: &str,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(upstream.bytes_stream()));
    AnthropicTransform {
        reader: Box::pin(reader),
        model: model.to_string(),
        session_id: session_id.to_string(),
        started: false,
        finished: false,
        message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
        saw_content: false,
    }
}

struct AnthropicTransform {
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: String,
    session_id: String,
    started: bool,
    finished: bool,
    message_id: String,
    saw_content: bool,
}

impl AnthropicTransform {
    fn stop_events(&self) -> String {
        format!(
            "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            serde_json::json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": 0 } }),
            serde_json::json!({ "type": "message_stop" })
        )
    }
}

impl Stream for AnthropicTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.started {
            self.started = true;
            let start = format!(
                "event: message_start\ndata: {}\n\n",
                serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": self.message_id,
                        "type": "message",
                        "role": "assistant",
                        "model": self.model,
                        "content": [],
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": { "input_tokens": 0, "output_tokens": 0 }
                    }
                })
            );
            return Poll::Ready(Some(Ok(start)));
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
                    return Poll::Ready(Some(Ok(self.stop_events())));
                }
                Poll::Ready(Ok(_)) => {
                    let line = line.trim_end_matches('\n').trim_end_matches('\r');
                    if line.is_empty() { continue; }
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Some(evt) = parse(data) {
                            match evt.event.as_str() {
                                "thinking" => {
                                    let delta = evt.json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                    let frame = format!(
                                        "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\n",
                                        serde_json::json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
                                        serde_json::json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": delta } })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "chunk" => {
                                    let delta = evt.json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                    if !delta.is_empty() { self.saw_content = true; }
                                    let frame = format!(
                                        "event: content_block_delta\ndata: {}\n\n",
                                        serde_json::json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": delta } })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "citation" => continue,
                                "tool_use" => {
                                    let name = evt.json.get("name").and_then(|v| v.as_str()).unwrap_or("web_search");
                                    let args = evt.json.get("arguments").cloned().unwrap_or(serde_json::json!({}));
                                    let frame = format!(
                                        "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\nevent: content_block_stop\ndata: {}\n\n",
                                        serde_json::json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": format!("toolu_{}", self.session_id.chars().take(8).collect::<String>()), "name": name, "input": args } }),
                                        serde_json::json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": serde_json::to_string(&args).unwrap_or_default() } }),
                                        serde_json::json!({ "type": "content_block_stop", "index": 1 })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "error" => {
                                    let msg = evt.json.get("message").and_then(|v| v.as_str()).unwrap_or("上游流错误");
                                    self.finished = true;
                                    let frame = format!(
                                        "event: content_block_delta\ndata: {}\n\nevent: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
                                        serde_json::json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": format!("\n\n[上游错误: {msg}]") } }),
                                        serde_json::json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": 0 } }),
                                        serde_json::json!({ "type": "message_stop" })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "done" => {
                                    self.finished = true;
                                    return Poll::Ready(Some(Ok(self.stop_events())));
                                }
                                _ => continue,
                            }
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.finished = true;
                    return Poll::Ready(Some(Ok(self.stop_events())));
                }
            }
        }
    }
}

fn parse(data: &str) -> Option<Event> {
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


