//! 整批匯入：讀匯入清單、逐篇逐欄檢查、掃描容器（資料夾或 zip）。
//!
//! 掃描不碰文獻庫，外殼不持文獻庫的鎖呼叫 `scan_import`；比對成預覽與逐篇寫入是 `LocalLibrary` 的方法
//! （`local/import.rs`）。核心不寫給人看的句子，只交原因代碼。
//!
//! 清單先整份讀成 JSON 值，再逐篇逐欄檢查：一個欄位錯只略過那一篇，不讓整份不收。

use crate::backup::is_safe_backup_path;
use crate::error::{LibError, Result};
use crate::format::{Author, BibDraft, FORM_PDF, ROLE_SOURCE, ROLE_TRANSLATION, normalize_doi};
use crate::jsonio::{hash_reader, sha256_hex};
use crate::naming::last_segment;
use crate::pdf::{PDF_HEADER_WINDOW, looks_like_pdf, title_from_file_name};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::ops::Bound;
use std::path::{Path, PathBuf};

/// 匯入清單的名稱：放在資料夾或 zip 的根目錄（找它用名稱，認它用內容的 `kind`）。
pub const MANIFEST_NAME: &str = "paperreader-import.json";
/// 匯入清單的 `kind`。
pub const MANIFEST_KIND: &str = "paperreader-import";
/// 這個版本認得的匯入清單格式。
pub const MANIFEST_FORMAT: u64 = 1;
/// 清單最大的位元組數。
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
/// 一份清單最多幾篇。
pub const MAX_PAPERS: usize = 10_000;
/// 一篇最多幾個檔。
pub const MAX_FILES: usize = 32;
/// 字串欄位（`path` 除外）最長的位元組數。
pub const MAX_STRING_BYTES: usize = 4096;
/// zip 最多幾個項目。
pub const MAX_ZIP_ENTRIES: usize = 50_000;
/// zip 裡清單以外每一項宣告的未壓縮大小上限。
pub const MAX_ZIP_ENTRY_BYTES: u64 = 4 << 30;
/// zip 裡清單以外各項宣告的未壓縮大小總和上限。
pub const MAX_ZIP_TOTAL_BYTES: u64 = 64 << 30;
/// zip 的項目名稱用了反斜線時，整份不收的原因（`Unreadable` 的 `reason` 以它開頭；外殼靠它轉成畫面認得的種類）。
pub const ZIP_BACKSLASH: &str = "zip entry names use backslashes";
/// 「沒收的書目欄位」最多列幾個鍵名。
const MAX_DROPPED_LISTED: usize = 20;
/// 重點筆記要整份讀進記憶體才能確認是重點筆記的格式：最多讀這麼多（自訂，沿用清單的上限）。
const MAX_DIGEST_BYTES: u64 = MAX_MANIFEST_BYTES;

pub(crate) use crate::digest::{FORM_MARKDOWN, ROLE_DIGEST};
/// 抽出文字（保留的角色）。
pub(crate) const ROLE_TEXT: &str = "text";
const PAGE_MAPS: [&str; 3] = ["same-as-source", "marked-in-text", "none"];
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// 略過一篇的原因代碼。
pub mod reason {
    pub const BAD_ENTRY: &str = "bad-entry";
    pub const TOO_MANY_FILES: &str = "too-many-files";
    pub const BAD_ROLE: &str = "bad-role";
    pub const NO_SOURCE: &str = "no-source";
    pub const MULTIPLE_SOURCES: &str = "multiple-sources";
    pub const BAD_PATH: &str = "bad-path";
    pub const DUPLICATE_PATH: &str = "duplicate-path";
    pub const MISSING_FILE: &str = "missing-file";
    pub const FORM_MISMATCH: &str = "form-mismatch";
    pub const CHECKSUM_MISMATCH: &str = "checksum-mismatch";
    pub const ALREADY_PRESENT: &str = "already-present";
    pub const IN_TRASH: &str = "in-trash";
    pub const DUPLICATE_SOURCE: &str = "duplicate-source";
    pub const FAILED: &str = "failed";
}

/// 提醒的種類（不擋）。
pub mod notice {
    pub const SAME_DOI: &str = "same-doi";
    pub const BIB_FIELDS_DROPPED: &str = "bib-fields-dropped";
}

use reason::*;

/// 要匯入的容器：外殼交給核心的路徑。zip 由外殼先複製到應用自己的暫存處。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportContainer {
    Folder(PathBuf),
    Zip(PathBuf),
}

impl ImportContainer {
    /// 外殼交來的路徑（整份層級的錯誤 `Unreadable { path }` 寫它）。
    pub fn path(&self) -> &Path {
        match self {
            ImportContainer::Folder(p) | ImportContainer::Zip(p) => p,
        }
    }
}

/// 掃描的結果。不碰文獻庫，可以在鎖外做；交給 `LocalLibrary::prepare_import`。
#[derive(Debug)]
pub struct ImportScan {
    pub(crate) container: ImportContainer,
    /// 清單的指紋：第一次寫入時重讀清單比對。
    pub(crate) manifest_sha: String,
    pub(crate) total: u32,
    /// 掃描時會收的篇（依序號）。
    pub(crate) accepted: Vec<ScannedPaper>,
    /// 掃描時略過的篇（依序號）。
    pub(crate) skipped: Vec<ImportSkip>,
}

/// 掃描時會收的一篇：檢查後的資料與原文的指紋。
#[derive(Debug, Clone)]
pub(crate) struct ScannedPaper {
    pub index: u32,
    /// 對照後的書目（沒有篇名時已從原文的檔名取）。
    pub bib: BibDraft,
    pub short_name: Option<String>,
    pub aliases: Vec<String>,
    pub files: Vec<PlannedFile>,
    pub source_sha: String,
    pub notices: Vec<ImportNotice>,
}

