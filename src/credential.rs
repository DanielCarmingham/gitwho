//! The git credential helper.
//!
//! git invokes this on every https transport operation, handing over the URL
//! it is about to contact. That is the whole trick: the request itself carries
//! the org (given `credential.useHttpPath=true`), so the account resolves from
//! what git is *doing* rather than from where the working tree happens to sit
//! -- including during `clone`, when no repo exists yet (R1, R2).

use std::path::Path;

use crate::config::{Account, Config};
use crate::resolve;
use crate::secrets::Backend;

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("cannot tell which account owns this request: {0}")]
    Unresolved(#[from] resolve::ResolveError),
    #[error("account {account} declares no gitCredential variable")]
    NoCredentialVariable { account: String },
    #[error(
        "nothing identified an account for this request; it would only fall back to {account}"
    )]
    LowConfidence { account: String },
    #[error("account {account} needs {var}, which has no stored value")]
    MissingSecret { account: String, var: String },
    #[error("secret store failed: {0}")]
    Store(#[from] crate::secrets::SecretError),
}

/// One request from git, as key/value lines.
#[derive(Debug, Default)]
pub struct Request {
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
}

impl Request {
    pub fn parse(input: &str) -> Self {
        let mut request = Request::default();

        for line in input.lines() {
            // A blank line terminates the request; anything after it is not
            // ours to read.
            if line.is_empty() {
                break;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "protocol" => request.protocol = Some(value.to_string()),
                "host" => request.host = Some(value.to_string()),
                "path" => request.path = Some(value.to_string()),
                // Unknown keys are ignored rather than rejected: git adds new
                // ones over time, and a helper that fails on them breaks on
                // upgrade.
                _ => {}
            }
        }

        request
    }

    /// The `host/path` form the resolver matches against, or `None` when git
    /// told us too little to identify anything.
    fn target(&self) -> Option<String> {
        let host = self.host.as_ref()?;
        match &self.path {
            Some(path) => Some(format!("{host}/{path}")),
            None => Some(host.clone()),
        }
    }
}

#[derive(Debug)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

/// Answer a credential request, or fail loudly.
///
/// `cwd` is consulted only when the request itself is not specific enough to
/// identify an account.
pub fn respond(
    config: &Config,
    backend: &dyn Backend,
    request: &Request,
    cwd: Option<&Path>,
) -> Result<Credential, CredentialError> {
    let account = choose_account(config, request, cwd)?;

    let var = account
        .git_credential
        .as_deref()
        .ok_or_else(|| CredentialError::NoCredentialVariable {
            account: account.name.clone(),
        })?;

    let password = backend.get(&account.name, var)?.ok_or_else(|| {
        // Loudly, and without falling back to any other account's token: a
        // working-but-wrong credential is the failure this project exists to
        // remove (R8).
        CredentialError::MissingSecret {
            account: account.name.clone(),
            var: var.to_string(),
        }
    })?;

    Ok(Credential {
        username: account.name.clone(),
        password,
    })
}

fn choose_account<'a>(
    config: &'a Config,
    request: &Request,
    cwd: Option<&Path>,
) -> Result<&'a Account, CredentialError> {
    if let Some(target) = request.target() {
        return Ok(resolve::resolve_url(config, &target)?.account);
    }

    let cwd = cwd.ok_or(CredentialError::Unresolved(
        resolve::ResolveError::NoMatch("request carried no host".to_string()),
    ))?;

    let resolved = resolve::resolve_repo(config, cwd)?;

    // The declared default is a reasonable answer for *identity* -- committing
    // as your usual self in a scratch repo is harmless. It is not a reasonable
    // basis for releasing a *credential*: nothing here identified the account,
    // so a token handed over now is right only by luck (R8).
    if !resolved.reason.identifies_an_account() {
        return Err(CredentialError::LowConfidence {
            account: resolved.account.name.clone(),
        });
    }

    Ok(resolved.account)
}
