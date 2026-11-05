use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};

use super::{
    PatchOp, ToolRuntime, canonical_tool_name, parse_apply_patch, path_arg, required_str, schema,
    validate_public_url,
};
use crate::models::{ToolCall, TurnCancellation};

pub(super) fn tool_requires_approval(name: &str) -> bool {
    matches!(
        canonical_tool_name(name),
        "runCommand" | "auditDependencies" | "writeFile" | "editFile" | "multiEdit" | "applyPatch"
    )
}

pub(super) fn audit_tool_allowed(name: &str, allow_mutations: bool) -> bool {
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

#[derive(Debug, Clone, Copy)]
pub enum ExecutionMode<'a> {
    Workspace,
    Audit {
        scope: &'a Path,
        allow_mutations: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    RequireApproval(&'static str),
    Deny(String),
}

pub struct PreparedCall<'a> {
    requested: ToolCall,
    call: ToolCall,
    targets: Vec<PathBuf>,
    mode: ExecutionMode<'a>,
    decision: PolicyDecision,
    approved: bool,
}

impl PreparedCall<'_> {
    pub fn decision(&self) -> &PolicyDecision {
        &self.decision
    }
    pub fn call(&self) -> &ToolCall {
        &self.call
    }

    pub fn approve(&mut self) {
        if matches!(self.decision, PolicyDecision::RequireApproval(_)) {
            self.approved = true;
        }
    }
}

impl ToolRuntime {
    pub(super) fn call_requires_approval(&self, call: &ToolCall) -> bool {
        if tool_requires_approval(&call.name) {
            return true;
        }

        matches!(canonical_tool_name(&call.name), "readFile" | "generateDiff")
            && path_arg(&call.arguments).is_some_and(|path| self.read_path_is_sensitive(path))
    }

    pub fn prepare_call<'a>(
        &self,
        requested: &ToolCall,
        mode: ExecutionMode<'a>,
    ) -> PreparedCall<'a> {
        let mut prepared = PreparedCall {
            requested: requested.clone(),
            call: requested.clone(),
            targets: Vec::new(),
            mode,
            decision: PolicyDecision::Allow,
            approved: false,
        };
        match self.resolve_policy_call(requested, mode) {
            Ok((call, targets)) => {
                prepared.decision = if self.call_requires_approval(&call)
                    || self.call_requires_approval(requested)
                {
                    PolicyDecision::RequireApproval(match call.name.as_str() {
                        "runCommand" => "Shell commands run without an OS sandbox",
                        "auditDependencies" => {
                            "Dependency checks send public package versions to OSV.dev"
                        }
                        "readFile" | "generateDiff" => "Sensitive file reads require approval",
                        _ => "File changes require approval",
                    })
                } else {
                    PolicyDecision::Allow
                };
                prepared.call = call;
                prepared.targets = targets;
            }
            Err(error) => prepared.decision = PolicyDecision::Deny(error.to_string()),
        }
        prepared
    }

    pub async fn execute_prepared(
        &self,
        prepared: PreparedCall<'_>,
        cancellation: &TurnCancellation,
    ) -> Result<Value> {
        ensure!(
            !cancellation.is_cancelled(),
            "Tool cancelled before execution"
        );
        check_approval(&prepared.decision, prepared.approved)?;
        // Resolve again after approval. A grant only covers the reviewed arguments and targets.
        let current = self.prepare_call(&prepared.requested, prepared.mode);
        check_approval(&current.decision, prepared.approved)?;
        ensure!(
            current.call == prepared.call && current.targets == prepared.targets,
            "Tool targets changed after policy review; submit the call again"
        );
        self.dispatch(&current.call, cancellation).await
    }

    fn resolve_policy_call(
        &self,
        requested: &ToolCall,
        mode: ExecutionMode<'_>,
    ) -> Result<(ToolCall, Vec<PathBuf>)> {
        let mut call = normalize_call(requested)?;
        let scope = match mode {
            ExecutionMode::Workspace => self.workspace_root.clone(),
            ExecutionMode::Audit {
                scope,
                allow_mutations,
            } => {
                ensure!(
                    audit_tool_allowed(&call.name, allow_mutations),
                    "unavailable in this audit mode"
                );
                self.resolve_in_workspace(path_text(scope)?)?
            }
        };
        if matches!(mode, ExecutionMode::Audit { .. }) {
            if matches!(
                call.name.as_str(),
                "listDirectory" | "searchFiles" | "grep" | "auditDependencies"
            ) && call.arguments.get("path").is_none()
            {
                call.arguments["path"] = json!(path_text(&scope)?);
            }
            if matches!(call.name.as_str(), "searchFiles" | "grep") {
                call.arguments["excludeGenerated"] = json!(true);
            }
            if call.name == "readFile" {
                call.arguments["lineNumbers"] = json!(true);
            }
        }
        schema::validate_call(&call)?;
        if call.name == "runCommand" {
            required_str(&call.arguments, "command")?;
        }
        let mut targets = vec![self.workspace_root.clone(), scope.clone()];
        let path_key = match call.name.as_str() {
            "readFile" | "listDirectory" | "searchFiles" | "grep" | "fileInfo"
            | "auditDependencies" | "generateDiff" | "writeFile" | "editFile" | "multiEdit" => {
                Some("path")
            }
            "runCommand" => Some("cwd"),
            _ => None,
        };
        if let Some(key) = path_key {
            let input = call
                .arguments
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(".");
            if matches!(
                call.name.as_str(),
                "readFile" | "fileInfo" | "generateDiff" | "writeFile" | "editFile" | "multiEdit"
            ) {
                required_str(&call.arguments, "path")?;
            }
            let resolved = match call.name.as_str() {
                "writeFile" => self.policy_mutation_target(input, false)?,
                "editFile" | "multiEdit" => self.policy_mutation_target(input, true)?,
                "fileInfo" => self.resolve_existing_or_missing_in_workspace(input)?,
                _ => self.resolve_in_workspace(input)?,
            };
            ensure!(
                resolved.starts_with(&scope),
                "Path '{}' is outside audit scope '{}'",
                resolved.display(),
                scope.display()
            );
            if matches!(
                call.name.as_str(),
                "readFile" | "generateDiff" | "editFile" | "multiEdit"
            ) {
                ensure!(
                    resolved.is_file(),
                    "Path '{}' is not a regular file",
                    resolved.display()
                );
            }
            if matches!(
                call.name.as_str(),
                "listDirectory" | "searchFiles" | "runCommand"
            ) {
                ensure!(
                    resolved.is_dir(),
                    "Path '{}' is not a directory",
                    resolved.display()
                );
            }
            if matches!(call.name.as_str(), "listDirectory" | "searchFiles" | "grep") {
                ensure!(
                    !self.is_sensitive_workspace_path(&resolved),
                    "Sensitive paths cannot be searched or listed"
                );
            }
            call.arguments[key] = json!(path_text(&resolved)?);
            targets.push(resolved);
        }
        if call.name == "applyPatch" {
            for operation in parse_apply_patch(required_str(&call.arguments, "patchText")?)? {
                let paths = match operation {
                    PatchOp::Add { path, .. } => vec![(path, false)],
                    PatchOp::Delete { path } => vec![(path, true)],
                    PatchOp::Update { path, move_to, .. } => {
                        let mut paths = vec![(path, true)];
                        if let Some(target) = move_to {
                            paths.push((target, false));
                        }
                        paths
                    }
                };
                for (path, must_exist) in paths {
                    let target = self.policy_mutation_target(&path, must_exist)?;
                    ensure!(
                        target.starts_with(&scope),
                        "Path '{}' is outside audit scope '{}'",
                        target.display(),
                        scope.display()
                    );
                    targets.push(target);
                }
            }
        }
        if call.name == "webFetch" {
            let url = reqwest::Url::parse(required_str(&call.arguments, "url")?)?;
            ensure!(
                matches!(url.scheme(), "http" | "https"),
                "Unsupported URL scheme '{}'",
                url.scheme()
            );
            validate_public_url(&url).map_err(|error| anyhow!(error))?;
            call.arguments["url"] = json!(url.as_str());
        }
        Ok((call, targets))
    }

    fn policy_mutation_target(&self, input: &str, must_exist: bool) -> Result<PathBuf> {
        let relative = self.workspace_relative_path(input)?;
        let mut path = self.workspace_root.clone();
        for part in relative.components() {
            path.push(part);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => ensure!(
                    !metadata.file_type().is_symlink(),
                    "Symlink paths are not allowed for file changes: '{}'",
                    path.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        if must_exist {
            self.resolve_in_workspace(input)
        } else {
            self.resolve_existing_or_missing_in_workspace(input)
        }
    }
}

fn check_approval(decision: &PolicyDecision, approved: bool) -> Result<()> {
    match decision {
        PolicyDecision::Deny(reason) => bail!("{reason}"),
        PolicyDecision::RequireApproval(_) if !approved => bail!("Tool requires user approval"),
        _ => Ok(()),
    }
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("Tool paths must be valid UTF-8"))
}

