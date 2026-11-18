//! 同步包：匯出、匯入的準備與寫入、丟棄預覽、把沒做完的合併做完、進度。
//!
//! 匯入的選檔與匯出的交出都經由手動選檔的傳輸（`transport::PickerTransport`）：匯出先寫到 `exports/`
//! 底下的工作資料夾，`put` 放進 `exports/`，畫面再用 `deliver_export` 交給「存到…」或分享；
//! 匯入用系統選檔畫面選一份，`fetch` 改名到 `picks/`，準備完就刪（內容已解到文獻庫的暫存裡）。
//! 選檔的暫存在啟動時清掉（`clear_pick_cache`），匯出的暫存只留一小時（`clean_exports`）。
//! 合併規則都在核心；這裡只轉接，並把核心以 `reason` 區分的錯誤轉成畫面認得的種類（`error::sync_error`）。

use crate::commands::add::{picks_dir, remove_pick};
use crate::commands::backup::pick_one;
use crate::commands::export::{exports_dir, exports_path};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::transport::PickerTransport;
use paperreader_core::error::LibError;
use paperreader_core::ids::{new_id, now_compact};
use paperreader_core::store::{
    ConflictChoice, SkippedFile, SyncImportReport, SyncMode, SyncPreview,
};
use paperreader_core::sync::transport::SyncTransport;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

/// 匯出與匯入準備的進度（已處理, 全部）。不放在文獻庫那把鎖後面：做的時候鎖被握著，畫面照樣讀得到。
#[derive(Default)]
pub struct SyncProgress {
    done: AtomicU64,
    total: AtomicU64,
}

impl SyncProgress {
    fn set(&self, done: u64, total: u64) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(done, Ordering::Relaxed);
    }
}

/// 這台裝置的名稱（同步包的 `device_label`，另一台的預覽裡顯示）：Android 取型號，電腦取電腦名稱。取不到是 `None`。
pub fn device_label() -> Option<String> {
    #[cfg(target_os = "android")]
    let raw = std::process::Command::new("getprop")
        .arg("ro.product.model")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());
    #[cfg(not(target_os = "android"))]
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok();
    raw.map(|s| s.trim().chars().take(60).collect::<String>())
        .filter(|s| !s.is_empty())
}

