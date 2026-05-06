use crate::api::Session;
use crate::cookies::RawCookie;
use anyhow::{anyhow, Result};
use reqwest::header::SET_COOKIE;
use serde::Deserialize;
use serde_json::Value;

const QR_GENERATE: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const QR_POLL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";

pub struct QrStart {
    pub url: String,
    pub qrcode_key: String,
}

pub enum QrStatus {
    Scanning,
    Scanned,
    Confirmed { cookies: Vec<RawCookie> },
    Expired,
}

pub async fn qr_generate() -> Result<QrStart> {
    let client = Session::anonymous()?;
    let v: Envelope = client.get(QR_GENERATE).send().await?.json().await?;
    if v.code != 0 {
        return Err(anyhow!("qrcode/generate failed: {}", v.message));
    }
    let data = v.data.ok_or_else(|| anyhow!("qrcode/generate: no data"))?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("qrcode/generate: no url"))?
        .to_string();
    let qrcode_key = data
        .get("qrcode_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("qrcode/generate: no qrcode_key"))?
        .to_string();
    Ok(QrStart { url, qrcode_key })
}

pub async fn qr_poll(qrcode_key: &str) -> Result<QrStatus> {
    let client = Session::anonymous()?;
    let resp = client
        .get(QR_POLL)
        .query(&[("qrcode_key", qrcode_key)])
        .send()
        .await?;

    // Cookies arrive as Set-Cookie headers on the success response — collect
    // them BEFORE consuming the body (the headers go away once we move into
    // .json()).
    let set_cookies: Vec<String> = resp
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();

    let v: Envelope = resp.json().await?;
    if v.code != 0 {
        return Err(anyhow!("qrcode/poll failed: {}", v.message));
    }
    let data = v.data.ok_or_else(|| anyhow!("qrcode/poll: no data"))?;
    let inner_code = data
        .get("code")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("qrcode/poll: no inner code"))?;
    Ok(match inner_code {
        86101 => QrStatus::Scanning,
        86090 => QrStatus::Scanned,
        86038 => QrStatus::Expired,
        0 => {
            let cookies = parse_set_cookies(&set_cookies);
            if cookies.iter().all(|c| c.name != "SESSDATA") {
                return Err(anyhow!(
                    "qrcode/poll succeeded but no SESSDATA in Set-Cookie"
                ));
            }
            QrStatus::Confirmed { cookies }
        }
        other => return Err(anyhow!("qrcode/poll unknown inner code {other}")),
    })
}

/// Parse a list of Set-Cookie header values into the on-disk RawCookie shape.
/// Only the name/value pair is required — domain/path/etc. are best-effort.
fn parse_set_cookies(headers: &[String]) -> Vec<RawCookie> {
    headers
        .iter()
        .filter_map(|h| {
            let mut parts = h.split(';');
            let nv = parts.next()?;
            let (name, value) = nv.split_once('=')?;
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let mut domain = None;
            let mut path = None;
            for attr in parts {
                let attr = attr.trim();
                if let Some(rest) = attr.strip_prefix("Domain=") {
                    domain = Some(rest.to_string());
                } else if let Some(rest) = attr.strip_prefix("domain=") {
                    domain = Some(rest.to_string());
                } else if let Some(rest) = attr.strip_prefix("Path=") {
                    path = Some(rest.to_string());
                } else if let Some(rest) = attr.strip_prefix("path=") {
                    path = Some(rest.to_string());
                }
            }
            Some(RawCookie {
                name,
                value,
                domain,
                path,
                expires: None,
                http_only: None,
                secure: None,
                same_site: None,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct Envelope {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<Value>,
}
