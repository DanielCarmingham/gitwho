use gitwho::provider::{always_cleared, Provider, Value};
use serde::Deserialize;

#[derive(Deserialize)]
struct Holder {
    provider: Provider,
}

fn parse(value: &str) -> Result<Provider, toml::de::Error> {
    toml::from_str::<Holder>(&format!("provider = {value:?}")).map(|h| h.provider)
}

#[test]
fn github_hands_the_token_to_gh_and_to_github_mcp_server() {
    assert_eq!(
        Provider::Github.variables(),
        &[
            ("GH_TOKEN", Value::Token),
            ("GITHUB_PERSONAL_ACCESS_TOKEN", Value::Token),
        ]
    );
}

/// tea reads GITEA_TOKEN + GITEA_INSTANCE_URL; gitea-mcp reads
/// GITEA_ACCESS_TOKEN + GITEA_HOST. One account serves both from one token.
#[test]
fn gitea_hands_token_and_url_to_tea_and_to_gitea_mcp() {
    assert_eq!(
        Provider::Gitea.variables(),
        &[
            ("GITEA_TOKEN", Value::Token),
            ("GITEA_INSTANCE_URL", Value::Url),
            ("GITEA_ACCESS_TOKEN", Value::Token),
            ("GITEA_HOST", Value::Url),
        ]
    );
}

#[test]
fn forgejo_is_accepted_as_gitea() {
    assert_eq!(parse("forgejo").unwrap(), Provider::Gitea);
    assert_eq!(parse("gitea").unwrap(), Provider::Gitea);
    assert_eq!(parse("github").unwrap(), Provider::Github);
}

#[test]
fn an_unknown_provider_is_rejected_naming_the_valid_ones() {
    let message = parse("gitlab").unwrap_err().to_string();
    assert!(
        message.contains("github") && message.contains("gitea"),
        "{message}"
    );
}

#[test]
fn a_capitalised_provider_is_rejected_rather_than_guessed() {
    assert!(parse("GitHub").is_err());
}

#[test]
fn the_cli_that_holds_each_providers_token() {
    assert_eq!(Provider::Github.cli(), "gh");
    assert_eq!(Provider::Gitea.cli(), "tea");
}

/// Fallbacks are read by tools but never set by gitwho: gh reads GITHUB_TOKEN
/// when GH_TOKEN is empty, so a stray one in the shell would answer for the
/// wrong account unless it is cleared.
#[test]
fn always_cleared_is_every_provider_variable_plus_the_fallbacks() {
    let cleared = always_cleared();
    for provider in Provider::ALL {
        for (name, _) in provider.variables() {
            assert!(cleared.contains(name), "{name} is not cleared");
        }
    }
    for name in [
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        assert!(cleared.contains(name), "{name} is not cleared");
    }
    assert_eq!(cleared.len(), 9);
}
