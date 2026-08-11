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
    gitCredential = "GH_TOKEN"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    email = "me@work.example"
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
        .env_remove("GITFRIEND_SECRET_BACKEND")
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

/// The credential helper is run *by git*, inside whatever environment the
/// surrounding shell had. Building a map of all of it to find `HOME` decoded
/// every other variable too, and `std::env::vars` panics on one that is not
/// Unicode -- a legacy locale, a path off a non-UTF-8 filesystem, anything a
/// tool exported. git reads a failed helper as "no credential" and falls
/// through to a prompt, so the whole design would be defeated by a variable
/// gitfriend has no interest in.
#[cfg(unix)]
#[test]
fn a_non_unicode_variable_elsewhere_in_the_environment_is_ignored() {
    use std::io::Write;
    use std::os::unix::ffi::OsStringExt;

    // A real HOME rather than the GITFRIEND_* overrides the other tests use:
    // those short-circuit before the environment is ever consulted, which is
    // exactly why this went unnoticed.
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".config").join("gitfriend");
    std::fs::create_dir_all(&store).unwrap();
    fixture(&store);

    let mut command = Command::new(env!("CARGO_BIN_EXE_gitfriend"));
    command
        .args(["credential", "get"])
        .env("HOME", home.path())
        // Not valid UTF-8, and nothing to do with gitfriend.
        .env(
            "GITFRIEND_TEST_BYSTANDER",
            std::ffi::OsString::from_vec(vec![0xff, 0xfe]),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for var in [
        "GITFRIEND_CONFIG",
        "GITFRIEND_SECRETS",
        "GITFRIEND_IDENTITY",
        "GITFRIEND_SECRET_BACKEND",
    ] {
        command.env_remove(var);
    }
    let mut child = command.spawn().expect("gitfriend should run");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"protocol=https\nhost=github.com\npath=Personal/thing.git\n\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the helper failed on a variable it never reads; stderr={stderr}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("username="),
        "no credential came back; stderr={stderr}"
    );
}

// --- exec -------------------------------------------------------------------

const EXEC_ACCOUNTS: &str = r#"
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
    name = "Digilope"
    provider = "gitea"
    email = "me@digilope.example"
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
        .env_remove("GITFRIEND_SECRET_BACKEND")
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
        .env_remove("GITFRIEND_SECRET_BACKEND")
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
        .env_remove("GITFRIEND_SECRET_BACKEND")
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

#[test]
fn exec_works_in_a_third_party_clone_but_says_so() {
    // The real case: ~12 clones of other people's repos sit under this
    // machine's personal root. `gh` should still work in them, using the
    // default account -- and should mention that nothing claimed the remote,
    // since that is also what a forgotten pattern looks like.
    let dir = tempfile::tempdir().unwrap();
    exec_fixture(dir.path());
    let repo = repo_for(
        dir.path(),
        "someone-elses-repo",
        "https://github.com/microsoft/vscode.git",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_gitfriend"))
        .args(["exec", "--", "/usr/bin/env"])
        .current_dir(&repo)
        .env("GITFRIEND_CONFIG", dir.path().join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.path().join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.path().join("identity.key"))
        .env_remove("GITFRIEND_SECRET_BACKEND")
        .output()
        .unwrap();

    assert!(output.status.success(), "exec should not fail in an unclaimed clone");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("GH_TOKEN=personal-token"),
        "the default account's token should be used"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no account claims"),
        "the fallback should be stated, not silent; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// With no home directory to resolve against, gitfriend used to fall back to a
/// *relative* path and read `.config/gitfriend/accounts.toml` from wherever it
/// was standing. A config planted in a working tree decides which host is
/// handed which token, so it must refuse rather than obey (R8).
#[test]
fn a_missing_home_does_not_read_config_from_the_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let ambush = dir.path().join(".config").join("gitfriend");
    std::fs::create_dir_all(&ambush).unwrap();
    std::fs::write(
        ambush.join("accounts.toml"),
        r#"
        [defaults]
        account = "Ambush"
        gitName = "Someone Else"

        [[accounts]]
        name = "Ambush"
        provider = "github"
        email = "someone@else.example"
        match = ["github.com/**"]
    "#,
    )
    .unwrap();

    // `sync --dir` is the cheapest subcommand that reads accounts.toml and
    // nothing else, so a config that was read is visible in the output as a
    // generated `<Account>.gitconfig`.
    let mut command = Command::new(env!("CARGO_BIN_EXE_gitfriend"));
    command
        .args(["sync", "--dir"])
        .arg(dir.path().join("out"))
        .current_dir(dir.path());
    for var in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "GITFRIEND_CONFIG",
        "GITFRIEND_SECRETS",
        "GITFRIEND_IDENTITY",
        "GITFRIEND_GIT_DIR",
        "GITFRIEND_SECRET_BACKEND",
    ] {
        command.env_remove(var);
    }
    let output = command.output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "it should refuse rather than resolve a home directory it does not have; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("Ambush") && !stderr.contains("Ambush"),
        "the planted config in the working directory was read; stdout=\n{stdout}\nstderr={stderr}"
    );
    assert!(
        stderr.contains("HOME"),
        "the error should name the variable that is missing; stderr={stderr}"
    );
}

// Modes only exist where `doctor::check_permissions` does.
#[cfg(unix)]
#[test]
fn doctor_reports_a_world_readable_config_file() {
    // The only cover for the Store wiring in main.rs: that doctor stats the
    // files the rest of the binary actually reads.
    //
    // `fixture` writes accounts.toml with plain `fs::write`, so the local
    // umask decides its mode -- 0644 here. That is the loosened state under
    // test; any future CLI test that runs doctor will see the same finding.
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    std::fs::set_permissions(
        dir.path().join("accounts.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_gitfriend"))
        .arg("doctor")
        // Without an emptied git config the assertion would depend on the
        // developer's own machine. Doctor then FAILs for unrelated and correct
        // reasons, so this asserts on the line rather than the exit code.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GITFRIEND_CONFIG", dir.path().join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.path().join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.path().join("identity.key"))
        .env_remove("GITFRIEND_SECRET_BACKEND")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("[permissions]") && l.contains("accounts.toml")),
        "doctor should name the loosened config file; stdout=\n{stdout}\nstderr={stderr}"
    );

    // R10: the new output path must not have become a way to print a value.
    for value in ["personal-token", "work-token"] {
        assert!(
            !stdout.contains(value) && !stderr.contains(value),
            "doctor printed a stored value"
        );
    }
}
