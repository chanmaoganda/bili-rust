# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Tauri 2 desktop Bilibili client. Cargo workspace with two members:
- `src-tauri/` — Rust backend (Tauri host process, Bilibili API client)
- `ui/` — Leptos 0.8 + WASM frontend, built by Trunk

The release profile in the workspace `Cargo.toml` uses `lto = true`, `codegen-units = 1`, `strip = true` — first release build is slow, subsequent ones are cached.

## Reference

- Upstream Bilibili HTTP API docs: <https://sessionhu.github.io/bilibili-API-collect/> — primary source for endpoint shapes, query params, and the WBI signing scheme. Consult this before adding a new `Bili::*` method.

## Commands

| Task | Command |
|---|---|
| Dev (hot-reload UI + backend) | `cargo tauri dev` |
| Release build (binary + bundles) | `cargo tauri build` |
| Release binary, no installers | `cargo tauri build --no-bundle` |
| UI alone (WASM → `ui/dist/`) | `cd ui && trunk build --release` |
| UI dev server only | `cd ui && trunk serve --port 1420` |
| Backend type-check | `cargo check -p bili-rust-lib` |
| Live integration smoke test | `cargo test --test smoke -- --ignored --nocapture` (requires valid `cookies.json`) |
| Refresh login cookies | `node login.js` (Playwright QR flow → writes `cookies.json`) |

`cargo tauri dev`/`build` auto-runs the Trunk step via `tauri.conf.json`'s `beforeDevCommand`/`beforeBuildCommand` — don't run Trunk separately unless iterating on the UI in isolation.

Prereqs: `rustup target add wasm32-unknown-unknown`, `cargo install trunk tauri-cli@^2`.

## Architecture

### Backend (`src-tauri/src/`)

- `lib.rs` — `run()` is the Tauri entry point. Loads `cookies.json` once at startup, builds an `Arc<Bili>` HTTP client, registers the `bilistream://` and `biliimg://` URI scheme handlers, and exposes commands.
- `api.rs` — `Bili` struct wraps a singleton `reqwest::Client` + cookie jar. WBI keys cached in `Arc<RwLock<Option<(WbiKeys, Instant)>>>` with a 1-hour TTL. Shared to commands via Tauri `State<Arc<Bili>>`.
- `commands.rs` — Six `#[tauri::command]`s: `get_user_info`, `get_rcmd`, `get_related`, `get_play_info`, `get_danmaku`, `get_comments`. All return `Result<_, String>` (errors stringified at the Tauri boundary).
- `wbi.rs` — Bilibili WBI signature scheme (mixin-key permutation + `wts`/`w_rid`). **Most endpoints reject unsigned requests with HTTP 412** — always sign via `Bili`'s helpers, don't hand-build query strings.
- `stream.rs` — Async URI scheme handler. Rewrites HTTPS DASH segment URLs to `bilistream://seg/<base64>` and image URLs to `biliimg://img/<base64>` so the frontend can embed them without CSP violations. The handler proxies the actual HTTPS fetch through `Bili` so Bilibili CDN's `Referer`/`Origin` requirements are met.
- `cookies.rs` — Reads `cookies.json` at startup (path overridable via `BILI_COOKIES`). **No hot-reload**: re-running `login.js` requires restarting the app.
- `danmaku.rs` — Parses Bilibili's `list.so` XML response, auto-detecting raw deflate vs. pre-inflated payloads.

### Frontend (`ui/src/`)

- Leptos 0.8 CSR + `leptos_router` 0.8. Mounted on `#app` from `main.rs`.
- `api.rs` — Thin wrapper around `window.__TAURI__.core.invoke()`; one async fn per backend command.
- `state.rs` — `RecommendState` is provided as **app-level context** in `app.rs` so the Home feed survives back-navigation from `/watch/:bvid`. Don't move this state into the route component or the feed will refetch on every back-nav.
- `prefs.rs` — localStorage-backed preferences: `bili.preferred_qn` (sticky video quality), `bili.danmaku_enabled`, `bili.danmaku_opacity`.
- `routes/` — `home` (feed + infinite scroll), `watch` (player + danmaku + related).
- `components/player.rs` — Wraps dash.js. Stores the `HtmlVideoElement` in a `StoredValue` and tears down dash.js inside `on_cleanup()`. Skipping the explicit teardown causes a WASM panic on unmount (regression fixed in `ddb0b63`).

## Load-bearing patterns

- **WBI signing is mandatory** for nav/recommend/related/play/search endpoints. Add a new endpoint? Route it through `Bili` so it inherits `wbi::sign()`.
- **Custom URI schemes are not optional.** `tauri.conf.json`'s CSP lists `bilistream:` under `media-src` and `biliimg:` under `img-src`; raw HTTPS Bilibili URLs in `<video>` / `<img>` will be blocked. Always rewrite via the helpers in `stream.rs` / `commands.rs::proxy_image`.
- **Dash.js teardown.** Any change to `components/player.rs` must keep `on_cleanup()` calling the JS teardown before Leptos drops the node.
- **Cookies are read once at startup.** `login.js` drives a CDP-controlled browser via Playwright to walk the QR-login flow, renders the QR to the terminal with `qrcode-terminal`, and writes `cookies.json` at the repo root. After re-login, restart the Tauri app — there is no hot-reload.
- **Logging.** `tracing_subscriber` honors `RUST_LOG`; the default filter is `info,bili_rust_lib=debug`. Bump to e.g. `RUST_LOG=bili_rust_lib=trace` when diagnosing WBI/sign failures.
- **Quality preference round-trip.** UI must call `set_preferred_qn()` after the user picks a quality, otherwise the next session falls back to the default.

## Stack notes

- `reqwest` is built with `rustls-tls` (no native-tls) — system certificate stores are not used.
- Danmaku decode path: `flate2` (raw deflate) + `quick-xml` (streaming SAX). Bilibili sometimes returns pre-inflated XML; `danmaku.rs` auto-detects.
- WBI key cache uses `parking_lot::RwLock`; lazy statics use `once_cell`.
