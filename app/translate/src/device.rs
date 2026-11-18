//! 裝置上的引擎（離線翻譯）：Firefox 翻譯（Bergamot，在網頁元件裡跑 WASM）。
//!
//! 這兩個引擎不在 Rust 裡，所以這個套件只定一個「把一批字交出去、拿回譯文」的接口 [`DeviceTranslator`]，
//! 由外殼實作（轉給外掛或畫面）。工作的推進（分批、自動檢查、接著翻、寫檔）照樣由 [`crate::run_job`] 負責，
//! 和雲端引擎走同一條路。

use crate::Params;
use crate::error::Result;
use std::future::Future;
use std::pin::Pin;

pub type DeviceFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

/// 外殼提供的裝置上的引擎。交回的譯文要和 `texts` 一樣多、一樣的順序。
///
/// 錯誤的種類照 [`crate::ErrorKind`]：模型還沒下載是 `ModelMissing`、語言組合不支援是 `UnsupportedLanguage`（整個工作停下來）；
/// 引擎沒有回應（例如畫面還沒準備好）是 `Network`（等一下重試）。
pub trait DeviceTranslator: Send + Sync {
    fn translate<'a>(&'a self, params: &'a Params, texts: Vec<String>) -> DeviceFuture<'a>;
}
