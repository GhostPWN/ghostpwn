use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::{ExecutionMode, PolicyDecision};
use crate::models::{ToolCall, TurnCancellation};
use crate::tools::ToolRuntime;

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        name: name.to_string(),
        arguments,
    }
}

#[test]
fn decisions_cover_normal_and_audit_modes() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("source.rs"), "code").unwrap();
    fs::write(root.path().join(".env"), "secret").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let normal = tools.prepare_call(
        &call("readFile", json!({"path":"source.rs"})),
        ExecutionMode::Workspace,
    );
    assert_eq!(normal.decision(), &PolicyDecision::Allow);
    for request in [
        call("readFile", json!({"path":".env"})),
        call("writeFile", json!({"path":"new.rs", "content":"code"})),
        call("runCommand", json!({"command":"true"})),
        call("auditDependencies", json!({})),
    ] {
        assert!(matches!(
            tools
                .prepare_call(&request, ExecutionMode::Workspace)
                .decision(),
            PolicyDecision::RequireApproval(_)
        ));
    }
    for allow_mutations in [false, true] {
        let mode = ExecutionMode::Audit {
            scope: root.path(),
            allow_mutations,
        };
        for request in [
            call("runCommand", json!({"command":"true"})),
            call("webFetch", json!({"url":"https://example.com"})),
        ] {
            assert!(matches!(
                tools.prepare_call(&request, mode).decision(),
                PolicyDecision::Deny(_)
            ));
        }
        let write = tools.prepare_call(
            &call("writeFile", json!({"path":"new.rs", "content":"code"})),
            mode,
        );
        assert_eq!(
            matches!(write.decision(), PolicyDecision::RequireApproval(_)),
            allow_mutations
        );
        assert_eq!(
            matches!(write.decision(), PolicyDecision::Deny(_)),
            !allow_mutations
        );
    }
}

#[tokio::test]
async fn execution_requires_approval_for_sensitive_reads_mutations_commands_and_osv() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".env"), "must not read").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    for request in [
        call("readFile", json!({"path":".env"})),
        call("writeFile", json!({"path":"blocked", "content":"no"})),
        call(
            "runCommand",
            json!({"command":"this command must not start"}),
        ),
        call("auditDependencies", json!({})),
    ] {
        let prepared = tools.prepare_call(&request, ExecutionMode::Workspace);
        let error = tools
            .execute_prepared(prepared, &TurnCancellation::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("requires user approval"));
    }
    assert!(!root.path().join("blocked").exists());
}

#[tokio::test]
async fn approval_cannot_override_denials_or_change_approved_arguments() {
    let root = tempdir().unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let request = call("writeFile", json!({"path":"blocked", "content":"no"}));
    let mut denied = tools.prepare_call(
        &request,
        ExecutionMode::Audit {
            scope: root.path(),
            allow_mutations: false,
        },
    );
    denied.approve();
    assert!(
        tools
            .execute_prepared(denied, &TurnCancellation::default())
            .await
            .is_err()
    );
    assert!(!root.path().join("blocked").exists());
    let mut request = call(
        "writeFile",
        json!({"path":"approved", "content":"reviewed"}),
    );
    let mut prepared = tools.prepare_call(&request, ExecutionMode::Workspace);
    prepared.approve();
    request.arguments = json!({"path":"different", "content":"unreviewed"});
    tools
        .execute_prepared(prepared, &TurnCancellation::default())
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("approved")).unwrap(),
        "reviewed"
    );
    assert!(!root.path().join("different").exists());
}

