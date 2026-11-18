//! 譯文疊加檔的讀寫。格式與規則寫在 `crate::overlay` 的開頭說明。
//!
//! 寫入的共通流程與筆跡相同：讀現在的檔案（讀不懂就回報、不覆寫）→ 版本記號不符回報 `Conflict`
//! → 改 → 原子寫入。

use super::LocalLibrary;
use crate::error::{LibError, Result};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::jsonio::{VersionToken, read_json, write_json_atomic};
use crate::overlay::*;
use crate::store::CreatedOverlay;
use std::path::PathBuf;

impl LocalLibrary {
    /// 建立一份譯文疊加檔：每一段都是待翻。`token` 是那篇 `paper.json` 的版本記號。
    pub fn create_overlay(
        &mut self,
        paper_id: &str,
        draft: OverlayDraft,
        token: &VersionToken,
    ) -> Result<CreatedOverlay> {
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
        validate_draft(&draft, source.pages)?;

        let dir = self.papers_dir().join(&folder);
        // 挑檔名之後、寫之前又出現了同名的項目：不蓋掉它
        let (name, target) =
            self.new_material_target(&dir, &paper.materials, "translation-overlay", "json")?;
        let now = now_utc();
        let file = OverlayFile {
            format: FORMAT_VERSION,
            source_material: draft.source_material.clone(),
            provider: draft.provider.clone(),
            model: draft.model.clone(),
            source_lang: draft.source_lang.clone(),
            target_lang: draft.target_lang.clone(),
            created: Some(now.clone()),
            created_by: Some(self.device_id.clone()),
            modified: Some(now.clone()),
            modified_by: Some(self.device_id.clone()),
            segments: draft
                .segments
                .iter()
                .enumerate()
                .map(|(i, s)| OverlaySegment {
                    id: segment_id(i),
                    page: s.page,
                    bbox: s.bbox.map(round2),
                    kind: s.kind.clone(),
                    size: s.size.map(round2),
                    source: s.source.clone(),
                    target: None,
                    status: STATUS_PENDING.to_string(),
                    error: None,
                    issues: Vec::new(),
                    modified: None,
                    modified_by: None,
                    service: None,
                    by_user: false,
                    extra: Map::new(),
                })
                .collect(),
            terms: Vec::new(),
            extra: Map::new(),
        };
        let overlay_token = write_json_atomic(&target, &file)?;

        let material_id = new_id();
        let material = Material {
            id: material_id.clone(),
            role: ROLE_TRANSLATION.to_string(),
            form: OVERLAY_FORM.to_string(),
            path: name,
            sha256: None,
            bytes: None,
            pages: source.pages,
            pages_modified: source.pages.map(|_| now.clone()),
            pages_modified_by: source.pages.map(|_| self.device_id.clone()),
            title: None,
            provenance: Provenance {
                method: Some(METHOD_GENERATED.to_string()),
                time: Some(now),
                origin_name: None,
                // 產生的譯本一律「未校對」
                proofread: Some(false),
                page_map: None,
                source_sha256: source.sha256.clone(),
                extra: Map::new(),
            },
            extra: Map::new(),
        };
        // 清單寫不成時拿掉剛放的檔
        let paper_token = self.commit_new_material(&folder, paper, material)?;
        Ok(CreatedOverlay {
            material_id,
            paper_token,
            overlay_token,
        })
    }

    /// 讀一份譯文疊加檔與它的版本記號。檔案不存在回報 `NotFound`；讀不懂回報 `Unreadable`。
    pub fn read_overlay(
        &self,
        paper_id: &str,
        material_id: &str,
    ) -> Result<(OverlayFile, VersionToken)> {
        let path = self.overlay_path(paper_id, material_id)?;
        read_json::<OverlayFile>(&path)
    }

    /// 更新一批段落的譯文與狀態，只寫這一個檔案。任何一筆不合規則或找不到那一段，整批都不寫。
    pub fn update_overlay_segments(
        &mut self,
        paper_id: &str,
        material_id: &str,
        updates: &[SegmentUpdate],
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.update_overlay(paper_id, material_id, updates, None, token)
    }

    /// 更新一批段落，`terms` 有值時一併換掉這份譯本的術語（同一次寫入）。術語或任何一段不合規則，整個不寫。
    pub fn update_overlay(
        &mut self,
        paper_id: &str,
        material_id: &str,
        updates: &[SegmentUpdate],
        terms: Option<&[OverlayTerm]>,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        self.require_writable()?;
        let path = self.overlay_path(paper_id, material_id)?;
        let (mut doc, current) = read_json::<OverlayFile>(&path)?;
        if &current != token {
            return Err(LibError::Conflict);
        }
        if doc.format > FORMAT_VERSION {
            return Err(LibError::FormatTooNew { found: doc.format });
        }
        for u in updates {
            validate_update(u)?;
        }
        if let Some(t) = terms {
            validate_terms(t)?;
        }
        // 先全部找得到才改（整批不寫一半）
        let mut index: Vec<usize> = Vec::with_capacity(updates.len());
        for u in updates {
            let i = doc
                .segments
                .iter()
                .position(|s| s.id == u.id)
                .ok_or_else(|| LibError::NotFound {
                    what: format!("segment {}", u.id),
                })?;
            index.push(i);
        }
        let now = now_utc();
        for (u, i) in updates.iter().zip(index) {
            let s = &mut doc.segments[i];
            s.modified = Some(now.clone());
            s.modified_by = Some(self.device_id.clone());
            s.status = u.status.clone();
            s.target = if u.status == STATUS_DONE {
                u.target.clone()
            } else {
                None
            };
            s.error = if u.status == STATUS_FAILED {
                Some(u.error.clone().unwrap_or_default())
            } else {
                None
            };
            s.issues = if u.status == STATUS_DONE {
                u.issues.clone()
            } else {
                Vec::new()
            };
            // 翻這一段的服務：待翻的段落沒有（檢查已擋下帶著服務的待翻）
            s.service = u.service.clone();
            // 使用者決定的譯文：檢查已擋下不是已翻的
            s.by_user = u.by_user;
        }
        if let Some(t) = terms {
            doc.terms = t.to_vec();
        }
        doc.modified = Some(now);
        doc.modified_by = Some(self.device_id.clone());
        write_json_atomic(&path, &doc)
    }

    /// 材料必須在清單裡、是 `translation` 加 `overlay`、路徑合法。
    fn overlay_path(&self, paper_id: &str, material_id: &str) -> Result<PathBuf> {
        let located = self.locate(paper_id)?;
        let m = located
            .paper
            .materials
            .iter()
            .find(|m| m.id == material_id)
            .ok_or_else(|| LibError::NotFound {
                what: format!("material {material_id}"),
            })?;
        if m.role != ROLE_TRANSLATION || m.form != OVERLAY_FORM {
            return Err(LibError::InvalidInput {
                reason: format!("material {material_id} is not an overlay translation"),
            });
        }
        if !super::is_valid_material_path(&m.path) {
            return Err(LibError::InvalidInput {
                reason: format!("invalid material path: {}", m.path),
            });
        }
        Ok(self.papers_dir().join(&located.folder).join(&m.path))
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
