# Requirements: automatic per-repository credential selection

Goal: **the correct identity and credentials are selected for a repository
automatically, wherever that repository happens to live on disk.**

Status: requirements only. Supersedes the direction in `handoff.md`, which
assumed a path-based model. Current implemented state is in `ACCOUNTS.md`.

Written 2026-08-09. Everything in "Evidence" was verified on this machine
(git 2.50.1, direnv 2.37.1, gh 2.97.0), not assumed.

---

## The core problem

Two things must be chosen per repository, and today they are chosen by two
unrelated mechanisms that both key off **filesystem path**:

| Axis | What it decides | Chosen today by |
|---|---|---|
| **git identity** | author name/email, ssh key | `includeIf "gitdir:"` path rules |
| **CLI credentials** | what `gh`/`tea`/`glab` authenticate as | `GH_TOKEN` exported by direnv, per directory |

Path is the wrong key. A repository's account is a property of **the repository**
— which server it lives on, under which org — not of where it happens to sit in
the filesystem.

---

## Evidence

### Path-based rules break on relocation

| Scenario | `gitdir:` | `hasconfig:remote.*.url:` |
|---|---|---|
| repo inside the account root | resolves | resolves |
| `git worktree add` to a path outside the root | **resolves** | resolves |
| repo cloned outside the root, same remote | **does not resolve** | resolves |

The worktree case survives only incidentally: a linked worktree's `$GIT_DIR` is
`<main-repo>/.git/worktrees/<name>`, which is still inside the account root, so
the path rule still matches. That protection disappears the moment a tool
**clones** rather than adds a worktree, and it says nothing about a repo cloned
anywhere else by hand. When it fails it fails **silently**, falling back to the
default (personal) identity.

`hasconfig:remote.*.url:` requires git >= 2.36; this machine has 2.50.1.

### direnv cannot be the source of truth

- `direnvrc` is evaluated with cwd = the directory you entered; an `.envrc` is
  evaluated with cwd = its own directory.
- direnv does **not** chain parent `.envrc` files (no `source_up` -> parent is
  not loaded).
- direnv does **not** re-evaluate when moving between subdirectories governed by
  the same `.envrc`.

Consequence: direnv's **resolution** can be repo-based (the current `direnvrc`
already asks git), but its **trigger** cannot. It fires only when an `.envrc`
boundary is crossed, and those boundaries are paths.

Demonstrated with two sibling repos belonging to different accounts, sharing one
ancestor `.envrc` — the shape a worktree manager produces when it puts every
worktree under a single root:

```
in repoA (orgA): git says=AcctA  TESTTOK=token-for-A   correct
in repoB (orgB): git says=AcctB  TESTTOK=token-for-A   STALE, silently wrong
back in repoA:   git says=AcctA  TESTTOK=token-for-A   correct again, by luck
```

Git identity resolved correctly in both (URL-based rules are location
independent). The credential did not: direnv exported it once on entering the
parent scope and never re-evaluated. **This is a direct R8 violation and the
central reason the current design cannot be kept as-is.**

Note what this does *not* say: an isolated worktree in a foreign root resolves
fine, because entering it crosses into a different `.envrc` scope. The failure
is specifically **two directories needing different accounts under one `.envrc`**.

### `gh` has no per-repository account binding

`gh auth switch` takes only `--hostname`/`--user`, and the active account is a
single global field in `hosts.yml`, mutated process-wide — two terminals in two
repos share it. This is why `GH_TOKEN` (per-process) is used instead. Any design
that reintroduces global active-account state is a regression.

### Providers differ in *mechanism*, not just variable name

- GitHub accounts here use **https** remotes; git authenticates through the
  credential helper, which supplies `GH_TOKEN`.
- Digilope (Gitea) uses **ssh** remotes (`gitea@app-gitea.digilope.com:...`);
  git authenticates with a key via `core.sshcommand` and **no token is involved**
  in push/pull at all.
- Each provider's CLI reads its own variables: `gh` -> `GH_TOKEN`;
  `tea` -> `GITEA_TOKEN`/`GITEA_HOST`; `glab` -> `GITLAB_TOKEN`; az devops its
  own. CLI credentials are a separate axis from git transport.

### Unverified state

`~/.zshrc.local` exports `GITEA_TOKEN`, `GITEA_HOST`, `GITHUB_PAT`, and
`GITHUB_PAT_PROFOUND`. No consumer was found in the dotfiles, `~/.local/bin`,
direnv config, or MCP config, and `tea` has never been logged in
(`~/.config/tea` does not exist). Treat as unknown, not as dead.

---

## Requirements

### Resolution

- **R1 — Repo-based.** The account MUST be derived from the repository itself.
  The remote URL is the primary signal.
- **R2 — Location-independent.** Correct resolution MUST NOT depend on where the
  working tree sits. This explicitly includes linked worktrees placed in a
  separate root by external tooling (e.g. supacode), and clones made anywhere.
- **R3 — Multiple accounts per host.** `github.com/DanielCarmingham/**` and
  `github.com/EJ-Rice/**` MUST resolve to different accounts.
