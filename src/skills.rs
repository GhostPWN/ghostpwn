use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::fs;
use walkdir::WalkDir;

const MAX_SKILL_CONTENT_BYTES: usize = 200_000;
const DEFAULT_SKILL_SEARCH_LIMIT: usize = 8;
const MAX_SKILL_SEARCH_LIMIT: usize = 25;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct SkillRuntime {
    root: PathBuf,
}

impl SkillRuntime {
    pub fn new() -> Self {
        Self {
            root: env::var("GHOSTPWN_SKILLS_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    let root = PathBuf::from("skills");
                    let development_root = PathBuf::from("src/skills");
                    if !root.is_dir() && development_root.is_dir() {
                        return development_root;
                    }
                    root
                }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn list_tool(&self) -> Result<Value> {
        let skills = self.list().await?;
        Ok(json!({
            "root": self.root.display().to_string(),
            "count": skills.len(),
            "skills": skills,
        }))
    }

    pub async fn search_tool(&self, args: &Value) -> Result<Value> {
        let query = required_str(args, "query")?;
        let limit = args
            .get("limit")
            .or_else(|| args.get("count"))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_SKILL_SEARCH_LIMIT)
            .clamp(1, MAX_SKILL_SEARCH_LIMIT);
        let matches = self.search(query, limit).await?;

        Ok(json!({
            "query": query,
            "matches": matches,
        }))
    }

    pub async fn read_tool(&self, args: &Value) -> Result<Value> {
        let selector = args
            .get("name")
            .or_else(|| args.get("skill"))
            .or_else(|| args.get("path"))
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| anyhow!("Missing required string argument 'name|skill|path'"))?;

        let skills = self.list().await?;
        let summary = skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(selector) || skill.path == selector)
            .ok_or_else(|| anyhow!("Skill '{}' was not found", selector))?;
        let path = self.resolve_skill_path(&summary.path)?;
        let bytes = fs::read(&path).await?;
        let truncated = bytes.len() > MAX_SKILL_CONTENT_BYTES;
        let content =
            String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_SKILL_CONTENT_BYTES)]).to_string();

        Ok(json!({
            "name": summary.name,
            "description": summary.description,
            "path": summary.path,
            "content": content,
            "truncated": truncated,
        }))
    }

    pub async fn prompt_section(&self) -> String {
        let count = self.count_skill_files().await.unwrap_or(0);
        if count == 0 {
            return "Skills:\n- No local skills were found. Set GHOSTPWN_SKILLS_DIR to enable them.\n"
                .to_string();
        }

        format!(
            "Skills:\n- {count} local skills are available in {}.\n- For cybersecurity, forensics, compliance, cloud security, vulnerability testing, incident response, or other specialized workflow tasks, call searchSkills with the user's intent before answering.\n- If searchSkills returns a relevant match, call readSkill for the best matching skill and follow its instructions before using other tools or giving the final answer.\n- If no match is relevant, continue normally and mention that no matching local skill applied only if useful.\n",
            self.root.display()
        )
    }

    async fn list(&self) -> Result<Vec<SkillSummary>> {
        if fs::metadata(&self.root).await.is_err() {
            return Ok(Vec::new());
        }

        let mut skills = Vec::<SkillSummary>::new();
        for entry in WalkDir::new(&self.root).follow_links(false) {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
                continue;
            }

            let path = entry.path();
            let content = match fs::read_to_string(path).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let relative = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .display()
                .to_string();
            skills.push(parse_skill_summary(&content, &relative));
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    async fn count_skill_files(&self) -> Result<usize> {
        if fs::metadata(&self.root).await.is_err() {
            return Ok(0);
        }

        Ok(WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file() && entry.file_name() == "SKILL.md")
            .count())
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillSummary>> {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut scored = self
            .list()
            .await?
            .into_iter()
            .filter_map(|skill| {
                let haystack = format!("{} {}", skill.name, skill.description).to_lowercase();
                let score = query_tokens
                    .iter()
                    .filter(|token| haystack.contains(token.as_str()))
                    .count();
                (score > 0).then_some((score, skill))
            })
            .collect::<Vec<(usize, SkillSummary)>>();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, skill)| skill)
            .collect())
    }

    fn resolve_skill_path(&self, relative: &str) -> Result<PathBuf> {
        if Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(anyhow!("Skill path '{}' is outside skills root", relative));
        }

        let path = self.root.join(relative);
        let canonical_root = self.root.canonicalize()?;
        let canonical_path = path.canonicalize()?;
        if !canonical_path.starts_with(canonical_root) {
            return Err(anyhow!("Skill path '{}' is outside skills root", relative));
        }
        Ok(canonical_path)
    }
}

