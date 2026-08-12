use gitwho::config::Config;

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
    assert_eq!(config.accounts[0].match_patterns, ["github.com/Personal/**"]);
}

#[test]
fn an_account_can_use_ssh_and_https_at_once() {
    // Digilope's real shape: `app-gitea.digilope.com` over ssh AND
    // `gitea.digilope.com` over https, with one repo cloned each way. A single
    // account-level `gitAuth` cannot express that, so transport is not
    // declared at all -- the account states what it HAS, and git decides which
    // to use per remote. The credential helper is only ever asked about
    // https, so the split falls out for free.
    let config = Config::parse(
        r#"
        [defaults]
        account = "Digilope"

        [[accounts]]
        name = "Digilope"
        provider = "gitea"
        email = "me@digilope.example"
        gitCredential = "GITEA_TOKEN"
        sshKey = "~/.ssh/id_ed25519_Digilope_Gitea_Daniel"
        match = ["app-gitea.digilope.com/**", "gitea.digilope.com/**"]
    "#,
    )
    .expect("an account should be able to declare both a token and a key");

    let account = config.account("Digilope").unwrap();
    assert_eq!(account.git_credential.as_deref(), Some("GITEA_TOKEN"));
    assert_eq!(
        account.ssh_key.as_deref(),
        Some("~/.ssh/id_ed25519_Digilope_Gitea_Daniel")
    );
}
