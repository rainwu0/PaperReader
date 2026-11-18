//! 抽出的文字的讀寫與給全文搜尋的索引讀檔的取法。檔案是 `papers/<資料夾>/text.json`，格式在
//! `crate::text`。操作是 `LocalLibrary` 的公開方法。

use super::{LocalLibrary, PAPERS_DIR, is_valid_material_path};
use crate::digest::is_digest;
use crate::error::{LibError, Result};
use crate::format::{FORMAT_VERSION, Map, ROLE_SOURCE};
use crate::ids::now_utc;
use crate::jsonio::{VersionToken, read_json_untokened, sha256_hex, write_json_atomic};
use crate::overlay::{OVERLAY_FORM, OverlayFile};
use crate::references::Extraction;
use crate::store::{FileStamp, IndexRead, IndexSource};
use crate::text::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 一個一般檔案的大小與修改時間（只讀檔案屬性）；不存在、不是一般檔案是 `None`。
pub(crate) fn file_stamp(path: &Path) -> Option<FileStamp> {
    let m = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some(FileStamp {
        bytes: m.len(),
        modified_ms: m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64),
    })
}

/// 內容相同（不比 `extraction` 的時間與裝置）。
fn same_content(a: &TextFile, b: &TextFile) -> bool {
    a.format == b.format
        && a.extraction.extractor == b.extraction.extractor
        && a.extraction.version == b.extraction.version
        && a.extraction.source_material == b.extraction.source_material
        && a.extraction.source_sha256 == b.extraction.source_sha256
        && a.extraction.extra == b.extraction.extra
        && a.truncated == b.truncated
        && a.extra == b.extra
        && a.pages == b.pages
}

/// 索引要讀的一種來源（`read_for_index` 的鍵）。
enum SourceKey<'a> {
    Text,
    Notes,
    Overlay(&'a str),
    Digest(&'a str),
}

fn parse_key(key: &str) -> Result<SourceKey<'_>> {
    match key {
        "text" => return Ok(SourceKey::Text),
        "notes" => return Ok(SourceKey::Notes),
        _ => {}
    }
    match key.split_once(':') {
        Some(("overlay", m)) if !m.is_empty() => Ok(SourceKey::Overlay(m)),
        Some(("digest", m)) if !m.is_empty() => Ok(SourceKey::Digest(m)),
        _ => Err(bad(format!("unknown source key: {key:?}"))),
    }
}

impl LocalLibrary {
    pub(crate) fn text_path(&self, folder: &str) -> PathBuf {
        self.root.join(PAPERS_DIR).join(folder).join(TEXT_FILE)
    }

    /// 讀一篇的抽出的文字；沒有檔是 `None`。不在目錄 `NotFound`；讀不懂 `Unreadable`；`format` 較新照樣交回，
    /// 呼叫的一方只讀。
    pub fn read_text(&self, paper_id: &str) -> Result<Option<(TextFile, VersionToken)>> {
        let located = self.locate(paper_id)?;
        read_text_file(&self.text_path(&located.folder))
    }

