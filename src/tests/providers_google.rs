use serde_json::json;

use super::parse_gemini_models;

#[test]
fn keeps_only_generate_content_models() {
    let body = json!({
        "models": [
            {
                "name": "models/gemini-2.5-flash",
                "supportedGenerationMethods": ["generateContent", "countTokens"]
            },
            {
                "name": "models/gemini-embedding-001",
                "supportedGenerationMethods": ["embedContent"]
            },
            { "name": "models/text-bison" }
        ]
    });

    assert_eq!(parse_gemini_models(&body), vec!["gemini-2.5-flash"]);
}
