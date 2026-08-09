//! Deciding which account owns a repository.
//!
//! Every answer carries a [`Reason`], because a resolution that cannot say
//! *why* it chose an account is exactly the silent-wrong-answer failure this
//! project exists to remove (R8).

use crate::config::{Account, Config};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
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
}

#[derive(Debug)]
pub struct Resolved<'a> {
    pub account: &'a Account,
    pub reason: Reason,
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
