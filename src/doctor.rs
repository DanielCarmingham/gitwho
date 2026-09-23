//! Reporting on whether the wiring is coherent (R12).
//!
//! Strictly read-only. `doctor` inspects and describes; `sync` is what
//! changes things. Keeping them apart means this can be pointed at a live
//! machine before committing to anything.
//!
//! Nothing here ever prints a token value -- only fingerprints (R10).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::secrets::{fingerprint, Backend, BackendKind, Choice};
use crate::sources::{self, Runner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Worth knowing, but not blocking.
    Warn,
    /// Something is or will be wrong.
    Problem,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub check: String,
    pub message: String,
}

impl Finding {
    fn new(level: Level, check: &str, message: String) -> Self {
        Self {
            level,
            check: check.to_string(),
            message,
        }
    }
}

/// The parts of git's configuration that decide whether gitwho is reachable.
#[derive(Debug, Default)]
pub struct GitWiring {
    /// The `credential.helper` values in effect globally, resets already
    /// applied.
    pub credential_helpers: Vec<String>,
    /// The helper git resolves for `https://github.com`.
    ///
    /// Checked separately because a `[credential "https://github.com"]`
    /// section overrides the general list entirely -- so a correct global
    /// helper can still be bypassed for the host that matters most here.
    pub github_helper: Option<String>,
    /// `credential.useHttpPath` for github.com. `None` means unset.
    pub use_http_path: Option<bool>,
    /// Every remote of the repository `doctor` was run from, as
    /// `(name, url)`. Empty when the cwd is not a repository.
    ///
    /// Identity rules are generated as `includeIf
    /// "hasconfig:remote.*.url:"`, which matches when *any* remote matches --
    /// so a repo whose remotes belong to two accounts applies both rules and
    /// git's last-include-wins settles it. That is a property of the repo, not
    /// of the config, which is why the remotes have to come in here.
    pub remotes: Vec<(String, String)>,
    /// Whether the repository's own config already settles the identity -- a
    /// local `user.email` or `include.path`.
    ///
    /// Local config beats every included global rule, so once this is true the
    /// include order decides nothing and there is nothing left to report.
    pub identity_pinned: bool,
}

/// Where the config and the secrets live, and who they should belong to.
///
/// Paths rather than already-collected modes, because extracting a mode and a
/// uid from a `stat` is exactly the part that can be wrong -- keeping it inside
/// the module means the tests exercise it.
#[derive(Debug)]
pub struct Store {
    /// Expected `0700`.
    pub dir: PathBuf,
    /// `accounts.toml`, expected `0600`.
    pub config: PathBuf,
    /// `identity.key`, expected `0600`.
    pub identity: PathBuf,
    /// `secrets.age`, expected `0600`.
    pub secrets: PathBuf,
    /// The uid everything here should belong to. Passed in rather than read
    /// from the process, because chowning a file to another user needs root --
    /// varying the expectation is the only way to test this without one.
    pub owner: u32,
    /// Which store actually holds the values, and what decided that. Both
    /// halves matter: `accounts.toml` saying one thing while the shell says
    /// another is precisely when someone runs `doctor`.
    pub backend: Choice,
    /// Whether gitwho could apply owner-only permissions at all.
    ///
    /// False on Windows, where it writes no ACLs and the files inherit the
    /// directory's. Carried as a fact to report rather than closed with code
    /// nobody here can run -- see `secrets::Protection`.
    pub owner_only_enforced: bool,
}

/// The uid whose files this process can be expected to own.
#[cfg(unix)]
pub fn current_uid() -> u32 {
    // std has no geteuid. The call takes no arguments, touches no memory and
    // cannot fail, so there is nothing for the caller to get wrong.
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
pub fn current_uid() -> u32 {
    0
}

/// Inspect everything and return what was found. Never writes.
pub fn run(
    config: &Config,
    backend: &dyn Backend,
    runner: &dyn Runner,
    ambient_env: &BTreeMap<String, String>,
    git: &GitWiring,
    store: &Store,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // First: if accounts.toml is writable by someone else, nothing the later
    // checks report about its contents can be trusted.
    check_permissions(store, &mut findings);
    check_storage(store, &mut findings);
    check_config(config, &mut findings);
    check_secrets(config, backend, runner, &mut findings);
    check_ambient(config, ambient_env, &mut findings);
    check_git_wiring(git, &mut findings);
    check_repo_identity(config, git, &mut findings);

    findings
}

/// True when anything found would stop the setup working.
pub fn has_problems(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.level == Level::Problem)
}

