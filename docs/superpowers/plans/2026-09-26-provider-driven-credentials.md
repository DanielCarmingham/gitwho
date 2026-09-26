# Provider-driven credentials Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace per-account `env`/`gitCredential` declarations and gitwho's secret store with a built-in provider table and tokens read on demand from `gh` and `tea`.

**Architecture:** A new `provider` module holds, per provider, the variables its tools read. `sources::token` fetches an account's token from its provider's CLI (`gh auth token --user`, or a guarded `tea login helper get`). `exec`, the credential helper and `doctor` all derive from provider + token, so `accounts.toml` says only what an account *is*. Phase A switches every consumer over; phase B deletes the now-unreachable store.

**Tech Stack:** Rust 2021 (MSRV 1.88), clap 4, serde + toml 0.8, serde_json, thiserror; tests use tempfile and fake `gh`/`tea` shell scripts.

**Spec:** `docs/superpowers/specs/2026-09-26-provider-driven-credentials-design.md`

## Global Constraints

- `rust-version = "1.88"` stays; nothing may need a newer toolchain.
- Version control is **jj**, never `git commit`. Each task ends with `jj describe -m "…"` then `jj new`. Do not push, do not move `main`.
- Every task ends green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- **Never print a token value** — not in output, errors, test failure messages or debug logs. Fingerprints only (`fingerprint()`), R10.
- **Nothing personal in the repo.** Fixtures use `example.com` / `example.net` / `acme-*` and placeholder logins.
- Wrong-and-quiet is a bug (R8): no fallback to another source, another account, or ambient variables.
- Comments: default to none. Only *why* the code cannot say; no narration, no changelog comments. Public items keep their `///` docs (project convention).
- Platform differences are parameters, never `cfg!` branches the tests cannot reach (see CLAUDE.md).
- `cargo build` can leave `target/debug/gitwho` stale: before trusting a manual run, compare `stat -f "%Sm" target/debug/gitwho` with the source, and `cargo clean -p gitwho` if it lags.
- Provider values: `github` | `gitea` | `forgejo` (alias of `gitea`), exact lowercase.
- Provider table, verbatim:

  | Provider | Variable | Value |
  |---|---|---|
  | github | `GH_TOKEN` | token |
  | github | `GITHUB_PERSONAL_ACCESS_TOKEN` | token |
  | gitea | `GITEA_TOKEN` | token |
  | gitea | `GITEA_INSTANCE_URL` | url |
  | gitea | `GITEA_ACCESS_TOKEN` | token |
  | gitea | `GITEA_HOST` | url |

- Always cleared by `exec`: every variable above plus `GITHUB_TOKEN`, `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`.
- gh invocation, exact argument order: `gh auth token --hostname github.com --user <login>`.
- tea invocations, exact: `tea login ls -o json`, then `tea login helper get` with stdin `protocol=https\nhost=<host>\n\n`.

## Review Focus

1. **An account `url` with a trailing slash or different letter case** (`https://Git.Example.net/`) against tea's stored `https://git.example.net` — a person expects them to match. Pinned in Task 2.
2. **A server on a non-default port or under a sub-path** (`https://git.example.net:3000/gitea`) — the helper request must carry `host=git.example.net:3000`, not the path. Pinned in Task 2.
3. **`tea login ls -o json` printing something that is not a JSON list** (an older tea, or an error on stdout) — expected: a clear error naming the command, never a panic. Pinned in Task 2.
4. **`tea login helper get` succeeding with no `password=` line** — expected: refused as an empty token, not an empty string handed to git. Pinned in Task 2.
5. **`provider = "GitHub"` (capitalised)** — expected: a parse error listing the valid values, not a silent default. Pinned in Task 1.

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `src/provider.rs` (new) | Provider enum, variable table, always-cleared set | 1 |
| `src/sources.rs` | Running CLIs (`Runner`), `token()` from gh/tea, later `fingerprint()` | 2, 3, 6 |
| `src/config.rs` | New schema, removed-field errors, per-provider validation | 3 |
| `src/exec.rs` | Env plan from provider table + token | 3 |
| `src/credential.rs` | Helper answers with account `login` + token | 3 |
| `src/sync.rs` | Credential sections for every account's hosts | 3 |
| `src/doctor.rs` | Token checks, ambient check, later store-free permissions | 3, 6 |
| `src/discover.rs` | Proposes the new schema | 4 |
| `src/main.rs` | Wiring; `secret`/`renew` removed; later store code removed | 3, 5, 6 |
| `src/secrets/` | Deleted | 6 |
| `tests/common/mod.rs` (new) | Fake `gh`/`tea` for binary-level tests | 3 |
| `tests/provider.rs`, `tests/token.rs` (new) | Unit-level tests for tasks 1–2 | 1, 2 |
| docs, example config, CLAUDE.md, site | Tell the new story | 3, 5, 7 |

---

## Phase A — the provider model

### Task 1: Provider table

**Files:**
- Create: `src/provider.rs`
- Modify: `src/lib.rs`
- Test: `tests/provider.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum Provider { Github, Gitea }` — `Deserialize` from `"github"`, `"gitea"`, `"forgejo"`; `Copy + Eq + Debug`.
  - `impl Provider { pub const ALL: [Provider; 2]; pub fn variables(self) -> &'static [(&'static str, Value)]; pub fn cli(self) -> &'static str; pub fn name(self) -> &'static str }`
  - `pub enum Value { Token, Url }`
  - `pub const FALLBACKS: &[&str]`
  - `pub fn always_cleared() -> std::collections::BTreeSet<&'static str>`

- [ ] **Step 1: Write the failing tests** — create `tests/provider.rs`:

```rust
use gitwho::provider::{always_cleared, Provider, Value};
use serde::Deserialize;

#[derive(Deserialize)]
struct Holder {
    provider: Provider,
}

fn parse(value: &str) -> Result<Provider, toml::de::Error> {
    toml::from_str::<Holder>(&format!("provider = {value:?}")).map(|h| h.provider)
}

#[test]
fn github_hands_the_token_to_gh_and_to_github_mcp_server() {
    assert_eq!(
        Provider::Github.variables(),
        &[
            ("GH_TOKEN", Value::Token),
            ("GITHUB_PERSONAL_ACCESS_TOKEN", Value::Token),
        ]
    );
}

/// tea reads GITEA_TOKEN + GITEA_INSTANCE_URL; gitea-mcp reads
/// GITEA_ACCESS_TOKEN + GITEA_HOST. One account serves both from one token.
#[test]
fn gitea_hands_token_and_url_to_tea_and_to_gitea_mcp() {
    assert_eq!(
        Provider::Gitea.variables(),
        &[
            ("GITEA_TOKEN", Value::Token),
            ("GITEA_INSTANCE_URL", Value::Url),
            ("GITEA_ACCESS_TOKEN", Value::Token),
            ("GITEA_HOST", Value::Url),
        ]
    );
}

#[test]
fn forgejo_is_accepted_as_gitea() {
    assert_eq!(parse("forgejo").unwrap(), Provider::Gitea);
    assert_eq!(parse("gitea").unwrap(), Provider::Gitea);
    assert_eq!(parse("github").unwrap(), Provider::Github);
}

#[test]
fn an_unknown_provider_is_rejected_naming_the_valid_ones() {
    let message = parse("gitlab").unwrap_err().to_string();
    assert!(
        message.contains("github") && message.contains("gitea"),
        "{message}"
    );
}

#[test]
fn a_capitalised_provider_is_rejected_rather_than_guessed() {
    assert!(parse("GitHub").is_err());
}

#[test]
fn the_cli_that_holds_each_providers_token() {
    assert_eq!(Provider::Github.cli(), "gh");
    assert_eq!(Provider::Gitea.cli(), "tea");
}

/// Fallbacks are read by tools but never set by gitwho: gh reads GITHUB_TOKEN
/// when GH_TOKEN is empty, so a stray one in the shell would answer for the
/// wrong account unless it is cleared.
#[test]
fn always_cleared_is_every_provider_variable_plus_the_fallbacks() {
    let cleared = always_cleared();
    for provider in Provider::ALL {
        for (name, _) in provider.variables() {
            assert!(cleared.contains(name), "{name} is not cleared");
        }
    }
    for name in ["GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"] {
        assert!(cleared.contains(name), "{name} is not cleared");
    }
    assert_eq!(cleared.len(), 9);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test provider`
Expected: compile error, `could not find provider in gitwho`.

- [ ] **Step 3: Implement** — create `src/provider.rs`:

```rust
//! Which variables each provider's tools read.
//!
//! Facts about `gh`, `tea` and their MCP servers, kept here rather than
//! restated per account: restating them is how `GITEA_HOST` came to be
//! documented for a tool that never reads it.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    #[serde(alias = "forgejo")]
    Gitea,
}

/// What a variable is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Token,
    Url,
}

/// Read by these tools but never set by gitwho, so cleared all the same.
pub const FALLBACKS: &[&str] = &[
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Github, Provider::Gitea];

    /// Every variable this provider's tools read, and what it holds.
    pub fn variables(self) -> &'static [(&'static str, Value)] {
        match self {
            Provider::Github => &[
                ("GH_TOKEN", Value::Token),
                ("GITHUB_PERSONAL_ACCESS_TOKEN", Value::Token),
            ],
            Provider::Gitea => &[
                ("GITEA_TOKEN", Value::Token),
                ("GITEA_INSTANCE_URL", Value::Url),
                ("GITEA_ACCESS_TOKEN", Value::Token),
                ("GITEA_HOST", Value::Url),
            ],
        }
    }

    /// The CLI that holds this provider's tokens.
    pub fn cli(self) -> &'static str {
        match self {
            Provider::Github => "gh",
            Provider::Gitea => "tea",
        }
    }

    /// The name as written in `accounts.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Github => "github",
            Provider::Gitea => "gitea",
        }
    }
}

/// Every variable `exec` clears before setting any, whatever the config says.
pub fn always_cleared() -> BTreeSet<&'static str> {
    Provider::ALL
        .iter()
        .flat_map(|provider| provider.variables().iter().map(|(name, _)| *name))
        .chain(FALLBACKS.iter().copied())
        .collect()
}
```

In `src/lib.rs` add `pub mod provider;` in alphabetical position (after `pub mod paths;`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --test provider`
Expected: 7 passed.

- [ ] **Step 5: Full check and commit**

```bash
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
jj describe -m "Add the provider table: which variables each provider's tools read"
jj new
```

---

### Task 2: Read a token from gh or tea

**Files:**
- Modify: `src/sources.rs` (additive: `Runner::run_input`, `MapRunner::calls`, `TokenOwner`, `TokenError`, `token`)
- Test: `tests/token.rs`

**Interfaces:**
- Consumes: `gitwho::provider::Provider` (Task 1).
- Produces:
  - `pub trait Runner { fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Option<Captured>>; fn run_input(&self, program: &str, args: &[&str], input: &str) -> std::io::Result<Option<Captured>>; }`
  - `impl MapRunner { pub fn calls(&self) -> Vec<(String, Option<String>)> }` — every invocation as `(MapRunner::key(program, args), stdin)`.
  - `pub struct TokenOwner<'a> { pub account: &'a str, pub provider: Provider, pub login: &'a str, pub url: Option<&'a str> }` (`Debug, Clone, Copy`)
  - `pub enum TokenError` (thiserror; every message starts with the account name and never contains a token)
  - `pub fn token(runner: &dyn Runner, owner: &TokenOwner) -> Result<String, TokenError>`

