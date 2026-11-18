//! 備份檔：匯出整個文獻庫、從備份檔還原。
//!
//! 匯出先寫到應用暫存資料夾的 `exports/`，再交給系統的「存到…」或分享畫面（`deliver_export`，同其他匯出）。
//! 還原與從另一台加入：選檔、複製到暫存、放出來、把文獻庫換過去。目前的文獻庫是空的（或還沒有）時放在正式位置，
//! 不是空的放在旁邊的新位置、舊的原樣留著（`AppState::place_archive`）。

use crate::commands::add::{pick_failure, picks_dir, remove_pick};
use crate::commands::export::exports_dir;
use crate::error::{ApiError, ApiResult};
use crate::location::Storage;
use crate::state::{AppState, ArchiveKind};
use paperreader_core::ids::now_compact;
use paperreader_core::store::SkippedFile;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// 備份檔的檔名（存到…時當預設檔名）。
    pub name: String,
    /// 暫存的備份檔路徑；交給 `deliver_export`。
    pub path: String,
    pub files: usize,
    pub bytes: u64,
    /// 沒有放進備份檔的檔案（路徑、原因）。
    pub skipped: Vec<SkippedFile>,
    pub changed_during_export: Vec<String>,
}

/// 匯出：把整個文獻庫寫成一個備份檔，放在應用暫存資料夾。耗時，在背景執行緒做。
#[tauri::command]
pub async fn export_backup_file(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<ExportResult> {
    let dir = exports_dir(&app).await?;
    state
        .run("export_backup_file", move |s| {
            let name = format!("PaperReader-backup-{}.zip", now_compact());
            let out = dir.join(&name);
            let report = s.export_backup(&out, &mut |_, _| {})?;
            Ok(ExportResult {
                name,
                path: out.display().to_string(),
                files: report.files,
                bytes: report.bytes,
                skipped: report.skipped,
                changed_during_export: report.changed_during_export,
            })
        })
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub cancelled: bool,
    /// 讀進來的是備份檔（`backup`）還是同步包（`sync`）。
    pub kind: Option<ArchiveKind>,
    pub files: usize,
    pub bytes: u64,
    /// 還原後快速檢查找到的問題數。
    pub problems: usize,
    /// 還原後重讀每個檔、與備份清單對不上的檔數。
    pub mismatches: usize,
    /// 原本的文獻庫還在這裡，沒有動；放在原位（原本是空的或還沒有文獻庫）時是 `null`。
    pub old_path: Option<String>,
    pub new_path: String,
}

impl RestoreResult {
    fn cancelled() -> RestoreResult {
        RestoreResult {
            cancelled: true,
            kind: None,
            files: 0,
            bytes: 0,
            problems: 0,
            mismatches: 0,
            old_path: None,
            new_path: String::new(),
        }
    }
}

/// 使用者用系統選檔畫面選一個檔（外掛已複製到 `cache/picks/`）。取消是 `None`。
pub async fn pick_one(app: &AppHandle) -> ApiResult<Option<PathBuf>> {
    let mut picked = app.paperreader().pick_any_file().await?;
    let Some(p) = picked.pop() else {
        return Ok(None);
    };
    match p.path {
        Some(path) => Ok(Some(PathBuf::from(path))),
        None => Err(ApiError::new(
            pick_failure(p.error.as_deref()),
            "could not copy the chosen file",
        )),
    }
}

/// 選檔 → 放出來（`accept_sync` 時同步包也收）→ 換過去。失敗時目前的文獻庫維持原樣。
async fn place_from_picker(
    app: AppHandle,
    state: &AppState,
    storage: Option<Storage>,
    accept_sync: bool,
    command: &'static str,
) -> ApiResult<RestoreResult> {
    let info = app.paperreader().storage_info().await?;
    let Some(temp) = pick_one(&app).await? else {
        return Ok(RestoreResult::cancelled());
    };
    let picks = picks_dir(std::path::Path::new(&info.cache_dir));
    state
        .run(command, move |s| {
            let result = s.place_archive(&temp, &info, storage, accept_sync);
            remove_pick(&temp, &picks);
            let placed = result?;
            Ok(RestoreResult {
                cancelled: false,
                kind: Some(placed.kind),
                files: placed.report.files,
                bytes: placed.report.bytes,
                problems: placed.report.problems.len(),
                mismatches: placed.report.manifest_mismatches.len(),
                old_path: placed.old.map(|p| p.display().to_string()),
                new_path: placed.new.display().to_string(),
            })
        })
        .await
}

/// 從備份檔還原：目前的文獻庫是空的就放在原位，不是空的放在旁邊的新位置。
#[tauri::command]
pub async fn restore_backup_from_picker(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<RestoreResult> {
    state.require_lib().await?;
    place_from_picker(app, &state, None, false, "restore_backup_from_picker").await
}

/// 從另一台加入：選同步包或備份檔，照清單的種類加入或還原。還沒有文獻庫時放在正式位置，
/// `storage` 是放哪一種空間（`null`：剛從授權頁回來，有權限放共用空間）。
#[tauri::command]
pub async fn library_join_from_picker(
    app: AppHandle,
    state: State<'_, AppState>,
    storage: Option<Storage>,
) -> ApiResult<RestoreResult> {
    place_from_picker(app, &state, storage, true, "library_join_from_picker").await
}
