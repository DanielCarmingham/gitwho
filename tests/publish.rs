//! `gitwho publish`: create the repository, point `origin` at it, push.

mod common;

use std::path::Path;
use std::process::Command;

use common::FakeTools;
use gitwho::config::Config;
use gitwho::publish::{
    choose_account, create_args, foreign_authors, parse_created, transport, Transport,
};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"
    gitName = "Test Person"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"
    match = ["github.com/personal/**"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    url = "https://git.example.net"
    login = "you"
    email = "you@example.net"
    sshKey = "~/.ssh/id_selfhosted"
    match = ["git.example.net/**"]
    paths = ["PLACEHOLDER_PATHS"]
"#;

fn config_claiming(dir: &Path) -> Config {
    Config::parse(&ACCOUNTS.replace("PLACEHOLDER_PATHS", &dir.display().to_string())).unwrap()
}

// --- choosing the account ---------------------------------------------------

#[test]
fn a_named_account_is_used() {
    let elsewhere = tempfile::tempdir().unwrap();
    let config = config_claiming(Path::new("/nonexistent"));
    let account = choose_account(&config, Some("Personal"), elsewhere.path()).unwrap();
    assert_eq!(account.name, "Personal");
}

#[test]
fn an_unknown_named_account_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_claiming(Path::new("/nonexistent"));
    let message = choose_account(&config, Some("Nobody"), dir.path())
        .unwrap_err()
        .to_string();
    assert!(message.contains("Nobody"), "{message}");
}

#[test]
fn a_directory_an_account_claims_by_paths_needs_no_flag() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("widget");
    std::fs::create_dir_all(&repo).unwrap();
    let config = config_claiming(root.path());
    let account = choose_account(&config, None, &repo).unwrap();
    assert_eq!(account.name, "SelfHosted");
}

/// Creating a repository as the default account because nothing else matched
/// is right only by luck -- the same reason the credential helper refuses a
/// fallback (R8).
#[test]
fn anywhere_else_it_refuses_rather_than_use_the_default() {
    let elsewhere = tempfile::tempdir().unwrap();
    let config = config_claiming(Path::new("/nonexistent"));
    let message = choose_account(&config, None, elsewhere.path())
        .unwrap_err()
        .to_string();
    assert!(message.contains("--account"), "{message}");
}

// --- authors ----------------------------------------------------------------

#[test]
fn authors_other_than_the_account_are_reported_once_each() {
    let authors = [
        "you@example.net".to_string(),
        "YOU@Example.net".to_string(),
        "me@example.com".to_string(),
        "me@example.com".to_string(),
    ];
    assert_eq!(
        foreign_authors("you@example.net", &authors),
        ["me@example.com"]
    );
}

// --- the create call --------------------------------------------------------

#[test]
fn a_personal_repository_is_created_private_under_the_login() {
    assert_eq!(
        create_args("widget", None, false),
        [
            "api",
            "-X",
            "POST",
            "user/repos",
            "-f",
            "name=widget",
            "-F",
            "private=true"
        ]
    );
}

#[test]
fn an_owner_creates_it_in_that_organisation_and_public_is_opt_in() {
    let args = create_args("widget", Some("acme-corp"), true);
    assert!(
        args.contains(&"orgs/acme-corp/repos".to_string()),
        "{args:?}"
    );
    assert!(args.contains(&"private=false".to_string()), "{args:?}");
}

#[test]
fn the_servers_answer_supplies_every_url() {
    let created = parse_created(
        r#"{"name":"widget","clone_url":"https://git.example.net/you/widget.git",
            "ssh_url":"git@ssh.git.example.net:you/widget.git",
            "html_url":"https://git.example.net/you/widget"}"#,
    )
    .unwrap();
    assert_eq!(
        created.remote(Transport::Https),
        "https://git.example.net/you/widget.git"
    );
    assert_eq!(
        created.remote(Transport::Ssh),
        "git@ssh.git.example.net:you/widget.git"
    );
    assert_eq!(created.html_url, "https://git.example.net/you/widget");
}

