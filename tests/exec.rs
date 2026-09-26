use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::exec::{plan_cleared, plan_env};
use gitwho::sources::{Captured, MapRunner};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    login = "you"
    url = "https://git.example.net"
    email = "you@example.net"
    match = ["git.example.net/**"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn tools() -> MapRunner {
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
            ok("personal-token\n"),
        ),
        (
            MapRunner::key("tea", &["login", "ls", "-o", "json"]),
            ok(r#"[{"name":"acme","url":"https://git.example.net","user":"you"}]"#),
        ),
        (
            MapRunner::key("tea", &["login", "helper", "get"]),
            ok("protocol=https\nhost=git.example.net\nusername=you\npassword=gitea-token\n"),
        ),
    ]))
}

fn set_of(plan: &gitwho::exec::EnvPlan) -> Vec<(&str, &str)> {
    plan.set
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

#[test]
fn a_github_account_gets_its_token_under_every_name_githubs_tools_read() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("Personal").unwrap()).unwrap();
    assert_eq!(
        set_of(&plan),
        [
            ("GH_TOKEN", "personal-token"),
            ("GITHUB_PERSONAL_ACCESS_TOKEN", "personal-token"),
        ]
    );
}

#[test]
fn a_gitea_account_gets_token_and_url_under_teas_and_gitea_mcps_names() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("SelfHosted").unwrap()).unwrap();
    assert_eq!(
        set_of(&plan),
        [
            ("GITEA_ACCESS_TOKEN", "gitea-token"),
            ("GITEA_HOST", "https://git.example.net"),
            ("GITEA_INSTANCE_URL", "https://git.example.net"),
            ("GITEA_TOKEN", "gitea-token"),
        ]
    );
}

/// Entering a Gitea repo must not leave a GitHub token reachable, including
/// the fallbacks gh reads when GH_TOKEN is empty (R11).
#[test]
fn every_provider_variable_and_fallback_is_cleared_whichever_account_runs() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("SelfHosted").unwrap()).unwrap();
    for var in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GITHUB_PERSONAL_ACCESS_TOKEN",
        "GH_ENTERPRISE_TOKEN",
    ] {
        assert!(
            plan.remove.contains(var),
            "{var} not cleared: {:?}",
            plan.remove
        );
        assert!(
            !plan.set.contains_key(var),
            "{var} handed to a gitea account"
        );
    }
}

/// Running anyway would leave the CLI to authenticate as whatever it could
/// find, which is the silent-wrong-account failure (R8).
#[test]
fn a_token_the_cli_cannot_supply_refuses_rather_than_running_with_a_gap() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let message = plan_env(
        &MapRunner::new(HashMap::new()),
        config.account("Personal").unwrap(),
    )
    .unwrap_err()
    .to_string();
    assert!(message.starts_with("Personal"), "{message}");
}

#[test]
fn a_command_that_establishes_credentials_gets_everything_cleared_and_nothing_set() {
    let plan = plan_cleared();
    assert!(plan.set.is_empty());
    for var in ["GH_TOKEN", "GITEA_TOKEN", "GITHUB_TOKEN"] {
        assert!(plan.remove.contains(var), "{var} not cleared");
    }
}