#[tokio::test]
async fn alias_normalization_and_audit_defaults_use_the_execution_target() {
    let root = tempdir().unwrap();
    let scope = root.path().join("src");
    fs::create_dir(&scope).unwrap();
    fs::write(scope.join("inside.rs"), "code").unwrap();
    fs::write(root.path().join("outside.rs"), "outside").unwrap();
    let scope = scope.canonicalize().unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let mode = ExecutionMode::Audit {
        scope: &scope,
        allow_mutations: false,
    };
    let default = tools.prepare_call(&call("Glob", json!({"pattern":"*.rs"})), mode);
    assert_eq!(default.decision(), &PolicyDecision::Allow);
    assert_eq!(default.call().name, "searchFiles");
    assert_eq!(default.call().arguments["path"], scope.to_str().unwrap());
    assert_eq!(default.call().arguments["excludeGenerated"], true);
    let result = tools
        .execute_prepared(default, &TurnCancellation::default())
        .await
        .unwrap();
    assert_eq!(result["matches"], json!(["inside.rs"]));
    for args in [
        json!({"pattern":"*.rs", "cwd":"."}),
        json!({"pattern":"*.rs", "path":"src", "cwd":"."}),
    ] {
        assert!(matches!(
            tools.prepare_call(&call("Glob", args), mode).decision(),
            PolicyDecision::Deny(_)
        ));
    }
    let read = tools.prepare_call(
        &call("Read", json!({"file_path":"src/inside.rs", "limit":1})),
        mode,
    );
    assert_eq!(read.decision(), &PolicyDecision::Allow);
    assert_eq!(read.call().arguments["maxLines"], 1);
    assert_eq!(read.call().arguments["lineNumbers"], true);
}

#[test]
fn invalid_shapes_unknown_tools_private_urls_and_invalid_cwd_are_denied() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "data").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    for request in [
        call("unknown", json!({})),
        call("readFile", json!({"path":3})),
        call("readFile", json!({"path":""})),
        call("readFile", json!({"path":"."})),
        call("runCommand", json!({"command":"   "})),
        call("runCommand", json!({"command":"true", "timeout":"30000"})),
        call("runCommand", json!({"command":"true", "cwd":"file"})),
        call("webFetch", json!({"url":"http://127.0.0.1/"})),
        call("webFetch", json!({"url":"file:///etc/passwd"})),
        call("writeFile", json!({"path":"../outside", "content":"no"})),
    ] {
        assert!(
            matches!(
                tools
                    .prepare_call(&request, ExecutionMode::Workspace)
                    .decision(),
                PolicyDecision::Deny(_)
            ),
            "{request:?}"
        );
    }
}

#[test]
fn every_patch_target_is_checked_including_moves_and_missing_parent_directories() {
    let root = tempdir().unwrap();
    let scope = root.path().join("src");
    fs::create_dir(&scope).unwrap();
    fs::write(scope.join("inside"), "code\n").unwrap();
    fs::write(root.path().join("outside"), "outside\n").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let mode = ExecutionMode::Audit {
        scope: &scope,
        allow_mutations: true,
    };
    for patch in [
        "*** Begin Patch\n*** Add File: src/new\n+okay\n*** Add File: outside-new/nested\n+no\n*** End Patch",
        "*** Begin Patch\n*** Delete File: outside\n*** End Patch",
        "*** Begin Patch\n*** Update File: outside\n@@\n-outside\n+no\n*** End Patch",
        "*** Begin Patch\n*** Update File: src/inside\n*** Move to: moved/nested\n@@\n-code\n+no\n*** End Patch",
    ] {
        let prepared = tools.prepare_call(&call("apply_patch", json!({"patch":patch})), mode);
        assert!(
            matches!(prepared.decision(), PolicyDecision::Deny(reason) if reason.contains("outside audit scope"))
        );
    }
    let valid = tools.prepare_call(&call("applyPatch", json!({"patchText":"*** Begin Patch\n*** Add File: src/new/nested\n+okay\n*** End Patch"})), mode);
    assert!(matches!(
        valid.decision(),
        PolicyDecision::RequireApproval(_)
    ));
    assert!(
        !scope.join("new").exists(),
        "policy evaluation created directories"
    );
    assert_eq!(fs::read_to_string(scope.join("inside")).unwrap(), "code\n");
}

#[test]
fn paths_in_a_sensitive_workspace_parent_remain_normal_source_reads() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("secrets/project");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("main.rs"), "code").unwrap();
    let tools = ToolRuntime::new(workspace).unwrap();
    let prepared = tools.prepare_call(
        &call("readFile", json!({"path":"main.rs"})),
        ExecutionMode::Workspace,
    );
    assert_eq!(prepared.decision(), &PolicyDecision::Allow);
}

