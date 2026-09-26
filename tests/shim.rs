//! Shim generation for both targets.
//!
//! The Windows target is exercised as a pure function because it cannot be
//! exercised any other way from here: there is no Windows machine, so the
//! script's *content* is checkable and its behaviour is not. Said out loud so
//! nothing below reads as proof that a Windows shim runs.

use gitwho::shim::{find_real_binary, shim_file_name, shim_script, ShimTarget};

#[test]
fn a_posix_shim_is_still_a_sh_script() {
    // A characterisation guard, not a driver: this pins today's output through
    // the ShimTarget refactor so the platform that is actually in use cannot
    // change shape unnoticed.
    let script = shim_script(
        "gh",
        "/usr/local/bin/gitwho",
        "/opt/bin/gh",
        ShimTarget::Posix,
    );

    assert_eq!(shim_file_name("gh", ShimTarget::Posix), "gh");
    assert!(script.starts_with("#!/bin/sh\n"), "got:\n{script}");
    assert!(script.contains("exec '/usr/local/bin/gitwho' exec -- '/opt/bin/gh' \"$@\""));
}

/// `PATHEXT` resolves `gh.exe` or `gh.cmd` and never an extensionless file, so
/// a shim written as plain `gh` would simply never be found.
#[test]
fn a_windows_shim_is_a_cmd_file_that_forwards_its_arguments() {
    let script = shim_script(
        "gh",
        r"C:\Users\d\bin\gitwho.exe",
        r"C:\Program Files\GitHub CLI\gh.exe",
        ShimTarget::Windows,
    );

    assert_eq!(shim_file_name("gh", ShimTarget::Windows), "gh.cmd");
    assert!(!script.contains("#!/bin/sh"), "got:\n{script}");
    // CRLF, unlike every other file this project writes: `cmd` mis-parses an
    // LF-only batch file around labels, and the global "LF everywhere" rule is
    // about shell scripts, which this is not.
    assert!(script.starts_with("@echo off\r\n"), "got:\n{script}");
    assert!(
        !script.contains("\n\n") && script.matches('\r').count() == script.matches('\n').count(),
        "every line should be CRLF-terminated; got:\n{script:?}"
    );
    assert!(
        script.contains(
            r#""C:\Users\d\bin\gitwho.exe" exec -- "C:\Program Files\GitHub CLI\gh.exe" %*"#
        ),
        "the wrapped call should quote both paths and forward every argument; got:\n{script}"
    );
}

/// `cmd` discards the exit code of the last command when a batch file ends, so
/// a failing `gh` would look like success to whatever ran the shim.
#[test]
fn a_windows_shim_passes_the_exit_code_back_out() {
    let script = shim_script("gh", "gitwho.exe", "gh.exe", ShimTarget::Windows);

    assert!(
        script.trim_end().ends_with("exit /b %ERRORLEVEL%"),
        "got:\n{script}"
    );
}

/// A literal `%` in a batch file starts a variable reference. Doubling it is the
/// escape; a path containing one would otherwise be silently mangled into
/// something shorter. (`"` needs no escaping -- a Windows path cannot contain
/// one.)
#[test]
fn a_percent_in_a_path_is_escaped_for_cmd() {
    let script = shim_script(
        "gh",
        r"C:\bin\gitwho.exe",
        r"C:\100%\gh.exe",
        ShimTarget::Windows,
    );

    assert!(
        script.contains(r"C:\100%%\gh.exe"),
        "the percent should be doubled; got:\n{script}"
    );
}

#[test]
fn find_real_binary_finds_an_exe_when_the_target_uses_path_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("gh.exe");
    std::fs::write(&real, b"MZ").unwrap();
    let shim_dir = dir.path().join("shims");
    std::fs::create_dir(&shim_dir).unwrap();

    let found = find_real_binary(
        "gh",
        &shim_dir,
        &dir.path().display().to_string(),
        ShimTarget::Windows,
    );

    assert_eq!(found.as_deref(), Some(real.as_path()));
}

/// A name that already carries an extension is used as written, rather than
/// having a second one appended.
#[test]
fn a_name_that_already_has_an_extension_is_looked_up_as_given() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("gh.exe");
    std::fs::write(&real, b"MZ").unwrap();
    let shim_dir = dir.path().join("shims");
    std::fs::create_dir(&shim_dir).unwrap();

    let found = find_real_binary(
        "gh.exe",
        &shim_dir,
        &dir.path().display().to_string(),
        ShimTarget::Windows,
    );

    assert_eq!(found.as_deref(), Some(real.as_path()));
}

/// Another characterisation guard, and the one worth keeping: skipping the shim
/// directory is what stops a generated shim from finding itself and recursing
/// forever.
#[test]
fn find_real_binary_still_ignores_the_shim_directory() {
    let dir = tempfile::tempdir().unwrap();
    let shim_dir = dir.path().join("shims");
    std::fs::create_dir(&shim_dir).unwrap();
    std::fs::write(shim_dir.join("gh"), b"#!/bin/sh\n").unwrap();
    let real_dir = dir.path().join("real");
    std::fs::create_dir(&real_dir).unwrap();
    std::fs::write(real_dir.join("gh"), b"#!/bin/sh\n").unwrap();

    let path_var = format!("{}:{}", shim_dir.display(), real_dir.display());
    let found = find_real_binary("gh", &shim_dir, &path_var, ShimTarget::Posix);

    assert_eq!(found.as_deref(), Some(real_dir.join("gh").as_path()));
}

