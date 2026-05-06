use crate::cookies::{Cookies, RawCookie};
use crate::wbi::{self, WbiKeys};
use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// One immutable login session: a reqwest `Client` whose default headers
/// include the cookie string for these `Cookies`. Sessions are wrapped in
/// `Arc` and swapped atomically on login so in-flight requests keep using the
/// session they started with.
pub struct Session {
    pub client: Client,
    pub cookies: Cookies,
}

impl Session {
    pub fn build(cookies: Cookies) -> Result<Arc<Self>> {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://www.bilibili.com/"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://www.bilibili.com"),
        );
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&cookies.header).context("cookie header bytes")?,
        );

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .tcp_nodelay(true)
            .build()
            .context("build reqwest client")?;

        Ok(Arc::new(Self { client, cookies }))
    }

    /// A bare client with no cookies — used by the QR-login flow so we don't
    /// leak any prior identity into the passport endpoints.
    pub fn anonymous() -> Result<Client> {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://passport.bilibili.com/"),
        );
        Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .context("build anonymous client")
    }
}

#[derive(Clone)]
pub struct Bili {
    session: Arc<RwLock<Arc<Session>>>,
    wbi_cache: Arc<RwLock<Option<(WbiKeys, Instant)>>>,
    cookies_path: Arc<RwLock<Option<PathBuf>>>,
}

impl Bili {
    pub fn new(cookies: Cookies) -> Result<Self> {
        let session = Session::build(cookies)?;
        Ok(Self {
            session: Arc::new(RwLock::new(session)),
            wbi_cache: Arc::new(RwLock::new(None)),
            cookies_path: Arc::new(RwLock::new(None)),
        })
    }

