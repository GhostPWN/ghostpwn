use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::models::{
    Completion, ConversationMessage, ConversationPart, MessageRole, NativeContent,
};
use crate::providers::native::GoogleStream;
use crate::providers::sse::{consume_sse, is_event_stream};
use crate::providers::{
    MAX_OUTPUT_TOKENS, Provider, dedup_preserve_order, image_base64, message_text,
    provider_http_client, request_error,
};

const MAX_INLINE_REQUEST_BYTES: usize = 20_000_000;

pub struct GoogleProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl GoogleProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: provider_http_client(),
        }
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn display_name(&self) -> String {
        format!("google / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        let mut page_token = None::<String>;
        let mut seen_page_tokens = std::collections::HashSet::new();

        loop {
            let mut request = self
                .client
                .get("https://generativelanguage.googleapis.com/v1beta/models")
                .header("x-goog-api-key", &self.api_key)
                .query(&[("pageSize", "1000")]);
            if let Some(token) = page_token.as_deref() {
                request = request.query(&[("pageToken", token)]);
            }
            let response = request.send().await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Google models API error {}: {}", status, body));
            }

            let body: Value = response.json().await?;
            models.extend(parse_gemini_models(&body));
            let Some(next) = body
                .get("nextPageToken")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
            else {
                break;
            };
            if !seen_page_tokens.insert(next.to_string()) {
                return Err(anyhow!("Google models API returned a repeated page token"));
            }
            page_token = Some(next.to_string());
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
            "systemInstruction": {
                "role": "system",
                "parts": [{ "text": system }]
            },
            "contents": map_messages(messages),
            "tools": [{"functionDeclarations": tool_declarations()}],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": MAX_OUTPUT_TOKENS
            }
        });
        ensure_inline_request_size(&payload)?;

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.model
        );

        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(request_error("Google API", status, &body, messages));
        }

        let mut stream = GoogleStream::default();
        if !is_event_stream(response.headers()) {
            stream.ingest(&response.json::<Value>().await?, on_delta)?;
        } else {
            consume_sse(response, |data| {
                stream.ingest(&serde_json::from_str::<Value>(data)?, on_delta)?;
                Ok(true)
            })
            .await?;
        }
        stream.finish()
    }
}

fn tool_declarations() -> Vec<Value> {
    crate::tools::schema::definitions()
        .into_iter()
        .map(|tool| {
            let mut declaration = json!({"name": tool.name, "description": tool.description});
            if tool.parameters["properties"]
                .as_object()
                .is_some_and(|properties| !properties.is_empty())
            {
                let mut parameters = tool.parameters;
                remove_additional_properties(&mut parameters);
                declaration["parameters"] = parameters;
            }
            declaration
        })
        .collect()
}

fn remove_additional_properties(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("additionalProperties");
            for value in map.values_mut() {
                remove_additional_properties(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_additional_properties(value);
            }
        }
        _ => {}
    }
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    let messages: Vec<Value> = history
        .iter()
        .map(|m| {
            if let Some(NativeContent::Google(parts)) = &m.native_content {
                return json!({"role": "model", "parts": parts});
            }
            if let Some(result) = &m.tool_result {
                let mut response = json!({"name": result.name, "response": result.output});
                if let Some(id) = &result.provider_id { response["id"] = json!(id); }
                return json!({"role": "user", "parts": [{"functionResponse": response}]});
            }
            if !m.tool_calls.is_empty() {
                let mut parts = Vec::new();
                let text = message_text(m);
                if !text.is_empty() { parts.push(json!({"text": text})); }
                parts.extend(m.tool_calls.iter().map(|c| json!({"functionCall": {"id": c.id, "name": c.call.name, "args": c.call.arguments}})));
                return json!({"role": "model", "parts": parts});
            }
            match m.role {
            MessageRole::User => json!({
                "role": "user",
                "parts": m.content.iter().map(|part| match part {
                    ConversationPart::Text(text) => json!({ "text": text }),
                    ConversationPart::Image(image) => json!({
                        "inlineData": {
                            "mimeType": image.media_type.as_str(),
                            "data": image_base64(image),
                        }
                    }),
                }).collect::<Vec<_>>(),
            }),
            MessageRole::Assistant => json!({
                "role": "model",
                "parts": [{ "text": message_text(m) }],
            }),
            MessageRole::Tool => json!({
                "role": "user",
                "parts": [{ "text": format!("[tool] {}", message_text(m)) }],
            }),
        }})
        .collect();
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        if message["parts"].as_array().is_some_and(|parts| {
            parts
                .first()
                .is_some_and(|part| part.get("functionResponse").is_some())
        }) && let Some(last) = out.last_mut()
            && last["role"] == "user"
            && let Some(parts) = last["parts"].as_array_mut()
        {
            parts.extend(message["parts"].as_array().into_iter().flatten().cloned());
        } else {
            out.push(message);
        }
    }
    out
}

fn ensure_inline_request_size(payload: &Value) -> Result<()> {
    let size = serde_json::to_vec(payload)?.len();
    if size > MAX_INLINE_REQUEST_BYTES {
        return Err(anyhow!(
            "Google inline request is {size} bytes and exceeds the {MAX_INLINE_REQUEST_BYTES}-byte limit; clear old image messages or attach smaller images"
        ));
    }
    Ok(())
}

fn parse_gemini_models(body: &Value) -> Vec<String> {
    body.get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|model| {
                    model
                        .get("supportedGenerationMethods")
                        .and_then(Value::as_array)
                        .is_none_or(|methods| {
                            methods
                                .iter()
                                .any(|method| method.as_str() == Some("generateContent"))
                        })
                })
                .filter_map(|model| model.get("name").and_then(|v| v.as_str()))
                .filter_map(|name| name.strip_prefix("models/"))
                .filter(|id| id.contains("gemini"))
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/providers_google.rs"]
mod tests;
