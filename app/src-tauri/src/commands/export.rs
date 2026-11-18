//! 匯出與列印：把文獻庫裡的東西另存成一份檔案，交給系統的分享、「存到…」或列印。
//!
//! 每一項先在應用暫存資料夾的 `exports/` 準備好一份檔案（`export_*` 指令，交回檔名與路徑），
//! 再用 `deliver_export` 交給使用者。文獻庫裡的檔案一律不動：原文只是複製一份出來。
//! 帶標註的 PDF 由畫面用 pdf-lib 組出來，位元組經 `export_put_bytes` 放進同一個暫存資料夾。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::{AppState, blocking};
use paperreader_core::bibexport::{BibFormat, export_bibliography};
use paperreader_core::citation::{CitationStyle, csl_json, has_author, render_citations};
use paperreader_core::translation_md::{translation_markdown, translation_text_markdown};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::PaperReaderExt;

/// 準備好、等著交給使用者的一份檔案。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    /// 檔名（存到…時當預設檔名，分享時別的應用看到的名稱）。
    pub name: String,
    /// 暫存檔的路徑，交給 `deliver_export`。
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BibExport {
    pub file: ExportFile,
    /// 寫進檔案的篇數。
    pub count: usize,
    /// 讀不出書目、沒有寫進去的篇數。
    pub skipped: usize,
}

/// 匯出的暫存資料夾 `<暫存>/exports`（一般匯出、備份檔、同步包都放這裡）。
pub(crate) fn exports_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("exports")
}

/// 匯出的暫存資料夾；順手清掉一小時前留下的（`clean_exports`）。
pub(crate) async fn exports_dir(app: &AppHandle) -> ApiResult<PathBuf> {
    let info = app.paperreader().storage_info().await?;
    let dir = exports_path(Path::new(&info.cache_dir));
    clean_exports(&dir, SystemTime::now())?;
    Ok(dir)
}

/// 清掉上次留下的匯出暫存（啟動時呼叫）：同 `clean_exports`，只刪一小時前的。
pub fn clear_export_cache(cache_dir: &Path) {
    let _ = clean_exports(&exports_path(cache_dir), SystemTime::now());
}

/// 匯出暫存留多久：分享出去之後別的應用可能還在讀，這之內的不刪。
const EXPORT_KEEP: Duration = Duration::from_secs(3600);

/// 清理暫存的匯出資料夾：一小時前留下的項目（檔，或同步包的工作資料夾整個）刪掉，這之內的不動，
/// 另一個流程可能正在分享。不存在就建立。一般匯出、備份檔、同步包、啟動時都用這一個。
pub(crate) fn clean_exports(dir: &Path, now: SystemTime) -> ApiResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| paperreader_core::error::io_error(dir, e))?;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else {
                continue;
            };
            let old = md
                .modified()
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > EXPORT_KEEP);
            if !old {
                continue;
            }
            let _ = if md.is_dir() {
                std::fs::remove_dir_all(e.path())
            } else {
                std::fs::remove_file(e.path())
            };
        }
    }
    Ok(())
}

/// 篇名等文字轉成能當檔名的樣子：換掉路徑分隔與不能用的字元、去掉頭尾空白與點，過長就截短。
pub(crate) fn file_stem(text: &str, fallback: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned: String = cleaned.trim().trim_matches('.').chars().take(80).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

/// 把文獻庫裡的一個檔複製一份到匯出的暫存資料夾，檔名是 `name`。文獻庫裡的檔不動。
pub(crate) fn copy_out(dir: &Path, src: &Path, name: String) -> ApiResult<ExportFile> {
    let dest = dir.join(&name);
    std::fs::copy(src, &dest).map_err(|e| paperreader_core::error::io_error(&dest, e))?;
    Ok(ExportFile {
        name,
        path: dest.display().to_string(),
    })
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> ApiResult<ExportFile> {
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| paperreader_core::error::io_error(&path, e))?;
    Ok(ExportFile {
        name: name.to_string(),
        path: path.display().to_string(),
    })
}

/// 一篇的顯示篇名：書目的篇名，沒有就用資料夾名稱。
fn title_of(paper: &paperreader_core::format::PaperFile) -> String {
    paper
        .bib
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| paper.short_name.clone())
        .unwrap_or_default()
}

pub(crate) fn paper_title(
    l: &paperreader_core::local::LocalLibrary,
    id: &str,
) -> Result<String, paperreader_core::error::LibError> {
    Ok(title_of(&l.get_paper(id)?.0))
}

