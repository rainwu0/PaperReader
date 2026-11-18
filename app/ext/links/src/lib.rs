//! 外掛：文獻之間的連結，Rust 一側。
//!
//! 比對（`matching`）與連結的索引（`index`）。只依賴核心公開的格式與純函式（`fold_text`、
//! `resolve_judgments`）；不讀寫文獻庫，資料由外殼經核心的操作讀出來交進來（`PaperInput`）。外殼裡只有 `ext.rs`
//! 引用這個 crate（檢查腳本 `scripts/ext-boundary.mjs` 確認）。

pub mod index;
pub mod matching;

pub use index::{
    Candidate, CitingPaper, EntryView, ExtractState, INDEX_VERSION, LinkIndex, LinkTo, LinksView,
    LostJudgment, PaperInput, Rejected,
};
pub use matching::By;
