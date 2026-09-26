//! Generating wrapper scripts that route a CLI through `gitwho exec`.
//!
//! A shim is how a tool that reads environment variables gets covered without
//! wrapping every call site by hand: put the shim directory early on `PATH`,
//! and `gh` means `gitwho exec -- /real/path/to/gh`.
//!
//! The script and the file name are pure functions of a [`ShimTarget`], so the
//! Windows forms are readable and testable from a unix machine. **The Windows
//! target is generated but unverified** -- there is no Windows box here, so what
//! the tests pin is the content of the script, never that it runs.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ShimError {
    #[error("cannot find {name} on PATH to wrap")]
    NotFound { name: String },
    #[error("cannot write shim {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Which platform's conventions a shim is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimTarget {
    Posix,
    Windows,
}

impl ShimTarget {
    /// The platform this build runs on. `install` uses it; the tests do not, so
    /// both targets stay reachable from one machine.
    pub const HOST: ShimTarget = if cfg!(windows) {
        ShimTarget::Windows
    } else {
        ShimTarget::Posix
    };
}

/// The suffixes Windows appends when resolving a bare command name.
///
/// A constant rather than a read of `PATHEXT`: the default is what matters here
/// (an installer puts `gh.exe` on PATH, not `gh.wsf`), and reading the variable
/// would make the baked-in path depend on the environment of whichever shell
/// happened to run `shim install`.
const WINDOWS_EXTENSIONS: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

/// Argument prefixes that must never run with an injected credential, per tool.
///
/// These are the commands whose *purpose* is to establish a credential. Handing
/// one a token does not help it: `gh auth login` refuses outright while
/// `GH_TOKEN` is set, which through a shim makes a brand-new account
/// impossible to set up at all.
///
/// Tool knowledge, so it lives here beside the rest of it rather than inside a
/// generic `exec`. It can go stale when a tool gains a subcommand; the failure
/// mode when it does is the tool's own error, not a silently wrong account.
const ESTABLISHES_CREDENTIALS: &[(&str, &[&[&str]])] = &[
    (
        "gh",
        &[
            &["auth", "login"],
            &["auth", "logout"],
            &["auth", "refresh"],
            &["auth", "switch"],
            &["auth", "setup-git"],
        ],
    ),
    ("tea", &[&["login", "add"], &["logout"]]),
];

/// Whether running `program` with `args` is an attempt to establish a
/// credential, rather than to use one.
///
/// `program` is matched by file name: the shim runs the real binary by absolute
/// path, so anything stricter would never fire where it matters. Arguments are
/// matched ignoring flags, so a global flag before the subcommand cannot hide
/// it.
pub fn establishes_credentials(program: &str, args: &[String]) -> bool {
    matches_any(ESTABLISHES_CREDENTIALS, program, args)
}

/// The commands gitwho itself runs through a CLI to read a token or list its
/// logins: `sources::token` and `discover`. Nothing else is ever run with
/// `sources::FETCHING_TOKEN` set.
const FETCHES_TOKEN: &[(&str, &[&[&str]])] = &[
    ("gh", &[&["auth", "token"], &["auth", "status"]]),
    ("tea", &[&["login", "ls"], &["login", "helper"]]),
];

/// Whether running `program` with `args` is one of gitwho's own token reads,
/// matched the same way as [`establishes_credentials`].
pub fn fetches_token(program: &str, args: &[String]) -> bool {
    matches_any(FETCHES_TOKEN, program, args)
}

fn matches_any(table: &[(&str, &[&[&str]])], program: &str, args: &[String]) -> bool {
    // Both separators, deliberately: `Path` on unix does not treat a backslash
    // as one, so a Windows path would arrive here as a single long file name
    // and match nothing. Keeping this a pure function of the string is what
    // makes the Windows case reachable from a test run on macOS.
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = WINDOWS_EXTENSIONS
        .iter()
        .find_map(|ext| file.strip_suffix(ext))
        .unwrap_or(file);

    let Some((_, prefixes)) = table.iter().find(|(tool, _)| *tool == name) else {
        return false;
    };

    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with('-'))
        .collect();

    prefixes
        .iter()
        .any(|prefix| words.len() >= prefix.len() && words[..prefix.len()] == **prefix)
}

/// What the shim must be called to be found.
///
/// On Windows, `PATHEXT` resolves `gh.exe` or `gh.cmd` and never an
/// extensionless file, so a shim written as plain `gh` would sit on `PATH`
/// being ignored.
pub fn shim_file_name(name: &str, target: ShimTarget) -> String {
    match target {
        ShimTarget::Posix => name.to_string(),
        ShimTarget::Windows => format!("{name}.cmd"),
    }
}