impl ScannedPaper {
    /// 原文在清單裡的路徑（一篇正好一個原文）。
    pub(crate) fn source_path(&self) -> &str {
        self.files
            .iter()
            .find(|f| f.role == ROLE_SOURCE)
            .map(|f| f.path.as_str())
            .unwrap_or_default()
    }
}

/// 清單裡的一個檔（檢查後）。
#[derive(Debug, Clone)]
pub(crate) struct PlannedFile {
    pub path: String,
    pub role: String,
    pub form: String,
    pub title: Option<String>,
    pub page_map: Option<String>,
    pub proofread: Option<bool>,
    /// 清單寫的指紋（小寫）。
    pub sha256: Option<String>,
    /// 掃描時的大小。
    pub bytes: u64,
}

/// 比對之後的預覽：會收的篇與略過的篇（依清單裡的序號排）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportPreview {
    pub preview_id: String,
    /// 清單裡的篇數。
    pub total: u32,
    pub accept: Vec<ImportItem>,
    pub skip: Vec<ImportSkip>,
    /// 會收的篇的檔案總大小。
    pub bytes: u64,
}

/// 會收的一篇。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportItem {
    /// 在 `papers` 裡的序號，從 0 起。
    pub index: u32,
    /// 對照後的篇名。
    pub title: Option<String>,
    pub files: u32,
    pub notices: Vec<ImportNotice>,
}

/// 略過（或寫入失敗）的一篇。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportSkip {
    pub index: u32,
    /// `reason` 模組裡的代碼。
    pub reason: String,
    /// 欄位位置、路徑，或相關文獻的代號。
    pub detail: Option<String>,
}

impl ImportSkip {
    pub(crate) fn new(index: u32, reason: &str, detail: Option<String>) -> ImportSkip {
        ImportSkip {
            index,
            reason: reason.to_string(),
            detail,
        }
    }
}

/// 一則提醒。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportNotice {
    /// `same-doi`、`bib-fields-dropped`。
    pub kind: String,
    pub detail: Option<String>,
}

/// `import_step` 一次的結果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ImportStep {
    /// 寫完一篇（或那篇在寫入時被略過、失敗），還有下一篇。
    Progress { done: u32, total: u32 },
    /// 全部做完。
    Finished(ImportReport),
}

/// 寫入的回報。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// （序號, 新代號）。
    pub added: Vec<(u32, String)>,
    /// 預覽時略過的，加上寫入時才發現而略過的（依序號）。
    pub skipped: Vec<ImportSkip>,
    /// `reason` 是 `failed`，`detail` 是錯誤種類與原因。
    pub failed: Vec<ImportSkip>,
    /// 取消或空間不足而沒做的。
    pub not_done: Vec<u32>,
    /// 加入的篇裡有提醒的篇數。
    pub notices: u32,
}

// ---- 掃描 ----

