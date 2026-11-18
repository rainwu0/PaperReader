//! 書目匯出：把一批文獻的書目寫成 BibTeX（`.bib`）或 RIS（`.ris`）的文字。
//!
//! 純計算：不讀不寫檔案，由外殼把結果存成檔案。只用 `paper.json` 的篇名、作者、年份、期刊（`venue`）、
//! DOI、網址；沒有的欄位就不寫。不做引用格式的排版（APA 等）。
//!
//! 鍵值：第一作者姓＋年份＋篇名第一個字，全部小寫。沒有作者寫 `anon`、沒有年份寫 `nd`、沒有篇名就沒有最後那一段。
//! 鍵值相同的幾篇，每一篇都在後面加 `a`、`b`、`c`……（依輸入的順序）。

use crate::format::{Author, Bib};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BibFormat {
    Bibtex,
    Ris,
}

impl BibFormat {
    pub fn extension(self) -> &'static str {
        match self {
            BibFormat::Bibtex => "bib",
            BibFormat::Ris => "ris",
        }
    }

    pub fn from_name(name: &str) -> Option<BibFormat> {
        match name {
            "bibtex" => Some(BibFormat::Bibtex),
            "ris" => Some(BibFormat::Ris),
            _ => None,
        }
    }
}

/// 把一批書目寫成一份檔案的內容，順序照輸入。
pub fn export_bibliography(entries: &[&Bib], format: BibFormat) -> String {
    let keys = cite_keys(entries);
    let mut out = String::new();
    for (bib, key) in entries.iter().zip(&keys) {
        match format {
            BibFormat::Bibtex => bibtex_entry(&mut out, bib, key),
            BibFormat::Ris => ris_entry(&mut out, bib, key),
        }
        out.push('\n');
    }
    out
}

/// 每一篇的鍵值（彼此不重複）。
pub fn cite_keys(entries: &[&Bib]) -> Vec<String> {
    let base: Vec<String> = entries.iter().map(|b| base_key(b)).collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for k in &base {
        *count.entry(k.as_str()).or_default() += 1;
    }
    let mut seen_of: HashMap<&str, usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut keys = Vec::with_capacity(base.len());
    for k in &base {
        let mut key = if count[k.as_str()] > 1 {
            let n = seen_of.entry(k.as_str()).or_default();
            let s = format!("{k}{}", letters(*n));
            *n += 1;
            s
        } else {
            k.clone()
        };
        // 加了字母之後撞到別篇的鍵值（例如篇名第一個字剛好是 a）：再往後加
        while !used.insert(key.clone()) {
            key.push('x');
        }
        keys.push(key);
    }
    keys
}

