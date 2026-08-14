//! Reading a value from a tool that already holds it.
//!
//! The measurements these encode were taken against gh 2.97.0 on macOS,
//! 2026-08-13. Where a test asserts on gh's own wording, that wording is
//! quoted from a real run rather than invented.

use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::secrets::EnvBackend;
use gitwho::sources::{
    fetch, value_for, Captured, MapRunner, ProcessRunner, SourceError, ValueError,
};

/// One account reading its token from gh, one holding its own.
const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"
    gitName = "Someone"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    gitCredential = "GH_TOKEN"
    match = ["github.com/personal/**"]
    env = [{ var = "GH_TOKEN", from = "gh", user = "octocat" }]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    email = "me@example.net"
    gitCredential = "GITEA_TOKEN"
    match = ["git.example.net/**"]
    env = ["GITEA_TOKEN", "GITEA_HOST=https://git.example.net"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn refused(stderr: &str) -> Captured {
    Captured {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

/// gh answering for the account the config names.
fn gh_holding(token: &str) -> MapRunner {
    MapRunner::new(HashMap::from([(
        MapRunner::key("gh", &["auth", "token", "--user", "octocat"]),
        ok(token),
    )]))
}

fn store_holding_everything() -> EnvBackend {
    EnvBackend::from_map(HashMap::from([
        (
            "GH_TOKEN_Personal".to_string(),
            "from-the-store".to_string(),
        ),
        (
            "GITEA_TOKEN_SelfHosted".to_string(),
            "gitea-token".to_string(),
        ),
    ]))
}

// --- the schema --------------------------------------------------------------

#[test]
fn a_sourced_entry_parses_alongside_the_string_forms() {
    let config = Config::parse(ACCOUNTS).unwrap();

    let personal = config.account("Personal").unwrap();
    let sourced = personal.source_for("GH_TOKEN").expect("declared from gh");
    assert_eq!(sourced.from, "gh");
    assert_eq!(sourced.user.as_deref(), Some("octocat"));
    assert_eq!(sourced.host, None);

    // The string forms are untouched by the new variant.
    let selfhosted = config.account("SelfHosted").unwrap();
    assert!(selfhosted.source_for("GITEA_TOKEN").is_none());
    assert_eq!(
        selfhosted
            .env
            .iter()
            .find(|s| s.name() == "GITEA_HOST")
            .and_then(|s| s.literal()),
        Some("https://git.example.net")
    );
}

/// A referenced variable needs no stored value, so reporting it as missing from
/// the store would make a correct setup look broken.
#[test]
fn a_referenced_variable_is_not_something_the_store_must_hold() {
    let config = Config::parse(ACCOUNTS).unwrap();

    let personal = config.account("Personal").unwrap();
    assert!(
        personal.secret_vars().is_empty(),
        "GH_TOKEN comes from gh, so nothing is owed to the store: {:?}",
        personal.secret_vars()
    );
    assert_eq!(personal.sourced_vars().len(), 1);

    // Even though it is also this account's gitCredential -- which is the case
    // that would otherwise slip back in through the git-credential path.
    assert_eq!(personal.git_credential.as_deref(), Some("GH_TOKEN"));

    let selfhosted = config.account("SelfHosted").unwrap();
    assert_eq!(selfhosted.secret_vars(), vec!["GITEA_TOKEN"]);
}

// --- fetching ----------------------------------------------------------------

#[test]
fn the_user_is_passed_through_so_gh_never_has_to_switch() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    // The key encodes the exact command line: --user, and no `gh auth switch`.
    let value = fetch(&gh_holding("gho_from-gh"), "Personal", sourced).unwrap();
    assert_eq!(value, "gho_from-gh");
}

#[test]
fn a_host_is_only_sent_when_declared() {
    let toml = ACCOUNTS.replace(
        r#"{ var = "GH_TOKEN", from = "gh", user = "octocat" }"#,
        r#"{ var = "GH_TOKEN", from = "gh", user = "octocat", host = "github.example.com" }"#,
    );
    let config = Config::parse(&toml).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let runner = MapRunner::new(HashMap::from([(
        MapRunner::key(
            "gh",
            &[
                "auth",
                "token",
                "--hostname",
                "github.example.com",
                "--user",
                "octocat",
            ],
        ),
        ok("gho_enterprise"),
    )]));

    assert_eq!(
        fetch(&runner, "Personal", sourced).unwrap(),
        "gho_enterprise"
    );
}

/// Trailing whitespace on a token is rejected by the server with an error that
/// mentions nothing about whitespace.
#[test]
fn the_fetched_value_is_trimmed() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let value = fetch(&gh_holding("gho_padded\n"), "Personal", sourced).unwrap();
    assert_eq!(value, "gho_padded");
}

#[test]
fn a_tool_that_is_not_installed_says_so() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let error = fetch(&gh_holding("x").without("gh"), "Personal", sourced)
        .expect_err("gh is absent from PATH");

    assert!(
        matches!(error, SourceError::NotInstalled { .. }),
        "got {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("Personal") && message.contains("GH_TOKEN"),
        "{message}"
    );
}

