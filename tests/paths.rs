//! Where gitwho looks for its own files.
//!
//! Both the environment *and* the platform layout are injected, so the Windows
//! answer is exercised from this Mac and nothing here depends on the
//! developer's real home directory.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use gitwho::paths::{config_dir, Layout, PathError};

/// The environment as a lookup, which is the shape `config_dir` takes: a map of
/// everything would mean decoding variables it has no business reading.
fn dir_for(pairs: &[(&str, &str)], layout: Layout) -> Result<PathBuf, PathError> {
    let env: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    config_dir(&|var| env.get(var).map(OsString::from), layout)
}

#[test]
fn the_config_directory_hangs_off_home() {
    let dir = dir_for(&[("HOME", "/Users/someone")], Layout::Unix).unwrap();

    assert!(
        dir.ends_with("gitwho"),
        "expected gitwho's own directory; got {}",
        dir.display()
    );
    assert!(
        dir.starts_with("/Users/someone"),
        "expected it under HOME; got {}",
        dir.display()
    );
}

/// Windows sets `USERPROFILE`, not `HOME`. Consulting only `HOME` is why the
/// path code was platform-bound.
#[test]
fn the_config_directory_falls_back_to_userprofile_when_home_is_unset() {
    let dir = dir_for(&[("USERPROFILE", "/Users/someone")], Layout::Unix).unwrap();

    assert!(
        dir.starts_with("/Users/someone"),
        "USERPROFILE should stand in for HOME; got {}",
        dir.display()
    );
}

/// The branch a `cfg!(windows)` made unreachable from here. It was documented
/// as tested while no test could reach it, which is the failure mode the
/// evidence rule exists to prevent -- so the layout is a parameter, exactly as
/// `shim::ShimTarget` is.
///
/// Unverified in the only sense left: the *shape* is pinned here, but no
/// Windows machine has ever read the resulting directory.
#[test]
fn the_windows_layout_puts_the_store_under_appdata() {
    let dir = dir_for(
        &[
            ("APPDATA", r"C:\Users\someone\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\someone"),
        ],
        Layout::Windows,
    )
    .unwrap();

    assert_eq!(
        dir,
        PathBuf::from(r"C:\Users\someone\AppData\Roaming").join("gitwho"),
        "APPDATA is where Windows keeps per-user application data; got {}",
        dir.display()
    );
}

/// A dotfile directory under the profile root is invisible to every convention
/// on Windows, so `APPDATA` has to win where both are set.
#[test]
fn appdata_wins_over_userprofile_on_windows() {
    let dir = dir_for(
        &[
            ("APPDATA", r"C:\Users\someone\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\someone"),
        ],
        Layout::Windows,
    )
    .unwrap();

    assert!(
        !dir.starts_with(r"C:\Users\someone\.config"),
        "the unix layout was used on Windows; got {}",
        dir.display()
    );
}

/// The same environment, read two ways. Injecting the layout is only worth
/// anything if it actually changes the answer.
#[test]
fn the_layout_and_not_the_host_decides_which_variable_is_consulted() {
    let env = [
        ("APPDATA", r"C:\Users\someone\AppData\Roaming"),
        ("HOME", "/Users/someone"),
    ];

    let unix = dir_for(&env, Layout::Unix).unwrap();
    let windows = dir_for(&env, Layout::Windows).unwrap();

    assert_ne!(
        unix, windows,
        "both layouts resolved to {}; the parameter is doing nothing",
        unix.display()
    );
}

/// The bug this replaces: `HOME` missing produced a *relative* path, so
/// gitwho read its config out of the current working directory. Wrong and
/// quiet is worse than broken and loud (R8).
#[test]
fn no_home_variable_at_all_is_an_error_naming_both() {
    let error = dir_for(&[("PATH", "/usr/bin")], Layout::Unix)
        .expect_err("with no home variable there is no defensible directory");

    let message = error.to_string();
    assert!(
        message.contains("HOME") && message.contains("USERPROFILE"),
        "the error should name both variables so it can be acted on; got: {message}"
    );
    assert!(matches!(error, PathError::NoHome { .. }));
}

/// The error has to name what *this* layout would have used. Telling a Windows
/// user to set `HOME` while the code consulted `APPDATA` first sends them to
/// the wrong variable.
#[test]
fn the_windows_error_names_appdata_because_that_is_what_was_consulted() {
    let error = dir_for(&[("PATH", "/usr/bin")], Layout::Windows)
        .expect_err("with no home variable there is no defensible directory");

    let message = error.to_string();
    assert!(
        message.contains("APPDATA"),
        "the variable consulted first should be named; got: {message}"
    );
}

/// An empty `HOME` is the same failure wearing a different hat: joining onto it
/// yields a relative path just as an absent one did.
#[test]
fn an_empty_home_is_treated_as_unset_rather_than_as_the_root() {
    let error = dir_for(&[("HOME", "")], Layout::Unix)
        .expect_err("an empty HOME names no directory");

    assert!(matches!(error, PathError::NoHome { .. }));
}

#[test]
fn the_resolved_directory_is_absolute() {
    // The whole point: whatever comes back must not be interpreted relative to
    // wherever the process happens to be standing.
    let dir = dir_for(&[("HOME", "/Users/someone")], Layout::Unix).unwrap();

    assert!(dir.is_absolute(), "got a relative path: {}", dir.display());
}
