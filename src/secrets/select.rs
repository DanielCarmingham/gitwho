//! Which store holds the values, decided without opening one.
//!
//! **This module deliberately does not import `keyring`.** That absence is the
//! guarantee, not a convention: a selection that constructed an `Entry` to see
//! whether the store worked would be the GUI prompt it exists to avoid, and no
//! test can assert the absence of a call that was never written.
//!
//! ## Why strength is not detected
//!
//! "Will the keychain prompt?" cannot be answered without opening the keychain,
//! and opening it is the failure mode (see [`KeychainBackend`] and
//! `examples/keychain_probe.rs`). The obvious substitute -- asking `codesign`
//! whether this binary's designated requirement is stable -- is a process spawn
//! on a path budgeted in single-digit milliseconds (R15), and a valid signature
//! still would not prove the ACL survived the last rebuild. Only the probe does.
//!
//! So the capability is split in two, with different answers:
//!
//! | Question | Answered from | Effect |
//! |---|---|---|
//! | Is the store *present* here? | environment and filesystem facts | refuses even an explicit request, loudly |
//! | Is it strong enough, and prompt-free? | unproven; the operator runs the probe | never chosen automatically; honoured when asked for |
//!
//! [`KeychainBackend`]: super::KeychainBackend

use std::path::Path;

/// The variable that overrides everything else, matching the `GITFRIEND_*`
/// convention the paths already use.
///
/// An environment variable rather than a flag: the credential helper is
/// launched by git, so a flag would never reach the path that matters.
pub const ENV_VAR: &str = "GITFRIEND_SECRET_BACKEND";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    AgeFile,
    Keychain,
}

/// What may be written in `GITFRIEND_SECRET_BACKEND` or `secretBackend`.
///
/// Kept beside [`BackendKind::parse`] so an added backend cannot be accepted
/// without also appearing in the error that lists them.
pub const KNOWN: [&str; 2] = ["age", "keychain"];

/// The age file, on every platform, today.
///
/// macOS: the keychain's per-application ACL is the one property worth having
/// here, and it is unverified for this binary. Windows and Linux: DPAPI and the
/// Secret Service draw their boundary at the *user*, which owner-only
/// permissions on the identity file already reach -- so flipping the default
/// there would buy nothing measurable, on a machine nobody has measured.
const DEFAULT: BackendKind = BackendKind::AgeFile;

impl BackendKind {
    pub fn name(self) -> &'static str {
        match self {
            BackendKind::AgeFile => "age",
            BackendKind::Keychain => "keychain",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "age" => Some(BackendKind::AgeFile),
            "keychain" => Some(BackendKind::Keychain),
            _ => None,
        }
    }
}

/// Where the answer came from. Reported by `doctor`, because "which store am I
/// using" is only half the question an operator is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Environment,
    Config,
    Default,
}

impl Source {
    pub fn describe(self) -> &'static str {
        match self {
            Source::Environment => "by GITFRIEND_SECRET_BACKEND",
            Source::Config => "by [defaults] secretBackend in accounts.toml",
            Source::Default => "by default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub kind: BackendKind,
    pub source: Source,
}

/// The facts about this machine that decide whether a store exists at all.
///
/// A value rather than a lookup, so the whole decision is a pure function of
/// something a test can hand it.
#[derive(Debug, Clone)]
pub struct Platform {
    /// `std::env::consts::OS` on a real machine.
    pub os: String,
    /// Whether there is a D-Bus session bus, which is what the Secret Service
    /// is reached through.
    pub session_bus: bool,
}

impl Platform {
    /// Read the machine. Environment and one `exists` check -- no store is
    /// opened and no process is spawned.
    pub fn detect() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            session_bus: std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
                || std::env::var_os("XDG_RUNTIME_DIR")
                    .is_some_and(|dir| Path::new(&dir).join("bus").exists()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SelectError {
    #[error("unknown secret backend {name:?}; known backends are {}", KNOWN.join(", "))]
    Unknown { name: String },
    #[error("the {kind} store is not available here: {reason}")]
    Unavailable {
        kind: &'static str,
        reason: String,
    },
}

/// Why this store cannot be opened on this machine, if it cannot.
///
/// Only one store can genuinely be *absent*: the Secret Service needs a session
/// bus, and a headless box or a CI runner has none. The macOS login keychain and
/// the Windows Credential Manager are always there. An unrecognised platform is
/// treated like Linux rather than assumed fine -- erring toward a refusal that
/// says why, rather than a failure inside the store later.
pub fn missing_store_reason(kind: BackendKind, platform: &Platform) -> Option<String> {
    if kind != BackendKind::Keychain {
        return None;
    }
    match platform.os.as_str() {
        "macos" | "ios" | "windows" => None,
        _ if platform.session_bus => None,
        _ => Some(
            "the Secret Service is reached over a D-Bus session bus, and neither \
             DBUS_SESSION_BUS_ADDRESS nor $XDG_RUNTIME_DIR/bus is present -- the usual \
             shape of a headless machine or a CI runner"
                .to_string(),
        ),
    }
}

/// Resolve the backend: environment, then config, then the default.
///
/// Never falls back. An unknown name and an absent store are both refusals,
/// because the alternative is a store that works while being the wrong one --
/// the failure mode R8 exists to rule out.
pub fn choose(
    env_override: Option<&str>,
    configured: Option<&str>,
    platform: &Platform,
) -> Result<Choice, SelectError> {
    let requested = stated(env_override)
        .map(|name| (name, Source::Environment))
        .or_else(|| stated(configured).map(|name| (name, Source::Config)));

    let Some((name, source)) = requested else {
        return Ok(Choice {
            kind: DEFAULT,
            source: Source::Default,
        });
    };

    let kind = BackendKind::parse(name).ok_or_else(|| SelectError::Unknown {
        name: name.to_string(),
    })?;

    if let Some(reason) = missing_store_reason(kind, platform) {
        return Err(SelectError::Unavailable {
            kind: kind.name(),
            reason,
        });
    }

    Ok(Choice { kind, source })
}

/// A value someone actually wrote. `GITFRIEND_SECRET_BACKEND=` is how a shell
/// clears a variable, not a request for a backend named `""`.
fn stated(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}
