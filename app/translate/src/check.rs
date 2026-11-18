//! 譯文的自動檢查：比對原文與譯文，找出明顯的問題。
//!
//! 只做便宜、確定的檢查，不判斷翻得好不好：
//! - 數字：原文裡的數字（含小數、百分比）要在譯文裡原樣出現。
//! - 引用編號：`[12]`、`[3, 5]`、`[3–5]` 這類標記要原樣保留。
//! - 公式與程式片段：`$…$` 裡的內容、`\command` 形式的 LaTeX 指令要原樣保留。
//! - 網址與 DOI 要原樣保留。
//! - 漏翻：目標語言是中日韓文時，譯文幾乎沒有中日韓文字、又和原文差不多，視為沒翻。
//! - 亂碼：出現替代字元（U+FFFD）。
//! - 長度異常：譯文相對原文過短（可能被截斷或摘要）。
//!
//! 檢查結果分兩級：`Critical` 會讓工作把這一段單獨重翻一次；`Warning` 只記下來給使用者看。

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueKind {
    /// 原文的數字在譯文裡找不到。
    MissingNumber(String),
    /// 原文的引用編號在譯文裡找不到。
    MissingCitation(String),
    /// 原文的公式或 LaTeX 指令在譯文裡找不到。
    MissingFormula(String),
    /// 原文的網址或 DOI 在譯文裡找不到。
    MissingUrl(String),
    /// 看起來沒有翻（目標是中日韓文，譯文卻幾乎沒有中日韓文字）。
    Untranslated,
    /// 譯文裡有替代字元。
    Garbled,
    /// 譯文相對原文過短。
    TooShort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    pub severity: Severity,
}

impl Issue {
    /// 給記錄與畫面用的短代號（不含原文內容，避免把論文文字寫進記錄）。
    pub fn code(&self) -> &'static str {
        match self.kind {
            IssueKind::MissingNumber(_) => "missing_number",
            IssueKind::MissingCitation(_) => "missing_citation",
            IssueKind::MissingFormula(_) => "missing_formula",
            IssueKind::MissingUrl(_) => "missing_url",
            IssueKind::Untranslated => "untranslated",
            IssueKind::Garbled => "garbled",
            IssueKind::TooShort => "too_short",
        }
    }
}

/// 目標語言是不是中日韓文（決定漏翻與長度的判斷方式）。
pub fn is_cjk_target(target_lang: &str) -> bool {
    let l = target_lang.to_ascii_lowercase();
    l.starts_with("zh") || l.starts_with("ja") || l.starts_with("ko")
}

/// 檢查一段譯文。`source` 是原文，`target` 是譯文。
pub fn check_segment(source: &str, target: &str, target_lang: &str) -> Vec<Issue> {
    let mut out = Vec::new();
    let src = source.trim();
    let tgt = target.trim();
    if src.is_empty() {
        return out;
    }

    if tgt.contains('\u{FFFD}') {
        out.push(Issue {
            kind: IssueKind::Garbled,
            severity: Severity::Critical,
        });
    }

    let cjk = is_cjk_target(target_lang);
    let src_letters = count_letters(src);
    if cjk && src_letters >= 20 && cjk_ratio(tgt) < 0.05 {
        out.push(Issue {
            kind: IssueKind::Untranslated,
            severity: Severity::Critical,
        });
    }

    // 長度：中日韓文的譯文字數大約是英文字母數的 0.25 到 0.6；低於 0.12 很可能被截斷或摘要
    let src_len = src.chars().count();
    let tgt_len = tgt.chars().count();
    let min_ratio = if cjk { 0.12 } else { 0.4 };
    if src_len >= 80 && (tgt_len as f64) < (src_len as f64) * min_ratio {
        out.push(Issue {
            kind: IssueKind::TooShort,
            severity: Severity::Critical,
        });
    }

    let tgt_norm = normalize_for_match(tgt);
    // 網址與 DOI 裡的數字由網址檢查負責，不重複算成數字
    let src_urls = urls(src);
    let mut src_no_urls = src.to_string();
    for u in &src_urls {
        src_no_urls = src_no_urls.replace(u.as_str(), " ");
    }
    for n in missing(&numbers(&src_no_urls), &numbers(&tgt_norm)) {
        out.push(Issue {
            kind: IssueKind::MissingNumber(n),
            severity: Severity::Warning,
        });
    }
    let tgt_cite = citation_form(&tgt_norm);
    for c in citations(src) {
        if !tgt_cite.contains(&citation_form(&normalize_for_match(&c))) {
            out.push(Issue {
                kind: IssueKind::MissingCitation(c),
                severity: Severity::Critical,
            });
        }
    }
    for f in formulas(src) {
        if !tgt.contains(&f) {
            out.push(Issue {
                kind: IssueKind::MissingFormula(f),
                severity: Severity::Critical,
            });
        }
    }
    for u in src_urls {
        if !tgt.contains(&u) {
            out.push(Issue {
                kind: IssueKind::MissingUrl(u),
                severity: Severity::Warning,
            });
        }
    }
    out
}

