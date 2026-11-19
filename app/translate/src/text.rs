//! 送出前與寫入前的文字整理，翻譯與重點筆記共用：
//!
//! - 送出前濾掉控制字元：PDF 抽出的公式括號常是 `\u{0}`、`\u{1}`，讓 Firefox 翻譯回不出東西、整篇的譯本 PDF
//!   不自動產生。換行與定位字元留著。
//! - 譯成中文時，把中文旁邊的半形標點換成全形（約六成的段落用半形逗號）。數字、英文、公式、
//!   網址裡的不動；括號只在裡面有中文字時才換（括號裡只有英文的多半是引用，例如 `(Smith et al., 2020)`，留著）。
//! - 譯成中文時，模型換成全形的作者年份引用括號換回原文的半形；譯文後面用括號附上的整段原文拿掉；
//!   同一個括號英文在一篇裡只留第一次。

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

// ---------- 譯文的整理（譯成中文時） ----------

/// 年份：19xx、20xx，後面可以接一個小寫字母（`2020a`）。前後不能接著數字。
fn has_year(v: &[char]) -> bool {
    (0..v.len().saturating_sub(3)).any(|i| {
        let four = &v[i..i + 4];
        (four[0] == '1' && four[1] == '9' || four[0] == '2' && four[1] == '0')
            && four[2].is_ascii_digit()
            && four[3].is_ascii_digit()
            && (i == 0 || !v[i - 1].is_ascii_digit())
            && v.get(i + 4).is_none_or(|c| !c.is_ascii_digit())
    })
}

/// 括號裡像作者年份引用：有年份，又有大寫開頭的姓或 `et al`。
fn looks_like_citation(inner: &[char]) -> bool {
    let s: String = inner.iter().collect();
    has_year(inner)
        && (s.contains("et al")
            || s.split(|c: char| !c.is_alphabetic())
                .any(|w| w.chars().count() >= 2 && w.starts_with(|c: char| c.is_ascii_uppercase())))
}

/// 括號裡只有年份（「Smith（2020）」「Smith et al.（2020a, 2021）」）。
fn only_years(inner: &[char]) -> bool {
    has_year(inner)
        && inner.iter().all(|c| {
            c.is_ascii_digit()
                || c.is_ascii_lowercase()
                || matches!(c, ',' | '，' | ';' | '；' | ' ')
        })
}

/// `i` 前面緊接著（可隔空白）一個姓或 `et al.`：大寫開頭的英文字，或以 `al.` 結尾。
fn after_name(v: &[char], i: usize) -> bool {
    let Some(p) = prev_solid(v, i) else {
        return false;
    };
    let mut k = p + 1;
    while k > 0
        && (v[k - 1].is_alphabetic() && !han(v[k - 1])
            || matches!(v[k - 1], '.' | '-' | '\'' | '’'))
    {
        k -= 1;
    }
    let word: String = v[k..=p].iter().collect();
    word.ends_with("al.") || word.starts_with(|c: char| c.is_ascii_uppercase())
}

/// 比對用：去掉空白，全形的逗號、分號換成半形。
fn citation_key(s: impl Iterator<Item = char>) -> String {
    s.filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '，' => ',',
            '；' => ';',
            '：' => ':',
            c => c,
        })
        .collect()
}

/// 原文裡的半形括號：（括號前面是不是空白, 括號裡比對用的寫法, 原文的括號整段）。
fn source_parens(source: &str) -> Vec<(bool, String, String)> {
    let v: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    for (i, c) in v.iter().enumerate() {
        if *c != '(' {
            continue;
        }
        if let Some(j) = (i + 1..v.len().min(i + 302))
            .take_while(|&k| v[k] != '(')
            .find(|&k| v[k] == ')')
        {
            let spaced = i > 0 && v[i - 1].is_whitespace();
            out.push((
                spaced,
                citation_key(v[i + 1..j].iter().copied()),
                v[i..=j].iter().collect(),
            ));
        }
    }
    out
}

