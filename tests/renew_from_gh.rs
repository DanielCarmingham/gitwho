use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use gitwho::secrets::fingerprint;

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Fallback"

    [[accounts]]
    name = "Fallback"
    provider = "github"
    email = "fallback@example.com"
    match = ["github.com/Fallback/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "work-login"
    provider = "github"
    email = "me@work.example"
    gitCredential = "GH_TOKEN"
    match = ["github.com/WorkOrg/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "Renamed"
    provider = "github"
    email = "renamed@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/RenamedOrg/**"]
    env = ["GH_TOKEN"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    email = "you@example.net"
    gitCredential = "GITEA_TOKEN"
    match = ["git.example.net/**"]
    env = ["GITEA_TOKEN"]
"#;

/// A `gh` whose state lives in files, so a test can say "this token is dead"
/// and then watch `auth login` bring it back.
///
/// It mirrors the two real behaviours this feature turns on: `auth status`
/// reports a dead token as "Failed to log in to" rather than listing it, and
/// `auth login` refuses outright when `GH_TOKEN` is set in the environment
/// (verified against the real gh).
const FAKE_GH: &str = r#"#!/bin/sh
state="$FAKE_GH_DIR/state"
token="$FAKE_GH_DIR/token"

case "$1 $2" in
"auth status")
    echo "github.com"
    if [ "$(cat "$state")" = "live" ]; then
        echo "  ✓ Logged in to github.com account work-login (keyring)"
    else
        echo "  X Failed to log in to github.com account work-login (keyring)"
    fi
    echo "  ✓ Logged in to github.com account someone-else (keyring)"
    ;;
"auth token")
    if [ "$(cat "$state")" = "live" ]; then
        cat "$token"
    else
        echo "no oauth token found for github.com account work-login" >&2
        exit 1
    fi
    ;;
"auth login")
    if [ -n "$GH_TOKEN" ]; then
        echo "The value of the GH_TOKEN environment variable is being used for authentication." >&2
        exit 1
    fi
    echo "logged-in" > "$FAKE_GH_DIR/login_ran"
    if [ -f "$FAKE_GH_DIR/login_authenticates_someone_else" ]; then
        exit 0
    fi
    echo "live" > "$state"
    echo "token-minted-by-login" > "$token"
    ;;
esac
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

struct Fixture {
    config: tempfile::TempDir,
    gh: tempfile::TempDir,
}

impl Fixture {
    /// `state` is what gh's token for `work-login` starts out as: "live" or
    /// anything else for a token gh reports as broken.
    fn new(state: &str) -> Self {
        let config = tempfile::tempdir().unwrap();
        std::fs::write(config.path().join("accounts.toml"), ACCOUNTS).unwrap();

        let gh = tempfile::tempdir().unwrap();
        std::fs::write(gh.path().join("state"), format!("{state}\n")).unwrap();
        std::fs::write(gh.path().join("token"), "token-gh-already-had\n").unwrap();
        let bin = gh.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("gh"), FAKE_GH).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }

        let fixture = Self { config, gh };
        fixture.run(&["secret", "init"]);
        fixture
    }

    /// A `PATH` holding `git` -- which gitwho needs to read the remote -- and
    /// nothing else, so the real gh on this machine cannot answer for the fake.
    fn without_gh(&self) -> String {
        let dir = self.gh.path().join("gitonly");
        std::fs::create_dir_all(&dir).unwrap();
        let git = String::from_utf8(
            Command::new("sh")
                .args(["-c", "command -v git"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let link = dir.join("git");
        if !link.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(git.trim(), &link).unwrap();
        }
        dir.display().to_string()
    }

    fn command(&self, args: &[&str], path: String) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
        cmd.args(args)
            .env("GITWHO_CONFIG", self.config.path().join("accounts.toml"))
            .env("GITWHO_SECRETS", self.config.path().join("secrets.age"))
            .env("GITWHO_IDENTITY", self.config.path().join("identity.key"))
            .env_remove("GITWHO_SECRET_BACKEND")
            .env("FAKE_GH_DIR", self.gh.path())
            .env("PATH", path);
        cmd
    }

    fn path_with_fake_gh(&self) -> String {
        format!(
            "{}:{}",
            self.gh.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args, self.path_with_fake_gh())
            .output()
            .unwrap()
    }

    fn run_in(&self, cwd: &Path, args: &[&str], input: &str) -> std::process::Output {
        self.run_in_with_path(cwd, args, input, self.path_with_fake_gh())
    }

    fn run_in_with_path(
        &self,
        cwd: &Path,
        args: &[&str],
        input: &str,
        path: String,
    ) -> std::process::Output {
        let mut child = self
            .command(args, path)
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

    fn stored(&self) -> String {
        String::from_utf8_lossy(&self.run(&["secret", "list"]).stdout).to_string()
    }

    fn login_ran(&self) -> bool {
        self.gh.path().join("login_ran").exists()
    }
}

fn work_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/WorkOrg/somerepo.git");
    repo
}

/// The whole point: gh already holds a good token, so nothing is typed.
#[test]
fn pulls_the_token_gh_already_holds_instead_of_asking_for_it() {
    let fixture = Fixture::new("live");
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew"], "y\n");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(
        fixture
            .stored()
            .contains(&fingerprint("token-gh-already-had")),
        "did not store gh's token:\n{}",
        fixture.stored()
    );
    assert!(!fixture.login_ran(), "should not have run a login flow");
}

