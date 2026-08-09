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

// --- exec -------------------------------------------------------------------

const EXEC_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    gitAuth = "https"
    gitCredential = "GH_TOKEN"
    match = ["github.com/Personal/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "Digilope"
    provider = "gitea"
    email = "me@digilope.example"
    gitAuth = "ssh"
    match = ["app-gitea.digilope.com/**"]
    env = ["GITEA_TOKEN"]
"#;

fn exec_fixture(dir: &Path) {
    std::fs::write(dir.join("accounts.toml"), EXEC_ACCOUNTS).unwrap();
    let key_path = dir.join("identity.key");
    AgeFileBackend::generate_identity_file(&key_path).unwrap();
    let backend = AgeFileBackend::with_identity_file(dir.join("secrets.age"), &key_path).unwrap();
    backend.set("Personal", "GH_TOKEN", "personal-token").unwrap();
    backend.set("Digilope", "GITEA_TOKEN", "gitea-token").unwrap();
}

fn repo_for(dir: &Path, name: &str, origin: &str) -> std::path::PathBuf {
    let repo = dir.join(name);
    std::fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["remote", "add", "origin", origin],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
    }
    repo
}

#[test]
fn exec_scrubs_a_hostile_token_inherited_from_the_parent_shell() {
    // The scenario the project exists for: a GitHub token is already loaded in
    // the shell, and a Gitea tool is launched. The child must not see it (R11).
    let dir = tempfile::tempdir().unwrap();
    exec_fixture(dir.path());
    let repo = repo_for(
        dir.path(),
        "digilope-repo",
        "gitea@app-gitea.digilope.com:daniel/site.git",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_gitfriend"))
        .args(["exec", "--", "/usr/bin/env"])
        .current_dir(&repo)
        .env("GITFRIEND_CONFIG", dir.path().join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.path().join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.path().join("identity.key"))
        .env("GH_TOKEN", "hostile-github-token")
        .output()
        .unwrap();

    let env_out = String::from_utf8_lossy(&output.stdout);
    assert!(
        !env_out.contains("hostile-github-token"),
        "a GitHub token leaked into a Gitea process; env=\n{env_out}"
    );
    assert!(
        env_out.contains("GITEA_TOKEN=gitea-token"),
        "the Gitea token was not injected; env=\n{env_out}\nstderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_generated_shim_routes_a_cli_through_exec() {
    // Shims are how tools that read environment variables get covered without
    // wrapping every call site by hand. Using `env` as the stand-in CLI lets
    // the test see exactly what the wrapped process received.
    let dir = tempfile::tempdir().unwrap();
    exec_fixture(dir.path());
    let repo = repo_for(
        dir.path(),
        "digilope-repo",
        "gitea@app-gitea.digilope.com:daniel/site.git",
    );
    let shim_dir = dir.path().join("shims");

    let install = Command::new(env!("CARGO_BIN_EXE_gitfriend"))
        .args(["shim", "install", "--dir"])
        .arg(&shim_dir)
        .arg("env")
        .env("GITFRIEND_CONFIG", dir.path().join("accounts.toml"))
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "shim install failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let output = Command::new(shim_dir.join("env"))
        .current_dir(&repo)
        .env("GITFRIEND_CONFIG", dir.path().join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.path().join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.path().join("identity.key"))
        .env("GH_TOKEN", "hostile-github-token")
        .env(
            "PATH",
            format!(
                "{}:{}",
                shim_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();

    let env_out = String::from_utf8_lossy(&output.stdout);
    assert!(
        !env_out.contains("hostile-github-token"),
        "the shim did not scrub an inherited token; env=\n{env_out}"
    );
    assert!(
        env_out.contains("GITEA_TOKEN=gitea-token"),
        "the shim did not inject the account's token; env=\n{env_out}\nstderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}
