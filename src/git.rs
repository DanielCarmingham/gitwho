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

/// Every configured `credential.helper`, in the order git would consult them.
pub fn credential_helpers() -> Vec<String> {
    config_values(&["config", "--get-all", "credential.helper"])
}

/// `credential.useHttpPath` for github.com. `None` when unset.
///
/// Asked as the URL-scoped question git itself would ask, so a
/// `[credential "https://github.com"]` section is honoured rather than only
/// the global default.
pub fn use_http_path_for_github() -> Option<bool> {
    let values = config_values(&[
        "config",
        "--get-urlmatch",
        "credential.useHttpPath",
        "https://github.com",
    ]);
    values.first().map(|v| v == "true")
}

fn config_values(args: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new("git").args(args).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}
