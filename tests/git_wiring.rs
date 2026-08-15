use gitwho::git::effective_helpers;

/// An empty `credential.helper` value clears everything configured before it.
/// This machine's stack relies on that: Xcode's bundled gitconfig contributes
/// `osxkeychain`, which `~/.gitconfig-common` then resets away.
#[test]
fn an_empty_value_resets_the_helpers_before_it() {
    let configured = vec![
        "osxkeychain".to_string(),
        String::new(),
        "manager".to_string(),
        String::new(),
        "/usr/local/share/gcm-core/git-credential-manager".to_string(),
    ];

    let effective = effective_helpers(&configured);

    assert_eq!(
        effective,
        vec!["/usr/local/share/gcm-core/git-credential-manager".to_string()],
        "helpers cleared by a reset were reported as active"
    );
}

#[test]
fn helpers_with_no_reset_are_all_effective() {
    let configured = vec!["osxkeychain".to_string(), "manager".to_string()];

    assert_eq!(
        effective_helpers(&configured),
        vec!["osxkeychain".to_string(), "manager".to_string()]
    );
}

#[test]
fn a_trailing_reset_clears_everything() {
    let configured = vec!["manager".to_string(), String::new()];

    assert!(effective_helpers(&configured).is_empty());
}

mod repo_facts {
    use std::path::Path;
    use std::process::Command;

    use gitwho::git::{identity_pinned, remotes};

    /// Hermetic: the developer's own global and system gitconfig are switched
    /// off, so nothing here depends on the machine it runs on.
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

    #[test]
    fn every_remote_is_listed_with_its_url() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/tool.git",
            ],
        );
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "upstream",
                "https://git.example.net/acme/tool.git",
            ],
        );

        let mut found = remotes(dir.path());
        found.sort();

        assert_eq!(
            found,
            vec![
                (
                    "origin".to_string(),
                    "https://github.com/acme/tool.git".to_string()
                ),
                (
                    "upstream".to_string(),
                    "https://git.example.net/acme/tool.git".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_directory_that_is_not_a_repository_has_no_remotes() {
        let dir = tempfile::tempdir().unwrap();

        assert!(remotes(dir.path()).is_empty());
    }

    #[test]
    fn a_repo_that_sets_no_identity_of_its_own_is_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);

        assert!(!identity_pinned(dir.path()));
    }

    #[test]
    fn a_local_user_email_pins_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "me@example.invalid"]);

        assert!(identity_pinned(dir.path()));
    }

    #[test]
    fn a_local_include_pins_the_identity() {
        // The recommended fix for a repo spanning two accounts: include the
        // account's generated gitconfig locally, where it beats every global
        // rule.
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(
            dir.path(),
            &["config", "include.path", "/somewhere/Personal.gitconfig"],
        );

        assert!(identity_pinned(dir.path()));
    }
}
