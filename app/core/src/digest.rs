//! 重點筆記：一份人看得懂、筆記工具打得開的 Markdown。
//! 這個模組是純計算（產生、解析、改幾行、兩份合併），讀寫檔案在 `local/digest.rs`。
//!
//! # 在文獻庫裡的位置
//!
//! 檔案放在那一篇的資料夾裡，檔名 `digest.md`（被占用時 `digest-2.md` …；資料夾裡的項目與清單裡別的材料的
//! 檔名都算占用，不分大小寫），並在 `paper.json` 的材料清單加一項：`role` 是 `digest`、`form` 是 `markdown`
//! （這個角色在材料格式裡已保留）。
//!
//! 使用者可以在筆記工具裡改這份檔。所以照譯文疊加檔的做法，**材料的 `sha256` 與 `bytes` 一律是 `null`**：
//! 檔案在應用以外被改過不算問題，`check` 對它只確認讀得懂（`UnreadableDigestFile`），不比大小與指紋；
//! 應用讀到的就是改過的內容，下一次寫入從那份內容改起。刪一點只寫這份檔；標為已校對另外更新材料的
//! `provenance.proofread`（與屬性區一致）。材料的 `provenance`：`method` `generated`、`page_map` `marked-in-text`、
//! `source_sha256` 是產生時原文的指紋。
//!
//! 同步：依每一點的代號合併，看 `digest-point-deleted` 記錄；同一點兩邊文字不同時留本機的，
//! 匯入的那一份另存衝突副本；點以外的文字不同也另存；「已校對」兩邊有一邊是就算是（`merge`）。
//!
//! # 檔案內容
//!
//! ```markdown
//! ---
//! paperreader_digest: 1
//! paperreader_digest_of: <文獻代號>
//! paperreader_source_material: <原文材料代號>
//! paperreader_source_sha256: <產生時原文的指紋；不知道是 null>
//! paperreader_service: "Claude（Anthropic）"
//! paperreader_provider: anthropic
//! paperreader_model: "claude-sonnet-5-5"
//! paperreader_lang: zh-TW
//! paperreader_sections: [{"id":"question","label":"研究問題"},{"id":"method","label":"方法"}]
//! paperreader_created: 2026-11-16T08:00:00.000Z
//! paperreader_created_by: <裝置代號>
//! paperreader_modified: 2026-11-16T08:00:00.000Z
//! paperreader_modified_by: <裝置代號>
//! paperreader_proofread: false
//! ---
//!
//! # <篇名>
//!
//! ## 研究問題
//!
//! - 即時偵測器的速度與準確率難以兼顧。 [p. 1](paperreader://open?paper=…&material=…&page=1) <!-- paperreader:point {"v":1,"id":"d-01jck…","section":"question","pages":[0],"segments":["g0003"],"created":"…"} -->
//! ```
//!
//! - 屬性區的鍵照筆記檔的寫法（`paperreader_` 開頭、逐行比對，不用 YAML 函式庫）。自由文字的值
//!   （服務名稱、模型）寫成 JSON 字串，同時是合法的 YAML 雙引號字串。`paperreader_digest` 是格式版本；
//!   沒有這個鍵的檔不是重點筆記，讀不懂、不寫。用 `paperreader_digest_of` 而不用 `paperreader_paper`：
//!   這份檔被搬進筆記資料夾時，不會被當成那篇的筆記檔。衝突副本把這個鍵改名成 `paperreader_conflict_of`。
//! - **每一點是一行**：清單項目、回原文的連結（每個頁碼一個，連結的 `page=` 從 1 起算）、最後是看不見的資料
//!   `<!-- paperreader:point {JSON} -->`。JSON 裡的小於、大於號寫成 `<`、`>`。資料的鍵：`v`（1）、
//!   `id`（`d-` 加小寫的 ULID：每一點有自己的代號）、`section`、`pages`（原文的頁序，從 0 起算，1 到 8 個）、
//!   `segments`（產生時的段落代號，只當輔助）、`created`（每一點有自己的時間）。
//!   不認得的鍵原樣保留。
//! - 點的文字照 Markdown 的規則跳脫：`\`、`<`、`[`、`]` 前面加反斜線，所以模型交來的圖片、HTML、
//!   連結在筆記工具裡都是純文字；讀的時候還原。篇名去掉註解標記。
//! - 區段（使用者可以增刪改）：屬性區的 `paperreader_sections` 依順序記下每個區段的代號與名稱，
//!   寫成一行 JSON（`[{"id":"question","label":"研究問題"},…]`，也是合法的 YAML 流式清單）。每一點的 `section`
//!   是其中一個代號；依這個順序分節，`## ` 標題是區段的名稱（產生時交來），只給人看，解析不看它。
//!   沒有這個鍵的檔（這一版之前寫的）照原本的五區讀：`question`（研究問題）、`method`（方法）、`results`（主要結果）、
//!   `limitations`（限制）、`quotes`（值得記下的引句或數字），名稱空白，由畫面的文字表補上。
//! - 解析只看資料，不看標題與清單符號。看得到的文字以檔案為準：使用者在筆記工具裡改了那一點的文字，
//!   讀到的就是改過的（去掉行首的清單符號與行尾回原文的連結）。
//! - 改檔只換掉要改的那幾行（刪一點刪那一行，同代號被複製的行一起刪；屬性區換值），其餘逐位元組不動
//!   （含 `\r\n`、使用者自己的段落）。資料讀不懂的那一行列進 `unreadable`，任何寫入都不動它。

