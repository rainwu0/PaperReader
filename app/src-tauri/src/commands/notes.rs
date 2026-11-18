//! 筆記：劃線、短評、整篇的想法（存在每篇一份的筆記檔，核心管格式）。
//!
//! 筆記檔的版本記號由畫面保管：讀的時候拿到，寫的時候原樣交回；檔案在別處被改過就回 `Conflict`，
//! 畫面重新讀、提示使用者。筆記檔還不存在時記號是 `null`。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::notes::{Highlight, Quote, TextPosition};
use paperreader_core::store::{HighlightDraft, NotesDoc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

#[derive(Serialize)]
pub struct NotesView {
    pub doc: NotesDoc,
    pub token: Option<String>,
}

#[tauri::command]
pub async fn read_notes(state: State<'_, AppState>, paper_id: String) -> ApiResult<NotesView> {
    state
        .lib("read_notes", move |l| {
            let (doc, token) = l.read_notes(&paper_id)?;
            Ok(NotesView {
                doc,
                token: token.map(|t| t.0),
            })
        })
        .await
}

/// 畫面送來的一條新劃線（核心的 `HighlightDraft` 沒有反序列化，所以在這裡轉）。
#[derive(Deserialize)]
pub struct HighlightInput {
    pub material: String,
    /// 頁序，從 0 起算。
    pub page_index: u32,
    pub page_label: Option<String>,
    /// 頁面座標的矩形 `[左, 下, 右, 上]`，單位是點。
    pub rects: Vec<[f64; 4]>,
    pub position: Option<TextPosition>,
    pub quote: Quote,
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub comment: String,
}

impl From<HighlightInput> for HighlightDraft {
    fn from(d: HighlightInput) -> HighlightDraft {
        HighlightDraft {
            material: d.material,
            page_index: d.page_index,
            page_label: d.page_label,
            rects: d.rects,
            next_page_rects: Vec::new(),
            position: d.position,
            quote: d.quote,
            fingerprint: d.fingerprint,
            comment: d.comment,
        }
    }
}

#[derive(Serialize)]
pub struct AddedHighlight {
    pub highlight: Highlight,
    pub token: String,
}

#[tauri::command]
pub async fn add_highlight(
    state: State<'_, AppState>,
    paper_id: String,
    draft: HighlightInput,
    token: Option<String>,
) -> ApiResult<AddedHighlight> {
    state
        .lib("add_highlight", move |l| {
            let token = token.map(VersionToken);
            let (highlight, new_token) =
                l.add_highlight(&paper_id, draft.into(), token.as_ref())?;
            Ok(AddedHighlight {
                highlight,
                token: new_token.0,
            })
        })
        .await
}

#[tauri::command]
pub async fn set_highlight_comment(
    state: State<'_, AppState>,
    paper_id: String,
    highlight_id: String,
    comment: String,
    token: String,
) -> ApiResult<String> {
    state
        .lib("set_highlight_comment", move |l| {
            l.set_highlight_comment(&paper_id, &highlight_id, &comment, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

#[tauri::command]
pub async fn delete_highlight(
    state: State<'_, AppState>,
    paper_id: String,
    highlight_id: String,
    token: String,
) -> ApiResult<String> {
    state
        .lib("delete_highlight", move |l| {
            l.delete_highlight(&paper_id, &highlight_id, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}

/// 寫整篇的想法。筆記檔還不存在時 `token` 是 `null`。
#[tauri::command]
pub async fn set_thoughts(
    state: State<'_, AppState>,
    paper_id: String,
    text: String,
    token: Option<String>,
) -> ApiResult<String> {
    state
        .lib("set_thoughts", move |l| {
            let token = token.map(VersionToken);
            l.set_thoughts(&paper_id, &text, token.as_ref())
                .map(|t| t.0)
        })
        .await
}

/// 把這篇的筆記檔交給別的應用。`obsidian` 為真用 `obsidian://open?vault=…&file=…` 開啟，否則開系統的
/// 「用其他應用打開」選單。筆記檔還不存在時先建立一份。交回 `opened` 或 `no_app`（沒裝 Obsidian 等）。
#[tauri::command]
pub async fn open_notes_external(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    obsidian: bool,
) -> ApiResult<String> {
    let path = state
        .run("open_notes_external", move |s| {
            s.with_lib(|l| {
                let (doc, token) = l.read_notes(&paper_id)?;
                let rel = match doc.path {
                    Some(p) => p,
                    None => {
                        l.set_thoughts(&paper_id, "", token.as_ref())?;
                        l.read_notes(&paper_id)?.0.path.unwrap_or_default()
                    }
                };
                Ok(rel)
            })
            .map(|rel| (s.open_library_root(), rel))
        })
        .await?;
    let (Some(dir), rel) = path else {
        return Err(crate::error::ApiError::new(
            crate::error::ErrorKind::NotFound,
            "library not ready",
        ));
    };
    let full = dir.join(rel);
    Ok(app
        .paperreader()
        .open_external(&full.to_string_lossy(), obsidian)
        .await?)
}
