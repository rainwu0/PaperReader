//! 重點筆記的讀寫。格式與規則寫在 `crate::digest` 的開頭說明。
//!
//! - 建立：先檢查交來的內容（架構 5.3 規則 4）→ 挑檔名（資料夾裡的項目與清單裡別的材料的檔名都算占用；
//!   目標已經存在就是衝突）→ 原子寫入檔案 → 材料清單加一項；清單寫不成就拿掉剛放的檔。
//!   材料的 `sha256`、`bytes` 是 `null`（可以在筆記工具裡改）。
//! - 改（刪一點、標為已校對）：版本記號是檔案內容的指紋，不符回報 `Conflict`、什麼都不寫；比過記號的
//!   那份位元組就是要改的內容。讀不懂（不是 UTF-8、沒有 `paperreader_digest`）不覆寫；格式較新只讀。
//!   只換掉要改的那幾行。刪一點只寫這份檔；標為已校對另外把材料的 `provenance.proofread` 改成一樣：
//!   寫之前重讀 `paper.json`，寫不成就把舊的內容寫回去。
//! - 刪一點是唯一刪除一點的操作，順序：檢查都過 → 記一筆 `digest-point-deleted`
//!   → 寫檔。

use super::{LocalLibrary, PAPER_FILE, is_valid_material_path};
use crate::changes::DIGEST_POINT_DELETED;
use crate::digest::*;
use crate::error::{LibError, Result, io_error};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, sha256_hex, write_bytes_atomic, write_json_atomic};
use crate::overlay::METHOD_GENERATED;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// 解析一份檔的位元組：不是 UTF-8、不是重點筆記都是 `Unreadable`。
pub(crate) fn parse_bytes(path: &Path, bytes: &[u8]) -> Result<ParsedDigest> {
    let unreadable = |reason: String| LibError::Unreadable {
        path: path.display().to_string(),
        reason,
    };
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("not UTF-8".into()))?;
    crate::digest::parse(text).map_err(unreadable)
}

