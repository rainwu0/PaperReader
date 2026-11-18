//! 筆記檔的讀寫與索引。
//!
//! 筆記檔放在 `<根目錄>/<notes_dir>/`，一篇一份，以檔案開頭屬性區的 `paperreader_paper`
//! 對應文獻（不靠檔名）。索引（文獻代號 → 檔名）只存在記憶體裡，開啟與重新掃描時建立，
//! 建索引是盡力而為：讀不了的檔略過，不讓開啟或重新掃描失敗。

use super::{LocalLibrary, TEMP_PREFIX, is_valid_material_path};
use crate::changes::{
    HIGHLIGHT_DELETED, THOUGHTS_WRITTEN, thoughts_hash, thoughts_record_for_write,
};
use crate::error::{LibError, Result, io_error};
use crate::format::PaperFile;
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, sha256_hex, write_bytes_atomic};
use crate::naming::unique_stem;
use crate::notes::{
    self, HEAD_BYTES, HEAD_LINES, Highlight, NotesLabels, ParsedNotes, Stamp, check_free_text,
    check_quote, clean_title, new_file, parse, read_paper_id_from_bytes, sanitize_label,
};
use crate::store::{HighlightDraft, NotesDoc};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 頁序的上限（不含）。
const MAX_PAGE_INDEX: u32 = 100_000;
const MAX_BASIS_CHARS: usize = 64;
const MAX_FINGERPRINT_CHARS: usize = 128;

/// `notes_dir` 的檢查：材料路徑的規則（相對、不含 `..`、不以 `@` 開頭、不是絕對路徑）
/// 再加上：不含 `\`；以 `/` 切開的每一段非空、不是 `.`、不以 `.` 開頭；第一段不是
/// `papers`、`logs`、`jobs`、`library.json`。
///
/// 理由：`.` 會把筆記寫進根目錄（變成「不認得的項目」）；`papers/某篇` 會寫進文獻資料夾；
/// 以 `.` 開頭的位置在匯出時被當成隱藏項目略過、筆記進不了備份。
pub fn is_valid_notes_dir(dir: &str) -> bool {
    if !is_valid_material_path(dir) || dir.contains('\\') {
        return false;
    }
    let mut segs = dir.split('/');
    let first = segs.clone().next().unwrap_or("");
    if ["papers", "logs", "jobs", "library.json"].contains(&first) {
        return false;
    }
    segs.all(|s| !s.is_empty() && !s.starts_with('.'))
}

// ---- 索引 ----

#[derive(Debug, Clone)]
struct NoteFile {
    mtime: Option<SystemTime>,
    size: u64,
    /// 屬性區的 `paperreader_paper`；沒有（使用者自己的筆記）是 `None`。
    paper: Option<String>,
    /// 讀得懂，而且有劃線、想法或讀不懂但留著的段落。只看檔頭的掃描是 `false`。
    has_notes: bool,
}

/// 文獻代號 → 筆記檔名，連同每個檔的修改時間與大小（重新掃描時只重讀有變的）。
#[derive(Debug, Default, Clone)]
pub(crate) struct NotesIndex {
    /// 依檔名的位元組順序。
    files: BTreeMap<String, NoteFile>,
}

