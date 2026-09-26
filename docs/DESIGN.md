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

## Terminology

"Identity" carries two unrelated meanings around this project, each correct in
its own vocabulary. They are worth separating once.

| Term | Means | Set by |
|---|---|---|
| **git identity** | `user.name`, `user.email` and `core.sshcommand` | the generated per-account gitconfig, included per remote URL |
| **credential** | a token: git's https password, or what a CLI authenticates as | `gh` or `tea`, read on demand |

The collision is not sloppiness. "Identity" is git's own word for commit
authorship, reused here for the generated gitconfig that sets it. Nothing is
renamed to avoid it, because the name is right where it is used; this table
exists so the overlap can be read rather than guessed at.

**The ssh key sits on the identity axis while being a credential in every
ordinary sense.** That is forced, not chosen. `core.sshcommand` can only be
delivered per-remote through a config include, which *is* the identity
mechanism, and a credential helper is only ever consulted for https. So there is
nowhere else to put it.

The consequence is worth stating plainly, because it decides how bad a wrong
answer is:

- On an **https** remote the axes are independent. Identity writes the author
  into the commit; a token authenticates the push. A wrong identity gives you
  correctly-authenticated commits with the wrong author in your history, and
  nothing fails.
- On an **ssh** remote identity *is* the authentication, since it names the key,
  and no token is involved at all (R7). A wrong identity usually fails at the
  server instead of landing quietly in history.

An account names a **provider** and a **login**, never a value (R10): the
provider decides which variables its tools read, and the login is which one of
that provider's CLI logins holds the token. `accounts.toml` itself carries no
secret, so it has nothing to leak.

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
- Each provider's tools read their own variables, and the provider decides all
  of them — not the config, which never names one:

  | Provider | Variable | Value |
  |---|---|---|
  | github | `GH_TOKEN` | token |
  | github | `GITHUB_PERSONAL_ACCESS_TOKEN` | token |
  | gitea | `GITEA_TOKEN` | token |
  | gitea | `GITEA_INSTANCE_URL` | url |
  | gitea | `GITEA_ACCESS_TOKEN` | token |
  | gitea | `GITEA_HOST` | url |

  Sources: `gh help environment` (gh 2.101.0); the `github-mcp-server` and
  `gitea-mcp` READMEs; `tea`'s `GetLoginByEnvVar` plus the fake-host
  measurement below.

  Measured with tea 0.15.1 against unresolvable `.invalid` hosts, reading the
  host each run dialed: an env login needs *both* `GITEA_TOKEN` and
  `GITEA_INSTANCE_URL` before `tea` builds one, and that login then overrides
  whatever is stored in its own config; `GITEA_HOST` is read by nothing in
  `tea` and is ignored without a word. `tea login helper get` returns the
  *first* login for a host regardless of which user was asked for. `tea login
  ls -o json` never prints a token, for any login. Against the author's real
  login (2026-09-26, 15 runs, token discarded) `tea login ls -o json` takes a
  median 14 ms and `tea login helper get` 31 ms -- the helper decrypts
  `credentials.json.enc` through the keychain, which plaintext fake logins
  (17 ms) skip.

Transport is a property of a **remote**, not of an account: git picks it per
remote, and a credential helper is only ever consulted for https. An account
using both needs no special case, which is why there is no `gitAuth` field to
get wrong.

Every token is read from `gh` or `tea` **per invocation** rather than kept: no
second copy exists to disagree with the one the CLI holds, and rotation or
revocation is visible on the next call rather than leaving gitwho holding a
credential that is present, decryptable and wrong. **A provider's CLI that
cannot answer is an error, never a fallback** — not to another source, and not
to another account (R8).

### Why there used to be a secret store, and why there no longer is

Through 0.2, gitwho kept its own age-encrypted file and read tokens out of it.
That store is gone as of 0.3: `gh` and `tea` already hold every token gitwho
needs, keeping a second copy meant it could disagree with the one the CLI
holds, and reading it on demand rather than caching it is what makes rotation
and revocation visible on the next call instead of silently stale (R6, R8).

Two of the measurements that shaped the store while it existed are worth
keeping, because they explain why neither platform's own secret storage was
going to beat asking the CLI directly, and are cited nowhere else:

**macOS keys a Keychain ACL to the calling binary's designated requirement.**
For an unsigned binary that is its code hash, so *every rebuild* presents as a
new application and a read blocks on a GUI prompt — verified by writing an
entry with one build and reading it with the next, which hung until killed.
Anything on the credential-helper hot path fails the latency requirement
outright if it depends on this. `codesign` then blocked on a second prompt for
the signing key, so "sign it with a stable identity" remains plausible but
unverified. (Verified with a small keychain probe, since removed along with
the store it was written to test.)

**age's passphrase mode is far too slow.** scrypt is deliberately expensive:
measured at **1.53 s per read**, against a budget in milliseconds. That is why
the store, while it existed, used an x25519 identity rather than a passphrase.

Neither finding is about `gh` or `tea`'s own storage: `gh` keeps its token in
the macOS keychain under its own signed identity, and `tea` keeps its tokens in
`credentials.json.enc` with the key in the keychain — both were built and
signed by someone else, so the ACL and signing problems above are theirs to
have already solved, not gitwho's.

**Resolving `gh` normally re-enters gitwho.** The shim directory leads `PATH`
and its `gh` runs `gitwho exec -- /real/gh`, so asking `gh` for a token calls
itself. The runner skips its own shim directories. This still matters with the
store gone, because it is exactly how `exec` gets a GitHub token today.

---

## What this protects, and what it does not

Worth being blunt, because "gitwho decides who gets logged in" invites an
assumption it does not earn.

