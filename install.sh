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

mkdir -p "$dest"

tmp="$(mktemp)"
curl -fsSL "$url" -o "$tmp"
chmod +x "$tmp"
mv "$tmp" "$dest/zakhar"

echo "installed zakhar to $dest/zakhar"
"$dest/zakhar" --version