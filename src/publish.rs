//! The decisions behind `gitwho publish`: which account creates the
//! repository, whether the commits may be pushed as it, what to ask the
//! server, and which of its URLs becomes `origin`.
//!
//! Pure functions, so every one of those decisions is testable without a
//! server, a CLI or a repository. `main` runs the steps.

use std::path::Path;

use serde::Deserialize;

use crate::config::{Account, Config};
use crate::resolve::{self, Reason};

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("no account named {0:?}; `gitwho accounts` lists them")]
    UnknownAccount(String),
    #[error(
        "no account claims this directory through `paths`, and publishing as the default \
         would be a guess; name the account with --account <name> (`gitwho accounts` lists them)"
    )]
    NoAccount,
    #[error(transparent)]
    Resolve(#[from] resolve::ResolveError),
    #[error("the server's answer is not a repository: {0}")]
    BadAnswer(String),
}

/// How `origin` reaches the new repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Ssh,
    Https,
}

/// The URLs the server reports for a repository it has just created.
///
/// Taken from the answer rather than assembled, because a server's ssh host
/// is often not its web host and only the server knows both.
#[derive(Debug, Deserialize)]
pub struct Created {
    pub clone_url: String,
    pub ssh_url: String,
    pub html_url: String,
}

impl Created {
    pub fn remote(&self, transport: Transport) -> &str {
        match transport {
            Transport::Ssh => &self.ssh_url,
            Transport::Https => &self.clone_url,
        }
    }
}

/// The account that publishes: the one named, or the one whose `paths`
/// claims `dir`. Never the default -- creating a repository as whichever
/// account nothing else matched is right only by luck (R8).
pub fn choose_account<'a>(
    config: &'a Config,
    named: Option<&str>,
    dir: &Path,
) -> Result<&'a Account, PublishError> {
    if let Some(name) = named {
        return config
            .account(name)
            .ok_or_else(|| PublishError::UnknownAccount(name.to_string()));
    }
    let resolved = resolve::resolve_repo(config, dir)?;
    if resolved.reason == Reason::PathFallback {
        Ok(resolved.account)
    } else {
        Err(PublishError::NoAccount)
    }
}

/// Author emails that are not `email`, each once. Pushing them would publish
/// someone else's authorship under this account.
pub fn foreign_authors(email: &str, authors: &[String]) -> Vec<String> {
    let mut foreign: Vec<String> = authors
        .iter()
        .filter(|author| !author.eq_ignore_ascii_case(email))
        .cloned()
        .collect();
    foreign.sort();
    foreign.dedup();
    foreign
}

/// Shell commands that rewrite every commit by a `foreign` author to the
/// account's identity. git-filter-repo maps author and committer alike, and
/// needs `--force` because a local repository is not a fresh clone.
pub fn author_fix(git_name: Option<&str>, email: &str, foreign: &[String]) -> String {
    let proper = match git_name {
        Some(name) => format!("{name} <{email}>"),
        None => format!("<{email}>"),
    };
    let lines: Vec<String> = foreign
        .iter()
        .map(|old| crate::shim::shell_quote(&format!("{proper} <{old}>")))
        .collect();
    format!(
        "printf '%s\\n' {} > .git/gitwho.mailmap\n\
         git filter-repo --force --mailmap .git/gitwho.mailmap",
        lines.join(" ")
    )
}

/// Arguments to `gh`/`tea` that create the repository. Both CLIs' `api`
/// commands take the same flags, and both servers the same endpoint.
pub fn create_args(name: &str, owner: Option<&str>, public: bool) -> Vec<String> {
    let endpoint = match owner {
        Some(org) => format!("orgs/{org}/repos"),
        None => "user/repos".to_string(),
    };
    vec![
        "api".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        endpoint,
        "-f".to_string(),
        format!("name={name}"),
        "-F".to_string(),
        format!("private={}", !public),
    ]
}

pub fn parse_created(body: &str) -> Result<Created, PublishError> {
    serde_json::from_str(body).map_err(|e| PublishError::BadAnswer(e.to_string()))
}

/// ssh for an account that declares a key, so a new repository matches how
/// that account's other clones already work; https otherwise.
pub fn transport(account: &Account, requested: Option<Transport>) -> Transport {
    requested.unwrap_or(if account.ssh_key.is_some() {
        Transport::Ssh
    } else {
        Transport::Https
    })
}

/// The account whose identity rule wins once `publishing` joins the accounts
/// already claiming `existing` remotes, or `None` if only one account is
/// involved. The later declaration in `accounts.toml` wins, because `sync`
/// writes the rules in declaration order and git's last include wins.
pub fn deciding_account<'a>(
    config: &'a Config,
    existing: &[(String, String)],
    publishing: &Account,
) -> Option<&'a Account> {
    let claims = resolve::claimants(config, existing);
    let involved = |a: &Account| {
        a.name == publishing.name || claims.claimed.iter().any(|(_, c)| c.name == a.name)
    };
    let count = config.accounts.iter().filter(|a| involved(a)).count();
    (count > 1)
        .then(|| config.accounts.iter().rev().find(|a| involved(a)))
        .flatten()
}
