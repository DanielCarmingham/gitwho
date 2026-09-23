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
    env = ["GITEA_TOKEN", "GITEA_INSTANCE_URL=https://git.example.net"]
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

fn add_remote(dir: &Path, name: &str, url: &str) {
    git(dir, &["remote", "add", name, url]);
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
    assert!(stdout.contains("GITEA_INSTANCE_URL"), "{stdout}");
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

/// `resolve_repo` consults only `origin`, which is right for credentials and
/// wrong for identity: `includeIf "hasconfig:remote.*.url:"` fires when *any*
/// remote matches, so the last-declared claimant wins. Reporting origin's
/// account as though it decided is the wrong-and-quiet failure (R8).
#[test]
fn two_remotes_owned_by_two_accounts_name_the_one_that_actually_decides() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");
    add_remote(
        repo.path(),
        "upstream",
        "https://github.com/WorkOrg/other.git",
    );

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    // Both remotes are reported with the account each authenticates as.
    assert!(stdout.contains("origin"), "{stdout}");
    assert!(stdout.contains("upstream"), "{stdout}");
    assert!(stdout.contains("Personal"), "{stdout}");
    assert!(stdout.contains("Work"), "{stdout}");
    // And the identity winner is named as the last declared, not as origin.
    assert!(
        stdout.contains("declared last") || stdout.contains("decides"),
        "did not say which account decides identity:\n{stdout}"
    );
}

/// The whole bug in one assertion: the email printed must be the one git will
/// use, not the one belonging to whichever account owns `origin`.
#[test]
fn the_email_reported_is_the_one_git_will_actually_use() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");
    git(
        repo.path(),
        &["config", "user.email", "pinned@example.test"],
    );

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.contains("pinned@example.test"), "{stdout}");
    assert!(
        !stdout.contains("me@example.com"),
        "reported the config's email over git's own:\n{stdout}"
    );
}

/// Local config beats every included global rule, so once a repo pins its own
/// identity the include order decides nothing and saying otherwise misleads.
#[test]
fn a_repo_that_pins_its_own_identity_is_reported_as_settled() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");
    add_remote(
        repo.path(),
        "upstream",
        "https://github.com/WorkOrg/other.git",
    );
    git(
        repo.path(),
        &["config", "user.email", "pinned@example.test"],
    );

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(
        stdout.contains("pinned") || stdout.contains("set by this repository"),
        "did not report that the repo settles its own identity:\n{stdout}"
    );
}

/// One account claiming everything is the common case and must stay terse.
#[test]
fn several_remotes_owned_by_one_account_do_not_raise_a_conflict() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");
    add_remote(repo.path(), "fork", "https://github.com/Personal/fork.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(
        !stdout.contains("decides") && !stdout.contains("declared last"),
        "reported a conflict where there is none:\n{stdout}"
    );
}

/// Both halves of what ends up on a commit, from git rather than from the
/// config -- showing one and not the other was arbitrary.
#[test]
fn the_name_is_reported_alongside_the_email_and_both_come_from_git() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");
    git(repo.path(), &["config", "user.name", "Pinned Person"]);
    git(
        repo.path(),
        &["config", "user.email", "pinned@example.test"],
    );

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(stdout.contains("Pinned Person"), "{stdout}");
    assert!(stdout.contains("pinned@example.test"), "{stdout}");
}
