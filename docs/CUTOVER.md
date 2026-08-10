# Cutover runbook

Switching this machine from the path-based dotfiles setup to gitfriend.

Written 2026-08-09, after every build task was finished and verified. **Nothing
on the machine has been changed yet.** Everything below is the remaining work.

Each step says what to do, how to check it worked, and how to undo it. Do them
in order — later steps depend on earlier ones — and stop at the first
verification that fails.

---

## State when this was written

Built and tested (69 tests, clippy clean):

| Command | What it does |
|---|---|
| `gitfriend credential get\|store\|erase` | git credential helper; resolves from the URL git is about to contact |
| `gitfriend exec [--account X] -- cmd` | runs one command with exactly one account's variables, scrubbing the rest |
| `gitfriend shim install --dir D names...` | wrapper scripts so `gh`/`tea` route through `exec` |
| `gitfriend secret init\|set\|list\|delete\|import --from-env` | token storage; values on stdin, only fingerprints printed |
| `gitfriend sync [--write]` | generates the identity `includeIf` rules |
| `gitfriend mcp sync [--write] paths...` | routes provider MCP servers through `exec` |
| `gitfriend doctor` | read-only coherence report; non-zero on problems |

`sync`, `mcp sync` and `doctor` never write unless `--write` is passed.
`doctor` never writes at all.

**Secrets live in an age-encrypted file**, not the Keychain. The Keychain
backend is implemented but unusable: macOS keys its ACL to the calling binary,
so every rebuild blocks on a GUI prompt. See `src/secrets/keychain.rs` and
`examples/keychain_probe.rs` if you want to revisit that.

---

## Before you start

### Install to a stable path

**Do this first.** `mcp sync` and `shim install` bake the path of the binary
that generated them into their output. Generating from `target/debug` produces
config that breaks the moment you `cargo clean`.

```sh
cargo build --release
mkdir -p ~/.local/bin
cp target/release/gitfriend ~/.local/bin/gitfriend   # already on PATH via .zshenv
which gitfriend                                       # expect ~/.local/bin/gitfriend
```

If this session's `CARGO_TARGET_DIR=target.noindex` is still set in your shell,
the binary is under `target.noindex/release/` instead. A fresh shell uses
`target/`.

### Editing files in `$HOME`

`$HOME` is the `cfg` bare repo. Plain `git` does not work there. Use:

```sh
git --git-dir=/Users/daniel/.cfg --work-tree=/Users/daniel status
```

`git add` needs `-f` for anything below the top level (`~/.gitignore` ends
with `*`).

### Keep a way back

Every step is reversible, but the fastest global undo is: remove the one
`[include]` line added in step 2, and restore the `credential` sections in
`~/.gitconfig-darwin`. Note their current contents before you start:

```sh
cp ~/.gitconfig-darwin ~/.gitconfig-darwin.pre-gitfriend
```

---

## Step 1 — Install the config and the secrets

```sh
mkdir -p ~/.config/gitfriend
cp docs/accounts.toml.example ~/.config/gitfriend/accounts.toml
$EDITOR ~/.config/gitfriend/accounts.toml      # see "Decisions still open" below
gitfriend secret init                           # writes ~/.config/gitfriend/identity.key
```

Then import the existing tokens. **This must run in an interactive shell**:
`~/.zshrc.local` is only sourced for those, and `import` reads the environment
of the process it runs in.

```sh
gitfriend secret import --from-env
gitfriend secret list
```

**Verify.** `secret list` shows a fingerprint, not `MISSING`, for every row.
Values are never printed; a fingerprint is the whole check.

**Undo.** `rm -rf ~/.config/gitfriend`. Nothing else references it yet.

---

## Step 2 — Generate the identity rules

```sh
gitfriend sync                 # dry run; read the output
gitfriend sync --write
```

Then add the include, once, to `~/.gitconfig-common`:

```ini
[include]
    path = ~/.config/gitfriend/git/includes.gitconfig
```

Put it **after** the existing `includeIf "gitdir:..."` block so the new rules
win while both are present. Leaving the old rules in place is deliberate —
they are the fallback if you stop here.

**Verify** identity resolves by repository, not location:

```sh
cd ~/Developer/Profound/<any repo> && git config --get user.email
cd ~/Developer/Digilope/one-drop-visuals && git config --get user.email
# and the case path rules cannot do — a clone outside every account root:
git clone https://github.com/EJ-Rice/<repo> /tmp/relocated
git -C /tmp/relocated config --get user.email     # expect the Profound address
```

**Undo.** Remove the `[include]` line. The old `gitdir:` rules resume alone.

---

## Step 3 — Take over git credentials

This is the riskiest step: it is what currently makes `git push` work.

Two changes, both in `~/.gitconfig-darwin`:

1. **`credential.useHttpPath` must be true for github.com.** Without it the
   helper is only ever told `github.com`, so all three GitHub accounts resolve
   identically — a total, silent failure.
2. **The url-scoped helper must be replaced.** `[credential
   "https://github.com"]` currently sets `!/opt/homebrew/bin/gh auth
   git-credential`, and a url-scoped section overrides the general list
   outright. Changing only the global helper achieves nothing.

```ini
[credential "https://github.com"]
    helper =
    helper = /Users/daniel/.local/bin/gitfriend credential
    useHttpPath = true
```

Do the same for the Digilope hosts, which also use https
(`gitea.digilope.com`), keeping `provider = generic` where it exists.

Leave the general `credential.helper` (git-credential-manager) alone for hosts
gitfriend does not claim.

**Verify:**

```sh
gitfriend doctor                        # expect no FAIL lines for [git]
git -C ~/Developer/Profound/<repo> fetch
git -C ~/Developer/DanielCarmingham/<repo> fetch
```

**Undo.** `cp ~/.gitconfig-darwin.pre-gitfriend ~/.gitconfig-darwin`.

### Note on `osxkeychain`

It comes from Xcode's bundled gitconfig
(`/Applications/Xcode.app/Contents/Developer/usr/share/git-core/gitconfig`),
not from your dotfiles, and there is no `/etc/gitconfig`. It is already
neutralised by the empty `helper =` reset in `~/.gitconfig-common`. Nothing to
do — recorded so it does not look alarming in `doctor` output later.

---

## Step 4 — Cover the CLIs

```sh
gitfriend shim install --dir ~/.local/share/gitfriend/shims gh tea
```

Put that directory **early** on `PATH` in `~/.zshenv`, before Homebrew.

**Verify** the identity follows the repo, not the shell:

```sh
cd ~/Developer/Profound/<repo>          && gh api user --jq .login
cd ~/Developer/DanielCarmingham/<repo>  && gh api user --jq .login
```

Two different logins, from the same terminal, with no `cd` back and forth. If
they match, the shim directory is not early enough on `PATH`.

**Undo.** Remove the directory from `PATH`.

---

## Step 5 — Wrap the MCP servers

```sh
gitfriend mcp sync ~/Developer/Digilope/one-drop-visuals/.mcp.json     # dry run
gitfriend mcp sync --write ~/Developer/Digilope/one-drop-visuals/.mcp.json
```

That file is the live exposure: it runs `gitea-mcp` with an empty `env` block,
so it currently holds whatever credential was ambient when the editor started.

Only that one file needs it today. Re-run after adding any provider MCP
elsewhere; `mcp sync` without `--write` is the check.

**Verify.** Restart the editor, then confirm the Gitea MCP still works. Its
process should carry `GITEA_TOKEN` and no `GH_TOKEN`.

**Undo.** `git checkout` the `.mcp.json` — it is committed in its own repo.

---

## Step 6 — Remove the ambient tokens

