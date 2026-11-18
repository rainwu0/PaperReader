//! 參考文獻清單 `references.json`：每篇文獻的資料夾裡最多一份，與
//! `paper.json`、`ink.json` 同一層；不列在材料清單（比照 `ink.json`）。
//!
//! 兩部分：抽出的部分（`extraction` 與 `entries`）由逐篇處理整份寫入，重抽時整份換掉；使用者的判斷
//! （`judgments`）逐項加刪，用找回鍵（`entry_key`）指向條目，不用條目在陣列裡的位置，所以重抽不會弄丟。
//!
//! 這個模組只有格式、檢查與純計算（正規化、找回鍵、判斷的套用）。讀寫檔案在 `local/references.rs`，
//! 同步的合併在 `sync/merge/references.rs`。外掛（文獻之間的連結）的 Rust 一側用這裡的同一組函式。

use crate::error::{LibError, Result, io_error};
use crate::format::{Map, format_or_current, normalize_doi, null_as_default, one};
use crate::ink::{MAX_COORD, round2};
use crate::jsonio::{VersionToken, read_json};
use crate::sync::merge::cmp_stamp;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

pub const REFERENCES_FILE: &str = "references.json";

/// 找回鍵規則的版本。規則改了就加一，舊版的函式留著；每項判斷依自己的 `key_version` 算。
pub const ENTRY_KEY_VERSION: u32 = 1;

/// 條數、字數、位元組的上限。「字」指 Unicode 字元。
pub const MAX_ENTRIES: usize = 2000;
pub const MAX_JUDGMENTS: usize = 5000;
pub const MAX_ENTRY_TEXT: usize = 1000;
pub const MAX_LABEL: usize = 40;
pub const MAX_EXTRACTOR: usize = 40;
pub const MAX_DOI: usize = 300;
pub const MAX_NUMBER: u32 = 99_999;
/// 抽出的部分（`extraction` 與 `entries`）寫成 JSON 的上限。
pub const MAX_EXTRACTED_BYTES: usize = 6 * 1024 * 1024;
/// `judgments` 寫成 JSON 的上限。
pub const MAX_JUDGMENT_BYTES: usize = 2 * 1024 * 1024;
/// 整份檔超過這個大小當成讀不懂。
pub const MAX_FILE_BYTES: u64 = 9 * 1024 * 1024;

pub const KIND_CONFIRM: &str = "confirm";
pub const KIND_REJECT: &str = "reject";
pub const KIND_MANUAL: &str = "manual";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferencesFile {
    #[serde(default = "one", deserialize_with = "format_or_current")]
    pub format: u32,
    /// 這份檔最後一次寫入的時間與裝置；給人看，合併不用它。
    #[serde(default)]
    pub modified: Option<String>,
    #[serde(default)]
    pub modified_by: Option<String>,
    pub extraction: Extraction,
    #[serde(default, deserialize_with = "null_as_default")]
    pub entries: Vec<RefEntry>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub judgments: Vec<Judgment>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 抽出的部分的檔頭；重抽時連同 `entries` 整個換。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    /// 抽取的代號（`links`）。
    pub extractor: String,
    /// 抽取規則的版本，1 以上。
    pub version: u32,
    /// 從哪一份原文、哪一個版本抽的。
    pub source_material: String,
    pub source_sha256: String,
    /// 抽的時間與裝置，由核心蓋。
    pub created: String,
    pub created_by: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// 參考文獻清單的一條。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefEntry {
    /// 標記上的編號（「[12]」的 12）；沒有編號的樣式是 `null`。
    #[serde(default)]
    pub number: Option<u32>,
    /// 條目前面的標記原樣（`[12]`、`12.`）。
    #[serde(default)]
    pub label: Option<String>,
    /// 條目原文，不含 `label`，斷行的連字號已接回。
    pub text: String,
    /// 條目起頭所在的頁序（從 0 起）。
    pub page: u32,
    /// 條目起頭所在那一段的方框（PDF 座標）。
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(flatten)]
    pub extra: Map,
}

