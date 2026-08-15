#!/usr/bin/env bash
#
# Build this working tree and install it over ~/.cargo/bin/gitwho.
#
# Not the release installer -- `dist` generates that one, and it downloads a
# published archive. This builds what is in front of you, including uncommitted
# work, and puts it where your credential helper and shims already point.
#
# Usage: ./install.sh [--no-check]

set -euo pipefail

no_check=false
for arg in "$@"; do
    case "$arg" in
        --no-check) no_check=true ;;
        -h|--help) sed -n '3,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "install.sh: unknown option $arg" >&2; exit 2 ;;
    esac
done

cd "$(dirname "$0")"

if ! grep -q '^name = "gitwho"' Cargo.toml 2>/dev/null; then
    echo "install.sh: run this from the gitwho repository" >&2
    exit 1
fi

if [ "$no_check" = false ]; then
    echo "==> cargo test"
    cargo test --quiet
    echo "==> cargo clippy"
    cargo clippy --all-targets --quiet -- -D warnings
    echo "==> cargo fmt --check"
    cargo fmt --check
fi

# --force because the version in Cargo.toml rarely changes between dev
# installs, and cargo refuses to replace an equal version without it.
echo "==> cargo install"
cargo install --path . --locked --force

target="${CARGO_HOME:-$HOME/.cargo}/bin/gitwho"
echo
"$target" --version

# The failure this catches is not hypothetical: an older binary in a directory
# earlier on PATH silently keeps serving every `gitwho` you type, and the
# install looks like it worked.
found="$(command -v gitwho || true)"
if [ -z "$found" ]; then
    echo "warning: no gitwho on PATH; add ${target%/gitwho} to it" >&2
elif [ "$found" != "$target" ]; then
    echo >&2
    echo "warning: PATH resolves gitwho to $found, not the copy just installed at" >&2
    echo "         $target -- that one will keep answering until you remove it." >&2
fi

# Deliberately not `gitwho init --write`: it appends to ~/.gitconfig and
# ~/.zshrc, which duplicates wiring you already have. Re-run it by hand only
# when the binary's *path* changes, which a plain reinstall does not do.
echo
"$target" doctor
