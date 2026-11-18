//! 整批匯入的比對與寫入：預覽與寫入的計畫留在記憶體裡，用預覽代號交接（同同步包）。
//!
//! 一次寫一篇：外殼每呼叫一次 `import_step` 持一次鎖，呼叫之間放開。每篇照「加入一篇文獻」的步驟寫：
//! `.pr-` 開頭的暫存資料夾 → 逐檔再檢查、複製、算指紋、驗形式 → `paper.json` → 改成正式名稱。

use super::{LocalLibrary, PAPER_FILE};
use crate::digest::PAGE_MAP_MARKED;
use crate::error::{LibError, Result, io_error};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::import::{
    FORM_MARKDOWN, Halt, ImportContainer, ImportItem, ImportNotice, ImportPreview, ImportReport,
    ImportScan, ImportSkip, ImportStep, Opened, ROLE_DIGEST, ReadRules, ScannedPaper, content_ok,
    digest_proofread, notice, reason, stream,
};
use crate::jsonio::{sha256_hex, write_json_atomic};
use crate::naming::{folder_base_name, last_segment, numbered_names};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

/// 測試掛勾：複製某一篇（序號）的某個檔（在清單 `files` 裡的位置）之前呼叫；交回錯誤就當成寫那個檔時發生的錯誤。
#[doc(hidden)]
pub type ImportCopyHookFn = Box<dyn FnMut(u32, usize) -> Option<LibError> + Send>;

/// `ImportCopyHookFn` 的外框（同 `CopyHook`）。
#[derive(Default)]
pub(crate) struct ImportCopyHook(pub(crate) Option<ImportCopyHookFn>);

impl std::fmt::Debug for ImportCopyHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "ImportCopyHook(set)"
        } else {
            "ImportCopyHook(none)"
        })
    }
}

/// 記憶體裡的一份匯入計畫（同一時間只有一份，新的預覽取代舊的）。
#[derive(Debug)]
pub(crate) struct PendingImport {
    preview_id: String,
    container: ImportContainer,
    manifest_sha: String,
    /// 預覽時會收的篇，依序號。
    queue: Vec<ScannedPaper>,
    /// 下一篇在 `queue` 裡的位置。
    next: usize,
    /// 第一次寫入時重讀清單、確認沒變之後打開的容器，之後每一篇沿用。
    opened: Option<Opened>,
    report: ImportReport,
}

/// 寫一篇的結果（錯誤另外交回）。
enum Written {
    Added(String),
    Skipped(&'static str, Option<String>),
}

impl LocalLibrary {
    /// 只給測試用：設定複製匯入的檔之前呼叫的掛勾（`None` 清掉；不會自動清掉）。
    #[doc(hidden)]
    pub fn set_import_copy_hook(&mut self, hook: Option<ImportCopyHookFn>) {
        self.import_copy_hook = ImportCopyHook(hook);
    }

    /// 比對成預覽：外殼持鎖呼叫，時間短。原文指紋在目錄 → `already-present`；在回收區 →
    /// `in-trash`；和前面一篇會收的篇相同 → `duplicate-source`（被略過的篇不占用原文）。DOI 和目錄裡某篇相同
    /// 只記提醒 `same-doi`。計畫存在記憶體裡（取代舊的），交回預覽。除此之外什麼都不寫。
    pub fn prepare_import(&mut self, scan: ImportScan) -> Result<ImportPreview> {
        self.require_writable()?;
        let mut in_catalog: HashMap<String, String> = HashMap::new();
        for (_, p) in self.catalog() {
            for m in p.materials.iter().filter(|m| m.role == ROLE_SOURCE) {
                if let Some(sha) = &m.sha256 {
                    in_catalog
                        .entry(sha.clone())
                        .or_insert_with(|| p.id.clone());
                }
            }
        }
        let in_trash = self.trash_source_index();

        let mut skip = scan.skipped;
        let mut accept = Vec::new();
        let mut queue = Vec::new();
        let mut claimed: HashMap<String, u32> = HashMap::new();
        let mut bytes = 0u64;
        for mut p in scan.accepted {
            let sha = p.source_sha.clone();
            if let Some(id) = in_catalog.get(&sha) {
                skip.push(ImportSkip::new(
                    p.index,
                    reason::ALREADY_PRESENT,
                    Some(id.clone()),
                ));
            } else if let Some(id) = in_trash.get(&sha) {
                skip.push(ImportSkip::new(p.index, reason::IN_TRASH, Some(id.clone())));
            } else if let Some(first) = claimed.get(&sha) {
                skip.push(ImportSkip::new(
                    p.index,
                    reason::DUPLICATE_SOURCE,
                    Some(format!("papers[{first}]")),
                ));
            } else {
                claimed.insert(sha, p.index);
                let same: Vec<ImportNotice> = p
                    .bib
                    .doi
                    .as_deref()
                    .map(|d| self.find_by_doi(d))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|id| ImportNotice {
                        kind: notice::SAME_DOI.to_string(),
                        detail: Some(id),
                    })
                    .collect();
                p.notices.splice(0..0, same);
                bytes += p.files.iter().map(|f| f.bytes).sum::<u64>();
                accept.push(ImportItem {
                    index: p.index,
                    title: p.bib.title.clone(),
                    files: p.files.len() as u32,
                    notices: p.notices.clone(),
                });
                queue.push(p);
            }
        }
        skip.sort_by_key(|s| s.index);

