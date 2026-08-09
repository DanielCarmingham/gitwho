//! Deciding which account owns a repository.
//!
//! Every answer carries a [`Reason`], because a resolution that cannot say
//! *why* it chose an account is exactly the silent-wrong-answer failure this
//! project exists to remove (R8).

use std::path::Path;

use crate::config::{Account, Config};
use crate::git;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{0} has no origin remote")]
    NoOrigin(std::path::PathBuf),
    #[error("defaults.account names {0}, which is not a declared account")]
    UnknownDefault(String),
    #[error("no account matches {0}")]
    NoMatch(String),
    #[error("{url} matches {} accounts equally well ({}); pin one with a more specific pattern", accounts.len(), accounts.join(", "))]
    Ambiguous { url: String, accounts: Vec<String> },
    #[error("invalid match pattern {pattern:?} on account {account}: {source}")]
    BadPattern {
        account: String,
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

/// How an account was arrived at. Callers use this to decide whether handing
/// over a secret is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Matched a remote URL against the account's patterns.
    UrlMatch,
    /// Matched the repo's `origin` remote against the account's patterns.
    OriginUrl,
    /// The repo has no remote; matched a directory prefix instead.
    PathFallback,
    /// The repo has a remote, but no account claims it -- a third-party clone,
    /// or an org nobody has declared yet.
    ///
    /// Distinct from [`Default`](Reason::Default) on purpose. Both land on the
    /// declared default account, but this one means "we looked and found
    /// nothing", which is also what a *forgotten* pattern looks like. Callers
    /// that are about to release a credential must treat it as low confidence.
    Unmatched,
    /// Nothing matched, so the explicitly declared default account was used.
    /// Callers must treat this as low confidence: it is the only outcome that
    /// can be right by luck rather than by evidence.
    Default,
}

#[derive(Debug)]
pub struct Resolved<'a> {
    pub account: &'a Account,
    pub reason: Reason,
}

impl<'a> Resolved<'a> {
    fn with_reason(self, reason: Reason) -> Self {
        Self { reason, ..self }
    }
}

impl Reason {
    /// Whether this answer is firm enough to release a credential on.
    ///
    /// `Default` and `Unmatched` both land on the declared default account
    /// without anything having identified it, so a token handed over on either
    /// basis is right only by luck (R8).
    pub fn identifies_an_account(self) -> bool {
        !matches!(self, Reason::Default | Reason::Unmatched)
    }
}

/// Reduce a remote URL to the `host/path` form that patterns are written
/// against, so one pattern covers every way a remote can be spelled.
///
/// Handles the three shapes git accepts:
///   `https://github.com/Org/repo.git`
///   `ssh://git@host:2222/Org/repo.git`
///   `git@host:Org/repo.git`          (scp-style: no scheme, `:` separates)
///
/// A scheme is what disambiguates a port from an scp separator, so the colon
/// is only rewritten when no scheme was present.
fn normalize(url: &str) -> String {
    let (had_scheme, rest) = match url.split_once("://") {
        Some((_, rest)) => (true, rest),
        None => (false, url),
    };

    // Drop any `user@` prefix -- the ssh login is not part of the identity.
    let rest = rest.split_once('@').map_or(rest, |(_, after)| after);

    let rest = if had_scheme {
        rest.to_string()
    } else {
        rest.replacen(':', "/", 1)
    };

    rest.trim_end_matches('/').to_string()
}

