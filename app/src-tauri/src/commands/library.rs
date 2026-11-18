//! 文獻庫本身：狀況、建立、搬到共用空間、重新掃描、檢查，以及權限頁。

use crate::commands::digest::DigestSlot;
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::location::{Storage, storage_after_permission_step};
use crate::state::{AppState, LibraryStatus, MoveOutcome, MovePlan, MoveProgress};
use crate::translate::{HasState, JobSlot, JobState, JobStatus};
use paperreader_core::error::io_error;
use paperreader_core::jsonio::write_bytes_atomic;
use paperreader_core::store::{CheckLevel, Problem, ScanDiff};
use std::path::Path;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

/// 文獻庫的狀況（還沒建立、已就緒、唯讀、失去權限；在共用空間還是私有空間；路徑）。
#[tauri::command]
pub async fn library_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<LibraryStatus> {
    let info = app.paperreader().storage_info().await?;
    state.run("library_status", move |s| s.status(&info)).await
}

/// 在指定的位置建立文獻庫。`storage` 傳 `null` 表示「剛從授權頁回來」：有權限放共用空間，
/// 沒有就放應用私有空間。
#[tauri::command]
pub async fn library_create(
    app: AppHandle,
    state: State<'_, AppState>,
    storage: Option<Storage>,
) -> ApiResult<LibraryStatus> {
    let info = app.paperreader().storage_info().await?;
    let chosen = storage.unwrap_or_else(|| storage_after_permission_step(info.all_files_access));
    state
        .run("library_create", move |s| s.create(chosen, &info))
        .await
}

/// 應用專屬空間的文獻庫是空的（沒有文獻、回收區也空）時，改放到共用空間。要先有「所有檔案存取」。
/// 共用空間已經有文獻庫時接回那一個（搬到共用空間的估計交回 `adopt` 時，畫面改走這裡）。
#[tauri::command]
pub async fn library_switch_to_shared(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<LibraryStatus> {
    let info = app.paperreader().storage_info().await?;
    state
        .run("library_switch_to_shared", move |s| {
            s.switch_to_shared(&info)
        })
        .await
}

/// 從 `df -k` 的輸出讀出可用的位元組：第二列的第四欄（Available，單位 1024 位元組）。
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn parse_df(out: &str) -> Option<u64> {
    let line = out.lines().nth(1)?;
    let kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    kb.checked_mul(1024)
}

/// 共用空間可用的空間（搬之前估）。Android 用系統的 `df`（toybox）；量不到或不是 Android 是 `None`，
/// 搬的時候照樣做，真的不夠時照失敗的規則退回。
fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(target_os = "android")]
    {
        let out = std::process::Command::new("df")
            .arg("-k")
            .arg(path)
            .output()
            .ok()?;
        parse_df(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = path;
        None
    }
}

/// 搬到共用空間之前的估計：要搬去哪裡、要多少空間；共用空間已經有文獻庫、這裡的是空的時交回 `adopt`。
#[tauri::command]
pub async fn library_move_plan(app: AppHandle, state: State<'_, AppState>) -> ApiResult<MovePlan> {
    let info = app.paperreader().storage_info().await?;
    state
        .run("library_move_plan", move |s| {
            s.move_plan(&info, &free_bytes)
        })
        .await
}

/// 把應用專屬空間的文獻庫搬到共用空間。翻譯或重點筆記正在跑時不開始（`TranslateBusy`）。
/// 做完才交回；畫面另外問進度（`library_move_progress`）、可以在核對之前取消（`library_move_cancel`）。
#[tauri::command]
pub async fn library_move_to_shared(
    app: AppHandle,
    state: State<'_, AppState>,
    translating: State<'_, JobSlot<JobStatus>>,
    digesting: State<'_, DigestSlot>,
) -> ApiResult<MoveOutcome> {
    let running = translating
        .status()
        .is_some_and(|s| s.state() == JobState::Running)
        || digesting
            .status()
            .is_some_and(|s| s.state() == JobState::Running);
    if running {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "a job is running"));
    }
    let info = app.paperreader().storage_info().await?;
    state
        .run("library_move_to_shared", move |s| {
            s.move_to_shared(&info, &free_bytes, &mut |_, _| {})
        })
        .await
}

/// 搬移的進度。不拿文獻庫的鎖（搬的時候鎖被握著）。
#[tauri::command]
pub fn library_move_progress(state: State<'_, AppState>) -> MoveProgress {
    state.move_progress()
}

/// 取消搬移：換位置之前才收，交回收了沒有。
#[tauri::command]
pub fn library_move_cancel(state: State<'_, AppState>) -> bool {
    state.cancel_move()
}

#[tauri::command]
pub async fn library_rescan(state: State<'_, AppState>) -> ApiResult<ScanDiff> {
    state.lib("library_rescan", move |l| l.rescan()).await
}

/// 檢查文獻庫。`full` 為真時連材料檔的指紋也重算。
#[tauri::command]
pub async fn library_check(state: State<'_, AppState>, full: bool) -> ApiResult<Vec<Problem>> {
    let level = if full {
        CheckLevel::Full
    } else {
        CheckLevel::Quick
    };
    state.lib("library_check", move |l| l.check(level)).await
}

/// 逐篇處理的紀錄一份最多多大（3000 篇、每篇兩件抽取，一件一兩百位元組，遠小於這個）。
const MAX_PER_PAPER_STATE: usize = 16 * 1024 * 1024;

/// 讀逐篇處理的紀錄：這個文獻庫還沒有、或讀不懂時交回 `null`（當成紀錄不見，照規則重做）。
/// 內容由畫面定，外殼不解讀。
#[tauri::command]
pub async fn per_paper_state_get(
    state: State<'_, AppState>,
) -> ApiResult<Option<serde_json::Value>> {
    state
        .run("per_paper_state_get", move |s| {
            let path = s.per_paper_path()?;
            match std::fs::read(&path) {
                Ok(bytes) => Ok(serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .filter(|v| v.is_object())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(io_error(&path, e).into()),
            }
        })
        .await
}

/// 存逐篇處理的紀錄（整份換掉，原子寫入）。要是一個 JSON 物件。
#[tauri::command]
pub async fn per_paper_state_put(
    state: State<'_, AppState>,
    record: serde_json::Value,
) -> ApiResult<()> {
    state
        .run("per_paper_state_put", move |s| {
            if !record.is_object() {
                return Err(ApiError::new(ErrorKind::InvalidInput, "not an object"));
            }
            let bytes = serde_json::to_vec(&record)
                .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "not serializable"))?;
            if bytes.len() > MAX_PER_PAPER_STATE {
                return Err(ApiError::new(ErrorKind::InvalidInput, "record too large"));
            }
            let path = s.per_paper_path()?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
            }
            write_bytes_atomic(&path, &bytes)?;
            Ok(())
        })
        .await
}

/// 開啟系統的「所有檔案存取」授權頁。
#[tauri::command]
pub async fn open_all_files_settings(app: AppHandle) -> ApiResult<()> {
    app.paperreader()
        .open_all_files_settings()
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_is_read_from_the_available_column_of_df() {
        // Android 的 toybox `df -k` 的樣子
        let out = "Filesystem      1K-blocks    Used Available Use% Mounted on\n\
                   /dev/fuse         5992940 2711240   3265316  46% /storage/emulated\n";
        assert_eq!(parse_df(out), Some(3_265_316 * 1024));
        assert_eq!(parse_df(""), None);
        assert_eq!(
            parse_df("Filesystem 1K-blocks Used Available\n/dev/x 1 2 -\n"),
            None
        );
    }
}
