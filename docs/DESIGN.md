# Design

Why gitwho is shaped the way it is, and the measurements that decided it.

Everything under "Evidence" was run, not assumed. Where a result depends on a
tool's version, the version is given. All of it was measured on macOS with
git 2.50.1, direnv 2.37.1 and gh 2.97.0, during August 2026.

---

## The problem

Two things must be chosen per repository, and they are usually chosen by two
unrelated mechanisms that nothing keeps in agreement:

| Axis | What it decides | Usually chosen by |
|---|---|---|
| **git identity** | author name/email, ssh key | `includeIf "gitdir:"` path rules |
| **CLI credentials** | what `gh`/`tea`/`glab` and MCP servers authenticate as | a token exported per directory, typically by direnv |

Path is the wrong key. An account is a property of **the repository** — which
server it lives on, under which organisation — not of where it happens to sit
in the filesystem. Keying on path means the two mechanisms can disagree, and
when they do you commit as one person while authenticating as another.

---

## Evidence

### Path rules break on relocation, silently

| Scenario | `gitdir:` | `hasconfig:remote.*.url:` |
|---|---|---|
| repo inside the account root | resolves | resolves |
| `git worktree add` to a path outside the root | resolves | resolves |
| repo cloned outside the root, same remote | **does not resolve** | resolves |

The worktree row survives only incidentally: a linked worktree's `$GIT_DIR` is
`<main-repo>/.git/worktrees/<name>`, still inside the account root, so the path
rule still matches. That protection disappears the moment a tool **clones**
rather than adds a worktree, and it says nothing about a repo cloned elsewhere
by hand. When it fails it fails silently, falling back to whichever account is
the default.

`hasconfig:remote.*.url:` needs git ≥ 2.36.

### A per-directory environment cannot be the trigger

Measured with direnv 2.37.1:

- `direnvrc` is evaluated with cwd = the directory you entered; an `.envrc` is
  evaluated with cwd = its own directory.
- direnv does **not** chain parent `.envrc` files.
- direnv does **not** re-evaluate when moving between subdirectories governed
  by the same `.envrc`.

So direnv's *resolution* can be repo-based — asking git which account owns the
repo is easy — but its *trigger* cannot. It fires when an `.envrc` boundary is
crossed, and those boundaries are paths.

Demonstrated with two sibling repos belonging to different accounts under one
ancestor `.envrc`, which is the shape a worktree manager produces when it puts
every worktree under a single root:

```
in repoA (orgA): git says=AcctA  TESTTOK=token-for-A   correct
in repoB (orgB): git says=AcctB  TESTTOK=token-for-A   STALE, silently wrong
back in repoA:   git says=AcctA  TESTTOK=token-for-A   correct again, by luck
```

Identity resolved correctly in both, because URL-based rules are
location-independent. The credential did not: it was exported once on entering
the parent scope and never re-evaluated.

Note what this does *not* say. An isolated worktree in a foreign root is fine,
because entering it crosses into a different `.envrc` scope. The failure is
specifically **two directories needing different accounts under one `.envrc`**.

### `gh` has no per-repository account binding

`gh auth switch` takes only `--hostname`/`--user`, and the active account is a
single global field in `hosts.yml`, mutated process-wide — two terminals in two
repos share it. Per-process environment variables are the only per-repository
mechanism `gh` offers. Any design that reintroduces global active-account state
is a regression.

### The credential helper already knows everything it needs

Three probes, and the third is the result the whole design rests on:

1. **A credential helper runs with cwd = the repo worktree.** Probed with an
   inline `-c credential.helper=!pwd`; it printed the repo path.
2. **`GIT_DIR` is not exported to the helper.** Only `GIT_PREFIX`,
   `GIT_EXEC_PATH`, `GIT_EDITOR` and `GIT_CONFIG_PARAMETERS` are. Resolution
   must ask git from cwd rather than read an environment variable.
3. **`credential.useHttpPath = true` puts the organisation in the request.** A
   fill for `https://github.com/SomeOrg/somerepo.git` yields
   `path=SomeOrg/somerepo.git`.

Finding 3 means the helper can resolve the account **at clone time, before any
repository exists on disk**. Git transport therefore needs no shell hook and no
path rule at all. Without `useHttpPath`, the helper is told `github.com` and
nothing more, so every account on that host resolves identically — a total and
silent failure.

### Providers differ in mechanism, not just in variable name

- A GitHub account with https remotes authenticates git through the credential
  helper, which supplies a token.
- A self-hosted Gitea or Forgejo is commonly reached over ssh for push/pull —
  git authenticates with a key via `core.sshcommand`, and no token is involved
  in the transport path — *and* over https for its API, which does need one.
