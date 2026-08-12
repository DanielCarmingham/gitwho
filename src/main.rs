//! CLI entry point.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};
use std::sync::OnceLock;

use clap::{Parser, Subcommand};

use gitwho::config::Config;
use gitwho::credential::{respond, Request};
use gitwho::exec::plan_env;
use gitwho::resolve::{resolve_repo, Reason};
use gitwho::secrets::select::{self, BackendKind, Choice, Platform};
use gitwho::secrets::{fingerprint, AgeFileBackend, Backend, EnvBackend, KeychainBackend};

#[derive(Parser)]
#[command(name = "gitwho", version, about = "Per-repository git identity and credentials")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Speak the git credential helper protocol on stdin/stdout.
    ///
    /// Configure with:
    ///     git config --global credential.helper "gitwho credential"
    ///     git config --global credential.useHttpPath true
    Credential {
        /// `get`, `store`, or `erase` -- supplied by git.
        operation: String,
    },

    /// Run a command with exactly one account's credentials.
    ///
    /// Variables managed by any account are cleared first, then the resolved
    /// account's are set, so nothing inherited from the shell survives:
    ///     gitwho exec -- gh pr list
    Exec {
        /// Use this account instead of resolving one from the current repo.
        #[arg(long)]
        account: Option<String>,
        /// The command to run, after `--`.
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },

    /// Set up everything on a machine that has never run gitwho.
    ///
    /// Reports what it would do and stops, unless `--write` is given. Safe to
    /// re-run: every step is idempotent, so this is also how you apply a newly
    /// added account.
    Init {
        /// Apply the changes instead of only reporting them.
        #[arg(long)]
        write: bool,
        /// Where to write the `gh`/`tea` wrappers.
        #[arg(long)]
        shim_dir: Option<PathBuf>,
        /// Which CLIs to wrap. Names not found on PATH are skipped.
        #[arg(long, value_delimiter = ',', default_value = "gh,tea")]
        shims: Vec<String>,
    },

    /// Report whether the wiring is coherent. Read-only; changes nothing.
    Doctor,

    /// Generate the git config that selects an identity per repository.
    ///
    /// Writes only into gitwho's own directory. Include it once from your
    /// main gitconfig; nothing hand-written is ever rewritten.
    Sync {
        /// Directory to generate into.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Apply the changes instead of only reporting them.
        #[arg(long)]
        write: bool,
    },

    /// Route provider MCP servers through `exec`.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Store and inspect token values.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },

    /// Manage wrapper scripts that route a CLI through `exec`.
    Shim {
        #[command(subcommand)]
        action: ShimAction,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Rewrite `.mcp.json` so provider servers launch through `exec`.
    ///
    /// Prints what it would change and stops, unless `--write` is given: these
    /// files are usually committed, so an accidental rewrite is a diff someone
    /// has to review.
    Sync {
        /// `.mcp.json` files to process.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Apply the changes instead of only reporting them.
        #[arg(long)]
        write: bool,
    },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Create the identity key that unlocks the secrets file.
    Init,
    /// Store a value, read from stdin.
    ///
    /// The value is never an argument: anything in argv is readable by every
    /// process on the machine through `ps`.
    Set { account: String, var: String },
    /// Show every declared secret as a fingerprint, or as missing.
    List,
    /// Remove a stored value.
    Delete { account: String, var: String },
    /// Copy values in from `<VAR>_<Account>` environment variables.
    Import {
        /// Read from this process's environment -- run it from a shell that
        /// has the old exports loaded.
        #[arg(long)]
        from_env: bool,
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
        Command::Doctor => match doctor_report() {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Init {
            write,
            shim_dir,
            shims,
        } => match init(write, shim_dir, &shims) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Sync { dir, write } => match sync_config(dir, write) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Mcp { action } => match mcp(action) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Secret { action } => match secret(action) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Shim { action } => match shim(action) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Exec { account, command } => match exec(account.as_deref(), &command) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

fn doctor_report() -> Result<ExitCode, String> {
    let config = Config::load(&config_path()?).map_err(|e| e.to_string())?;
    let (backend, choice) = open_backend(config.defaults.secret_backend.as_deref())?;
    let backend = backend.as_ref();

    let ambient: std::collections::BTreeMap<String, String> = unicode_env().collect();
    let wiring = gitwho::doctor::GitWiring {
        credential_helpers: gitwho::git::credential_helpers(),
        github_helper: gitwho::git::credential_helper_for("https://github.com"),
        use_http_path: gitwho::git::use_http_path_for_github(),
    };

    // The directory is taken from the config's own parent rather than assumed
    // to be `~/.config/gitwho`, so the environment overrides below keep
    // pointing everything at one place.
    let config_file = config_path()?;
    let store = gitwho::doctor::Store {
        dir: config_file
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
        config: config_file,
        identity: identity_path()?,
        secrets: secrets_path()?,
        owner: gitwho::doctor::current_uid(),
        backend: choice,
        owner_only_enforced: AgeFileBackend::protection()
            == gitwho::secrets::Protection::OwnerOnly,
    };

    let findings = gitwho::doctor::run(&config, backend, &ambient, &wiring, &store);

    for finding in &findings {
        let tag = match finding.level {
            gitwho::doctor::Level::Ok => "ok  ",
            gitwho::doctor::Level::Warn => "warn",
            gitwho::doctor::Level::Problem => "FAIL",
        };
        println!("{tag} [{}] {}", finding.check, finding.message);
    }

    if gitwho::doctor::has_problems(&findings) {
        println!();
        println!("doctor found problems; nothing was changed");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Everything the manual install did, in one idempotent command.
///
/// The order matters and is the order of the guide: the store must exist before
/// a key can go in it, the config must be right before rules are generated from
/// it, and the rules must exist before anything is told to include them.
///
/// It stops at the one step it cannot do for you. `accounts.toml` needs *your*
/// accounts, and generating rules from a template of placeholders would produce
/// a machine that looks configured and resolves everything to a fictional
/// account -- working-but-wrong, which is the failure mode this project exists
/// to prevent (R8).
fn init(write: bool, shim_dir: Option<PathBuf>, shims: &[String]) -> Result<ExitCode, String> {
    let store = store_dir()?.clone();
    let config_path = config_path()?;
    let git_dir = path_from_env("GITWHO_GIT_DIR", "git")?;
    let shim_dir = match shim_dir {
        Some(dir) => dir,
        None => default_shim_dir()?,
    };

    // --- 1. the store, and the key that unlocks it --------------------------
    let choice = choose_backend(None)?;
    if choice.kind == BackendKind::AgeFile {
        let identity = identity_path()?;
        if identity.exists() {
            step("ok", format!("store {}", store.display()));
        } else if write {
            AgeFileBackend::generate_identity_file(&identity).map_err(|e| e.to_string())?;
            step("created", format!("{} (owner-only)", identity.display()));
        } else {
            step("would create", format!("{}", identity.display()));
        }
    } else {
        step("ok", format!("store: {} (no key file needed)", choice.kind.name()));
    }

    // --- 2. the config, which is where this stops ---------------------------
    if !config_path.exists() {
        if !write {
            step("would create", format!("{} from the template", config_path.display()));
            println!();
            println!("nothing was written; re-run with --write to apply");
            return Ok(ExitCode::SUCCESS);
        }

        write_owner_only(&config_path, gitwho::init::TEMPLATE)?;
        step("created", format!("{}", config_path.display()));
        println!();
        println!("Now the part only you can do:");
        println!();
        println!("  1. edit {}", config_path.display());
        println!("     replace the example accounts with yours");
        println!("  2. gitwho secret set <Account> <VAR>    once per token");
        println!("  3. gitwho init --write                  re-run to finish");
        println!();
        println!("Stopping here on purpose: generating rules from the template");
        println!("would give you a machine that looks configured and resolves");
        println!("every repository to an account that does not exist.");
        return Ok(ExitCode::FAILURE);
    }
    step("ok", format!("{}", config_path.display()));

    let config = Config::load(&config_path).map_err(|e| e.to_string())?;

    // --- 3. the generated rules ---------------------------------------------
    let gitwho_path = current_exe_path()?;
    let plan = gitwho::sync::plan(&config, &git_dir, &gitwho_path);
    if write {
        let changed = gitwho::sync::apply(&plan).map_err(|e| e.to_string())?;
        if changed.is_empty() {
            step("ok", format!("rules in {}", git_dir.display()));
        } else {
            for path in &changed {
                step("wrote", format!("{}", path.display()));
            }
        }
    } else {
        step(
            "would write",
            format!("{} files in {}", plan.files.len(), git_dir.display()),
        );
    }

    // --- 4. the one line in your gitconfig ----------------------------------
    let includes = git_dir.join("includes.gitconfig");
    let snippet = gitwho::init::Snippet::gitconfig_include(&includes);
    report_snippet(&gitconfig_path()?, &snippet, write)?;

    // --- 5. the shims -------------------------------------------------------
    let path_var = std::env::var("PATH").unwrap_or_default();
    let mut installed_any = false;
    for name in shims {
        match gitwho::shim::install(name, &shim_dir, &path_var) {
            Ok(installed) if write => {
                installed_any = true;
                let state = if installed.changed { "wrote" } else { "ok" };
                step(state, format!("{}", installed.path.display()));
            }
            Ok(_) => {
                installed_any = true;
                step("would wrap", format!("{name} -> {}", shim_dir.display()));
            }
            // Not an error: wrapping a CLI you have not installed would create
            // a shim pointing at nothing, which fails later and further away.
            Err(gitwho::shim::ShimError::NotFound { .. }) => {
                step("skipped", format!("{name} is not on PATH"));
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    // --- 6. the line that puts them ahead of the real ones ------------------
    if installed_any {
        let snippet = gitwho::init::Snippet::path_export(&shim_dir);
        report_snippet(&shell_rc_path()?, &snippet, write)?;
    }

    if !write {
        println!();
        println!("nothing was written; re-run with --write to apply");
        return Ok(ExitCode::SUCCESS);
    }

    println!();
    doctor_report()
}

/// Print one aligned step line. Every step says what happened, including
/// "nothing, it was already right" -- a silent step is indistinguishable from a
/// skipped one.
fn step(state: &str, detail: String) {
    println!("{state:<13} {detail}");
}

/// Handle one of the two files gitwho does not own.
fn report_snippet(
    path: &Path,
    snippet: &gitwho::init::Snippet,
    write: bool,
) -> Result<(), String> {
    use gitwho::init::Applied;

    let applied = gitwho::init::ensure(path, snippet, write)
        .map_err(|e| format!("cannot update {}: {e}", path.display()))?;

    match applied {
        Applied::AlreadyPresent => step("ok", format!("{} ({})", path.display(), snippet.purpose)),
        Applied::Appended => step("appended", format!("{} ({})", path.display(), snippet.purpose)),
        Applied::WouldAppend => step(
            "would append",
            format!("{} ({})", path.display(), snippet.purpose),
        ),
        Applied::FileMissing => {
            step("missing", format!("{} -- add this yourself:", path.display()));
            for line in snippet.text.lines().filter(|l| !l.trim().is_empty()) {
                println!("               {line}");
            }
        }
    }
    Ok(())
}

fn current_exe_path() -> Result<String, String> {
    Ok(std::env::current_exe()
        .map_err(|e| format!("cannot find my own path: {e}"))?
        .to_string_lossy()
        .into_owned())
}

/// Written at `0600` for the same reason the guide says to: `accounts.toml` is a
/// redirect vector. Whoever can write it can add a `match` for a host they
/// control and be handed one of your tokens.
fn write_owner_only(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot secure {}: {e}", path.display()))?;
    }

    Ok(())
}

/// `$HOME/.gitconfig`. Not `git config --global --edit`'s idea of it: that can
/// be `$XDG_CONFIG_HOME/git/config`, and appending to the wrong one of the two
/// is a silent no-op.
fn gitconfig_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    let xdg = home.join(".config/git/config");
    if xdg.exists() {
        return Ok(xdg);
    }
    Ok(home.join(".gitconfig"))
}

/// The rc file a shim `PATH` line belongs in.
///
/// `.zshrc` rather than `.zshenv` on purpose -- see `Snippet::path_export`.
fn shell_rc_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    Ok(if shell.ends_with("bash") {
        home.join(".bashrc")
    } else {
        home.join(".zshrc")
    })
}

fn default_shim_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".local/share/gitwho/shims"))
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "neither HOME nor USERPROFILE is set".to_string())
}

fn sync_config(dir: Option<PathBuf>, write: bool) -> Result<ExitCode, String> {
    let config = Config::load(&config_path()?).map_err(|e| e.to_string())?;
    let dir = match dir {
        Some(dir) => dir,
        None => path_from_env("GITWHO_GIT_DIR", "git")?,
    };

    let gitwho_path = std::env::current_exe()
        .map_err(|e| format!("cannot find my own path: {e}"))?
        .to_string_lossy()
        .into_owned();

    let plan = gitwho::sync::plan(&config, &dir, &gitwho_path);

    if !write {
        for file in &plan.files {
            let current = std::fs::read_to_string(&file.path).ok();
            let state = match current.as_deref() {
                Some(existing) if existing == file.contents => "unchanged",
                Some(_) => "would update",
                None => "would create",
            };
            println!("{state:<13} {}", file.path.display());
        }
        println!();
        println!("nothing was written; re-run with --write to apply");
        println!("then add this to your gitconfig, once:");
        println!("    [include]");
        println!("        path = {}", dir.join("includes.gitconfig").display());
        return Ok(ExitCode::SUCCESS);
    }

    let changed = gitwho::sync::apply(&plan).map_err(|e| e.to_string())?;
    if changed.is_empty() {
        println!("already up to date");
    } else {
        for path in &changed {
            println!("wrote {}", path.display());
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn mcp(action: McpAction) -> Result<ExitCode, String> {
    let McpAction::Sync { paths, write } = action;

    let gitwho = std::env::current_exe()
        .map_err(|e| format!("cannot find my own path: {e}"))?
        .to_string_lossy()
        .into_owned();

    let mut any_changes = false;

    for path in &paths {
        let original = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

        let (rewritten, changed) =
            gitwho::mcp::wrap(&original, &gitwho).map_err(|e| format!("{}: {e}", path.display()))?;

        if changed.is_empty() {
            println!("{}: nothing to change", path.display());
            continue;
        }
        any_changes = true;

        if write {
            std::fs::write(path, rewritten)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            println!("{}: wrapped {}", path.display(), changed.join(", "));
        } else {
            println!("{}: would wrap {}", path.display(), changed.join(", "));
        }
    }

    if any_changes && !write {
        println!();
        println!("nothing was written; re-run with --write to apply");
    }
    Ok(ExitCode::SUCCESS)
}

fn secret(action: SecretAction) -> Result<(), String> {
    if matches!(action, SecretAction::Init) {
        return secret_init();
    }

    let config = Config::load(&config_path()?).map_err(|e| e.to_string())?;
    let (backend, _) = open_backend(config.defaults.secret_backend.as_deref())?;
    let backend = backend.as_ref();

    match action {
        SecretAction::Init => unreachable!("handled above"),

        SecretAction::Set { account, var } => {
            // Catch a typo before it becomes a secret nothing ever reads --
            // the symptom would otherwise surface later as a missing
            // credential somewhere else entirely.
            let declared = config.account(&account).ok_or_else(|| {
                let known: Vec<&str> = config.accounts.iter().map(|a| a.name.as_str()).collect();
                format!(
                    "no account named {account:?}; accounts.toml declares: {}",
                    known.join(", ")
                )
            })?;

            if !declared.secret_vars().contains(&var.as_str()) {
                // A warning, not an error: the variable may be about to be
                // added to accounts.toml. Silence would let it sit unread.
                eprintln!(
                    "gitwho: warning: account {account} does not declare {var}; \
                     nothing will read it until accounts.toml lists it"
                );
            }

            let value = read_value(&account, &var)?;

            backend
                .set(&account, &var, &value)
                .map_err(|e| e.to_string())?;
            println!("stored {account}/{var} ({})", fingerprint(&value));
            Ok(())
        }

        SecretAction::Delete { account, var } => {
            backend
                .delete(&account, &var)
                .map_err(|e| e.to_string())?;
            println!("deleted {account}/{var}");
            Ok(())
        }

        SecretAction::List => {
            println!("{:<24} {:<16} FINGERPRINT", "ACCOUNT", "VARIABLE");
            for account in &config.accounts {
                for var in account.secret_vars() {
                    let status = match backend.get(&account.name, var).map_err(|e| e.to_string())? {
                        // Only ever the fingerprint (R10).
                        Some(value) => fingerprint(&value),
                        None => "MISSING".to_string(),
                    };
                    println!("{:<24} {:<16} {}", account.name, var, status);
                }
            }
            Ok(())
        }

        SecretAction::Import { from_env } => {
            if !from_env {
                return Err("specify a source, e.g. --from-env".to_string());
            }

            let source = EnvBackend::from_map(unicode_env().collect());
            let mut imported = 0;

            for account in &config.accounts {
                for var in account.secret_vars() {
                    let Some(value) = source.get(&account.name, var).map_err(|e| e.to_string())?
                    else {
                        continue;
                    };
                    backend
                        .set(&account.name, var, &value)
                        .map_err(|e| e.to_string())?;
                    println!("imported {}/{var} ({})", account.name, fingerprint(&value));
                    imported += 1;
                }
            }

            if imported == 0 {
                return Err(
                    "found no <VAR>_<Account> variables to import; run this from a shell that has them loaded"
                        .to_string(),
                );
            }
            Ok(())
        }
    }
}

/// Create the identity key that unlocks the age file -- unless the store in
/// effect has no such thing.
///
/// `accounts.toml` is read so `secretBackend` is honoured, but a *missing* one
/// is not an error: `init` is the first command anyone runs, and with no config
/// there is no configured backend to ignore. A config that exists and is broken
/// still fails, loudly.
fn secret_init() -> Result<(), String> {
    let configured = match Config::load(&config_path()?) {
        Ok(config) => config.defaults.secret_backend,
        Err(gitwho::config::ConfigError::Read { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            None
        }
        Err(e) => return Err(e.to_string()),
    };

    let choice = choose_backend(configured.as_deref())?;
    if choice.kind != BackendKind::AgeFile {
        // Said, not done. A stray identity key would sit there decrypting
        // nothing, and asking the store itself is precisely what the selection
        // code exists to avoid.
        println!(
            "the {} store needs no identity file (chosen {})",
            choice.kind.name(),
            choice.source.describe()
        );
        return Ok(());
    }

    let path = identity_path()?;
    AgeFileBackend::generate_identity_file(&path).map_err(|e| e.to_string())?;
    println!("created {}", path.display());
    Ok(())
}

/// Read the value, prompting only when someone is actually there to read the
/// prompt.
///
/// On a terminal: a prompt naming what is being set, and hidden input that
/// ends at Enter -- no invisible wait for a Ctrl-D nobody was told about. When
/// piped, behaviour is unchanged, so scripts and the test suite are unaffected.
fn read_value(account: &str, var: &str) -> Result<String, String> {
    use std::io::IsTerminal;

    if std::io::stdin().is_terminal() {
        let value = rpassword::prompt_password(format!(
            "Value for {account}/{var} (input hidden, Enter when done): "
        ))
        .map_err(|e| format!("cannot read the value: {e}"))?;

        let value = value.trim().to_string();
        if value.is_empty() {
            return Err("the value was empty; nothing stored".to_string());
        }
        return Ok(value);
    }

    gitwho::secrets::read_value_from(&mut std::io::stdin()).map_err(|e| e.to_string())
}

fn shim(action: ShimAction) -> Result<(), String> {
    match action {
        ShimAction::Install { dir, names } => {
            let path_var = std::env::var("PATH").unwrap_or_default();
            for name in &names {
                let written = gitwho::shim::install(name, &dir, &path_var)
                    .map_err(|e| e.to_string())?;
                println!("{}", written.path.display());
            }
            Ok(())
        }
    }
}

fn exec(account_name: Option<&str>, command: &[String]) -> Result<ExitCode, String> {
    let config = Config::load(&config_path()?).map_err(|e| e.to_string())?;
    let (backend, _) = open_backend(config.defaults.secret_backend.as_deref())?;
    let backend = backend.as_ref();

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    let account = match account_name {
        Some(name) => config
            .account(name)
            .ok_or_else(|| format!("no account named {name:?}"))?,
        // Unlike the credential path, a low-confidence resolution is allowed
        // here: running `gh` outside any repo, or inside a third-party clone,
        // should use the declared default. Refusing would be hostile rather
        // than safe. It is still said out loud, because an unclaimed remote is
        // also what a forgotten pattern looks like.
        None => {
            let resolved = resolve_repo(&config, &cwd).map_err(|e| e.to_string())?;
            if resolved.reason == Reason::Unmatched {
                eprintln!(
                    "gitwho: no account claims this repository's remote; using {}",
                    resolved.account.name
                );
            }
            resolved.account
        }
    };

    let plan = plan_env(&config, backend, account).map_err(|e| e.to_string())?;

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
                eprintln!("gitwho: {message}");
                ExitCode::FAILURE
            }
        },
        // Deliberately no-ops. gitwho's own store is the source of truth;
        // letting git cache a copy elsewhere would put the same token in a
        // second place with different access rules (R11).
        "store" | "erase" => ExitCode::SUCCESS,
        other => {
            eprintln!("gitwho: unknown credential operation {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn credential_get() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("cannot read the request from git: {e}"))?;

    let config = Config::load(&config_path()?).map_err(|e| e.to_string())?;
    let (backend, _) = open_backend(config.defaults.secret_backend.as_deref())?;
    let backend = backend.as_ref();

    let request = Request::parse(&input);
    let cwd = std::env::current_dir().ok();

    let credential =
        respond(&config, backend, &request, cwd.as_deref()).map_err(|e| e.to_string())?;

    Ok(format!(
        "username={}\npassword={}\n",
        credential.username, credential.password
    ))
}

/// Locations are overridable by environment variable so the test suite can run
/// against a fixture without touching the real machine.
///
/// Fallible, because the alternative is a *relative* path: without a home
/// directory there is no defensible answer, and answering anyway means reading
/// config out of the current working directory.
fn path_from_env(var: &str, default_file: &str) -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os(var) {
        return Ok(PathBuf::from(value));
    }
    Ok(store_dir()?.join(default_file))
}

/// gitwho's own directory, resolved once per process.
///
/// Once, because three files are asked for on every invocation and the answer
/// cannot change inside one -- and because this sits on the credential hot path
/// (R15). The failure is cached too, so it reads the same wherever it surfaces.
fn store_dir() -> Result<&'static PathBuf, String> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();

    DIR.get_or_init(|| {
        gitwho::paths::config_dir(
            &gitwho::paths::from_process,
            gitwho::paths::Layout::HOST,
        )
        .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(String::clone)
}

/// The environment as UTF-8 pairs, skipping anything that is not.
///
/// `std::env::vars` panics on a single non-Unicode entry *anywhere* in the
/// environment, and gitwho inherits whatever git or a shell happened to have
/// -- so one stray variable would take out the credential helper before it read
/// a byte of the request. Nothing gitwho looks for here, a managed variable
/// name or a token value, can be non-Unicode and still be usable, so skipping
/// loses nothing that was ever going to be found.
fn unicode_env() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os()
        .filter_map(|(var, value)| Some((var.into_string().ok()?, value.into_string().ok()?)))
}

/// Resolve which store is in effect, and open it.
///
/// One place, so every subcommand answers the question identically -- and so
/// the answer is a value the caller can report rather than a fact buried in a
/// constructor.
fn open_backend(configured: Option<&str>) -> Result<(Box<dyn Backend>, Choice), String> {
    let choice = choose_backend(configured)?;

    let backend: Box<dyn Backend> = match choice.kind {
        BackendKind::AgeFile => Box::new(
            AgeFileBackend::with_identity_file(secrets_path()?, &identity_path()?)
                .map_err(|e| e.to_string())?,
        ),
        BackendKind::Keychain => Box::new(KeychainBackend::new()),
    };

    Ok((backend, choice))
}

fn choose_backend(configured: Option<&str>) -> Result<Choice, String> {
    let from_env = std::env::var(select::ENV_VAR).ok();
    select::choose(from_env.as_deref(), configured, &Platform::detect())
        .map_err(|e| e.to_string())
}

fn config_path() -> Result<PathBuf, String> {
    path_from_env("GITWHO_CONFIG", "accounts.toml")
}

fn secrets_path() -> Result<PathBuf, String> {
    path_from_env("GITWHO_SECRETS", "secrets.age")
}

fn identity_path() -> Result<PathBuf, String> {
    path_from_env("GITWHO_IDENTITY", "identity.key")
}