fn normalize_call(requested: &ToolCall) -> Result<ToolCall> {
    let mut call = requested.clone();
    call.name = canonical_tool_name(&call.name).to_string();
    ensure!(
        call.arguments.is_object(),
        "Tool arguments must be a JSON object"
    );
    let arguments = &mut call.arguments;
    for (canonical, aliases) in [
        ("path", &["file_path"][..]),
        ("oldString", &["old_string"][..]),
        ("newString", &["new_string"][..]),
        ("replaceAll", &["replace_all"][..]),
        ("patchText", &["patch_text", "patch"][..]),
    ] {
        normalize_aliases(arguments, canonical, aliases)?;
    }
    if call.name == "readFile" {
        normalize_aliases(arguments, "maxLines", &["limit"])?;
    }
    if call.name == "webFetch" {
        normalize_aliases(arguments, "maxBytes", &["max_bytes"])?;
    }
    if call.name == "searchFiles" {
        normalize_aliases(arguments, "path", &["cwd"])?;
    }
    if call.name == "multiEdit"
        && let Some(edits) = arguments.get_mut("edits").and_then(Value::as_array_mut)
    {
        for edit in edits {
            ensure!(edit.is_object(), "Each edit must be an object");
            for (canonical, alias) in [
                ("oldString", "old_string"),
                ("newString", "new_string"),
                ("replaceAll", "replace_all"),
            ] {
                normalize_aliases(edit, canonical, &[alias])?;
            }
        }
    }
    Ok(call)
}

fn normalize_aliases(arguments: &mut Value, canonical: &str, aliases: &[&str]) -> Result<()> {
    let object = arguments
        .as_object_mut()
        .ok_or_else(|| anyhow!("Tool arguments must be an object"))?;
    for alias in aliases {
        if let Some(value) = object.remove(*alias) {
            if let Some(existing) = object.get(canonical) {
                ensure!(
                    existing == &value,
                    "Conflicting arguments '{canonical}' and '{alias}'"
                );
            } else {
                object.insert(canonical.to_string(), value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/policy.rs"]
mod tests;
