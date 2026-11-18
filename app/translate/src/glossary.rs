//! 術語庫：使用者自己的「原文詞 → 譯法」對照。
//!
//! 交換格式是三欄 TSV（原文、譯法、備註），和 OmegaT 的術語表相同，也能直接轉成 DeepL 術語表的條目
//! 存放與畫面在外殼；這裡只有格式與「這一批用得到哪些」。

use serde::{Deserialize, Serialize};

/// 一條術語。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

/// 讀 TSV 的結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub terms: Vec<Term>,
    /// 讀不出原文或譯法、沒有收進來的行數（空行與 `#` 開頭的註解不算）。
    pub skipped: usize,
}

/// 讀三欄 TSV：原文、譯法、備註（備註可以沒有）。空行與 `#` 開頭的行略過；
/// 同一個原文詞出現兩次，留後面那一條。
pub fn parse_tsv(text: &str) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Parsed::default();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let source = cols.next().unwrap_or("").trim().to_string();
        let target = cols.next().unwrap_or("").trim().to_string();
        let note = cols.next().unwrap_or("").trim().to_string();
        if source.is_empty() || target.is_empty() {
            out.skipped += 1;
            continue;
        }
        merge(
            &mut out.terms,
            Term {
                source,
                target,
                note,
            },
        );
    }
    out
}

/// 加進清單：原文詞相同（不分大小寫）的就取代。交回 `true` 表示是取代既有的。
pub fn merge(list: &mut Vec<Term>, t: Term) -> bool {
    if let Some(old) = list
        .iter_mut()
        .find(|o| o.source.to_lowercase() == t.source.to_lowercase())
    {
        *old = t;
        true
    } else {
        list.push(t);
        false
    }
}

/// 寫成三欄 TSV（UTF-8，換行 `\n`）。欄位裡的定位與換行字元換成空白，免得把欄位切壞。
pub fn to_tsv(terms: &[Term]) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut s = String::new();
    for t in terms {
        s.push_str(&clean(&t.source));
        s.push('\t');
        s.push_str(&clean(&t.target));
        if !t.note.is_empty() {
            s.push('\t');
            s.push_str(&clean(&t.note));
        }
        s.push('\n');
    }
    s
}

/// 這一批原文用得到的術語（原文詞出現在任一段裡，不分大小寫）。照原本的順序。
pub fn relevant<'a>(glossary: &'a [(String, String)], texts: &[&str]) -> Vec<&'a (String, String)> {
    if glossary.is_empty() {
        return Vec::new();
    }
    let lower: Vec<String> = texts.iter().map(|t| t.to_lowercase()).collect();
    glossary
        .iter()
        .filter(|(from, _)| {
            let f = from.to_lowercase();
            !f.trim().is_empty() && lower.iter().any(|t| t.contains(&f))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_three_columns_and_skips_bad_lines() {
        let p = parse_tsv(
            "\u{feff}# comment\nattention\t注意力\tNLP\r\nencoder\t編碼器\n\nonly-one-column\nAttention\t關注\n",
        );
        assert_eq!(p.skipped, 1);
        assert_eq!(
            p.terms,
            vec![
                Term {
                    source: "Attention".into(),
                    target: "關注".into(),
                    note: String::new()
                },
                Term {
                    source: "encoder".into(),
                    target: "編碼器".into(),
                    note: String::new()
                },
            ],
            "the later duplicate wins, in the place of the first"
        );
    }

    #[test]
    fn writing_and_reading_back_round_trips() {
        let terms = vec![
            Term {
                source: "a\tb".into(),
                target: "甲".into(),
                note: "n".into(),
            },
            Term {
                source: "c".into(),
                target: "丙".into(),
                note: String::new(),
            },
        ];
        let s = to_tsv(&terms);
        assert_eq!(s, "a b\t甲\tn\nc\t丙\n");
        assert_eq!(parse_tsv(&s).terms.len(), 2);
    }

    #[test]
    fn only_terms_in_the_text_are_relevant() {
        let g = vec![
            ("LoRA".to_string(), "低秩適應".to_string()),
            ("GAN".to_string(), "生成對抗網路".to_string()),
        ];
        let r = relevant(&g, &["We fine-tune with lora adapters."]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1, "低秩適應");
        assert!(relevant(&g, &["nothing"]).is_empty());
    }
}
