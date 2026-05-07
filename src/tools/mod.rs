use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, anyhow};
use globset::Glob;
use regex::Regex;
use serde_json::{Value, json};
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;
use walkdir::WalkDir;

use crate::models::ToolCall;

const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;

pub struct ToolRuntime {
    workspace_root: PathBuf,
}

impl ToolRuntime {
    pub fn new(workspace_root: PathBuf) -> Result<Self> {
        let canonical = workspace_root.canonicalize().map_err(|err| {
            anyhow!(
                "Failed to resolve workspace root '{}': {}",
                workspace_root.display(),
                err
            )
        })?;

        Ok(Self {
            workspace_root: canonical,
        })
    }

    pub fn arg_summary(&self, name: &str, args: &Value) -> String {
        let key = match name {
            "readFile" => "path",
            "listDirectory" => "path",
            "searchFiles" => "pattern",
            "grep" => "pattern",
            "runCommand" => "command",
            "fileInfo" => "path",
            "generateDiff" => "path",
            _ => "",
        };

        if key.is_empty() {
            return serde_json::to_string(args)
                .unwrap_or_else(|_| "{}".to_string())
                .chars()
                .take(80)
                .collect();
        }

        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<Value> {
        match call.name.as_str() {
            "readFile" => self.read_file(&call.arguments).await,
            "listDirectory" => self.list_directory(&call.arguments).await,
            "searchFiles" => self.search_files(&call.arguments).await,
            "grep" => self.grep(&call.arguments).await,
            "runCommand" => self.run_command(&call.arguments).await,
            "fileInfo" => self.file_info(&call.arguments).await,
            "generateDiff" => self.generate_diff(&call.arguments).await,
            other => Err(anyhow!("Unknown tool '{}'", other)),
        }
    }

    async fn read_file(&self, args: &Value) -> Result<Value> {
        let path = required_str(args, "path")?;
        let max_lines = args
            .get("maxLines")
            .and_then(Value::as_u64)
            .map(|v| v as usize);

        let resolved = self.resolve_in_workspace(path)?;
        let content = fs::read_to_string(&resolved).await?;
        let total_lines = content.lines().count();

        let output = if let Some(limit) = max_lines {
            content
                .lines()
                .take(limit)
                .collect::<Vec<&str>>()
                .join("\n")
        } else {
            content.clone()
        };

        let truncated = max_lines.map(|v| total_lines > v).unwrap_or(false);

        Ok(json!({
            "path": resolved.display().to_string(),
            "content": output,
            "totalLines": total_lines,
            "truncated": truncated,
        }))
    }

    async fn list_directory(&self, args: &Value) -> Result<Value> {
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");

        let resolved = self.resolve_in_workspace(path)?;
        let mut entries = fs::read_dir(&resolved).await?;
        let mut out = Vec::<Value>::new();

        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            out.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "type": if file_type.is_dir() { "directory" } else { "file" },
            }));
        }

        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

        Ok(json!({
            "path": resolved.display().to_string(),
            "entries": out,
        }))
    }

    async fn search_files(&self, args: &Value) -> Result<Value> {
        let pattern = required_str(args, "pattern")?;
        let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");

        let base = self.resolve_in_workspace(cwd)?;
        let matcher = Glob::new(pattern)
            .map_err(|err| anyhow!("Invalid glob pattern '{}': {}", pattern, err))?
            .compile_matcher();

        let mut matches = Vec::<String>::new();
        for entry in WalkDir::new(&base).follow_links(false) {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let rel = match entry.path().strip_prefix(&base) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if matcher.is_match(rel) {
                matches.push(rel.display().to_string());
                if matches.len() >= 100 {
                    break;
                }
            }
        }

        Ok(json!({
            "pattern": pattern,
            "cwd": base.display().to_string(),
            "matches": matches,
            "truncated": matches.len() >= 100,
        }))
    }

    async fn grep(&self, args: &Value) -> Result<Value> {
        let pattern = required_str(args, "pattern")?;
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let glob = args.get("glob").and_then(Value::as_str);

        let resolved = self.resolve_in_workspace(path)?;
        let regex = Regex::new(pattern)
            .map_err(|err| anyhow!("Invalid regex pattern '{}': {}", pattern, err))?;
        let glob_matcher = match glob {
            Some(g) => Some(
                Glob::new(g)
                    .map_err(|err| anyhow!("Invalid file glob '{}': {}", g, err))?
                    .compile_matcher(),
            ),
            None => None,
        };

        let mut results = Vec::<Value>::new();
        let walker = if resolved.is_file() {
            WalkDir::new(resolved.parent().unwrap_or(self.workspace_root.as_path()))
        } else {
            WalkDir::new(&resolved)
        };

        for entry in walker.follow_links(false) {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };

            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }

            if resolved.is_file() && path != resolved {
                continue;
            }

            if let Some(matcher) = &glob_matcher {
                let rel = match path.strip_prefix(&self.workspace_root) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if !matcher.is_match(rel) {
                    continue;
                }
            }

            let content = match fs::read_to_string(path).await {
                Ok(v) => v,
                Err(_) => continue,
            };

            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(json!({
                        "file": path.display().to_string(),
                        "line": index + 1,
                        "text": line,
                    }));
                    if results.len() >= 50 {
                        break;
                    }
                }
            }

            if results.len() >= 50 {
                break;
            }
        }

        Ok(json!({
            "pattern": pattern,
            "matches": results,
            "totalMatches": results.len(),
        }))
    }

    async fn run_command(&self, args: &Value) -> Result<Value> {
        let command = required_str(args, "command")?;
        let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");
        let timeout_ms = args
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS)
            .clamp(1, MAX_COMMAND_TIMEOUT_MS);

        let resolved_cwd = self.resolve_in_workspace(cwd)?;

        let mut command_builder = Command::new("sh");
        command_builder
            .arg("-c")
            .arg(command)
            .current_dir(resolved_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output =
            match timeout(Duration::from_millis(timeout_ms), command_builder.output()).await {
                Ok(result) => result?,
                Err(_) => {
                    return Ok(json!({
                        "stdout": "",
                        "stderr": "Command timed out",
                        "exitCode": -1,
                        "truncated": false,
                    }));
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let truncated = stdout.len() > 10_000 || stderr.len() > 5_000;
        let stdout_out = truncate_string(&stdout, 10_000);
        let stderr_out = truncate_string(&stderr, 5_000);

        Ok(json!({
            "stdout": stdout_out,
            "stderr": stderr_out,
            "exitCode": output.status.code().unwrap_or(-1),
            "truncated": truncated,
        }))
    }

    async fn file_info(&self, args: &Value) -> Result<Value> {
        let path = required_str(args, "path")?;
        let resolved = self.resolve_existing_or_missing_in_workspace(path)?;

        let metadata = match fs::metadata(&resolved).await {
            Ok(v) => v,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Ok(json!({
                    "path": resolved.display().to_string(),
                    "exists": false,
                    "type": null,
                    "size": null,
                    "modified": null,
                }));
            }
            Err(err) => return Err(err.into()),
        };
        let kind = if metadata.is_dir() {
            "directory"
        } else {
            "file"
        };

        Ok(json!({
            "path": resolved.display().to_string(),
            "exists": true,
            "type": kind,
            "size": metadata.len(),
            "modified": metadata.modified().ok().map(|t| {
                chrono_like(t)
            }),
        }))
    }

    async fn generate_diff(&self, args: &Value) -> Result<Value> {
        let path = required_str(args, "path")?;
        let proposed = required_str(args, "content")?;
        let resolved = self.resolve_in_workspace(path)?;
        let original = fs::read_to_string(&resolved).await?;
        let diff = unified_diff(path, &original, proposed);

        Ok(json!({
            "path": resolved.display().to_string(),
            "diff": diff,
        }))
    }

    fn resolve_in_workspace(&self, input: &str) -> Result<PathBuf> {
        let absolute = self.absolute_input_path(input);

        let canonical = absolute
            .canonicalize()
            .map_err(|err| anyhow!("Path '{}' is not accessible: {}", absolute.display(), err))?;

        if !canonical.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Path '{}' is outside workspace root '{}'",
                canonical.display(),
                self.workspace_root.display()
            ));
        }

        Ok(canonical)
    }

    fn resolve_existing_or_missing_in_workspace(&self, input: &str) -> Result<PathBuf> {
        let absolute = self.absolute_input_path(input);

        match absolute.canonicalize() {
            Ok(canonical) => {
                self.ensure_in_workspace(&canonical)?;
                Ok(canonical)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                reject_parent_dir(input, &self.workspace_root)?;
                let (ancestor, tail) = nearest_existing_ancestor(&absolute)?;
                let canonical_ancestor = ancestor.canonicalize().map_err(|err| {
                    anyhow!("Path '{}' is not accessible: {}", ancestor.display(), err)
                })?;

                self.ensure_in_workspace(&canonical_ancestor)?;
                Ok(canonical_ancestor.join(tail))
            }
            Err(err) => Err(anyhow!(
                "Path '{}' is not accessible: {}",
                absolute.display(),
                err
            )),
        }
    }

    fn absolute_input_path(&self, input: &str) -> PathBuf {
        let path = Path::new(input);
        if path.is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace_root.join(path)
        }
    }

    fn ensure_in_workspace(&self, canonical: &Path) -> Result<()> {
        if !canonical.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Path '{}' is outside workspace root '{}'",
                canonical.display(),
                self.workspace_root.display()
            ));
        }

        Ok(())
    }
}

