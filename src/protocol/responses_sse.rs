//! OpenAI Responses API SSE：上游 TokenHarbor SSE → Responses API SSE
//!
//! 事件序列：
//!   response.created
//!   response.output_text.delta (逐字)
//!   response.output_text.done
//!   response.completed
//! 非流式：完整 response 对象（output 数组）

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

pub struct ResponsesSseResponse {
    pub body: Body,
}

impl IntoResponse for ResponsesSseResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(self.body)
            .unwrap()
    }
}

pub fn responses_events(
    upstream: reqwest::Response,
    model: &str,
    response_id: &str,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(
        upstream.bytes_stream(),
    ));
    ResponsesTransform {
        reader: Box::pin(reader),
        model: model.to_string(),
        response_id: response_id.to_string(),
        started: false,
        finished: false,
        saw_content: false,
        accumulated: String::new(),
        pending_event: String::new(),
    }
}

struct ResponsesTransform {
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: String,
    response_id: String,
    started: bool,
    finished: bool,
    saw_content: bool,
    accumulated: String,
    pending_event: String,
}

impl ResponsesTransform {
    fn created_event(&self) -> String {
        format!(
            "event: response.created\ndata: {}\n\n",
            serde_json::json!({
                "type": "response.created",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "model": self.model,
                    "status": "in_progress",
                    "output": []
                }
            })
        )
    }

    fn done_event(&self) -> String {
        format!(
            "event: response.output_text.done\ndata: {}\n\n",
            serde_json::json!({
                "type": "response.output_text.done",
                "item_id": format!("msg_{}", &self.response_id[..self.response_id.len().min(20)]),
                "output_index": 0,
                "content_index": 0,
                "text": self.accumulated
            })
        )
    }

    fn completed_event(&self) -> String {
        format!(
            "event: response.completed\ndata: {}\n\n",
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "model": self.model,
                    "status": "completed",
                    "output": [{
                        "id": format!("msg_{}", &self.response_id[..self.response_id.len().min(20)]),
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": self.accumulated, "annotations": [] }]
                    }]
                }
            })
        )
    }
}

impl Stream for ResponsesTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.started {
            self.started = true;
            return Poll::Ready(Some(Ok(self.created_event())));
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
                    let done = self.done_event();
                    let completed = self.completed_event();
                    return Poll::Ready(Some(Ok(format!("{done}{completed}"))));
                }
                Poll::Ready(Ok(_)) => {
                    let line = line.trim_end_matches('\n').trim_end_matches('\r');
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(ev) = line.strip_prefix("event: ") {
                        self.pending_event = ev.trim().to_string();
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        let json: serde_json::Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        match self.pending_event.as_str() {
                            "thinking" => {
                                let delta =
                                    json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if delta.is_empty() {
                                    continue;
                                }
                                let frame = format!(
                                    "event: response.reasoning_summary_text.delta\ndata: {}\n\n",
                                    serde_json::json!({
                                        "type": "response.reasoning_summary_text.delta",
                                        "item_id": format!("msg_{}", &self.response_id[..self.response_id.len().min(20)]),
                                        "output_index": 0,
                                        "content_index": 0,
                                        "delta": delta
                                    })
                                );
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "chunk" => {
                                let delta =
                                    json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if !delta.is_empty() {
                                    self.saw_content = true;
                                    self.accumulated.push_str(delta);
                                }
                                let frame = format!(
                                    "event: response.output_text.delta\ndata: {}\n\n",
                                    serde_json::json!({
                                        "type": "response.output_text.delta",
                                        "item_id": format!("msg_{}", &self.response_id[..self.response_id.len().min(20)]),
                                        "output_index": 0,
                                        "content_index": 0,
                                        "delta": delta
                                    })
                                );
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "citation" => continue,
                            "done" | "error" => {
                                self.finished = true;
                                let done = self.done_event();
                                let completed = self.completed_event();
                                return Poll::Ready(Some(Ok(format!("{done}{completed}"))));
                            }
                            _ => continue,
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.finished = true;
                    let done = self.done_event();
                    let completed = self.completed_event();
                    return Poll::Ready(Some(Ok(format!("{done}{completed}"))));
                }
            }
        }
    }
}

/// 非流式 response 对象
pub fn responses_nonstream(content: &str, model: &str, response_id: &str) -> String {
    serde_json::json!({
        "id": response_id,
        "object": "response",
        "model": model,
        "status": "completed",
        "output": [{
            "id": format!("msg_{}", &response_id[..response_id.len().min(20)]),
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": content, "annotations": [] }]
        }]
    })
    .to_string()
}
