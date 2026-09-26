use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use gitwho::config::Config;
use gitwho::doctor::{self, GitWiring, Level, Store};
use gitwho::sources::{Captured, MapRunner};
use tempfile::TempDir;

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"
    gitName = "Test Person"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    login = "selfhosted"
    url = "https://ssh.git.example.net"
    email = "you@example.net"
    match = ["ssh.git.example.net/**"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

/// gh and tea both holding the logins `ACCOUNTS` names.
fn stocked_runner() -> MapRunner {
    MapRunner::new(HashMap::from([
        (
            MapRunner::key(
                "gh",
                &[
                    "auth",
                    "token",
                    "--hostname",
                    "github.com",
                    "--user",
                    "personal",
                ],
            ),
            ok("personal-token"),
        ),
        (
            MapRunner::key("tea", &["login", "ls", "-o", "json"]),
            ok(r#"[{"name":"sh","url":"https://ssh.git.example.net","user":"selfhosted"}]"#),
        ),
        (
            MapRunner::key("tea", &["login", "helper", "get"]),
            ok("password=gitea-token\n"),
        ),
    ]))
}

fn healthy_wiring() -> GitWiring {
    GitWiring {
        credential_helpers: vec!["gitwho credential".to_string()],
        github_helper: Some("gitwho credential".to_string()),
        use_http_path: Some(true),
        ..GitWiring::default()
    }
}

fn problems(findings: &[doctor::Finding]) -> Vec<&doctor::Finding> {
    findings
        .iter()
        .filter(|f| f.level == Level::Problem)
        .collect()
}

/// A mode, where the platform has such a thing. `std::os::unix` does not exist
/// on Windows, and `check_permissions` is itself a no-op there -- gating here
/// rather than at every call site keeps the rest of this file buildable, the
/// way `tests/secrets.rs` already does.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

/// A store laid out the way the cutover left the real machine: the directory
/// owner-only, `accounts.toml` owner-read-write.
///
/// The contents are inert placeholders -- this fixture is about modes, and
/// nothing here should ever be mistaken for, or shaped like, a token.
fn hardened_store() -> TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");

    // mkdtemp already gives 0700, but say it rather than rely on it.
    set_mode(dir.path(), 0o700);

    let path = dir.path().join("accounts.toml");
    std::fs::write(&path, b"# placeholder\n").unwrap();
    // The developer's umask decides what `write` produces (0644 here), so
    // the mode has to be set explicitly for the fixture to mean anything.
    set_mode(&path, 0o600);

    dir
}

fn store_at(dir: &Path) -> Store {
    Store {
        dir: dir.to_path_buf(),
        config: dir.join("accounts.toml"),
        owner: doctor::current_uid(),
    }
}

fn permission_problems(findings: &[doctor::Finding]) -> Vec<&doctor::Finding> {
    problems(findings)
        .into_iter()
        .filter(|f| f.check == "permissions")
        .collect()
}

/// Run against a store whose permissions are already correct, so a test about
/// some other check is not quietly answering a permissions question it did not
/// ask.
fn run_with(
    config: &Config,
    runner: &dyn gitwho::sources::Runner,
    ambient: &BTreeMap<String, String>,
    wiring: &GitWiring,
) -> Vec<doctor::Finding> {
    // Bound with a `let`: as a temporary it would drop before `run` stats
    // anything, and every path would silently read as nonexistent.
    let dir = hardened_store();
    doctor::run(config, runner, ambient, wiring, &store_at(dir.path()))
}

#[test]
fn a_healthy_setup_reports_no_problems() {
    let config = Config::parse(ACCOUNTS).unwrap();

    let findings = run_with(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );

    assert!(
        problems(&findings).is_empty(),
        "expected a clean bill of health; got {:?}",
        problems(&findings)
    );
}

/// The before-picture of the cutover: a managed token sitting in the ambient
/// environment, inherited by every process launched from that shell.
#[test]
fn a_managed_variable_present_in_the_environment_is_reported() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let ambient = BTreeMap::from([("GH_TOKEN".to_string(), "leaked-token".to_string())]);

    let findings = run_with(&config, &stocked_runner(), &ambient, &healthy_wiring());

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
        credential_helpers: vec!["gitwho credential".to_string()],
        github_helper: Some("gitwho credential".to_string()),
        use_http_path: Some(false),
        ..GitWiring::default()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("useHttpPath")),
        "useHttpPath being off should be a problem; got {messages:?}"
    );
}

