use std::io::ErrorKind;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use globset::Glob;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use walkdir::WalkDir;

use crate::models::ToolCall;
use crate::skills::SkillRuntime;

const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_WEB_FETCH_BYTES: usize = 1_000_000;
const MAX_WEB_FETCH_BYTES: usize = 5_000_000;
const MAX_WEB_SEARCH_RESULTS: usize = 10;

pub fn tool_requires_approval(name: &str) -> bool {
    matches!(
        canonical_tool_name(name),
        "runCommand" | "writeFile" | "editFile" | "multiEdit" | "applyPatch"
    )
}

pub struct ToolRuntime {
    workspace_root: PathBuf,
    http_client: Client,
    skills: SkillRuntime,
}

impl ToolRuntime {
    pub fn new(workspace_root: PathBuf) -> Result<Self> {
        Self::new_with_skills(workspace_root, SkillRuntime::new())
    }

    #[cfg(test)]
    fn new_with_skills_root(workspace_root: PathBuf, skills_root: PathBuf) -> Result<Self> {
        Self::new_with_skills(workspace_root, SkillRuntime::with_root(skills_root))
    }

    fn new_with_skills(workspace_root: PathBuf, skills: SkillRuntime) -> Result<Self> {
        let canonical = workspace_root.canonicalize().map_err(|err| {
            anyhow!(
                "Failed to resolve workspace root '{}': {}",
                workspace_root.display(),
                err
            )
        })?;

        let http_client = Client::builder()
            .user_agent("GhostPWN/0.1")
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("too many redirects");
                }
                match validate_public_url(attempt.url()) {
                    Ok(()) => attempt.follow(),
                    Err(err) => attempt.error(err),
                }
            }))
            .build()?;

        Ok(Self {
            skills,
            workspace_root: canonical,
            http_client,
        })
    }

    pub async fn prompt_skill_section(&self) -> String {
        self.skills.prompt_section().await
    }

    pub fn arg_summary(&self, name: &str, args: &Value) -> String {
        let key = match canonical_tool_name(name) {
            "listSkills" => "",
            "searchSkills" => "query",
            "readSkill" => "name",
            "readFile" => "path",
            "listDirectory" => "path",
            "searchFiles" => "pattern",
            "grep" => "pattern",
            "runCommand" => "command",
            "fileInfo" => "path",
            "generateDiff" => "path",
            "writeFile" => "path",
            "editFile" => "path",
            "multiEdit" => "path",
            "applyPatch" => "patchText",
            "webFetch" => "url",
            "webSearch" => "query",
            _ => "",
        };

        if key.is_empty() {
            return serde_json::to_string(args)
                .unwrap_or_else(|_| "{}".to_string())
                .chars()
                .take(80)
                .collect();
        }

        if key == "path" {
            return path_arg(args).unwrap_or_default().to_string();
        }

        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<Value> {
        match canonical_tool_name(&call.name) {
            "listSkills" => self.skills.list_tool().await,
            "searchSkills" => self.skills.search_tool(&call.arguments).await,
            "readSkill" => self.skills.read_tool(&call.arguments).await,
            "readFile" => self.read_file(&call.arguments).await,
            "listDirectory" => self.list_directory(&call.arguments).await,
            "searchFiles" => self.search_files(&call.arguments).await,
            "grep" => self.grep(&call.arguments).await,
            "runCommand" => self.run_command(&call.arguments).await,
            "fileInfo" => self.file_info(&call.arguments).await,
            "generateDiff" => self.generate_diff(&call.arguments).await,
            "writeFile" => self.write_file(&call.arguments).await,
            "editFile" => self.edit_file(&call.arguments).await,
            "multiEdit" => self.multi_edit(&call.arguments).await,
            "applyPatch" => self.apply_patch(&call.arguments).await,
            "webFetch" => self.web_fetch(&call.arguments).await,
            "webSearch" => self.web_search(&call.arguments).await,
            other => Err(anyhow!("Unknown tool '{}'", other)),
        }
    }

    async fn read_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let limit = args
            .get("maxLines")
            .or_else(|| args.get("limit"))
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let offset = args
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(0);

        let resolved = self.resolve_in_workspace(path)?;
        let content = fs::read_to_string(&resolved).await?;
        let total_lines = content.lines().count();

        let output = if let Some(limit) = limit {
            content
                .lines()
                .skip(offset)
                .take(limit)
                .collect::<Vec<&str>>()
                .join("\n")
        } else if offset > 0 {
            content
                .lines()
                .skip(offset)
                .collect::<Vec<&str>>()
                .join("\n")
        } else {
            content.clone()
        };

        let truncated = limit
            .map(|v| total_lines > offset.saturating_add(v))
            .unwrap_or(false);

        Ok(json!({
            "path": resolved.display().to_string(),
            "content": output,
            "totalLines": total_lines,
            "truncated": truncated,
        }))
    }

    async fn list_directory(&self, args: &Value) -> Result<Value> {
        let path = path_arg(args).unwrap_or(".");

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
        let cwd = args
            .get("cwd")
            .or_else(|| args.get("path"))
            .and_then(Value::as_str)
            .unwrap_or(".");

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

        let invocation = command_shell(command);
        let mut command_builder = Command::new(invocation.program);
        command_builder
            .args(invocation.args)
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
        let path = required_path(args)?;
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
        let path = required_path(args)?;
        let proposed = required_str(args, "content")?;
        let resolved = self.resolve_in_workspace(path)?;
        let original = fs::read_to_string(&resolved).await?;
        let diff = unified_diff(path, &original, proposed);

        Ok(json!({
            "path": resolved.display().to_string(),
            "diff": diff,
        }))
    }

    async fn write_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let content = required_str(args, "content")?;
        let resolved = self.resolve_existing_or_missing_in_workspace(path)?;

        if let Some(parent) = resolved.parent() {
            let parent =
                self.resolve_existing_or_missing_in_workspace(&parent.display().to_string())?;
            fs::create_dir_all(parent).await?;
        }

        fs::write(&resolved, content).await?;

        Ok(json!({
            "path": resolved.display().to_string(),
            "bytes": content.len(),
            "written": true,
        }))
    }

    async fn edit_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let old = required_any_str(args, &["oldString", "old_string"])?;
        let new = required_any_str(args, &["newString", "new_string"])?;
        let replace_all = bool_arg(args, &["replaceAll", "replace_all"]).unwrap_or(false);

        let resolved = self.resolve_in_workspace(path)?;
        let content = fs::read_to_string(&resolved).await?;
        let (updated, replacements) = apply_string_edit(&content, old, new, replace_all)?;
        fs::write(&resolved, updated).await?;

        Ok(json!({
            "path": resolved.display().to_string(),
            "replacements": replacements,
        }))
    }

    async fn multi_edit(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let edits = args
            .get("edits")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Missing required array argument 'edits'"))?;

        let resolved = self.resolve_in_workspace(path)?;
        let mut content = fs::read_to_string(&resolved).await?;
        let mut total = 0usize;

        for edit in edits {
            let old = required_any_str(edit, &["oldString", "old_string"])?;
            let new = required_any_str(edit, &["newString", "new_string"])?;
            let replace_all = bool_arg(edit, &["replaceAll", "replace_all"]).unwrap_or(false);
            let (next, replacements) = apply_string_edit(&content, old, new, replace_all)?;
            content = next;
            total += replacements;
        }

        fs::write(&resolved, content).await?;

        Ok(json!({
            "path": resolved.display().to_string(),
            "edits": edits.len(),
            "replacements": total,
        }))
    }

    async fn apply_patch(&self, args: &Value) -> Result<Value> {
        let patch_text = required_any_str(args, &["patchText", "patch_text", "patch"])?;
        let ops = parse_apply_patch(patch_text)?;
        let mut actions = Vec::<PatchAction>::new();

        for op in ops {
            match op {
                PatchOp::Add { path, content } => {
                    let resolved = self.resolve_existing_or_missing_in_workspace(&path)?;
                    if resolved.exists() {
                        return Err(anyhow!("Cannot add '{}': file already exists", path));
                    }
                    actions.push(PatchAction::Write {
                        path: resolved,
                        content,
                        create_new: true,
                    });
                }
                PatchOp::Delete { path } => {
                    let resolved = self.resolve_in_workspace(&path)?;
                    actions.push(PatchAction::Delete { path: resolved });
                }
                PatchOp::Update {
                    path,
                    move_to,
                    changes,
                } => {
                    let source = self.resolve_in_workspace(&path)?;
                    let original = fs::read_to_string(&source).await?;
                    let updated = apply_patch_lines(&original, &changes)?;
                    let target = if let Some(move_to) = move_to {
                        let target = self.resolve_existing_or_missing_in_workspace(&move_to)?;
                        if target.exists() && target != source {
                            return Err(anyhow!(
                                "Cannot move to '{}': file already exists",
                                move_to
                            ));
                        }
                        target
                    } else {
                        source.clone()
                    };

                    let create_new = target != source;
                    if create_new {
                        actions.push(PatchAction::Delete { path: source });
                    }
                    actions.push(PatchAction::Write {
                        path: target,
                        content: updated,
                        create_new,
                    });
                }
            }
        }

        let mut changed = Vec::<String>::new();
        for action in actions {
            match action {
                PatchAction::Write {
                    path,
                    content,
                    create_new,
                } => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).await?;
                    }
                    if create_new {
                        let mut file = OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                            .await
                            .map_err(|err| anyhow!("Cannot add '{}': {}", path.display(), err))?;
                        file.write_all(content.as_bytes()).await?;
                        file.flush().await?;
                    } else {
                        fs::write(&path, content).await?;
                    }
                    changed.push(path.display().to_string());
                }
                PatchAction::Delete { path } => {
                    fs::remove_file(&path).await?;
                    changed.push(path.display().to_string());
                }
            }
        }

        Ok(json!({
            "changed": changed,
        }))
    }

    async fn web_fetch(&self, args: &Value) -> Result<Value> {
        let url = required_str(args, "url")?;
        let max_bytes = args
            .get("maxBytes")
            .or_else(|| args.get("max_bytes"))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_WEB_FETCH_BYTES)
            .clamp(1, MAX_WEB_FETCH_BYTES);

        let body = self.fetch_url_limited(url, max_bytes).await?;
        Ok(json!({
            "url": body.url,
            "status": body.status,
            "content": body.content,
            "bytes": body.bytes,
            "truncated": body.truncated,
        }))
    }

    async fn web_search(&self, args: &Value) -> Result<Value> {
        let query = required_str(args, "query")?;
        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(5)
            .clamp(1, MAX_WEB_SEARCH_RESULTS);
        let url = format!("https://duckduckgo.com/html/?q={}", url_encode_query(query));
        let body = self
            .fetch_url_limited(&url, DEFAULT_WEB_FETCH_BYTES)
            .await?;
        let results = parse_duckduckgo_results(&body.content, count);

        if results.is_empty() {
            return Err(anyhow!(
                "DuckDuckGo HTML search returned no parsable results"
            ));
        }

        Ok(json!({
            "query": query,
            "results": results,
        }))
    }

    async fn fetch_url_limited(&self, url: &str, max_bytes: usize) -> Result<WebFetchBody> {
        let parsed =
            reqwest::Url::parse(url).map_err(|err| anyhow!("Invalid URL '{}': {}", url, err))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(anyhow!("Unsupported URL scheme '{}'", parsed.scheme()));
        }
        validate_public_url(&parsed)
            .map_err(|err| anyhow!("Refusing to fetch '{}': {}", url, err))?;

        let response = self
            .http_client
            .get(parsed)
            .header(
                "Accept",
                "text/html, text/plain, application/json;q=0.9, */*;q=0.8",
            )
            .send()
            .await?;
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::<u8>::new();
        let mut truncated = false;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let remaining = max_bytes.saturating_sub(bytes.len());
            if chunk.len() > remaining {
                bytes.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() >= max_bytes {
                truncated = true;
                break;
            }
        }

        let content = String::from_utf8_lossy(&bytes).to_string();
        Ok(WebFetchBody {
            url: final_url,
            status,
            bytes: bytes.len(),
            content,
            truncated,
        })
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