        let preview_id = new_id();
        self.pending_import = Some(PendingImport {
            preview_id: preview_id.clone(),
            container: scan.container,
            manifest_sha: scan.manifest_sha,
            queue,
            next: 0,
            opened: None,
            report: ImportReport {
                skipped: skip.clone(),
                ..Default::default()
            },
        });
        Ok(ImportPreview {
            preview_id,
            total: scan.total,
            accept,
            skip,
            bytes,
        })
    }

    /// 寫下一篇會收的。預覽代號不是目前那一份 → `InvalidInput`（`unknown preview`）。第一次呼叫時
    /// 重讀清單，和掃描時不同（或讀不到）→ `Conflict`，丟掉計畫、什麼都不寫。寫入時才發現的問題記成略過；
    /// I/O 錯誤記成 `failed`，接著做下一篇；空間不足時這篇與剩下的記成「沒做」，交回 `Finished`。
    pub fn import_step(&mut self, preview_id: &str) -> Result<ImportStep> {
        self.pending_for(preview_id)?;
        self.require_writable()?;
        let mut pending = self.pending_import.take().expect("checked above");
        if pending.opened.is_none() {
            match Opened::open(&pending.container, false) {
                Ok((opened, bytes)) if sha256_hex(&bytes) == pending.manifest_sha => {
                    pending.opened = Some(opened);
                }
                _ => return Err(LibError::Conflict),
            }
        }
        let total = pending.queue.len() as u32;
        if pending.next >= pending.queue.len() {
            return Ok(ImportStep::Finished(finish(pending)));
        }
        let k = pending.next;
        pending.next += 1;
        let paper = &pending.queue[k];
        let opened = pending.opened.as_mut().expect("opened above");
        match self.import_one(opened, paper) {
            Ok(Written::Added(id)) => {
                if !paper.notices.is_empty() {
                    pending.report.notices += 1;
                }
                pending.report.added.push((paper.index, id));
            }
            Ok(Written::Skipped(code, detail)) => {
                pending
                    .report
                    .skipped
                    .push(ImportSkip::new(paper.index, code, detail));
            }
            Err(LibError::NoSpace) => {
                pending.next = k;
                return Ok(ImportStep::Finished(finish(pending)));
            }
            Err(e) => {
                let detail = format!("{}: {e}", e.kind_name());
                pending.report.failed.push(ImportSkip::new(
                    paper.index,
                    reason::FAILED,
                    Some(detail),
                ));
            }
        }
        if pending.next >= pending.queue.len() {
            return Ok(ImportStep::Finished(finish(pending)));
        }
        let done = pending.next as u32;
        self.pending_import = Some(pending);
        Ok(ImportStep::Progress { done, total })
    }

    /// 使用者取消：已收的保留，剩下的記成「沒做」，交回報告並丟掉計畫。
    pub fn finish_import(&mut self, preview_id: &str) -> Result<ImportReport> {
        self.pending_for(preview_id)?;
        let pending = self.pending_import.take().expect("checked above");
        Ok(finish(pending))
    }

    fn pending_for(&self, preview_id: &str) -> Result<()> {
        match &self.pending_import {
            Some(p) if p.preview_id == preview_id => Ok(()),
            _ => Err(LibError::InvalidInput {
                reason: format!("unknown preview: {preview_id}"),
            }),
        }
    }

    /// 寫一篇：原文再和目錄、回收區比一次；建暫存資料夾、逐檔寫入、寫 `paper.json`、改成正式名稱。
    /// 沒寫成（略過或錯誤）時刪掉暫存資料夾。
    fn import_one(&mut self, opened: &mut Opened, paper: &ScannedPaper) -> Result<Written> {
        if let Some(id) = self.find_by_source_sha(&paper.source_sha) {
            return Ok(Written::Skipped(reason::ALREADY_PRESENT, Some(id)));
        }
        if let Some(id) = self.find_in_trash_by_source_sha(&paper.source_sha) {
            return Ok(Written::Skipped(reason::IN_TRASH, Some(id)));
        }
        let papers = self.papers_dir();
        fs::create_dir_all(&papers).map_err(|e| io_error(&papers, e))?;
        let id = new_id();
        let staging = papers.join(format!(".pr-importing-{id}"));
        fs::create_dir(&staging).map_err(|e| io_error(&staging, e))?;
        let r = self.import_into(&staging, opened, paper, id);
        if !matches!(r, Ok(Written::Added(_))) {
            let _ = fs::remove_dir_all(&staging);
        }
        r
    }