/// 掃描：讀清單、逐篇檢查、讀原文算指紋。不碰文獻庫。
/// `progress(已讀位元組, 總位元組)` 交回 `false` 表示取消，這時交回 `Ok(None)`、什麼都沒寫。
///
/// 整份層級的錯誤（容器打不開、清單不合）交 `Unreadable { path: 容器 }`；`format` 太新交 `FormatTooNew`。
pub fn scan_import(
    container: &ImportContainer,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<Option<ImportScan>> {
    let (mut opened, manifest) = Opened::open(container, true)?;
    let manifest_sha = sha256_hex(&manifest);
    let papers = parse_manifest(container.path(), &manifest)?;
    drop(manifest);

    // 第一輪：欄位、角色、路徑規則，再找每個檔（不讀內容）。還不知道要讀的總量，每篇回報一次 (0, 0)，
    // 讓取消在這一輪也有效
    let mut first: Vec<std::result::Result<(Entry, Vec<Located>), ImportSkip>> =
        Vec::with_capacity(papers.len());
    for (i, v) in papers.iter().enumerate() {
        first.push(check_entry(i, v).map(|e| {
            let found = e.files.iter().map(|f| opened.resolve(&f.path)).collect();
            (e, found)
        }));
        if !progress(0, 0) {
            return Ok(None);
        }
    }
    drop(papers);
    // 要讀的總量：每個檔都找得到的篇，檔案大小的總和
    let total: u64 = first
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .filter_map(|(_, found)| budget(found))
        .sum();

    // 第二輪：依序判斷重複、找不到、內容；會收的篇才占用路徑
    let mut done = 0u64;
    let mut claimed: HashSet<String> = HashSet::new();
    let mut accepted = Vec::new();
    let mut skipped = Vec::new();
    for (i, r) in first.into_iter().enumerate() {
        let index = i as u32;
        let (entry, found) = match r {
            Ok(v) => v,
            Err(s) => {
                skipped.push(s);
                continue;
            }
        };
        let base = done;
        let paper_budget = budget(&found).unwrap_or(0);
        let outcome = judge(&mut opened, index, entry, found, &claimed, &mut |read| {
            progress(base + read.min(paper_budget), total)
        });
        done = base + paper_budget;
        match outcome {
            Judged::Accept(p) => {
                claimed.extend(p.files.iter().map(|f| f.path.to_lowercase()));
                accepted.push(*p);
            }
            Judged::Skip(s) => skipped.push(s),
            Judged::Cancel => return Ok(None),
        }
        if paper_budget > 0 && !progress(done, total) {
            return Ok(None);
        }
    }
    if !progress(total, total) {
        return Ok(None);
    }
    Ok(Some(ImportScan {
        container: container.clone(),
        manifest_sha,
        total: (accepted.len() + skipped.len()) as u32,
        accepted,
        skipped,
    }))
}

/// 每個檔都找得到時，這篇檔案大小的總和。
fn budget(found: &[Located]) -> Option<u64> {
    found
        .iter()
        .map(|f| f.as_ref().ok().map(|r| r.size))
        .sum::<Option<u64>>()
}

type Located = std::result::Result<Resolved, &'static str>;

enum Judged {
    Accept(Box<ScannedPaper>),
    Skip(ImportSkip),
    Cancel,
}

/// 第二輪的一篇：經過符號連結等 → `bad-path`；重複 → `duplicate-path`；找不到 → `missing-file`；
/// 內容 → `form-mismatch`；指紋 → `checksum-mismatch`。`tick(這篇已讀的位元組)` 交回 `false` 表示取消。
fn judge(
    opened: &mut Opened,
    index: u32,
    entry: Entry,
    found: Vec<Located>,
    claimed: &HashSet<String>,
    tick: &mut dyn FnMut(u64) -> bool,
) -> Judged {
    let skip =
        |code: &str, path: &str| Judged::Skip(ImportSkip::new(index, code, Some(path.into())));
    if let Some(k) = found.iter().position(|f| f == &Err(BAD_PATH)) {
        return skip(BAD_PATH, &entry.files[k].path);
    }
    let mut mine: HashSet<String> = HashSet::new();
    for f in &entry.files {
        let lower = f.path.to_lowercase();
        if claimed.contains(&lower) || !mine.insert(lower) {
            return skip(DUPLICATE_PATH, &f.path);
        }
    }
    if let Some(k) = found.iter().position(|f| f.is_err()) {
        return skip(MISSING_FILE, &entry.files[k].path);
    }

    // 內容：原文、寫了指紋的、Markdown 讀完整份；其餘的 PDF 只讀檔頭
    let mut read_before = 0u64;
    let mut checksum_bad: Option<usize> = None;
    let mut source_sha = String::new();
    let mut files = Vec::with_capacity(entry.files.len());
    for (k, (f, loc)) in entry.files.iter().zip(found).enumerate() {
        let r = loc.expect("checked above");
        let full = f.role == ROLE_SOURCE || f.sha256.is_some() || f.form == FORM_MARKDOWN;
        let rules = ReadRules::new(opened, &r, &f.role, &f.form);
        let looked = match opened.reader(&r) {
            Err(code) => Err(Halt::Skip(code)),
            Ok(mut input) if full => stream(&mut input, &rules, &mut |_| Ok(()), &mut |n| {
                tick(read_before + n)
            }),
            Ok(input) => peek(input, rules.read_code),
        };
        let seen = match looked {
            Ok(s) => s,
            Err(Halt::Cancel) => return Judged::Cancel,
            Err(Halt::Skip(code)) => return skip(code, &f.path),
            Err(Halt::Write(_)) => return skip(MISSING_FILE, &f.path),
        };
        if !content_ok(&f.role, &f.form, &seen) {
            return skip(FORM_MISMATCH, &f.path);
        }
        if let Some(sha) = &seen.sha {
            if f.sha256.as_ref().is_some_and(|d| d != sha) && checksum_bad.is_none() {
                checksum_bad = Some(k);
            }
            if f.role == ROLE_SOURCE {
                source_sha = sha.clone();
            }
        }
        read_before += r.size;
        if !tick(read_before) {
            return Judged::Cancel;
        }
        files.push(PlannedFile {
            path: f.path.clone(),
            role: f.role.clone(),
            form: f.form.clone(),
            title: f.title.clone(),
            page_map: f.page_map.clone(),
            proofread: f.proofread,
            sha256: f.sha256.clone(),
            bytes: r.size,
        });
    }
    if let Some(k) = checksum_bad {
        return skip(CHECKSUM_MISMATCH, &entry.files[k].path);
    }

    let mut bib = entry.bib;
    if bib.title.is_none() {
        bib.title = files
            .iter()
            .find(|f| f.role == ROLE_SOURCE)
            .and_then(|f| title_from_file_name(last_segment(&f.path)));
    }
    let mut notices = Vec::new();
    if !entry.dropped.is_empty() {
        let listed: Vec<&str> = entry
            .dropped
            .iter()
            .take(MAX_DROPPED_LISTED)
            .map(String::as_str)
            .collect();
        notices.push(ImportNotice {
            kind: notice::BIB_FIELDS_DROPPED.to_string(),
            detail: Some(listed.join(", ")),
        });
    }
    Judged::Accept(Box::new(ScannedPaper {
        index,
        bib,
        short_name: entry.short_name,
        aliases: entry.aliases,
        files,
        source_sha,
        notices,
    }))
}

/// 內容符合 `form`：大小大於 0；PDF 前 1024 個位元組內有 `%PDF-`（Markdown 的 UTF-8 在讀的時候驗）。
/// 重點筆記另外要讀得懂（重點筆記的格式）：核心的 `digest` 角色一律照那個格式讀（檢查文獻庫、同步、閱讀畫面）。
pub(crate) fn content_ok(role: &str, form: &str, seen: &Streamed) -> bool {
    if seen.bytes == 0 || (form == FORM_PDF && !looks_like_pdf(&seen.head)) {
        return false;
    }
    if role == ROLE_DIGEST {
        return seen
            .body
            .as_deref()
            .and_then(|b| std::str::from_utf8(b).ok())
            .is_some_and(|t| crate::digest::parse(t).is_ok());
    }
    true
}

/// 重點筆記屬性區的 `paperreader_proofread`（沒寫是 `false`）。匯入時材料的「已校對」取這個值。
pub(crate) fn digest_proofread(seen: &Streamed) -> bool {
    seen.body
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|t| crate::digest::parse(t).ok())
        .is_some_and(|p| p.doc().proofread)
}

