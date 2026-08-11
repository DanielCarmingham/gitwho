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
    /// The author name for every account that does not override it. Kept here
    /// because it is the same person throughout; only the address differs.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
    /// Which secret store holds the values on this machine: `age` or
    /// `keychain`. Unset means the built-in default.
    ///
    /// Kept as a plain string. Which names are legal is
    /// `secrets::select`'s business -- a parser that knew them would reject a
    /// typo with a message about TOML rather than about backends, and would
    /// have to be edited to add one.
    #[serde(rename = "secretBackend", default)]
    pub secret_backend: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: String,
    pub email: String,
    /// Which declared variable holds the token git should authenticate with.
    ///
    /// Named explicitly rather than inferred from the provider: an account may
    /// hold several credentials, and guessing which one is the git password is
    /// the kind of implicit behaviour that goes wrong quietly.
    ///
    /// Absent when the account never uses https, which is how R7 is honoured:
    /// a key-authenticated account is not made to invent a token. Transport is
    /// deliberately NOT declared per account -- git chooses it per remote, and
    /// a credential helper is only ever consulted for https, so an account
    /// using both (as Digilope does, over two hostnames) needs no special
    /// case.
    #[serde(rename = "gitCredential", default)]
    pub git_credential: Option<String>,
    /// The ssh key for remotes that use it, written into `core.sshcommand`.
    ///
    /// Independent of `gitCredential`: an account may have both, either, or
    /// neither.
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    /// Author name, when this account differs from `defaults.gitName`.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
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

impl Account {
    /// The variables this account needs a stored value for.
    ///
    /// Literal `VAR=value` entries are excluded -- they carry their own value
    /// and are not secrets. The git credential variable is included even if it
    /// is not repeated in `env`, since it still needs a value to exist.
    /// Deduplicated, because declaring it in both places is natural.
    pub fn secret_vars(&self) -> Vec<&str> {
        let mut vars: Vec<&str> = Vec::new();

        for spec in &self.env {
            if spec.contains('=') {
                continue;
            }
            if !vars.contains(&spec.as_str()) {
                vars.push(spec);
            }
        }

        if let Some(var) = &self.git_credential {
            if !vars.contains(&var.as_str()) {
                vars.push(var);
            }
        }

        vars
    }
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