The existing `fetch`, `value_for`, `SourceError`, `ValueError`, `KNOWN_SOURCES` stay untouched in this task (Task 3 deletes them).

- [ ] **Step 1: Write the failing tests** — create `tests/token.rs`:

```rust
//! Reading an account's token from its provider's own CLI.
//!
//! tea behaviour encoded here was measured against tea 0.15.1 on 2026-09-26
//! with fake logins: `login helper get` answers with the first login for a
//! host, ignoring the username asked for and the login marked default.

use std::collections::HashMap;

use gitwho::provider::Provider;
use gitwho::sources::{token, Captured, MapRunner, TokenOwner};

const TOKEN: &str = "tok_do_not_print";

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn refused(stderr: &str) -> Captured {
    Captured {
        success: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

fn github(login: &str) -> TokenOwner<'_> {
    TokenOwner {
        account: "Work",
        provider: Provider::Github,
        login,
        url: None,
    }
}

fn gitea<'a>(login: &'a str, url: &'a str) -> TokenOwner<'a> {
    TokenOwner {
        account: "SelfHosted",
        provider: Provider::Gitea,
        login,
        url: Some(url),
    }
}

fn gh_replying(login: &str, reply: Captured) -> MapRunner {
    MapRunner::new(HashMap::from([(
        MapRunner::key(
            "gh",
            &["auth", "token", "--hostname", "github.com", "--user", login],
        ),
        reply,
    )]))
}

/// tea listing `logins` as (name, url, user); its helper hands out TOKEN.
fn tea_listing(logins: &[(&str, &str, &str)]) -> MapRunner {
    let listed: Vec<_> = logins
        .iter()
        .map(|(name, url, user)| {
            serde_json::json!({
                "name": name, "url": url, "ssh_host": "x", "user": user, "default": "false"
            })
        })
        .collect();
    tea_replying(ok(&serde_json::to_string(&listed).unwrap()), ok(&format!(
        "protocol=https\nhost=git.example.net\nusername=you\npassword={TOKEN}\n"
    )))
}

fn tea_replying(ls: Captured, helper: Captured) -> MapRunner {
    MapRunner::new(HashMap::from([
        (MapRunner::key("tea", &["login", "ls", "-o", "json"]), ls),
        (MapRunner::key("tea", &["login", "helper", "get"]), helper),
    ]))
}

fn asked_tea_for_a_token(runner: &MapRunner) -> bool {
    runner
        .calls()
        .iter()
        .any(|(key, _)| key == "tea login helper get")
}

#[test]
fn github_asks_gh_for_that_login_on_github_com() {
    let runner = gh_replying("work-login", ok(&format!("{TOKEN}\n")));
    assert_eq!(token(&runner, &github("work-login")).unwrap(), TOKEN);
}

#[test]
fn a_login_gh_does_not_hold_says_what_to_run() {
    let runner = gh_replying(
        "work-login",
        refused("no oauth token found for github.com account work-login"),
    );
    let message = token(&runner, &github("work-login")).unwrap_err().to_string();
    assert!(message.starts_with("Work"), "{message}");
    assert!(message.contains("work-login"), "{message}");
    assert!(message.contains("gh auth login --hostname github.com"), "{message}");
}

#[test]
fn gh_not_being_installed_is_its_own_error() {
    let runner = MapRunner::new(HashMap::new()).without("gh");
    let message = token(&runner, &github("work-login")).unwrap_err().to_string();
    assert!(message.contains("gh is not installed"), "{message}");
}

#[test]
fn an_empty_answer_from_gh_is_refused() {
    let runner = gh_replying("work-login", ok("\n"));
    let message = token(&runner, &github("work-login")).unwrap_err().to_string();
    assert!(message.contains("empty"), "{message}");
}

#[test]
fn tea_with_exactly_the_right_login_hands_over_its_token() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "you")]);
    assert_eq!(
        token(&runner, &gitea("you", "https://git.example.net")).unwrap(),
        TOKEN
    );
    let helper_input = runner
        .calls()
        .into_iter()
        .find(|(key, _)| key == "tea login helper get")
        .and_then(|(_, input)| input);
    assert_eq!(
        helper_input.as_deref(),
        Some("protocol=https\nhost=git.example.net\n\n")
    );
}

#[test]
fn no_tea_login_for_the_url_says_what_to_run_and_never_asks_for_a_token() {
    let runner = tea_listing(&[("other", "https://elsewhere.example.net", "you")]);
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("tea login add --url https://git.example.net"),
        "{message}"
    );
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

/// tea would hand over the first of these whatever was asked for.
#[test]
fn two_tea_logins_for_one_url_are_refused_rather_than_guessed_between() {
    let runner = tea_listing(&[
        ("alice", "https://git.example.net", "alice"),
        ("bob", "https://git.example.net", "bob"),
    ]);
    let message = token(&runner, &gitea("bob", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("alice") && message.contains("bob"), "{message}");
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

#[test]
fn a_tea_login_for_a_different_user_is_refused_naming_both() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "someone-else")]);
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("someone-else") && message.contains("\"you\""),
        "{message}"
    );
    assert!(!asked_tea_for_a_token(&runner));
    assert!(!message.contains(TOKEN));
}

#[test]
fn a_trailing_slash_or_different_case_in_the_url_still_matches() {
    let runner = tea_listing(&[("acme", "https://git.example.net", "you")]);
    assert_eq!(
        token(&runner, &gitea("you", "https://Git.Example.net/")).unwrap(),
        TOKEN
    );
}

#[test]
fn a_port_is_kept_and_a_sub_path_dropped_in_the_helper_request() {
    let url = "https://git.example.net:3000/gitea";
    let runner = tea_listing(&[("acme", url, "you")]);
    token(&runner, &gitea("you", url)).unwrap();
    let helper_input = runner
        .calls()
        .into_iter()
        .find(|(key, _)| key == "tea login helper get")
        .and_then(|(_, input)| input);
    assert_eq!(
        helper_input.as_deref(),
        Some("protocol=https\nhost=git.example.net:3000\n\n")
    );
}

#[test]
fn a_tea_listing_that_is_not_json_is_an_error_not_a_panic() {
    let runner = tea_replying(ok("Name  URL\nacme  https://git.example.net\n"), ok(""));
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("tea login ls -o json"), "{message}");
}

#[test]
fn a_helper_answer_with_no_password_line_is_refused_as_empty() {
    let runner = tea_replying(
        ok(r#"[{"name":"acme","url":"https://git.example.net","user":"you"}]"#),
        ok("protocol=https\nhost=git.example.net\n"),
    );
    let message = token(&runner, &gitea("you", "https://git.example.net"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("empty"), "{message}");
}

#[test]
fn a_gitea_owner_without_a_url_is_an_error() {
    let owner = TokenOwner {
        account: "SelfHosted",
        provider: Provider::Gitea,
        login: "you",
        url: None,
    };
    let message = token(&MapRunner::new(HashMap::new()), &owner)
        .unwrap_err()
        .to_string();
    assert!(message.contains("url"), "{message}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test token`
Expected: compile error, `no function or associated item named token` / `TokenOwner` not found.

- [ ] **Step 3: Extend `Runner` and both runners** in `src/sources.rs`.

Add to the `Runner` trait, after `run`:

```rust
    /// Run with `input` on stdin, for tools that take their request there.
    fn run_input(&self, program: &str, args: &[&str], input: &str)
        -> std::io::Result<Option<Captured>>;
```

Replace `impl Runner for ProcessRunner { … }` with:

```rust
impl Runner for ProcessRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Option<Captured>> {
        self.spawn(program, args, None)
    }

    fn run_input(
        &self,
        program: &str,
        args: &[&str],
        input: &str,
    ) -> std::io::Result<Option<Captured>> {
        self.spawn(program, args, Some(input))
    }
}

impl ProcessRunner {
    fn spawn(
        &self,
        program: &str,
        args: &[&str],
        input: Option<&str>,
    ) -> std::io::Result<Option<Captured>> {
        use std::io::Write;
        use std::process::Stdio;

        let Some(resolved) = self.resolve(program) else {
            return Ok(None);
        };

        let mut command = std::process::Command::new(&resolved);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if let Some(input) = input {
            child
                .stdin
                .take()
                .expect("stdin was piped")
                .write_all(input.as_bytes())?;
        }
        let output = child.wait_with_output()?;

        Ok(Some(Captured {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }))
    }
}
```

In `MapRunner`: add the field `calls: std::cell::RefCell<Vec<(String, Option<String>)>>,`, initialise it with `calls: Default::default(),` in `new`, add

```rust
    /// Every invocation so far, as its key and what it was given on stdin.
    pub fn calls(&self) -> Vec<(String, Option<String>)> {
        self.calls.borrow().clone()
    }
```

and replace `impl Runner for MapRunner` with a shared `answer` method:

```rust
impl MapRunner {
    fn answer(
        &self,
        program: &str,
        args: &[&str],
        input: Option<&str>,
    ) -> std::io::Result<Option<Captured>> {
        self.calls
            .borrow_mut()
            .push((Self::key(program, args), input.map(String::from)));

        if self.missing.contains(program) {
            return Ok(None);
        }

        // An unlisted command is a failure rather than a default success: a
        // test that silently got a token it never arranged for would be
        // asserting nothing.
        Ok(Some(match self.replies.get(&Self::key(program, args)) {
            Some(reply) => Captured {
                success: reply.success,
                stdout: reply.stdout.clone(),
                stderr: reply.stderr.clone(),
            },
            None => Captured {
                success: false,
                stdout: String::new(),
                stderr: format!("MapRunner has no reply for {:?}", Self::key(program, args)),
            },
        }))
    }
}

impl Runner for MapRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Option<Captured>> {
        self.answer(program, args, None)
    }

    fn run_input(
        &self,
        program: &str,
        args: &[&str],
        input: &str,
    ) -> std::io::Result<Option<Captured>> {
        self.answer(program, args, Some(input))
    }
}
```

- [ ] **Step 4: Add `TokenOwner`, `TokenError` and `token`** at the end of `src/sources.rs` (add `use crate::provider::Provider;` to the imports):

