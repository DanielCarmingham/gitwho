# gitfriend — design

Written 2026-08-09. Derives from [REQUIREMENTS.md](../../../REQUIREMENTS.md)
(requirements R1–R15 and the evidence behind them). Everything under "Verified
findings" was measured on this machine, not assumed.

## Context

Working across four accounts (`DanielCarmingham`, `DanielAtProfound`,
`DanielAtKitchenCloud`, `Digilope`/Gitea) means every repo needs a different
author identity *and* different credentials for whatever tooling touches it —
`git`, `gh`, `tea`, an IDE, an MCP server.

Today both are chosen by **filesystem path**, by two unrelated mechanisms that
nothing keeps in agreement:

- `includeIf "gitdir:"` rules in `~/.gitconfig-common` pick the identity.
- `direnv` (via `~/.config/direnv/direnvrc` + per-root `.envrc`) exports
  `GH_TOKEN` to pick the credential.

Path is the wrong key — an account is a property of the *repository*, not of
where it sits on disk. `REQUIREMENTS.md` documents the measured failures: a
clone made outside its account root resolves to the wrong identity **silently**,
and two repos needing different accounts under one `.envrc` get a stale token
because direnv never re-evaluates. Worse, `direnvrc`'s own comment concedes that
`GH_TOKEN` is now loaded in **every** direnv-managed directory — so every
project, and every MCP server or agent launched from it, sees every other
project's credential. `Digilope/one-drop-visuals` runs `gitea-mcp` with an empty
`env` block right now, inheriting whatever was ambient.

**Outcome:** a `gitfriend` binary where the correct identity and credentials are
selected from the repository itself, wherever it lives, and where no token sits
in the ambient environment at all.

## Decisions taken

| Decision | Choice |
|---|---|
| Form factor | Rust binary + generated shell glue |
| Isolation | Least-privilege by default; `gitfriend shell-env` as explicit opt-in escape hatch |
| Secret store | macOS Keychain, behind a pluggable backend trait |
| v1 providers | GitHub, Gitea/Forgejo (incl. Codeberg), GitLab, Azure DevOps |
| Non-git creds (AWS, npm) | **Out of scope for v1** |
| MCP wiring | Rewrite `command` to launch via `gitfriend exec`; resolve from cwd at launch |
| Multiple remotes | `origin` wins; error only if `origin` itself is ambiguous |
| No remote | Path-rule fallback → explicitly declared `default` account |
| Dotfiles | Single `accounts.toml` is source of truth; `gitfriend sync` generates gitconfigs; the `direnvrc` account block is deleted |

## Verified findings this plan depends on

Measured on this machine (git 2.50.1, direnv 2.37.1, gh 2.97.0, jj 0.44.0):

1. **A credential helper runs with cwd = the repo worktree.** Probed with an
   inline `-c credential.helper=!pwd`; it printed the repo path.
2. **`GIT_DIR` is *not* exported to the helper** — only `GIT_PREFIX`,
   `GIT_EXEC_PATH`, `GIT_EDITOR`, `GIT_CONFIG_PARAMETERS`. Resolution must ask
   git from cwd rather than read an env var.
3. **`credential.useHttpPath=true` puts the org in the request**: a fill for
   `https://github.com/EJ-Rice/somerepo.git` yields `path=EJ-Rice/somerepo.git`.
   This is the key result — **the helper can resolve the account at clone time,
   before any repo exists on disk**, so git transport needs no shell hook and no
   path rule whatsoever.
4. `hasconfig:remote.*.url:` requires git ≥ 2.36; this machine has 2.50.1. ✅
5. **supacode uses worktrees, not clones** (`supacode worktree` subcommands;
   `git worktree list` on a supacode-managed repo). Answers open question 2:
   today's path rules survive by luck, so the migration is important but not an
   emergency.
6. `tea` is installed but **has never been logged in** (`~/.config/tea` absent);
   `glab` is **not** installed; only `security` (Keychain) is available as a
   secret store — no `op`, `pass`, or `age`.
7. `~/.zshrc.local` holds 7 plaintext exports: `GH_TOKEN_{DanielCarmingham,
   DanielAtProfound,DanielAtKitchenCloud}`, `GITEA_HOST`, `GITEA_TOKEN`,
   `GITHUB_PAT`, `GITHUB_PAT_PROFOUND`. The last two have no known consumer
   (open question 5) — resolve during migration, do not assume dead.

## Architecture

Four entry points into one resolver. Nothing shares mutable global state.

