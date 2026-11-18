//! 產生的譯本 PDF：畫面用譯文疊加檔產生一份版面相同的 PDF，交給核心存成這篇的譯本材料
//! （`role` `translation`、`form` `pdf`，和使用者自己加入的 PDF 譯本同一種，所以劃線、筆跡、筆記、匯出都直接可用）。
//!
//! - 材料的出處：`method` 是 `generated`，`time` 是這次產生的時間，`source_sha256` 沿用疊加檔那一項的、
//!   也就是當時那份原文的指紋，`generated_from`（出處裡不認得的欄位）記是由哪一份疊加檔產生的。
//! - 檔名 `translation-generated.pdf`（被占用時 `-2` …）。使用者自己加入的譯本檔不會被碰到。
//! - 接著翻或重翻之後再產生：找到 `generated_from` 是同一份疊加檔的那一項，**原地更新**——同一個材料代號、
//!   同一個檔名，檔案換成新的，`sha256`、`bytes`（版本記號）、`pages`、`time` 一起更新。
//!   劃線與筆跡以材料代號記錄，所以全部保留、不刪（對不上新版的由畫面標為找不回，見結果文件）。
//! - 內容與現有的完全相同：什麼都不寫，回報 `Unchanged`。
//! - 先檢查（架構 5.3 規則 4）：是 PDF、不是空的、大小不超過 `MAX_GENERATED_BYTES`、頁數合理。

use super::add::copy_with_hash_and_head;
use super::{LocalLibrary, PAPER_FILE, TEMP_PREFIX, check_pages, is_valid_material_path};
use crate::error::{LibError, Result, io_error};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, sync_dir, write_json_atomic};
use crate::overlay::{METHOD_GENERATED, OVERLAY_FORM};
use crate::pdf::looks_like_pdf;
use crate::store::PutGenerated;
use std::fs;
use std::io::Read;
use std::path::Path;

/// 產生的譯本最大幾個位元組（原文的圖都在裡面，所以給大一點）。
pub const MAX_GENERATED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 出處裡記「由哪一份疊加檔產生」的欄位名稱。
pub const GENERATED_FROM: &str = "generated_from";

fn generated_from(m: &Material) -> Option<&str> {
    m.provenance
        .extra
        .get(GENERATED_FROM)
        .and_then(|v| v.as_str())
}

