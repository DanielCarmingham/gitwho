//! Reading the repositories already on disk to propose an `accounts.toml`.
//!
//! `init` scaffolds a template and stops at *"edit this file"*. That is the last
//! step where someone can be **silently** wrong: a missing `match` pattern does
//! not error, it resolves to the default account -- the exact failure this
//! project exists to prevent, left as homework. The information needed to write
//! that file correctly is already on the disk, so this reads it.
//!
//! Writes nothing, ever. Same posture as `sync` and `mcp sync`: the output goes
//! to stdout to be reviewed, edited and piped, so using it extends no trust.
//!
//! **The limit that shapes everything here: this finds organisations, not
//! accounts.** Nothing on disk says `github.com/acme-corp` and
//! `github.com/acme-labs` belong to the same person while `github.com/you` does
//! not. That mapping is human knowledge, so the output states it needs finishing
//! rather than guessing and producing something that looks authoritative and is
//! subtly wrong.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::resolve;

/// How far to walk, and how much to walk it.
#[derive(Debug, Clone)]
pub struct Limits {
    /// How many directories below a root to descend before giving up.
    ///
    /// A source tree can be very large and a backup mount larger still. This is
    /// a one-shot command a person waits on, so it is bounded rather than
    /// exhaustive, and what it skipped is reported.
    pub max_depth: usize,
    /// Directory names never descended into, whatever their depth.
    pub skip: Vec<String>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // Deep enough for ~/src/<host>/<org>/<repo>/<worktree> and a couple
            // of levels of personal filing above it.
            max_depth: 6,
            skip: [
                "node_modules",
                "target",
                "vendor",
                ".venv",
                "venv",
                "Library",
                ".Trash",
                ".cache",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

/// Asking a repository what its `origin` is.
///
/// A trait so the walk can be tested without creating real repositories, and so
/// the shape of a tree can be exercised independently of git being installed --
/// the same reasoning as `sources::Runner` and `paths::Layout`.
pub trait Remotes {
    /// The `origin` URL for the repository at `dir`, or `None` when it has no
    /// `origin`.
    fn origin_url(&self, dir: &Path) -> Option<String>;
}

/// Asks the real git binary.
pub struct GitRemotes;

impl Remotes for GitRemotes {
    fn origin_url(&self, dir: &Path) -> Option<String> {
        crate::git::origin_url(dir)
    }
}

/// One `host/org` pair and the repositories that reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Org {
    pub host: String,
    pub org: String,
    /// Distinct repository names seen under this org, sorted.
    pub repos: Vec<String>,
}

impl Org {
    /// The `match` pattern this org implies.
    pub fn pattern(&self) -> String {
        format!("{}/{}/**", self.host, self.org)
    }
}

/// What a scan found. Every category is reported, including the empty ones --
/// "found nothing" and "found nothing new" are different facts.
#[derive(Debug, Default)]
pub struct Scan {
    /// Organisations, sorted by host then org.
    pub orgs: Vec<Org>,
    /// Repositories with no `origin` at all. These are the only case `paths`
    /// exists for, so they are surfaced rather than dropped.
    pub no_remote: Vec<PathBuf>,
    /// Remotes whose URL could not be reduced to a host and an org, with the
    /// URL, so the user knows the scan was not silently partial.
    pub unparsed: Vec<(PathBuf, String)>,
    /// Roots that did not exist, so a typo does not read as an empty machine.
    pub missing_roots: Vec<PathBuf>,
    /// Directories not descended into because `max_depth` was reached.
    pub truncated_at: Vec<PathBuf>,
}

impl Scan {
    /// Whether the scan found anything at all worth writing a config from.
    pub fn is_empty(&self) -> bool {
        self.orgs.is_empty() && self.no_remote.is_empty()
    }