```rust
/// Whose token to fetch: an account, its provider, and the login that
/// provider's CLI holds it under.
#[derive(Debug, Clone, Copy)]
pub struct TokenOwner<'a> {
    pub account: &'a str,
    pub provider: Provider,
    pub login: &'a str,
    pub url: Option<&'a str>,
}

/// Why no token came back. Every message names the account first and says
/// what to run; none ever contains a token.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("{account}: {program} is not installed, and it holds this account's token")]
    NotInstalled { account: String, program: String },
    #[error("{account}: {program} failed: {detail}")]
    Failed {
        account: String,
        program: String,
        detail: String,
    },
    #[error("{account}: gh has no token for login {login}: {detail}; run `gh auth login --hostname github.com`")]
    NoGhLogin {
        account: String,
        login: String,
        detail: String,
    },
    #[error("{account}: {program} returned an empty token for login {login}")]
    Empty {
        account: String,
        program: String,
        login: String,
    },
    #[error("{account}: a gitea account needs `url`, the server's https address")]
    NoUrl { account: String },
    #[error("{account}: `tea login ls -o json` did not return a login list: {detail}")]
    TeaList { account: String, detail: String },
    #[error("{account}: tea has no login for {url}; run `tea login add --url {url}`")]
    NoTeaLogin { account: String, url: String },
    #[error(
        "{account}: tea has {count} logins for {url} ({names}) and cannot be told which to use, \
         so gitwho will not guess; keep one with `tea login delete`"
    )]
    AmbiguousTeaLogin {
        account: String,
        url: String,
        count: usize,
        names: String,
    },
    #[error(
        "{account}: tea's login for {url} is user {found}, but accounts.toml says login = \"{login}\"; \
         fix whichever is wrong"
    )]
    WrongTeaLogin {
        account: String,
        url: String,
        found: String,
        login: String,
    },
    #[error("{account}: tea has no token for {url}: {detail}")]
    NoTeaToken {
        account: String,
        url: String,
        detail: String,
    },
}

/// The account's token, read on demand from its provider's CLI.
///
/// Never falls back: a CLI that cannot answer is an error, because anything
/// else produces a working-but-wrong credential (R8).
pub fn token(runner: &dyn Runner, owner: &TokenOwner) -> Result<String, TokenError> {
    match owner.provider {
        Provider::Github => gh_token(runner, owner),
        Provider::Gitea => tea_token(runner, owner),
    }
}

fn invoke(
    runner: &dyn Runner,
    owner: &TokenOwner,
    program: &str,
    args: &[&str],
    input: Option<&str>,
) -> Result<Captured, TokenError> {
    let ran = match input {
        Some(input) => runner.run_input(program, args, input),
        None => runner.run(program, args),
    };
    ran.map_err(|e| TokenError::Failed {
        account: owner.account.to_string(),
        program: program.to_string(),
        detail: e.to_string(),
    })?
    .ok_or_else(|| TokenError::NotInstalled {
        account: owner.account.to_string(),
        program: program.to_string(),
    })
}

/// `--user` reads that login without `gh auth switch`, so nothing global is
/// mutated (R9), and it wins over an ambient `GH_TOKEN` (gh 2.97.0).
fn gh_token(runner: &dyn Runner, owner: &TokenOwner) -> Result<String, TokenError> {
    let captured = invoke(
        runner,
        owner,
        "gh",
        &["auth", "token", "--hostname", "github.com", "--user", owner.login],
        None,
    )?;
    if !captured.success {
        return Err(TokenError::NoGhLogin {
            account: owner.account.to_string(),
            login: owner.login.to_string(),
            detail: captured.stderr.trim().to_string(),
        });
    }
    non_empty(owner, "gh", captured.stdout.trim())
}

#[derive(serde::Deserialize)]
struct TeaLogin {
    name: String,
    url: String,
    #[serde(default)]
    user: String,
}

/// tea's helper answers with the *first* login for a host whatever user is
/// asked for (tea 0.15.1), so it is only asked once the listing proves there
/// is exactly one candidate and it is the right user.
fn tea_token(runner: &dyn Runner, owner: &TokenOwner) -> Result<String, TokenError> {
    let account = owner.account.to_string();
    let url = owner.url.ok_or_else(|| TokenError::NoUrl {
        account: account.clone(),
    })?;

    let listed = invoke(runner, owner, "tea", &["login", "ls", "-o", "json"], None)?;
    if !listed.success {
        return Err(TokenError::TeaList {
            account,
            detail: listed.stderr.trim().to_string(),
        });
    }
    let logins: Vec<TeaLogin> =
        serde_json::from_str(&listed.stdout).map_err(|e| TokenError::TeaList {
            account: account.clone(),
            detail: e.to_string(),
        })?;

    let matching: Vec<&TeaLogin> = logins.iter().filter(|l| same_url(&l.url, url)).collect();
    match matching.as_slice() {
        [] => {
            return Err(TokenError::NoTeaLogin {
                account,
                url: url.to_string(),
            })
        }
        [only] if only.user == owner.login => {}
        [only] => {
            return Err(TokenError::WrongTeaLogin {
                account,
                url: url.to_string(),
                found: only.user.clone(),
                login: owner.login.to_string(),
            })
        }
        many => {
            return Err(TokenError::AmbiguousTeaLogin {
                account,
                url: url.to_string(),
                count: many.len(),
                names: many
                    .iter()
                    .map(|l| l.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
        }
    }

    let request = format!("protocol=https\nhost={}\n\n", host_of(url));
    let answer = invoke(
        runner,
        owner,
        "tea",
        &["login", "helper", "get"],
        Some(&request),
    )?;
    if !answer.success {
        return Err(TokenError::NoTeaToken {
            account,
            url: url.to_string(),
            detail: answer.stderr.trim().to_string(),
        });
    }
    let password = answer
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("password="))
        .unwrap_or("")
        .trim();
    non_empty(owner, "tea", password)
}

fn non_empty(owner: &TokenOwner, program: &str, value: &str) -> Result<String, TokenError> {
    if value.is_empty() {
        return Err(TokenError::Empty {
            account: owner.account.to_string(),
            program: program.to_string(),
            login: owner.login.to_string(),
        });
    }
    Ok(value.to_string())
}

fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

/// The `host[:port]` git's credential protocol names a server by.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest)
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test --test token`
Expected: 13 passed. Then `cargo test` — every pre-existing test still passes (nothing else changed behaviour).

- [ ] **Step 6: Check and commit**

```bash
cargo clippy --all-targets -- -D warnings && cargo fmt --check
jj describe -m "Read an account's token from gh, or from tea behind a one-login guard"
jj new
```

---

### Task 3: Switch every consumer to the provider model

This is the one atomic step: the schema change breaks every caller at once, so they all move together. `secret` and `renew` are removed here because they only existed to write the store. The store *code* stays until Task 6, reachable only from `init` and `doctor`'s permission report.

**Files:**
- Modify: `src/config.rs` (rewrite), `src/exec.rs` (rewrite), `src/credential.rs`, `src/sync.rs:66-88`, `src/doctor.rs`, `src/sources.rs` (delete old `fetch` API), `src/main.rs`
- Modify: `docs/accounts.toml.example` (rewrite; it is `init::TEMPLATE`)
- Create: `tests/common/mod.rs`
- Rewrite: `tests/exec.rs`, `tests/credential.rs`
- Modify: `tests/config.rs`, `tests/doctor.rs`, `tests/whoami.rs`, `tests/cli.rs`, `tests/exec_passthrough.rs`, `tests/init.rs`, `tests/sync.rs`, `tests/sources.rs`, and fixtures in `tests/identity_rules.rs`, `tests/resolve.rs`, `tests/repo.rs`
- Delete: `tests/secret_cli.rs`, `tests/renew.rs`, `tests/renew_from_gh.rs`

**Interfaces:**
- Consumes: `Provider`, `Value`, `always_cleared` (Task 1); `TokenOwner`, `TokenError`, `token`, `Runner::run_input`, `MapRunner::calls` (Task 2).
- Produces:
  - `config::Account { name, provider: Provider, login: String, url: Option<String>, email, ssh_key, git_name, match_patterns, paths }` and `Account::token_owner(&self) -> TokenOwner<'_>`
  - `config::Defaults { account, git_name }` (no `secret_backend`)
  - `config::ConfigError::{Parse, Read, Removed { place, field, instead }, Invalid { account, problem }}`
  - `exec::plan_env(runner: &dyn Runner, account: &Account) -> Result<EnvPlan, TokenError>`; `exec::plan_cleared() -> EnvPlan`
  - `credential::respond(config: &Config, runner: &dyn Runner, request: &Request, cwd: Option<&Path>) -> Result<Credential, CredentialError>`; `CredentialError::{Unresolved, LowConfidence, Token(TokenError)}`
  - `doctor::run(config: &Config, runner: &dyn Runner, ambient_env: &BTreeMap<String, String>, git: &GitWiring, store: &Store) -> Vec<Finding>`
  - `tests/common/mod.rs`: `FakeTools::new()`, `.path() -> String`, `.gh_login(login, token)`, `.tea_login(name, url, user, token)`

- [ ] **Step 1: Convert every test fixture mechanically.** Save as a scratch script (outside the repo) and run it once:

```python
#!/usr/bin/env python3
"""One-off: rewrite accounts.toml fixtures in test files to the provider schema."""
import pathlib, re, sys

def convert(block: str) -> str:
    def first(pattern):
        for line in block.split("\n"):
            m = re.match(pattern, line)
            if m:
                return m
        return None
    name = first(r'\s*name = "([^"]+)"')
    provider = first(r'\s*provider = "([^"]+)"')
    host = first(r'\s*match = \["([^/"]+)')
    out = []
    for line in block.split("\n"):
        if re.match(r'\s*(gitCredential|env) = ', line):
            continue
        out.append(line)
        m = re.match(r'(\s*)provider = "', line)
        if m and name:
            out.append(f'{m.group(1)}login = "{name.group(1).lower()}"')
            if provider and provider.group(1) == "gitea" and host:
                out.append(f'{m.group(1)}url = "https://{host.group(1)}"')
    return "\n".join(out)

for path in map(pathlib.Path, sys.argv[1:]):
    text = path.read_text()
    head, *blocks = text.split("[[accounts]]")
    path.write_text(head + "".join("[[accounts]]" + convert(b) for b in blocks))
```

Run: `python3 <scratch>/convert_fixtures.py tests/config.rs tests/cli.rs tests/exec_passthrough.rs tests/doctor.rs tests/init.rs tests/identity_rules.rs tests/resolve.rs tests/repo.rs tests/sync.rs tests/whoami.rs`
Then: `jj diff --stat` — only those files change; skim `jj diff tests/resolve.rs` to confirm each account gained one `login` line (and gitea ones a `url`) and lost `gitCredential`/`env`.

Delete the files whose subject is gone: `jj file untrack` is not needed — just `rm tests/secret_cli.rs tests/renew.rs tests/renew_from_gh.rs`.

- [ ] **Step 2: Create `tests/common/mod.rs`:**

```rust
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
```

- [ ] **Step 3: Write the new config tests.** In `tests/config.rs`: in `the_shipped_example_config_parses`, delete the `for account in &config.accounts { for spec in &account.env { … } }` loop (there is no `env` any more) and instead assert the example exercises both providers:

```rust
    use gitwho::provider::Provider;
    assert!(config.accounts.iter().any(|a| a.provider == Provider::Github));
    assert!(config.accounts.iter().any(|a| a.provider == Provider::Gitea));
```

Append:

```rust
fn parse_err(toml: &str) -> String {
    Config::parse(toml).unwrap_err().to_string()
}

const HEAD: &str = "[defaults]\naccount = \"A\"\n\n[[accounts]]\nname = \"A\"\nemail = \"a@example.com\"\n";

#[test]
fn each_removed_field_is_named_with_what_replaced_it() {
    let env = parse_err(&format!("{HEAD}provider = \"github\"\nlogin = \"a\"\nenv = [\"GH_TOKEN\"]\n"));
    assert!(env.contains("account A") && env.contains("`env`") && env.contains("login"), "{env}");

    let cred = parse_err(&format!("{HEAD}provider = \"github\"\nlogin = \"a\"\ngitCredential = \"GH_TOKEN\"\n"));
    assert!(cred.contains("`gitCredential`"), "{cred}");

    let backend = parse_err(
        "[defaults]\naccount = \"A\"\nsecretBackend = \"age\"\n\n[[accounts]]\nname = \"A\"\nprovider = \"github\"\nlogin = \"a\"\nemail = \"a@example.com\"\n",
    );
    assert!(backend.contains("[defaults]") && backend.contains("`secretBackend`"), "{backend}");
}

#[test]
fn gitea_without_a_url_is_rejected() {
    let message = parse_err(&format!("{HEAD}provider = \"gitea\"\nlogin = \"a\"\n"));
    assert!(message.contains("account A") && message.contains("url"), "{message}");
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
fn a_forgejo_account_parses_as_gitea() {
    let config = Config::parse(&format!(
        "{HEAD}provider = \"forgejo\"\nlogin = \"a\"\nurl = \"https://git.example.net\"\n"
    ))
    .unwrap();
    assert_eq!(config.accounts[0].provider, gitwho::provider::Provider::Gitea);
    assert_eq!(config.accounts[0].url.as_deref(), Some("https://git.example.net"));
}
```

- [ ] **Step 4: Rewrite `tests/exec.rs` entirely:**

```rust
use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::exec::{plan_cleared, plan_env};
use gitwho::sources::{Captured, MapRunner};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "SelfHosted"
    provider = "gitea"
    login = "you"
    url = "https://git.example.net"
    email = "you@example.net"
    match = ["git.example.net/**"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured {
        success: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn tools() -> MapRunner {
    MapRunner::new(HashMap::from([
        (
            MapRunner::key("gh", &["auth", "token", "--hostname", "github.com", "--user", "personal"]),
            ok("personal-token\n"),
        ),
        (
            MapRunner::key("tea", &["login", "ls", "-o", "json"]),
            ok(r#"[{"name":"acme","url":"https://git.example.net","user":"you"}]"#),
        ),
        (
            MapRunner::key("tea", &["login", "helper", "get"]),
            ok("protocol=https\nhost=git.example.net\nusername=you\npassword=gitea-token\n"),
        ),
    ]))
}

fn set_of(plan: &gitwho::exec::EnvPlan) -> Vec<(&str, &str)> {
    plan.set.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn a_github_account_gets_its_token_under_every_name_githubs_tools_read() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("Personal").unwrap()).unwrap();
    assert_eq!(
        set_of(&plan),
        [
            ("GH_TOKEN", "personal-token"),
            ("GITHUB_PERSONAL_ACCESS_TOKEN", "personal-token"),
        ]
    );
}

#[test]
fn a_gitea_account_gets_token_and_url_under_teas_and_gitea_mcps_names() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("SelfHosted").unwrap()).unwrap();
    assert_eq!(
        set_of(&plan),
        [
            ("GITEA_ACCESS_TOKEN", "gitea-token"),
            ("GITEA_HOST", "https://git.example.net"),
            ("GITEA_INSTANCE_URL", "https://git.example.net"),
            ("GITEA_TOKEN", "gitea-token"),
        ]
    );
}

/// Entering a Gitea repo must not leave a GitHub token reachable, including
/// the fallbacks gh reads when GH_TOKEN is empty (R11).
#[test]
fn every_provider_variable_and_fallback_is_cleared_whichever_account_runs() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let plan = plan_env(&tools(), config.account("SelfHosted").unwrap()).unwrap();
    for var in ["GH_TOKEN", "GITHUB_TOKEN", "GITHUB_PERSONAL_ACCESS_TOKEN", "GH_ENTERPRISE_TOKEN"] {
        assert!(plan.remove.contains(var), "{var} not cleared: {:?}", plan.remove);
        assert!(!plan.set.contains_key(var), "{var} handed to a gitea account");
    }
}

/// Running anyway would leave the CLI to authenticate as whatever it could
/// find, which is the silent-wrong-account failure (R8).
#[test]
fn a_token_the_cli_cannot_supply_refuses_rather_than_running_with_a_gap() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let message = plan_env(&MapRunner::new(HashMap::new()), config.account("Personal").unwrap())
        .unwrap_err()
        .to_string();
    assert!(message.starts_with("Personal"), "{message}");
}

#[test]
fn a_command_that_establishes_credentials_gets_everything_cleared_and_nothing_set() {
    let plan = plan_cleared();
    assert!(plan.set.is_empty());
    for var in ["GH_TOKEN", "GITEA_TOKEN", "GITHUB_TOKEN"] {
        assert!(plan.remove.contains(var), "{var} not cleared");
    }
}
```

- [ ] **Step 5: Rewrite `tests/credential.rs`.** Replace the imports, `ACCOUNTS`, `backend()` and `no_sources()`:

```rust
use std::collections::HashMap;

use gitwho::config::Config;
use gitwho::credential::{respond, Request};
use gitwho::sources::{Captured, MapRunner};

const ACCOUNTS: &str = r#"
    [defaults]
    account = "Personal"

    [[accounts]]
    name = "Personal"
    provider = "github"
    login = "personal-login"
    email = "me@example.com"
    match = ["github.com/Personal/**"]

    [[accounts]]
    name = "Work"
    provider = "github"
    login = "work-login"
    email = "me@work.example"
    match = ["github.com/WorkOrg/**"]
"#;

fn ok(stdout: &str) -> Captured {
    Captured { success: true, stdout: stdout.to_string(), stderr: String::new() }
}

fn gh_key(login: &str) -> String {
    MapRunner::key("gh", &["auth", "token", "--hostname", "github.com", "--user", login])
}

fn tools() -> MapRunner {
    MapRunner::new(HashMap::from([
        (gh_key("personal-login"), ok("personal-token")),
        (gh_key("work-login"), ok("work-token")),
    ]))
}

fn only_personal() -> MapRunner {
    MapRunner::new(HashMap::from([(gh_key("personal-login"), ok("personal-token"))]))
}
```

Then in every remaining test: `respond(&config, &backend(), &no_sources(), …)` becomes `respond(&config, &tools(), …)`; in `a_missing_secret_fails_instead_of_falling_back_to_another_account` use `&only_personal()` and change the name assertion to `message.contains("Work") && message.contains("work-login")`, renaming the test `a_login_gh_lacks_fails_instead_of_falling_back_to_another_account`. Delete `SSH_ACCOUNT` and `an_ssh_account_is_never_handed_a_token` (every account now names a login; ssh pushes never reach the helper). Add:

```rust
/// Forgejo checks the username against the token's owner; GitHub ignores it.
#[test]
fn the_username_is_the_accounts_login_not_its_gitwho_name() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let request = Request::parse("protocol=https\nhost=github.com\npath=WorkOrg/thing.git\n");
    let credential = respond(&config, &tools(), &request, None).unwrap();
    assert_eq!(credential.username, "work-login");
    assert_eq!(credential.password, "work-token");
}
```

- [ ] **Step 6: Run the library tests to see them fail**

Run: `cargo test --test config --test exec --test credential`
Expected: compile errors (`login`/`url`/`provider` fields, `plan_env` arity, `respond` arity).

- [ ] **Step 7: Rewrite `src/config.rs`:**

```rust
//! Parsing of `accounts.toml` -- the single place an account is declared.
//!
//! It names logins and patterns and never a secret, so it can be committed to
//! a dotfiles repo (R10).

use serde::Deserialize;

use crate::provider::Provider;
use crate::sources::TokenOwner;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("accounts.toml is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("cannot read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{place} uses `{field}`, which gitwho no longer reads: {instead}. See docs/accounts.toml.example")]
    Removed {
        place: String,
        field: &'static str,
        instead: &'static str,
    },
    #[error("account {account}: {problem}")]
    Invalid { account: String, problem: &'static str },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub defaults: Defaults,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// The account used when nothing else matches. Declared explicitly so the
    /// fallback is stated rather than emergent (R4).
    pub account: String,
    /// The author name for every account that does not override it.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: Provider,
    /// The login this account's token is held under by its provider's CLI.
    /// Not `user`: beside `gitName` and `email` that would read as git's
    /// `user.name`, which it has nothing to do with.
    pub login: String,
    /// The server's https address. Required for gitea; github means github.com.
    #[serde(default)]
    pub url: Option<String>,
    pub email: String,
    /// Written into `core.sshcommand` for remotes that use ssh.
    #[serde(rename = "sshKey", default)]
    pub ssh_key: Option<String>,
    /// Author name, when this account differs from `defaults.gitName`.
    #[serde(rename = "gitName", default)]
    pub git_name: Option<String>,
    /// Glob patterns matched against `host/path` of a remote URL.
    #[serde(rename = "match", default)]
    pub match_patterns: Vec<String>,
    /// Directory prefixes consulted **only** for a repo with no remote yet.
    #[serde(default)]
    pub paths: Vec<String>,
}

const REMOVED_FROM_ACCOUNTS: &[(&str, &str)] = &[
    (
        "env",
        "the provider now decides which variables are set; delete it and set `login` (and `url` for gitea)",
    ),
    (
        "gitCredential",
        "the git password is now the token from the account's provider CLI; delete it",
    ),
];

const REMOVED_FROM_DEFAULTS: &[(&str, &str)] =
    &[("secretBackend", "gitwho stores no secrets any more; delete it")];

impl Account {
    /// Who to ask for this account's token.
    pub fn token_owner(&self) -> TokenOwner<'_> {
        TokenOwner {
            account: &self.name,
            provider: self.provider,
            login: &self.login,
            url: self.url.as_deref(),
        }
    }
}

impl Config {
    pub fn parse(toml_str: &str) -> Result<Self, ConfigError> {
        // Parsed twice on purpose: deny_unknown_fields would report a removed
        // field as merely unknown, without saying what replaced it.
        let table: toml::Table = toml::from_str(toml_str)?;
        reject_removed(&table)?;
        let config: Config = toml::from_str(toml_str)?;
        config.validate()?;
        Ok(config)
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name == name)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for account in &self.accounts {
            let problem = match (account.provider, &account.url) {
                (Provider::Gitea, None) => "a gitea account needs `url`, the server's https address",
                (Provider::Github, Some(_)) => "github means github.com, so `url` is not allowed",
                _ => continue,
            };
            return Err(ConfigError::Invalid {
                account: account.name.clone(),
                problem,
            });
        }
        Ok(())
    }
}

fn reject_removed(table: &toml::Table) -> Result<(), ConfigError> {
    if let Some(defaults) = table.get("defaults").and_then(toml::Value::as_table) {
        for (field, instead) in REMOVED_FROM_DEFAULTS {
            if defaults.contains_key(*field) {
                return Err(ConfigError::Removed {
                    place: "[defaults]".to_string(),
                    field,
                    instead,
                });
            }
        }
    }
    let accounts = table
        .get("accounts")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table);
    for account in accounts {
        for (field, instead) in REMOVED_FROM_ACCOUNTS {
            if account.contains_key(*field) {
                let name = account.get("name").and_then(toml::Value::as_str).unwrap_or("?");
                return Err(ConfigError::Removed {
                    place: format!("account {name}"),
                    field,
                    instead,
                });
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 8: Rewrite `src/exec.rs`:**

```rust
//! Running a command with exactly one account's credentials.
//!
//! CLIs and MCP servers cannot be reached by a credential helper -- they read
//! environment variables. Rather than exporting those into the shell, where
//! every unrelated process inherits them, `exec` injects them into the single
//! process that needs them.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::Account;
use crate::provider::{always_cleared, Value};
use crate::sources::{self, Runner, TokenError};

