//! 整批匯入：選資料夾或 zip → 掃描（不持文獻庫的鎖、可取消）→ 比對成預覽
//! → 一篇一步寫入（每一步持一次鎖，閱讀畫面的寫入不必等整批做完）→ 取消時保留已加入的。
//!
//! 掃描的結果、進度、取消旗標、zip 的複本放在這一層（`ImportSlot`），不在文獻庫的鎖後面。
//! zip 由系統選檔畫面複製到 `cache/picks/<隨機>/`；寫完、取消、放棄預覽時刪掉（啟動時 `clear_pick_cache` 也會清）。
//! 資料夾經平台轉接器轉成主要儲存空間的路徑，讀它要有「所有檔案存取」權限。

use crate::commands::add::{picks_dir, remove_pick};
use crate::commands::backup::pick_one;
use crate::commands::export::{copy_out, exports_dir, file_stem, paper_title};
use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::AppState;
use paperreader_core::error::LibError;
use paperreader_core::import::{
    ImportContainer, ImportPreview, ImportReport, ImportScan, ImportStep, ZIP_BACKSLASH,
};
use paperreader_core::store::ArchiveKind;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::{PaperReaderExt, PickedFolder};

/// 整批匯入在外殼這一層的狀態。
#[derive(Default)]
pub struct ImportSlot {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    inner: Mutex<SlotInner>,
}

#[derive(Default)]
struct SlotInner {
    /// 掃描完、還沒比對成預覽的結果。
    scan: Option<ImportScan>,
    /// zip 的複本（`<picks>/<隨機>/<檔名>`）與 `picks` 資料夾：用完連同那個隨機資料夾一起刪。
    copy: Option<(PathBuf, PathBuf)>,
}

