use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use gitfriend::secrets::fingerprint;

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
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "Digilope"
    provider = "gitea"
    email = "me@digilope.example"
    gitAuth = "ssh"
    match = ["app-gitea.digilope.com/**"]
    env = ["GITEA_TOKEN", "GITEA_HOST=https://app-gitea.digilope.com/api/v1"]
"#;

fn setup(dir: &Path) {
    std::fs::write(dir.join("accounts.toml"), ACCOUNTS).unwrap();
}

fn gitfriend(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitfriend"));
    cmd.args(args)
        .env("GITFRIEND_CONFIG", dir.join("accounts.toml"))
        .env("GITFRIEND_SECRETS", dir.join("secrets.age"))
        .env("GITFRIEND_IDENTITY", dir.join("identity.key"));
    cmd
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    gitfriend(dir, args).output().unwrap()
}

fn run_with_stdin(dir: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = gitfriend(dir, args)
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

/// The value arrives on stdin, never as an argument. Anything in argv is
/// readable by every process on the machine via `ps`, which would undo the
/// point of storing it encrypted.
#[test]
fn a_secret_set_from_stdin_can_be_read_back() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    assert!(run(dir.path(), &["secret", "init"]).status.success());

    let set = run_with_stdin(
        dir.path(),
        &["secret", "set", "Personal", "GH_TOKEN"],
        "tok-personal\n",
    );
    assert!(
        set.status.success(),
        "set failed: {}",
        String::from_utf8_lossy(&set.stderr)
    );

    let list = run(dir.path(), &["secret", "list"]);
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains(&fingerprint("tok-personal")),
        "list did not show the stored secret's fingerprint; got:\n{stdout}"
    );
}

#[test]
fn secret_list_never_prints_token_material() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    run_with_stdin(
        dir.path(),
        &["secret", "set", "Personal", "GH_TOKEN"],
        "supersecrettokenvalue\n",
    );

    let list = run(dir.path(), &["secret", "list"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );

    assert!(
        !combined.contains("supersecrettokenvalue"),
        "secret list leaked a token value:\n{combined}"
    );
}

/// A declared variable with nothing stored must be visible as missing --
/// that is the drift a checker exists to surface (R12).
#[test]
fn secret_list_reports_declared_variables_with_no_stored_value() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);

    let list = run(dir.path(), &["secret", "list"]);
    let stdout = String::from_utf8_lossy(&list.stdout);

    assert!(
        stdout.contains("GITEA_TOKEN") && stdout.to_lowercase().contains("missing"),
        "an unstored declared variable should be reported as missing; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("GITEA_HOST"),
        "GITEA_HOST is a literal, not a secret, and should not be listed as one; got:\n{stdout}"
    );
}

#[test]
fn the_identity_and_secrets_files_are_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    run_with_stdin(
        dir.path(),
        &["secret", "set", "Personal", "GH_TOKEN"],
        "tok\n",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["identity.key", "secrets.age"] {
            let mode = std::fs::metadata(dir.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name} should be 0600, was {mode:o}");
        }
    }
}

/// The existing machine keeps tokens as `GH_TOKEN_<Account>` exports in
/// `~/.zshrc.local`. Import copies them rather than requiring a re-issue.
#[test]
fn import_from_the_environment_preserves_the_value() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);

    let imported = gitfriend(dir.path(), &["secret", "import", "--from-env"])
        .env("GH_TOKEN_Personal", "existing-personal-token")
        .env("GITEA_TOKEN_Digilope", "existing-gitea-token")
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let list = run(dir.path(), &["secret", "list"]);
    let stdout = String::from_utf8_lossy(&list.stdout);

    assert!(
        stdout.contains(&fingerprint("existing-personal-token")),
        "the imported GitHub token's fingerprint is missing; got:\n{stdout}"
    );
    assert!(
        stdout.contains(&fingerprint("existing-gitea-token")),
        "the imported Gitea token's fingerprint is missing; got:\n{stdout}"
    );
}
