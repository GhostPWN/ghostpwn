use std::path::Path;

use super::{
    UiRole, UiState, apply_agent_event, build_audit_prompt, oauth_deadline, parse_audit_command,
    resolve_approval, transcript_line_count,
};
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
