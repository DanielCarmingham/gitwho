# Installing gitwho

Setting up gitwho on a machine that has never run it.

If you are migrating off an existing path-based setup rather than starting from
nothing, do it in this order: get gitwho working alongside what you have (its
`[include]` line placed *after* your existing `includeIf "gitdir:"` rules, so
later-wins hands control over while both are present), verify with the checks
below, and only then remove the old rules. Every step here is reversible by
deleting one line.

Why it is built this way is in [DESIGN.md](DESIGN.md).

## What you need

| | |
|---|---|
| Rust toolchain | to build; there are no prebuilt binaries yet |
| git ≥ 2.36 | `includeIf "hasconfig:remote.*.url:"` landed in 2.36 |
| a POSIX shell | zsh and bash are what the shims are written for |

**Platform reality.** Everything below has been run on macOS, and on Debian
(aarch64) — including the store permissions, the credential helper, a shim
executed for real, and the `PATH` line landing in `.bashrc` rather than
`.zshrc`. x86-64 Linux is covered by CI but has not been driven by hand.

Windows is *unverified in the strong sense*: the `%APPDATA%` path, the `.cmd`
shim and the `PATHEXT` lookup are unit-tested as pure functions from macOS and
have never run on Windows. Do not treat them as working.

## Step 0 — Build and install the binary

```sh
git clone https://github.com/DanielCarmingham/gitwho
cd gitwho
cargo install --path . --locked
```

That puts `gitwho` in `~/.cargo/bin`. If you would rather choose the location:

```sh
cargo build --release
cp target/release/gitwho ~/.local/bin/gitwho
```

**Pick the path deliberately, then leave it alone.** `init`, `shim install`,
`sync` and `mcp sync` all bake the absolute path of the binary that generated
them into their output. Installing to `target/debug`, or to a directory you
later tidy up, produces config that breaks silently the moment the binary
moves. If both `~/.cargo/bin` and `~/.local/bin` are on your `PATH`, decide
which one owns `gitwho` now — two copies means `which gitwho` and the path
inside your generated config can disagree.

**Verify.**

```sh
which gitwho      # the path you chose
gitwho --version
```

## Step 1 — `gitwho init`

One command, run twice, with your accounts written in between.

```sh
gitwho init             # dry run: lists every step, writes nothing
gitwho init --write
```

The first `--write` creates the store, scaffolds `accounts.toml` from the
template, and **stops**:

```
created       ~/.config/gitwho/identity.key (owner-only)
created       ~/.config/gitwho/accounts.toml

Now the part only you can do:

  1. edit ~/.config/gitwho/accounts.toml
     replace the example accounts with yours
  2. gitwho secret set <Account> <VAR>    once per token
  3. gitwho init --write                  re-run to finish
```

That stop is deliberate. Generating rules from a template of placeholders would
give you a machine that looks configured and resolves every repository to an
account that does not exist — working-but-wrong, which is the failure this tool
exists to prevent.

