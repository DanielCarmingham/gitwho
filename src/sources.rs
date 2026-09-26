//! Reading a value from a tool that already holds it.
//!
//! The alternative -- copying the token into gitwho's store -- creates a second
//! copy that goes stale the moment the original is rotated. A stale credential
//! is present, decryptable and wrong, which is exactly the failure R8 exists to
//! refuse. A reference cannot drift, because there is only ever one value.
//!
//! This costs a process spawn. Measured against gh 2.97.0 on macOS,
//! `gh auth token` takes ~60 ms against ~10 ms for a store read -- so it is
//! declared per variable rather than switched on globally, and R15's budget
//! still holds for everyone who does not opt in.
//!
//! Nothing here ever logs the value it fetched. Diagnostics quote stderr, never
//! stdout, because stdout is the secret.

use crate::config::{Account, SourcedVar};
use crate::provider::Provider;
use crate::secrets::Backend;

/// What a source refused to do, always naming the account and variable so the
/// message says which declaration to go and look at.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{account}/{var} names source {from:?}, which gitwho does not know")]
    UnknownSource {
        account: String,
        var: String,
        from: String,
    },
    #[error("{account}/{var} is read from {program}, which is not installed")]
    NotInstalled {
        account: String,
        var: String,
        program: String,
    },
    #[error(
        "{account}/{var} is read from {program}, which has no credentials for {who}: {detail}"
    )]
    NoCredentials {
        account: String,
        var: String,
        program: String,
        who: String,
        detail: String,
    },
    #[error("{account}/{var} is read from {program}, which returned an empty value")]
    Empty {
        account: String,
        var: String,
        program: String,
    },
    #[error("{account}/{var} is read from {program}, which failed: {detail}")]
    Failed {
        account: String,
        var: String,
        program: String,
        detail: String,
    },
}

/// What a command produced.
///
/// `stdout` is a secret whenever the command succeeded. Only `stderr` is safe
/// to put in a message.
#[derive(Debug)]
pub struct Captured {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Running an external command.
///
/// A trait rather than a direct `Command` call so the failure paths -- not
/// installed, no such account, empty output -- are reachable from a test on a
/// machine where the real tool is installed and logged in. Same reasoning as
/// `paths::Layout` and `shim::ShimTarget`: a branch nothing in the test suite
/// can reach is a branch that gets documented as working while nothing checks
/// it.
pub trait Runner {
    /// Run and capture. `Ok(None)` means the program is not on `PATH`, which is
    /// a different fact from the program running and refusing.
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Option<Captured>>;

    /// Run with `input` on stdin, for tools that take their request there.
    fn run_input(
        &self,
        program: &str,
        args: &[&str],
        input: &str,
    ) -> std::io::Result<Option<Captured>>;
}

/// Runs commands for real, resolving them past gitwho's own shims.
///
/// That last part is not a refinement, it is the difference between working and
/// looping. gitwho installs a shim directory at the front of `PATH`, and the
/// `gh` shim runs `gitwho exec -- /real/gh` -- so a naive `Command::new("gh")`
/// from inside the credential helper re-enters gitwho, which resolves an
/// account, which asks for a token, which runs `gh`. Found by running this
/// against a real machine rather than by reading the code.
pub struct ProcessRunner {
    /// Directories to skip when resolving a program name, because gitwho put
    /// wrappers in them.
    shim_dirs: Vec<std::path::PathBuf>,
}

impl ProcessRunner {
    /// A runner that resolves names against `PATH` unchanged.
    ///
    /// Only correct where no shim directory exists -- tests, and any caller
    /// that has already resolved an absolute path.
    pub fn unshimmed() -> Self {
        Self {
            shim_dirs: Vec::new(),
        }
    }

    /// A runner that refuses to find a program inside one of gitwho's own shim
    /// directories.
    pub fn skipping(shim_dirs: Vec<std::path::PathBuf>) -> Self {
        Self { shim_dirs }
    }

    fn resolve(&self, program: &str) -> Option<std::path::PathBuf> {
        self.resolve_in(program, std::env::var_os("PATH")?)
    }

