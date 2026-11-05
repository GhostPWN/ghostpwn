use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::models::{
    Completion, ConversationMessage, MessageRole, NativeContent, ToolCall, ToolRequest,
};
use crate::providers::sse::{consume_sse, extract_error_message, is_event_stream};
use crate::providers::{image_data_url, message_text};

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Tool response is missing {key}"))
}

fn request(id: String, name: String, arguments: Value) -> Result<ToolRequest> {
    if !arguments.is_object() {
        bail!("Tool arguments must be a JSON object");
    }
    Ok(ToolRequest {
        provider_id: (!id.is_empty()).then(|| id.clone()),
        id,
        call: ToolCall { name, arguments },
    })
}

pub fn response_completion(body: &Value) -> Result<Completion> {
    if body["status"] != "completed" {
        bail!("Model response did not complete; no tools were executed");
    }
    let items = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Model response is missing output"))?;
    let mut completion = Completion::default();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => completion.tool_calls.push(request(
                required_string(item, "call_id")?,
                required_string(item, "name")?,
                serde_json::from_str(&required_string(item, "arguments")?)?,
            )?),
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = part
                        .get("text")
                        .or_else(|| part.get("refusal"))
                        .and_then(Value::as_str)
                    {
                        completion.text.push_str(text);
                    }
                }
            }
            _ => {}
        }
    }
    completion.native_content = Some(NativeContent::Responses(items.clone()));
    Ok(completion)
}

#[derive(Default)]
pub struct ResponsesStream {
    completion: Option<Completion>,
    emitted_text: bool,
}

impl ResponsesStream {
    pub fn ingest(
        &mut self,
        event: &Value,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<()> {
        if let Some(error) = extract_error_message(event) {
            bail!("Model stream error: {error}");
        }
        match event.get("type").and_then(Value::as_str) {
            Some(
                "response.output_text.delta" | "response.refusal.delta" | "response.text.delta",
            ) => {
                if let Some(text) = event.get("delta").and_then(Value::as_str) {
                    self.emitted_text = true;
                    on_delta(text.to_owned());
                }
            }
            Some("response.completed") => {
                let completion = response_completion(&event["response"])?;
                if !self.emitted_text && !completion.text.is_empty() {
                    on_delta(completion.text.clone());
                }
                self.completion = Some(completion);
            }
            Some("response.incomplete" | "response.failed") => {
                bail!("Model stream did not complete; no tools were executed")
            }
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Completion> {
        self.completion.ok_or_else(|| {
            anyhow!("Model stream ended before response.completed; no tools were executed")
        })
    }
}

pub async fn read_responses(
    response: reqwest::Response,
    on_delta: &mut (dyn FnMut(String) + Send),
) -> Result<Completion> {
    if !is_event_stream(response.headers()) {
        let body = response.text().await?;
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            let completion = response_completion(&value)?;
            if !completion.text.is_empty() {
                on_delta(completion.text.clone());
            }
            return Ok(completion);
        }
        // Some Responses endpoints omit the SSE content type.
        let mut stream = ResponsesStream::default();
        for line in body.lines().filter_map(|line| line.strip_prefix("data:")) {
            if line.trim() != "[DONE]" {
                stream.ingest(&serde_json::from_str::<Value>(line.trim())?, on_delta)?;
            }
        }
        return stream.finish();
    }
    let mut stream = ResponsesStream::default();
    consume_sse(response, |data| {
        if data.trim() == "[DONE]" {
            return Ok(false);
        }
        let event: Value = serde_json::from_str(data)?;
        stream.ingest(&event, on_delta)?;
        Ok(event["type"] != "response.completed")
    })
    .await?;
    stream.finish()
}

pub fn response_messages(history: &[ConversationMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for message in history {
        if let Some(NativeContent::Responses(items)) = &message.native_content {
            out.extend(items.iter().cloned());
            continue;
        }
        if let Some(result) = &message.tool_result {
            out.push(json!({"type": "function_call_output", "call_id": result.call_id, "output": result.output.to_string()}));
            continue;
        }
        if message.has_images() || !message_text(message).is_empty() {
            let assistant = message.role == MessageRole::Assistant;
            out.push(json!({"type": "message", "role": if assistant {"assistant"} else {"user"},
                "content": message.content.iter().map(|part| match part {
                    crate::models::ConversationPart::Text(text) => json!({"type": if assistant {"output_text"} else {"input_text"}, "text": if message.role == MessageRole::Tool {format!("[tool] {text}")} else {text.clone()}}),
                    crate::models::ConversationPart::Image(image) => json!({"type": "input_image", "image_url": image_data_url(image)}),
                }).collect::<Vec<_>>()
            }));
        }
        for call in &message.tool_calls {
            out.push(json!({"type": "function_call", "call_id": call.id, "name": call.call.name, "arguments": call.call.arguments.to_string()}));
        }
    }
    out
}

pub fn chat_completion(body: &Value) -> Result<Completion> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow!("Chat response is missing a choice"))?;
    if !matches!(
        choice["finish_reason"].as_str(),
        Some("stop" | "tool_calls")
    ) {
        bail!("Chat response did not complete; no tools were executed");
    }
    let message = &choice["message"];
    let mut completion =
        Completion::text(message["content"].as_str().unwrap_or_default().to_owned());
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        completion.tool_calls.push(request(
            required_string(call, "id")?,
            required_string(&call["function"], "name")?,
            serde_json::from_str(&required_string(&call["function"], "arguments")?)?,
        )?);
    }
    Ok(completion)
}