- Each CLI reads its own variables: `gh` → `GH_TOKEN`; `tea` → `GITEA_TOKEN`
  and `GITEA_HOST`; `glab` → `GITLAB_TOKEN`; `az devops` its own.

Transport is a property of a **remote**, not of an account: git picks it per
remote, and a credential helper is only ever consulted for https. An account
using both needs no special case, which is why there is no `gitAuth` field to
get wrong.

### The secret store: two measurements that overturned the first choice

The platform keychain was the obvious store, and both reasons it is not the
default are reproducible.

**macOS keys a Keychain ACL to the calling binary's designated requirement.**
For an unsigned binary that is its code hash, so *every rebuild* presents as a
new application and the read blocks on a GUI prompt — verified by writing an
entry with one build and reading it with the next, which hung until killed.
The credential helper runs on every git transport operation, so this fails the
latency requirement outright. `codesign` then blocked on a second prompt for
the signing key, so "sign it with a stable identity" remains plausible but
unverified. `examples/keychain_probe.rs` is how to confirm it.

**age's passphrase mode is far too slow.** scrypt is deliberately expensive:
measured at **1.53 s per read**, against a budget in milliseconds. Switching to
an x25519 identity removes the KDF entirely and brings a read under a
millisecond — the test suite went from 3.05 s to 0.00 s. A regression test now
pins reads under 100 ms so a KDF cannot creep back in.

The other two platform stores are implemented and reachable, but nothing
chooses them automatically, because they do not draw the same boundary:

| Store | Keeps a secret from |
|---|---|
| macOS Keychain | *other applications*, via a per-application ACL |
| Windows Credential Manager (DPAPI) | other users |
| Linux Secret Service | other users |

Only the first is stronger than an age file that is already `0600` and owned by
you. On the other two, flipping the default would buy nothing measurable.

---

## What this protects, and what it does not

Worth being blunt, because the encryption invites an assumption it does not
earn.

The identity key sits on disk beside the encrypted secrets, both owned by you.
Anything running as you can read both and decrypt. So the age file defends the
secrets **at rest** — a backup, a sync folder, an accidental commit — and not
against a local process.

The cross-project exposure this tool exists to remove comes from somewhere
else: **secrets are fetched on demand by the one process that needs them,
instead of sitting in the ambient environment where every unrelated process
inherits them.** That property holds for either backend, and it is the one that
matters day to day.

---

## Architecture

Four entry points into one resolver. Nothing shares mutable global state.

```
                    ┌──────────────────────────┐
                    │  ~/.config/gitwho/       │
                    │      accounts.toml       │  ← the only file you edit
                    └───────────┬──────────────┘
                                │
                     ┌──────────▼──────────┐
                     │      resolver       │  URL → account, else cwd-repo → account
                     └──────────┬──────────┘
                                │
   ┌───────────────┬────────────┼─────────────┬──────────────────┐
   │               │            │             │                  │
┌──▼───────────┐ ┌─▼────────┐ ┌─▼─────────┐ ┌─▼──────────┐  ┌────▼────────┐
│ credential   │ │  exec    │ │   sync    │ │  doctor    │  │  mcp sync   │
│ get/store/   │ │ (CLIs,   │ │ generate  │ │ read-only  │  │  rewrite    │
│ erase        │ │  via     │ │ gitconfig │ │ report,    │  │ .mcp.json   │
│              │ │  shims)  │ │           │ │ fingerpr.  │  │             │
└──────┬───────┘ └────┬─────┘ └───────────┘ └────────────┘  └─────────────┘
       │              │
       └──────┬───────┘
         ┌────▼─────────────────┐
         │  secrets backend     │  trait: AgeFile | Keychain | Env
         └──────────────────────┘
```

**Why this shape.** Git transport is handled entirely by the credential helper
(finding 3), so it never needs the environment. CLIs and MCP servers are
processes gitwho launches, so `exec` scopes credentials to exactly one process.
Identity is static config, so `sync` generates it once. That leaves *nothing*
requiring an ambient token — which kills the exposure at the root rather than
re-triggering it more often.

`git` itself is shelled out to rather than linked as a library. That guarantees
behaviour matches the installed git's own config resolution, and one
`git config` call is ~5 ms, inside budget.

---

## Resolution

One function, one output — `Resolved { account, reason }`. The `reason` is what
makes failures loud instead of quiet.

1. **A URL is given** (the credential helper, or an explicit account): match
   `host/path` against every account's `match` globs. **Most specific wins**,
   measured by how many characters a pattern pins literally, so nested
   organisations are not order-sensitive. Two accounts tying on specificity is
   an **error**, not a coin flip.
2. **No URL, cwd is a repo**: read `remote.origin.url` from git, then step 1.
   Non-origin remotes are ignored for identity — a fork's `upstream` still
   authenticates correctly at fetch time, because the helper resolves per-URL.
