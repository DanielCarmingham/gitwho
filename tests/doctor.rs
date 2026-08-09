use std::collections::{BTreeMap, HashMap};

use gitfriend::config::Config;
use gitfriend::doctor::{self, GitWiring, Level};
use gitfriend::secrets::EnvBackend;

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"
    gitName = "Test Person"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/Personal/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "Digilope"
    provider = "gitea"
    email = "me@digilope.example"
    match = ["app-gitea.digilope.com/**"]
    env = ["GITEA_TOKEN"]
"#;

fn stocked_backend() -> EnvBackend {
    EnvBackend::from_map(HashMap::from([
        ("GH_TOKEN_Personal".to_string(), "personal-token".to_string()),
        ("GITEA_TOKEN_Digilope".to_string(), "gitea-token".to_string()),
    ]))
}

fn healthy_wiring() -> GitWiring {
    GitWiring {
        credential_helpers: vec!["gitfriend credential".to_string()],
        github_helper: Some("gitfriend credential".to_string()),
        use_http_path: Some(true),
    }
}

fn problems(findings: &[doctor::Finding]) -> Vec<&doctor::Finding> {
    findings
        .iter()
        .filter(|f| f.level == Level::Problem)
        .collect()
}

#[test]
fn a_healthy_setup_reports_no_problems() {
    let config = Config::parse(ACCOUNTS).unwrap();

    let findings = doctor::run(
        &config,
        &stocked_backend(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );

    assert!(
        problems(&findings).is_empty(),
        "expected a clean bill of health; got {:?}",
        problems(&findings)
    );
}

#[test]
fn a_declared_secret_with_no_stored_value_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let half_stocked = EnvBackend::from_map(HashMap::from([(
        "GH_TOKEN_Personal".to_string(),
        "personal-token".to_string(),
    )]));

    let findings = doctor::run(&config, &half_stocked, &BTreeMap::new(), &healthy_wiring());

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("Digilope") && m.contains("GITEA_TOKEN")),
        "the missing secret should be named; got {messages:?}"
    );
}

/// The before-picture of the cutover: a managed token sitting in the ambient
/// environment, inherited by every process launched from that shell.
#[test]
fn a_managed_variable_present_in_the_environment_is_reported() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let ambient = BTreeMap::from([("GH_TOKEN".to_string(), "leaked-token".to_string())]);

    let findings = doctor::run(&config, &stocked_backend(), &ambient, &healthy_wiring());

    let reported = findings.iter().find(|f| f.message.contains("GH_TOKEN"));
    let reported = reported.expect("an ambient managed variable should be reported");
    assert!(
        !reported.message.contains("leaked-token"),
        "doctor printed a token value: {}",
        reported.message
    );
}

#[test]
fn github_without_use_http_path_is_a_problem() {
    // Without it the helper only ever sees `github.com`, so every GitHub
    // account resolves identically -- the failure would be silent and total.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        credential_helpers: vec!["gitfriend credential".to_string()],
        github_helper: Some("gitfriend credential".to_string()),
        use_http_path: Some(false),
    };

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("useHttpPath")),
        "useHttpPath being off should be a problem; got {messages:?}"
    );
}

#[test]
fn a_credential_helper_that_is_not_gitfriend_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        credential_helpers: vec!["manager".to_string()],
        github_helper: None,
        use_http_path: Some(true),
    };

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("credential.helper")),
        "a foreign credential helper should be a problem; got {messages:?}"
    );
}

#[test]
fn a_default_naming_an_undeclared_account_is_a_problem() {
    let config = Config::parse(
        r#"
        [defaults]
        account = "Ghost"
        gitName = "Test Person"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Personal/**"]
    "#,
    )
    .unwrap();

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &healthy_wiring());

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("Ghost")),
        "an undeclared default should be named; got {messages:?}"
    );
}

#[test]
fn two_accounts_claiming_the_same_pattern_is_a_problem() {
    // Resolution would refuse at run time; better to say so up front.
    let config = Config::parse(
        r#"
        [defaults]
        account = "Personal"
        gitName = "Test Person"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Shared/**"]

        [[accounts]]
        name = "Other"
        provider = "github"
        email = "other@example.com"
        match = ["github.com/Shared/**"]
    "#,
    )
    .unwrap();

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &healthy_wiring());

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("github.com/Shared/**")),
        "the duplicated pattern should be named; got {messages:?}"
    );
}

#[test]
fn a_variable_shared_by_several_accounts_is_reported_once() {
    // GH_TOKEN is declared by every GitHub account, but there is only one of
    // it in the environment. Reporting it per-account turns one fact into a
    // wall of identical lines.
    // Two GitHub accounts, both declaring GH_TOKEN -- the real shape on this
    // machine, where three do.
    let config = Config::parse(
        r#"
        [defaults]
        account = "Personal"
        gitName = "Test Person"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        gitCredential = "GH_TOKEN"
        match = ["github.com/Personal/**"]
        env = ["GH_TOKEN"]

        [[accounts]]
        name = "Work"
        provider = "github"
        email = "me@work.example"
        gitCredential = "GH_TOKEN"
        match = ["github.com/WorkOrg/**"]
        env = ["GH_TOKEN"]
    "#,
    )
    .unwrap();
    let ambient = BTreeMap::from([("GH_TOKEN".to_string(), "leaked".to_string())]);

    let findings = doctor::run(&config, &stocked_backend(), &ambient, &healthy_wiring());

    let mentions = findings
        .iter()
        .filter(|f| f.check == "ambient" && f.message.contains("GH_TOKEN"))
        .count();
    assert_eq!(mentions, 1, "expected one line for GH_TOKEN, got {mentions}");
}

#[test]
fn a_url_scoped_helper_bypassing_gitfriend_is_a_problem() {
    // This machine's actual state: the global helper could be perfect, but
    // `[credential "https://github.com"]` overrides it outright, so github.com
    // is served by `gh auth git-credential` instead.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        credential_helpers: vec!["gitfriend credential".to_string()],
        github_helper: Some("!/opt/homebrew/bin/gh auth git-credential".to_string()),
        use_http_path: Some(true),
    };

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("github.com is served by")),
        "a URL-scoped override should be caught even with a correct global helper; got {messages:?}"
    );
}

#[test]
fn an_account_with_no_author_name_anywhere_is_a_problem() {
    // Generating `name = ` produces a gitconfig that makes commits fail. Seen
    // for real: the draft config declared no gitName, and sync emitted an
    // empty one without complaint.
    let config = Config::parse(
        r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Personal/**"]
    "#,
    )
    .unwrap();

    let findings = doctor::run(&config, &stocked_backend(), &BTreeMap::new(), &healthy_wiring());

    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("author name")),
        "a missing author name should be a problem; got {messages:?}"
    );
}
