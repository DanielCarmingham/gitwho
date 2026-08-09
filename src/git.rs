//! A thin wrapper over the `git` binary.
//!
//! Shelling out rather than linking a git library is deliberate: it guarantees
//! the answers match the git that is actually installed, including its own
//! config resolution rules, which is the thing being reasoned about here.

use std::path::Path;
use std::process::Command;

/// The `origin` remote's URL for the repo containing `dir`, or `None` when
/// `dir` is not a repo or the repo has no `origin`.
///
/// Asking git rather than inspecting the path is what makes this work from a
/// linked worktree, whose `.git` is a file pointing elsewhere (R2).
pub fn origin_url(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!url.is_empty()).then_some(url)
}
