//! 加入譯本：複製進那一篇的資料夾，材料清單加一項 `role: translation`。

use super::add::copy_with_hash_and_head;
use super::{LocalLibrary, Located, check_pages};
use crate::error::{LibError, Result, io_error};
use crate::format::*;
use crate::ids::{new_id, now_utc};
use crate::jsonio::VersionToken;
use crate::pdf::looks_like_pdf;
use crate::store::AddMaterialOutcome;
use std::fs;
use std::io::Read;
use std::path::Path;

impl LocalLibrary {
    /// 把一份 PDF 加成這篇的譯本。`token` 是那篇 `paper.json` 的版本記號。
    /// 指紋與這篇任何一項材料（含原文）相同：`AlreadyPresent`，什麼都不寫。
    pub fn add_translation(
        &mut self,
        paper_id: &str,
        src: &Path,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<AddMaterialOutcome> {
        let pre = self.translation_precheck(paper_id, pages, token)?;
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
        self.add_translation_inner(paper_id, pre, &mut file, origin_name, pages, token)
    }

    /// 同 `add_translation`，來源是資料流（別的應用分享進來的檔案在 Android 上不一定有路徑）。
    pub fn add_translation_from_reader(
        &mut self,
        paper_id: &str,
        reader: &mut dyn Read,
        origin_name: Option<&str>,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<AddMaterialOutcome> {
        let pre = self.translation_precheck(paper_id, pages, token)?;
        self.add_translation_inner(
            paper_id,
            pre,
            reader,
            origin_name.map(String::from),
            pages,
            token,
        )
    }

    /// 第 1 步：唯讀 → `ReadOnly`；不在目錄 → `NotFound`；記號不符 → `Conflict`；
    /// `format` 較新 → `FormatTooNew`；頁數不在範圍 → `InvalidInput`。還沒寫任何東西。
    fn translation_precheck(
        &mut self,
        paper_id: &str,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<Located> {
        let located = self.locate_for_write(paper_id, token)?;
        if let Some(p) = pages {
            check_pages(p)?;
        }
        Ok(located)
    }

    fn add_translation_inner(
        &mut self,
        paper_id: &str,
        pre: Located,
        reader: &mut dyn Read,
        origin_name: Option<String>,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<AddMaterialOutcome> {
        let dir = self.papers_dir().join(&pre.folder);
        // 2. 暫存檔：邊複製邊算指紋
        let tmp = dir.join(format!("{}tmp-{}", super::TEMP_PREFIX, new_id()));
        let result = self.finish_translation(
            paper_id,
            &pre,
            &dir,
            &tmp,
            reader,
            origin_name,
            pages,
            token,
        );
        if result.is_err() || matches!(result, Ok(AddMaterialOutcome::AlreadyPresent { .. })) {
            // 失敗、或指紋已存在：暫存檔刪掉（資料夾已不在時就不在了，不算錯）
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_translation(
        &mut self,
        paper_id: &str,
        pre: &Located,
        dir: &Path,
        tmp: &Path,
        reader: &mut dyn Read,
        origin_name: Option<String>,
        pages: Option<u32>,
        token: &VersionToken,
    ) -> Result<AddMaterialOutcome> {
        let (sha, bytes, head) = copy_with_hash_and_head(reader, tmp)?;
        if let Some(mut hook) = self.translation_copy_hook.0.take() {
            hook();
        }
        // 3. 空的、不是 PDF。檔頭用複製時留下的前 1024 個位元組判斷，不按路徑再開暫存檔：
        //    複製途中文獻資料夾若被改名，暫存檔跟著搬走、舊路徑已不存在，按路徑開會得到
        //    `NotFound` 而不是第 5 步該有的 `Conflict`。
        if bytes == 0 {
            return Err(LibError::InvalidInput {
                reason: "empty content".to_string(),
            });
        }
        if !looks_like_pdf(&head) {
            return Err(LibError::InvalidInput {
                reason: "not a PDF: no %PDF- marker in the first 1024 bytes".to_string(),
            });
        }
        // 4. 指紋與這篇任何一項材料相同
        if let Some(m) = pre
            .paper
            .materials
            .iter()
            .find(|m| m.sha256.as_deref() == Some(sha.as_str()))
        {
            return Ok(AddMaterialOutcome::AlreadyPresent {
                material_id: m.id.clone(),
            });
        }
        // 5. 再確認一次：記號不符，或資料夾名稱與第 1 步不同（複製期間被改名；兩個位置的暫存檔都清）
        let paper = self
            .relocate_after_copy(paper_id, token, &pre.folder, tmp)?
            .paper;

        // 6. 挑檔名：實際在資料夾裡的任何項目（不分大小寫、含使用者自己放的）與清單裡的檔名都算被占用；
        //    改名前再確認那個名稱不存在（fs::rename 會蓋掉同名的檔）
        let (target_name, target) =
            self.new_material_target(dir, &paper.materials, "translation", "pdf")?;
        fs::rename(tmp, &target).map_err(|e| io_error(&target, e))?;

        // 7. 材料清單加一項；8. 更新記憶體裡的目錄
        let now = now_utc();
        let material_id = new_id();
        let material = Material {
            id: material_id.clone(),
            role: ROLE_TRANSLATION.to_string(),
            form: FORM_PDF.to_string(),
            path: target_name,
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
        };
        let new_token = self.commit_new_material(&pre.folder, paper, material)?;
        Ok(AddMaterialOutcome::Added {
            material_id,
            token: new_token,
        })
    }
}
