use std::path::Path;

use gitwho::init::{self, Applied, Snippet};

mod common;
use common::FakeTools;

fn home() -> &'static Path {
    Path::new("/home/someone")
}

fn includes() -> Snippet {
    Snippet::gitconfig_include(
        Path::new("/home/someone/.config/gitwho/git/includes.gitconfig"),
        home(),
    )
}

#[test]
fn the_gitconfig_snippet_is_an_include_naming_the_generated_file() {
    let snippet = includes();

    assert!(snippet.text.contains("[include]"));
    assert!(
        snippet
            .text
            .contains("path = ~/.config/gitwho/git/includes.gitconfig"),
        "git expands a leading ~ in include.path, so no username is needed; got: {}",
        snippet.text
    );
    assert!(
        !snippet.text.contains("/home/someone"),
        "got: {}",
        snippet.text
    );
}

/// The point of a marker is that it survives reformatting. Someone who pasted
/// the line by hand, or let an editor reindent it, must not get a second copy.
#[test]
fn an_include_added_by_hand_counts_as_already_present() {
    let snippet = includes();

    for hand_written in [
        "[include]\n    path = /home/someone/.config/gitwho/git/includes.gitconfig\n",
        "[include]\n    path = ~/.config/gitwho/git/includes.gitconfig\n",
        "[include]\n\tpath = \"~/.config/gitwho/git/includes.gitconfig\"\n",
    ] {
        assert!(
            snippet.is_present_in(hand_written),
            "a hand-written include of the same file must not be duplicated: {hand_written}"
        );
    }
}

#[test]
fn an_unrelated_gitconfig_does_not_count_as_wired() {
    let snippet = includes();

    let other = "[include]\n\tpath = ~/.gitconfig-common\n[user]\n\temail = you@example.com\n";

    assert!(!snippet.is_present_in(other));
}

/// A different store directory is a different install, not the same one. This
/// is what makes `GITWHO_CONFIG`-based testing safe: a temp-dir run must not
/// see the real machine's wiring as already done.
#[test]
fn a_different_store_directory_is_not_the_same_wiring() {
    let snippet = includes();
    let elsewhere = "[include]\n\tpath = /tmp/other/git/includes.gitconfig\n";

    assert!(!snippet.is_present_in(elsewhere));
}

#[test]
fn a_store_outside_home_is_included_by_its_full_path() {
    let snippet = Snippet::gitconfig_include(Path::new("/opt/gitwho/includes.gitconfig"), home());

    assert!(
        snippet
            .text
            .contains("path = /opt/gitwho/includes.gitconfig"),
        "got: {}",
        snippet.text
    );
}

fn default_shims() -> Snippet {
    Snippet::path_export(Path::new("/home/someone/.local/share/gitwho/shims"), home())
}

#[test]
fn the_path_snippet_prepends_the_shim_directory() {
    let snippet = default_shims();

    assert!(
        snippet
            .text
            .contains("\"$HOME/.local/share/gitwho/shims:$PATH\""),
        "the shim dir must come first, or the real gh wins; got: {}",
        snippet.text
    );
    assert!(
        snippet
            .text
            .lines()
            .any(|line| line.starts_with("export PATH=")),
        "the snippet must contain an export line of its own; got: {}",
        snippet.text
    );
}

/// A dotfiles repo shared across machines cannot carry `/Users/<name>`, so the
/// line gitwho writes must not either.
#[test]
fn the_path_snippet_does_not_spell_out_the_home_directory() {
    let snippet = default_shims();

    assert!(
        !snippet.text.contains("/home/someone"),
        "got: {}",
        snippet.text
    );
}

/// The duplicate that prompted this: a line written by hand, or by an older
/// gitwho, names the same directory through a different spelling of HOME.
#[test]
fn a_path_line_naming_home_any_common_way_counts_as_already_present() {
    let snippet = default_shims();

    for line in [
        "export PATH=\"/home/someone/.local/share/gitwho/shims:$PATH\"\n",
        "export PATH=\"$HOME/.local/share/gitwho/shims:$PATH\"\n",
        "export PATH=\"${HOME}/.local/share/gitwho/shims:$PATH\"\n",
        "export PATH=~/.local/share/gitwho/shims:$PATH\n",
        "path=($HOME/.local/share/gitwho/shims $path)\n",
    ] {
        assert!(
            snippet.is_present_in(line),
            "must not be duplicated: {line}"
        );
    }
}