/// The environment changes to apply before running a command.
#[derive(Debug, Default)]
pub struct EnvPlan {
    /// Variables to set, with their values.
    pub set: BTreeMap<String, String>,
    /// Variables to unset before setting anything.
    pub remove: BTreeSet<String>,
}

/// For a command that must not be handed a credential: clear, set nothing.
/// Clearing still happens, so a token the shell exported cannot reach a tool
/// that would then authenticate as it (R11).
pub fn plan_cleared() -> EnvPlan {
    EnvPlan {
        remove: cleared(),
        set: BTreeMap::new(),
    }
}

/// The account's token and url under every name its provider's tools read.
pub fn plan_env(runner: &dyn Runner, account: &Account) -> Result<EnvPlan, TokenError> {
    let token = sources::token(runner, &account.token_owner())?;
    let url = account.url.clone().unwrap_or_default();

    let set = account
        .provider
        .variables()
        .iter()
        .map(|(name, value)| {
            let value = match value {
                Value::Token => token.clone(),
                Value::Url => url.clone(),
            };
            (name.to_string(), value)
        })
        .collect();

    Ok(EnvPlan {
        remove: cleared(),
        set,
    })
}

fn cleared() -> BTreeSet<String> {
    always_cleared().into_iter().map(String::from).collect()
}
```

- [ ] **Step 9: Update `src/credential.rs`.** Replace the imports `use crate::secrets::Backend;` and `use crate::sources::{self, Runner};` with `use crate::sources::{self, Runner, TokenError};`. Replace the error enum with:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("cannot tell which account owns this request: {0}")]
    Unresolved(#[from] resolve::ResolveError),
    #[error(
        "nothing identified an account for this request; it would only fall back to {account}"
    )]
    LowConfidence { account: String },
    #[error(transparent)]
    Token(#[from] TokenError),
}
```

and `respond` with:

```rust
pub fn respond(
    config: &Config,
    runner: &dyn Runner,
    request: &Request,
    cwd: Option<&Path>,
) -> Result<Credential, CredentialError> {
    let account = choose_account(config, request, cwd)?;
    let password = sources::token(runner, &account.token_owner())?;
    Ok(Credential {
        username: account.login.clone(),
        password,
    })
}
```

- [ ] **Step 10: Update `src/sync.rs` `served_hosts`.** Delete the `if account.git_credential.is_none() { continue; }` block and replace the function's doc comment with:

```rust
/// The hosts gitwho serves credentials for, in declaration order, deduplicated.
///
/// Every account has a token now, so every host an account claims is served.
/// A hostname only ever used over ssh is harmless here: git consults a
/// credential helper for https alone.
```

In `tests/sync.rs`, delete `an_account_with_no_git_credential_claims_no_host` and its doc comment (the case no longer exists).

- [ ] **Step 11: Delete the old source API** from `src/sources.rs`: `use crate::config::{Account, SourcedVar};`, `use crate::secrets::Backend;`, `SourceError`, `fetch`, `KNOWN_SOURCES`, `value_for`, `ValueError`, `fetch_gh`, `describe_who`. Rewrite the module doc to:

```rust
//! Reading an account's token from the CLI that already holds it.
//!
//! There is no second copy: gh and tea keep the token, and gitwho asks for it
//! per invocation. A copy goes stale the moment the original is rotated, and a
//! stale credential is present, plausible and wrong -- the failure R8 refuses.
//!
//! Costs a process spawn or two: `gh auth token` ~60 ms (gh 2.97.0);
//! `tea login ls -o json` 14 ms and `tea login helper get` 17 ms median
//! (tea 0.15.1, fake plaintext logins).
//!
//! Nothing here ever logs a token. Diagnostics quote stderr, never stdout.
```

In `tests/sources.rs` keep only `a_program_is_never_resolved_from_our_own_shim_directory`, `an_absolute_program_path_is_passed_through`, `a_program_that_is_nowhere_on_the_path_resolves_to_nothing` and whatever helpers they use; delete every other test, `ACCOUNTS`, and imports of `Config`, `EnvBackend`, `fetch`, `value_for`, `SourceError`, `ValueError`.

- [ ] **Step 12: Update `src/doctor.rs`.**

Imports: remove `Backend` from `use crate::secrets::…` (keep `fingerprint`, `BackendKind`, `Choice` for now), keep `use crate::sources::{self, Runner};`, and add `use crate::provider::always_cleared;`.

`run` loses its `backend` parameter and calls the new checks:

```rust
pub fn run(
    config: &Config,
    runner: &dyn Runner,
    ambient_env: &BTreeMap<String, String>,
    git: &GitWiring,
    store: &Store,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // First: if accounts.toml is writable by someone else, nothing the later
    // checks report about its contents can be trusted.
    check_permissions(store, &mut findings);
    check_storage(store, &mut findings);
    check_config(config, &mut findings);
    check_tokens(config, runner, &mut findings);
    check_ambient(ambient_env, &mut findings);
    check_git_wiring(git, &mut findings);
    check_repo_identity(config, git, &mut findings);

    findings
}
```

In `check_config`, delete the whole block that begins `// tea builds a login from the environment only when both are set.` (the GITEA_TOKEN/GITEA_INSTANCE_URL pair loop).

Replace `check_secrets` with:

```rust
/// The one check that runs other programs: a perfect config is no use if the
/// CLI it points at has lost the login. A token the server has revoked still
/// looks healthy here -- telling those apart needs the network.
fn check_tokens(config: &Config, runner: &dyn Runner, findings: &mut Vec<Finding>) {
    for account in &config.accounts {
        match sources::token(runner, &account.token_owner()) {
            Ok(token) => findings.push(Finding::new(
                Level::Ok,
                "tokens",
                format!(
                    "{}: {} login {} {}",
                    account.name,
                    account.provider.cli(),
                    account.login,
                    fingerprint(&token)
                ),
            )),
            Err(e) => findings.push(Finding::new(Level::Problem, "tokens", e.to_string())),
        }
    }
}
```

Replace `check_ambient` with:

```rust
/// A cleared variable sitting in the environment is the condition this
/// project exists to remove: every process launched from that shell inherits
/// it, including ones belonging to a different account.
fn check_ambient(env: &BTreeMap<String, String>, findings: &mut Vec<Finding>) {
    for var in always_cleared() {
        if env.contains_key(var) {
            findings.push(Finding::new(
                Level::Warn,
                "ambient",
                // The name only; printing the value would leak the very thing
                // being complained about.
                format!("{var} is set in the environment; every process launched from this shell inherits it"),
            ));
        }
    }
}
```

- [ ] **Step 13: Update `tests/doctor.rs`.**

Imports: drop `Backend`, `EnvBackend`; keep `BackendKind, Choice, Source`; import `gitwho::sources::{Captured, MapRunner}` if not already.

Replace `stocked_backend()` with:

```rust
fn ok(stdout: &str) -> Captured {
    Captured { success: true, stdout: stdout.to_string(), stderr: String::new() }
}

/// gh and tea both holding the logins `ACCOUNTS` names.
fn stocked_runner() -> MapRunner {
    MapRunner::new(HashMap::from([
        (
            MapRunner::key("gh", &["auth", "token", "--hostname", "github.com", "--user", "personal"]),
            ok("personal-token"),
        ),
        (
            MapRunner::key("tea", &["login", "ls", "-o", "json"]),
            ok(r#"[{"name":"sh","url":"https://ssh.git.example.net","user":"selfhosted"}]"#),
        ),
        (
            MapRunner::key("tea", &["login", "helper", "get"]),
            ok("password=gitea-token\n"),
        ),
    ]))
}
```

`run_with(config, backend, ambient, wiring)` becomes `run_with(config: &Config, runner: &dyn gitwho::sources::Runner, ambient, wiring)` and calls `doctor::run(config, runner, ambient, wiring, &store_at(dir.path()))`. Replace every `&stocked_backend()` argument with `&stocked_runner()`.

Delete these tests and helpers (their subject is gone): `a_declared_secret_with_no_stored_value_is_a_problem`, `WITH_A_SOURCE`, `gh_holding`, `run_with_sources`, `a_referenced_variable_is_not_reported_as_missing_from_the_store`, `a_referenced_variable_is_reported_with_its_source_and_a_fingerprint`, `a_reference_the_tool_cannot_answer_is_a_problem`, `a_referenced_variable_sitting_in_the_environment_still_warns`, `tea_config`, `problem_messages`, `a_gitea_token_without_an_instance_url_is_a_problem`, `a_gitea_instance_url_without_a_token_is_a_problem`, `gitea_mcp_variables_beside_teas_are_not_a_problem`. Keep `no_sources()`.

Add:

```rust
#[test]
fn every_accounts_token_is_reported_as_a_fingerprint_never_a_value() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let findings = run_with(&config, &stocked_runner(), &BTreeMap::new(), &healthy_wiring());
    let tokens: Vec<_> = findings.iter().filter(|f| f.check == "tokens").collect();
    assert_eq!(tokens.len(), 2, "{findings:#?}");
    assert!(tokens.iter().all(|f| f.level == Level::Ok), "{tokens:#?}");
    for finding in tokens {
        assert!(
            !finding.message.contains("personal-token") && !finding.message.contains("gitea-token"),
            "doctor printed a token: {}",
            finding.message
        );
    }
}

#[test]
fn a_login_the_cli_does_not_hold_is_a_problem_that_says_what_to_run() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let findings = run_with(&config, &no_sources(), &BTreeMap::new(), &healthy_wiring());
    let messages: Vec<_> = problems(&findings).iter().map(|f| f.message.clone()).collect();
    assert!(
        messages.iter().any(|m| m.contains("Personal") && m.contains("gh auth login")),
        "{messages:?}"
    );
}

/// gh reads GITHUB_TOKEN when GH_TOKEN is empty, though no account names it.
#[test]
fn a_fallback_variable_in_the_environment_is_reported() {
    let config = Config::parse(ACCOUNTS).unwrap();
    let ambient = BTreeMap::from([("GITHUB_TOKEN".to_string(), "leaked".to_string())]);
    let findings = run_with(&config, &stocked_runner(), &ambient, &healthy_wiring());
    let warned = findings
        .iter()
        .find(|f| f.level == Level::Warn && f.message.contains("GITHUB_TOKEN"))
        .expect("an ambient fallback variable should be reported");
    assert!(!warned.message.contains("leaked"), "{}", warned.message);
}
```

- [ ] **Step 14: Update `src/main.rs`.**

