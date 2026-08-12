# Installing gitwho

Setting up gitwho on a machine that has never run it.

If you are migrating off an existing path-based setup rather than starting from
nothing, do it in this order: get gitwho working alongside what you have (its
`[include]` line placed *after* your existing `includeIf "gitdir:"` rules, so
later-wins hands control over while both are present), verify with the checks
below, and only then remove the old rules. Every step here is reversible by
deleting one line.

Why it is written this way is in [DESIGN.md](DESIGN.md).

## What you need

| | |
|---|---|
| Rust toolchain | to build; there are no prebuilt binaries |
| git ≥ 2.36 | `includeIf "hasconfig:remote.*.url:"` landed in 2.36 |
| a POSIX shell | zsh and bash are what the shims are written for |

**Platform reality.** Everything below has been run on macOS. Linux should
work — the secret store is an age-encrypted file, not the Keychain, and the
shims are plain `#!/bin/sh` — but no step here has been executed on Linux.
Windows is *unverified in the strong sense*: the `%APPDATA%` path, the `.cmd`
shim and the `PATHEXT` lookup are unit-tested as pure functions from macOS and
have never run on Windows. Do not treat them as working.

## Step 0 — Build and install the binary

```sh
git clone https://github.com/DanielCarmingham/gitwho
cd gitwho
cargo install --path . --locked
```

That puts `gitwho` in `~/.cargo/bin`. If you would rather choose the
location:

```sh
cargo build --release
cp target/release/gitwho ~/.local/bin/gitwho
```

**Pick the path deliberately, then leave it alone.** `shim install` and
`mcp sync` bake the absolute path of the binary that generated them into their
output. Installing to `target/debug`, or to a directory you later tidy up,
produces config that breaks silently the moment the binary moves. If both
`~/.cargo/bin` and `~/.local/bin` are on your `PATH`, decide which one owns
`gitwho` now — having two copies means `which gitwho` and the path
inside your shims can disagree.

**Verify.**

```sh
which gitwho      # the path you chose
gitwho --version
```

## Step 1 — Create the store, then the config

```sh
gitwho secret init
```

Use this rather than `mkdir -p`. It creates `~/.config/gitwho` owner-only
(`0700`) and writes `identity.key` at `0600`; `mkdir` applies your umask, and
the usual `022` leaves the directory `0755` — group- and world-traversable,
which is the only thing keeping the identity key and every stored token out of
another local account's reach. `doctor` treats anything but `0700` as a
failure, so a hand-made directory fails on its first run.

Then write the config:

```sh
cp docs/accounts.toml.example ~/.config/gitwho/accounts.toml
chmod 600 ~/.config/gitwho/accounts.toml
$EDITOR ~/.config/gitwho/accounts.toml
```

The `chmod` is not hygiene, it is the point: `accounts.toml` is a redirect
vector. Whoever can write it can add a `match` pattern for a host they control
and be handed one of your tokens.

The example declares three archetypes — a personal account, a work account
distinguished by organisation rather than by host, and a self-hosted Gitea
using ssh and https at once. Replace every value; keep whichever shapes match
how you actually work. The shape that matters:

```toml
[defaults]
account = "Personal"          # used only when nothing else matches
gitName = "Your Name"

[[accounts]]
name          = "Personal"
provider      = "github"      # required, but see below — nothing reads it yet
email         = "you@example.com"
gitCredential = "GH_TOKEN"    # names the variable; the value lives in the store
sshKey        = "~/.ssh/id_ed25519_personal"
match         = ["github.com/YourUser/**"]
paths         = ["/home/you/src/personal/"]   # only for repos with no remote
env           = ["GH_TOKEN"]                  # what `exec` injects
```

`match` is what does the real work — it is matched against `host/path`, so
`github.com/SomeOrg/**` selects an account by *organisation*, which is why
several GitHub accounts on one host can be told apart. `paths` is only a
fallback for repositories with no remote. Neither `env` nor `gitCredential`
ever holds a value; they name variables.

`provider` is required by the parser but is currently read by nothing —
`mcp sync` recognises provider servers from their command, not from this
field, and nothing else consults it. Write the obvious value (`github`,
`gitea` — Forgejo and Codeberg are Gitea forks) and do not expect it to
change any behaviour.

Then store the tokens, one per account and variable:

```sh
gitwho secret set Personal GH_TOKEN     # prompts; the value is never in argv
gitwho secret list
```

**Verify.** `secret list` shows a fingerprint for every declared secret and
`MISSING` for none. Values are never printed — the fingerprint is the whole
check.

**Check the tokens are alive before you trust them.** A stored-but-dead token
looks identical to a working one here; `doctor` cannot yet tell them apart.
For GitHub, without printing the value:

```sh
curl -sI -H "Authorization: Bearer $TOKEN" https://api.github.com/user \
  | grep -iE 'HTTP/|github-authentication-token-expiration|x-oauth-scopes'
```

`401` means dead. `200` with an expiry header is a PAT with a known lifetime;
`200` with `x-oauth-scopes` and no expiry is an OAuth token.

## Step 2 — Generate the identity rules

```sh
gitwho sync            # dry run: prints what it would write
gitwho sync --write
```

`sync` only ever writes inside `~/.config/gitwho/git/`. It does not touch
anything you hand-wrote. Wire it in yourself, once:

```ini
[include]
    path = ~/.config/gitwho/git/includes.gitconfig
```

If you already have `includeIf "gitdir:..."` rules, put this **after** them —
later wins, so the new rules take over while both are present, and removing
this one line hands control straight back.

