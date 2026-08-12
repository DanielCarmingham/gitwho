use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::exec::plan_env;
use gitwho::secrets::EnvBackend;

/// Two providers, so one account's variables are another's contamination.
const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/Personal/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    email = "you@example.net"
    match = ["ssh.git.example.net/**"]
    env = ["GITEA_TOKEN", "GITEA_HOST=https://ssh.git.example.net/api/v1"]
"#;

fn backend() -> EnvBackend {
    EnvBackend::from_map(HashMap::from([
        (
            "GH_TOKEN_Personal".to_string(),
            "personal-token".to_string(),
        ),
        (
            "GITEA_TOKEN_SelfHosted".to_string(),
            "gitea-token".to_string(),
        ),
    ]))
}

#[test]
fn injects_the_declared_variables_for_the_account() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let account = config.account("Personal").unwrap();

    let plan = plan_env(&config, &backend(), account).unwrap();

    assert_eq!(
        plan.set.get("GH_TOKEN").map(String::as_str),
        Some("personal-token")
    );
}

#[test]
fn variables_belonging_to_other_accounts_are_scrubbed() {
    // Entering a Gitea repo must not leave a GitHub token reachable. The set
    // of variables to clear comes from the config as a whole, not from the
    // chosen account -- the account being run knows what it needs, not what
    // it must be protected from (R11).
    let config = Config::parse(ACCOUNTS).unwrap();
    let selfhosted = config.account("SelfHosted").unwrap();

    let plan = plan_env(&config, &backend(), selfhosted).unwrap();

    assert!(
        plan.remove.contains("GH_TOKEN"),
        "GH_TOKEN was not scrubbed; remove={:?}",
        plan.remove
    );
    assert!(
        !plan.set.contains_key("GH_TOKEN"),
        "a GitHub token was handed to a Gitea account"
    );
}

#[test]
fn a_scrubbed_variable_the_account_needs_is_still_set() {
    // GITEA_TOKEN appears in the managed set, so it is scrubbed -- and then
    // set. Order matters: clear everything managed, then populate.
    let config = Config::parse(ACCOUNTS).unwrap();
    let selfhosted = config.account("SelfHosted").unwrap();

    let plan = plan_env(&config, &backend(), selfhosted).unwrap();

    assert_eq!(
        plan.set.get("GITEA_TOKEN").map(String::as_str),
        Some("gitea-token")
    );
    assert_eq!(
        plan.set.get("GITEA_HOST").map(String::as_str),
        Some("https://ssh.git.example.net/api/v1"),
        "a literal VAR=value entry should pass through unchanged"
    );
}

#[test]
fn a_missing_secret_refuses_rather_than_running_with_a_gap() {
    // Running the command anyway would leave the CLI to authenticate as
    // whatever it could find, which is the silent-wrong-account failure (R8).
    let config = Config::parse(ACCOUNTS).unwrap();
    let empty = EnvBackend::from_map(HashMap::new());
    let account = config.account("Personal").unwrap();

    let error = plan_env(&config, &empty, account).expect_err("a missing secret must refuse");

    let message = error.to_string();
    assert!(
        message.contains("Personal") && message.contains("GH_TOKEN"),
        "the error must name the account and variable; got: {message}"
    );
}
