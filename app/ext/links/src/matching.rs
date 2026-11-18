//! 比對：庫裡那篇的比對鍵、條目的比對鍵、篇名規則。純函式，測試直接呼叫。
//!
//! 三種規則依序試，成立就停：DOI 相同、arXiv 編號相同（不看版本）、篇名。同一條規則成立的候選有兩篇以上時
//! 不自動連，全部列成「待確認」（排列與判斷的套用在 `index.rs`）。正規化用核心的 `fold_text`。

use paperreader_core::format::{Bib, normalize_doi};
use paperreader_core::references::{fold_text, is_arxiv_id};
use serde::Serialize;

/// 連到的依據。前三種是自動比對；`manual`、`confirm` 是使用者的判斷。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Doi,
    Arxiv,
    Title,
    Manual,
    Confirm,
}

/// 篇名後面（略過空白）可以接的字：結尾或這幾個標點。
const TITLE_FOLLOW: &[char] = &[
    '.', ',', '"', '\u{201D}', '\u{2019}', ':', ';', '?', '!', ')', ']',
];

/// 篇名規則要的最少詞數；條目沒有年份時要更多（或第一作者的姓在篇名之前）。
pub const MIN_TITLE_WORDS: usize = 4;
pub const MIN_TITLE_WORDS_NO_YEAR: usize = 6;

/// 庫裡一篇的比對鍵。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaperKeys {
    /// 給人看的篇名（書目沒有篇名時是空的）。
    pub title: String,
    /// 篇名經 `fold_text`、只留字母與數字。
    pub title_key: String,
    /// 篇名經 `fold_text`、以非字母數字切開的詞。
    pub title_words: Vec<String>,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    /// 書目的年份，加上 arXiv 編號的年份（第一次投稿那年）。
    pub years: Vec<i32>,
    /// 第一作者的姓（`fold_text` 之後）。
    pub first_author: Option<String>,
}

/// 只留字母與數字（`char::is_alphanumeric`，和找回鍵同一個定義）。
fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// 以非字母數字切開的詞（已經 `fold_text` 過的字串）。
pub fn words_of(folded: &str) -> Vec<String> {
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// 去掉 arXiv 編號尾端的版本號（`v2`）。
fn strip_version(s: &str) -> &str {
    if let Some(i) = s.rfind('v') {
        let (head, tail) = (&s[..i], &s[i + 1..]);
        if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) && is_arxiv_id(head) {
            return head;
        }
    }
    s
}

/// 舊式編號的子類（`math.AG`）在 DOI 裡會被轉成小寫：轉回大寫再認。
fn arxiv_id(raw: &str) -> Option<String> {
    let s = strip_version(raw.trim());
    if is_arxiv_id(s) {
        return Some(s.to_string());
    }
    if let Some((cat, num)) = s.split_once('/')
        && let Some((base, sub)) = cat.split_once('.')
    {
        let fixed = format!("{base}.{}/{num}", sub.to_ascii_uppercase());
        if is_arxiv_id(strip_version(&fixed)) {
            return Some(strip_version(&fixed).to_string());
        }
    }
    None
}

/// 網址裡的 arXiv 編號：`arxiv.org/abs/<編號>` 或 `arxiv.org/pdf/<編號>`，後面可帶版本號與 `.pdf`。
pub fn arxiv_from_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let at = ["arxiv.org/abs/", "arxiv.org/pdf/"]
        .iter()
        .find_map(|m| lower.find(m).map(|i| i + m.len()))?;
    let rest = &url[at..];
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let rest = rest.trim_end_matches('/');
    let rest = rest
        .strip_suffix(".pdf")
        .or_else(|| rest.strip_suffix(".PDF"))
        .unwrap_or(rest);
    arxiv_id(rest)
}

/// arXiv 的 DOI（`10.48550/arxiv.<編號>`）裡的編號。
pub fn arxiv_from_doi(doi: &str) -> Option<String> {
    let d = normalize_doi(doi);
    arxiv_id(d.strip_prefix("10.48550/arxiv.")?)
}

/// arXiv 編號的年份：新式 `YYMM.NNNNN` 是 20YY（2007 年 4 月起）；舊式 `類別/YYMMNNN` 依 `pdfMeta.ts` 的同一條規則
/// （91 以上是 19YY，其餘 20YY）。
pub fn arxiv_year(id: &str) -> Option<i32> {
    let digits = match id.split_once('/') {
        Some((_, num)) => num,
        None => id,
    };
    let yy: i32 = digits.get(..2)?.parse().ok()?;
    let mm: i32 = digits.get(2..4)?.parse().ok()?;
    if !(1..=12).contains(&mm) {
        return None;
    }
    if id.contains('/') {
        Some(if yy >= 91 { 1900 + yy } else { 2000 + yy })
    } else {
        (yy >= 7).then_some(2000 + yy)
    }
}

/// 一個作者的姓：`family`，沒有時 `literal` 的最後一個詞；經 `fold_text`。
fn family_of(a: &paperreader_core::format::Author) -> Option<String> {
    let raw = a
        .family
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            a.literal
                .as_deref()
                .and_then(|l| l.split_whitespace().last())
        })?;
    let f = fold_text(raw);
    (!f.is_empty()).then_some(f)
}

