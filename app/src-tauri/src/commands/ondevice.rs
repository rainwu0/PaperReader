//! 離線翻譯：兩個引擎的狀態、下載與刪除語言模型、畫面一側引擎（Bergamot）的橋與模型檔。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::ondevice::{
    self, BERGAMOT_PAIRS, Bridge, BridgeRequest, DownloadProgress, Downloads, OLD_ENGINE_FILE,
};
use crate::state::AppState;
use crate::translate::api_kind;
use paperreader_translate::{Cancel, Provider, download::fetch_verified};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_paperreader::PaperReaderExt;

pub(crate) fn data_dir(app: &AppHandle) -> ApiResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|_| ApiError::new(ErrorKind::Io, "app data dir"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BergamotModel {
    /// 語言組合，例如 `en-zh-Hant`。
    pub id: String,
    pub target: String,
    pub downloaded: bool,
    /// 已下載的大小（裝置上；沒下載是 0）。
    pub bytes: u64,
    /// 要下載的大小（壓縮後）。
    pub download_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDeviceStatus {
    /// 這次要譯成的語言（設定裡的目標語言）。
    pub target_lang: String,
    pub bergamot: Vec<BergamotModel>,
    /// Bergamot 有沒有目標語言的模型（`None`：沒有這個語言組合）。
    pub bergamot_ready: Option<bool>,
    /// 模型檔的來源主機。
    pub bergamot_host: String,
    pub download: DownloadProgress,
    /// 現在是不是不計流量的網路（查不到時 `None`）。
    pub unmetered: Option<bool>,
}

#[tauri::command]
pub async fn ondevice_status(
    app: AppHandle,
    state: State<'_, AppState>,
    dl: State<'_, Arc<Downloads>>,
) -> ApiResult<OnDeviceStatus> {
    let t = state.translate_settings();
    let data = data_dir(&app)?;
    let dir = ondevice::bergamot_dir(&data);
    let bergamot = BERGAMOT_PAIRS
        .iter()
        .map(|p| {
            let downloaded = ondevice::bergamot_ready(&data, p);
            BergamotModel {
                id: p.id.to_string(),
                target: p.target.to_string(),
                downloaded,
                bytes: if downloaded {
                    ondevice::disk_size(&dir.join(p.id))
                } else {
                    0
                },
                download_bytes: ondevice::bergamot_download_size(p),
            }
        })
        .collect();
    let pair = ondevice::bergamot_pair(Some("en"), &t.target_lang);
    let unmetered = app
        .paperreader()
        .network_info()
        .await
        .ok()
        .map(|n| n.unmetered);
    Ok(OnDeviceStatus {
        target_lang: t.target_lang.clone(),
        bergamot,
        bergamot_ready: pair.map(|p| ondevice::bergamot_ready(&data, p)),
        bergamot_host: ondevice::bergamot_host().to_string(),
        download: DownloadProgress {
            running: dl.running.load(Ordering::Relaxed),
            done: dl.done.load(Ordering::Relaxed),
            total: dl.total.load(Ordering::Relaxed),
        },
        unmetered,
    })
}

/// 下載目標語言要用的模型。`wifi_only` 為真而現在不是 Wi-Fi 時不下載（`NotOnWifi`）。
/// 下載完才交回；進度用 `ondevice_status` 的 `download` 看。
#[tauri::command]
pub async fn ondevice_download(
    app: AppHandle,
    state: State<'_, AppState>,
    dl: State<'_, Arc<Downloads>>,
    engine: Provider,
    wifi_only: bool,
) -> ApiResult<()> {
    let t = state.translate_settings();
    if wifi_only
        && let Ok(n) = app.paperreader().network_info().await
        && !n.unmetered
    {
        return Err(ApiError::new(ErrorKind::NotOnWifi, "not on wifi"));
    }
    match engine {
        Provider::Bergamot => {
            let pair = ondevice::bergamot_pair(Some("en"), &t.target_lang).ok_or_else(|| {
                ApiError::new(ErrorKind::TranslateLanguage, "unsupported language")
            })?;
            if dl.running.swap(true, Ordering::SeqCst) {
                return Err(ApiError::new(ErrorKind::TranslateBusy, "downloading"));
            }
            let cancel = Cancel::new();
            *dl.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
            let r = download_bergamot(&data_dir(&app)?, pair, &dl, &cancel).await;
            dl.running.store(false, Ordering::SeqCst);
            r.map_err(|e| e.log("bergamot_download"))
        }
        _ => Err(ApiError::new(ErrorKind::InvalidInput, "engine")),
    }
}

async fn download_bergamot(
    data: &std::path::Path,
    pair: &ondevice::BergamotPair,
    dl: &Downloads,
    cancel: &Cancel,
) -> ApiResult<()> {
    let dir = ondevice::bergamot_dir(data);
    let mut jobs: Vec<(&ondevice::BergamotFile, PathBuf)> = Vec::new();
    for f in &pair.files {
        let p = dir.join(pair.id).join(f.name);
        if !p.is_file() {
            jobs.push((f, p));
        }
    }
    dl.done.store(0, Ordering::Relaxed);
    dl.total.store(
        jobs.iter().map(|(f, _)| f.remote.size).sum(),
        Ordering::Relaxed,
    );
    for (f, path) in jobs {
        let bytes = fetch_verified(&f.remote, &dl.done, cancel)
            .await
            .map_err(|e| ApiError::new(api_kind(e.kind), "download"))?;
        // 先寫暫存檔再改名：寫到一半被中斷不會留下看起來完整的檔
        let parent = path.parent().unwrap_or(&dir).to_path_buf();
        let tmp = parent.join(format!("{}.part", f.name));
        let target = path.clone();
        tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::create_dir_all(&parent)?;
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &target)
        })
        .await
        .map_err(|_| ApiError::new(ErrorKind::Io, "write"))?
        .map_err(|e| {
            if e.raw_os_error() == Some(28) {
                ApiError::new(ErrorKind::NoSpace, "no space")
            } else {
                ApiError::new(ErrorKind::Io, "write")
            }
        })?;
    }
    Ok(())
}