/// Resolve the account owning the repo that contains `dir`.
///
/// Only `origin` is consulted. A fork whose `upstream` belongs to another
/// account still authenticates correctly when fetching from it, because the
/// credential helper resolves per-URL at transport time -- so identity can
/// follow the remote you actually push to without ambiguity.
pub fn resolve_repo<'a>(config: &'a Config, dir: &Path) -> Result<Resolved<'a>, ResolveError> {
    if let Some(url) = git::origin_url(dir) {
        return match resolve_url(config, &url) {
            Ok(resolved) => Ok(Resolved {
                account: resolved.account,
                reason: Reason::OriginUrl,
            }),
            // Nobody claims this remote. Fall back, but say so: erroring would
            // break every third-party clone, while a silent fall-back would
            // make a forgotten pattern indistinguishable from a deliberate
            // one.
            Err(ResolveError::NoMatch(_)) => {
                Ok(default_account(config)?.with_reason(Reason::Unmatched))
            }
            // Ambiguity is different in kind: there *is* a right answer and we
            // cannot tell which, so falling back would be guessing.
            Err(other) => Err(other),
        };
    }

    if let Some(resolved) = resolve_path(config, dir) {
        return Ok(resolved);
    }

    Ok(default_account(config)?.with_reason(Reason::Default))
}

fn default_account(config: &Config) -> Result<Resolved<'_>, ResolveError> {
    let account = config
        .account(&config.defaults.account)
        .ok_or_else(|| ResolveError::UnknownDefault(config.defaults.account.clone()))?;

    Ok(Resolved {
        account,
        reason: Reason::Default,
    })
}

/// Longest-prefix match of `dir` against the accounts' `paths`.
///
/// Longest wins so a nested root (KitchenCloud inside Profound) beats the root
/// containing it, without depending on declaration order.
fn resolve_path<'a>(config: &'a Config, dir: &Path) -> Option<Resolved<'a>> {
    // Temp dirs and symlinked roots differ between the configured spelling and
    // the real path, so compare canonical forms.
    let target = dir.canonicalize().ok()?;
    let mut best: Option<(usize, &Account)> = None;

    for account in &config.accounts {
        for prefix in &account.paths {
            let Ok(prefix) = Path::new(prefix).canonicalize() else {
                continue;
            };
            if !target.starts_with(&prefix) {
                continue;
            }
            let depth = prefix.components().count();
            if best.is_none_or(|(best_depth, _)| depth > best_depth) {
                best = Some((depth, account));
            }
        }
    }

    best.map(|(_, account)| Resolved {
        account,
        reason: Reason::PathFallback,
    })
}

/// How specific a pattern is: the number of characters it pins down literally.
///
/// `github.com/Profound-Kitchen/**` pins 28 characters and so beats
/// `github.com/Profound-*/**`, which pins 21. Using specificity rather than
/// declaration order is what keeps nested orgs from being order-sensitive --
/// the ordering trap the `includeIf` rules have today.
fn specificity(pattern: &str) -> usize {
    pattern.chars().filter(|c| !matches!(c, '*' | '?')).count()
}

pub fn resolve_url<'a>(config: &'a Config, url: &str) -> Result<Resolved<'a>, ResolveError> {
    let candidate = normalize(url);
    let mut best_score: Option<usize> = None;
    let mut winners: Vec<&Account> = Vec::new();

    for account in &config.accounts {
        for pattern in &account.match_patterns {
            let glob = globset::Glob::new(pattern).map_err(|source| ResolveError::BadPattern {
                account: account.name.clone(),
                pattern: pattern.clone(),
                source,
            })?;
            if !glob.compile_matcher().is_match(&candidate) {
                continue;
            }

            let score = specificity(pattern);
            match best_score {
                Some(best) if score < best => {}
                Some(best) if score == best => {
                    // A single account may match on several of its own
                    // patterns; that is not ambiguity.
                    if !winners.iter().any(|w| w.name == account.name) {
                        winners.push(account);
                    }
                }
                _ => {
                    best_score = Some(score);
                    winners = vec![account];
                }
            }
        }
    }

    match winners.len() {
        0 => Err(ResolveError::NoMatch(candidate)),
        1 => Ok(Resolved {
            account: winners[0],
            reason: Reason::UrlMatch,
        }),
        _ => Err(ResolveError::Ambiguous {
            url: candidate,
            accounts: winners.iter().map(|a| a.name.clone()).collect(),
        }),
    }
}