// ---- 讀內容 ----

/// 讀過一個檔之後知道的事。
pub(crate) struct Streamed {
    /// 讀完整份才有。
    pub sha: Option<String>,
    pub bytes: u64,
    /// 前 1024 個位元組。
    pub head: Vec<u8>,
    /// 整份內容（只有重點筆記留著，要解析）。
    pub body: Option<Vec<u8>>,
}

/// 讀一個檔的規則。
#[derive(Clone, Copy)]
pub(crate) struct ReadRules {
    /// 讀取出錯時的代碼。
    pub read_code: &'static str,
    /// zip 項目宣告的大小：讀超過就停（`form-mismatch`）。
    pub limit: Option<u64>,
    /// 驗整份是合法的 UTF-8（Markdown）。
    pub utf8: bool,
    /// 留下整份內容，最多這麼多（重點筆記）；超過 → `form-mismatch`。
    pub keep: Option<u64>,
}

impl ReadRules {
    pub(crate) fn new(opened: &Opened, r: &Resolved, role: &str, form: &str) -> ReadRules {
        ReadRules {
            read_code: opened.read_code(),
            limit: opened.limit(r),
            utf8: form == FORM_MARKDOWN,
            keep: (role == ROLE_DIGEST).then_some(MAX_DIGEST_BYTES),
        }
    }
}

/// 讀到一半停下的原因。
pub(crate) enum Halt {
    /// 這篇略過（代碼）。
    Skip(&'static str),
    Cancel,
    /// 寫出去時的錯誤（寫入時才有）。
    Write(LibError),
}

/// 讀完 `input`：邊讀邊算指紋、交給 `sink`（寫入時複製到暫存資料夾），照 `rules` 驗 UTF-8、
/// 停在 zip 宣告的大小、留下整份內容。`tick(已讀的位元組)` 交回 `false` 表示取消。
pub(crate) fn stream(
    input: &mut dyn Read,
    rules: &ReadRules,
    sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    tick: &mut dyn FnMut(u64) -> bool,
) -> std::result::Result<Streamed, Halt> {
    let ReadRules {
        read_code,
        limit,
        utf8,
        keep,
    } = *rules;
    let mut halt: Option<Halt> = None;
    let mut read_failed = false;
    let mut head: Vec<u8> = Vec::with_capacity(PDF_HEADER_WINDOW);
    let mut body: Option<Vec<u8>> = keep.map(|_| Vec::new());
    let mut text = Utf8Check::default();
    let mut seen = 0u64;
    // 停下的原因記在 `halt`；交給 `hash_reader` 的錯誤只是讓它停下
    let stop = || LibError::Conflict;
    let r = hash_reader(
        input,
        |_| {
            read_failed = true;
            stop()
        },
        |chunk| {
            seen += chunk.len() as u64;
            if limit.is_some_and(|l| seen > l)
                || keep.is_some_and(|k| seen > k)
                || (utf8 && !text.feed(chunk))
            {
                halt = Some(Halt::Skip(FORM_MISMATCH));
                return Err(stop());
            }
            if let Some(b) = body.as_mut() {
                b.extend_from_slice(chunk);
            }
            if head.len() < PDF_HEADER_WINDOW {
                let take = (PDF_HEADER_WINDOW - head.len()).min(chunk.len());
                head.extend_from_slice(&chunk[..take]);
            }
            if let Err(e) = sink(chunk) {
                halt = Some(Halt::Write(e));
                return Err(stop());
            }
            if !tick(seen) {
                halt = Some(Halt::Cancel);
                return Err(stop());
            }
            Ok(())
        },
    );
    match r {
        Ok((sha, bytes)) => {
            if utf8 && !text.finish() {
                return Err(Halt::Skip(FORM_MISMATCH));
            }
            Ok(Streamed {
                sha: Some(sha),
                bytes,
                head,
                body,
            })
        }
        Err(_) => Err(halt.unwrap_or(Halt::Skip(if read_failed {
            read_code
        } else {
            FORM_MISMATCH
        }))),
    }
}

/// 只讀前 1024 個位元組（不需要指紋的 PDF）。
fn peek(input: Box<dyn Read + '_>, read_code: &'static str) -> std::result::Result<Streamed, Halt> {
    let mut head = Vec::with_capacity(PDF_HEADER_WINDOW);
    input
        .take(PDF_HEADER_WINDOW as u64)
        .read_to_end(&mut head)
        .map_err(|_| Halt::Skip(read_code))?;
    Ok(Streamed {
        sha: None,
        bytes: head.len() as u64,
        head,
        body: None,
    })
}

/// 分段驗 UTF-8：一個字可能跨在兩段之間，把結尾不完整的位元組留到下一段。開頭的位元組順序記號是合法的 UTF-8。
#[derive(Default)]
struct Utf8Check {
    carry: Vec<u8>,
}

impl Utf8Check {
    fn feed(&mut self, chunk: &[u8]) -> bool {
        let joined;
        let data: &[u8] = if self.carry.is_empty() {
            chunk
        } else {
            joined = [self.carry.as_slice(), chunk].concat();
            &joined
        };
        match std::str::from_utf8(data) {
            Ok(_) => {
                self.carry.clear();
                true
            }
            // 結尾停在一個字的中間：留著等下一段
            Err(e) if e.error_len().is_none() => {
                self.carry = data[e.valid_up_to()..].to_vec();
                true
            }
            Err(_) => false,
        }
    }

