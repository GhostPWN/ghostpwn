use std::io::Read as _;
use std::io::{Error as IoError, ErrorKind};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use futures_util::{StreamExt, stream};
use globset::Glob;
use rand::RngCore;
use regex::Regex;
use reqwest::Client;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::timeout;
use walkdir::WalkDir;

use crate::models::ToolCall;
use crate::skills::SkillRuntime;

const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;
const MAX_COMMAND_STDOUT_BYTES: usize = 10_000;
const MAX_COMMAND_STDERR_BYTES: usize = 5_000;
const MAX_TEXT_FILE_BYTES: usize = 5_000_000;
const DEFAULT_WEB_FETCH_BYTES: usize = 1_000_000;
const MAX_WEB_FETCH_BYTES: usize = 5_000_000;
const MAX_WEB_SEARCH_RESULTS: usize = 10;
const MAX_SEARCH_FILES: usize = 100;
const MAX_GREP_MATCHES: usize = 50;
const MAX_AUDIT_LOCKFILES: usize = 25;
const MAX_AUDIT_PACKAGES: usize = 1_000;
const MAX_AUDIT_ADVISORIES: usize = 100;
const DEFAULT_READ_FILE_LINES: usize = 200;
const MAX_DIFF_LINES: usize = 5_000;
const MAX_DIFF_MATRIX_CELLS: usize = 1_000_000;
const OSV_QUERY_BATCH_URL: &str = "https://api.osv.dev/v1/querybatch";

pub fn tool_requires_approval(name: &str) -> bool {
    matches!(
        canonical_tool_name(name),
        "runCommand" | "auditDependencies" | "writeFile" | "editFile" | "multiEdit" | "applyPatch"
    )
}

pub fn audit_tool_allowed(name: &str, allow_mutations: bool) -> bool {
    matches!(
        canonical_tool_name(name),
        "listSkills"
            | "searchSkills"
            | "readSkill"
            | "readFile"
            | "listDirectory"
            | "searchFiles"
            | "grep"
            | "fileInfo"
            | "auditDependencies"
    ) || allow_mutations
        && matches!(
            canonical_tool_name(name),
            "generateDiff" | "writeFile" | "editFile" | "multiEdit" | "applyPatch"
        )
}

pub struct ToolRuntime {
    workspace_root: PathBuf,
    workspace_dir: Arc<Dir>,
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
            .dns_resolver(Arc::new(PublicDnsResolver))
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
        let workspace_dir =
            Dir::open_ambient_dir(&canonical, ambient_authority()).map_err(|err| {
                anyhow!(
                    "Failed to open workspace root '{}': {}",
                    canonical.display(),
                    err
                )
            })?;

