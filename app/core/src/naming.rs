//! 名稱的規則。核心裡挑名稱只用這裡的幾個函式：
//!
//! - 文獻資料夾、回收區的資料夾、筆記檔、衝突副本：已有同名時加 ` (2)`、` (3)` …（`unique_name`、
//!   `unique_stem`）。資料夾名稱只給人看，應用不從它讀任何資訊。
//! - 材料檔：已有同名時在副檔名之前加 `-2`、`-3` …（`numbered_names`、`free_file_name`）。
//!
//! 兩條規則各留一份實作、格式不合併：合併會改到磁碟上的檔名。

use crate::format::BibDraft;
use std::collections::HashSet;
use unicode_segmentation::UnicodeSegmentation;

const MAX_CHARS: usize = 60;
const MAX_BYTES: usize = 180;
const ILLEGAL: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 依命名規則處理出基本名稱（還沒處理撞名）。
pub fn folder_base_name(bib: &BibDraft, origin_name: Option<&str>) -> String {
    let first_author = bib
        .authors
        .first()
        .and_then(|a| non_empty(a.family.as_deref()).or_else(|| non_empty(a.literal.as_deref())));
    let mut left: Vec<String> = Vec::new();
    if let Some(a) = first_author {
        left.push(a.to_string());
    }
    if let Some(y) = bib.year {
        left.push(y.to_string());
    }
    let left = left.join(" ");
    let title = non_empty(bib.title.as_deref()).unwrap_or("");

    let raw = match (left.is_empty(), title.is_empty()) {
        (false, false) => format!("{left} - {title}"),
        (false, true) => left,
        (true, false) => title.to_string(),
        (true, true) => origin_name.map(strip_extension).unwrap_or_default(),
    };
    finish(&raw)
}

/// 命名規則第 5 步：已有同名時加 ` (2)`、` (3)`…；不分大小寫比對。
pub fn unique_name(existing: &[String], base: &str) -> String {
    let taken: Vec<String> = existing.iter().map(|s| s.to_lowercase()).collect();
    let is_taken = |n: &str| taken.contains(&n.to_lowercase());
    if !is_taken(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base} ({n})");
        if !is_taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 同 `unique_name`，比的是去掉結尾的 `ext`（例如 `.md`，大小寫要相同）之後的名稱：`existing` 是資料夾裡的
/// 項目名稱（含 `ext`）。交回不含 `ext` 的名稱，由呼叫的一方加上 `ext`。
pub(crate) fn unique_stem(existing: &[String], ext: &str, base: &str) -> String {
    let stems: Vec<String> = existing
        .iter()
        .map(|n| n.strip_suffix(ext).unwrap_or(n).to_string())
        .collect();
    unique_name(&stems, base)
}

/// 材料檔的候選名稱：`<stem><ext>`、`<stem>-2<ext>`、`<stem>-3<ext>` …（`ext` 含開頭的點，可以是空的）。
pub(crate) fn numbered_names<'a>(stem: &'a str, ext: &'a str) -> impl Iterator<Item = String> + 'a {
    std::iter::once(format!("{stem}{ext}")).chain((2u64..).map(move |n| format!("{stem}-{n}{ext}")))
}

/// `numbered_names` 裡第一個不在 `taken` 裡的名稱（`taken` 是小寫的名稱，不分大小寫比對）。不會失敗。
pub(crate) fn free_file_name(taken: &HashSet<String>, stem: &str, ext: &str) -> String {
    numbered_names(stem, ext)
        .find(|n| !taken.contains(&n.to_lowercase()))
        .expect("an endless sequence has a free name")
}

/// 把檔名拆成（主檔名, 副檔名）：在最後一個不在開頭的點拆開，副檔名含那個點；沒有這樣的點時副檔名是空的。
pub(crate) fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// 路徑的最後一段（`/` 與 `\` 都當分隔）。
pub(crate) fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn strip_extension(name: &str) -> String {
    split_extension(name).0.to_string()
}

fn finish(raw: &str) -> String {
    // 1. 去掉不合法字元與控制字元；連續空白併成一個。
    //    換行、定位字元先當空白，不讓兩個字黏在一起。
    let mut cleaned = String::with_capacity(raw.len());
    for c in raw.chars() {
        if ILLEGAL.contains(&c) {
            continue;
        }
        if c.is_whitespace() {
            cleaned.push(' ');
        } else if !c.is_control() {
            cleaned.push(c);
        }
    }
    let collapsed = cleaned
        .split(' ')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // 2. 去掉開頭的 `.` 與結尾的 `.` 和空白。
    let stripped = trim_edges(&collapsed);

    // 3. 截到 60 個字元、180 個位元組以內，不切在字中間；截完再去一次結尾。
    let truncated = trim_edges(&truncate(&stripped));

    // 4. 空的用 untitled；Windows 保留名稱前面加 `_`。
    if truncated.is_empty() {
        return "untitled".to_string();
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&truncated)) {
        return format!("_{truncated}");
    }
    truncated
}

fn trim_edges(s: &str) -> String {
    s.trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' '])
        .to_string()
}

/// 以使用者看到的「一個字」（字素叢集）為單位截斷：帶重音的字母、組合起來的表情符號、
/// 韓文的一個音節都各算一個，不拆開。
fn truncate(s: &str) -> String {
    let mut out = String::new();
    for (count, g) in s.graphemes(true).enumerate() {
        if count >= MAX_CHARS || out.len() + g.len() > MAX_BYTES {
            break;
        }
        out.push_str(g);
    }
    out
}
