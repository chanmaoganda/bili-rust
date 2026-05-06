//! Live integration smoke test — calls the real Bilibili API using cookies.json.
//! Run with: `cargo test --test smoke -- --ignored --nocapture`

use bili_rust_lib::api::{Bili, FollowingItem};
use bili_rust_lib::cookies::Cookies;

fn cookie_path() -> std::path::PathBuf {
    std::env::var("BILI_COOKIES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cookies.json")
        })
}

#[tokio::test]
#[ignore]
async fn nav_returns_logged_in() {
    let cookies = Cookies::load(cookie_path()).expect("load cookies");
    let bili = Bili::new(cookies).expect("client");
    let nav = bili.nav().await.expect("nav");
    println!(
        "isLogin={} uname={} mid={}",
        nav.data.is_login, nav.data.uname, nav.data.mid
    );
    assert!(
        nav.data.is_login,
        "cookies are stale — log in again via the app's /login route"
    );
}

#[tokio::test]
#[ignore]
async fn rcmd_returns_videos() {
    let cookies = Cookies::load(cookie_path()).expect("load cookies");
    let bili = Bili::new(cookies).expect("client");
    let items = bili.rcmd(0, 0, 12, "").await.expect("rcmd");
    println!("got {} rcmd items", items.len());
    let first = items.first().expect("at least one");
    println!(
        "first: bvid={:?} title={:?}",
        first.get("bvid").and_then(|v| v.as_str()),
        first.get("title").and_then(|v| v.as_str()),
    );
    assert!(items.len() > 0);
}

#[tokio::test]
#[ignore]
async fn related_for_known_video() {
    let cookies = Cookies::load(cookie_path()).expect("load cookies");
    let bili = Bili::new(cookies).expect("client");
    // Pick first video from rcmd, then ask for its related list.
    let items = bili.rcmd(0, 0, 12, "").await.expect("rcmd");
    let bvid = items
        .iter()
        .find_map(|i| i.get("bvid").and_then(|v| v.as_str()))
        .expect("rcmd item with bvid");
    let related = bili.related(bvid).await.expect("related");
    println!("{} related for {bvid}", related.len());
    assert!(related.len() > 0);
}

/// Probe `attribute` values returned by /x/relation for UPs the current user
/// already follows. Goal: confirm whether the bit-mask hypothesis is right —
/// i.e. specially-followed UPs come back as `attribute = 130` (bit7|bit1) and
/// the old `attr == 2 || attr == 6` check was missing them.
///
/// Prints every (mid, uname, attribute, bits) so we can read the raw values.
/// No assertions — this is purely diagnostic.
#[tokio::test]
#[ignore]
async fn attribute_for_followings() {
    let cookies = Cookies::load(cookie_path()).expect("load cookies");
    let bili = Bili::new(cookies).expect("client");
    let nav = bili.nav().await.expect("nav");
    assert!(nav.data.is_login, "cookies stale; log in via the app first");
    let uid = nav.data.mid;
    println!("self uid={uid} uname={}", nav.data.uname);

    // 1) First page of regular followings (most-recent first).
    let regular = bili.followings(uid, 1, 20).await.expect("followings");
    println!(
        "\n=== /x/relation/followings (first {} of {} entries) ===",
        regular.list.len(),
        regular.total
    );
    probe_attributes(&bili, &regular.list).await;

    // 2) "特别关注" tag (tag_id = -10). Most likely to surface attribute=130.
    let special = match bili.followings_tag(-10, 1, 20).await {
        Ok(v) => v,
        Err(e) => {
            println!("\n=== /x/relation/tag tagid=-10 unavailable: {e} ===");
            Vec::new()
        }
    };
    println!(
        "\n=== /x/relation/tag tagid=-10 (special, {} entries) ===",
        special.len()
    );
    probe_attributes(&bili, &special).await;
}

async fn probe_attributes(bili: &Bili, items: &[FollowingItem]) {
    for it in items.iter().take(10) {
        let live = bili.user_relation_raw(it.mid).await;
        match live {
            Ok(attr) => {
                println!(
                    "  mid={:<10} uname={:<24} list_attr={:<4} live_attr={:<4} bits={:08b} \
                     follows_them={} mutual={} special={}",
                    it.mid,
                    truncate(&it.uname, 24),
                    it.attribute,
                    attr,
                    attr,
                    attr & 2 != 0,
                    attr & 4 != 0,
                    attr & 128 != 0,
                );
            }
            Err(e) => println!("  mid={} uname={:?} ERROR: {e}", it.mid, it.uname),
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[tokio::test]
#[ignore]
async fn play_url_returns_dash() {
    let cookies = Cookies::load(cookie_path()).expect("load cookies");
    let bili = Bili::new(cookies).expect("client");
    let items = bili.rcmd(0, 0, 12, "").await.expect("rcmd");
    let item = items
        .iter()
        .find(|i| i.get("goto").and_then(|v| v.as_str()) == Some("av"))
        .expect("an av item");
    let bvid = item
        .get("bvid")
        .and_then(|v| v.as_str())
        .expect("bvid")
        .to_string();
    let view = bili.view(&bvid).await.expect("view");
    let raw = bili.play_url(&bvid, view.cid, 80).await.expect("playurl");
    let dash = raw.get("dash").expect("dash present");
    let v_count = dash
        .get("video")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let a_count = dash
        .get("audio")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    println!("bvid={bvid} video tracks={v_count} audio tracks={a_count}");
    let first_v = dash.pointer("/video/0").expect("video[0]");
    println!(
        "VIDEO[0] keys: {:?}",
        first_v.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    println!("VIDEO[0]: {first_v:#}");
    let first_a = dash.pointer("/audio/0").expect("audio[0]");
    println!(
        "AUDIO[0] keys: {:?}",
        first_a.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    println!("AUDIO[0]: {first_a:#}");
    assert!(v_count > 0);
    assert!(a_count > 0);
}