/// 作者年份引用的括號換回原文的半形：模型常把「Smith et al. (2020)」寫成「Smith et al.（2020）」。
/// 括號裡沒有中文字、有年份、又有姓或 et al.（或只有年份而前面緊接著姓）的全形括號，換回原文那一段的寫法
/// （括號裡的標點與空白照原文）；原文裡找不到同樣的括號就只換括號。括號前面的空白照原文，不另外補。
pub fn restore_citations(source: &str, target: &str) -> String {
    if !target.contains('（') {
        return target.to_string();
    }
    let src = source_parens(source);
    let v: Vec<char> = target.chars().collect();
    let mut out = String::with_capacity(target.len());
    let mut i = 0;
    while i < v.len() {
        if v[i] == '（'
            && let Some(j) = (i + 1..v.len().min(i + 302))
                .take_while(|&k| v[k] != '（' && v[k] != '(' && v[k] != '\n')
                .find(|&k| v[k] == '）')
        {
            let inner = &v[i + 1..j];
            let cite = !inner.iter().any(|c| han(*c))
                && (looks_like_citation(inner) || only_years(inner) && after_name(&v, i));
            if cite {
                let key = citation_key(inner.iter().copied());
                match src.iter().find(|(_, k, _)| *k == key) {
                    Some((spaced, _, text)) => {
                        // 原文括號前有空白、譯文括號前是英文（姓、et al.）時，空白照原文
                        let latin_before =
                            i > 0 && (v[i - 1].is_ascii_alphanumeric() || v[i - 1] == '.');
                        if *spaced && latin_before {
                            out.push(' ');
                        }
                        out.push_str(text);
                    }
                    None => {
                        out.push('(');
                        out.extend(inner.iter().map(|c| match c {
                            '，' => ',',
                            '；' => ';',
                            c => *c,
                        }));
                        out.push(')');
                    }
                }
                i = j + 1;
                continue;
            }
        }
        out.push(v[i]);
        i += 1;
    }
    out
}

/// 譯文後面用括號附上整段原文（篇名的譯文附了整個英文篇名）：拿掉。只看 15 字以上的原文。
pub fn drop_echoed_source(source: &str, target: &str) -> String {
    let src = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if src.chars().count() < 15 {
        return target.to_string();
    }
    let bare = src.trim_end_matches(['.', '。']);
    for s in [src.as_str(), bare] {
        for (open, close) in [("（", "）"), ("(", ")")] {
            let echo = format!("{open}{s}{close}");
            if let Some(at) = target.find(&echo) {
                let before = target[..at].trim_end_matches([' ', '\u{3000}']);
                return format!("{before}{}", &target[at + echo.len()..]);
            }
        }
    }
    target.to_string()
}

