use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_raw(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

async fn invoke<R, A>(cmd: &str, args: A) -> Result<R, String>
where
    R: for<'de> Deserialize<'de>,
    A: Serialize,
{
    let args = to_value(&args).map_err(|e| format!("serialize args: {e}"))?;
    let v = invoke_raw(cmd, args)
        .await
        .map_err(|e| format!("invoke {cmd}: {:?}", e))?;
    from_value::<R>(v).map_err(|e| format!("deserialize {cmd} result: {e}"))
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct RcmdArgs<'a> {
    #[serde(rename = "freshIdx")]
    fresh_idx: u32,
    _marker: std::marker::PhantomData<&'a ()>,
}

#[derive(Serialize)]
struct BvidArgs<'a> {
    bvid: &'a str,
}

#[derive(Serialize)]
struct PlayArgs<'a> {
    bvid: &'a str,
    cid: Option<i64>,
    qn: Option<u32>,
}

#[derive(Serialize)]
struct CidArgs {
    cid: i64,
}

pub async fn get_user_info() -> Result<UserInfo, String> {
    invoke("get_user_info", NoArgs {}).await
}

pub async fn get_rcmd(fresh_idx: u32) -> Result<Vec<VideoCard>, String> {
    invoke(
        "get_rcmd",
        RcmdArgs { fresh_idx, _marker: std::marker::PhantomData },
    )
    .await
}

pub async fn get_related(bvid: &str) -> Result<Vec<VideoCard>, String> {
    invoke("get_related", BvidArgs { bvid }).await
}

pub async fn get_play_info(
    bvid: &str,
    cid: Option<i64>,
    qn: Option<u32>,
) -> Result<PlayInfo, String> {
    invoke("get_play_info", PlayArgs { bvid, cid, qn }).await
}

pub async fn get_danmaku(cid: i64) -> Result<Vec<Danmaku>, String> {
    invoke("get_danmaku", CidArgs { cid }).await
}