/// The shim's contents, with both absolute paths baked in.
pub fn shim_script(name: &str, gitwho: &str, real: &str, target: ShimTarget) -> String {
    match target {
        ShimTarget::Posix => format!(
            "#!/bin/sh\n\
             # Generated by gitwho. Re-run `gitwho shim install {name}` after\n\
             # the wrapped binary moves.\n\
             exec {} exec -- {} \"$@\"\n",
            shell_quote(gitwho),
            shell_quote(real),
        ),
        // `exit /b` because `cmd` otherwise discards the exit code when a batch
        // file ends, and a failing `gh` would look like success to whatever ran
        // the shim.
        ShimTarget::Windows => format!(
            "@echo off\r\n\
             REM Generated by gitwho. Re-run `gitwho shim install {name}` after\r\n\
             REM the wrapped binary moves.\r\n\
             {} exec -- {} %*\r\n\
             exit /b %ERRORLEVEL%\r\n",
            cmd_quote(gitwho),
            cmd_quote(real),
        ),
    }
}

/// Find the real binary for `name`, ignoring anything inside `shim_dir`.
///
/// Skipping the shim directory is what stops a generated shim from finding
/// itself and recursing forever.
pub fn find_real_binary(
    name: &str,
    shim_dir: &Path,
    path_var: &str,
    target: ShimTarget,
) -> Option<PathBuf> {
    let shim_dir = shim_dir.canonicalize().ok();
    let names = candidate_names(name, target);

    std::env::split_paths(path_var)
        .filter(|dir| match (&shim_dir, dir.canonicalize().ok()) {
            (Some(shim), Some(candidate)) => *shim != candidate,
            _ => true,
        })
        .flat_map(|dir| {
            names
                .iter()
                .map(|candidate| dir.join(candidate))
                .collect::<Vec<_>>()
        })
        .find(|candidate| candidate.is_file())
}

/// The file names a bare command could be on this target, in the order the
/// platform itself would try them.
pub(crate) fn candidate_names(name: &str, target: ShimTarget) -> Vec<String> {
    match target {
        ShimTarget::Posix => vec![name.to_string()],
        // A name that already carries an extension is used as written -- `gh.exe`
        // must not be looked up as `gh.exe.com`.
        ShimTarget::Windows if has_windows_extension(name) => vec![name.to_string()],
        ShimTarget::Windows => WINDOWS_EXTENSIONS
            .iter()
            .map(|ext| format!("{name}{ext}"))
            .collect(),
    }
}

fn has_windows_extension(name: &str) -> bool {
    WINDOWS_EXTENSIONS
        .iter()
        .any(|ext| name.to_ascii_lowercase().ends_with(ext))
}

/// Write a shim for `name` into `shim_dir`.
///
/// The real binary's absolute path is baked in rather than re-resolved at run
/// time: it makes the shim trivially readable, and a stale path is something
/// `doctor` can detect, whereas a recursion loop is not.
pub fn install(name: &str, shim_dir: &Path, path_var: &str) -> Result<Installed, ShimError> {
    std::fs::create_dir_all(shim_dir).map_err(|source| ShimError::Write {
        path: shim_dir.to_path_buf(),
        source,
    })?;

    let target = ShimTarget::HOST;

    let real =
        find_real_binary(name, shim_dir, path_var, target).ok_or_else(|| ShimError::NotFound {
            name: name.to_string(),
        })?;

    let gitwho = std::env::current_exe().map_err(|source| ShimError::Write {
        path: shim_dir.to_path_buf(),
        source,
    })?;

    let script = shim_script(
        name,
        &gitwho.to_string_lossy(),
        &real.to_string_lossy(),
        target,
    );

    let path = shim_dir.join(shim_file_name(name, target));

    // Compare before writing, exactly as `sync::apply` does. A re-run that
    // reported "wrote" for a file it did not change would make the report
    // useless for spotting the one thing that *did* move.
    if std::fs::read_to_string(&path).ok().as_deref() == Some(script.as_str()) {
        return Ok(Installed {
            path,
            changed: false,
        });
    }

    std::fs::write(&path, script).map_err(|source| ShimError::Write {
        path: path.clone(),
        source,
    })?;
    make_executable(&path)?;

    Ok(Installed {
        path,
        changed: true,
    })
}

/// Where a shim went, and whether writing it changed anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    pub changed: bool,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Quote for `cmd`. Double quotes are enough because a Windows path cannot
/// contain one; a literal `%` must be doubled, or `cmd` reads it as the start of
/// a variable reference and swallows the rest of the path.
fn cmd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('%', "%%"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), ShimError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|source| {
        ShimError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// A genuine no-op: Windows decides executability from the extension, and
/// `shim_file_name` has already supplied `.cmd`. Nothing is lost here, so
/// nothing needs reporting.
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), ShimError> {
    Ok(())
}
