//! Stand-ins for `gh` and `tea`, for tests that run the real binary.
//!
//! Each answers the questions gitwho asks — a login's token, and for tea which
//! logins exist — from files in a temp directory. Any other invocation reports
//! which variables it was handed, never their values (R10).

#![allow(dead_code)]

use std::cell::RefCell;
use std::path::Path;

pub struct FakeTools {
    dir: tempfile::TempDir,
    tea_logins: RefCell<Vec<serde_json::Value>>,
}

const GH: &str = r#"#!/bin/sh
if [ "$1" = auth ] && [ "$2" = token ]; then
  login=""
  while [ $# -gt 0 ]; do [ "$1" = --user ] && login="$2"; shift; done
  if [ -f "ROOT/gh/$login" ]; then cat "ROOT/gh/$login"; exit 0; fi
  echo "no oauth token found for github.com account $login" >&2
  exit 1
fi
for v in GH_TOKEN GITHUB_PERSONAL_ACCESS_TOKEN GITEA_TOKEN; do
  eval "val=\$$v"
  if [ -n "$val" ]; then echo "$v: set"; else echo "$v: unset"; fi
done
echo "args: $*"
"#;

const TEA: &str = r#"#!/bin/sh
if [ "$1" = login ] && [ "$2" = ls ]; then cat "ROOT/tea/logins.json"; exit 0; fi
if [ "$1" = login ] && [ "$2" = helper ] && [ "$3" = get ]; then
  host=""
  while IFS= read -r line; do
    [ -z "$line" ] && break
    case "$line" in host=*) host="${line#host=}" ;; esac
  done
  if [ -f "ROOT/tea/token-$host" ]; then
    printf 'protocol=https\nhost=%s\nusername=x\npassword=%s\n' "$host" "$(cat "ROOT/tea/token-$host")"
    exit 0
  fi
  exit 1
fi
for v in GITEA_TOKEN GITEA_ACCESS_TOKEN GH_TOKEN; do
  eval "val=\$$v"
  if [ -n "$val" ]; then echo "$v: set"; else echo "$v: unset"; fi
done
echo "GITEA_INSTANCE_URL: ${GITEA_INSTANCE_URL:-unset}"
echo "args: $*"
"#;

impl FakeTools {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for sub in ["bin", "gh", "tea"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tea/logins.json"), "[]").unwrap();
        let root_text = root.display().to_string();
        write_script(&root.join("bin/gh"), &GH.replace("ROOT", &root_text));
        write_script(&root.join("bin/tea"), &TEA.replace("ROOT", &root_text));
        Self {
            dir,
            tea_logins: RefCell::new(Vec::new()),
        }
    }

    /// `PATH` with the fakes ahead of everything else.
    pub fn path(&self) -> String {
        format!(
            "{}:{}",
            self.dir.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    pub fn gh_login(&self, login: &str, token: &str) {
        std::fs::write(self.dir.path().join("gh").join(login), token).unwrap();
    }

    /// As tea 0.15.1 does, the helper answers with the first login added for a host.
    pub fn tea_login(&self, name: &str, url: &str, user: &str, token: &str) {
        let host = url
            .split_once("://")
            .map_or(url, |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap();
        let token_file = self.dir.path().join("tea").join(format!("token-{host}"));
        if !token_file.exists() {
            std::fs::write(&token_file, token).unwrap();
        }
        let mut logins = self.tea_logins.borrow_mut();
        logins.push(serde_json::json!({
            "name": name, "url": url, "ssh_host": host, "user": user, "default": "false"
        }));
        std::fs::write(
            self.dir.path().join("tea/logins.json"),
            serde_json::to_string(&*logins).unwrap(),
        )
        .unwrap();
    }
}

fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
