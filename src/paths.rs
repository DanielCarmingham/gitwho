//! Where gitwho keeps its own files.
//!
//! Split out of `main.rs` so the answer can be tested against an injected
//! environment. The bug this replaces defaulted a missing `HOME` to an empty
//! path, which made every location *relative* -- gitwho would read
//! `accounts.toml` out of whatever directory it was invoked from. A config
//! anyone can drop into a working tree decides which host is handed which
//! token, so that is a redirect vector, not just a porting gap (R8).

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error(
        "none of {} is set, so there is no home directory to put gitwho's files \
         under; set one, or point GITWHO_CONFIG, GITWHO_SECRETS and \
         GITWHO_IDENTITY at explicit paths",
        .consulted.join(", ")
    )]
    NoHome {
        /// The variables this layout would have used, in order. Named rather
        /// than fixed, because a Windows user told to set `HOME` would be sent
        /// to a variable the code never reads first.
        consulted: Vec<&'static str>,
    },
}

/// Which platform's convention for per-user files to follow.
///
/// A parameter rather than a `cfg!`, mirroring [`shim::ShimTarget`]: a
/// compile-time switch made the Windows branch unreachable from a test run
/// here, so it was documented as tested while nothing could reach it.
///
/// [`shim::ShimTarget`]: crate::shim::ShimTarget
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Unix,
    Windows,
}

impl Layout {
    /// The platform this build runs on. `main` uses it; the tests do not, so
    /// both layouts stay reachable from one machine.
    pub const HOST: Layout = if cfg!(windows) {
        Layout::Windows
    } else {
        Layout::Unix
    };
}

/// How a single variable is looked up.
///
/// A function rather than a map of the whole environment, because building one
/// means decoding every variable in it: `std::env::vars` panics on a single
/// non-Unicode entry, and gitwho runs inside whatever environment git or a
/// shell happened to have. Only the names below are ever read.
pub type Lookup<'a> = dyn Fn(&str) -> Option<OsString> + 'a;

/// The real environment, one variable at a time.
pub fn from_process(var: &str) -> Option<OsString> {
    std::env::var_os(var)
}

/// gitwho's own directory, derived from the environment it is handed.
///
/// Values are taken in a stated order -- `APPDATA` (Windows layout only), then
/// `HOME`, then `USERPROFILE` -- rather than guessed from the platform, so a
/// machine with an unusual setup can be reasoned about from this list alone.
pub fn config_dir(env: &Lookup<'_>, layout: Layout) -> Result<PathBuf, PathError> {
    // Windows keeps per-user application data outside the profile root, and a
    // dotfile directory there would be invisible to every convention on that
    // platform. Unverified: there is no Windows machine to measure on.
    if layout == Layout::Windows {
        if let Some(appdata) = present(env, "APPDATA") {
            return Ok(PathBuf::from(appdata).join("gitwho"));
        }
    }

    let home = present(env, "HOME")
        .or_else(|| present(env, "USERPROFILE"))
        .ok_or_else(|| PathError::NoHome {
            consulted: consulted(layout),
        })?;

    Ok(PathBuf::from(home).join(".config").join("gitwho"))
}

/// What this layout would have read, in the order it would have read it.
fn consulted(layout: Layout) -> Vec<&'static str> {
    match layout {
        Layout::Unix => vec!["HOME", "USERPROFILE"],
        Layout::Windows => vec!["APPDATA", "HOME", "USERPROFILE"],
    }
}

/// Set *and* non-empty. `HOME=` is how a shell clears a variable, and treating
/// that as a directory name reintroduces the relative-path bug exactly.
fn present(env: &Lookup<'_>, var: &str) -> Option<OsString> {
    env(var).filter(|value| !value.is_empty())
}
