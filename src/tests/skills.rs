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
        Some("---\nname: path-traversal\ndescription: Directory traversal testing\n---\n# Steps\n")
    );
}
