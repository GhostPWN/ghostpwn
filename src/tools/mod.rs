use std::path::{Path, PathBuf};
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
            .unwrap_or(30_000);

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
        let resolved = self.resolve_in_workspace(path)?;

        let metadata = fs::metadata(&resolved).await?;
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

    fn resolve_in_workspace(&self, input: &str) -> Result<PathBuf> {
        let path = Path::new(input);
        let absolute = if path.is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace_root.join(path)
        };

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
}