```
                    ┌──────────────────────────┐
                    │  ~/.config/gitfriend/    │
                    │      accounts.toml       │  ← the only file you edit
                    └───────────┬──────────────┘
                                │
                     ┌──────────▼──────────┐
                     │      resolver       │   URL → account, then cwd-repo → account
                     └──────────┬──────────┘
                                │
   ┌───────────────┬────────────┼─────────────┬──────────────────┐
   │               │            │             │                  │
┌──▼───────────┐ ┌─▼────────┐ ┌─▼─────────┐ ┌─▼──────────┐  ┌────▼────────┐
│ credential   │ │  exec    │ │   sync    │ │  doctor    │  │  shell-env  │
│ get/store/   │ │ (CLIs +  │ │ generate  │ │  drift +   │  │  opt-in     │
│ erase        │ │  MCPs)   │ │ gitconfig │ │  fingerpr. │  │  ambient    │
└──────┬───────┘ └────┬─────┘ └───────────┘ └────────────┘  └─────────────┘
       │              │
       └──────┬───────┘
         ┌────▼─────────────────┐
         │  secrets backend     │  trait: Keychain | Env | (later: op/pass)
         └──────────────────────┘
```

**Why this shape:** git transport is handled entirely by the credential helper
(finding 3), so it never needs the environment. CLIs and MCP servers are
processes we launch, so `exec` can scope credentials to exactly one process.
Identity (author name/email) is static config, so `sync` generates it once. That
leaves *nothing* requiring an ambient token — which is what kills the exposure
problem at the root rather than re-triggering it more often.

## Config schema

`~/.config/gitfriend/accounts.toml` — tracked in the `cfg` repo, **names only,
never values** (R10).

The `email`, `match`, and `sshKey` values below are **illustrative**. The real
ones come from the existing `~/.gitconfig-Github-*` /
`~/.gitconfig-Digilope-Gitea-Daniel` files and are transcribed during phase 5.

```toml
[defaults]
account = "DanielCarmingham"        # explicit; used only when nothing else matches

[[accounts]]
name     = "DanielAtProfound"
provider = "github"
email    = "daniel@profound.example"
gitAuth  = "https"                  # → credential helper supplies a token
match    = ["github.com/Profound-*/**", "github.com/EJ-Rice/**"]
paths    = ["~/Developer/Profound/"] # remote-less repos only
env      = ["GH_TOKEN"]              # secret, from the backend
secret   = "github/DanielAtProfound" # backend key

[[accounts]]
name       = "Digilope"
provider   = "gitea"
email      = "daniel@digilope.example"
gitAuth    = "ssh"                  # no token in the push/pull path at all (R7)
sshKey     = "~/.ssh/id_ed25519_digilope"
match      = ["app-gitea.digilope.com/**", "forgejo.digilope.com/**"]
env        = ["GITEA_TOKEN", "GITEA_HOST=https://app-gitea.digilope.com/api/v1"]
secret     = "gitea/Digilope"       # for `tea`/gitea-mcp only, not for git
```

Adding Codeberg is then ~6 lines with `provider = "gitea"` and a different host
— the R6 proof, with no mechanism change.

**Ordering hazard to preserve:** `DanielAtKitchenCloud` lives *inside*
`~/Developer/Profound/`. Under URL matching this stops being ordering-sensitive
(different orgs, different patterns), but the `paths` fallback must still apply
longest-prefix-wins. Test this explicitly.

## Resolution algorithm

Single function, four inputs, one output — `Resolved { account, reason }`. The
`reason` is what makes failures loud instead of quiet.

1. **URL given** (credential helper, or an explicit `--url`): match
   `host + path` against every account's `match` globs. Longest match wins.
   Two accounts tying on the same pattern → **error**.
2. **No URL, cwd is a repo**: read `remote.origin.url` via git; go to step 1.
   Non-origin remotes are ignored for identity (a fork's `upstream` still
   authenticates correctly at fetch time, because the helper resolves per-URL).
3. **Repo with no `origin`**: longest-prefix match on `paths`.
4. **Nothing matched**: `[defaults].account`, tagged
   `reason = Default`, so `doctor` and `--explain` can surface it.

**R8 enforcement:** the resolver never guesses. `Resolved.reason` is one of
`UrlMatch | OriginUrl | PathFallback | Default`, and `exec`/`credential` refuse
to hand over a *secret* on `Default` unless the account is the declared default
— an unknown account with a missing secret errors loudly rather than falling
through to the personal token, which is precisely the regression the current
`direnvrc` briefly caused.

## Module layout

```
src/
  main.rs           clap dispatch
  config.rs         accounts.toml load + validate (serde + toml)
  resolve.rs        the algorithm above — pure, no I/O beyond a git query
  git.rs            thin wrapper over `git config` / `rev-parse`
  secrets/
    mod.rs          trait Backend { get, set, delete, fingerprint }
    keychain.rs     `security` / `keyring` crate
    env.rs          reads GH_TOKEN_<Account> — migration + CI fallback
  cmd/
    credential.rs   git credential helper protocol (get/store/erase)
    exec.rs         spawn with a scrubbed, then populated, env
    sync.rs         generate ~/.gitconfig-* + include block
    doctor.rs       drift + fingerprints, never values
    mcp.rs          rewrite .mcp.json commands
    shell_env.rs    opt-in ambient export lines
```

