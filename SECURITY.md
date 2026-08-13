# Security

gitwho holds provider tokens and decides which one to hand to which process, so
a bug here is a bug about credentials. This document says how to report one,
and — just as important — what the design does and does not claim, so you can
tell a vulnerability from a documented limit.

## Reporting a vulnerability

**Use [GitHub's private vulnerability
reporting](https://github.com/DanielCarmingham/gitwho/security/advisories/new).**
It is enabled on this repository. Please do not open a public issue for
anything that would expose or misdirect someone's credentials — a public issue
is a disclosure.

This is one person's project, maintained alongside other work. Expect an
acknowledgement within about a week rather than within hours. If a report is
valid I will fix it, credit you unless you would rather I did not, and say
plainly in the release notes what was wrong.

Include, if you can: the version (`gitwho --version`), the platform, and a
`gitwho doctor` transcript. `doctor` never prints a token value — only
fingerprints — so its output is safe to paste. **Never send a real token**, in
a report or anywhere else; a fingerprint or the first few characters is enough
to identify one.

## Supported versions

The latest release only. This is pre-1.0 software with a single maintainer;
fixes go into a new version rather than being backported.

## What counts as a vulnerability here

These are the properties gitwho is built to hold. A reproducible break in any
of them is a security bug, not a feature request:

- **A working-but-incorrect account.** Resolution returning an account that
  authenticates successfully but is the *wrong* one. This is the failure the
  whole project exists to prevent: it means committing or acting as someone you
  are not, with nothing to signal it. Refusing is correct; guessing is not.
- **A token crossing accounts.** Entering one account's repository handing a
  different account's token to any tool.
- **A token value leaving the process.** Appearing in output, an error message,
  a log, a generated file, or `argv` — anywhere `ps`, a shoulder, or a pasted
  transcript could pick it up. Only fingerprints are ever printed.
- **Store permissions widening.** The store directory is `0700` and the
  identity key, secrets file and `accounts.toml` are `0600`. That directory is
  the only thing keeping the key out of another local account's reach.
- **A credential served for a host no account claims.** The helper declines
  when resolution is not confident; serving anyway would be a redirect.

## What is *not* a vulnerability

These are known, documented, and explained in
[docs/DESIGN.md](docs/DESIGN.md#what-this-protects-and-what-it-does-not).
Reports about them are welcome as ideas, but they are not undisclosed holes:

- **A local process running as you can read everything.** The age identity key
  sits on disk beside the encrypted secrets, both owned by you, so anything
  running as your user decrypts both. The encryption defends the secrets *at
  rest* — a backup, a sync folder, an accidental commit — and not against local
  code. If something is executing as you, it has your tokens, with or without
  gitwho.
- **A token is in the environment of the process gitwho launches.** That is how
  `gh` and `tea` read it, so during `gitwho exec` the value is visible in that
  child's environment to your own user. The gain is scope, not absence: one
  process for one invocation, instead of every process for the whole session.
- **A dead token looks like a live one.** `doctor` reports that a value is
  stored, not that the provider still accepts it.
- **The platform keychain is not the default.** It is implemented, and not
  selected automatically, because macOS keys its ACL to the calling binary's
  code hash — so every rebuild of an unsigned binary blocks on a GUI prompt, on
  a helper that runs on every fetch. See `examples/keychain_probe.rs`.
- **Anything on Windows.** The Windows paths and shim are unit-tested as pure
  functions and have never been executed on Windows. Treat that platform as
  unverified rather than as broken or as working.

## If you think a token has been exposed

Rotate it at the provider first — that is the only step that actually revokes
anything. Then `gitwho secret set <Account> <VAR>` to store the replacement,
and `gitwho secret list` to confirm the fingerprint changed.

If the store itself may have been read, rotate **every** token in it and
re-create the identity key: the one key decrypts all of them.
