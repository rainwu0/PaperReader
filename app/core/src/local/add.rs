//! 加入一篇文獻。

use super::{Folder, LocalLibrary, PAPER_FILE, TRASH_DIR, names_in};
use crate::error::{LibError, Result, io_error};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::jsonio::{hash_reader, read_json, sync_dir, write_json_atomic};
use crate::naming::{folder_base_name, unique_name};
use crate::pdf::is_pdf_file;
use crate::store::AddOutcome;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

const RENAME_TRIES: usize = 100;

impl LocalLibrary {
    /// 加入一個 PDF。步驟：暫存資料夾 → 邊複製邊算指紋 → 比對指紋 → 寫 `paper.json` → 改成正式名稱。
    /// 任何一步失敗都刪掉暫存資料夾再交回錯誤。
    pub fn add_paper(
        &mut self,
        src: &Path,
        draft: BibDraft,
        pages: Option<u32>,
    ) -> Result<AddOutcome> {
        self.require_writable()?;

        let meta = fs::metadata(src).map_err(|e| io_error(src, e))?;
        if !meta.is_file() {
            return Err(LibError::InvalidInput {
                reason: format!("not a file: {}", src.display()),
            });
        }
        if meta.len() == 0 {
            return Err(LibError::InvalidInput {
                reason: format!("empty file: {}", src.display()),
            });
        }
        let origin_name = src.file_name().map(|n| n.to_string_lossy().into_owned());
        let mut file = fs::File::open(src).map_err(|e| io_error(src, e))?;
        self.add_paper_from_reader(&mut file, origin_name.as_deref(), draft, pages)
    }

    /// 加入一段資料流（別的應用分享進來的檔案在 Android 上不一定有路徑）。
    /// 步驟與 `add_paper` 相同；內容長度為 0 交回 `InvalidInput`；讀取或寫入途中出錯，
    /// 暫存資料夾整個刪掉再交回錯誤。
    pub fn add_paper_from_reader(
        &mut self,
        reader: &mut dyn Read,
        origin_name: Option<&str>,
        draft: BibDraft,
        pages: Option<u32>,
    ) -> Result<AddOutcome> {
        self.require_writable()?;

        let papers = self.papers_dir();
        fs::create_dir_all(&papers).map_err(|e| io_error(&papers, e))?;
        let id = new_id();
        let staging = papers.join(format!(".pr-adding-{id}"));
        fs::create_dir(&staging).map_err(|e| io_error(&staging, e))?;

        match self.add_into(
            &staging,
            reader,
            draft,
            pages,
            id,
            origin_name.map(String::from),
        ) {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                let _ = fs::remove_dir_all(&staging);
                Err(e)
            }
        }
    }

    fn add_into(
        &mut self,
        staging: &Path,
        reader: &mut dyn Read,
        draft: BibDraft,
        pages: Option<u32>,
        id: String,
        origin_name: Option<String>,
    ) -> Result<AddOutcome> {
        // 2. 複製成 source.pdf，邊複製邊算指紋。
        let dest = staging.join("source.pdf");
        let (sha, bytes) = copy_with_hash(reader, &dest)?;
        if bytes == 0 {
            return Err(LibError::InvalidInput {
                reason: "empty content".to_string(),
            });
        }
        // 核心自己驗檔頭：不是 PDF 就不加入
        if !is_pdf_file(&dest)? {
            return Err(LibError::InvalidInput {
                reason: "not a PDF: no %PDF- marker in the first 1024 bytes".to_string(),
            });
        }

        // 3. 指紋與目錄裡或 .trash/ 裡某篇的原文相同：刪掉暫存資料夾，交回對應的結果。
        if let Some(existing) = self.find_by_source_sha(&sha) {
            fs::remove_dir_all(staging).map_err(|e| io_error(staging, e))?;
            return Ok(AddOutcome::AlreadyPresent(existing));
        }
        if let Some(existing) = self.find_in_trash_by_source_sha(&sha) {
            fs::remove_dir_all(staging).map_err(|e| io_error(staging, e))?;
            return Ok(AddOutcome::InTrash(existing));
        }

        // 4. 寫入 paper.json。
        let now = now_utc();
        let base = folder_base_name(&draft, origin_name.as_deref());
        let paper = PaperFile {
            format: FORMAT_VERSION,
            id: id.clone(),
            short_name: None,
            aliases: vec![],
            added: Some(now.clone()),
            bib: Bib {
                title: draft.title,
                authors: draft.authors,
                year: draft.year,
                venue: draft.venue,
                doi: draft
                    .doi
                    .as_deref()
                    .map(normalize_doi)
                    .filter(|d| !d.is_empty()),
                url: draft.url,
                cleared: Vec::new(),
                modified: Some(now.clone()),
                modified_by: Some(self.device_id.clone()),
                extra: Map::new(),
            },
            materials: vec![Material {
                id: new_id(),
                role: ROLE_SOURCE.to_string(),
                form: FORM_PDF.to_string(),
                path: "source.pdf".to_string(),
                sha256: Some(sha),
                bytes: Some(bytes),
                pages,
                pages_modified: pages.map(|_| now.clone()),
                pages_modified_by: pages.map(|_| self.device_id.clone()),
                title: None,
                provenance: Provenance {
                    method: Some(METHOD_ADDED_BY_USER.to_string()),
                    time: Some(now),
                    origin_name,
                    proofread: None,
                    page_map: None,
                    source_sha256: None,
                    extra: Map::new(),
                },
                extra: Map::new(),
            }],
            groups: Vec::new(),
            groups_modified: None,
            groups_modified_by: None,
            extra: Map::new(),
        };
        write_json_atomic(&staging.join(PAPER_FILE), &paper)?;

        // 5. 暫存資料夾改成正式的名稱。
        self.place_new_folder(staging, &base, paper)?;
        Ok(AddOutcome::Added(id))
    }

    /// 新文獻的暫存資料夾（裡面已有 `paper.json`）改成正式的名稱：名稱已存在就換下一個序號；
    /// 落盤、把這篇放進記憶體裡的目錄。交回正式名稱。加入與整批匯入共用。
    pub(crate) fn place_new_folder(
        &mut self,
        staging: &Path,
        base: &str,
        paper: PaperFile,
    ) -> Result<String> {
        let papers = self.papers_dir();
        let mut existing = names_in(&papers)?;
        let mut final_name = None;
        for _ in 0..RENAME_TRIES {
            let candidate = unique_name(&existing, base);
            let target = papers.join(&candidate);
            if fs::symlink_metadata(&target).is_ok() {
                existing.push(candidate);
                continue;
            }
            match fs::rename(staging, &target) {
                Ok(()) => {
                    final_name = Some(candidate);
                    break;
                }
                Err(e) => {
                    if fs::symlink_metadata(&target).is_ok() {
                        // 別處在這一瞬間占用了這個名稱
                        existing.push(candidate);
                        continue;
                    }
                    return Err(io_error(&target, e));
                }
            }
        }
        let final_name = final_name.ok_or_else(|| LibError::Io {
            path: papers.display().to_string(),
            reason: format!("no free folder name after {RENAME_TRIES} tries"),
        })?;
        sync_dir(&papers);

        let pj = papers.join(&final_name).join(PAPER_FILE);
        let m = fs::metadata(&pj).map_err(|e| io_error(&pj, e))?;
        self.folders.insert(
            final_name.clone(),
            Folder {
                name: final_name.clone(),
                mtime: m.modified().ok(),
                size: m.len(),
                paper: Some(paper),
            },
        );
        Ok(final_name)
    }

    /// 相同 DOI 的文獻（先 `normalize_doi` 再比對）。供加入時提醒「可能是同一篇」。
    pub fn find_by_doi(&self, doi: &str) -> Vec<String> {
        let want = normalize_doi(doi);
        if want.is_empty() {
            return vec![];
        }
        self.catalog()
            .into_iter()
            .filter(|(_, p)| {
                p.bib.doi.as_deref().map(normalize_doi).as_deref() == Some(want.as_str())
            })
            .map(|(_, p)| p.id.clone())
            .collect()
    }

    pub(crate) fn find_by_source_sha(&self, sha: &str) -> Option<String> {
        self.catalog()
            .into_iter()
            .find(|(_, p)| has_source_sha(p, sha))
            .map(|(_, p)| p.id.clone())
    }

    /// 讀 `.trash/*/paper.json`，比對 `role` 為 `source` 的 `sha256`。
    pub(crate) fn find_in_trash_by_source_sha(&self, sha: &str) -> Option<String> {
        self.trash_papers()
            .find(|p| has_source_sha(p, sha))
            .map(|p| p.id)
    }

    /// 回收區每篇原文的指紋 → 那篇的代號（依資料夾名稱排序，第一個算數）。整批匯入比對時讀一次。
    pub(crate) fn trash_source_index(&self) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        for p in self.trash_papers() {
            for m in p.materials.iter().filter(|m| m.role == ROLE_SOURCE) {
                if let Some(sha) = &m.sha256 {
                    out.entry(sha.clone()).or_insert_with(|| p.id.clone());
                }
            }
        }
        out
    }

    /// `.trash/*/paper.json` 讀得懂的每一篇，依資料夾名稱排序。
    fn trash_papers(&self) -> impl Iterator<Item = PaperFile> {
        let trash = self.root.join(TRASH_DIR);
        let mut names: Vec<_> = fs::read_dir(&trash)
            .map(|rd| rd.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();
        names.sort_by_key(|e| e.file_name());
        names
            .into_iter()
            .filter_map(|e| read_json::<PaperFile>(&e.path().join(PAPER_FILE)).ok())
            .map(|(p, _)| p)
    }
}

