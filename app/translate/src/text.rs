//! 送出前與寫入前的文字整理，翻譯與重點筆記共用：
//!
//! - 送出前濾掉控制字元：PDF 抽出的公式括號常是 `\u{0}`、`\u{1}`，讓 Firefox 翻譯回不出東西、整篇的譯本 PDF
//!   不自動產生。換行與定位字元留著。
//! - 譯成中文時，把中文旁邊的半形標點換成全形（約六成的段落用半形逗號）。數字、英文、公式、
//!   網址裡的不動；括號只在裡面有中文字時才換（括號裡只有英文的多半是引用，例如 `(Smith et al., 2020)`，留著）。

/// 去掉控制字元（換行、定位字元以外）。
pub fn strip_controls(s: &str) -> String {
    if !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return s.to_string();
    }
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// 目標語言是不是中文（繁體或簡體）。
pub fn is_chinese(lang: &str) -> bool {
    let l = lang.to_ascii_lowercase();
    l == "zh" || l.starts_with("zh-")
}

/// 中文字（表意文字）。
fn han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 中文字，或中日韓的標點與全形字（`，`、`。`、`「`、`）`……）。
fn cjk(c: char) -> bool {
    han(c) || matches!(c as u32, 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

/// 右引號、右括號：英文句子的標點緊接在它前面時，標點屬於引號裡的英文（`「Is it enough?」`），不換。
fn closing(c: char) -> bool {
    matches!(c, '」' | '』' | '）' | '】' | '》' | '〉')
}

fn is_space(c: char) -> bool {
    c == ' ' || c == '\u{3000}'
}

/// `i` 前面第一個不是空白的字的位置。
fn prev_solid(v: &[char], i: usize) -> Option<usize> {
    (0..i).rev().find(|&k| !is_space(v[k]))
}

/// `i` 後面第一個不是空白的字的位置。
fn next_solid(v: &[char], i: usize) -> Option<usize> {
    (i + 1..v.len()).find(|&k| !is_space(v[k]))
}

/// 換成全形的標點前後的空白拿掉（全形標點本身帶著間距）；開頭與結尾的空白留著。
fn trim_around(v: &[char], drop: &mut [bool], i: usize) {
    if let Some(p) = prev_solid(v, i) {
        (p + 1..i).for_each(|k| drop[k] = true);
    }
    if let Some(n) = next_solid(v, i) {
        (i + 1..n).for_each(|k| drop[k] = true);
    }
}

/// 中文旁邊的半形標點換成全形，並拿掉它前後的空白。沒有中文字的段落原樣交回。
pub fn fullwidth_punct(s: &str) -> String {
    if !s.chars().any(han) {
        return s.to_string();
    }
    let mut v: Vec<char> = s.chars().collect();
    // 要拿掉的空白
    let mut drop = vec![false; v.len()];
    // 網址整段不動（含中文的網址）：`http://`、`https://` 到下一個空白
    let mut url = vec![false; v.len()];
    for (i, _) in s.char_indices().filter(|(b, _)| {
        let rest = &s[*b..];
        rest.starts_with("http://") || rest.starts_with("https://")
    }) {
        let start = s[..i].chars().count();
        for k in start..v.len() {
            if v[k].is_whitespace() {
                break;
            }
            url[k] = true;
        }
    }

    // 括號：一對、中間沒有別的括號與換行、裡面有中文字
    let mut i = 0;
    while i < v.len() {
        if v[i] == '(' && !url[i] {
            let close = (i + 1..v.len().min(i + 302))
                .take_while(|&k| v[k] != '(' && v[k] != '\n')
                .find(|&k| v[k] == ')');
            if let Some(j) = close
                && v[i + 1..j].iter().any(|c| han(*c))
            {
                v[i] = '（';
                v[j] = '）';
                trim_around(&v, &mut drop, i);
                trim_around(&v, &mut drop, j);
                i = j;
            }
        }
        i += 1;
    }

    for i in 0..v.len() {
        let c = v[i];
        if url[i] {
            continue;
        }
        let full = match c {
            ',' => '，',
            ';' => '；',
            ':' => '：',
            '?' => '？',
            '!' => '！',
            '.' => '。',
            _ => continue,
        };
        let prev = prev_solid(&v, i).map(|k| v[k]);
        let next = next_solid(&v, i).map(|k| v[k]);
        let ok = if c == '.' {
            // 句號：緊接在中文字或中文標點後面；後面是結尾、空白、中文或右引號。
            // 刪節號（`...`）、小數、縮寫（`et al.`、`Fig.`）都不動
            // 中文數字的編號（`一. 緒論`）也不動
            let numbered = i > 0
                && "一二三四五六七八九十百零〇".contains(v[i - 1])
                && (i == 1 || v[i - 2].is_whitespace());
            i > 0
                && !numbered
                && cjk(v[i - 1])
                && v.get(i + 1).is_none_or(|n| {
                    *n != '.' && (n.is_whitespace() || cjk(*n) || matches!(n, '"' | '\'' | ')'))
                })
        } else if c == '!'
            && (v.get(i + 1) == Some(&'=') || (i > 0 && v[i - 1].is_ascii_alphanumeric()))
        {
            // `!=`、階乘 `n!` 不動
            false
        } else {
            prev.is_some_and(cjk) || next.is_some_and(|n| cjk(n) && !closing(n))
        };
        if ok {
            v[i] = full;
            trim_around(&v, &mut drop, i);
        }
    }
    v.iter()
        .zip(drop)
        .filter(|(_, d)| !d)
        .map(|(c, _)| *c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check_segment;

    #[test]
    fn control_characters_are_removed_but_line_breaks_stay() {
        assert_eq!(strip_controls("a\u{0}(x\u{1})\tb\nc\u{7f}"), "a(x)\tb\nc");
        assert_eq!(strip_controls("plain"), "plain");
    }

    #[test]
    fn half_width_punctuation_next_to_chinese_becomes_full_width() {
        // 兩句 Claude 的譯文與一句含數字、英文、括號的
        let cases = [
            (
                "先前的正則化方法將每個狀態單獨考量,並限制策略在該狀態下對任何擾動的敏感度。",
                "先前的正則化方法將每個狀態單獨考量，並限制策略在該狀態下對任何擾動的敏感度。",
            ),
            (
                "配備 6 億像素的鑲嵌式 CCD 相機,可提供 47 deg2 的廣闊視野(Dekany et al. 2020)。",
                "配備 6 億像素的鑲嵌式 CCD 相機，可提供 47 deg2 的廣闊視野(Dekany et al. 2020)。",
            ),
            (
                "在 COCO 上 53.1 AP, 比 YOLOv8 快 2 倍 (見表 3). 方法: LoRA; 結果如何? 很好!",
                "在 COCO 上 53.1 AP，比 YOLOv8 快 2 倍（見表 3）。方法：LoRA；結果如何？很好！",
            ),
            // 中文句子結尾、緊接右引號的照樣換
            ("他問:「可以嗎?」", "他問：「可以嗎？」"),
        ];
        for (from, to) in cases {
            assert_eq!(fullwidth_punct(from), to);
        }
    }

    #[test]
    fn numbers_english_formulas_and_urls_are_left_alone() {
        for s in [
            "共 1,000 筆，比例 3:1，時間 10:30，誤差 0.5%",
            "見 https://x.org/a?b=1 的說明",
            "其中 $a,b$ 與 f(x) 的值",
            "其中(a+b)^2為",
            "正則化(regularization)與 e.g. 的用法",
            "如前所述...接著",
            "若 x!=0 則",
            "We train the model, then test it.",
            // 引句照抄（重點筆記的引句區）：引號裡英文句子結尾的標點不動
            "「Is it enough?」這句值得記下",
            "「It works, mostly.」作者這樣說",
            "（see Table 3, row 2!）",
            // 階乘、含中文的網址、中文數字的編號
            "共有 n! 種排列",
            "見 https://zh.wikipedia.org/wiki/深度學習?action=edit 的說明",
            "一. 緒論",
        ] {
            assert_eq!(fullwidth_punct(s), s, "{s}");
        }
    }

    #[test]
    fn the_automatic_check_sees_the_same_problems_before_and_after() {
        let pairs = [
            (
                "Prior regularizers consider each state on its own, and limit the sensitivity [12], [3, 5].",
                "先前的正則化方法將每個狀態單獨考量,並限制敏感度 [12], [3, 5]。",
            ),
            (
                "a 600 megapixel mosaic CCD camera with a field of view of 47 deg2 (Dekany et al. 2020).",
                "配備 6 億像素的 CCD 相機,視野 47 deg2(Dekany et al. 2020)。",
            ),
            (
                "Aggregate CRB 181.9202 199.9954, $x^2$ and 13.94% (see Table 3).",
                "總 CRB 181.9202 199.9954,$x^2$ 與 13.94%(見表 3).",
            ),
            (
                "See https://zh.wikipedia.org/wiki/深度學習?action=edit for details.",
                "詳見 https://zh.wikipedia.org/wiki/深度學習?action=edit 的說明.",
            ),
        ];
        for (src, tgt) in pairs {
            let before = check_segment(src, tgt, "zh-TW");
            let after = check_segment(src, &fullwidth_punct(tgt), "zh-TW");
            assert_eq!(before, after, "{tgt}");
        }
    }
}
