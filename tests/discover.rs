//! Reading the repositories on disk to propose an `accounts.toml`.
//!
//! The walk is exercised against real directory trees; only the *remote* is
//! faked, because a `.git` file versus a `.git` directory is exactly the
//! distinction these tests exist to pin and creating one of each is cheap.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gitwho::discover::{gh_logins, render, scan, Limits, Logins, Org, Remotes, Scan};
use gitwho::sources::{Captured, MapRunner};

/// Answers from a fixed map of directory to origin URL. A directory with no
/// entry is a repository with no remote, which is a case in its own right.
struct FakeRemotes {
    urls: HashMap<PathBuf, String>,
}

impl Remotes for FakeRemotes {
    fn origin_url(&self, dir: &Path) -> Option<String> {
        self.urls.get(dir).cloned()
    }
}

/// Build a tree. `repo` entries get a `.git` directory, `worktree` entries a
/// `.git` *file*, matching what git actually writes for a linked worktree.
struct Tree {
    root: tempfile::TempDir,
    urls: HashMap<PathBuf, String>,
}

impl Tree {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            urls: HashMap::new(),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn repo(mut self, rel: &str, url: Option<&str>) -> Self {
        let dir = self.path(rel);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        if let Some(url) = url {
            self.urls.insert(dir, url.to_string());
        }
        self
    }

    /// A linked worktree: `.git` is a file, not a directory.
    fn worktree(mut self, rel: &str, url: &str) -> Self {
        let dir = self.path(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".git"), "gitdir: /elsewhere/.git/worktrees/wt\n").unwrap();
        self.urls.insert(dir, url.to_string());
        self
    }

    fn plain_dir(self, rel: &str) -> Self {
        std::fs::create_dir_all(self.path(rel)).unwrap();
        self
    }

    fn scan(&self) -> gitwho::discover::Scan {
        self.scan_with(&Limits::default())
    }

    fn scan_with(&self, limits: &Limits) -> gitwho::discover::Scan {
        let remotes = FakeRemotes {
            urls: self.urls.clone(),
        };
        scan(&[self.root.path().to_path_buf()], limits, &remotes)
    }
}

#[test]
fn repositories_group_by_host_and_organisation() {
    let tree = Tree::new()
        .repo("src/api", Some("https://github.com/acme-corp/api.git"))
        .repo("src/web", Some("https://github.com/acme-corp/web.git"))
        .repo("src/mine", Some("https://github.com/someone/mine.git"));

    let found = tree.scan();

    assert_eq!(found.orgs.len(), 2, "{:#?}", found.orgs);

    let acme = found
        .orgs
        .iter()
        .find(|o| o.org == "acme-corp")
        .expect("acme-corp");
    assert_eq!(acme.host, "github.com");
    assert_eq!(acme.repos, vec!["api", "web"]);
    assert_eq!(acme.pattern(), "github.com/acme-corp/**");
}

/// A linked worktree has a `.git` file. Checking only for a directory makes
/// every worktree invisible -- and worktrees are one of the cases that motivated
/// this project.
#[test]
fn a_worktree_whose_git_is_a_file_is_still_a_repository() {
    let tree = Tree::new().worktree("wt-3", "https://github.com/acme-corp/api.git");

    let found = tree.scan();

    assert_eq!(found.orgs.len(), 1, "{:#?}", found);
    assert_eq!(found.orgs[0].repos, vec!["api"]);
}

/// Many worktrees of one repository are one entry, not twenty.
#[test]
fn worktrees_of_one_repository_collapse_to_one_entry() {
    let tree = Tree::new()
        .repo("src/api", Some("https://github.com/acme-corp/api.git"))
        .worktree("wt/a", "https://github.com/acme-corp/api.git")
        .worktree("wt/b", "git@github.com:acme-corp/api.git");

    let found = tree.scan();

    assert_eq!(found.orgs.len(), 1);
    assert_eq!(
        found.orgs[0].repos,
        vec!["api"],
        "the scp-style URL is the same repository and must not become a second entry"
    );
}

/// Reusing `resolve::normalize` rather than a second spelling of URL parsing is
/// the difference between seeing ssh remotes and silently missing all of them.
#[test]
fn every_url_form_git_writes_is_understood() {
    let tree = Tree::new()
        .repo("a", Some("https://github.com/acme-corp/one.git"))
        .repo("b", Some("git@github.com:acme-corp/two.git"))
        .repo("c", Some("ssh://git@github.com/acme-corp/three.git"))
        .repo("d", Some("gitea@git.example.net:acme-corp/four.git"))
        .repo("e", Some("ssh://git@github.com:2222/acme-corp/five.git"));

    let found = tree.scan();

    let github = found
        .orgs
        .iter()
        .find(|o| o.host == "github.com")
        .expect("github.com");
    assert_eq!(github.repos, vec!["five", "one", "three", "two"]);

    // The non-`git` ssh user is the case a naive parser gets wrong.
    let gitea = found
        .orgs
        .iter()
        .find(|o| o.host == "git.example.net")
        .expect("the gitea@ remote must be seen");
    assert_eq!(gitea.repos, vec!["four"]);
}