fn reject_parent_dir(input: &str, workspace_root: &Path) -> Result<()> {
    if Path::new(input)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!(
            "Path '{}' is outside workspace root '{}'",
            input,
            workspace_root.display()
        ));
    }

    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let mut ancestor = path.to_path_buf();
    let mut tail = PathBuf::new();

    loop {
        if ancestor.exists() {
            return Ok((ancestor, tail));
        }

        let Some(name) = ancestor.file_name().map(|v| v.to_os_string()) else {
            break;
        };

        let mut next_tail = PathBuf::from(name);
        if !tail.as_os_str().is_empty() {
            next_tail.push(tail);
        }
        tail = next_tail;

        if !ancestor.pop() {
            break;
        }
    }

    Err(anyhow!(
        "Path '{}' is not accessible: no existing ancestor",
        path.display()
    ))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("Missing required string argument '{}'", key))
}

fn truncate_string(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

fn unified_diff(path: &str, original: &str, proposed: &str) -> String {
    if original == proposed {
        return format!("--- a/{path}\n+++ b/{path}\n");
    }

    let old = original.lines().collect::<Vec<&str>>();
    let new = proposed.lines().collect::<Vec<&str>>();
    let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];

    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = vec![
        format!("--- a/{path}"),
        format!("+++ b/{path}"),
        format!("@@ -1,{} +1,{} @@", old.len(), new.len()),
    ];

    let mut i = 0;
    let mut j = 0;
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            out.push(format!(" {}", old[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(format!("-{}", old[i]));
            i += 1;
        } else {
            out.push(format!("+{}", new[j]));
            j += 1;
        }
    }

    while i < old.len() {
        out.push(format!("-{}", old[i]));
        i += 1;
    }
    while j < new.len() {
        out.push(format!("+{}", new[j]));
        j += 1;
    }

    out.join("\n")
}

fn chrono_like(time: std::time::SystemTime) -> String {
    let datetime = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0);
    format!("{}", datetime)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::ToolRuntime;
    use super::unified_diff;
    use crate::models::ToolCall;

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
}