/// 庫裡那篇的比對鍵。
pub fn paper_keys(bib: &Bib) -> PaperKeys {
    let title = bib.title.clone().unwrap_or_default();
    let folded = fold_text(&title);
    let doi = bib
        .doi
        .as_deref()
        .map(normalize_doi)
        .filter(|d| !d.is_empty());
    let arxiv = bib
        .url
        .as_deref()
        .and_then(arxiv_from_url)
        .or_else(|| doi.as_deref().and_then(arxiv_from_doi));
    let mut years: Vec<i32> = bib.year.into_iter().collect();
    if let Some(y) = arxiv.as_deref().and_then(arxiv_year)
        && !years.contains(&y)
    {
        years.push(y);
    }
    PaperKeys {
        title_key: alnum(&folded),
        title_words: words_of(&folded),
        title,
        doi,
        arxiv,
        years,
        first_author: bib.authors.first().and_then(family_of),
    }
}

/// 條目一側的比對鍵。
#[derive(Debug, Clone)]
pub struct EntryKeys {
    /// 條目原文經 `fold_text`，一個字一格。
    folded: Vec<char>,
    /// 只留字母與數字。
    key: Vec<char>,
    /// `key` 的每一個字在 `folded` 裡的位置。
    pos: Vec<usize>,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    pub year: Option<i32>,
}

impl EntryKeys {
    /// 條目原文經 `fold_text` 後的詞。
    pub fn words(&self) -> Vec<String> {
        words_of(&self.folded.iter().collect::<String>())
    }
}

/// 條目的 DOI 與 arXiv 編號（DOI 是 `10.48550/arxiv.X` 的也當成 arXiv 編號 X）。
pub fn entry_ids(doi: Option<&str>, arxiv: Option<&str>) -> (Option<String>, Option<String>) {
    let doi = doi.map(normalize_doi).filter(|d| !d.is_empty());
    let arxiv = arxiv
        .and_then(arxiv_id)
        .or_else(|| doi.as_deref().and_then(arxiv_from_doi));
    (doi, arxiv)
}

/// 條目的比對鍵。
pub fn entry_keys(
    text: &str,
    doi: Option<&str>,
    arxiv: Option<&str>,
    year: Option<i32>,
) -> EntryKeys {
    let folded: Vec<char> = fold_text(text).chars().collect();
    let mut key = Vec::with_capacity(folded.len());
    let mut pos = Vec::with_capacity(folded.len());
    for (i, c) in folded.iter().enumerate() {
        if c.is_alphanumeric() {
            key.push(*c);
            pos.push(i);
        }
    }
    let (doi, arxiv) = entry_ids(doi, arxiv);
    EntryKeys {
        folded,
        key,
        pos,
        doi,
        arxiv,
        year,
    }
}

/// `needle` 在 `hay` 裡每一個出現的位置。
fn occurrences(hay: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|&i| hay[i] == needle[0] && hay[i..i + needle.len()] == *needle)
        .collect()
}

/// 第一作者的姓在 `folded[..end]` 裡出現成一個完整的詞。
fn author_before(name: &str, folded: &[char], end: usize) -> bool {
    let name: Vec<char> = name.chars().collect();
    let hay = &folded[..end.min(folded.len())];
    occurrences(hay, &name).into_iter().any(|i| {
        let before = i == 0 || !hay[i - 1].is_alphanumeric();
        let after = hay.get(i + name.len()).is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
}

/// 篇名規則：成立交回 `Some(待確認)`，不成立 `None`。
///
/// ① 篇名至少 4 個詞；② 篇名對回條目原文（`fold_text` 之後），前一個字不是字母或數字，後面（略過空白）是結尾或
/// `TITLE_FOLLOW` 的標點；③ 條目有年份、那篇也有年份時，任一個差 1 以內才成立（自動連上）。條目或那篇沒有年份時，
/// 要篇名至少 6 個詞，或第一作者的姓出現在條目原文裡篇名之前，而且一律是待確認。
pub fn title_rule(paper: &PaperKeys, entry: &EntryKeys) -> Option<bool> {
    if paper.title_words.len() < MIN_TITLE_WORDS {
        return None;
    }
    let tk: Vec<char> = paper.title_key.chars().collect();
    for s in occurrences(&entry.key, &tk) {
        let a = entry.pos[s];
        let b = entry.pos[s + tk.len() - 1];
        if a > 0 && entry.folded[a - 1].is_alphanumeric() {
            continue;
        }
        let mut j = b + 1;
        while j < entry.folded.len() && entry.folded[j].is_whitespace() {
            j += 1;
        }
        if j < entry.folded.len() && !TITLE_FOLLOW.contains(&entry.folded[j]) {
            continue;
        }
        match entry.year {
            Some(y) if !paper.years.is_empty() => {
                return paper
                    .years
                    .iter()
                    .any(|py| (py - y).abs() <= 1)
                    .then_some(false);
            }
            _ => {
                let long = paper.title_words.len() >= MIN_TITLE_WORDS_NO_YEAR;
                let by_author = paper
                    .first_author
                    .as_deref()
                    .is_some_and(|n| author_before(n, &entry.folded, a));
                if long || by_author {
                    return Some(true);
                }
            }
        }
    }
    None
}
