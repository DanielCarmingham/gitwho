use std::path::Path;
use std::process::Command;

use gitwho::config::Config;
use gitwho::resolve::{resolve_repo, Reason};

const TWO_GITHUB_ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    email = "me@work.example"
    match = ["github.com/WorkOrg/**"]
"#;

/// A real git repo in a temp dir. Hermetic: the ambient global and system
/// gitconfig are switched off so the developer's own `includeIf` rules cannot
/// influence the result.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} failed");
}

fn repo_with_origin(dir: &Path, origin: &str) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "test@example.invalid"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["remote", "add", "origin", origin]);
}

#[test]
fn resolves_a_repo_from_its_origin_remote_wherever_it_lives() {
    // A temp dir is by construction outside every account root -- this is the
    // relocated-clone case that path rules get silently wrong (R1, R2).
    let dir = tempfile::tempdir().unwrap();
    repo_with_origin(dir.path(), "https://github.com/WorkOrg/somerepo.git");
    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();

    let resolved = resolve_repo(&config, dir.path()).unwrap();

    assert_eq!(resolved.account.name, "Work");
    assert_eq!(resolved.reason, Reason::OriginUrl);
}

#[test]
fn resolves_a_linked_worktree_placed_in_a_foreign_root() {
    // supacode places worktrees under its own root, far from the main repo.
    // The worktree's `.git` is a file, not a directory, so anything reading
    // the path rather than asking git gets this wrong (R2).
    let main = tempfile::tempdir().unwrap();
    repo_with_origin(main.path(), "https://github.com/WorkOrg/somerepo.git");
    git(main.path(), &["commit", "-q", "--allow-empty", "-m", "init"]);

    let foreign_root = tempfile::tempdir().unwrap();
    let worktree = foreign_root.path().join("feature-branch");
    git(
        main.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );

    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();
    let resolved = resolve_repo(&config, &worktree).unwrap();

    assert_eq!(resolved.account.name, "Work");
    assert_eq!(resolved.reason, Reason::OriginUrl);
}

/// Path rules apply only to repos with no remote, so a fresh `git init` in a
/// known root still commits as the right person (R4).
fn accounts_with_path_rule(root: &Path) -> String {
    format!(
        r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Personal/**"]

        [[accounts]]
        name = "Work"
        provider = "github"
        email = "me@work.example"
        match = ["github.com/WorkOrg/**"]
        paths = ["{}"]
    "#,
        root.display()
    )
}

#[test]
fn a_repo_with_no_remote_falls_back_to_a_path_rule() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("scratch");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);

    let config = Config::parse(&accounts_with_path_rule(root.path())).unwrap();
    let resolved = resolve_repo(&config, &repo).unwrap();

    assert_eq!(resolved.account.name, "Work");
    assert_eq!(resolved.reason, Reason::PathFallback);
}

#[test]
fn a_repo_matching_nothing_falls_back_to_the_declared_default() {
    // No remote, and nowhere near a declared path root. R4 requires this be a
    // stated fallback rather than an emergent one, and the reason must say so
    // -- callers use it to refuse handing over a secret.
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);

    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();
    let resolved = resolve_repo(&config, repo.path()).unwrap();

    assert_eq!(resolved.account.name, "Personal");
    assert_eq!(resolved.reason, Reason::Default);
}

#[test]
fn a_fork_resolves_to_its_origin_not_its_upstream() {
    // Guard, not a discovery: only `origin` is ever consulted. Fetching from
    // `upstream` still authenticates correctly, because the credential helper
    // resolves per-URL at transport time rather than from this identity.
    let dir = tempfile::tempdir().unwrap();
    repo_with_origin(dir.path(), "https://github.com/Personal/fork.git");
    git(
        dir.path(),
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/WorkOrg/original.git",
        ],
    );

    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();
    let resolved = resolve_repo(&config, dir.path()).unwrap();

    assert_eq!(resolved.account.name, "Personal");
}

#[test]
fn a_remote_matching_no_account_falls_back_to_the_default_as_unmatched() {
    // ~12 third-party clones live under this machine's personal root
    // (microsoft, dotnet, charmbracelet...). Erroring in them would make
    // `gitwho exec` fail inside any repo you merely cloned to read.
    // Falling back is fine; falling back *silently* is not, so it carries its
    // own reason rather than posing as the ordinary default.
    let dir = tempfile::tempdir().unwrap();
    repo_with_origin(dir.path(), "https://github.com/microsoft/vscode.git");

    let config = Config::parse(TWO_GITHUB_ACCOUNTS).unwrap();
    let resolved = resolve_repo(&config, dir.path()).unwrap();

    assert_eq!(resolved.account.name, "Personal");
    assert_eq!(resolved.reason, Reason::Unmatched);
}

#[test]
fn an_ambiguous_remote_is_still_an_error_not_a_fallback() {
    // Unmatched means "nobody claims this". Ambiguous means "two accounts
    // claim it" -- there is a right answer and we cannot tell which, so
    // falling back would be guessing.
    let dir = tempfile::tempdir().unwrap();
    repo_with_origin(dir.path(), "https://github.com/Shared/thing.git");

    let config = Config::parse(
        r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Shared/**"]

        [[accounts]]
        name = "Work"
        provider = "github"
        email = "me@work.example"
        match = ["github.com/Shared/**"]
    "#,
    )
    .unwrap();

    resolve_repo(&config, dir.path()).expect_err("an ambiguous remote must not fall back");
}
