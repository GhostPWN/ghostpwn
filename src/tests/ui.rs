use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::{
    ModelSelector, ModelSelectorMode, UiRole, UiState, apply_agent_event, approval_panel_height,
    build_audit_prompt, display_user_message, handle_submit, message_visible_lines, oauth_deadline,
    parse_audit_command, paste_terminal_text, queue_image, resolve_approval, transcript_line_count,
};
use ratatui::layout::Size;

use crate::agent::Agent;
use crate::config::{ProviderKeys, ProviderKind};
use crate::models::{
    AgentEvent, ConversationMessage, ConversationPart, ImageAttachment, ImageMediaType,
};
use crate::secrets::SecretStore;
use crate::tools::ToolRuntime;

fn test_image(name: &str, bytes: usize) -> ImageAttachment {
    ImageAttachment {
        media_type: ImageMediaType::Png,
        data: Arc::from(vec![0; bytes]),
        name: name.to_string(),
    }
}

#[test]
fn approval_event_waits_for_user_response() {
    let mut state = UiState::new("test".to_string());
    let (response, mut approval) = tokio::sync::oneshot::channel();

    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "runCommand".to_string(),
            args_summary: "cargo test".to_string(),
            reason: "File changes require approval",
            response,
        },
    );
    assert!(state.pending_approval.is_some());

    resolve_approval(&mut state, true);
    assert!(approval.try_recv().expect("approval response"));
}

#[test]
fn approval_panel_holds_request_and_transcript_records_answer() {
    let mut state = UiState::new("test".to_string());
    let (response, mut approval) = tokio::sync::oneshot::channel();

    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "runCommand".to_string(),
            args_summary: "UNSANDBOXED shell command in .: id".to_string(),
            reason: "Shell commands run without an OS sandbox",
            response,
        },
    );
    assert!(state.messages.is_empty(), "request belongs in the panel");
    let pending = state.pending_approval.as_ref().expect("pending approval");
    assert_eq!(pending.args_summary, "UNSANDBOXED shell command in .: id");
    assert_eq!(pending.reason, "Shell commands run without an OS sandbox");

    resolve_approval(&mut state, false);
    assert!(!approval.try_recv().expect("approval response"));
    let last = state.messages.last().expect("denial message");
    assert_eq!(last.role, UiRole::Error);
    assert_eq!(
        last.content,
        "Denied runCommand(UNSANDBOXED shell command in .: id)."
    );
}

#[test]
fn approval_panel_shrinks_transcript_but_keeps_minimum_rows() {
    let mut state = UiState::new("test".to_string());
    let size = Size::new(100, 30);
    let idle = message_visible_lines(&state, size);
    let (response, _approval) = tokio::sync::oneshot::channel();
    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "runCommand".to_string(),
            args_summary: "x".repeat(400),
            reason: "Shell commands run without an OS sandbox",
            response,
        },
    );

    let panel = approval_panel_height(&state, size.width, size.height);
    assert!(panel > 7, "long command wraps instead of being cut");
    assert_eq!(message_visible_lines(&state, size), idle - panel);

    let small = Size::new(40, 12);
    assert_eq!(message_visible_lines(&state, small), 3);
}

#[test]
fn tab_completes_audit_command() {
    let mut state = UiState::new("test".to_string());
    state.input = "/au".to_string();

    state.apply_completion();

    assert_eq!(state.input, "/audit ");
}

#[test]
fn terminal_paste_reaches_api_key_input() {
    let mut state = UiState::new("test".to_string());
    state.selector = Some(ModelSelector {
        id: 1,
        providers: vec![ProviderKind::OpenAi],
        provider_index: 0,
        provider_states: HashMap::new(),
        mode: ModelSelectorMode::ApiKeyInput {
            provider: ProviderKind::OpenAi,
            input: String::new(),
        },
        status: None,
        oauth_task: None,
    });

    paste_terminal_text(&mut state, "secret\r\nkey");

    let ModelSelectorMode::ApiKeyInput { input, .. } = &state.selector.unwrap().mode else {
        panic!("expected API key input");
    };
    assert_eq!(input, "secret key");
    assert!(state.input.is_empty());
}