fn parse_skill_summary(content: &str, relative_path: &str) -> SkillSummary {
    let metadata = parse_frontmatter(content);
    let fallback_name = Path::new(relative_path)
        .parent()
        .and_then(Path::file_name)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| relative_path.to_string());
    let description = metadata
        .description
        .or_else(|| first_body_sentence(content))
        .unwrap_or_default();

    SkillSummary {
        name: metadata.name.unwrap_or(fallback_name),
        description,
        path: relative_path.to_string(),
    }
}

#[derive(Default)]
struct SkillMetadata {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(content: &str) -> SkillMetadata {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return SkillMetadata::default();
    }

    let mut metadata = SkillMetadata::default();
    let mut current_key = None::<String>;

    for line in lines {
        if line == "---" {
            break;
        }

        if let Some((key, value)) = line.split_once(':')
            && !line.starts_with(' ')
            && !line.starts_with('\t')
        {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            current_key = Some(key.clone());
            match key.as_str() {
                "name" => metadata.name = non_empty(value),
                "description" => metadata.description = non_empty(value),
                _ => {}
            }
            continue;
        }

        if matches!(current_key.as_deref(), Some("description")) {
            let extra = line.trim();
            if !extra.is_empty()
                && let Some(description) = metadata.description.as_mut()
            {
                description.push(' ');
                description.push_str(extra);
            }
        }
    }

    metadata
}

fn first_body_sentence(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && *line != "---")
        .map(|line| line.chars().take(200).collect())
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("Missing required string argument '{}'", key))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::SkillRuntime;

    #[tokio::test]
    async fn list_reads_skill_frontmatter() {
        let root = tempdir().expect("tempdir");
        let skills = root.path().join("skills/example");
        fs::create_dir_all(&skills).expect("skills dir");
        fs::write(
            skills.join("SKILL.md"),
            "---\nname: example-skill\ndescription: First line\n  continued line\n---\n# Body\n",
        )
        .expect("skill");

        let runtime = SkillRuntime {
            root: root.path().join("skills"),
        };
        let result = runtime.list_tool().await.expect("list");
        assert_eq!(result["count"].as_u64(), Some(1));
        assert_eq!(result["skills"][0]["name"].as_str(), Some("example-skill"));
        assert_eq!(
            result["skills"][0]["description"].as_str(),
            Some("First line continued line")
        );
    }

    #[tokio::test]
    async fn search_and_read_skill_by_name() {
        let root = tempdir().expect("tempdir");
        let skills = root.path().join("skills/path-traversal");
        fs::create_dir_all(&skills).expect("skills dir");
        fs::write(
            skills.join("SKILL.md"),
            "---\nname: path-traversal\ndescription: Directory traversal testing\n---\n# Steps\n",
        )
        .expect("skill");

        let runtime = SkillRuntime {
            root: root.path().join("skills"),
        };
        let search = runtime
            .search_tool(&json!({ "query": "directory traversal" }))
            .await
            .expect("search");
        assert_eq!(
            search["matches"][0]["name"].as_str(),
            Some("path-traversal")
        );

        let read = runtime
            .read_tool(&json!({ "name": "path-traversal" }))
            .await
            .expect("read");
        assert_eq!(
            read["content"].as_str(),
            Some(
                "---\nname: path-traversal\ndescription: Directory traversal testing\n---\n# Steps\n"
            )
        );
    }
}