/// The store's own permissions. Every other check here is about what the files
/// say; this one is about whether anything they say can be trusted.
///
/// Still read-only -- a `stat` per path, no writes and no network, so it
/// belongs in the default offline run.
#[cfg(unix)]
fn check_permissions(store: &Store, findings: &mut Vec<Finding>) {
    use std::os::unix::fs::MetadataExt;

    // What each path is worth to whoever can reach it. Carrying the stake
    // alongside the mode is what makes the finding actionable rather than a
    // number to be silenced.
    let expected = [
        (
            store.dir.as_path(),
            0o700,
            "everything below it is only out of reach because it is",
        ),
        (
            store.config.as_path(),
            0o600,
            "whoever can write it can add a match pattern for their own host and be handed a token",
        ),
        (
            store.identity.as_path(),
            0o600,
            "anything able to read it can decrypt the secrets file",
        ),
        (
            store.secrets.as_path(),
            0o600,
            "it holds every stored token",
        ),
    ];

    let mut clean = true;

    for (path, want, stake) in expected {
        // Symlinks are followed on purpose: the target is what gitwho
        // actually reads. One pointing into a world-writable directory is a
        // residual gap this does not close.
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            // Nothing there yet. A fresh install has no secrets.age until the
            // first `secret set`, and a missing identity is already reported
            // loudly by the secrets check -- saying it twice would make a
            // clean install look broken.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                clean = false;
                findings.push(Finding::new(
                    Level::Problem,
                    "permissions",
                    format!("{} cannot be inspected: {e}", path.display()),
                ));
                continue;
            }
        };

        // Exact equality, in both directions: drift is what a regression looks
        // like, and deciding which drift is benign is how a check becomes
        // clever and wrong. setgid and sticky are masked off rather than
        // judged.
        let mode = meta.mode() & 0o777;
        if mode != want {
            clean = false;
            findings.push(Finding::new(
                Level::Problem,
                "permissions",
                // With the remedy, because nothing in gitwho applies one:
                // `doctor` reports and `sync` writes gitconfig, and
                // `accounts.toml` arrives by hand from the example. A finding
                // naming only the stake is one an operator learns to read past.
                format!(
                    "{} is {mode:04o}, not {want:04o}: {stake}; fix with chmod {want:o} {}",
                    path.display(),
                    path.display()
                ),
            ));
        }

        if meta.uid() != store.owner {
            clean = false;
            findings.push(Finding::new(
                Level::Problem,
                "permissions",
                format!(
                    "{} is owned by uid {}, not {}: {stake}",
                    path.display(),
                    meta.uid(),
                    store.owner
                ),
            ));
        }
    }

    // Said out loud when it passes, the way stored secrets are, so the check
    // is visible rather than only noticeable when it fails.
    if clean {
        findings.push(Finding::new(
            Level::Ok,
            "permissions",
            format!("{} and its files are owner-only", store.dir.display()),
        ));
    }
}

#[cfg(not(unix))]
fn check_permissions(_store: &Store, _findings: &mut Vec<Finding>) {}

/// Which store is in effect, and whether it is as closed down as intended.
///
/// Stated on every run rather than only when something is wrong: the store is
/// overridable per invocation, so "which one am I talking to right now" is not
/// answerable from the config file alone.
fn check_storage(store: &Store, findings: &mut Vec<Finding>) {
    let where_it_lives = match store.backend.kind {
        BackendKind::AgeFile => format!("the age file at {}", store.secrets.display()),
        BackendKind::Keychain => "the platform keychain".to_string(),
    };
    findings.push(Finding::new(
        Level::Ok,
        "backend",
        format!(
            "secrets are in {where_it_lives}, chosen {}",
            store.backend.source.describe()
        ),
    ));

    // Only the file-backed store has permissions to lose; warning about a file
    // the keychain never writes would point at the wrong thing entirely.
    if store.backend.kind == BackendKind::AgeFile && !store.owner_only_enforced {
        findings.push(Finding::new(
            Level::Warn,
            "backend",
            format!(
                "{} cannot be made owner-only on this platform; it inherits whatever {} grants",
                store.secrets.display(),
                store.dir.display()
            ),
        ));
    }
}