impl ImportSlot {
    fn lock(&self) -> std::sync::MutexGuard<'_, SlotInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 丟掉掃描的結果、刪掉 zip 的複本。
    fn clear(&self) {
        let mut g = self.lock();
        g.scan = None;
        if let Some((file, picks)) = g.copy.take() {
            remove_pick(&file, &picks);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScanned {
    /// 使用者在選擇畫面或掃描途中取消了。
    pub cancelled: bool,
    /// 選到的資料夾不在主要儲存空間（記憶卡、雲端）：這一版不收，請使用者改用 zip。
    pub unsupported: bool,
}

impl ImportScanned {
    fn cancelled() -> ImportScanned {
        ImportScanned {
            cancelled: true,
            unsupported: false,
        }
    }
}

/// 選資料夾（`zip` 為假）或 zip，接著掃描。掃描不持文獻庫的鎖；進度用 `import_progress` 讀，`import_cancel` 取消。
/// 掃描好的結果留在外殼，接著由 `prepare_import` 比對成預覽。
#[tauri::command]
pub async fn scan_import(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<ImportSlot>>,
    zip: bool,
) -> ApiResult<ImportScanned> {
    state.require_lib().await?;
    slot.clear();
    let info = app.paperreader().storage_info().await?;
    let container = if zip {
        let Some(file) = pick_one(&app).await? else {
            return Ok(ImportScanned::cancelled());
        };
        let picks = picks_dir(std::path::Path::new(&info.cache_dir));
        slot.lock().copy = Some((file.clone(), picks));
        ImportContainer::Zip(file)
    } else {
        match app.paperreader().pick_folder().await? {
            PickedFolder::Cancelled => return Ok(ImportScanned::cancelled()),
            PickedFolder::Unsupported => {
                return Ok(ImportScanned {
                    cancelled: false,
                    unsupported: true,
                });
            }
            // 主要儲存空間的資料夾要有「所有檔案存取」才讀得到
            PickedFolder::Path(_) if !info.all_files_access => {
                return Err(ApiError::new(
                    ErrorKind::PermissionDenied,
                    "all files access is needed to read the folder",
                ));
            }
            PickedFolder::Path(p) => ImportContainer::Folder(PathBuf::from(p)),
        }
    };
    let slot = slot.inner().clone();
    state
        .run("scan_import", move |_| {
            slot.done.store(0, Ordering::Relaxed);
            slot.total.store(0, Ordering::Relaxed);
            slot.cancel.store(false, Ordering::Relaxed);
            let scanned = paperreader_core::import::scan_import(&container, &mut |done, total| {
                slot.total.store(total, Ordering::Relaxed);
                slot.done.store(done, Ordering::Relaxed);
                !slot.cancel.load(Ordering::Relaxed)
            });
            match scanned {
                Ok(Some(scan)) => {
                    slot.lock().scan = Some(scan);
                    Ok(ImportScanned {
                        cancelled: false,
                        unsupported: false,
                    })
                }
                Ok(None) => {
                    slot.clear();
                    Ok(ImportScanned::cancelled())
                }
                Err(e) => {
                    // 先判斷再清：判斷時要看 zip 的複本
                    let err = scan_error(&container, e);
                    slot.clear();
                    Err(err)
                }
            }
        })
        .await
}

/// 掃描的錯誤轉成畫面認得的種類；`reason` 本身不交給畫面。
/// - zip 的項目名稱用了反斜線（核心的 `reason` 以 `ZIP_BACKSLASH` 開頭）：畫面直接請使用者換工具重新壓縮。
/// - 讀不懂的 zip 其實是同步包或備份檔：說該用哪個入口。只在掃描已經失敗時才看，掃描得了的 zip 照常匯入。
/// - 其餘照一般的對應。
fn scan_error(container: &ImportContainer, e: LibError) -> ApiError {
    match &e {
        LibError::Unreadable { reason, .. } if reason.starts_with(ZIP_BACKSLASH) => {
            ApiError::new(ErrorKind::ImportZipBackslash, "ImportZipBackslash")
        }
        LibError::Unreadable { .. } => match container {
            ImportContainer::Zip(p) => paperreader_core::sync::package::peek_kind(p)
                .ok()
                .and_then(wrong_entry)
                .unwrap_or_else(|| e.into()),
            ImportContainer::Folder(_) => e.into(),
        },
        _ => e.into(),
    }
}

/// 整批匯入選到別的入口用的檔：同步包、備份檔各有自己的種類；其餘是 `None`。
fn wrong_entry(kind: ArchiveKind) -> Option<ApiError> {
    let kind = match kind {
        ArchiveKind::SyncPackage => ErrorKind::ImportGotSyncPackage,
        ArchiveKind::Backup => ErrorKind::ImportGotBackup,
        ArchiveKind::ImportZip | ArchiveKind::Other => return None,
    };
    Some(ApiError::new(kind, format!("{kind:?}")))
}

/// 把掃描的結果和文獻庫比對成預覽（持鎖，時間短）。除了記憶體裡的計畫，什麼都不寫。
#[tauri::command]
pub async fn prepare_import(
    state: State<'_, AppState>,
    slot: State<'_, Arc<ImportSlot>>,
) -> ApiResult<ImportPreview> {
    let scan = slot.lock().scan.take().ok_or_else(|| {
        ApiError::from(LibError::InvalidInput {
            reason: "nothing scanned".into(),
        })
    })?;
    let slot = slot.inner().clone();
    let r = state
        .lib("prepare_import", move |l| l.prepare_import(scan))
        .await;
    if r.is_err() {
        slot.clear();
    }
    r
}

/// 寫下一篇（每呼叫一次持一次鎖）。做完最後一篇、或計畫已經不在（清單被改過 → `Conflict`）時刪掉 zip 的複本。
#[tauri::command]
pub async fn import_step(
    state: State<'_, AppState>,
    slot: State<'_, Arc<ImportSlot>>,
    preview_id: String,
) -> ApiResult<ImportStep> {
    let slot = slot.inner().clone();
    let r = state
        .lib("import_step", move |l| l.import_step(&preview_id))
        .await;
    match &r {
        Ok(ImportStep::Progress { .. }) => {}
        Err(e) if e.kind == ErrorKind::ReadOnly => {}
        _ => slot.clear(),
    }
    r
}

/// 使用者取消寫入、或放棄預覽：已加入的保留，剩下的記成「沒做」；刪掉 zip 的複本。
#[tauri::command]
pub async fn finish_import(
    state: State<'_, AppState>,
    slot: State<'_, Arc<ImportSlot>>,
    preview_id: String,
) -> ApiResult<ImportReport> {
    let slot = slot.inner().clone();
    let r = state
        .lib("finish_import", move |l| l.finish_import(&preview_id))
        .await;
    slot.clear();
    r
}

/// 掃描的進度：`[已讀位元組, 總位元組]`；第一輪（檢查欄位）還不知道總量時是 `[0, 0]`。
#[tauri::command]
pub fn import_progress(slot: State<'_, Arc<ImportSlot>>) -> [u64; 2] {
    [
        slot.done.load(Ordering::Relaxed),
        slot.total.load(Ordering::Relaxed),
    ]
}

/// 取消掃描（下一次回報進度時停下）。
#[tauri::command]
pub fn import_cancel(slot: State<'_, Arc<ImportSlot>>) {
    slot.cancel.store(true, Ordering::Relaxed);
}

/// 用其他應用打開一份 Markdown 材料（匯入的譯本、抽出文字；閱讀畫面這一版開不了它們）：交出唯讀的一份複本。
/// 交回 `opened`、`no_app` 或 `not_shareable`。
#[tauri::command]
pub async fn material_open_external(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<String> {
    let dir = exports_dir(&app).await?;
    let f = state
        .run("material_open_external", move |s| {
            let (src, title) = s.with_lib(|l| {
                let (paper, _) = l.get_paper(&paper_id)?;
                let m = paper
                    .materials
                    .iter()
                    .find(|m| m.id == material_id)
                    .ok_or_else(|| LibError::NotFound {
                        what: format!("material {material_id}"),
                    })?;
                if m.form != "markdown" {
                    return Err(LibError::InvalidInput {
                        reason: "not a markdown material".into(),
                    });
                }
                Ok((
                    l.material_path(&paper_id, &material_id)?,
                    paper_title(l, &paper_id)?,
                ))
            })?;
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "material.md".into());
            copy_out(&dir, &src, format!("{}-{name}", file_stem(&title, "paper")))
        })
        .await?;
    Ok(app.paperreader().open_external(&f.path, false).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreadable(reason: &str) -> LibError {
        LibError::Unreadable {
            path: "x.zip".into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn backslash_zip_gets_its_own_kind() {
        let zip = ImportContainer::Zip("x.zip".into());
        let e = unreadable(&format!("{ZIP_BACKSLASH}: pdf\\a.pdf"));
        assert_eq!(scan_error(&zip, e).kind, ErrorKind::ImportZipBackslash);
        // 檔不存在、看不出是哪一種：照一般的對應
        let e = unreadable("not an import manifest");
        assert_eq!(scan_error(&zip, e).kind, ErrorKind::Unreadable);
    }

    /// 整批匯入選到同步包、備份檔：各有自己的種類；整批匯入用的 zip 與別的檔照一般的對應。
    #[test]
    fn sync_packages_and_backups_get_their_own_kinds() {
        let kind = |k| wrong_entry(k).map(|e| e.kind);
        assert_eq!(
            kind(ArchiveKind::SyncPackage),
            Some(ErrorKind::ImportGotSyncPackage)
        );
        assert_eq!(kind(ArchiveKind::Backup), Some(ErrorKind::ImportGotBackup));
        assert_eq!(kind(ArchiveKind::ImportZip), None);
        assert_eq!(kind(ArchiveKind::Other), None);

        // 真的同步包與備份檔：掃描失敗後認得出來
        let lib_dir = tempfile::tempdir().unwrap();
        let mut lib = paperreader_core::local::LocalLibrary::open(
            lib_dir.path(),
            "01JCK3T1A2B3C4D5E6F7G8H9JK",
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let pkg = out.path().join("s.zip");
        lib.export_sync_package(
            &pkg,
            paperreader_core::store::SyncMode::Full,
            None,
            &mut |_, _| {},
        )
        .unwrap();
        let backup = out.path().join("b.zip");
        lib.export_backup(&backup, &mut |_, _| {}).unwrap();
        for (file, want) in [
            (pkg, ErrorKind::ImportGotSyncPackage),
            (backup, ErrorKind::ImportGotBackup),
        ] {
            let zip = ImportContainer::Zip(file);
            let e = paperreader_core::import::scan_import(&zip, &mut |_, _| true).unwrap_err();
            assert_eq!(scan_error(&zip, e).kind, want);
        }

        // 選到的不是 zip、或選的是資料夾：照樣是讀不懂
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();
        let e = unreadable("invalid Zip archive");
        assert_eq!(
            scan_error(&ImportContainer::Zip(pdf), e).kind,
            ErrorKind::Unreadable
        );
        let e = unreadable("paperreader-import.json is missing");
        assert_eq!(
            scan_error(&ImportContainer::Folder(dir.path().into()), e).kind,
            ErrorKind::Unreadable
        );
    }
}
