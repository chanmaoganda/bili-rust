use md5::{Digest, Md5};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

#[derive(Clone, Debug)]
pub struct WbiKeys {
    pub img_key: String,
    pub sub_key: String,
}

impl WbiKeys {
    /// Extract the keys from the wbi_img URLs returned by /x/web-interface/nav.
    /// e.g. `https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png`
    /// => `7cd084941338484aae1ad9425b84077c`
    pub fn from_urls(img_url: &str, sub_url: &str) -> Self {
        Self {
            img_key: filename_stem(img_url),
            sub_key: filename_stem(sub_url),
        }
    }

    pub fn mixin_key(&self) -> String {
        let raw: Vec<char> = format!("{}{}", self.img_key, self.sub_key)
            .chars()
            .collect();
        MIXIN_KEY_ENC_TAB
            .iter()
            .take(32)
            .map(|&i| raw.get(i).copied().unwrap_or('0'))
            .collect()
    }
}

fn filename_stem(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    last.rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| last.to_string())
}

/// Sign params with WBI. Mutates `params` to add `wts` and returns the `w_rid`.
/// Caller appends both as query params.
pub fn sign(params: &mut BTreeMap<String, String>, keys: &WbiKeys) -> (String, String) {
    let wts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string();
    params.insert("wts".to_string(), wts.clone());

    // strip these characters from values (Bilibili web client behavior)
    let sanitize = |s: &str| -> String {
        s.chars()
            .filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*'))
            .collect()
    };

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(&sanitize(v))))
        .collect::<Vec<_>>()
        .join("&");

    let mut hasher = Md5::new();
    hasher.update(query.as_bytes());
    hasher.update(keys.mixin_key().as_bytes());
    let w_rid = hex::encode(hasher.finalize());

    (w_rid, wts)
}

/// encodeURIComponent-equivalent: percent-encode everything except
/// A-Z a-z 0-9 - _ . ~  (uppercase hex, space => %20)
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_mixin_key() {
        // Sanity check the table & filename extraction with the example
        // shipped in the bilibili-API-collect docs.
        let k = WbiKeys::from_urls(
            "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",
            "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png",
        );
        assert_eq!(k.img_key, "7cd084941338484aae1ad9425b84077c");
        assert_eq!(k.sub_key, "4932caff0ff746eab6f01bf08b70ac45");
        let mk = k.mixin_key();
        assert_eq!(mk.len(), 32);
        // documented expected mixin_key for these inputs
        assert_eq!(mk, "ea1db124af3c7062474693fa704f4ff8");
    }

    #[test]
    fn encode_basics() {
        assert_eq!(encode("hello world"), "hello%20world");
        assert_eq!(encode("a=b&c"), "a%3Db%26c");
        assert_eq!(encode("中"), "%E4%B8%AD");
    }
}
