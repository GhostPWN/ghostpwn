use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::models::{
    Completion, ConversationMessage, ConversationPart, MessageRole, NativeContent,
};
use crate::providers::native::{AnthropicStream, anthropic_completion, tool_result_text};
use crate::providers::sse::{consume_sse, is_event_stream};
use crate::providers::{
    MAX_OUTPUT_TOKENS, Provider, dedup_preserve_order, image_base64, message_text,
    provider_http_client, request_error,
};

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: provider_http_client(),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn display_name(&self) -> String {
        format!("anthropic / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        let mut after_id = None::<String>;
        let mut seen_after_ids = std::collections::HashSet::new();

        loop {
            let mut request = self
                .client
                .get("https://api.anthropic.com/v1/models")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .query(&[("limit", "1000")]);
            if let Some(cursor) = after_id.as_deref() {
                request = request.query(&[("after_id", cursor)]);
            }
            let response = request.send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Anthropic models API error {}: {}", status, body));
            }

            let body: Value = response.json().await?;
            models.extend(parse_claude_models(&body));
            if body.get("has_more").and_then(Value::as_bool) != Some(true) {
                break;
            }

            let next = body
                .get("last_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Anthropic models API returned an invalid cursor"))?;
            if !seen_after_ids.insert(next.to_string()) {
                return Err(anyhow!("Anthropic models API returned a repeated cursor"));
            }
            after_id = Some(next.to_string());
        }

        dedup_preserve_order(&mut models);
        Ok(models)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<Completion> {
        let payload = json!({
            "model": self.model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "temperature": 0.2,
            "system": system,
            "stream": true,
            "messages": map_messages(messages),
            "tools": crate::tools::schema::definitions().into_iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters})).collect::<Vec<_>>(),
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(request_error("Anthropic API", status, &body, messages));
        }

        if !is_event_stream(response.headers()) {
            let completion = anthropic_completion(&response.json::<Value>().await?)?;
            if !completion.text.is_empty() {
                on_delta(completion.text.clone());
            }
            return Ok(completion);
        }
        let mut stream = AnthropicStream::default();
        consume_sse(response, |data| {
            let event: Value = serde_json::from_str(data)?;
            stream.ingest(&event, on_delta)?;
            Ok(event["type"] != "message_stop")
        })
        .await?;
        stream.finish()
    }
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    let messages: Vec<Value> = history
        .iter()
        .map(|m| {
            if let Some(NativeContent::Anthropic(blocks)) = &m.native_content {
                return json!({"role": "assistant", "content": blocks});
            }
            if let Some(result) = &m.tool_result {
                return json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": result.call_id, "content": tool_result_text(m), "is_error": result.is_error}]});
            }
            if !m.tool_calls.is_empty() {
                let mut content = Vec::new();
                let text = message_text(m);
                if !text.is_empty() { content.push(json!({"type": "text", "text": text})); }
                content.extend(m.tool_calls.iter().map(|c| json!({"type": "tool_use", "id": c.id, "name": c.call.name, "input": c.call.arguments})));
                return json!({"role": "assistant", "content": content});
            }
            match m.role {
            MessageRole::User if m.has_images() => json!({
                "role": "user",
                "content": m.content.iter().map(|part| match part {
                    ConversationPart::Text(text) => json!({ "type": "text", "text": text }),
                    ConversationPart::Image(image) => json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": image.media_type.as_str(),
                            "data": image_base64(image),
                        }
                    }),
                }).collect::<Vec<_>>(),
            }),
            MessageRole::User => json!({ "role": "user", "content": message_text(m) }),
            MessageRole::Assistant => json!({
                "role": "assistant",
                "content": message_text(m),
            }),
            MessageRole::Tool => json!({
                "role": "user",
                "content": format!("[tool] {}", message_text(m)),
            }),
        }})
        .collect();
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        if message["content"].as_array().is_some_and(|parts| {
            parts
                .first()
                .is_some_and(|part| part["type"] == "tool_result")
        }) && let Some(last) = out.last_mut()
            && last["role"] == "user"
            && let Some(parts) = last["content"].as_array_mut()
        {
            parts.extend(message["content"].as_array().into_iter().flatten().cloned());
        } else {
            out.push(message);
        }
    }
    out
}

fn parse_claude_models(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
                .filter(|id| id.starts_with("claude-"))
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/providers_anthropic.rs"]
mod tests;