impl NotesIndex {
    /// 屬性區有 `paperreader_paper` 的檔名，依檔名的位元組順序。
    pub(crate) fn claimed_files(&self) -> Vec<String> {
        self.files
            .iter()
            .filter(|(_, f)| f.paper.is_some())
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// （檔名, 文獻代號），依檔名的位元組順序。
    pub(crate) fn entries(&self) -> Vec<(String, String)> {
        self.files
            .iter()
            .filter_map(|(n, f)| f.paper.clone().map(|p| (n.clone(), p)))
            .collect()
    }

    /// 兩份檔案都說自己屬於同一篇：依檔名的位元組順序第一個算數。
    pub(crate) fn file_for(&self, paper_id: &str) -> Option<&str> {
        self.entry_for(paper_id).map(|(name, _)| name.as_str())
    }

    /// 這篇有沒有筆記：看 `file_for` 取的那一份。沒有筆記檔是 `false`。
    pub(crate) fn has_notes(&self, paper_id: &str) -> bool {
        self.entry_for(paper_id).is_some_and(|(_, f)| f.has_notes)
    }

    fn entry_for(&self, paper_id: &str) -> Option<(&String, &NoteFile)> {
        self.files
            .iter()
            .find(|(_, f)| f.paper.as_deref() == Some(paper_id))
    }
}

/// 筆記檔的內容算不算「有筆記」：有劃線、整篇的想法去掉空白後不是空的，或讀不懂但留著的段落。
fn text_has_notes(text: &str) -> bool {
    let p = parse(text);
    !p.highlights().is_empty() || !p.thoughts().trim().is_empty() || !p.unreadable().is_empty()
}

/// 取檔案開頭只看屬性區用的部分：最多 64 行、8 KB（切在字中間時由 `read_paper_id_from_bytes` 處理）。
fn index_head(bytes: &[u8]) -> &[u8] {
    let mut lines = 0;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            lines += 1;
            if lines == HEAD_LINES {
                return &bytes[..=i];
            }
        }
    }
    bytes
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    let f = fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity(HEAD_BYTES);
    f.take(HEAD_BYTES as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// 筆記檔的名稱：`.md` 結尾、不以 `.` 開頭。
fn is_note_name(name: &str) -> bool {
    name.ends_with(".md") && !name.starts_with('.')
}

/// 掃描 `notes_dir` 底下直接一層的 `.md` 檔案，連同每一篇有沒有筆記（整份讀）。`notes_dir` 不合規則、
/// 不存在、讀不了都是空的索引（不碰文獻庫外面或別處的東西）；讀不了的檔略過。`prev` 裡修改時間與大小
/// 都沒變的檔不重讀。存成文獻庫記憶體裡的索引的，都用這一個。
pub(crate) fn scan_notes_dir(
    root: &Path,
    notes_dir: &str,
    prev: Option<&NotesIndex>,
) -> NotesIndex {
    scan_notes(root, notes_dir, prev, true)
}

/// 同 `scan_notes_dir`，只讀每個檔的開頭（找文獻代號用，不算有沒有筆記），不存回記憶體的索引。
pub(crate) fn scan_notes_heads(root: &Path, notes_dir: &str) -> NotesIndex {
    scan_notes(root, notes_dir, None, false)
}

fn scan_notes(root: &Path, notes_dir: &str, prev: Option<&NotesIndex>, whole: bool) -> NotesIndex {
    let mut out = NotesIndex::default();
    if !is_valid_notes_dir(notes_dir) {
        return out;
    }
    let dir = root.join(notes_dir);
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_note_name(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().ok();
        let size = meta.len();
        if let Some(old) = prev.and_then(|p| p.files.get(&name))
            && old.mtime == mtime
            && old.size == size
        {
            out.files.insert(name, old.clone());
            continue;
        }
        let (paper, has_notes) = if whole {
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let paper = read_paper_id_from_bytes(index_head(&bytes[..bytes.len().min(HEAD_BYTES)]));
            // 不是 UTF-8 的筆記檔讀不懂，算沒有筆記
            let has = paper.is_some() && std::str::from_utf8(&bytes).is_ok_and(text_has_notes);
            (paper, has)
        } else {
            let Some(head) = read_head(&path) else {
                continue;
            };
            (read_paper_id_from_bytes(index_head(&head)), false)
        };
        out.files.insert(
            name,
            NoteFile {
                mtime,
                size,
                paper,
                has_notes,
            },
        );
    }
    out
}

/// `notes_dir` 對應的資料夾路徑；`notes_dir` 不合規則是 `None`。
pub(crate) fn notes_dir_path(root: &Path, notes_dir: &str) -> Option<PathBuf> {
    is_valid_notes_dir(notes_dir).then(|| root.join(notes_dir))
}

impl LocalLibrary {
    /// 設定新建文字用的標題與連結文字（畫面照介面語言設定）。
    pub fn set_notes_labels(&mut self, labels: NotesLabels) -> Result<()> {
        labels.validate()?;
        self.notes_labels = labels;
        Ok(())
    }

    fn notes_dir_or_invalid(&self) -> Result<PathBuf> {
        notes_dir_path(&self.root, &self.library.notes_dir).ok_or_else(|| LibError::InvalidInput {
            reason: format!("invalid notes_dir: {:?}", self.library.notes_dir),
        })
    }

    /// 找一篇文獻的筆記檔名：先看記憶體裡的索引，確認那份檔的屬性區代號對得上；
    /// 對不上、讀不到，或記憶體裡根本沒有這一篇，都重掃一次 `notes_dir` 再下結論。
    /// 這個版本不改記憶體（供 `&self` 的操作用）。
    pub(crate) fn find_notes_file(&self, paper_id: &str) -> Option<String> {
        let dir = notes_dir_path(&self.root, &self.library.notes_dir)?;
        if let Some(name) = self.notes_index.file_for(paper_id)
            && head_matches(&dir.join(name), paper_id)
        {
            return Some(name.to_string());
        }
        scan_notes_heads(&self.root, &self.library.notes_dir)
            .file_for(paper_id)
            .map(String::from)
    }

    /// 同 `find_notes_file`，重掃時把結果存回記憶體。
    fn find_notes_file_mut(&mut self, paper_id: &str) -> Option<String> {
        let dir = notes_dir_path(&self.root, &self.library.notes_dir)?;
        if let Some(name) = self.notes_index.file_for(paper_id)
            && head_matches(&dir.join(name), paper_id)
        {
            return Some(name.to_string());
        }
        self.notes_index = scan_notes_dir(&self.root, &self.library.notes_dir, None);
        self.notes_index.file_for(paper_id).map(String::from)
    }

    /// 讀一篇文獻的筆記。筆記檔不存在時交回空的 `NotesDoc` 與 `None`。
    pub fn read_notes(&self, paper_id: &str) -> Result<(NotesDoc, Option<VersionToken>)> {
        self.locate(paper_id)?;
        self.notes_dir_or_invalid()?;
        let Some(name) = self.find_notes_file(paper_id) else {
            return Ok((
                NotesDoc {
                    path: None,
                    modified: None,
                    modified_by: None,
                    thoughts: String::new(),
                    thoughts_complete: true,
                    highlights: Vec::new(),
                    unreadable: Vec::new(),
                },
                None,
            ));
        };
        let path = self.root.join(&self.library.notes_dir).join(&name);
        let (text, token) = read_notes_text(&path)?;
        Ok((self.notes_doc(&name, &text), Some(token)))
    }

    /// 一份筆記檔（`name` 是 `notes_dir` 裡的檔名，`text` 是它的內容）整理成 `NotesDoc`。`read_notes` 與
    /// `read_for_index` 共用。
    pub(crate) fn notes_doc(&self, name: &str, text: &str) -> NotesDoc {
        let parsed = parse(text);
        NotesDoc {
            path: Some(format!("{}/{}", self.library.notes_dir, name)),
            modified: parsed.modified().map(String::from),
            modified_by: parsed.modified_by().map(String::from),
            thoughts: parsed.thoughts(),
            thoughts_complete: parsed.thoughts_complete(),
            highlights: parsed.highlights(),
            unreadable: parsed.unreadable(),
        }
    }

    /// 加一條劃線。筆記檔還不存在時 `token` 帶 `None`（第一次寫入時建立）。
    pub fn add_highlight(
        &mut self,
        paper_id: &str,
        draft: HighlightDraft,
        token: Option<&VersionToken>,
    ) -> Result<(Highlight, VersionToken)> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        self.notes_dir_or_invalid()?;
        let m = paper
            .paper
            .materials
            .iter()
            .find(|m| m.id == draft.material)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {}", draft.material),
            })?;
        validate_draft(&draft, m.pages)?;
        let comment = check_free_text(&draft.comment, "comment")?;
        check_quote(&draft.quote)?;

        let now = now_utc();
        let h = Highlight {
            id: format!("h-{}", new_id().to_lowercase()),
            kind: "highlight".to_string(),
            material: draft.material.clone(),
            file: notes::FileRef {
                sha256: m.sha256.clone(),
                fingerprint: draft.fingerprint.clone(),
                extra: Default::default(),
            },
            page: notes::PageRef {
                index: draft.page_index,
                label: sanitize_label(draft.page_label.as_deref()),
                extra: Default::default(),
            },
            rects: draft.rects.iter().map(round_rect).collect(),
            next_page_rects: draft.next_page_rects.iter().map(round_rect).collect(),
            position: draft.position.clone(),
            quote: draft.quote.clone(),
            color: None,
            created: now.clone(),
            modified: now.clone(),
            extra: Default::default(),
            comment,
            complete: true,
        };
        let stamp = Stamp {
            time: now,
            device: self.device_id.clone(),
        };
        let pid = paper_id.to_string();
        let new_token = self.write_notes(
            paper_id,
            &paper.paper,
            &paper.folder,
            token,
            &stamp,
            false,
            |p, labels| p.with_new_highlight(&h, labels, &pid, &stamp),
            |_, _, _| Ok(()),
        )?;
        Ok((h, new_token))
    }

    /// 改一條劃線的短評。
    pub fn set_highlight_comment(
        &mut self,
        paper_id: &str,
        highlight_id: &str,
        comment: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        self.notes_dir_or_invalid()?;
        let stamp = self.stamp_now();
        let pid = paper_id.to_string();
        self.write_notes(
            paper_id,
            &paper.paper,
            &paper.folder,
            Some(token),
            &stamp,
            true,
            |p, labels| p.with_comment(highlight_id, comment, labels, &pid, &stamp),
            |_, _, _| Ok(()),
        )
    }

    /// 刪一條劃線。
    pub fn delete_highlight(
        &mut self,
        paper_id: &str,
        highlight_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        self.notes_dir_or_invalid()?;
        let stamp = self.stamp_now();
        self.write_notes(
            paper_id,
            &paper.paper,
            &paper.folder,
            Some(token),
            &stamp,
            true,
            |p, _| p.without_highlight(highlight_id, &stamp),
            |lib, _, _| {
                let rec = lib.new_change(
                    Some(paper_id),
                    HIGHLIGHT_DELETED,
                    json!({ "highlight": highlight_id }),
                );
                lib.append_change(&rec)
            },
        )
    }

    /// 改整篇的想法。筆記檔還不存在時 `token` 帶 `None`。
    pub fn set_thoughts(
        &mut self,
        paper_id: &str,
        text: &str,
        token: Option<&VersionToken>,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        self.notes_dir_or_invalid()?;
        // 先檢查文字，不合規則時一個位元組也不寫
        check_free_text(text, "thoughts")?;
        let stamp = self.stamp_now();
        self.write_notes(
            paper_id,
            &paper.paper,
            &paper.folder,
            token,
            &stamp,
            false,
            |p, labels| p.with_thoughts(text, labels, &stamp),
            |lib, before, new_text| lib.record_thoughts_write(paper_id, before, new_text),
        )
    }

    /// `set_thoughts` 的記錄：寫入前後的想法相同不記；節流期間不記；其餘附加一筆。
    fn record_thoughts_write(
        &mut self,
        paper_id: &str,
        before: &ParsedNotes,
        new_text: &str,
    ) -> Result<()> {
        let before_hash = thoughts_hash(&before.thoughts());
        let new_hash = thoughts_hash(&parse(new_text).thoughts());
        let records = super::changes::read_all_changes(&self.root)?;
        let Some(prev) = thoughts_record_for_write(
            &records,
            &self.device_id,
            paper_id,
            &before_hash,
            &new_hash,
            crate::ids::now_utc_millis(),
        ) else {
            return Ok(());
        };
        let rec = self.new_change(
            Some(paper_id),
            THOUGHTS_WRITTEN,
            json!({ "sha256": new_hash, "prev": prev }),
        );
        self.append_change(&rec)
    }

    /// 每一篇有筆記檔（讀得了、想法完整）的文獻：（文獻代號, 目前的想法）。補記用。
    pub(crate) fn all_thoughts(&self) -> Vec<(String, String)> {
        let Some(dir) = notes_dir_path(&self.root, &self.library.notes_dir) else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (name, f) in &self.notes_index.files {
            let Some(paper) = f.paper.as_deref() else {
                continue;
            };
            if !seen.insert(paper.to_string()) {
                continue;
            }
            let Ok((text, _)) = read_notes_text(&dir.join(name)) else {
                continue;
            };
            let p = parse(&text);
            if p.thoughts_complete() {
                out.push((paper.to_string(), p.thoughts()));
            }
        }
        out
    }

    fn stamp_now(&self) -> Stamp {
        Stamp {
            time: now_utc(),
            device: self.device_id.clone(),
        }
    }

    /// 寫入的共通流程：找筆記檔；`token` 與檔案現在的內容不符（或檔案存在與否與 `token`
    /// 的 `None`／`Some` 不符）回報 `Conflict`；檔案不是 UTF-8 回報 `Unreadable`；用 `op` 算出新的內容，
    /// 以 `write_bytes_atomic` 寫入（`notes_dir` 不存在就建立）；更新索引。`missing_is_not_found`：
    /// 帶了記號、檔案卻不存在時，回報 `NotFound` 而不是 `Conflict`（改短評、刪除用）。
    #[allow(clippy::too_many_arguments)]
    fn write_notes<F, R>(
        &mut self,
        paper_id: &str,
        paper: &PaperFile,
        folder: &str,
        token: Option<&VersionToken>,
        stamp: &Stamp,
        missing_is_not_found: bool,
        op: F,
        record: R,
    ) -> Result<VersionToken>
    where
        F: FnOnce(&ParsedNotes, &NotesLabels) -> Result<String>,
        R: FnOnce(&mut Self, &ParsedNotes, &str) -> Result<()>,
    {
        let dir = self.notes_dir_or_invalid()?;
        let labels = self.notes_labels.clone();
        let existing = self.find_notes_file_mut(paper_id);
        let (name, text) = match (existing, token) {
            (Some(name), Some(t)) => {
                let (text, current) = read_notes_text(&dir.join(&name))?;
                if &current != t {
                    return Err(LibError::Conflict);
                }
                (name, text)
            }
            (Some(_), None) => return Err(LibError::Conflict),
            (None, Some(_)) if missing_is_not_found => {
                return Err(LibError::NotFound {
                    what: "notes file".to_string(),
                });
            }
            (None, Some(_)) => return Err(LibError::Conflict),
            (None, None) => {
                // 先試算要寫的內容，檢查都過了才建立資料夾與檔案
                let title = paper
                    .bib
                    .title
                    .as_deref()
                    .map(clean_title)
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| folder.to_string());
                let text = new_file(paper_id, &title, &labels, stamp);
                let parsed = parse(&text);
                let new_text = op(&parsed, &labels)?;
                // 先紀錄後檔案
                record(self, &parsed, &new_text)?;
                fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
                let name = pick_new_name(&dir, folder)?;
                let path = dir.join(&name);
                let t = write_bytes_atomic(&path, new_text.as_bytes())?;
                self.index_written(&name, paper_id, &path, &new_text);
                return Ok(t);
            }
        };
        let parsed = parse(&text);
        let new_text = op(&parsed, &labels)?;
        record(self, &parsed, &new_text)?;
        let path = dir.join(&name);
        let t = write_bytes_atomic(&path, new_text.as_bytes())?;
        self.index_written(&name, paper_id, &path, &new_text);
        Ok(t)
    }

    /// 寫完之後更新索引裡那個檔；「有沒有筆記」用剛寫進去的內容算（下一次掃描時修改時間與大小沒變，不會重讀）。
    fn index_written(&mut self, name: &str, paper_id: &str, path: &Path, text: &str) {
        let (mtime, size) = match fs::metadata(path) {
            Ok(m) => (m.modified().ok(), m.len()),
            Err(_) => (None, 0),
        };
        self.notes_index.files.insert(
            name.to_string(),
            NoteFile {
                mtime,
                size,
                paper: Some(paper_id.to_string()),
                has_notes: text_has_notes(text),
            },
        );
    }
}