#[derive(Default)]
pub struct ChatStream {
    text: String,
    calls: std::collections::BTreeMap<u64, Value>,
    finished: bool,
}

impl ChatStream {
    pub fn ingest(
        &mut self,
        event: &Value,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<()> {
        if let Some(error) = extract_error_message(event) {
            bail!("Chat stream error: {error}");
        }
        let Some(choice) = event["choices"].as_array().and_then(|a| a.first()) else {
            return Ok(());
        };
        if let Some(reason) = choice["finish_reason"].as_str() {
            if !matches!(reason, "stop" | "tool_calls") {
                bail!("Chat stream stopped with {reason}; no tools were executed");
            }
            self.finished = true;
        }
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str() {
            self.text.push_str(text);
            on_delta(text.to_owned());
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let index = call["index"]
                .as_u64()
                .ok_or_else(|| anyhow!("Tool delta is missing index"))?;
            let target = self
                .calls
                .entry(index)
                .or_insert_with(|| json!({"id": "", "function": {"name": "", "arguments": ""}}));
            for (key, value) in [
                ("id", &call["id"]),
                ("name", &call["function"]["name"]),
                ("arguments", &call["function"]["arguments"]),
            ] {
                if let Some(fragment) = value.as_str() {
                    let slot = if key == "id" {
                        &mut target["id"]
                    } else {
                        &mut target["function"][key]
                    };
                    let mut text = slot.as_str().unwrap_or_default().to_owned();
                    text.push_str(fragment);
                    *slot = json!(text);
                }
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Completion> {
        if !self.finished {
            bail!("Chat stream ended before finish_reason; no tools were executed");
        }
        chat_completion(
            &json!({"choices": [{"finish_reason": "stop", "message": {"content": self.text, "tool_calls": self.calls.into_values().collect::<Vec<_>>()}}]}),
        )
    }
}

pub fn anthropic_completion(body: &Value) -> Result<Completion> {
    if !matches!(
        body["stop_reason"].as_str(),
        Some("end_turn" | "tool_use" | "stop_sequence" | "refusal")
    ) {
        bail!("Anthropic response did not complete; no tools were executed");
    }
    let blocks = body["content"]
        .as_array()
        .ok_or_else(|| anyhow!("Anthropic response is missing content"))?;
    let mut completion = Completion::default();
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => completion
                .text
                .push_str(block["text"].as_str().unwrap_or_default()),
            Some("tool_use") => completion.tool_calls.push(request(
                required_string(block, "id")?,
                required_string(block, "name")?,
                block["input"].clone(),
            )?),
            _ => {}
        }
    }
    completion.native_content = Some(NativeContent::Anthropic(blocks.clone()));
    Ok(completion)
}

#[derive(Default)]
pub struct AnthropicStream {
    blocks: std::collections::BTreeMap<u64, Value>,
    arguments: std::collections::HashMap<u64, String>,
    stop_reason: Value,
    finished: bool,
}

impl AnthropicStream {
    pub fn ingest(
        &mut self,
        event: &Value,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<()> {
        if let Some(error) = extract_error_message(event) {
            bail!("Anthropic stream error: {error}");
        }
        match event["type"].as_str() {
            Some("content_block_start") => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("Missing content block index"))?;
                let block = event["content_block"].clone();
                if block["type"] == "text"
                    && let Some(text) = block["text"].as_str()
                {
                    on_delta(text.to_owned());
                }
                self.blocks.insert(index, block);
            }
            Some("content_block_delta") => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("Missing content block index"))?;
                let block = self
                    .blocks
                    .get_mut(&index)
                    .ok_or_else(|| anyhow!("Delta for unknown content block"))?;
                let delta = &event["delta"];
                if delta["type"] == "input_json_delta" {
                    self.arguments.entry(index).or_default().push_str(
                        delta["partial_json"]
                            .as_str()
                            .ok_or_else(|| anyhow!("Missing partial tool JSON"))?,
                    );
                } else {
                    let key = match delta["type"].as_str() {
                        Some("text_delta") => "text",
                        Some("thinking_delta") => "thinking",
                        Some("signature_delta") => "signature",
                        _ => return Ok(()),
                    };
                    if let Some(text) = delta[key].as_str() {
                        let mut value = block[key].as_str().unwrap_or_default().to_owned();
                        value.push_str(text);
                        block[key] = json!(value);
                        if key == "text" {
                            on_delta(text.to_owned());
                        }
                    }
                }
            }
            Some("content_block_stop") => {
                let index = event["index"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("Missing content block index"))?;
                if let Some(arguments) = self.arguments.remove(&index) {
                    self.blocks
                        .get_mut(&index)
                        .ok_or_else(|| anyhow!("Unknown content block"))?["input"] =
                        serde_json::from_str(&arguments)?;
                }
            }
            Some("message_delta") => self.stop_reason = event["delta"]["stop_reason"].clone(),
            Some("message_stop") => self.finished = true,
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Completion> {
        if !self.finished || !self.arguments.is_empty() {
            bail!("Anthropic stream ended before completion; no tools were executed");
        }
        anthropic_completion(
            &json!({"stop_reason": self.stop_reason, "content": self.blocks.into_values().collect::<Vec<_>>()}),
        )
    }
}

#[derive(Default)]
pub struct GoogleStream {
    parts: Vec<Value>,
    finished: bool,
}

impl GoogleStream {
    pub fn ingest(
        &mut self,
        event: &Value,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<()> {
        if let Some(error) = extract_error_message(event) {
            bail!("Google response error: {error}");
        }
        let candidate = event["candidates"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow!("Google response is missing a candidate"))?;
        if let Some(reason) = candidate["finishReason"].as_str() {
            if reason != "STOP" {
                bail!("Google response stopped with {reason}; no tools were executed");
            }
            self.finished = true;
        }
        for part in candidate["content"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if part["thought"] != true
                && let Some(text) = part["text"].as_str()
            {
                on_delta(text.to_owned());
            }
            self.parts.push(part.clone());
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Completion> {
        if !self.finished {
            bail!("Google stream ended before completion; no tools were executed");
        }
        let mut completion = Completion::default();
        for part in &self.parts {
            if let Some(call) = part.get("functionCall") {
                let id = call["id"].as_str().map(str::to_owned).unwrap_or_default();
                completion.tool_calls.push(request(
                    id,
                    required_string(call, "name")?,
                    call.get("args").cloned().unwrap_or_else(|| json!({})),
                )?);
            }
            if part["thought"] != true
                && let Some(text) = part["text"].as_str()
            {
                completion.text.push_str(text);
            }
        }
        completion.native_content = Some(NativeContent::Google(self.parts));
        Ok(completion)
    }
}

pub fn tool_result_text(message: &ConversationMessage) -> String {
    message
        .tool_result
        .as_ref()
        .map(|r| r.output.to_string())
        .unwrap_or_else(|| message_text(message))
}

#[cfg(test)]
#[path = "../tests/providers_native.rs"]
mod tests;