#[test]
fn a_credential_helper_that_is_not_gitwho_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        credential_helpers: vec!["manager".to_string()],
        github_helper: None,
        use_http_path: Some(true),
        ..GitWiring::default()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
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
        login = "personal"
        email = "me@example.com"
        match = ["github.com/Personal/**"]
    "#,
    )
    .unwrap();

    let findings = run_with(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
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
        login = "personal"
        email = "me@example.com"
        match = ["github.com/Shared/**"]

        [[accounts]]
        name = "Other"
        provider = "github"
        login = "other"
        email = "other@example.com"
        match = ["github.com/Shared/**"]
    "#,
    )
    .unwrap();

    let findings = run_with(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("github.com/Shared/**")),
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
        login = "personal"
        email = "me@example.com"
        match = ["github.com/Personal/**"]

        [[accounts]]
        name = "Work"
        provider = "github"
        login = "work"
        email = "me@work.example"
        match = ["github.com/WorkOrg/**"]
    "#,
    )
    .unwrap();
    let ambient = BTreeMap::from([("GH_TOKEN".to_string(), "leaked".to_string())]);

    let findings = run_with(&config, &stocked_runner(), &ambient, &healthy_wiring());

    let mentions = findings
        .iter()
        .filter(|f| f.check == "ambient" && f.message.contains("GH_TOKEN"))
        .count();
    assert_eq!(
        mentions, 1,
        "expected one line for GH_TOKEN, got {mentions}"
    );
}

#[test]
fn a_url_scoped_helper_bypassing_gitwho_is_a_problem() {
    // This machine's actual state: the global helper could be perfect, but
    // `[credential "https://github.com"]` overrides it outright, so github.com
    // is served by `gh auth git-credential` instead.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        credential_helpers: vec!["gitwho credential".to_string()],
        github_helper: Some("!/opt/homebrew/bin/gh auth git-credential".to_string()),
        use_http_path: Some(true),
        ..GitWiring::default()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
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
        login = "personal"
        email = "me@example.com"
        match = ["github.com/Personal/**"]
    "#,
    )
    .unwrap();

    let findings = run_with(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );

    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("author name")),
        "a missing author name should be a problem; got {messages:?}"
    );
}

/// The hardening applied by hand during the cutover, now guarded. Nothing
/// stops a later umask, editor, or backup restore from loosening these, and
/// the loss would otherwise be silent.
// Modes only exist where `check_permissions` does.
#[cfg(unix)]
#[test]
fn a_hardened_store_reports_no_permission_problems() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let dir = hardened_store();

    let findings = doctor::run(
        &config,
        &no_sources(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    assert!(
        permission_problems(&findings).is_empty(),
        "correct modes should raise nothing; got {:?}",
        permission_problems(&findings)
    );
}

/// The one that matters most: everything under the directory is only out of
/// reach because the directory itself is. `sync`'s generated `git/` subtree is
/// 0755, and this is what keeps it unreachable.
// Modes only exist where `check_permissions` does.
#[cfg(unix)]
#[test]
fn a_config_directory_that_is_not_owner_only_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let dir = hardened_store();
    set_mode(dir.path(), 0o755);

    let findings = doctor::run(
        &config,
        &no_sources(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    let messages: Vec<_> = permission_problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    let named = dir.path().display().to_string();
    assert!(
        messages
            .iter()
            .any(|m| m.contains(&named) && m.contains("0755") && m.contains("0700")),
        "the loosened directory, its mode and the expected one should all be named; got {messages:?}"
    );
}

/// accounts.toml is a redirect vector: whoever can write it can add a `match`
/// pattern for their own host and be handed a token.
// Modes only exist where `check_permissions` does.
#[cfg(unix)]
#[test]
fn a_group_or_world_readable_accounts_toml_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let dir = hardened_store();
    set_mode(&dir.path().join("accounts.toml"), 0o644);

    let findings = doctor::run(
        &config,
        &no_sources(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    let messages: Vec<_> = permission_problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("accounts.toml") && m.contains("0644") && m.contains("0600")),
        "the loosened config file and its mode should be named; got {messages:?}"
    );
}

/// `doctor` reports and `sync` fixes -- but nothing in gitwho fixes a mode,
/// and `accounts.toml` arrives by hand from the example. A finding that names
/// the stake without the remedy is one the operator learns to read past, which
/// is how the redirect vector this check exists for would later go unnoticed.
// Modes only exist where `check_permissions` does.
#[cfg(unix)]
#[test]
fn a_permission_finding_says_what_to_run_to_fix_it() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let dir = hardened_store();
    set_mode(&dir.path().join("accounts.toml"), 0o644);

    let findings = doctor::run(
        &config,
        &no_sources(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    let messages: Vec<_> = permission_problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("accounts.toml") && m.contains("chmod 600")),
        "the finding should carry the command that fixes it; got {messages:?}"
    );
}

