use anyhow::{anyhow, Context, Result};
use flate2::read::DeflateDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use std::io::Read;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Danmaku {
    pub progress: f64,
    pub mode: u8,
    pub size: u16,
    pub color: u32,
    pub text: String,
}

/// Parse Bilibili's `list.so` response. Body is raw RFC1951 deflate.
pub fn parse_response(bytes: &[u8]) -> Result<Vec<Danmaku>> {
    // Some responses come back already-inflated (rare, but cheap to detect).
    // A `<` opener at byte 0 means we have plain XML; otherwise inflate first.
    if bytes.first() == Some(&b'<') {
        parse_xml(bytes)
    } else {
        let mut decoder = DeflateDecoder::new(bytes);
        let mut xml = Vec::with_capacity(bytes.len() * 4);
        decoder
            .read_to_end(&mut xml)
            .context("inflate danmaku xml")?;
        parse_xml(&xml)
    }
}

/// Parse pre-inflated XML. Public so unit tests can hit it directly.
pub fn parse_xml(xml: &[u8]) -> Result<Vec<Danmaku>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut out: Vec<Danmaku> = Vec::new();
    let mut current_p: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(anyhow!("xml parse: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"d" => {
                current_p = e
                    .try_get_attribute("p")?
                    .map(|a| String::from_utf8_lossy(&a.value).into_owned());
            }
            Ok(Event::Text(ref t)) if current_p.is_some() => {
                let text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                if let Some(dm) = build(&current_p.take().unwrap(), &text) {
                    out.push(dm);
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"d" => {
                current_p = None;
            }
            _ => {}
        }
        buf.clear();
    }

    out.sort_by(|a, b| {
        a.progress
            .partial_cmp(&b.progress)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn build(p: &str, raw_text: &str) -> Option<Danmaku> {
    let mut it = p.split(',');
    let progress: f64 = it.next()?.parse().ok()?;
    let mode: u8 = it.next()?.parse().ok()?;
    let size: u16 = it.next()?.parse().ok()?;
    let color: u32 = it.next()?.parse().ok()?;

    // v1 ignores reverse(6) and special(7-9) modes.
    if !(1..=5).contains(&mode) {
        return None;
    }

    let text: String = raw_text
        .chars()
        .filter(|&c| c == '\t' || !c.is_control())
        .collect();
    if text.is_empty() {
        return None;
    }

    Some(Danmaku {
        progress,
        mode,
        size,
        color,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_xml() {
        let xml = br#"<?xml version="1.0"?><i>
            <d p="3.5,1,25,16777215,1700000000,0,abcd,1,10">hello</d>
            <d p="1.0,5,25,16711680,1700000000,0,abcd,2,10">top text</d>
            <d p="2.0,7,25,16777215,1700000000,0,abcd,3,10">special drop</d>
            <d p="0.5,1,25,16777215,1700000000,0,abcd,4,10">first</d>
        </i>"#;
        let out = parse_xml(xml).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].text, "first");
        assert_eq!(out[0].progress, 0.5);
        assert_eq!(out[1].text, "top text");
        assert_eq!(out[1].mode, 5);
        assert_eq!(out[2].progress, 3.5);
    }

    #[test]
    fn strip_control_chars() {
        let xml = b"<i><d p=\"1,1,25,16777215,0,0,x,1,0\">a\x01b\tc</d></i>";
        let out = parse_xml(xml).unwrap();
        assert_eq!(out[0].text, "ab\tc");
    }

    #[test]
    fn empty_after_filter_dropped() {
        let xml = b"<i><d p=\"1,1,25,16777215,0,0,x,1,0\">\x01\x02</d></i>";
        let out = parse_xml(xml).unwrap();
        assert!(out.is_empty());
    }
}
