//! 一處的位置與前後文。用和索引相同的正規化、切詞與比法，所以檔沒變時每一筆 FTS5 的命中
//! 都找得到位置。位置在這裡從位元組換成 UTF-16（全系統只在這一處換）。

use crate::query::{Query, Term};
use crate::tokenize::{Class, Token, Tokens, class, tokens};
use paperreader_core::references::{Folded, fold_text_mapped};

/// 前後文：`range` 之前幾個字、之後幾個字（Unicode 字元）。
pub const CONTEXT_BEFORE: usize = 30;
pub const CONTEXT_AFTER: usize = 60;

/// 找到的一處。`range` 是單位整段文字裡的 UTF-16 位置；`marks` 是 `context` 裡的 UTF-16 位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub range: (usize, usize),
    /// `range` 那一段的原樣文字。
    pub quote: String,
    pub context: String,
    /// 前面截掉了（畫面加「…」）。
    pub cut_start: bool,
    pub cut_end: bool,
    pub marks: Vec<(usize, usize)>,
}

/// 片語在 `row[k..]` 開頭成立：記號依序相等，`prefix_last` 時最後一個比字首（和 FTS5 相同）。
fn matches_at(row: &[Token], k: usize, term: &Term) -> bool {
    let n = term.tokens.len();
    if n == 0 || k + n > row.len() {
        return false;
    }
    term.tokens.iter().enumerate().all(|(i, t)| {
        let r = &row[k + i].text;
        if i == n - 1 && term.prefix_last {
            r.starts_with(t.as_str())
        } else {
            r == t
        }
    })
}

/// 一次出現的字元範圍：第一個記號起、最後一個記號止。字首比到的中日韓記號只到它的第一個字。
fn char_range(row: &[Token], k: usize, term: &Term) -> (usize, usize) {
    let n = term.tokens.len();
    let first = &row[k];
    let last = &row[k + n - 1];
    let cjk_prefix = term.prefix_last
        && last
            .text
            .chars()
            .next()
            .is_some_and(|c| class(c) == Class::Cjk);
    let end = if cjk_prefix { last.start + 1 } else { last.end };
    (first.start, end)
}

/// 一個詞每一次出現的字元範圍（主列與接回列）。
fn occurrences(toks: &Tokens, term: &Term) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for row in [&toks.main, &toks.joined] {
        for k in 0..row.len() {
            if matches_at(row, k, term) {
                out.push(char_range(row, k, term));
            }
        }
    }
    out
}

/// 第一次出現：先找主列，主列沒有才找接回列。
fn first_occurrence(toks: &Tokens, term: &Term) -> Option<(usize, usize)> {
    for row in [&toks.main, &toks.joined] {
        if let Some(k) = (0..row.len()).find(|&k| matches_at(row, k, term)) {
            return Some(char_range(row, k, term));
        }
    }
    None
}

/// 字元範圍 → 原字串的位元組範圍（經 `spans`）。
fn byte_range(f: &Folded, (s, e): (usize, usize)) -> (usize, usize) {
    (f.spans[s].0, f.spans[e - 1].1)
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// 找位置：第一個詞（照使用者打的先後）的第一次出現；其餘的詞在前後文裡每一次出現都標。
pub fn locate(q: &Query, unit_text: &str) -> Option<Located> {
    let first = q.terms.first()?;
    let f = fold_text_mapped(unit_text);
    let toks = tokens(&f);
    let hit = first_occurrence(&toks, first)?;
    // 幾個詞都要出現在這個單位裡（和 FTS5 的 AND 相同）；檔在索引之後變了時，少一個詞就找不到
    if !q.terms[1..]
        .iter()
        .all(|t| first_occurrence(&toks, t).is_some())
    {
        return None;
    }
    let (bs, be) = byte_range(&f, hit);
    // 前後文：range 之前 30 個字、之後 60 個字，不切開字元
    let cs = unit_text[..bs]
        .char_indices()
        .rev()
        .nth(CONTEXT_BEFORE - 1)
        .map_or(0, |(i, _)| i);
    let ce = unit_text[be..]
        .char_indices()
        .nth(CONTEXT_AFTER)
        .map_or(unit_text.len(), |(i, _)| be + i);
    let context = &unit_text[cs..ce];
    // 標記：每個詞落在前後文裡的每一次出現（含 range 本身），重疊的併起來
    let mut marks: Vec<(usize, usize)> = q
        .terms
        .iter()
        .flat_map(|t| occurrences(&toks, t))
        .map(|r| byte_range(&f, r))
        .filter_map(|(s, e)| {
            let (s, e) = (s.max(cs), e.min(ce));
            (s < e).then_some((s, e))
        })
        .collect();
    marks.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in marks {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    let to16 = |b: usize| utf16_len(&unit_text[cs..b]);
    Some(Located {
        range: (utf16_len(&unit_text[..bs]), utf16_len(&unit_text[..be])),
        quote: unit_text[bs..be].to_string(),
        context: context.to_string(),
        cut_start: cs > 0,
        cut_end: ce < unit_text.len(),
        marks: merged
            .into_iter()
            .map(|(s, e)| (to16(s), to16(e)))
            .collect(),
    })
}