So: edit the config (see [the schema](#the-config) below), store your tokens,
and run it a third time. Now it finishes, and ends by running `doctor`.

**`init` is idempotent.** Re-run it after adding an account, or after moving the
binary. Every step reports `ok` when there was nothing to do, so a second run
tells you exactly what changed — nothing, ideally.

### The config

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
paths         = ["~/src/personal/"]           # only for repos with no remote
env           = ["GH_TOKEN"]                  # what `exec` injects
```

`match` is what does the real work — it is matched against `host/path`, so
`github.com/SomeOrg/**` selects an account by *organisation*, which is why
several GitHub accounts on one host can be told apart. `paths` is only a
fallback for repositories with no remote. Neither `env` nor `gitCredential`
ever holds a value; they name variables.

`provider` is required by the parser but is currently read by nothing —
`mcp sync` recognises provider servers from their command, not from this field.
Write the obvious value (`github`, `gitea` — Forgejo and Codeberg are Gitea
forks) and do not expect it to change any behaviour.

**Check the tokens are alive before you trust them.** A stored-but-dead token
looks identical to a working one here; `doctor` cannot yet tell them apart. For
GitHub, without printing the value:

```sh
curl -sI -H "Authorization: Bearer $TOKEN" https://api.github.com/user \
  | grep -iE 'HTTP/|github-authentication-token-expiration|x-oauth-scopes'
```

`401` means dead. `200` with an expiry header is a PAT with a known lifetime;
`200` with `x-oauth-scopes` and no expiry is an OAuth token.

## Step 2 — Wrap any provider MCP servers

Not part of `init`, because `.mcp.json` files live in project repositories
rather than in your home directory, and they are usually committed.

```sh
gitwho mcp sync path/to/.mcp.json            # dry run
gitwho mcp sync --write path/to/.mcp.json
```

This rewrites provider servers to launch through `gitwho exec`, so each one
gets exactly its own account's credentials instead of whatever happened to be
ambient when the editor started. Re-run it after adding a provider MCP
anywhere; the dry run is the check.

---

## What `init` did, and how to do it by hand

Everything below is what the one command automates. Read it if you want to
place things yourself, if `init` reported a file as `missing`, or if something
is not behaving and you need to know which piece to look at.

### The store, at `0700`

```sh
gitwho secret init
```

Use this rather than `mkdir -p`. It creates `~/.config/gitwho` owner-only
(`0700`) and writes `identity.key` at `0600`; `mkdir` applies your umask, and
the usual `022` leaves the directory `0755` — group- and world-traversable,
which is the only thing keeping the identity key and every stored token out of
another local account's reach. `doctor` treats anything but `0700` as a
failure, so a hand-made directory fails on its first run.

`accounts.toml` is written `0600` for a different reason: it is a **redirect
vector**. Whoever can write it can add a `match` pattern for a host they
control and be handed one of your tokens.

### The generated rules

```sh
gitwho sync            # dry run: prints what it would write
gitwho sync --write
```

`sync` only ever writes inside `~/.config/gitwho/git/`. It does not touch
anything you hand-wrote. It generates:

| File | What it holds |
|---|---|
| `<Account>.gitconfig` | one account's identity and ssh key |
| `credentials.gitconfig` | a `[credential]` section per host any account claims |
| `includes.gitconfig` | the rules that pick an identity, plus an include of the credentials file |

### The one line you add

```ini
[include]
    path = ~/.config/gitwho/git/includes.gitconfig
```

That single line is the whole hookup — everything else is reached from it, so
deleting it hands control straight back. If you already have `includeIf
"gitdir:..."` rules, put this **after** them: later wins, so the new rules take
over while both are present.

### Why the credential sections are generated rather than documented

They used to be a paste-this-into-your-gitconfig block, and three details in it
are load-bearing. Each fails *silently* when it is wrong, which is why they are
now emitted for you:

- **The empty `helper =` is a reset, not a helper.** It clears everything
  configured before it. Without it you are appending to a list, and whatever
  ran first still answers.
- **A url-scoped section overrides the general helper list outright.** Setting
  only the global `credential.helper` achieves nothing if a
  `[credential "https://host"]` section exists — and, conversely, these
  sections leave every other host alone.
- **`useHttpPath = true` is mandatory for multi-account hosts.** Without it the
  helper is told `github.com` and nothing more, so every GitHub account
  resolves identically — a total and silent failure.

Hosts gitwho does not claim keep whatever helper you already had.

### The shims, and where the `PATH` line goes

```sh
gitwho shim install --dir ~/.local/share/gitwho/shims gh tea
```

Then that directory has to come early on `PATH`. On zsh this belongs at the
**end of `~/.zshrc`**, not in `~/.zshenv`:

```sh
export PATH="$HOME/.local/share/gitwho/shims:$PATH"
```

`.zshenv` looks like the right place and is not. `.zshrc` then prepends a dozen
or more entries of its own — Homebrew among them — so anything set in `.zshenv`
ends up buried and the real `gh` wins. Keep a copy in `.zshenv` as well, since
non-interactive shells never read `.zshrc`, but the `.zshrc` line is the one
that takes effect.

`init` adds this line to `~/.zshrc` (or `~/.bashrc`, if `$SHELL` ends in
`bash`). If that file does not exist it says so and prints the line rather than
creating one a login shell may never read.

---

## Verifying

The three checks worth running, in order of how much they prove.

**1. Is the wiring coherent?**

```sh
gitwho doctor
env | grep -E 'GH_TOKEN|GITEA_TOKEN'    # expect nothing
```

`doctor` is read-only and exits non-zero on problems. It checks store
permissions first, because a readable store makes every other check moot. An
`[ambient]` warning means a provider token is sitting in your environment where
every process can read it — gitwho does not need it, and that warning is the
exposure the tool exists to remove.

**2. Does identity follow the repository rather than its location?**

```sh
cd ~/src/work/some-repo && git config --get user.email
git clone https://github.com/SomeOrg/repo /tmp/relocated
git -C /tmp/relocated config --get user.email    # the work address, outside every root
```

That second case is the one path rules cannot do.

**3. Does the account follow the repo rather than the shell?**

```sh
cd ~/src/work/some-repo     && gh api user --jq .login
cd ~/src/personal/some-repo && gh api user --jq .login
```

Two different logins, from one terminal. If they match, the shim directory is
not early enough on `PATH`.

**Two traps that produce false passes**, both met in practice:

- `zsh -l -c '...'` is **not interactive**, so it never reads `.zshrc` at all.
  Use `zsh -l -i -c '...'`.
- A shell **inherits** its parent's environment, so one launched from a session
  that already holds tokens proves nothing. Test with
  `env -i HOME=$HOME TERM=xterm PATH=/usr/bin:/bin zsh -l -i -c '...'`.

## A second machine, or a rebuilt one

If your dotfiles are in version control, they carry roughly everything except
the part that matters most.

**Carried by dotfiles:** `accounts.toml` (it names variables, never values),
the `[include]` line, the `PATH` line.

**Not carried, and not carryable:** `identity.key` and `secrets.age`. They are
the store. Committing them would put every token in a repository, so on a new
machine you re-create them:

```sh
cargo install --path . --locked      # the binary is not in your dotfiles either
gitwho secret set <Account> <VAR>    # once per token
gitwho init --write                  # regenerates everything; paths differ per machine
```

There is no way around re-entering the tokens by hand. That is deliberate: it
is the cost of the store never being in a repo.

## Things that will bite you

- **The binary path is baked in.** Generated config records the path of the
  binary that produced it. Re-run `gitwho init --write` after moving or
  reinstalling gitwho — it regenerates everything that carries a path.
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