/// 使用者的一項判斷。建立後不改。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgment {
    /// `j-` 加小寫的 ULID。
    pub id: String,
    /// `confirm`、`reject`、`manual`；不認得的種類原樣保留、不套用。
    pub kind: String,
    /// 判斷關於哪一篇（連到的，或被否決的）。
    pub paper: String,
    pub entry_key: String,
    pub key_version: u32,
    /// 判斷時那一條的原文；找不回時給人看。
    pub entry_text: String,
    #[serde(default)]
    pub entry_doi: Option<String>,
    #[serde(default)]
    pub entry_arxiv: Option<String>,
    pub created: String,
    pub created_by: String,
    #[serde(flatten)]
    pub extra: Map,
}

/// 加判斷時的種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JudgmentKind {
    /// 是這篇（待確認的候選按了「是這篇」）。
    Confirm,
    /// 不是這篇。
    Reject,
    /// 指定這篇。
    Manual,
}

impl JudgmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JudgmentKind::Confirm => KIND_CONFIRM,
            JudgmentKind::Reject => KIND_REJECT,
            JudgmentKind::Manual => KIND_MANUAL,
        }
    }
}

/// 寫入抽出的部分時交來的草稿（`write_references`）。`extraction.created` 這類由核心蓋的欄位不在這裡。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReferencesDraft {
    pub extractor: String,
    pub version: u32,
    pub source_material: String,
    pub source_sha256: String,
    #[serde(default)]
    pub entries: Vec<EntryDraft>,
}

/// 草稿的一條（欄位同 `RefEntry`，沒有不認得的欄位）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EntryDraft {
    #[serde(default)]
    pub number: Option<u32>,
    #[serde(default)]
    pub label: Option<String>,
    pub text: String,
    pub page: u32,
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
}

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// arXiv 編號，不含版本號：新式 `^\d{4}\.\d{4,5}$`，舊式 `^[a-z-]+(\.[A-Z]{2})?/\d{7}$`。
pub fn is_arxiv_id(s: &str) -> bool {
    if let Some((a, b)) = s.split_once('.')
        && a.len() == 4
        && a.bytes().all(|c| c.is_ascii_digit())
        && (4..=5).contains(&b.len())
        && b.bytes().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    let Some((cat, num)) = s.split_once('/') else {
        return false;
    };
    if num.len() != 7 || !num.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let (base, sub) = match cat.split_once('.') {
        Some((b, s)) => (b, Some(s)),
        None => (cat, None),
    };
    !base.is_empty()
        && base.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
        && sub.is_none_or(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_uppercase()))
}

/// 檢查過、整理好的 DOI（`normalize_doi` 之後以 `10.` 開頭、含 `/`，最多 300 字）。空的是 `None`。
pub(crate) fn checked_doi(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let d = normalize_doi(raw);
    if d.is_empty() {
        return Ok(None);
    }
    if !d.starts_with("10.") || !d.contains('/') || chars(&d) > MAX_DOI {
        return Err(bad(format!(
            "doi must start with 10. and contain / (at most {MAX_DOI} characters): {raw:?}"
        )));
    }
    Ok(Some(d))
}

pub(crate) fn check_year(y: Option<i32>) -> Result<()> {
    match y {
        Some(y) if !(1000..=9999).contains(&y) => {
            Err(bad(format!("year must be between 1000 and 9999: {y}")))
        }
        _ => Ok(()),
    }
}

