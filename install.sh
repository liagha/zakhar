#!/usr/bin/env bash
set -euo pipefail

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Linux) target="linux" ;;
    Darwin) target="darwin" ;;
    *) echo "unsupported OS: $os" >&2; exit 1 ;;
esac

case "$arch" in
    x86_64|amd64) target="${target}-x86_64" ;;
    aarch64|arm64) target="${target}-aarch64" ;;
    *) echo "unsupported arch: $arch" >&2; exit 1 ;;
esac

dest="${ZAKHAR_INSTALL_DIR:-$HOME/.local/bin}"
url="https://github.com/liagha/zakhar/releases/latest/download/zakhar-${target}"
tmp="$(mktemp)"

mkdir -p "$dest"

if curl -fsSL "$url" -o "$tmp" 2>/dev/null; then
    chmod +x "$tmp"
    mv "$tmp" "$dest/zakhar"
    echo "installed zakhar to $dest/zakhar"
    "$dest/zakhar" --version
    exit 0
fi
rm -f "$tmp"

echo "no prebuilt binary for $target — building from source (needs cargo)" >&2
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. install rust from https://rustup.rs then re-run" >&2
    exit 1
fi

build="$(mktemp -d)"
trap 'rm -rf "$build"' EXIT
git clone --depth 1 https://github.com/liagha/zakhar.git "$build/repo"
cargo build --release --manifest-path "$build/repo/Cargo.toml"
mv "$build/repo/target/release/zakhar" "$dest/zakhar"
echo "installed zakhar to $dest/zakhar"
"$dest/zakhar" --version