#[derive(Debug)]
struct WebFetchBody {
    url: String,
    status: u16,
    bytes: usize,
    content: String,
    truncated: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct CommandShell<'a> {
    program: &'static str,
    args: Vec<&'a str>,
}

#[cfg(windows)]
fn command_shell(command: &str) -> CommandShell<'_> {
    CommandShell {
        program: "powershell.exe",
        args: vec![
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            command,
        ],
    }
}

#[cfg(not(windows))]
fn command_shell(command: &str) -> CommandShell<'_> {
    CommandShell {
        program: "sh",
        args: vec!["-c", command],
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug)]
enum PatchOp {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        changes: Vec<PatchLine>,
    },
}

#[derive(Debug)]
enum PatchLine {
    Context(String),
    Add(String),
    Remove(String),
}

enum PatchAction {
    Write {
        path: PathBuf,
        content: String,
        create_new: bool,
    },
    Delete {
        path: PathBuf,
    },
}

fn canonical_tool_name(name: &str) -> &str {
    match name {
        "Read" | "read" | "readFile" => "readFile",
        "Skill" | "ReadSkill" | "readSkill" => "readSkill",
        "ListSkills" | "listSkills" => "listSkills",
        "SearchSkills" | "searchSkills" => "searchSkills",
        "LS" | "list" | "listDirectory" => "listDirectory",
        "Glob" | "glob" | "searchFiles" => "searchFiles",
        "Grep" | "grep" => "grep",
        "Bash" | "bash" | "runCommand" => "runCommand",
        "fileInfo" => "fileInfo",
        "generateDiff" => "generateDiff",
        "Write" | "write" | "writeFile" => "writeFile",
        "Edit" | "edit" | "editFile" => "editFile",
        "MultiEdit" | "multiedit" | "multiEdit" => "multiEdit",
        "apply_patch" | "ApplyPatch" | "applyPatch" => "applyPatch",
        "WebFetch" | "webfetch" | "webFetch" => "webFetch",
        "WebSearch" | "websearch" | "webSearch" => "webSearch",
        other => other,
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

fn required_path(args: &Value) -> Result<&str> {
    required_any_str(args, &["path", "file_path"])
}

fn path_arg(args: &Value) -> Option<&str> {
    args.get("path")
        .or_else(|| args.get("file_path"))
        .and_then(Value::as_str)
}

fn required_any_str<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a str> {
    for key in keys {
        if let Some(value) = args
            .get(*key)
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            return Ok(value);
        }
    }

    Err(anyhow!(
        "Missing required string argument '{}'",
        keys.join("|")
    ))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("Missing required string argument '{}'", key))
}