/// 複製一份原文（或使用者加的譯本 PDF）出來，給分享、存到…、列印用。
#[tauri::command]
pub async fn export_original(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
) -> ApiResult<ExportFile> {
    let dir = exports_dir(&app).await?;
    state
        .run("export_original", move |s| {
            let (src, title) = s.with_lib(|l| {
                Ok((
                    l.material_path(&paper_id, &material_id)?,
                    paper_title(l, &paper_id)?,
                ))
            })?;
            copy_out(&dir, &src, format!("{}.pdf", file_stem(&title, "paper")))
        })
        .await
}

/// 複製這篇的 Markdown 筆記檔出來。還沒有筆記檔時交回 `NotFound`。
#[tauri::command]
pub async fn export_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
) -> ApiResult<ExportFile> {
    let dir = exports_dir(&app).await?;
    state
        .run("export_notes", move |s| {
            let (rel, title) = s.with_lib(|l| {
                let (doc, _) = l.read_notes(&paper_id)?;
                Ok((doc.path, paper_title(l, &paper_id)?))
            })?;
            let (Some(root), Some(rel)) = (s.open_library_root(), rel) else {
                return Err(ApiError::new(ErrorKind::NotFound, "no notes file"));
            };
            copy_out(
                &dir,
                &root.join(rel),
                format!("{}-筆記.md", file_stem(&title, "paper")),
            )
        })
        .await
}

/// 把一份譯本輸出成 Markdown：只有譯文；`bilingual` 為真是原文與譯文逐段對照。
#[tauri::command]
pub async fn export_translation(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_id: String,
    material_id: String,
    bilingual: Option<bool>,
) -> ApiResult<ExportFile> {
    let dir = exports_dir(&app).await?;
    state
        .run("export_translation", move |s| {
            let (doc, title) = s.with_lib(|l| {
                let (doc, _) = l.read_overlay(&paper_id, &material_id)?;
                Ok((doc, paper_title(l, &paper_id)?))
            })?;
            let stem = file_stem(&title, "paper");
            let (md, name) = if bilingual.unwrap_or(false) {
                (
                    translation_markdown(&title, &doc),
                    format!("{stem}-譯本對照.md"),
                )
            } else {
                (
                    translation_text_markdown(&title, &doc),
                    format!("{stem}-譯文.md"),
                )
            };
            write_file(&dir, &name, md.as_bytes())
        })
        .await
}

/// 把幾篇的書目一次寫成一份 BibTeX、RIS 或 CSL-JSON 檔。`format` 是 `bibtex`、`ris` 或 `csljson`；`name` 是檔名的前半
/// （群組的名稱，沒有就叫「書目」）。順序照 `paper_ids`。
#[tauri::command]
pub async fn export_bibliography_file(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_ids: Vec<String>,
    format: String,
    name: Option<String>,
) -> ApiResult<BibExport> {
    // CSL-JSON 由外殼寫（`citation.rs`），BibTeX、RIS 由核心寫
    let fmt = match format.as_str() {
        "csljson" => None,
        f => Some(
            BibFormat::from_name(f)
                .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "unknown format"))?,
        ),
    };
    let dir = exports_dir(&app).await?;
    state
        .run("export_bibliography_file", move |s| {
            let bibs = s.with_lib(|l| {
                Ok(paper_ids
                    .iter()
                    .filter_map(|id| l.get_paper(id).ok().map(|(p, _)| p.bib))
                    .collect::<Vec<_>>())
            })?;
            let refs: Vec<_> = bibs.iter().collect();
            let (text, ext) = match fmt {
                Some(f) => (export_bibliography(&refs, f), f.extension()),
                None => (csl_json(&refs), "json"),
            };
            let stem = file_stem(name.as_deref().unwrap_or(""), "書目");
            let file = write_file(&dir, &format!("{stem}.{ext}"), text.as_bytes())?;
            Ok(BibExport {
                file,
                count: bibs.len(),
                skipped: paper_ids.len() - bibs.len(),
            })
        })
        .await
}

/// 缺作者或年份的一篇（引用格式會略過那一段；畫面列出來，點了去補）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingBib {
    pub id: String,
    pub title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationExport {
    /// `copy` 時沒有檔案。
    pub file: Option<ExportFile>,
    pub count: usize,
    pub skipped: usize,
    pub missing: Vec<MissingBib>,
}

