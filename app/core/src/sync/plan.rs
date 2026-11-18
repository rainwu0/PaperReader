//! 計畫檔 `.pr-sync-<代號>/plan.json` 的格式。執行在 `local/sync_import.rs`。
//!
//! 計畫檔寫成的那一刻就是確定要做；之後每一項都是改名或附加小量的行，每一項都認得出已經做過，
//! 所以中斷之後可以從頭重做一遍。

use super::merge::trash::Place;
use crate::changes::ChangeRecord;
use crate::store::{SkippedPaper, SyncItem, SyncNotice};
use serde::{Deserialize, Serialize};

pub const PLAN_FILE: &str = "plan.json";
pub const SYNC_TEMP_PREFIX: &str = ".pr-sync-";

/// 計畫的一項。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum PlanItem {
    /// 附加記錄：解決衝突要寫的記錄附加到這台的變更紀錄檔；再依 4.9 在這一刻重讀本機的紀錄檔，
    /// 附加 `out/logs/` 裡匯入的每一份還沒有的行。
    Append {
        records: Vec<ChangeRecord>,
        logs: Vec<String>,
    },
    /// 把 `source`（暫存裡組好的資料夾）改名成 `papers/<name>` 或 `.trash/<name>`。
    NewPaper {
        paper: String,
        to: Place,
        name: String,
        source: String,
        /// 組好的資料夾裡每一個檔與它的指紋（改名之前驗）。
        #[serde(default)]
        files: Vec<StagedFile>,
    },
    /// 把那篇的資料夾在 `papers/` 與 `.trash/` 之間移動。
    Move {
        paper: String,
        to: Place,
        name: String,
    },
    /// 把暫存裡的 `source` 改名蓋過目標。`paper` 有值時 `target` 相對於那篇的資料夾（執行當下才找），
    /// 否則相對於根目錄。`before` 是寫之前的指紋（不存在是 `null`），`after` 是預期的結果。
    Write {
        paper: Option<String>,
        target: String,
        source: String,
        before: Option<String>,
        after: String,
        /// 是筆記檔：計畫之後被別處改過時，把算好的內容另存成衝突副本。
        #[serde(default)]
        notes: bool,
    },
    /// 合併後的刪除記錄拿掉了這篇的一項材料（`material-removed`）：那篇的 `paper.json` 寫好之後，把本機的材料檔
    /// 搬進回收區，同 `remove_material`。`paper.json` 還列著它、或檔已經不在（做過了）時不搬。
    TrashMaterial {
        paper: String,
        material: Box<crate::format::Material>,
    },
    /// 合併後這篇最晚的位置記錄是 `paper-purged`、兩邊都不在目錄裡：本機回收區裡這篇的每一個資料夾都刪掉。
    /// 目錄裡有這篇（計畫之後被放回）時不刪；已經不在時什麼都不做。
    PurgeTrash { paper: String },
    /// 匯入寫完之後的 `sync-imported` 記錄。
    SyncImported { record: ChangeRecord },
    /// 收尾：刪掉暫存資料夾、重新掃描、快速檢查。
    Finish,
}

/// 暫存裡的一個檔與它預期的指紋。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagedFile {
    pub path: String,
    pub sha256: String,
}

/// 回報要用的內容（重開之後重做也要交得出回報）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanReport {
    pub added: Vec<SyncItem>,
    pub updated: Vec<SyncItem>,
    pub removed: Vec<SyncItem>,
    pub conflict_copies: Vec<String>,
    pub skipped_papers: Vec<SkippedPaper>,
    pub notices: Vec<SyncNotice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub format: u32,
    pub package: String,
    /// 匯出那台裝置（衝突副本的檔名用）。
    pub from_device: String,
    pub items: Vec<PlanItem>,
    pub report: PlanReport,
}
