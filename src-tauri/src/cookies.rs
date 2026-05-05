use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawCookie {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "httpOnly")]
    pub http_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "sameSite")]
    pub same_site: Option<String>,
}

pub struct Cookies {
    pub raw: Vec<RawCookie>,
    pub header: String,
    pub csrf: String,
    pub uid: String,
}

impl Cookies {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read cookies file {}", path.as_ref().display()))?;
        let raw: Vec<RawCookie> = serde_json::from_str(&text).context("parse cookies.json")?;
        Self::from_raw(raw)
    }

    pub fn from_raw(raw: Vec<RawCookie>) -> Result<Self> {
        let header = raw
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        let find = |name: &str| raw.iter().find(|c| c.name == name).map(|c| c.value.clone());

        let csrf = find("bili_jct").ok_or_else(|| anyhow!("missing bili_jct cookie"))?;
        let uid = find("DedeUserID").unwrap_or_default();

        if find("SESSDATA").is_none() {
            return Err(anyhow!("missing SESSDATA cookie"));
        }

        Ok(Self {
            raw,
            header,
            csrf,
            uid,
        })
    }

    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.raw).context("serialize cookies")?;
        std::fs::write(path.as_ref(), json)
            .with_context(|| format!("write cookies to {}", path.as_ref().display()))?;
        Ok(())
    }

    /// Merge `extras` into the cookie set, only adding entries whose name is
    /// not already present — never overwrite a real value that came from a
    /// browser. Returns a fresh `Cookies` with `header`/`csrf`/`uid`
    /// re-derived from the merged set.
    pub fn with_extra(self, extras: Vec<RawCookie>) -> Result<Self> {
        let Cookies { mut raw, .. } = self;
        for c in extras {
            if !raw.iter().any(|existing| existing.name == c.name) {
                raw.push(c);
            }
        }
        Self::from_raw(raw)
    }
}
