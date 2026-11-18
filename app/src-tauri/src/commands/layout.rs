//! 版面模型（只給翻譯的分段用）：模型檔的狀態、下載（同意後才下載、核對雜湊）、刪除，
//! 與一頁的偵測。
//!
//! - 模型檔放在應用私有的空間 `<app_data>/layout/`，不進文獻庫、不進備份；下載的只有模型（資料），
//!   執行它的 ONNX Runtime 跟著安裝檔走。下載與核對和離線翻譯的模型同一套
//!   （`paperreader_translate::download`、`ondevice::Downloads`），只是這個檔一百多 MB，邊收邊寫進檔案。
//! - 偵測：畫面把一頁畫好、縮成 800×800 的 RGB，以 Base64 字串交來（Android 上原始位元組會被改成數字陣列，
//!   慢 5 倍以上）。這是外來的內容，先檢查長度與頁面大小再解碼。
//!   交回方框（PDF 點，左上為原點）。不寫文獻庫。
//! - ONNX Runtime 的環境整個程序一個（`layout_model.rs`）；模型第一次偵測時載入，之後留著用，刪除模型檔前才放掉。
//!   執行緒數由 ONNX Runtime 決定。

use crate::commands::ondevice::data_dir;
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::ondevice::{DownloadProgress, Downloads};
use crate::translate::api_kind;
use base64::Engine;
use paperreader_translate::download::{FetchFileError, RemoteFile};
use paperreader_translate::{Cancel, ErrorKind as TErr};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

/// PP-DocLayoutV3 的官方 ONNX 版（PaddlePaddle 在 Hugging Face 的頁面）。網址釘在一個版本上，
/// 大小與 SHA-256 是模型檔的。Hugging Face 會轉到自己的 CDN（`*.hf.co`）。
pub const MODEL: RemoteFile = RemoteFile {
    url: "https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_onnx/resolve/46bbdf188bb0a772c08aed74882ce7e51a8f1ea6/inference.onnx",
    size: 130_502_049,
    zstd: false,
    sha256: "45bf71750b00739a41fc209f132eb104a4d6b5bb29483c9078164d8b87cf28ba",
};

/// 給使用者看「會連到哪裡」。
pub const MODEL_HOST: &str = "huggingface.co";

/// 模型的輸入邊長與一張圖的位元組數（RGB）。和 `layout_model.rs` 的 `INPUT_SIZE`、`RGB_BYTES` 相同（測試核對）。
const SIDE: usize = 800;
const RGB_BYTES: usize = SIDE * SIDE * 3;
/// 那張圖的 Base64 長度（`RGB_BYTES` 是 3 的倍數，沒有補位）。
const B64_LEN: usize = RGB_BYTES / 3 * 4;
/// 頁面的寬高（PDF 點）最多這麼大（Acrobat 的上限是 14,400 點；留一點餘裕）。
const MAX_PAGE: f64 = 20_000.0;

pub fn model_path(data: &Path) -> PathBuf {
    data.join("layout").join("PP-DocLayoutV3.onnx")
}

/// 外殼裡的版面模型：載入好的模型（第一次偵測時載入）與下載的進度。
#[derive(Default)]
pub struct LayoutState {
    #[cfg(feature = "layout-model")]
    model: std::sync::Mutex<Option<paperreader_translate::layout_model::LayoutModel>>,
    download: Downloads,
}