    /// Construct a Bili with no cookies — caller must `replace_cookies` before
    /// any authenticated call. Used when the app starts without a `cookies.json`
    /// so the user can log in from inside the app.
    pub fn empty() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://www.bilibili.com/"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://www.bilibili.com"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(60))
            .build()
            .context("build empty reqwest client")?;
        let cookies = Cookies {
            raw: Vec::new(),
            header: String::new(),
            csrf: String::new(),
            uid: String::new(),
        };
        let session = Arc::new(Session { client, cookies });
        Ok(Self {
            session: Arc::new(RwLock::new(session)),
            wbi_cache: Arc::new(RwLock::new(None)),
            cookies_path: Arc::new(RwLock::new(None)),
        })
    }

    pub fn set_cookies_path(&self, path: PathBuf) {
        *self.cookies_path.write() = Some(path);
    }

    /// Snapshot the active session. Cheap: `Arc::clone`. Callers should hold
    /// the snapshot for the duration of one logical request — a concurrent
    /// login can swap in a new session, but in-flight calls keep their pinned
    /// snapshot.
    pub fn session(&self) -> Arc<Session> {
        self.session.read().clone()
    }

    pub fn http(&self) -> Client {
        self.session().client.clone()
    }

    pub fn uid(&self) -> String {
        self.session().cookies.uid.clone()
    }

    /// Replace the active session with one built from `raw` cookies, and
    /// persist them to disk if a cookies path is known.
    pub fn replace_cookies(&self, raw: Vec<RawCookie>) -> Result<()> {
        let cookies = Cookies::from_raw(raw)?;
        let path = self.cookies_path.read().clone();
        if let Some(p) = path {
            cookies
                .write_to(&p)
                .with_context(|| format!("persist new cookies to {}", p.display()))?;
        }
        let session = Session::build(cookies)?;
        *self.session.write() = session;
        // Force a fresh WBI fetch — keys travel with the session.
        *self.wbi_cache.write() = None;
        // QR login only captures auth cookies — re-attach fingerprint cookies
        // in the background so the new session is risk-control-clean too.
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = me.ensure_fingerprint_cookies().await {
                tracing::warn!(error = %e,
                    "ensure_fingerprint_cookies after login failed");
            }
        });
        Ok(())
    }

    /// Bilibili's risk-control checks the session for the device-fingerprint
    /// cookies a real browser carries (`buvid3`, `buvid4`, `b_nut`, `_uuid`).
    /// The QR-login flow only captures auth cookies from passport.bilibili.com
    /// so these are missing on first run, and write endpoints like
    /// `archive/like` start failing with `code=-403 账号异常`. This populates
    /// any of the four that are missing — `buvid3`/`buvid4` from Bilibili's
    /// public `finger/spi` endpoint, `b_nut`/`_uuid` synthesized locally —
    /// and atomically swaps in a session that includes them. Idempotent: a
    /// second call when all four are present is a no-op.
    pub async fn ensure_fingerprint_cookies(&self) -> Result<()> {
        let snap = self.session();
        let names: std::collections::HashSet<&str> =
            snap.cookies.raw.iter().map(|c| c.name.as_str()).collect();
        let need_buvid = !names.contains("buvid3") || !names.contains("buvid4");
        let need_b_nut = !names.contains("b_nut");
        let need_uuid = !names.contains("_uuid");
        if !need_buvid && !need_b_nut && !need_uuid {
            return Ok(());
        }

        let mut extras: Vec<RawCookie> = Vec::new();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if need_buvid {
            match self.fetch_buvids().await {
                Ok((b3, b4)) => {
                    if !names.contains("buvid3") {
                        extras.push(make_domain_cookie("buvid3", &b3));
                    }
                    if !names.contains("buvid4") {
                        extras.push(make_domain_cookie("buvid4", &b4));
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "fetch_buvids failed; skipping buvid3/buvid4");
                }
            }
        }
        if need_b_nut {
            extras.push(make_domain_cookie("b_nut", &now_secs.to_string()));
        }
        if need_uuid {
            extras.push(make_domain_cookie("_uuid", &synth_uuid_cookie()));
        }
        if extras.is_empty() {
            return Ok(());
        }

        let added: Vec<String> = extras.iter().map(|c| c.name.clone()).collect();
        let merged = Cookies::from_raw(snap.cookies.raw.clone())?.with_extra(extras)?;
        let path = self.cookies_path.read().clone();
        if let Some(p) = path {
            if let Err(e) = merged.write_to(&p) {
                tracing::warn!(error = %e, path = %p.display(),
                    "persist fingerprint cookies failed (continuing in memory)");
            }
        }
        let session = Session::build(merged)?;
        *self.session.write() = session;
        *self.wbi_cache.write() = None;
        tracing::info!(added = ?added, "fingerprint cookies attached");
        Ok(())
    }

    async fn fetch_buvids(&self) -> Result<(String, String)> {
        #[derive(Deserialize)]
        struct Spi {
            b_3: String,
            b_4: String,
        }
        let v: ApiEnvelope<Spi> = self
            .http()
            .get("https://api.bilibili.com/x/frontend/finger/spi")
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "finger/spi failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let d = v.data.ok_or_else(|| anyhow!("finger/spi: no data"))?;
        Ok((d.b_3, d.b_4))
    }

    /// /x/web-interface/nav — user info + WBI keys.
    ///
    /// Bilibili returns `code=-101` ("账号未登录") for unauthenticated callers
    /// but still includes `wbi_img` in `data`, so we accept code=-101 as a
    /// legitimate "logged out" response. This is what lets the home feed (and
    /// any other WBI-signed endpoint) work for guests.
    pub async fn nav(&self) -> Result<NavResponse> {
        let v: ApiEnvelope<NavData> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/nav")
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 && v.code != -101 {
            return Err(anyhow!("nav failed: code={} msg={}", v.code, v.message));
        }
        Ok(NavResponse {
            data: v.data.ok_or_else(|| anyhow!("nav: no data"))?,
        })
    }

    async fn wbi_keys(&self) -> Result<WbiKeys> {
        // 1h cache
        if let Some((k, when)) = &*self.wbi_cache.read() {
            if when.elapsed() < Duration::from_secs(3600) {
                return Ok(k.clone());
            }
        }
        let nav = self.nav().await?;
        let keys = WbiKeys::from_urls(&nav.data.wbi_img.img_url, &nav.data.wbi_img.sub_url);
        *self.wbi_cache.write() = Some((keys.clone(), Instant::now()));
        Ok(keys)
    }

    /// Personalized homepage feed.
    ///
    /// `fresh_idx` is the per-page scroll cursor (increment on infinite-scroll);
    /// `brush` is the session-level "换一换" counter (increment only on explicit
    /// rotate, otherwise pinned). Mixing the two confuses the ranker — see
    /// bilibili-API-collect docs.
    ///
    /// `last_showlist` is a CSV of `av_<aid>` already shown to the user, used by
    /// the server to dedup across pages. Pass an empty string for the first
    /// request.
    pub async fn rcmd(
        &self,
        fresh_idx: u32,
        brush: u32,
        ps: u32,
        last_showlist: &str,
    ) -> Result<Vec<Value>> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("fresh_type".to_string(), "4".to_string());
        params.insert("version".to_string(), "1".to_string());
        params.insert("ps".to_string(), ps.to_string());
        params.insert("fresh_idx".to_string(), fresh_idx.to_string());
        params.insert("fresh_idx_1h".to_string(), fresh_idx.to_string());
        params.insert("brush".to_string(), brush.to_string());
        params.insert("homepage_ver".to_string(), "1".to_string());
        params.insert("feed_version".to_string(), "V8".to_string());
        params.insert("web_location".to_string(), "1430650".to_string());
        if !last_showlist.is_empty() {
            params.insert("last_showlist".to_string(), last_showlist.to_string());
        }
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let v: ApiEnvelope<RcmdData> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/wbi/index/top/feed/rcmd")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("rcmd failed: code={} msg={}", v.code, v.message));
        }
        Ok(v.data.map(|d| d.item).unwrap_or_default())
    }

    /// /x/feed/dislike — train the rcmd ranker by hiding a card and signalling
    /// why. `reason_id` semantics (matched against B站 web client):
    ///   1 = 内容不感兴趣 (just `id`)
    ///   2 = UP不再推荐    (`id` + `mid`)
    ///   3 = 分区不再推荐  (`id` + `rid`)
    /// Server side is async — operation succeeds even if the next /rcmd batch
    /// is already in flight.
    pub async fn dislike(
        &self,
        goto: &str,
        id: i64,
        mid: Option<i64>,
        rid: Option<i64>,
        tag_id: Option<i64>,
        reason_id: u32,
    ) -> Result<()> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("goto".to_string(), goto.to_string());
        params.insert("id".to_string(), id.to_string());
        if let Some(m) = mid {
            params.insert("mid".to_string(), m.to_string());
        }
        if let Some(r) = rid {
            params.insert("rid".to_string(), r.to_string());
        }
        if let Some(t) = tag_id {
            params.insert("tag_id".to_string(), t.to_string());
        }
        params.insert("reason_id".to_string(), reason_id.to_string());
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let v: ApiEnvelope<Value> = self
            .http()
            .get("https://api.bilibili.com/x/feed/dislike")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("dislike failed: code={} msg={}", v.code, v.message));
        }
        tracing::debug!(goto, id, ?mid, ?rid, ?tag_id, reason_id, "dislike ok");
        Ok(())
    }

    /// Related videos for a given bvid
    pub async fn related(&self, bvid: &str) -> Result<Vec<Value>> {
        let v: ApiEnvelope<Vec<Value>> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/archive/related")
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("related failed: code={} msg={}", v.code, v.message));
        }
        Ok(v.data.unwrap_or_default())
    }

    /// view info for a video — gives us the cid for the first part
    pub async fn view(&self, bvid: &str) -> Result<ViewData> {
        let v: ApiEnvelope<ViewData> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/view")
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("view failed: code={} msg={}", v.code, v.message));
        }
        v.data.ok_or_else(|| anyhow!("view: no data"))
    }

    /// /x/v2/reply — top-level comments for a video (type=1, oid=aid).
    /// Pagination endpoint; `pn` is 1-based, `ps` ≤ 20.
    pub async fn comments(&self, aid: i64, pn: u32, ps: u32, sort: u32) -> Result<Value> {
        let v: ApiEnvelope<Value> = self
            .http()
            .get("https://api.bilibili.com/x/v2/reply")
            .query(&[
                ("type", "1".to_string()),
                ("oid", aid.to_string()),
                ("pn", pn.to_string()),
                ("ps", ps.to_string()),
                ("sort", sort.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "comments failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        v.data.ok_or_else(|| anyhow!("comments: no data"))
    }

    /// /x/v2/dm/post — post a danmaku at `progress_ms` into the video.
    /// `mode`: 1 = scroll, 4 = bottom, 5 = top. `color` is decimal RGB
    /// (0xFFFFFF = white). `fontsize`: 25 = standard, 18 = small, 36 = large.
    /// Returns the new `dmid` on success.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_danmaku(
        &self,
        aid: i64,
        cid: i64,
        bvid: &str,
        msg: &str,
        progress_ms: i64,
        mode: u8,
        color: u32,
        fontsize: u8,
    ) -> Result<i64> {
        let csrf = self.session().cookies.csrf.clone();
        if csrf.is_empty() {
            return Err(anyhow!("send_danmaku: not logged in (no csrf)"));
        }
        let rnd = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut form = BTreeMap::new();
        form.insert("type", "1".to_string());
        form.insert("aid", aid.to_string());
        form.insert("oid", cid.to_string());
        form.insert("bvid", bvid.to_string());
        form.insert("msg", msg.to_string());
        form.insert("progress", progress_ms.to_string());
        form.insert("mode", mode.to_string());
        form.insert("color", color.to_string());
        form.insert("fontsize", fontsize.to_string());
        form.insert("pool", "0".to_string());
        form.insert("plat", "1".to_string());
        form.insert("rnd", rnd.to_string());
        form.insert("csrf", csrf);

        let url = "https://api.bilibili.com/x/v2/dm/post";
        let resp = self.http().post(url).form(&form).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        let v: ApiEnvelope<Value> = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "send_danmaku parse failed: status={status} body={}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                cid,
                mode,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "send_danmaku non-zero code"
            );
            return Err(anyhow!(
                "send_danmaku failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let dmid = v
            .data
            .as_ref()
            .and_then(|d| d.get("dmid"))
            .and_then(|v| v.as_i64())
            .or_else(|| {
                v.data
                    .as_ref()
                    .and_then(|d| d.get("dmid_str"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
            })
            .ok_or_else(|| {
                anyhow!(
                    "send_danmaku: missing dmid in response body={}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        tracing::info!(cid, dmid, mode, "send_danmaku ok");
        Ok(dmid)
    }

    /// /x/v2/reply/add — post a top-level comment (when `root`/`parent` are
    /// `None`) or a reply (set `root` to the top-level rpid and `parent` to the
    /// rpid being replied to; for direct replies they're the same value).
    /// Returns the new `rpid` on success.
    pub async fn reply_add(
        &self,
        oid: i64,
        message: &str,
        root: Option<i64>,
        parent: Option<i64>,
    ) -> Result<i64> {
        let csrf = self.session().cookies.csrf.clone();
        if csrf.is_empty() {
            return Err(anyhow!("reply_add: not logged in (no csrf)"));
        }
        let mut form = BTreeMap::new();
        form.insert("oid", oid.to_string());
        form.insert("type", "1".to_string());
        form.insert("message", message.to_string());
        form.insert("plat", "1".to_string());
        if let Some(r) = root {
            form.insert("root", r.to_string());
        }
        if let Some(p) = parent {
            form.insert("parent", p.to_string());
        }
        form.insert("csrf", csrf);

        let url = "https://api.bilibili.com/x/v2/reply/add";
        let resp = self.http().post(url).form(&form).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        let v: ApiEnvelope<Value> = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "reply_add parse failed: status={status} body={}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                oid,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "reply_add non-zero code"
            );
            return Err(anyhow!("reply_add failed: code={} msg={}", v.code, v.message));
        }
        let rpid = v
            .data
            .as_ref()
            .and_then(|d| d.get("rpid"))
            .and_then(|v| v.as_i64())
            .or_else(|| {
                v.data
                    .as_ref()
                    .and_then(|d| d.get("rpid_str"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
            })
            .ok_or_else(|| {
                anyhow!(
                    "reply_add: missing rpid in response body={}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        tracing::info!(oid, rpid, "reply_add ok");
        Ok(rpid)
    }

    /// /x/v1/dm/list.so — danmaku XML for a given cid (deflate-compressed)
    pub async fn danmaku(&self, cid: i64) -> Result<Vec<crate::danmaku::Danmaku>> {
        let t0 = Instant::now();
        let bytes = self
            .http()
            .get("https://api.bilibili.com/x/v1/dm/list.so")
            .query(&[("oid", cid.to_string())])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let out = crate::danmaku::parse_response(&bytes)?;
        tracing::info!(
            cid,
            ms = t0.elapsed().as_millis() as u64,
            count = out.len(),
            "danmaku"
        );
        Ok(out)
    }

    /// /x/web-interface/archive/relation — current user's relation to a video.
    /// Unauthenticated callers still get code=0 with all fields zeroed.
    pub async fn archive_relation(&self, bvid: &str) -> Result<ArchiveRelation> {
        let url = "https://api.bilibili.com/x/web-interface/archive/relation";
        let bytes = self
            .http()
            .get(url)
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .bytes()
            .await?;
        let v: ApiEnvelope<ArchiveRelationRaw> =
            serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "archive_relation parse failed: body={}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                bvid,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "archive_relation non-zero code"
            );
            return Err(anyhow!(
                "archive_relation failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let r = v.data.unwrap_or_default();
        tracing::debug!(
            bvid,
            like = r.like,
            coin = r.coin,
            favorite = r.favorite,
            "archive_relation"
        );
        Ok(ArchiveRelation {
            liked: r.like,
            coined: r.coin,
            favorited: r.favorite,
        })
    }

    /// /x/relation — relation to a specific user. `attribute` is a bit mask:
    /// bit 1 (=2) "I follow them", bit 2 (=4) "they follow me",
    /// bit 7 (=128) "specially followed". So mutual = 6, special = 130, etc.
    /// We only need the "I follow them" bit.
    pub async fn user_relation(&self, mid: i64) -> Result<bool> {
        let attr = self.user_relation_raw(mid).await?;
        Ok(attr & 2 != 0)
    }

    /// Same call as `user_relation`, but returns the raw `attribute` mask so
    /// callers (smoke tests, diagnostics) can inspect every bit.
    pub async fn user_relation_raw(&self, mid: i64) -> Result<i64> {
        let url = "https://api.bilibili.com/x/relation";
        let bytes = self
            .http()
            .get(url)
            .query(&[("fid", mid.to_string())])
            .send()
            .await?
            .bytes()
            .await?;
        let v: ApiEnvelope<UserRelationRaw> =
            serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "user_relation parse failed: body={}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                fid = mid,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "user_relation non-zero code"
            );
            return Err(anyhow!(
                "user_relation failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let attr = v.data.map(|d| d.attribute).unwrap_or(0);
        tracing::debug!(fid = mid, attribute = attr, "user_relation");
        Ok(attr)
    }

    /// /x/relation/followings — list of users `vmid` follows. `pn` is 1-based,
    /// `ps` ≤ 50. Response wraps the list in `data.list` and the grand total
    /// in `data.total`.
    pub async fn followings(&self, vmid: i64, pn: u32, ps: u32) -> Result<FollowingsPage> {
        let url = "https://api.bilibili.com/x/relation/followings";
        let bytes = self
            .http()
            .get(url)
            .query(&[
                ("vmid", vmid.to_string()),
                ("pn", pn.to_string()),
                ("ps", ps.to_string()),
                ("order", "desc".to_string()),
            ])
            .send()
            .await?
            .bytes()
            .await?;
        let v: ApiEnvelope<FollowingsRaw> = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "followings parse failed: body={}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                vmid,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "followings non-zero code"
            );
            return Err(anyhow!(
                "followings failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let raw = v.data.unwrap_or_default();
        Ok(FollowingsPage {
            list: raw.list,
            total: raw.total,
        })
    }

    /// /x/relation/tag — followings within a specific tag. `tag_id = -10` is
    /// the built-in "特别关注" tag. `data` is a bare array (different shape
    /// from `/x/relation/followings`).
    pub async fn followings_tag(
        &self,
        tag_id: i64,
        pn: u32,
        ps: u32,
    ) -> Result<Vec<FollowingItem>> {
        let url = "https://api.bilibili.com/x/relation/tag";
        let bytes = self
            .http()
            .get(url)
            .query(&[
                ("tagid", tag_id.to_string()),
                ("pn", pn.to_string()),
                ("ps", ps.to_string()),
            ])
            .send()
            .await?
            .bytes()
            .await?;
        let v: ApiEnvelope<Vec<FollowingItem>> =
            serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "followings_tag parse failed: body={}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                tag_id,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "followings_tag non-zero code"
            );
            return Err(anyhow!(
                "followings_tag failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        Ok(v.data.unwrap_or_default())
    }

    /// /x/web-interface/archive/like — toggle like (1 = like, 2 = un-like).
    pub async fn like_video(&self, bvid: &str, like: bool) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("like", if like { "1" } else { "2" }.to_string());
        form.insert("csrf", csrf);
        archive_risk_fields(&mut form, bvid, &self.uid());
        self.post_form(
            "https://api.bilibili.com/x/web-interface/archive/like",
            form,
            "like_video",
        )
        .await
    }

    /// /x/web-interface/coin/add — coin a video. multiply ∈ {1, 2}.
    pub async fn coin_video(&self, bvid: &str, multiply: u8, with_like: bool) -> Result<()> {
        let m = multiply.clamp(1, 2);
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("multiply", m.to_string());
        form.insert("select_like", if with_like { "1" } else { "0" }.to_string());
        form.insert("csrf", csrf);
        archive_risk_fields(&mut form, bvid, &self.uid());
        self.post_form(
            "https://api.bilibili.com/x/web-interface/coin/add",
            form,
            "coin_video",
        )
        .await
    }

    /// /x/web-interface/archive/like/triple — like + coin + favorite in one shot.
    /// Server may partially succeed; the returned booleans report which actions
    /// went through.
    pub async fn triple_video(&self, bvid: &str) -> Result<TripleResult> {
        let csrf = self.session().cookies.csrf.clone();
        let url = "https://api.bilibili.com/x/web-interface/archive/like/triple";
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("csrf", csrf);
        archive_risk_fields(&mut form, bvid, &self.uid());
        let resp = self.http().post(url).form(&form).send().await?;
        let bytes = resp.bytes().await?;
        let v: ApiEnvelope<TripleRaw> = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "triple_video parse failed: body={}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        if v.code != 0 {
            tracing::warn!(
                url,
                bvid,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "triple_video non-zero code"
            );
            return Err(anyhow!(
                "triple_video failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let r = v.data.unwrap_or_default();
        Ok(TripleResult {
            like: r.like,
            coin: r.coin,
            fav: r.fav,
        })
    }

    /// /x/relation/modify — follow (act=1) or unfollow (act=2). The extra
    /// fields (re_src=14, gaia_source, spmid, extend_content) mirror what
    /// the web client posts from a video page; without them risk-control
    /// rejects the request on some accounts even though like/coin succeed.
    pub async fn relation_modify(&self, mid: i64, follow: bool) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("fid", mid.to_string());
        form.insert("act", if follow { "1" } else { "2" }.to_string());
        form.insert("re_src", "14".to_string());
        form.insert("gaia_source", "web_main".to_string());
        form.insert("spmid", "333.999.0.0".to_string());
        form.insert(
            "extend_content",
            format!(r#"{{"entity":"user","entity_id":{mid}}}"#),
        );
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/relation/modify",
            form,
            "relation_modify",
        )
        .await
    }

    async fn post_form(
        &self,
        url: &str,
        form: BTreeMap<&str, String>,
        op: &'static str,
    ) -> Result<()> {
        let resp = self.http().post(url).form(&form).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        let v: ApiEnvelope<Value> = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "{op} parse failed: status={status} body={}",
                String::from_utf8_lossy(&bytes)
            )
        })?;
        if v.code != 0 {
            tracing::warn!(
                op,
                url,
                status = %status,
                code = v.code,
                msg = %v.message,
                body = %String::from_utf8_lossy(&bytes),
                "post_form non-zero code"
            );
            return Err(anyhow!("{op} failed: code={} msg={}", v.code, v.message));
        }
        tracing::debug!(op, url, "post_form ok");
        Ok(())
    }

    /// /x/player/wbi/playurl — DASH manifest
    pub async fn play_url(&self, bvid: &str, cid: i64, qn: u32) -> Result<Value> {
        let t0 = Instant::now();
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("bvid".to_string(), bvid.to_string());
        params.insert("cid".to_string(), cid.to_string());
        params.insert("qn".to_string(), qn.to_string());
        params.insert("fnval".to_string(), "4048".to_string());
        params.insert("fnver".to_string(), "0".to_string());
        params.insert("fourk".to_string(), "1".to_string());
        // platform=pc (not html5): html5 caps frame rate at 30fps server-side,
        // so qn=116/120 etc. silently downgrade to 1080P30. Our segment proxy
        // sets Referer/Origin/Cookie, so referer-strict CDN nodes still work.
        params.insert("platform".to_string(), "pc".to_string());
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let v: ApiEnvelope<Value> = self
            .http()
            .get("https://api.bilibili.com/x/player/wbi/playurl")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("playurl failed: code={} msg={}", v.code, v.message));
        }
        let data = v.data.ok_or_else(|| anyhow!("playurl: no data"))?;
        tracing::info!(
            bvid = bvid,
            cid = cid,
            qn = qn,
            ms = t0.elapsed().as_millis() as u64,
            "play_url"
        );
        Ok(data)
    }

    /// /x/player/wbi/v2 — per-video player metadata. We only deserialize the
    /// "where did the user stop last time" fields (`last_play_time` is in ms,
    /// `last_play_cid` may differ from the requested cid for multi-part videos
    /// when the user previously played a different page).
    pub async fn player_v2(&self, bvid: &str, cid: i64) -> Result<PlayerV2Info> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("bvid".to_string(), bvid.to_string());
        params.insert("cid".to_string(), cid.to_string());
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let v: ApiEnvelope<PlayerV2Info> = self
            .http()
            .get("https://api.bilibili.com/x/player/wbi/v2")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "player_v2 failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        Ok(v.data.unwrap_or_default())
    }

    /// /x/space/wbi/acc/info + /x/relation/stat — public profile + follow counts.
    pub async fn space_info(&self, mid: i64) -> Result<SpaceInfoData> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("mid".to_string(), mid.to_string());
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let http = self.http();
        let f_info = http
            .get("https://api.bilibili.com/x/space/wbi/acc/info")
            .query(&params)
            .send();
        let f_stat = http
            .get("https://api.bilibili.com/x/relation/stat")
            .query(&[("vmid", mid.to_string())])
            .send();
        let (info_resp, stat_resp) = tokio::join!(f_info, f_stat);
        let info_v: ApiEnvelope<Value> = info_resp?.json().await?;
        if info_v.code != 0 {
            return Err(anyhow!(
                "space_info failed: code={} msg={}",
                info_v.code,
                info_v.message
            ));
        }
        let info = info_v.data.ok_or_else(|| anyhow!("space_info: no data"))?;
        let stat_v: ApiEnvelope<Value> = stat_resp?.json().await?;
        let (following, follower) = if stat_v.code == 0 {
            let d = stat_v.data.unwrap_or(Value::Null);
            (
                d.get("following").and_then(|v| v.as_i64()).unwrap_or(0),
                d.get("follower").and_then(|v| v.as_i64()).unwrap_or(0),
            )
        } else {
            (0, 0)
        };

        Ok(SpaceInfoData {
            mid: info.get("mid").and_then(|v| v.as_i64()).unwrap_or(mid),
            name: info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            face: info
                .get("face")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            sign: info
                .get("sign")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            level: info.get("level").and_then(|v| v.as_i64()).unwrap_or(0),
            top_photo: info
                .get("top_photo")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            following,
            follower,
        })
    }

    /// /x/click-interface/web/heartbeat — report playback progress so the
    /// server records this video in the user's watch history. Bilibili's web
    /// player fires this every 15s during playback (and once on start). Without
    /// it, videos played through this client never appear in /history.
    pub async fn report_heartbeat(
        &self,
        aid: i64,
        cid: i64,
        bvid: &str,
        played_time: i64,
        duration: i64,
        play_type: u8,
    ) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mid = self.session().cookies.uid.clone();
        let mut form = BTreeMap::new();
        form.insert("aid", aid.to_string());
        form.insert("cid", cid.to_string());
        form.insert("bvid", bvid.to_string());
        form.insert("played_time", played_time.to_string());
        form.insert("realtime", played_time.to_string());
        form.insert("video_duration", duration.to_string());
        form.insert("type", "3".to_string());
        form.insert("sub_type", "0".to_string());
        form.insert("dt", "2".to_string());
        form.insert("play_type", play_type.to_string());
        if !mid.is_empty() {
            form.insert("mid", mid);
        }
        if !csrf.is_empty() {
            form.insert("csrf", csrf);
        }
        // Heartbeat returns code=0 even for guests; only fail on transport errors.
        let v: ApiEnvelope<Value> = self
            .http()
            .post("https://api.bilibili.com/x/click-interface/web/heartbeat")
            .form(&form)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "heartbeat failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        Ok(())
    }

    /// /x/web-interface/history/cursor — paginated watch history.
    ///
    /// Cursor pagination: pass `max=0` and `view_at=0` for the first page; on
    /// subsequent pages echo `cursor.max` and `cursor.view_at` from the prior
    /// response. We pin `business="archive"` so every row opens in our
    /// /watch/:bvid view (live/bangumi/article rows would need different routes).
    pub async fn history_cursor(
        &self,
        max: i64,
        view_at: i64,
        business: &str,
        ps: u32,
    ) -> Result<HistoryRaw> {
        let v: ApiEnvelope<HistoryRaw> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/history/cursor")
            .query(&[
                ("max", max.to_string()),
                ("view_at", view_at.to_string()),
                ("business", business.to_string()),
                ("ps", ps.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "history_cursor failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        v.data.ok_or_else(|| anyhow!("history_cursor: no data"))
    }

    /// /x/v2/history/delete — remove one history entry. `kid` is the
    /// `"<business>_<id>"` form Bilibili expects (e.g. `"archive_540580868"`).
    pub async fn history_delete(&self, kid: &str) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("kid", kid.to_string());
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/delete",
            form,
            "history_delete",
        )
        .await
    }

    /// /x/v2/history/clear — wipe all history.
    pub async fn history_clear(&self) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/clear",
            form,
            "history_clear",
        )
        .await
    }

    /// /x/v2/history/toview — full Watch Later list (server caps at 100 items).
    pub async fn toview_list(&self) -> Result<ToviewRaw> {
        let v: ApiEnvelope<ToviewRaw> = self
            .http()
            .get("https://api.bilibili.com/x/v2/history/toview")
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "toview_list failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        // Empty list returns data: null on some accounts.
        Ok(v.data.unwrap_or_default())
    }

    /// /x/v2/history/toview/add — add by bvid.
    pub async fn toview_add(&self, bvid: &str) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/toview/add",
            form,
            "toview_add",
        )
        .await
    }

    /// /x/v2/history/toview/del — remove one entry by aid.
    pub async fn toview_del(&self, aid: i64) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("aid", aid.to_string());
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/toview/del",
            form,
            "toview_del",
        )
        .await
    }

    /// /x/v2/history/toview/del with `viewed=true` — purge already-watched.
    pub async fn toview_del_viewed(&self) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("viewed", "true".to_string());
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/toview/del",
            form,
            "toview_del_viewed",
        )
        .await
    }

    /// /x/v2/history/toview/clear — empty the Watch Later list.
    pub async fn toview_clear(&self) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("csrf", csrf);
        self.post_form(
            "https://api.bilibili.com/x/v2/history/toview/clear",
            form,
            "toview_clear",
        )
        .await
    }

    /// /x/space/wbi/arc/search — paginated list of a user's published videos.
    pub async fn space_videos(&self, mid: i64, pn: u32, ps: u32) -> Result<Value> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("mid".to_string(), mid.to_string());
        params.insert("pn".to_string(), pn.to_string());
        params.insert("ps".to_string(), ps.to_string());
        params.insert("order".to_string(), "pubdate".to_string());
        params.insert("platform".to_string(), "web".to_string());
        params.insert("web_location".to_string(), "1550101".to_string());
        let (w_rid, wts) = wbi::sign(&mut params, &keys);
        params.insert("w_rid".to_string(), w_rid);
        params.insert("wts".to_string(), wts);

        let v: ApiEnvelope<Value> = self
            .http()
            .get("https://api.bilibili.com/x/space/wbi/arc/search")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "space_videos failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        v.data.ok_or_else(|| anyhow!("space_videos: no data"))
    }
}

