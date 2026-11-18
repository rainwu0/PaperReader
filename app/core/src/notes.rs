//! 筆記檔的解析與產生。純計算，不碰檔案，只處理字串。
//!
//! 做法：把原文切成一行一行的「範圍」（位元組位置），解析只在範圍上做，不重新排版整份文件。
//! 之後的每一次修改都是把原文的某一段位元組範圍換成新的字串，再更新屬性區的兩行時間與裝置；
//! 其餘一個位元組都不動（使用者在筆記工具裡的編輯、`\r\n`、位元組順序記號都原樣保留）。
//!
//! 解析一律看標記行（`<!-- paperreader:… -->`），不看標題文字。

use crate::error::{LibError, Result};
use crate::format::Map;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub(crate) const BOM: char = '\u{FEFF}';
pub(crate) const MARKER_PREFIX: &str = "<!-- paperreader:";
pub(crate) const MARKER_SUFFIX: &str = "-->";

/// 短評、整篇的想法最長的字元數。
pub const MAX_TEXT_CHARS: usize = 100_000;
/// 引句本身最長的字元數；前後文各最長的字元數。
pub const MAX_QUOTE_CHARS: usize = 10_000;
pub const MAX_CONTEXT_CHARS: usize = 200;
/// 頁碼標籤最長的字元數。
pub const MAX_LABEL_CHARS: usize = 32;
/// 新檔的篇名最長的字元數。
pub const MAX_TITLE_CHARS: usize = 200;
/// 文字段的標題與連結文字最長的字元數。
pub const MAX_LABEL_TEXT_CHARS: usize = 100;
/// 建索引時只讀屬性區：最多這麼多行、這麼多位元組。
pub const HEAD_LINES: usize = 64;
pub const HEAD_BYTES: usize = 8 * 1024;