/// A port is part of reaching a host, not naming it, and `match` patterns carry
/// no port.
#[test]
fn a_port_does_not_become_part_of_the_host() {
    let tree = Tree::new().repo("a", Some("ssh://git@git.example.net:2222/acme/one.git"));

    let found = tree.scan();

    assert_eq!(found.orgs[0].host, "git.example.net");
    assert_eq!(found.orgs[0].pattern(), "git.example.net/acme/**");
}

/// `paths` exists for exactly these, so dropping them silently would remove the
/// only evidence that the field is needed.
#[test]
fn a_repository_with_no_remote_is_reported_not_dropped() {
    let tree = Tree::new()
        .repo("scratch", None)
        .repo("src/api", Some("https://github.com/acme-corp/api.git"));

    let found = tree.scan();

    assert_eq!(found.no_remote.len(), 1);
    assert!(found.no_remote[0].ends_with("scratch"));
    assert_eq!(found.orgs.len(), 1);
}

/// The user has to be able to tell a partial scan from a complete one.
#[test]
fn a_remote_that_cannot_be_read_is_reported_with_its_url() {
    let tree = Tree::new().repo("odd", Some("https://github.com/just-a-host"));

    let found = tree.scan();

    assert!(found.orgs.is_empty(), "{:#?}", found.orgs);
    assert_eq!(found.unparsed.len(), 1);
    assert_eq!(found.unparsed[0].1, "https://github.com/just-a-host");
}

#[test]
fn a_root_that_does_not_exist_is_reported_rather_than_read_as_empty() {
    let missing = PathBuf::from("/nonexistent-root-for-a-test");

    let found = scan(
        std::slice::from_ref(&missing),
        &Limits::default(),
        &FakeRemotes {
            urls: HashMap::new(),
        },
    );

    assert_eq!(found.missing_roots, vec![missing]);
    assert!(found.is_empty());
}

/// A repository inside a repository is a submodule, and belongs to its parent.
#[test]
fn the_walk_does_not_descend_into_a_repository() {
    let tree = Tree::new()
        .repo("outer", Some("https://github.com/acme-corp/outer.git"))
        .repo(
            "outer/vendor/inner",
            Some("https://github.com/other/inner.git"),
        );

    let found = tree.scan();

    assert_eq!(found.orgs.len(), 1, "{:#?}", found.orgs);
    assert_eq!(found.orgs[0].org, "acme-corp");
}

#[test]
fn the_depth_bound_stops_the_walk_and_says_where() {
    let tree = Tree::new().repo("a/b/c/d/e/f/deep", Some("https://github.com/acme/deep.git"));

    let shallow = Limits {
        max_depth: 2,
        ..Default::default()
    };
    let found = tree.scan_with(&shallow);

    assert!(found.orgs.is_empty());
    assert!(
        !found.truncated_at.is_empty(),
        "a bounded scan must say it was bounded"
    );
}

#[test]
fn skipped_directories_are_not_walked() {
    let tree = Tree::new()
        .plain_dir("node_modules")
        .repo("node_modules/pkg", Some("https://github.com/npm/pkg.git"))
        .repo("src/api", Some("https://github.com/acme-corp/api.git"));

    let found = tree.scan();

    assert_eq!(found.orgs.len(), 1, "{:#?}", found.orgs);
    assert_eq!(found.orgs[0].org, "acme-corp");
}

// --- the proposed config -----------------------------------------------------

fn rendered(tree: &Tree, logins: &Logins) -> String {
    let found = tree.scan();
    render(&found, &[tree.root.path().to_path_buf()], logins, &[])
}

#[test]
fn the_proposal_parses_once_the_placeholders_are_filled_in() {
    let tree = Tree::new()
        .repo("src/api", Some("https://github.com/acme-corp/api.git"))
        .repo("src/web", Some("https://github.com/acme-corp/web.git"))
        .repo("src/mine", Some("git@git.example.net:someone/mine.git"))
        .repo("src/other", Some("git@git.example.net:someone/other.git"));

    let text = rendered(&tree, &Logins::default());

    // Every uncertainty is a marker rather than a guess.
    assert!(text.contains("REPLACE-ME"), "{text}");

    // Filling them in is all it should take to get a valid config.
    let filled = text
        .replace("account = \"REPLACE-ME\"", "account = \"acme-corp\"")
        .replace("gitName = \"REPLACE-ME\"", "gitName = \"A Person\"")
        .replace("email = \"REPLACE-ME\"", "email = \"someone@example.com\"");

    let config = gitwho::config::Config::parse(&filled)
        .unwrap_or_else(|e| panic!("proposal did not parse: {e}\n---\n{filled}"));

    assert_eq!(config.accounts.len(), 2);
    let acme = config.account("acme-corp").expect("acme-corp");
    assert_eq!(acme.match_patterns, vec!["github.com/acme-corp/**"]);
}

