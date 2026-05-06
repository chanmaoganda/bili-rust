#!/usr/bin/env bash
# Build platform-native installers via `cargo tauri build`.
#
#   ./scripts/bundle.sh              # auto-detect host, only bundles whose
#                                    # external tooling is on PATH
#   ./scripts/bundle.sh deb          # explicit subset
#   ./scripts/bundle.sh deb appimage rpm
#
# Output: src-tauri/target/release/bundle/<format>/...
# Cross-compiling Windows .msi or macOS .dmg from Linux is not in scope here —
# use the .github/workflows/release.yml matrix instead.

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

command -v cargo >/dev/null || { echo "cargo not found" >&2; exit 1; }
cargo tauri --version >/dev/null 2>&1 || {
    echo "tauri-cli not installed; run: cargo install tauri-cli@^2" >&2
    exit 1
}

UNAME="$(uname -s)"

declare -a REQUESTED=()
if [[ $# -gt 0 ]]; then
    REQUESTED=("$@")
else
    case "$UNAME" in
        Linux)
            REQUESTED=(deb appimage rpm)
            ;;
        Darwin)
            REQUESTED=(app dmg)
            ;;
        MINGW*|MSYS*|CYGWIN*)
            REQUESTED=(msi nsis)
            ;;
        *)
            echo "unsupported host: $UNAME" >&2
            exit 1
            ;;
    esac
fi

# Drop bundles whose required external tool is missing on PATH, with a clear
# warning. Better to skip-and-explain than to fail mid-build with a cryptic
# tauri-bundler error.
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
        appimage)
            # tauri-bundler downloads appimagetool itself if absent, so accept unconditionally.
            SELECTED+=("$fmt") ;;
        msi|nsis)
            if [[ "$UNAME" != MINGW* && "$UNAME" != MSYS* && "$UNAME" != CYGWIN* ]]; then
                echo "skip $fmt — Windows installers require a Windows host (use the GitHub Actions release workflow)" >&2
            else
                SELECTED+=("$fmt")
            fi ;;
        app|dmg)
            if [[ "$UNAME" != "Darwin" ]]; then
                echo "skip $fmt — macOS bundles require a macOS host" >&2
            else
                SELECTED+=("$fmt")
            fi ;;
        *)
            echo "unknown bundle format: $fmt" >&2
            exit 2 ;;
    esac
done

if [[ ${#SELECTED[@]} -eq 0 ]]; then
    echo "no buildable bundles for this host with the requested set" >&2
    exit 1
fi

JOINED="$(IFS=,; echo "${SELECTED[*]}")"
echo "==> bundling: $JOINED"
cargo tauri build --bundles "$JOINED"

echo
echo "==> output:"
find src-tauri/target/release/bundle -maxdepth 3 -type f \
    \( -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" \
       -o -name "*.msi" -o -name "*.exe" -o -name "*.dmg" \) -print