fn invalid(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

// ---- 公開的型別 ----

/// 新建的文字用的標題與連結文字（由畫面照介面語言設定）。
#[derive(Debug, Clone, PartialEq)]
pub struct NotesLabels {
    pub thoughts: String,
    pub highlights: String,
    pub back_link: String,
}

impl Default for NotesLabels {
    fn default() -> Self {
        NotesLabels {
            thoughts: "Thoughts".into(),
            highlights: "Highlights".into(),
            back_link: "Open in PaperReader".into(),
        }
    }
}

impl NotesLabels {
    /// 三段文字各 1 到 100 個字元，不含控制字元（含換行）。
    pub fn validate(&self) -> Result<()> {
        for (name, s) in [
            ("thoughts", &self.thoughts),
            ("highlights", &self.highlights),
            ("back_link", &self.back_link),
        ] {
            let n = s.chars().count();
            if n == 0 || n > MAX_LABEL_TEXT_CHARS {
                return Err(invalid(format!(
                    "label {name} must be 1 to {MAX_LABEL_TEXT_CHARS} characters"
                )));
            }
            if s.chars().any(char::is_control) {
                return Err(invalid(format!(
                    "label {name} must not contain control characters"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRef {
    pub index: u32,
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPosition {
    pub start: u32,
    pub end: u32,
    pub basis: String,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub exact: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub sha256: Option<String>,
    pub fingerprint: Option<String>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 交給畫面的一條劃線。資料行用另一個內部結構序列化（含 `v`，不含 `comment`、`complete`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Highlight {
    pub id: String,
    pub kind: String,
    pub material: String,
    pub file: FileRef,
    pub page: PageRef,
    pub rects: Vec<[f64; 4]>,
    pub next_page_rects: Vec<[f64; 4]>,
    pub position: Option<TextPosition>,
    pub quote: Quote,
    pub color: Option<String>,
    pub created: String,
    pub modified: String,
    #[serde(flatten)]
    pub extra: Map,
    pub comment: String,
    /// 找得到配對的 `highlight-end`；`false` 時改短評、刪除都會被拒絕。
    pub complete: bool,
}

/// 讀不懂的劃線：帶它的原始文字、行號（從 1 起算）、原因。照樣留在檔案裡，任何寫入都不動它。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnreadableBlock {
    pub line: usize,
    pub text: String,
    pub reason: String,
}

/// 寫入時蓋上的時間與裝置。
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    pub time: String,
    pub device: String,
}

// ---- 資料行（內部結構）----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DataLine {
    v: u32,
    id: String,
    kind: String,
    material: String,
    file: FileRef,
    page: PageRef,
    rects: Vec<[f64; 4]>,
    next_page_rects: Vec<[f64; 4]>,
    position: Option<TextPosition>,
    quote: Quote,
    color: Option<String>,
    created: String,
    modified: String,
    #[serde(flatten)]
    extra: Map,
}

fn id_is_well_formed(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// 資料行的 JSON → 內部結構。必填的鍵缺少或型別不對、`v` 大於 1、`id` 不合規則都是讀不懂。
fn parse_data(json: &str) -> std::result::Result<DataLine, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let Value::Object(obj) = &value else {
        return Err("data is not a JSON object".into());
    };
    for k in [
        "v",
        "id",
        "kind",
        "material",
        "file",
        "page",
        "rects",
        "next_page_rects",
        "position",
        "quote",
        "color",
        "created",
        "modified",
    ] {
        if !obj.contains_key(k) {
            return Err(format!("missing key {k}"));
        }
    }
    let need = |v: &Value, parent: &str, keys: &[&str]| -> std::result::Result<(), String> {
        let o = v
            .as_object()
            .ok_or_else(|| format!("{parent} is not an object"))?;
        for k in keys {
            if !o.contains_key(*k) {
                return Err(format!("missing key {parent}.{k}"));
            }
        }
        Ok(())
    };
    need(&obj["file"], "file", &["sha256", "fingerprint"])?;
    need(&obj["page"], "page", &["index", "label"])?;
    need(&obj["quote"], "quote", &["exact", "prefix", "suffix"])?;
    if !obj["position"].is_null() {
        need(&obj["position"], "position", &["start", "end", "basis"])?;
    }
    let data: DataLine = serde_json::from_value(value).map_err(|e| format!("bad value: {e}"))?;
    if data.v > 1 {
        return Err(format!("data version {} is newer than 1", data.v));
    }
    if !id_is_well_formed(&data.id) {
        return Err("id must hold only ASCII letters, digits and hyphens".into());
    }
    if data.rects.is_empty() {
        return Err("rects is empty".into());
    }
    Ok(data)
}

/// 寫出資料行的 JSON：不換行，並把 `<`、`>` 換成 `<`、`>`（引句裡有 `-->` 也不會提早結束註解）。
fn data_json(d: &DataLine) -> String {
    let s = serde_json::to_string(d).expect("data line serializes");
    s.replace('<', "\\u003c").replace('>', "\\u003e")
}

fn to_data(h: &Highlight) -> DataLine {
    DataLine {
        v: 1,
        id: h.id.clone(),
        kind: h.kind.clone(),
        material: h.material.clone(),
        file: h.file.clone(),
        page: PageRef {
            index: h.page.index,
            label: h.page.label.clone(),
            extra: h.page.extra.clone(),
        },
        rects: h.rects.clone(),
        next_page_rects: h.next_page_rects.clone(),
        position: h.position.clone(),
        quote: h.quote.clone(),
        color: h.color.clone(),
        created: h.created.clone(),
        modified: h.modified.clone(),
        extra: h.extra.clone(),
    }
}

// ---- 文字的規則 ----

/// 頁碼標籤：1 到 32 個字元，只有 Unicode 字母、數字、空白、`-`、`.`。不合的當成 `None`。
pub fn sanitize_label(label: Option<&str>) -> Option<String> {
    let l = label?;
    let n = l.chars().count();
    if n == 0 || n > MAX_LABEL_CHARS {
        return None;
    }
    if l.chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '.')
    {
        Some(l.to_string())
    } else {
        None
    }
}

/// 新檔的篇名：換行與控制字元換成空白、前後空白去掉、最多 200 個字元。
pub fn clean_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_TITLE_CHARS).collect()
}

/// 把 `\r\n`、`\r` 換成 `\n`。
pub(crate) fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn starts_like_marker(line: &str) -> bool {
    line.trim_start().starts_with(MARKER_PREFIX)
}

/// 短評、整篇的想法的檢查；交回換行已正規化的文字。
pub fn check_free_text(text: &str, what: &str) -> Result<String> {
    let t = normalize_newlines(text);
    if t.contains('\0') {
        return Err(invalid(format!("{what} must not contain NUL")));
    }
    if t.chars().count() > MAX_TEXT_CHARS {
        return Err(invalid(format!(
            "{what} is longer than {MAX_TEXT_CHARS} characters"
        )));
    }
    if t.split('\n').any(starts_like_marker) {
        return Err(invalid(format!(
            "{what} must not contain a line that starts with \"{MARKER_PREFIX}\""
        )));
    }
    Ok(t)
}

/// 引句的檢查：不可以有 NUL；`exact` 非空、不超過 10000 個字元；前後文各不超過 200 個字元。
pub fn check_quote(q: &Quote) -> Result<()> {
    for (name, s) in [
        ("exact", Some(&q.exact)),
        ("prefix", q.prefix.as_ref()),
        ("suffix", q.suffix.as_ref()),
    ] {
        if s.is_some_and(|s| s.contains('\0')) {
            return Err(invalid(format!("quote.{name} must not contain NUL")));
        }
    }
    let n = q.exact.chars().count();
    if n == 0 || n > MAX_QUOTE_CHARS {
        return Err(invalid(format!(
            "quote.exact must be 1 to {MAX_QUOTE_CHARS} characters"
        )));
    }
    for (name, s) in [("prefix", &q.prefix), ("suffix", &q.suffix)] {
        if s.as_ref()
            .is_some_and(|s| s.chars().count() > MAX_CONTEXT_CHARS)
        {
            return Err(invalid(format!(
                "quote.{name} is longer than {MAX_CONTEXT_CHARS} characters"
            )));
        }
    }
    Ok(())
}

// ---- 解析 ----

#[derive(Debug, Clone, Copy)]
pub(crate) struct Line {
    /// 這一行第一個位元組的位置（第一行含開頭的位元組順序記號）。
    pub(crate) start: usize,
    /// 內容的結尾（不含 `\n`、也不含它前面的 `\r`）。
    pub(crate) end: usize,
    /// 下一行的開頭（含這一行的換行）。
    pub(crate) next: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Marker {
    Thoughts,
    ThoughtsEnd,
    Highlights,
    HighlightsEnd,
    /// 資料行的 JSON 文字。
    Highlight(String),
    HighlightEnd,
    Unknown,
}

#[derive(Debug, Clone)]
struct Block {
    id: String,
    /// 資料行所在的行（從 0 起算）。
    first: usize,
    /// 結尾標記所在的行；不完整時是 `None`。
    end: Option<usize>,
    /// 資料行的 JSON 原文：改短評時只換掉 `modified`，其餘（含不合規則的值、不認得的鍵、
    /// 整數座標的寫法）原樣寫回。
    raw_json: String,
    highlight: Highlight,
}

/// 新劃線的插入位置。
enum InsertAt {
    /// 這一行之前（那一行是 `highlights-end`）。
    BeforeLine(usize),
    /// 這一行之後（那一行是 `highlights` 開頭標記）。
    AfterLine(usize),
}

/// 解析的結果。保留原文，之後的修改只換掉要改的那一段。
#[derive(Debug, Clone)]
pub struct ParsedNotes {
    text: String,
    lines: Vec<Line>,
    paper_id: Option<String>,
    modified: Option<String>,
    modified_by: Option<String>,
    /// （行號, 標記），屬性區以外、依出現的順序。
    markers: Vec<(usize, Marker)>,
    blocks: Vec<Block>,
    unreadable: Vec<UnreadableBlock>,
}

pub(crate) fn split_lines(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        match text[pos..].find('\n') {
            Some(i) => {
                let nl = pos + i;
                let end = if nl > pos && text.as_bytes()[nl - 1] == b'\r' {
                    nl - 1
                } else {
                    nl
                };
                lines.push(Line {
                    start: pos,
                    end,
                    next: nl + 1,
                });
                pos = nl + 1;
            }
            None => {
                lines.push(Line {
                    start: pos,
                    end: text.len(),
                    next: text.len(),
                });
                pos = text.len();
            }
        }
    }
    lines
}

pub(crate) fn content<'a>(text: &'a str, l: &Line) -> &'a str {
    &text[l.start..l.end]
}

/// 屬性區的一行：去掉位元組順序記號（只有第一行會有）與行尾空白。
fn fm_line(s: &str) -> &str {
    s.strip_prefix(BOM).unwrap_or(s).trim_end()
}

/// 屬性區：第一行是 `---`，到下一個只有 `---` 或 `...` 的行為止。交回（開始行, 結束行）。
pub(crate) fn find_front_matter(text: &str, lines: &[Line]) -> Option<(usize, usize)> {
    let first = lines.first()?;
    if fm_line(content(text, first)) != "---" {
        return None;
    }
    (1..lines.len())
        .find(|&i| matches!(fm_line(content(text, &lines[i])), "---" | "..."))
        .map(|end| (0, end))
}

/// `key: value` → 值（去掉前後空白與一層單引號或雙引號）。
pub(crate) fn fm_value(line: &str, key: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix(key)?.trim_start();
    let v = rest.strip_prefix(':')?.trim();
    let v = match (v.chars().next(), v.chars().last()) {
        (Some(a), Some(b)) if v.len() >= 2 && a == b && (a == '"' || a == '\'') => {
            &v[1..v.len() - 1]
        }
        _ => v,
    };
    Some(v.to_string())
}

fn parse_marker(line: &str) -> Option<Marker> {
    let t = line.trim_start().trim_end();
    let t = t.strip_prefix(MARKER_PREFIX)?.strip_suffix(MARKER_SUFFIX)?;
    let t = t.trim();
    let (name, rest) = match t.find(char::is_whitespace) {
        Some(i) => (&t[..i], t[i..].trim()),
        None => (t, ""),
    };
    Some(match name {
        "thoughts" => Marker::Thoughts,
        "thoughts-end" => Marker::ThoughtsEnd,
        "highlights" => Marker::Highlights,
        "highlights-end" => Marker::HighlightsEnd,
        "highlight" => Marker::Highlight(rest.to_string()),
        "highlight-end" => Marker::HighlightEnd,
        _ => Marker::Unknown,
    })
}

/// 只看屬性區的 `paperreader_paper`，供建索引用。
pub fn read_paper_id(head: &str) -> Option<String> {
    let lines = split_lines(head);
    let (s, e) = find_front_matter(head, &lines)?;
    (s + 1..e).find_map(|i| fm_value(content(head, &lines[i]), "paperreader_paper"))
}

/// 同 `read_paper_id`，輸入是檔案開頭的位元組：切在一個字中間時只取到最後一個完整的字。
pub fn read_paper_id_from_bytes(head: &[u8]) -> Option<String> {
    let s = match std::str::from_utf8(head) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
    };
    read_paper_id(s)
}

/// 解析。永遠不失敗：讀不懂的部分記在 `unreadable`。
pub fn parse(text: &str) -> ParsedNotes {
    let lines = split_lines(text);
    let front = find_front_matter(text, &lines);
    let mut paper_id = None;
    let mut modified = None;
    let mut modified_by = None;
    if let Some((s, e)) = front {
        for l in &lines[s + 1..e] {
            let c = content(text, l);
            if paper_id.is_none() {
                paper_id = fm_value(c, "paperreader_paper");
            }
            // `paperreader_modified_by` 以 `paperreader_modified` 開頭，所以先比較長的鍵
            if modified_by.is_none() {
                modified_by = fm_value(c, "paperreader_modified_by");
            }
            if modified.is_none() {
                modified = fm_value(c, "paperreader_modified");
            }
        }
    }

    let first_body = front.map_or(0, |(_, e)| e + 1);
    let markers: Vec<(usize, Marker)> = (first_body..lines.len())
        .filter_map(|i| parse_marker(content(text, &lines[i])).map(|m| (i, m)))
        .collect();

    let mut blocks = Vec::new();
    let mut unreadable = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (k, (line_idx, m)) in markers.iter().enumerate() {
        let Marker::Highlight(json) = m else {
            continue;
        };
        let next = markers.get(k + 1);
        let end = match next {
            Some((j, Marker::HighlightEnd)) => Some(*j),
            _ => None,
        };
        // 範圍（不含資料行與結尾標記）的結尾：下一個標記行，沒有就是檔尾
        let body_end = next.map_or(lines.len(), |(j, _)| *j);
        let raw = content(text, &lines[*line_idx]).to_string();
        let data = match parse_data(json) {
            Ok(d) => d,
            Err(reason) => {
                unreadable.push(UnreadableBlock {
                    line: line_idx + 1,
                    text: raw,
                    reason,
                });
                continue;
            }
        };
        if !seen.insert(data.id.clone()) {
            unreadable.push(UnreadableBlock {
                line: line_idx + 1,
                text: raw,
                reason: format!("duplicate id {}", data.id),
            });
            continue;
        }
        let comment = read_comment(text, &lines[line_idx + 1..body_end], &data.id);
        let highlight = Highlight {
            id: data.id.clone(),
            kind: data.kind.clone(),
            material: data.material.clone(),
            file: data.file.clone(),
            page: data.page.clone(),
            rects: data.rects.clone(),
            next_page_rects: data.next_page_rects.clone(),
            position: data.position.clone(),
            quote: data.quote.clone(),
            color: data.color.clone(),
            created: data.created.clone(),
            modified: data.modified.clone(),
            extra: {
                // 資料行裡多出來的 `comment`、`complete` 不可以和交給畫面的欄位重複
                let mut x = data.extra.clone();
                x.remove("comment");
                x.remove("complete");
                x
            },
            comment,
            complete: end.is_some(),
        };
        blocks.push(Block {
            id: data.id.clone(),
            first: *line_idx,
            end,
            raw_json: json.clone(),
            highlight,
        });
    }

    ParsedNotes {
        text: text.to_string(),
        lines,
        paper_id,
        modified,
        modified_by,
        markers,
        blocks,
        unreadable,
    }
}

/// 一條劃線範圍裡的短評（不含資料行與結尾標記）。
fn read_comment(text: &str, body: &[Line], id: &str) -> String {
    let rows: Vec<&str> = body.iter().map(|l| content(text, l)).collect();
    let anchor = format!("^{id}");
    let rest: &[&str] = match rows.iter().position(|r| r.trim() == anchor) {
        Some(i) => &rows[i + 1..],
        None => {
            // 找不到 `^id` 那一行：略過第一個非空行（頁碼）、其後的空行與以 `>` 開頭的連續各行（引句）
            let mut i = 0;
            while i < rows.len() && rows[i].trim().is_empty() {
                i += 1;
            }
            i += 1; // 頁碼那一行
            while i < rows.len() && (rows[i].trim().is_empty() || rows[i].starts_with('>')) {
                i += 1;
            }
            rows.get(i..).unwrap_or(&[])
        }
    };
    let mut rest: Vec<&str> = rest.to_vec();
    // 最後一個非空行若是這條自己的「回到原文」連結，去掉
    if let Some(last) = rest.iter().rposition(|r| !r.trim().is_empty())
        && rest[last].contains("](paperreader://")
        && rest[last].contains(&format!("highlight={id}"))
    {
        rest.remove(last);
    }
    let first = rest.iter().position(|r| !r.trim().is_empty());
    let last = rest.iter().rposition(|r| !r.trim().is_empty());
    match (first, last) {
        (Some(a), Some(b)) => rest[a..=b].join("\n"),
        _ => String::new(),
    }
}

// ---- 查詢 ----

impl ParsedNotes {
    /// 屬性區的 `paperreader_paper`。
    pub fn paper_id(&self) -> Option<&str> {
        self.paper_id.as_deref()
    }

    /// `paperreader_modified`。
    pub fn modified(&self) -> Option<&str> {
        self.modified.as_deref()
    }

    /// `paperreader_modified_by`。
    pub fn modified_by(&self) -> Option<&str> {
        self.modified_by.as_deref()
    }

    /// 第一個 `thoughts` 標記的位置在 `markers` 裡的索引。
    fn thoughts_marker(&self) -> Option<usize> {
        self.markers
            .iter()
            .position(|(_, m)| *m == Marker::Thoughts)
    }

    /// 整篇的想法（去掉前後的空行、換行轉成 `\n`）。沒有區段是空字串；
    /// 不完整時讀到下一個標記行之前（或檔尾）。
    pub fn thoughts(&self) -> String {
        let Some(k) = self.thoughts_marker() else {
            return String::new();
        };
        let start = self.markers[k].0 + 1;
        let stop = self
            .markers
            .get(k + 1)
            .map_or(self.lines.len(), |(j, _)| *j);
        let rows: Vec<&str> = self.lines[start..stop]
            .iter()
            .map(|l| content(&self.text, l))
            .collect();
        let first = rows.iter().position(|r| !r.trim().is_empty());
        let last = rows.iter().rposition(|r| !r.trim().is_empty());
        match (first, last) {
            (Some(a), Some(b)) => rows[a..=b].join("\n"),
            _ => String::new(),
        }
    }

    /// 沒有 `thoughts` 標記時也是 `true`（還沒有區段）。
    pub fn thoughts_complete(&self) -> bool {
        match self.thoughts_marker() {
            None => true,
            Some(k) => matches!(self.markers.get(k + 1), Some((_, Marker::ThoughtsEnd))),
        }
    }

    /// 依（頁序、`position.start` 沒有就當 0、建立時間、代號）排序。
    pub fn highlights(&self) -> Vec<Highlight> {
        let mut v: Vec<Highlight> = self.blocks.iter().map(|b| b.highlight.clone()).collect();
        v.sort_by(|a, b| {
            (
                a.page.index,
                a.position.as_ref().map_or(0, |p| p.start),
                &a.created,
                &a.id,
            )
                .cmp(&(
                    b.page.index,
                    b.position.as_ref().map_or(0, |p| p.start),
                    &b.created,
                    &b.id,
                ))
        });
        v
    }

    pub fn unreadable(&self) -> Vec<UnreadableBlock> {
        self.unreadable.clone()
    }

    /// 這份檔的換行：出現過 `\r\n` 就用它，否則 `\n`。
    fn nl(&self) -> &'static str {
        if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    fn block(&self, highlight_id: &str) -> Result<&Block> {
        self.blocks
            .iter()
            .find(|b| b.id == highlight_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("highlight {highlight_id}"),
            })
    }

    fn require_complete(b: &Block) -> Result<usize> {
        b.end.ok_or_else(|| {
            invalid(format!(
                "highlight {} has no highlight-end marker; add it back in the notes tool",
                b.id
            ))
        })
    }

    // ---- 修改：每一個都換掉一段範圍，再更新屬性區的兩行 ----

    /// 加一條劃線：加在第一個 `highlights` 標記之後的第一個 `highlights-end` 之前；
    /// 找不到完整的插入點就在檔尾加一整段新的劃線區。
    pub fn with_new_highlight(
        &self,
        h: &Highlight,
        labels: &NotesLabels,
        paper_id: &str,
        stamp: &Stamp,
    ) -> Result<String> {
        labels.validate()?;
        let comment = check_free_text(&h.comment, "comment")?;
        check_quote(&h.quote)?;
        if !id_is_well_formed(&h.id) {
            return Err(invalid(
                "highlight id must hold only ASCII letters, digits and hyphens",
            ));
        }
        if self.blocks.iter().any(|b| b.id == h.id) {
            return Err(invalid(format!("highlight id {} already exists", h.id)));
        }
        let nl = self.nl();
        let mut h = h.clone();
        h.comment = comment;
        // 不合規則的頁碼標籤寫成 null（新的劃線才檢查；已經在檔案裡的原樣保留）
        h.page.label = sanitize_label(h.page.label.as_deref());
        let json = data_json(&to_data(&h));
        let mut block = render_block(&h, &json, labels, paper_id, nl);
        block.push_str(nl); // 每一條後面一個空行

        let out = match self.highlights_insert_point() {
            Some(InsertAt::BeforeLine(line)) => {
                let at = self.lines[line].start;
                splice(&self.text, at, at, &block)
            }
            Some(InsertAt::AfterLine(line)) => {
                // 區段開頭標記是檔案最後一行、沒有換行時，先補一個換行
                let at = self.lines[line].next;
                if at == self.text.len() && !self.text.ends_with('\n') {
                    let mut s = self.text.clone();
                    s.push_str(nl);
                    s.push_str(&block);
                    s
                } else {
                    splice(&self.text, at, at, &block)
                }
            }
            None => {
                let mut s = self.text.clone();
                ensure_trailing_newline(&mut s, nl);
                if !s.is_empty() {
                    s.push_str(nl);
                }
                s.push_str(&format!("## {}{nl}{nl}", labels.highlights));
                s.push_str(&format!("{MARKER_PREFIX}highlights {MARKER_SUFFIX}{nl}"));
                s.push_str(&block);
                s.push_str(&format!(
                    "{MARKER_PREFIX}highlights-end {MARKER_SUFFIX}{nl}"
                ));
                s
            }
        };
        Ok(self.stamped(&out, stamp))
    }

    /// 新的劃線放在哪裡：筆記檔已有劃線區時一律放進第一個劃線區，不另開新區。
    /// 該區有結尾標記、而且兩個標記之間沒有不完整的劃線：放在結尾標記之前；否則放在區段開頭標記
    /// 之後。完全沒有劃線區才是 `None`（呼叫的一方在檔尾開一整段）。
    fn highlights_insert_point(&self) -> Option<InsertAt> {
        let k = self
            .markers
            .iter()
            .position(|(_, m)| *m == Marker::Highlights)?;
        let (h_line, _) = self.markers[k];
        let end = self.markers[k + 1..]
            .iter()
            .find(|(_, m)| *m == Marker::HighlightsEnd)
            .map(|(l, _)| *l);
        if let Some(e_line) = end {
            let incomplete_between = self
                .blocks
                .iter()
                .any(|b| b.end.is_none() && b.first > h_line && b.first < e_line);
            let unreadable_open_between = self.markers.iter().any(|(line, m)| {
                *line > h_line
                    && *line < e_line
                    && matches!(m, Marker::Highlight(_))
                    && !self.blocks.iter().any(|b| b.first == *line)
                    && !matches!(
                        self.markers.iter().find(|(l2, _)| *l2 > *line),
                        Some((_, Marker::HighlightEnd))
                    )
            });
            if !incomplete_between && !unreadable_open_between {
                return Some(InsertAt::BeforeLine(e_line));
            }
        }
        Some(InsertAt::AfterLine(h_line))
    }

    /// 改一條劃線的短評：那一條（開頭標記到結尾標記）照樣子整段重新產生，`modified` 換成現在。
    pub fn with_comment(
        &self,
        highlight_id: &str,
        comment: &str,
        labels: &NotesLabels,
        paper_id: &str,
        stamp: &Stamp,
    ) -> Result<String> {
        let b = self.block(highlight_id)?;
        let end_line = Self::require_complete(b)?;
        labels.validate()?;
        let comment = check_free_text(comment, "comment")?;
        let nl = self.nl();
        let mut h = b.highlight.clone();
        h.comment = comment;
        h.modified = stamp.time.clone();
        // 資料行：原文的 JSON 值只換掉 `modified`（值與順序都不動，包括不合規則的 `page.label`）
        let mut value: Value = serde_json::from_str(&b.raw_json).expect("raw data line was parsed");
        value["modified"] = Value::String(stamp.time.clone());
        let json = serde_json::to_string(&value)
            .expect("json value serializes")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e");
        let mut block = render_block(&h, &json, labels, paper_id, nl);
        let from = self.lines[b.first].start;
        let to = self.lines[end_line].next;
        // 結尾標記是檔案最後一行、沒有換行時，新的也不加
        if self.lines[end_line].next == self.lines[end_line].end {
            block.truncate(block.len() - nl.len());
        }
        let out = splice(&self.text, from, to, &block);
        Ok(self.stamped(&out, stamp))
    }

    /// 刪一條劃線：開頭標記到結尾標記（含兩個標記行），以及緊接在後的一個空行（如果有）。
    pub fn without_highlight(&self, highlight_id: &str, stamp: &Stamp) -> Result<String> {
        let out = self.remove_highlight(highlight_id)?;
        Ok(self.stamped(&out, stamp))
    }

    /// 同 `without_highlight`，但不更新屬性區（合併用：最後才一起蓋時間）。
    pub fn remove_highlight(&self, highlight_id: &str) -> Result<String> {
        let b = self.block(highlight_id)?;
        let end_line = Self::require_complete(b)?;
        let from = self.lines[b.first].start;
        let mut to = self.lines[end_line].next;
        if let Some(after) = self.lines.get(end_line + 1)
            && after.start == after.end
        {
            to = after.next;
        }
        Ok(splice(&self.text, from, to, ""))
    }

    /// 改整篇的想法。有開始標記但不完整：`InvalidInput`；沒有開始標記：在檔尾加整段。
    pub fn with_thoughts(&self, text: &str, labels: &NotesLabels, stamp: &Stamp) -> Result<String> {
        let out = self.replace_thoughts(text, labels)?;
        Ok(self.stamped(&out, stamp))
    }

    /// 同 `with_thoughts`，但不更新屬性區。
    pub fn replace_thoughts(&self, text: &str, labels: &NotesLabels) -> Result<String> {
        labels.validate()?;
        let t = check_free_text(text, "thoughts")?;
        let nl = self.nl();
        let body: String = if t.is_empty() {
            String::new()
        } else {
            t.split('\n').map(|l| format!("{l}{nl}")).collect()
        };
        let out = match self.thoughts_marker() {
            Some(k) => {
                if !self.thoughts_complete() {
                    return Err(invalid(
                        "thoughts has no thoughts-end marker; add it back in the notes tool",
                    ));
                }
                let start_line = self.markers[k].0 + 1;
                let end_line = self.markers[k + 1].0;
                // 開始標記到結束標記之間的各行整段換掉（沒有內容時兩端相同）
                let from = self.lines[start_line].start;
                let to = self.lines[end_line].start;
                splice(&self.text, from, to, &body)
            }
            None => {
                let mut s = self.text.clone();
                ensure_trailing_newline(&mut s, nl);
                if !s.is_empty() {
                    s.push_str(nl);
                }
                s.push_str(&format!("## {}{nl}{nl}", labels.thoughts));
                s.push_str(&format!("{MARKER_PREFIX}thoughts {MARKER_SUFFIX}{nl}"));
                s.push_str(&body);
                s.push_str(&format!("{MARKER_PREFIX}thoughts-end {MARKER_SUFFIX}{nl}"));
                s
            }
        };
        Ok(out)
    }

    /// 更新屬性區的 `paperreader_modified`、`paperreader_modified_by`（修改後的整份文字上做）。
    fn stamped(&self, text: &str, stamp: &Stamp) -> String {
        stamp_text(text, stamp)
    }

    // ---- 合併用----

    /// 原文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 一條完整的劃線的原文：資料行到 `highlight-end` 那一行（含它的換行）。不完整或不存在是 `None`。
    pub fn highlight_raw(&self, highlight_id: &str) -> Option<String> {
        let b = self.blocks.iter().find(|b| b.id == highlight_id)?;
        let end = b.end?;
        Some(self.text[self.lines[b.first].start..self.lines[end].next].to_string())
    }

    /// 一條劃線資料行的 JSON 值（原文，含不認得的鍵）。
    pub fn highlight_data(&self, highlight_id: &str) -> Option<Value> {
        let b = self.blocks.iter().find(|b| b.id == highlight_id)?;
        serde_json::from_str(&b.raw_json).ok()
    }

    /// 把一條完整的劃線換成另一份原文（換行改成這份檔的換行）。不更新屬性區。
    pub fn replace_highlight_raw(&self, highlight_id: &str, raw: &str) -> Result<String> {
        let b = self.block(highlight_id)?;
        let end_line = Self::require_complete(b)?;
        let nl = self.nl();
        let mut block = with_newlines(raw, nl);
        if !block.ends_with('\n') {
            block.push_str(nl);
        }
        let from = self.lines[b.first].start;
        let to = self.lines[end_line].next;
        if self.lines[end_line].next == self.lines[end_line].end {
            block.truncate(block.len() - nl.len());
        }
        Ok(splice(&self.text, from, to, &block))
    }

    /// 插入一條劃線的原文（插入點照上面的規則）。換行改成這份檔的換行；後面一個空行。不更新屬性區。
    pub fn insert_highlight_raw(&self, raw: &str, labels: &NotesLabels) -> String {
        let nl = self.nl();
        let mut block = with_newlines(raw, nl);
        if !block.ends_with('\n') {
            block.push_str(nl);
        }
        block.push_str(nl);
        match self.highlights_insert_point() {
            Some(InsertAt::BeforeLine(line)) => {
                let at = self.lines[line].start;
                splice(&self.text, at, at, &block)
            }
            Some(InsertAt::AfterLine(line)) => {
                let at = self.lines[line].next;
                if at == self.text.len() && !self.text.ends_with('\n') {
                    let mut s = self.text.clone();
                    s.push_str(nl);
                    s.push_str(&block);
                    s
                } else {
                    splice(&self.text, at, at, &block)
                }
            }
            None => {
                let mut s = self.text.clone();
                ensure_trailing_newline(&mut s, nl);
                if !s.is_empty() {
                    s.push_str(nl);
                }
                s.push_str(&format!("## {}{nl}{nl}", labels.highlights));
                s.push_str(&format!("{MARKER_PREFIX}highlights {MARKER_SUFFIX}{nl}"));
                s.push_str(&block);
                s.push_str(&format!(
                    "{MARKER_PREFIX}highlights-end {MARKER_SUFFIX}{nl}"
                ));
                s
            }
        }
    }

    /// 標記以外的文字，照五步正規化之後（比較兩份筆記檔用）。
    pub fn outside_text(&self) -> String {
        let text = normalize_newlines(self.text.strip_prefix(BOM).unwrap_or(&self.text));
        let p = parse(&text);
        let n = p.lines.len();
        let mut drop = vec![false; n];
        // 2. 屬性區的三行
        let mut body_start = 0;
        if let Some((fs, fe)) = find_front_matter(&p.text, &p.lines) {
            for (i, d) in drop.iter_mut().enumerate().take(fe).skip(fs + 1) {
                let k = key_of(content(&p.text, &p.lines[i]));
                if matches!(
                    k,
                    "paperreader_paper" | "paperreader_modified" | "paperreader_modified_by"
                ) {
                    *d = true;
                }
            }
            body_start = fe + 1;
        }
        // 3. 應用管的各段：想法區（含兩行標記）、劃線區的兩行標記、每一條劃線（含讀不懂、不完整的）
        let markers = &p.markers;
        for (k, (line, m)) in markers.iter().enumerate() {
            let stop = |want: &Marker| match markers.get(k + 1) {
                Some((j, mm)) if mm == want => *j,
                Some((j, _)) => j - 1,
                None => n - 1,
            };
            match m {
                Marker::Thoughts => {
                    let s = stop(&Marker::ThoughtsEnd);
                    for d in drop.iter_mut().take(s + 1).skip(*line) {
                        *d = true;
                    }
                }
                Marker::Highlight(_) => {
                    let s = stop(&Marker::HighlightEnd);
                    for d in drop.iter_mut().take(s + 1).skip(*line) {
                        *d = true;
                    }
                }
                Marker::Highlights | Marker::HighlightsEnd => drop[*line] = true,
                _ => {}
            }
        }
        // 4. 應用寫的標題：屬性區之後第一個 `# ` 行；想法區、劃線區開始標記之前（中間只隔空行）的標題
        if let Some(i) = (body_start..n).find(|&i| content(&p.text, &p.lines[i]).starts_with("# "))
        {
            drop[i] = true;
        }
        for (line, m) in markers {
            if matches!(m, Marker::Thoughts | Marker::Highlights) {
                let mut j = *line;
                while j > body_start {
                    j -= 1;
                    let c = content(&p.text, &p.lines[j]);
                    if c.trim().is_empty() {
                        continue;
                    }
                    if c.starts_with('#') {
                        drop[j] = true;
                    }
                    break;
                }
            }
        }
        // 5. 行尾空白、連續空行併成一行、頭尾空行
        let mut out: Vec<&str> = Vec::new();
        for (i, d) in drop.iter().enumerate() {
            if *d {
                continue;
            }
            let c = content(&p.text, &p.lines[i]).trim_end();
            if c.is_empty() && out.last().is_some_and(|l| l.is_empty()) {
                continue;
            }
            out.push(c);
        }
        while out.first().is_some_and(|l| l.is_empty()) {
            out.remove(0);
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }
}

/// 把一段文字的換行改成 `nl`。
fn with_newlines(s: &str, nl: &str) -> String {
    let n = normalize_newlines(s);
    if nl == "\n" { n } else { n.replace('\n', nl) }
}

/// 更新屬性區的 `paperreader_modified`、`paperreader_modified_by`（合併用）。
pub fn stamp(text: &str, stamp: &Stamp) -> String {
    stamp_text(text, stamp)
}

/// 衝突副本：屬性區的鍵 `paperreader_paper` 改名成 `paperreader_conflict_of`（值不變），其餘逐位元組不動。
pub fn as_conflict_copy(text: &str) -> String {
    let lines = split_lines(text);
    let Some((fs, fe)) = find_front_matter(text, &lines) else {
        return text.to_string();
    };
    for l in lines.iter().take(fe).skip(fs + 1) {
        let c = content(text, l);
        if key_of(c) == "paperreader_paper" {
            let at = l.start + c.find("paperreader_paper").expect("key is there");
            return splice(
                text,
                at,
                at + "paperreader_paper".len(),
                "paperreader_conflict_of",
            );
        }
    }
    text.to_string()
}

/// 屬性區的 `paperreader_conflict_of`（衝突副本屬於哪一篇）。
pub fn conflict_of(text: &str) -> Option<String> {
    let lines = split_lines(text);
    let (s, e) = find_front_matter(text, &lines)?;
    (s + 1..e).find_map(|i| {
        let c = content(text, &lines[i]);
        if key_of(c) == "paperreader_conflict_of" {
            fm_value(c, "paperreader_conflict_of")
        } else {
            None
        }
    })
}

/// 換行一律轉成 `\n`（比對衝突副本是否相同用）。
pub fn unify_newlines(s: &str) -> String {
    normalize_newlines(s)
}

pub(crate) fn splice(text: &str, from: usize, to: usize, with: &str) -> String {
    let mut s = String::with_capacity(text.len() + with.len());
    s.push_str(&text[..from]);
    s.push_str(with);
    s.push_str(&text[to..]);
    s
}

fn ensure_trailing_newline(s: &mut String, nl: &str) {
    if !s.is_empty() && !s.ends_with('\n') {
        s.push_str(nl);
    }
}

pub(crate) fn escape_link_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 一條劃線的樣子（開頭標記到結尾標記，每行以 `nl` 結尾；後面的空行不含）。
///
/// `data_json` 是資料行的 JSON（新劃線由 `to_data` 產生；改短評時是原文只換掉 `modified`）。
/// 頁碼標籤只在看得到的 `**p. …**` 與連結文字上檢查：不合規則的用 `page.index + 1`；資料行不動。
fn render_block(
    h: &Highlight,
    data_json: &str,
    labels: &NotesLabels,
    paper_id: &str,
    nl: &str,
) -> String {
    let shown =
        sanitize_label(h.page.label.as_deref()).unwrap_or_else(|| (h.page.index + 1).to_string());
    let mut s = String::new();
    s.push_str(&format!(
        "{MARKER_PREFIX}highlight {data_json} {MARKER_SUFFIX}{nl}"
    ));
    s.push_str(&format!("**p. {shown}**{nl}{nl}"));
    for line in normalize_newlines(&h.quote.exact).split('\n') {
        if line.is_empty() {
            s.push('>');
        } else {
            s.push_str("> ");
            s.push_str(line);
        }
        s.push_str(nl);
    }
    s.push_str(nl);
    s.push_str(&format!("^{}{nl}{nl}", h.id));
    if !h.comment.is_empty() {
        for line in h.comment.split('\n') {
            s.push_str(line);
            s.push_str(nl);
        }
        s.push_str(nl);
    }
    s.push_str(&format!(
        "[{} p. {}](paperreader://open?paper={}&material={}&page={}&highlight={}){nl}",
        escape_link_text(&labels.back_link),
        shown,
        paper_id,
        h.material,
        h.page.index + 1,
        h.id
    ));
    s.push_str(&format!("{MARKER_PREFIX}highlight-end {MARKER_SUFFIX}{nl}"));
    s
}

/// 新的筆記檔（第一次寫入時）。
pub fn new_file(paper_id: &str, title: &str, labels: &NotesLabels, stamp: &Stamp) -> String {
    let title = clean_title(title);
    let title = if title.is_empty() { "untitled" } else { &title };
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("paperreader_paper: {paper_id}\n"));
    s.push_str(&format!("paperreader_modified: {}\n", stamp.time));
    s.push_str(&format!("paperreader_modified_by: {}\n", stamp.device));
    s.push_str("---\n\n");
    s.push_str(&format!("# {title}\n\n"));
    s.push_str(&format!("## {}\n\n", labels.thoughts));
    s.push_str(&format!("{MARKER_PREFIX}thoughts {MARKER_SUFFIX}\n"));
    s.push_str(&format!("{MARKER_PREFIX}thoughts-end {MARKER_SUFFIX}\n\n"));
    s.push_str(&format!("## {}\n\n", labels.highlights));
    s.push_str(&format!("{MARKER_PREFIX}highlights {MARKER_SUFFIX}\n"));
    s.push_str(&format!("{MARKER_PREFIX}highlights-end {MARKER_SUFFIX}\n"));
    s
}

