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
    paths = ["PLACEHOLDER_ROOT"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "BrandNew"
    provider = "github"
    email = "new@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/BrandNewOrg/**"]
    env = ["GH_TOKEN"]
"#;

/// Reports whether it was handed a token, without ever printing one (R10).
const FAKE_GH: &str = r#"#!/bin/sh
if [ -n "$GH_TOKEN" ]; then echo "GH_TOKEN: set"; else echo "GH_TOKEN: unset"; fi
echo "args: $*"
"#;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("accounts.toml"),
            ACCOUNTS.replace("PLACEHOLDER_ROOT", &dir.path().display().to_string()),
        )
        .unwrap();

        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for tool in ["gh", "tea"] {
            std::fs::write(bin.join(tool), FAKE_GH).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(bin.join(tool), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }

        let fixture = Self { dir };
        fixture.run(&["secret", "init"], None);
        fixture.store("Personal", "GH_TOKEN", "personal-token");
        fixture
    }

    fn store(&self, account: &str, var: &str, value: &str) {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = self
            .command(&["secret", "set", account, var], None)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{value}\n").as_bytes())
            .unwrap();
        child.wait().unwrap();
    }

    fn command(&self, args: &[&str], inherited_token: Option<&str>) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
        cmd.args(args)
            .current_dir(self.dir.path())
            .env("GITWHO_CONFIG", self.dir.path().join("accounts.toml"))
            .env("GITWHO_SECRETS", self.dir.path().join("secrets.age"))
            .env("GITWHO_IDENTITY", self.dir.path().join("identity.key"))
            .env_remove("GITWHO_SECRET_BACKEND")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        match inherited_token {
            Some(token) => cmd.env("GH_TOKEN", token),
            None => cmd.env_remove("GH_TOKEN"),
        };
        cmd
    }

    fn run(&self, args: &[&str], inherited_token: Option<&str>) -> std::process::Output {
        self.command(args, inherited_token).output().unwrap()
    }
}

fn out(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The directory is matched by a `paths` prefix and is not a repo, which is
/// exactly where this was found: a token arrives anyway and gh refuses to log in.
#[test]
fn gh_auth_login_is_run_without_an_injected_token() {
    let fixture = Fixture::new();

    let output = fixture.run(&["exec", "--", "gh", "auth", "login"], None);
    let combined = out(&output);

    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("GH_TOKEN: unset"), "{combined}");
}

/// R11 still holds: stepping aside means injecting nothing, not letting
/// whatever was in the shell through.
#[test]
fn an_inherited_token_is_still_cleared_for_those_commands() {
    let fixture = Fixture::new();

    let output = fixture.run(
        &["exec", "--", "gh", "auth", "login"],
        Some("hostile-inherited-token"),
    );
    let combined = out(&output);

    assert!(combined.contains("GH_TOKEN: unset"), "{combined}");
    assert!(
        !combined.contains("hostile-inherited-token"),
        "the inherited token survived: {combined}"
    );
}

#[test]
fn it_says_that_it_stepped_aside_rather_than_doing_it_silently() {
    let fixture = Fixture::new();

    let output = fixture.run(&["exec", "--", "gh", "auth", "login"], None);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("gh"), "{stderr}");
    assert!(stderr.contains("no token was injected"), "{stderr}");
}

/// The ordinary case is the whole reason the shim exists and must not change.
#[test]
fn an_ordinary_command_still_gets_the_accounts_credentials() {
    let fixture = Fixture::new();

    let output = fixture.run(&["exec", "--", "gh", "pr", "list"], None);
    let combined = out(&output);

    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("GH_TOKEN: set"), "{combined}");
}

/// The bootstrap case: an account declared but never used has nothing stored,
/// so the ordinary path fails with MissingSecret before the tool ever runs.
/// Logging in is how you would fix that, so it must not be what is blocked.
#[test]
fn an_account_with_nothing_stored_can_still_log_in() {
    let fixture = Fixture::new();

    let output = fixture.run(
        &["exec", "--account", "BrandNew", "--", "gh", "auth", "login"],
        None,
    );
    let combined = out(&output);

    assert!(
        output.status.success(),
        "a brand-new account could not log in: {combined}"
    );
    assert!(combined.contains("GH_TOKEN: unset"), "{combined}");
}

#[test]
fn an_account_with_nothing_stored_still_fails_for_an_ordinary_command() {
    let fixture = Fixture::new();

    let output = fixture.run(
        &["exec", "--account", "BrandNew", "--", "gh", "pr", "list"],
        None,
    );
    let combined = out(&output);

    // Running the CLI as nobody in particular would be the quiet-and-wrong
    // outcome; the loud one is correct here.
    assert!(!output.status.success(), "{combined}");
    assert!(combined.contains("GH_TOKEN"), "{combined}");
}

#[test]
fn tea_login_add_is_covered_too() {
    let fixture = Fixture::new();

    let output = fixture.run(&["exec", "--", "tea", "login", "add"], None);
    let combined = out(&output);

    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("GH_TOKEN: unset"), "{combined}");
}