use crate::error::{LibError, Result};
use crate::format::{Map, Material};
use crate::notes::{
    Line, MARKER_SUFFIX, UnreadableBlock, clean_title, content, find_front_matter, key_of, splice,
    split_lines,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const ROLE_DIGEST: &str = "digest";
pub const FORM_MARKDOWN: &str = "markdown";
/// 這一版寫出、認得的格式版本。
pub const DIGEST_FORMAT: u32 = 1;
/// 出處的 `page_map`：頁碼寫在文字裡。
pub const PAGE_MAP_MARKED: &str = "marked-in-text";

/// 沒有記下區段的檔（這一版之前寫的）用的區段，依顯示的順序（原先固定的格式）。
pub const DEFAULT_SECTIONS: [&str; 5] = ["question", "method", "results", "limitations", "quotes"];

/// 一份最多幾個區段；區段代號最長幾個位元組；名稱最多幾個字。
pub const MAX_SECTIONS: usize = 12;
pub const MAX_SECTION_ID: usize = 32;
pub const MAX_SECTION_LABEL: usize = 100;

/// 一份最多幾點、一點最多幾個字、幾個頁碼、幾個段落代號。
pub const MAX_POINTS: usize = 200;
pub const MAX_POINT_CHARS: usize = 1000;
pub const MAX_POINT_PAGES: usize = 8;
pub const MAX_POINT_SEGMENTS: usize = 16;
/// 頁序的上限（與譯文疊加檔相同）。
const MAX_PAGE_INDEX: u32 = 100_000;

const POINT_MARKER: &str = "<!-- paperreader:point ";
const KEY_FORMAT: &str = "paperreader_digest";
const KEY_OF: &str = "paperreader_digest_of";
const KEY_CONFLICT_OF: &str = "paperreader_conflict_of";
const KEY_SECTIONS: &str = "paperreader_sections";

fn invalid(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 這項材料是不是重點筆記。
pub fn is_digest(m: &Material) -> bool {
    m.role == ROLE_DIGEST && m.form == FORM_MARKDOWN
}

// ---- 公開的型別 ----

/// 一個區段（使用者可以增刪改）：代號寫在每一點的資料裡（`section`），名稱是 `## ` 標題的文字
/// （產生的一方交來；內建的區段跟著輸出的語言）。依清單的順序寫出與顯示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestSection {
    pub id: String,
    pub label: String,
}

/// 區段代號的寫法：1 到 `MAX_SECTION_ID` 個小寫英文字母、數字、底線（它是回覆 JSON 的鍵，也寫進每一點的資料）。
pub fn section_id_ok(id: &str) -> bool {
    (1..=MAX_SECTION_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 沒有記下區段的檔用的區段：原本的五區，名稱是空的（畫面用自己的文字表）。
pub fn default_sections() -> Vec<DigestSection> {
    DEFAULT_SECTIONS
        .iter()
        .map(|id| DigestSection {
            id: (*id).to_string(),
            label: String::new(),
        })
        .collect()
}

/// 區段清單合不合規則：1 到 `MAX_SECTIONS` 個、代號合寫法且不重複、名稱是一行給人看的文字。
pub fn validate_sections(sections: &[DigestSection]) -> Result<()> {
    if sections.is_empty() || sections.len() > MAX_SECTIONS {
        return Err(invalid(format!(
            "a digest has 1 to {MAX_SECTIONS} sections"
        )));
    }
    for (i, s) in sections.iter().enumerate() {
        if !section_id_ok(&s.id) || sections[..i].iter().any(|o| o.id == s.id) {
            return Err(invalid(format!("section id {:?} is not valid", s.id)));
        }
        check_line_text(&s.label, MAX_SECTION_LABEL, "section label")?;
        // 名稱不重複：同名的兩區在筆記工具裡分不出來
        if sections[..i]
            .iter()
            .any(|o| o.label.trim() == s.label.trim())
        {
            return Err(invalid(format!(
                "section label {:?} is used twice",
                s.label
            )));
        }
    }
    Ok(())
}

/// 屬性區裡的區段：一行 JSON（應用寫的），或筆記工具改寫成的區塊式 YAML（`- id: …` 與 `label: …`）。
/// 交回 `None` 是沒有這個鍵；`Some(Err(行號))` 是有但讀不懂（行號從 0 起算）。
fn read_sections(
    text: &str,
    lines: &[Line],
    front: (usize, usize),
) -> Option<std::result::Result<Vec<DigestSection>, usize>> {
    let at = (front.0 + 1..front.1).find(|&i| key_of(content(text, &lines[i])) == KEY_SECTIONS)?;
    let first = content(text, &lines[at]);
    let value = first.trim_start()[KEY_SECTIONS.len()..]
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
        .unwrap_or("");
    let ok = |s: Vec<DigestSection>| validate_sections(&s).is_ok().then_some(s);
    if !value.is_empty() {
        return Some(
            serde_json::from_str::<Vec<DigestSection>>(value)
                .ok()
                .and_then(ok)
                .ok_or(at),
        );
    }
    // 區塊式：接在後面、縮排或以 `- ` 開頭的幾行
    let unquote = |v: &str| -> String {
        let v = v.trim();
        if v.starts_with('"') {
            serde_json::from_str::<String>(v).unwrap_or_else(|_| v.to_string())
        } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
            v[1..v.len() - 1].replace("''", "'")
        } else {
            v.to_string()
        }
    };
    let mut items: Vec<(Option<String>, Option<String>)> = Vec::new();
    for l in &lines[at + 1..front.1] {
        let c = content(text, l);
        if !(c.starts_with(' ') || c.starts_with('\t') || c.starts_with('-')) {
            break;
        }
        let mut t = c.trim();
        if let Some(rest) = t.strip_prefix('-') {
            items.push((None, None));
            t = rest.trim();
        }
        if t.is_empty() {
            continue;
        }
        let (Some(item), Some((k, v))) = (items.last_mut(), t.split_once(':')) else {
            return Some(Err(at));
        };
        match k.trim() {
            "id" => item.0 = Some(unquote(v)),
            "label" => item.1 = Some(unquote(v)),
            _ => {}
        }
    }
    let sections: Option<Vec<DigestSection>> = items
        .into_iter()
        .map(|(id, label)| {
            Some(DigestSection {
                id: id?,
                label: label?,
            })
        })
        .collect();
    Some(sections.and_then(ok).ok_or(at))
}

/// 交來的一點。`pages` 是原文的頁序（從 0 起算）。
#[derive(Debug, Clone, PartialEq)]
pub struct PointDraft {
    pub section: String,
    pub text: String,
    pub pages: Vec<u32>,
    pub segments: Vec<String>,
}

/// 建立一份重點筆記要的資料。核心當成外來的內容，先檢查才寫（架構 5.3 規則 4）。
#[derive(Debug, Clone, PartialEq)]
pub struct DigestDraft {
    /// 依據哪一份原文材料（頁碼的範圍照它的頁數檢查）。
    pub source_material: String,
    /// 服務的名稱（給人看）。
    pub service: String,
    /// 引擎種類（例如 `anthropic`）。
    pub provider: String,
    pub model: String,
    /// 輸出的語言（例如 `zh-TW`）。
    pub lang: String,
    /// `# ` 標題（篇名）。
    pub title: String,
    /// 區段，依寫出的順序（記進屬性區的 `paperreader_sections`）。
    pub sections: Vec<DigestSection>,
    pub points: Vec<PointDraft>,
}

/// 讀出來的一點。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestPoint {
    pub id: String,
    pub section: String,
    /// 看得到的文字（以檔案為準）。
    pub text: String,
    pub pages: Vec<u32>,
    pub segments: Vec<String>,
    pub created: String,
}

/// 一份重點筆記（交給畫面）。屬性區缺的鍵是 `None`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DigestDoc {
    pub format: u32,
    pub paper: Option<String>,
    pub source_material: Option<String>,
    pub source_sha256: Option<String>,
    pub service: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub lang: Option<String>,
    pub created: Option<String>,
    pub created_by: Option<String>,
    pub modified: Option<String>,
    pub modified_by: Option<String>,
    pub proofread: bool,
    /// 區段，依顯示的順序。屬性區沒有 `paperreader_sections`（這一版之前寫的）時是原本的五區、
    /// 名稱空白（`default_sections`）；有但讀不懂時照每一點前面的 `## ` 標題與點的順序（`sections_unreadable`）。
    pub sections: Vec<DigestSection>,
    /// 屬性區的區段讀不懂（那一行也列在 `unreadable`）。
    pub sections_unreadable: bool,
    /// 屬性區之後第一個 `# ` 標題。
    pub title: Option<String>,
    /// 依檔案裡的順序。
    pub points: Vec<DigestPoint>,
    pub unreadable: Vec<UnreadableBlock>,
}

