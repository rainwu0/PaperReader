//! 一份來源切成單位。索引與找位置共用，兩邊的單位文字就相同。

use paperreader_core::overlay::STATUS_DONE;
use paperreader_core::store::IndexSource;

/// 單位的種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    /// 原文的一頁。
    Source,
    /// 譯文的一段（疊加檔裡已翻、有譯文的段落）。
    Translation,
    /// 重點筆記的一點。
    Digest,
    /// 筆記檔的一條劃線，或自由書寫的一段。
    Note,
}

impl UnitKind {
    pub fn code(self) -> i64 {
        match self {
            UnitKind::Source => 0,
            UnitKind::Translation => 1,
            UnitKind::Digest => 2,
            UnitKind::Note => 3,
        }
    }

    pub fn from_code(c: i64) -> Option<Self> {
        Some(match c {
            0 => UnitKind::Source,
            1 => UnitKind::Translation,
            2 => UnitKind::Digest,
            3 => UnitKind::Note,
            _ => return None,
        })
    }

    /// 交給畫面的名稱。
    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Source => "source",
            UnitKind::Translation => "translation",
            UnitKind::Digest => "digest",
            UnitKind::Note => "note",
        }
    }
}

/// 一個指得回去的地方。
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub kind: UnitKind,
    /// 原文：原文的材料；譯文：那一份疊加檔；重點筆記：那一份重點筆記；劃線：劃在哪一份材料；自由書寫：沒有。
    pub material: Option<String>,
    /// 頁序，從 0 起；自由書寫沒有。
    pub page: Option<u32>,
    /// 段落代號、點的代號、劃線代號；自由書寫是 `t<段落序>`；原文沒有。
    pub target: Option<String>,
    /// 譯文那一段的方框（PDF 頁面座標）。
    pub bbox: Option<[f64; 4]>,
    pub text: String,
}

impl Unit {
    /// 同一份來源裡認得出這個單位：原文看頁序，其餘看代號。
    pub fn same_place(&self, kind: UnitKind, page: Option<u32>, target: Option<&str>) -> bool {
        self.kind == kind
            && match kind {
                UnitKind::Source => self.page == page,
                _ => self.target.as_deref() == target,
            }
    }
}

/// 鍵裡 `:` 後面的材料代號（`overlay:<材料>`、`digest:<材料>`）。
fn key_material(key: &str) -> Option<String> {
    key.split_once(':').map(|(_, m)| m.to_string())
}

/// 自由書寫以空行分開的每一段（去頭尾空白之後不是空的）。
fn paragraphs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in s.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(cur.join("\n"));
                cur.clear();
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur.join("\n"));
    }
    out
}

/// 一份來源切成單位。沒有字的單位不列。
pub fn units_of(key: &str, src: &IndexSource) -> Vec<Unit> {
    let mut out = Vec::new();
    match src {
        IndexSource::Text(t) => {
            for p in &t.pages {
                out.push(Unit {
                    kind: UnitKind::Source,
                    material: Some(t.extraction.source_material.clone()),
                    page: Some(p.page),
                    target: None,
                    bbox: None,
                    text: p.text.clone(),
                });
            }
        }
        IndexSource::Overlay(o) => {
            let material = key_material(key);
            for s in &o.segments {
                if s.status != STATUS_DONE {
                    continue;
                }
                if let Some(target) = &s.target {
                    out.push(Unit {
                        kind: UnitKind::Translation,
                        material: material.clone(),
                        page: Some(s.page),
                        target: Some(s.id.clone()),
                        bbox: Some(s.bbox),
                        text: target.clone(),
                    });
                }
            }
        }
        IndexSource::Digest(d) => {
            let material = key_material(key);
            for p in &d.points {
                out.push(Unit {
                    kind: UnitKind::Digest,
                    material: material.clone(),
                    page: p.pages.first().copied(),
                    target: Some(p.id.clone()),
                    bbox: None,
                    text: p.text.clone(),
                });
            }
        }
        IndexSource::Notes(n) => {
            for h in &n.highlights {
                let text = if h.comment.trim().is_empty() {
                    h.quote.exact.clone()
                } else {
                    format!("{}\n{}", h.quote.exact, h.comment)
                };
                out.push(Unit {
                    kind: UnitKind::Note,
                    material: Some(h.material.clone()),
                    page: Some(h.page.index),
                    target: Some(h.id.clone()),
                    bbox: h.rects.first().copied(),
                    text,
                });
            }
            for (i, p) in paragraphs(&n.thoughts).into_iter().enumerate() {
                out.push(Unit {
                    kind: UnitKind::Note,
                    material: None,
                    page: None,
                    target: Some(format!("t{i}")),
                    bbox: None,
                    text: p,
                });
            }
        }
    }
    out.retain(|u| !u.text.trim().is_empty());
    out
}