#[tauri::command]
pub async fn ondevice_download_cancel(dl: State<'_, Arc<Downloads>>) -> ApiResult<()> {
    if let Some(c) = dl.cancel.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        c.cancel();
    }
    Ok(())
}

/// 刪掉一個引擎已下載的模型：Bergamot 給語言組合（例如 `en-zh-Hant`）。
/// Bergamot 的最後一個語言組合刪掉時，舊版留下的引擎檔也一起刪。
#[tauri::command]
pub async fn ondevice_delete(app: AppHandle, engine: Provider, model: String) -> ApiResult<()> {
    match engine {
        Provider::Bergamot => {
            let pair = BERGAMOT_PAIRS
                .iter()
                .find(|p| p.id == model)
                .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "model"))?;
            let dir = ondevice::bergamot_dir(&data_dir(&app)?);
            let _ = std::fs::remove_dir_all(dir.join(pair.id));
            if BERGAMOT_PAIRS.iter().all(|p| !dir.join(p.id).exists()) {
                let _ = std::fs::remove_file(dir.join(OLD_ENGINE_FILE));
            }
            Ok(())
        }
        _ => Err(ApiError::new(ErrorKind::InvalidInput, "engine")),
    }
}

/// 畫面一側的引擎來拿下一批要翻的字；最長等 20 秒，沒有就交回 `null`（畫面接著再問）。
#[tauri::command]
pub async fn ondevice_next(bridge: State<'_, Arc<Bridge>>) -> ApiResult<Option<BridgeRequest>> {
    Ok(bridge.next(Duration::from_secs(20)).await)
}

/// 畫面交回一批的譯文，或錯誤代碼（`no_model`、`failed`）。
#[tauri::command]
pub async fn ondevice_reply(
    bridge: State<'_, Arc<Bridge>>,
    id: u64,
    texts: Option<Vec<String>>,
    error: Option<String>,
) -> ApiResult<()> {
    let r = match (texts, error) {
        (Some(t), None) => Ok(t),
        (_, e) => Err(e.unwrap_or_else(|| "failed".into())),
    };
    bridge.reply(id, r);
    Ok(())
}

/// Bergamot 某個語言組合的模型檔，交給畫面載入（引擎跟著安裝檔走，畫面自己讀）。
#[tauri::command]
pub async fn bergamot_file(app: AppHandle, pair: String, name: String) -> ApiResult<Response> {
    let dir = ondevice::bergamot_dir(&data_dir(&app)?);
    let p = BERGAMOT_PAIRS
        .iter()
        .find(|p| p.id == pair)
        .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "pair"))?;
    let f = p
        .files
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "name"))?;
    let path = dir.join(p.id).join(f.name);
    let bytes = tauri::async_runtime::spawn_blocking(move || std::fs::read(path))
        .await
        .map_err(|_| ApiError::new(ErrorKind::Io, "read"))?
        .map_err(|_| ApiError::new(ErrorKind::TranslateNoModel, "model file missing"))?;
    Ok(Response::new(bytes))
}
