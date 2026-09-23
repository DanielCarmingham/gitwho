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

/// The `credential.helper` values that are actually in effect.
///
/// Empty values are not helpers -- they clear everything configured before
/// them. Ignoring that reports helpers git will never call: on this machine
/// Xcode's bundled gitconfig contributes `osxkeychain`, which the user config
/// then resets away.
pub fn credential_helpers() -> Vec<String> {
    let configured = config_values_keeping_empties(&["config", "--get-all", "credential.helper"]);
    effective_helpers(&configured)
}

/// Apply git's reset semantics: keep only what follows the last empty value.
pub fn effective_helpers(configured: &[String]) -> Vec<String> {
    match configured.iter().rposition(|v| v.is_empty()) {
        Some(last_reset) => configured[last_reset + 1..].to_vec(),
        None => configured.to_vec(),
    }
}

/// The helper git will actually use for a URL.
///
/// This is the question that matters: a `[credential "https://github.com"]`
/// section overrides the general list entirely, and on this machine it does --
/// github.com is served by `gh auth git-credential`, not by the global helper.
pub fn credential_helper_for(url: &str) -> Option<String> {
    config_values(&["config", "--get-urlmatch", "credential.helper", url])
        .into_iter()
        .next_back()
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
    config_values_keeping_empties(args)
        .into_iter()
        .filter(|v| !v.is_empty())
        .collect()
}

fn config_values_keeping_empties(args: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new("git").args(args).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(str::to_string)
        .collect()
}

/// Every remote of the repo containing `dir`, as `(name, url)`.
///
/// Empty when `dir` is not a repo. Read from `--local` so the answer is the
/// repo's own remotes and nothing inherited, and in one process rather than a
/// `git remote` followed by a `get-url` per remote.
pub fn remotes(dir: &Path) -> Vec<(String, String)> {
    local_config(dir, &["--get-regexp", r"^remote\..*\.url$"])
        .iter()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            // A remote name may itself contain dots, so trim the fixed ends
            // rather than splitting on the separator.
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            (!name.is_empty() && !url.is_empty()).then(|| (name.to_string(), url.to_string()))
        })
        .collect()
}

/// Whether the repo containing `dir` settles its own identity, with a local
/// `user.email` or a local `include.path`.
///
/// Repo-local config beats every included global rule, so this is what decides
/// whether the generated `includeIf` rules still have the last word.
pub fn identity_pinned(dir: &Path) -> bool {
    ["user.email", "include.path"]
        .iter()
        .any(|key| !local_config(dir, &["--get-all", key]).is_empty())
}

/// What git will actually use for `key` in the repo containing `dir`.
///
/// The *effective* value, not the local one: it is the answer after every
/// include and `includeIf` has been applied, which is the only version worth
/// reporting. Inferring it from the resolved account instead is how a report
/// comes to disagree with what git does.
pub fn effective(dir: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", key])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// `git config --local` inside a repo, as trimmed non-empty lines.
///
/// Separate from [`config_values`] because that one asks about the ambient
/// configuration; these questions are about one repository, so they need a
/// working directory and must not see anything inherited.
fn local_config(dir: &Path, args: &[&str]) -> Vec<String> {
    let mut command = Command::new("git");
    command
        .args(["config", "--local"])
        .args(args)
        .current_dir(dir);

    let Ok(output) = command.output() else {
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