impl LocalLibrary {
    /// 建立一份重點筆記。`token` 是那篇 `paper.json` 的版本記號。
    pub fn create_digest(
        &mut self,
        paper_id: &str,
        draft: DigestDraft,
        token: &VersionToken,
    ) -> Result<CreatedDigest> {
        let located = self.locate_for_write(paper_id, token)?;
        let folder = located.folder;
        let paper = located.paper;
        let source = paper
            .materials
            .iter()
            .find(|m| m.id == draft.source_material)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {}", draft.source_material),
            })?
            .clone();
        let pages = source.pages.ok_or_else(|| LibError::InvalidInput {
            reason: "the page count of the source is not known yet".to_string(),
        })?;
        validate_draft(&draft, pages)?;

        let now = now_utc();
        let stamp = DigestStamp {
            time: now.clone(),
            device: self.device_id.clone(),
        };
        let text = render(&draft, paper_id, source.sha256.as_deref(), &stamp);
        let dir = self.papers_dir().join(&folder);
        // 挑檔名之後、寫之前又出現了同名的項目：不蓋掉它
        let (name, target) = self.new_material_target(&dir, &paper.materials, "digest", "md")?;
        let digest_token = write_bytes_atomic(&target, text.as_bytes())?;

        let material_id = new_id();
        let material = Material {
            id: material_id.clone(),
            role: ROLE_DIGEST.to_string(),
            form: FORM_MARKDOWN.to_string(),
            path: name,
            // 可以在筆記工具裡改：指紋與大小不記（同譯文疊加檔）
            sha256: None,
            bytes: None,
            pages: None,
            pages_modified: None,
            pages_modified_by: None,
            title: None,
            provenance: Provenance {
                method: Some(METHOD_GENERATED.to_string()),
                time: Some(now),
                origin_name: None,
                // 產生的重點筆記一律「未校對」
                proofread: Some(false),
                page_map: Some(PAGE_MAP_MARKED.to_string()),
                source_sha256: source.sha256.clone(),
                extra: Map::new(),
            },
            extra: Map::new(),
        };
        // 清單寫不成時拿掉剛放的檔
        let paper_token = self.commit_new_material(&folder, paper, material)?;
        Ok(CreatedDigest {
            material_id,
            paper_token,
            digest_token,
        })
    }

    /// 讀一份重點筆記與它的版本記號（檔案內容的指紋）。
    pub fn read_digest(
        &self,
        paper_id: &str,
        material_id: &str,
    ) -> Result<(DigestDoc, VersionToken)> {
        let (_, path) = self.digest_path(paper_id, material_id)?;
        let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
        let parsed = parse_bytes(&path, &bytes)?;
        Ok((parsed.into_doc(), VersionToken(sha256_hex(&bytes))))
    }

    /// 刪一點。`token` 是重點筆記檔的版本記號。
    pub fn delete_digest_point(
        &mut self,
        paper_id: &str,
        material_id: &str,
        point_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.change_digest(paper_id, material_id, token, None, |lib, parsed, stamp| {
            let text = parsed.without_point(point_id, stamp)?;
            let rec = lib.new_change(
                Some(paper_id),
                DIGEST_POINT_DELETED,
                json!({ "material": material_id, "point": point_id }),
            );
            lib.append_change(&rec)?;
            Ok(Some(text))
        })
    }

    /// 標為已校對（或改回未校對）。已經是這個狀態時不寫，交回原本的記號。
    pub fn set_digest_proofread(
        &mut self,
        paper_id: &str,
        material_id: &str,
        proofread: bool,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.change_digest(
            paper_id,
            material_id,
            token,
            Some(proofread),
            |_, parsed, stamp| Ok(parsed.with_proofread(proofread, stamp)),
        )
    }

    /// 改一份重點筆記的共通流程。`change` 交回新的內容（`None` 是不必改）；它也可以先寫變更記錄。
    /// `proofread` 有值時材料的 `provenance.proofread` 跟著改。
    fn change_digest<F>(
        &mut self,
        paper_id: &str,
        material_id: &str,
        token: &VersionToken,
        proofread: Option<bool>,
        change: F,
    ) -> Result<VersionToken>
    where
        F: FnOnce(&mut LocalLibrary, &ParsedDigest, &DigestStamp) -> Result<Option<String>>,
    {
        self.require_writable()?;
        let located = self.locate_mut(paper_id)?;
        if located.paper.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew {
                found: located.paper.format,
            });
        }
        let path = self.digest_path_in(&located.folder, &located.paper, material_id)?;
        // 比過記號的這一份位元組就是要改的內容
        let old = fs::read(&path).map_err(|e| io_error(&path, e))?;
        if &VersionToken(sha256_hex(&old)) != token {
            return Err(LibError::Conflict);
        }
        let parsed = parse_bytes(&path, &old)?;
        if is_newer(parsed.doc()) {
            return Err(LibError::FormatTooNew {
                found: parsed.doc().format,
            });
        }
        let stamp = DigestStamp {
            time: now_utc(),
            device: self.device_id.clone(),
        };
        let Some(text) = change(self, &parsed, &stamp)? else {
            return Ok(token.clone());
        };
        let new_token = write_bytes_atomic(&path, text.as_bytes())?;
        let Some(p) = proofread else {
            return Ok(new_token);
        };
        // 材料的「已校對」跟著改：寫之前重讀 `paper.json`，只改這一項
        let again = match self.locate_mut(paper_id) {
            Ok(a) if a.folder == located.folder => a,
            Ok(_) | Err(_) => {
                let _ = write_bytes_atomic(&path, &old);
                return Err(LibError::Conflict);
            }
        };
        let mut paper = again.paper;
        if let Some(m) = paper.materials.iter_mut().find(|m| m.id == material_id) {
            m.provenance.proofread = Some(p);
            m.sha256 = None;
            m.bytes = None;
        }
        let dir = self.papers_dir().join(&again.folder);
        if let Err(e) = write_json_atomic(&dir.join(PAPER_FILE), &paper) {
            // 清單沒寫成：把舊的內容寫回去，檔案與清單仍然一致
            let _ = write_bytes_atomic(&path, &old);
            return Err(e);
        }
        self.remember_paper(&again.folder, paper)?;
        Ok(new_token)
    }

    /// 材料必須在清單裡、是 `digest` 加 `markdown`、路徑合法。交回（資料夾名稱, 檔案位置）。
    fn digest_path(&self, paper_id: &str, material_id: &str) -> Result<(String, PathBuf)> {
        let located = self.locate(paper_id)?;
        let path = self.digest_path_in(&located.folder, &located.paper, material_id)?;
        Ok((located.folder, path))
    }

    fn digest_path_in(
        &self,
        folder: &str,
        paper: &PaperFile,
        material_id: &str,
    ) -> Result<PathBuf> {
        let m = paper
            .materials
            .iter()
            .find(|m| m.id == material_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {material_id}"),
            })?;
        if !is_digest(m) {
            return Err(LibError::InvalidInput {
                reason: format!("material {material_id} is not a digest"),
            });
        }
        if !is_valid_material_path(&m.path) {
            return Err(LibError::InvalidInput {
                reason: format!("invalid material path: {}", m.path),
            });
        }
        Ok(self.papers_dir().join(folder).join(&m.path))
    }
}