/// 建立重點筆記的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatedDigest {
    /// 新材料的代號。
    pub material_id: String,
    /// `paper.json` 新的版本記號。
    pub paper_token: crate::jsonio::VersionToken,
    /// 重點筆記檔的版本記號（接著刪一點、標已校對時帶這個）。
    pub digest_token: crate::jsonio::VersionToken,
}

/// 寫入時蓋上的時間與裝置。
#[derive(Debug, Clone, PartialEq)]
pub struct DigestStamp {
    pub time: String,
    pub device: String,
}

// ---- 資料 ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PointData {
    v: u32,
    id: String,
    section: String,
    pages: Vec<u32>,
    segments: Vec<String>,
    created: String,
    #[serde(flatten)]
    extra: Map,
}

fn data_json(d: &PointData) -> String {
    let s = serde_json::to_string(d).expect("point data serializes");
    s.replace('<', "\\u003c").replace('>', "\\u003e")
}

fn id_ok(id: &str) -> bool {
    id.len() > 2
        && id.len() <= 64
        && id.starts_with("d-")
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn segment_ok(s: &str) -> bool {
    (1..=20).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 新的一點的代號：`d-` 加小寫的 ULID。
pub fn new_point_id() -> String {
    format!("d-{}", crate::ids::new_id().to_lowercase())
}

// ---- 文字 ----

/// 去掉註解標記，直到一個都不剩（只取代一次時，拼起來又是一個標記）。
pub fn strip_markers(s: &str) -> String {
    let mut t = s.to_string();
    loop {
        let n = t.replace("<!--", "").replace("-->", "");
        if n == t {
            return n;
        }
        t = n;
    }
}

/// 點的文字寫進檔案：`\`、`<`、`[`、`]` 前面加反斜線。
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '<' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `escape_text` 的反向：反斜線加 `\`、`<`、`[`、`]` 還原成那個字。其他反斜線原樣。
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\'
            && let Some(&n) = it.peek()
            && matches!(n, '\\' | '<' | '[' | ']')
        {
            out.push(n);
            it.next();
            continue;
        }
        out.push(c);
    }
    out
}

// ---- 檢查（架構 5.3 規則 4）----

/// 一段給人看的文字：去掉前後空白後 1 到 `max` 個字、沒有控制字元（含換行），
/// 不含 `<!--` 與 `-->`（會弄亂看不見的資料）。
fn check_line_text(s: &str, max: usize, what: &str) -> Result<()> {
    let t = s.trim();
    let n = t.chars().count();
    if n == 0 || n > max {
        return Err(invalid(format!("{what} must be 1 to {max} characters")));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{what} must not contain control characters"
        )));
    }
    if t.contains("<!--") || t.contains("-->") {
        return Err(invalid(format!("{what} must not contain <!-- or -->")));
    }
    Ok(())
}

fn check_word(s: &str, max: usize, what: &str, ok: impl Fn(u8) -> bool) -> Result<()> {
    if s.is_empty() || s.len() > max || !s.bytes().all(ok) {
        return Err(invalid(format!("{what} is not valid")));
    }
    Ok(())
}

/// 一點合不合規則（區段在 `sections` 裡、文字、頁碼範圍、段落代號）。`source_pages` 是原文的頁數。
/// 外殼交給核心之前也用它濾掉不合的點。
pub fn validate_point(p: &PointDraft, sections: &[DigestSection], source_pages: u32) -> Result<()> {
    if !sections.iter().any(|s| s.id == p.section) {
        return Err(invalid(format!("unknown section {:?}", p.section)));
    }
    check_line_text(&p.text, MAX_POINT_CHARS, "point text")?;
    if p.pages.is_empty() || p.pages.len() > MAX_POINT_PAGES {
        return Err(invalid(format!("1 to {MAX_POINT_PAGES} pages are needed")));
    }
    if let Some(&pg) = p
        .pages
        .iter()
        .find(|&&pg| pg >= source_pages.min(MAX_PAGE_INDEX))
    {
        return Err(invalid(format!(
            "page index {pg} is beyond the {source_pages} pages of the source"
        )));
    }
    if p.segments.len() > MAX_POINT_SEGMENTS || !p.segments.iter().all(|s| segment_ok(s)) {
        return Err(invalid("segment ids are not valid"));
    }
    Ok(())
}

/// 檢查交來的內容。`source_pages` 是原文的頁數（頁序必須小於它）。
pub fn validate_draft(d: &DigestDraft, source_pages: u32) -> Result<()> {
    if d.points.is_empty() || d.points.len() > MAX_POINTS {
        return Err(invalid(format!("a digest has 1 to {MAX_POINTS} points")));
    }
    validate_sections(&d.sections)?;
    check_word(&d.provider, 40, "provider", |b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
    })?;
    check_word(&d.lang, 20, "language", |b| {
        b.is_ascii_alphanumeric() || b == b'-'
    })?;
    for (what, s, max) in [("service", &d.service, 100), ("model", &d.model, 200)] {
        if s.chars().count() > max || s.chars().any(char::is_control) {
            return Err(invalid(format!("{what} must be at most {max} characters")));
        }
    }
    for (i, p) in d.points.iter().enumerate() {
        validate_point(p, &d.sections, source_pages).map_err(|e| match e {
            LibError::InvalidInput { reason } => invalid(format!("point {i}: {reason}")),
            other => other,
        })?;
    }
    Ok(())
}

// ---- 產生 ----

/// 屬性區的字串值：JSON 字串（也是合法的 YAML 雙引號字串）。
fn quoted(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

/// 一點的那一行（不含換行）。`text` 已經跳脫好。
fn point_line(paper_id: &str, source: &str, text: &str, data: &PointData) -> String {
    let mut s = String::from("- ");
    s.push_str(text);
    for pg in &data.pages {
        s.push_str(&format!(
            " [p. {n}](paperreader://open?paper={paper_id}&material={source}&page={n})",
            n = pg + 1
        ));
    }
    s.push(' ');
    s.push_str(POINT_MARKER);
    s.push_str(&data_json(data));
    s.push(' ');
    s.push_str(MARKER_SUFFIX);
    s
}

/// 一份新的重點筆記。先用 `validate_draft` 檢查過。
pub(crate) fn render(
    d: &DigestDraft,
    paper_id: &str,
    source_sha256: Option<&str>,
    stamp: &DigestStamp,
) -> String {
    // 篇名不可以帶註解標記：否則 `# ` 那一行會被讀成一點
    let title = clean_title(&strip_markers(&d.title));
    let title = if title.trim().is_empty() {
        "untitled"
    } else {
        title.trim()
    };
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("{KEY_FORMAT}: {DIGEST_FORMAT}\n"));
    s.push_str(&format!("{KEY_OF}: {paper_id}\n"));
    s.push_str(&format!(
        "paperreader_source_material: {}\n",
        d.source_material
    ));
    s.push_str(&format!(
        "paperreader_source_sha256: {}\n",
        source_sha256.unwrap_or("null")
    ));
    s.push_str(&format!("paperreader_service: {}\n", quoted(&d.service)));
    s.push_str(&format!("paperreader_provider: {}\n", d.provider));
    s.push_str(&format!("paperreader_model: {}\n", quoted(&d.model)));
    s.push_str(&format!("paperreader_lang: {}\n", d.lang));
    // 區段的代號與名稱：一行 JSON（也是合法的 YAML 流式清單），依順序
    let sections = serde_json::to_string(&d.sections).expect("sections serialize");
    s.push_str(&format!("{KEY_SECTIONS}: {sections}\n"));
    s.push_str(&format!("paperreader_created: {}\n", stamp.time));
    s.push_str(&format!("paperreader_created_by: {}\n", stamp.device));
    s.push_str(&format!("paperreader_modified: {}\n", stamp.time));
    s.push_str(&format!("paperreader_modified_by: {}\n", stamp.device));
    s.push_str("paperreader_proofread: false\n");
    s.push_str("---\n\n");
    s.push_str(&format!("# {title}\n"));
    for sec in &d.sections {
        let section = sec.id.as_str();
        let points: Vec<&PointDraft> = d.points.iter().filter(|p| p.section == section).collect();
        if points.is_empty() {
            continue;
        }
        s.push_str(&format!("\n## {}\n\n", sec.label.trim()));
        for p in points {
            let mut pages = p.pages.clone();
            pages.sort_unstable();
            pages.dedup();
            let data = PointData {
                v: 1,
                id: new_point_id(),
                section: section.to_string(),
                pages,
                segments: p.segments.clone(),
                created: stamp.time.clone(),
                extra: Map::new(),
            };
            let text = escape_text(p.text.trim());
            s.push_str(&point_line(paper_id, &d.source_material, &text, &data));
            s.push('\n');
        }
    }
    s
}