1. Delete `Command::Renew`, `Command::Secret`, `enum SecretAction`, their `match` arms in `main`, and the functions `secret`, `account_here`, `set_target`, `read_value`, `renew`, `enum Offer`, `from_gh`, `gh_login`, `read_line`, `renewal_command`. Keep `secret_init`, `open_backend`, `choose_backend`, `secrets_path`, `identity_path` if the compiler still reports them used (`init` and `doctor_report` use some until Task 6); delete any it reports unused.
2. `doctor_report`: replace the `open_backend(config.defaults.secret_backend…)` pair with `let choice = choose_backend(None)?;`, and the call with `gitwho::doctor::run(&config, &runner(), &ambient, &wiring, &store)`.
3. `exec`: delete the `open_backend` lines; `return run_with(program, args, &plan_cleared());`; `let plan = plan_env(&runner(), account).map_err(|e| e.to_string())?;`
4. `credential_get`: delete the `open_backend` lines; `respond(&config, &runner(), &request, cwd.as_deref())`.
5. `credential`: replace the comment above `"store" | "erase"` with:

```rust
        // Deliberately no-ops. The provider's CLI is the source of truth;
        // letting git cache a copy elsewhere would put the same token in a
        // second place with different access rules (R11).
```

6. `whoami`: replace the `match &account.git_credential { … }` block with

```rust
    let cli = account.provider.cli();
    match account.url.as_deref() {
        Some(url) => println!("{:<12}{cli} login {} at {url}", "token", account.login),
        None => println!("{:<12}{cli} login {}", "token", account.login),
    }
```

and replace the trailing `if !account.env.is_empty() { … }` block with

```rust
    println!();
    println!("{:<30}VALUE FROM", "VARIABLE");
    for (name, value) in account.provider.variables() {
        let from = match value {
            gitwho::provider::Value::Token => format!("{cli} login {}", account.login),
            gitwho::provider::Value::Url => account.url.clone().unwrap_or_default(),
        };
        println!("{name:<30}{from}");
    }
```

7. `init`: replace the step-2 instruction line `println!("  2. gitwho secret set <Account> <VAR>    once per token");` with `println!("  2. log gh (or tea) in as each account's `login`");`.
8. Remove imports the compiler reports unused.

- [ ] **Step 15: Rewrite `docs/accounts.toml.example`** — keep the header's warning about `0600` and the redirect vector, update the claim about secrets, and replace the three accounts:

```toml
# Starting point for ~/.config/gitwho/accounts.toml.
#
# Replace every value below. The three accounts are archetypes, not defaults:
# a personal account, a work account told apart by organisation rather than by
# host, and a self-hosted Gitea/Forgejo account that uses ssh and https at once.
#
# Contains no secrets, and never needs any. Each account names the login its
# provider's CLI holds (`gh` or `tea`); gitwho asks that CLI for the token when
# it is needed. That is what lets this file be committed to a dotfiles repo.
#
# Copy it into place with mode 0600:
#
#     mkdir -p ~/.config/gitwho && chmod 700 ~/.config/gitwho
#     cp docs/accounts.toml.example ~/.config/gitwho/accounts.toml
#     chmod 600 ~/.config/gitwho/accounts.toml
#
# The chmod is not hygiene, it is the point. This file is a redirect vector:
# whoever can write it can add a `match` pattern for a host they control and be
# handed one of your tokens.

[defaults]
# Used when nothing else matches. A repo that matches no account resolves here
# and is tagged `Unmatched`: `gh` works with it while the credential helper
# refuses, because a wrong account served quietly is the failure this tool
# exists to prevent.
account = "Personal"

# The author name for every account that does not set its own `gitName`.
gitName = "Your Name"


# --- A personal account ------------------------------------------------------

[[accounts]]
name = "Personal"
# `github` or `gitea` (`forgejo` means the same). The provider decides which
# variables `exec` sets -- GH_TOKEN and GITHUB_PERSONAL_ACCESS_TOKEN here -- so
# no variable is ever named in this file.
provider = "github"
# The gh login this account's token is held under: `gh auth token --user`.
login = "your-username"
email = "you@example.com"
# Written into `core.sshcommand` for ssh remotes. Optional.
sshKey = "~/.ssh/id_ed25519_personal"
# Matched against `host/path` of the remote URL, so identity follows the
# repository rather than where it sits on disk.
match = ["github.com/your-username/**"]
# Consulted only for a repo with no remote yet -- a fresh `git init`.
paths = ["~/src/personal/"]


# --- A work account on the same host -----------------------------------------
#
# Two accounts on github.com, told apart by organisation. gh holds both logins
# at once and `--user` picks one without `gh auth switch`, so nothing global
# changes.

[[accounts]]
name = "Work"
provider = "github"
login = "your-work-username"
email = "you@example-corp.com"
gitName = "Your Name (Example Corp)"
match = [
    "github.com/example-corp/**",
    "github.com/example-corp-labs/**",
]
paths = ["~/src/work/"]


# --- A self-hosted Gitea / Forgejo account -----------------------------------
#
# Two hostnames, one account: ssh for push/pull on one, the API over https on
# the other. Transport is a property of a remote, so that needs no special
# case. `exec` sets GITEA_TOKEN + GITEA_INSTANCE_URL for tea and
# GITEA_ACCESS_TOKEN + GITEA_HOST for gitea-mcp, all from this one account.
#
# tea hands out the first login it holds for a server whatever user is asked
# for, so gitwho requires exactly one tea login for `url`, belonging to `login`.
# Two accounts on one Gitea server are therefore not supported.

[[accounts]]
name = "SelfHosted"
provider = "gitea"
url = "https://git.example.net"
login = "you"
email = "you@example.net"
sshKey = "~/.ssh/id_ed25519_selfhosted"
match = [
    "git.example.net/**",
    "ssh.git.example.net/**",
]
paths = ["~/src/selfhosted/"]
```

- [ ] **Step 16: Update the binary-level tests to use `FakeTools`.** Add `mod common;` and `use common::FakeTools;` at the top of each file below.

`tests/whoami.rs` — fixture accounts after Step 1 have logins `personal`, `work`, `selfhosted` and `url = "https://git.example.net"`. whoami never runs gh/tea, so no fakes are needed. Replace `names_the_credential_variable_and_where_each_value_lives` with:

```rust
#[test]
fn names_every_variable_exec_sets_and_where_each_value_comes_from() {
    let config = tempfile::tempdir().unwrap();
    setup(config.path());
    let repo = tempfile::tempdir().unwrap();
    repo_with_origin(repo.path(), "https://git.example.net/someone/site.git");

    let out = whoami(config.path(), repo.path(), &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();

    for name in ["GITEA_TOKEN", "GITEA_INSTANCE_URL", "GITEA_ACCESS_TOKEN", "GITEA_HOST"] {
        assert!(stdout.contains(name), "{name} missing:\n{stdout}");
    }
    assert!(stdout.contains("tea login selfhosted"), "{stdout}");
    assert!(stdout.contains("https://git.example.net"), "{stdout}");
}
```

and change the next test (`says_when_a_value_is_read_from_another_tool_rather_than_stored`) to assert `stdout.contains("gh login work")` in place of its old source/`user` assertions, renaming it `says_which_gh_login_supplies_a_github_token`. Leave the Step-1-converted fixture alone.

`tests/cli.rs` — replace `fixture(dir)` so it writes `ACCOUNTS` and returns fakes holding both logins, and make every command that spawns gitwho use `fakes.path()`:

```rust
fn fixture(dir: &Path) -> FakeTools {
    std::fs::write(dir.join("accounts.toml"), ACCOUNTS).unwrap();
    let fakes = FakeTools::new();
    fakes.gh_login("personal", "personal-token");
    fakes.gh_login("work", "work-token");
    fakes
}
```

(Check the converted `ACCOUNTS` in `tests/cli.rs`: its accounts must be named so their lowercased names are `personal` and `work`; adjust the two `gh_login` calls to whatever logins Step 1 produced.) Give `git_credential_fill_in` a `path: &str` parameter and `.env("PATH", path)`; callers pass `&fakes.path()` from `let fakes = fixture(dir.path());`. In `a_non_unicode_variable_elsewhere_in_the_environment_is_ignored`, keep `let fakes = fixture(&store);` and add `.env("PATH", fakes.path())`. Replace `exec_fixture` with:

```rust
fn exec_fixture(dir: &Path) -> FakeTools {
    std::fs::write(dir.join("accounts.toml"), EXEC_ACCOUNTS).unwrap();
    let fakes = FakeTools::new();
    fakes.gh_login("personal", "personal-token");
    fakes.tea_login("sh", "https://ssh.git.example.net", "selfhosted", "gitea-token");
    fakes
}
```

and add `.env("PATH", fakes.path())` to each exec test's `Command`. `exec_scrubs_a_hostile_token_inherited_from_the_parent_shell` keeps its assertions (`GITEA_TOKEN=gitea-token` is still injected). Delete the `use gitwho::secrets::…` import.

`tests/exec_passthrough.rs` — delete `FAKE_GH`, the bin-dir script writing, `fixture.run(&["secret", "init"], None)`, `store()`, and the `GITWHO_SECRETS`/`GITWHO_IDENTITY` lines. `Fixture` gains `fakes: FakeTools`, created in `new()` with `fakes.gh_login("personal", "personal-token")` (BrandNew deliberately has no gh login); `command()` sets `.env("PATH", self.fakes.path())`. The fake gh prints `GH_TOKEN: set|unset` exactly as the old one did, so assertions stand.

`tests/init.rs` — delete `store_secret`. In `a_completed_setup_re_runs_without_duplicating_anything`, the written config becomes

```toml
[[accounts]]
name = "Personal"
provider = "github"
login = "someone"
email = "you@example.com"
match = ["github.com/someone/**"]
```

delete the `store_secret(…)` call and its comment, create `let fakes = FakeTools::new(); fakes.gh_login("someone", "fake-token");`, and give `gitwho()` a `path: &str` parameter passed to `.env("PATH", path)` (other callers pass `&std::env::var("PATH").unwrap_or_default()`). Update any assertion on the printed step-2 text to the new wording.

- [ ] **Step 17: Run everything**

Run: `cargo test`
Expected: all pass. If a test fails on a login name, the fixture's converted `login` and the fake's `gh_login` disagree — align them rather than weakening the assertion.

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: clean.

- [ ] **Step 18: Check the binary by hand** (fresh binary first, see Global Constraints):

```bash
cargo build && ./target/debug/gitwho exec --help | tail -5
GITWHO_CONFIG=docs/accounts.toml.example ./target/debug/gitwho whoami
```

Expected: `whoami` prints the `token` line and the variable table (resolution in this repo falls to the default account).

- [ ] **Step 19: Commit**

```bash
jj describe -m "Drive credentials from the provider; tokens come from gh and tea

accounts.toml now says what an account is -- provider, login, and url for
gitea -- and no longer names variables. exec sets every variable the
provider's tools read and always clears the same fixed set. The credential
helper sends the account's login as the username. env, gitCredential and
secretBackend fail with an error naming their replacement. The secret and
renew commands are gone; the store they wrote is unused."
jj new
```

---

### Task 4: `init --discover` proposes the new schema

**Files:**
- Modify: `src/discover.rs` (`render`, `provider_for`, delete `credential_var`, add `tea_logins`)
- Modify: `src/main.rs` (`discover_config`)
- Test: `tests/discover.rs`

