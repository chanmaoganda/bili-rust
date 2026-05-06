#!/usr/bin/env bash
# Build Linux installers (deb/rpm) and a no-bundle binary tarball,
# then collect everything under dist/ for shipping.
#
#   ./scripts/linux.sh         # default: deb rpm + tarball
#   ./scripts/linux.sh deb     # explicit subset (tarball still produced)
#
# Output: dist/*.{deb,rpm,tar.gz}
# Cross-platform bundling lives in .github/workflows/release.yml.

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

UNAME="$(uname -s)"
if [[ "$UNAME" != "Linux" ]]; then
    echo "ERROR: linux.sh must run on a Linux host (got: $UNAME)" >&2
    echo "       For Windows/macOS bundles use the GitHub Actions release workflow." >&2
    exit 1
fi

# ── Prereqs ──────────────────────────────────────────────────────────
command -v cargo >/dev/null || { echo "cargo not found" >&2; exit 1; }
cargo tauri --version >/dev/null 2>&1 || {
    echo "tauri-cli not installed; run: cargo install tauri-cli@^2" >&2
    exit 1
}
rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$' || {
    echo "wasm32 target not installed; run: rustup target add wasm32-unknown-unknown" >&2
    exit 1
}

# ── Pick bundle formats ──────────────────────────────────────────────
declare -a REQUESTED=()
if [[ $# -gt 0 ]]; then
    REQUESTED=("$@")
else
    REQUESTED=(deb rpm)
fi

declare -a SELECTED=()
for fmt in "${REQUESTED[@]}"; do
    case "$fmt" in
        deb)
            if command -v dpkg-deb >/dev/null; then SELECTED+=("$fmt"); else
                echo "skip deb — dpkg-deb not on PATH" >&2
            fi ;;
        rpm)
            if command -v rpmbuild >/dev/null; then SELECTED+=("$fmt"); else
                echo "skip rpm — rpmbuild not on PATH (Arch: pacman -S rpm-tools; Debian: apt install rpm)" >&2
            fi ;;
        *)
            echo "ERROR: unsupported bundle format '$fmt' (linux.sh accepts: deb rpm)" >&2
            exit 2 ;;
    esac
done

if [[ ${#SELECTED[@]} -eq 0 ]]; then
    echo "no buildable bundles for this host with the requested set" >&2
    exit 1
fi

JOINED="$(IFS=,; echo "${SELECTED[*]}")"

# ── Build ────────────────────────────────────────────────────────────
echo "==> cargo tauri build --bundles ${JOINED}"
cargo tauri build --bundles "$JOINED"

# ── Stage outputs into dist/ ─────────────────────────────────────────
VERSION="$(grep -m1 '^version *= *' src-tauri/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
BIN_NAME="bili-rust"
TARGET_BIN="target/release/${BIN_NAME}"
STAGE="dist/${BIN_NAME}-${VERSION}-linux-${ARCH}"
TARBALL="${STAGE}.tar.gz"

mkdir -p dist

# Copy installers, filtered by current version so prior-version artifacts
# left behind by tauri-bundler don't ride along into dist/.
# Naming: deb uses _<ver>_, rpm uses -<ver>-.
shopt -s globstar nullglob
for f in target/release/bundle/**/*"_${VERSION}_"*.deb \
         target/release/bundle/**/*"-${VERSION}-"*.rpm; do
    [[ -e "$f" ]] && cp -f "$f" dist/
done
shopt -u globstar nullglob

if [[ -x "$TARGET_BIN" ]]; then
    echo "==> staging $STAGE"
    rm -rf "$STAGE" "$TARBALL"
    mkdir -p "$STAGE"
    cp "$TARGET_BIN" "$STAGE/"
    [[ -f LICENSE ]] && cp LICENSE "$STAGE/"
    [[ -f README.md ]] && cp README.md "$STAGE/"

    echo "==> packaging $TARBALL"
    tar -C dist -czf "$TARBALL" "$(basename "$STAGE")"
else
    echo "WARN: $TARGET_BIN missing — skipping tarball" >&2
fi

# ── Manifest ─────────────────────────────────────────────────────────
echo
echo "==> dist/:"
ls -lh dist/ | tail -n +2