// ---- 解析 ----

/// 一行裡看不見的資料：交回（標記開始的位置, JSON）。行尾必須是 `-->`。
fn split_point_line(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let body = t.strip_suffix(MARKER_SUFFIX)?;
    let at = body.rfind(POINT_MARKER)?;
    Some((at, body[at + POINT_MARKER.len()..].trim()))
}

/// 看得到的文字：去掉行首的空白與清單符號、行尾回原文的連結（跳脫過的 `\]` 不是連結），再還原跳脫。
fn visible_text(before_marker: &str) -> String {
    let mut s = before_marker.trim();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = s.strip_prefix(bullet) {
            s = rest.trim_start();
            break;
        }
    }
    loop {
        let t = s.trim_end();
        let Some(head) = t.strip_suffix(')') else {
            s = t;
            break;
        };
        let Some(open) = head.rfind("](paperreader://") else {
            s = t;
            break;
        };
        if head[open..].contains(')') || head[..open].ends_with('\\') {
            s = t;
            break;
        }
        let Some(lb) = head[..open].rfind('[') else {
            s = t;
            break;
        };
        if head[lb..open].contains(']') {
            s = t;
            break;
        }
        s = &t[..lb];
    }
    unescape_text(s.trim())
}

fn parse_data(json: &str) -> std::result::Result<PointData, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let Value::Object(_) = &v else {
        return Err("data is not a JSON object".into());
    };
    let d: PointData = serde_json::from_value(v).map_err(|e| format!("bad data: {e}"))?;
    if d.v > 1 {
        return Err(format!("data version {} is newer", d.v));
    }
    if !id_ok(&d.id) {
        return Err(format!("bad id {:?}", d.id));
    }
    if d.pages.is_empty() || d.pages.iter().any(|&p| p >= MAX_PAGE_INDEX) {
        return Err("bad pages".into());
    }
    Ok(d)
}