    fn finish(&self) -> bool {
        self.carry.is_empty()
    }
}

// ---- 容器 ----

/// 在容器裡找到的一個檔。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    loc: Loc,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Loc {
    File(PathBuf),
    Entry(usize),
}

/// zip 裡的一個項目（依名稱查）。
#[derive(Debug, Clone)]
pub(crate) struct ZipItem {
    index: usize,
    size: u64,
    /// 資料夾、符號連結、加密、名稱完全相同的兩項：清單引用到 → `bad-path`。
    unusable: bool,
}

/// 打開的容器：資料夾（正規化後的根目錄）或 zip。
#[derive(Debug)]
pub(crate) enum Opened {
    Folder {
        root: PathBuf,
        /// 掃描時快取每個資料夾的項目名稱（寫入時每次重讀）。
        listings: Option<HashMap<PathBuf, Vec<OsString>>>,
    },
    Zip {
        archive: zip::ZipArchive<fs::File>,
        /// 名稱合乎路徑規則的項目，依名稱排序（找「某個路徑是不是資料夾」時查以 `路徑/` 開頭的名稱）。
        /// 記憶體和名稱的總長度成正比：不事先展開每一層上層資料夾。
        items: BTreeMap<String, ZipItem>,
    },
}

fn unreadable(container: &Path, reason: impl Into<String>) -> LibError {
    LibError::Unreadable {
        path: container.display().to_string(),
        reason: reason.into(),
    }
}

impl Opened {
    /// 打開容器、讀出清單的位元組。`cache`：掃描時快取資料夾的項目名稱。
    pub(crate) fn open(container: &ImportContainer, cache: bool) -> Result<(Opened, Vec<u8>)> {
        match container {
            ImportContainer::Folder(p) => Self::open_folder(p, cache),
            ImportContainer::Zip(p) => Self::open_zip(p),
        }
    }

    fn open_folder(given: &Path, cache: bool) -> Result<(Opened, Vec<u8>)> {
        // 根目錄只正規化一次；根目錄本身是符號連結（例如 Android 的 /sdcard）不受限制
        let root = fs::canonicalize(given)
            .map_err(|e| unreadable(given, format!("cannot open the folder: {e}")))?;
        if !root.is_dir() {
            return Err(unreadable(given, "not a folder"));
        }
        let names = read_names(&root).ok_or_else(|| unreadable(given, "cannot list the folder"))?;
        if !names.iter().any(|n| n == MANIFEST_NAME) {
            return Err(unreadable(given, format!("{MANIFEST_NAME} is missing")));
        }
        let mp = root.join(MANIFEST_NAME);
        let meta = fs::symlink_metadata(&mp)
            .map_err(|e| unreadable(given, format!("cannot read {MANIFEST_NAME}: {e}")))?;
        if !meta.is_file() {
            return Err(unreadable(
                given,
                format!("{MANIFEST_NAME} is not a regular file"),
            ));
        }
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(unreadable(given, format!("{MANIFEST_NAME} is too large")));
        }
        let mut bytes = Vec::new();
        fs::File::open(&mp)
            .and_then(|f| f.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| unreadable(given, format!("cannot read {MANIFEST_NAME}: {e}")))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(unreadable(given, format!("{MANIFEST_NAME} is too large")));
        }
        let listings = cache.then(|| HashMap::from([(root.clone(), names)]));
        Ok((Opened::Folder { root, listings }, bytes))
    }

    fn open_zip(path: &Path) -> Result<(Opened, Vec<u8>)> {
        let file =
            fs::File::open(path).map_err(|e| unreadable(path, format!("cannot open: {e}")))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| unreadable(path, e.to_string()))?;
        if archive.len() > MAX_ZIP_ENTRIES {
            return Err(unreadable(
                path,
                format!("more than {MAX_ZIP_ENTRIES} entries"),
            ));
        }
        let mut items: BTreeMap<String, ZipItem> = BTreeMap::new();
        let mut sum = 0u64;
        for i in 0..archive.len() {
            let e = archive
                .by_index_data(i)
                .map_err(|e| unreadable(path, e.to_string()))?;
            // 名稱不是 UTF-8 的項目，清單引用不到它：略過不看
            let Ok(name) = std::str::from_utf8(e.name_raw()) else {
                continue;
            };
            // zip 格式規定路徑用 `/`；Windows PowerShell 5.1 的 `Compress-Archive` 寫成 `\`。不替它開例外，但整份不收、
            // 說清楚原因，不讓使用者只看到每篇都找不到檔。外殼靠這個開頭認出它
            if name.contains('\\') {
                return Err(unreadable(path, format!("{ZIP_BACKSLASH}: {name}")));
            }
            let is_dir = e.is_dir();
            let key = name.strip_suffix('/').unwrap_or(name).to_string();
            if !is_dir && key != MANIFEST_NAME {
                if e.size() > MAX_ZIP_ENTRY_BYTES {
                    return Err(unreadable(path, format!("{name} is too large")));
                }
                sum = sum.saturating_add(e.size());
                if sum > MAX_ZIP_TOTAL_BYTES {
                    return Err(unreadable(path, "the entries are too large in total"));
                }
            }
            // 名稱不合路徑規則的項目（例如 `__MACOSX/` 以外的怪名稱、超過 1024 個位元組），清單的路徑一定對不上
            // 它，也不拿它判斷上層資料夾：略過不看
            if !is_safe_import_path(&key) {
                continue;
            }
            let symlink = e.is_symlink() || e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
            let unusable = is_dir || symlink || e.encrypted();
            match items.get_mut(&key) {
                // 名稱完全相同的兩項：引用到就是 bad-path
                Some(prev) => prev.unusable = true,
                None => {
                    items.insert(
                        key,
                        ZipItem {
                            index: i,
                            size: e.size(),
                            unusable,
                        },
                    );
                }
            }
        }
        let m = match items.get(MANIFEST_NAME) {
            None => {
                return Err(unreadable(
                    path,
                    format!("{MANIFEST_NAME} is not at the root of the archive"),
                ));
            }
            Some(m) if m.unusable => {
                return Err(unreadable(
                    path,
                    format!("{MANIFEST_NAME} is not a readable file"),
                ));
            }
            Some(m) => m.clone(),
        };
        if m.size > MAX_MANIFEST_BYTES {
            return Err(unreadable(path, format!("{MANIFEST_NAME} is too large")));
        }
        let mut bytes = Vec::new();
        archive
            .by_index(m.index)
            .map_err(|e| unreadable(path, e.to_string()))?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| unreadable(path, format!("cannot read {MANIFEST_NAME}: {e}")))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(unreadable(path, format!("{MANIFEST_NAME} is too large")));
        }
        Ok((Opened::Zip { archive, items }, bytes))
    }

    /// 依清單的 `path`（已經過路徑規則）找檔：名稱要完全相同（大小寫也是）；不存在 → `missing-file`；
    /// 經過符號連結、不是一般檔案、zip 裡加密 → `bad-path`。
    pub(crate) fn resolve(&mut self, path: &str) -> Located {
        match self {
            Opened::Folder { root, listings } => resolve_in_folder(root, listings, path),
            Opened::Zip { items, .. } => match items.get(path) {
                // 沒有這個名稱的項目，但有以 `路徑/` 開頭的：是資料夾（沒有自己的項目也算）
                None => {
                    let dir = format!("{path}/");
                    let below = items
                        .range::<str, _>((Bound::Included(dir.as_str()), Bound::Unbounded))
                        .next()
                        .is_some_and(|(k, _)| k.starts_with(&dir));
                    Err(if below { BAD_PATH } else { MISSING_FILE })
                }
                Some(it) if it.unusable => Err(BAD_PATH),
                Some(it) => Ok(Resolved {
                    loc: Loc::Entry(it.index),
                    size: it.size,
                }),
            },
        }
    }

    /// 讀取途中出錯時的代碼：資料夾是「讀不到」，zip 是「項目壞了」。
    pub(crate) fn read_code(&self) -> &'static str {
        match self {
            Opened::Folder { .. } => MISSING_FILE,
            Opened::Zip { .. } => FORM_MISMATCH,
        }
    }

    /// 讀取的上限：zip 項目宣告的大小。
    pub(crate) fn limit(&self, r: &Resolved) -> Option<u64> {
        match self {
            Opened::Folder { .. } => None,
            Opened::Zip { .. } => Some(r.size),
        }
    }

    /// 打開找到的檔。zip 項目最多讀宣告的大小加 1（讀超過就停，由 `stream` 判斷）。
    pub(crate) fn reader(
        &mut self,
        r: &Resolved,
    ) -> std::result::Result<Box<dyn Read + '_>, &'static str> {
        match (self, &r.loc) {
            (Opened::Folder { .. }, Loc::File(p)) => fs::File::open(p)
                .map(|f| Box::new(f) as Box<dyn Read>)
                .map_err(|_| MISSING_FILE),
            (Opened::Zip { archive, .. }, Loc::Entry(i)) => archive
                .by_index(*i)
                .map(|f| Box::new(f.take(r.size.saturating_add(1))) as Box<dyn Read + '_>)
                .map_err(|_| FORM_MISMATCH),
            _ => Err(MISSING_FILE),
        }
    }
}