// Modes only exist where `check_permissions` does.
#[cfg(unix)]
#[test]
fn a_store_owned_by_someone_else_is_a_problem() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let dir = hardened_store();

    // Chowning to another user needs root, so the expectation is what varies.
    // wrapping_add rather than 0, which would pass by accident under root.
    let mut store = store_at(dir.path());
    store.owner = doctor::current_uid().wrapping_add(1);

    let findings = doctor::run(
        &config,
        &no_sources(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store,
    );

    let messages: Vec<_> = permission_problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("accounts.toml") && m.contains("owned by")),
        "accounts.toml should be reported as foreign-owned; got {messages:?}"
    );
    let named = dir.path().display().to_string();
    assert!(
        messages
            .iter()
            .any(|m| m.contains(&named) && m.contains("owned by")),
        "the directory should be reported as foreign-owned; got {messages:?}"
    );
}

/// A runner with no answers at all.
///
/// For tests about something other than tokens: every token check it answers
/// fails loudly with "no reply", so a token cannot quietly appear to be fine.
fn no_sources() -> MapRunner {
    MapRunner::new(HashMap::new())
}

/// The two remotes a fork of a mirrored project has: one on each host, owned
/// by different accounts.
fn split_remotes() -> Vec<(String, String)> {
    vec![
        (
            "origin".to_string(),
            "https://github.com/Personal/tool.git".to_string(),
        ),
        (
            "upstream".to_string(),
            "ssh://git@ssh.git.example.net/acme/tool.git".to_string(),
        ),
    ]
}

fn identity_finding(findings: &[doctor::Finding]) -> Option<&doctor::Finding> {
    findings.iter().find(|f| f.check == "identity")
}

#[test]
fn remotes_owned_by_two_accounts_are_reported_with_the_one_that_wins() {
    // `includeIf "hasconfig:remote.*.url:"` fires if *any* remote matches, so
    // both accounts' rules apply and git's last-include-wins settles it --
    // measured on git 2.54.0. The identity follows declaration order, not
    // `origin`, and nothing else says so.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        remotes: split_remotes(),
        ..healthy_wiring()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    let finding = identity_finding(&findings).expect("two accounts on one repo must be reported");
    assert_eq!(finding.level, Level::Warn);
    for expected in ["Personal", "SelfHosted", "origin", "upstream"] {
        assert!(
            finding.message.contains(expected),
            "message must name {expected}: {}",
            finding.message
        );
    }
    // SelfHosted is declared second, so its include is applied last and wins.
    assert!(
        finding.message.contains("SelfHosted decides"),
        "the winning account must be named as the one that decides: {}",
        finding.message
    );
}

