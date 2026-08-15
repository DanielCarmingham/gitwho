use std::path::Path;
use std::process::Command;

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
    name = "Work"
    provider = "github"
    email = "me@work.example"
    gitCredential = "GH_TOKEN"
    match = ["github.com/WorkOrg/**"]
    env = [{ var = "GH_TOKEN", from = "gh", user = "work-login" }]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    email = "you@example.net"
    gitCredential = "GITEA_TOKEN"
    match = ["git.example.net/**"]
    env = ["GITEA_TOKEN", "GITEA_HOST=https://git.example.net/api/v1"]
"#;

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

fn repo_with_origin(dir: &Path, origin: &str) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["remote", "add", "origin", origin]);
}

/// The config lives beside the repo rather than inside it, so nothing here
/// depends on the developer's real `~/.config/gitwho`.
fn setup(config_dir: &Path) {
    std::fs::write(config_dir.join("accounts.toml"), ACCOUNTS).unwrap();
}

fn whoami(config_dir: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
    cmd.arg("whoami")
        .args(args)
        .current_dir(cwd)
        .env("GITWHO_CONFIG", config_dir.join("accounts.toml"))
        .env("GITWHO_SECRETS", config_dir.join("secrets.age"))
        .env("GITWHO_IDENTITY", config_dir.join("identity.key"))
        .env_remove("GITWHO_SECRET_BACKEND");
    cmd.output().unwrap()
}

#[test]
fn reports_the_account_and_how_it_was_arrived_at() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    assert!(stdout.contains("Work"), "{stdout}");
    assert!(stdout.contains("matched the origin remote"), "{stdout}");
}

#[test]
fn names_the_credential_variable_and_where_each_value_lives() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://git.example.net/someone/site.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.contains("GITEA_TOKEN"), "{stdout}");
    assert!(stdout.contains("stored"), "{stdout}");
    // A literal is declared in accounts.toml and needs nothing stored; saying
    // so is the difference between "you have not set this yet" and "you never
    // will".
    assert!(stdout.contains("GITEA_HOST"), "{stdout}");
    assert!(stdout.contains("literal"), "{stdout}");
}

#[test]
fn says_when_a_value_is_read_from_another_tool_rather_than_stored() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.contains("gh"), "{stdout}");
    assert!(stdout.contains("work-login"), "{stdout}");
}

#[test]
fn quiet_prints_the_bare_account_name_for_substitution() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    let out = whoami(config.path(), repo.path(), &["--quiet"]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(out.status.success());
    assert_eq!(stdout, "Work\n");
}

#[test]
fn quiet_refuses_to_answer_when_nothing_identified_the_account() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Stranger/theirs.git");

    let out = whoami(config.path(), repo.path(), &["--quiet"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();

    // The default account would be a working answer and the wrong one -- the
    // R8 failure. A command whose output is about to be pasted into another
    // command must not supply it.
    assert!(!out.status.success());
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("no account claims this remote"), "{stderr}");
}

#[test]
fn the_report_still_answers_for_a_repo_nobody_claims() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Stranger/theirs.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    // Reporting is not releasing: a third-party clone is a normal thing to
    // stand in, and the honest answer is the fallback plus why it was reached.
    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    assert!(stdout.contains("Personal"), "{stdout}");
    assert!(stdout.contains("no account claims this remote"), "{stdout}");
}
