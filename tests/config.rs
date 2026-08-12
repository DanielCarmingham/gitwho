use gitwho::config::Config;

/// The example file is the first thing anyone copies, and `deny_unknown_fields`
/// means one renamed field turns it into a parse error on someone's first run.
/// Nothing else would catch that: the file is documentation, so it is never
/// otherwise loaded by a test.
#[test]
fn the_shipped_example_config_parses() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/accounts.toml.example");
    let text = std::fs::read_to_string(&path).expect("the example config should be readable");

    let config = Config::parse(&text).expect("the shipped example should parse");

    // The declared default must name an account that exists, or the fallback
    // every unmatched repo lands on does not resolve.
    assert!(
        config.account(&config.defaults.account).is_some(),
        "[defaults] account = {:?} names no account in the example",
        config.defaults.account
    );

    // It carries no values, only variable names -- the property that lets it be
    // committed (R10).
    for account in &config.accounts {
        for spec in &account.env {
            if let Some((_, value)) = spec.split_once('=') {
                assert!(
                    !value.to_ascii_lowercase().contains("token"),
                    "{} declares a literal that looks like a secret: {spec}",
                    account.name
                );
            }
        }
    }
}

#[test]
fn parses_an_account_with_its_match_patterns() {
    let toml = r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        match = ["github.com/Personal/**"]
    "#;

    let config = Config::parse(toml).expect("config should parse");

    assert_eq!(config.defaults.account, "Personal");
    assert_eq!(config.accounts.len(), 1);
    assert_eq!(config.accounts[0].name, "Personal");
    assert_eq!(config.accounts[0].email, "me@example.com");
    assert_eq!(
        config.accounts[0].match_patterns,
        ["github.com/Personal/**"]
    );
}

#[test]
fn an_account_can_use_ssh_and_https_at_once() {
    // The shape a self-hosted Gitea actually has: `ssh.git.example.net` over ssh AND
    // `git.example.net` over https, with one repo cloned each way. A single
    // account-level `gitAuth` cannot express that, so transport is not
    // declared at all -- the account states what it HAS, and git decides which
    // to use per remote. The credential helper is only ever asked about
    // https, so the split falls out for free.
    let config = Config::parse(
        r#"
        [defaults]
        account = "SelfHosted"

        [[accounts]]
        name = "SelfHosted"
        provider = "gitea"
        email = "you@example.net"
        gitCredential = "GITEA_TOKEN"
        sshKey = "~/.ssh/id_ed25519_selfhosted"
        match = ["ssh.git.example.net/**", "git.example.net/**"]
    "#,
    )
    .expect("an account should be able to declare both a token and a key");

    let account = config.account("SelfHosted").unwrap();
    assert_eq!(account.git_credential.as_deref(), Some("GITEA_TOKEN"));
    assert_eq!(
        account.ssh_key.as_deref(),
        Some("~/.ssh/id_ed25519_selfhosted")
    );
}
