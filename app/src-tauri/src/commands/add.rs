//! 加入：開系統的選檔畫面選 PDF（或領取別的應用分享進來的 PDF），確認是 PDF，交給核心加入。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::AppState;
use paperreader_core::format::BibDraft;
use paperreader_core::store::{AddMaterialOutcome, AddOutcome};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::{PaperReaderExt, PickedPdf};

/// 一份檔案的加入結果。`outcome`：`added`、`already_present`、`in_trash`、`failed`。
/// `paper_id` 只有前三種有；`error` 只有 `failed` 有（錯誤的種類，畫面依它說原因）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddItem {
    /// 使用者選的檔案的顯示名稱，給畫面列出「哪幾份失敗」；不進日誌。
    pub name: String,
    pub outcome: &'static str,
    pub paper_id: Option<String>,
    pub error: Option<ErrorKind>,
}

/// 一次選檔的結果。使用者在選檔畫面取消，`cancelled` 為真、`items` 是空的。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddBatch {
    pub cancelled: bool,
    pub items: Vec<AddItem>,
}

/// 確認選到的檔案是 PDF（判斷在核心）；不是就回應用層的 `NotPdf`。
fn check_pdf(path: &Path) -> ApiResult<()> {
    if paperreader_core::pdf::is_pdf_file(path)? {
        Ok(())
    } else {
        Err(ApiError::new(ErrorKind::NotPdf, "no PDF header"))
    }
}

/// 把一份選到的檔案加入（確認是 PDF、取檔名當預設篇名、交給核心）。
fn add_one(s: &AppState, temp: &Path) -> ApiResult<AddItem> {
    check_pdf(temp)?;
    let name = temp
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let draft = BibDraft {
        title: paperreader_core::pdf::title_from_file_name(&name),
        ..BibDraft::default()
    };
    let outcome = s.with_lib(|l| l.add_paper(temp, draft, None))?;
    let (outcome, id) = match outcome {
        AddOutcome::Added(id) => ("added", id),
        AddOutcome::AlreadyPresent(id) => ("already_present", id),
        AddOutcome::InTrash(id) => ("in_trash", id),
    };
    Ok(AddItem {
        name,
        outcome,
        paper_id: Some(id),
        error: None,
    })
}

