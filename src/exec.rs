//! Running a command with exactly one account's credentials.
//!
//! CLIs and MCP servers cannot be reached by a credential helper -- they read
//! environment variables. Rather than exporting those variables into the shell,
//! where every unrelated process inherits them, `exec` injects them into the
//! single process that needs them.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Account, Config};
use crate::secrets::Backend;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("account {account} needs {var}, which has no stored value")]
    MissingSecret { account: String, var: String },
    #[error("secret store failed: {0}")]
    Store(#[from] crate::secrets::SecretError),
}

/// The environment changes to apply before running a command.
#[derive(Debug, Default)]
pub struct EnvPlan {
    /// Variables to set, with their values.
    pub set: BTreeMap<String, String>,
    /// Variables to unset before setting anything.
    pub remove: BTreeSet<String>,
}

/// Work out what one account's environment should look like.
///
/// Returns a plan rather than mutating anything, so the decision can be tested
/// without spawning a process and inspected by `doctor` without running one.
pub fn plan_env(
    config: &Config,
    backend: &dyn Backend,
    account: &Account,
) -> Result<EnvPlan, ExecError> {
    // Everything any account manages gets cleared first. Deriving this from
    // the chosen account would be the wrong way round: an account knows what
    // it needs, not what it must be protected from. A Gitea account never
    // mentions GH_TOKEN, which is precisely why it would otherwise survive.
    let mut plan = EnvPlan {
        remove: managed_variables(config),
        set: BTreeMap::new(),
    };

    for spec in &account.env {
        // `VAR=value` is a literal, for non-secret settings such as an API
        // host. A bare `VAR` names a secret to fetch.
        if let Some((var, value)) = spec.split_once('=') {
            plan.set.insert(var.to_string(), value.to_string());
            continue;
        }

        let value = backend
            .get(&account.name, spec)?
            .ok_or_else(|| ExecError::MissingSecret {
                account: account.name.clone(),
                var: spec.clone(),
            })?;
        plan.set.insert(spec.clone(), value);
    }

    Ok(plan)
}

/// Every variable name any account declares, plus every account's git
/// credential variable.
///
/// This is the set `exec` clears before populating. It is deliberately drawn
/// from the whole config so that adding an account automatically protects
/// every other account from it.
fn managed_variables(config: &Config) -> BTreeSet<String> {
    let mut names = BTreeSet::new();

    for account in &config.accounts {
        for spec in &account.env {
            let name = spec.split_once('=').map_or(spec.as_str(), |(name, _)| name);
            names.insert(name.to_string());
        }
        if let Some(var) = &account.git_credential {
            names.insert(var.clone());
        }
    }

    names
}
