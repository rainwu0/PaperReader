//! 記下一項材料的總頁數。

use super::{LocalLibrary, check_pages};
use crate::error::{LibError, Result};
use crate::ids::now_utc;

impl LocalLibrary {
    /// 記下一項材料的總頁數。與現有的值相同時不寫，交回 `false`；不同（包括原本是 `null`）
    /// 時寫入，交回 `true`。背景寫入。只改那一項材料的 `pages`、`pages_modified`、
    /// `pages_modified_by`，不動 `bib.modified`（頁數不是書目）。以實際檔案為準，原本有值而不同時照樣覆寫。
    ///
    /// 已知的後果：頁數變少時，頁碼超過新頁數的舊 `position` 事件變成「不可用」，
    /// 閱讀位置可能退到更早的一筆。
    pub fn set_material_pages(
        &mut self,
        paper_id: &str,
        material_id: &str,
        pages: u32,
    ) -> Result<bool> {
        self.require_writable()?;
        let device = self.device_id.clone();
        let written = self.update_paper_background(paper_id, |paper| {
            let m = paper
                .materials
                .iter_mut()
                .find(|m| m.id == material_id)
                .ok_or_else(|| LibError::NotFound {
                    what: format!("material {material_id}"),
                })?;
            check_pages(pages)?;
            if m.pages == Some(pages) {
                return Ok(false);
            }
            m.pages = Some(pages);
            m.pages_modified = Some(now_utc());
            m.pages_modified_by = Some(device.clone());
            Ok(true)
        })?;
        Ok(written.is_some())
    }
}