/// gh's own wording is better than anything this could synthesise, so it is
/// passed through rather than replaced.
#[test]
fn a_missing_account_reports_what_the_tool_said() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let runner = MapRunner::new(HashMap::from([(
        MapRunner::key("gh", &["auth", "token", "--user", "octocat"]),
        refused("no oauth token found for github.com account octocat"),
    )]));

    let error = fetch(&runner, "Personal", sourced).expect_err("gh has no such account");
    let message = error.to_string();

    assert!(
        matches!(error, SourceError::NoCredentials { .. }),
        "got {error:?}"
    );
    assert!(message.contains("no oauth token found"), "{message}");
    assert!(message.contains("octocat"), "{message}");
}

/// A stored empty string would satisfy every "is it present?" check while
/// authenticating as nobody. Same rule as the store's own reader.
#[test]
fn an_empty_answer_is_refused_rather_than_returned() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let error = fetch(&gh_holding("  \n"), "Personal", sourced).expect_err("empty is not a token");
    assert!(matches!(error, SourceError::Empty { .. }), "got {error:?}");
}

#[test]
fn an_unknown_source_names_itself_rather_than_guessing() {
    let toml = ACCOUNTS.replace(r#"from = "gh""#, r#"from = "ghh""#);
    let config = Config::parse(&toml).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    let error = fetch(&gh_holding("x"), "Personal", sourced).expect_err("ghh is not a source");
    let message = error.to_string();

    assert!(
        matches!(error, SourceError::UnknownSource { .. }),
        "got {error:?}"
    );
    assert!(message.contains("ghh"), "{message}");
}

// --- the resolver ------------------------------------------------------------

#[test]
fn a_declared_source_is_used_in_preference_to_the_store() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let personal = config.account("Personal").unwrap();

    let value = value_for(
        &store_holding_everything(),
        &gh_holding("from-gh"),
        personal,
        "GH_TOKEN",
    )
    .unwrap();

    // The store holds "from-the-store" for exactly this account and variable.
    // The declaration is what decides, not whichever answers first.
    assert_eq!(value.as_deref(), Some("from-gh"));
}

#[test]
fn a_variable_with_no_source_still_comes_from_the_store() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let selfhosted = config.account("SelfHosted").unwrap();

    let value = value_for(
        &store_holding_everything(),
        &gh_holding("from-gh"),
        selfhosted,
        "GITEA_TOKEN",
    )
    .unwrap();

    assert_eq!(value.as_deref(), Some("gitea-token"));
}

/// The whole point. A source that cannot answer must fail, even when the store
/// is holding a perfectly good value for the same account and variable --
/// because that value is the stale copy this feature exists to stop keeping.
#[test]
fn a_failing_source_never_falls_back_to_the_store() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let personal = config.account("Personal").unwrap();

    let error = value_for(
        &store_holding_everything(),
        &gh_holding("unused").without("gh"),
        personal,
        "GH_TOKEN",
    )
    .expect_err("a declared source that cannot answer is a fault, not an absence");

    assert!(
        matches!(error, ValueError::Source(SourceError::NotInstalled { .. })),
        "got {error:?}"
    );
    assert!(
        !error.to_string().contains("from-the-store"),
        "the store's value must not appear anywhere: {error}"
    );
}