/// 中文詞後面括號附的英文（「正則化（regularization）」）：交回（拿掉時要拿掉的字元範圍, 比對用的小寫英文）。
/// 括號裡要是英文字（可以有空白、連字號），後面可以接「，……」；有數字的不算（引用、年份、編號）。
/// 後面還接著別的內容時（「（policy，即決策規則）」），範圍只到英文與它後面的逗號、空白，括號與其餘的內容留著；
/// 後面只有逗號、空白時（「（policy，）」）連括號一起。
fn bracket_glosses(v: &[char]) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < v.len() {
        if matches!(v[i], '（' | '(') && han(v[i - 1]) {
            let close = if v[i] == '（' { '）' } else { ')' };
            if let Some(j) = (i + 1..v.len().min(i + 120))
                .take_while(|&k| !matches!(v[k], '（' | '(' | '\n'))
                .find(|&k| v[k] == close)
            {
                let inner = &v[i + 1..j];
                let head: String = inner
                    .iter()
                    .take_while(|c| !matches!(c, '，' | ',' | '；' | ';'))
                    .collect();
                let word = head.trim();
                let ok = word.chars().count() >= 3
                    && word.starts_with(|c: char| c.is_ascii_alphabetic())
                    && word
                        .chars()
                        .all(|c| c.is_ascii_alphabetic() || c == ' ' || c == '-')
                    && !inner.iter().any(|c| c.is_ascii_digit());
                if ok {
                    let sep = i + 1 + head.chars().count();
                    // 英文後面除了逗號與空白沒有別的內容：括號整個拿掉
                    let rest_empty = v[sep..j]
                        .iter()
                        .all(|c| c.is_whitespace() || matches!(c, '，' | ',' | '；' | ';'));
                    let range = if sep < j && !rest_empty {
                        let mut end = sep + 1;
                        while end < j && v[end].is_whitespace() {
                            end += 1;
                        }
                        (i + 1, end - 1)
                    } else {
                        (i, j)
                    };
                    out.push((range.0, range.1, word.to_lowercase()));
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// 一段譯文裡附在中文詞後面的括號英文（小寫），依出現順序。
pub fn glosses(target: &str) -> Vec<String> {
    let v: Vec<char> = target.chars().collect();
    bracket_glosses(&v).into_iter().map(|(_, _, w)| w).collect()
}

/// 同一個括號英文在這篇前面出現過（`seen`），就拿掉這裡的；這一段裡第一次出現的記進 `seen`。
pub fn drop_repeated_glosses(target: &str, seen: &mut std::collections::HashSet<String>) -> String {
    let v: Vec<char> = target.chars().collect();
    let found = bracket_glosses(&v);
    if found.is_empty() {
        return target.to_string();
    }
    let mut drop = vec![false; v.len()];
    for (i, j, w) in found {
        if !seen.insert(w) {
            (i..=j).for_each(|k| drop[k] = true);
        }
    }
    v.iter()
        .zip(drop)
        .filter(|(_, d)| !d)
        .map(|(c, _)| *c)
        .collect()
}

/// 中文詞後面用半形括號附的英文（「行為複製 (behavior cloning)」）換成全形括號、拿掉中間的空白：
/// 「行為複製（behavior cloning）」。括號裡只有英文字母、空白、連字號、撇號、句點與逗號，至少有一個兩個字母以上的字，
/// 而且不含數字（引用、編號、式的指引不動）；只有單一字母的（變數）、程式碼裡的（反引號之間）、
/// 原文同樣寫著這個半形括號的（照抄原文）都不動。
pub fn fullwidth_glosses(source: &str, target: &str) -> String {
    if !target.contains('(') {
        return target.to_string();
    }
    let v: Vec<char> = target.chars().collect();
    let mut drop = vec![false; v.len()];
    let mut swap: Vec<(usize, usize)> = Vec::new();
    let mut ticks = 0usize;
    let mut i = 0;
    while i < v.len() {
        if v[i] == '`' {
            ticks += 1;
        }
        if v[i] != '(' || ticks % 2 == 1 {
            i += 1;
            continue;
        }
        let Some(j) = (i + 1..v.len().min(i + 200))
            .take_while(|&k| v[k] != '(' && v[k] != '\n')
            .find(|&k| v[k] == ')')
        else {
            i += 1;
            continue;
        };
        let inner = &v[i + 1..j];
        let allowed = inner
            .iter()
            .all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’' | '.' | ','));
        let word = inner
            .split(|c| !c.is_ascii_alphabetic())
            .any(|w| w.len() >= 2);
        let inner_s: String = inner.iter().collect();
        let copied = source.contains(&format!("({inner_s})"));
        let after_han = prev_solid(&v, i).is_some_and(|p| han(v[p]));
        if allowed && word && !copied && after_han {
            if let Some(p) = prev_solid(&v, i) {
                (p + 1..i).for_each(|k| drop[k] = true);
            }
            if let Some(n) = next_solid(&v, j)
                && cjk(v[n])
            {
                (j + 1..n).for_each(|k| drop[k] = true);
            }
            swap.push((i, j));
            i = j + 1;
            continue;
        }
        i += 1;
    }
    let mut v = v;
    for (a, b) in swap {
        v[a] = '（';
        v[b] = '）';
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

    #[test]
    fn author_year_citations_get_their_original_brackets_back() {
        let src = "As shown by Wang et al. (2025), the method works (Smith et al., 2020; Lee, 2021). In 1990 (and later) it was known.";
        let cases = [
            ("如 Wang et al.（2025）所示", "如 Wang et al. (2025)所示"),
            (
                "方法有效（Smith et al.，2020；Lee，2021）。",
                "方法有效(Smith et al., 2020; Lee, 2021)。",
            ),
            (
                "方法有效（Smith et al., 2020; Lee, 2021）。",
                "方法有效(Smith et al., 2020; Lee, 2021)。",
            ),
            // 原文沒有這一段：只換括號，不補空白
            ("見 Kim（2019）的說明", "見 Kim(2019)的說明"),
            ("見（Park and Choi, 2018）", "見(Park and Choi, 2018)"),
        ];
        for (from, to) in cases {
            assert_eq!(restore_citations(src, from), to, "{from}");
        }
        // 括號裡有中文、只有年份但前面不是姓、編號引用、沒有年份的：都不動
        for t in [
            "在 1990 年（以及之後）就已知",
            "於（2020）年",
            "如［12］所示",
            "正則化（Regularization）",
            "（見 Smith et al., 2020 的表 3）",
        ] {
            assert_eq!(restore_citations(src, t), t, "{t}");
        }
    }

    #[test]
    fn restoring_citations_keeps_the_automatic_check_unchanged() {
        let src =
            "As shown by Wang et al. (2025) and [12], the gain is 13.94% (Smith et al., 2020).";
        let tgt = "如 Wang et al.（2025）與 [12] 所示，增益為 13.94%（Smith et al.，2020）。";
        assert_eq!(
            check_segment(src, tgt, "zh-TW"),
            check_segment(src, &restore_citations(src, tgt), "zh-TW")
        );
    }

    #[test]
    fn the_whole_source_echoed_after_the_translation_is_dropped() {
        let src = "Spectral Smoothing for Policy Learning";
        assert_eq!(
            drop_echoed_source(
                src,
                "策略學習的頻譜平滑（Spectral Smoothing for Policy Learning）"
            ),
            "策略學習的頻譜平滑"
        );
        assert_eq!(
            drop_echoed_source(
                src,
                "策略學習的頻譜平滑 (Spectral Smoothing for Policy Learning)"
            ),
            "策略學習的頻譜平滑"
        );
        // 只附一部分、原文太短：不動
        let t = "策略學習（Policy Learning）的頻譜平滑";
        assert_eq!(drop_echoed_source(src, t), t);
        assert_eq!(
            drop_echoed_source("Systems", "系統（Systems）"),
            "系統（Systems）"
        );
    }

    #[test]
    fn a_repeated_english_gloss_is_kept_only_the_first_time() {
        let mut seen = std::collections::HashSet::new();
        let a = drop_repeated_glosses(
            "正則化（regularization）讓策略（policy）穩定，正則化（Regularization）也",
            &mut seen,
        );
        assert_eq!(a, "正則化（regularization）讓策略（policy）穩定，正則化也");
        let b = drop_repeated_glosses("策略（policy，即決策規則）與大型語言模型（LLM）", &mut seen);
        assert_eq!(b, "策略（即決策規則）與大型語言模型（LLM）");
        // 引用、編號、英文後面的括號、太短的：不算
        for t in [
            "作者（Smith, 2020）",
            "見表（Table 3）",
            "Transformer (Vaswani) 架構",
            "模型（AI）",
        ] {
            assert_eq!(drop_repeated_glosses(t, &mut seen), t, "{t}");
            assert_eq!(drop_repeated_glosses(t, &mut seen), t, "{t}");
        }
        assert_eq!(
            glosses("正則化（regularization）與策略(policy)"),
            vec!["regularization", "policy"]
        );
    }

    #[test]
    fn a_repeated_gloss_keeps_the_rest_of_its_parenthesis() {
        let mut seen: std::collections::HashSet<String> = ["policy".to_string()].into();
        assert_eq!(
            drop_repeated_glosses("策略（policy，即決策規則）", &mut seen),
            "策略（即決策規則）"
        );
        assert_eq!(
            drop_repeated_glosses("策略(policy, i.e. the rule)", &mut seen),
            "策略(i.e. the rule)"
        );
    }

    #[test]
    fn a_parenthesis_left_empty_is_dropped_too() {
        let mut seen: std::collections::HashSet<String> = ["policy".to_string()].into();
        assert_eq!(
            drop_repeated_glosses("策略（policy，）與", &mut seen),
            "策略與"
        );
        assert_eq!(
            drop_repeated_glosses("策略(policy, )與", &mut seen),
            "策略與"
        );
    }

    #[test]
    fn english_in_half_width_parentheses_after_chinese_becomes_full_width() {
        let src = "Unlike behavior cloning, we use inverse reinforcement learning here.";
        for (from, to) in [
            (
                "行為複製 (behavior cloning) 中",
                "行為複製（behavior cloning）中",
            ),
            (
                "模擬度量(bisimulation metrics)的",
                "模擬度量（bisimulation metrics）的",
            ),
            (
                "黎曼 (Riemannian motion policies, RMP) 中",
                "黎曼（Riemannian motion policies, RMP）中",
            ),
            ("方法 (O'Brien's rule) 與", "方法（O'Brien's rule）與"),
        ] {
            assert_eq!(fullwidth_glosses(src, from), to, "{from}");
        }
    }

    #[test]
    fn citations_math_references_code_urls_and_copied_parentheses_stay() {
        let src = "The policy (BC) is cloned, see f (x) and https://x.org/a (b).";
        for t in [
            // 作者年份引用、編號引用、指引：含數字
            "如前所述 (Smith et al., 2020)。",
            "如前所述 [12]。",
            "代入式 (5) 中",
            // 數學式、變數：只有單一字母
            "函數 (x) 的值",
            "函數 (x, y) 的值",
            // 程式碼、網址
            "呼叫 `run (fast)` 即可",
            "見 https://x.org/a (b) 的說明",
            // 原文本身就有的半形括號英文，譯文照抄
            "這個策略 (BC) 被複製",
            // 英文後面的括號
            "使用 Transformer (encoder) 架構",
        ] {
            assert_eq!(fullwidth_glosses(src, t), t, "{t}");
        }
    }
}
