//! 譯本的逐段對照匯出成翻譯業界的標準格式，給專業翻譯軟體（OmegaT、Trados、memoQ……）用。
//!
//! - XLIFF 2.1（OASIS 標準，ISO 21720）：核心命名空間仍是 `urn:oasis:names:tc:xliff:document:2.0`，
//!   `srcLang` 必填，有 `<target>` 時 `trgLang` 必填；`<file>`、`<unit>` 的 `id` 必填。
//! - TMX 1.4b Level 1：只放純文字的原文與譯文對照，只收翻好的段落。
//!
//! 狀態只在匯出時對到 XLIFF 的狀態值（我們的格式不配合外部）：`pending` → `initial`（沒有譯文）、
//! `done` → `translated`、`failed` → `initial` 加子狀態 `pr:failed`。手寫 XML，不加相依。

use paperreader_core::overlay::{OverlayFile, STATUS_DONE, STATUS_FAILED};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Xliff,
    Tmx,
}

/// 原文語言沒記（讓服務自己判斷）時當作英文：論文的原文目前一律是英文。
fn src_lang(doc: &OverlayFile) -> &str {
    doc.source_lang
        .as_deref()
        .filter(|l| !l.is_empty())
        .unwrap_or("en")
}

/// XML 的文字與屬性值：跳脫特殊字元，去掉 XML 不允許的控制字元（PDF 抽出的文字偶爾會有）。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// `id` 屬性用的代號（NMTOKEN）：只留英數與 `-_.`，其餘換成 `_`。
fn token(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if t.is_empty() { "_".to_string() } else { t }
}

pub fn xliff(doc: &OverlayFile, title: &str, material_id: &str) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<xliff xmlns=\"urn:oasis:names:tc:xliff:document:2.0\" version=\"2.1\" srcLang=\"{}\" trgLang=\"{}\">\n",
        esc(src_lang(doc)),
        esc(&doc.target_lang)
    ));
    s.push_str(&format!(
        "  <file id=\"{}\" original=\"{}\">\n",
        token(material_id),
        esc(title)
    ));
    for g in &doc.segments {
        let id = token(&g.id);
        s.push_str(&format!("    <unit id=\"{id}\">\n"));
        let target = g.target.as_deref().filter(|t| !t.is_empty());
        let state = match (g.status.as_str(), target) {
            (STATUS_DONE, Some(_)) => " state=\"translated\"".to_string(),
            (STATUS_FAILED, _) => " state=\"initial\" subState=\"pr:failed\"".to_string(),
            _ => " state=\"initial\"".to_string(),
        };
        s.push_str(&format!("      <segment id=\"{id}\"{state}>\n"));
        s.push_str(&format!("        <source>{}</source>\n", esc(&g.source)));
        if let (STATUS_DONE, Some(t)) = (g.status.as_str(), target) {
            s.push_str(&format!("        <target>{}</target>\n", esc(t)));
        }
        s.push_str("      </segment>\n    </unit>\n");
    }
    s.push_str("  </file>\n</xliff>\n");
    s
}

pub fn tmx(doc: &OverlayFile) -> String {
    let src = esc(src_lang(doc));
    let tgt = esc(&doc.target_lang);
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<tmx version=\"1.4\">\n");
    s.push_str(&format!(
        "  <header creationtool=\"PaperReader\" creationtoolversion=\"{}\" segtype=\"paragraph\" o-tmf=\"PaperReader\" adminlang=\"en\" srclang=\"{src}\" datatype=\"plaintext\"/>\n",
        crate::commands::app::public_version()
    ));
    s.push_str("  <body>\n");
    for g in &doc.segments {
        let Some(t) = g.target.as_deref().filter(|t| !t.is_empty()) else {
            continue;
        };
        if g.status != STATUS_DONE {
            continue;
        }
        s.push_str(&format!("    <tu tuid=\"{}\">\n", esc(&g.id)));
        s.push_str(&format!(
            "      <tuv xml:lang=\"{src}\"><seg>{}</seg></tuv>\n",
            esc(&g.source)
        ));
        s.push_str(&format!(
            "      <tuv xml:lang=\"{tgt}\"><seg>{}</seg></tuv>\n",
            esc(t)
        ));
        s.push_str("    </tu>\n");
    }
    s.push_str("  </body>\n</tmx>\n");
    s
}

/// 檔名（畫面交給匯出的流程時會再整理成合法的檔名）。
pub fn file_name(title: &str, format: Format) -> String {
    let ext = match format {
        Format::Xliff => "xlf",
        Format::Tmx => "tmx",
    };
    format!("{title}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> OverlayFile {
        serde_json::from_value(serde_json::json!({
            "format": 1, "source_material": "m1", "provider": "anthropic", "model": "x",
            "source_lang": null, "target_lang": "zh-TW", "created": null, "modified": null,
            "segments": [
                {"id": "s1", "page": 0, "bbox": [0,0,1,1], "kind": "body", "size": null,
                 "source": "A < B & \"C\"\u{0001}", "target": "甲 < 乙", "status": "done", "error": null},
                {"id": "s2", "page": 0, "bbox": [0,0,1,1], "kind": "body", "size": null,
                 "source": "Pending", "target": null, "status": "pending", "error": null},
                {"id": "s3", "page": 0, "bbox": [0,0,1,1], "kind": "body", "size": null,
                 "source": "Failed", "target": null, "status": "failed", "error": "Network"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn xliff_maps_states_only_on_export() {
        let x = xliff(&doc(), "T & T", "mat 1");
        assert!(x.contains(
            "xmlns=\"urn:oasis:names:tc:xliff:document:2.0\" version=\"2.1\" srcLang=\"en\" trgLang=\"zh-TW\""
        ));
        assert!(x.contains("<file id=\"mat_1\" original=\"T &amp; T\">"));
        assert!(x.contains("<segment id=\"s1\" state=\"translated\">"));
        assert!(
            x.contains("<source>A &lt; B &amp; &quot;C&quot;</source>"),
            "control characters dropped"
        );
        assert!(x.contains("<target>甲 &lt; 乙</target>"));
        assert!(x.contains("<segment id=\"s2\" state=\"initial\">\n        <source>Pending</source>\n      </segment>"));
        assert!(x.contains("state=\"initial\" subState=\"pr:failed\""));
        assert_eq!(x.matches("<target>").count(), 1);
    }

    #[test]
    fn tmx_has_only_translated_pairs() {
        let t = tmx(&doc());
        assert!(t.contains("<tmx version=\"1.4\">"));
        assert!(t.contains("srclang=\"en\""));
        assert_eq!(t.matches("<tu ").count(), 1);
        assert!(t.contains("<tuv xml:lang=\"zh-TW\"><seg>甲 &lt; 乙</seg></tuv>"));
    }
}
