//! 查詢的解析：查詢字串 → 幾個詞，每個詞是一個片語；組成 FTS5 的式子。

use crate::tokenize::{Class, class, tokenize};
use paperreader_core::references::fold_text_mapped;

/// 一個詞：記號要相鄰、依序；`prefix_last` 時最後一個記號比字首。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub tokens: Vec<String>,
    pub prefix_last: bool,
}

/// 幾個詞都要出現在同一個單位裡，詞與詞之間不要求相鄰、不限先後。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub terms: Vec<Term>,
}

/// 查詢最少幾個字（Unicode 字元，正規化、去頭尾空白之後）。
pub const MIN_QUERY_CHARS: usize = 2;

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '\u{201D}')
}

/// 最後一個記號比不比字首：中日韓字一律比；字母數字要兩個字以上、不全是數字。
fn prefix_ok(token: &str) -> bool {
    let mut cs = token.chars();
    match cs.next() {
        None => false,
        Some(c) if class(c) == Class::Cjk => true,
        Some(_) => token.chars().count() >= 2 && !token.chars().all(char::is_numeric),
    }
}

/// 解析查詢。少於 2 個字、或一個詞也不剩交回 `None`。
pub fn parse_query(q: &str) -> Option<Query> {
    let f = fold_text_mapped(q);
    let chars: Vec<char> = f.text.chars().collect();
    if chars.len() < MIN_QUERY_CHARS {
        return None;
    }
    // 引號依序兩兩配對；沒有配對的最後一個當成空白
    let quotes: Vec<usize> = (0..chars.len()).filter(|&i| is_quote(chars[i])).collect();
    let paired = &quotes[..quotes.len() / 2 * 2];
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let outside = |s: usize, e: usize, pieces: &mut Vec<(usize, usize)>| {
        let mut i = s;
        while i < e {
            while i < e && (chars[i].is_whitespace() || is_quote(chars[i])) {
                i += 1;
            }
            let w = i;
            while i < e && !chars[i].is_whitespace() && !is_quote(chars[i]) {
                i += 1;
            }
            if i > w {
                pieces.push((w, i));
            }
        }
    };
    let mut at = 0;
    for pair in paired.chunks(2) {
        outside(at, pair[0], &mut pieces);
        pieces.push((pair[0] + 1, pair[1]));
        at = pair[1] + 1;
    }
    outside(at, chars.len(), &mut pieces);

    let terms: Vec<Term> = pieces
        .into_iter()
        .filter_map(|(s, e)| {
            let joins: Vec<usize> = f
                .joins
                .iter()
                .filter(|&&j| j >= s && j < e)
                .map(|j| j - s)
                .collect();
            let tokens: Vec<String> = tokenize(&chars[s..e], &joins)
                .main
                .into_iter()
                .map(|t| t.text)
                .collect();
            let last = tokens.last()?;
            let prefix_last = prefix_ok(last);
            Some(Term {
                tokens,
                prefix_last,
            })
        })
        .collect();
    (!terms.is_empty()).then_some(Query { terms })
}

impl Term {
    /// 這個詞在 FTS5 的式子：`"a b c" *`（比字首時）或 `"a b c"`。記號只含字母數字與中日韓字，沒有引號。
    pub fn fts(&self) -> String {
        let phrase = format!("\"{}\"", self.tokens.join(" "));
        if self.prefix_last {
            format!("{phrase} *")
        } else {
            phrase
        }
    }
}

impl Query {
    /// 交給 FTS5 `MATCH` 的式子：每個詞一個片語，以 `AND` 接起來。使用者打的 `"`、`*`、`AND`、`NEAR`、`-`
    /// 都只是字，不會變成語法。
    pub fn fts(&self) -> String {
        self.terms
            .iter()
            .map(Term::fts)
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}