/// 開系統選檔畫面（可以一次選多份 PDF），逐一加入。使用者在選檔畫面取消，交回 `cancelled`。
/// 每一份各自處理：一份失敗（不是 PDF、複製失敗、核心拒絕）不影響其他份，結果逐一回報。
/// 選到的檔案先由外掛複製成暫存檔；不論成功或失敗，每一份的暫存檔（與它所在的資料夾）都會刪掉。
#[tauri::command]
pub async fn add_papers_from_picker(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<AddBatch> {
    let info = app.paperreader().storage_info().await?;
    // 文獻庫還沒開就不必讓使用者選檔
    state.require_lib().await?;
    let picked = app.paperreader().pick_pdfs().await?;
    if picked.is_empty() {
        return Ok(AddBatch {
            cancelled: true,
            items: Vec::new(),
        });
    }
    // 暫存根目錄與外掛用的是同一個來源（`activity.cacheDir`），刪除前用它確認位置
    let temp_root = picks_dir(Path::new(&info.cache_dir));
    let items = state
        .run("add_papers_from_picker", move |s| {
            Ok(add_picked(s, picked, &temp_root))
        })
        .await?;
    Ok(AddBatch {
        cancelled: false,
        items,
    })
}

/// 選檔的暫存放在 `<暫存>/picks/`，分享進來的放在 `<暫存>/shared/`（由外掛自己在啟動時清理，
/// 不和選檔的混在一起：啟動時清 `picks/` 不會刪到剛分享進來、還沒領取的檔）。
const PICKS: &str = "picks";
const SHARED: &str = "shared";

/// 別的應用分享進來、還沒加入的份數。畫面在啟動與回到前景時問；非 Android 沒有這個功能，交回 0。
#[tauri::command]
pub async fn shared_pending(app: AppHandle) -> ApiResult<u32> {
    match app.paperreader().shared_count().await {
        Ok(n) => Ok(n),
        Err(tauri_plugin_paperreader::Error::Unsupported) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// 領取別的應用分享進來的 PDF，逐一加入：和「加入」一次選多份是同一條路（先確認是 PDF，再交給核心；
/// 每一份的結果各自回報，暫存一律清掉）。文獻庫還沒準備好時不領取（分享留著，下次再領）；沒有可以領的交回 `cancelled`。
#[tauri::command]
pub async fn add_shared_papers(app: AppHandle, state: State<'_, AppState>) -> ApiResult<AddBatch> {
    let info = app.paperreader().storage_info().await?;
    state.require_lib().await?;
    let shared = app.paperreader().take_shared().await?;
    if shared.is_empty() {
        return Ok(AddBatch {
            cancelled: true,
            items: Vec::new(),
        });
    }
    let temp_root = Path::new(&info.cache_dir).join(SHARED);
    let items = state
        .run("add_shared_papers", move |s| {
            Ok(add_picked(s, shared, &temp_root))
        })
        .await?;
    Ok(AddBatch {
        cancelled: false,
        items,
    })
}

/// 逐一加入外掛交來的檔案（`temp_root` 是暫存放的地方：`<暫存>/picks` 或 `<暫存>/shared`），
/// 每一份都清掉暫存（不論成敗）。
fn add_picked(s: &AppState, picked: Vec<PickedPdf>, temp_root: &Path) -> Vec<AddItem> {
    picked
        .into_iter()
        .map(|p| {
            let failed = |name: String, kind: ErrorKind| AddItem {
                name,
                outcome: "failed",
                paper_id: None,
                error: Some(kind),
            };
            let Some(path) = p.path else {
                return failed(p.name, pick_failure(p.error.as_deref()));
            };
            let temp = std::path::PathBuf::from(path);
            let result = add_one(s, &temp);
            remove_pick(&temp, temp_root);
            match result {
                Ok(mut item) => {
                    item.name = p.name;
                    item
                }
                Err(e) => {
                    eprintln!(
                        "paperreader: add_papers_from_picker item failed: {:?}",
                        e.kind
                    );
                    failed(p.name, e.kind)
                }
            }
        })
        .collect()
}

/// 加入譯本的結果。`outcome`：`added`、`already_present`（同一份檔案已是這篇的材料）、`failed`。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationResult {
    pub cancelled: bool,
    pub outcome: &'static str,
    pub name: String,
    pub error: Option<ErrorKind>,
}

/// 開系統選檔畫面選一份 PDF，加成這篇文獻的譯本。步驟與「加入」相同：先確認是 PDF，再交給核心。
#[tauri::command]
pub async fn add_translation_from_picker(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
) -> ApiResult<TranslationResult> {
    let info = app.paperreader().storage_info().await?;
    state.require_lib().await?;
    let mut picked = app.paperreader().pick_one_pdf().await?;
    let Some(p) = picked.pop() else {
        return Ok(TranslationResult {
            cancelled: true,
            outcome: "failed",
            name: String::new(),
            error: None,
        });
    };
    let temp_root = picks_dir(Path::new(&info.cache_dir));
    state
        .run("add_translation_from_picker", move |s| {
            Ok(add_translation_picked(s, &paper_id, p, &temp_root))
        })
        .await
}

fn add_translation_picked(
    s: &AppState,
    paper_id: &str,
    p: PickedPdf,
    temp_root: &Path,
) -> TranslationResult {
    let failed = |name: String, kind: ErrorKind| TranslationResult {
        cancelled: false,
        outcome: "failed",
        name,
        error: Some(kind),
    };
    let Some(path) = p.path else {
        return failed(p.name, pick_failure(p.error.as_deref()));
    };
    let temp = std::path::PathBuf::from(path);
    let result = check_pdf(&temp).and_then(|()| {
        s.with_lib(|l| {
            let (_, token) = l.get_paper(paper_id)?;
            l.add_translation(paper_id, &temp, None, &token)
        })
    });
    remove_pick(&temp, temp_root);
    match result {
        Ok(AddMaterialOutcome::Added { .. }) => TranslationResult {
            cancelled: false,
            outcome: "added",
            name: p.name,
            error: None,
        },
        Ok(AddMaterialOutcome::AlreadyPresent { .. }) => TranslationResult {
            cancelled: false,
            outcome: "already_present",
            name: p.name,
            error: None,
        },
        Err(e) => {
            eprintln!("paperreader: add_translation failed: {:?}", e.kind);
            failed(p.name, e.kind)
        }
    }
}

/// 暫存檔所在的資料夾，而且只有它真的是 `<暫存根目錄>/<隨機>`（`temp_root` 是 `<暫存>/picks` 或 `<暫存>/shared`）
/// 才交回；否則交回 `None`（不刪）。
/// 路徑來自外掛、不是網頁，目前不會出錯；這道檢查讓「刪的是整個暫存資料夾或更上層」不靠外掛的寫法維持。
fn pick_dir_to_remove(temp_file: &Path, temp_root: &Path) -> Option<std::path::PathBuf> {
    let dir = temp_file.parent()?;
    (dir.parent()? == temp_root).then(|| dir.to_path_buf())
}

/// 選檔的暫存資料夾 `<暫存>/picks`（外掛把使用者選的檔複製到它底下的 `<隨機>/` 裡）。
pub fn picks_dir(cache_dir: &Path) -> std::path::PathBuf {
    cache_dir.join(PICKS)
}

/// 用完一份選檔的暫存：暫存檔放在 `<暫存根目錄>/<隨機>/<檔名>`，連同那個隨機資料夾一起刪；位置不對就不刪。
pub fn remove_pick(temp_file: &Path, temp_root: &Path) {
    if let Some(dir) = pick_dir_to_remove(temp_file, temp_root) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 外掛沒能複製選到的檔：空間不足要讓畫面說「空間不足」，其餘一律是平台的問題。
pub fn pick_failure(error: Option<&str>) -> ErrorKind {
    if error == Some("no_space") {
        ErrorKind::NoSpace
    } else {
        ErrorKind::Platform
    }
}

/// 清掉上次被中斷留下的選檔暫存（應用啟動時呼叫）。分享進來的暫存（`shared/`）由外掛自己清。
pub fn clear_pick_cache(cache_dir: &Path) {
    let _ = std::fs::remove_dir_all(picks_dir(cache_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_pdf_file_is_refused_and_pdf_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.pdf");
        std::fs::write(&bad, b"this is not a pdf").unwrap();
        assert_eq!(check_pdf(&bad).unwrap_err().kind, ErrorKind::NotPdf);
        let good = dir.path().join("good.pdf");
        std::fs::write(&good, b"%PDF-1.4\n1 0 obj").unwrap();
        assert!(check_pdf(&good).is_ok());
    }

    #[test]
    fn only_a_folder_directly_under_the_temp_root_may_be_removed() {
        let picks = Path::new("/data/cache/picks");
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/picks/abc/x.pdf"), picks),
            Some(Path::new("/data/cache/picks/abc").to_path_buf())
        );
        // 直接放在 picks/ 底下：刪 parent 會刪掉整個 picks
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/picks/x.pdf"), picks),
            None
        );
        // 放在暫存根目錄
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/x.pdf"), picks),
            None
        );
        // 更深一層、或完全不相干
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/picks/a/b/x.pdf"), picks),
            None
        );
        assert_eq!(
            pick_dir_to_remove(Path::new("/elsewhere/picks/abc/x.pdf"), picks),
            None
        );
        assert_eq!(pick_dir_to_remove(Path::new("x.pdf"), picks), None);
        // 分享進來的放在 shared/：只認 shared/ 底下的那一層，不刪 picks/ 的
        let shared = Path::new("/data/cache/shared");
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/shared/abc/x.pdf"), shared),
            Some(Path::new("/data/cache/shared/abc").to_path_buf())
        );
        assert_eq!(
            pick_dir_to_remove(Path::new("/data/cache/picks/abc/x.pdf"), shared),
            None
        );
    }

    #[test]
    fn clearing_the_pick_cache_removes_leftovers_only() {
        let dir = tempfile::tempdir().unwrap();
        let left = dir.path().join("picks/abc");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join("x.pdf"), b"%PDF-").unwrap();
        std::fs::write(dir.path().join("other.txt"), b"keep").unwrap();
        clear_pick_cache(dir.path());
        assert!(!dir.path().join("picks").exists());
        assert!(dir.path().join("other.txt").exists());
    }

    // ---- 一次加入多份 ----

    fn app_state(root: &Path) -> AppState {
        use crate::location::Storage;
        use crate::state::Settings;
        use tauri_plugin_paperreader::StorageInfo;
        let sp = root.join("settings.json");
        let (settings, recovered) = Settings::load_or_create(&sp).unwrap();
        let st = AppState::new(settings, sp, recovered);
        let info = StorageInfo {
            sdk: 36,
            all_files_access: false,
            shared_dir: root.join("shared").display().to_string(),
            private_dir: root.join("private").display().to_string(),
            cache_dir: root.join("cache").display().to_string(),
        };
        st.create(Storage::Private, &info).unwrap();
        st
    }

    /// 外掛交來的一份：暫存檔放在 `<temp_root>/<folder>/<name>`。
    fn picked(temp_root: &Path, folder: &str, name: &str, body: &[u8]) -> PickedPdf {
        let dir = temp_root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        PickedPdf {
            name: name.to_string(),
            path: Some(path.display().to_string()),
            bytes: Some(body.len() as u64),
            error: None,
        }
    }

    #[test]
    fn a_batch_reports_each_file_and_one_failure_does_not_stop_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let st = app_state(dir.path());
        let cache = dir.path().join("cache").join(PICKS);
        let batch = vec![
            picked(&cache, "a", "One.pdf", b"%PDF-1.4 one"),
            picked(&cache, "b", "fake.pdf", b"not a pdf at all"),
            picked(&cache, "c", "One-again.pdf", b"%PDF-1.4 one"), // 內容相同：原本就在
            PickedPdf {
                name: "Big.pdf".into(),
                path: None,
                bytes: None,
                error: Some("no_space".into()),
            },
            PickedPdf {
                name: "Gone.pdf".into(),
                path: None,
                bytes: None,
                error: Some("failed".into()),
            },
            picked(&cache, "d", "Two.pdf", b"%PDF-1.4 two"),
        ];
        let items = add_picked(&st, batch, &cache);
        let got: Vec<(&str, &str, Option<ErrorKind>)> = items
            .iter()
            .map(|i| (i.name.as_str(), i.outcome, i.error))
            .collect();
        assert_eq!(
            got,
            vec![
                ("One.pdf", "added", None),
                ("fake.pdf", "failed", Some(ErrorKind::NotPdf)),
                ("One-again.pdf", "already_present", None),
                ("Big.pdf", "failed", Some(ErrorKind::NoSpace)),
                ("Gone.pdf", "failed", Some(ErrorKind::Platform)),
                ("Two.pdf", "added", None),
            ]
        );
        // 已存在的那一份指向原本那一篇
        assert_eq!(items[0].paper_id, items[2].paper_id);
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        // 不論成敗，每一份的暫存資料夾都清掉了
        let left: Vec<_> = std::fs::read_dir(&cache).unwrap().collect();
        assert!(left.is_empty(), "temp folders must all be removed");
    }

    #[test]
    fn a_file_in_the_trash_is_reported_and_not_added_again() {
        let dir = tempfile::tempdir().unwrap();
        let st = app_state(dir.path());
        let cache = dir.path().join("cache").join(PICKS);
        let first = add_picked(
            &st,
            vec![picked(&cache, "a", "A.pdf", b"%PDF-1.4 a")],
            &cache,
        );
        let id = first[0].paper_id.clone().unwrap();
        st.with_lib(|l| l.remove_paper(&id)).unwrap();
        let again = add_picked(
            &st,
            vec![picked(&cache, "b", "A2.pdf", b"%PDF-1.4 a")],
            &cache,
        );
        assert_eq!(again[0].outcome, "in_trash");
        assert_eq!(again[0].paper_id.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn shared_files_go_through_the_same_add_and_their_temp_folders_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let st = app_state(dir.path());
        let shared = dir.path().join("cache").join(SHARED);
        let items = add_picked(
            &st,
            vec![
                picked(&shared, "a", "Shared-1.pdf", b"%PDF-1.4 shared one"),
                picked(&shared, "b", "Shared-2.pdf", b"%PDF-1.4 shared two"),
                picked(&shared, "c", "notes.pdf", b"plain text with a pdf name"),
            ],
            &shared,
        );
        let got: Vec<(&str, &str)> = items.iter().map(|i| (i.name.as_str(), i.outcome)).collect();
        assert_eq!(
            got,
            vec![
                ("Shared-1.pdf", "added"),
                ("Shared-2.pdf", "added"),
                ("notes.pdf", "failed"),
            ]
        );
        // 讀不出來是 PDF 的不加入
        assert_eq!(items[2].error, Some(ErrorKind::NotPdf));
        assert_eq!(st.with_lib(|l| Ok(l.list_papers().len())).unwrap(), 2);
        let left: Vec<_> = std::fs::read_dir(&shared).unwrap().collect();
        assert!(left.is_empty(), "shared temp folders must all be removed");
    }
}
