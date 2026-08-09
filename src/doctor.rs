//! Reporting on whether the wiring is coherent (R12).
//!
//! Strictly read-only. `doctor` inspects and describes; `sync` is what
//! changes things. Keeping them apart means this can be pointed at a live
//! machine before committing to anything.
//!
//! Nothing here ever prints a token value -- only fingerprints (R10).

use std::collections::BTreeMap;

use crate::config::Config;
use crate::secrets::{fingerprint, Backend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Worth knowing, but not blocking.
    Warn,
    /// Something is or will be wrong.
    Problem,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub check: String,
    pub message: String,
}

impl Finding {
    fn new(level: Level, check: &str, message: String) -> Self {
        Self {
            level,
            check: check.to_string(),
            message,
        }
    }
}

/// The parts of git's configuration that decide whether gitfriend is reachable.
#[derive(Debug, Default)]
pub struct GitWiring {
    /// The `credential.helper` values in effect globally, resets already
    /// applied.
    pub credential_helpers: Vec<String>,
    /// The helper git resolves for `https://github.com`.
    ///
    /// Checked separately because a `[credential "https://github.com"]`
    /// section overrides the general list entirely -- so a correct global
    /// helper can still be bypassed for the host that matters most here.
    pub github_helper: Option<String>,
    /// `credential.useHttpPath` for github.com. `None` means unset.
    pub use_http_path: Option<bool>,
}

/// Inspect everything and return what was found. Never writes.
pub fn run(
    config: &Config,
    backend: &dyn Backend,
    ambient_env: &BTreeMap<String, String>,
    git: &GitWiring,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    check_config(config, &mut findings);
    check_secrets(config, backend, &mut findings);
    check_ambient(config, ambient_env, &mut findings);
    check_git_wiring(git, &mut findings);

    findings
}

/// True when anything found would stop the setup working.
pub fn has_problems(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.level == Level::Problem)
}

fn check_config(config: &Config, findings: &mut Vec<Finding>) {
    if config.account(&config.defaults.account).is_none() {
        findings.push(Finding::new(
            Level::Problem,
            "config",
            format!(
                "defaults.account names {}, which is not a declared account",
                config.defaults.account
            ),
        ));
    }

    // A pattern claimed by two accounts makes resolution refuse at run time.
    // Saying so here turns a confusing failure during a push into a startup
    // check.
    let mut claims: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for account in &config.accounts {
        for pattern in &account.match_patterns {
            claims.entry(pattern).or_default().push(&account.name);
        }
    }
    for (pattern, owners) in claims {
        if owners.len() > 1 {
            findings.push(Finding::new(
                Level::Problem,
                "config",
                format!(
                    "pattern {pattern} is claimed by {}; resolution would refuse",
                    owners.join(" and ")
                ),
            ));
        }
    }

    for account in &config.accounts {
        for pattern in &account.match_patterns {
            if let Err(e) = globset::Glob::new(pattern) {
                findings.push(Finding::new(
                    Level::Problem,
                    "config",
                    format!("account {} has an invalid pattern {pattern}: {e}", account.name),
                ));
            }
        }
    }
}

fn check_secrets(config: &Config, backend: &dyn Backend, findings: &mut Vec<Finding>) {
    for account in &config.accounts {
        for var in account.secret_vars() {
            match backend.get(&account.name, var) {
                Ok(Some(value)) => findings.push(Finding::new(
                    Level::Ok,
                    "secrets",
                    format!("{}/{var} {}", account.name, fingerprint(&value)),
                )),
                Ok(None) => findings.push(Finding::new(
                    Level::Problem,
                    "secrets",
                    format!("{}/{var} has no stored value", account.name),
                )),
                Err(e) => findings.push(Finding::new(
                    Level::Problem,
                    "secrets",
                    format!("{}/{var} could not be read: {e}", account.name),
                )),
            }
        }
    }
}

/// A managed variable sitting in the environment is the condition this project
/// exists to remove: every process launched from that shell inherits it,
/// including ones belonging to a different account.
fn check_ambient(config: &Config, env: &BTreeMap<String, String>, findings: &mut Vec<Finding>) {
    // By variable, not by account. Every GitHub account declares GH_TOKEN, but
    // there is only one of it in the environment -- reporting per account
    // turns one fact into a wall of identical lines.
    let mut seen = std::collections::BTreeSet::new();

    for account in &config.accounts {
        for var in account.secret_vars() {
            if env.contains_key(var) && seen.insert(var.to_string()) {
                findings.push(Finding::new(
                    Level::Warn,
                    "ambient",
                    // The name only. Printing the value would leak the very
                    // thing being complained about.
                    format!("{var} is set in the environment; every process launched from this shell inherits it"),
                ));
            }
        }
    }
}

fn check_git_wiring(git: &GitWiring, findings: &mut Vec<Finding>) {
    // What github.com resolves to is the question that decides whether
    // gitfriend is reached at all, because a URL-scoped section wins outright.
    match &git.github_helper {
        Some(helper) if helper.contains("gitfriend") => {}
        Some(helper) => findings.push(Finding::new(
            Level::Problem,
            "git",
            format!("github.com is served by {helper}, not gitfriend"),
        )),
        None => {
            let wired = git
                .credential_helpers
                .iter()
                .any(|helper| helper.contains("gitfriend"));
            if !wired {
                findings.push(Finding::new(
                    Level::Problem,
                    "git",
                    format!(
                        "credential.helper does not mention gitfriend (in effect: {})",
                        if git.credential_helpers.is_empty() {
                            "nothing".to_string()
                        } else {
                            git.credential_helpers.join(", ")
                        }
                    ),
                ));
            }
        }
    }

    match git.use_http_path {
        Some(true) => {}
        // Without the path, the helper is asked only about `github.com`, so
        // every GitHub account resolves identically. The failure would be
        // total and silent, which is why it is a problem rather than a warning.
        _ => findings.push(Finding::new(
            Level::Problem,
            "git",
            "credential.useHttpPath is not true for github.com, so the org never reaches the helper and all GitHub accounts resolve identically".to_string(),
        )),
    }
}
