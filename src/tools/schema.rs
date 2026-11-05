use serde_json::{Value, json};

pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

pub fn definitions() -> Vec<ToolDefinition> {
    let string = json!({"type": "string"});
    let integer = json!({"type": "integer", "minimum": 0});
    let boolean = json!({"type": "boolean"});
    let edit = json!({
        "type": "object",
        "properties": {"oldString": string, "newString": string, "replaceAll": boolean},
        "required": ["oldString", "newString"],
        "additionalProperties": false
    });
    vec![
        definition("listSkills", "List available local skills.", json!({}), &[]),
        definition(
            "searchSkills",
            "Find skills for a task before using domain tools.",
            json!({"query": string, "limit": integer}),
            &["query"],
        ),
        definition(
            "readSkill",
            "Read a skill by its exact name.",
            json!({"name": string}),
            &["name"],
        ),
        definition(
            "readFile",
            "Read a workspace text file. Offset is zero-based; maxLines defaults to 200.",
            json!({"path": string, "offset": integer, "maxLines": integer, "lineNumbers": boolean}),
            &["path"],
        ),
        definition(
            "listDirectory",
            "List a workspace directory.",
            json!({"path": string}),
            &[],
        ),
        definition(
            "searchFiles",
            "Find workspace files by glob pattern.",
            json!({"pattern": string, "path": string, "excludeGenerated": boolean}),
            &["pattern"],
        ),
        definition(
            "grep",
            "Search workspace file contents with a regular expression.",
            json!({"pattern": string, "path": string, "glob": string, "excludeGenerated": boolean}),
            &["pattern"],
        ),
        definition(
            "runCommand",
            "Run an approved shell command. This is not an OS sandbox. Timeout is in milliseconds, at most 120000.",
            json!({"command": string, "cwd": string, "timeout": {"type": "integer", "minimum": 1, "maximum": 120000}}),
            &["command"],
        ),
        definition(
            "auditDependencies",
            "Query OSV for public packages in workspace Cargo.lock files. Requires approval.",
            json!({"path": string}),
            &[],
        ),
        definition(
            "fileInfo",
            "Read workspace file metadata.",
            json!({"path": string}),
            &["path"],
        ),
        definition(
            "generateDiff",
            "Preview a proposed replacement for an existing workspace file.",
            json!({"path": string, "content": string}),
            &["path", "content"],
        ),
        definition(
            "writeFile",
            "Write a workspace file after approval.",
            json!({"path": string, "content": string}),
            &["path", "content"],
        ),
        definition(
            "editFile",
            "Replace exact text in a workspace file after approval.",
            json!({"path": string, "oldString": string, "newString": string, "replaceAll": boolean}),
            &["path", "oldString", "newString"],
        ),
        definition(
            "multiEdit",
            "Apply ordered exact text replacements to one file after approval.",
            json!({"path": string, "edits": {"type": "array", "items": edit}}),
            &["path", "edits"],
        ),
        definition(
            "applyPatch",
            "Apply a patch after approval. Use *** Begin Patch, *** Add File:, *** Update File:, or *** Delete File:, and *** End Patch markers.",
            json!({"patchText": string}),
            &["patchText"],
        ),
        definition(
            "webFetch",
            "Fetch a public HTTP or HTTPS URL with a bounded response.",
            json!({"url": string, "maxBytes": integer}),
            &["url"],
        ),
        definition(
            "webSearch",
            "Search the public web.",
            json!({"query": string, "count": integer}),
            &["query"],
        ),
    ]
}

fn definition(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
) -> ToolDefinition {
    ToolDefinition {
        name,
        description,
        parameters: json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false}),
    }
}

pub fn response_tools() -> Vec<Value> {
    definitions()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function", "name": tool.name, "description": tool.description,
                "parameters": tool.parameters, "strict": false
            })
        })
        .collect()
}

pub fn chat_tools() -> Vec<Value> {
    definitions().into_iter().map(|tool| json!({
        "type": "function", "function": {"name": tool.name, "description": tool.description, "parameters": tool.parameters}
    })).collect()
}

pub fn validate_call(call: &crate::models::ToolCall) -> anyhow::Result<()> {
    let tool = definitions()
        .into_iter()
        .find(|tool| tool.name == call.name)
        .ok_or_else(|| anyhow::anyhow!("Unknown tool '{}'", call.name))?;
    validate_value(&call.arguments, &tool.parameters, "arguments")
}

fn validate_value(value: &Value, schema: &Value, path: &str) -> anyhow::Result<()> {
    let valid = match schema["type"].as_str() {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_u64().is_some(),
        Some("boolean") => value.is_boolean(),
        _ => false,
    };
    anyhow::ensure!(valid, "Invalid type for {path}");
    if let Some(object) = value.as_object() {
        for required in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            anyhow::ensure!(
                object.contains_key(required),
                "Missing required argument {path}.{required}"
            );
        }
        for (key, value) in object {
            let property = &schema["properties"][key];
            anyhow::ensure!(!property.is_null(), "Unknown argument {path}.{key}");
            validate_value(value, property, &format!("{path}.{key}"))?;
        }
    }
    if let Some(array) = value.as_array() {
        for (index, item) in array.iter().enumerate() {
            validate_value(item, &schema["items"], &format!("{path}[{index}]"))?;
        }
    }
    if let Some(number) = value.as_u64() {
        anyhow::ensure!(
            schema["minimum"].as_u64().is_none_or(|min| number >= min),
            "{path} is below its minimum"
        );
        anyhow::ensure!(
            schema["maximum"].as_u64().is_none_or(|max| number <= max),
            "{path} exceeds its maximum"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_call;
    use crate::models::ToolCall;
    use serde_json::json;

    #[test]
    fn validates_nested_edits_and_rejects_invalid_types_and_unknown_fields() {
        let call = |arguments| ToolCall {
            name: "multiEdit".to_string(),
            arguments,
        };
        assert!(
            validate_call(&call(
                json!({"path":"x", "edits":[{"oldString":"a", "newString":"b"}]})
            ))
            .is_ok()
        );
        for arguments in [
            json!({"path":"x", "edits":[{"oldString":"a"}]}),
            json!({"path":"x", "edits":[{"oldString":null,"newString":"b"}]}),
            json!({"path":"x", "edits":[], "unexpected":true}),
        ] {
            assert!(validate_call(&call(arguments)).is_err());
        }
        for timeout in [json!("30000"), json!(-1), json!(120001)] {
            assert!(
                validate_call(&ToolCall {
                    name: "runCommand".to_string(),
                    arguments: json!({"command":"true", "timeout":timeout})
                })
                .is_err()
            );
        }
    }
}