#[test]
fn a_path_line_for_another_directory_under_home_is_not_the_same_wiring() {
    let snippet = default_shims();

    assert!(!snippet.is_present_in("export PATH=\"$HOME/.local/bin:$PATH\"\n"));
}

/// `--shim-dir` can point anywhere; there is no `$HOME` form to write then.
#[test]
fn a_shim_directory_outside_home_is_written_as_is() {
    let snippet = Snippet::path_export(Path::new("/opt/shims"), home());

    assert!(
        snippet.text.contains("\"/opt/shims:$PATH\""),
        "got: {}",
        snippet.text
    );
    assert!(snippet.is_present_in("export PATH=\"/opt/shims:$PATH\"\n"));
}

#[test]
fn ensure_reports_what_it_would_do_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let rc = dir.path().join(".zshrc");
    std::fs::write(&rc, "# existing content\n").unwrap();

    let snippet = Snippet::path_export(Path::new("/opt/shims"), home());
    let applied = init::ensure(&rc, &snippet, false).unwrap();

    assert_eq!(applied, Applied::WouldAppend);
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        "# existing content\n",
        "a dry run must not touch the file"
    );
}

#[test]
fn ensure_appends_once_and_then_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let rc = dir.path().join(".zshrc");
    std::fs::write(&rc, "# existing content\n").unwrap();

    let snippet = Snippet::path_export(Path::new("/opt/shims"), home());

    assert_eq!(
        init::ensure(&rc, &snippet, true).unwrap(),
        Applied::Appended
    );
    let after_first = std::fs::read_to_string(&rc).unwrap();
    assert!(after_first.starts_with("# existing content\n"));
    assert!(after_first.contains("/opt/shims"));

    assert_eq!(
        init::ensure(&rc, &snippet, true).unwrap(),
        Applied::AlreadyPresent
    );
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        after_first,
        "re-running must not append a second copy"
    );
}

/// Appending to a file that does not end in a newline would otherwise splice
/// the new line onto the end of the old one -- which, for a shell rc file, is a
/// syntax error the user did not cause.
#[test]
fn ensure_separates_itself_from_a_file_with_no_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let rc = dir.path().join(".zshrc");
    std::fs::write(&rc, "alias foo=bar").unwrap();

    let snippet = Snippet::path_export(Path::new("/opt/shims"), home());
    init::ensure(&rc, &snippet, true).unwrap();

    let after = std::fs::read_to_string(&rc).unwrap();
    assert!(
        after.contains("alias foo=bar\n"),
        "the existing last line must stay intact; got:\n{after}"
    );
}

/// A missing `~/.zshrc` is a real situation (bash users, fresh containers) and
/// is not an error -- but silently creating one that a login shell may never
/// read would be the quiet wrongness this project exists to avoid.
#[test]
fn a_missing_file_is_reported_rather_than_created() {
    let dir = tempfile::tempdir().unwrap();
    let rc = dir.path().join(".zshrc");

    let snippet = Snippet::path_export(Path::new("/opt/shims"), home());
    let applied = init::ensure(&rc, &snippet, true).unwrap();

    assert_eq!(applied, Applied::FileMissing);
    assert!(!rc.exists(), "ensure must not conjure a shell rc file");
}

// --- The `gitwho init` command, end to end -----------------------------------
//
// A throwaway HOME, so these exercise the real decisions about where the
// gitconfig and the shell rc file live -- which the pure functions above cannot
// cover, and which are exactly what a first-time user hits.

use std::process::Command;

/// A HOME that looks like a machine someone actually uses: an existing
/// gitconfig and an existing zshrc, both of which must survive.
fn fresh_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".gitconfig"), "[user]\n\tname = Someone\n").unwrap();
    std::fs::write(home.path().join(".zshrc"), "# existing content\n").unwrap();
    home
}

