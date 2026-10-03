use std::path::Path;
use std::process::Command;

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Work"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"

    [[accounts]]
    name = "Work"
    provider = "github"
    login = "work"
    email = "me@work.example"

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    login = "selfhosted"
    url = "https://git.example.net"
    email = "you@example.net"
"#;

fn accounts(config: &str, args: &[&str]) -> (std::process::Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.toml");
    std::fs::write(&path, config).unwrap();
    let out = run(&path, dir.path(), args);
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    (out, stdout)
}

fn run(config: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gitwho"))
        .arg("accounts")
        .args(args)
        .current_dir(cwd)
        .env("GITWHO_CONFIG", config)
        .output()
        .unwrap()
}

fn row<'a>(stdout: &'a str, name: &str) -> &'a str {
    stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no row for {name}:\n{stdout}"))
}

#[test]
fn lists_every_account_in_declaration_order() {
    let (out, stdout) = accounts(ACCOUNTS, &[]);

    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    let names: Vec<&str> = stdout
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(names, ["Personal", "Work", "SelfHosted"], "{stdout}");
}

#[test]
fn shows_provider_login_server_and_email_for_each() {
    let (_, stdout) = accounts(ACCOUNTS, &[]);

    let gitea = row(&stdout, "SelfHosted");
    for field in [
        "gitea",
        "selfhosted",
        "https://git.example.net",
        "you@example.net",
    ] {
        assert!(gitea.contains(field), "{field} missing: {gitea}");
    }
    let github = row(&stdout, "Personal");
    for field in ["github", "personal", "github.com", "me@example.com"] {
        assert!(github.contains(field), "{field} missing: {github}");
    }
}

#[test]
fn marks_the_default_account_and_only_that_one() {
    let (_, stdout) = accounts(ACCOUNTS, &[]);

    assert!(row(&stdout, "Work").contains("default"), "{stdout}");
    assert!(!row(&stdout, "Personal").contains("default"), "{stdout}");
    assert!(!row(&stdout, "SelfHosted").contains("default"), "{stdout}");
}

#[test]
fn quiet_prints_bare_names_for_scripts() {
    let (out, stdout) = accounts(ACCOUNTS, &["--quiet"]);

    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    assert_eq!(stdout, "Personal\nWork\nSelfHosted\n");
}

#[test]
fn says_so_when_no_account_is_declared() {
    let (out, stdout) = accounts("[defaults]\naccount = \"Nobody\"\n", &[]);

    assert!(out.status.success(), "stderr: {:?}", out.stderr);
    assert!(stdout.is_empty(), "{stdout}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no accounts"), "{stderr}");
}

#[test]
fn reports_a_missing_config_rather_than_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&dir.path().join("absent.toml"), dir.path(), &[]);

    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("absent.toml"), "{stderr}");
}
