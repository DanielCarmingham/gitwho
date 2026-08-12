# gitwho

Pick the right git identity **and** the right credentials for a repository —
automatically, wherever that repository lives on disk — across multiple hosted
git providers and the CLIs/MCP servers that talk to them.

Read [REQUIREMENTS.md](REQUIREMENTS.md) before proposing anything. It contains
the requirements (R1–R15), the measured evidence behind them, and the open
questions. [README.md](README.md) is the short version.

## Status

**Built and tested; not adopted.** All seven build phases are done — resolver,
credential helper, secret storage, `exec` + shims, `doctor`, `sync`, MCP
wrapping. Nothing in `$HOME` has been changed; the machine still runs the old
path-based dotfiles setup described under "Prior art" below.

The remaining work is the cutover: **[docs/CUTOVER.md](docs/CUTOVER.md)**, with
a starting config in [docs/accounts.toml.example](docs/accounts.toml.example).
Read both before touching anything in `$HOME`.

**Generated but unverified: the Windows paths.** `%APPDATA%\gitwho` in
`src/paths.rs`, the `.cmd` shim and `PATHEXT` lookup in `src/shim.rs`, and the
fact that gitwho applies no ACLs there (`Protection::DirectoryInherited`, so
`doctor` warns rather than the file being closed down). All are unit-tested as
pure functions from macOS; none has ever run on Windows. Do not describe them as
working.

The mechanism that makes that testable is a **parameter, never a `cfg!`**:
`paths::Layout` and `shim::ShimTarget` are arguments, with `HOST` used only by
`main`. A `cfg!(windows)` branch is unreachable from a test run here, so it can
be documented as covered while nothing can reach it — which is exactly what
happened to the `APPDATA` branch. The test binaries are `#[cfg(unix)]`-gated
around anything touching `std::os::unix`, so `cargo test` still *builds*
elsewhere.

Corrections to `REQUIREMENTS.md` found by measuring the real machine:

- **Digilope is not ssh-only.** It uses `app-gitea.digilope.com` over ssh *and*
  `gitea.digilope.com` over https, so it does need a token in the transport
  path. Transport is a property of a remote, not an account — which is why
  there is no `gitAuth` field.
- **github.com is served by `gh auth git-credential`**, set url-scoped in
  `~/.gitconfig-darwin`, not by the credential manager in `.gitconfig-common`.
  A url-scoped section overrides the general list outright.
- **supacode uses worktrees, not clones**, so today's path rules survive by
  luck rather than by design.

## Ground rules

- **Evidence, not assumption.** Every claim in REQUIREMENTS.md was verified on
  this machine with a command. Keep that bar: if you assert direnv/git/gh
  behaviour, run it first and record the tool version alongside the result.
- **Wrong-and-quiet is worse than broken-and-loud** (R8). Any fallback that
  produces a *working but incorrect* account is a bug, not a convenience.
- **Secrets never enter this repo** (R10). Config that *names* a variable is
  tracked; values live in untracked files (`~/.envrc.local`, `~/.zshrc.local`).
  Never echo a token value — fingerprints/prefixes only, including in test
  output and error messages.
- **No global mutable credential state** (R9). No `gh auth switch`-style
  process-wide active account. Per-process / per-invocation only.
- **Resolution is on the hot path** (R15). It runs on every `cd` or CLI
  invocation; budget is single-digit-to-low-double-digit milliseconds.
- YAGNI. This replaces a working-ish shell setup; scope creep is the main risk.

## Tooling

### Version control: `jj` (colocated on top of git)

This repo is a **colocated** jj repo (`.jj/` and `.git/` side by side), so git
tooling still works, but **drive it with `jj`**:

- `jj st` — working-copy status. There is no staging area; the working copy is
  always a commit.