#[test]
fn an_answer_without_the_urls_is_an_error_not_a_guess() {
    assert!(parse_created(r#"{"name":"widget"}"#).is_err());
    assert!(parse_created("gh: Not Found (HTTP 404)").is_err());
}

#[test]
fn ssh_is_the_default_only_for_an_account_with_a_key() {
    let config = config_claiming(Path::new("/nonexistent"));
    let with_key = config.account("SelfHosted").unwrap();
    let without = config.account("Personal").unwrap();
    assert_eq!(transport(with_key, None), Transport::Ssh);
    assert_eq!(transport(without, None), Transport::Https);
    assert_eq!(
        transport(with_key, Some(Transport::Https)),
        Transport::Https
    );
}

// --- end to end -------------------------------------------------------------

/// A config dir, fake gh/tea, a local repository with one commit, and a bare
/// repository standing in for the server, so the real `git push` runs.
struct Fixture {
    config: tempfile::TempDir,
    fakes: FakeTools,
    repo: tempfile::TempDir,
    server: tempfile::TempDir,
}

impl Fixture {
    fn new(author: &str) -> Self {
        let config = tempfile::tempdir().unwrap();
        std::fs::write(
            config.path().join("accounts.toml"),
            ACCOUNTS.replace("PLACEHOLDER_PATHS", "/nonexistent"),
        )
        .unwrap();

        let fakes = FakeTools::new();
        fakes.gh_login("personal", "personal-token");

        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("README"), "hello\n").unwrap();
        git(repo.path(), &["add", "README"]);
        git(
            repo.path(),
            &[
                "-c",
                &format!("user.email={author}"),
                "-c",
                "user.name=Someone",
                "commit",
                "-q",
                "-m",
                "first",
            ],
        );

        let server = tempfile::tempdir().unwrap();
        git(server.path(), &["init", "-q", "--bare"]);

        Self {
            config,
            fakes,
            repo,
            server,
        }
    }

    /// The server answers the create call with a clone URL that is the bare
    /// repository, so the push has somewhere real to land.
    fn server_answers(&self) {
        let path = self.server.path().display().to_string();
        self.fakes.api_reply(
            "gh",
            &serde_json::json!({
                "clone_url": path,
                "ssh_url": "git@github.com:personal/unused.git",
                "html_url": "https://github.com/personal/widget",
            })
            .to_string(),
        );
    }

    fn publish(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_gitwho"))
            .arg("publish")
            .args(args)
            .current_dir(self.repo.path())
            .env("GITWHO_CONFIG", self.config.path().join("accounts.toml"))
            .env("PATH", self.fakes.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GH_TOKEN")
            .output()
            .unwrap()
    }
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

fn out(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn it_creates_the_repository_sets_origin_and_pushes() {
    let fixture = Fixture::new("me@example.com");
    fixture.server_answers();

    let output = fixture.publish(&["--account", "Personal", "--name", "widget"]);
    let combined = out(&output);

    assert!(output.status.success(), "{combined}");
    assert!(
        combined.contains("https://github.com/personal/widget"),
        "{combined}"
    );

    let args = fixture.fakes.api_args("gh").expect("gh api was not called");
    assert!(
        args.contains("user/repos") && args.contains("name=widget"),
        "{args}"
    );
    assert_eq!(fixture.fakes.api_saw_token("gh"), Some(true));

    assert_eq!(
        gitwho::git::origin_url(fixture.repo.path()).as_deref(),
        Some(fixture.server.path().display().to_string().as_str())
    );
    let pushed = Command::new("git")
        .args(["rev-parse", "--verify", "refs/heads/main"])
        .current_dir(fixture.server.path())
        .output()
        .unwrap();
    assert!(pushed.status.success(), "main did not reach the server");
}

#[test]
fn a_commit_by_another_author_stops_it_before_anything_is_created() {
    let fixture = Fixture::new("someone-else@example.org");
    fixture.server_answers();

    let output = fixture.publish(&["--account", "Personal"]);
    let combined = out(&output);

    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("someone-else@example.org"), "{combined}");
    assert!(combined.contains("--reset-author"), "{combined}");
    assert_eq!(
        fixture.fakes.api_args("gh"),
        None,
        "a repository was created"
    );
    assert_eq!(gitwho::git::origin_url(fixture.repo.path()), None);
}

#[test]
fn without_an_account_outside_any_paths_it_refuses() {
    let fixture = Fixture::new("me@example.com");
    fixture.server_answers();

    let output = fixture.publish(&[]);
    let combined = out(&output);

    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("--account"), "{combined}");
    assert_eq!(fixture.fakes.api_args("gh"), None);
}

#[test]
fn a_repository_that_already_has_an_origin_is_left_alone() {
    let fixture = Fixture::new("me@example.com");
    fixture.server_answers();
    git(
        fixture.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/personal/other.git",
        ],
    );

    let output = fixture.publish(&["--account", "Personal"]);
    let combined = out(&output);

    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("origin"), "{combined}");
    assert_eq!(fixture.fakes.api_args("gh"), None);
}

/// Once the server has the repository, gitwho never deletes it; it says where
/// it is and how to finish.
#[test]
fn a_failed_push_after_creation_says_the_repository_exists_and_how_to_retry() {
    let fixture = Fixture::new("me@example.com");
    fixture.fakes.api_reply(
        "gh",
        &serde_json::json!({
            "clone_url": "/nonexistent/server.git",
            "ssh_url": "git@github.com:personal/widget.git",
            "html_url": "https://github.com/personal/widget",
        })
        .to_string(),
    );

    let output = fixture.publish(&["--account", "Personal", "--name", "widget"]);
    let combined = out(&output);

    assert!(!output.status.success(), "{combined}");
    assert!(
        combined.contains("https://github.com/personal/widget"),
        "{combined}"
    );
    assert!(combined.contains("git push -u origin main"), "{combined}");
}

/// The same command on Gitea/Forgejo: tea creates it with the account's token,
/// and an account with an sshKey gets the ssh URL the server reported.
#[test]
fn a_gitea_account_publishes_through_tea_over_ssh() {
    let fixture = Fixture::new("you@example.net");
    fixture
        .fakes
        .tea_login("sh", "https://git.example.net", "you", "gitea-token");
    fixture.fakes.api_reply(
        "tea",
        &serde_json::json!({
            "clone_url": "https://git.example.net/you/widget.git",
            "ssh_url": fixture.server.path().display().to_string(),
            "html_url": "https://git.example.net/you/widget",
        })
        .to_string(),
    );

    let output = fixture.publish(&["--account", "SelfHosted", "--name", "widget"]);
    let combined = out(&output);

    assert!(output.status.success(), "{combined}");
    assert_eq!(fixture.fakes.api_saw_token("tea"), Some(true));
    assert_eq!(fixture.fakes.api_args("gh"), None, "gh was asked instead");
    assert_eq!(
        gitwho::git::origin_url(fixture.repo.path()).as_deref(),
        Some(fixture.server.path().display().to_string().as_str()),
        "origin should be the ssh_url for an account with an sshKey"
    );
}