/// A re-run that reports "wrote" for a file it did not change makes the report
/// useless for spotting the one thing that *did* move -- and `gitwho init` is
/// meant to be re-run every time an account is added.
#[cfg(unix)]
#[test]
fn installing_the_same_shim_twice_reports_the_second_as_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("bin");
    let shim_dir = dir.path().join("shims");
    std::fs::create_dir_all(&real_dir).unwrap();

    let real = real_dir.join("gh");
    std::fs::write(&real, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path_var = real_dir.display().to_string();

    let first = gitwho::shim::install("gh", &shim_dir, &path_var).unwrap();
    assert!(first.changed, "the first install must write");

    let second = gitwho::shim::install("gh", &shim_dir, &path_var).unwrap();
    assert!(
        !second.changed,
        "an identical second install must report no change"
    );
    assert_eq!(first.path, second.path);
}

mod establishes_credentials {
    use gitwho::shim::establishes_credentials;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The case this exists for: a token in the environment is exactly what
    /// stops `gh auth login` from storing one.
    #[test]
    fn gh_auth_login_must_not_see_an_injected_token() {
        assert!(establishes_credentials("gh", &args(&["auth", "login"])));
    }

    /// The shim runs the real binary by absolute path, so matching the whole
    /// string would never fire where it matters.
    #[test]
    fn the_program_is_matched_by_its_file_name_not_its_path() {
        assert!(establishes_credentials(
            "/opt/homebrew/bin/gh",
            &args(&["auth", "login"])
        ));
        assert!(establishes_credentials(
            "C:\\Program Files\\GitHub CLI\\gh.exe",
            &args(&["auth", "login"])
        ));
    }

    #[test]
    fn flags_around_the_subcommand_do_not_hide_it() {
        assert!(establishes_credentials(
            "gh",
            &args(&["auth", "login", "--hostname", "github.com"])
        ));
        assert!(establishes_credentials(
            "gh",
            &args(&["--verbose", "auth", "login"])
        ));
    }

    #[test]
    fn the_other_credential_establishing_subcommands_are_covered() {
        for sub in [
            vec!["auth", "logout"],
            vec!["auth", "refresh"],
            vec!["auth", "switch"],
            vec!["auth", "setup-git"],
        ] {
            assert!(
                establishes_credentials("gh", &args(&sub)),
                "{sub:?} should pass through"
            );
        }
        assert!(establishes_credentials("tea", &args(&["login", "add"])));
        assert!(establishes_credentials("tea", &args(&["logout"])));
    }

    /// Everything else is the ordinary case, and must still be given the
    /// account's credentials -- that is the whole point of the shim.
    #[test]
    fn ordinary_commands_are_untouched() {
        assert!(!establishes_credentials("gh", &args(&["pr", "list"])));
        assert!(!establishes_credentials("gh", &args(&["auth", "status"])));
        assert!(!establishes_credentials("gh", &args(&["auth", "token"])));
        assert!(!establishes_credentials("gh", &args(&[])));
        assert!(!establishes_credentials("tea", &args(&["pr", "list"])));
    }

    /// A tool gitwho knows nothing about gets the normal treatment, including
    /// one that merely happens to have a subcommand spelled the same way.
    #[test]
    fn an_unknown_program_is_never_passed_through() {
        assert!(!establishes_credentials("git", &args(&["auth", "login"])));
        assert!(!establishes_credentials("glab", &args(&["auth", "login"])));
    }
}

mod fetches_token {
    use gitwho::shim::fetches_token;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Exactly what `sources::token` and `discover` run through a shim.
    #[test]
    fn the_commands_gitwho_runs_to_read_a_token_are_recognised() {
        let gh_token = args(&[
            "auth",
            "token",
            "--hostname",
            "github.com",
            "--user",
            "octocat",
        ]);
        assert!(fetches_token("/opt/homebrew/bin/gh", &gh_token));
        assert!(fetches_token("gh", &args(&["auth", "status"])));
        assert!(fetches_token("tea", &args(&["login", "ls", "-o", "json"])));
        assert!(fetches_token("tea", &args(&["login", "helper", "get"])));
        assert!(fetches_token(
            "C:\\bin\\tea.exe",
            &args(&["login", "helper", "get"])
        ));
    }

    #[test]
    fn anything_else_is_not() {
        assert!(!fetches_token("gh", &args(&["pr", "list"])));
        assert!(!fetches_token("gh", &args(&["api", "user"])));
        assert!(!fetches_token("tea", &args(&["repos", "ls"])));
        assert!(!fetches_token("git", &args(&["auth", "token"])));
    }
}
