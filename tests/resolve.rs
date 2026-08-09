use gitfriend::config::Config;
use gitfriend::resolve::{resolve_url, Reason};

/// Mirrors the real shape: two accounts on the same host, distinguished only
/// by org (R3).
const TWO_GITHUB_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    email = "me@work.example"
    match = ["github.com/WorkOrg/**"]
"#;

/// Digilope's remotes look like `gitea@app-gitea.digilope.com:daniel/repo.git`
/// -- scp-style, no scheme, `:` instead of `/`, and a user prefix.
const SSH_ACCOUNT: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Digilope"
    provider = "gitea"
    email = "me@digilope.example"
    match = ["app-gitea.digilope.com/**"]
"#;

/// The real nesting hazard: KitchenCloud's org sits inside the pattern that
/// covers Profound, so both accounts match. Declared broad-first on purpose --
/// the answer must not depend on declaration order.
const OVERLAPPING_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Profound"
    provider = "github"
    email = "me@profound.example"
    match = ["github.com/Profound-*/**"]

    [[accounts]]
    name = "KitchenCloud"
    provider = "github"
    email = "me@kitchencloud.example"
    match = ["github.com/Profound-Kitchen/**"]
"#;

/// Two accounts claiming the same org with equal specificity. There is no
/// right answer, so there must be no answer.
const AMBIGUOUS_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    match = ["github.com/Shared/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    email = "me@work.example"
    match = ["github.com/Shared/**"]
"#;

#[test]
fn equally_specific_matches_on_different_accounts_are_an_error() {
    let config = Config::parse(AMBIGUOUS_ACCOUNTS).unwrap();

    let error = resolve_url(&config, "https://github.com/Shared/repo.git")
        .expect_err("an ambiguous match must not silently pick an account");

    let message = error.to_string();
    assert!(
        message.contains("Personal") && message.contains("Work"),
        "the error must name both candidates so it can be fixed; got: {message}"
    );
}

#[test]
fn the_more_specific_pattern_wins_when_two_accounts_match() {
    let config = Config::parse(OVERLAPPING_ACCOUNTS).unwrap();

    let resolved = resolve_url(&config, "https://github.com/Profound-Kitchen/app.git").unwrap();

    assert_eq!(resolved.account.name, "KitchenCloud");
}

#[test]
fn resolves_an_scp_style_ssh_url() {
    let config = Config::parse(SSH_ACCOUNT).unwrap();

    let resolved = resolve_url(&config, "gitea@app-gitea.digilope.com:daniel/site.git").unwrap();

    assert_eq!(resolved.account.name, "Digilope");
    assert_eq!(resolved.reason, Reason::UrlMatch);
}

#[test]
fn resolves_an_https_url_to_the_account_owning_that_org() {
    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();

    let resolved = resolve_url(&config, "https://github.com/WorkOrg/somerepo.git").unwrap();

    assert_eq!(resolved.account.name, "Work");
    assert_eq!(resolved.reason, Reason::UrlMatch);
}