/// Bilibili's risk-control silently rejects bare `csrf+bvid+like` forms with
/// `code=-403 账号异常` for some accounts. The web client posts a fuller
/// payload that mirrors the spm/source/extend triplet from the video page.
/// We add the same set to like/coin/triple, mirroring what `relation_modify`
/// does for the follow endpoint (CLAUDE.md: "assume new write endpoints will
/// not [escape risk control]").
///
/// `entity_id` for archive operations is the bvid as a quoted string —
/// matches what bilibili.com's web client posts.
///
/// Note on per-user independence: only `ramval` is varied here. The other
/// fields (`spmid`, `source`, `gaia_source`, …) are page-level constants in
/// the real web client — every Chrome instance sends the same string. Faking
/// them per-user would be a fingerprintable deviation from real browsers and
/// invites more risk-control attention, not less. The genuine per-user
/// signal lives in cookies (SESSDATA / bili_jct / DedeUserID / buvid3),
/// which are already isolated per-`cookies.json`.
fn archive_risk_fields(form: &mut BTreeMap<&'static str, String>, bvid: &str, uid: &str) {
    form.insert("eab_x", "2".to_string());
    form.insert("ramval", ramval_for(uid).to_string());
    form.insert("source", "web_normal".to_string());
    form.insert("ga", "1".to_string());
    form.insert("gaia_source", "web_normal".to_string());
    form.insert("spmid", "333.788.0.0".to_string());
    form.insert(
        "extend_content",
        format!(r#"{{"entity":"archive","entity_id":"{bvid}"}}"#),
    );
}

/// Per-user, per-request `ramval` seed. Real web traffic shows a 6–7 digit
/// integer that's stable per page-load and increments slightly between
/// actions. We approximate by hashing the user's `DedeUserID` (so two users
/// on the same machine have disjoint sequences) and adding a process-wide
/// monotonic counter mixed with startup nanos (so restarts don't collide).
fn ramval_for(uid: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static START_NANOS: once_cell::sync::Lazy<u64> = once_cell::sync::Lazy::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
    });

    let mut h = DefaultHasher::new();
    uid.hash(&mut h);
    let user_seed = h.finish();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = user_seed
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(*START_NANOS)
        .wrapping_add(n);
    mixed % 9_000_000 + 1_000_000
}