impl LocalLibrary {
    /// 存下（或原地更新）由 `overlay_material_id` 這份疊加檔產生的譯本 PDF。`token` 是那篇 `paper.json` 的版本記號。
    pub fn put_generated_translation(
        &mut self,
        paper_id: &str,
        overlay_material_id: &str,
        reader: &mut dyn Read,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<PutGenerated> {
        let located = self.locate_for_write(paper_id, token)?;
        if let Some(p) = pages {
            check_pages(p)?;
        }
        if !located.paper.materials.iter().any(|m| {
            m.id == overlay_material_id && m.role == ROLE_TRANSLATION && m.form == OVERLAY_FORM
        }) {
            return Err(LibError::NotFound {
                what: format!("overlay material {overlay_material_id}"),
            });
        }
        let folder = located.folder.clone();
        let dir = self.papers_dir().join(&folder);
        let tmp = dir.join(format!("{TEMP_PREFIX}tmp-{}", new_id()));
        let result = self.finish_generated(
            paper_id,
            overlay_material_id,
            &folder,
            &dir,
            &tmp,
            reader,
            pages,
            token,
        );
        if !matches!(
            result,
            Ok(PutGenerated::Created { .. } | PutGenerated::Updated { .. })
        ) {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_generated(
        &mut self,
        paper_id: &str,
        overlay_id: &str,
        folder: &str,
        dir: &Path,
        tmp: &Path,
        reader: &mut dyn Read,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<PutGenerated> {
        // 邊複製邊算指紋
        let (sha, bytes, head) =
            copy_with_hash_and_head(&mut reader.take(MAX_GENERATED_BYTES + 1), tmp)?;
        if bytes == 0 {
            return Err(LibError::InvalidInput {
                reason: "empty content".to_string(),
            });
        }
        if bytes > MAX_GENERATED_BYTES {
            return Err(LibError::InvalidInput {
                reason: format!("generated translation is larger than {MAX_GENERATED_BYTES} bytes"),
            });
        }
        if !looks_like_pdf(&head) {
            return Err(LibError::InvalidInput {
                reason: "not a PDF: no %PDF- marker in the first 1024 bytes".to_string(),
            });
        }
        // 再確認一次：複製期間記號變了或資料夾被改名
        let mut paper = self
            .relocate_after_copy(paper_id, token, folder, tmp)?
            .paper;
        let overlay_sha = paper
            .materials
            .iter()
            .find(|m| m.id == overlay_id)
            .and_then(|m| m.provenance.source_sha256.clone());
        let now = now_utc();
        let existing = paper.materials.iter().position(|m| {
            m.role == ROLE_TRANSLATION
                && m.form == FORM_PDF
                && m.provenance.method.as_deref() == Some(METHOD_GENERATED)
                && generated_from(m) == Some(overlay_id)
        });
        let pj = dir.join(PAPER_FILE);

        if let Some(i) = existing {
            let m = &paper.materials[i];
            // 路徑不合規則（paper.json 被改壞），或別的材料用同一個路徑：拒絕，什麼都不動
            let path = m.path.clone();
            if !is_valid_material_path(&path)
                || paper
                    .materials
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && o.path == path)
            {
                return Err(LibError::InvalidInput {
                    reason: format!("generated translation has an unusable path: {path:?}"),
                });
            }
            let target = dir.join(&path);
            let file_exists = fs::symlink_metadata(&target).is_ok_and(|md| md.is_file());
            if file_exists && m.sha256.as_deref() == Some(sha.as_str()) {
                return Ok(PutGenerated::Unchanged {
                    material_id: m.id.clone(),
                });
            }
            // 舊檔先移到旁邊；清單寫成功才刪，寫不成就搬回來。舊檔不在（使用者刪了）就直接放新檔
            let backup = dir.join(format!("{TEMP_PREFIX}old-{}", new_id()));
            if file_exists {
                fs::rename(&target, &backup).map_err(|e| io_error(&target, e))?;
            }
            let restore = |backup: &Path, target: &Path| {
                if file_exists {
                    let _ = fs::rename(backup, target);
                } else {
                    let _ = fs::remove_file(target);
                }
            };
            if let Err(e) = fs::rename(tmp, &target) {
                restore(&backup, &target);
                return Err(io_error(&target, e));
            }
            let material_id = m.id.clone();
            let m = &mut paper.materials[i];
            m.sha256 = Some(sha);
            m.bytes = Some(bytes);
            if pages.is_some() {
                m.pages = pages;
                m.pages_modified = Some(now.clone());
                m.pages_modified_by = Some(self.device_id.clone());
            }
            m.provenance.time = Some(now);
            m.provenance.source_sha256 = overlay_sha;
            let new_token = match write_json_atomic(&pj, &paper) {
                Ok(t) => t,
                Err(e) => {
                    restore(&backup, &target);
                    return Err(e);
                }
            };
            if file_exists {
                let _ = fs::remove_file(&backup);
            }
            sync_dir(dir);
            self.remember_paper(folder, paper)?;
            return Ok(PutGenerated::Updated {
                material_id,
                token: new_token,
            });
        }

        // 第一次：挑檔名、放進去、清單加一項
        let (name, target) =
            self.new_material_target(dir, &paper.materials, "translation-generated", "pdf")?;
        fs::rename(tmp, &target).map_err(|e| io_error(&target, e))?;
        let material_id = new_id();
        let mut extra = Map::new();
        extra.insert(GENERATED_FROM.to_string(), overlay_id.into());
        let material = Material {
            id: material_id.clone(),
            role: ROLE_TRANSLATION.to_string(),
            form: FORM_PDF.to_string(),
            path: name,
            sha256: Some(sha),
            bytes: Some(bytes),
            pages,
            pages_modified: pages.map(|_| now.clone()),
            pages_modified_by: pages.map(|_| self.device_id.clone()),
            title: None,
            provenance: Provenance {
                method: Some(METHOD_GENERATED.to_string()),
                time: Some(now),
                origin_name: None,
                proofread: Some(false),
                page_map: None,
                source_sha256: overlay_sha,
                extra,
            },
            extra: Map::new(),
        };
        let new_token = self.commit_new_material(folder, paper, material)?;
        Ok(PutGenerated::Created {
            material_id,
            token: new_token,
        })
    }
}