/// 記憶體裡記的那份檔，屬性區的代號是不是還對得上。
fn head_matches(path: &Path, paper_id: &str) -> bool {
    read_head(path)
        .and_then(|h| read_paper_id_from_bytes(index_head(&h)))
        .as_deref()
        == Some(paper_id)
}

/// 讀筆記檔的全部內容與版本記號（依檔案的原始位元組算）。不是 UTF-8 回報 `Unreadable`。
pub(crate) fn read_notes_text(path: &Path) -> Result<(String, VersionToken)> {
    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    let token = VersionToken(sha256_hex(&bytes));
    Ok((notes_utf8(path, bytes)?, token))
}

/// 同 `read_notes_text`，但不算版本記號（給全文搜尋的索引讀檔，`read_for_index`）。
pub(crate) fn read_notes_text_untokened(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    notes_utf8(path, bytes)
}

fn notes_utf8(path: &Path, bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|e| LibError::Unreadable {
        path: path.display().to_string(),
        reason: format!("not valid UTF-8: {e}"),
    })
}

/// 新檔的檔名：文獻資料夾名稱加 `.md`；已有同名項目（不分大小寫）時加 ` (2)`、` (3)`，加在 `.md` 之前
/// （`notes_dir` 裡既有項目的 `.md` 先去掉再比）。
fn pick_new_name(dir: &Path, folder: &str) -> Result<String> {
    let mut existing = super::names_in(dir)?;
    for _ in 0..1000 {
        let name = format!("{}.md", unique_stem(&existing, ".md", folder));
        if fs::symlink_metadata(dir.join(&name)).is_err() {
            return Ok(name);
        }
        existing.push(name);
    }
    Err(LibError::Io {
        path: dir.display().to_string(),
        reason: "no free notes file name".to_string(),
    })
}

