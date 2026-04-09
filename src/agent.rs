use anyhow::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::models::{AgentEvent, ConversationMessage, ModelEnvelope, ToolCall};
use crate::providers::Provider;
use crate::tools::ToolRuntime;

const MAX_STEPS: usize = 15;

const SYSTEM_PROMPT: &str = r#"You are GhostPWN, an autonomous web penetration testing assistant for academic security research.

Always respond with JSON only (no markdown, no extra text) using this schema:
{
  "assistant": "string",
  "tool_calls": [
    { "name": "readFile|listDirectory|searchFiles|grep|runCommand|fileInfo", "arguments": { ... } }
  ]
}

Rules:
- If no tool is needed, return tool_calls as an empty array.
- Keep assistant concise and technical.
- Use tools proactively for repo exploration and command execution.
- Never include secrets in output.
"#;

pub struct Agent {
    provider: Box<dyn Provider>,
    tools: ToolRuntime,
    history: Vec<ConversationMessage>,
}

impl Agent {
    pub fn new(provider: Box<dyn Provider>, tools: ToolRuntime) -> Self {
        Self {
            provider,
            tools,
            history: Vec::new(),
        }
    }

    pub fn provider_name(&self) -> String {
        self.provider.display_name()
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub async fn handle_user_input(
        &mut self,
        user_text: String,
        events: UnboundedSender<AgentEvent>,
    ) -> Result<()> {
        self.history.push(ConversationMessage::user(user_text));

        for _ in 0..MAX_STEPS {
            let mut stream_extractor = AssistantStreamExtractor::default();
            let mut on_delta = |chunk: String| {
                if let Some(delta) = stream_extractor.ingest_chunk(&chunk) {
                    let _ = events.send(AgentEvent::AssistantDelta(delta));
                }
            };

            let raw = self
                .provider
                .stream_complete(SYSTEM_PROMPT, &self.history, &mut on_delta)
                .await?;

            let envelope = parse_envelope(&raw);

            if let Some(assistant) = envelope.assistant.as_deref()
                && !assistant.trim().is_empty()
            {
                if let Some(remaining) = stream_extractor.finish_with(assistant) {
                    let _ = events.send(AgentEvent::AssistantDelta(remaining));
                }

                self.history
                    .push(ConversationMessage::assistant(assistant.to_string()));
            }

            if envelope.tool_calls.is_empty() {
                let _ = events.send(AgentEvent::Done);
                return Ok(());
            }

            for call in envelope.tool_calls {
                let summary = self.tools.arg_summary(&call.name, &call.arguments);
                let _ = events.send(AgentEvent::ToolCall {
                    name: call.name.clone(),
                    args_summary: summary,
                });

                match self.tools.execute(&call).await {
                    Ok(result) => {
                        let text = format!(
                            "tool_result {}: {}",
                            call.name,
                            serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
                        );
                        self.history.push(ConversationMessage::tool(text));
                    }
                    Err(err) => {
                        let text = format!("tool_error {}: {}", call.name, err);
                        self.history.push(ConversationMessage::tool(text.clone()));
                        let _ = events.send(AgentEvent::Error(text));
                    }
                }
            }
        }

        let _ = events.send(AgentEvent::Error("step limit reached".to_string()));
        let _ = events.send(AgentEvent::Done);
        Ok(())
    }
}

fn parse_envelope(raw: &str) -> ModelEnvelope {
    if let Some(env) = try_parse_envelope(raw) {
        return env;
    }

    ModelEnvelope {
        assistant: Some(raw.to_string()),
        tool_calls: Vec::<ToolCall>::new(),
    }
}

fn try_parse_envelope(raw: &str) -> Option<ModelEnvelope> {
    if let Ok(env) = serde_json::from_str::<ModelEnvelope>(raw) {
        return Some(env);
    }

    if let Some(json) = extract_json_block(raw)
        && let Ok(env) = serde_json::from_str::<ModelEnvelope>(&json)
    {
        return Some(env);
    }

    None
}

fn extract_json_block(input: &str) -> Option<String> {
    let fence = "```";
    let start = input.find(fence)?;
    let after_start = &input[start + fence.len()..];

    let first_newline = after_start.find('\n')?;
    let body = &after_start[first_newline + 1..];
    let end = body.find(fence)?;

    Some(body[..end].trim().to_string())
}

#[derive(Default)]
struct AssistantStreamExtractor {
    raw: String,
    emitted_chars: usize,
}

impl AssistantStreamExtractor {
    fn ingest_chunk(&mut self, chunk: &str) -> Option<String> {
        self.raw.push_str(chunk);

        if let Some(parsed) = try_parse_envelope(&self.raw)
            && let Some(assistant) = parsed.assistant
        {
            return self.emit_new_suffix(&assistant);
        }

        if let Some(partial) = extract_partial_assistant_value(&self.raw) {
            return self.emit_new_suffix(&partial);
        }

        None
    }

    fn finish_with(&mut self, assistant: &str) -> Option<String> {
        self.emit_new_suffix(assistant)
    }

    fn emit_new_suffix(&mut self, full: &str) -> Option<String> {
        let total = full.chars().count();
        if total <= self.emitted_chars {
            return None;
        }

        let delta = full.chars().skip(self.emitted_chars).collect::<String>();
        self.emitted_chars = total;

        if delta.is_empty() { None } else { Some(delta) }
    }
}

fn extract_partial_assistant_value(raw: &str) -> Option<String> {
    let key = "\"assistant\"";
    let start = raw.find(key)?;
    let rest = &raw[start + key.len()..];

    let colon_pos = rest.find(':')?;
    let mut chars = rest[colon_pos + 1..].chars().peekable();

    while let Some(ch) = chars.peek() {
        if ch.is_whitespace() {
            let _ = chars.next();
        } else {
            break;
        }
    }

    if chars.next()? != '"' {
        return None;
    }

    let mut out = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => {
                let esc = match chars.next() {
                    Some(v) => v,
                    None => return Some(out),
                };

                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000C}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            match chars.next() {
                                Some(v) => hex.push(v),
                                None => return Some(out),
                            }
                        }

                        if let Ok(code) = u32::from_str_radix(&hex, 16)
                            && let Some(decoded) = char::from_u32(code)
                        {
                            out.push(decoded);
                        }
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{AssistantStreamExtractor, extract_partial_assistant_value, parse_envelope};

    #[test]
    fn parse_envelope_reads_json_block() {
        let raw = "```json\n{\"assistant\":\"ok\",\"tool_calls\":[]}\n```";
        let env = parse_envelope(raw);
        assert_eq!(env.assistant.as_deref(), Some("ok"));
        assert!(env.tool_calls.is_empty());
    }

    #[test]
    fn partial_assistant_extracts_incrementally() {
        let chunked = "{\"assistant\":\"hello\\nwor";
        let partial = extract_partial_assistant_value(chunked);
        assert_eq!(partial.as_deref(), Some("hello\nwor"));
    }

    #[test]
    fn stream_extractor_emits_only_new_suffix() {
        let mut extractor = AssistantStreamExtractor::default();
        let first = extractor.ingest_chunk("{\"assistant\":\"hel");
        let second = extractor.ingest_chunk("lo\",\"tool_calls\":[]}");

        assert_eq!(first.as_deref(), Some("hel"));
        assert_eq!(second.as_deref(), Some("lo"));
        assert_eq!(extractor.finish_with("hello"), None);
    }
}
