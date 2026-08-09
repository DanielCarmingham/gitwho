//! CLI entry point.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use gitfriend::config::Config;
use gitfriend::credential::{respond, Request};
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Credential { operation } => credential(&operation),
    }
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