/// 檢查草稿、轉成要寫的條目（DOI 正規化、座標取到小數兩位）。`pages` 是原文的頁數。
pub(crate) fn draft_entries(d: &ReferencesDraft, pages: u32) -> Result<Vec<RefEntry>> {
    let n = chars(&d.extractor);
    if n == 0 || n > MAX_EXTRACTOR {
        return Err(bad(format!(
            "extractor must be 1 to {MAX_EXTRACTOR} characters: {:?}",
            d.extractor
        )));
    }
    if d.version < 1 {
        return Err(bad("extraction version must be at least 1"));
    }
    if d.entries.len() > MAX_ENTRIES {
        return Err(bad(format!(
            "at most {MAX_ENTRIES} entries: {}",
            d.entries.len()
        )));
    }
    let mut out = Vec::with_capacity(d.entries.len());
    for (i, e) in d.entries.iter().enumerate() {
        let n = chars(&e.text);
        if e.text.trim().is_empty() || n > MAX_ENTRY_TEXT {
            return Err(bad(format!(
                "entry {i}: text must be 1 to {MAX_ENTRY_TEXT} characters"
            )));
        }
        if e.label.as_deref().is_some_and(|l| chars(l) > MAX_LABEL) {
            return Err(bad(format!(
                "entry {i}: label is longer than {MAX_LABEL} characters"
            )));
        }
        if e.number.is_some_and(|n| !(1..=MAX_NUMBER).contains(&n)) {
            return Err(bad(format!(
                "entry {i}: number must be between 1 and {MAX_NUMBER}"
            )));
        }
        if e.page >= pages {
            return Err(bad(format!(
                "entry {i}: page {} is beyond the {pages} pages of the source",
                e.page
            )));
        }
        if let Some(b) = e.bbox
            && b.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD)
        {
            return Err(bad(format!(
                "entry {i}: bbox must be finite numbers within ±{MAX_COORD}"
            )));
        }
        let doi = checked_doi(e.doi.as_deref()).map_err(|_| {
            bad(format!(
                "entry {i}: doi must start with 10. and contain /: {:?}",
                e.doi
            ))
        })?;
        let arxiv = match e.arxiv.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(a) if is_arxiv_id(a) => Some(a.to_string()),
            Some(a) => return Err(bad(format!("entry {i}: not an arXiv id: {a:?}"))),
        };
        check_year(e.year).map_err(|_| bad(format!("entry {i}: year out of range")))?;
        out.push(RefEntry {
            number: e.number,
            label: e.label.clone(),
            text: e.text.clone(),
            page: e.page,
            bbox: e.bbox.map(|b| b.map(round2)),
            doi,
            arxiv,
            year: e.year,
            extra: Map::new(),
        });
    }
    Ok(out)
}

/// 條目從另一份接過不認得的欄位（不認得的欄位不丟）：同一個找回鍵（目前的版本）的條目上有、
/// 這邊沒有的鍵搬過來。重抽（`write_references`）與合併（輸的那份）都用它。
pub(crate) fn carry_entry_extras(to: &mut [RefEntry], from: &[RefEntry]) {
    let mut by_key: HashMap<String, &RefEntry> = HashMap::new();
    for e in from {
        if e.extra.is_empty() {
            continue;
        }
        if let Some(k) = entry_key(&e.text, ENTRY_KEY_VERSION) {
            by_key.entry(k).or_insert(e);
        }
    }
    if by_key.is_empty() {
        return;
    }
    for e in to.iter_mut() {
        let Some(old) = entry_key(&e.text, ENTRY_KEY_VERSION).and_then(|k| by_key.get(&k)) else {
            continue;
        };
        for (k, v) in &old.extra {
            if !e.extra.contains_key(k) {
                e.extra.insert(k.clone(), v.clone());
            }
        }
    }
}

/// 抽出的部分寫成 JSON 的位元組數。
pub(crate) fn extracted_bytes(extraction: &Extraction, entries: &[RefEntry]) -> usize {
    #[derive(Serialize)]
    struct Part<'a> {
        extraction: &'a Extraction,
        entries: &'a [RefEntry],
    }
    serde_json::to_vec_pretty(&Part {
        extraction,
        entries,
    })
    .map(|v| v.len())
    .unwrap_or(usize::MAX)
}

