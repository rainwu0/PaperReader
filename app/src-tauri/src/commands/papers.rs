//! 目錄與單篇文獻：列出、取描述、修改書目、移除、還原、回收區。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::format::{Author, BibDraft, BibFill, PaperFile};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::store::{PaperSummary, PurgeReport, RemovedMaterialEntry, TrashEntry};
use serde::{Deserialize, Serialize};
use tauri::State;

/// 目錄：每篇帶閱讀狀況，依資料夾名稱排序（畫面再依加入時間排）。
#[tauri::command]
pub async fn list_papers(state: State<'_, AppState>) -> ApiResult<Vec<PaperSummary>> {
    state.lib("list_papers", move |l| Ok(l.list_papers())).await
}

/// 「繼續閱讀」：最近讀的一篇的代號；沒有就是 `null`。
#[tauri::command]
pub async fn continue_reading(state: State<'_, AppState>) -> ApiResult<Option<String>> {
    state
        .lib("continue_reading", move |l| {
            Ok(l.continue_reading(1).into_iter().next())
        })
        .await
}

#[derive(Serialize)]
pub struct PaperDetail {
    pub paper: PaperFile,
    /// 版本記號：改書目時原樣交回，檔案在別處被改過就會被擋下。
    pub token: String,
}

#[tauri::command]
pub async fn get_paper(state: State<'_, AppState>, id: String) -> ApiResult<PaperDetail> {
    state
        .lib("get_paper", move |l| {
            let (paper, token) = l.get_paper(&id)?;
            Ok(PaperDetail {
                paper,
                token: token.0,
            })
        })
        .await
}

/// 畫面送來的書目草稿（核心的 `BibDraft` 沒有反序列化，所以在這裡轉）。
#[derive(Deserialize)]
pub struct BibDraftInput {
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Vec<Author>,
    pub year: Option<i32>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub url: Option<String>,
}

impl From<BibDraftInput> for BibDraft {
    fn from(d: BibDraftInput) -> BibDraft {
        BibDraft {
            title: d.title,
            authors: d.authors,
            year: d.year,
            venue: d.venue,
            doi: d.doi,
            url: d.url,
        }
    }
}

/// 修改書目；交回新的版本記號。
#[tauri::command]
pub async fn update_bib(
    state: State<'_, AppState>,
    id: String,
    draft: BibDraftInput,
    token: String,
) -> ApiResult<String> {
    state
        .lib("update_bib", move |l| {
            l.update_bib(&id, draft.into(), &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

#[tauri::command]
pub async fn remove_paper(state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .lib("remove_paper", move |l| l.remove_paper(&id))
        .await
}

#[tauri::command]
pub async fn restore_paper(state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .lib("restore_paper", move |l| l.restore_paper(&id))
        .await
}

#[tauri::command]
pub async fn list_trash(state: State<'_, AppState>) -> ApiResult<Vec<TrashEntry>> {
    state.lib("list_trash", move |l| l.list_trash()).await
}

/// 把一項材料（原文以外）移到回收區。`token` 是畫面手上那篇的版本記號；交回新的。
#[tauri::command]
pub async fn remove_material(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    token: String,
) -> ApiResult<String> {
    state
        .lib("remove_material", move |l| {
            l.remove_material(&paper_id, &material_id, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

/// 回收區裡移除的材料。
#[tauri::command]
pub async fn list_removed_materials(
    state: State<'_, AppState>,
) -> ApiResult<Vec<RemovedMaterialEntry>> {
    state
        .lib("list_removed_materials", move |l| {
            l.list_removed_materials()
        })
        .await
}

/// 永久刪除回收區裡的一項（`.trash/` 裡的資料夾名稱）。畫面先確認一次。
#[tauri::command]
pub async fn purge_trash_entry(state: State<'_, AppState>, folder: String) -> ApiResult<()> {
    state
        .lib("purge_trash_entry", move |l| l.purge_trash_entry(&folder))
        .await
}

/// 把回收區裡移除的材料放回那篇（`.trash/` 裡的資料夾名稱），交回那篇的代號。
#[tauri::command]
pub async fn restore_material(state: State<'_, AppState>, folder: String) -> ApiResult<String> {
    state
        .lib("restore_material", move |l| l.restore_material(&folder))
        .await
}

/// 清空回收區：每一篇與每一項移除的材料都永久刪除。
#[tauri::command]
pub async fn empty_trash(state: State<'_, AppState>) -> ApiResult<PurgeReport> {
    state.lib("empty_trash", move |l| l.empty_trash()).await
}

/// 自動補書目（逐篇處理的 `bib.ids`）：只補空著的年份、DOI、網址，不改書目的修改時間。
/// 補到了交回 `true`。
#[tauri::command]
pub async fn fill_bib_blanks(
    state: State<'_, AppState>,
    paper_id: String,
    fill: BibFill,
) -> ApiResult<bool> {
    state
        .lib("fill_bib_blanks", move |l| {
            l.fill_bib_blanks(&paper_id, fill)
        })
        .await
}

/// 記下一項材料的總頁數（PDF 載入、知道頁數時呼叫；已經對了就不寫）。
/// 它會改 `paper.json`，所以畫面手上的版本記號因此過期（核心的規則）。
#[tauri::command]
pub async fn set_material_pages(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    pages: u32,
) -> ApiResult<bool> {
    state
        .lib("set_material_pages", move |l| {
            l.set_material_pages(&paper_id, &material_id, pages)
        })
        .await
}
