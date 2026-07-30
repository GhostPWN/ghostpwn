use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::ToolRuntime;
use super::audit_tool_allowed;
use super::command_shell;
use super::decode_duckduckgo_url;
use super::parse_cargo_lock;
use super::parse_duckduckgo_results;
use super::tool_requires_approval;
use super::unified_diff;
use super::url_encode_query;
use crate::models::ToolCall;

#[cfg(windows)]
fn shell_cwd_display(path: &std::path::Path) -> String {
    let display = path.to_string_lossy();
    display
        .as_ref()
        .strip_prefix(r"\\?\")
        .unwrap_or(display.as_ref())
        .to_string()
}

#[cfg(not(windows))]
fn shell_cwd_display(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(windows)]
#[test]
fn command_shell_uses_powershell_on_windows() {
    let invocation = command_shell("Write-Output hello");

    assert_eq!(invocation.program, "powershell.exe");
    assert_eq!(
        invocation.args,
        vec![
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Write-Output hello",
        ]
    );
}

#[cfg(not(windows))]
#[test]
fn command_shell_uses_sh_on_unix() {
    let invocation = command_shell("printf hello");

    assert_eq!(invocation.program, "sh");
    assert_eq!(invocation.args, vec!["-c", "printf hello"]);
}

#[test]
fn mutations_and_commands_require_approval() {
    assert!(tool_requires_approval("runCommand"));
    assert!(tool_requires_approval("auditDependencies"));
    assert!(tool_requires_approval("Bash"));
    assert!(tool_requires_approval("apply_patch"));
    assert!(!tool_requires_approval("readFile"));
    assert!(!tool_requires_approval("webFetch"));
}

#[test]
fn audit_mode_allows_only_scoped_read_tools_and_dependency_audit() {
    assert!(audit_tool_allowed("readFile", false));
    assert!(audit_tool_allowed("auditDependencies", false));
    assert!(!audit_tool_allowed("runCommand", false));
    assert!(!audit_tool_allowed("webFetch", false));
    assert!(!audit_tool_allowed("writeFile", false));

    for tool in [
        "generateDiff",
        "writeFile",
        "editFile",
        "multiEdit",
        "applyPatch",
    ] {
        assert!(audit_tool_allowed(tool, true));
    }
}

#[tokio::test]
async fn audit_mode_rejects_tools_and_paths_outside_scope() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    let scope = workspace.join("src");
    fs::create_dir_all(&scope).expect("scope");
    fs::write(scope.join("inside.rs"), "safe").expect("inside");
    fs::write(workspace.join("outside.rs"), "outside").expect("outside");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let scope = scope.canonicalize().expect("canonical scope");
    let blocked_tool = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({"url": "https://example.com"}),
    };
    assert!(
        tools
            .execute_audit(&blocked_tool, &scope, false)
            .await
            .expect_err("web must be blocked")
            .to_string()
            .contains("unavailable")
    );

    let outside_read = ToolCall {
        name: "readFile".to_string(),
        arguments: json!({"path": root.path().join("workspace/outside.rs").display().to_string()}),
    };
    assert!(
        tools
            .execute_audit(&outside_read, &scope, false)
            .await
            .expect_err("outside read must be blocked")
            .to_string()
            .contains("outside audit scope")
    );

    let inside_read = ToolCall {
        name: "readFile".to_string(),
        arguments: json!({"path": scope.join("inside.rs").display().to_string()}),
    };
    assert!(
        tools
            .execute_audit(&inside_read, &scope, false)
            .await
            .is_ok()
    );

    let default_list = ToolCall {
        name: "listDirectory".to_string(),
        arguments: json!({}),
    };
    let result = tools
        .execute_audit(&default_list, &scope, false)
        .await
        .expect("scoped default");
    assert_eq!(result["path"].as_str(), scope.to_str());

    let inside_write = ToolCall {
        name: "writeFile".to_string(),
        arguments: json!({"path": scope.join("fixed.rs").display().to_string(), "content": "fixed"}),
    };
    assert!(
        tools
            .execute_audit(&inside_write, &scope, true)
            .await
            .is_ok()
    );

    let outside_write = ToolCall {
        name: "writeFile".to_string(),
        arguments: json!({
            "path": root.path().join("workspace/not-scoped.rs").display().to_string(),
            "content": "blocked",
        }),
    };
    assert!(
        tools
            .execute_audit(&outside_write, &scope, true)
            .await
            .expect_err("outside write must be blocked")
            .to_string()
            .contains("outside audit scope")
    );

    let outside_patch = ToolCall {
        name: "applyPatch".to_string(),
        arguments: json!({
            "patch": "*** Begin Patch\n*** Add File: not-scoped.rs\n+blocked\n*** End Patch"
        }),
    };
    assert!(
        tools
            .execute_audit(&outside_patch, &scope, true)
            .await
            .expect_err("outside patch must be blocked")
            .to_string()
            .contains("outside audit scope")
    );
}

