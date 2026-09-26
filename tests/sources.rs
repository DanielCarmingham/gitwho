//! Not finding our own shims.

use gitwho::sources::{ProcessRunner, Runner};

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

/// An exported GITEA_TOKEN or GH_TOKEN must not let gh/tea answer from the
/// ambient environment instead of the login gitwho asked for (R8).
#[cfg(unix)]
#[test]
fn a_cleared_variable_never_reaches_the_child_process() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("print_var.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho \"${GITWHO_TEST_CLEARED_VAR:-unset}\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    std::env::set_var("GITWHO_TEST_CLEARED_VAR", "leaked");

    let runner = ProcessRunner::unshimmed().clearing(&["GITWHO_TEST_CLEARED_VAR"]);
    let captured = runner
        .run(script.to_str().unwrap(), &[])
        .unwrap()
        .expect("the script should run");

    std::env::remove_var("GITWHO_TEST_CLEARED_VAR");

    assert_eq!(captured.stdout.trim(), "unset");
}

/// Windows resolves a bare `gh` as `gh.exe` (or `.com`/`.bat`/`.cmd`) and never
/// as an extensionless file, so without this every token fetch there reported
/// gh as not installed. Reached from any host by naming the target.
#[cfg(unix)]
#[test]
fn a_windows_target_finds_a_bare_name_by_its_executable_extension() {
    use gitwho::shim::ShimTarget;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("gh.exe");
    std::fs::write(&exe, "").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();

    let windows = ProcessRunner::unshimmed().for_target(ShimTarget::Windows);
    assert_eq!(windows.resolve_in("gh", dir.path()), Some(exe));

    let posix = ProcessRunner::unshimmed().for_target(ShimTarget::Posix);
    assert_eq!(posix.resolve_in("gh", dir.path()), None);
}
