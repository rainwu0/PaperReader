//! 外掛「內文搜尋」的 Rust 一側：切詞、查詢的解析、一份來源切成單位、找位置，以及 SQLite
//! FTS5 的索引。依賴核心（正規化 `fold_text_mapped`、來源的型別）與 rusqlite，不依賴 Tauri；外殼的 `ext.rs` 是唯一
//! 引用它的地方。

mod index;
mod locate;
mod query;
mod tokenize;
mod units;

pub use index::{
    CANDIDATES_PER_PAPER, FulltextIndex, INDEX_FILE, INDEX_VERSION, Ranked, RankedPaper, Stamps,
    UnitHit, is_corrupt, user_version,
};
pub use locate::{CONTEXT_AFTER, CONTEXT_BEFORE, Located, locate};
pub use query::{MIN_QUERY_CHARS, Query, Term, parse_query};
/// 索引交回的錯誤（SQLite），外殼不必另外依賴 rusqlite。
pub use rusqlite::Error as SqlError;
pub use tokenize::{Token, Tokens, tokens};
pub use units::{Unit, UnitKind, units_of};