Crates: `clap`, `serde`/`toml`, `anyhow`+`thiserror`, `keyring`, `globset`,
`serde_json` (for `.mcp.json`). Shell out to `git` rather than linking `gix` —
it guarantees behaviour matches the installed git, and one `git config` call is
~5 ms, inside the R15 budget. Revisit only if `doctor` shows it isn't.

## Build sequence

Track in dex (`.dex/` is already local to this repo — `dex dir` confirmed).
Each phase is independently verifiable; phases 1–3 land before anything on the
live machine changes.

**Phase 0 — spec.** ✅ This document.

**Phase 1 — resolver core.** `config.rs` + `resolve.rs` + `git.rs`, with a
hermetic fixture harness (temp `HOME`, temp repos, no real tokens). Tests map
1:1 onto requirements: R1/R2 (relocated clone, linked worktree), R3 (two orgs
one host), R4 (no remote → path → default), the KitchenCloud nesting case, and
the fork/`origin`-wins case.

**Phase 2 — credential helper.** `gitfriend credential get|store|erase`. This is
the single highest-value piece: per finding 3 it alone satisfies R1/R2/R3 for
git transport, including at clone time. Requires setting
`credential.useHttpPath = true` for `github.com` so the org reaches the helper —
note this **replaces GCM for github.com**, so `~/.gitconfig-common`'s existing
`[credential]` stack needs untangling (it currently sets `helper` four times,
including two empty resets).

**Phase 3 — secrets backends.** Keychain + env, `gitfriend secret set|list`
(list shows fingerprints only). Import the 3 existing `GH_TOKEN_*` values and
`GITEA_TOKEN` from `~/.zshrc.local` into the Keychain.

**Phase 4 — `exec` + shims.** `gitfriend exec [--account X] -- cmd …`, scrubbing
inherited provider vars before injecting the resolved ones (so a stale ambient
`GH_TOKEN` can't leak past — R11). Generated shims for `gh` and `tea` on `PATH`.

**Phase 5 — `sync` + `doctor`.** Generate `~/.gitconfig-<Account>` files and the
`hasconfig:remote.*.url:` include block into clearly-marked generated regions;
`doctor` reports drift, missing secrets, accounts declaring vars with no
backing secret, and unwrapped provider MCPs. Retire the `direnvrc` account block
and the account-root `.envrc` token exports.

**Phase 6 — MCP wiring.** `gitfriend mcp sync` rewrites provider MCP `command`
entries to launch via `gitfriend exec`. Fix `Digilope/one-drop-visuals` first —
it's the live exposure case.

**Phase 7 — migration + docs.** Cut the live machine over, rewrite `~/ACCOUNTS.md`
as the gitfriend checklist (R13), resolve the two unexplained `GITHUB_PAT*`
variables (open question 5) and the vestigial `[github] account` key (open
question 6), and confirm the fresh-machine story (R14).

## Verification

Automated, hermetic (temp `HOME`, fixture repos, fake tokens — no network, no
real credentials):

- **R1/R2** — resolve a repo cloned to `/tmp/anywhere`, and a linked worktree in
  a foreign root; both must yield the right account.
- **R3** — `github.com/DanielCarmingham/x` and `github.com/EJ-Rice/y` resolve
  differently from identical cwds.
- **R4** — `git init` with no remote: path rule, then declared default; assert
  `reason` is `PathFallback`/`Default`, never silent.
- **R7** — an `ssh` account resolves and pushes with **no token defined at all**.
- **R8** — an account naming a secret that doesn't exist must **fail non-zero**,
  and must not emit the default account's token. Assert on exit code *and* that
  the env contains no token.
- **R11** — `exec` with a hostile pre-set `GH_TOKEN` in the parent env: the
  child of a Gitea account must not see it.
- **R15** — `hyperfine` the helper and `resolve`; budget is single-digit ms.

Manual, on the live machine after phase 5:

```
gitfriend doctor                    # expect: no drift, all secrets present
env | grep -E 'GH_TOKEN|GITEA_TOKEN'   # expect: empty
cd ~/Developer/Profound/<repo> && git config user.email && gh api user --jq .login
cd ~/Developer/Digilope/<repo> && gh api user --jq .login   # must NOT be a Profound login
git clone https://github.com/EJ-Rice/<repo> /tmp/relocated && \
  git -C /tmp/relocated config user.email    # correct identity outside any account root
```

## Risks

- **Replacing GCM for github.com** is the riskiest single step. Keep GCM
  configured for non-github hosts and stage phase 2 behind a per-host override
  so it can be reverted with one config line.
- **Keychain prompts** on every helper invocation would violate R15. Verify the
  ACL grants the `gitfriend` binary persistent access; re-test after every
  rebuild, since a changed binary signature can re-trigger prompts.
- **R14 regression** — Keychain secrets can't be committed. Fresh-machine setup
  becomes "clone `cfg`, run `gitfriend secret set` ×N". Document it as an
  explicit manual step rather than letting it be discovered.
