use crate::cookies::Cookies;
use crate::wbi::{self, WbiKeys};
use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

#[derive(Clone)]
pub struct Bili {
    pub client: Client,
    pub cookies: Arc<Cookies>,
    wbi_cache: Arc<RwLock<Option<(WbiKeys, Instant)>>>,
}

impl Bili {
    pub fn new(cookies: Cookies) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(header::REFERER, HeaderValue::from_static("https://www.bilibili.com/"));
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://www.bilibili.com"));
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

        Ok(Self {
            client,
            cookies: Arc::new(cookies),
            wbi_cache: Arc::new(RwLock::new(None)),
        })
    }

    /// /x/web-interface/nav — user info + WBI keys
    pub async fn nav(&self) -> Result<NavResponse> {
        let v: ApiEnvelope<NavData> = self
            .client
            .get("https://api.bilibili.com/x/web-interface/nav")
            .send()
            .await?
            .json()
            .await?;
        if v.code != 0 {
            return Err(anyhow!("nav failed: code={} msg={}", v.code, v.message));
        }
        Ok(NavResponse { data: v.data.ok_or_else(|| anyhow!("nav: no data"))? })
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
            .client
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
            .client
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
            .client
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
            .client
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
            .client
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
            .client
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
            .client
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
            .client
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
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("like", if like { "1" } else { "2" }.to_string());
        form.insert("csrf", self.cookies.csrf.clone());
        self.post_form("https://api.bilibili.com/x/web-interface/archive/like", form, "like_video").await
    }

    /// /x/web-interface/coin/add — coin a video. multiply ∈ {1, 2}.
    pub async fn coin_video(&self, bvid: &str, multiply: u8, with_like: bool) -> Result<()> {
        let m = multiply.clamp(1, 2);
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("multiply", m.to_string());
        form.insert("select_like", if with_like { "1" } else { "0" }.to_string());
        form.insert("csrf", self.cookies.csrf.clone());
        self.post_form("https://api.bilibili.com/x/web-interface/coin/add", form, "coin_video").await
    }

    /// /x/web-interface/archive/like/triple — like + coin + favorite in one shot.
    /// Server may partially succeed; the returned booleans report which actions
    /// went through.
    pub async fn triple_video(&self, bvid: &str) -> Result<TripleResult> {
        let mut form = BTreeMap::new();
        form.insert("bvid", bvid.to_string());
        form.insert("csrf", self.cookies.csrf.clone());
        let v: ApiEnvelope<TripleRaw> = self
            .client
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

    /// /x/relation/modify — follow (act=1) or unfollow (act=2). re_src=11 mimics
    /// the web client's source tag.
    pub async fn relation_modify(&self, mid: i64, follow: bool) -> Result<()> {
        let mut form = BTreeMap::new();
        form.insert("fid", mid.to_string());
        form.insert("act", if follow { "1" } else { "2" }.to_string());
        form.insert("re_src", "11".to_string());
        form.insert("csrf", self.cookies.csrf.clone());
        self.post_form("https://api.bilibili.com/x/relation/modify", form, "relation_modify").await
    }

    async fn post_form(
        &self,
        url: &str,
        form: BTreeMap<&str, String>,
        op: &'static str,
    ) -> Result<()> {
        let v: ApiEnvelope<Value> = self
            .client
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
            .client
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