/// 幾篇的書目排成引用格式（`style`：`apa`、`ieee`、`chicago`、`mla`）。`action` 是 `copy`（純文字
/// 加 HTML 放到剪貼簿）或 `file`（寫成一份 HTML 檔，貼進文書軟體保留斜體）。順序照 `paper_ids`。
#[tauri::command]
pub async fn export_citations(
    app: AppHandle,
    state: State<'_, AppState>,
    paper_ids: Vec<String>,
    style: String,
    action: String,
    name: Option<String>,
) -> ApiResult<CitationExport> {
    let Some(style) = CitationStyle::from_name(&style) else {
        return Err(ApiError::new(ErrorKind::InvalidInput, "unknown style"));
    };
    let (rendered, count, skipped, missing) = state
        .run("export_citations", move |s| {
            let papers = s.with_lib(|l| {
                Ok(paper_ids
                    .iter()
                    .filter_map(|id| {
                        let (p, _) = l.get_paper(id).ok()?;
                        Some((id.clone(), title_of(&p), p.bib))
                    })
                    .collect::<Vec<_>>())
            })?;
            let missing = papers
                .iter()
                .filter(|(_, _, b)| b.year.is_none() || !has_author(b))
                .map(|(id, title, _)| MissingBib {
                    id: id.clone(),
                    title: title.clone(),
                })
                .collect::<Vec<_>>();
            let refs: Vec<_> = papers.iter().map(|(_, _, b)| b).collect();
            let rendered = render_citations(&refs, style);
            Ok((
                rendered,
                papers.len(),
                paper_ids.len() - papers.len(),
                missing,
            ))
        })
        .await?;
    let file = if action == "copy" {
        app.paperreader()
            .copy_text(&rendered.text(), Some(&rendered.html()))
            .await?;
        None
    } else {
        let dir = exports_dir(&app).await?;
        let stem = file_stem(name.as_deref().unwrap_or(""), "引用清單");
        let file_name = format!("{stem}-{}.html", style.label());
        let html = rendered.html_document(&stem);
        Some(
            blocking(move || write_file(&dir, &file_name, html.as_bytes()))
                .await
                .map_err(|e| e.log("export_citations"))?,
        )
    };
    Ok(CitationExport {
        file,
        count,
        skipped,
        missing,
    })
}

/// 百分比編碼的檔名（HTTP 標頭只能放 ASCII）解回來。
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 畫面組好的檔案（帶標註的 PDF）放進暫存資料夾。位元組在請求本文，檔名（百分比編碼）在標頭 `export-name`。
#[tauri::command]
pub async fn export_put_bytes(app: AppHandle, request: Request<'_>) -> ApiResult<ExportFile> {
    let name = request
        .headers()
        .get("export-name")
        .and_then(|v| v.to_str().ok())
        .and_then(percent_decode)
        .unwrap_or_default();
    let bytes: Vec<u8> = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        InvokeBody::Json(v) => serde_json::from_value(v.clone())
            .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "no body"))?,
    };
    if bytes.is_empty() {
        return Err(ApiError::new(ErrorKind::InvalidInput, "empty file"));
    }
    let dir = exports_dir(&app).await?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !e.is_empty() && e.len() <= 5 => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let name = format!("{}{ext}", file_stem(&stem, "export"));
    blocking(move || write_file(&dir, &name, &bytes))
        .await
        .map_err(|e| e.log("export_put_bytes"))
}

fn mime_of(name: &str, share: bool) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "md" => "text/markdown",
        // 分享給郵件、通訊軟體等，純文字最通用；存到…則用各自的種類，免得系統硬加 .txt
        "bib" if share => "text/plain",
        "ris" if share => "text/plain",
        "bib" => "application/x-bibtex",
        "ris" => "application/x-research-info-systems",
        "html" => "text/html",
        "json" if share => "text/plain",
        "json" => "application/json",
        // 備份檔與同步包（外掛不給種類時用的也是這個）
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// 存好之後要刪掉的暫存：匯出資料夾裡的備份檔與同步包（zip，檔大；面板存好就換成「已存好」，不會再用）。
/// 其他匯出存好之後面板還能分享、列印，留著（一小時後清）。取消、分享時都留著：分享畫面要等對方應用讀完。
fn drop_after_save(path: &Path, exports: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        && path.parent() == Some(exports)
}

