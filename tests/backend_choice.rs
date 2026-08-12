//! Choosing a secret store without opening one.
//!
//! Every platform here is injected, so a headless Linux runner and a Windows
//! box are both exercised from this Mac -- and nothing in this file can pop a
//! Keychain dialog, because `choose` has no way to reach a store.

use gitwho::secrets::select::{self, BackendKind, Platform, SelectError, Source};

fn macos() -> Platform {
    Platform {
        os: "macos".to_string(),
        session_bus: false,
    }
}

fn windows() -> Platform {
    Platform {
        os: "windows".to_string(),
        session_bus: false,
    }
}

fn linux_with_bus() -> Platform {
    Platform {
        os: "linux".to_string(),
        session_bus: true,
    }
}

fn headless_linux() -> Platform {
    Platform {
        os: "linux".to_string(),
        session_bus: false,
    }
}

/// The regression guard on the default. macOS keys a Keychain ACL to the
/// calling binary's designated requirement; for an unsigned build that changes
/// on every rebuild and the read blocks on a GUI prompt (measured 2026-08-09).
/// Until code signing is set up and re-verified, nothing may pick the keychain
/// on its own -- on any platform, since there is no Windows or Linux machine
/// here to measure the alternative on.
#[test]
fn nothing_chooses_the_keychain_on_its_own() {
    for platform in [macos(), windows(), linux_with_bus(), headless_linux()] {
        let choice = select::choose(None, None, &platform).unwrap();

        assert_eq!(
            choice.kind,
            BackendKind::AgeFile,
            "{} must default to the age file",
            platform.os
        );
        assert_eq!(choice.source, Source::Default);
    }
}

/// Presence, not strength. A CI runner or a headless box has no D-Bus session
/// bus, so there is no Secret Service to talk to at all -- and refusing loudly
/// is the only correct answer. Quietly demoting to the age file would hand back
/// a working store the operator did not ask for (R8).
#[test]
fn an_explicit_keychain_is_refused_where_there_is_no_secret_service() {
    let error = select::choose(Some("keychain"), None, &headless_linux())
        .expect_err("there is no store to open");

    let message = error.to_string();
    assert!(
        message.contains("bus"),
        "the reason should name what is missing so it can be fixed; got: {message}"
    );
    assert!(matches!(error, SelectError::Unavailable { .. }));
}

/// The other side of the line: *absent* is refused, *unproven* is not. Whether
/// the macOS keychain will prompt cannot be answered without opening it, and
/// opening it is the bug -- so an operator who has run the probe gets what they
/// asked for.
#[test]
fn an_explicit_keychain_is_honoured_where_the_store_exists() {
    let choice = select::choose(Some("keychain"), None, &macos()).unwrap();

    assert_eq!(choice.kind, BackendKind::Keychain);
    assert_eq!(choice.source, Source::Environment);
}

#[test]
fn a_configured_keychain_is_honoured_and_says_where_it_came_from() {
    let choice = select::choose(None, Some("keychain"), &windows()).unwrap();

    assert_eq!(choice.kind, BackendKind::Keychain);
    assert_eq!(choice.source, Source::Config);
}

/// A typo must not resolve to the default. Silently using the age file when
/// `keychain` was asked for is exactly the working-but-incorrect outcome R8
/// forbids.
#[test]
fn an_unknown_backend_name_is_refused_and_lists_the_real_ones() {
    let error = select::choose(Some("kechain"), None, &macos())
        .expect_err("an unknown name has no defensible fallback");

    let message = error.to_string();
    assert!(
        message.contains("kechain") && message.contains("age") && message.contains("keychain"),
        "the error should quote the typo and name every real backend; got: {message}"
    );
    assert!(matches!(error, SelectError::Unknown { .. }));
}

#[test]
fn the_environment_beats_the_configured_backend() {
    let choice = select::choose(Some("age"), Some("keychain"), &macos()).unwrap();

    assert_eq!(choice.kind, BackendKind::AgeFile);
    assert_eq!(choice.source, Source::Environment);
}

/// `GITWHO_SECRET_BACKEND=` is how a shell clears a variable. Treating the
/// empty string as a backend name would refuse every command in that shell.
#[test]
fn a_cleared_environment_variable_falls_through_to_the_configured_backend() {
    let choice = select::choose(Some(""), Some("keychain"), &macos()).unwrap();

    assert_eq!(choice.kind, BackendKind::Keychain);
    assert_eq!(choice.source, Source::Config);
}

/// R15: selection runs on the git credential hot path. The guard that matters
/// is against anyone later reaching for `codesign` to answer "will it prompt?"
/// -- a process spawn per git transport operation. Detection is env and
/// filesystem facts only, and this is what says so in numbers.
#[test]
fn choosing_a_backend_stays_inside_the_hot_path_budget() {
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let platform = Platform::detect();
        select::choose(None, None, &platform).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "1000 selections took {elapsed:?}; something is spawning a process or opening a store"
    );
}