impl LayoutState {
    /// 放掉載入好的模型（刪除模型檔之前）。
    fn unload(&self) {
        #[cfg(feature = "layout-model")]
        {
            *self.model.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutStatus {
    /// 這個建置有沒有版面模型（`layout-model` 功能）。沒有時畫面不顯示這一項。
    pub supported: bool,
    pub downloaded: bool,
    /// 已下載的大小（裝置上；沒下載是 0）。
    pub bytes: u64,
    /// 要下載的大小。
    pub download_bytes: u64,
    pub host: String,
    pub download: DownloadProgress,
    /// 現在是不是不計流量的網路（查不到時 `None`）。
    pub unmetered: Option<bool>,
}

#[tauri::command]
pub async fn layout_status(
    app: AppHandle,
    st: State<'_, Arc<LayoutState>>,
) -> ApiResult<LayoutStatus> {
    let path = model_path(&data_dir(&app)?);
    let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
    let unmetered = app
        .paperreader()
        .network_info()
        .await
        .ok()
        .map(|n| n.unmetered);
    let dl = &st.download;
    Ok(LayoutStatus {
        supported: cfg!(feature = "layout-model"),
        downloaded: bytes > 0,
        bytes,
        download_bytes: MODEL.size,
        host: MODEL_HOST.to_string(),
        download: DownloadProgress {
            running: dl.running.load(Ordering::Relaxed),
            done: dl.done.load(Ordering::Relaxed),
            total: dl.total.load(Ordering::Relaxed),
        },
        unmetered,
    })
}

/// 下載模型檔。`wifi_only` 為真而現在不是 Wi-Fi 時不下載（`NotOnWifi`）。下載完才交回；進度用 `layout_status` 看。
#[tauri::command]
pub async fn layout_download(
    app: AppHandle,
    st: State<'_, Arc<LayoutState>>,
    wifi_only: bool,
) -> ApiResult<()> {
    if !cfg!(feature = "layout-model") {
        return Err(ApiError::new(
            ErrorKind::Unsupported,
            "built without layout-model",
        ));
    }
    if wifi_only
        && let Ok(n) = app.paperreader().network_info().await
        && !n.unmetered
    {
        return Err(ApiError::new(ErrorKind::NotOnWifi, "not on wifi"));
    }
    let dest = model_path(&data_dir(&app)?);
    let dl = &st.download;
    if dl.running.swap(true, Ordering::SeqCst) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "downloading"));
    }
    let cancel = Cancel::new();
    *dl.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
    dl.done.store(0, Ordering::Relaxed);
    dl.total.store(MODEL.size, Ordering::Relaxed);
    let r = paperreader_translate::download::fetch_to_file(&MODEL, &dest, &dl.done, &cancel).await;
    dl.running.store(false, Ordering::SeqCst);
    r.map_err(|e| download_error(e).log("layout_download"))
}

fn download_error(e: FetchFileError) -> ApiError {
    match e {
        FetchFileError::Fetch(t) if t.kind == TErr::Cancelled => {
            ApiError::new(ErrorKind::TranslateBadResponse, "cancelled")
        }
        FetchFileError::Fetch(t) => ApiError::new(api_kind(t.kind), "download"),
        FetchFileError::Io(e) if e.raw_os_error() == Some(28) => {
            ApiError::new(ErrorKind::NoSpace, "no space")
        }
        FetchFileError::Io(_) => ApiError::new(ErrorKind::Io, "write"),
    }
}

#[tauri::command]
pub async fn layout_download_cancel(st: State<'_, Arc<LayoutState>>) -> ApiResult<()> {
    if let Some(c) = st
        .download
        .cancel
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
    {
        c.cancel();
    }
    Ok(())
}

/// 刪掉模型檔（與下載到一半的暫存檔）。先放掉載入好的模型。
#[tauri::command]
pub async fn layout_delete(app: AppHandle, st: State<'_, Arc<LayoutState>>) -> ApiResult<()> {
    let path = model_path(&data_dir(&app)?);
    st.unload();
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_file(dir.join("PP-DocLayoutV3.onnx.part"));
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ApiError::new(ErrorKind::Io, "delete").log("layout_delete")),
    }
}

/// 一個版面區塊（交給畫面的形狀，和 `src/translate/regions.ts` 的 `RegionBox` 相同）。
#[derive(Debug, Serialize)]
pub struct RegionBox {
    pub label: &'static str,
    pub score: f32,
    pub bbox: [f32; 4],
}

/// 檢查並解開畫面交來的圖：Base64 的長度要剛好是一張 800×800 的 RGB，解開後的位元組數也要對。
fn decode_image(image: &str) -> ApiResult<Vec<u8>> {
    if image.len() != B64_LEN {
        return Err(ApiError::new(ErrorKind::InvalidInput, "image size"));
    }
    let rgb = base64::engine::general_purpose::STANDARD
        .decode(image)
        .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "bad base64"))?;
    if rgb.len() != RGB_BYTES {
        return Err(ApiError::new(ErrorKind::InvalidInput, "image size"));
    }
    Ok(rgb)
}

/// 頁面的寬高（PDF 點）：有限、大於 0、不超過 `MAX_PAGE`。
fn check_page(width: f64, height: f64) -> ApiResult<(f32, f32)> {
    let ok = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_PAGE;
    if ok(width) && ok(height) {
        #[allow(clippy::cast_possible_truncation)]
        Ok((width as f32, height as f32))
    } else {
        Err(ApiError::new(ErrorKind::InvalidInput, "page size"))
    }
}

/// 跑一頁。`image` 是 800×800 的 RGB 以 Base64 編成的字串；`width`、`height` 是頁面的寬高（PDF 點），
/// 交回的方框用這個單位。模型沒下載交回 `TranslateNoModel`；這個建置沒有版面模型交回 `Unsupported`。
#[tauri::command]
pub async fn layout_detect(
    app: AppHandle,
    st: State<'_, Arc<LayoutState>>,
    image: String,
    width: f64,
    height: f64,
) -> ApiResult<Vec<RegionBox>> {
    let (w, h) = check_page(width, height)?;
    let rgb = decode_image(&image)?;
    drop(image);
    let path = model_path(&data_dir(&app)?);
    let st = Arc::clone(&st);
    tauri::async_runtime::spawn_blocking(move || detect(&st, &path, &rgb, w, h))
        .await
        .map_err(|_| ApiError::new(ErrorKind::Io, "background task failed"))?
}

