//! 匯出譯本：把譯文疊加檔輸出成 Markdown。兩種：
//!
//! - 只有譯文（`translation_text_markdown`，預設）：給別人讀。依頁序、同一頁照檔案裡的順序，不分頁；
//!   標題段落寫成 `##`；還沒翻、翻失敗的段落照原文列出，不空著。
//! - 原文與譯文對照（`translation_markdown`）：每一頁一個標題（頁序 + 1），頁內每一段先列原文（引用區塊），
//!   再列譯文。還沒翻的段落、翻失敗的段落照樣列出原文，譯文處標明。
//!
//! 純計算。

use crate::overlay::{OverlayFile, OverlaySegment};

/// 還沒翻、翻失敗時譯文處的說明。
const NOT_TRANSLATED: &str = "（尚未翻譯）";
const FAILED: &str = "（翻譯失敗）";

/// 段落種類裡的標題（`kind`，核心不另外解讀，只在匯出時排成標題）。
const KIND_HEADING: &str = "heading";

pub fn translation_markdown(title: &str, file: &OverlayFile) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title_or(title, "譯本對照")));
    if let Some(meta) = meta_line(file) {
        out.push_str(&format!("{meta}\n\n"));
    }

    let mut current: Option<u32> = None;
    for s in in_page_order(file) {
        if current != Some(s.page) {
            out.push_str(&format!("## 第 {} 頁\n\n", s.page + 1));
            current = Some(s.page);
        }
        for line in s.source.lines() {
            if line.trim().is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str(&format!("> {line}\n"));
            }
        }
        out.push('\n');
        match (target_of(s), s.status.as_str()) {
            (Some(t), _) => out.push_str(t),
            (None, "failed") => out.push_str(FAILED),
            (None, _) => out.push_str(NOT_TRANSLATED),
        }
        out.push_str("\n\n");
    }
    out
}

/// 只有譯文：開頭的篇名與「翻譯：…」之後，每一段的譯文一個段落（空一行分開）；標題段落是 `## `。
/// 沒有譯文的段落照原文列出。結尾一個換行。
pub fn translation_text_markdown(title: &str, file: &OverlayFile) -> String {
    let mut blocks = vec![format!("# {}", title_or(title, "譯文"))];
    blocks.extend(meta_line(file));
    for s in in_page_order(file) {
        let text = target_of(s).unwrap_or_else(|| s.source.trim());
        if text.is_empty() {
            continue;
        }
        if s.kind == KIND_HEADING {
            blocks.push(format!("## {}", text.replace('\n', " ")));
        } else {
            blocks.push(text.to_string());
        }
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

fn title_or<'a>(title: &'a str, fallback: &'a str) -> &'a str {
    let title = title.trim();
    if title.is_empty() { fallback } else { title }
}

/// 「翻譯：引擎 模型；目標語言：…」；兩項都不知道時沒有這一行。
fn meta_line(file: &OverlayFile) -> Option<String> {
    let mut meta = Vec::new();
    if !file.provider.is_empty() {
        let who = if file.model.is_empty() {
            file.provider.clone()
        } else {
            format!("{} {}", file.provider, file.model)
        };
        meta.push(format!("翻譯：{who}"));
    }
    if !file.target_lang.is_empty() {
        meta.push(format!("目標語言：{}", file.target_lang));
    }
    (!meta.is_empty()).then(|| meta.join("；"))
}

/// 頁序由小到大；同一頁內保持檔案裡的順序（穩定排序）。
fn in_page_order(file: &OverlayFile) -> Vec<&OverlaySegment> {
    let mut segs: Vec<_> = file.segments.iter().collect();
    segs.sort_by_key(|s| s.page);
    segs
}

/// 去掉頭尾空白後不是空的譯文。
fn target_of(s: &OverlaySegment) -> Option<&str> {
    s.target.as_deref().map(str::trim).filter(|t| !t.is_empty())
}