    /// The first executable named `program` in `path_var` that is not one of
    /// our own wrappers, or `None` if there is no such thing.
    ///
    /// `path_var` is an argument rather than read from the environment, for the
    /// same reason `shim::install` takes one: a test cannot set `PATH` for
    /// itself without setting it for every other test in the binary.
    ///
    /// An absolute or relative path is passed through untouched -- it was not a
    /// `PATH` lookup in the first place.
    pub fn resolve_in(
        &self,
        program: &str,
        path_var: impl AsRef<std::ffi::OsStr>,
    ) -> Option<std::path::PathBuf> {
        if program.contains(std::path::MAIN_SEPARATOR) {
            return Some(std::path::PathBuf::from(program));
        }

        for dir in std::env::split_paths(path_var.as_ref()) {
            if self.shim_dirs.iter().any(|shim| shim == &dir) {
                continue;
            }
            let candidate = dir.join(program);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    // No mode bits to consult. Being a file is as much as can be checked here,
    // and this branch has never run -- see the note on Windows in CLAUDE.md.
    path.is_file()
}

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

/// A runner with a fixed set of answers, keyed by the whole command line.
///
/// Every failure this module can report -- not installed, no such account,
/// empty output -- is one that cannot be produced on demand by a machine with
/// the real tool installed and logged in. Shipped alongside the real runner for
/// the same reason [`crate::secrets::EnvBackend`] is: a fake that lives in the
/// library is one the library's own contract is tested against.
pub struct MapRunner {
    replies: std::collections::HashMap<String, Captured>,
    /// Programs to report as absent from `PATH`.
    missing: std::collections::HashSet<String>,
    calls: std::cell::RefCell<Vec<(String, Option<String>)>>,
}

impl MapRunner {
    /// Answers keyed by the command as `"gh auth token --user octocat"`.
    pub fn new(replies: std::collections::HashMap<String, Captured>) -> Self {
        Self {
            replies,
            missing: std::collections::HashSet::new(),
            calls: Default::default(),
        }
    }

    /// Report `program` as not installed.
    pub fn without(mut self, program: &str) -> Self {
        self.missing.insert(program.to_string());
        self
    }

    /// The key [`MapRunner`] looks an invocation up by.
    pub fn key(program: &str, args: &[&str]) -> String {
        let mut key = String::from(program);
        for arg in args {
            key.push(' ');
            key.push_str(arg);
        }
        key
    }

    /// Every invocation so far, as its key and what it was given on stdin.
    pub fn calls(&self) -> Vec<(String, Option<String>)> {
        self.calls.borrow().clone()
    }

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

/// Fetch the value `sourced` points at.
///
/// Never falls back. A declared source that cannot answer is an error, because
/// the alternative -- quietly reaching for the store, or for whatever is in the
/// environment -- produces a working-but-wrong credential (R8).
pub fn fetch(
    runner: &dyn Runner,
    account: &str,
    sourced: &SourcedVar,
) -> Result<String, SourceError> {
    match sourced.from.as_str() {
        "gh" => fetch_gh(runner, account, sourced),
        other => Err(SourceError::UnknownSource {
            account: account.to_string(),
            var: sourced.var.clone(),
            from: other.to_string(),
        }),
    }
}

/// The tools this build knows how to read from, for messages and for `doctor`.
pub const KNOWN_SOURCES: &[&str] = &["gh"];

/// The value for `var`, from wherever this account says it lives.
///
/// The one place that decision is made, so `exec` and the credential helper
/// cannot drift apart about it -- which matters because `gitCredential` names a
/// variable, so a referenced token has to serve as git's password and not only
/// as an environment variable.
///
/// `Ok(None)` means the store holds nothing, preserving [`Backend::get`]'s
/// contract that absence is distinguishable from breakage. A *sourced* variable
/// never returns `None`: a declared source that cannot answer is a fault, and
/// reporting it as merely absent would invite the caller to carry on without it.
pub fn value_for(
    backend: &dyn Backend,
    runner: &dyn Runner,
    account: &Account,
    var: &str,
) -> Result<Option<String>, ValueError> {
    match account.source_for(var) {
        Some(sourced) => Ok(Some(fetch(runner, &account.name, sourced)?)),
        None => Ok(backend.get(&account.name, var)?),
    }
}

/// Either half of [`value_for`] failing.
///
/// Deliberately not collapsed into one message: "the store is broken" and "gh
/// does not know that account" call for entirely different fixes.
#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Store(#[from] crate::secrets::SecretError),
}

/// `gh auth token`, for one specific account.
///
/// `--user` is the whole reason this is safe: it reads the named account
/// without `gh auth switch`, so there is no process-wide "active account" to
/// mutate (R9). It also wins over an ambient `GH_TOKEN`, verified against gh
/// 2.97.0 -- so a token already exported into the shell cannot silently answer
/// for a different account.
fn fetch_gh(
    runner: &dyn Runner,
    account: &str,
    sourced: &SourcedVar,
) -> Result<String, SourceError> {
    let mut args = vec!["auth", "token"];

    // Omitted rather than defaulted to github.com: gh resolves its own default
    // host, which on an enterprise machine is not github.com, and second-
    // guessing that would break the case the field exists for.
    if let Some(host) = &sourced.host {
        args.push("--hostname");
        args.push(host);
    }
    if let Some(user) = &sourced.user {
        args.push("--user");
        args.push(user);
    }

    let captured = runner
        .run("gh", &args)
        .map_err(|e| SourceError::Failed {
            account: account.to_string(),
            var: sourced.var.clone(),
            program: "gh".to_string(),
            detail: e.to_string(),
        })?
        .ok_or_else(|| SourceError::NotInstalled {
            account: account.to_string(),
            var: sourced.var.clone(),
            program: "gh".to_string(),
        })?;

    if !captured.success {
        // gh says "no oauth token found for github.com account X" here, which
        // is more useful than anything this could synthesise -- so it is passed
        // through rather than replaced.
        return Err(SourceError::NoCredentials {
            account: account.to_string(),
            var: sourced.var.clone(),
            program: "gh".to_string(),
            who: describe_who(sourced),
            detail: captured.stderr.trim().to_string(),
        });
    }

    // Trimmed for the same reason stored values are: every ordinary way of
    // producing one appends a newline, and a token sent with trailing
    // whitespace is rejected by the server with an error that mentions nothing
    // about whitespace.
    let value = captured.stdout.trim();
    if value.is_empty() {
        return Err(SourceError::Empty {
            account: account.to_string(),
            var: sourced.var.clone(),
            program: "gh".to_string(),
        });
    }

    Ok(value.to_string())
}

/// How to name the account that was asked for, in a message.
fn describe_who(sourced: &SourcedVar) -> String {
    match (&sourced.user, &sourced.host) {
        (Some(user), Some(host)) => format!("{user} on {host}"),
        (Some(user), None) => user.clone(),
        (None, Some(host)) => format!("its active account on {host}"),
        // Worth spelling out: no user means gitwho asked for whatever the tool
        // considers active, which is only ever right by luck on a machine with
        // more than one login.
        (None, None) => "its active account".to_string(),
    }
}

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
        &[
            "auth",
            "token",
            "--hostname",
            "github.com",
            "--user",
            owner.login,
        ],
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