/// 資料夾裡依名稱找檔：逐段讀上層資料夾的項目，名稱要完全相同；每一段用 `symlink_metadata` 檢查。
fn resolve_in_folder(
    root: &Path,
    listings: &mut Option<HashMap<PathBuf, Vec<OsString>>>,
    path: &str,
) -> Located {
    let segs: Vec<&str> = path.split('/').collect();
    let mut cur = root.to_path_buf();
    for (k, seg) in segs.iter().enumerate() {
        let present = match listings {
            Some(cache) => cache
                .entry(cur.clone())
                .or_insert_with(|| read_names(&cur).unwrap_or_default())
                .iter()
                .any(|n| n == seg),
            None => read_names(&cur).is_some_and(|names| names.iter().any(|n| n == seg)),
        };
        if !present {
            return Err(MISSING_FILE);
        }
        cur.push(seg);
        let meta = fs::symlink_metadata(&cur).map_err(|_| MISSING_FILE)?;
        if meta.file_type().is_symlink() {
            return Err(BAD_PATH);
        }
        if k + 1 == segs.len() {
            if !meta.is_file() {
                return Err(BAD_PATH);
            }
            return Ok(Resolved {
                loc: Loc::File(cur),
                size: meta.len(),
            });
        }
        if !meta.is_dir() {
            return Err(MISSING_FILE);
        }
    }
    Err(MISSING_FILE)
}

fn read_names(dir: &Path) -> Option<Vec<OsString>> {
    let rd = fs::read_dir(dir).ok()?;
    Some(rd.flatten().map(|e| e.file_name()).collect())
}

// ---- 清單 ----

