use gitfriend::config::Config;

#[test]
fn parses_an_account_with_its_match_patterns() {
    let toml = r#"
        [defaults]
        account = "Personal"

        [[accounts]]
        name = "Personal"
        provider = "github"
        email = "me@example.com"
        gitAuth = "https"
        match = ["github.com/Personal/**"]
    "#;

    let config = Config::parse(toml).expect("config should parse");

    assert_eq!(config.defaults.account, "Personal");
    assert_eq!(config.accounts.len(), 1);
    assert_eq!(config.accounts[0].name, "Personal");
    assert_eq!(config.accounts[0].email, "me@example.com");
    assert_eq!(config.accounts[0].match_patterns, ["github.com/Personal/**"]);
}
