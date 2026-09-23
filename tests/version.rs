use std::process::Command;

/// `ci.yml` never runs on a tag, so every test run builds from a checkout and
/// must be marked as one. A release version here would mean `build.rs` has
/// started treating a working tree as a published build.
#[test]
fn a_build_from_a_checkout_says_it_is_not_a_release() {
    let output = Command::new(env!("CARGO_BIN_EXE_gitwho"))
        .arg("--version")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    let dev = format!("gitwho {}-dev", env!("CARGO_PKG_VERSION"));
    assert!(stdout.starts_with(&dev), "{stdout}");
}
