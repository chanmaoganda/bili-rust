use crate::api::Bili;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::FutureExt;
use http::{header, Response, StatusCode};
use std::sync::Arc;
use tauri::{UriSchemeContext, UriSchemeResponder, Wry};

pub fn rewrite(url: &str) -> String {
    // bilistream://seg/<b64> — "seg" is the URI authority (acts as a host).
    // The handler reads the path, which is "/<b64>".
    let b = URL_SAFE_NO_PAD.encode(url.as_bytes());
    format!("bilistream://seg/{b}")
}

pub fn rewrite_image(url: &str) -> String {
    let b = URL_SAFE_NO_PAD.encode(url.as_bytes());
    format!("biliimg://img/{b}")
}

#[derive(Clone, Copy)]
enum Kind {
    Segment,
    Image,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Segment => "bilistream",
            Kind::Image => "biliimg",
        }
    }
}

/// Tauri URI scheme handler for stream segments. Invoked from the GTK main
/// thread, so we MUST (a) spawn via Tauri's runtime (no Tokio context on this
/// thread), and (b) never let a panic cross the C FFI boundary.
pub fn handle(
    bili: Arc<Bili>,
    ctx: UriSchemeContext<'_, Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    dispatch(Kind::Segment, bili, ctx, request, responder);
}

/// Tauri URI scheme handler for images proxied through the Bili HTTP client
/// (which carries the Referer/Origin/Cookie headers Bilibili's CDN requires).
pub fn handle_image(
    bili: Arc<Bili>,
    ctx: UriSchemeContext<'_, Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    dispatch(Kind::Image, bili, ctx, request, responder);
}

fn dispatch(
    kind: Kind,
    bili: Arc<Bili>,
    _ctx: UriSchemeContext<'_, Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    tauri::async_runtime::spawn(async move {
        let result = std::panic::AssertUnwindSafe(serve(kind, &bili, &request))
            .catch_unwind()
            .await;

        let resp = match result {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                tracing::error!("{} error: {e:#}", kind.label());
                error_response(StatusCode::BAD_GATEWAY, format!("upstream: {e:#}"))
            }
            Err(panic) => {
                let msg = panic_message(&panic);
                tracing::error!("{} panic: {msg}", kind.label());
                error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("panic: {msg}"))
            }
        };
        responder.respond(resp);
    });
}

fn error_response(status: StatusCode, body: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status.as_u16())
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body.into_bytes())
        .expect("static error response is always valid")
}

fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

async fn serve(
    kind: Kind,
    bili: &Bili,
    request: &http::Request<Vec<u8>>,
) -> anyhow::Result<Response<Vec<u8>>> {
    let t0 = std::time::Instant::now();
    let uri = request.uri();
    let encoded = uri.path().trim_start_matches('/');
    if encoded.is_empty() {
        anyhow::bail!("empty {} path", kind.label());
    }
    let real = String::from_utf8(URL_SAFE_NO_PAD.decode(encoded)?)?;
    let host = url_host(&real).unwrap_or("?");
    let range_str = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    let mut req = bili.http().get(&real);
    // Images are returned whole; only segments need byte-range passthrough.
    if matches!(kind, Kind::Segment) {
        if let Some(rng) = request.headers().get(header::RANGE) {
            req = req.header(header::RANGE, rng.clone());
        }
    }

    let upstream = req.send().await?;
    let status = upstream.status();
    let version = upstream.version();
    let t_headers = t0.elapsed();

    let mut builder = Response::builder().status(status.as_u16());
    for h in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::CACHE_CONTROL,
        header::ETAG,
        header::LAST_MODIFIED,
    ] {
        if let Some(v) = upstream.headers().get(&h) {
            builder = builder.header(h, v.clone());
        }
    }
    builder = builder.header("access-control-allow-origin", "*");

    let body = upstream.bytes().await?.to_vec();
    let t_total = t0.elapsed();
    let tag = match kind {
        Kind::Segment => "seg",
        Kind::Image => "img",
    };
    tracing::info!(
        host = host,
        range = range_str,
        status = status.as_u16(),
        proto = ?version,
        bytes = body.len(),
        headers_ms = t_headers.as_millis() as u64,
        total_ms = t_total.as_millis() as u64,
        rate_mbps = mbps(body.len(), t_total),
        kind = tag,
        "proxy",
    );

    Ok(builder.body(body)?)
}

fn url_host(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    let host = after_scheme.split(['/', ':', '?']).next()?;
    Some(host)
}

fn mbps(bytes: usize, dur: std::time::Duration) -> f64 {
    let secs = dur.as_secs_f64().max(0.001);
    (bytes as f64 * 8.0 / 1_000_000.0) / secs
}