/// 寫進檔案用的問題代號：去掉重複、照固定順序（只有幾種代號，所以一段最多幾筆，不會超過核心的上限）。
pub fn issue_codes(issues: &[Issue]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in issues {
        let c = i.code().to_string();
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out.sort();
    out
}

/// 一段的檢查結果裡有沒有要重翻的問題。
pub fn has_critical(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Critical)
}

/// 兩次結果取問題較少的：先比嚴重問題的數量，再比全部的數量。
pub fn better_of<'a>(a: &'a [Issue], b: &'a [Issue]) -> bool {
    let crit = |v: &[Issue]| {
        v.iter()
            .filter(|i| i.severity == Severity::Critical)
            .count()
    };
    (crit(b), b.len()) < (crit(a), a.len())
}

fn count_letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphabetic()).count()
}

fn is_cjk_char(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // 假名
        | 0x3400..=0x4DBF // 中日韓擴充 A
        | 0x4E00..=0x9FFF // 中日韓統一表意文字
        | 0xAC00..=0xD7AF // 韓文音節
        | 0xF900..=0xFAFF // 相容表意文字
        | 0x20000..=0x2FA1F)
}

fn cjk_ratio(s: &str) -> f64 {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return 0.0;
    }
    letters.iter().filter(|c| is_cjk_char(**c)).count() as f64 / letters.len() as f64
}

/// 比對前把全形數字、全形括號換成半形，模型把 `[12]` 寫成 `［12］` 不算漏。
fn normalize_for_match(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - 0xFF10 + 0x30).unwrap_or(c),
            '［' => '[',
            '］' => ']',
            '（' => '(',
            '）' => ')',
            '，' => ',',
            '．' => '.',
            '％' => '%',
            '－' | '–' | '—' => '-',
            _ => c,
        })
        .collect()
}

/// 引用編號比對用的寫法：去掉空白，範圍的符號（`-`、`~`、`–`、`—`）一律當成 `-`。
/// 所以 `[3, 5]`、`[3,5]`、`［3，5］`、`[3~5]` 與 `[3–5]` 都算保留了。
fn citation_form(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '~' | '～' | '–' | '—' => '-',
            _ => c,
        })
        .collect()
}

/// 原文裡的數字（連續數字，可帶一個小數點），以出現次數計。
/// 單一位數不算（「one of 3 models」譯成「三個模型之一」是合理的），年份、小數、百分比都算。
fn numbers(s: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()))
            {
                i += 1;
            }
            // 前一個字元是字母（例如 `L2`、`x1`）的是名稱的一部分，交給公式檢查
            let prev_alpha = start > 0 && chars[start - 1].is_ascii_alphabetic();
            let n: String = chars[start..i].iter().collect();
            if !prev_alpha && (n.len() >= 2 || n.contains('.')) {
                *out.entry(n).or_insert(0) += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn missing(src: &BTreeMap<String, usize>, tgt: &BTreeMap<String, usize>) -> Vec<String> {
    src.iter()
        .filter(|(k, n)| tgt.get(*k).copied().unwrap_or(0) < **n)
        .map(|(k, _)| k.clone())
        .collect()
}

/// 引用編號：方括號裡只有數字、逗號、連字號、空白的標記。
fn citations(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let inner = &after[..close];
        if !inner.is_empty()
            && inner.chars().any(|c| c.is_ascii_digit())
            && inner
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, ',' | '-' | '–' | ' '))
        {
            let c = format!("[{inner}]");
            if !out.contains(&c) {
                out.push(c);
            }
        }
        rest = &after[close + 1..];
    }
    out
}

