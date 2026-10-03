#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"
    match = ["github.com/Personal/**"]
"#;

/// Records its arguments, then does whatever the test asked of it: replace the
/// file with `EDIT_WITH`, save it the way rename-on-save editors do, or fail.
const FAKE_EDITOR: &str = r#"#!/bin/sh
for arg in "$@"; do printf '%s\n' "$arg"; done > "$EDIT_LOG"
for last; do :; done
if [ -n "$EDIT_WITH" ]; then cat "$EDIT_WITH" > "$last"; fi
if [ -n "$EDIT_BY_RENAME" ]; then
    cp "$last" "$last.new" && chmod 644 "$last.new" && mv "$last.new" "$last"
fi
exit "${EDIT_EXIT:-0}"
"#;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    /// A directory with a space in its name, because the editor is run through
    /// a shell and a path that splits there is the classic way that breaks.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("my config")).unwrap();
        let editor = dir.path().join("editor");
        std::fs::write(&editor, FAKE_EDITOR).unwrap();
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fixture { dir }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("my config").join("accounts.toml")
    }

    fn write_config(&self, contents: &str) {
        std::fs::write(self.config(), contents).unwrap();
        std::fs::set_permissions(self.config(), std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("editor.log")
    }

    fn editor_args(&self) -> Option<Vec<String>> {
        let log = std::fs::read_to_string(self.log()).ok()?;
        Some(log.lines().map(str::to_string).collect())
    }

    /// `gitwho` with git's config isolated, so the developer's own
    /// `core.editor` can never be what a test observes.
    fn gitwho(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gitwho"));
        cmd.args(args)
            .current_dir(self.dir.path())
            .env("GITWHO_CONFIG", self.config())
            .env("GITWHO_GIT_DIR", self.dir.path().join("git"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("VISUAL")
            .env_remove("EDITOR")
            .env("EDIT_LOG", self.log());
        cmd
    }

    fn edit(&self, editor: &str, configure: impl FnOnce(&mut Command)) -> (Output, String, String) {
        let mut cmd = self.gitwho(&["config", "edit"]);
        cmd.env("GIT_EDITOR", editor);
        configure(&mut cmd);
        let out = cmd.output().unwrap();
        let stdout = String::from_utf8(out.stdout.clone()).unwrap();
        let stderr = String::from_utf8(out.stderr.clone()).unwrap();
        (out, stdout, stderr)
    }

    fn editor(&self) -> String {
        format!("'{}'", self.dir.path().join("editor").display())
    }

    fn replacement(&self, contents: &str) -> PathBuf {
        let path = self.dir.path().join("replacement.toml");
        std::fs::write(&path, contents).unwrap();
        path
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn opens_the_config_in_gits_editor_keeping_the_editors_own_arguments() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);

    let (out, _, stderr) = f.edit(&format!("{} --wait", f.editor()), |_| {});

    assert!(out.status.success(), "{stderr}");
    let config = f.config().display().to_string();
    assert_eq!(f.editor_args(), Some(vec!["--wait".to_string(), config]));
}

#[test]
fn reports_an_invalid_config_as_soon_as_the_editor_closes() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);
    let broken = f.replacement(&ACCOUNTS.replace("login = \"personal\"", "login = \"\""));

    let (out, _, stderr) = f.edit(&f.editor(), |cmd| {
        cmd.env("EDIT_WITH", &broken);
    });

    assert!(!out.status.success());
    assert!(stderr.contains("`login`"), "{stderr}");
    assert!(stderr.contains("gitwho config edit"), "{stderr}");
}

#[test]
fn says_to_run_sync_when_the_generated_rules_no_longer_match() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);

    let (out, stdout, stderr) = f.edit(&f.editor(), |_| {});

    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("gitwho sync --write"), "{stdout}");
}

#[test]
fn stays_quiet_about_sync_when_the_rules_are_current() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);
    let synced = f.gitwho(&["sync", "--write"]).output().unwrap();
    assert!(synced.status.success(), "{synced:?}");

    let (out, stdout, stderr) = f.edit(&f.editor(), |_| {});

    assert!(out.status.success(), "{stderr}");
    assert!(!stdout.contains("sync"), "{stdout}");
}

#[test]
fn warns_when_the_editor_saved_the_file_readable_by_others() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);

    let (out, _, stderr) = f.edit(&f.editor(), |cmd| {
        cmd.env("EDIT_BY_RENAME", "1");
    });

    assert_eq!(
        mode(&f.config()),
        0o644,
        "the fake editor should have loosened it"
    );
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("0644"), "{stderr}");
    assert!(stderr.contains("chmod 600"), "{stderr}");
}

#[test]
fn fails_when_the_editor_does() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);

    let (out, _, stderr) = f.edit(&f.editor(), |cmd| {
        cmd.env("EDIT_EXIT", "3");
    });

    assert!(!out.status.success());
    assert!(stderr.contains("editor"), "{stderr}");
}

#[test]
fn points_at_init_instead_of_editing_a_config_that_does_not_exist() {
    let f = Fixture::new();

    let (out, _, stderr) = f.edit(&f.editor(), |_| {});

    assert!(!out.status.success());
    assert!(stderr.contains("gitwho init"), "{stderr}");
    assert_eq!(f.editor_args(), None, "the editor should not have run");
    assert!(!f.config().exists());
}

#[test]
fn names_the_file_when_git_has_no_editor_to_offer() {
    let f = Fixture::new();
    f.write_config(ACCOUNTS);

    let out = f
        .gitwho(&["config", "edit"])
        .env_remove("GIT_EDITOR")
        .env("TERM", "dumb")
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains(&f.config().display().to_string()),
        "{stderr}"
    );
}