    /// Total repositories accounted for.
    pub fn repo_count(&self) -> usize {
        self.orgs.iter().map(|o| o.repos.len()).sum::<usize>()
            + self.no_remote.len()
            + self.unparsed.len()
    }
}

/// Walk `roots` and report what is there.
pub fn scan(roots: &[PathBuf], limits: &Limits, remotes: &dyn Remotes) -> Scan {
    let mut found: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut scan = Scan::default();
    // Keyed by resolved URL, because many worktrees of one repository are one
    // entry and not twenty.
    let mut seen_urls: BTreeMap<String, ()> = BTreeMap::new();

    for root in roots {
        if !root.is_dir() {
            scan.missing_roots.push(root.clone());
            continue;
        }
        walk(
            root,
            0,
            limits,
            remotes,
            &mut found,
            &mut seen_urls,
            &mut scan,
        );
    }

    scan.orgs = found
        .into_iter()
        .map(|((host, org), mut repos)| {
            repos.sort();
            repos.dedup();
            Org { host, org, repos }
        })
        .collect();

    scan.no_remote.sort();
    scan.unparsed.sort();
    scan
}

#[allow(clippy::too_many_arguments)]
fn walk(
    dir: &Path,
    depth: usize,
    limits: &Limits,
    remotes: &dyn Remotes,
    found: &mut BTreeMap<(String, String), Vec<String>>,
    seen_urls: &mut BTreeMap<String, ()>,
    scan: &mut Scan,
) {
    if is_repo(dir) {
        record(dir, remotes, found, seen_urls, scan);
        // Do not descend into a repository. Its `.git` holds nothing a scan
        // wants, and a repo containing another repo is a submodule, which
        // belongs to its parent rather than to this list.
        return;
    }

    if depth >= limits.max_depth {
        scan.truncated_at.push(dir.to_path_buf());
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        // Unreadable directories are common and uninteresting -- a permissions
        // error on one folder is not worth failing a whole scan over.
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // `file_type` does not follow symlinks, which is the point: following
        // one can leave the roots entirely, and a link back up a tree turns the
        // walk into a loop.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || limits.skip.iter().any(|s| *s == name) {
            continue;
        }

        walk(&path, depth + 1, limits, remotes, found, seen_urls, scan);
    }
}

/// A repository is anything with a `.git`, file **or** directory.
///
/// A linked worktree has a `.git` *file* pointing elsewhere. Checking only for a
/// directory makes every worktree invisible to the scan -- and worktrees are one
/// of the cases that motivated this whole project.
fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn record(
    dir: &Path,
    remotes: &dyn Remotes,
    found: &mut BTreeMap<(String, String), Vec<String>>,
    seen_urls: &mut BTreeMap<String, ()>,
    scan: &mut Scan,
) {
    let Some(url) = remotes.origin_url(dir) else {
        scan.no_remote.push(dir.to_path_buf());
        return;
    };

    // `resolve::normalize` rather than a second spelling of URL parsing: it
    // already knows every form git might write, including scp-style with a
    // non-`git` user. A discovery pass that only understood `https://` would
    // silently miss every ssh remote.
    let normalized = resolve::normalize(&url);

    let Some((host, org, repo)) = split_host_org_repo(&normalized) else {
        scan.unparsed.push((dir.to_path_buf(), url));
        return;
    };

    // Many worktrees of one repository are one entry.
    if seen_urls.insert(normalized, ()).is_some() {
        return;
    }

    found.entry((host, org)).or_default().push(repo);
}

/// `host[:port]/org/repo[.git]` into its three parts.
///
/// Returns `None` for anything that does not carry at least a host, an owner
/// and a name -- a bare `host/repo` has no organisation to key an account on,
/// and guessing one would invent the very grouping this refuses to guess.
fn split_host_org_repo(normalized: &str) -> Option<(String, String, String)> {
    let mut parts = normalized.split('/');

    let host = parts.next()?;
    // A port is part of reaching the host, not part of naming it, and `match`
    // patterns are written without one.
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    if host.is_empty() {
        return None;
    }

    let org = parts.next()?;
    let repo = parts.next()?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }

    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if repo.is_empty() {
        return None;
    }

    Some((host.to_string(), org.to_string(), repo.to_string()))
}

// --- proposing a config ------------------------------------------------------

/// Accounts a tool is already logged in as, offered as hints.
///
/// Deliberately not a mapping. gh knows which accounts exist; it does not know
/// which of them owns `acme-corp`, and neither does anything on disk. Guessing
/// would be the same mistake as guessing which orgs are one person, so these are
/// listed for a human to choose from.
#[derive(Debug, Default, Clone)]
pub struct Logins {
    /// Host to the account names available on it.
    pub by_host: BTreeMap<String, Vec<String>>,
    /// Host to the account names gh holds but could not authenticate.
    ///
    /// Kept apart from `by_host` rather than dropped: for `init` these are
    /// accounts not worth suggesting, but for a renewal they are the whole
    /// point -- "your token died" and "gh never knew this account" call for
    /// completely different next steps.
    pub failed_by_host: BTreeMap<String, Vec<String>>,
}

