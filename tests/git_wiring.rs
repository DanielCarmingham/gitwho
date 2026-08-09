use gitfriend::git::effective_helpers;

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