/// 放進檔名的部分：字母與數字以外的換成 `-`，連續的併成一個，頭尾不留，最多 40 個字。
fn file_part(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

/// 同步包的檔名：`PaperReader-sync-<年月日-時分>-<裝置名稱>.zip`。時間由畫面給（當地時間）；
/// 不是「數字與 -」就用世界協調時間。
fn package_name(stamp: &str, label: Option<&str>) -> String {
    let stamp = if !stamp.is_empty()
        && stamp.len() <= 20
        && stamp.chars().all(|c| c.is_ascii_digit() || c == '-')
    {
        stamp.to_string()
    } else {
        now_compact()
    };
    match label.map(file_part).filter(|p| !p.is_empty()) {
        Some(part) => format!("PaperReader-sync-{stamp}-{part}.zip"),
        None => format!("PaperReader-sync-{stamp}.zip"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncExportResult {
    /// 檔名（存到…時當預設檔名）。
    pub name: String,
    /// 暫存的同步包；交給 `deliver_export`。
    pub path: String,
    /// 實際的模式：要求只帶變更、但另一台還沒匯入過這台的同步包時是完整的。
    pub mode: SyncMode,
    pub listed: usize,
    pub included: usize,
    pub bytes: u64,
    pub skipped: Vec<SkippedFile>,
    pub changed_during_export: Vec<String>,
}

/// 匯出同步包：預設只帶變更，`full` 時完整。寫好放在暫存，畫面接著交給「存到…」或分享。
/// 匯出時握著文獻庫的鎖，其他操作等它做完（同備份檔）。
#[tauri::command]
pub async fn sync_export(
    app: AppHandle,
    state: State<'_, AppState>,
    progress: State<'_, Arc<SyncProgress>>,
    full: bool,
    stamp: String,
) -> ApiResult<SyncExportResult> {
    let exports = exports_dir(&app).await?;
    let p = progress.inner().clone();
    state
        .run("sync_export", move |s| {
            let label = device_label();
            let name = package_name(&stamp, label.as_deref());
            let work = exports.join(format!("work-{}", new_id()));
            std::fs::create_dir_all(&work)
                .map_err(|e| paperreader_core::error::io_error(&work, e))?;
            let out = work.join(&name);
            p.set(0, 0);
            let mode = if full {
                SyncMode::Full
            } else {
                SyncMode::Changes
            };
            let result = s
                .with_lib_sync(|l| {
                    l.export_sync_package(&out, mode, label.as_deref(), &mut |d, t| p.set(d, t))
                })
                .and_then(|report| {
                    PickerTransport::new(None, &exports)
                        .put(&out, &name)
                        .map(|_| report)
                        .map_err(ApiError::from)
                });
            let _ = std::fs::remove_dir_all(&work);
            let report = result?;
            Ok(SyncExportResult {
                path: exports.join(&name).display().to_string(),
                name,
                mode: report.mode,
                listed: report.listed,
                included: report.included,
                bytes: report.bytes,
                skipped: report.skipped,
                changed_during_export: report.changed_during_export,
            })
        })
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPrepared {
    pub cancelled: bool,
    /// 核心的預覽，欄位原樣（snake_case）。
    pub preview: Option<SyncPreview>,
}

/// 取回選好的那一份、改名到 `picks/` 底下（同一個暫存空間，不多複製一份）。
fn fetch_picked(picked: &Path, cache: &Path) -> Result<PathBuf, LibError> {
    let mut t = PickerTransport::new(Some(picked.to_path_buf()), &exports_path(cache));
    let refs = t.list()?;
    let pkg = refs.first().ok_or_else(|| LibError::NotFound {
        what: "picked package".into(),
    })?;
    let dest = picks_dir(cache).join(format!("sync-{}.zip", new_id()));
    t.fetch(pkg, &dest)?;
    Ok(dest)
}

/// 匯入同步包的準備：選檔 → 解到文獻庫的暫存、算出預覽。除了暫存與補記的想法記錄，不寫文獻庫。
#[tauri::command]
pub async fn sync_import_prepare(
    app: AppHandle,
    state: State<'_, AppState>,
    progress: State<'_, Arc<SyncProgress>>,
) -> ApiResult<SyncPrepared> {
    state.require_lib().await?;
    let info = app.paperreader().storage_info().await?;
    let Some(picked) = pick_one(&app).await? else {
        return Ok(SyncPrepared {
            cancelled: true,
            preview: None,
        });
    };
    let cache = PathBuf::from(&info.cache_dir);
    let p = progress.inner().clone();
    state
        .run("sync_import_prepare", move |s| {
            let fetched = fetch_picked(&picked, &cache);
            remove_pick(&picked, &picks_dir(&cache));
            let dest = fetched?;
            // 先看清單：選到備份檔或別的檔就直接說（不比對核心錯誤的英文）
            if let Err(e) = crate::error::expect_sync_package(&dest) {
                let _ = std::fs::remove_file(&dest);
                return Err(e);
            }
            p.set(0, 0);
            let preview =
                s.with_lib_sync(|l| l.prepare_sync_import(&dest, &mut |d, t| p.set(d, t)));
            let _ = std::fs::remove_file(&dest);
            Ok(SyncPrepared {
                cancelled: false,
                preview: Some(preview?),
            })
        })
        .await
}

/// 照預覽與每個衝突的選擇寫入（`choices`：`[["c1","Local"], …]`）。之後畫面要重讀目錄。
#[tauri::command]
pub async fn sync_import_apply(
    state: State<'_, AppState>,
    preview_id: String,
    choices: Vec<(String, ConflictChoice)>,
) -> ApiResult<SyncImportReport> {
    state
        .lib_sync("sync_import_apply", move |l| {
            l.apply_sync_import(&preview_id, &choices)
        })
        .await
}

/// 丟棄預覽（離開預覽頁）。已經不是目前這一份（寫過、丟過、重開過）就當成丟好了。
#[tauri::command]
pub async fn sync_import_discard(state: State<'_, AppState>, preview_id: String) -> ApiResult<()> {
    state
        .lib_sync("sync_import_discard", move |l| {
            match l.discard_sync_import(&preview_id) {
                Err(LibError::NotFound { .. }) => Ok(()),
                other => other,
            }
        })
        .await
}

/// 把上一次沒做完的合併做完（文獻庫因此唯讀時的「重試」）。沒有沒做完的是 `NotFound`。
#[tauri::command]
pub async fn sync_resume(state: State<'_, AppState>) -> ApiResult<SyncImportReport> {
    state
        .lib_sync("sync_resume", move |l| l.resume_sync_import())
        .await
}

/// 畫面已經說了「上一次沒做完的合併已經做完」：清掉那份回報，之後的狀況不再帶。
#[tauri::command]
pub async fn sync_resumed_seen(state: State<'_, AppState>) -> ApiResult<()> {
    state
        .lib("sync_resumed_seen", |l| {
            l.clear_resumed_sync_report();
            Ok(())
        })
        .await
}

/// 匯出或匯入準備的進度：`[已處理, 全部]`。
#[tauri::command]
pub fn sync_progress(progress: State<'_, Arc<SyncProgress>>) -> [u64; 2] {
    [
        progress.done.load(Ordering::Relaxed),
        progress.total.load(Ordering::Relaxed),
    ]
}

/// 這台裝置的代號（閱讀位置是不是別台記的，畫面要比對）。
#[tauri::command]
pub fn this_device(state: State<'_, AppState>) -> String {
    state.device_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_name_uses_the_given_stamp_and_a_safe_device_part() {
        assert_eq!(
            package_name("20261116-1030", Some("Pixel 7 Pro")),
            "PaperReader-sync-20261116-1030-Pixel-7-Pro.zip"
        );
        assert_eq!(
            package_name("20261116-1030", Some("a/b\\c:..")),
            "PaperReader-sync-20261116-1030-a-b-c.zip"
        );
        assert_eq!(
            package_name("20261116-1030", None),
            "PaperReader-sync-20261116-1030.zip"
        );
        assert_eq!(
            package_name("20261116-1030", Some("///")),
            "PaperReader-sync-20261116-1030.zip"
        );
        // 不合格的時間字串不進檔名
        let n = package_name("../x", Some("A"));
        assert!(n.starts_with("PaperReader-sync-") && !n.contains(".."));
    }

    #[test]
    fn fetch_picked_moves_the_file_into_picks() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        let picked = cache.join("picks/abc/My Sync.zip");
        std::fs::create_dir_all(picked.parent().unwrap()).unwrap();
        std::fs::write(&picked, b"zip").unwrap();
        let dest = fetch_picked(&picked, cache).unwrap();
        assert_eq!(dest.parent(), Some(cache.join("picks").as_path()));
        assert_eq!(std::fs::read(&dest).unwrap(), b"zip");
        assert!(!picked.exists());
    }
}