/// 座標取到小數兩位。
fn round_rect(r: &[f64; 4]) -> [f64; 4] {
    let f = |x: f64| (x * 100.0).round() / 100.0;
    [f(r[0]), f(r[1]), f(r[2]), f(r[3])]
}

fn bad_input(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

fn check_rect(r: &[f64; 4]) -> Result<()> {
    if r.iter().any(|x| !x.is_finite()) {
        return Err(bad_input("rect holds a non-finite number"));
    }
    if r[0] > r[2] || r[1] > r[3] {
        return Err(bad_input(
            "rect must satisfy left <= right and bottom <= top",
        ));
    }
    Ok(())
}

/// 畫面交來的劃線欄位的檢查（引句與短評的文字規則由呼叫的一方另外檢查）。
fn validate_draft(d: &HighlightDraft, material_pages: Option<u32>) -> Result<()> {
    if d.page_index >= MAX_PAGE_INDEX {
        return Err(bad_input(format!(
            "page index must be below {MAX_PAGE_INDEX}"
        )));
    }
    if let Some(n) = material_pages
        && d.page_index >= n
    {
        return Err(bad_input(format!(
            "page index {} is beyond the material's {n} pages",
            d.page_index
        )));
    }
    if d.rects.is_empty() {
        return Err(bad_input("rects must not be empty"));
    }
    for r in d.rects.iter().chain(d.next_page_rects.iter()) {
        check_rect(r)?;
    }
    if let Some(p) = &d.position {
        if p.start > p.end {
            return Err(bad_input("position.start must not exceed position.end"));
        }
        let n = p.basis.chars().count();
        if n == 0 || n > MAX_BASIS_CHARS {
            return Err(bad_input(format!(
                "position.basis must be 1 to {MAX_BASIS_CHARS} characters"
            )));
        }
    }
    if d.fingerprint
        .as_ref()
        .is_some_and(|f| f.chars().count() > MAX_FINGERPRINT_CHARS)
    {
        return Err(bad_input(format!(
            "fingerprint is longer than {MAX_FINGERPRINT_CHARS} characters"
        )));
    }
    Ok(())
}

/// 檢查文獻庫用：`notes_dir` 底下每份筆記檔的狀況。
pub(crate) struct NoteFileStatus {
    pub name: String,
    pub paper: Option<String>,
    /// 讀不了或不是 UTF-8。
    pub unreadable: bool,
    /// 有讀不懂或不完整的劃線、或不完整的想法。
    pub bad_blocks: bool,
}

/// 逐一讀 `notes_dir` 底下的筆記檔（依檔名排序）。只回報，不寫。
pub(crate) fn inspect_notes_dir(root: &Path, notes_dir: &str) -> Vec<NoteFileStatus> {
    let mut out = Vec::new();
    let Some(dir) = notes_dir_path(root, notes_dir) else {
        return out;
    };
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_note_name(n))
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let is_file = fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        match fs::read(&path) {
            Err(_) => out.push(NoteFileStatus {
                name,
                paper: None,
                unreadable: true,
                bad_blocks: false,
            }),
            Ok(bytes) => match String::from_utf8(bytes) {
                Err(e) => {
                    let paper = read_paper_id_from_bytes(index_head(e.as_bytes()));
                    out.push(NoteFileStatus {
                        name,
                        paper,
                        unreadable: true,
                        bad_blocks: false,
                    });
                }
                Ok(text) => {
                    let p = parse(&text);
                    let bad_blocks = !p.unreadable().is_empty()
                        || p.highlights().iter().any(|h| !h.complete)
                        || !p.thoughts_complete();
                    out.push(NoteFileStatus {
                        name,
                        paper: p.paper_id().map(String::from),
                        unreadable: false,
                        bad_blocks,
                    });
                }
            },
        }
    }
    out
}

/// 開啟時清掉 `notes_dir` 這一層以 `.pr-` 開頭的暫存項目（只在 `notes_dir` 合規則時）。
pub(crate) fn clean_notes_dir_temp(root: &Path, notes_dir: &str) {
    let Some(dir) = notes_dir_path(root, notes_dir) else {
        return;
    };
    let Ok(rd) = fs::read_dir(&dir) else {
        return;
    };
    for entry in rd.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
            continue;
        }
        let p = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let _ = if is_dir {
            fs::remove_dir_all(&p)
        } else {
            fs::remove_file(&p)
        };
    }
}