- `jj describe -m "msg"` — set the current change's message.
- `jj new` — start a new change on top (the usual "commit and move on").
- `jj log` — history. `jj diff` — current change's diff.
- `jj bookmark set main -r @-` then `jj git push` — bookmarks are jj's branches;
  they do **not** move automatically. Move `main` explicitly before pushing.
- `main@origin` is tracked. Remote: `github.com/DanielCarmingham/gitwho`.
- Don't mix `git commit` into a jj workflow here; use `jj` and let colocation
  export to git.

### Task tracking: `dex`

Multi-step work is tracked in dex, **by default, without being asked**. Use the
`dex` / `dex-plan` skills. The store resolves from cwd — this repo has its own
`.dex/` (confirm with `dex dir`; outside a git repo dex silently falls back to
the shared global store).

Create the task list up front and keep it current as work lands.

### Specs and plans

Design docs go in `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`;
implementation plans follow from them (superpowers `brainstorming` →
`writing-plans` flow).

## Domain model (as of the requirements doc)

Two independent axes, both currently keyed off filesystem path — which is the
bug:

| Axis | Decides | Chosen today by | Should be keyed on |
|---|---|---|---|
| **git identity** | author name/email, ssh key | `includeIf "gitdir:"` | remote URL (`includeIf "hasconfig:remote.*.url:"`) |
| **CLI / MCP credentials** | what `gh`/`tea`/`glab`/MCP authenticate as | `GH_TOKEN` exported by direnv per directory | the repo, re-resolved per directory change |

Providers differ in **mechanism**, not just variable name:

- GitHub here uses **https** remotes → git authenticates via the credential
  helper, which needs a token.
- Digilope (Gitea) uses **ssh** remotes → git authenticates with a key via
  `core.sshcommand`; **no token is involved in push/pull at all**. An ssh
  account must not be forced to invent a token (R7).
- Each CLI reads its own variables: `gh`→`GH_TOKEN`, `tea`→`GITEA_TOKEN`
  +`GITEA_HOST`, `glab`→`GITLAB_TOKEN`, `az devops`→its own.

The provider-neutral declaration schema already drafted in the dotfiles:

```ini
[account]
    provider = github
    name     = DanielAtProfound
    gitAuth  = https            ; or ssh
    env      = GH_TOKEN                    ; value read from GH_TOKEN_<name>
    env      = GITEA_HOST=https://host/api ; literal, non-secret
```

## Prior art on this machine (read before designing)

The existing implementation lives in the `cfg` bare repo (`$HOME`), not here:

| Path | Role |
|---|---|
| `~/.gitconfig-common` | default identity + the `includeIf gitdir:` rules |
| `~/.gitconfig-Github-<Account>`, `~/.gitconfig-Digilope-Gitea-Daniel` | per-account identity, `[account]`/`[github] account`, `core.sshcommand` |
| `~/.config/direnv/direnvrc` | the current resolver — asks git for `account.name`, exports `<VAR>_<Account>` |
| `~/.envrc.local`, `~/.zshrc.local` | **secret**, untracked, hold the token values |
| `~/ACCOUNTS.md` | current-state doc + "add an account" checklist + known gaps |

Known accounts: `DanielCarmingham` (personal), `DanielAtProfound`,
`DanielAtKitchenCloud` (nested inside Profound — ordering-sensitive),
`Digilope` (Gitea, ssh, no token).

Reading those files is fine. **Editing `$HOME` needs the cfg repo's explicit
git-dir invocation** (see the user's global CLAUDE.md) — and should be a
deliberate, separate step, not a side effect of work in this repo.

## Verified environment

Verified 2026-08-09 on this machine:

- macOS (darwin 25.6.0), zsh
- git 2.50.1 (Apple Git-155) — `hasconfig:remote.*.url:` needs ≥2.36 ✅
- direnv 2.37.1, gh 2.97.0, jj 0.44.0
- `tea` and `az` installed; **`tea` has never been logged in**
  (`~/.config/tea` does not exist)
- `glab` **not** installed
