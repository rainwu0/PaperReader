//! 筆跡：每篇資料夾裡的 `ink.json`（核心管格式）。
//!
//! 本機只有一個寫入者，所以每次寫入前才讀最新的版本記號，畫面不必保管記號
//! （和群組的指令同一做法）。

use crate::error::ApiResult;
use crate::state::AppState;
use paperreader_core::ink::{InkFile, Stroke};
use paperreader_core::store::StrokeDraft;
use serde::Deserialize;
use tauri::State;

/// 畫面送來的一筆新筆跡（核心的 `StrokeDraft` 沒有反序列化，所以在這裡轉）。
#[derive(Deserialize)]
pub struct StrokeInput {
    pub material: String,
    /// 頁序，從 0 起算。
    pub page_index: u32,
    pub color: String,
    /// 線寬，單位是 PDF 的點。
    pub width: f64,
    pub opacity: f64,
    /// PDF 頁面座標 `[x, y]`。
    pub points: Vec<[f64; 2]>,
}

impl From<StrokeInput> for StrokeDraft {
    fn from(d: StrokeInput) -> StrokeDraft {
        StrokeDraft {
            material: d.material,
            page_index: d.page_index,
            color: d.color,
            width: d.width,
            opacity: d.opacity,
            points: d.points,
        }
    }
}

#[tauri::command]
pub async fn read_ink(state: State<'_, AppState>, paper_id: String) -> ApiResult<InkFile> {
    state
        .lib("read_ink", move |l| {
            l.read_ink(&paper_id).map(|(doc, _)| doc)
        })
        .await
}

#[tauri::command]
pub async fn add_stroke(
    state: State<'_, AppState>,
    paper_id: String,
    draft: StrokeInput,
) -> ApiResult<Stroke> {
    state
        .lib("add_stroke", move |l| {
            let (_, token) = l.read_ink(&paper_id)?;
            l.add_stroke(&paper_id, draft.into(), token.as_ref())
                .map(|(stroke, _)| stroke)
        })
        .await
}

#[tauri::command]
pub async fn delete_stroke(
    state: State<'_, AppState>,
    paper_id: String,
    stroke_id: String,
) -> ApiResult<()> {
    state
        .lib("delete_stroke", move |l| {
            let (_, token) = l.read_ink(&paper_id)?;
            let token = token.ok_or_else(|| paperreader_core::error::LibError::NotFound {
                what: "ink file".to_string(),
            })?;
            l.delete_stroke(&paper_id, &stroke_id, &token).map(|_| ())
        })
        .await
}

#[tauri::command]
pub async fn clear_ink_page(
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    page_index: u32,
) -> ApiResult<()> {
    state
        .lib("clear_ink_page", move |l| {
            let (_, Some(token)) = l.read_ink(&paper_id)? else {
                return Ok(());
            };
            l.clear_ink_page(&paper_id, &material_id, page_index, &token)
                .map(|_| ())
        })
        .await
}
