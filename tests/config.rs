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

    // It carries no values, only logins -- the property that lets it be
    // committed (R10).
    use gitwho::provider::Provider;
    assert!(config
        .accounts
        .iter()
        .any(|a| a.provider == Provider::Github));
    assert!(config
        .accounts
        .iter()
        .any(|a| a.provider == Provider::Gitea));
}

#[test]
fn parses_an_account_with_its_match_patterns() {
    let toml = r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        login = "personal"
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
        login = "selfhosted"
        url = "https://ssh.git.example.net"
        email = "you@example.net"
        sshKey = "~/.ssh/id_ed25519_selfhosted"
        match = ["ssh.git.example.net/**", "git.example.net/**"]
    "#,
    )
    .expect("an account should be able to declare both a token and a key");

    let account = config.account("SelfHosted").unwrap();
    assert_eq!(account.url.as_deref(), Some("https://ssh.git.example.net"));
    assert_eq!(
        account.ssh_key.as_deref(),
        Some("~/.ssh/id_ed25519_selfhosted")
    );
}

fn parse_err(toml: &str) -> String {
    Config::parse(toml).unwrap_err().to_string()
}

const HEAD: &str =
    "[defaults]\naccount = \"A\"\n\n[[accounts]]\nname = \"A\"\nemail = \"a@example.com\"\n";

#[test]
fn each_removed_field_is_named_with_what_replaced_it() {
    let env = parse_err(&format!(
        "{HEAD}provider = \"github\"\nlogin = \"a\"\nenv = [\"GH_TOKEN\"]\n"
    ));
    assert!(
        env.contains("account A") && env.contains("`env`") && env.contains("login"),
        "{env}"
    );

    let cred = parse_err(&format!(
        "{HEAD}provider = \"github\"\nlogin = \"a\"\ngitCredential = \"GH_TOKEN\"\n"
    ));
    assert!(cred.contains("`gitCredential`"), "{cred}");

    let backend = parse_err(
        "[defaults]\naccount = \"A\"\nsecretBackend = \"age\"\n\n[[accounts]]\nname = \"A\"\nprovider = \"github\"\nlogin = \"a\"\nemail = \"a@example.com\"\n",
    );
    assert!(
        backend.contains("[defaults]") && backend.contains("`secretBackend`"),
        "{backend}"
    );
}

#[test]
fn gitea_without_a_url_is_rejected() {
    let message = parse_err(&format!("{HEAD}provider = \"gitea\"\nlogin = \"a\"\n"));
    assert!(
        message.contains("account A") && message.contains("url"),
        "{message}"
    );
}

#[test]
fn github_with_a_url_is_rejected() {
    let message = parse_err(&format!(
        "{HEAD}provider = \"github\"\nlogin = \"a\"\nurl = \"https://github.example.com\"\n"
    ));
    assert!(message.contains("github.com"), "{message}");
}

#[test]
fn an_account_without_a_login_is_rejected() {
    let message = parse_err(&format!("{HEAD}provider = \"github\"\n"));
    assert!(message.contains("login"), "{message}");
}

#[test]
fn an_account_with_an_empty_login_is_rejected() {
    let message = parse_err(&format!("{HEAD}provider = \"github\"\nlogin = \"\"\n"));
    assert!(
        message.contains("account A") && message.contains("login"),
        "{message}"
    );
}

#[test]
fn an_account_with_a_blank_login_is_rejected() {
    let message = parse_err(&format!("{HEAD}provider = \"github\"\nlogin = \"  \"\n"));
    assert!(
        message.contains("account A") && message.contains("login"),
        "{message}"
    );
}

#[test]
fn a_forgejo_account_parses_as_gitea() {
    let config = Config::parse(&format!(
        "{HEAD}provider = \"forgejo\"\nlogin = \"a\"\nurl = \"https://git.example.net\"\n"
    ))
    .unwrap();
    assert_eq!(
        config.accounts[0].provider,
        gitwho::provider::Provider::Gitea
    );
    assert_eq!(
        config.accounts[0].url.as_deref(),
        Some("https://git.example.net")
    );
}

#[test]
fn a_gitea_url_without_an_http_scheme_is_rejected() {
    for url in [
        "git.example.net",
        "ssh://git.example.net",
        "ftp://git.example.net",
    ] {
        let message = parse_err(&format!(
            "{HEAD}provider = \"gitea\"\nlogin = \"a\"\nurl = \"{url}\"\n"
        ));
        assert!(
            message.contains("account A") && message.contains("https://"),
            "{url}: {message}"
        );
    }
}

#[test]
fn a_gitea_url_over_http_or_https_is_accepted() {
    for url in ["https://git.example.net", "http://git.example.net:3000"] {
        Config::parse(&format!(
            "{HEAD}provider = \"gitea\"\nlogin = \"a\"\nurl = \"{url}\"\n"
        ))
        .unwrap_or_else(|e| panic!("{url}: {e}"));
    }
}

#[test]
fn a_removed_field_points_at_the_upgrade_instructions() {
    let message = parse_err(&format!(
        "{HEAD}provider = \"github\"\nlogin = \"a\"\ngitCredential = \"GH_TOKEN\"\n"
    ));
    assert!(
        message.contains(
            "https://github.com/DanielCarmingham/gitwho/blob/main/docs/INSTALL.md#upgrading-from-02"
        ),
        "{message}"
    );
}

#[test]
fn a_leftover_secret_backend_says_to_remove_the_keychain_entries_too() {
    let message = parse_err(
        "[defaults]\naccount = \"A\"\nsecretBackend = \"keychain\"\n\n[[accounts]]\nname = \"A\"\nprovider = \"github\"\nlogin = \"a\"\nemail = \"a@example.com\"\n",
    );
    assert!(
        message.contains("keychain") && message.contains("service `gitwho`"),
        "{message}"
    );
}