/// 一份參考文獻清單在上限內：條數、判斷的項數、兩部分各自的位元組、整份寫成 JSON 的大小。
/// 合併的結果超過就不寫：核心自己不寫出會被當成讀不懂的檔。
pub(crate) fn within_limits(doc: &ReferencesFile) -> bool {
    doc.entries.len() <= MAX_ENTRIES
        && doc.judgments.len() <= MAX_JUDGMENTS
        && judgment_bytes(&doc.judgments) <= MAX_JUDGMENT_BYTES
        && extracted_bytes(&doc.extraction, &doc.entries) <= MAX_EXTRACTED_BYTES
        && crate::jsonio::to_canonical_json(doc).is_ok_and(|s| s.len() as u64 <= MAX_FILE_BYTES)
}

/// `judgments` 寫成 JSON 的位元組數。
pub(crate) fn judgment_bytes(judgments: &[Judgment]) -> usize {
    serde_json::to_vec_pretty(judgments)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
}

/// 讀一份 `references.json`：不存在是 `None`；超過 9 MB、不是一般檔案、讀不懂是 `Unreadable`。
/// 讀參考文獻清單只用這一個（讀寫、檢查、同步都是），「超過上限」到哪裡都是讀不懂。
pub fn read_references_file(path: &Path) -> Result<Option<(ReferencesFile, VersionToken)>> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(path, e)),
    };
    let unreadable = |reason: &str| LibError::Unreadable {
        path: path.display().to_string(),
        reason: reason.to_string(),
    };
    if !meta.is_file() {
        return Err(unreadable("not a file"));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(unreadable("file is larger than 9 MB"));
    }
    match read_json::<ReferencesFile>(path) {
        Ok(v) => Ok(Some(v)),
        Err(LibError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

// ---------- 正規化與找回鍵 ----------

/// 比對與找回鍵用的正規化：
/// 1. NFKC（連字 `ﬁ` 變 `fi`、全形變半形）；
/// 2. 拿掉軟連字號（U+00AD）；`-` 或 `‐`（U+2010）後面接一個以上的空白（含換行）再接小寫字母時，拿掉連字號與空白；
/// 3. 轉小寫（Unicode）；
/// 4. 去重音：NFD 之後拿掉附加符號（Unicode 的 Mark 類，`unicode-normalization` 的 `is_combining_mark`），再 NFC；
/// 5. 連續的空白併成一個，去頭尾空白。
pub fn fold_text(s: &str) -> String {
    let nfkc: Vec<char> = s.nfkc().filter(|c| *c != '\u{00AD}').collect();
    let mut joined = String::with_capacity(nfkc.len());
    let mut i = 0;
    while i < nfkc.len() {
        let c = nfkc[i];
        if c == '-' || c == '\u{2010}' {
            let mut j = i + 1;
            while j < nfkc.len() && nfkc[j].is_whitespace() {
                j += 1;
            }
            if j > i + 1 && j < nfkc.len() && nfkc[j].is_lowercase() {
                i = j;
                continue;
            }
        }
        joined.push(c);
        i += 1;
    }
    let lower = joined.to_lowercase();
    let bare: String = lower
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .nfc()
        .collect();
    bare.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 正規化規則的版本：`fold_text` 或 `fold_text_mapped` 交出的字串改了就加一。全文搜尋的外掛把它
/// 算進索引的版本；文獻連結的找回鍵另有 `ENTRY_KEY_VERSION`，不受影響。
pub const FOLD_VERSION: u32 = 1;

/// 帶位置的正規化結果（`fold_text_mapped`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folded {
    /// 逐位元組等於 `fold_text(s)`。
    pub text: String,
    /// `text` 的每個 Unicode 字元一項：產生它的那幾個原字在 `s` 裡的 UTF-8 位元組範圍 `[起, 訖)`。
    pub spans: Vec<(usize, usize)>,
    /// 接回行尾連字號的地方：接回點之後第一個字在 `text` 裡的字元序，由小到大。
    pub joins: Vec<usize>,
}

/// 正規化途中的一個字：產生它的原字的位元組範圍，以及它是不是接回點之後的第一個字。
#[derive(Debug, Clone, Copy)]
struct Tagged {
    c: char,
    span: (usize, usize),
    join: bool,
}

fn union(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// 照 `unicode-normalization` 的 `Decompositions` 做（相容或標準）分解：逐字分解，再把每一串非起始字（組合類別
/// 不是 0）依類別穩定排序。排序會讓附加符號的起點倒退，但組合類別不是 0 的都是附加符號，第 4 步一律拿掉（或合進
/// 前面的起始字，範圍取聯集、起點是起始字的），所以交出的字起點不倒退。
fn tagged_decompose(items: &[Tagged], compatible: bool) -> Vec<Tagged> {
    let mut out: Vec<Tagged> = Vec::with_capacity(items.len());
    for t in items {
        let mut first = true;
        let mut emit = |d: char| {
            out.push(Tagged {
                c: d,
                span: t.span,
                join: t.join && first,
            });
            first = false;
        };
        if compatible {
            unicode_normalization::char::decompose_compatible(t.c, &mut emit);
        } else {
            unicode_normalization::char::decompose_canonical(t.c, &mut emit);
        }
    }
    let ccc = unicode_normalization::char::canonical_combining_class;
    let mut i = 0;
    while i < out.len() {
        if ccc(out[i].c) == 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < out.len() && ccc(out[i].c) != 0 {
            i += 1;
        }
        out[start..i].sort_by_key(|t| ccc(t.c));
    }
    out
}

/// 照 `unicode-normalization` 的 `Recompositions` 做標準合成（逐步照它的狀態機，結果才逐字相同）。合成的字涵蓋
/// 合進來的每一個字的範圍。
fn tagged_recompose(items: Vec<Tagged>) -> Vec<Tagged> {
    use unicode_normalization::char::{canonical_combining_class as ccc, compose};
    let merge = |k: Tagged, ch: Tagged, r: char| Tagged {
        c: r,
        span: union(k.span, ch.span),
        join: k.join || ch.join,
    };
    let mut out: Vec<Tagged> = Vec::with_capacity(items.len());
    let mut buffer: Vec<Tagged> = Vec::new();
    let mut composee: Option<Tagged> = None;
    let mut last_ccc: Option<u8> = None;
    for ch in items {
        let ch_class = ccc(ch.c);
        let Some(k) = composee else {
            if ch_class != 0 {
                out.push(ch);
            } else {
                composee = Some(ch);
            }
            continue;
        };
        match last_ccc {
            None => match compose(k.c, ch.c) {
                Some(r) => composee = Some(merge(k, ch, r)),
                None => {
                    if ch_class == 0 {
                        out.push(k);
                        composee = Some(ch);
                    } else {
                        buffer.push(ch);
                        last_ccc = Some(ch_class);
                    }
                }
            },
            Some(l_class) => {
                if l_class >= ch_class {
                    // 被擋住了
                    if ch_class == 0 {
                        out.push(k);
                        out.append(&mut buffer);
                        composee = Some(ch);
                        last_ccc = None;
                    } else {
                        buffer.push(ch);
                        last_ccc = Some(ch_class);
                    }
                    continue;
                }
                match compose(k.c, ch.c) {
                    Some(r) => composee = Some(merge(k, ch, r)),
                    None => {
                        buffer.push(ch);
                        last_ccc = Some(ch_class);
                    }
                }
            }
        }
    }
    if let Some(k) = composee {
        out.push(k);
    }
    out.append(&mut buffer);
    out
}

/// 拿掉一些字：被拿掉的字若是接回點之後的第一個字，記號交給下一個留下的字。
fn tagged_retain(items: Vec<Tagged>, keep: impl Fn(char) -> bool) -> Vec<Tagged> {
    let mut out = Vec::with_capacity(items.len());
    let mut pending = false;
    for mut t in items {
        if keep(t.c) {
            t.join |= pending;
            pending = false;
            out.push(t);
        } else {
            pending |= t.join;
        }
    }
    out
}

/// 去重音（`fold_text` 第 4 步）拿掉附加符號：拿掉的併進前一個留下的字，只加長它的結尾（起點不倒退），框到以它
/// 結尾的詞時才不會切開一個字形。前面沒有留下的字（字串開頭）就不併。接回的記號同 `tagged_retain`。
fn tagged_drop_marks(items: Vec<Tagged>) -> Vec<Tagged> {
    let mut out: Vec<Tagged> = Vec::with_capacity(items.len());
    let mut pending = false;
    for mut t in items {
        if is_combining_mark(t.c) {
            pending |= t.join;
            if let Some(prev) = out.last_mut() {
                prev.span.1 = prev.span.1.max(t.span.1);
            }
        } else {
            t.join |= pending;
            pending = false;
            out.push(t);
        }
    }
    out
}

/// 帶位置的 `fold_text`：`text` 逐字等於 `fold_text(s)`，另交每個字的原字範圍與接回點。
///
/// 每一步照 `fold_text` 的同一步做，每個字帶著產生它的原字的位元組範圍：NFKC 照 `unicode-normalization` 的分解與
/// 合成逐步做（合成時範圍取聯集）；轉小寫用整串的 `str::to_lowercase`（希臘字尾的 Σ 看前後文），再逐字對回；
/// 併起來的空白涵蓋整串空白（含夾在裡面被拿掉的軟連字號）。
pub fn fold_text_mapped(s: &str) -> Folded {
    // 1. NFKC，拿掉軟連字號
    let chars: Vec<Tagged> = s
        .char_indices()
        .map(|(i, c)| Tagged {
            c,
            span: (i, i + c.len_utf8()),
            join: false,
        })
        .collect();
    let nfkc = tagged_retain(tagged_recompose(tagged_decompose(&chars, true)), |c| {
        c != '\u{00AD}'
    });
    // 2. 行尾的連字號：`-` 或 `‐` 後面接一個以上的空白再接小寫字母時，拿掉連字號與空白
    let mut joined: Vec<Tagged> = Vec::with_capacity(nfkc.len());
    let mut i = 0;
    while i < nfkc.len() {
        let c = nfkc[i].c;
        if c == '-' || c == '\u{2010}' {
            let mut j = i + 1;
            while j < nfkc.len() && nfkc[j].c.is_whitespace() {
                j += 1;
            }
            if j > i + 1 && j < nfkc.len() && nfkc[j].c.is_lowercase() {
                let mut t = nfkc[j];
                t.join = true;
                joined.push(t);
                i = j + 1;
                continue;
            }
        }
        joined.push(nfkc[i]);
        i += 1;
    }
    // 3. 轉小寫：整串做（Σ 看前後文），逐字對回（每個字的結果除了 Σ 都和單獨轉的相同）
    let plain: String = joined.iter().map(|t| t.c).collect();
    let lowered: Vec<char> = plain.to_lowercase().chars().collect();
    let width = |c: char| {
        if c == 'Σ' {
            1
        } else {
            c.to_lowercase().count()
        }
    };
    let aligned = joined.iter().map(|t| width(t.c)).sum::<usize>() == lowered.len();
    let mut lower: Vec<Tagged> = Vec::with_capacity(lowered.len());
    let mut at = 0;
    for t in &joined {
        let n = width(t.c);
        let mapped: Vec<char> = if aligned {
            lowered[at..at + n].to_vec()
        } else {
            // 不會發生（標準庫的整串轉換就是逐字轉換加上 Σ）；萬一對不上也不讓程式停下
            t.c.to_lowercase().collect()
        };
        for (k, c) in mapped.into_iter().enumerate() {
            lower.push(Tagged {
                c,
                span: t.span,
                join: t.join && k == 0,
            });
        }
        at += n;
    }
    // 4. 去重音：NFD、拿掉附加符號、NFC
    let nfd = tagged_decompose(&lower, false);
    let bare = tagged_drop_marks(nfd);
    let bare = tagged_recompose(tagged_decompose(&bare, false));
    // 5. 連續的空白併成一個、去頭尾空白
    let mut text = String::with_capacity(bare.len());
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(bare.len());
    let mut joins: Vec<usize> = Vec::new();
    let mut space: Option<(usize, usize)> = None;
    let mut pending_join = false;
    let mut n = 0usize;
    for t in bare {
        if t.c.is_whitespace() {
            space = Some(space.map_or(t.span, |s| union(s, t.span)));
            pending_join |= t.join;
            continue;
        }
        if let Some(sp) = space.take()
            && n > 0
        {
            text.push(' ');
            spans.push(sp);
            n += 1;
        }
        if t.join || pending_join {
            joins.push(n);
        }
        pending_join = false;
        text.push(t.c);
        spans.push(t.span);
        n += 1;
    }
    joins.dedup();
    Folded { text, spans, joins }
}

/// 去掉開頭的編號標記：`[12]`、`(12)`、`12.`、`12)`（1 到 5 位數字），之後的空白一起去掉。
fn strip_label(s: &str) -> &str {
    let t = s.trim_start();
    let digits = |x: &str| x.bytes().take_while(u8::is_ascii_digit).count();
    let after = if let Some(rest) = t.strip_prefix('[') {
        let n = digits(rest);
        (1..=5)
            .contains(&n)
            .then(|| rest[n..].strip_prefix(']'))
            .flatten()
    } else if let Some(rest) = t.strip_prefix('(') {
        let n = digits(rest);
        (1..=5)
            .contains(&n)
            .then(|| rest[n..].strip_prefix(')'))
            .flatten()
    } else {
        let n = digits(t);
        (1..=5)
            .contains(&n)
            .then(|| {
                let rest = &t[n..];
                rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))
            })
            .flatten()
    };
    after.map_or(t, str::trim_start)
}

/// 找回鍵第 1 版：`fold_text` → 去掉開頭的編號標記 → 只留字母與數字（`char::is_alphanumeric`）。
fn entry_key_v1(text: &str) -> String {
    let folded = fold_text(text);
    strip_label(&folded)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// 條目的找回鍵。`version` 是這一版不認得的版本時交回 `None`。
pub fn entry_key(text: &str, version: u32) -> Option<String> {
    match version {
        1 => Some(entry_key_v1(text)),
        _ => None,
    }
}

// ---------- 判斷的套用 ----------

/// 一條上作用的判斷（都是在 `judgments` 裡的位置）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedEntry {
    /// 每個對象（文獻）現在有效的那一項：同一條、同一個對象取（`created`, `created_by`）最晚的。依那一項的位置排。
    pub by_paper: Vec<usize>,
    /// 連到哪一篇：有效的「連到」（`confirm`、`manual`）裡最晚的一項。
    pub link: Option<usize>,
    /// 被否決的每一篇（有效的一項是 `reject`）。
    pub rejected: Vec<usize>,
}

/// `resolve_judgments` 的結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// 和 `entries` 一一對應。
    pub entries: Vec<ResolvedEntry>,
    /// 找不回的判斷（對不到任何一條），依在 `judgments` 裡的順序。
    pub lost: Vec<usize>,
}

