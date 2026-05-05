use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct RawCookie {
    pub name: String,
    pub value: String,
}

pub struct Cookies {
    pub header: String,
    pub csrf: String,
    pub uid: String,
}

impl Cookies {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read cookies file {}", path.as_ref().display()))?;
        let raw: Vec<RawCookie> = serde_json::from_str(&text).context("parse cookies.json")?;

        let header = raw
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        let find = |name: &str| raw.iter().find(|c| c.name == name).map(|c| c.value.clone());

        let csrf = find("bili_jct").ok_or_else(|| anyhow!("missing bili_jct cookie"))?;
        let uid = find("DedeUserID").unwrap_or_default();

        if find("SESSDATA").is_none() {
            return Err(anyhow!("missing SESSDATA cookie — re-run `node login.js`"));
        }

        Ok(Self { header, csrf, uid })
    }
}