#[test]
fn terminal_paste_reaches_input_when_selector_browsing() {
    let mut state = UiState::new("test".to_string());
    state.selector = Some(ModelSelector {
        id: 1,
        providers: vec![ProviderKind::OpenAi],
        provider_index: 0,
        provider_states: HashMap::new(),
        mode: ModelSelectorMode::Browse,
        status: None,
        oauth_task: None,
    });

    paste_terminal_text(&mut state, "hello\r\nworld");

    assert_eq!(state.input, "hello world");
    assert!(matches!(
        state.selector.unwrap().mode,
        ModelSelectorMode::Browse
    ));
}

#[test]
fn audit_prompt_requires_scope_evidence_and_coverage() {
    let prompt = build_audit_prompt("workspace path \"src\"", Path::new("/workspace/src"), false);

    assert!(prompt.contains("Read-only mode blocks shell commands"));
    assert!(prompt.contains("numberedContent"));
    assert!(prompt.contains("auditDependencies"));
    assert!(prompt.contains("Coverage section"));
}

#[test]
fn audit_fix_flag_is_parsed_and_enables_approved_mutations() {
    assert_eq!(parse_audit_command("/audit --fix src"), Some((true, "src")));
    assert_eq!(
        parse_audit_command("/audit authentication"),
        Some((false, "authentication"))
    );
    assert_eq!(parse_audit_command("/auditor"), None);

    let prompt = build_audit_prompt("workspace path \"src\"", Path::new("/workspace/src"), true);
    assert!(prompt.contains("Fix mode permits generateDiff"));
    assert!(prompt.contains("Every mutation requires user approval"));
}

#[test]
fn transcript_count_includes_wrapped_display_lines() {
    let mut state = UiState::new("test".to_string());
    state.push_message(UiRole::User, "one two three four five six".to_string());

    assert!(transcript_line_count(&state, 8) > transcript_line_count(&state, 80));
}

#[test]
fn queued_images_are_counted_and_rendered_without_data() {
    let mut state = UiState::new("test".to_string());
    queue_image(&mut state, test_image("clipboard.png", 4)).unwrap();
    assert_eq!(state.pending_images.len(), 1);

    let message = ConversationMessage::user_with_parts(vec![
        ConversationPart::Text("inspect ".to_string()),
        ConversationPart::Image(test_image("shot.png", 4)),
    ]);
    let display = display_user_message(&message);
    assert_eq!(display, "inspect [image: shot.png]");
    assert!(!display.contains("AAAA"));
}

#[tokio::test]
async fn invalid_image_submission_preserves_input_and_queued_images() {
    let workspace = tempfile::tempdir().unwrap();
    let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();
    let agent = Arc::new(tokio::sync::Mutex::new(Agent::new(
        ProviderKind::Ollama,
        "test".to_string(),
        ProviderKeys::default(),
        SecretStore::file_only(workspace.path().join("state.json")),
        tools,
    )));
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = UiState::new("test".to_string());
    state.input = "inspect @missing.png".to_string();
    state.pending_images.push(test_image("clipboard.png", 4));

    handle_submit(state.input.clone(), &mut state, &agent, &events).await;

    assert_eq!(state.input, "inspect @missing.png");
    assert_eq!(state.pending_images.len(), 1);
    assert_eq!(state.messages.last().unwrap().role, UiRole::Error);
}

#[tokio::test]
async fn clear_images_command_removes_only_pending_images() {
    let workspace = tempfile::tempdir().unwrap();
    let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();
    let agent = Arc::new(tokio::sync::Mutex::new(Agent::new(
        ProviderKind::Ollama,
        "test".to_string(),
        ProviderKeys::default(),
        SecretStore::file_only(workspace.path().join("state.json")),
        tools,
    )));
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = UiState::new("test".to_string());
    state.pending_images.push(test_image("clipboard.png", 4));
    state.push_message(UiRole::User, "keep".to_string());

    handle_submit("/clear-images".to_string(), &mut state, &agent, &events).await;

    assert!(state.pending_images.is_empty());
    assert_eq!(state.messages.len(), 1);
}