fn make_domain_cookie(name: &str, value: &str) -> RawCookie {
    RawCookie {
        name: name.to_string(),
        value: value.to_string(),
        domain: Some(".bilibili.com".to_string()),
        path: Some("/".to_string()),
        expires: None,
        http_only: Some(false),
        secure: Some(true),
        same_site: None,
    }
}

/// Web client `_uuid` shape: 5 uppercase-hex blocks `8-4-4-4-12` followed by
/// a zero-padded 5-digit ms-of-second and the literal `infoc`. Risk-control
/// only checks the shape, not the bytes — but any deviation from this layout
/// is itself a signal, so match it exactly.
fn synth_uuid_cookie() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut h = DefaultHasher::new();
    now.as_nanos().hash(&mut h);
    n.hash(&mut h);
    let a = h.finish();
    let mut h2 = DefaultHasher::new();
    a.hash(&mut h2);
    (a ^ 0x9E3779B97F4A7C15).hash(&mut h2);
    let b = h2.finish();
    let hex32 = format!("{:016X}{:016X}", a, b);
    let ms5 = (now.subsec_millis() as u64 * 100 + (n % 100)) % 100_000;
    format!(
        "{}-{}-{}-{}-{}{:05}infoc",
        &hex32[0..8],
        &hex32[8..12],
        &hex32[12..16],
        &hex32[16..20],
        &hex32[20..32],
        ms5
    )
}