#[cfg(unix)]
#[tokio::test]
async fn changed_symlink_targets_require_a_new_review_even_after_approval() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    fs::write(root.path().join(".env.one"), "one").unwrap();
    fs::write(root.path().join(".env.two"), "two").unwrap();
    let alias = root.path().join("config");
    symlink(".env.one", &alias).unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let mut prepared = tools.prepare_call(
        &call("Read", json!({"path":"config"})),
        ExecutionMode::Workspace,
    );
    assert!(matches!(
        prepared.decision(),
        PolicyDecision::RequireApproval(_)
    ));
    prepared.approve();
    fs::remove_file(&alias).unwrap();
    symlink(".env.two", &alias).unwrap();
    let error = tools
        .execute_prepared(prepared, &TurnCancellation::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("targets changed"));
}

#[cfg(unix)]
#[tokio::test]
async fn changed_mutation_parents_and_outside_audit_roots_are_denied() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let parent = root.path().join("new");
    fs::create_dir(&parent).unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let mut prepared = tools.prepare_call(
        &call("writeFile", json!({"path":"new/blocked", "content":"no"})),
        ExecutionMode::Workspace,
    );
    prepared.approve();
    fs::remove_dir(&parent).unwrap();
    symlink(outside.path(), &parent).unwrap();
    assert!(
        tools
            .execute_prepared(prepared, &TurnCancellation::default())
            .await
            .is_err()
    );
    assert!(!outside.path().join("blocked").exists());
    let outside_scope = tools.prepare_call(
        &call("listDirectory", json!({})),
        ExecutionMode::Audit {
            scope: outside.path(),
            allow_mutations: true,
        },
    );
    assert!(matches!(outside_scope.decision(), PolicyDecision::Deny(_)));
}

#[cfg(unix)]
#[tokio::test]
async fn allowed_reads_cannot_gain_sensitive_access_when_the_target_changes() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    fs::write(root.path().join("public"), "public").unwrap();
    fs::write(root.path().join(".env"), "must not read").unwrap();
    let alias = root.path().join("config");
    symlink("public", &alias).unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let mut prepared = tools.prepare_call(
        &call("Read", json!({"path":"config"})),
        ExecutionMode::Workspace,
    );
    assert_eq!(prepared.decision(), &PolicyDecision::Allow);
    // Approving an allowed call does not grant permission for a later sensitive target.
    prepared.approve();
    fs::remove_file(&alias).unwrap();
    symlink(".env", &alias).unwrap();
    let error = tools
        .execute_prepared(prepared, &TurnCancellation::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("requires user approval"));
}

#[tokio::test]
async fn sensitive_diffs_require_approval_before_exposing_file_contents() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".env"), "fake-secret-value\n").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    let prepared = tools.prepare_call(
        &call("generateDiff", json!({"path":".env", "content":""})),
        ExecutionMode::Workspace,
    );
    assert!(matches!(
        prepared.decision(),
        PolicyDecision::RequireApproval(_)
    ));
    assert!(
        tools
            .execute_prepared(prepared, &TurnCancellation::default())
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn file_content_reads_reject_symlink_changes_after_the_final_policy_check() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    fs::write(root.path().join(".env"), "fake-secret-value").unwrap();
    let tools = ToolRuntime::new(root.path().to_path_buf()).unwrap();
    for name in ["readFile", "generateDiff"] {
        let target = root.path().join("public");
        fs::write(&target, "public").unwrap();
        let args = if name == "generateDiff" {
            json!({"path":"public", "content":""})
        } else {
            json!({"path":"public"})
        };
        let prepared = tools.prepare_call(&call(name, args), ExecutionMode::Workspace);
        assert_eq!(prepared.decision(), &PolicyDecision::Allow);
        fs::remove_file(&target).unwrap();
        symlink(".env", &target).unwrap();
        // Exercise the read itself after the policy check, without a second policy evaluation.
        let result = if name == "readFile" {
            tools.read_file(&prepared.call().arguments).await
        } else {
            tools.generate_diff(&prepared.call().arguments).await
        };
        assert!(result.is_err(), "{name} followed a replacement symlink");
        fs::remove_file(&target).unwrap();
    }
}
