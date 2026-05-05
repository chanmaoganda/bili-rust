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

    /// Personalized homepage feed
    pub async fn rcmd(&self, fresh_idx: u32, ps: u32) -> Result<Vec<Value>> {
        let keys = self.wbi_keys().await?;
        let mut params = BTreeMap::new();
        params.insert("fresh_type".to_string(), "4".to_string());
        params.insert("version".to_string(), "1".to_string());
        params.insert("ps".to_string(), ps.to_string());
        params.insert("fresh_idx".to_string(), fresh_idx.to_string());
        params.insert("fresh_idx_1h".to_string(), fresh_idx.to_string());
        params.insert("brush".to_string(), fresh_idx.to_string());
        params.insert("homepage_ver".to_string(), "1".to_string());
        params.insert("feed_version".to_string(), "V8".to_string());
        params.insert("web_location".to_string(), "1430650".to_string());
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