fn bool_arg(args: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_bool))
}

fn apply_string_edit(
    content: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, usize)> {
    if old.is_empty() {
        return Err(anyhow!("oldString cannot be empty"));
    }

    let count = content.matches(old).count();
    if count == 0 {
        return Err(anyhow!("oldString was not found"));
    }
    if !replace_all && count > 1 {
        return Err(anyhow!(
            "oldString matched {} times; set replaceAll=true to replace all matches",
            count
        ));
    }

    let updated = if replace_all {
        content.replace(old, new)
    } else {
        content.replacen(old, new, 1)
    };
    Ok((updated, if replace_all { count } else { 1 }))
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

fn parse_apply_patch(patch: &str) -> Result<Vec<PatchOp>> {
    let lines = patch.lines().collect::<Vec<&str>>();
    if lines.first() != Some(&"*** Begin Patch") {
        return Err(anyhow!("Patch must start with '*** Begin Patch'"));
    }
    if lines.last() != Some(&"*** End Patch") {
        return Err(anyhow!("Patch must end with '*** End Patch'"));
    }

    let mut ops = Vec::<PatchOp>::new();
    let mut index = 1usize;
    while index + 1 < lines.len() {
        let line = lines[index];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            index += 1;
            let mut content = Vec::<String>::new();
            while index < lines.len() && !lines[index].starts_with("*** ") {
                let Some(text) = lines[index].strip_prefix('+') else {
                    return Err(anyhow!("Add file lines must start with '+'"));
                };
                content.push(text.to_string());
                index += 1;
            }
            ops.push(PatchOp::Add {
                path: path.to_string(),
                content: finish_patch_content(content),
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(PatchOp::Delete {
                path: path.to_string(),
            });
            index += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Update File: ") {
            index += 1;
            let mut move_to = None::<String>;
            let mut changes = Vec::<PatchLine>::new();

            while index < lines.len() && !is_patch_hunk_header(lines[index]) {
                let line = lines[index];
                if let Some(target) = line.strip_prefix("*** Move to: ") {
                    move_to = Some(target.to_string());
                } else if line.starts_with("@@") || line == "*** End of File" {
                } else if let Some(text) = line.strip_prefix('+') {
                    changes.push(PatchLine::Add(text.to_string()));
                } else if let Some(text) = line.strip_prefix('-') {
                    changes.push(PatchLine::Remove(text.to_string()));
                } else if let Some(text) = line.strip_prefix(' ') {
                    changes.push(PatchLine::Context(text.to_string()));
                } else {
                    return Err(anyhow!("Invalid update patch line '{}'", line));
                }
                index += 1;
            }

            ops.push(PatchOp::Update {
                path: path.to_string(),
                move_to,
                changes,
            });
            continue;
        }

        return Err(anyhow!("Invalid patch header '{}'", line));
    }

    Ok(ops)
}

fn is_patch_hunk_header(line: &str) -> bool {
    line.starts_with("*** Add File: ")
        || line.starts_with("*** Delete File: ")
        || line.starts_with("*** Update File: ")
        || line == "*** End Patch"
}

fn finish_patch_content(lines: Vec<String>) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn apply_patch_lines(original: &str, changes: &[PatchLine]) -> Result<String> {
    if changes.is_empty() {
        return Ok(original.to_string());
    }

    let old_lines = original
        .lines()
        .map(str::to_string)
        .collect::<Vec<String>>();
    let mut out = Vec::<String>::new();
    let mut cursor = 0usize;

    for change in changes {
        match change {
            PatchLine::Context(text) => {
                copy_until_match(&old_lines, &mut cursor, &mut out, text)?;
                out.push(text.clone());
                cursor += 1;
            }
            PatchLine::Remove(text) => {
                copy_until_match(&old_lines, &mut cursor, &mut out, text)?;
                cursor += 1;
            }
            PatchLine::Add(text) => out.push(text.clone()),
        }
    }

    while cursor < old_lines.len() {
        out.push(old_lines[cursor].clone());
        cursor += 1;
    }

    let mut updated = out.join("\n");
    if original.ends_with('\n') || !updated.is_empty() {
        updated.push('\n');
    }
    Ok(updated)
}

fn copy_until_match(
    old_lines: &[String],
    cursor: &mut usize,
    out: &mut Vec<String>,
    target: &str,
) -> Result<()> {
    let Some(position) = old_lines[*cursor..].iter().position(|line| line == target) else {
        return Err(anyhow!("Patch context line not found: '{}'", target));
    };

    let target_index = *cursor + position;
    while *cursor < target_index {
        out.push(old_lines[*cursor].clone());
        *cursor += 1;
    }
    Ok(())
}

fn parse_duckduckgo_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let link_re = Regex::new(
        r#"(?is)<a[^>]*class="[^"]*\bresult__a\b[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#,
    )
    .expect("valid result regex");
    let snippet_re = Regex::new(
        r#"(?is)<(?:a|div)[^>]*class="[^"]*\bresult__snippet\b[^"]*"[^>]*>(.*?)</(?:a|div)>"#,
    )
    .expect("valid snippet regex");
    let mut out = Vec::<SearchResult>::new();

    for link in link_re.captures_iter(html).take(limit) {
        let Some(full_match) = link.get(0) else {
            continue;
        };
        let href = link.get(1).map(|m| m.as_str()).unwrap_or_default();
        let title = link
            .get(2)
            .map(|m| strip_html(m.as_str()))
            .unwrap_or_default();
        let tail = &html[full_match.end()..];
        let snippet = snippet_re
            .captures(tail)
            .and_then(|captures| captures.get(1))
            .map(|m| strip_html(m.as_str()))
            .unwrap_or_default();

        let url = decode_duckduckgo_url(href);
        if !title.is_empty() && !url.is_empty() {
            out.push(SearchResult {
                title,
                url,
                snippet,
            });
        }
    }

    out
}

fn decode_duckduckgo_url(href: &str) -> String {
    let decoded = decode_html_entities(href);
    let normalized = if decoded.starts_with("//") {
        format!("https:{decoded}")
    } else {
        decoded
    };

    if let Some(query) = normalized.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if key == "uddg" {
                return percent_decode(value);
            }
        }
    }

    normalized
}

