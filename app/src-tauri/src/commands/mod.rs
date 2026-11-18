//! 畫面呼叫的指令，依主題分檔。每個指令一對一轉給核心的操作，交回核心的資料，不加工，
//! 不含資料規則。

pub mod add;
pub mod app;
pub mod backup;
pub mod digest;
pub mod export;
pub mod generated;
pub mod groups;
pub mod import;
pub mod ink;
pub mod layout;
pub mod library;
pub mod notes;
pub mod ondevice;
pub mod papers;
pub mod reading;
pub mod references;
pub mod sync;
pub mod text;
pub mod thumbs;
pub mod translate;
pub mod update;
