#!/usr/bin/env bash
# Build the Leptos/Trunk UI into ui/dist if any source is newer than the
# current dist index. Invoked by tauri.conf.json's beforeBuildCommand.
#
# Skipping when fresh keeps repeat `cargo tauri build` runs as cache hits,
# because Trunk rewrites every file in ui/dist on each run (same bytes,
# new mtimes), which would otherwise trip tauri-build's
# `cargo:rerun-if-changed=ui/dist` watcher and force an LTO relink.

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

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