/// 一行資料裡的代號（資料不必完全合規則；刪一點時同代號的行一起刪用）。
fn line_id(line: &str) -> Option<String> {
    let (_, json) = split_point_line(line)?;
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("id")?.as_str().map(String::from)
}

/// 屬性區一個鍵的值：JSON 字串就解開；`null` 或空白是 `None`。
fn fm_get(text: &str, lines: &[Line], front: (usize, usize), key: &str) -> Option<String> {
    let (s, e) = front;
    (s + 1..e).find_map(|i| {
        let c = content(text, &lines[i]);
        if key_of(c) != key {
            return None;
        }
        let v = c.trim_start()[key.len()..]
            .trim_start()
            .strip_prefix(':')?
            .trim();
        if v.is_empty() || v == "null" || v == "~" {
            return None;
        }
        if v.starts_with('"') {
            return serde_json::from_str::<String>(v).ok();
        }
        if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
            return Some(v[1..v.len() - 1].to_string());
        }
        Some(v.to_string())
    })
}

#[derive(Debug, Clone)]
struct PointAt {
    line: usize,
    point: DigestPoint,
}

/// 解析的結果。保留原文，之後的修改只換掉要改的那幾行。
#[derive(Debug, Clone)]
pub(crate) struct ParsedDigest {
    text: String,
    lines: Vec<Line>,
    front: (usize, usize),
    doc: DigestDoc,
    points: Vec<PointAt>,
}