fn gitwho(home: &Path, args: &[&str], path: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gitwho"))
        .args(args)
        .env("HOME", home)
        .env("SHELL", "/bin/zsh")
        .env("PATH", path)
        // Not inherited: a developer machine has these set, and a test that
        // silently used the real store would be both wrong and dangerous.
        .env_remove("GITWHO_CONFIG")
        .env_remove("GITWHO_GIT_DIR")
        // Either one points git's global config away from the fake HOME, and
        // `init` asks git whether that config already includes the rules.
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_dry_run_on_a_virgin_machine_writes_nothing() {
    let home = fresh_home();

    let out = gitwho(
        home.path(),
        &["init"],
        &std::env::var("PATH").unwrap_or_default(),
    );
    let text = stdout(&out);

    assert!(out.status.success(), "a dry run is not a failure: {text}");
    assert!(text.contains("would create"), "got:\n{text}");
    assert!(
        !home.path().join(".config/gitwho/accounts.toml").exists(),
        "a dry run must not create the config"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join(".gitconfig")).unwrap(),
        "[user]\n\tname = Someone\n",
        "a dry run must not touch the gitconfig"
    );
}

/// The one step it cannot do for you. Generating rules from the template would
/// produce a machine that looks configured and resolves every repository to an
/// account that does not exist -- working-but-wrong (R8).
#[test]
fn the_first_write_scaffolds_the_config_and_stops() {
    let home = fresh_home();

    let out = gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );
    let text = stdout(&out);

    assert!(
        !out.status.success(),
        "an unfinished setup must exit non-zero; got:\n{text}"
    );

    let config = home.path().join(".config/gitwho/accounts.toml");
    assert!(config.exists(), "the config should have been scaffolded");
    assert!(
        !home.path().join(".config/gitwho/git").exists(),
        "no rules should be generated from a template of placeholders"
    );
    assert!(
        !std::fs::read_to_string(home.path().join(".gitconfig"))
            .unwrap()
            .contains("includes.gitconfig"),
        "nothing should be wired up before the config is real"
    );
}

/// `0700` on the directory and `0600` on the config are not hygiene. The
/// config is a redirect vector: whoever can write it can add a `match` for a
/// host they control and be handed a token.
#[cfg(unix)]
#[test]
fn the_scaffolded_store_and_config_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let home = fresh_home();
    gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );

    let mode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode(home.path().join(".config/gitwho")), 0o700);
    assert_eq!(
        mode(home.path().join(".config/gitwho/accounts.toml")),
        0o600
    );
}

/// What the scaffold hands you must be usable as-is. If the shipped template
/// did not parse, the second `init` would fail on a file gitwho itself wrote.
#[test]
fn the_scaffolded_config_is_one_gitwho_can_read_back() {
    let home = fresh_home();
    gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );

    let text = std::fs::read_to_string(home.path().join(".config/gitwho/accounts.toml")).unwrap();
    gitwho::config::Config::parse(&text).expect("the scaffolded config must parse");
}

/// The whole promise of `init`: run it again after adding an account and it
/// changes only what changed. A second copy of either line would be a bug the
/// user has to find by reading their own dotfiles.
#[test]
fn a_completed_setup_re_runs_without_duplicating_anything() {
    let home = fresh_home();
    let path = std::env::var("PATH").unwrap_or_default();
    gitwho(home.path(), &["init", "--write"], &path);
    let fakes = complete_setup(home.path());

    let first = gitwho(home.path(), &["init", "--write"], &fakes.path());
    assert!(
        first.status.success(),
        "a completed setup should pass doctor; got:\n{}",
        stdout(&first)
    );

    let gitconfig = std::fs::read_to_string(home.path().join(".gitconfig")).unwrap();
    let zshrc = std::fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert_eq!(gitconfig.matches("includes.gitconfig").count(), 1);
    assert!(
        gitconfig.contains("[user]"),
        "existing content must survive"
    );
    assert!(zshrc.contains("# existing content"));

    let second = gitwho(home.path(), &["init", "--write"], &fakes.path());
    let text = stdout(&second);

    assert!(
        !text.contains("appended"),
        "a second run must append nothing; got:\n{text}"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join(".gitconfig")).unwrap(),
        gitconfig,
        "the gitconfig must be byte-identical after a second run"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join(".zshrc")).unwrap(),
        zshrc,
        "the shell rc must be byte-identical after a second run"
    );
}