**Interfaces:**
- Consumes: `Runner` (Task 2), `Provider` (Task 1).
- Produces: `pub fn tea_logins(runner: &dyn Runner) -> Vec<(String, String)>` — `(url, user)` pairs, never a token; `pub fn render(scan: &Scan, roots: &[PathBuf], logins: &Logins, tea: &[(String, String)]) -> String`.

- [ ] **Step 1: Write the failing tests** — append to `tests/discover.rs` (add `Org, Scan` to its `use gitwho::discover::{…}` line):

```rust
/// One org with `repos` repositories, enough to be proposed at 2 or more.
fn scan_of(host: &str, org: &str, repos: usize) -> Scan {
    Scan {
        orgs: vec![Org {
            host: host.to_string(),
            org: org.to_string(),
            repos: (0..repos).map(|i| format!("repo{i}")).collect(),
        }],
        ..Default::default()
    }
}

#[test]
fn a_proposed_account_parses_once_its_placeholders_are_filled() {
    let scan = scan_of("github.com", "acme-corp", 3);
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &[])
        .replace("REPLACE-ME", "acme-corp");
    let config = gitwho::config::Config::parse(&text).unwrap();
    assert_eq!(config.accounts[0].login, "acme-corp");
}

#[test]
fn a_gitea_org_is_proposed_with_a_url_and_the_tea_logins_for_it() {
    let scan = scan_of("git.example.net", "acme", 3);
    let tea = vec![("https://git.example.net".to_string(), "you".to_string())];
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &tea);
    assert!(text.contains("provider = \"gitea\""), "{text}");
    assert!(text.contains("url = \"https://git.example.net\""), "{text}");
    assert!(text.contains("tea holds: you"), "{text}");
    assert!(!text.contains("env ="), "{text}");
    assert!(!text.contains("gitCredential"), "{text}");
}

#[test]
fn an_unsupported_provider_is_proposed_commented_out() {
    let scan = scan_of("gitlab.com", "acme", 3);
    let text = gitwho::discover::render(&scan, &[], &Default::default(), &[]);
    assert!(text.contains("# gitwho does not support gitlab.com"), "{text}");
    assert!(!text.contains("\n[[accounts]]\nname = \"acme\""), "{text}");
}
```

Update the existing `render(…)` call in the `rendered` helper (`tests/discover.rs:265`) to pass `&[]` as the fourth argument, and change any assertion expecting `gitCredential`/`env =` lines to expect `login = "REPLACE-ME"` instead.

- [ ] **Step 2: Run to verify failure** — `cargo test --test discover` → compile error on `render` arity.

- [ ] **Step 3: Implement.** In `src/discover.rs`: change `provider_for` to return `Option<&'static str>` — `Some("github")` when the host contains `github`, `None` when it contains `gitlab`, `azure` or `visualstudio`, else `Some("gitea")` — and update its doc to say gitlab/azure are recognised only to be left out. Delete `credential_var`. Add:

```rust
/// Read `tea login ls -o json` for the logins it holds, as (url, user).
///
/// Never reads a token -- this is `init` output a user pipes into a file.
pub fn tea_logins(runner: &dyn crate::sources::Runner) -> Vec<(String, String)> {
    #[derive(serde::Deserialize)]
    struct Listed {
        url: String,
        #[serde(default)]
        user: String,
    }
    let Ok(Some(captured)) = runner.run("tea", &["login", "ls", "-o", "json"]) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Listed>>(&captured.stdout)
        .map(|listed| listed.into_iter().map(|l| (l.url, l.user)).collect())
        .unwrap_or_default()
}
```

In `render`, add the parameter `tea: &[(String, String)]` and replace the per-org body (from `let provider = provider_for(&org.host);` through the final `env = [...]` line) with:

```rust
        let Some(provider) = provider_for(&org.host) else {
            out.push_str(&format!(
                "# gitwho does not support {}; {}/{} is left out.\n\n",
                org.host, org.host, org.org
            ));
            continue;
        };

        out.push_str(&format!(
            "# --- {}/{} {}\n",
            org.host,
            org.org,
            "-".repeat(58usize.saturating_sub(org.host.len() + org.org.len()))
        ));
        out.push_str(&format!(
            "# {} repositories: {}\n",
            org.repos.len(),
            summarise(&org.repos)
        ));

        let url = format!("https://{}", org.host);
        let held: Vec<&str> = if provider == "github" {
            logins
                .by_host
                .get(&org.host)
                .map(|names| names.iter().map(String::as_str).collect())
                .unwrap_or_default()
        } else {
            tea.iter()
                .filter(|(u, _)| u.trim_end_matches('/').eq_ignore_ascii_case(&url))
                .map(|(_, user)| user.as_str())
                .collect()
        };
        if !held.is_empty() {
            let cli = if provider == "github" { "gh" } else { "tea" };
            out.push_str(&format!(
                "# {cli} holds: {}. Which one owns this org is the one thing discovery\n\
                 # cannot know, so set `login` to it yourself.\n",
                held.join(", ")
            ));
        }

        out.push_str("[[accounts]]\n");
        out.push_str(&format!("name = \"{}\"\n", org.org));
        out.push_str(&format!("provider = \"{provider}\"\n"));
        if provider == "gitea" {
            out.push_str(&format!(
                "# The API address; check it, an ssh hostname often differs.\nurl = \"{url}\"\n"
            ));
        }
        out.push_str("login = \"REPLACE-ME\"\n");
        out.push_str("email = \"REPLACE-ME\"\n");
        out.push_str(&format!("match = [\"{}\"]\n\n", org.pattern()));
```

In `src/main.rs` `discover_config`: `let tea = gitwho::discover::tea_logins(&runner());` and `render(&scan, roots, &logins, &tea)`.

- [ ] **Step 4: Run** — `cargo test --test discover` passes; then `cargo test`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
jj describe -m "init --discover proposes provider, login and url, offering the logins gh and tea hold"
jj new
```

---

### Task 5: Document the provider model

**Files:**
- Modify: `src/main.rs` (`EXEC_EXAMPLES`), `README.md`, `docs/INSTALL.md`, `docs/DESIGN.md`, `CLAUDE.md`, `site/src/components/FlowDiagram.astro`

No code behaviour changes; the check is that nothing still describes `env`, `gitCredential`, `secret set` or `renew` as current.

- [ ] **Step 1: Find every stale reference**

Run: `grep -rn "gitCredential\|env = \[\|secret set\|secret init\|gitwho renew\|secretBackend\|from = \"gh\"" README.md docs CLAUDE.md src site/src --include=*.md --include=*.rs --include=*.astro --include=*.example`
Expected: a list to work through; every hit is rewritten or removed below.

- [ ] **Step 2: `EXEC_EXAMPLES` in `src/main.rs`** — replace the tea paragraph's last sentence (`It authenticates from the account's GITEA_TOKEN and GITEA_INSTANCE_URL, and silently ignores GITEA_HOST.`) with `It authenticates as the account's tea login, which exec hands it as GITEA_TOKEN and GITEA_INSTANCE_URL.` Verify with `cargo build && ./target/debug/gitwho exec --help`.

- [ ] **Step 3: README.** The config sample under "The approach" becomes:

```toml
[[accounts]]
name     = "Work"
provider = "github"
login    = "you-at-work"             # the gh login; gitwho asks gh for the token
email    = "you@example-corp.com"
match    = ["github.com/example-corp/**"]
```

and the sentence after it explains that `provider` decides which variables `exec` sets. In "Quick start", replace the `gitwho secret set …` block with "log `gh`/`tea` in as each account's `login` (`gh auth login`, `tea login add --url …`)". In "Commands", delete the `renew` and `secret …` lines. In "Starting a new repository", replace the `GITEA_TOKEN`/`GITEA_INSTANCE_URL`/`doctor reports…` paragraph with: "`exec` hands tea the account's token and URL under the names it reads; the account needs `provider = "gitea"`, `url` and `login`." Keep the Security section's stored-values paragraphs for Task 7.

- [ ] **Step 4: INSTALL.md.** Rewrite "Skipping step 2 for accounts `gh` already knows" and "Renewing a token later" into one section, "Logging the CLIs in", covering `gh auth login --hostname github.com` per GitHub login and `tea login add --url <url>` per Gitea server, the one-tea-login-per-server rule, and `doctor` reporting the exact command when a login is missing. Update "The config" sample to the new fields. Replace the two bullets added on 2026-09-25 under "Things that will bite you" (new repo; tea's variable) with: new repo bullet kept but without the variable detail, plus "**Two accounts on one Gitea server are refused.** tea cannot be told which login to use, so gitwho will not guess."

- [ ] **Step 5: DESIGN.md.**
  - "Providers differ in mechanism" section: replace the "Each CLI reads its own variables" bullet with the provider table from Global Constraints, the sources for each row, and the tea measurements (0.15.1: env login needs both variables and overrides the stored login; `GITEA_HOST` ignored; helper returns the first login per host; `login ls` never prints tokens; 14 ms / 17 ms).
  - Rewrite **R6** to: "Adding a provider is a row in `provider.rs` plus its token source, with a test — not configuration. Asking users to restate which variables a tool reads is how `GITEA_HOST` came to be documented for a tool that never reads it."
  - Rewrite **R7**: pushing over ssh never involves a token; every account still names a CLI login, and `doctor` reports a missing one.
  - Replace the "Known limits" bullet `provider is required by the parser and read by nothing` with: "A token the server has revoked but the CLI still hands out looks healthy to `doctor`; detecting it needs the network (`doctor --check-remote`, planned)." Add: "Two accounts on one Gitea server are unsupported (tea picks the first login per host)."
  - Delete the section on referencing a token instead of storing it (`from = "gh"`), folding its surviving facts (per-invocation read, no second copy, no fallback) into the provider section.

- [ ] **Step 6: CLAUDE.md** — Domain model: the `tea` line already names `GITEA_INSTANCE_URL`; replace the paragraph starting "The declaration schema is in" with: "The schema is in docs/accounts.toml.example. An account names its `provider`, its CLI `login` and (gitea) `url`; `src/provider.rs` decides which variables that means. Tokens are read from `gh`/`tea` on demand." Update the test count in "Checks before calling anything done" to what `cargo test` reports.

- [ ] **Step 7: Site diagram** `site/src/components/FlowDiagram.astro` — the `ENV` rows: `GH_TOKEN` set; `GITEA_TOKEN` and `GITEA_INSTANCE_URL` cleared. Replace the comment above `ENV` with: "What `exec` clears is a fixed set (`always_cleared` in ../src/provider.rs), not whatever the config names; three of its rows are enough to show one set and the rest cleared." Update the `<desc>` sentence "What gets cleared is every variable named by any account in this config" to "What gets cleared is every variable any supported tool reads".

- [ ] **Step 8: Re-run the grep from Step 1** — remaining hits are only in the spec/plan and in DESIGN.md's historical evidence notes. Then `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.

- [ ] **Step 9: Commit**

```bash
jj describe -m "Document provider-driven credentials"
jj new
```

---

## Phase B — delete the store

### Task 6: Remove the secret store