fn known_kind(k: &str) -> bool {
    k == KIND_CONFIRM || k == KIND_REJECT || k == KIND_MANUAL
}

/// 一項判斷對到的條（位置）。依序：用它的 `key_version` 算的找回鍵相同的；DOI 相同的；arXiv 編號相同的。
fn matches_of(
    j: &Judgment,
    entries: &[RefEntry],
    keys: &mut HashMap<u32, Vec<Option<String>>>,
) -> Vec<usize> {
    let ks = keys
        .entry(j.key_version)
        .or_insert_with(|| {
            entries
                .iter()
                .map(|e| entry_key(&e.text, j.key_version))
                .collect()
        })
        .clone();
    let by_key: Vec<usize> = ks
        .iter()
        .enumerate()
        .filter(|(_, k)| k.as_deref() == Some(j.entry_key.as_str()))
        .map(|(i, _)| i)
        .collect();
    if !by_key.is_empty() {
        return by_key;
    }
    if let Some(d) = j
        .entry_doi
        .as_deref()
        .map(normalize_doi)
        .filter(|d| !d.is_empty())
    {
        let by_doi: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.doi.as_deref().map(normalize_doi).as_deref() == Some(d.as_str()))
            .map(|(i, _)| i)
            .collect();
        if !by_doi.is_empty() {
            return by_doi;
        }
    }
    if let Some(a) = j.entry_arxiv.as_deref().filter(|a| !a.is_empty()) {
        return entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.arxiv.as_deref() == Some(a))
            .map(|(i, _)| i)
            .collect();
    }
    Vec::new()
}