/// 公式：`$…$` 的整段內容，以及 `\command` 形式的 LaTeX 指令名稱。
fn formulas(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |x: String| {
        if !x.is_empty() && !out.contains(&x) {
            out.push(x);
        }
    };
    let mut rest = s;
    while let Some(a) = rest.find('$') {
        let after = &rest[a + 1..];
        let Some(b) = after.find('$') else { break };
        let inner = &after[..b];
        // 金額（`$300 and $5,000`）不是公式：TeX 的行內公式緊貼著 `$`，而且不以數字開頭
        let looks_like_math = !inner.is_empty()
            && !inner.starts_with(|c: char| c.is_whitespace() || c.is_ascii_digit())
            && !inner.ends_with(char::is_whitespace);
        if looks_like_math {
            push(format!("${inner}$"));
            rest = &after[b + 1..];
        } else {
            // 這個 `$` 不是公式的開頭：從下一個 `$` 重新找
            rest = after;
        }
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

fn urls(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in s.split_whitespace() {
        let w = w.trim_end_matches(['.', ',', ';', ')', ']']);
        let lower = w.to_ascii_lowercase();
        if (lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("10."))
            && w.contains('/')
            && !out.contains(&w.to_string())
        {
            out.push(w.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(v: &[Issue]) -> Vec<&'static str> {
        v.iter().map(Issue::code).collect()
    }

    #[test]
    fn a_good_translation_has_no_issues() {
        let src = "Our model reaches 92.5% accuracy on ImageNet [12], which is 3.1 points above the baseline of 2019 [3, 5].";
        let tgt = "我們的模型在 ImageNet 上達到 92.5% 的準確率 [12]，比 2019 年的基準高 3.1 個百分點 [3, 5]。";
        assert!(check_segment(src, tgt, "zh-TW").is_empty());
    }

    #[test]
    fn full_width_digits_and_brackets_still_match() {
        let src = "See Table 12 and reference [24] for details.";
        let tgt = "細節見表１２與文獻［24］。";
        assert!(check_segment(src, tgt, "zh-TW").is_empty());
    }

    #[test]
    fn a_missing_number_is_a_warning() {
        let src = "The dataset contains 1500 images and 30 classes.";
        let tgt = "資料集包含 1500 張影像與若干類別。";
        let v = check_segment(src, tgt, "zh-TW");
        assert_eq!(
            v,
            vec![Issue {
                kind: IssueKind::MissingNumber("30".into()),
                severity: Severity::Warning
            }]
        );
    }

    #[test]
    fn single_digits_and_identifiers_are_not_numbers() {
        let src = "We compare 3 models with L2 loss.";
        let tgt = "我們以 L2 損失比較三個模型。";
        assert!(check_segment(src, tgt, "zh-TW").is_empty());
    }

    #[test]
    fn a_missing_citation_is_critical() {
        let src = "This follows prior work [7] closely.";
        let tgt = "這緊隨先前的研究。";
        let v = check_segment(src, tgt, "zh-TW");
        assert_eq!(codes(&v), vec!["missing_citation"]);
        assert!(has_critical(&v));
    }

    #[test]
    fn formulas_and_latex_commands_must_survive() {
        let src = "The loss is $\\mathcal{L} = \\sum_i x_i$ where \\alpha controls the weight.";
        let ok = "損失為 $\\mathcal{L} = \\sum_i x_i$，其中 \\alpha 控制權重。";
        assert!(check_segment(src, ok, "zh-TW").is_empty());
        let bad = "損失為 L 等於 x 的總和，其中 alpha 控制權重。";
        let v = check_segment(src, bad, "zh-TW");
        assert!(codes(&v).iter().all(|c| *c == "missing_formula"));
        assert!(has_critical(&v));
    }

    #[test]
    fn untranslated_text_is_critical_for_cjk_targets_only() {
        let src = "The proposed method outperforms all baselines on three benchmarks.";
        let v = check_segment(src, src, "zh-TW");
        assert!(codes(&v).contains(&"untranslated"));
        assert!(
            check_segment(src, src, "de")
                .iter()
                .all(|i| i.kind != IssueKind::Untranslated)
        );
    }

    #[test]
    fn a_very_short_translation_is_critical() {
        let src = "In this section we describe the architecture of the encoder, the decoder, and the attention module in detail.";
        let v = check_segment(src, "架構。", "zh-TW");
        assert!(codes(&v).contains(&"too_short"));
    }

    #[test]
    fn replacement_characters_are_garbled() {
        let v = check_segment("Hello world, this is a test.", "你好\u{FFFD}世界", "zh-TW");
        assert!(codes(&v).contains(&"garbled"));
    }

    #[test]
    fn urls_and_dois_are_checked() {
        let src = "Code is at https://github.com/example/repo and the paper is 10.1000/xyz123.";
        let tgt = "程式碼在 https://github.com/example/repo。";
        let v = check_segment(src, tgt, "zh-TW");
        assert_eq!(codes(&v), vec!["missing_url"]);
    }

    #[test]
    fn better_of_prefers_fewer_critical_issues() {
        let crit = vec![Issue {
            kind: IssueKind::Untranslated,
            severity: Severity::Critical,
        }];
        let warn = vec![
            Issue {
                kind: IssueKind::MissingNumber("12".into()),
                severity: Severity::Warning,
            },
            Issue {
                kind: IssueKind::MissingNumber("34".into()),
                severity: Severity::Warning,
            },
        ];
        assert!(
            better_of(&crit, &warn),
            "two warnings beat one critical issue"
        );
        assert!(!better_of(&warn, &crit));
        assert!(!better_of(&warn, &warn), "equal is not better");
    }

    #[test]
    fn citation_spacing_full_width_and_range_marks_still_match() {
        let src = "As in [3, 5] and [7-9], values lie in [0, 1].";
        for tgt in [
            "如 [3,5] 與 [7~9]，數值在 [0,1] 之間。",
            "如［3，5］與［7–9］，數值在［0，1］之間。",
            "如 [3, 5] 與 [7 - 9]，數值在 [0, 1] 之間。",
        ] {
            let v = check_segment(src, tgt, "zh-TW");
            assert!(
                v.iter()
                    .all(|i| !matches!(i.kind, IssueKind::MissingCitation(_))),
                "{tgt}: {v:?}"
            );
        }
    }

    #[test]
    fn money_is_not_a_formula() {
        let src = "The device costs $300 and the full system $5,000 in total.";
        let tgt = "這個裝置要價 $300，整套系統總共 $5,000。";
        assert!(check_segment(src, tgt, "zh-TW").is_empty());
        // 金額之後的真公式仍然要檢查
        let src = "It costs $300 and minimizes $f(x)$ over the set.";
        let v = check_segment(src, "要價 $300，並在集合上最小化 f(x)。", "zh-TW");
        assert_eq!(codes(&v), vec!["missing_formula"]);
    }

    #[test]
    fn issue_codes_are_unique_and_sorted() {
        let src = (1..=17)
            .map(|i| format!("[{i}]"))
            .collect::<Vec<_>>()
            .join(" ")
            + " and the numbers 10, 20, 30, 40, 50, 60, 70, 80.";
        let v = check_segment(&src, "見 [1-17]。", "zh-TW");
        assert!(v.len() > 16, "many issues of the same kind");
        let c = issue_codes(&v);
        assert!(c.len() <= 7);
        let mut sorted = c.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(c, sorted);
    }

    #[test]
    fn empty_source_has_no_issues() {
        assert!(check_segment("  ", "", "zh-TW").is_empty());
    }
}
