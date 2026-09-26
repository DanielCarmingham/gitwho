//! Which variables each provider's tools read.
//!
//! Facts about `gh`, `tea` and their MCP servers, kept here rather than
//! restated per account: restating them is how `GITEA_HOST` came to be
//! documented for a tool that never reads it.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    #[serde(alias = "forgejo")]
    Gitea,
}

/// What a variable is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Token,
    Url,
}

/// Read by these tools but never set by gitwho, so cleared all the same.
pub const FALLBACKS: &[&str] = &[
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Github, Provider::Gitea];

    /// Every variable this provider's tools read, and what it holds.
    pub fn variables(self) -> &'static [(&'static str, Value)] {
        match self {
            Provider::Github => &[
                ("GH_TOKEN", Value::Token),
                ("GITHUB_PERSONAL_ACCESS_TOKEN", Value::Token),
            ],
            Provider::Gitea => &[
                ("GITEA_TOKEN", Value::Token),
                ("GITEA_INSTANCE_URL", Value::Url),
                ("GITEA_ACCESS_TOKEN", Value::Token),
                ("GITEA_HOST", Value::Url),
            ],
        }
    }

    /// The CLI that holds this provider's tokens.
    pub fn cli(self) -> &'static str {
        match self {
            Provider::Github => "gh",
            Provider::Gitea => "tea",
        }
    }

    /// The name as written in `accounts.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Github => "github",
            Provider::Gitea => "gitea",
        }
    }
}

/// Every variable `exec` clears before setting any, whatever the config says.
pub fn always_cleared() -> BTreeSet<&'static str> {
    Provider::ALL
        .iter()
        .flat_map(|provider| provider.variables().iter().map(|(name, _)| *name))
        .chain(FALLBACKS.iter().copied())
        .collect()
}