/// 整份的檢查：交回 `papers` 的每一項。
fn parse_manifest(container: &Path, bytes: &[u8]) -> Result<Vec<Value>> {
    let body = bytes.strip_prefix(&BOM).unwrap_or(bytes);
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| unreadable(container, format!("{MANIFEST_NAME} is not valid JSON: {e}")))?;
    let Value::Object(mut obj) = value else {
        return Err(unreadable(container, "not an import manifest"));
    };
    if obj.get("kind").and_then(Value::as_str) != Some(MANIFEST_KIND) {
        return Err(unreadable(container, "not an import manifest"));
    }
    let format = match obj.get("format") {
        Some(Value::Number(n)) if n.is_u64() || n.is_i64() => n.as_u64(),
        _ => return Err(unreadable(container, "format must be an integer")),
    };
    match format {
        Some(f) if f > MANIFEST_FORMAT => {
            return Err(LibError::FormatTooNew {
                found: u32::try_from(f).unwrap_or(u32::MAX),
            });
        }
        Some(f) if f >= 1 => {}
        _ => return Err(unreadable(container, "unknown format")),
    }
    let papers = match obj.remove("papers") {
        Some(Value::Array(a)) => a,
        _ => return Err(unreadable(container, "papers must be an array")),
    };
    if papers.is_empty() {
        return Err(unreadable(container, "papers is empty"));
    }
    if papers.len() > MAX_PAPERS {
        return Err(unreadable(
            container,
            format!("more than {MAX_PAPERS} papers"),
        ));
    }
    Ok(papers)
}

/// 一篇檢查過欄位、角色與路徑規則之後的樣子。
struct Entry {
    bib: BibDraft,
    /// 沒收的 CSL 鍵名（含型別不對而當作沒有的）。
    dropped: Vec<String>,
    short_name: Option<String>,
    aliases: Vec<String>,
    files: Vec<FileSpec>,
}

struct FileSpec {
    path: String,
    role: String,
    form: String,
    title: Option<String>,
    page_map: Option<String>,
    proofread: Option<bool>,
    sha256: Option<String>,
}

/// 匯入清單的各項規則與路徑規則，依代碼的優先序：`bad-entry` → `too-many-files` → `bad-role` →
/// `no-source`／`multiple-sources` → `bad-path`。
fn check_entry(i: usize, v: &Value) -> std::result::Result<Entry, ImportSkip> {
    let index = i as u32;
    let bad = |loc: String| ImportSkip::new(index, BAD_ENTRY, Some(loc));
    let Some(obj) = v.as_object() else {
        return Err(bad(format!("papers[{i}]")));
    };
    let (bib, dropped) = match field(obj, "csl") {
        None => (BibDraft::default(), Vec::new()),
        Some(Value::Object(m)) => map_csl(m).map_err(bad)?,
        Some(_) => return Err(bad("csl".into())),
    };
    let short_name = optional_text(obj, "short_name", "short_name").map_err(bad)?;
    let aliases = match field(obj, "aliases") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            // 去重用集合，和別名數成正比；保留第一次出現的順序
            let mut out: Vec<String> = Vec::new();
            let mut seen: HashSet<&str> = HashSet::new();
            for (k, a) in items.iter().enumerate() {
                let Some(s) = a.as_str() else {
                    return Err(bad(format!("aliases[{k}]")));
                };
                if s.len() > MAX_STRING_BYTES {
                    return Err(bad(format!("aliases[{k}]")));
                }
                let t = s.trim();
                if !t.is_empty() && seen.insert(t) {
                    out.push(t.to_string());
                }
            }
            out
        }
        Some(_) => return Err(bad("aliases".into())),
    };
    let files = match field(obj, "files") {
        Some(Value::Array(a)) if !a.is_empty() => a,
        _ => return Err(bad("files".into())),
    };
    let files = files
        .iter()
        .enumerate()
        .map(|(k, f)| check_file(k, f))
        .collect::<std::result::Result<Vec<_>, String>>()
        .map_err(bad)?;

    if files.len() > MAX_FILES {
        return Err(ImportSkip::new(index, TOO_MANY_FILES, None));
    }
    if let Some(k) = files
        .iter()
        .position(|f| !role_takes_form(&f.role, &f.form))
    {
        return Err(ImportSkip::new(
            index,
            BAD_ROLE,
            Some(format!("files[{k}]")),
        ));
    }
    match files.iter().filter(|f| f.role == ROLE_SOURCE).count() {
        0 => return Err(ImportSkip::new(index, NO_SOURCE, None)),
        1 => {}
        _ => return Err(ImportSkip::new(index, MULTIPLE_SOURCES, None)),
    }
    if let Some(f) = files.iter().find(|f| !is_safe_import_path(&f.path)) {
        return Err(ImportSkip::new(index, BAD_PATH, Some(f.path.clone())));
    }
    Ok(Entry {
        bib,
        dropped,
        short_name,
        aliases,
        files,
    })
}

/// 清單裡每個檔的檢查；錯了交回欄位位置。
fn check_file(k: usize, v: &Value) -> std::result::Result<FileSpec, String> {
    let at = |name: &str| format!("files[{k}].{name}");
    let Some(obj) = v.as_object() else {
        return Err(format!("files[{k}]"));
    };
    let path = match field(obj, "path") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(at("path")),
    };
    let one_of = |name: &str, allowed: &[&str]| -> std::result::Result<String, String> {
        match field(obj, name) {
            Some(Value::String(s)) if allowed.contains(&s.as_str()) => Ok(s.clone()),
            _ => Err(at(name)),
        }
    };
    let role = one_of(
        "role",
        &[ROLE_SOURCE, ROLE_TRANSLATION, ROLE_DIGEST, ROLE_TEXT],
    )?;
    let form = one_of("form", &[FORM_PDF, FORM_MARKDOWN])?;
    let title = optional_text(obj, "title", &at("title"))?;
    let page_map = match field(obj, "page_map") {
        None => None,
        Some(_) => Some(one_of("page_map", &PAGE_MAPS)?),
    };
    let proofread = match field(obj, "proofread") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(at("proofread")),
    };
    let sha256 = match field(obj, "sha256") {
        None => None,
        Some(Value::String(s)) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(s.to_ascii_lowercase())
        }
        Some(_) => return Err(at("sha256")),
    };
    Ok(FileSpec {
        path,
        role,
        form,
        title,
        page_map,
        proofread,
        sha256,
    })
}

