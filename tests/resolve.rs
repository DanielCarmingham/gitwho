use gitwho::config::Config;
use gitwho::resolve::{resolve_url, Reason};

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

/// SelfHosted's remotes look like `gitea@ssh.git.example.net:someone/repo.git`
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
    name = "SelfHosted"
    provider = "gitea"
    email = "you@example.net"
    match = ["ssh.git.example.net/**"]
"#;

/// The real nesting hazard: AcmeKitchen's org sits inside the pattern that
/// covers Acme, so both accounts match. Declared broad-first on purpose --
/// the answer must not depend on declaration order.
const OVERLAPPING_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Acme"
    provider = "github"
    email = "you@acme.example"
    match = ["github.com/acme-*/**"]

    [[accounts]]
    name = "AcmeKitchen"
    provider = "github"
    email = "you@kitchen.example"
    match = ["github.com/acme-kitchen/**"]
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

    let resolved = resolve_url(&config, "https://github.com/acme-kitchen/app.git").unwrap();

    assert_eq!(resolved.account.name, "AcmeKitchen");
}

#[test]
fn resolves_an_scp_style_ssh_url() {
    let config = Config::parse(SSH_ACCOUNT).unwrap();

    let resolved = resolve_url(&config, "gitea@ssh.git.example.net:someone/site.git").unwrap();

    assert_eq!(resolved.account.name, "SelfHosted");
    assert_eq!(resolved.reason, Reason::UrlMatch);
}

#[test]
fn resolves_an_https_url_to_the_account_owning_that_org() {
    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();

    let resolved = resolve_url(&config, "https://github.com/WorkOrg/somerepo.git").unwrap();

    assert_eq!(resolved.account.name, "Work");
    assert_eq!(resolved.reason, Reason::UrlMatch);
}

#[test]
fn every_reason_describes_itself_in_words_a_person_can_act_on() {
    // These strings are printed next to an account name before a secret is
    // written, so they have to distinguish "we found this" from "we guessed".
    assert_eq!(Reason::UrlMatch.describe(), "matched a remote URL");
    assert_eq!(Reason::OriginUrl.describe(), "matched the origin remote");
    assert_eq!(
        Reason::PathFallback.describe(),
        "matched a directory prefix"
    );
    assert_eq!(
        Reason::Unmatched.describe(),
        "no account claims this remote"
    );
    assert_eq!(Reason::Default.describe(), "the declared default");
}
