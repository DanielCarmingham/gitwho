//! Parsing of `accounts.toml` -- the single place an account is declared.
//!
//! This file is tracked in the `cfg` repo, so it names variables and never
//! holds their values (R10).

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("accounts.toml is not valid: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub defaults: Defaults,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// The account used when nothing else matches. Declared explicitly so the
    /// fallback is stated rather than emergent (R4).
    pub account: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: String,
    pub email: String,
    #[serde(rename = "gitAuth")]
    pub git_auth: String,
    /// Glob patterns matched against `host/path` of a remote URL.
    #[serde(rename = "match", default)]
    pub match_patterns: Vec<String>,
    /// Directory prefixes claimed by this account, consulted **only** for a
    /// repo that has no remote yet. Everything else resolves by URL, so a
    /// relocated clone is unaffected by these.
    #[serde(default)]
    pub paths: Vec<String>,
}

impl Config {
    pub fn parse(toml_str: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(toml_str)?)
    }
}
