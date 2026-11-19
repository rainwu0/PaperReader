//! 接續：一個句子跨欄或跨頁，被版面分析切成前後兩段時，後一段是前一段的接續。
//!
//! 版面分析換欄、換頁一定切開，所以一句話可能分在兩段，中間還可能夾著圖說。這裡只看段落的位置、種類與原文，
//! 認出這樣的前後兩段：翻譯時兩段同批送出並註明接續，譯文檢視把兩段接成一段。
//! 只認版面模型分段的頁：只用規則分段時，圖裡的字常被當成本文，接上去比少接一句更糟。

/// 認接續用的一段。
#[derive(Debug, Clone, Copy)]
pub struct Placed<'a> {
    /// 頁序。
    pub page: u32,
    /// `[x0, y0, x1, y1]`，PDF 頁面座標。
    pub bbox: [f64; 4],
    /// 種類（`body`、`caption`、`heading`……）；空字串當作 `body`。
    pub kind: &'a str,
    /// 主要的字級（點）；不知道是 `None`。
    pub size: Option<f64>,
    /// 這一段所在的頁是用版面模型分段的。
    pub by_model: bool,
    pub source: &'a str,
}

fn is_body(kind: &str) -> bool {
    kind.is_empty() || kind == "body"
}

/// 以句末標點結束（後面可以再接一個右引號或右括號）。
fn ends_sentence(s: &str) -> bool {
    let mut it = s.trim_end().chars().rev();
    let last = it.next();
    let last = match last {
        Some('"' | '\'' | '’' | '”' | ')' | ']') => it.next(),
        c => c,
    };
    matches!(last, Some('.' | '!' | '?' | ':' | ';'))
}

fn has_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("http://") || l.contains("https://") || l.contains("www.")
}

/// 以小寫的英文字母開頭，而且不是網址。
fn starts_lower(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with(|c: char| c.is_ascii_lowercase())
        && !t.to_ascii_lowercase().starts_with("http")
        && !t.to_ascii_lowercase().starts_with("www.")
}

/// 兩段的字級差不到一成（圖裡的字常被當成本文，字級和內文不同）；有一段不知道字級時不比。
fn same_size(a: &Placed, b: &Placed) -> bool {
    match (a.size, b.size) {
        (Some(x), Some(y)) => (x - y).abs() <= 0.1 * x.max(y),
        _ => true,
    }
}

/// 換了頁，或換了欄（兩段左右不重疊）。
fn moved(a: &Placed, b: &Placed) -> bool {
    a.page != b.page || a.bbox[2].min(b.bbox[2]) - a.bbox[0].max(b.bbox[0]) <= 0.0
}