/// `a` 比 `b` 晚：比（`created`, `created_by`）；完全相同時比判斷代號（位元組順序，大的較晚），
/// 不看在陣列裡的位置，所以兩台的順序不同也算出同一個結果。代號也相同（同一項重複）時才看位置。
fn later(judgments: &[Judgment], a: usize, b: usize) -> bool {
    let (x, y) = (&judgments[a], &judgments[b]);
    cmp_stamp(
        Some(&x.created),
        Some(&x.created_by),
        Some(&y.created),
        Some(&y.created_by),
    )
    .then_with(|| x.id.as_bytes().cmp(y.id.as_bytes()))
    .then(a.cmp(&b))
        == Ordering::Greater
}

/// 判斷的套用：
/// 1. 每一項判斷找它的條（找回鍵、DOI、arXiv 編號依序），對到幾條就作用在幾條；都對不到是找不回；
/// 2. 同一條、同一個對象的幾項取最晚的一項：是 `reject` 就是否決那篇，是 `confirm` 或 `manual` 就是連到那篇；
/// 3. 同一條有幾個「連到」時取最晚的那一個。
///
/// 不認得的種類（新版應用寫的）不套用，也不算找不回。
pub fn resolve_judgments(entries: &[RefEntry], judgments: &[Judgment]) -> Resolution {
    let mut keys: HashMap<u32, Vec<Option<String>>> = HashMap::new();
    let mut per_entry: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut lost = Vec::new();
    for (ji, j) in judgments.iter().enumerate() {
        if !known_kind(&j.kind) {
            continue;
        }
        let m = matches_of(j, entries, &mut keys);
        if m.is_empty() {
            lost.push(ji);
        }
        for ei in m {
            per_entry[ei].push(ji);
        }
    }
    let entries = per_entry
        .into_iter()
        .map(|js| {
            // 每個對象最晚的一項
            let mut latest: Vec<usize> = Vec::new();
            for ji in js {
                match latest
                    .iter_mut()
                    .find(|k| judgments[**k].paper == judgments[ji].paper)
                {
                    Some(k) => {
                        if later(judgments, ji, *k) {
                            *k = ji;
                        }
                    }
                    None => latest.push(ji),
                }
            }
            latest.sort_unstable();
            let mut link: Option<usize> = None;
            let mut rejected = Vec::new();
            for &k in &latest {
                if judgments[k].kind == KIND_REJECT {
                    rejected.push(k);
                } else if link.is_none_or(|l| later(judgments, k, l)) {
                    link = Some(k);
                }
            }
            ResolvedEntry {
                by_paper: latest,
                link,
                rejected,
            }
        })
        .collect();
    Resolution { entries, lost }
}