fn strip_html(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;

    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    decode_html_entities(&out)
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn decode_html_entities(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }

        let mut entity = String::new();
        while let Some(next) = chars.peek().copied() {
            entity.push(next);
            let _ = chars.next();
            if next == ';' || entity.len() > 12 {
                break;
            }
        }

        let decoded = match entity.as_str() {
            "amp;" => Some('&'),
            "lt;" => Some('<'),
            "gt;" => Some('>'),
            "quot;" => Some('"'),
            "#39;" | "apos;" => Some('\''),
            _ => decode_numeric_entity(&entity),
        };

        if let Some(decoded) = decoded {
            out.push(decoded);
        } else {
            out.push('&');
            out.push_str(&entity);
        }
    }

    out
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    if let Some(hex) = entity.strip_prefix("#x").and_then(|v| v.strip_suffix(';')) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(decimal) = entity.strip_prefix('#').and_then(|v| v.strip_suffix(';')) {
        return decimal.parse::<u32>().ok().and_then(char::from_u32);
    }
    None
}

fn url_encode_query(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::<u8>::new();
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    out.push((high << 4) | low);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn chrono_like(time: std::time::SystemTime) -> String {
    let datetime = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0);
    format!("{}", datetime)
}

fn validate_public_url(url: &reqwest::Url) -> std::result::Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("unsupported URL scheme '{}'", url.scheme()));
    }

    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return Err(format!("host '{host}' is not a public address"));
    }

    let ip_candidate = host
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err(format!("host '{host}' is not a public address"));
        }
        return Ok(());
    }

    let port = url.port_or_known_default().unwrap_or(0);
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve '{host}': {err}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(format!("host '{host}' did not resolve"));
    }
    for addr in addrs {
        if !is_public_ip(addr.ip()) {
            return Err(format!("host '{host}' resolves to a non-public address"));
        }
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_documentation()
            {
                return false;
            }
            let o = v4.octets();
            // 0.0.0.0/8
            if o[0] == 0 {
                return false;
            }
            // CGNAT 100.64.0.0/10
            if o[0] == 100 && (64..=127).contains(&o[1]) {
                return false;
            }
            // 192.0.0.0/24 protocol assignments
            if o[0] == 192 && o[1] == 0 && o[2] == 0 {
                return false;
            }
            // Benchmarking 198.18.0.0/15
            if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
                return false;
            }
            // Reserved 240.0.0.0/4 (excluding broadcast)
            if o[0] >= 240 {
                return false;
            }
            true
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            let s = v6.segments();
            // Unique local fc00::/7
            if (s[0] & 0xfe00) == 0xfc00 {
                return false;
            }
            // Link-local fe80::/10
            if (s[0] & 0xffc0) == 0xfe80 {
                return false;
            }
            // Documentation 2001:db8::/32
            if s[0] == 0x2001 && s[1] == 0x0db8 {
                return false;
            }
            // ::ffff:0:0/96 IPv4-mapped → check underlying v4
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::ToolRuntime;
    use super::command_shell;
    use super::decode_duckduckgo_url;
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
        assert!(tool_requires_approval("Bash"));
        assert!(tool_requires_approval("apply_patch"));
        assert!(!tool_requires_approval("readFile"));
        assert!(!tool_requires_approval("webFetch"));
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
        let expected_cwd =
            shell_cwd_display(&workspace.canonicalize().expect("canonical workspace"));

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
}
