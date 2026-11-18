//! 抽出的文字：每篇資料夾裡的 `text.json`（核心管格式）。抽文字是內建的（逐篇處理的
//! `text.pages`），所以這兩個是一般的指令，全文搜尋的外掛拿掉之後照樣在；外掛自己的指令在 `ext.rs`（件二）。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::text::{TextDraft, TextState};
use std::collections::HashMap;
use tauri::State;

/// 寫入或整份取代一篇的抽出的文字（逐篇處理的抽取用）；交回新的版本記號（沒寫時是現有的）。
#[tauri::command]
pub async fn text_write(
    state: State<'_, AppState>,
    paper_id: String,
    draft: TextDraft,
) -> ApiResult<String> {
    state
        .lib("text_write", move |l| {
            l.write_text(&paper_id, draft).map(|t| t.0)
        })
        .await
}

/// 逐篇處理判斷做過沒有：一次問一批，交回每篇的檔頭（沒有檔、讀不懂、不在目錄是 `null`）。
#[tauri::command]
pub async fn text_states(
    state: State<'_, AppState>,
    paper_ids: Vec<String>,
) -> ApiResult<HashMap<String, Option<TextState>>> {
    state
        .lib("text_states", move |l| {
            Ok(l.text_states(&paper_ids).into_iter().collect())
        })
        .await
}