    fn import_into(
        &mut self,
        staging: &Path,
        opened: &mut Opened,
        paper: &ScannedPaper,
        id: String,
    ) -> Result<Written> {
        let now = now_utc();
        let mut materials = Vec::with_capacity(paper.files.len());
        let mut used: HashMap<(String, &str), usize> = HashMap::new();
        for (k, f) in paper.files.iter().enumerate() {
            let skipped = |code: &'static str| Ok(Written::Skipped(code, Some(f.path.clone())));
            // `<role>.<副檔名>`，同角色同副檔名的第二份起加 `-2`、`-3`……
            let ext = if f.form == FORM_MARKDOWN { "md" } else { "pdf" };
            let n = used.entry((f.role.clone(), ext)).or_default();
            let dot = format!(".{ext}");
            let name = numbered_names(&f.role, &dot)
                .nth(*n)
                .expect("an endless sequence");
            *n += 1;

            if let Some(hook) = self.import_copy_hook.0.as_mut()
                && let Some(e) = hook(paper.index, k)
            {
                return Err(e);
            }
            // 再檢查一次（掃描之後可能被換掉）
            let found = match opened.resolve(&f.path) {
                Ok(r) => r,
                Err(code) => return skipped(code),
            };
            let rules = ReadRules::new(opened, &found, &f.role, &f.form);
            let mut input = match opened.reader(&found) {
                Ok(r) => r,
                Err(code) => return skipped(code),
            };
            let dest = staging.join(&name);
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
                .map_err(|e| io_error(&dest, e))?;
            let seen = match stream(
                &mut input,
                &rules,
                &mut |chunk| out.write_all(chunk).map_err(|e| io_error(&dest, e)),
                &mut |_| true,
            ) {
                Ok(s) => s,
                Err(Halt::Skip(code)) => return skipped(code),
                Err(Halt::Write(e)) => return Err(e),
                Err(Halt::Cancel) => return skipped(reason::MISSING_FILE),
            };
            drop(input);
            out.sync_all().map_err(|e| io_error(&dest, e))?;
            if !content_ok(&f.role, &f.form, &seen) {
                return skipped(reason::FORM_MISMATCH);
            }
            let sha = seen.sha.clone().unwrap_or_default();
            if (f.role == ROLE_SOURCE && sha != paper.source_sha)
                || f.sha256.as_ref().is_some_and(|d| d != &sha)
            {
                return skipped(reason::CHECKSUM_MISMATCH);
            }
            // 重點筆記可以在筆記工具裡改，指紋與大小不記；清單寫的指紋上面照樣比過
            // 重點筆記：已校對取檔案屬性區的值（清單寫的不同以檔案為準）、頁碼寫在文字裡；
            // 其他角色沒有屬性區，照清單
            let (sha256, bytes, proofread, page_map) = if f.role == ROLE_DIGEST {
                (
                    None,
                    None,
                    Some(digest_proofread(&seen)),
                    Some(PAGE_MAP_MARKED.to_string()),
                )
            } else {
                (Some(sha), Some(seen.bytes), f.proofread, f.page_map.clone())
            };
            materials.push(Material {
                id: new_id(),
                role: f.role.clone(),
                form: f.form.clone(),
                path: name,
                sha256,
                bytes,
                pages: None,
                pages_modified: None,
                pages_modified_by: None,
                title: f.title.clone(),
                provenance: Provenance {
                    method: Some(METHOD_IMPORTED.to_string()),
                    time: Some(now.clone()),
                    origin_name: Some(last_segment(&f.path).to_string()),
                    proofread,
                    page_map,
                    source_sha256: None,
                    extra: Map::new(),
                },
                extra: Map::new(),
            });
        }

        let b = &paper.bib;
        let file = PaperFile {
            format: FORMAT_VERSION,
            id: id.clone(),
            short_name: paper.short_name.clone(),
            aliases: paper.aliases.clone(),
            added: Some(now.clone()),
            bib: Bib {
                title: b.title.clone(),
                authors: b.authors.clone(),
                year: b.year,
                venue: b.venue.clone(),
                doi: b.doi.clone(),
                url: b.url.clone(),
                cleared: Vec::new(),
                modified: Some(now),
                modified_by: Some(self.device_id.clone()),
                extra: Map::new(),
            },
            materials,
            groups: Vec::new(),
            groups_modified: None,
            groups_modified_by: None,
            extra: Map::new(),
        };
        write_json_atomic(&staging.join(PAPER_FILE), &file)?;
        let base = folder_base_name(b, Some(last_segment(paper.source_path())));
        self.place_new_folder(staging, &base, file)?;
        Ok(Written::Added(id))
    }
}

/// 收尾：剩下沒做的記成「沒做」，略過的依序號排。
fn finish(mut pending: PendingImport) -> ImportReport {
    let rest = pending.queue[pending.next.min(pending.queue.len())..]
        .iter()
        .map(|p| p.index);
    pending.report.not_done.extend(rest);
    pending.report.skipped.sort_by_key(|s| s.index);
    pending.report
}
