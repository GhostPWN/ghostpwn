use std::collections::HashMap;
use std::path::Path;

use super::{
    ModelSelector, ModelSelectorMode, UiRole, UiState, apply_agent_event, build_audit_prompt,
    oauth_deadline, parse_audit_command, resolve_approval, transcript_line_count,
};
use crate::config::ProviderKind;
use crate::models::AgentEvent;

#[test]
fn approval_event_waits_for_user_response() {
    let mut state = UiState::new("test".to_string());
    let (response, mut approval) = tokio::sync::oneshot::channel();

    apply_agent_event(
        &mut state,
        AgentEvent::ApprovalRequired {
            name: "runCommand".to_string(),
            args_summary: "cargo test".to_string(),
            response,
        },
    );
    assert!(state.pending_approval.is_some());

    resolve_approval(&mut state, true);
    assert!(approval.try_recv().expect("approval response"));
}

#[test]
fn tab_completes_audit_command() {
    let mut state = UiState::new("test".to_string());
    state.input = "/au".to_string();

    state.apply_completion();

    assert_eq!(state.input, "/audit ");
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
