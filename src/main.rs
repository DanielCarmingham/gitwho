//! CLI entry point.

use std::io::Read;
use std::path::PathBuf;
use std::process::{self, ExitCode};

use clap::{Parser, Subcommand};

use gitfriend::config::Config;
use gitfriend::credential::{respond, Request};
use gitfriend::exec::plan_env;
use gitfriend::resolve::resolve_repo;
use gitfriend::secrets::AgeFileBackend;

#[derive(Parser)]
#[command(name = "gitfriend", version, about = "Per-repository git identity and credentials")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Speak the git credential helper protocol on stdin/stdout.
    ///
    /// Configure with:
    ///     git config --global credential.helper "gitfriend credential"
    ///     git config --global credential.useHttpPath true
    Credential {
        /// `get`, `store`, or `erase` -- supplied by git.
        operation: String,
    },

    /// Run a command with exactly one account's credentials.
    ///
    /// Variables managed by any account are cleared first, then the resolved
    /// account's are set, so nothing inherited from the shell survives:
    ///     gitfriend exec -- gh pr list
    Exec {
        /// Use this account instead of resolving one from the current repo.
        #[arg(long)]
        account: Option<String>,
        /// The command to run, after `--`.
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },

    /// Manage wrapper scripts that route a CLI through `exec`.
    Shim {
        #[command(subcommand)]
        action: ShimAction,
    },
}

#[derive(Subcommand)]
enum ShimAction {
    /// Write a shim for each named CLI.
    Install {
        /// Directory to write shims into. Put it early on PATH.
        #[arg(long)]
        dir: PathBuf,
        /// CLI names to wrap, e.g. `gh tea`.
        #[arg(required = true)]
        names: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Credential { operation } => credential(&operation),
        Command::Shim { action } => match shim(action) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("gitfriend: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Exec { account, command } => match exec(account.as_deref(), &command) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitfriend: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

fn shim(action: ShimAction) -> Result<(), String> {
    match action {
        ShimAction::Install { dir, names } => {
            let path_var = std::env::var("PATH").unwrap_or_default();
            for name in &names {
                let written = gitfriend::shim::install(name, &dir, &path_var)
                    .map_err(|e| e.to_string())?;
                println!("{}", written.display());
            }
            Ok(())
        }
    }
}

fn exec(account_name: Option<&str>, command: &[String]) -> Result<ExitCode, String> {
    let config = Config::load(&config_path()).map_err(|e| e.to_string())?;
    let backend = AgeFileBackend::with_identity_file(secrets_path(), &identity_path())
        .map_err(|e| e.to_string())?;

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    let account = match account_name {
        Some(name) => config
            .account(name)
            .ok_or_else(|| format!("no account named {name:?}"))?,
        // Unlike the credential path, a Default resolution is allowed here:
        // running `gh` outside any repo should use the declared default, and
        // refusing would be hostile rather than safe. A repo whose remote
        // matches nothing still errors rather than falling back.
        None => resolve_repo(&config, &cwd)
            .map_err(|e| e.to_string())?
            .account,
    };

    let plan = plan_env(&config, &backend, account).map_err(|e| e.to_string())?;

    let (program, args) = command.split_first().expect("clap requires a command");
    let mut child = process::Command::new(program);
    child.args(args);

    // Clear everything managed before setting anything, so a variable an
    // account does not declare cannot survive from the parent (R11).
    for var in &plan.remove {
        child.env_remove(var);
    }
    for (var, value) in &plan.set {
        child.env(var, value);
    }

    let status = child
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;

    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    })
}

fn credential(operation: &str) -> ExitCode {
    match operation {
        "get" => match credential_get() {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                // Loud, and stdout stays empty so git cannot mistake a failure
                // for a credential (R8).
                eprintln!("gitfriend: {message}");
                ExitCode::FAILURE
            }
        },
        // Deliberately no-ops. gitfriend's own store is the source of truth;
        // letting git cache a copy elsewhere would put the same token in a
        // second place with different access rules (R11).
        "store" | "erase" => ExitCode::SUCCESS,
        other => {
            eprintln!("gitfriend: unknown credential operation {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn credential_get() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("cannot read the request from git: {e}"))?;

    let config = Config::load(&config_path()).map_err(|e| e.to_string())?;
    let backend = AgeFileBackend::with_identity_file(secrets_path(), &identity_path())
        .map_err(|e| e.to_string())?;

    let request = Request::parse(&input);
    let cwd = std::env::current_dir().ok();

    let credential =
        respond(&config, &backend, &request, cwd.as_deref()).map_err(|e| e.to_string())?;

    Ok(format!(
        "username={}\npassword={}\n",
        credential.username, credential.password
    ))
}

/// Locations are overridable by environment variable so the test suite can run
/// against a fixture without touching the real machine.
fn path_from_env(var: &str, default_file: &str) -> PathBuf {
    if let Some(value) = std::env::var_os(var) {
        return PathBuf::from(value);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config").join("gitfriend").join(default_file)
}

fn config_path() -> PathBuf {
    path_from_env("GITFRIEND_CONFIG", "accounts.toml")
}

fn secrets_path() -> PathBuf {
    path_from_env("GITFRIEND_SECRETS", "secrets.age")
}

fn identity_path() -> PathBuf {
    path_from_env("GITFRIEND_IDENTITY", "identity.key")
}
