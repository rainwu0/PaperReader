//! 參考文獻清單的讀寫與目錄摘要。檔案是 `papers/<資料夾>/references.json`，格式在
//! `crate::references`。操作是 `LocalLibrary` 的公開方法。

use super::text::file_stamp;
use super::{LocalLibrary, PAPERS_DIR, is_valid_material_path};
use crate::digest::is_digest;
use crate::error::{LibError, Result};
use crate::format::{FORMAT_VERSION, ROLE_SOURCE};
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, sha256_hex, write_json_atomic};
use crate::overlay::OVERLAY_FORM;
use crate::references::*;
use crate::store::{FileStamp, PaperDigest};
use crate::text::TEXT_FILE;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn bad(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 檔案已有、讀得懂、`format` 不較新：交回內容與記號；不存在交回 `NotFound`。
fn existing(path: &std::path::Path) -> Result<(ReferencesFile, VersionToken)> {
    let (doc, token) = read_references_file(path)?.ok_or_else(|| LibError::NotFound {
        what: "references file".to_string(),
    })?;
    if doc.format > FORMAT_VERSION {
        return Err(LibError::FormatTooNew { found: doc.format });
    }
    Ok((doc, token))
}

/// 抽出的部分相同（不比時間）。
fn same_extraction(a: &ReferencesFile, b: &ReferencesFile) -> bool {
    a.extraction.extractor == b.extraction.extractor
        && a.extraction.version == b.extraction.version
        && a.extraction.source_material == b.extraction.source_material
        && a.extraction.source_sha256 == b.extraction.source_sha256
        && a.extraction.extra == b.extraction.extra
        && a.entries == b.entries
}

impl LocalLibrary {
    fn references_path(&self, folder: &str) -> PathBuf {
        self.root
            .join(PAPERS_DIR)
            .join(folder)
            .join(REFERENCES_FILE)
    }

    /// 讀一篇的參考文獻清單；沒有檔是 `None`。不在目錄 `NotFound`；讀不懂 `Unreadable`；`format` 較新照樣交回，
    /// 呼叫的一方只讀。
    pub fn read_references(
        &self,
        paper_id: &str,
    ) -> Result<Option<(ReferencesFile, VersionToken)>> {
        let located = self.locate(paper_id)?;
        read_references_file(&self.references_path(&located.folder))
    }

    /// 寫入或取代抽出的部分。`judgments` 原樣保留；舊條目上不認得的欄位依找回鍵搬到新的條目；
    /// 和現有的抽出的部分相同（不比時間）就不寫，交回現有的記號。背景寫入：寫之前再讀一次，不同就從頭重做一次，
    /// 仍不同 `Conflict`。
    pub fn write_references(
        &mut self,
        paper_id: &str,
        draft: ReferencesDraft,
    ) -> Result<VersionToken> {
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
            let mut entries = draft_entries(&draft, pages)?;
            let path = self.references_path(&located.folder);
            let current = read_references_file(&path)?;
            if let Some((doc, _)) = &current
                && doc.format > FORMAT_VERSION
            {
                return Err(LibError::FormatTooNew { found: doc.format });
            }
            let now = now_utc();
            let mut doc = match &current {
                Some((old, old_token)) => {
                    carry_entry_extras(&mut entries, &old.entries);
                    let mut doc = old.clone();
                    doc.extraction.extractor = draft.extractor.clone();
                    doc.extraction.version = draft.version;
                    doc.extraction.source_material = draft.source_material.clone();
                    doc.extraction.source_sha256 = draft.source_sha256.clone();
                    doc.entries = entries;
                    if same_extraction(&doc, old) {
                        return Ok(old_token.clone());
                    }
                    doc
                }
                None => ReferencesFile {
                    format: FORMAT_VERSION,
                    modified: None,
                    modified_by: None,
                    extraction: Extraction {
                        extractor: draft.extractor.clone(),
                        version: draft.version,
                        source_material: draft.source_material.clone(),
                        source_sha256: draft.source_sha256.clone(),
                        created: String::new(),
                        created_by: String::new(),
                        extra: Default::default(),
                    },
                    entries,
                    judgments: Vec::new(),
                    extra: Default::default(),
                },
            };
            doc.extraction.created = now.clone();
            doc.extraction.created_by = self.device_id.clone();
            let size = extracted_bytes(&doc.extraction, &doc.entries);
            if size > MAX_EXTRACTED_BYTES {
                return Err(bad(format!(
                    "extracted part is {size} bytes, more than {MAX_EXTRACTED_BYTES} bytes"
                )));
            }
            doc.modified = Some(now);
            doc.modified_by = Some(self.device_id.clone());
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

    /// 加一項判斷：核心從那一條算找回鍵、抄原文、DOI、arXiv 編號。這一條、這個對象現在有效的那一項
    /// 已經是同一種時不寫，交回那一項與原本的記號。
    pub fn add_link_judgment(
        &mut self,
        paper_id: &str,
        index: usize,
        kind: JudgmentKind,
        target: &str,
        token: &VersionToken,
    ) -> Result<(Judgment, VersionToken)> {
        self.require_writable()?;
        let located = self.locate_mut(paper_id)?;
        let path = self.references_path(&located.folder);
        let (mut doc, current) = existing(&path)?;
        if &current != token {
            return Err(LibError::Conflict);
        }
        let Some(entry) = doc.entries.get(index) else {
            return Err(bad(format!(
                "entry index {index} is beyond the {} entries",
                doc.entries.len()
            )));
        };
        if target == paper_id {
            return Err(bad("a paper cannot be linked to itself"));
        }
        if !self.catalog().iter().any(|(_, p)| p.id == target) {
            return Err(bad(format!("target paper {target} is not in the catalog")));
        }
        // 已經是同一種：不寫
        let res = resolve_judgments(&doc.entries, &doc.judgments);
        if let Some(&k) = res.entries[index]
            .by_paper
            .iter()
            .find(|&&k| doc.judgments[k].paper == target)
            && doc.judgments[k].kind == kind.as_str()
        {
            return Ok((doc.judgments[k].clone(), current));
        }
        if doc.judgments.len() >= MAX_JUDGMENTS {
            return Err(bad(format!(
                "at most {MAX_JUDGMENTS} judgments; clear some lost ones first"
            )));
        }
        let now = now_utc();
        let j = Judgment {
            id: format!("j-{}", new_id().to_lowercase()),
            kind: kind.as_str().to_string(),
            paper: target.to_string(),
            entry_key: entry_key(&entry.text, ENTRY_KEY_VERSION).unwrap_or_default(),
            key_version: ENTRY_KEY_VERSION,
            entry_text: entry.text.clone(),
            entry_doi: entry.doi.clone(),
            entry_arxiv: entry.arxiv.clone(),
            created: now.clone(),
            created_by: self.device_id.clone(),
            extra: Default::default(),
        };
        doc.judgments.push(j.clone());
        let size = judgment_bytes(&doc.judgments);
        if size > MAX_JUDGMENT_BYTES {
            return Err(bad(format!(
                "judgments are {size} bytes, more than {MAX_JUDGMENT_BYTES} bytes; clear some lost ones first"
            )));
        }
        doc.modified = Some(now);
        doc.modified_by = Some(self.device_id.clone());
        let t = write_json_atomic(&path, &doc)?;
        Ok((j, t))
    }

    /// 刪判斷（撤銷、清掉找不回的）。順序：檢查都過 → 變更紀錄檔附加 `link-judgment-deleted` → 寫檔。
    pub fn remove_link_judgments(
        &mut self,
        paper_id: &str,
        ids: &[String],
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let located = self.locate_mut(paper_id)?;
        let path = self.references_path(&located.folder);
        let (mut doc, current) = existing(&path)?;
        if &current != token {
            return Err(LibError::Conflict);
        }
        if ids.is_empty() {
            return Err(LibError::NotFound {
                what: "no judgment given".to_string(),
            });
        }
        if let Some(missing) = ids
            .iter()
            .find(|id| !doc.judgments.iter().any(|j| &j.id == *id))
        {
            return Err(LibError::NotFound {
                what: format!("judgment {missing}"),
            });
        }
        let rec = self.new_change(
            Some(paper_id),
            crate::changes::LINK_JUDGMENT_DELETED,
            serde_json::json!({ "judgments": ids }),
        );
        self.append_change(&rec)?;
        doc.judgments.retain(|j| !ids.contains(&j.id));
        doc.modified = Some(now_utc());
        doc.modified_by = Some(self.device_id.clone());
        write_json_atomic(&path, &doc)
    }

    /// 目錄摘要：目錄裡每一篇的代號、書目的指紋（記憶體裡的書目，不讀檔）、
    /// `references.json` 的大小與修改時間，以及全文搜尋的來源檔（`files`）的大小與修改時間：`text.json`、筆記檔、
    /// 每份疊加檔、每份重點筆記。只讀檔案屬性，不讀內容；沒有的檔不列。筆記檔照記憶體裡的筆記索引找。
    pub fn paper_digests(&self) -> Vec<PaperDigest> {
        let notes_dir = super::notes::notes_dir_path(&self.root, &self.library.notes_dir);
        self.catalog()
            .into_iter()
            .map(|(folder, paper)| {
                let dir = self.root.join(PAPERS_DIR).join(&folder.name);
                let mut files = BTreeMap::new();
                if let Some(s) = file_stamp(&dir.join(TEXT_FILE)) {
                    files.insert("text".to_string(), s);
                }
                if let Some(nd) = &notes_dir
                    && let Some(name) = self.notes_index.file_for(&paper.id)
                    && let Some(s) = file_stamp(&nd.join(name))
                {
                    files.insert("notes".to_string(), s);
                }
                for m in &paper.materials {
                    if !is_valid_material_path(&m.path) {
                        continue;
                    }
                    let key = if m.form == OVERLAY_FORM {
                        format!("overlay:{}", m.id)
                    } else if is_digest(m) {
                        format!("digest:{}", m.id)
                    } else {
                        continue;
                    };
                    if let Some(s) = file_stamp(&dir.join(&m.path)) {
                        files.insert(key, s);
                    }
                }
                let bib = serde_json::to_vec(&paper.bib).unwrap_or_default();
                let references = std::fs::metadata(self.references_path(&folder.name))
                    .ok()
                    .map(|m| FileStamp {
                        bytes: m.len(),
                        modified_ms: m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_millis() as i64),
                    });
                PaperDigest {
                    id: paper.id.clone(),
                    bib_sha256: sha256_hex(&bib),
                    references,
                    files,
                }
            })
            .collect()
    }
}