#[test]
fn a_repo_that_pins_its_own_identity_is_left_alone() {
    // Once the repo settles the question locally, include order no longer
    // decides anything -- and a warning that cannot be cleared is one people
    // learn to scroll past.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        remotes: split_remotes(),
        identity_pinned: true,
        ..healthy_wiring()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    assert!(
        identity_finding(&findings).is_none(),
        "a pinned repo is not ambiguous: {:?}",
        identity_finding(&findings)
    );
}

#[test]
fn several_remotes_owned_by_one_account_are_not_ambiguous() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        remotes: vec![
            (
                "origin".to_string(),
                "https://github.com/Personal/tool.git".to_string(),
            ),
            (
                "fork".to_string(),
                "https://github.com/Personal/tool-fork.git".to_string(),
            ),
        ],
        ..healthy_wiring()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    assert!(
        identity_finding(&findings).is_none(),
        "one account cannot conflict with itself: {:?}",
        identity_finding(&findings)
    );
}

#[test]
fn an_unclaimed_remote_alongside_a_claimed_one_is_not_an_identity_conflict() {
    // A third-party clone added as a remote matches no account, so it applies
    // no identity rule and cannot compete with one.
    let config = Config::parse(ACCOUNTS).unwrap();
    let wiring = GitWiring {
        remotes: vec![
            (
                "origin".to_string(),
                "https://github.com/Personal/tool.git".to_string(),
            ),
            (
                "vendor".to_string(),
                "https://github.com/some-stranger/tool.git".to_string(),
            ),
        ],
        ..healthy_wiring()
    };

    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &wiring);

    assert!(
        identity_finding(&findings).is_none(),
        "an unmatched remote claims no identity: {:?}",
        identity_finding(&findings)
    );
}

#[test]
fn every_accounts_token_is_reported_as_a_fingerprint_never_a_value() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let findings = run_with(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
    );
    let tokens: Vec<_> = findings.iter().filter(|f| f.check == "tokens").collect();
    assert_eq!(tokens.len(), 2, "{findings:#?}");
    assert!(tokens.iter().all(|f| f.level == Level::Ok), "{tokens:#?}");
    for finding in tokens {
        assert!(
            !finding.message.contains("personal-token") && !finding.message.contains("gitea-token"),
            "doctor printed a token: {}",
            finding.message
        );
    }
}

#[test]
fn a_login_the_cli_does_not_hold_is_a_problem_that_says_what_to_run() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let findings = run_with(&config, &no_sources(), &BTreeMap::new(), &healthy_wiring());
    let messages: Vec<_> = problems(&findings)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Personal") && m.contains("gh auth login")),
        "{messages:?}"
    );
}

/// gh reads GITHUB_TOKEN when GH_TOKEN is empty, though no account names it.
#[test]
fn a_fallback_variable_in_the_environment_is_reported() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let ambient = BTreeMap::from([("GITHUB_TOKEN".to_string(), "leaked".to_string())]);
    let findings = run_with(&config, &stocked_runner(), &ambient, &healthy_wiring());
    let warned = findings
        .iter()
        .find(|f| f.level == Level::Warn && f.message.contains("GITHUB_TOKEN"))
        .expect("an ambient fallback variable should be reported");
    assert!(!warned.message.contains("leaked"), "{}", warned.message);
}

/// Left behind by 0.2: harmless, but nothing reads them now, and they still
/// hold every token they ever stored.
#[test]
fn a_leftover_secret_store_is_reported_as_unused_and_never_deleted() {
    let dir = hardened_store();
    std::fs::write(dir.path().join("secrets.age"), "x").unwrap();
    std::fs::write(dir.path().join("identity.key"), "x").unwrap();
    let config = Config::parse(ACCOUNTS).unwrap();

    let findings = doctor::run(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    for file in ["secrets.age", "identity.key"] {
        assert!(
            findings.iter().any(|f| f.level == Level::Warn
                && f.message.contains(file)
                && f.message.contains("no longer used")),
            "{file} not reported: {findings:#?}"
        );
        assert!(dir.path().join(file).exists(), "{file} was deleted");
    }
}