/// 原文只能是 PDF；譯本 PDF 或 Markdown；重點筆記與抽出文字只能是 Markdown。
fn role_takes_form(role: &str, form: &str) -> bool {
    match role {
        ROLE_SOURCE => form == FORM_PDF,
        ROLE_TRANSLATION => form == FORM_PDF || form == FORM_MARKDOWN,
        _ => form == FORM_MARKDOWN,
    }
}

/// 匯入清單的路徑規則：備份檔的路徑規則（`is_safe_backup_path`），再加上不含任何控制字元。
pub(crate) fn is_safe_import_path(path: &str) -> bool {
    is_safe_backup_path(path) && !path.chars().any(char::is_control)
}

/// 欄位的值；沒寫與寫 `null` 一樣是 `None`。
fn field<'a>(obj: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    obj.get(name).filter(|v| !v.is_null())
}

fn check_len(s: &str, loc: &str) -> std::result::Result<(), String> {
    if s.len() > MAX_STRING_BYTES {
        Err(loc.to_string())
    } else {
        Ok(())
    }
}

/// 選填的文字欄位：去頭尾空白，空字串等同沒寫；不是字串或太長 → 欄位位置 `loc`。
fn optional_text(
    obj: &Map<String, Value>,
    name: &str,
    loc: &str,
) -> std::result::Result<Option<String>, String> {
    match field(obj, name) {
        None => Ok(None),
        Some(Value::String(s)) => {
            check_len(s, loc)?;
            Ok(non_empty(s))
        }
        Some(_) => Err(loc.to_string()),
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

// ---- CSL ----

/// CSL 條目寬鬆地對照到書目。型別不對的欄位當作沒有、記進沒收的鍵名；讀到的字串超過 4096 個位元組
/// 交回欄位位置（`bad-entry`）。
fn map_csl(m: &Map<String, Value>) -> std::result::Result<(BibDraft, Vec<String>), String> {
    let mut bib = BibDraft::default();
    let mut dropped: Vec<String> = Vec::new();
    for (key, val) in m {
        if val.is_null() {
            continue;
        }
        let text = |loc: &str| -> std::result::Result<Option<Option<String>>, String> {
            match val {
                Value::String(s) => {
                    check_len(s, loc)?;
                    Ok(Some(non_empty(s)))
                }
                _ => Ok(None),
            }
        };
        let ok = match key.as_str() {
            "id" | "type" => true,
            "title" => text("csl.title")?.map(|t| bib.title = t).is_some(),
            "container-title" => text("csl.container-title")?
                .map(|t| bib.venue = t)
                .is_some(),
            "URL" => text("csl.URL")?.map(|t| bib.url = t).is_some(),
            "DOI" => text("csl.DOI")?
                .map(|t| {
                    bib.doi = t.map(|d| normalize_doi(&d)).filter(|d| !d.is_empty());
                })
                .is_some(),
            "author" => match val {
                Value::Array(items) => {
                    for (j, it) in items.iter().enumerate() {
                        if let Some(a) = map_author(j, it)? {
                            bib.authors.push(a);
                        }
                    }
                    true
                }
                _ => false,
            },
            "issued" => match map_year(val) {
                Some(y) => {
                    bib.year = y;
                    true
                }
                None => false,
            },
            _ => false,
        };
        if !ok {
            dropped.push(key.clone());
        }
    }
    Ok((bib, dropped))
}

/// 一位作者：`family` 是非空字串時取 `family`（`given` 是非空字串時一起取）；否則 `literal` 是非空字串時
/// 只取 `literal`；都沒有、或這一項不是物件，交回 `None`（丟掉那一項）。不從 `literal` 猜姓名。
fn map_author(j: usize, v: &Value) -> std::result::Result<Option<Author>, String> {
    let Some(o) = v.as_object() else {
        return Ok(None);
    };
    let get = |name: &str| -> std::result::Result<Option<String>, String> {
        match o.get(name) {
            Some(Value::String(s)) => {
                check_len(s, &format!("csl.author[{j}].{name}"))?;
                Ok(non_empty(s))
            }
            _ => Ok(None),
        }
    };
    if let Some(family) = get("family")? {
        return Ok(Some(Author {
            family: Some(family),
            given: get("given")?,
            ..Default::default()
        }));
    }
    Ok(get("literal")?.map(|literal| Author {
        literal: Some(literal),
        ..Default::default()
    }))
}

/// `issued.date-parts[0][0]`：整數或全是數字的字串，1000 到 9999 之外當作沒有；只有 `raw`、`literal` 不猜。
/// 交回 `None` 表示型別不對（記成沒收）；`Some(None)` 是沒有年份。
fn map_year(v: &Value) -> Option<Option<i32>> {
    let o = v.as_object()?;
    let parts = match o.get("date-parts") {
        None | Some(Value::Null) => return Some(None),
        Some(Value::Array(a)) => a,
        Some(_) => return None,
    };
    let first = match parts.first() {
        None => return Some(None),
        Some(Value::Array(p)) => p,
        Some(_) => return None,
    };
    let year: i64 = match first.first() {
        None | Some(Value::Null) => return Some(None),
        Some(Value::Number(n)) if !n.is_f64() => n.as_i64().unwrap_or(i64::MAX),
        Some(Value::String(s)) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().unwrap_or(i64::MAX)
        }
        Some(_) => return None,
    };
    Some(
        (1000..=9999)
            .contains(&year)
            .then(|| i32::try_from(year).unwrap_or_default()),
    )
}
