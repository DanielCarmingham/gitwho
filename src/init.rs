//! The two lines gitwho cannot generate for you.
//!
//! Everything else it writes lives inside its own directory, where re-running
//! is free and uninstalling is deleting a directory. These two do not: one line
//! in your gitconfig and one in your shell rc file are what connect that
//! directory to anything. They are also the two steps of the manual install
//! that were most often got wrong.
//!
//! So they are handled with more care than the generated files:
//!
//! - **Presence is decided by a marker, not by an exact string.** Someone who
//!   pasted the line by hand, or let an editor reindent it, must not end up
//!   with two.
//! - **Nothing is created.** A missing `~/.zshrc` is reported, not conjured --
//!   writing one that a login shell may never read is exactly the kind of
//!   quiet wrongness this project exists to avoid (R8).
//! - **Appending never splices.** A file with no trailing newline gets one
//!   first, so the last existing line survives.

use std::path::Path;

/// A line to be added to a file gitwho does not own.
pub struct Snippet {
    /// The exact text to append, newline-terminated.
    pub text: String,
    /// The distinctive substring whose presence anywhere in the file means this
    /// is already wired.
    ///
    /// Always an absolute path, which is what makes it distinctive: two installs
    /// pointing at different store directories are genuinely different wiring,
    /// and a temp-directory test run must not mistake the real machine's line
    /// for its own.
    pub marker: String,
    /// What this line is for, in a few words, for the report.
    pub purpose: &'static str,
}

impl Snippet {
    /// The one line that connects your gitconfig to everything gitwho
    /// generates -- identity rules and credential sections alike.
    pub fn gitconfig_include(includes: &Path) -> Snippet {
        let path = includes.display().to_string();
        Snippet {
            text: format!(
                "\n# Added by `gitwho init`. Identity and credential rules,\n\
                 # generated from ~/.config/gitwho/accounts.toml.\n\
                 [include]\n\tpath = {path}\n"
            ),
            marker: path,
            purpose: "identity and credential rules",
        }
    }

    /// Put the shims ahead of the real `gh` and `tea`.
    ///
    /// This belongs at the **end** of `~/.zshrc`, not in `~/.zshenv`. `.zshrc`
    /// prepends a dozen or more entries of its own -- Homebrew among them --
    /// so anything set in `.zshenv` ends up buried and the real binary wins.
    pub fn path_export(shim_dir: &Path) -> Snippet {
        let path = shim_dir.display().to_string();
        Snippet {
            text: format!(
                "\n# Added by `gitwho init`. Must stay ahead of the real gh/tea,\n\
                 # so keep it at the end of this file.\n\
                 export PATH=\"{path}:$PATH\"\n"
            ),
            marker: path,
            purpose: "shims ahead of the real CLIs on PATH",
        }
    }

    pub fn is_present_in(&self, contents: &str) -> bool {
        contents.contains(&self.marker)
    }
}

/// What `ensure` did, or would have done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    AlreadyPresent,
    Appended,
    WouldAppend,
    /// The file does not exist. Deliberately not an error, and deliberately not
    /// a creation -- see the module docs.
    FileMissing,
}

/// Add `snippet` to `path` unless it is already there.
///
/// `write == false` reports without touching anything, matching `sync` and
/// `mcp sync`: these files belong to the user, so the default is to say what
/// would happen.
pub fn ensure(path: &Path, snippet: &Snippet, write: bool) -> std::io::Result<Applied> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Applied::FileMissing),
        Err(e) => return Err(e),
    };

    if snippet.is_present_in(&contents) {
        return Ok(Applied::AlreadyPresent);
    }

    if !write {
        return Ok(Applied::WouldAppend);
    }

    // Read-modify-write rather than an append handle: the newline fix needs to
    // know how the file currently ends, and these files are a few kilobytes.
    let mut updated = contents;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&snippet.text);
    std::fs::write(path, updated)?;

    Ok(Applied::Appended)
}

/// What `ensure_owner_only` found, or would have done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    AlreadyOwnerOnly,
    Tightened,
    WouldTighten,
    /// A platform that cannot express it. `doctor` reports the gap instead.
    NotApplicable,
}

/// Close the store directory down to `0700`, even if something else created it.
///
/// `init` creates the directory 0700 itself, and only tightens one it finds
/// looser -- an explicit setup command, run deliberately, on gitwho's own
/// directory, where `doctor` otherwise just prints `chmod 700` and waits.
///
/// This is not hypothetical. dist's shell installer writes its receipt to
/// `${XDG_CONFIG_HOME:-~/.config}/gitwho/` -- the same directory -- with a
/// plain `mkdir -p`, so the ubiquitous `022` umask leaves it `0755` *before
/// gitwho has run at all*. Following the recommended install and then running
/// `init` would fail `doctor` on permissions gitwho never set.
pub fn ensure_owner_only(dir: &Path, write: bool) -> std::io::Result<Mode> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(dir)?.permissions().mode() & 0o777;
        if mode == 0o700 {
            return Ok(Mode::AlreadyOwnerOnly);
        }
        if !write {
            return Ok(Mode::WouldTighten);
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(Mode::Tightened)
    }

    #[cfg(not(unix))]
    {
        let _ = (dir, write);
        Ok(Mode::NotApplicable)
    }
}

/// The starting `accounts.toml`, shipped in the binary.
///
/// The same bytes as `docs/accounts.toml.example`, so the test that parses that
/// file also covers what `init` scaffolds. Keep `docs/accounts.toml.example`
/// out of any `exclude` list in `Cargo.toml` -- excluding it breaks this
/// `include_str!` at compile time, which at least fails loudly.
pub const TEMPLATE: &str = include_str!("../docs/accounts.toml.example");