        Ok(Self {
            skills,
            workspace_root: canonical,
            workspace_dir: Arc::new(workspace_dir),
            http_client,
        })
    }

    pub async fn prompt_skill_section(&self) -> String {
        self.skills.prompt_section().await
    }

    pub fn arg_summary(&self, name: &str, args: &Value) -> String {
        let canonical = canonical_tool_name(name);
        let detailed = match canonical {
            "runCommand" => Some(format!(
                "UNSANDBOXED shell command: {}",
                args.get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )),
            "writeFile" => Some(format!(
                "{} ({} bytes)",
                path_arg(args).unwrap_or_default(),
                args.get("content")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0)
            )),
            "editFile" => Some(format!(
                "{} (replace {} chars with {} chars, replaceAll={})",
                path_arg(args).unwrap_or_default(),
                args.get("oldString")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0),
                args.get("newString")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0),
                bool_arg(args, &["replaceAll", "replace_all"]).unwrap_or(false)
            )),
            "multiEdit" => Some(format!(
                "{} ({} edits)",
                path_arg(args).unwrap_or_default(),
                args.get("edits")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            )),
            "applyPatch" => Some(format!(
                "patch ({} chars): {}",
                args.get("patchText")
                    .or_else(|| args.get("patch"))
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0),
                args.get("patchText")
                    .or_else(|| args.get("patch"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )),
            _ => None,
        };
        if let Some(summary) = detailed {
            return summary.chars().take(240).collect();
        }

        let key = match canonical {
            "listSkills" => "",
            "searchSkills" => "query",
            "readSkill" => "name",
            "readFile" => "path",
            "listDirectory" => "path",
            "searchFiles" => "pattern",
            "grep" => "pattern",
            "runCommand" => "command",
            "auditDependencies" => "path",
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
            return path_arg(args)
                .unwrap_or_default()
                .chars()
                .take(240)
                .collect();
        }

        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect()
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
            "auditDependencies" => self.audit_dependencies(&call.arguments).await,
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

    pub async fn execute_audit(
        &self,
        call: &ToolCall,
        scope: &Path,
        allow_mutations: bool,
    ) -> Result<Value> {
        if !audit_tool_allowed(&call.name, allow_mutations) {
            return Err(anyhow!(
                "Tool '{}' is unavailable in this audit mode",
                call.name
            ));
        }

        let mut scoped_call = call.clone();
        let name = canonical_tool_name(&scoped_call.name);
        if matches!(
            name,
            "listDirectory" | "searchFiles" | "grep" | "auditDependencies"
        ) && tool_path_arg(name, &scoped_call.arguments).is_none()
        {
            scoped_call.arguments["path"] = json!(scope.display().to_string());
        }
        if matches!(name, "searchFiles" | "grep") {
            scoped_call.arguments["excludeGenerated"] = json!(true);
        }
        if name == "readFile" {
            scoped_call.arguments["lineNumbers"] = json!(true);
        }

        let resolved = match name {
            "readFile" | "listDirectory" | "searchFiles" | "grep" | "fileInfo"
            | "auditDependencies" | "generateDiff" | "editFile" | "multiEdit" => {
                let path = tool_path_arg(name, &scoped_call.arguments).unwrap_or(".");
                Some(self.resolve_in_workspace(path)?)
            }
            "writeFile" => {
                let path = required_path(&scoped_call.arguments)?;
                Some(self.resolve_existing_or_missing_in_workspace(path)?)
            }
            "applyPatch" => {
                self.ensure_patch_in_scope(&scoped_call.arguments, scope)?;
                None
            }
            _ => None,
        };
        if let Some(resolved) = resolved
            && !resolved.starts_with(scope)
        {
            return Err(anyhow!(
                "Path '{}' is outside audit scope '{}'",
                resolved.display(),
                scope.display()
            ));
        }

        self.execute(&scoped_call).await
    }

    fn ensure_patch_in_scope(&self, args: &Value, scope: &Path) -> Result<()> {
        let patch_text = required_any_str(args, &["patchText", "patch_text", "patch"])?;
        for op in parse_apply_patch(patch_text)? {
            let mut paths = match op {
                PatchOp::Add { path, .. } => vec![(path, false)],
                PatchOp::Delete { path } => vec![(path, true)],
                PatchOp::Update { path, move_to, .. } => {
                    let mut paths = vec![(path, true)];
                    if let Some(move_to) = move_to {
                        paths.push((move_to, false));
                    }
                    paths
                }
            };
            for (path, must_exist) in paths.drain(..) {
                let resolved = if must_exist {
                    self.resolve_in_workspace(&path)?
                } else {
                    self.resolve_existing_or_missing_in_workspace(&path)?
                };
                if !resolved.starts_with(scope) {
                    return Err(anyhow!(
                        "Path '{}' is outside audit scope '{}'",
                        resolved.display(),
                        scope.display()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn resolve_audit_scope(&self, target: &str) -> Result<(PathBuf, String)> {
        if target.is_empty() {
            return Ok((
                self.workspace_root.clone(),
                "the entire workspace".to_string(),
            ));
        }

        let candidate = self.absolute_input_path(target);
        if candidate.exists() {
            let resolved = self.resolve_in_workspace(target)?;
            let relative = resolved
                .strip_prefix(&self.workspace_root)
                .unwrap_or(&resolved);
            let display = if relative.as_os_str().is_empty() {
                ".".to_string()
            } else {
                relative.display().to_string()
            };
            return Ok((resolved, format!("workspace path {display:?}")));
        }

        if looks_like_path(target) {
            return Err(anyhow!("Audit path '{}' does not exist", target));
        }

        Ok((
            self.workspace_root.clone(),
            format!("the entire workspace, focused on {target:?}"),
        ))
    }

    async fn read_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let limit = args
            .get("maxLines")
            .or_else(|| args.get("limit"))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_READ_FILE_LINES);
        let offset = args
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(0);

        let resolved = self.resolve_in_workspace(path)?;
        let content = read_text_file(&resolved).await?;
        let total_lines = content.lines().count();

        let selected = content
            .lines()
            .enumerate()
            .skip(offset)
            .take(limit)
            .collect::<Vec<(usize, &str)>>();
        let output = selected
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<&str>>()
            .join("\n");
        let numbered = selected
            .iter()
            .map(|(index, line)| format!("{}: {}", index + 1, line))
            .collect::<Vec<String>>()
            .join("\n");
        let end_line = selected.last().map(|(index, _)| index + 1);
        let include_line_numbers = args
            .get("lineNumbers")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let truncated = total_lines > offset.saturating_add(limit);

        let mut result = json!({
            "path": resolved.display().to_string(),
            "content": output,
            "startLine": selected.first().map(|(index, _)| index + 1),
            "endLine": end_line,
            "totalLines": total_lines,
            "truncated": truncated,
        });
        if include_line_numbers {
            result["numberedContent"] = json!(numbered);
        }
        Ok(result)
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
        let exclude_generated = args
            .get("excludeGenerated")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut matches = Vec::<String>::new();
        for entry in WalkDir::new(&base)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| !exclude_generated || include_walk_entry(entry))
        {
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
                if matches.len() > MAX_SEARCH_FILES {
                    break;
                }
            }
        }
        let truncated = matches.len() > MAX_SEARCH_FILES;
        matches.truncate(MAX_SEARCH_FILES);

        Ok(json!({
            "pattern": pattern,
            "cwd": base.display().to_string(),
            "matches": matches,
            "truncated": truncated,
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
        let exclude_generated = args
            .get("excludeGenerated")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut results = Vec::<Value>::new();
        let mut skipped_files = 0usize;
        let walker = if resolved.is_file() {
            WalkDir::new(resolved.parent().unwrap_or(self.workspace_root.as_path()))
        } else {
            WalkDir::new(&resolved)
        };

        for entry in walker
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| !exclude_generated || include_walk_entry(entry))
        {
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

            let content = match read_text_file(path).await {
                Ok(v) => v,
                Err(_) => {
                    skipped_files += 1;
                    continue;
                }
            };

            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(json!({
                        "file": path.display().to_string(),
                        "line": index + 1,
                        "text": line,
                    }));
                    if results.len() > MAX_GREP_MATCHES {
                        break;
                    }
                }
            }

            if results.len() > MAX_GREP_MATCHES {
                break;
            }
        }
        let truncated = results.len() > MAX_GREP_MATCHES;
        results.truncate(MAX_GREP_MATCHES);

        Ok(json!({
            "pattern": pattern,
            "matches": results,
            "totalMatches": results.len(),
            "truncated": truncated,
            "complete": !truncated && skipped_files == 0,
            "skippedFiles": skipped_files,
        }))
    }

    async fn audit_dependencies(&self, args: &Value) -> Result<Value> {
        let path = path_arg(args).unwrap_or(".");
        let scope = self.resolve_in_workspace(path)?;
        let (lockfiles, lockfiles_truncated) = cargo_lockfiles(&scope);
        if lockfiles.is_empty() {
            return Ok(json!({
                "database": "OSV.dev",
                "ecosystem": "crates.io",
                "lockfiles": [],
                "packagesScanned": 0,
                "findings": [],
                "complete": true,
                "message": "No Cargo.lock files found in scope",
            }));
        }

        let mut packages = Vec::<CargoPackage>::new();
        for lockfile in &lockfiles {
            let content = read_text_file(lockfile).await?;
            for (name, version) in parse_cargo_lock(&content) {
                packages.push(CargoPackage {
                    lockfile: lockfile.display().to_string(),
                    name,
                    version,
                });
            }
        }
        packages.sort_by(|a, b| {
            (&a.lockfile, &a.name, &a.version).cmp(&(&b.lockfile, &b.name, &b.version))
        });
        packages.dedup_by(|a, b| {
            a.lockfile == b.lockfile && a.name == b.name && a.version == b.version
        });
        let packages_truncated = packages.len() > MAX_AUDIT_PACKAGES;
        packages.truncate(MAX_AUDIT_PACKAGES);
        if packages.is_empty() {
            return Ok(json!({
                "database": "OSV.dev",
                "ecosystem": "crates.io",
                "lockfiles": lockfiles.iter().map(|path| path.display().to_string()).collect::<Vec<String>>(),
                "packagesScanned": 0,
                "findings": [],
                "complete": !lockfiles_truncated,
                "message": "Cargo.lock contained no packages",
            }));
        }

        let queries = packages
            .iter()
            .map(|package| {
                json!({
                    "version": package.version,
                    "package": {
                        "name": package.name,
                        "ecosystem": "crates.io",
                    }
                })
            })
            .collect::<Vec<Value>>();
        let response = self
            .http_client
            .post(OSV_QUERY_BATCH_URL)
            .json(&json!({ "queries": queries }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let results = response
            .get("results")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("OSV returned an invalid batch response"))?;
        if results.len() != packages.len() {
            return Err(anyhow!(
                "OSV returned {} results for {} packages",
                results.len(),
                packages.len()
            ));
        }

        let mut findings = Vec::<Value>::new();
        let mut advisory_ids = Vec::<String>::new();
        let mut paginated = false;
        for (package, result) in packages.iter().zip(results) {
            paginated |= result
                .get("next_page_token")
                .and_then(Value::as_str)
                .is_some();
            let ids = result
                .get("vulns")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|vulnerability| vulnerability.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<String>>();
            if ids.is_empty() {
                continue;
            }
            advisory_ids.extend(ids.iter().cloned());
            findings.push(json!({
                "lockfile": package.lockfile,
                "package": package.name,
                "version": package.version,
                "advisoryIds": ids,
            }));
        }

        advisory_ids.sort();
        advisory_ids.dedup();
        let advisories_truncated = advisory_ids.len() > MAX_AUDIT_ADVISORIES;
        advisory_ids.truncate(MAX_AUDIT_ADVISORIES);
        let client = &self.http_client;
        let details = stream::iter(advisory_ids.into_iter().map(|id| async move {
            let url = format!("https://api.osv.dev/v1/vulns/{id}");
            let result = client
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .json::<Value>()
                .await;
            result.map(|value| (id, summarize_osv_advisory(&value)))
        }))
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
        let mut advisories = serde_json::Map::new();
        let mut advisory_errors = Vec::<String>::new();
        for result in details {
            match result {
                Ok((id, detail)) => {
                    advisories.insert(id, detail);
                }
                Err(err) => advisory_errors.push(err.to_string()),
            }
        }

        let complete = !lockfiles_truncated
            && !packages_truncated
            && !paginated
            && !advisories_truncated
            && advisory_errors.is_empty();
        Ok(json!({
            "database": "OSV.dev",
            "ecosystem": "crates.io",
            "lockfiles": lockfiles.iter().map(|path| path.display().to_string()).collect::<Vec<String>>(),
            "packagesScanned": packages.len(),
            "findings": findings,
            "advisories": advisories,
            "complete": complete,
            "truncation": {
                "lockfiles": lockfiles_truncated,
                "packages": packages_truncated,
                "pagination": paginated,
                "advisories": advisories_truncated,
            },
            "errors": advisory_errors,
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

        let mut child = command_builder.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture command stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("Failed to capture command stderr"))?;

        let result = timeout(Duration::from_millis(timeout_ms), async {
            tokio::join!(
                read_capped(stdout, MAX_COMMAND_STDOUT_BYTES),
                read_capped(stderr, MAX_COMMAND_STDERR_BYTES),
                child.wait()
            )
        })
        .await;
        let (stdout, stderr, status) = match result {
            Ok((stdout, stderr, status)) => {
                let (stdout, stdout_truncated) = stdout?;
                let (stderr, stderr_truncated) = stderr?;
                (
                    stdout,
                    stderr,
                    (status?, stdout_truncated || stderr_truncated),
                )
            }
            Err(_) => {
                return Ok(json!({
                    "stdout": "",
                    "stderr": "Command timed out",
                    "exitCode": -1,
                    "truncated": false,
                }));
            }
        };

        Ok(json!({
            "stdout": String::from_utf8_lossy(&stdout),
            "stderr": String::from_utf8_lossy(&stderr),
            "exitCode": status.0.code().unwrap_or(-1),
            "truncated": status.1,
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
        let proposed = required_any_string(args, &["content"])?;
        let resolved = self.resolve_in_workspace(path)?;
        let original = read_text_file(&resolved).await?;
        if original.lines().count() > MAX_DIFF_LINES || proposed.lines().count() > MAX_DIFF_LINES {
            return Err(anyhow!(
                "Diff input exceeds the {}-line safety limit; split the edit into smaller files",
                MAX_DIFF_LINES
            ));
        }
        let diff = unified_diff(path, &original, proposed);

        Ok(json!({
            "path": resolved.display().to_string(),
            "diff": diff,
        }))
    }

    async fn write_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let content = required_any_string(args, &["content"])?;
        let relative = self.workspace_relative_path(path)?;
        self.write_workspace_file(&relative, content.as_bytes(), false)
            .await?;

        Ok(json!({
            "path": self.workspace_root.join(relative).display().to_string(),
            "bytes": content.len(),
            "written": true,
        }))
    }

    async fn edit_file(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let old = required_any_str(args, &["oldString", "old_string"])?;
        let new = required_any_string(args, &["newString", "new_string"])?;
        let replace_all = bool_arg(args, &["replaceAll", "replace_all"]).unwrap_or(false);

        let relative = self.workspace_relative_path(path)?;
        let content = self.read_workspace_text(&relative).await?;
        let (updated, replacements) = apply_string_edit(&content, old, new, replace_all)?;
        self.write_workspace_file(&relative, updated.as_bytes(), false)
            .await?;

        Ok(json!({
            "path": self.workspace_root.join(relative).display().to_string(),
            "replacements": replacements,
        }))
    }

    async fn multi_edit(&self, args: &Value) -> Result<Value> {
        let path = required_path(args)?;
        let edits = args
            .get("edits")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Missing required array argument 'edits'"))?;

        let relative = self.workspace_relative_path(path)?;
        let mut content = self.read_workspace_text(&relative).await?;
        let mut total = 0usize;

        for edit in edits {
            let old = required_any_str(edit, &["oldString", "old_string"])?;
            let new = required_any_string(edit, &["newString", "new_string"])?;
            let replace_all = bool_arg(edit, &["replaceAll", "replace_all"]).unwrap_or(false);
            let (next, replacements) = apply_string_edit(&content, old, new, replace_all)?;
            content = next;
            total += replacements;
        }

        self.write_workspace_file(&relative, content.as_bytes(), false)
            .await?;

        Ok(json!({
            "path": self.workspace_root.join(relative).display().to_string(),
            "edits": edits.len(),
            "replacements": total,
        }))
    }

    async fn apply_patch(&self, args: &Value) -> Result<Value> {
        self.apply_patch_with_failure(args, None, None).await
    }

    async fn apply_patch_with_failure(
        &self,
        args: &Value,
        stage_failure: Option<usize>,
        commit_failure: Option<usize>,
    ) -> Result<Value> {
        let patch_text = required_any_str(args, &["patchText", "patch_text", "patch"])?;
        let ops = parse_apply_patch(patch_text)?;
        let mut actions = Vec::<PatchAction>::new();

        for op in ops {
            match op {
                PatchOp::Add { path, content } => {
                    let relative = self.workspace_relative_path(&path)?;
                    if self.workspace_path_exists(&relative).await? {
                        return Err(anyhow!("Cannot add '{}': file already exists", path));
                    }
                    actions.push(PatchAction::Write {
                        path: relative,
                        content,
                        create_new: true,
                    });
                }
                PatchOp::Delete { path } => {
                    let relative = self.workspace_relative_path(&path)?;
                    self.ensure_workspace_regular_file(&relative).await?;
                    actions.push(PatchAction::Delete { path: relative });
                }
                PatchOp::Update {
                    path,
                    move_to,
                    changes,
                } => {
                    let source = self.workspace_relative_path(&path)?;
                    let original = self.read_workspace_text(&source).await?;
                    let updated = apply_patch_lines(&original, &changes)?;
                    let target = if let Some(move_to) = move_to {
                        let target = self.workspace_relative_path(&move_to)?;
                        if self.workspace_path_exists(&target).await? && target != source {
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
                        actions.push(PatchAction::Write {
                            path: target,
                            content: updated,
                            create_new,
                        });
                        actions.push(PatchAction::Delete { path: source });
                        continue;
                    }
                    actions.push(PatchAction::Write {
                        path: target,
                        content: updated,
                        create_new,
                    });
                }
            }
        }

        validate_patch_actions(&actions)?;
        let changed = actions
            .iter()
            .map(|action| {
                self.workspace_root
                    .join(action.path())
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let workspace_dir = Arc::clone(&self.workspace_dir);
        tokio::task::spawn_blocking(move || {
            apply_patch_transaction(&workspace_dir, actions, stage_failure, commit_failure)
        })
        .await
        .map_err(|err| anyhow!("Workspace patch task failed: {err}"))??;

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

    fn workspace_relative_path(&self, input: &str) -> Result<PathBuf> {
        let input_path = Path::new(input);
        let relative = if input_path.is_absolute() {
            input_path.strip_prefix(&self.workspace_root).map_err(|_| {
                anyhow!(
                    "Path '{}' is outside workspace root '{}'",
                    input_path.display(),
                    self.workspace_root.display()
                )
            })?
        } else {
            input_path
        };

        let mut normalized = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => normalized.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(anyhow!(
                        "Path '{}' is outside workspace root '{}'",
                        input_path.display(),
                        self.workspace_root.display()
                    ));
                }
            }
        }
        if normalized.as_os_str().is_empty() {
            return Err(anyhow!("Path '{}' does not name a file", input));
        }
        Ok(normalized)
    }

    fn open_workspace_parent(
        workspace_dir: &Dir,
        path: &Path,
        create: bool,
    ) -> Result<(Dir, PathBuf)> {
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow!("Path '{}' does not name a file", path.display()))?;
        let mut dir = workspace_dir.try_clone()?;

        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let Component::Normal(part) = component else {
                    return Err(anyhow!("Invalid workspace path '{}'", path.display()));
                };
                match dir.open_dir_nofollow(part) {
                    Ok(next) => dir = next,
                    Err(err) if create && err.kind() == ErrorKind::NotFound => {
                        match dir.create_dir(part) {
                            Ok(()) => {}
                            Err(create_err) if create_err.kind() == ErrorKind::AlreadyExists => {}
                            Err(create_err) => return Err(create_err.into()),
                        }
                        dir = dir.open_dir_nofollow(part)?;
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }

        Ok((dir, PathBuf::from(file_name)))
    }

    fn workspace_path_exists_blocking(workspace_dir: &Dir, path: &Path) -> Result<bool> {
        let (parent, file_name) = match Self::open_workspace_parent(workspace_dir, path, false) {
            Ok(parts) => parts,
            Err(err)
                if err
                    .downcast_ref::<IoError>()
                    .is_some_and(|io| io.kind() == ErrorKind::NotFound) =>
            {
                return Ok(false);
            }
            Err(err) => return Err(err),
        };
        match parent.symlink_metadata(file_name) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    async fn workspace_path_exists(&self, path: &Path) -> Result<bool> {
        let workspace_dir = Arc::clone(&self.workspace_dir);
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Self::workspace_path_exists_blocking(&workspace_dir, &path)
        })
        .await
        .map_err(|err| anyhow!("Workspace path check task failed: {err}"))?
    }

    fn ensure_workspace_regular_file_blocking(workspace_dir: &Dir, path: &Path) -> Result<()> {
        let (parent, file_name) = Self::open_workspace_parent(workspace_dir, path, false)?;
        let metadata = parent.symlink_metadata(&file_name)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(anyhow!("Path '{}' is not a regular file", path.display()));
        }
        Ok(())
    }

    async fn ensure_workspace_regular_file(&self, path: &Path) -> Result<()> {
        let workspace_dir = Arc::clone(&self.workspace_dir);
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Self::ensure_workspace_regular_file_blocking(&workspace_dir, &path)
        })
        .await
        .map_err(|err| anyhow!("Workspace file check task failed: {err}"))?
    }

    fn read_workspace_text_blocking(workspace_dir: &Dir, path: &Path) -> Result<String> {
        let (parent, file_name) = Self::open_workspace_parent(workspace_dir, path, false)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent.open_with(file_name, &options)?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(MAX_TEXT_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_TEXT_FILE_BYTES {
            return Err(anyhow!(
                "Text file '{}' exceeds the {}-byte safety limit",
                path.display(),
                MAX_TEXT_FILE_BYTES
            ));
        }
        String::from_utf8(bytes)
            .map_err(|err| anyhow!("Text file '{}' is not valid UTF-8: {}", path.display(), err))
    }

    async fn read_workspace_text(&self, path: &Path) -> Result<String> {
        let workspace_dir = Arc::clone(&self.workspace_dir);
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Self::read_workspace_text_blocking(&workspace_dir, &path)
        })
        .await
        .map_err(|err| anyhow!("Workspace file read task failed: {err}"))?
    }

    fn write_workspace_file_blocking(
        workspace_dir: &Dir,
        path: &Path,
        content: &[u8],
        create_new: bool,
    ) -> Result<()> {
        let (parent, file_name) = Self::open_workspace_parent(workspace_dir, path, true)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create(true)
            .create_new(create_new)
            .truncate(true)
            .follow(FollowSymlinks::No);
        let mut file = parent.open_with(file_name, &options)?;
        std::io::Write::write_all(&mut file, content)?;
        std::io::Write::flush(&mut file)?;
        Ok(())
    }

    async fn write_workspace_file(
        &self,
        path: &Path,
        content: &[u8],
        create_new: bool,
    ) -> Result<()> {
        let workspace_dir = Arc::clone(&self.workspace_dir);
        let path = path.to_path_buf();
        let content = content.to_vec();
        tokio::task::spawn_blocking(move || {
            Self::write_workspace_file_blocking(&workspace_dir, &path, &content, create_new)
        })
        .await
        .map_err(|err| anyhow!("Workspace file write task failed: {err}"))?
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

#[derive(Debug)]
struct CargoPackage {
    lockfile: String,
    name: String,
    version: String,
}

fn include_walk_entry(entry: &walkdir::DirEntry) -> bool {
    !entry.file_type().is_dir()
        || !matches!(
            entry.file_name().to_str(),
            Some(".git" | "node_modules" | "target")
        )
}

fn looks_like_path(target: &str) -> bool {
    let path = Path::new(target);
    path.is_absolute()
        || target.starts_with('.')
        || target.contains('/')
        || target.contains('\\')
        || (!target.chars().any(char::is_whitespace) && path.extension().is_some())
}

fn cargo_lockfiles(scope: &Path) -> (Vec<PathBuf>, bool) {
    let mut lockfiles = if scope.is_file() {
        match scope.file_name().and_then(|name| name.to_str()) {
            Some("Cargo.lock") => vec![scope.to_path_buf()],
            _ => Vec::new(),
        }
    } else {
        WalkDir::new(scope)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(include_walk_entry)
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file() && entry.file_name() == "Cargo.lock")
            .map(|entry| entry.into_path())
            .take(MAX_AUDIT_LOCKFILES + 1)
            .collect::<Vec<PathBuf>>()
    };
    let truncated = lockfiles.len() > MAX_AUDIT_LOCKFILES;
    lockfiles.truncate(MAX_AUDIT_LOCKFILES);
    (lockfiles, truncated)
}

fn parse_cargo_lock(content: &str) -> Vec<(String, String)> {
    let mut packages = Vec::<(String, String)>::new();
    let mut name = None::<String>;
    let mut version = None::<String>;

    for line in content.lines().map(str::trim) {
        if line == "[[package]]" {
            if let (Some(name), Some(version)) = (name.take(), version.take()) {
                packages.push((name, version));
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("name = ") {
            name = parse_toml_string(value);
        } else if let Some(value) = line.strip_prefix("version = ") {
            version = parse_toml_string(value);
        }
    }
    if let (Some(name), Some(version)) = (name, version) {
        packages.push((name, version));
    }
    packages
}

fn parse_toml_string(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
}

fn summarize_osv_advisory(advisory: &Value) -> Value {
    let fixed_versions = advisory
        .get("affected")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|affected| {
            affected
                .get("ranges")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .flat_map(|range| {
            range
                .get("events")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|event| event.get("fixed").and_then(Value::as_str))
        .collect::<Vec<&str>>();

    json!({
        "summary": advisory.get("summary"),
        "aliases": advisory.get("aliases"),
        "severity": advisory.get("severity"),
        "databaseSpecific": advisory.get("database_specific"),
        "fixedVersions": fixed_versions,
        "url": advisory.get("id").and_then(Value::as_str).map(|id| format!("https://osv.dev/vulnerability/{id}")),
    })
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
        "AuditDependencies" | "auditDependencies" => "auditDependencies",
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

fn tool_path_arg<'a>(name: &str, args: &'a Value) -> Option<&'a str> {
    if name == "searchFiles" {
        args.get("cwd")
            .and_then(Value::as_str)
            .or_else(|| path_arg(args))
    } else {
        path_arg(args)
    }
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

fn required_any_string<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a str> {
    for key in keys {
        if let Some(value) = args.get(*key).and_then(Value::as_str) {
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

fn unified_diff(path: &str, original: &str, proposed: &str) -> String {
    if original == proposed {
        return format!("--- a/{path}\n+++ b/{path}\n");
    }

    let old = original.lines().collect::<Vec<&str>>();
    let new = proposed.lines().collect::<Vec<&str>>();
    let prefix_len = old
        .iter()
        .zip(&new)
        .take_while(|(old_line, new_line)| old_line == new_line)
        .count();
    let suffix_len = old[prefix_len..]
        .iter()
        .rev()
        .zip(new[prefix_len..].iter().rev())
        .take_while(|(old_line, new_line)| old_line == new_line)
        .count();
    let old_middle = &old[prefix_len..old.len() - suffix_len];
    let new_middle = &new[prefix_len..new.len() - suffix_len];

    let mut out = Vec::with_capacity(old.len().saturating_add(new.len()).saturating_add(3));
    out.push(format!("--- a/{path}"));
    out.push(format!("+++ b/{path}"));
    out.push(format!("@@ -1,{} +1,{} @@", old.len(), new.len()));
    out.extend(old[..prefix_len].iter().map(|line| format!(" {line}")));

    if old_middle.len().saturating_mul(new_middle.len()) <= MAX_DIFF_MATRIX_CELLS {
        let mut lcs = vec![vec![0usize; new_middle.len() + 1]; old_middle.len() + 1];
        for i in (0..old_middle.len()).rev() {
            for j in (0..new_middle.len()).rev() {
                lcs[i][j] = if old_middle[i] == new_middle[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }

        let mut i = 0;
        let mut j = 0;
        while i < old_middle.len() && j < new_middle.len() {
            if old_middle[i] == new_middle[j] {
                out.push(format!(" {}", old_middle[i]));
                i += 1;
                j += 1;
            } else if lcs[i + 1][j] >= lcs[i][j + 1] {
                out.push(format!("-{}", old_middle[i]));
                i += 1;
            } else {
                out.push(format!("+{}", new_middle[j]));
                j += 1;
            }
        }
        out.extend(old_middle[i..].iter().map(|line| format!("-{line}")));
        out.extend(new_middle[j..].iter().map(|line| format!("+{line}")));
    } else {
        out.extend(old_middle.iter().map(|line| format!("-{line}")));
        out.extend(new_middle.iter().map(|line| format!("+{line}")));
    }

    out.extend(
        old[old.len() - suffix_len..]
            .iter()
            .map(|line| format!(" {line}")),
    );

    out.join("\n")
}

fn validate_patch_actions(actions: &[PatchAction]) -> Result<()> {
    for (index, action) in actions.iter().enumerate() {
        let path = action.path();
        for other in &actions[..index] {
            let other_path = other.path();
            if patch_paths_conflict(path, other_path) {
                return Err(anyhow!(
                    "Patch contains conflicting actions for '{}' and '{}'",
                    other_path.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn patch_paths_conflict(path: &Path, other: &Path) -> bool {
    path == other || path.starts_with(other) || other.starts_with(path)
}

#[cfg(windows)]
fn patch_paths_conflict(path: &Path, other: &Path) -> bool {
    fn normalized_components(path: &Path) -> Vec<String> {
        path.components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(
                    value
                        .to_string_lossy()
                        .trim_end_matches([' ', '.'])
                        .to_lowercase(),
                ),
                _ => None,
            })
            .collect()
    }

    let path = normalized_components(path);
    let other = normalized_components(other);
    path == other || path.starts_with(&other) || other.starts_with(&path)
}

impl PatchAction {
    fn path(&self) -> &Path {
        match self {
            Self::Write { path, .. } | Self::Delete { path } => path,
        }
    }
}

struct AppliedPatchAction {
    path: PathBuf,
    backup_name: Option<PathBuf>,
    installed: bool,
}

fn apply_patch_transaction(
    workspace_dir: &Dir,
    actions: Vec<PatchAction>,
    stage_failure: Option<usize>,
    commit_failure: Option<usize>,
) -> Result<()> {
    validate_patch_action_state(workspace_dir, &actions)?;
    let (transaction_name, transaction_dir) = create_patch_transaction_dir(workspace_dir)?;

    let result = apply_patch_transaction_inner(
        workspace_dir,
        &transaction_dir,
        &actions,
        stage_failure,
        commit_failure,
    );
    drop(transaction_dir);
    let remove_result = workspace_dir.remove_dir(&transaction_name);
    match (result, remove_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(cleanup_error)) => Err(anyhow!(
            "Patch committed but transaction cleanup failed: {cleanup_error}"
        )),
        (Err(error), Err(cleanup_error)) => Err(anyhow!(
            "{error}; transaction cleanup also failed: {cleanup_error}"
        )),
    }
}

fn validate_patch_action_state(workspace_dir: &Dir, actions: &[PatchAction]) -> Result<()> {
    for action in actions {
        match action {
            PatchAction::Write {
                path,
                create_new: true,
                ..
            } => {
                if ToolRuntime::workspace_path_exists_blocking(workspace_dir, path)? {
                    return Err(anyhow!(
                        "Cannot add '{}': file already exists",
                        path.display()
                    ));
                }
            }
            PatchAction::Write { path, .. } | PatchAction::Delete { path } => {
                ToolRuntime::ensure_workspace_regular_file_blocking(workspace_dir, path)?;
            }
        }
    }
    Ok(())
}

fn create_patch_transaction_dir(workspace_dir: &Dir) -> Result<(PathBuf, Dir)> {
    for _ in 0..32 {
        let name = PathBuf::from(format!(".ghostpwn-patch-{:016x}", rand::rng().next_u64()));
        match workspace_dir.create_dir(&name) {
            Ok(()) => return Ok((name.clone(), workspace_dir.open_dir_nofollow(name)?)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Err(anyhow!("Cannot allocate patch transaction directory"))
}

fn apply_patch_transaction_inner(
    workspace_dir: &Dir,
    transaction_dir: &Dir,
    actions: &[PatchAction],
    stage_failure: Option<usize>,
    commit_failure: Option<usize>,
) -> Result<()> {
    for (index, action) in actions.iter().enumerate() {
        if let PatchAction::Write {
            path,
            content,
            create_new,
        } = action
        {
            let stage_result = (|| -> Result<()> {
                if stage_failure == Some(index) {
                    return Err(anyhow!(
                        "Injected patch staging failure before action {index}"
                    ));
                }
                let stage_name = patch_stage_name(index);
                let mut options = OpenOptions::new();
                options
                    .write(true)
                    .create_new(true)
                    .follow(FollowSymlinks::No);
                let mut file = transaction_dir.open_with(&stage_name, &options)?;
                std::io::Write::write_all(&mut file, content.as_bytes())?;
                if !create_new {
                    let (source_parent, source_name) =
                        ToolRuntime::open_workspace_parent(workspace_dir, path, false)?;
                    let metadata = source_parent.symlink_metadata(source_name)?;
                    file.set_permissions(metadata.permissions())?;
                }
                std::io::Write::flush(&mut file)?;
                Ok(())
            })();
            if let Err(error) = stage_result {
                cleanup_patch_transaction(transaction_dir, actions)?;
                return Err(error);
            }
        }
    }

    let mut applied = Vec::<AppliedPatchAction>::new();
    let mut created_dirs = Vec::<PathBuf>::new();
    for (index, action) in actions.iter().enumerate() {
        let commit_result = (|| -> Result<()> {
            if commit_failure == Some(index) {
                return Err(anyhow!(
                    "Injected patch commit failure before action {index}"
                ));
            }

            let path = action.path();
            let (parent, file_name) =
                open_workspace_parent_tracking(workspace_dir, path, &mut created_dirs)?;
            let backup_name = if action.create_new() {
                match parent.symlink_metadata(&file_name) {
                    Err(err) if err.kind() == ErrorKind::NotFound => None,
                    Ok(_) => {
                        return Err(anyhow!(
                            "Cannot add '{}': file already exists",
                            path.display()
                        ));
                    }
                    Err(err) => return Err(err.into()),
                }
            } else {
                let metadata = parent.symlink_metadata(&file_name)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(anyhow!("Path '{}' is not a regular file", path.display()));
                }
                let backup_name = patch_backup_name(index);
                parent.rename(&file_name, transaction_dir, &backup_name)?;
                Some(backup_name)
            };

            applied.push(AppliedPatchAction {
                path: path.to_path_buf(),
                backup_name,
                installed: false,
            });
            if matches!(action, PatchAction::Write { .. }) {
                transaction_dir.rename(patch_stage_name(index), &parent, &file_name)?;
                applied.last_mut().expect("applied action exists").installed = true;
            }
            Ok(())
        })();

        if let Err(error) = commit_result {
            let rollback = rollback_patch_transaction(
                workspace_dir,
                transaction_dir,
                &mut applied,
                &created_dirs,
            );
            return match rollback {
                Ok(()) => {
                    cleanup_patch_transaction(transaction_dir, actions)?;
                    Err(error)
                }
                Err(rollback_error) => Err(anyhow!(
                    "{error}; patch rollback also failed: {rollback_error}"
                )),
            };
        }
    }
    cleanup_patch_transaction(transaction_dir, actions)
}

fn open_workspace_parent_tracking(
    workspace_dir: &Dir,
    path: &Path,
    created_dirs: &mut Vec<PathBuf>,
) -> Result<(Dir, PathBuf)> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("Path '{}' does not name a file", path.display()))?;
    let mut dir = workspace_dir.try_clone()?;
    let mut relative = PathBuf::new();
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            let Component::Normal(part) = component else {
                return Err(anyhow!("Invalid workspace path '{}'", path.display()));
            };
            relative.push(part);
            match dir.open_dir_nofollow(part) {
                Ok(next) => dir = next,
                Err(err) if err.kind() == ErrorKind::NotFound => {
                    dir.create_dir(part)?;
                    created_dirs.push(relative.clone());
                    dir = dir.open_dir_nofollow(part)?;
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
    Ok((dir, PathBuf::from(file_name)))
}

fn rollback_patch_transaction(
    workspace_dir: &Dir,
    transaction_dir: &Dir,
    applied: &mut [AppliedPatchAction],
    created_dirs: &[PathBuf],
) -> Result<()> {
    let mut errors = Vec::<String>::new();
    for action in applied.iter().rev() {
        match ToolRuntime::open_workspace_parent(workspace_dir, &action.path, false) {
            Ok((parent, file_name)) => {
                if action.installed
                    && let Err(error) = parent.remove_file(&file_name)
                {
                    errors.push(format!("remove '{}': {error}", action.path.display()));
                    continue;
                }
                if let Some(backup_name) = &action.backup_name
                    && let Err(error) = transaction_dir.rename(backup_name, &parent, &file_name)
                {
                    errors.push(format!("restore '{}': {error}", action.path.display()));
                }
            }
            Err(error) => errors.push(format!("open '{}': {error}", action.path.display())),
        }
    }
    for path in created_dirs.iter().rev() {
        if let Err(error) = workspace_dir.remove_dir(path)
            && error.kind() != ErrorKind::NotFound
        {
            errors.push(format!("remove directory '{}': {error}", path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("; ")))
    }
}

fn cleanup_patch_transaction(transaction_dir: &Dir, actions: &[PatchAction]) -> Result<()> {
    for index in 0..actions.len() {
        for name in [patch_stage_name(index), patch_backup_name(index)] {
            if let Err(error) = transaction_dir.remove_file(name)
                && error.kind() != ErrorKind::NotFound
            {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

fn patch_stage_name(index: usize) -> PathBuf {
    PathBuf::from(format!("stage-{index}"))
}

fn patch_backup_name(index: usize) -> PathBuf {
    PathBuf::from(format!("backup-{index}"))
}

impl PatchAction {
    fn create_new(&self) -> bool {
        matches!(
            self,
            Self::Write {
                create_new: true,
                ..
            }
        )
    }
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

    Ok(())
}

struct PublicDnsResolver;

impl Resolve for PublicDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = tokio::task::spawn_blocking(move || resolve_public_host(&host))
                .await
                .map_err(|err| IoError::other(format!("DNS resolver task failed: {err}")))??;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn resolve_public_host(host: &str) -> std::io::Result<Vec<SocketAddr>> {
    let addrs = (host, 0).to_socket_addrs()?.collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(IoError::new(
            ErrorKind::NotFound,
            format!("host '{host}' did not resolve"),
        ));
    }
    if addrs.iter().any(|addr| !is_public_ip(addr.ip())) {
        return Err(IoError::new(
            ErrorKind::PermissionDenied,
            format!("host '{host}' resolves to a non-public address"),
        ));
    }
    Ok(addrs)
}

async fn read_capped(mut reader: impl AsyncRead + Unpin, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 8_192];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        truncated |= read > remaining;
    }

    Ok((output, truncated))
}

async fn read_text_file(path: &Path) -> Result<String> {
    let file = fs::File::open(path).await?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_TEXT_FILE_BYTES {
        return Err(anyhow!(
            "Text file '{}' exceeds the {}-byte safety limit",
            path.display(),
            MAX_TEXT_FILE_BYTES
        ));
    }
    String::from_utf8(bytes)
        .map_err(|err| anyhow!("Text file '{}' is not valid UTF-8: {}", path.display(), err))
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
#[path = "../tests/tools.rs"]
mod tests;
