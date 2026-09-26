//! Parsing of `accounts.toml` -- the single place an account is declared.
//!
//! It names logins and patterns and never a secret, so it can be committed to
//! a dotfiles repo (R10).

use serde::Deserialize;

use crate::provider::Provider;
use crate::sources::TokenOwner;

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
    #[error("{place} uses `{field}`, which gitwho no longer reads: {instead}. See docs/accounts.toml.example")]
    Removed {
        place: String,
        field: &'static str,
        instead: &'static str,
    },
    #[error("account {account}: {problem}")]
    Invalid {
        account: String,
        problem: &'static str,
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
    /// The author name for every account that does not override it.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: Provider,
    /// The login this account's token is held under by its provider's CLI.
    /// Not `user`: beside `gitName` and `email` that would read as git's
    /// `user.name`, which it has nothing to do with.
    pub login: String,
    /// The server's https address. Required for gitea; github means github.com.
    #[serde(default)]
    pub url: Option<String>,
    pub email: String,
    /// Written into `core.sshcommand` for remotes that use ssh.
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    /// Author name, when this account differs from `defaults.gitName`.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
    /// Glob patterns matched against `host/path` of a remote URL.
    #[serde(rename = "match", default)]
    pub match_patterns: Vec<String>,
    /// Directory prefixes consulted **only** for a repo with no remote yet.
    #[serde(default)]
    pub paths: Vec<String>,
}

const REMOVED_FROM_ACCOUNTS: &[(&str, &str)] = &[
    (
        "env",
        "the provider now decides which variables are set; delete it and set `login` (and `url` for gitea)",
    ),
    (
        "gitCredential",
        "the git password is now the token from the account's provider CLI; delete it",
    ),
];

const REMOVED_FROM_DEFAULTS: &[(&str, &str)] = &[(
    "secretBackend",
    "gitwho stores no secrets any more; delete it",
)];

impl Account {
    /// Who to ask for this account's token.
    pub fn token_owner(&self) -> TokenOwner<'_> {
        TokenOwner {
            account: &self.name,
            provider: self.provider,
            login: &self.login,
            url: self.url.as_deref(),
        }
    }
}

impl Config {
    pub fn parse(toml_str: &str) -> Result<Self, ConfigError> {
        // Parsed twice on purpose: deny_unknown_fields would report a removed
        // field as merely unknown, without saying what replaced it.
        let table: toml::Table = toml::from_str(toml_str)?;
        reject_removed(&table)?;
        let config: Config = toml::from_str(toml_str)?;
        config.validate()?;
        Ok(config)
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

    fn validate(&self) -> Result<(), ConfigError> {
        for account in &self.accounts {
            if account.login.trim().is_empty() {
                return Err(ConfigError::Invalid {
                    account: account.name.clone(),
                    problem: "`login` must name the gh or tea login, and it is empty",
                });
            }
            let problem = match (account.provider, &account.url) {
                (Provider::Gitea, None) => {
                    "a gitea account needs `url`, the server's https address"
                }
                (Provider::Github, Some(_)) => "github means github.com, so `url` is not allowed",
                _ => continue,
            };
            return Err(ConfigError::Invalid {
                account: account.name.clone(),
                problem,
            });
        }
        Ok(())
    }
}

fn reject_removed(table: &toml::Table) -> Result<(), ConfigError> {
    if let Some(defaults) = table.get("defaults").and_then(toml::Value::as_table) {
        for (field, instead) in REMOVED_FROM_DEFAULTS {
            if defaults.contains_key(*field) {
                return Err(ConfigError::Removed {
                    place: "[defaults]".to_string(),
                    field,
                    instead,
                });
            }
        }
    }
    let accounts = table
        .get("accounts")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table);
    for account in accounts {
        for (field, instead) in REMOVED_FROM_ACCOUNTS {
            if account.contains_key(*field) {
                let name = account
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("?");
                return Err(ConfigError::Removed {
                    place: format!("account {name}"),
                    field,
                    instead,
                });
            }
        }
    }
    Ok(())
}
