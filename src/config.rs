//! Parsing of `accounts.toml` -- the single place an account is declared.
//!
//! This file is tracked in the `cfg` repo, so it names variables and never
//! holds their values (R10).

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("accounts.toml is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("cannot read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
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
    /// Which declared variable holds the token git should authenticate with.
    ///
    /// Named explicitly rather than inferred from the provider: an account
    /// may hold several credentials, and guessing which one is the git
    /// password is exactly the kind of implicit behaviour that goes wrong
    /// quietly. Absent for `gitAuth = "ssh"` accounts, which authenticate with
    /// a key and need no token in the transport path at all (R7).
    #[serde(rename = "gitCredential", default)]
    pub git_credential: Option<String>,
    /// Glob patterns matched against `host/path` of a remote URL.
    #[serde(rename = "match", default)]
    pub match_patterns: Vec<String>,
    /// Variables this account's CLIs and MCP servers need. A bare `VAR` names
    /// a secret to fetch; `VAR=value` is a literal, for non-secret settings
    /// such as an API host.
    #[serde(default)]
    pub env: Vec<String>,
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

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name == name)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }
}