/// The output must not read as finished work. Discovery finds organisations; it
/// cannot know which are the same person, and a config that looks authoritative
/// while being subtly wrong is worse than one that visibly needs an editor.
#[test]
fn the_proposal_says_it_needs_merging_by_hand() {
    let tree = Tree::new()
        .repo("a", Some("https://github.com/acme-corp/one.git"))
        .repo("a2", Some("https://github.com/acme-corp/one-b.git"))
        .repo("b", Some("https://github.com/acme-labs/two.git"))
        .repo("b2", Some("https://github.com/acme-labs/two-b.git"));

    let text = rendered(&tree, &Logins::default());

    assert!(text.contains("organisations"), "{text}");
    assert!(text.to_lowercase().contains("merge"), "{text}");
    // Two orgs stay two blocks; it must not have decided they are one person.
    assert_eq!(text.matches("[[accounts]]").count(), 2, "{text}");
}

#[test]
fn an_email_is_never_invented() {
    let tree = Tree::new()
        .repo("a", Some("https://github.com/acme-corp/one.git"))
        .repo("b", Some("https://github.com/acme-corp/two.git"));

    let text = rendered(&tree, &Logins::default());

    assert!(text.contains("email = \"REPLACE-ME\""), "{text}");
    assert!(!text.contains("@acme-corp"), "{text}");
}

#[test]
fn finding_nothing_says_so_rather_than_emitting_an_empty_config() {
    let tree = Tree::new().plain_dir("empty");

    let text = rendered(&tree, &Logins::default());

    assert!(!text.contains("[[accounts]]"), "{text}");
    assert!(text.contains("No repositories found"), "{text}");
}

#[test]
fn repositories_with_no_remote_become_a_paths_suggestion() {
    let tree = Tree::new()
        .repo("scratch", None)
        .repo("src/api", Some("https://github.com/acme-corp/api.git"));

    let text = rendered(&tree, &Logins::default());

    assert!(text.contains("`paths`"), "{text}");
    assert!(text.contains("scratch"), "{text}");
}

// --- hints from gh -----------------------------------------------------------

#[test]
fn gh_logins_are_read_per_host() {
    let status = "\
github.com
  ✓ Logged in to github.com account octocat (keyring)
  - Active account: true
  ✓ Logged in to github.com account octocat-work (keyring)
  - Active account: false
git.example.net
  ✓ Logged in to git.example.net account someone (keyring)
";

    let runner = MapRunner::new(HashMap::from([(
        MapRunner::key("gh", &["auth", "status"]),
        Captured {
            success: true,
            stdout: status.to_string(),
            stderr: String::new(),
        },
    )]));

    let logins = gh_logins(&runner);

    assert_eq!(
        logins.by_host.get("github.com").map(Vec::as_slice),
        Some(["octocat".to_string(), "octocat-work".to_string()].as_slice())
    );
    assert_eq!(
        logins.by_host.get("git.example.net").map(Vec::as_slice),
        Some(["someone".to_string()].as_slice())
    );
}

/// gh knows which accounts exist. It does not know which one owns `acme-corp`,
/// and neither does anything on disk -- so the accounts are offered, not
/// assigned.
#[test]
fn gh_accounts_are_offered_as_a_choice_not_assigned() {
    let tree = Tree::new()
        .repo("a", Some("https://github.com/acme-corp/one.git"))
        .repo("b", Some("https://github.com/acme-corp/two.git"));

    let mut logins = Logins::default();
    logins.by_host.insert(
        "github.com".to_string(),
        vec!["octocat".to_string(), "octocat-work".to_string()],
    );

    let text = rendered(&tree, &logins);

    assert!(text.contains("octocat, octocat-work"), "{text}");
    assert!(text.contains("gh holds:"), "{text}");
    // Which gh login owns this org is the user's call, so the account still
    // needs its `login` filled in rather than one being assigned.
    assert!(text.contains("login = \"REPLACE-ME\""), "{text}");
}

#[test]
fn no_gh_on_the_machine_simply_offers_nothing() {
    let runner = MapRunner::new(HashMap::new()).without("gh");

    let logins = gh_logins(&runner);

    assert!(logins.by_host.is_empty());
}

