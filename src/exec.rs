//! Running a command with exactly one account's credentials.
//!
//! CLIs and MCP servers cannot be reached by a credential helper -- they read
//! environment variables. Rather than exporting those into the shell, where
//! every unrelated process inherits them, `exec` injects them into the single
//! process that needs them.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::Account;
use crate::provider::{always_cleared, Value};
use crate::sources::{self, Runner, TokenError};

/// The environment changes to apply before running a command.
#[derive(Debug, Default)]
pub struct EnvPlan {
    /// Variables to set, with their values.
    pub set: BTreeMap<String, String>,
    /// Variables to unset before setting anything.
    pub remove: BTreeSet<String>,
}

/// For a command that must not be handed a credential: clear, set nothing.
/// Clearing still happens, so a token the shell exported cannot reach a tool
/// that would then authenticate as it (R11).
pub fn plan_cleared() -> EnvPlan {
    EnvPlan {
        remove: cleared(),
        set: BTreeMap::new(),
    }
}

/// The account's token and url under every name its provider's tools read.
pub fn plan_env(runner: &dyn Runner, account: &Account) -> Result<EnvPlan, TokenError> {
    let token = sources::token(runner, &account.token_owner())?;
    let url = account.url.clone().unwrap_or_default();

    let set = account
        .provider
        .variables()
        .iter()
        .map(|(name, value)| {
            let value = match value {
                Value::Token => token.clone(),
                Value::Url => url.clone(),
            };
            (name.to_string(), value)
        })
        .collect();

    Ok(EnvPlan {
        remove: cleared(),
        set,
    })
}

fn cleared() -> BTreeSet<String> {
    always_cleared().into_iter().map(String::from).collect()
}