/// 解析。沒有屬性區、或屬性區沒有 `paperreader_digest`（不是重點筆記）交回錯誤的原因。
pub(crate) fn parse(text: &str) -> std::result::Result<ParsedDigest, String> {
    let lines = split_lines(text);
    let front = find_front_matter(text, &lines).ok_or("no front matter")?;
    let get = |k: &str| fm_get(text, &lines, front, k);
    let format: u32 = get(KEY_FORMAT)
        .ok_or("not a digest: no paperreader_digest")?
        .parse()
        .map_err(|_| "paperreader_digest is not a number")?;
    if format == 0 {
        return Err("paperreader_digest is not a version".into());
    }
    let title = lines[front.1 + 1..].iter().find_map(|l| {
        let c = content(text, l);
        c.strip_prefix("# ").map(|t| t.trim().to_string())
    });
    let mut points: Vec<PointAt> = Vec::new();
    let mut unreadable = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(front.1 + 1) {
        let c = content(text, l);
        let Some((at, json)) = split_point_line(c) else {
            continue;
        };
        match parse_data(json) {
            Ok(d) if points.iter().any(|p| p.point.id == d.id) => {
                unreadable.push(UnreadableBlock {
                    line: i + 1,
                    text: c.to_string(),
                    reason: format!("duplicate id {}", d.id),
                })
            }
            Ok(d) => points.push(PointAt {
                line: i,
                point: DigestPoint {
                    id: d.id,
                    section: d.section,
                    text: visible_text(&c[..at]),
                    pages: d.pages,
                    segments: d.segments,
                    created: d.created,
                },
            }),
            Err(reason) => unreadable.push(UnreadableBlock {
                line: i + 1,
                text: c.to_string(),
                reason,
            }),
        }
    }
    let (sections, sections_unreadable) = match read_sections(text, &lines, front) {
        None => (default_sections(), false),
        Some(Ok(s)) => (s, false),
        Some(Err(at)) => {
            // 讀不懂：列出來（不默默換成五區），區段照每一點前面的 `## ` 標題與點的順序
            unreadable.insert(
                0,
                UnreadableBlock {
                    line: at + 1,
                    text: content(text, &lines[at]).to_string(),
                    reason: format!("{KEY_SECTIONS} is not readable"),
                },
            );
            let mut s: Vec<DigestSection> = Vec::new();
            for p in &points {
                if s.iter().any(|x| x.id == p.point.section) {
                    continue;
                }
                let label = (front.1 + 1..p.line)
                    .rev()
                    .map(|i| content(text, &lines[i]))
                    .find_map(|c| c.strip_prefix("## "))
                    .map(|h| h.trim().to_string())
                    .unwrap_or_default();
                s.push(DigestSection {
                    id: p.point.section.clone(),
                    label,
                });
            }
            (s, true)
        }
    };
    let doc = DigestDoc {
        format,
        paper: get(KEY_OF),
        source_material: get("paperreader_source_material"),
        source_sha256: get("paperreader_source_sha256"),
        service: get("paperreader_service"),
        provider: get("paperreader_provider"),
        model: get("paperreader_model"),
        lang: get("paperreader_lang"),
        created: get("paperreader_created"),
        created_by: get("paperreader_created_by"),
        modified: get("paperreader_modified"),
        modified_by: get("paperreader_modified_by"),
        proofread: get("paperreader_proofread").as_deref() == Some("true"),
        sections,
        sections_unreadable,
        title,
        points: points.iter().map(|p| p.point.clone()).collect(),
        unreadable,
    };
    Ok(ParsedDigest {
        text: text.to_string(),
        lines,
        front,
        doc,
        points,
    })
}

impl ParsedDigest {
    pub(crate) fn doc(&self) -> &DigestDoc {
        &self.doc
    }

    pub(crate) fn into_doc(self) -> DigestDoc {
        self.doc
    }

    /// 這份檔用的換行（有 `\r\n` 就用它）。
    fn newline(&self) -> &'static str {
        if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// 拿掉這幾行（行號從 0 起算）之後的內容。
    fn without_lines(&self, drop: &HashSet<usize>) -> String {
        let mut s = String::with_capacity(self.text.len());
        for (i, l) in self.lines.iter().enumerate() {
            if !drop.contains(&i) {
                s.push_str(&self.text[l.start..l.next]);
            }
        }
        s
    }

    /// 這個代號的每一行（含被複製、列為讀不懂的那幾行）。
    fn lines_of(&self, id: &str) -> HashSet<usize> {
        (self.front.1 + 1..self.lines.len())
            .filter(|&i| line_id(content(&self.text, &self.lines[i])).as_deref() == Some(id))
            .collect()
    }

    /// 刪掉一點（那一行，同代號被複製的行一起）並蓋上時間與裝置。找不到交回 `NotFound`。
    pub(crate) fn without_point(&self, id: &str, stamp: &DigestStamp) -> Result<String> {
        if !self.points.iter().any(|p| p.point.id == id) {
            return Err(LibError::NotFound {
                what: format!("digest point {id}"),
            });
        }
        Ok(stamped(&self.without_lines(&self.lines_of(id)), stamp, &[]))
    }

    /// 標為已校對（或改回未校對）。已經是這個狀態交回 `None`。
    pub(crate) fn with_proofread(&self, proofread: bool, stamp: &DigestStamp) -> Option<String> {
        if self.doc.proofread == proofread {
            return None;
        }
        let v = if proofread { "true" } else { "false" };
        Some(stamped(&self.text, stamp, &[("paperreader_proofread", v)]))
    }