fn check_config(config: &Config, findings: &mut Vec<Finding>) {
    if config.account(&config.defaults.account).is_none() {
        findings.push(Finding::new(
            Level::Problem,
            "config",
            format!(
                "defaults.account names {}, which is not a declared account",
                config.defaults.account
            ),
        ));
    }

    // A pattern claimed by two accounts makes resolution refuse at run time.
    // Saying so here turns a confusing failure during a push into a startup
    // check.
    let mut claims: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for account in &config.accounts {
        for pattern in &account.match_patterns {
            claims.entry(pattern).or_default().push(&account.name);
        }
    }
    for (pattern, owners) in claims {
        if owners.len() > 1 {
            findings.push(Finding::new(
                Level::Problem,
                "config",
                format!(
                    "pattern {pattern} is claimed by {}; resolution would refuse",
                    owners.join(" and ")
                ),
            ));
        }
    }

    // An identity with no author name generates `name = `, which makes every
    // commit in that repo fail. Cheap to check, confusing to diagnose later.
    for account in &config.accounts {
        let has_name = account
            .git_name
            .as_deref()
            .or(config.defaults.git_name.as_deref())
            .is_some_and(|n| !n.trim().is_empty());
        if !has_name {
            findings.push(Finding::new(
                Level::Problem,
                "config",
                format!(
                    "account {} has no author name; set defaults.gitName or the account's gitName",
                    account.name
                ),
            ));
        }
    }

    // tea builds a login from the environment only when both are set. With one
    // it falls back to the login in its own config -- a working tea, quite
    // possibly as another account, which is R8's failure exactly.
    for account in &config.accounts {
        let declares = |var: &str| account.env.iter().any(|spec| spec.name() == var);
        let missing = match (declares("GITEA_TOKEN"), declares("GITEA_INSTANCE_URL")) {
            (true, false) => "GITEA_INSTANCE_URL",
            (false, true) => "GITEA_TOKEN",
            _ => continue,
        };
        findings.push(Finding::new(
            Level::Problem,
            "config",
            format!(
                "account {} declares one of tea's GITEA_TOKEN and GITEA_INSTANCE_URL but not {missing}; \
                 tea needs both, and otherwise silently uses the login in its own config",
                account.name
            ),
        ));
    }

    for account in &config.accounts {
        for pattern in &account.match_patterns {
            if let Err(e) = globset::Glob::new(pattern) {
                findings.push(Finding::new(
                    Level::Problem,
                    "config",
                    format!(
                        "account {} has an invalid pattern {pattern}: {e}",
                        account.name
                    ),
                ));
            }
        }
    }
}

fn check_secrets(
    config: &Config,
    backend: &dyn Backend,
    runner: &dyn Runner,
    findings: &mut Vec<Finding>,
) {
    for account in &config.accounts {
        for var in account.secret_vars() {
            match backend.get(&account.name, var) {
                Ok(Some(value)) => findings.push(Finding::new(
                    Level::Ok,
                    "secrets",
                    format!("{}/{var} {}", account.name, fingerprint(&value)),
                )),
                Ok(None) => findings.push(Finding::new(
                    Level::Problem,
                    "secrets",
                    format!("{}/{var} has no stored value", account.name),
                )),
                Err(e) => findings.push(Finding::new(
                    Level::Problem,
                    "secrets",
                    format!("{}/{var} could not be read: {e}", account.name),
                )),
            }
        }

        // Referenced variables are not in `secret_vars` -- nothing is owed to
        // the store for them -- so without this they would vanish from the
        // report entirely, and a credential silently missing from `doctor` is
        // the opposite of what `doctor` is for.
        //
        // This is the one check that runs another program. It is why `doctor`
        // is worth running after `gh auth logout` and not only after editing
        // `accounts.toml`: the declaration can be perfect while the tool it
        // points at has nothing.
        for sourced in account.sourced_vars() {
            let var = &sourced.var;
            match sources::fetch(runner, &account.name, sourced) {
                Ok(value) => findings.push(Finding::new(
                    Level::Ok,
                    "secrets",
                    format!(
                        "{}/{var} {} (from {})",
                        account.name,
                        fingerprint(&value),
                        sourced.from
                    ),
                )),
                // The source's own message, which already names the account and
                // the variable and says what the tool said.
                Err(e) => findings.push(Finding::new(Level::Problem, "secrets", e.to_string())),
            }
        }
    }
}

