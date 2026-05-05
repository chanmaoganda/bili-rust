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
            cookies.write_to(&p).with_context(|| {
                format!("persist new cookies to {}", p.display())
            })?;
        }
        let session = Session::build(cookies)?;
        *self.session.write() = session;
        // Force a fresh WBI fetch — keys travel with the session.
        *self.wbi_cache.write() = None;
        Ok(())
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
            return Err(anyhow!("comments failed: code={} msg={}", v.code, v.message));
        }
        v.data.ok_or_else(|| anyhow!("comments: no data"))
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
        let v: ApiEnvelope<ArchiveRelationRaw> = self
            .http()
            .get("https://api.bilibili.com/x/web-interface/archive/relation")
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "archive_relation failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let r = v.data.unwrap_or_default();
        Ok(ArchiveRelation {
            liked: r.like != 0,
            coined: r.coin_number,
            favorited: r.favorite != 0,
        })
    }

    /// /x/relation — relation to a specific user. `attribute` ∈ {2, 6} ⇒ 已关注.
    pub async fn user_relation(&self, mid: i64) -> Result<bool> {
        let v: ApiEnvelope<UserRelationRaw> = self
            .http()
            .get("https://api.bilibili.com/x/relation")
            .query(&[("fid", mid.to_string())])
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!(
                "user_relation failed: code={} msg={}",
                v.code,
                v.message
            ));
        }
        let attr = v.data.map(|d| d.attribute).unwrap_or(0);
        Ok(attr == 2 || attr == 6)
    }

    /// /x/web-interface/archive/like — toggle like (1 = like, 2 = un-like).
    pub async fn like_video(&self, bvid: &str, like: bool) -> Result<()> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("like", if like { "1" } else { "2" }.to_string());
        form.insert("csrf", csrf);
        self.post_form("https://api.bilibili.com/x/web-interface/archive/like", form, "like_video").await
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
        self.post_form("https://api.bilibili.com/x/web-interface/coin/add", form, "coin_video").await
    }

    /// /x/web-interface/archive/like/triple — like + coin + favorite in one shot.
    /// Server may partially succeed; the returned booleans report which actions
    /// went through.
    pub async fn triple_video(&self, bvid: &str) -> Result<TripleResult> {
        let csrf = self.session().cookies.csrf.clone();
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("csrf", csrf);
        let v: ApiEnvelope<TripleRaw> = self
            .http()
            .post("https://api.bilibili.com/x/web-interface/archive/like/triple")
            .form(&form)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
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
        self.post_form("https://api.bilibili.com/x/relation/modify", form, "relation_modify").await
    }

    async fn post_form(
        &self,
        url: &str,
        form: BTreeMap<&str, String>,
        op: &'static str,
    ) -> Result<()> {
        let v: ApiEnvelope<Value> = self
            .http()
            .post(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("{op} failed: code={} msg={}", v.code, v.message));
        }
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
        let info = info_v
            .data
            .ok_or_else(|| anyhow!("space_info: no data"))?;
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
        v.data
            .ok_or_else(|| anyhow!("history_cursor: no data"))
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
    like: i64,
    #[serde(default, rename = "coin_number")]
    coin_number: i64,
    #[serde(default)]
    favorite: i64,
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