    /// 點以外的文字（比較兩份有沒有使用者自己加的東西用）：屬性區 `paperreader_` 以外的鍵，加上本文裡
    /// 不是點的各行；行尾空白去掉、連續空行併成一行、頭尾空行去掉。
    fn outside_text(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        // 屬性區裡 `paperreader_` 鍵的區塊式續行（筆記工具改寫的 `- id: …`）也是應用的
        let mut in_ours = false;
        for (i, l) in self.lines.iter().enumerate() {
            let c = content(&self.text, l).trim_end();
            if i == self.front.0 || i == self.front.1 {
                continue;
            }
            if i < self.front.1 {
                let continued = c.starts_with(' ') || c.starts_with('\t') || c.starts_with('-');
                if continued && in_ours {
                    continue;
                }
                in_ours = key_of(c).starts_with("paperreader_");
                if in_ours {
                    continue;
                }
            } else if split_point_line(c).is_some() {
                continue;
            }
            if c.is_empty() && out.last().is_none_or(|p| p.is_empty()) {
                continue;
            }
            out.push(c);
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }

    /// 一點所在的那一行的原文（不含換行）。
    fn line_text(&self, at: &PointAt) -> &str {
        content(&self.text, &self.lines[at.line])
    }

    /// 這一點前面最近的 `## ` 標題（沒有是 `None`）。
    fn heading_before(&self, line: usize) -> Option<&str> {
        (self.front.1 + 1..line)
            .rev()
            .map(|i| content(&self.text, &self.lines[i]))
            .find(|c| c.starts_with("## "))
    }
}

/// 換掉屬性區幾個鍵的值（含 `paperreader_modified`、`paperreader_modified_by`）：有那一行就只換那一行
/// （保留它自己的換行），沒有就加在屬性區結束行之前。其餘逐位元組不動。
fn stamped(text: &str, stamp: &DigestStamp, more: &[(&str, &str)]) -> String {
    let mut wanted: Vec<(&str, &str)> = vec![
        ("paperreader_modified", stamp.time.as_str()),
        ("paperreader_modified_by", stamp.device.as_str()),
    ];
    wanted.extend_from_slice(more);
    let lines = split_lines(text);
    let Some((fs, fe)) = find_front_matter(text, &lines) else {
        return text.to_string();
    };
    let mut done = vec![false; wanted.len()];
    let mut out = String::with_capacity(text.len() + 64);
    for (i, l) in lines.iter().enumerate() {
        let nl = match &text[l.end..l.next] {
            "" => "\n",
            n => n,
        };
        if i == fe {
            for (k, (key, value)) in wanted.iter().enumerate() {
                if !done[k] {
                    out.push_str(&format!("{key}: {value}{nl}"));
                    done[k] = true;
                }
            }
        }
        if i > fs && i < fe {
            let key = key_of(content(text, l));
            if let Some(k) = wanted.iter().position(|(w, _)| *w == key)
                && !done[k]
            {
                out.push_str(&format!("{key}: {}{nl}", wanted[k].1));
                done[k] = true;
                continue;
            }
        }
        out.push_str(&text[l.start..l.next]);
    }
    out
}

/// 屬性區的格式版本，給「新格式只讀」用。
pub(crate) fn is_newer(doc: &DigestDoc) -> bool {
    doc.format > DIGEST_FORMAT
}

// ---- 合併----

/// 兩份重點筆記合併的結果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DigestMerged {
    /// 本機那份要換成的內容；不必改是 `None`。
    pub text: Option<String>,
    /// 要另存的衝突副本（匯入那份，鍵改名成 `paperreader_conflict_of`）；不必存是 `None`。
    pub copy: Option<String>,
    /// 兩邊文字不同的點數。
    pub point_conflicts: usize,
}

/// 衝突副本：屬性區的 `paperreader_digest_of` 改名成 `paperreader_conflict_of`（值不變），其餘逐位元組不動。
pub(crate) fn as_conflict_copy(text: &str) -> String {
    let lines = split_lines(text);
    let Some((fs, fe)) = find_front_matter(text, &lines) else {
        return text.to_string();
    };
    for l in lines.iter().take(fe).skip(fs + 1) {
        let c = content(text, l);
        if key_of(c) == KEY_OF {
            let at = l.start + c.find(KEY_OF).expect("key is there");
            return splice(text, at, at + KEY_OF.len(), KEY_CONFLICT_OF);
        }
    }
    text.to_string()
}