/// 找出接續：交回（前一段, 後一段）的位置，依前一段的順序。
///
/// - 前一段是本文、不以句末標點結束、不含網址；
/// - 中間只隔著圖說（跳過）；
/// - 後一段是本文、以小寫字母開頭；
/// - 兩段換了頁或換了欄，字級差不到一成；
/// - 兩段所在的頁都是用版面模型分段的。
pub fn continuations(segs: &[Placed]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, a) in segs.iter().enumerate() {
        if !a.by_model || !is_body(a.kind) || ends_sentence(a.source) || has_url(a.source) {
            continue;
        }
        let mut j = i + 1;
        while j < segs.len() && segs[j].kind == "caption" {
            j += 1;
        }
        let Some(b) = segs.get(j) else { continue };
        if b.by_model && is_body(b.kind) && starts_lower(b.source) && moved(a, b) && same_size(a, b)
        {
            out.push((i, j));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p<'a>(page: u32, x0: f64, kind: &'a str, source: &'a str) -> Placed<'a> {
        Placed {
            page,
            bbox: [x0, 100.0, x0 + 240.0, 300.0],
            kind,
            size: Some(10.0),
            by_model: true,
            source,
        }
    }

    #[test]
    fn pages_segmented_by_rules_have_no_continuations() {
        // 只用規則分段的頁：圖裡的字常被當成本文，不認接續；一對裡有一頁是規則分的也不算
        let rules = |q: Placed<'static>| Placed {
            by_model: false,
            ..q
        };
        let a = p(
            0,
            50.0,
            "body",
            "Our intuition is that, at least locally, the robot",
        );
        let b = p(1, 50.0, "body", "should act in a consistent way.");
        assert_eq!(continuations(&[a, b]), vec![(0, 1)]);
        assert!(continuations(&[rules(a), rules(b)]).is_empty());
        assert!(continuations(&[a, rules(b)]).is_empty());
        assert!(continuations(&[rules(a), b]).is_empty());
    }

    #[test]
    fn text_of_a_different_size_is_not_a_continuation() {
        // 圖裡的字（小一號）被當成本文時，不接到旁邊一欄的本文；字級不知道時照舊
        let small = Placed {
            size: Some(8.5),
            ..p(1, 50.0, "body", "local neighborhood the robot")
        };
        let next = p(1, 320.0, "body", "state sensitivity at one state");
        assert!(continuations(&[small, next]).is_empty());
        let close = Placed {
            size: Some(9.5),
            ..small
        };
        assert_eq!(continuations(&[close, next]), vec![(0, 1)]);
        let unknown = Placed {
            size: None,
            ..small
        };
        assert_eq!(continuations(&[unknown, next]), vec![(0, 1)]);
    }

    #[test]
    fn a_sentence_split_across_columns_or_pages_is_found() {
        let segs = [
            p(
                0,
                50.0,
                "body",
                "We train the policy so that, at least locally, the robot",
            ),
            p(
                0,
                320.0,
                "body",
                "should act in a consistent way across nearby states.",
            ),
            p(
                0,
                320.0,
                "body",
                "The second half of the analysis is given in",
            ),
            p(1, 50.0, "body", "the appendix, together with the proofs."),
        ];
        assert_eq!(
            continuations(&segs),
            vec![(0, 1), (2, 3)],
            "a sentence that starts with a capital is not a continuation"
        );
        let segs = [
            p(0, 50.0, "body", "Our intuition is that the robot"),
            p(0, 320.0, "", "should act consistently."),
        ];
        assert_eq!(continuations(&segs), vec![(0, 1)], "an empty kind is body");
    }

    #[test]
    fn captions_in_between_are_skipped() {
        let segs = [
            p(
                0,
                50.0,
                "body",
                "Our intuition is that, at least locally, the robot",
            ),
            p(1, 50.0, "caption", "Fig. 1. Overview of the method."),
            p(1, 50.0, "caption", "Fig. 2. Results."),
            p(1, 50.0, "body", "should act in a consistent way."),
        ];
        assert_eq!(continuations(&segs), vec![(0, 3)]);
    }

    #[test]
    fn ends_of_sentences_same_column_headings_and_urls_are_not_continuations() {
        let cases: [[Placed; 2]; 6] = [
            // 前一段以句末標點（加右括號）結束
            [
                p(0, 50.0, "body", "This is the end (see Table 2)."),
                p(1, 50.0, "body", "and this is not a continuation."),
            ],
            [
                p(0, 50.0, "body", "He said “it works.”"),
                p(1, 50.0, "body", "and then left."),
            ],
            // 同一欄、同一頁：版面分析本來就會接起來的，不另算
            [
                p(0, 50.0, "body", "Our intuition is that the robot"),
                p(0, 60.0, "body", "should act consistently."),
            ],
            // 標題不算
            [
                p(0, 50.0, "heading", "Our intuition is that the robot"),
                p(1, 50.0, "body", "should act consistently."),
            ],
            // 註腳的網址
            [
                p(
                    0,
                    50.0,
                    "body",
                    "Code is available at https://example.org/code",
                ),
                p(1, 50.0, "body", "and the data too."),
            ],
            [
                p(0, 50.0, "body", "Code is available at"),
                p(1, 50.0, "body", "https://example.org/code for everyone."),
            ],
        ];
        for c in cases {
            assert!(continuations(&c).is_empty(), "{c:?}");
        }
    }

    #[test]
    fn a_three_part_sentence_chains() {
        let segs = [
            p(
                0,
                50.0,
                "body",
                "A very long sentence that runs from the left column",
            ),
            p(0, 320.0, "body", "through the right column and"),
            p(1, 50.0, "body", "onto the next page."),
        ];
        assert_eq!(continuations(&segs), vec![(0, 1), (1, 2)]);
    }
}