**Verify** that identity follows the repository rather than its location:

```sh
cd ~/src/work/some-repo && git config --get user.email
git clone https://github.com/SomeOrg/repo /tmp/relocated
git -C /tmp/relocated config --get user.email    # the work address, outside every root
```

That second case is the one path rules cannot do.

## Step 3 — Hand git its credentials

Per host you want gitwho to serve:

```ini
[credential "https://github.com"]
    helper =
    helper = /Users/you/.local/bin/gitwho credential
    useHttpPath = true
```

Three things are load-bearing here:

- **The empty `helper =` is a reset, not a helper.** It clears everything
  configured before it. Without it you are appending to a list, and whatever
  ran first still answers.
- **A url-scoped section overrides the general helper list outright.** Setting
  only the global `credential.helper` achieves nothing if a
  `[credential "https://host"]` section exists.
- **`useHttpPath = true` is mandatory for multi-account hosts.** Without it
  the helper is told `github.com` and nothing more, so every GitHub account
  resolves identically — a total and silent failure. With it, git passes
  `path=Org/repo.git`, which is what makes the org visible.

Leave the general `credential.helper` alone for hosts gitwho does not
claim.

**Verify.**

```sh
gitwho doctor                  # no FAIL lines under [git]
git -C ~/src/work/some-repo fetch
```

## Step 4 — Cover the CLIs

`gh`, `tea` and friends read their own environment variables, so they need
wrappers:

```sh
gitwho shim install --dir ~/.local/share/gitwho/shims gh tea
```

Then put that directory early on `PATH`. On zsh this belongs at the **end of
`~/.zshrc`**, not in `~/.zshenv`:

```sh
export PATH="$HOME/.local/share/gitwho/shims:$PATH"
```

`.zshenv` looks like the right place and is not. `.zshrc` then prepends a
dozen or more entries of its own — Homebrew among them — so anything set in
`.zshenv` ends up buried and the real `gh` wins. Keep a copy in `.zshenv` as
well, since non-interactive shells never read `.zshrc`, but the `.zshrc` line
is the one that takes effect.

**Verify** that the account follows the repo, not the shell:

```sh
cd ~/src/work/some-repo     && gh api user --jq .login
cd ~/src/personal/some-repo && gh api user --jq .login
```

Two different logins, from one terminal. If they match, the shim directory is
not early enough on `PATH`.

Two traps that produce false passes, both met in practice:

- `zsh -l -c '...'` is **not interactive**, so it never reads `.zshrc` at all.
  Use `zsh -l -i -c '...'`.
- A shell **inherits** its parent's environment, so one launched from a
  session that already holds tokens proves nothing. Test with
  `env -i HOME=$HOME TERM=xterm PATH=/usr/bin:/bin zsh -l -i -c '...'`.

## Step 5 — Wrap any provider MCP servers

```sh
gitwho mcp sync path/to/.mcp.json            # dry run
gitwho mcp sync --write path/to/.mcp.json
```

This rewrites provider servers to launch through `gitwho exec`, so each one
gets exactly its own account's credentials instead of whatever happened to be
ambient when the editor started. `.mcp.json` files are usually committed,
which is why it refuses to write without `--write`.

Re-run it after adding a provider MCP anywhere; the dry run is the check.

## Step 6 — Confirm the whole thing

```sh
gitwho doctor
env | grep -E 'GH_TOKEN|GITEA_TOKEN'    # expect nothing
```

`doctor` is read-only and exits non-zero on problems. It checks store
permissions first, because a readable store makes every other check moot.

An `[ambient]` warning means a provider token is sitting in your environment
where every process can read it. gitwho does not need it — that warning is
the exposure the tool exists to remove.

## A second machine, or a rebuilt one

If your dotfiles are in version control, they carry roughly everything except
the part that matters most.

**Carried by dotfiles:** `accounts.toml` (it names variables, never values),
the `[include]` line, the `[credential]` sections, the `PATH` lines.

**Not carried, and not carryable:** `identity.key` and `secrets.age`. They are
the store. Committing them would put every token in a repository, so on a new
machine you re-create them:

```sh
cargo install --path . --locked        # 1. the binary is not in your dotfiles either
gitwho secret init                  # 2. new identity key
gitwho secret set <Account> <VAR>   # 3. once per token
gitwho sync --write                 # 4. regenerate; paths differ per machine
gitwho shim install --dir ~/.local/share/gitwho/shims gh tea
gitwho doctor
```

Steps 3 and 5 of the main install — the gitconfig `[credential]` sections and
the `PATH` line — come back with your dotfiles, but check that the **absolute
binary path** inside them matches where you actually installed it. That path
is baked in, and `/Users/you` on macOS is `/home/you` elsewhere.

There is no way around re-entering the tokens by hand. That is deliberate: it
is the cost of the store never being in a repo.

## Things that will bite you

- **The binary path is baked in.** Shims and wrapped MCP commands record the
  path of the binary that generated them. Re-run `shim install` and `mcp sync`
  after moving or reinstalling gitwho.
- **A url-scoped credential section beats the global one.** If `doctor` says a
  host is served by something else while your global helper looks correct,
  that is why.
- **An account with no `gitName` and no `[defaults] gitName` generates
  `name = `**, which makes every commit in that repo fail. `doctor` checks for
  it.
- **Repos matching no account** resolve to the default tagged `Unmatched`:
  `gh` works and says nothing claimed the remote, while the credential helper
  refuses. That is the designed behaviour, not a misconfiguration — a wrong
  account served quietly is the failure this tool was built to prevent.
