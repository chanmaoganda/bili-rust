# bili-rust

A Tauri 2 + Leptos desktop client for Bilibili.

## Stack

- **Backend** — Rust, Tauri 2, `reqwest` (rustls-tls). Lives in `src-tauri/`.
- **Frontend** — Leptos 0.8 CSR compiled to WASM via Trunk. Lives in `ui/`.
- **Custom URI schemes** — `bilistream://` for DASH segments and `biliimg://` for images, so the WebView can play/display Bilibili CDN URLs without tripping CSP or the CDN's `Referer` checks.

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli@^2
```

## Run

```sh
cargo tauri dev                  # hot-reload UI + backend
cargo tauri build                # release: binary + installer bundles
cargo tauri build --no-bundle    # release: binary only
```

`cargo tauri dev`/`build` runs the Trunk step automatically; you only need to invoke Trunk directly when iterating on the UI in isolation (`cd ui && trunk serve --port 1420`).

## First-time login

Launch the app and navigate to `/login`. Scan the QR code with the Bilibili mobile app and confirm; the backend writes `cookies.json` at the repo root and swaps the live session in place — no restart needed. Set `BILI_COOKIES` to override the cookie path.

## Logging

`tracing_subscriber` honors `RUST_LOG`. Default is `info,bili_rust_lib=debug`. Use `RUST_LOG=bili_rust_lib=trace` to debug WBI signing or HTTP retries.

## Tests

```sh
cargo check -p bili-rust
cargo test --test smoke -- --ignored --nocapture   # live; needs a logged-in cookies.json
```

## Reference

- Bilibili HTTP API: <https://sessionhu.github.io/bilibili-API-collect/>
- Architecture and load-bearing patterns: see [`CLAUDE.md`](./CLAUDE.md).
