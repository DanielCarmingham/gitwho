//! The generated identity rules, checked against real git rather than against
//! expectations about it.
//!
//! `hasconfig:remote.*.url:` compares against the **literal** remote string, so
//! every spelling `sync` emits either matches or silently does not, and a miss
//! looks exactly like a repo with no rule: the identity falls through to a
//! directory rule or the default. The only way to know is to ask git.

use std::path::Path;
use std::process::Command;

use gitwho::config::Config;
use gitwho::sync;

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"
    gitName = "Your Name"

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
    url = "https://git.example.net"
    email = "you@example.net"
    match = ["git.example.net/**"]
"#;

/// The identity git settles on for a repo whose only remote is `url`, with the
/// generated rules as the global config and nothing else in scope.
fn email_for(remote_url: &str) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::parse(ACCOUNTS).unwrap();
    let generated = dir.path().join("generated");
    std::fs::create_dir_all(&generated).unwrap();

    let plan = sync::plan(&config, &generated, "/nowhere/gitwho");
    for file in &plan.files {
        std::fs::create_dir_all(file.path.parent().unwrap()).unwrap();
        std::fs::write(&file.path, &file.contents).unwrap();
    }

    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["remote", "add", "origin", remote_url]);

    let includes = generated.join("includes.gitconfig");
    let output = Command::new("git")
        .args(["config", "--get", "user.email"])
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", &includes)
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn a_plain_https_remote_gets_its_accounts_identity() {
    assert_eq!(
        email_for("https://github.com/Personal/thing.git").as_deref(),
        Some("me@example.com")
    );
}

/// The bug this file exists for. A remote spelled `https://someone@host/...`
/// matched nothing, so identity came from a directory rule or the default --
/// path-keyed identity, which is the failure R1 and R2 exist to prevent.
#[test]
fn an_https_remote_carrying_a_username_still_gets_its_accounts_identity() {
    assert_eq!(
        email_for("https://someone@github.com/Personal/thing.git").as_deref(),
        Some("me@example.com")
    );
}

/// The same shape, and how it turns up in the wild: a token pasted into the
/// remote URL. Bad practice, but it must not silently change who you commit as.
#[test]
fn an_https_remote_carrying_a_token_still_gets_its_accounts_identity() {
    assert_eq!(
        email_for("https://someone:sometoken@github.com/Personal/thing.git").as_deref(),
        Some("me@example.com")
    );
}

#[test]
fn the_ssh_spellings_still_match() {
    for url in [
        "git@github.com:Personal/thing.git",
        "ssh://git@github.com/Personal/thing.git",
        "ssh://github.com/Personal/thing.git",
    ] {
        assert_eq!(
            email_for(url).as_deref(),
            Some("me@example.com"),
            "{url} did not match"
        );
    }
}

/// A host-wide pattern has to reach `org/repo`, which is the case the `:*/`
/// spelling exists for.
#[test]
fn a_host_wide_pattern_reaches_a_nested_path_in_every_spelling() {
    for url in [
        "https://git.example.net/someone/site.git",
        "https://someone@git.example.net/someone/site.git",
        "gitea@git.example.net:someone/site.git",
        "ssh://gitea@git.example.net/someone/site.git",
    ] {
        assert_eq!(
            email_for(url).as_deref(),
            Some("you@example.net"),
            "{url} did not match"
        );
    }
}

/// A remote nobody claims must pick up no identity at all, rather than
/// borrowing one.
#[test]
fn an_unclaimed_remote_matches_nothing() {
    assert_eq!(email_for("https://github.com/Stranger/theirs.git"), None);
}
