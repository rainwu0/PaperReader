//! 封面縮圖：由畫面用 PDF.js 畫原文第一頁，存在應用的快取資料夾（不放進文獻庫）。
//! 它可以隨時重畫，不屬於使用者的資料；快取被清掉只是下次重畫。

use crate::error::{ApiError, ApiResult, ErrorKind};
use paperreader_core::ids::is_valid_id;
use std::path::PathBuf;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager};

/// 縮圖最大的位元組數（一張小圖不會超過這個）。
const MAX_THUMB: usize = 2 * 1024 * 1024;

fn thumb_path(app: &AppHandle, paper_id: &str) -> ApiResult<PathBuf> {
    if !is_valid_id(paper_id) {
        return Err(ApiError::new(ErrorKind::InvalidInput, "bad paper id"));
    }
    let base = app
        .path()
        .app_cache_dir()
        .map_err(|_| ApiError::new(ErrorKind::Io, "no cache dir"))?;
    Ok(base.join("thumbs").join(format!("{paper_id}.jpg")))
}

/// 讀一篇的封面縮圖；還沒有就交回空的資料。
#[tauri::command]
pub async fn thumb_get(app: AppHandle, paper_id: String) -> ApiResult<Response> {
    let path = thumb_path(&app, &paper_id)?;
    let bytes =
        tauri::async_runtime::spawn_blocking(move || std::fs::read(path).unwrap_or_default())
            .await
            .unwrap_or_default();
    Ok(Response::new(bytes))
}

/// 存一篇的封面縮圖。資料在請求本文，文獻代號在標頭 `paper-id`。
#[tauri::command]
pub async fn thumb_put(app: AppHandle, request: Request<'_>) -> ApiResult<()> {
    let id = request
        .headers()
        .get("paper-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    // 位元組走原始請求本文；某些路徑（或舊的 WebView）會改成 JSON 的數字陣列，兩種都收
    let bytes: Vec<u8> = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        InvokeBody::Json(v) => serde_json::from_value(v.clone())
            .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "no body"))?,
    };
    if bytes.is_empty() || bytes.len() > MAX_THUMB {
        return Err(ApiError::new(ErrorKind::InvalidInput, "bad size"));
    }
    let path = thumb_path(&app, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)
    })
    .await
    .map_err(|_| ApiError::new(ErrorKind::Io, "background task failed"))?
    .map_err(|_| ApiError::new(ErrorKind::Io, "could not write the thumbnail"))
}