gitwho holds no secret of its own. A token lives in `gh`'s keychain entry, or
in `tea`'s `credentials.json.enc` with its key in the keychain — both owned by
you, both readable by anything running as you. Anything running as your user
can ask for the same token gitwho would get, exactly the way gitwho asks:

```sh
gh auth token --hostname github.com --user <login>
```

That is no harder than what gitwho itself does to answer a credential request,
so **against code running as your user, gitwho is not an improvement on asking
`gh` or `tea` directly, and must not be described as one.**

What gitwho changes is exposure over time: a token is fetched by the one
process that needs it, for one invocation, rather than sitting in the ambient
environment where every process you launch for the whole session inherits it.
That is the property that matters day to day, and it holds whether the token
came from `gh`, from `tea`, or — through 0.2 — from gitwho's own store.

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
         │   gh / tea (CLI)     │  the token source, asked on demand
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

`init` sits above all of this and adds no capability of its own — it calls the
same entry points in the order the setup guide gives, and owns only the two
lines that live in files gitwho does not control. Keeping it a pure
orchestrator is what makes it safe to re-run.

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
   Non-origin remotes are ignored *here*, and a fork's `upstream` still
   authenticates correctly at fetch time because the helper resolves per-URL.
   The generated identity rules are a separate mechanism and do **not** ignore
   them — see "A repo whose remotes span two accounts" under Known limits.
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
- **R6 — Multi-provider, multi-CLI.** Adding a provider is a row in
  `provider.rs` plus its token source, with a test — not configuration. Asking
  users to restate which variables a tool reads is how `GITEA_HOST` came to be
  documented for a tool that never reads it.
- **R7 — Both auth mechanisms.** Pushing over ssh never involves a token; every
  account still names a CLI login, and `doctor` reports a missing one.

### Safety

- **R8 — Never silently wrong.** A resolution failure does not fall back to a
  working-but-incorrect account. Wrong-and-quiet is worse than broken-and-loud.
- **R9 — No global mutable state.** Credential selection is per-process or
  per-invocation. No `gh auth switch`-style process-wide active account.
- **R10 — Secrets never enter this repo, and gitwho stores none.**
  `accounts.toml` names logins, the CLIs hold tokens. `accounts.toml` is
  designed to live in a dotfiles repo.
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
  transport operation. gitwho's own share — parsing `accounts.toml` and
  matching remotes — is held to single-digit-to-low-double-digit milliseconds.
  There are no stored values to read any more. The real cost is the token
  fetch on top of that, paid on every https git operation and every `exec`:
  one `gh auth token` spawn, 68 ms median (gh 2.101.0), or two tea spawns,
  `tea login ls -o json` and `tea login helper get`, 14 ms and 31 ms median
  (tea 0.15.1, real keychain-backed login). Measured 2026-09-26, 15 runs each.

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

- **`jj` gets the credentials but not the identity.** Measured with jj 0.44.0.
  Its remote operations go through git, so a push over https is served by the
  credential helper like any other — that half works. But jj keeps author
  identity in its own config and **does not read gitconfig's `includeIf` rules
  at all**, so `user.email` comes from `~/.config/jj/config.toml` regardless of
  what git resolves. In a colocated repo the two can therefore disagree, and
  the disagreement is silent: `git config user.email` and `jj config get
  user.email` return different addresses, and which one lands on a commit
  depends on which binary you happened to use.

  jj's own answer is `[[--scope]]` with `--when.repositories = [...]` — which
  is **path-based and ordering-sensitive**, the exact shape this project exists
  to replace. Mirroring the accounts there by hand works, and is what the author
  currently does, but it is a second source of truth that drifts.

  Covering it properly means generating that jj config from `accounts.toml` the
  way `sync` generates the gitconfig. Nothing here does that yet.

- **A repo whose remotes span two accounts takes the wrong identity.**
  Measured 2026-08-22 with git 2.54.0 (Apple Git-157). Credentials are fine:
  the helper is asked per URL at transport time, so a github `origin` and a
  gitea `upstream` each authenticate as their own account. Identity is not.
  `sync` emits one `includeIf "hasconfig:remote.*.url:"` per account, and that
  keyword matches when **any** remote matches — so both rules apply and git's
  last-include-wins hands `user.email` and `core.sshcommand` to whichever
  account `accounts.toml` declares last. Not to `origin`.

  It cannot be fixed in the generated rules: `hasconfig:remote.origin.url:` is
  not a supported keyword, and a rule using it silently never matches while the
  same pattern under `remote.*.url` does. Both halves were measured on one
  fixture repo with only the rule text changed.

  So `doctor` reports it instead — a `warn`, because a repo that genuinely
  spans two accounts has no single right answer and only the person who set it
  up knows which should sign the commits. The finding names both accounts, says
  which one wins and why, and stops once the repo pins its own identity with a
  local `user.email` or `include.path`, which beats every included global rule.

- **A GUI application launched outside a shell** — an editor started from the
  Dock — inherits no shim `PATH` and no environment. Git identity and git
  transport are still correct there, because both come from gitconfig. A CLI
  invoked from inside that application is not covered.
- **A token the server has revoked but the CLI still hands out looks healthy
  to `doctor`**; detecting it needs the network (`doctor --check-remote`,
  planned).
- **Two tea logins on one host are unsupported** (tea's helper is asked by host
  alone and picks the first login for it), even when they are different
  servers under different paths, or `http` and `https`.
- **`exec` fetches the resolved account's token whatever program it runs.**
  `gh` in a repository that resolves to a Gitea account asks tea for that
  account's token first, so it fails if tea is missing or logged in as someone
  else. That is loud, not wrong: no token is invented and none from another
  account is used.