#[test]
fn declining_the_pull_stores_nothing() {
    let fixture = Fixture::new("live");
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew"], "n\n");

    assert!(
        !fixture
            .stored()
            .contains(&fingerprint("token-gh-already-had")),
        "stored a token that was declined"
    );
    assert!(
        String::from_utf8_lossy(&renew.stdout).contains("nothing stored")
            || String::from_utf8_lossy(&renew.stderr).contains("nothing stored"),
        "stdout: {}",
        String::from_utf8_lossy(&renew.stdout)
    );
}

/// A dead token is what a renewal is *for*, so this is the path that must not
/// make the user go and find a browser command for themselves.
#[test]
fn a_dead_token_goes_straight_into_the_login_flow_and_stores_the_result() {
    let fixture = Fixture::new("dead");
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew"], "");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(fixture.login_ran(), "never ran the login flow");
    assert!(
        fixture
            .stored()
            .contains(&fingerprint("token-minted-by-login")),
        "did not store the freshly minted token:\n{}",
        fixture.stored()
    );
}

/// Real gh refuses to log in while `GH_TOKEN` is set, and gitwho's own shim is
/// what sets it. Clearing it is gitwho's job, not the user's.
#[test]
fn the_login_flow_runs_with_gh_token_cleared() {
    let fixture = Fixture::new("dead");
    let repo = work_repo();

    let renew = fixture
        .command(&["renew"], fixture.path_with_fake_gh())
        .current_dir(repo.path())
        .env("GH_TOKEN", "injected-by-the-shim")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();

    assert!(
        renew.status.success(),
        "the login flow inherited GH_TOKEN and gh refused: {}",
        String::from_utf8_lossy(&renew.stderr)
    );
    assert!(fixture
        .stored()
        .contains(&fingerprint("token-minted-by-login")));
}

/// Logging in as somebody else leaves the account still broken. Storing
/// whatever gh last handed out would be the wrong-and-quiet failure.
#[test]
fn refuses_when_the_login_authenticated_a_different_account() {
    let fixture = Fixture::new("dead");
    std::fs::write(
        fixture.gh.path().join("login_authenticates_someone_else"),
        "",
    )
    .unwrap();
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew"], "");
    let stderr = String::from_utf8_lossy(&renew.stderr);

    assert!(!renew.status.success(), "should not have claimed success");
    assert!(stderr.contains("work-login"), "{stderr}");
    assert!(
        fixture.stored().contains("MISSING"),
        "stored something anyway:\n{}",
        fixture.stored()
    );
}

/// The account is named for the org, not for the gh login. Guessing that
/// `someone-else` is the right token would store another account's credential.
#[test]
fn offers_the_available_logins_when_none_matches_the_account_name() {
    let fixture = Fixture::new("live");
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/RenamedOrg/thing.git");

    let renew = fixture.run_in(repo.path(), &["renew"], "\ntyped-by-hand\n");
    let stdout = String::from_utf8_lossy(&renew.stdout);

    assert!(stdout.contains("work-login"), "{stdout}");
    assert!(stdout.contains("someone-else"), "{stdout}");
    assert!(
        fixture.stored().contains(&fingerprint("typed-by-hand")),
        "Enter should have fallen through to the paste prompt:\n{}",
        fixture.stored()
    );
}

#[test]
fn a_chosen_login_is_pulled_from_gh() {
    let fixture = Fixture::new("live");
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://github.com/RenamedOrg/thing.git");

    let renew = fixture.run_in(repo.path(), &["renew"], "1\ny\n");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(
        fixture
            .stored()
            .contains(&fingerprint("token-gh-already-had")),
        "did not pull the chosen login's token:\n{}",
        fixture.stored()
    );
}

/// gh has nothing to say about a self-hosted Forgejo, so renew must not drag
/// the user through a GitHub login for it.
#[test]
fn a_host_gh_does_not_know_falls_back_to_the_prompt() {
    let fixture = Fixture::new("live");
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://git.example.net/someone/site.git");

    let renew = fixture.run_in(repo.path(), &["renew"], "gitea-value\n");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(!fixture.login_ran(), "ran a GitHub login for a gitea host");
    assert!(fixture.stored().contains(&fingerprint("gitea-value")));
}

#[test]
fn no_gh_on_path_still_leaves_the_prompt_working() {
    let fixture = Fixture::new("live");
    let repo = work_repo();

    let renew = fixture.run_in_with_path(
        repo.path(),
        &["renew"],
        "typed-instead\n",
        fixture.without_gh(),
    );
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(fixture.stored().contains(&fingerprint("typed-instead")));
}

#[test]
fn paste_skips_gh_entirely() {
    let fixture = Fixture::new("live");
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew", "--paste"], "pasted\n");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );

    assert!(fixture.stored().contains(&fingerprint("pasted")));
    assert!(!fixture.login_ran());
}

#[test]
fn no_login_stops_before_the_browser() {
    let fixture = Fixture::new("dead");
    let repo = work_repo();

    let renew = fixture.run_in(repo.path(), &["renew", "--no-login"], "typed-instead\n");

    assert!(!fixture.login_ran(), "ran the login flow anyway");
    assert!(
        renew.status.success(),
        "renew failed: {}",
        String::from_utf8_lossy(&renew.stderr)
    );
    assert!(fixture.stored().contains(&fingerprint("typed-instead")));
}

/// Nothing changed, so nothing is written -- and it says so rather than
/// reporting a renewal that did not happen.
#[test]
fn a_token_identical_to_the_stored_one_is_reported_not_rewritten() {
    let fixture = Fixture::new("live");
    let repo = work_repo();
    fixture.run_in(repo.path(), &["renew", "--paste"], "token-gh-already-had\n");

    let renew = fixture.run_in(repo.path(), &["renew"], "y\n");
    let stdout = String::from_utf8_lossy(&renew.stdout);

    assert!(renew.status.success());
    assert!(stdout.contains("already current"), "{stdout}");
}
