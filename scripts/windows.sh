#!/usr/bin/env bash
# Cross-build the Windows NSIS installer for bili-rust from a Linux host
# and copy the .exe into dist/.
#
#   ./scripts/windows.sh
#
# Output: dist/*.exe
#
# Notes
# - NSIS works because makensis is portable Linux-native; Tauri downloads its
#   NSIS plugins and the WebView2 bootstrapper into ~/.local/share/tauri/.
# - MSI is not supported here — Tauri's WiX path runs Windows-only .exe tools.
#   Use the .github/workflows/release.yml matrix for MSI.
# - Target is x86_64-pc-windows-gnu (mingw-w64), not -msvc, so no cargo-xwin
#   or Wine binfmt setup is required on the host.

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TARGET="x86_64-pc-windows-gnu"

# ── Prereqs ──────────────────────────────────────────────────────────
echo "==> Checking prerequisites..."

missing=()
for cmd in cargo x86_64-w64-mingw32-gcc makensis; do
    command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
done
cargo tauri --version >/dev/null 2>&1 || missing+=("tauri-cli (run: cargo install tauri-cli@^2)")

if (( ${#missing[@]} > 0 )); then
    echo "ERROR: missing required tools: ${missing[*]}" >&2
    cat >&2 <<EOF

Arch:    pacman -S mingw-w64-gcc && paru -S nsis
Debian:  apt install gcc-mingw-w64-x86-64 nsis
Rust:    rustup target add x86_64-pc-windows-gnu wasm32-unknown-unknown
Tauri:   cargo install tauri-cli@^2 --locked
EOF
    exit 1
fi

if ! rustup target list --installed | grep -q "^${TARGET}\$"; then
    echo "==> Adding Rust target ${TARGET}..."
    rustup target add "$TARGET"
fi

if ! rustup target list --installed | grep -q "^wasm32-unknown-unknown\$"; then
    echo "==> Adding Rust target wasm32-unknown-unknown (needed for the Leptos UI)..."
    rustup target add wasm32-unknown-unknown
fi

# ── Build ────────────────────────────────────────────────────────────
echo "==> cargo tauri build --target ${TARGET} --bundles nsis"
cargo tauri build --target "$TARGET" --bundles nsis

# ── Stage output into dist/ ──────────────────────────────────────────
mkdir -p dist
shopt -s nullglob
for f in "target/${TARGET}/release/bundle/nsis"/*.exe; do
    cp -f "$f" dist/
done
shopt -u nullglob

# ── Manifest ─────────────────────────────────────────────────────────
echo
echo "==> dist/:"
ls -lh dist/ | tail -n +2
