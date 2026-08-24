use crate::config::{ProviderKeys, ProviderKind};
use crate::models::ConversationMessage;
use crate::secrets::{SecretMutationReport, SecretStore};
use crate::tools::ToolRuntime;

use super::{
    Agent, AssistantStreamExtractor, append_secret_warnings, extract_partial_assistant_value,
    normalize_model_name, parse_envelope,
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
    assert_eq!(agent.history[0].content, "5");
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