Only after steps 1–5 verify. This is the step that actually closes the
exposure; everything before it was building the replacement.

1. Delete the `_direnv_account_env` block from `~/.config/direnv/direnvrc`
   (the whole function and its call).
2. Remove `export GH_TOKEN=...` from the account-root `.envrc` files under
   `~/Developer/*/`.
3. Remove `GH_TOKEN` from `~/.envrc.local`.
4. Leave `~/.zshrc.local` alone for now — see the open question below.

**Verify**, in a brand-new terminal:

```sh
env | grep -E 'GH_TOKEN|GITEA_TOKEN'    # expect NOTHING
gitfriend doctor                         # expect no [ambient] warnings
cd ~/Developer/Profound/<repo> && git fetch && gh api user --jq .login
```

The `[ambient]` warnings disappearing is the whole point of the project.

**Undo.** Restore from the `cfg` repo: `git --git-dir=/Users/daniel/.cfg
--work-tree=/Users/daniel checkout .config/direnv/direnvrc`.

---

## Step 7 — Commit and document

```sh
git --git-dir=/Users/daniel/.cfg --work-tree=/Users/daniel add -f \
    .config/gitfriend/accounts.toml .config/direnv/direnvrc .gitconfig-darwin
git --git-dir=/Users/daniel/.cfg --work-tree=/Users/daniel commit
```

**Never commit** `identity.key` or `secrets.age`. `accounts.toml` names
variables only.

Then rewrite `~/ACCOUNTS.md`: it describes the old path-based system and is
wrong the moment step 2 lands. Adding an account should become one
`accounts.toml` entry plus one `gitfriend secret set`.

Sanity-check the claim that a new provider is cheap: adding Codeberg should be
~6 lines with `provider = "gitea"` and a different host, no mechanism change.

---

## Decisions still open

**Third-party clones.** About 12 repos under `~/Developer/DanielCarmingham`
(microsoft, dotnet, charmbracelet, ghostty-org…) plus one under `Digilope`
belong to no account. They resolve to the default under `Reason::Unmatched`:
`gh` works and prints a line saying nothing claimed the remote, while the
credential helper still refuses. If that line gets annoying, the alternative is
declaring patterns for them — but then a genuinely forgotten org stops being
visible.

**`~/.zshrc.local`.** After step 6, `GH_TOKEN_*` and `GITEA_TOKEN` there are
only needed if you re-run `secret import --from-env`. Keeping them is a second
copy of every token in plaintext; deleting them makes the secrets file the only
copy. Take a backup of the file somewhere off-repo before deleting.

**`GITHUB_PAT` and `GITHUB_PAT_PROFOUND`.** Still no known consumer — not in
the dotfiles, `~/.local/bin`, direnv config, or MCP config, and `tea` has never
been logged in. Confirm before removing; treat as unknown, not dead.

**Keychain.** Blocked on whether signing the installed binary with a stable
identity stops the per-rebuild prompt. `examples/keychain_probe.rs` answers it.
Not required — the age file works — but it would add encryption at rest that
does not depend on a key file sitting next to the data.

---

## Things that will bite you

- **The binary path is baked in.** Shims and wrapped MCP commands record the
  path of the binary that generated them. Re-run `shim install` and `mcp sync`
  after moving or reinstalling gitfriend.
- **`secret import --from-env` needs an interactive shell**, because
  `~/.zshrc.local` is only sourced there.
- **A url-scoped credential section beats the global one.** If `doctor` says
  github.com is served by something else while your global helper looks
  correct, that is why.
- **Digilope uses both transports** — `app-gitea.digilope.com` over ssh and
  `gitea.digilope.com` over https, with `simple-schematic` cloned each way.
  `REQUIREMENTS.md` says Digilope needs no token in the push/pull path; that is
  wrong for the https remote.
- **A missing author name generates `name = `**, which makes every commit in
  that repo fail. `doctor` checks for it.