/// An org you have a single clone of is usually somebody else's work. Measured
/// on the author's machine: 9 orgs discovered, 8 with exactly one repository and
/// every one of those eight a clone of an upstream project. Proposing an account
/// for each would have made 8 of 9 blocks noise.
#[test]
fn an_org_with_a_single_repository_is_not_proposed_as_an_account() {
    let tree = Tree::new()
        .repo("mine/a", Some("https://github.com/acme-corp/a.git"))
        .repo("mine/b", Some("https://github.com/acme-corp/b.git"))
        .repo(
            "clones/glow",
            Some("https://github.com/charmbracelet/glow.git"),
        );

    let text = rendered(&tree, &Logins::default());

    assert_eq!(
        text.matches("[[accounts]]").count(),
        1,
        "only the multi-repo org should be proposed:\n{text}"
    );
    assert!(text.contains("name = \"acme-corp\""), "{text}");
    assert!(!text.contains("name = \"charmbracelet\""), "{text}");
}

/// Demoted, never dropped. The guess is only safe because reversing it is a
/// copy-paste rather than another scan.
#[test]
fn a_demoted_org_is_still_listed_with_its_pattern() {
    let tree = Tree::new()
        .repo("mine/a", Some("https://github.com/acme-corp/a.git"))
        .repo("mine/b", Some("https://github.com/acme-corp/b.git"))
        .repo(
            "clones/glow",
            Some("https://github.com/charmbracelet/glow.git"),
        );

    let text = rendered(&tree, &Logins::default());

    assert!(text.contains("charmbracelet"), "{text}");
    assert!(
        text.contains("glow"),
        "the repository that produced it must be shown: {text}"
    );
    assert!(
        text.contains("github.com/charmbracelet/**"),
        "promoting it must be a copy-paste: {text}"
    );
}

/// gh lists accounts it could not log in to alongside working ones. Suggesting
/// a broken one hands someone a config that cannot work -- observed on the
/// author's machine, where one of three logins has an invalid token.
#[test]
fn an_account_gh_could_not_log_in_to_is_not_offered() {
    let status = "\
github.com
  ✓ Logged in to github.com account good (keyring)
  - Active account: true
  X Failed to log in to github.com account dead (default)
  - The token in default is invalid.
";

    let runner = MapRunner::new(HashMap::from([(
        MapRunner::key("gh", &["auth", "status"]),
        Captured {
            success: true,
            stdout: status.to_string(),
            stderr: String::new(),
        },
    )]));

    let logins = gh_logins(&runner);

    assert_eq!(
        logins.by_host.get("github.com").map(Vec::as_slice),
        Some(["good".to_string()].as_slice()),
        "a failed login must not be suggested"
    );
}

/// One org with `repos` repositories, enough to be proposed at 2 or more.
fn scan_of(host: &str, org: &str, repos: usize) -> Scan {
    Scan {
        orgs: vec![Org {
            host: host.to_string(),
            org: org.to_string(),
            repos: (0..repos).map(|i| format!("repo{i}")).collect(),
        }],
        ..Default::default()
    }
}

#[test]
fn a_proposed_account_parses_once_its_placeholders_are_filled() {
    let scan = scan_of("github.com", "acme-corp", 3);
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &[])
        .replace("REPLACE-ME", "acme-corp");
    let config = gitwho::config::Config::parse(&text).unwrap();
    assert_eq!(config.accounts[0].login, "acme-corp");
}

#[test]
fn a_gitea_org_is_proposed_with_a_url_and_the_tea_logins_for_it() {
    let scan = scan_of("git.example.net", "acme", 3);
    let tea = vec![("https://git.example.net".to_string(), "you".to_string())];
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &tea);
    assert!(text.contains("provider = \"gitea\""), "{text}");
    assert!(text.contains("url = \"https://git.example.net\""), "{text}");
    assert!(text.contains("tea holds: you"), "{text}");
    assert!(!text.contains("env ="), "{text}");
    assert!(!text.contains("gitCredential"), "{text}");
}

#[test]
fn an_unsupported_provider_is_proposed_commented_out() {
    let scan = scan_of("gitlab.com", "acme", 3);
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &[]);
    assert!(
        text.contains("# gitwho does not support gitlab.com"),
        "{text}"
    );
    assert!(!text.contains("\n[[accounts]]\nname = \"acme\""), "{text}");
}

#[test]
fn an_org_left_out_as_unsupported_is_not_counted_as_proposed() {
    let mut scan = scan_of("github.com", "acme-corp", 3);
    scan.orgs.extend(scan_of("gitlab.com", "acme", 3).orgs);
    let text = render(&scan, &[], &Default::default(), &[]);
    assert!(
        text.contains("in 2 organisation(s); 1 proposed as accounts."),
        "{text}"
    );
}
