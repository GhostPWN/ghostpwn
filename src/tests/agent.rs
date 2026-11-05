use crate::config::{ProviderKeys, ProviderKind};
use crate::models::{ConversationMessage, ConversationPart};
use crate::secrets::{SecretMutationReport, SecretStore};
use crate::tools::ToolRuntime;

use super::{
    Agent, AssistantStreamExtractor, append_secret_warnings, extract_partial_assistant_value,
    looks_like_structured_attempt, normalize_model_name, parse_envelope,
};

fn test_agent(provider: ProviderKind) -> Agent {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let tools = ToolRuntime::new(workspace.path().to_path_buf()).expect("tool runtime");
    let secret_store = SecretStore::file_only(workspace.path().join("state.json"));

    Agent::new(
        provider,
        provider.default_model().to_string(),
        ProviderKeys::default(),
        secret_store,
        tools,
    )
}

#[test]
fn parse_envelope_reads_json_block() {
    let raw = "```json\n{\"assistant\":\"ok\",\"tool_calls\":[]}\n```";
    let env = parse_envelope(raw);
    assert_eq!(env.assistant.as_deref(), Some("ok"));
    assert!(env.tool_calls.is_empty());
}

#[test]
fn flags_truncated_envelope_but_not_plain_prose() {
    // A cut-off envelope (valid opener + schema key, unparsable) should be flagged.
    let truncated = "{\"assistant\":\"writing\",\"tool_calls\":[{\"name\":\"writeFile\",\"argument";
    assert!(looks_like_structured_attempt(truncated));
    // Ordinary prose that merely mentions braces must not be flagged.
    assert!(!looks_like_structured_attempt(
        "Here is a JSON object like {\"key\": 1} you could use."
    ));
    assert!(!looks_like_structured_attempt("Just a normal answer."));
}