/// A managed variable sitting in the environment is the condition this project
/// exists to remove: every process launched from that shell inherits it,
/// including ones belonging to a different account.
fn check_ambient(config: &Config, env: &BTreeMap<String, String>, findings: &mut Vec<Finding>) {
    // By variable, not by account. Every GitHub account declares GH_TOKEN, but
    // there is only one of it in the environment -- reporting per account
    // turns one fact into a wall of identical lines.
    let mut seen = std::collections::BTreeSet::new();

    for account in &config.accounts {
        // Referenced variables included: where the value comes from changes
        // nothing about the hazard, which is that this shell already exports
        // one and every process launched from it inherits that one.
        let sourced = account.sourced_vars();
        let watched = account
            .secret_vars()
            .into_iter()
            .chain(sourced.iter().map(|s| s.var.as_str()));

        for var in watched {
            if env.contains_key(var) && seen.insert(var.to_string()) {
                findings.push(Finding::new(
                    Level::Warn,
                    "ambient",
                    // The name only. Printing the value would leak the very
                    // thing being complained about.
                    format!("{var} is set in the environment; every process launched from this shell inherits it"),
                ));
            }
        }
    }
}

fn check_git_wiring(git: &GitWiring, findings: &mut Vec<Finding>) {
    // What github.com resolves to is the question that decides whether
    // gitwho is reached at all, because a URL-scoped section wins outright.
    match &git.github_helper {
        Some(helper) if helper.contains("gitwho") => {}
        Some(helper) => findings.push(Finding::new(
            Level::Problem,
            "git",
            format!("github.com is served by {helper}, not gitwho"),
        )),
        None => {
            let wired = git
                .credential_helpers
                .iter()
                .any(|helper| helper.contains("gitwho"));
            if !wired {
                findings.push(Finding::new(
                    Level::Problem,
                    "git",
                    format!(
                        "credential.helper does not mention gitwho (in effect: {})",
                        if git.credential_helpers.is_empty() {
                            "nothing".to_string()
                        } else {
                            git.credential_helpers.join(", ")
                        }
                    ),
                ));
            }
        }
    }

    match git.use_http_path {
        Some(true) => {}
        // Without the path, the helper is asked only about `github.com`, so
        // every GitHub account resolves identically. The failure would be
        // total and silent, which is why it is a problem rather than a warning.
        _ => findings.push(Finding::new(
            Level::Problem,
            "git",
            "credential.useHttpPath is not true for github.com, so the org never reaches the helper and all GitHub accounts resolve identically".to_string(),
        )),
    }
}

/// Whether the repo `doctor` ran in has remotes claimed by more than one
/// account, and if so which one git will actually pick.
///
/// The credentials axis handles this case correctly on its own -- the helper is
/// asked per URL at transport time, so each remote authenticates as its own
/// account. Identity does not: `includeIf "hasconfig:remote.*.url:"` matches
/// when *any* remote matches, so every claiming account's rule applies and
/// git's last-include-wins picks the one declared last. Measured on git 2.54.0
/// (Apple Git-157); `hasconfig:remote.origin.url:` is not a supported keyword
/// and silently never matches, so this cannot be fixed in the generated rules.
///
/// A warning rather than a problem: there is no single right answer for a repo
/// that genuinely spans two accounts, and only the person who set it up knows
/// which one should sign the commits. What is wrong is being told nothing (R8).
fn check_repo_identity(config: &Config, git: &GitWiring, findings: &mut Vec<Finding>) {
    // A local `user.email` or `include.path` beats every included global rule,
    // so the question is already settled and there is nothing to report.
    if git.identity_pinned {
        return;
    }

    let claimants = crate::resolve::claimants(config, &git.remotes);
    let Some(winner) = claimants.identity_winner else {
        return;
    };

    let mut accounts: Vec<&str> = Vec::new();
    for (_, account) in &claimants.claimed {
        if !accounts.contains(&account.name.as_str()) {
            accounts.push(&account.name);
        }
    }

    let pairs: Vec<String> = claimants
        .claimed
        .iter()
        .map(|(remote, account)| format!("{remote} -> {}", account.name))
        .collect();

    findings.push(Finding::new(
        Level::Warn,
        "identity",
        format!(
            "this repo's remotes belong to {} accounts ({}); every matching account's \
             identity rule applies and {} decides because it is declared last in \
             accounts.toml -- not because it is origin. Pin the one you want with \
             `git config --local include.path <gitwho's git dir>/{}.gitconfig`. \
             Credentials are unaffected: each remote authenticates as its own account.",
            accounts.len(),
            pairs.join(", "),
            winner.name,
            winner.name,
        ),
    ));
}
