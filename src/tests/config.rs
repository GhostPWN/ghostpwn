use super::{ProviderKind, resolve_provider_and_model};

#[test]
fn saved_provider_and_model_are_used() {
    let (provider, model) =
        resolve_provider_and_model(Some(ProviderKind::Copilot), Some("gpt-4o".to_string()));

    assert_eq!(provider, ProviderKind::Copilot);
    assert_eq!(model, "gpt-4o");
}

#[test]
fn default_model_is_used_without_saved_model() {
    let (provider, model) = resolve_provider_and_model(Some(ProviderKind::Anthropic), None);

    assert_eq!(provider, ProviderKind::Anthropic);
    assert_eq!(model, ProviderKind::Anthropic.default_model());
}

#[test]
fn google_default_is_used_without_saved_provider() {
    let (provider, model) = resolve_provider_and_model(None, None);

    assert_eq!(provider, ProviderKind::Google);
    assert_eq!(model, ProviderKind::Google.default_model());
}

#[test]
fn provided_provider_and_model_take_precedence() {
    let (provider, model) =
        resolve_provider_and_model(Some(ProviderKind::OpenAi), Some("gpt-5-mini".to_string()));

    assert_eq!(provider, ProviderKind::OpenAi);
    assert_eq!(model, "gpt-5-mini");
}

#[test]
fn codex_provider_is_parsed_and_has_separate_secret_key() {
    assert_eq!(ProviderKind::parse("codex"), Some(ProviderKind::Codex));
    assert_eq!(
        ProviderKind::parse("openai-codex"),
        Some(ProviderKind::Codex)
    );
    assert_eq!(ProviderKind::Codex.as_str(), "codex");
    assert_eq!(ProviderKind::Codex.env_key(), "CODEX_OAUTH_TOKEN");
    assert_eq!(ProviderKind::Codex.default_model(), "gpt-5.3-codex");
}