/// 更新屬性區的兩行：有那一行就把整行換成 `鍵: 值`；沒有就在 `paperreader_paper` 那一行之後
/// （也沒有就在屬性區結束行之前）插入。沒有屬性區時原樣交回。其餘的行逐位元組不動。
fn stamp_text(text: &str, stamp: &Stamp) -> String {
    let lines = split_lines(text);
    let Some((fs, fe)) = find_front_matter(text, &lines) else {
        return text.to_string();
    };
    let wanted = [
        ("paperreader_modified", stamp.time.as_str()),
        ("paperreader_modified_by", stamp.device.as_str()),
    ];
    // 每個鍵對應哪一行（已有的）
    let mut found: [Option<usize>; 2] = [None, None];
    let mut paper_line = None;
    for (i, line) in lines.iter().enumerate().take(fe).skip(fs + 1) {
        let c = content(text, line);
        if paper_line.is_none() && fm_value(c, "paperreader_paper").is_some() {
            paper_line = Some(i);
        }
        for (k, (key, _)) in wanted.iter().enumerate() {
            if found[k].is_none() && fm_value(c, key).is_some() && key_of(c) == *key {
                found[k] = Some(i);
            }
        }
    }
    let insert_after = paper_line.unwrap_or(fe - 1);
    let mut out = String::with_capacity(text.len() + 100);
    let nl_of = |i: usize| -> &str {
        let l = &lines[i];
        let t = &text[l.end..l.next];
        if t.is_empty() { "\n" } else { t }
    };
    for (i, l) in lines.iter().enumerate() {
        if let Some(k) = found.iter().position(|f| *f == Some(i)) {
            out.push_str(&format!("{}: {}{}", wanted[k].0, wanted[k].1, nl_of(i)));
        } else {
            out.push_str(&text[l.start..l.next]);
            // 屬性區最後一行也可能沒有換行（不會：後面還有結束行），所以這裡只在行尾有換行時才補
        }
        if i == insert_after {
            // 這一行若沒有換行（insert_after 是屬性區的最後一行以前的行，一定有換行）
            for (k, (key, value)) in wanted.iter().enumerate() {
                if found[k].is_none() {
                    out.push_str(&format!("{key}: {value}{}", nl_of(i)));
                }
            }
        }
    }
    out
}

/// 一行 `key: value` 的鍵。
pub(crate) fn key_of(line: &str) -> &str {
    line.trim_start()
        .split_once(':')
        .map_or("", |(k, _)| k.trim_end())
}