3. **A repo with no `origin`**: longest-prefix match on the declared `paths`.
   Longest wins, so a nested root beats the root containing it without
   depending on declaration order.
4. **A remote that no account claims**: the declared default account, tagged
   `Unmatched`.
5. **No remote and no path match**: the declared default account, tagged
   `Default`.

The resolver never guesses, and the tag is load-bearing. `reason` is one of:

| `reason` | Meaning | Confident? |
|---|---|---|
| `UrlMatch` | a URL matched the account's patterns | yes |
| `OriginUrl` | the repo's `origin` matched | yes |
| `PathFallback` | no remote; a declared directory prefix matched | yes |
| `Unmatched` | there *is* a remote and nothing claimed it | **no** |
| `Default` | nothing to go on at all | **no** |

The last two both land on the same default account, and separating them is the
point: `Unmatched` is what a *forgotten pattern* looks like as well as what a
third-party clone looks like, and the two are indistinguishable from here.

So a repo that matched nothing resolves to the default, where `gh` will work
and say that nothing claimed the remote, while the credential helper
**refuses**. That asymmetry is deliberate. A wrong account served quietly is
the failure this tool was built to prevent.

---

## Design principles

The requirements the implementation is held to. R1–R15 are referenced by number
throughout the source.

### Resolution

- **R1 — Repo-based.** The account is derived from the repository itself. The
  remote URL is the primary signal.
- **R2 — Location-independent.** Correct resolution does not depend on where
  the working tree sits: linked worktrees placed in a separate root by external
  tooling, and clones made anywhere.
- **R3 — Multiple accounts per host.** Two organisations on `github.com`
  resolve to different accounts.
- **R4 — Defined behaviour with no remote.** A freshly `git init`-ed repo
  resolves to a fallback that is *stated* rather than emergent.

### Coverage

- **R5 — Both axes.** Git transport auth *and* CLI credentials. Solving only
  one leaves a split brain: committing as one account while authenticating as
  another.
- **R6 — Multi-provider, multi-CLI.** Adding a provider does not require
  changing the resolution mechanism — only declaring the new provider's
  variables. Adding Codeberg is about six lines with `provider = "gitea"` and a
  different host.
- **R7 — Both auth mechanisms.** Token-authenticated (https) and
  key-authenticated (ssh) accounts are both first-class. An ssh account is not
  required to invent a token it does not use.

### Safety

- **R8 — Never silently wrong.** A resolution failure does not fall back to a
  working-but-incorrect account. Wrong-and-quiet is worse than broken-and-loud.
- **R9 — No global mutable state.** Credential selection is per-process or
  per-invocation. No `gh auth switch`-style process-wide active account.
- **R10 — Secrets stay out of version control.** Config that *names* variables
  is committable; values are not. `accounts.toml` is designed to live in a
  dotfiles repo.
- **R11 — No token leakage across accounts.** Entering a Gitea repo does not
  hand a GitHub token to Gitea tooling, or the reverse. `exec` scrubs every
  managed variable before injecting the resolved account's.

### Operability

- **R12 — Verifiable.** `doctor` asserts the wiring is coherent and reports
  drift without printing secrets — fingerprints only.
- **R13 — Adding an account is a checklist.** Every step is enumerable in one
  document, and reduced to one place to edit.
- **R14 — Reproducible.** A fresh machine reaches a working state from a
  dotfiles repo plus re-entered tokens. Anything that cannot be committed is
  documented as a manual step rather than discovered.
- **R15 — Cheap.** Resolution runs on every CLI invocation and every git
  transport operation. The budget is single-digit milliseconds.

---

## Platform code is a parameter, never a `cfg!`

`paths::Layout` and `shim::ShimTarget` are arguments, with the host's value
supplied only by `main`. This is not style: a `cfg!(windows)` branch is
unreachable from a test run on macOS, so it can be *documented* as covered
while nothing can execute it — which is exactly what happened to an earlier
`APPDATA` branch before this rule existed.

The consequence is that the Windows behaviour is unit-tested as pure functions
and has still never run on Windows. Both halves of that sentence are true, and
the second is the one to act on.

---

## Known limits

- **A GUI application launched outside a shell** — an editor started from the
  Dock — inherits no shim `PATH` and no environment. Git identity and git
  transport are still correct there, because both come from gitconfig. A CLI
  invoked from inside that application is not covered.
- **A stored-but-dead token looks identical to a working one.** `doctor`
  reports that a value exists, not that the provider still accepts it.
- **`provider` is required by the parser and read by nothing.** `mcp sync`
  recognises provider servers from their command. It is declared for the day
  something needs it.