- **R4 — Defined behaviour with no remote.** A repo with no remote yet (freshly
  `git init`-ed) MUST resolve to a defined fallback, and that fallback MUST be
  stated rather than emergent.

### Coverage

- **R5 — Both axes.** The system MUST cover git transport auth *and* CLI
  credentials. Solving only one leaves a split-brain: committing as one account
  while authenticating as another.
- **R6 — Multi-provider, multi-CLI.** Adding a provider (Gitea, GitLab, Azure
  DevOps, Codeberg) MUST NOT require changing the resolution mechanism — only
  declaring the new provider's variables.
- **R7 — Both auth mechanisms.** Token-authenticated (https) and
  key-authenticated (ssh) accounts MUST both be first-class. An ssh account MUST
  NOT be required to invent a token it does not use.

### Safety

- **R8 — Never silently wrong.** A resolution failure MUST NOT fall back to a
  working-but-incorrect account. Wrong-and-quiet is worse than broken-and-loud —
  this is the exact regression the current `direnvrc` was introduced to fix and
  then briefly caused.
- **R9 — No global mutable state.** Credential selection MUST be per-process or
  per-invocation. No `gh auth switch`-style process-wide state.
- **R10 — Secrets stay untracked.** Token values MUST remain outside the `cfg`
  repo. Config that *names* variables is tracked; values are not.
- **R11 — No token leakage across accounts.** Entering a Gitea repo MUST NOT
  hand a GitHub token to Gitea tooling, and vice versa.

### Operability

- **R12 — Verifiable.** A checker MUST be able to assert the wiring is coherent
  and report drift, without printing secrets (fingerprints only).
- **R13 — Adding an account is a checklist.** Every step MUST be enumerable in
  one document, and ideally reduced to one place to edit.
- **R14 — Reproducible.** A fresh machine MUST be able to reach a working state
  from the `cfg` repo plus the untracked secrets file. Anything that cannot be
  committed (e.g. git-dir-local config) MUST be documented as a manual step.
- **R15 — Cheap.** Resolution runs on every shell entry or CLI invocation; it
  MUST NOT add noticeable latency.

---

## Design implications

These follow from the requirements; they are proposals, not requirements.

**Matching moves from `gitdir:` to `hasconfig:remote.*.url:`.** This satisfies
R1/R2/R3 directly and is a drop-in change to `.gitconfig-common` — the
per-account files do not change shape. Path rules can remain as a fallback for
repos with no remote (R4).

**Credentials keep being injected into the environment — the trigger changes.**
Environment injection is the only universal mechanism: IDEs, MCP servers, `act`,
and anything else that reads `GH_TOKEN` inherit the environment, and per-CLI
wrappers cannot reach any of them. So the answer is not to abandon env injection
but to fire it **per directory change** rather than per `.envrc` boundary — a
zsh `chpwd` hook that re-resolves the account from git and re-exports. Measured
cost of the resolution is ~12 ms, which is acceptable per `cd` and can be cached
by git-dir if it ever isn't.

**direnv is retained for what it is good at** — project environments (`use
flake`, per-project vars) — and stops being the credential source of truth. The
account-root `.envrc` files can then lose their explicit token exports, since
the hook covers every directory rather than only boundaries.

**Wrappers are a backstop, not the mechanism.** They remain useful only for the
case no shell hook can reach: a GUI app launched outside a shell (VS Code from
the Dock). Note direnv has the same blind spot today, partly covered by the
VS Code direnv extension.

**The `[account]` schema stays.** Provider-neutral declaration
(`provider`, `name`, `gitAuth`, `env`) is orthogonal to how the account is
matched, so work already done on it survives the change of matcher.

### Coverage of the candidate design

| Case | git identity | Credentials in environment |
|---|---|---|
| repo in account root | url rule | chpwd hook |
| worktree in a foreign root | url rule | chpwd hook |
| **many worktrees under one root** | url rule | chpwd hook (direnv fails here) |
| clone anywhere | url rule | chpwd hook |
| new repo, no remote | path rule or explicit fallback | fallback, loudly |
| ssh-auth provider (Gitea) | url rule + `core.sshcommand` | hook (only if a CLI needs it) |
| GUI app launched outside a shell | url rule | **not covered** — wrapper or IDE extension |

---

## Open questions

1. **Multiple remotes.** `hasconfig:remote.*.url` matches if *any* remote
   matches. A fork with `origin` (personal) and `upstream` (work) matches both.
   Which wins, and should ambiguity be an error?
2. **Does supacode use `git worktree add` or a fresh clone?** Worktrees survive
   path rules today; clones do not. This decides how urgent the migration is.
3. **Fallback identity for remote-less repos** — personal, or refuse to resolve
   and make the user choose?
4. **Do the wrappers need to cover `git` itself,** or is `includeIf` +
   credential helper + `core.sshcommand` sufficient for transport?
5. **The four unverified variables** — confirm consumers before removing.
6. **Is `[github] account` still needed** once `[account]` exists, or is it dead
   weight kept only for back-compat?
