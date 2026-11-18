//! 筆跡的讀寫。檔案是 `papers/<資料夾>/ink.json`，格式在 `crate::ink`。
//!
//! 寫入的共通流程：讀現在的檔案（讀不懂就回報、不覆寫）→ 版本記號與 `token` 不符回報 `Conflict`
//! → 改 → 原子寫入。

use super::{LocalLibrary, PAPERS_DIR};
use crate::error::{LibError, Result};
use crate::format::FORMAT_VERSION;
use crate::ids::{new_id, now_utc};
use crate::ink::{INK_FILE, InkFile, Stroke, round2, validate_draft};
use crate::jsonio::{VersionToken, read_json, write_json_atomic};
use crate::store::StrokeDraft;
use std::path::PathBuf;

impl LocalLibrary {
    fn ink_path(&self, folder: &str) -> PathBuf {
        self.root.join(PAPERS_DIR).join(folder).join(INK_FILE)
    }

    /// 讀一篇的筆跡。`ink.json` 還不存在時交回空的筆跡與 `None`；讀不懂回報 `Unreadable`。
    pub fn read_ink(&self, paper_id: &str) -> Result<(InkFile, Option<VersionToken>)> {
        let located = self.locate(paper_id)?;
        match read_ink_file(&self.ink_path(&located.folder))? {
            Some((doc, token)) => Ok((doc, Some(token))),
            None => Ok((InkFile::default(), None)),
        }
    }

    /// 加一筆。檔案還不存在時 `token` 帶 `None`。
    pub fn add_stroke(
        &mut self,
        paper_id: &str,
        draft: StrokeDraft,
        token: Option<&VersionToken>,
    ) -> Result<(Stroke, VersionToken)> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        let material = paper
            .paper
            .materials
            .iter()
            .find(|m| m.id == draft.material)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {}", draft.material),
            })?;
        validate_draft(&draft, material.pages)?;
        let stroke = Stroke {
            id: format!("s-{}", new_id().to_lowercase()),
            material: draft.material.clone(),
            page: draft.page_index,
            color: draft.color.to_lowercase(),
            width: round2(draft.width).max(0.01),
            opacity: round2(draft.opacity).max(0.01),
            created: now_utc(),
            points: draft
                .points
                .iter()
                .map(|p| [round2(p[0]), round2(p[1])])
                .collect(),
            extra: Default::default(),
        };
        let s = stroke.clone();
        let t = self.write_ink(
            &paper.folder,
            token,
            false,
            move |doc| {
                doc.strokes.push(s);
                Ok(Some(Vec::new()))
            },
            paper_id,
        )?;
        Ok((stroke, t.expect("add always writes")))
    }

    /// 刪一筆。
    pub fn delete_stroke(
        &mut self,
        paper_id: &str,
        stroke_id: &str,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        let t = self.write_ink(
            &paper.folder,
            Some(token),
            true,
            |doc| {
                let before = doc.strokes.len();
                doc.strokes.retain(|s| s.id != stroke_id);
                if doc.strokes.len() == before {
                    return Err(LibError::NotFound {
                        what: format!("stroke {stroke_id}"),
                    });
                }
                Ok(Some(vec![stroke_id.to_string()]))
            },
            paper_id,
        )?;
        Ok(t.expect("delete writes when it succeeds"))
    }

    /// 清除一份材料的一頁上所有的筆跡；那一頁沒有筆跡時不寫、交回原本的記號。
    pub fn clear_ink_page(
        &mut self,
        paper_id: &str,
        material_id: &str,
        page_index: u32,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let paper = self.locate_mut(paper_id)?;
        let t = self.write_ink(
            &paper.folder,
            Some(token),
            true,
            |doc| {
                // 記實際拿掉的每一筆的代號，不記「哪一頁、什麼時間之前」
                let removed: Vec<String> = doc
                    .strokes
                    .iter()
                    .filter(|s| s.material == material_id && s.page == page_index)
                    .map(|s| s.id.clone())
                    .collect();
                if removed.is_empty() {
                    return Ok(None);
                }
                doc.strokes
                    .retain(|s| !(s.material == material_id && s.page == page_index));
                Ok(Some(removed))
            },
            paper_id,
        )?;
        Ok(t.unwrap_or_else(|| token.clone()))
    }

    /// `op` 交回 `None` 表示不需要寫（交回 `None`）；交回 `Some(拿掉的筆跡代號)` 時，代號非空就先附加一筆
    /// `strokes-deleted` 再寫檔。`missing_is_not_found`：帶了記號、檔案卻不存在時，
    /// 回報 `NotFound` 而不是 `Conflict`（刪除、清除用）。
    fn write_ink<F>(
        &mut self,
        folder: &str,
        token: Option<&VersionToken>,
        missing_is_not_found: bool,
        op: F,
        paper_id: &str,
    ) -> Result<Option<VersionToken>>
    where
        F: FnOnce(&mut InkFile) -> Result<Option<Vec<String>>>,
    {
        let path = self.ink_path(folder);
        let mut doc = match (read_ink_file(&path)?, token) {
            (Some((doc, current)), Some(t)) => {
                if &current != t {
                    return Err(LibError::Conflict);
                }
                doc
            }
            (Some(_), None) => return Err(LibError::Conflict),
            (None, Some(_)) if missing_is_not_found => {
                return Err(LibError::NotFound {
                    what: "ink file".to_string(),
                });
            }
            (None, Some(_)) => return Err(LibError::Conflict),
            (None, None) => InkFile::default(),
        };
        if doc.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew { found: doc.format });
        }
        let Some(removed) = op(&mut doc)? else {
            return Ok(None);
        };
        if !removed.is_empty() {
            let rec = self.new_change(
                Some(paper_id),
                crate::changes::STROKES_DELETED,
                serde_json::json!({ "strokes": removed }),
            );
            self.append_change(&rec)?;
        }
        doc.modified = Some(now_utc());
        doc.modified_by = Some(self.device_id.clone());
        Ok(Some(write_json_atomic(&path, &doc)?))
    }
}

/// 讀 `ink.json`；不存在是 `None`，讀不懂是 `Unreadable`。
fn read_ink_file(path: &std::path::Path) -> Result<Option<(InkFile, VersionToken)>> {
    if !path.exists() {
        return Ok(None);
    }
    match read_json::<InkFile>(path) {
        Ok(v) => Ok(Some(v)),
        Err(LibError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}