#[derive(Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

#[derive(Deserialize)]
pub struct NavData {
    #[serde(rename = "isLogin")]
    pub is_login: bool,
    #[serde(default)]
    pub mid: i64,
    #[serde(default)]
    pub uname: String,
    #[serde(default)]
    pub face: String,
    pub wbi_img: WbiImg,
}

#[derive(Deserialize)]
pub struct WbiImg {
    pub img_url: String,
    pub sub_url: String,
}

pub struct NavResponse {
    pub data: NavData,
}

#[derive(Deserialize)]
struct RcmdData {
    #[serde(default)]
    item: Vec<Value>,
}

#[derive(Deserialize, Debug)]
pub struct ViewData {
    pub bvid: String,
    pub aid: i64,
    pub cid: i64,
    pub title: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub owner: ViewOwner,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub pubdate: i64,
    #[serde(default)]
    pub tname: String,
    #[serde(default)]
    pub tid: i64,
    #[serde(default)]
    pub stat: ViewStat,
}

#[derive(Deserialize, Debug, Default)]
pub struct ViewOwner {
    #[serde(default)]
    pub mid: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub face: String,
}

#[derive(Deserialize, Default)]
struct ArchiveRelationRaw {
    #[serde(default)]
    like: bool,
    #[serde(default)]
    coin: i64,
    #[serde(default)]
    favorite: bool,
}

