use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use gitwho::secrets::fingerprint;

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
    env = [{ var = "GH_TOKEN", from = "gh", user = "work-login", host = "github.com" }]

    [[accounts]]
    name = "Mixed"
    provider = "github"
    email = "mixed@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/Mixed/**"]
    env = [
        { var = "GH_TOKEN", from = "gh", user = "mixed-login" },
        "EXTRA_TOKEN",
    ]
"#;

const GH_ANSWERS: &str = "gho_whatever_gh_currently_holds";

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

/// A `gh` that answers `auth token` and nothing else, so a test can tell
/// "gitwho read the tool" apart from "gitwho read its own store".
fn fake_gh(dir: &Path) -> std::path::PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, format!("#!/bin/sh\necho {GH_ANSWERS}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn setup(dir: &Path) {
    std::fs::write(dir.join("accounts.toml"), ACCOUNTS).unwrap();
}

fn gitwho(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
    cmd.args(args)
        .env("GITWHO_CONFIG", dir.join("accounts.toml"))
        .env("GITWHO_SECRETS", dir.join("secrets.age"))
        .env("GITWHO_IDENTITY", dir.join("identity.key"))
        .env_remove("GITWHO_SECRET_BACKEND")
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_gh(dir).display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    cmd
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    gitwho(dir, args).output().unwrap()
}

fn run_in(dir: &Path, cwd: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = gitwho(dir, args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn renews_a_stored_variable_in_place() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Personal/thing.git");

    let renew = run_in(dir.path(), repo.path(), &["renew"], "the-new-token\n");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    let list = run(dir.path(), &["secret", "list"]);
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains(&fingerprint("the-new-token")),
        "the new value was not stored; got:\n{stdout}"
    );
}

/// The case that makes `renew` more than an alias: a referenced variable has
/// nothing to store, and storing a copy would defeat the pointer.
#[test]
fn a_referenced_variable_is_routed_to_its_tool_and_nothing_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    let renew = run_in(dir.path(), repo.path(), &["renew"], "");
    let stdout = String::from_utf8_lossy(&renew.stdout);

    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );
    assert!(stdout.contains("gh auth login"), "{stdout}");
    assert!(stdout.contains("work-login"), "{stdout}");
    assert!(stdout.contains("github.com"), "{stdout}");

    let list = run(dir.path(), &["secret", "list"]);
    let stored = String::from_utf8_lossy(&list.stdout);
    assert!(
        !stored.lines().any(|line| line.starts_with("Work")),
        "a referenced variable should not have gained a stored copy:\n{stored}"
    );
}

/// It reads the tool to show you what is current, and reports it the way
/// everything else here does: a fingerprint, never the value (R10).
#[test]
fn a_referenced_variable_is_reported_by_fingerprint_not_by_value() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    let renew = run_in(dir.path(), repo.path(), &["renew"], "");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&renew.stdout),
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(combined.contains(&fingerprint(GH_ANSWERS)), "{combined}");
    assert!(
        !combined.contains(GH_ANSWERS),
        "printed the token: {combined}"
    );
}

#[test]
fn refuses_in_a_repository_no_account_claims() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Stranger/theirs.git");

    let renew = run_in(dir.path(), repo.path(), &["renew"], "nope\n");
    let stderr = String::from_utf8_lossy(&renew.stderr);

    assert!(!renew.status.success());
    assert!(stderr.contains("no account claims this remote"), "{stderr}");

    let list = run(dir.path(), &["secret", "list"]);
    let stored = String::from_utf8_lossy(&list.stdout);
    assert!(!stored.contains(&fingerprint("nope")), "{stored}");
}

#[test]
fn an_account_holding_both_kinds_gets_both_treatments() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Mixed/thing.git");

    let renew = run_in(dir.path(), repo.path(), &["renew"], "extra-value\n");
    let stdout = String::from_utf8_lossy(&renew.stdout);

    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );
    assert!(stdout.contains("gh auth login"), "{stdout}");

    let list = run(dir.path(), &["secret", "list"]);
    let stored = String::from_utf8_lossy(&list.stdout);
    assert!(
        stored.contains(&fingerprint("extra-value")),
        "the stored half of the account was not renewed:\n{stored}"
    );
}

#[test]
fn a_named_variable_narrows_the_renewal_to_that_one() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Mixed/thing.git");

    let renew = run_in(
        dir.path(),
        repo.path(),
        &["renew", "EXTRA_TOKEN"],
        "only-this\n",
    );
    let stdout = String::from_utf8_lossy(&renew.stdout);

    assert!(renew.status.success());
    assert!(
        !stdout.contains("gh auth login"),
        "the referenced variable should have been left out:\n{stdout}"
    );

    let list = run(dir.path(), &["secret", "list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains(&fingerprint("only-this")));
}

#[test]
fn an_unknown_variable_names_the_ones_the_account_has() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/Mixed/thing.git");

    let renew = run_in(dir.path(), repo.path(), &["renew", "NOPE_TOKEN"], "");
    let stderr = String::from_utf8_lossy(&renew.stderr);

    assert!(!renew.status.success());
    assert!(stderr.contains("GH_TOKEN"), "{stderr}");
    assert!(stderr.contains("EXTRA_TOKEN"), "{stderr}");
}

/// A config whose values all live in other tools needs no store, and INSTALL.md
/// promises as much. Demanding one here would make that arrangement look broken.
#[test]
fn a_referenced_variable_needs_no_secret_store_at_all() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");

    // Deliberately no `secret init`: there is no identity.key and no
    // secrets.age anywhere.
    let renew = run_in(dir.path(), repo.path(), &["renew"], "");

    assert!(
        renew.status.success(),
        "renew demanded a store it never needed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );
    assert!(!dir.path().join("identity.key").exists());
}
