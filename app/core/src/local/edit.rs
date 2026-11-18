//! 修改書目與自動補書目。

use super::{LocalLibrary, PAPER_FILE};
use crate::error::{LibError, Result};
use crate::format::*;
use crate::ids::now_utc;
use crate::jsonio::{VersionToken, write_json_atomic};

/// 改書目時更新 `cleared`：自動補的三欄裡，原本有值、改成空的記下欄名；填了值的拿掉。
/// 其他欄位不記（自動補與合併都不補它們）。
/// 結果照 `canonical_cleared` 的順序。
fn update_cleared(old: &Bib, new: &mut Bib) {
    let mut out: Vec<String> = Vec::new();
    for f in FILLABLE_BIB_FIELDS {
        let had = old.cleared.iter().any(|c| c == f);
        if new.is_blank(f) && (had || !old.is_blank(f)) {
            out.push(f.to_string());
        }
    }
    out.extend(
        old.cleared
            .iter()
            .filter(|c| !FILLABLE_BIB_FIELDS.contains(&c.as_str()))
            .cloned(),
    );
    new.cleared = canonical_cleared(&out);
}

impl LocalLibrary {
    /// 修改書目。寫入前重新讀檔、重算版本記號；與呼叫的一方手上的不同，表示檔案在別處被改過，
    /// 不寫入，交回 `Conflict`。只換掉書目的六個欄位並更新 `modified`、`modified_by`（以及 `cleared`，
    /// 見 `update_cleared`），描述檔其餘部分（含不認得的欄位）原樣保留。不改資料夾名稱。
    pub fn update_bib(
        &mut self,
        id: &str,
        draft: BibDraft,
        token: &VersionToken,
    ) -> Result<VersionToken> {
        let located = self.locate_for_write(id, token)?;
        let folder_name = located.folder;
        let path = self.papers_dir().join(&folder_name).join(PAPER_FILE);
        let mut paper: PaperFile = located.paper;
        let old = paper.bib.clone();

        paper.bib.title = draft.title;
        paper.bib.authors = draft.authors;
        paper.bib.year = draft.year;
        paper.bib.venue = draft.venue;
        paper.bib.doi = draft
            .doi
            .as_deref()
            .map(normalize_doi)
            .filter(|d| !d.is_empty());
        paper.bib.url = draft.url;
        update_cleared(&old, &mut paper.bib);
        paper.bib.modified = Some(now_utc());
        paper.bib.modified_by = Some(self.device_id.clone());

        let new_token = write_json_atomic(&path, &paper)?;
        self.remember_paper(&folder_name, paper)?;
        Ok(new_token)
    }

    /// 自動補書目：只補空著的 `year`、`doi`、`url`（`null` 或空字串），有值的、`cleared` 記下的不動；
    /// **不改** `bib.modified`、`bib.modified_by`（自動補的值不是使用者的修改，不能讓這份書目在同步時變成「較新」）。
    /// 補到了交回 `true`。背景寫入（`update_paper_background`）。
    /// 檢查：年份 1000 到 9999；DOI 經 `normalize_doi` 後以 `10.` 開頭、含 `/`；網址以 `http://` 或 `https://` 開頭。
    pub fn fill_bib_blanks(&mut self, paper_id: &str, fill: BibFill) -> Result<bool> {
        self.require_writable()?;
        crate::references::check_year(fill.year)?;
        let doi = crate::references::checked_doi(fill.doi.as_deref())?;
        let url = fill
            .url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(String::from);
        if let Some(u) = &url
            && !(u.starts_with("http://") || u.starts_with("https://"))
        {
            return Err(LibError::InvalidInput {
                reason: format!("url must start with http:// or https://: {u:?}"),
            });
        }
        let from = Bib {
            year: fill.year,
            doi,
            url,
            ..Default::default()
        };
        let written =
            self.update_paper_background(paper_id, |paper| Ok(paper.bib.fill_blanks_from(&from)))?;
        Ok(written.is_some())
    }
}
