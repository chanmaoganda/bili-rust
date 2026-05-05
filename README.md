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

## Disclaimer / 免责声明

This is an **unofficial, personal-use** desktop client. It is not affiliated with, endorsed by, or sponsored by 上海宽娱数码科技有限公司 (Bilibili Inc.). All trademarks belong to their respective owners.

本项目是 **非官方、仅供个人学习使用** 的桌面客户端，与上海宽娱数码科技有限公司（哔哩哔哩）没有任何关联，未获其授权或认可。所有商标归各自所有者所有。

- The project is provided **AS IS, without warranty of any kind**. The author assumes no responsibility for any consequences of using this software, including but not limited to account suspension, content unavailability, or service-side rule changes.
- Use of the project requires logging in with your own Bilibili account. The software acts on your behalf using your own session cookies; **you are solely responsible** for whether your usage complies with the [Bilibili User Agreement (用户协议)](https://www.bilibili.com/protocal/licence.html).
- Distributing, promoting, monetizing, or redistributing content fetched through this client **may violate Chinese unfair-competition law and Bilibili's terms** — please don't.
- 本软件按"原样"提供，**不附带任何明示或默示担保**。作者不对使用本软件造成的任何后果（包括但不限于账号封禁、内容不可用、服务端规则变更）承担责任。
- 使用本软件需登录你自己的哔哩哔哩账号，软件以你的 session cookies 代理你的操作；**你个人需自行承担**使用是否合规的责任。
- **请勿** 分发、推广、商业化本项目，或转载/二次分发通过本客户端获取的内容——这可能触犯反不正当竞争法及哔哩哔哩用户协议。

## Out-of-scope features / 不接受的贡献

To keep the project on the right side of the line, the following are **explicitly out of scope** and will not be accepted as PRs:

为了让项目尽量远离争议红线，以下功能 **明确不在项目范围内**，相关 PR 不会被合并：

- Ad blocking, sponsor stripping, splash-screen removal. / 屏蔽广告、跳过推广、去开屏。
- Region / 大会员 / 港澳台 unlock, paywall bypass, DRM circumvention. / 解锁地区限制、破解大会员、绕过付费墙、规避 DRM。
- Bulk downloading, scraping, archival of others' uploads. / 批量下载、爬取、归档他人上传的内容。
- Automated mass actions (mass-following, mass-liking, batch comments, etc.). / 自动化批量操作（批量关注、点赞、评论等）。
- Detailed reverse-engineering write-ups inside this repo (WBI algorithm walkthroughs, signing-scheme tutorials, decoded protobuf schemas). Source code is unavoidable; **promotional documentation is not**. / 在本仓库内编写详细的逆向分析文档（WBI 算法解析、签名方案教程、protobuf 解码说明等）。源码层面不可避免，但**面向用户/读者的宣传性文档不写**。
- Anonymous / cookie-less access modes that work around login flows. / 绕过登录流程的匿名访问模式。

The supported scope is: a personal Bilibili viewer that works **with your own logged-in account**, exactly like the official web client does — nothing more.

支持的范围是：以**你自己已登录的账号**使用的个人观看器，行为与官方网页端一致——仅此而已。

## If you receive a takedown notice / 收到下架通知怎么办

If you publicly fork or distribute this project and receive a DMCA / 律师函 / takedown request:

1. **Archive the repository immediately** and stop distributing binaries. / 立即归档仓库并停止分发构建产物。
2. Don't escalate publicly. / 不要在网上对线。
3. Consult a lawyer if the request alleges damages, not just IP infringement. / 如对方主张损失（而非仅 IP 侵权），请咨询律师。

The 2024 takedown of the upstream API-documentation project [`SocialSisterYi/bilibili-API-collect`](https://github.com/SocialSisterYi/bilibili-API-collect) (now archived) is the cautionary precedent — the issue there was a comprehensive *public reverse-engineering manual*, not a personal client. Stay on the right side of that distinction.

2024 年上游 API 文档仓库 [`SocialSisterYi/bilibili-API-collect`](https://github.com/SocialSisterYi/bilibili-API-collect) 被起诉下架是先例——争议点在于"系统性公开整理逆向接口文档"，而非"自己写客户端使用"。请保持这条边界。
