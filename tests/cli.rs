use std::path::Path;
use std::process::Command;

use gitfriend::secrets::{AgeFileBackend, Backend};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    gitAuth = "https"
    gitCredential = "GH_TOKEN"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    email = "me@work.example"
    gitAuth = "https"
    gitCredential = "GH_TOKEN"
    match = ["github.com/WorkOrg/**"]
"#;

/// A config directory with two accounts and a token stored for each.
fn fixture(dir: &Path) {
    std::fs::write(dir.join("accounts.toml"), ACCOUNTS).unwrap();

    let key_path = dir.join("identity.key");
    AgeFileBackend::generate_identity_file(&key_path).unwrap();
    let backend = AgeFileBackend::with_identity_file(dir.join("secrets.age"), &key_path).unwrap();
    backend.set("Personal", "GH_TOKEN", "personal-token").unwrap();
    backend.set("Work", "GH_TOKEN", "work-token").unwrap();
}

/// Ask real git to fill a credential, with gitfriend configured as the helper.
///
/// This is the end-to-end proof: git decides when and how to call the helper,
/// so a passing result means the protocol is genuinely understood, not just
/// our idea of it.
fn git_credential_fill(dir: &Path, url: &str) -> std::process::Output {
    git_credential_fill_in(dir, url, dir)
}

fn git_credential_fill_in(dir: &Path, url: &str, cwd: &Path) -> std::process::Output {
    let helper = format!("{} credential", env!("CARGO_BIN_EXE_gitfriend"));

    let mut child = Command::new("git")
        .args([
            "-c",
            "credential.helper=",
            "-c",
            &format!("credential.helper={helper}"),
            "-c",
            "credential.useHttpPath=true",
            "credential",
            "fill",
        ])
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        // Never let git fall back to an interactive prompt: a hang would look
        // like a pass under a timeout, and a prompt would mean the helper
        // declined without us noticing.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GITFRIEND_CONFIG", dir.join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.join("identity.key"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("git should run");

    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("url={url}\n\n").as_bytes())
        .unwrap();

    child.wait_with_output().unwrap()
}

#[test]
fn git_asks_gitfriend_and_gets_the_token_for_that_org() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());

    let output = git_credential_fill(dir.path(), "https://github.com/WorkOrg/thing.git");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("password=work-token"),
        "expected Work's token; stdout={stdout} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_same_git_command_picks_a_different_account_for_a_different_org() {
    // Same host, same working directory, same invocation -- only the URL
    // differs. This is R3, and it is what filesystem-path rules cannot do.
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());

    let output = git_credential_fill(dir.path(), "https://github.com/Personal/thing.git");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("password=personal-token"),
        "expected Personal's token; stdout={stdout} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_url_wins_over_the_working_directory() {
    // Standing inside a repo that belongs to Personal, ask for a WorkOrg URL.
    // Anything keyed on filesystem location answers "Personal" here. This is
    // the whole thesis of the project, end to end (R1, R2).
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());

    let personal_repo = dir.path().join("a-personal-repo");
    std::fs::create_dir(&personal_repo).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "remote",
            "add",
            "origin",
            "https://github.com/Personal/mine.git",
        ],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(&personal_repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
    }

    let output = git_credential_fill_in(
        dir.path(),
        "https://github.com/WorkOrg/thing.git",
        &personal_repo,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("password=work-token"),
        "the working directory overrode the URL; stdout={stdout} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_unclaimed_host_gets_no_credential_from_the_real_binary() {
    // End-to-end R11: git must come away with nothing, not with whichever
    // token happened to be lying around.
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());

    let output = git_credential_fill(dir.path(), "https://evil.example.com/someone/repo.git");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("work-token") && !stdout.contains("personal-token"),
        "a token leaked to an unclaimed host: {stdout}"
    );
    assert!(!output.status.success(), "git should not have succeeded");
}
