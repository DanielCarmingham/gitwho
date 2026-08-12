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
    name = "SelfHosted"
    provider = "gitea"
    email = "you@example.net"
    match = ["ssh.git.example.net/**"]
    env = ["GITEA_TOKEN", "GITEA_HOST=https://ssh.git.example.net/api/v1"]
"#;

fn setup(dir: &Path) {
    std::fs::write(dir.join("accounts.toml"), ACCOUNTS).unwrap();
}

fn gitwho(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
    cmd.args(args)
        .env("GITWHO_CONFIG", dir.join("accounts.toml"))
        .env("GITWHO_SECRETS", dir.join("secrets.age"))
        .env("GITWHO_IDENTITY", dir.join("identity.key"))
        // Cleared, not merely unset here: it is the highest-precedence backend
        // selector, so a developer who has it exported would otherwise point
        // this whole suite at their real login keychain.
        .env_remove("GITWHO_SECRET_BACKEND");
    cmd
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    gitwho(dir, args).output().unwrap()
}

fn run_with_stdin(dir: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = gitwho(dir, args)
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

/// Whichever platform this runs on, something must be asserted. The `cfg(unix)`
/// block used to be the whole body, so on Windows -- the one platform where
/// gitwho cannot apply the mode -- the test passed while checking nothing.
/// Where the promise cannot be kept, the promise is that `doctor` says so.
#[test]
fn the_identity_and_secrets_files_are_owner_only() {
    use gitwho::secrets::{AgeFileBackend, Protection};

    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);
    run_with_stdin(
        dir.path(),
        &["secret", "set", "Personal", "GH_TOKEN"],
        "tok\n",
    );

    if AgeFileBackend::protection() == Protection::OwnerOnly {
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
    } else {
        // Unverified: this arm has never run, because there is no Windows
        // machine here. It is written so the gap is loud there rather than
        // silent.
        let doctor = run(dir.path(), &["doctor"]);
        let stdout = String::from_utf8_lossy(&doctor.stdout);
        assert!(
            stdout
                .lines()
                .any(|l| l.starts_with("warn") && l.contains("secrets.age")),
            "where owner-only cannot be applied, doctor must warn; got:\n{stdout}"
        );
    }
}

/// A machine records its store in `accounts.toml`; a single command overrides
/// it from the environment. Asserted through the age file, so no test in this
/// suite ever reaches the real login keychain.
#[test]
fn the_environment_overrides_the_configured_backend() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("accounts.toml"),
        ACCOUNTS.replace(
            "[defaults]\n    account = \"Personal\"",
            "[defaults]\n    account = \"Personal\"\n    secretBackend = \"keychain\"",
        ),
    )
    .unwrap();

    let with_override = |args: &[&str]| {
        let mut cmd = gitwho(dir.path(), args);
        cmd.env("GITWHO_SECRET_BACKEND", "age");
        cmd
    };

    assert!(with_override(&["secret", "init"])
        .output()
        .unwrap()
        .status
        .success());

    let mut child = with_override(&["secret", "set", "Personal", "GH_TOKEN"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"tok-overridden\n")
        .unwrap();
    let set = child.wait_with_output().unwrap();
    assert!(
        set.status.success(),
        "set failed: {}",
        String::from_utf8_lossy(&set.stderr)
    );

    assert!(
        dir.path().join("secrets.age").exists(),
        "the override was ignored; nothing was written to the age file"
    );

    let list = with_override(&["secret", "list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains(&fingerprint("tok-overridden")),
        "the value did not come back from the age file; got:\n{stdout}"
    );
}

/// A typo in the configured backend must not resolve to the default: a working
/// store that is not the one asked for is exactly the quiet wrongness R8 rules
/// out.
#[test]
fn an_unknown_configured_backend_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("accounts.toml"),
        ACCOUNTS.replace(
            "[defaults]\n    account = \"Personal\"",
            "[defaults]\n    account = \"Personal\"\n    secretBackend = \"kechain\"",
        ),
    )
    .unwrap();
    run(dir.path(), &["secret", "init"]);

    let list = run(dir.path(), &["secret", "list"]);

    assert!(
        !list.status.success(),
        "an unknown backend should be refused"
    );
    let err = String::from_utf8_lossy(&list.stderr);
    assert!(
        err.contains("kechain") && err.contains("age"),
        "the error should quote the typo and name the real backends; got: {err}"
    );
}