#[test]
fn a_shim_path_line_already_written_with_home_is_not_added_again() {
    let home = fresh_home();
    let zshrc = "export PATH=\"$HOME/.local/share/gitwho/shims:$PATH\"\n";
    std::fs::write(home.path().join(".zshrc"), zshrc).unwrap();
    gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );
    let fakes = complete_setup(home.path());

    let out = gitwho(home.path(), &["init", "--write"], &fakes.path());

    assert_eq!(
        std::fs::read_to_string(home.path().join(".zshrc")).unwrap(),
        zshrc,
        "report was:\n{}",
        stdout(&out)
    );
}

/// A gitconfig kept in a dotfiles repo often delegates to a shared file, so the
/// include can sit one file down and be spelled with `~`. git follows it, and
/// so must the check.
#[test]
fn an_include_reached_through_another_included_file_is_not_added_again() {
    let home = fresh_home();
    let gitconfig = "[include]\n\tpath = ~/.gitconfig-common\n";
    std::fs::write(home.path().join(".gitconfig"), gitconfig).unwrap();
    std::fs::write(
        home.path().join(".gitconfig-common"),
        "[include]\n    path = ~/.config/gitwho/git/includes.gitconfig\n",
    )
    .unwrap();
    gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );
    let fakes = complete_setup(home.path());

    let out = gitwho(home.path(), &["init", "--write"], &fakes.path());
    let text = stdout(&out);

    assert_eq!(
        std::fs::read_to_string(home.path().join(".gitconfig")).unwrap(),
        gitconfig,
        "report was:\n{text}"
    );
    assert!(
        text.contains("identity and credential rules"),
        "the step must still be reported; got:\n{text}"
    );
}

/// Steps 1 and 2 of what `init` asks of you: a real config in place of the
/// template, and gh logged in as the login it declares. Without the second,
/// `doctor` fails, correctly.
fn complete_setup(home: &Path) -> FakeTools {
    std::fs::write(
        home.join(".config/gitwho/accounts.toml"),
        r#"
[defaults]
account = "Personal"
gitName = "Test Person"

[[accounts]]
name = "Personal"
provider = "github"
login = "someone"
email = "you@example.com"
match = ["github.com/someone/**"]
"#,
    )
    .unwrap();

    let fakes = FakeTools::new();
    fakes.gh_login("someone", "fake-token");
    fakes
}

/// The dist shell installer writes its receipt to
/// `${XDG_CONFIG_HOME:-~/.config}/gitwho/` -- gitwho's own store directory --
/// with a plain `mkdir -p`, so a `022` umask leaves it `0755` before gitwho has
/// ever run. Found by running the real installer in a Linux container: the
/// documented install, followed by `init`, failed `doctor` on permissions
/// gitwho never set.
#[cfg(unix)]
#[test]
fn init_tightens_a_store_directory_someone_else_created() {
    use std::os::unix::fs::PermissionsExt;

    let home = fresh_home();
    let store = home.path().join(".config/gitwho");

    // Exactly what the installer leaves behind.
    std::fs::create_dir_all(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(store.join("gitwho-receipt.json"), "{}").unwrap();

    let out = gitwho(
        home.path(),
        &["init", "--write"],
        &std::env::var("PATH").unwrap_or_default(),
    );
    let text = stdout(&out);

    let mode = std::fs::metadata(&store).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o700,
        "init should have closed the store down; report was:\n{text}"
    );
    assert!(
        text.contains("tightened"),
        "and should have said so rather than fixing it silently; got:\n{text}"
    );

    // The receipt is not ours, and removing it would break `gitwho-update`.
    assert!(
        store.join("gitwho-receipt.json").exists(),
        "init must not delete the installer's receipt"
    );
}

/// A dry run reports the problem and changes nothing -- including permissions.
#[cfg(unix)]
#[test]
fn a_dry_run_does_not_tighten_anything() {
    use std::os::unix::fs::PermissionsExt;

    let home = fresh_home();
    let store = home.path().join(".config/gitwho");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = gitwho(
        home.path(),
        &["init"],
        &std::env::var("PATH").unwrap_or_default(),
    );

    assert_eq!(
        std::fs::metadata(&store).unwrap().permissions().mode() & 0o777,
        0o755,
        "a dry run must not change permissions"
    );
    assert!(stdout(&out).contains("would fix"), "got:\n{}", stdout(&out));
}