/// Read `gh auth status` for the accounts it holds.
///
/// Never reads a token, only the account list -- this is `init` output that a
/// user pipes into a file, so it must not be able to carry a secret.
pub fn gh_logins(runner: &dyn crate::sources::Runner) -> Logins {
    let mut logins = Logins::default();

    // gh writes this to stdout on success; a machine with no gh, or no logins,
    // simply yields nothing to suggest.
    let Ok(Some(captured)) = runner.run("gh", &["auth", "status"]) else {
        return logins;
    };

    let mut host: Option<String> = None;
    for line in captured.stdout.lines().chain(captured.stderr.lines()) {
        let trimmed = line.trim();

        // A bare host line introduces the block that follows it.
        if !line.starts_with(char::is_whitespace) && trimmed.contains('.') && !trimmed.contains(' ')
        {
            host = Some(trimmed.to_string());
            continue;
        }

        // gh lists both. Suggesting an account it has just reported as broken
        // would hand someone a config that cannot work -- observed on the
        // author's machine, where one of three logins has an invalid token.
        let failed = trimmed.contains("Failed to log in to");
        if !failed && !trimmed.contains("Logged in to") {
            continue;
        }

        if let (Some(host), Some(rest)) = (host.as_ref(), trimmed.split_once(" account ")) {
            // "✓ Logged in to github.com account NAME (keyring)"
            if let Some(name) = rest.1.split_whitespace().next() {
                let names = if failed {
                    logins.failed_by_host.entry(host.clone()).or_default()
                } else {
                    logins.by_host.entry(host.clone()).or_default()
                };
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }

    logins
}

/// The `provider` value a host implies.
///
/// A guess, and a harmless one: the field is required by the parser and read by
/// nothing, so a wrong value changes no behaviour. Emitted anyway because a
/// config that will not parse is no use to anyone.
fn provider_for(host: &str) -> &'static str {
    let h = host.to_ascii_lowercase();
    if h.contains("github") {
        "github"
    } else if h.contains("gitlab") {
        "gitlab"
    } else if h.contains("azure") || h.contains("visualstudio") {
        "azure"
    } else {
        // Gitea and Forgejo are the common self-hosted case and `tea` speaks to
        // both. A self-hosted GitLab lands here wrongly and costs nothing.
        "gitea"
    }
}

/// The credential variable a provider's CLI reads.
fn credential_var(provider: &str) -> &'static str {
    match provider {
        "github" => "GH_TOKEN",
        "gitlab" => "GITLAB_TOKEN",
        "azure" => "AZURE_DEVOPS_EXT_PAT",
        _ => "GITEA_TOKEN",
    }
}

/// How many repositories an org needs before it is proposed as an account.
///
/// Measured on the author's machine, which is the only place this could be
/// measured: 9 organisations discovered, one with 42 repositories and eight with
/// exactly one each -- and all eight were clones of other people's work
/// (`microsoft`, `charmbracelet`, a handful of individuals' dotfiles). Emitting
/// an account block for each would have made 8 of 9 blocks noise, which is how a
/// useful proposal becomes one nobody reads.
///
/// A heuristic, so it is applied by *demoting* rather than dropping: the
/// single-repo orgs are still listed, with the repository that produced them, so
/// promoting one is a copy-paste and never a re-scan. An account genuinely
/// starting life with one repository lands here, which the output says outright.
const LIKELY_OWN_MIN_REPOS: usize = 2;

