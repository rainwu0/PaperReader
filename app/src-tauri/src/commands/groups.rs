//! 群組：定義（建立、改名、刪除）與所屬（放進、移出）。
//!
//! 核心的每個寫入都要帶版本記號（群組的定義帶 `library.json` 的、所屬帶那篇 `paper.json` 的），
//! 這裡在每次呼叫前才讀最新的記號：本機只有一個寫入者，畫面不必保管記號。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::groups::GroupDef;
use serde::Serialize;
use tauri::State;

/// 所有群組（依檔案裡的順序）。
#[tauri::command]
pub async fn list_groups(state: State<'_, AppState>) -> ApiResult<Vec<GroupDef>> {
    state
        .lib("list_groups", move |l| l.list_groups().map(|(g, _)| g))
        .await
}

#[tauri::command]
pub async fn create_group(state: State<'_, AppState>, name: String) -> ApiResult<GroupDef> {
    state
        .lib("create_group", move |l| {
            let (_, token) = l.list_groups()?;
            l.create_group(&name, &token).map(|(g, _)| g)
        })
        .await
}

#[tauri::command]
pub async fn rename_group(state: State<'_, AppState>, id: String, name: String) -> ApiResult<()> {
    state
        .lib("rename_group", move |l| {
            let (_, token) = l.list_groups()?;
            l.rename_group(&id, &name, &token).map(|_| ())
        })
        .await
}

/// 刪除群組的結果：沒能移出這個群組的文獻代號（其餘都已移出；文獻本身不會被刪）。
#[derive(Serialize)]
pub struct DeleteGroupResult {
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn delete_group(state: State<'_, AppState>, id: String) -> ApiResult<DeleteGroupResult> {
    state
        .lib("delete_group", move |l| {
            let (_, token) = l.list_groups()?;
            l.delete_group(&id, &token)
                .map(|o| DeleteGroupResult { skipped: o.skipped })
        })
        .await
}

/// 把一篇放進、或移出一個群組。`member` 為真是放進、為假是移出。
#[tauri::command]
pub async fn set_group_member(
    state: State<'_, AppState>,
    paper_id: String,
    group_id: String,
    member: bool,
) -> ApiResult<()> {
    state
        .lib("set_group_member", move |l| {
            let (_, token) = l.get_paper(&paper_id)?;
            if member {
                l.add_to_group(&paper_id, &group_id, &token).map(|_| ())
            } else {
                l.remove_from_group(&paper_id, &group_id, &token)
                    .map(|_| ())
            }
        })
        .await
}
