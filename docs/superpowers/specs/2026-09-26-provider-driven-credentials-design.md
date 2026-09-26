# Provider-driven credentials

Status: approved design, not yet implemented. Target release: 0.3.0 (breaks the
`accounts.toml` format).

## Why

An account's `env` list currently mixes three different things in one syntax: a
bare name (fetch from gitwho's store), `NAME=value` (a literal), and
`{ var, from, user }` (fetch from another tool). Knowing which variable names a
tool reads is left to the user, and it has already gone wrong twice:

- The documented `GITEA_HOST` is not read by `tea` at all. `tea` builds a login
  from the environment only from `GITEA_TOKEN` + `GITEA_INSTANCE_URL`; with
  `GITEA_HOST` it silently falls back to its own stored login (tea 0.15.1,
  measured 2026-09-25 against `.invalid` hosts).
- `gitea-mcp` reads a *different* pair, `GITEA_HOST` + `GITEA_ACCESS_TOKEN`, so
  one account serving both tools would have to store the same token twice under
  two names, and rotation could leave them disagreeing without a word.

Knowing which variables each tool reads is gitwho's job. The account should say
what it *is*: its provider, its server where one is needed, and which login
holds its token.

## Decisions

1. **The provider decides the variables.** A table in the code maps each
   provider to the variables its tools read. There is no escape hatch for
   extra variables: a tool the table does not know needs a code change and a
   test.
2. **Tokens come only from the provider's own CLI.** `gh` for GitHub, `tea` for
   Gitea/Forgejo. gitwho stores no secret, and its secret store is removed.
3. **Old configs fail loudly** with an error naming the removed field. There is
   no automatic converter; the one existing config is converted by hand.

This reverses two recorded positions, and DESIGN.md is rewritten to say so:
R6 ("adding a provider only needs declared variables"), and the example
config's note that the git password is deliberately not inferred from the
provider. The evidence above is the reason for both reversals.

## Config schema

```toml
[[accounts]]
name     = "Work"
provider = "github"
login    = "work-login"                  # the gh login; required
email    = "you@example-corp.com"
match    = ["github.com/example-corp/**"]

[[accounts]]
name     = "SelfHosted"
provider = "gitea"                       # "forgejo" is accepted as the same thing
url      = "https://git.example.net"     # required for gitea; rejected for github
login    = "you"                         # the tea login's user; required
email    = "you@example.net"
match    = ["git.example.net/**", "ssh.git.example.net/**"]
sshKey   = "~/.ssh/id_ed25519_selfhosted"
paths    = ["~/src/selfhosted/"]
```

- `provider`: `github` | `gitea` | `forgejo` (alias of `gitea`). Anything else
  is a parse error.
- `login`: required on every account. It is the login gitwho asks the provider's
  CLI for; the account `name` stays a label. Deliberately not called `user`:
  next to `gitName` and `email` that reads as git's `user.name`, which it has
  nothing to do with.
- Git identity is untouched: `gitName` (or `[defaults] gitName`) still becomes
  `user.name`, `email` becomes `user.email`, and `sshKey` becomes
  `core.sshcommand`, keyed by `match`/`paths` exactly as today.
- `url`: required for `gitea`, rejected for `github`. `github` means github.com;
  GitHub Enterprise (`GH_ENTERPRISE_TOKEN` + `GH_HOST`) is out of scope until
  needed.
- `email`, `gitName`, `sshKey`, `match`, `paths`, `[defaults] account` and
  `[defaults] gitName` are unchanged.
- **Removed:** `env`, `gitCredential`, `[defaults] secretBackend`. Their presence
  is a parse error naming the field, what replaces it, and pointing at
  `docs/accounts.toml.example`.

## Provider table

| Provider | Variable | Value | Read by |
|---|---|---|---|
| github | `GH_TOKEN` | token | `gh` |
| github | `GITHUB_PERSONAL_ACCESS_TOKEN` | token | `github-mcp-server` |
| gitea | `GITEA_TOKEN` | token | `tea` |
| gitea | `GITEA_INSTANCE_URL` | url | `tea` |
| gitea | `GITEA_ACCESS_TOKEN` | token | `gitea-mcp` |
| gitea | `GITEA_HOST` | url | `gitea-mcp` |

Sources: `gh help environment` (gh 2.101.0); the `github-mcp-server` and
`gitea-mcp` READMEs; `tea`'s `GetLoginByEnvVar` plus the fake-host measurement.

**Always cleared** by `exec`, independent of the config: every variable above,
plus fallbacks gitwho never sets but tools still read — `GITHUB_TOKEN` (gh's
fallback for `GH_TOKEN`), `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`.
`tea` also falls back to `GH_TOKEN` when `GITEA_TOKEN` is empty, which the fixed
list covers.

## Getting a token

One function: account in, token or error out. The value is never printed;
`doctor` shows fingerprints only.

- **github:** `gh auth token --user <login> --hostname github.com`, through the
  existing runner, which already skips gitwho's own shim directories.
- **gitea:**
  1. `tea login ls -o json`. It returns `name`, `url`, `ssh_host`, `user`,
     `default` and never a token (verified with fake logins).
  2. Require **exactly one** login whose `url` equals the account's `url`, and
     require its `user` to equal the account's `login`. Otherwise fail, saying
     what `tea` has.
  3. Only then run `tea login helper get` for that host and take `password=`.

  The guard exists because `tea login helper get` returns the *first* login for
  a host, ignoring both the requested username and the login marked default
  (measured with two fake logins on one host). Consequence: **two accounts on
  one Gitea server are not supported**; gitwho refuses rather than guess.
- Empty output, a missing CLI, or a non-zero exit is an error. Nothing falls
  back to another source or another account (R8).

## Runtime

- **Resolution** is unchanged: remote URL, then `paths`, then the default, with
  `whoami` giving the reason.
- **`exec`:** clears the fixed list, then sets the resolved account's variables:
  its token under every token name for its provider, and `url` under every URL
  name. If getting the token fails, the command does not run.
  Credential-establishing commands (`gh auth login`, `tea login add`, …) still
  pass through with everything cleared and nothing injected.
- **Credential helper:** password is the account's token; **username is the
  account's `login`** (was the account name). GitHub ignores the username when
  the password is a token; for Forgejo the real login is the correct value.
  Unmatched remotes are still refused.
- **R7 changes shape.** Pushing over ssh still never involves a token. But
  every account now names a CLI login, and `doctor` reports an account whose
  login is missing as a problem -- so an ssh-only account is expected to have a
  `gh`/`tea` login too. DESIGN.md's R7 is rewritten to say so.
- **`sync`:** every account now has a token, so the credential helper is wired
  for every host in any account's `match`. Hosts only ever used over ssh are
  unaffected, because git consults a credential helper only for https.
- **`mcp sync`:** unchanged. Servers already run through `exec`, which now
  supplies `gitea-mcp`'s variables with no config.
- **Performance (R15):** github about 60 ms (one `gh` spawn, as today). gitea:
  `tea login ls -o json` median 14 ms, `tea login helper get` median 17 ms with
  plaintext fake logins. The real `helper get` also decrypts through the
  keychain; time it during implementation, discarding the token.

## Commands

| Command | Change |
|---|---|
| `secret …` | Removed. |
| `renew` | Removed. Errors and `doctor` print the exact fix instead: `gh auth login --hostname github.com`, or `tea login add --url <url>`, both of which already run through the shims untouched. |
| `init` | No store. Scaffolds `accounts.toml`, then sync, shims, `doctor`. |
| `init --discover` | Proposes `provider`/`url`/`login`. For `login` it offers the logins the CLI already holds -- `gh auth status` (read today by `discover::gh_logins`, names only) and `tea login ls` -- since which login owns an org cannot be known from a remote. |
| `whoami` | Shows provider, which `gh`/`tea` login supplies the token, and the variable names `exec` would set. Never values. |
| `doctor` | See below. |

`doctor`:

- **Keeps:** `accounts.toml` at `0600` (a writable config can point `match` at a
  host its writer controls), git wiring and identity checks, and the warning
  when an always-cleared variable is exported in the shell.
- **Removes:** store permission, backend and secret checks; the
  `GITEA_TOKEN`/`GITEA_INSTANCE_URL` pair check, which the schema makes
  impossible to get wrong.
- **Adds:** per account, that its `gh` login or guarded `tea` login answers,
  reported as a fingerprint, or as a problem whose message says exactly what
  to run. It distinguishes what can be known locally: the CLI is missing, the
  CLI has never heard of this login, or (tea) the logins it holds do not match.
  A token the CLI still hands out but the server has revoked looks healthy
  here; telling those apart needs a network call (`gh auth status` checks
  online), which `doctor` deliberately does not make. That belongs to the
  already-planned opt-in `doctor --check-remote`.
- **Reports** leftover `secrets.age` / `identity.key` as unused and safe to
  delete. It never deletes them.

## Dependencies and docs

- Remove `age`, `keyring`, `rpassword`. `rust-version` stays 1.88; its comment
  cites `keyring` and is rewritten.
- README, INSTALL, `docs/accounts.toml.example` (also `init`'s template via
  `include_str!`), CLAUDE.md and the site are updated.
- DESIGN.md: rewrite R6 and R10; drop the "age identity" row from the
  terminology table; rewrite "What this protects" for tokens held by `gh`
  (macOS keychain) and `tea` (its own `credentials.json.enc`, key in the
  keychain). gitwho no longer decrypts anything. Record the `tea` findings above
  with version and date.

## Testing

Test-first; each new test is seen to fail before the code that passes it.

- **Config:** new schema parses; `forgejo` equals `gitea`; each removed field
  rejected with an error naming it; gitea without `url`, github with `url`, and
  a missing `login` rejected.
- **Tokens** (fake runner, no real `gh`/`tea`): GitHub passes the right
  `--user`; tea with zero, two, or one wrong-login matching logins fails and
  never calls `helper get`; exactly one right login returns the token; empty
  helper output and a missing `tea` fail. Every error is asserted not to
  contain the token.
- **`exec`:** github sets 2 variables, gitea 4; stray `GITHUB_TOKEN` /
  `GITEA_TOKEN` are cleared; credential-establishing commands pass through.
- **Credential helper:** username is `login`; unmatched still refused.
- **`doctor`:** token checks; leftover store files reported, not deleted.
- The 100 ms store-read regression test goes with the store.

## Implementation order

Two commits, each building and passing on its own:

1. New schema, provider table, CLI token sources, runtime and commands. The
   store code is no longer reachable.
2. Delete the store: `secrets` modules, backends, the `secret` and `renew`
   commands, their tests and dependencies, and the store checks in `doctor`.

## Rollout on the author's machine

1. Confirm `tea`'s login for the Gitea account is the right one: `tea whoami`
   with no gitwho variables set.
2. Convert `~/.config/gitwho/accounts.toml` by hand.
3. `gitwho doctor`, then `gh` and `tea` through the shims, and an https
   `git ls-remote` for each provider.
4. Only then delete `secrets.age` and `identity.key`, by hand.