fn has_source_sha(p: &PaperFile, sha: &str) -> bool {
    p.materials
        .iter()
        .any(|m| m.role == ROLE_SOURCE && m.sha256.as_deref() == Some(sha))
}

/// 分段複製並算 SHA-256，最後 `sync_all`。交回（指紋，位元組數）。
pub(crate) fn copy_with_hash(input: &mut dyn Read, dest: &Path) -> Result<(String, u64)> {
    let (sha, total, _) = copy_with_hash_and_head(input, dest)?;
    Ok((sha, total))
}

/// 同 `copy_with_hash`，另外交回複製到的前 `PDF_HEADER_WINDOW` 個位元組：呼叫的一方可以
/// 不再按路徑去開暫存檔就驗檔頭（複製途中資料夾若被改名，暫存檔的舊路徑已不存在）。
pub(crate) fn copy_with_hash_and_head(
    input: &mut dyn Read,
    dest: &Path,
) -> Result<(String, u64, Vec<u8>)> {
    let mut output = fs::File::create(dest).map_err(|e| io_error(dest, e))?;
    let mut head: Vec<u8> = Vec::with_capacity(crate::pdf::PDF_HEADER_WINDOW);
    let (sha, total) = hash_reader(
        input,
        |e| io_error(Path::new("<stream>"), e),
        |chunk| {
            if head.len() < crate::pdf::PDF_HEADER_WINDOW {
                let take = (crate::pdf::PDF_HEADER_WINDOW - head.len()).min(chunk.len());
                head.extend_from_slice(&chunk[..take]);
            }
            output.write_all(chunk).map_err(|e| io_error(dest, e))
        },
    )?;
    output.sync_all().map_err(|e| io_error(dest, e))?;
    Ok((sha, total, head))
}
