#!/usr/bin/env bash
# Build a no-bundle release of bili-rust and package the binary as a tarball
# ready to ship. Run from any directory.
#
# Output: dist/bili-rust-<version>-linux-<arch>.tar.gz
# Optional flag: --install copies the binary to ~/.local/bin/bili-rust as well.

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --install) INSTALL=1 ;;
        *) echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

VERSION="$(grep -m1 '^version *= *' src-tauri/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
BIN_NAME="bili-rust"
TARGET_BIN="target/release/${BIN_NAME}"
STAGE="dist/${BIN_NAME}-${VERSION}-linux-${ARCH}"
TARBALL="${STAGE}.tar.gz"

echo "==> bili-rust ${VERSION} / linux-${ARCH}"

# Sanity-check prerequisites we can't recover from.
command -v cargo >/dev/null || { echo "cargo not found" >&2; exit 1; }
cargo tauri --version >/dev/null 2>&1 || {
    echo "tauri-cli not installed; run: cargo install tauri-cli@^2" >&2
    exit 1
}
rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$' || {
    echo "wasm32 target not installed; run: rustup target add wasm32-unknown-unknown" >&2
    exit 1
}

# Run Trunk only if any file under ui/src is newer than the current dist
# index. Trunk rewrites all dist files on every run (same bytes, new mtimes),
# which triggers tauri-build's frontendDist watcher and forces an LTO relink
# of bili-rust even when nothing has changed. Skipping when fresh keeps
# repeat builds at zero work.
DIST_INDEX="ui/dist/index.html"
need_trunk=1
if [[ -f "$DIST_INDEX" ]]; then
    if [[ -z "$(find ui/src ui/style.css ui/index.html ui/Trunk.toml -newer "$DIST_INDEX" -print -quit 2>/dev/null)" ]]; then
        need_trunk=0
    fi
fi
if (( need_trunk )); then
    echo "==> building UI (trunk)"
    (cd ui && trunk build --release)
else
    echo "==> ui/dist is fresh, skipping trunk"
fi

# Bypass `cargo tauri build` so a second run with no source changes is a true
# cache hit. The Tauri wrapper would re-run beforeBuildCommand unconditionally,
# defeating cargo incrementality.
echo "==> building release binary"
cargo build --release -p bili-rust --bin bili-rust

[[ -x "$TARGET_BIN" ]] || { echo "expected binary at $TARGET_BIN, not found" >&2; exit 1; }

echo "==> staging $STAGE"
rm -rf "$STAGE" "$TARBALL"
mkdir -p "$STAGE"
cp "$TARGET_BIN" "$STAGE/"
[[ -f LICENSE ]] && cp LICENSE "$STAGE/"
[[ -f README.md ]] && cp README.md "$STAGE/"

echo "==> packaging $TARBALL"
tar -C dist -czf "$TARBALL" "$(basename "$STAGE")"
SIZE="$(du -h "$TARBALL" | cut -f1)"

if [[ "$INSTALL" -eq 1 ]]; then
    DEST="${HOME}/.local/bin/${BIN_NAME}"
    mkdir -p "$(dirname "$DEST")"
    cp "$TARGET_BIN" "$DEST"
    echo "==> installed: $DEST"
fi

echo
echo "✓ tarball: $TARBALL ($SIZE)"
echo "✓ binary:  $TARGET_BIN"
[[ "$INSTALL" -eq 1 ]] && echo "✓ installed to ~/.local/bin/${BIN_NAME}"