#[test]
fn resolves_audit_paths_and_rejects_missing_path_like_targets() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).expect("workspace");
    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");

    let (scope, _) = tools.resolve_audit_scope("src").expect("existing path");
    assert_eq!(scope, workspace.join("src").canonicalize().expect("scope"));
    assert!(tools.resolve_audit_scope("missing/file.rs").is_err());

    let (focus_scope, label) = tools.resolve_audit_scope("authentication").expect("focus");
    assert_eq!(focus_scope, workspace.canonicalize().expect("workspace"));
    assert!(label.contains("authentication"));
}

#[test]
fn parses_rust_packages_from_cargo_lock() {
    let packages = parse_cargo_lock(
        "version = 4\n\n[[package]]\nname = \"alpha\"\nversion = \"1.2.3\"\n\
         \n[[package]]\nname = \"beta\"\nversion = \"2.0.0\"\n",
    );

    assert_eq!(
        packages,
        vec![
            ("alpha".to_string(), "1.2.3".to_string()),
            ("beta".to_string(), "2.0.0".to_string()),
        ]
    );
}

#[tokio::test]
async fn run_command_executes_through_platform_shell() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let command = if cfg!(windows) {
        "Write-Output hello"
    } else {
        "printf hello"
    };
    let call = ToolCall {
        name: "runCommand".to_string(),
        arguments: json!({
            "command": command,
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("exitCode").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(
        result.get("stdout").and_then(|v| v.as_str()).map(str::trim),
        Some("hello")
    );
}

#[tokio::test]
async fn run_command_uses_workspace_cwd() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let expected_cwd = shell_cwd_display(&workspace.canonicalize().expect("canonical workspace"));

    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");
    let command = if cfg!(windows) {
        "[System.IO.Directory]::GetCurrentDirectory()"
    } else {
        "pwd"
    };
    let call = ToolCall {
        name: "runCommand".to_string(),
        arguments: json!({
            "command": command,
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("exitCode").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(
        result.get("stdout").and_then(|v| v.as_str()).map(str::trim),
        Some(expected_cwd.as_str())
    );
}

#[tokio::test]
async fn run_command_reports_timeout() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let command = if cfg!(windows) {
        "Start-Sleep -Seconds 5"
    } else {
        "sleep 5"
    };
    let call = ToolCall {
        name: "runCommand".to_string(),
        arguments: json!({
            "command": command,
            "timeout": 50,
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("exitCode").and_then(|v| v.as_i64()), Some(-1));
    assert_eq!(
        result.get("stderr").and_then(|v| v.as_str()),
        Some("Command timed out")
    );
}

#[tokio::test]
async fn read_file_truncates_and_counts_lines() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let file = workspace.join("notes.txt");
    fs::write(&file, "line1\nline2\nline3\n").expect("write file");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "readFile".to_string(),
        arguments: json!({
            "path": "notes.txt",
            "maxLines": 2,
            "lineNumbers": true,
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("totalLines").and_then(|v| v.as_u64()), Some(3));
    assert_eq!(
        result.get("truncated").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("content").and_then(|v| v.as_str()),
        Some("line1\nline2")
    );
    assert_eq!(
        result.get("numberedContent").and_then(|v| v.as_str()),
        Some("1: line1\n2: line2")
    );
    assert_eq!(result.get("startLine").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(result.get("endLine").and_then(|v| v.as_u64()), Some(2));
}

#[tokio::test]
async fn search_and_grep_report_truncation() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    for index in 0..=100 {
        fs::write(workspace.join(format!("{index:03}.txt")), "match\n").expect("file");
    }

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let search = tools
        .execute(&ToolCall {
            name: "searchFiles".to_string(),
            arguments: json!({"pattern": "*.txt"}),
        })
        .await
        .expect("search");
    assert_eq!(search["matches"].as_array().map(Vec::len), Some(100));
    assert_eq!(search["truncated"].as_bool(), Some(true));

    let grep = tools
        .execute(&ToolCall {
            name: "grep".to_string(),
            arguments: json!({"pattern": "match", "glob": "*.txt"}),
        })
        .await
        .expect("grep");
    assert_eq!(grep["matches"].as_array().map(Vec::len), Some(50));
    assert_eq!(grep["truncated"].as_bool(), Some(true));
}

#[tokio::test]
async fn rejects_paths_outside_workspace() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let outside = root.path().join("outside.txt");
    fs::write(&outside, "secret").expect("outside file");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "readFile".to_string(),
        arguments: json!({
            "path": "../outside.txt"
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject traversal");
    let msg = err.to_string();
    assert!(msg.contains("outside workspace root"));
}

#[tokio::test]
async fn file_info_reports_missing_paths_inside_workspace() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "fileInfo".to_string(),
        arguments: json!({
            "path": "missing.txt"
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("exists").and_then(|v| v.as_bool()), Some(false));
    assert!(result.get("type").is_some_and(|v| v.is_null()));
}

#[tokio::test]
async fn file_info_rejects_missing_paths_outside_workspace() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "fileInfo".to_string(),
        arguments: json!({
            "path": "../missing.txt"
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject traversal");
    assert!(err.to_string().contains("outside workspace root"));
}

#[tokio::test]
async fn generate_diff_returns_unified_patch() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("app.txt"), "one\ntwo\n").expect("write file");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "generateDiff".to_string(),
        arguments: json!({
            "path": "app.txt",
            "content": "one\nthree\n"
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    let diff = result.get("diff").and_then(|v| v.as_str()).unwrap();
    assert!(diff.contains("--- a/app.txt"));
    assert!(diff.contains("-two"));
    assert!(diff.contains("+three"));
}

#[test]
fn unified_diff_handles_equal_content() {
    assert_eq!(
        unified_diff("same.txt", "same\n", "same\n"),
        "--- a/same.txt\n+++ b/same.txt\n"
    );
}

#[tokio::test]
async fn write_file_creates_parent_inside_workspace() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");
    let call = ToolCall {
        name: "Write".to_string(),
        arguments: json!({
            "file_path": "src/app.txt",
            "content": "hello"
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("written").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        fs::read_to_string(workspace.join("src/app.txt")).expect("read file"),
        "hello"
    );
}

#[tokio::test]
async fn write_file_rejects_parent_traversal() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "writeFile".to_string(),
        arguments: json!({
            "path": "../outside.txt",
            "content": "nope"
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject traversal");
    assert!(err.to_string().contains("outside workspace root"));
}

#[tokio::test]
async fn edit_file_replaces_exact_match() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("app.txt"), "one\ntwo\n").expect("write file");

    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");
    let call = ToolCall {
        name: "Edit".to_string(),
        arguments: json!({
            "file_path": "app.txt",
            "old_string": "two",
            "new_string": "three"
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result.get("replacements").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(
        fs::read_to_string(workspace.join("app.txt")).expect("read file"),
        "one\nthree\n"
    );
}

#[tokio::test]
async fn edit_file_rejects_ambiguous_match() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("app.txt"), "same\nsame\n").expect("write file");

    let tools = ToolRuntime::new(workspace).expect("runtime");
    let call = ToolCall {
        name: "editFile".to_string(),
        arguments: json!({
            "path": "app.txt",
            "oldString": "same",
            "newString": "next"
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject ambiguous edit");
    assert!(err.to_string().contains("matched 2 times"));
}

#[tokio::test]
async fn multi_edit_does_not_write_when_later_edit_fails() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("app.txt"), "alpha\nbeta\n").expect("write file");

    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");
    let call = ToolCall {
        name: "MultiEdit".to_string(),
        arguments: json!({
            "path": "app.txt",
            "edits": [
                { "oldString": "alpha", "newString": "one" },
                { "oldString": "missing", "newString": "two" }
            ]
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject failed multi edit");
    assert!(err.to_string().contains("oldString was not found"));
    assert_eq!(
        fs::read_to_string(workspace.join("app.txt")).expect("read file"),
        "alpha\nbeta\n"
    );
}

#[tokio::test]
async fn apply_patch_add_update_delete_and_move() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(workspace.join("app.txt"), "one\ntwo\n").expect("write file");
    fs::write(workspace.join("old.txt"), "move me\n").expect("write file");
    fs::write(workspace.join("gone.txt"), "delete me\n").expect("write file");

    let tools = ToolRuntime::new(workspace.clone()).expect("runtime");
    let call = ToolCall {
        name: "apply_patch".to_string(),
        arguments: json!({
            "patch": "*** Begin Patch\n*** Add File: new.txt\n+created\n*** Update File: app.txt\n@@\n one\n-two\n+three\n*** Update File: old.txt\n*** Move to: moved.txt\n@@\n move me\n*** Delete File: gone.txt\n*** End Patch"
        }),
    };

    let result = tools.execute(&call).await.expect("tool result");
    assert_eq!(result["changed"].as_array().map(Vec::len), Some(5));
    assert_eq!(
        fs::read_to_string(workspace.join("new.txt")).expect("new file"),
        "created\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("app.txt")).expect("app file"),
        "one\nthree\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("moved.txt")).expect("moved file"),
        "move me\n"
    );
    assert!(!workspace.join("old.txt").exists());
    assert!(!workspace.join("gone.txt").exists());
}

#[tokio::test]
async fn skill_tools_search_and_read_local_skills() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    let skills_root = workspace.join("skills");
    let skill_dir = skills_root.join("directory-traversal");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: directory-traversal\ndescription: Testing path traversal in web applications\n---\n# Workflow\n",
    )
    .expect("skill");

    let tools = ToolRuntime::new_with_skills_root(workspace, skills_root).expect("runtime");
    let search = ToolCall {
        name: "SearchSkills".to_string(),
        arguments: json!({
            "query": "path traversal web app"
        }),
    };
    let result = tools.execute(&search).await.expect("search skills");
    assert_eq!(
        result["matches"][0]["name"].as_str(),
        Some("directory-traversal")
    );

    let read = ToolCall {
        name: "readSkill".to_string(),
        arguments: json!({
            "name": "directory-traversal"
        }),
    };
    let result = tools.execute(&read).await.expect("read skill");
    assert_eq!(result["name"].as_str(), Some("directory-traversal"));
    assert!(
        result["content"]
            .as_str()
            .is_some_and(|v| v.contains("# Workflow"))
    );
}

#[tokio::test]
async fn web_fetch_rejects_loopback_ip() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({ "url": "http://127.0.0.1/" }),
    };
    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject loopback");
    assert!(err.to_string().contains("not a public address"));
}

#[tokio::test]
async fn web_fetch_rejects_metadata_ip() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({ "url": "http://169.254.169.254/latest/meta-data/" }),
    };
    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject link-local");
    assert!(err.to_string().contains("not a public address"));
}

#[tokio::test]
async fn web_fetch_rejects_private_rfc1918() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({ "url": "http://10.0.0.1/" }),
    };
    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject rfc1918");
    assert!(err.to_string().contains("not a public address"));
}

#[tokio::test]
async fn web_fetch_rejects_localhost_hostname() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({ "url": "http://localhost:8080/admin" }),
    };
    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject localhost");
    assert!(err.to_string().contains("not a public address"));
}

#[tokio::test]
async fn web_fetch_rejects_ipv6_loopback() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({ "url": "http://[::1]/" }),
    };
    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject ipv6 loopback");
    assert!(err.to_string().contains("not a public address"));
}