pub struct ArchiveRelation {
    pub liked: bool,
    pub coined: i64,
    pub favorited: bool,
}

#[derive(Deserialize, Default)]
struct UserRelationRaw {
    #[serde(default)]
    attribute: i64,
}

#[derive(Deserialize, Default)]
struct FollowingsRaw {
    #[serde(default)]
    list: Vec<FollowingItem>,
    #[serde(default)]
    total: i64,
}

pub struct FollowingsPage {
    pub list: Vec<FollowingItem>,
    pub total: i64,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct FollowingItem {
    #[serde(default)]
    pub mid: i64,
    #[serde(default)]
    pub uname: String,
    #[serde(default)]
    pub face: String,
    #[serde(default)]
    pub sign: String,
    #[serde(default)]
    pub mtime: i64,
    #[serde(default)]
    pub attribute: i64,
    #[serde(default)]
    pub special: i64,
}

#[derive(Deserialize, Default)]
struct TripleRaw {
    #[serde(default)]
    like: bool,
    #[serde(default)]
    coin: bool,
    #[serde(default)]
    fav: bool,
}

pub struct TripleResult {
    pub like: bool,
    pub coin: bool,
    pub fav: bool,
}

#[derive(Deserialize, Debug, Default)]
pub struct ViewStat {
    #[serde(default)]
    pub view: i64,
    #[serde(default)]
    pub danmaku: i64,
    #[serde(default)]
    pub reply: i64,
    #[serde(default)]
    pub favorite: i64,
    #[serde(default)]
    pub coin: i64,
    #[serde(default)]
    pub like: i64,
    #[serde(default)]
    pub share: i64,
}

#[derive(Deserialize, Default)]
pub struct PlayerV2Info {
    /// Last play position in **milliseconds**. 0 if the user has never played
    /// this video (or has watched it to completion — Bilibili resets to 0 in
    /// that case).
    #[serde(default)]
    pub last_play_time: i64,
    /// `cid` of the page the user was last on. May differ from the requested
    /// cid for multi-part videos.
    #[serde(default)]
    pub last_play_cid: i64,
}

pub struct SpaceInfoData {
    pub mid: i64,
    pub name: String,
    pub face: String,
    pub sign: String,
    pub level: i64,
    pub top_photo: String,
    pub following: i64,
    pub follower: i64,
}

#[derive(Deserialize, Default)]
pub struct HistoryRaw {
    #[serde(default)]
    pub cursor: HistoryCursor,
    #[serde(default)]
    pub list: Vec<HistoryRawItem>,
}

#[derive(Deserialize, Default)]
pub struct HistoryCursor {
    #[serde(default)]
    pub max: i64,
    #[serde(default)]
    pub view_at: i64,
    #[serde(default)]
    pub business: String,
    #[serde(default)]
    pub ps: i64,
}

#[derive(Deserialize, Default)]
pub struct HistoryRawItem {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub author_mid: i64,
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub view_at: i64,
    #[serde(default)]
    pub progress: i64,
    #[serde(default)]
    pub history: HistoryInner,
}

#[derive(Deserialize, Default)]
pub struct HistoryInner {
    #[serde(default)]
    pub oid: i64,
    #[serde(default)]
    pub bvid: String,
    #[serde(default)]
    pub cid: i64,
    #[serde(default)]
    pub business: String,
}

#[derive(Deserialize, Default)]
pub struct ToviewRaw {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub list: Vec<ToviewRawItem>,
}

#[derive(Deserialize, Default)]
pub struct ToviewRawItem {
    #[serde(default)]
    pub aid: i64,
    #[serde(default)]
    pub bvid: String,
    #[serde(default)]
    pub cid: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub tname: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub add_at: i64,
    #[serde(default)]
    pub progress: i64,
    #[serde(default)]
    pub owner: ViewOwner,
    #[serde(default)]
    pub stat: ViewStat,
}