/// 合併本機與匯入的兩份（同一份材料）。`deleted` 是合起來的變更紀錄裡這份材料被刪的點。
/// - 一點只在一邊：留下；`deleted` 有它就拿掉（刪除記錄勝過修改）。
/// - 兩邊都有、文字相同：照留；文字不同：留本機的，匯入那份另存衝突副本。
/// - 點以外的文字不同：留本機的，匯入那份另存衝突副本。
/// - 「已校對」兩邊有一邊是就算是。
///
/// 只在匯入那邊的點接在本機同一區段最後一點的後面；本機那一區段一點都沒有時，連同匯入那邊的標題接在檔尾。
/// 任一份讀不懂交回錯誤（哪一份由呼叫的一方另外判斷）。
pub(crate) fn merge(
    local: &str,
    remote: &str,
    deleted: &HashSet<String>,
    stamp: &DigestStamp,
) -> std::result::Result<DigestMerged, String> {
    let l = parse(local)?;
    let r = parse(remote)?;
    let lmap: HashMap<&str, &PointAt> = l.points.iter().map(|p| (p.point.id.as_str(), p)).collect();

    // 本機要拿掉的行：被刪的點（同代號的行一起）
    let mut drop: HashSet<usize> = HashSet::new();
    for p in &l.points {
        if deleted.contains(&p.point.id) {
            drop.extend(l.lines_of(&p.point.id));
        }
    }
    // 兩邊文字不同的點
    let point_conflicts = l
        .points
        .iter()
        .filter(|p| !deleted.contains(&p.point.id))
        .filter(|p| {
            r.points
                .iter()
                .find(|q| q.point.id == p.point.id)
                .is_some_and(|q| q.point.text != p.point.text)
        })
        .count();
    // 只在匯入那邊、沒有被刪的點：接到本機
    let nl = l.newline();
    let mut after_line: HashMap<usize, Vec<String>> = HashMap::new();
    let mut at_end: Vec<(String, Vec<String>)> = Vec::new();
    for q in &r.points {
        if lmap.contains_key(q.point.id.as_str()) || deleted.contains(&q.point.id) {
            continue;
        }
        let line = r.line_text(q).to_string();
        let heading = r.heading_before(q.line).unwrap_or("##").to_string();
        // 接在本機同一區段最後一點之後；那一區段沒有點了，就接在本機同一個標題（與它後面的空行）之後
        let anchor = l
            .points
            .iter()
            .filter(|p| p.point.section == q.point.section && !drop.contains(&p.line))
            .map(|p| p.line)
            .max()
            .or_else(|| {
                let h = (l.front.1 + 1..l.lines.len())
                    .find(|&i| content(local, &l.lines[i]) == heading.as_str())?;
                let blank = l
                    .lines
                    .get(h + 1)
                    .is_some_and(|n| content(local, n).trim().is_empty());
                Some(if blank { h + 1 } else { h })
            });
        match anchor {
            Some(a) => after_line.entry(a).or_default().push(line),
            None => match at_end.iter_mut().find(|(h, _)| *h == heading) {
                Some((_, v)) => v.push(line),
                None => at_end.push((heading, vec![line])),
            },
        }
    }
    let proofread = !l.doc.proofread && r.doc.proofread;
    let outside_differs = l.outside_text() != r.outside_text();

    let changed = !drop.is_empty() || !after_line.is_empty() || !at_end.is_empty() || proofread;
    let text = if changed {
        let mut s = String::with_capacity(local.len() + 256);
        for (i, ln) in l.lines.iter().enumerate() {
            if !drop.contains(&i) {
                s.push_str(&local[ln.start..ln.next]);
            }
            if let Some(v) = after_line.get(&i) {
                if !s.ends_with('\n') {
                    s.push_str(nl);
                }
                for line in v {
                    s.push_str(line);
                    s.push_str(nl);
                }
            }
        }
        for (heading, v) in &at_end {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push_str(nl);
            }
            s.push_str(nl);
            s.push_str(heading);
            s.push_str(nl);
            s.push_str(nl);
            for line in v {
                s.push_str(line);
                s.push_str(nl);
            }
        }
        let extra: &[(&str, &str)] = if proofread {
            &[("paperreader_proofread", "true")]
        } else {
            &[]
        };
        Some(stamped(&s, stamp, extra))
    } else {
        None
    };
    let copy = (point_conflicts > 0 || outside_differs).then(|| as_conflict_copy(remote));
    Ok(DigestMerged {
        text,
        copy,
        point_conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_text_strips_bullet_and_trailing_links_only() {
        assert_eq!(
            visible_text(
                "- 文字 [p. 3](paperreader://open?page=3) [p. 4](paperreader://open?page=4) "
            ),
            "文字"
        );
        assert_eq!(
            visible_text("* 見 [12] 與 [別處](https://x.org)"),
            "見 [12] 與 [別處](https://x.org)"
        );
        assert_eq!(visible_text("沒有清單符號"), "沒有清單符號");
        // 跳脫過的連結不是回原文的連結
        assert_eq!(
            visible_text(r"- a \[p. 1\](paperreader://x) [p. 2](paperreader://y)"),
            "a [p. 1](paperreader://x)"
        );
    }

    #[test]
    fn escaping_round_trips() {
        for s in [r"a\b", r"\<x>", "![i](u)", r"\\[", "普通的字"] {
            assert_eq!(unescape_text(&escape_text(s)), s);
        }
    }

    #[test]
    fn markers_are_stripped_until_none_is_left() {
        assert_eq!(strip_markers("<!<!----"), "");
        assert!(!strip_markers("x ---->> y").contains("-->"));
    }

    #[test]
    fn a_sections_list_rewritten_as_block_yaml_is_not_text_of_the_user() {
        // 一台在筆記工具裡被改寫成區塊式 YAML、另一台沒有：同步時不算「點以外的文字不同」，不另存衝突副本
        let stamp = DigestStamp {
            time: "2026-11-17T00:00:00.000Z".into(),
            device: "D".into(),
        };
        let d = DigestDraft {
            source_material: "M".into(),
            service: "S".into(),
            provider: "anthropic".into(),
            model: "m".into(),
            lang: "zh-TW".into(),
            title: "T".into(),
            sections: vec![
                DigestSection {
                    id: "question".into(),
                    label: "研究問題".into(),
                },
                DigestSection {
                    id: "results".into(),
                    label: "主要結果".into(),
                },
            ],
            points: vec![PointDraft {
                section: "question".into(),
                text: "一點。".into(),
                pages: vec![0],
                segments: vec![],
            }],
        };
        let local = render(&d, "P", None, &stamp);
        let line = local
            .lines()
            .find(|l| l.starts_with("paperreader_sections:"))
            .unwrap()
            .to_string();
        let remote = local.replace(
            &line,
            "paperreader_sections:\n  - id: question\n    label: 研究問題\n  - id: results\n    label: 主要結果",
        );
        assert_eq!(parse(&remote).unwrap().doc.sections, d.sections);
        let m = merge(&local, &remote, &HashSet::new(), &stamp).unwrap();
        assert_eq!(m.copy, None);
    }

    #[test]
    fn a_point_line_needs_the_marker_at_the_end() {
        assert!(split_point_line("- a <!-- paperreader:point {} -->").is_some());
        assert!(split_point_line("- a <!-- paperreader:point {} --> tail").is_none());
        assert!(split_point_line("- a <!-- other -->").is_none());
    }

    #[test]
    fn stamped_inserts_missing_keys_and_keeps_crlf() {
        let t = "---\r\npaperreader_digest: 1\r\npaperreader_modified: old\r\n---\r\nbody\r\n";
        let s = stamped(
            t,
            &DigestStamp {
                time: "T".into(),
                device: "D".into(),
            },
            &[("paperreader_proofread", "true")],
        );
        assert_eq!(
            s,
            "---\r\npaperreader_digest: 1\r\npaperreader_modified: T\r\npaperreader_modified_by: D\r\npaperreader_proofread: true\r\n---\r\nbody\r\n"
        );
    }
}
