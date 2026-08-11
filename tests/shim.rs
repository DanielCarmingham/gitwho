//! Shim generation for both targets.
//!
//! The Windows target is exercised as a pure function because it cannot be
//! exercised any other way from here: there is no Windows machine, so the
//! script's *content* is checkable and its behaviour is not. Said out loud so
//! nothing below reads as proof that a Windows shim runs.

use gitfriend::shim::{find_real_binary, shim_file_name, shim_script, ShimTarget};

#[test]
fn a_posix_shim_is_still_a_sh_script() {
    // A characterisation guard, not a driver: this pins today's output through
    // the ShimTarget refactor so the platform that is actually in use cannot
    // change shape unnoticed.
    let script = shim_script("gh", "/usr/local/bin/gitfriend", "/opt/bin/gh", ShimTarget::Posix);

    assert_eq!(shim_file_name("gh", ShimTarget::Posix), "gh");
    assert!(script.starts_with("#!/bin/sh\n"), "got:\n{script}");
    assert!(script.contains("exec '/usr/local/bin/gitfriend' exec -- '/opt/bin/gh' \"$@\""));
}

/// `PATHEXT` resolves `gh.exe` or `gh.cmd` and never an extensionless file, so
/// a shim written as plain `gh` would simply never be found.
#[test]
fn a_windows_shim_is_a_cmd_file_that_forwards_its_arguments() {
    let script = shim_script(
        "gh",
        r"C:\Users\d\bin\gitfriend.exe",
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
        script.contains(r#""C:\Users\d\bin\gitfriend.exe" exec -- "C:\Program Files\GitHub CLI\gh.exe" %*"#),
        "the wrapped call should quote both paths and forward every argument; got:\n{script}"
    );
}

/// `cmd` discards the exit code of the last command when a batch file ends, so
/// a failing `gh` would look like success to whatever ran the shim.
#[test]
fn a_windows_shim_passes_the_exit_code_back_out() {
    let script = shim_script("gh", "gitfriend.exe", "gh.exe", ShimTarget::Windows);

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
    let script = shim_script("gh", r"C:\bin\gitfriend.exe", r"C:\100%\gh.exe", ShimTarget::Windows);

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
