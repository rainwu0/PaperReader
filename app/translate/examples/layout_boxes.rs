//! 電腦上量分段準確度用：載入版面模型一次，從標準輸入一行讀一張圖，交回方框。
//! 和應用走同一段程式（`layout_model.rs`：前處理、類別表、門檻）。
//!
//! 執行（在 `app` 底下）：
//!   cargo run --release -p paperreader-translate --features layout-model --example layout_boxes -- <模型檔>
//! 每行輸入：`<800×800 的 RGB 原始位元組檔>\t<頁寬>\t<頁高>`（頁寬高用 PDF 點）。
//! 每行輸出：方框的 JSON 陣列（`[{label, score, bbox}]`，左上為原點、PDF 點）；出錯時是 `{"error": "..."}`。

use paperreader_translate::layout_model::{DEFAULT_THRESHOLD, LayoutModel};
use std::io::{BufRead, Write};
use std::path::Path;

fn main() {
    let Some(model) = std::env::args().nth(1) else {
        eprintln!("usage: layout_boxes <model.onnx>");
        std::process::exit(2);
    };
    let mut m = match LayoutModel::load(Path::new(&model), 0) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("load: {e}");
            std::process::exit(1);
        }
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let parts: Vec<&str> = line.split('\t').collect();
        let reply = match parts.as_slice() {
            [path, w, h] => match (std::fs::read(path), w.parse::<f32>(), h.parse::<f32>()) {
                (Ok(rgb), Ok(w), Ok(h)) => match m.detect(&rgb, w, h, DEFAULT_THRESHOLD) {
                    Ok(boxes) => serde_json::to_string(&boxes).unwrap_or_else(|_| "[]".into()),
                    Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
                },
                _ => serde_json::json!({ "error": "bad input line" }).to_string(),
            },
            _ => serde_json::json!({ "error": "bad input line" }).to_string(),
        };
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}
