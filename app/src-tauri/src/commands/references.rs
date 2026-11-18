//! 參考文獻清單：每篇資料夾裡的 `references.json`（核心管格式）。
//! 這幾個是一般的指令，外掛拿掉之後照樣在；外掛自己的指令在 `ext.rs`。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::references::{Judgment, JudgmentKind, ReferencesDraft};
use serde::Serialize;
use tauri::State;

/// 寫入或取代抽出的部分（逐篇處理的抽取用）；交回新的版本記號。
#[tauri::command]
pub async fn references_write(
    state: State<'_, AppState>,
    paper_id: String,
    draft: ReferencesDraft,
) -> ApiResult<String> {
    state
        .lib("references_write", move |l| {
            l.write_references(&paper_id, draft).map(|t| t.0)
        })
        .await
}

#[derive(Serialize)]
pub struct JudgmentAdded {
    pub judgment: Judgment,
    /// `references.json` 新的版本記號。
    pub token: String,
}

/// 加一項判斷（是這篇、不是這篇、指定這篇）。`token` 是畫面手上那份清單的版本記號。
#[tauri::command]
pub async fn link_judgment_add(
    state: State<'_, AppState>,
    paper_id: String,
    index: usize,
    kind: JudgmentKind,
    target: String,
    token: String,
) -> ApiResult<JudgmentAdded> {
    state
        .lib("link_judgment_add", move |l| {
            l.add_link_judgment(&paper_id, index, kind, &target, &VersionToken(token))
                .map(|(judgment, t)| JudgmentAdded {
                    judgment,
                    token: t.0,
                })
        })
        .await
}

/// 刪判斷（撤銷、清掉找不回的）；交回新的版本記號。
#[tauri::command]
pub async fn link_judgments_remove(
    state: State<'_, AppState>,
    paper_id: String,
    ids: Vec<String>,
    token: String,
) -> ApiResult<String> {
    state
        .lib("link_judgments_remove", move |l| {
            l.remove_link_judgments(&paper_id, &ids, &VersionToken(token))
                .map(|t| t.0)
        })
        .await
}