/// 0 → a、25 → z、26 → aa。
fn letters(mut n: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'a' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn base_key(bib: &Bib) -> String {
    let family = first_author_family(&bib.authors).unwrap_or_else(|| "anon".to_string());
    let year = bib.year.map_or_else(|| "nd".to_string(), |y| y.to_string());
    let word = bib.title.as_deref().map(first_word).unwrap_or_default();
    key_clean(&format!("{family}{year}{word}"))
}

fn key_clean(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn first_author_family(authors: &[Author]) -> Option<String> {
    authors.iter().find_map(|a| {
        if let Some(f) = non_empty(a.family.as_deref()) {
            return Some(f.to_string());
        }
        let lit = non_empty(a.literal.as_deref())?;
        // 「Family, Given」取逗號前；「Given Family」取最後一個詞；沒有空白（例如中文姓名）整個留著
        let name = match lit.split_once(',') {
            Some((fam, _)) if !fam.trim().is_empty() => fam.trim(),
            _ => lit.split_whitespace().last().unwrap_or(lit),
        };
        Some(name.to_string())
    })
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
}

/// 篇名的第一個字：英文取第一個詞，中日韓取第一個字（它們的詞之間沒有空白）。
fn first_word(title: &str) -> String {
    let mut chars = title.chars().skip_while(|c| !c.is_alphanumeric());
    match chars.next() {
        None => String::new(),
        Some(c) if is_cjk(c) => c.to_string(),
        Some(c) => std::iter::once(c)
            .chain(chars.take_while(|c| c.is_alphanumeric() && !is_cjk(*c)))
            .collect(),
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// 一個作者寫成「姓, 名」；只有姓就只寫姓。全空回 `None`。
fn person(a: &Author) -> Option<(String, bool)> {
    let family = non_empty(a.family.as_deref());
    let given = non_empty(a.given.as_deref());
    match (family, given) {
        (Some(f), Some(g)) => Some((format!("{f}, {g}"), false)),
        (Some(f), None) => Some((f.to_string(), false)),
        (None, Some(g)) => Some((g.to_string(), false)),
        // 整串姓名（literal）：BibTeX 要用大括號包起來，免得被拆成姓和名
        (None, None) => non_empty(a.literal.as_deref()).map(|l| (l.to_string(), true)),
    }
}

// ---------- BibTeX ----------

/// BibTeX 文字欄位的跳脫。換行與連續空白併成一個空格。
fn bibtex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
            continue;
        }
        prev_space = false;
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '&' | '%' | '$' | '#' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}

/// 網址與 DOI 不跳脫（`url` 欄位要的是原樣），只擋會弄壞大括號配對的字元，並去掉換行。
fn bibtex_raw(s: &str) -> String {
    s.split_whitespace()
        .collect::<String>()
        .replace('{', "%7B")
        .replace('}', "%7D")
}

fn bibtex_entry(out: &mut String, bib: &Bib, key: &str) {
    let kind = if non_empty(bib.venue.as_deref()).is_some() {
        "article"
    } else {
        "misc"
    };
    out.push_str(&format!("@{kind}{{{key},\n"));
    let authors: Vec<String> = bib
        .authors
        .iter()
        .filter_map(person)
        .map(|(name, whole)| {
            if whole {
                format!("{{{}}}", bibtex_escape(&name))
            } else {
                bibtex_escape(&name)
            }
        })
        .collect();
    let mut field = |name: &str, value: String| {
        out.push_str(&format!("  {name} = {{{value}}},\n"));
    };
    if !authors.is_empty() {
        field("author", authors.join(" and "));
    }
    if let Some(t) = non_empty(bib.title.as_deref()) {
        field("title", bibtex_escape(t));
    }
    if let Some(y) = bib.year {
        field("year", y.to_string());
    }
    if let Some(v) = non_empty(bib.venue.as_deref()) {
        field("journal", bibtex_escape(v));
    }
    if let Some(d) = non_empty(bib.doi.as_deref()) {
        field("doi", bibtex_raw(d));
    }
    if let Some(u) = non_empty(bib.url.as_deref()) {
        field("url", bibtex_raw(u));
    }
    out.push_str("}\n");
}

// ---------- RIS ----------

fn ris_value(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ris_entry(out: &mut String, bib: &Bib, key: &str) {
    let kind = if non_empty(bib.venue.as_deref()).is_some() {
        "JOUR"
    } else {
        "GEN"
    };
    let mut line = |tag: &str, value: String| {
        out.push_str(&format!("{tag}  - {value}\n"));
    };
    line("TY", kind.to_string());
    line("ID", key.to_string());
    for (name, _) in bib.authors.iter().filter_map(person) {
        line("AU", ris_value(&name));
    }
    if let Some(t) = non_empty(bib.title.as_deref()) {
        line("TI", ris_value(t));
    }
    if let Some(y) = bib.year {
        line("PY", y.to_string());
    }
    if let Some(v) = non_empty(bib.venue.as_deref()) {
        line("JO", ris_value(v));
    }
    if let Some(d) = non_empty(bib.doi.as_deref()) {
        line("DO", ris_value(d));
    }
    if let Some(u) = non_empty(bib.url.as_deref()) {
        line("UR", ris_value(u));
    }
    out.push_str("ER  - \n");
}

#[cfg(test)]
mod tests {
    use super::first_word;
    use super::letters;

    #[test]
    fn letters_count_like_columns() {
        assert_eq!(letters(0), "a");
        assert_eq!(letters(25), "z");
        assert_eq!(letters(26), "aa");
        assert_eq!(letters(27), "ab");
    }

    #[test]
    fn first_word_handles_cjk_and_punctuation() {
        assert_eq!(first_word("Deep Learning"), "Deep");
        assert_eq!(first_word("  “Attention” is all"), "Attention");
        assert_eq!(first_word("深度學習綜述"), "深");
        assert_eq!(first_word("???"), "");
    }
}