#[test]
fn parse_envelope_ignores_concatenated_json_tail() {
    let raw = concat!(
        "{\"assistant\":\"checking\",\"tool_calls\":[",
        "{\"name\":\"listDirectory\",\"arguments\":{\"path\":\".\"}}",
        "]}",
        "{\"assistant\":\"done\",\"tool_calls\":[]}"
    );

    let env = parse_envelope(raw);

    assert_eq!(env.assistant.as_deref(), Some("checking"));
    assert_eq!(env.tool_calls.len(), 1);
    assert_eq!(env.tool_calls[0].name, "listDirectory");
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

#[test]
fn partial_assistant_decodes_unicode_surrogate_pair() {
    let partial = extract_partial_assistant_value(r#"{"assistant":"status: \uD83D\uDE00"#);

    assert_eq!(partial.as_deref(), Some("status: 😀"));
}

#[test]
fn partial_assistant_replaces_unpaired_unicode_surrogates() {
    let high = extract_partial_assistant_value(r#"{"assistant":"\uD83D after"#);
    let low = extract_partial_assistant_value(r#"{"assistant":"\uDE00 after"#);

    assert_eq!(high.as_deref(), Some("� after"));
    assert_eq!(low.as_deref(), Some("� after"));
}

#[test]
fn partial_assistant_replaces_invalid_unicode_escape_and_continues() {
    let mut extractor = AssistantStreamExtractor::default();

    assert_eq!(
        extractor
            .ingest_chunk(r#"{"assistant":"before \u"#)
            .as_deref(),
        Some("before ")
    );
    assert_eq!(
        extractor.ingest_chunk("ZZZZafter").as_deref(),
        Some("�after")
    );
}

#[test]
fn stream_extractor_waits_for_chunked_unicode_surrogate_pair() {
    let mut extractor = AssistantStreamExtractor::default();

    assert_eq!(
        extractor
            .ingest_chunk(r#"{"assistant":"status: \uD83D"#)
            .as_deref(),
        Some("status: ")
    );
    assert_eq!(extractor.ingest_chunk(r#"\uDE"#), None);
    assert_eq!(
        extractor.ingest_chunk(r#"00","tool_calls":[]}"#).as_deref(),
        Some("😀")
    );
    assert_eq!(extractor.finish_with("status: 😀"), None);
}

#[test]
fn normalize_model_name_strips_common_prefix_and_whitespace() {
    assert_eq!(
        normalize_model_name(" models/gemini-2.5-pro  "),
        "gemini-2.5-pro"
    );
    assert_eq!(normalize_model_name("gpt-4o"), "gpt-4o");
}

#[test]
fn connected_provider_becomes_active_with_default_model() {
    let mut agent = test_agent(ProviderKind::Google);
    agent.history.push(ConversationMessage::user("old account"));

    agent.activate_connected_provider(ProviderKind::OpenAi, "sk-test-token".to_string());

    assert_eq!(agent.current_provider(), ProviderKind::OpenAi);
    assert_eq!(agent.current_model(), ProviderKind::OpenAi.default_model());
    assert!(
        agent
            .provider_keys_snapshot()
            .is_connected(ProviderKind::OpenAi)
    );
    assert!(agent.history.is_empty());
}

#[test]
fn switching_provider_clears_history() {
    let mut agent = test_agent(ProviderKind::Google);
    agent
        .history
        .push(ConversationMessage::user("provider-specific context"));

    agent.switch_model(ProviderKind::Anthropic, None);

    assert!(agent.history.is_empty());
}

#[test]
fn switching_model_on_same_provider_preserves_history() {
    let mut agent = test_agent(ProviderKind::Google);
    agent.history.push(ConversationMessage::user("keep me"));

    agent.switch_model(ProviderKind::Google, Some("gemini-test".to_string()));

    assert_eq!(agent.history.len(), 1);
}

#[test]
fn unavailable_current_model_is_replaced_by_first_discovered_model() {
    let mut agent = test_agent(ProviderKind::Google);
    agent.switch_model(ProviderKind::Google, Some("retired-model".to_string()));
    let models = vec!["gemini-current".to_string(), "gemini-other".to_string()];

    let message = agent
        .reconcile_current_model(ProviderKind::Google, &models)
        .expect("model should change");

    assert_eq!(agent.current_model(), "gemini-current");
    assert!(message.contains("gemini-current"));
}

#[test]
fn available_current_model_is_not_replaced() {
    let mut agent = test_agent(ProviderKind::Google);
    let current = ProviderKind::Google.default_model().to_string();

    assert_eq!(
        agent.reconcile_current_model(ProviderKind::Google, &[current]),
        None
    );
}

#[test]
fn history_is_bounded_to_recent_messages() {
    let mut agent = test_agent(ProviderKind::Google);
    for index in 0..105 {
        agent
            .history
            .push(ConversationMessage::user(index.to_string()));
    }

    agent.trim_history();

    assert_eq!(agent.history.len(), 100);
    assert_eq!(
        agent.history[0].content,
        vec![ConversationPart::Text("5".to_string())]
    );
}

#[test]
fn recent_image_parts_survive_history_trimming() {
    use std::sync::Arc;

    use crate::models::{ImageAttachment, ImageMediaType};

    let mut agent = test_agent(ProviderKind::Google);
    for index in 0..100 {
        agent
            .history
            .push(ConversationMessage::user(index.to_string()));
    }
    agent
        .history
        .push(ConversationMessage::user_with_parts(vec![
            ConversationPart::Text("inspect".to_string()),
            ConversationPart::Image(ImageAttachment {
                media_type: ImageMediaType::Png,
                data: Arc::from(*b"png"),
                name: "shot.png".to_string(),
            }),
        ]));

    agent.trim_history();

    assert_eq!(agent.history.len(), 100);
    assert!(agent.history.last().unwrap().has_images());
}

#[tokio::test]
async fn rejects_images_above_conversation_retention_limit() {
    use std::sync::Arc;

    use crate::images::{MAX_IMAGE_BYTES_PER_MESSAGE, MAX_RETAINED_IMAGE_BYTES};
    use crate::models::{ImageAttachment, ImageMediaType};

    let mut agent = test_agent(ProviderKind::Google);
    let data: Arc<[u8]> = Arc::from(vec![0; MAX_IMAGE_BYTES_PER_MESSAGE]);
    for index in 0..(MAX_RETAINED_IMAGE_BYTES / MAX_IMAGE_BYTES_PER_MESSAGE) {
        agent
            .history
            .push(ConversationMessage::user_with_parts(vec![
                ConversationPart::Image(ImageAttachment {
                    media_type: ImageMediaType::Png,
                    data: Arc::clone(&data),
                    name: format!("shot-{index}.png"),
                }),
            ]));
    }
    let next = ImageAttachment {
        media_type: ImageMediaType::Png,
        data: Arc::from(*b"x"),
        name: "next.png".to_string(),
    };

    let error = agent
        .prepare_user_input(String::new(), vec![next])
        .await
        .unwrap_err();

    assert!(error.to_string().contains("60 MiB retention limit"));
}

#[tokio::test]
async fn accepts_images_when_trim_drops_oldest_image() {
    use std::sync::Arc;

    use crate::images::{MAX_IMAGE_BYTES_PER_MESSAGE, MAX_RETAINED_IMAGE_BYTES};
    use crate::models::{ImageAttachment, ImageMediaType};

    use super::MAX_HISTORY_MESSAGES;

    let mut agent = test_agent(ProviderKind::Google);
    let data: Arc<[u8]> = Arc::from(vec![0; MAX_IMAGE_BYTES_PER_MESSAGE]);
    for index in 0..(MAX_RETAINED_IMAGE_BYTES / MAX_IMAGE_BYTES_PER_MESSAGE) {
        agent
            .history
            .push(ConversationMessage::user_with_parts(vec![
                ConversationPart::Image(ImageAttachment {
                    media_type: ImageMediaType::Png,
                    data: Arc::clone(&data),
                    name: format!("shot-{index}.png"),
                }),
            ]));
    }
    while agent.history.len() < MAX_HISTORY_MESSAGES {
        agent
            .history
            .push(ConversationMessage::user("filler".to_string()));
    }
    assert_eq!(agent.history.len(), MAX_HISTORY_MESSAGES);

    let next = ImageAttachment {
        media_type: ImageMediaType::Png,
        data: Arc::from(*b"x"),
        name: "next.png".to_string(),
    };

    let message = agent
        .prepare_user_input(String::new(), vec![next])
        .await
        .expect("trimmed history should accept new image");
    assert!(message.has_images());
}

#[test]
fn connected_copilot_becomes_active_with_default_model() {
    let mut agent = test_agent(ProviderKind::Google);

    agent.activate_connected_provider(ProviderKind::Copilot, "ghu-test-token".to_string());

    assert_eq!(agent.current_provider(), ProviderKind::Copilot);
    assert_eq!(agent.current_model(), ProviderKind::Copilot.default_model());
    assert!(
        agent
            .provider_keys_snapshot()
            .is_connected(ProviderKind::Copilot)
    );
    assert_eq!(
        agent.provider_name(),
        format!("copilot / {}", ProviderKind::Copilot.default_model())
    );
}

#[test]
fn connected_codex_becomes_active_with_default_model() {
    let mut agent = test_agent(ProviderKind::Google);

    agent.activate_connected_provider(
        ProviderKind::Codex,
        "{\"refresh_token\":\"refresh\",\"access_token\":\"access\",\"expires_at\":9999999999}"
            .to_string(),
    );

    assert_eq!(agent.current_provider(), ProviderKind::Codex);
    assert_eq!(agent.current_model(), ProviderKind::Codex.default_model());
    assert!(
        agent
            .provider_keys_snapshot()
            .is_connected(ProviderKind::Codex)
    );
    assert_eq!(
        agent.provider_name(),
        format!("codex / {}", ProviderKind::Codex.default_model())
    );
}

#[test]
fn partial_secret_backend_failures_are_visible() {
    let report = SecretMutationReport {
        keychain_saved: false,
        keychain_error: Some("locked".to_string()),
        file_saved: true,
        file_error: None,
    };
    let mut message = "Connected openai.".to_string();

    append_secret_warnings(&mut message, &report);

    assert_eq!(
        message,
        "Connected openai. OS keychain operation failed: locked."
    );
}

struct ScriptedProvider {
    replies: std::sync::Mutex<std::collections::VecDeque<crate::models::Completion>>,
    seen: std::sync::Arc<std::sync::Mutex<Vec<Vec<ConversationMessage>>>>,
    native: bool,
}

#[async_trait::async_trait]
impl crate::providers::Provider for ScriptedProvider {
    fn display_name(&self) -> String {
        "scripted".to_string()
    }
    fn uses_native_tools(&self) -> bool {
        self.native
    }
    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> anyhow::Result<crate::models::Completion> {
        assert_eq!(
            system.contains("Always output valid JSON only"),
            !self.native
        );
        self.seen.lock().unwrap().push(messages.to_vec());
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider call");
        on_delta(reply.text.clone());
        Ok(reply)
    }
}

fn tool_request(id: &str, name: &str, arguments: serde_json::Value) -> crate::models::ToolRequest {
    crate::models::ToolRequest {
        id: id.to_string(),
        provider_id: Some(id.to_string()),
        call: crate::models::ToolCall {
            name: name.to_string(),
            arguments,
        },
    }
}

#[tokio::test]
async fn native_turn_records_batch_results_and_executes_approved_mutation_once() {
    use crate::models::{AgentEvent, Completion};
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let mut agent = test_agent(ProviderKind::OpenAi);
    agent.tools = ToolRuntime::new(directory.path().to_path_buf()).unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    agent.provider = Box::new(ScriptedProvider {
        replies: std::sync::Mutex::new(std::collections::VecDeque::from([
            Completion {
                text: "Checking files".to_string(),
                tool_calls: vec![
                    tool_request(
                        "write",
                        "writeFile",
                        json!({"path": "result.txt", "content": "once"}),
                    ),
                    tool_request("read", "readFile", json!({"path": "result.txt"})),
                    tool_request("missing", "readFile", json!({"path": "absent.txt"})),
                ],
                ..Completion::default()
            },
            Completion::text("Done".to_string()),
        ])),
        seen: seen.clone(),
        native: true,
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (result, events) = tokio::join!(
        agent.handle_user_message(ConversationMessage::user("write and read"), tx),
        async {
            let mut calls = Vec::new();
            let mut approvals = 0;
            while let Some(event) = rx.recv().await {
                match event {
                    AgentEvent::ApprovalRequired { response, .. } => {
                        approvals += 1;
                        response.send(true).unwrap();
                    }
                    AgentEvent::ToolCall { name, .. } => calls.push(name),
                    AgentEvent::Error(error) => panic!("{error}"),
                    _ => {}
                }
            }
            (calls, approvals)
        }
    );
    result.unwrap();
    assert_eq!(
        events,
        (
            vec![
                "writeFile".to_string(),
                "readFile".to_string(),
                "readFile".to_string()
            ],
            1
        )
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("result.txt")).unwrap(),
        "once"
    );
    let snapshots = seen.lock().unwrap();
    assert_eq!(snapshots.len(), 2);
    let history = &snapshots[1];
    assert_eq!(
        history[1]
            .tool_calls
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["write", "read", "missing"]
    );
    let results = history[2..]
        .iter()
        .map(|m| m.tool_result.as_ref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .map(|r| r.call_id.as_str())
            .collect::<Vec<_>>(),
        ["write", "read", "missing"]
    );
    assert_eq!(results[1].output["content"], "once");
    assert!(results[2].is_error);
}

#[tokio::test]
async fn denied_mutation_returns_matching_error_and_model_continues() {
    use crate::models::{AgentEvent, Completion};
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let mut agent = test_agent(ProviderKind::OpenAi);
    agent.tools = ToolRuntime::new(directory.path().to_path_buf()).unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    agent.provider = Box::new(ScriptedProvider {
        replies: std::sync::Mutex::new(std::collections::VecDeque::from([
            Completion {
                tool_calls: vec![tool_request(
                    "denied",
                    "writeFile",
                    json!({"path": "denied.txt", "content": "no"}),
                )],
                ..Completion::default()
            },
            Completion::text("No changes made".to_string()),
        ])),
        seen: seen.clone(),
        native: true,
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (result, ()) = tokio::join!(
        agent.handle_user_message(ConversationMessage::user("write"), tx),
        async {
            while let Some(event) = rx.recv().await {
                match event {
                    AgentEvent::ApprovalRequired { response, .. } => response.send(false).unwrap(),
                    AgentEvent::ToolCall { .. } => panic!("denied tool executed"),
                    _ => {}
                }
            }
        }
    );
    result.unwrap();
    assert!(!directory.path().join("denied.txt").exists());
    let snapshots = seen.lock().unwrap();
    let result = snapshots[1][2].tool_result.as_ref().unwrap();
    assert_eq!(result.call_id, "denied");
    assert_eq!(result.output["error"], "denied by user");
    assert!(result.is_error);
}

#[tokio::test]
async fn duplicate_call_ids_reject_entire_batch_before_any_execution() {
    use crate::models::{AgentEvent, Completion};
    use serde_json::json;
    let mut agent = test_agent(ProviderKind::OpenAi);
    agent.provider = Box::new(ScriptedProvider {
        replies: std::sync::Mutex::new(std::collections::VecDeque::from([Completion {
            tool_calls: vec![
                tool_request("same", "listDirectory", json!({})),
                tool_request("same", "writeFile", json!({"path":"x", "content":"x"})),
            ],
            ..Completion::default()
        }])),
        seen: Default::default(),
        native: true,
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        agent
            .handle_user_message(ConversationMessage::user("inspect"), tx)
            .await
            .is_err()
    );
    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(
            event,
            AgentEvent::ApprovalRequired { .. } | AgentEvent::ToolCall { .. }
        ));
    }
    assert_eq!(agent.history.len(), 1);
}

#[tokio::test]
async fn legacy_turn_keeps_call_result_links_and_streams_plain_text() {
    use crate::models::{AgentEvent, Completion};
    let mut agent = test_agent(ProviderKind::Ollama);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    agent.provider = Box::new(ScriptedProvider {
        replies: std::sync::Mutex::new(std::collections::VecDeque::from([
            Completion::text(
                r#"{"assistant":"Inspecting","tool_calls":[{"name":"unknown","arguments":{}}]}"#
                    .to_string(),
            ),
            Completion::text(r#"{"assistant":"Finished","tool_calls":[]}"#.to_string()),
        ])),
        seen: seen.clone(),
        native: false,
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    agent
        .handle_user_message(ConversationMessage::user("inspect"), tx)
        .await
        .unwrap();
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::AssistantDelta(delta) = event {
            text.push_str(&delta);
        }
    }
    assert_eq!(text, "InspectingFinished");
    let history = seen.lock().unwrap();
    assert_eq!(
        history[1][1].tool_calls[0].id,
        history[1][2].tool_result.as_ref().unwrap().call_id
    );
}

#[test]
fn trimming_drops_complete_tool_batches() {
    use serde_json::json;
    let mut agent = test_agent(ProviderKind::OpenAi);
    let request = tool_request("old", "listDirectory", json!({}));
    let mut message = ConversationMessage::assistant("");
    message.tool_calls.push(request.clone());
    agent.history.push(message);
    agent.record_tool_result(&request, json!({}), false);
    for _ in 0..99 {
        agent.history.push(ConversationMessage::user("new"));
    }
    agent.trim_history();
    assert_eq!(agent.history.len(), 99);
    assert!(
        agent
            .history
            .iter()
            .all(|m| m.tool_result.is_none() && m.tool_calls.is_empty())
    );
}

#[test]
fn trimming_keeps_the_latest_batch_when_it_exceeds_the_message_limit() {
    use serde_json::json;
    let mut agent = test_agent(ProviderKind::OpenAi);
    let mut assistant = ConversationMessage::assistant("");
    assistant.tool_calls = (0..101)
        .map(|index| tool_request(&format!("call_{index}"), "listDirectory", json!({})))
        .collect();
    let requests = assistant.tool_calls.clone();
    agent.history.push(assistant);
    for request in &requests {
        agent.record_tool_result(request, json!({}), false);
    }
    agent.trim_history();
    assert_eq!(agent.history.len(), 102);
    assert_eq!(agent.history[0].tool_calls.len(), 101);
}

#[tokio::test]
async fn native_audit_returns_correlated_denials_without_approval_or_execution() {
    use crate::models::{AgentEvent, Completion};
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let mut agent = test_agent(ProviderKind::OpenAi);
    agent.tools = ToolRuntime::new(directory.path().to_path_buf()).unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    agent.provider = Box::new(ScriptedProvider {
        replies: std::sync::Mutex::new(std::collections::VecDeque::from([
            Completion {
                tool_calls: vec![
                    tool_request("write", "writeFile", json!({"path":"x", "content":"no"})),
                    tool_request("shell", "runCommand", json!({"command":"exit 0"})),
                ],
                ..Completion::default()
            },
            Completion::text("Audit only".to_string()),
        ])),
        seen: seen.clone(),
        native: true,
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    agent
        .handle_audit(
            "audit".to_string(),
            directory.path().to_path_buf(),
            false,
            tx,
        )
        .await
        .unwrap();
    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(
            event,
            AgentEvent::ApprovalRequired { .. } | AgentEvent::ToolCall { .. }
        ));
    }
    let history = seen.lock().unwrap();
    for (index, id) in ["write", "shell"].iter().enumerate() {
        let result = history[1][index + 2].tool_result.as_ref().unwrap();
        assert_eq!(result.call_id, *id);
        assert_eq!(result.output["error"], "unavailable in this audit mode");
        assert!(result.is_error);
    }
    assert!(!directory.path().join("x").exists());
}