#[test]
fn ssrf_helper_classifies_ip_ranges() {
    use super::is_public_ip;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255))));
    assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));

    assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
    assert!(!is_public_ip(IpAddr::V6("fc00::1".parse().unwrap())));
    assert!(!is_public_ip(IpAddr::V6("fe80::1".parse().unwrap())));
    assert!(is_public_ip(IpAddr::V6(
        "2606:4700:4700::1111".parse().unwrap()
    )));
}

#[tokio::test]
async fn web_fetch_rejects_non_http_url() {
    let root = tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let tools = ToolRuntime::new(workspace).expect("runtime");

    let call = ToolCall {
        name: "webFetch".to_string(),
        arguments: json!({
            "url": "file:///etc/passwd"
        }),
    };

    let err = tools
        .execute(&call)
        .await
        .expect_err("should reject scheme");
    assert!(err.to_string().contains("Unsupported URL scheme"));
}

#[test]
fn duckduckgo_parser_extracts_result_and_redirect() {
    let html = r#"
        <div class="result">
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs%3Fa%3D1&amp;rut=abc">Example &amp; Docs</a>
            <a class="result__snippet">Useful <b>docs</b> here.</a>
        </div>
    "#;

    let results = parse_duckduckgo_results(html, 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Example & Docs");
    assert_eq!(results[0].url, "https://example.com/docs?a=1");
    assert_eq!(results[0].snippet, "Useful docs here.");
}

#[test]
fn duckduckgo_url_decode_handles_direct_and_encoded_urls() {
    assert_eq!(
        decode_duckduckgo_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com"),
        "https://example.com"
    );
    assert_eq!(
        decode_duckduckgo_url("https://example.com?a=1&amp;b=2"),
        "https://example.com?a=1&b=2"
    );
}

#[test]
fn url_encode_query_encodes_spaces_and_unicode() {
    assert_eq!(url_encode_query("rust tui"), "rust+tui");
    assert_eq!(url_encode_query("café"), "caf%C3%A9");
}
