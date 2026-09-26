//! Reading an account's token from its provider's own CLI.
//!
//! tea behaviour encoded here was measured against tea 0.15.1 on 2026-09-26
//! with fake logins: `login helper get` answers with the first login for a
//! host, ignoring the username asked for and the login marked default.

use std::collections::HashMap;

use gitwho::provider::Provider;
use gitwho::sources::{token, Captured, MapRunner, TokenOwner};

const TOKEN: &str = "tok_do_not_print";

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn refused(stderr: &str) -> Captured {
    Captured {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

fn github(login: &str) -> TokenOwner<'_> {
    TokenOwner {
        account: "Work",
        provider: Provider::Github,
        login,
        url: None,
    }
}

fn gitea<'a>(login: &'a str, url: &'a str) -> TokenOwner<'a> {
    TokenOwner {
        account: "SelfHosted",
        provider: Provider::Gitea,
        login,
        url: Some(url),
    }
}

fn gh_replying(login: &str, reply: Captured) -> MapRunner {
    MapRunner::new(HashMap::from([(
        MapRunner::key(
            "gh",
            &["auth", "token", "--hostname", "github.com", "--user", login],
        ),
        reply,
    )]))
}

/// tea listing `logins` as (name, url, user); its helper hands out TOKEN.
fn tea_listing(logins: &[(&str, &str, &str)]) -> MapRunner {
    let listed: Vec<_> = logins
        .iter()
        .map(|(name, url, user)| {
            serde_json::json!({
                "name": name, "url": url, "ssh_host": "x", "user": user, "default": "false"
            })
        })
        .collect();
    tea_replying(
        ok(&serde_json::to_string(&listed).unwrap()),
        ok(&format!(
            "protocol=https\nhost=git.example.net\nusername=you\npassword={TOKEN}\n"
        )),
    )
}

fn tea_replying(ls: Captured, helper: Captured) -> MapRunner {
    MapRunner::new(HashMap::from([
        (MapRunner::key("tea", &["login", "ls", "-o", "json"]), ls),
        (MapRunner::key("tea", &["login", "helper", "get"]), helper),
    ]))
}

fn asked_tea_for_a_token(runner: &MapRunner) -> bool {
    runner
        .calls()
        .iter()
        .any(|(key, _)| key == "tea login helper get")
}

#[test]
fn github_asks_gh_for_that_login_on_github_com() {
    let runner = gh_replying("work-login", ok(&format!("{TOKEN}\n")));
    assert_eq!(token(&runner, &github("work-login")).unwrap(), TOKEN);
}

#[test]
fn a_login_gh_does_not_hold_says_what_to_run() {
    let runner = gh_replying(
        "work-login",
        refused("no oauth token found for github.com account work-login"),
    );
    let message = token(&runner, &github("work-login"))
        .unwrap_err()
        .to_string();
    assert!(message.starts_with("Work"), "{message}");
    assert!(message.contains("work-login"), "{message}");
    assert!(
        message.contains("gh auth login --hostname github.com"),
        "{message}"
    );
}

#[test]
fn gh_not_being_installed_is_its_own_error() {
    let runner = MapRunner::new(HashMap::new()).without("gh");
    let message = token(&runner, &github("work-login"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("gh is not installed"), "{message}");
}

#[test]
fn an_empty_answer_from_gh_is_refused() {
    let runner = gh_replying("work-login", ok("\n"));
    let message = token(&runner, &github("work-login"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("empty"), "{message}");
}

#[test]
fn tea_with_exactly_the_right_login_hands_over_its_token() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "you")]);
    assert_eq!(
        token(&runner, &gitea("you", "https://git.example.net")).unwrap(),
        TOKEN
    );
    let helper_input = runner
        .calls()
        .into_iter()
        .find(|(key, _)| key == "tea login helper get")
        .and_then(|(_, input)| input);
    assert_eq!(
        helper_input.as_deref(),
        Some("protocol=https\nhost=git.example.net\n\n")
    );
}

#[test]
fn no_tea_login_for_the_url_says_what_to_run_and_never_asks_for_a_token() {
    let runner = tea_listing(&[("other", "https://elsewhere.example.net", "you")]);
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("tea login add --url https://git.example.net"),
        "{message}"
    );
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

/// tea would hand over the first of these whatever was asked for.
#[test]
fn two_tea_logins_for_one_url_are_refused_rather_than_guessed_between() {
    let runner = tea_listing(&[
        ("alice", "https://git.example.net", "alice"),
        ("bob", "https://git.example.net", "bob"),
    ]);
    let message = token(&runner, &gitea("bob", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("alice") && message.contains("bob"),
        "{message}"
    );
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

#[test]
fn a_tea_login_for_a_different_user_is_refused_naming_both() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "someone-else")]);
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("someone-else") && message.contains("\"you\""),
        "{message}"
    );
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

#[test]
fn a_trailing_slash_or_different_case_in_the_url_still_matches() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "you")]);
    assert_eq!(
        token(&runner, &gitea("you", "https://Git.Example.net/")).unwrap(),
        TOKEN
    );
}

#[test]
fn a_port_is_kept_and_a_sub_path_dropped_in_the_helper_request() {
    let url = "https://git.example.net:3000/gitea";
    let runner = tea_listing(&[("acme", url, "you")]);
    token(&runner, &gitea("you", url)).unwrap();
    let helper_input = runner
        .calls()
        .into_iter()
        .find(|(key, _)| key == "tea login helper get")
        .and_then(|(_, input)| input);
    assert_eq!(
        helper_input.as_deref(),
        Some("protocol=https\nhost=git.example.net:3000\n\n")
    );
}

#[test]
fn a_tea_listing_that_is_not_json_is_an_error_not_a_panic() {
    let runner = tea_replying(ok("Name  URL\nacme  https://git.example.net\n"), ok(""));
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("tea login ls -o json"), "{message}");
}

#[test]
fn a_helper_answer_with_no_password_line_is_refused_as_empty() {
    let runner = tea_replying(
        ok(r#"[{"name":"acme","url":"https://git.example.net","user":"you"}]"#),
        ok("protocol=https\nhost=git.example.net\n"),
    );
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("empty"), "{message}");
}

#[test]
fn a_gitea_owner_without_a_url_is_an_error() {
    let owner = TokenOwner {
        account: "SelfHosted",
        provider: Provider::Gitea,
        login: "you",
        url: None,
    };
    let message = token(&MapRunner::new(HashMap::new()), &owner)
        .unwrap_err()
        .to_string();
    assert!(message.contains("url"), "{message}");
}
