use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::credential::{respond, Request};
use gitwho::sources::{Captured, MapRunner};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal-login"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    login = "work-login"
    email = "me@work.example"
    match = ["github.com/WorkOrg/**"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn gh_key(login: &str) -> String {
    MapRunner::key(
        "gh",
        &["auth", "token", "--hostname", "github.com", "--user", login],
    )
}

fn tools() -> MapRunner {
    MapRunner::new(HashMap::from([
        (gh_key("personal-login"), ok("personal-token")),
        (gh_key("work-login"), ok("work-token")),
    ]))
}

fn only_personal() -> MapRunner {
    MapRunner::new(HashMap::from([(
        gh_key("personal-login"),
        ok("personal-token"),
    )]))
}

/// git speaks a line-oriented protocol on stdin, terminated by a blank line.
/// With `credential.useHttpPath=true` the request carries the org, which is
/// what lets an account be chosen before any repo exists on disk.
#[test]
fn answers_a_request_with_the_token_for_the_org_in_the_url() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\nhost=github.com\npath=WorkOrg/thing.git\n");

    let credential = respond(&config, &tools(), &request, None).unwrap();

    assert_eq!(credential.password, "work-token");
}

#[test]
fn a_login_gh_lacks_fails_instead_of_falling_back_to_another_account() {
    // The exact regression this project exists to remove: an account is
    // identified correctly, but gh has no token for its login. Handing over
    // the default account's token here would authenticate as the wrong person
    // while looking like success (R8).
    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\nhost=github.com\npath=WorkOrg/thing.git\n");

    let error = respond(&config, &only_personal(), &request, None)
        .expect_err("a missing login must not fall back to another account");

    let message = error.to_string();
    assert!(
        !message.contains("personal-token"),
        "the error leaked another account's token: {message}"
    );
    assert!(
        message.contains("Work") && message.contains("work-login"),
        "the error must name the account and login to fix; got: {message}"
    );
}

#[test]
fn an_unknown_host_gets_no_credential_at_all() {
    // A host nothing claims must not receive the default account's token.
    // Handing GitHub credentials to an unrelated server is the cross-account
    // leak in R11.
    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\nhost=evil.example.com\npath=someone/repo.git\n");

    let error = respond(&config, &tools(), &request, None)
        .expect_err("an unclaimed host must not receive any token");

    let message = error.to_string();
    assert!(
        !message.contains("personal-token") && !message.contains("work-token"),
        "the error leaked a token: {message}"
    );
}

#[test]
fn a_low_confidence_resolution_does_not_release_a_token() {
    // If git tells us too little to identify a host, we fall back to the
    // working directory -- which may itself resolve to nothing and land on
    // the declared default. Releasing a token on that basis is right only by
    // luck, and being right by luck is the failure mode R8 names.
    let dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();

    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\n");

    let error = respond(&config, &tools(), &request, Some(dir.path()))
        .expect_err("a default-account resolution must not release a token");

    let message = error.to_string();
    assert!(
        !message.contains("personal-token") && !message.contains("work-token"),
        "a token leaked on a low-confidence resolution: {message}"
    );
}

#[test]
fn an_unmatched_remote_does_not_release_a_token_either() {
    // A third-party clone resolves to the default account so `gh` still works
    // there -- but that is not a basis for handing over a credential, and an
    // unmatched remote is also what a forgotten pattern looks like (R8, R11).
    let dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/microsoft/vscode.git",
        ])
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();

    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\n");

    let error = respond(&config, &tools(), &request, Some(dir.path()))
        .expect_err("an unmatched remote must not release a token");

    assert!(
        !error.to_string().contains("personal-token"),
        "a token leaked on an unmatched remote: {error}"
    );
}

/// Forgejo checks the username against the token's owner; GitHub ignores it.
#[test]
fn the_username_is_the_accounts_login_not_its_gitwho_name() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\nhost=github.com\npath=WorkOrg/thing.git\n");
    let credential = respond(&config, &tools(), &request, None).unwrap();
    assert_eq!(credential.username, "work-login");
    assert_eq!(credential.password, "work-token");
}