/// 把準備好的檔案交給使用者：`share` 開分享畫面、`save` 開「存到…」、`print` 開列印畫面（只限 PDF）。
/// 交回 `shared`、`saved`、`cancelled` 或 `printing`。備份檔、同步包、一般匯出都由這裡交出。
#[tauri::command]
pub async fn deliver_export(
    app: AppHandle,
    path: String,
    name: String,
    action: String,
) -> ApiResult<String> {
    match action.as_str() {
        "share" | "save" => {
            let share = action == "share";
            let result = app
                .paperreader()
                .export_file(&path, &name, share, Some(mime_of(&name, share)))
                .await?;
            // 已經交出去了：之後查不到暫存的位置只是不刪（一小時後照樣清），不回報失敗
            if result == "saved"
                && let Ok(info) = app.paperreader().storage_info().await
                && drop_after_save(Path::new(&path), &exports_path(Path::new(&info.cache_dir)))
            {
                let _ = std::fs::remove_file(&path);
            }
            Ok(result)
        }
        "print" => {
            app.paperreader().print_file(&path, &name).await?;
            Ok("printing".to_string())
        }
        _ => Err(ApiError::new(ErrorKind::InvalidInput, "unknown action")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 備份檔、同步包、一般匯出都用同一個清理，只刪一小時前留下的（檔與資料夾）；
    /// 剛做好、可能正在分享的不動。
    #[test]
    fn only_exports_older_than_an_hour_are_cleaned() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("exports");
        std::fs::create_dir_all(dir.join("work-1")).unwrap();
        std::fs::write(dir.join("work-1").join("p.zip"), "x").unwrap();
        std::fs::write(dir.join("PaperReader-backup.zip"), "x").unwrap();
        clean_exports(&dir, SystemTime::now()).unwrap();
        assert!(dir.join("PaperReader-backup.zip").is_file());
        assert!(dir.join("work-1").join("p.zip").is_file());
        // 兩個小時之後：都是一小時前的，連資料夾一起刪；資料夾本身留著
        clean_exports(&dir, SystemTime::now() + Duration::from_secs(7200)).unwrap();
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        // 不存在時建立
        let fresh = tmp.path().join("other");
        clean_exports(&fresh, SystemTime::now()).unwrap();
        assert!(fresh.is_dir());
    }

    #[test]
    fn file_names_lose_path_separators_and_stay_short() {
        assert_eq!(file_stem("a/b\\c:d", "x"), "a_b_c_d");
        assert_eq!(file_stem("  ..  ", "paper"), "paper");
        assert_eq!(file_stem("", "paper"), "paper");
        assert_eq!(file_stem(&"長".repeat(200), "x").chars().count(), 80);
        assert_eq!(file_stem("深度學習：綜述", "x"), "深度學習：綜述");
    }

    #[test]
    fn percent_decoding_restores_utf8_names() {
        assert_eq!(percent_decode("%E6%B7%B1.pdf").as_deref(), Some("深.pdf"));
        assert_eq!(percent_decode("a%2").as_deref(), None);
    }

    #[test]
    fn text_exports_share_as_plain_text_but_save_with_their_own_type() {
        assert_eq!(mime_of("x.bib", true), "text/plain");
        assert_eq!(mime_of("x.bib", false), "application/x-bibtex");
        assert_eq!(mime_of("x.pdf", true), "application/pdf");
        assert_eq!(mime_of("x.md", false), "text/markdown");
        // 備份檔與同步包：和外掛沒拿到種類時用的一樣
        assert_eq!(
            mime_of("PaperReader-backup-1.zip", false),
            "application/zip"
        );
        assert_eq!(mime_of("PaperReader-sync-1.ZIP", true), "application/zip");
    }

    /// 存好就刪的只有匯出資料夾裡的 zip（備份檔、同步包）；其他匯出存完面板還能分享，別處的檔一律不刪。
    #[test]
    fn only_zips_in_the_exports_folder_are_dropped_after_saving() {
        let exports = Path::new("/cache/exports");
        assert!(drop_after_save(
            Path::new("/cache/exports/PaperReader-backup-1.zip"),
            exports
        ));
        assert!(!drop_after_save(
            Path::new("/cache/exports/paper.pdf"),
            exports
        ));
        assert!(!drop_after_save(
            Path::new("/cache/exports/work-1/p.zip"),
            exports
        ));
        assert!(!drop_after_save(
            Path::new("/storage/Documents/PaperReader/x.zip"),
            exports
        ));
    }
}
