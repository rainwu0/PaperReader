//! 查詢閱讀狀況：每篇文獻的狀況、繼續閱讀。
//!
//! 狀況只存在記憶體裡，由事件整理結果加上目錄裡的材料清單即時算出；
//! 文獻在不在目錄裡、材料還在不在、頁數是多少，都在查詢時才對照。

use super::{LocalLibrary, find_folder};
use crate::error::{LibError, Result};
use crate::format::PaperFile;
use crate::progress::{MaterialInfo, ReadStatus, ReadingProgress, compute};

impl LocalLibrary {
    /// 這篇文獻目前的閱讀狀況。文獻不在目錄裡（包括已在回收區）交回 `NotFound`。
    pub fn reading_progress(&self, paper_id: &str) -> Result<ReadingProgress> {
        find_folder(&self.folders, paper_id)
            .and_then(|f| f.paper.as_ref())
            .map(|p| self.progress_of(p))
            .ok_or_else(|| LibError::NotFound {
                what: format!("paper {paper_id}"),
            })
    }

    /// 「繼續閱讀」：最多 `limit` 篇文獻的代號，依最近一次閱讀的時間由新到舊，時間相同依文獻代號。
    /// 條件：在目錄裡、狀態是讀到一半、有閱讀位置。只打開過、沒有留下可用位置的不算。
    pub fn continue_reading(&self, limit: usize) -> Vec<String> {
        let mut found: Vec<(String, String)> = self
            .catalog()
            .into_iter()
            .filter_map(|(_, paper)| {
                let pr = self.progress_of(paper);
                (pr.status == ReadStatus::Reading && pr.position.is_some())
                    .then(|| (pr.last_read_at.unwrap_or_default(), paper.id.clone()))
            })
            .collect();
        // 時間新的在前；時間相同，代號小的在前
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        found.into_iter().take(limit).map(|(_, id)| id).collect()
    }

    pub(crate) fn progress_of(&self, paper: &PaperFile) -> ReadingProgress {
        let materials: Vec<MaterialInfo<'_>> = paper
            .materials
            .iter()
            .map(|m| MaterialInfo {
                id: &m.id,
                pages: m.pages,
            })
            .collect();
        compute(self.events.events_of(&paper.id), &materials)
    }
}