#[tokio::test]
async fn audit_command_clears_pending_images() {
    let workspace = tempfile::tempdir().unwrap();
    let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();
    let agent = Arc::new(tokio::sync::Mutex::new(Agent::new(
        ProviderKind::Ollama,
        "test".to_string(),
        ProviderKeys::default(),
        SecretStore::file_only(workspace.path().join("state.json")),
        tools,
    )));
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    let mut state = UiState::new("test".to_string());
    state.pending_images.push(test_image("clipboard.png", 4));

    handle_submit("/audit".to_string(), &mut state, &agent, &events).await;

    assert!(state.pending_images.is_empty());
    assert!(state.is_streaming);
    let notice = state.messages.last().unwrap();
    assert_eq!(notice.role, UiRole::Assistant);
    assert!(notice.content.contains("Dropped 1 queued image"));
    assert!(notice.content.contains("/audit runs text-only"));
}

#[tokio::test]
async fn audit_command_without_pending_images_stays_quiet() {
    let workspace = tempfile::tempdir().unwrap();
    let tools = ToolRuntime::new(workspace.path().to_path_buf()).unwrap();
    let agent = Arc::new(tokio::sync::Mutex::new(Agent::new(
        ProviderKind::Ollama,
        "test".to_string(),
        ProviderKeys::default(),
        SecretStore::file_only(workspace.path().join("state.json")),
        tools,
    )));
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    let mut state = UiState::new("test".to_string());

    handle_submit("/audit".to_string(), &mut state, &agent, &events).await;

    assert!(state.pending_images.is_empty());
    assert!(state.is_streaming);
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages.last().unwrap().role, UiRole::User);
}

#[test]
fn oauth_deadline_rejects_unrepresentable_expiry() {
    assert!(oauth_deadline(u64::MAX).is_err());
}

#[tokio::test]
async fn closing_model_selector_cancels_oauth_task() {
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(signal) = self.0.take() {
                let _ = signal.send(());
            }
        }
    }

    let (dropped, cancelled) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _signal = DropSignal(Some(dropped));
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;

    let mut state = UiState::new("test".to_string());
    state.selector = Some(ModelSelector {
        id: 1,
        providers: vec![ProviderKind::Codex],
        provider_index: 0,
        provider_states: HashMap::new(),
        mode: ModelSelectorMode::Browse,
        status: None,
        oauth_task: Some(task),
    });

    state.selector = None;

    tokio::time::timeout(std::time::Duration::from_secs(1), cancelled)
        .await
        .expect("OAuth task cancellation timed out")
        .expect("OAuth task did not report cancellation");
}