/// Render a scan as a proposed `accounts.toml`.
///
/// Every uncertainty is left visible. Emails are never invented, the default
/// account is not chosen, and orgs are not merged -- because a config that looks
/// finished and is subtly wrong is worse than one that obviously needs an editor
/// passed over it.
pub fn render(scan: &Scan, roots: &[PathBuf], logins: &Logins) -> String {
    let mut out = String::new();

    out.push_str("# Proposed by `gitwho init --discover`. Nothing has been written.\n#\n");

    for root in roots {
        out.push_str(&format!("# Scanned: {}\n", root.display()));
    }
    for root in &scan.missing_roots {
        out.push_str(&format!(
            "# NOT SCANNED, no such directory: {}\n",
            root.display()
        ));
    }

    if scan.is_empty() {
        out.push_str("#\n# No repositories found. Either the roots are wrong, or this machine\n");
        out.push_str("# has none yet -- which is different from having nothing to add.\n");
        return out;
    }

    let (likely_own, single): (Vec<&Org>, Vec<&Org>) = scan
        .orgs
        .iter()
        .partition(|o| o.repos.len() >= LIKELY_OWN_MIN_REPOS);

    out.push_str(&format!(
        "#\n# Found {} repositories in {} organisation(s); {} proposed as accounts.\n",
        scan.repo_count(),
        scan.orgs.len(),
        likely_own.len()
    ));

    out.push_str(
        "#\n\
         # READ THIS BEFORE USING IT. This found *organisations*, not *accounts*.\n\
         # Nothing on disk says which orgs are the same person. Each block below is\n\
         # one org. Where several belong to one account, merge them by hand: keep\n\
         # one block and move the others' `match` patterns into it.\n\
         #\n\
         # Then fill in every REPLACE-ME. Emails are never guessed.\n\n",
    );

    out.push_str("[defaults]\n");
    out.push_str(
        "# Where a repository that matches no account lands. Point this at whichever\n\
         # block below is genuinely your default -- it is not a guess worth making.\n",
    );
    out.push_str("account = \"REPLACE-ME\"\n");
    out.push_str("gitName = \"REPLACE-ME\"\n\n");

    for org in &likely_own {
        let provider = provider_for(&org.host);
        let var = credential_var(provider);

        out.push_str(&format!(
            "# --- {}/{} {}\n",
            org.host,
            org.org,
            "-".repeat(58usize.saturating_sub(org.host.len() + org.org.len()))
        ));
        out.push_str(&format!(
            "# {} repositories: {}\n",
            org.repos.len(),
            summarise(&org.repos)
        ));

        out.push_str("[[accounts]]\n");
        out.push_str(&format!("name = \"{}\"\n", org.org));
        out.push_str(&format!("provider = \"{provider}\"\n"));
        out.push_str("email = \"REPLACE-ME\"\n");
        out.push_str(&format!("gitCredential = \"{var}\"\n"));
        out.push_str(&format!("match = [\"{}\"]\n", org.pattern()));

        match logins.by_host.get(&org.host) {
            Some(names) if !names.is_empty() => {
                out.push_str(&format!(
                    "# gh is logged in on {} as: {}.\n\
                     # Naming one here reads its token on demand, so there is nothing to\n\
                     # store and nothing to keep in sync. Which one owns this org is the\n\
                     # one thing discovery cannot know, so pick it yourself:\n\
                     #   env = [{{ var = \"{var}\", from = \"gh\", user = \"{}\" }}]\n",
                    org.host,
                    names.join(", "),
                    names[0]
                ));
            }
            _ => {}
        }
        out.push_str(&format!("env = [\"{var}\"]\n\n"));
    }

    if !single.is_empty() {
        out.push_str(
            "# --- one repository each ------------------------------------------\n\
             #\n\
             # NOT proposed as accounts, on the assumption that an org you have a\n\
             # single clone of is somebody else's work rather than an account of\n\
             # yours. That is a guess, so nothing was thrown away: if one of these is\n\
             # yours, copy the pattern into the matching block above, or paste the\n\
             # commented block into place.\n",
        );
        for org in &single {
            out.push_str(&format!(
                "#\n#   {}/{}  ({})\n#     match = [\"{}\"]\n",
                org.host,
                org.org,
                org.repos.join(", "),
                org.pattern()
            ));
        }
        out.push('\n');
    }

    if !scan.no_remote.is_empty() {
        out.push_str(
            "# --- repositories with no remote ----------------------------------\n\
             #\n\
             # `paths` is consulted ONLY for a repo that has no remote yet, which is\n\
             # exactly these. Anything with a remote is settled by `match` above, so\n\
             # adding a prefix here does not constrain where you keep your work.\n\
             # Add these to whichever account above should own a fresh `git init`:\n",
        );
        for path in &scan.no_remote {
            out.push_str(&format!("#   {}\n", path.display()));
        }
        out.push('\n');
    }

    if !scan.unparsed.is_empty() {
        out.push_str(
            "# --- remotes this could not read ----------------------------------\n\
             #\n\
             # Listed so the scan is not silently partial. Each of these has an origin\n\
             # that did not reduce to a host and an organisation:\n",
        );
        for (path, url) in &scan.unparsed {
            out.push_str(&format!("#   {}  ->  {url}\n", path.display()));
        }
        out.push('\n');
    }

    if !scan.truncated_at.is_empty() {
        out.push_str(&format!(
            "# --- not fully scanned --------------------------------------------\n\
             #\n\
             # {} director(ies) were not descended into because the depth limit was\n\
             # reached. Re-run with a root further down if you expect repos below them.\n",
            scan.truncated_at.len()
        ));
        for path in scan.truncated_at.iter().take(10) {
            out.push_str(&format!("#   {}\n", path.display()));
        }
        out.push('\n');
    }

    out
}

/// A few repository names, then a count. The list is context, not an inventory.
fn summarise(repos: &[String]) -> String {
    const SHOWN: usize = 6;
    if repos.len() <= SHOWN {
        return repos.join(", ");
    }
    format!(
        "{}, and {} more",
        repos[..SHOWN].join(", "),
        repos.len() - SHOWN
    )
}
