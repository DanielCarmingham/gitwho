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
(aarch64) — including the config directory's `0700`/`0600` permissions, the
credential helper, a shim executed for real, and the `PATH` line landing in
`.bashrc` rather than `.zshrc`. x86-64 Linux is covered by CI but has not been
driven by hand.

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

The first `--write` creates `~/.config/gitwho` (owner-only), scaffolds
`accounts.toml` from the template, and **stops**:

```
created       ~/.config/gitwho (owner-only)
created       ~/.config/gitwho/accounts.toml

Now the part only you can do:

  1. edit ~/.config/gitwho/accounts.toml
     replace the example accounts with yours
  2. log gh (or tea) in as each account's `login`
  3. gitwho init --write                  re-run to finish
```

That stop is deliberate. Generating rules from a template of placeholders would
give you a machine that looks configured and resolves every repository to an
account that does not exist — working-but-wrong, which is the failure this tool
exists to prevent.

So: edit the config (see [the schema](#the-config) below), log the CLIs in
(see [Logging the CLIs in](#logging-the-clis-in) below), and run it a third
time. Now it finishes, and ends by running `doctor`.

### Step 1 from what is already on disk

`gitwho init --discover <roots>` walks the given directories, reads every
`remote.origin.url`, groups by `host/org`, and prints a proposed `accounts.toml`
to stdout. It writes nothing, so the output can be reviewed, edited and piped:

```sh
gitwho init --discover ~/src                          # look at it first
gitwho init --discover ~/src > ~/.config/gitwho/accounts.toml
```

What it can and cannot do, because the difference decides how much to trust it:

- **It finds organisations, not accounts.** Nothing on disk says two orgs are
  the same person. Each org becomes its own block, and the header says to merge
  them by hand. It will not guess, because a config that looks authoritative and
  is subtly wrong is the failure this whole tool exists to prevent.
- **An org with a single repository is not proposed**, on the assumption that
  one clone means somebody else's work. Measured on the author's machine: of 9
  organisations found, 8 had exactly one repository and every one of those was
  an upstream project. They are still listed with their pattern, so promoting
  one is a copy-paste.
- **Emails are never invented.** Every one is `REPLACE-ME`, as is the default
  account — picking that is not a guess worth making.
- **It reports what it could not do**: repositories with no remote (which is
  what `paths` is for), remotes it could not parse, roots that do not exist, and
  directories the depth bound stopped it at. A partial scan says it was partial.

Worktrees are found: a linked worktree's `.git` is a *file*, and several
worktrees of one repository collapse to a single entry. Every URL form git
writes is understood, including scp-style with a non-`git` user, because this
reuses the resolver's own parsing rather than a second spelling of it.

### Logging the CLIs in

gitwho stores no token. Each account names a `provider` and a `login`, and
`exec` asks that provider's CLI for the token every time — so "configuring
credentials" means logging that CLI in as that login, once, and letting it
manage renewal from there:

- **GitHub:** `gh auth login --hostname github.com`, once per account's
  `login`. `gh auth status` lists the logins it already holds; `gh auth
  refresh` and token rotation are picked up on the next call, since gitwho
  never keeps a copy to go stale.
- **Gitea or Forgejo:** `tea login add --url <url>`, once per server named by
  an account's `url`. **Only one login per Gitea server is supported** — `tea
  login helper get` returns the first login for a host regardless of which one
  gitwho asks for, so two accounts on one server cannot be told apart, and
  gitwho refuses rather than guess.

`doctor` reports an account whose login does not answer as a problem, and the
message names the exact command to fix it: `gh auth login --hostname
github.com`, or `tea login add --url <url>`.

`gitwho whoami` answers the resolution question without touching anything, and
`gitwho whoami --quiet` prints just the account name for use in another
command — failing rather than answering when nothing identified it, since a
plausible wrong name is worse than no name.

The credential helper refuses to write against a repository no account
claims. The declared default would accept the write and look healthy in
`doctor` afterwards, which is the wrong-and-quiet failure (R8) rather than a
convenience.

**`init` is idempotent.** Re-run it after adding an account, or after moving the
binary. Every step reports `ok` when there was nothing to do, so a second run
tells you exactly what changed — nothing, ideally.

### The config

```toml
[defaults]
account = "Personal"          # used only when nothing else matches
gitName = "Your Name"

[[accounts]]
name     = "Personal"
provider = "github"            # "gitea" and "forgejo" (same thing) are the others
login    = "your-username"     # the gh login; gitwho asks gh for the token
email    = "you@example.com"
sshKey   = "~/.ssh/id_ed25519_personal"
match    = ["github.com/YourUser/**"]
paths    = ["~/src/personal/"]           # only for repos with no remote
```

`match` is what does the real work — it is matched against `host/path`, so
`github.com/SomeOrg/**` selects an account by *organisation*, which is why
several GitHub accounts on one host can be told apart. `paths` is only a
fallback for repositories with no remote.

`provider` decides which variables `exec` sets: a `github` account gets
`GH_TOKEN`; a `gitea` account (also required: `url`) gets `GITEA_TOKEN` and
`GITEA_INSTANCE_URL`. `login` is required on every account — it is the login
`gh`/`tea` holds the token under, not the git author identity. See
[docs/accounts.toml.example](accounts.toml.example) for every field, including
the self-hosted Gitea example.

**Check the tokens are alive before you trust them.** A token the server has
revoked still looks healthy to `doctor`, because telling them apart needs the
network. For GitHub, without printing the value:

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

### The config directory, at `0700`

`gitwho init --write` creates `~/.config/gitwho` at `0700` directly, rather
than `mkdir -p` followed by a `chmod` — the usual `022` umask leaves a plain
`mkdir` at `0755`, group- and world-traversable, and creating it at the right
mode from the start means there is no window where the directory is complete
and readable before it is locked down. Doing it by hand:

```sh
mkdir -m 700 -p ~/.config/gitwho
```

`doctor` treats anything but `0700` as a failure, so a hand-made directory
that used plain `mkdir` fails on its first run; `init` tightens a looser
existing directory the same way it creates a fresh one.

`accounts.toml` is written `0600` for a different reason: it is a **redirect
vector**. Whoever can write it can add a `match` pattern for a host they
control and be handed one of your tokens — gitwho holds no other secret for
that directory to protect.

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

**Logging in still works through a shim.** A command whose purpose is to
establish a credential is never handed one: `gh auth login`, `logout`,
`refresh`, `switch` and `setup-git`, and `tea login add` and `logout`, run with
every managed variable cleared and nothing injected. gitwho says so on stderr
rather than doing it silently.

That matters twice over. gh refuses to store credentials at all while
`GH_TOKEN` is set, and the shim is what sets it -- so without this, a shimmed
`gh auth login` could never succeed, in any directory. And an account declared
in `accounts.toml` whose CLI login does not exist yet has no token to give, which
the ordinary path reports as a problem; logging in is how you fix that, so it
is the one thing that must not be blocked by it.

Clearing still happens. Stepping aside means injecting nothing, not letting a
token the shell already exported reach a tool that would authenticate as it.

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

`doctor` is read-only and exits non-zero on problems. It checks the config
directory's permissions first, because a writable `accounts.toml` is a
redirect vector that makes every other check moot. An `[ambient]` warning
means a provider token is sitting in your environment where every process can
read it — gitwho does not need it, and that warning is the exposure the tool
exists to remove.

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

If your dotfiles are in version control, they carry everything gitwho needs —
`accounts.toml` names logins, never values, so there is no store to re-create:

```sh
cargo install --path . --locked      # the binary is not in your dotfiles either
gitwho init --write                  # regenerates everything; paths differ per machine
```

`init --write` still stops if `accounts.toml` is missing, but a dotfiles
checkout already has it, so the run that matters is logging the CLIs in as
each account's `login` — `gh auth login`, `tea login add --url <url>` — before
tokens are asked for. `doctor` names exactly which account still needs it.

## Upgrading from 0.2

0.3 removes the `env` and `gitCredential` fields, and with them the secret
store they drew from — `identity.key` and `secrets.age`. A config still using
either field fails to parse, with an error naming the field. There is no
automatic converter: the old fields named a variable, the new schema names a
provider and a login, and nothing here can be translated mechanically. The
one existing config was converted by hand.

Per account, in `accounts.toml`:

1. Delete `env` and `gitCredential`.
2. Check that each account's existing `provider` is `"github"`, `"gitea"`, or
   `"forgejo"` (`"forgejo"` means the same as `"gitea"`), since 0.3 rejects any
   other value.
3. Add `login`: the `gh` login, or the user your `tea` login for that server
   holds, that this account's token already lives under.
   `gitwho init --discover <root>` prints a proposed config that lists the
   logins gh and tea hold, which helps if you are unsure of the names.
4. For a `gitea` account, add `url`: the server's https address. Required for
   `gitea`, rejected for `github`, which always means github.com.

Then log the CLIs in as each account's `login`, and confirm:

```sh
gh auth login --hostname github.com     # once per GitHub login, if not already
tea login add --url <url>               # once per Gitea/Forgejo server
gitwho doctor                           # confirms every account's token answers
```

Only once `doctor` is clean, delete the leftover store by hand — `doctor`
reports `secrets.age` and `identity.key` as unused but never deletes them
itself:

```sh
rm ~/.config/gitwho/secrets.age ~/.config/gitwho/identity.key
```

If you had pointed `GITWHO_SECRETS` or `GITWHO_IDENTITY` elsewhere, delete the
files at those paths as well.

If you used the keychain backend (`secretBackend = "keychain"`, or
`GITWHO_SECRET_BACKEND=keychain`), your tokens are still in the platform's
credential store, filed under the service name `gitwho`, and 0.3 never reads or
removes them. Delete `secretBackend` from `[defaults]`, then remove the entries.
On macOS, `security delete-generic-password` removes one entry per run, so
repeat it until it reports that the item could not be found:

```sh
security delete-generic-password -s gitwho
```

On Linux and Windows, remove the equivalent entries for service `gitwho` in
Secret Service or Credential Manager.

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
- **A brand-new repository is unmatched until it has a remote**, so
  `gh repo create` run inside it acts as the default account. Pass
  `gitwho exec --account <name>`, and add the remote before the first commit.
  The README's [Starting a new repository](../README.md#starting-a-new-repository)
  has the `gh` and `tea` versions.
- **Two tea logins on one host are refused.** tea's helper is asked by host
  alone and cannot be told which login to use, so gitwho will not guess. That
  includes two servers under different paths on one host, and an `http` and an
  `https` login for the same host.