/// The setup this replaces keeps tokens as `GH_TOKEN_<Account>` exports in a
/// shell rc file. Import copies them rather than requiring a re-issue.
#[test]
fn import_from_the_environment_preserves_the_value() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);

    let imported = gitwho(dir.path(), &["secret", "import", "--from-env"])
        .env("GH_TOKEN_Personal", "existing-personal-token")
        .env("GITEA_TOKEN_SelfHosted", "existing-gitea-token")
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

#[test]
fn setting_a_secret_for_an_unknown_account_is_refused() {
    // A typo would otherwise store a secret nothing ever reads, and the
    // symptom appears later as a missing credential somewhere else entirely.
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);

    let out = run_with_stdin(
        dir.path(),
        &["secret", "set", "Personel", "GH_TOKEN"],
        "tok\n",
    );

    assert!(
        !out.status.success(),
        "an unknown account should be refused"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Personel") && err.contains("Personal"),
        "the error should name the typo and suggest the real accounts; got: {err}"
    );
}

/// A fresh install has to pass gitwho's own `doctor`, and `doctor` requires
/// the store directory to be `0700` -- it is the only thing keeping the
/// identity key and every stored token out of another local account's reach.
/// `create_dir_all` applies the umask, so the ubiquitous `022` left it `0755`
/// and a correct install failed the check it ships with.
///
/// Run through a shell with a stated umask, because the developer's own would
/// otherwise decide whether this test can fail at all.
#[cfg(unix)]
#[test]
fn secret_init_creates_the_store_directory_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".config").join("gitwho");

    let out = Command::new("/bin/sh")
        .args([
            "-c",
            "umask 022; exec \"$0\" secret init",
            env!("CARGO_BIN_EXE_gitwho"),
        ])
        .env("GITWHO_CONFIG", store.join("accounts.toml"))
        .env("GITWHO_SECRETS", store.join("secrets.age"))
        .env("GITWHO_IDENTITY", store.join("identity.key"))
        .env_remove("GITWHO_SECRET_BACKEND")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "secret init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mode = std::fs::metadata(&store).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o700,
        "a fresh install left the store directory {mode:04o}, which doctor calls a problem"
    );
}

/// The harness itself, not the binary. `GITWHO_SECRET_BACKEND` is the
/// highest-precedence backend selector and `docs/CUTOVER.md` tells the operator
/// to export it -- so a developer following the docs would turn this suite into
/// one that writes test values into the real login keychain and blocks on the
/// GUI prompt the age file exists to avoid.
#[test]
fn the_harness_clears_the_backend_override_so_no_test_can_reach_a_real_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let cmd = gitwho(dir.path(), &["secret", "list"]);

    let cleared = cmd.get_envs().any(|(var, value)| {
        var == std::ffi::OsStr::new("GITWHO_SECRET_BACKEND") && value.is_none()
    });

    assert!(
        cleared,
        "the helper lets the developer's shell choose this suite's secret store"
    );
}

#[test]
fn setting_an_undeclared_variable_warns_but_still_stores() {
    // Not an error: you may be adding the variable to accounts.toml next. But
    // silence would let a secret sit unread forever.
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    run(dir.path(), &["secret", "init"]);

    let out = run_with_stdin(
        dir.path(),
        &["secret", "set", "Personal", "NPM_TOKEN"],
        "tok\n",
    );

    assert!(out.status.success(), "it should still store");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.to_lowercase().contains("not declared")
            || err.to_lowercase().contains("does not declare"),
        "expected a warning about the undeclared variable; got: {err}"
    );
}