    /// 寫入或整份取代抽出的文字。不認得的欄位照 3.1 的表：頂層與 `extraction` 的留著；同一份原文時
    /// 舊的每頁上的依頁序搬到新的那一頁，原文換了丟掉。內容與現有的相同（不比時間）不寫；現有的是同一個抽法、
    /// 同一份原文、版本較高時也不寫（舊版應用不蓋新版抽的），都交回現有的記號。背景寫入：寫之前再讀一次，
    /// 不同就從頭重做一次，仍不同 `Conflict`。
    pub fn write_text(&mut self, paper_id: &str, draft: TextDraft) -> Result<VersionToken> {
        self.require_writable()?;
        for _ in 0..2 {
            let located = self.locate_mut(paper_id)?;
            if located.paper.format > FORMAT_VERSION {
                return Err(LibError::FormatTooNew {
                    found: located.paper.format,
                });
            }
            let source = located
                .paper
                .materials
                .iter()
                .find(|m| m.id == draft.source_material && m.role == ROLE_SOURCE)
                .ok_or_else(|| LibError::NotFound {
                    what: format!("source material {}", draft.source_material),
                })?;
            if source.sha256.as_deref() != Some(draft.source_sha256.as_str()) {
                return Err(LibError::Conflict);
            }
            let Some(pages) = source.pages else {
                return Err(bad("source pages unknown"));
            };
            check_draft(&draft, pages)?;
            let path = self.text_path(&located.folder);
            let current = read_text_file(&path)?;
            if let Some((doc, _)) = &current
                && doc.format > FORMAT_VERSION
            {
                return Err(LibError::FormatTooNew { found: doc.format });
            }
            let mut new_pages: Vec<TextPage> = draft
                .pages
                .iter()
                .enumerate()
                .map(|(i, t)| TextPage {
                    page: i as u32,
                    text: t.clone(),
                    extra: Map::new(),
                })
                .collect();
            let mut doc = match &current {
                Some((old, old_token)) => {
                    let same_source = old.extraction.source_sha256 == draft.source_sha256;
                    if same_source
                        && old.extraction.extractor == draft.extractor
                        && old.extraction.version > draft.version
                    {
                        return Ok(old_token.clone());
                    }
                    if same_source {
                        carry_page_extras(&mut new_pages, &old.pages);
                    }
                    let mut doc = old.clone();
                    doc.extraction.extractor = draft.extractor.clone();
                    doc.extraction.version = draft.version;
                    doc.extraction.source_material = draft.source_material.clone();
                    doc.extraction.source_sha256 = draft.source_sha256.clone();
                    doc.pages = new_pages;
                    doc.truncated = draft.truncated;
                    if same_content(&doc, old) {
                        return Ok(old_token.clone());
                    }
                    doc
                }
                None => TextFile {
                    format: FORMAT_VERSION,
                    extraction: Extraction {
                        extractor: draft.extractor.clone(),
                        version: draft.version,
                        source_material: draft.source_material.clone(),
                        source_sha256: draft.source_sha256.clone(),
                        created: String::new(),
                        created_by: String::new(),
                        extra: Map::new(),
                    },
                    pages: new_pages,
                    truncated: draft.truncated,
                    extra: Map::new(),
                },
            };
            doc.extraction.created = now_utc();
            doc.extraction.created_by = self.device_id.clone();
            let size = written_bytes(&doc);
            if size > MAX_WRITE_BYTES {
                return Err(bad(format!(
                    "text.json would be {size} bytes, more than {MAX_WRITE_BYTES} bytes"
                )));
            }
            // 寫之前再讀一次：和讀到的不同（或原本沒有、現在有了），從頭重做
            self.before_background_recheck(&path);
            let now_token = match std::fs::read(&path) {
                Ok(b) => Some(sha256_hex(&b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => continue,
            };
            if now_token != current.as_ref().map(|(_, t)| t.0.clone()) {
                continue;
            }
            return write_json_atomic(&path, &doc);
        }
        Err(LibError::Conflict)
    }

    /// 每篇的檔頭（逐篇處理判斷做過沒有）。只解析 `extraction`，`pages` 不建。不在目錄、沒有檔、
    /// 讀不懂是 `None`；`format` 較新照樣交回檔頭。用記憶體裡的目錄找資料夾，不讀 `paper.json`。
    pub fn text_states(&self, paper_ids: &[String]) -> Vec<(String, Option<TextState>)> {
        let folders: HashMap<&str, &str> = self
            .catalog()
            .into_iter()
            .map(|(f, p)| (p.id.as_str(), f.name.as_str()))
            .collect();
        paper_ids
            .iter()
            .map(|id| {
                let st = folders
                    .get(id.as_str())
                    .and_then(|f| read_text_state(&self.text_path(f)));
                (id.clone(), st)
            })
            .collect()
    }

    /// 給全文搜尋的索引讀一個來源檔。鍵和 `paper_digests` 的 `files` 相同。檔案屬性等於 `known`
    /// 交回 `Unchanged`，不讀內容；沒有檔、讀不懂、`format` 較新交回 `Missing`；否則交回讀之前取的檔案屬性與內容。
    /// 不算版本記號（任何一個檔的 SHA-256 都不算，`paper.json` 也不讀）：資料夾與材料清單照記憶體裡的目錄，和
    /// `paper_digests` 同一份。文獻不在目錄 `NotFound`；鍵不認得 `InvalidInput`。
    pub fn read_for_index(
        &self,
        paper_id: &str,
        key: &str,
        known: Option<FileStamp>,
    ) -> Result<IndexRead> {
        let kind = parse_key(key)?;
        let (folder, paper) = self
            .catalog()
            .into_iter()
            .find(|(_, p)| p.id == paper_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("paper {paper_id}"),
            })?;
        let dir = self.papers_dir().join(&folder.name);
        let material_path = |want: &dyn Fn(&crate::format::Material) -> bool, id: &str| {
            paper
                .materials
                .iter()
                .find(|m| m.id == id && want(m) && is_valid_material_path(&m.path))
                .map(|m| dir.join(&m.path))
        };
        let (path, notes_name) = match &kind {
            SourceKey::Text => (Some(self.text_path(&folder.name)), None),
            SourceKey::Notes => match (
                super::notes::notes_dir_path(&self.root, &self.library.notes_dir),
                self.find_notes_file(paper_id),
            ) {
                (Some(nd), Some(name)) => (Some(nd.join(&name)), Some(name)),
                _ => (None, None),
            },
            SourceKey::Overlay(m) => (material_path(&|x| x.form == OVERLAY_FORM, m), None),
            SourceKey::Digest(m) => (material_path(&is_digest, m), None),
        };
        let Some(path) = path else {
            return Ok(IndexRead::Missing);
        };
        let Some(stamp) = file_stamp(&path) else {
            return Ok(IndexRead::Missing);
        };
        if known == Some(stamp) {
            return Ok(IndexRead::Unchanged);
        }
        let source = match kind {
            SourceKey::Text => match read_text_file_untokened(&path) {
                Ok(Some(d)) if d.format <= FORMAT_VERSION => IndexSource::Text(d),
                _ => return Ok(IndexRead::Missing),
            },
            SourceKey::Overlay(_) => match read_json_untokened::<OverlayFile>(&path) {
                Ok(o) if o.format <= FORMAT_VERSION => IndexSource::Overlay(o),
                _ => return Ok(IndexRead::Missing),
            },
            SourceKey::Digest(_) => {
                let parsed = std::fs::read(&path)
                    .ok()
                    .and_then(|b| super::digest::parse_bytes(&path, &b).ok());
                match parsed.map(|p| p.into_doc()) {
                    Some(d) if d.format <= FORMAT_VERSION => IndexSource::Digest(d),
                    _ => return Ok(IndexRead::Missing),
                }
            }
            SourceKey::Notes => {
                let name = notes_name.unwrap_or_default();
                match super::notes::read_notes_text_untokened(&path) {
                    Ok(text) => IndexSource::Notes(self.notes_doc(&name, &text)),
                    Err(_) => return Ok(IndexRead::Missing),
                }
            }
        };
        Ok(IndexRead::Read(stamp, Box::new(source)))
    }
}