#[tokio::test]
async fn replacing_oauth_task_cancels_previous_task() {
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(signal) = self.0.take() {
                let _ = signal.send(());
            }
        }
    }

    let (dropped, cancelled) = tokio::sync::oneshot::channel();
    let previous = tokio::spawn(async move {
        let _signal = DropSignal(Some(dropped));
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;

    let replacement = tokio::spawn(std::future::pending::<()>());
    let mut selector = ModelSelector {
        id: 1,
        providers: vec![ProviderKind::Codex],
        provider_index: 0,
        provider_states: HashMap::new(),
        mode: ModelSelectorMode::Browse,
        status: None,
        oauth_task: Some(previous),
    };
    selector.replace_oauth_task(replacement);

    tokio::time::timeout(std::time::Duration::from_secs(1), cancelled)
        .await
        .expect("OAuth task replacement timed out")
        .expect("replaced OAuth task did not report cancellation");
}

#[test]
fn stale_selector_events_do_not_update_reopened_selector() {
    let mut state = UiState::new("test".to_string());
    state.selector = Some(ModelSelector {
        id: 2,
        providers: vec![ProviderKind::Codex],
        provider_index: 0,
        provider_states: HashMap::new(),
        mode: ModelSelectorMode::Browse,
        status: None,
        oauth_task: None,
    });

    apply_agent_event(
        &mut state,
        AgentEvent::Selector {
            id: 1,
            event: Box::new(AgentEvent::ProviderStatus {
                provider: ProviderKind::Codex,
                message: "stale".to_string(),
                error: true,
            }),
        },
    );

    assert!(state.selector.as_ref().unwrap().status.is_none());
}

#[tokio::test]
async fn cancellation_waits_for_acknowledgement_and_ignores_late_approval() {
    use super::ActiveTurn;
    use crate::models::TurnCancellation;
    let mut state = UiState::new("test".to_string());
    assert!(!state.cancel_turn());
    let cancellation = TurnCancellation::default();
    let task_cancellation = cancellation.clone();
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        task_cancellation.cancelled().await;
        events.send(AgentEvent::Cancelled).unwrap();
    });
    state.active_turn = Some(ActiveTurn {
        cancellation: cancellation.clone(),
        task,
    });
    state.is_streaming = true;
    state.streaming_content = "Partial response".to_string();
    let (response, approval) = tokio::sync::oneshot::channel();
    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "writeFile".to_string(),
            args_summary: "x".to_string(),
            reason: "File changes require approval",
            response,
        },
    );
    assert!(state.cancel_turn());
    assert!(cancellation.is_cancelled());
    assert!(
        state.is_streaming,
        "input reopened before cleanup acknowledgement"
    );
    assert!(state.pending_approval.is_none());
    assert!(approval.await.is_err());
    let (response, approval) = tokio::sync::oneshot::channel();
    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "writeFile".to_string(),
            args_summary: "late".to_string(),
            reason: "File changes require approval",
            response,
        },
    );
    assert!(state.pending_approval.is_none());
    assert!(approval.await.is_err());
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
        .await
        .unwrap()
        .unwrap();
    apply_agent_event(&mut state, event);
    assert!(!state.is_streaming);
    assert!(state.active_turn.is_none());
    assert!(!state.should_quit);
    assert!(
        state
            .messages
            .iter()
            .any(|m| m.content == "Partial response")
    );
    assert!(
        state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Turn cancelled")
    );
    assert!(state.streaming_content.is_empty());
}

#[tokio::test]
async fn shutdown_waits_for_active_turn_cleanup() {
    use super::{ActiveTurn, shutdown_active_turn};
    use crate::models::TurnCancellation;
    let cancellation = TurnCancellation::default();
    let task_cancellation = cancellation.clone();
    let started = Arc::new(tokio::sync::Notify::new());
    let task_started = started.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let task_release = release.clone();
    let cleaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task_cleaned = cleaned.clone();
    let task = tokio::spawn(async move {
        task_cancellation.cancelled().await;
        task_started.notify_one();
        task_release.notified().await;
        task_cleaned.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let mut state = UiState::new("test".to_string());
    state.active_turn = Some(ActiveTurn { cancellation, task });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let (result, ()) = tokio::join!(shutdown_active_turn(&mut state), async {
            started.notified().await;
            assert!(!cleaned.load(std::sync::atomic::Ordering::SeqCst));
            release.notify_one();
        });
        result.unwrap();
    })
    .await
    .expect("shutdown did not finish cleanup");
    assert!(cleaned.load(std::sync::atomic::Ordering::SeqCst));
    assert!(state.active_turn.is_none());
}

#[tokio::test]
async fn a_cancelled_audit_can_be_followed_by_another_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let agent = Arc::new(tokio::sync::Mutex::new(Agent::new(
        ProviderKind::OpenAi,
        "test".to_string(),
        ProviderKeys::default(),
        SecretStore::file_only(workspace.path().join("state.json")),
        ToolRuntime::new(workspace.path().to_path_buf()).unwrap(),
    )));
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
    let mut state = UiState::new("test".to_string());
    handle_submit("/audit".to_string(), &mut state, &agent, &events).await;
    assert!(state.cancel_turn());
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        let finished = matches!(event, AgentEvent::Cancelled);
        apply_agent_event(&mut state, event);
        if finished {
            break;
        }
    }
    assert!(!state.is_streaming);
    handle_submit("follow-up".to_string(), &mut state, &agent, &events).await;
    assert!(state.is_streaming);
    // The disconnected provider returns an error, then Done. No network request is sent.
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        let finished = matches!(event, AgentEvent::Done);
        apply_agent_event(&mut state, event);
        if finished {
            break;
        }
    }
    assert!(!state.is_streaming);
    assert!(state.active_turn.is_none());
    assert!(agent.try_lock().is_ok());
}