/// Nothing in this module may put a fetched value into a message.
#[test]
fn no_failure_path_echoes_a_value() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let sourced = config
        .account("Personal")
        .unwrap()
        .source_for("GH_TOKEN")
        .unwrap();

    // gh exits non-zero but has still written something to stdout: a helper
    // that quoted stdout on failure would leak it.
    let runner = MapRunner::new(HashMap::from([(
        MapRunner::key("gh", &["auth", "token", "--user", "octocat"]),
        Captured {
            success: false,
            stdout: "gho_leaked_secret".to_string(),
            stderr: "gh: something went wrong".to_string(),
        },
    )]));

    let error = fetch(&runner, "Personal", sourced).expect_err("non-zero is a failure");
    assert!(
        !error.to_string().contains("gho_leaked_secret"),
        "stdout must never reach a message: {error}"
    );
}

/// A mistyped field has to name itself.
///
/// Deriving `untagged` here reports "data did not match any variant of untagged
/// enum EnvSpec", which names neither the field nor the fix -- and this is a
/// file whose mistakes hand out credentials, so an unreadable parse error is
/// not a cosmetic problem. Pinned because the cheap derive is the obvious thing
/// for someone to reach for later.
#[test]
fn a_mistyped_field_names_itself_and_the_alternatives() {
    let toml = ACCOUNTS.replace(r#"from = "gh""#, r#"form = "gh""#);

    let error = Config::parse(&toml)
        .expect_err("form is not a field")
        .to_string();

    assert!(error.contains("unknown field `form`"), "{error}");
    assert!(error.contains("`var`"), "{error}");
    assert!(error.contains("`from`"), "{error}");
    assert!(
        !error.contains("did not match any variant"),
        "the untagged derive is back: {error}"
    );
}

// --- not finding our own shims ----------------------------------------------

/// gitwho puts a shim directory at the front of `PATH`, and its `gh` runs
/// `gitwho exec -- /real/gh`. Resolving `gh` normally from inside the credential
/// helper therefore re-enters gitwho, which resolves an account, which asks for
/// a token, which runs `gh`.
///
/// This was found by running the feature on a real machine, not by reading the
/// code, and it fails in a way that looks like a config error rather than a
/// loop -- so it is pinned here.
#[cfg(unix)]
#[test]
fn a_program_is_never_resolved_from_our_own_shim_directory() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let shims = root.path().join("shims");
    let real = root.path().join("bin");
    std::fs::create_dir_all(&shims).unwrap();
    std::fs::create_dir_all(&real).unwrap();

    for dir in [&shims, &real] {
        let exe = dir.join("gh");
        std::fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // The shim directory first, exactly as an installed machine has it.
    let path = format!("{}:{}", shims.display(), real.display());

    let skipping = ProcessRunner::skipping(vec![shims.clone()]);
    assert_eq!(
        skipping.resolve_in("gh", &path),
        Some(real.join("gh")),
        "the shim must be stepped over even though it comes first"
    );

    // And without the exclusion, the shim is what a plain lookup finds -- which
    // is the loop this exists to prevent.
    let naive = ProcessRunner::unshimmed();
    assert_eq!(naive.resolve_in("gh", &path), Some(shims.join("gh")));
}

/// An absolute path was never a `PATH` lookup, so nothing is skipped.
#[cfg(unix)]
#[test]
fn an_absolute_program_path_is_passed_through() {
    let runner = ProcessRunner::skipping(vec![std::path::PathBuf::from("/anything")]);
    assert_eq!(
        runner.resolve_in("/usr/bin/true", "/nowhere"),
        Some(std::path::PathBuf::from("/usr/bin/true"))
    );
}

/// A tool that is genuinely absent has to be distinguishable from one that ran
/// and refused.
#[test]
fn a_program_that_is_nowhere_on_the_path_resolves_to_nothing() {
    let runner = ProcessRunner::unshimmed();
    assert_eq!(
        runner.resolve_in("gitwho-no-such-program", "/nonexistent"),
        None
    );
}