#[cfg(feature = "layout-model")]
fn detect(st: &LayoutState, path: &Path, rgb: &[u8], w: f32, h: f32) -> ApiResult<Vec<RegionBox>> {
    use paperreader_translate::layout_model::{DEFAULT_THRESHOLD, LayoutError, LayoutModel};
    let mut guard = st.model.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        if !path.is_file() {
            return Err(ApiError::new(
                ErrorKind::TranslateNoModel,
                "no layout model",
            ));
        }
        let m = LayoutModel::load(path, 0)
            .map_err(|e| ApiError::new(ErrorKind::Io, e.to_string()).log("layout_detect"))?;
        *guard = Some(m);
    }
    let Some(model) = guard.as_mut() else {
        return Err(ApiError::new(ErrorKind::Io, "no model"));
    };
    let boxes = model
        .detect(rgb, w, h, DEFAULT_THRESHOLD)
        .map_err(|e| match e {
            LayoutError::BadInput(what) => ApiError::new(ErrorKind::InvalidInput, what),
            LayoutError::Runtime(s) => ApiError::new(ErrorKind::Io, s).log("layout_detect"),
        })?;
    Ok(boxes
        .into_iter()
        .map(|b| RegionBox {
            label: b.label,
            score: b.score,
            bbox: b.bbox,
        })
        .collect())
}

/// 沒開版面模型的建置：一律回「這個平台做不到」。
#[cfg(not(feature = "layout-model"))]
fn detect(
    _st: &LayoutState,
    _path: &Path,
    _rgb: &[u8],
    _w: f32,
    _h: f32,
) -> ApiResult<Vec<RegionBox>> {
    Err(ApiError::new(
        ErrorKind::Unsupported,
        "built without layout-model",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_must_be_exactly_one_rgb_page() {
        let one = base64::engine::general_purpose::STANDARD.encode(vec![7u8; RGB_BYTES]);
        assert_eq!(one.len(), B64_LEN);
        assert_eq!(decode_image(&one).unwrap().len(), RGB_BYTES);
        // RGBA（四個通道）、少一個點、空的、長度對但不是 Base64：都不收
        let rgba = base64::engine::general_purpose::STANDARD.encode(vec![7u8; SIDE * SIDE * 4]);
        assert_eq!(
            decode_image(&rgba).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        let short = base64::engine::general_purpose::STANDARD.encode(vec![7u8; RGB_BYTES - 3]);
        assert_eq!(
            decode_image(&short).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(decode_image("").unwrap_err().kind, ErrorKind::InvalidInput);
        let junk = "!".repeat(B64_LEN);
        assert_eq!(
            decode_image(&junk).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn page_size_is_bounded() {
        assert_eq!(check_page(612.0, 792.0).unwrap(), (612.0, 792.0));
        for (w, h) in [
            (0.0, 792.0),
            (-1.0, 792.0),
            (612.0, f64::NAN),
            (f64::INFINITY, 792.0),
            (612.0, 1e9),
        ] {
            assert_eq!(check_page(w, h).unwrap_err().kind, ErrorKind::InvalidInput);
        }
    }

    #[cfg(feature = "layout-model")]
    #[test]
    fn sizes_match_the_engine() {
        use paperreader_translate::layout_model as lm;
        assert_eq!(SIDE, lm::INPUT_SIZE);
        assert_eq!(RGB_BYTES, lm::RGB_BYTES);
    }

    #[test]
    fn model_is_pinned_and_checked() {
        assert!(
            MODEL
                .url
                .starts_with("https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_onnx/resolve/")
        );
        assert!(!MODEL.url.contains("/main/"));
        assert_eq!(MODEL.sha256.len(), 64);
        const { assert!(!MODEL.zstd) };
        assert!(
            model_path(Path::new("data"))
                .ends_with(Path::new("layout").join("PP-DocLayoutV3.onnx"))
        );
    }

    #[test]
    fn detect_without_a_model_file_says_so() {
        let st = LayoutState::default();
        let rgb = vec![255u8; RGB_BYTES];
        let e = detect(
            &st,
            Path::new("does-not-exist/PP-DocLayoutV3.onnx"),
            &rgb,
            612.0,
            792.0,
        )
        .unwrap_err();
        if cfg!(feature = "layout-model") {
            assert_eq!(e.kind, ErrorKind::TranslateNoModel);
        } else {
            assert_eq!(e.kind, ErrorKind::Unsupported);
        }
    }
}