**Files:**
- Delete: `src/secrets/` (all of `mod.rs`, `age_file.rs`, `keychain.rs`, `env.rs`, `select.rs`), `tests/secrets.rs`, `tests/backend_choice.rs`
- Modify: `src/lib.rs`, `src/sources.rs` (gains `fingerprint`), `src/doctor.rs`, `src/main.rs`, `src/init.rs:130-140` (doc comment), `src/shim.rs:281` (doc comment), `Cargo.toml`, `tests/doctor.rs`, every test setting `GITWHO_SECRETS`/`GITWHO_IDENTITY`/`GITWHO_SECRET_BACKEND`

**Interfaces:**
- Consumes: everything from Task 3.
- Produces: `pub fn sources::fingerprint(value: &str) -> String` (moved verbatim); `doctor::Store { dir: PathBuf, config: PathBuf, owner: u32 }`.

- [ ] **Step 1: Write the failing doctor test** — in `tests/doctor.rs` append:

```rust
/// Left behind by 0.2: harmless, but nothing reads them now, and they still
/// hold every token they ever stored.
#[test]
fn a_leftover_secret_store_is_reported_as_unused_and_never_deleted() {
    let dir = hardened_store();
    std::fs::write(dir.path().join("secrets.age"), "x").unwrap();
    std::fs::write(dir.path().join("identity.key"), "x").unwrap();
    let config = Config::parse(ACCOUNTS).unwrap();

    let findings = doctor::run(
        &config,
        &stocked_runner(),
        &BTreeMap::new(),
        &healthy_wiring(),
        &store_at(dir.path()),
    );

    for file in ["secrets.age", "identity.key"] {
        assert!(
            findings
                .iter()
                .any(|f| f.level == Level::Warn && f.message.contains(file) && f.message.contains("no longer used")),
            "{file} not reported: {findings:#?}"
        );
        assert!(dir.path().join(file).exists(), "{file} was deleted");
    }
}
```

and change `store_at` to build `Store { dir: dir.to_path_buf(), config: dir.join("accounts.toml"), owner: doctor::current_uid() }`; `hardened_store` creates only the dir (0700) and `accounts.toml` (0600). Delete the tests whose subject is gone: `a_readable_identity_key_is_a_problem`, `a_readable_secrets_file_is_a_problem`, `a_store_with_no_secrets_file_yet_is_not_a_permission_problem`, `doctor_names_the_backend_in_effect_and_where_the_choice_came_from`, `doctor_warns_when_owner_only_permissions_cannot_be_enforced`, `a_keychain_store_is_not_warned_about_file_permissions`, and the `BackendKind, Choice, Source` imports.

- [ ] **Step 2: Run to verify failure** — `cargo test --test doctor` → compile error (`Store` fields).

- [ ] **Step 3: Move `fingerprint`** from `src/secrets/mod.rs` to the end of `src/sources.rs`, unchanged including its doc comment. Change `use crate::secrets::{fingerprint, …}` in `src/doctor.rs` to `use crate::sources::fingerprint;`.

- [ ] **Step 4: Rework `doctor.rs` storage checks.** `Store` becomes:

```rust
/// Where the config lives, and who it should belong to.
#[derive(Debug)]
pub struct Store {
    /// Expected `0700`.
    pub dir: PathBuf,
    /// `accounts.toml`, expected `0600`.
    pub config: PathBuf,
    /// The uid both should belong to. Passed in, because chowning to another
    /// user needs root -- varying the expectation is how this is tested.
    pub owner: u32,
}
```

In `check_permissions`, `expected` keeps only the `store.dir` and `store.config` rows. Delete `check_storage` and its call in `run`, and add after `check_permissions`:

```rust
const LEFTOVERS: &[&str] = &["secrets.age", "identity.key"];

/// Files the 0.2 store left behind. Reported, never removed: deleting a user's
/// files is not a doctor's job, and they may want them back.
fn check_leftovers(store: &Store, findings: &mut Vec<Finding>) {
    for name in LEFTOVERS {
        let path = store.dir.join(name);
        if path.exists() {
            findings.push(Finding::new(
                Level::Warn,
                "leftovers",
                format!(
                    "{} is no longer used by gitwho and still holds any token it once stored; delete it",
                    path.display()
                ),
            ));
        }
    }
}
```

Call `check_leftovers(store, &mut findings);` in `run` right after `check_permissions`.

- [ ] **Step 5: Rework `src/main.rs`.** Delete `open_backend`, `choose_backend`, `secrets_path`, `identity_path`, `secret_init` (if still present) and every `gitwho::secrets` import. `doctor_report`'s `Store` becomes `{ dir, config: config_file, owner: gitwho::doctor::current_uid() }`. Replace `init`'s step 1 with:

```rust
    // --- 1. gitwho's own directory -------------------------------------------
    if store.exists() {
        match gitwho::init::ensure_owner_only(&store, write)
            .map_err(|e| format!("cannot check {}: {e}", store.display()))?
        {
            gitwho::init::Mode::AlreadyOwnerOnly | gitwho::init::Mode::NotApplicable => {
                step("ok", format!("{}", store.display()))
            }
            gitwho::init::Mode::Tightened => step(
                "tightened",
                format!("{} to 0700 (was group- or world-readable)", store.display()),
            ),
            gitwho::init::Mode::WouldTighten => {
                step("would fix", format!("{} is not 0700", store.display()))
            }
        }
    } else if write {
        create_owner_only_dir(&store)?;
        step("created", format!("{} (owner-only)", store.display()));
    } else {
        step("would create", format!("{}", store.display()));
    }
```

with

```rust
/// Only the leaf gets 0700; creating `~/.config` with that mode would be
/// gitwho reaching past its own directory.
fn create_owner_only_dir(dir: &Path) -> Result<(), String> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))
}
```

Update the `init` doc comment: "the store must exist before a key can go in it" becomes "gitwho's directory must exist before the config can go in it".

- [ ] **Step 6: Delete the module and its tests.** `rm -r src/secrets tests/secrets.rs tests/backend_choice.rs`; remove `pub mod secrets;` from `src/lib.rs`. Rewrite the two doc comments that cite it: `src/init.rs` (~line 132, about `secrets::age_file` narrowing only its own directory) now says `init` creates the directory 0700 itself and only tightens one it finds looser; `src/shim.rs:281` drops the comparison with `secrets::age_file`.

- [ ] **Step 7: Dependencies.** In `Cargo.toml` delete `age`, `keyring`, `rpassword`. Replace the `rust-version` comment with one stating what still sets it, verified:

Run: `rustup toolchain list | grep 1.88 && cargo +1.88 build --locked`
If 1.88 is installed and the build succeeds, the comment becomes `# Measured: \`cargo +1.88 build --locked\` succeeds (2026-09-26). CI pins this exact version so it cannot drift.` If 1.88 is not installed, run `rustup toolchain install 1.88` first — do not write the comment without the measurement.

- [ ] **Step 8: Strip store variables from tests.** Run `grep -rln "GITWHO_SECRETS\|GITWHO_IDENTITY\|GITWHO_SECRET_BACKEND" tests` and delete each `.env(...)`/`.env_remove(...)` line and array entry naming them.

- [ ] **Step 9: Run everything** — `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; then `grep -rn "secrets::\|AgeFile\|keychain\|rpassword" src tests` → no hits.

- [ ] **Step 10: Commit**

```bash
jj describe -m "Remove the secret store

Tokens come only from gh and tea, so the age file, the keychain backend
and their dependencies had nothing left to do. doctor now reports a
leftover secrets.age / identity.key as unused rather than checking them."
jj new
```

---

### Task 7: Document the store's removal

**Files:**
- Modify: `docs/DESIGN.md`, `README.md`, `docs/INSTALL.md`, `CLAUDE.md`, `SECURITY.md` (if it mentions the store)

- [ ] **Step 1: Find stale text**

Run: `grep -rn -i "age file\|secrets.age\|identity.key\|keychain\|secret store\|the store\|decryption oracle\|secretBackend" README.md docs/*.md CLAUDE.md SECURITY.md`

- [ ] **Step 2: DESIGN.md.** Remove the "age identity" row from the Terminology table and the sentence introducing three meanings (now two: git identity, credential). Rewrite "What this protects, and what it does not": gitwho holds no secret; tokens sit in `gh`'s keychain entry and in tea's `credentials.json.enc` (key in the keychain); anything running as the user can still ask `gh auth token` or `tea login helper get`, exactly as it could ask gitwho before — say so plainly; what gitwho still changes is exposure over time (a token in one process for one invocation). Rewrite **R10** to: "Secrets never enter this repo, and gitwho stores none: `accounts.toml` names logins, the CLIs hold tokens." Delete the R15 note about a regression test pinning store reads, and state R15's budget against the measured gh/tea costs.

- [ ] **Step 3: README Security, INSTALL "What init did", CLAUDE.md** — remove store setup (`secret init`, file modes for `secrets.age`/`identity.key`); keep the `0700` dir / `0600` `accounts.toml` rules and their reason. INSTALL gains a short "Upgrading from 0.2" section: convert `accounts.toml` (fields map: `env`/`gitCredential` → delete; add `login`, and `url` for gitea), log the CLIs in, run `doctor`, then delete `secrets.age` and `identity.key` by hand once `doctor` is clean. CLAUDE.md Status paragraph: "resolver, credential helper, secret storage…" becomes "resolver, credential helper, CLI-sourced tokens…".

- [ ] **Step 4: Re-run the grep** — remaining hits only in the upgrade section and historical evidence. `cargo test` (the example config test still passes).

- [ ] **Step 5: Commit**

```bash
jj describe -m "Document the store's removal and the 0.2 upgrade path"
jj new
```

---

### Task 8: Roll out on the author's machine (with the user, not a subagent)

Not a code task. Each step needs the user present; nothing here deletes a file without their say-so.

- [ ] **Step 1:** Confirm tea's login for the Gitea account is the right one, with no gitwho variables set: `env -u GITEA_TOKEN -u GITEA_INSTANCE_URL -u GITEA_ACCESS_TOKEN -u GITEA_HOST /opt/homebrew/bin/tea whoami`. The user confirms the name.
- [ ] **Step 2:** Show the user the converted `~/.config/gitwho/accounts.toml` as a diff (delete `env`, `gitCredential`; add `login` per account — the GitHub ones from their current `user = …` values; add `url` to the Gitea account) and apply it only on their approval. Keep mode `0600`.
- [ ] **Step 3:** `cargo install --path . --locked`, check `gitwho --version` shows `-dev`, then `gitwho doctor` — expect every account `ok` under `tokens`, and `leftovers` warnings for `secrets.age` / `identity.key`.
- [ ] **Step 3b:** Time the real `tea login helper get`, which decrypts through the keychain unlike the fake logins measured so far, discarding the token: 15 runs of `printf 'protocol=https\nhost=<gitea host>\n\n' | tea login helper get >/dev/null`, median and max. Record the result in DESIGN.md's provider section beside the 14 ms / 17 ms figures; if the median exceeds ~60 ms (gh's cost), tell the user before going further.
- [ ] **Step 4:** Through the shims: `gh api user -q .login` in a repo of each GitHub account; `tea whoami` in the Gitea repo; `git ls-remote` over https for one repo per provider.
- [ ] **Step 5:** Tell the user `secrets.age` and `identity.key` can now be deleted, and let them do it.
