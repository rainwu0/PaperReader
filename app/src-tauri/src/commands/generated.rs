//! 產生的譯本 PDF：把系統的中日韓字型唯讀交給畫面，並把畫面組好的譯本 PDF 交給核心存成材料。
//!
//! 字型不打包進應用：用系統裡的中日韓字型，畫面只把用到的字嵌進譯本 PDF。Android 內建的 Noto CJK
//! 在 `/system/fonts/`；電腦依作業系統列字型資料夾，而且只用授權允許嵌入的字型（OS/2 表的 `fsType`）。
//! 資料規則（檔名、版本記號、原地更新）都在核心的 `put_generated_translation`。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::{AppState, blocking};
use serde::Serialize;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tauri::State;
use tauri::ipc::{InvokeBody, Request, Response};

/// 系統的字型資料夾。
fn font_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "android")]
    {
        vec![PathBuf::from("/system/fonts")]
    }
    #[cfg(windows)]
    {
        // 全部使用者的字型，與只裝給目前使用者的字型
        let mut dirs = Vec::new();
        if let Some(w) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
            dirs.push(PathBuf::from(w).join("Fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(l)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        dirs
    }
    // 其他電腦作業系統（macOS、Linux）：列常見的位置
    #[cfg(all(not(target_os = "android"), not(windows)))]
    {
        let mut dirs: Vec<PathBuf> = [
            "/System/Library/Fonts",
            "/Library/Fonts",
            "/usr/share/fonts/opentype/noto",
            "/usr/share/fonts/noto-cjk",
            "/usr/share/fonts/google-noto-cjk",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        if let Some(h) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(&h).join("Library").join("Fonts"));
            dirs.push(PathBuf::from(&h).join(".local").join("share").join("fonts"));
        }
        dirs
    }
}

/// 這個字型檔適不適合當譯文的字型：越小越好，`None` 不考慮。
/// Noto Sans CJK 優先；其次是別的 Noto 與中日韓字型；最後是 Windows 內建的微軟正黑體、微軟雅黑體
/// （Windows 11 內建的 Noto Sans TC 是 TrueType 的可變字型，`usable_font` 不收，所以 Windows 上沒另裝 Noto 時用的是它們）。
/// 只收 `.ttc`、`.otf`、`.ttf`。
fn font_rank(file_name: &str) -> Option<u32> {
    let n = file_name.to_ascii_lowercase();
    let ok_ext = [".ttc", ".otf", ".ttf"].iter().any(|e| n.ends_with(e));
    if !ok_ext {
        return None;
    }
    let base = if n.contains("notosanscjk") {
        0
    } else if n.contains("notosanstc") || n.contains("notosanshant") {
        10
    } else if n.contains("notosanshk") {
        12
    } else if n.contains("notosanssc") || n.contains("notosanshans") {
        14
    } else if n.contains("sourcehan") {
        20
    } else if n.contains("notoserifcjk") {
        30
    } else if n.contains("notoseriftc") {
        32
    } else if n.contains("droidsansfallback") {
        40
    } else if n.contains("misans") || n.contains("milan") {
        50
    } else if n.starts_with("msjh") {
        60
    } else if n.starts_with("msyh") {
        65
    } else {
        return None;
    };
    // 同一種裡，名稱短的（Regular）優先，粗體等排後面
    Some(base + u32::from(n.contains("bold")) * 5 + u32::from(n.contains("light")) * 3)
}

/// 字型資料夾裡的中日韓字型，好的在前（同等的名稱短的在前）。
fn font_candidates(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<(u32, usize, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rank) = font_rank(&name) {
                found.push((rank, name.len(), e.path()));
            }
        }
    }
    found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    found.into_iter().map(|(_, _, p)| p).collect()
}

/// 在字型資料夾裡挑一個中日韓字型（第一個 `usable` 的）。
pub(crate) fn pick_font(dirs: &[PathBuf], usable: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    font_candidates(dirs).into_iter().find(|p| usable(p))
}

/// OS/2 表的 `fsType` 允不允許把字嵌進 PDF。我們只嵌用到的字（取子集），所以：
/// 只標「受限授權」（0x0002，沒有同時標 0x0004 或 0x0008）的不行、標了不准取子集（0x0100）或只准嵌點陣（0x0200）的不行；
/// 0（可安裝）、0x0004（預覽與列印）、0x0008（可編輯）可以。
fn fs_type_allows_embedding(fs_type: u16) -> bool {
    let restricted = fs_type & 0x000E == 0x0002;
    !restricted && fs_type & 0x0300 == 0
}

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// 一個字面（從 `face` 起的 sfnt 表目錄）的表：標籤與在檔案裡的位置。
fn face_tables(data: &[u8], face: usize) -> Vec<([u8; 4], usize)> {
    let Some(n) = be_u16(data, face + 4) else {
        return Vec::new();
    };
    (0..usize::from(n))
        .filter_map(|i| {
            let rec = face + 12 + i * 16;
            let tag: [u8; 4] = data.get(rec..rec + 4)?.try_into().ok()?;
            let offset = usize::try_from(be_u32(data, rec + 8)?).ok()?;
            Some((tag, offset))
        })
        .collect()
}

/// 字型檔裡每個字面的起點（`.ttc` 有好幾個）。
fn font_faces(data: &[u8]) -> Vec<usize> {
    if data.get(0..4) == Some(b"ttcf") {
        let n = be_u32(data, 8).unwrap_or(0) as usize;
        (0..n.min(64))
            .filter_map(|i| be_u32(data, 12 + i * 4).and_then(|o| usize::try_from(o).ok()))
            .collect()
    } else {
        vec![0]
    }
}

/// 一個字面能不能用：OS/2 的 `fsType` 允許嵌入，而且不是 TrueType 外框（`glyf`）的可變字型（有 `fvar`）。
/// 後者畫面用的字型子集套件（fontkit）取不好：Windows 11 的 `NotoSansTC-VF.ttf` 嵌進譯本後大半的字沒有外框，
/// 字重也變成預設的 Thin。CFF2 的可變字型由畫面自己轉（`cff2.ts`），照收。
fn face_usable(data: &[u8], face: usize) -> bool {
    let tables = face_tables(data, face);
    let has = |t: &[u8; 4]| tables.iter().any(|(tag, _)| tag == t);
    let embeddable = tables
        .iter()
        .find(|(tag, _)| tag == b"OS/2")
        .and_then(|(_, at)| be_u16(data, at + 8))
        .is_some_and(fs_type_allows_embedding);
    embeddable && !(has(b"fvar") && has(b"glyf"))
}

/// 字型檔的每個字面都能用（`.ttc` 逐一看；畫面依語言挑其中一個字面）。讀不懂的不用。
fn embedding_allowed(data: &[u8]) -> bool {
    let faces = font_faces(data);
    !faces.is_empty() && faces.iter().all(|&f| face_usable(data, f))
}

/// 這份字型檔可不可以嵌進譯本 PDF。Android 內建的字型都是 SIL OFL，照原本的做法不另外讀。
fn usable_font(path: &Path) -> bool {
    if cfg!(target_os = "android") {
        return true;
    }
    std::fs::read(path).is_ok_and(|data| embedding_allowed(&data))
}

/// 系統內建的中日韓字型檔（整個檔案的位元組）。找不到時是 `NoFont`，畫面說明要裝什麼字型並停下來。
#[tauri::command]
pub async fn system_cjk_font() -> ApiResult<Response> {
    blocking(|| {
        let path = pick_font(&font_dirs(), usable_font).ok_or_else(|| {
            ApiError::new(
                ErrorKind::NoFont,
                "no embeddable CJK font in the system font folders; install Noto Sans CJK or Noto Sans TC",
            )
        })?;
        let bytes = std::fs::read(&path)
            .map_err(|e| ApiError::new(ErrorKind::NoFont, format!("cannot read the font: {e}")))?;
        Ok(Response::new(bytes))
    })
    .await
}

/// 把原文 PDF 裡落在蓋住範圍內的文字指令刪掉，交回新檔。
/// 請求本文：四個位元組（大端）的 JSON 長度、JSON（`{"頁序": [[x0,y0,x1,y1], …]}`）、原文 PDF 的位元組。
/// 回應：四個位元組（大端）刪掉的指令數、新檔的位元組。讀不了（加密等）是 `Unreadable`，畫面保留原檔、只靠文字層過濾。
#[tauri::command]
pub async fn pdf_strip_text(request: Request<'_>) -> ApiResult<Response> {
    let body: Vec<u8> = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        InvokeBody::Json(v) => serde_json::from_value(v.clone())
            .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "no body"))?,
    };
    blocking(move || {
        let bad = || ApiError::new(ErrorKind::InvalidInput, "bad request body");
        let len = u32::from_be_bytes(
            body.get(..4)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ) as usize;
        let json = body.get(4..4 + len).ok_or_else(bad)?;
        let pdf = body.get(4 + len..).ok_or_else(bad)?;
        let raw: std::collections::BTreeMap<String, Vec<crate::pdfstrip::Rect>> =
            serde_json::from_slice(json).map_err(|_| bad())?;
        let covers = raw
            .into_iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|i| (i, v)))
            .collect();
        let (out, removed) = crate::pdfstrip::strip_covered_text(pdf, &covers)
            .map_err(|_| ApiError::new(ErrorKind::Unreadable, "cannot strip the PDF"))?;
        let mut resp = u32::try_from(removed)
            .unwrap_or(u32::MAX)
            .to_be_bytes()
            .to_vec();
        resp.extend_from_slice(&out);
        Ok(Response::new(resp))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPut {
    pub material_id: String,
    /// `created`、`updated`、`unchanged`。
    pub outcome: &'static str,
}

fn header(request: &Request<'_>, name: &str) -> String {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// 把畫面組好的譯本 PDF 存成這篇的譯本材料（第一次新增，之後原地更新）。
/// 位元組在請求本文；標頭 `paper-id`、`overlay-id`（由哪一份譯文疊加檔產生）、`pages`（頁數）。
#[tauri::command]
pub async fn translation_pdf_put(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> ApiResult<GeneratedPut> {
    let paper_id = header(&request, "paper-id");
    let overlay_id = header(&request, "overlay-id");
    let pages = header(&request, "pages").parse::<u32>().ok();
    let bytes: Vec<u8> = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        InvokeBody::Json(v) => serde_json::from_value(v.clone())
            .map_err(|_| ApiError::new(ErrorKind::InvalidInput, "no body"))?,
    };
    state
        .lib("translation_pdf_put", move |l| {
            let (_, token) = l.get_paper(&paper_id)?;
            let r = l.put_generated_translation(
                &paper_id,
                &overlay_id,
                &mut Cursor::new(bytes),
                pages,
                &token,
            )?;
            use paperreader_core::store::PutGenerated as P;
            Ok(match r {
                P::Created { material_id, .. } => GeneratedPut {
                    material_id,
                    outcome: "created",
                },
                P::Updated { material_id, .. } => GeneratedPut {
                    material_id,
                    outcome: "updated",
                },
                P::Unchanged { material_id } => GeneratedPut {
                    material_id,
                    outcome: "unchanged",
                },
            })
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(names: &[&str]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        d
    }

    /// 只看檔名（Android 的做法）。
    fn pick_in(d: &tempfile::TempDir) -> Option<PathBuf> {
        pick_font(&[d.path().to_path_buf()], |_| true)
    }

    #[test]
    fn noto_sans_cjk_is_preferred() {
        let d = dir_with(&[
            "Roboto-Regular.ttf",
            "NotoSerifCJK-Regular.ttc",
            "NotoSansCJK-Regular.ttc",
            "DroidSansFallback.ttf",
        ]);
        let p = pick_in(&d).unwrap();
        assert_eq!(p.file_name().unwrap(), "NotoSansCJK-Regular.ttc");
    }

    #[test]
    fn regular_beats_bold_and_other_cjk_fonts_are_fallbacks() {
        let d = dir_with(&["NotoSansCJK-Bold.ttc", "NotoSansCJK-Regular.ttc"]);
        assert_eq!(
            pick_in(&d).unwrap().file_name().unwrap(),
            "NotoSansCJK-Regular.ttc"
        );
        let d = dir_with(&["Roboto-Regular.ttf", "SourceHanSansTC-Regular.otf"]);
        assert_eq!(
            pick_in(&d).unwrap().file_name().unwrap(),
            "SourceHanSansTC-Regular.otf"
        );
    }

    #[test]
    fn no_cjk_font_means_none() {
        let d = dir_with(&[
            "Roboto-Regular.ttf",
            "NotoSansArabic-Regular.ttf",
            "readme.txt",
        ]);
        assert!(pick_in(&d).is_none());
        assert!(pick_font(&[PathBuf::from("/no/such/folder")], |_| true).is_none());
    }

    #[test]
    fn windows_fonts_rank_noto_first_then_microsoft_fonts() {
        // 這台 Windows 11 的字型資料夾裡有的
        let d = dir_with(&[
            "msjh.ttc",
            "msjhbd.ttc",
            "msyh.ttc",
            "NotoSerifTC-VF.ttf",
            "NotoSansHK-VF.ttf",
            "NotoSansTC-VF.ttf",
            "mingliu.ttc",
        ]);
        let order: Vec<String> = font_candidates(&[d.path().to_path_buf()])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            order,
            [
                "NotoSansTC-VF.ttf",
                "NotoSansHK-VF.ttf",
                "NotoSerifTC-VF.ttf",
                "msjh.ttc",
                "msjhbd.ttc",
                "msyh.ttc"
            ]
        );
    }

    #[test]
    fn folders_are_searched_together_and_unusable_fonts_are_skipped() {
        let a = dir_with(&["NotoSansTC-VF.ttf"]);
        let b = dir_with(&["NotoSansCJK-Regular.ttc", "msjh.ttc"]);
        let dirs = [a.path().to_path_buf(), b.path().to_path_buf()];
        // 使用者另外裝的 Noto Sans CJK 勝過系統的 Noto Sans TC
        assert_eq!(
            pick_font(&dirs, |_| true).unwrap().file_name().unwrap(),
            "NotoSansCJK-Regular.ttc"
        );
        // 前面的不准嵌入就往下找
        let not_noto = |p: &Path| !p.to_string_lossy().contains("Noto");
        assert_eq!(
            pick_font(&dirs, not_noto).unwrap().file_name().unwrap(),
            "msjh.ttc"
        );
        assert!(pick_font(&dirs, |_| false).is_none());
    }

    /// 最小的 sfnt 字面，從檔案的第 `base` 個位元組開始：一張 OS/2 表（`fsType` 在表的第 8 個位元組），
    /// 加上 `extra` 列的表（只有標籤，內容不看）。表的位置從檔案開頭算。
    fn face_at(fs_type: u16, extra: &[&[u8; 4]], base: usize) -> Vec<u8> {
        let n = 1 + extra.len();
        let data_at = (base + 12 + n * 16) as u32;
        let mut b = vec![0x00, 0x01, 0x00, 0x00];
        b.extend_from_slice(&(n as u16).to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        for tag in std::iter::once(b"OS/2").chain(extra.iter().copied()) {
            b.extend_from_slice(tag);
            b.extend_from_slice(&[0, 0, 0, 0]);
            b.extend_from_slice(&data_at.to_be_bytes());
            b.extend_from_slice(&10u32.to_be_bytes());
        }
        b.extend_from_slice(&[0, 4, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&fs_type.to_be_bytes());
        b
    }

    fn sfnt(fs_type: u16) -> Vec<u8> {
        face_at(fs_type, &[], 0)
    }

    /// 兩個字面的 `.ttc`。
    fn ttc(a: u16, b: u16) -> Vec<u8> {
        let head = 12 + 8;
        let fa = face_at(a, &[], head);
        let fb = face_at(b, &[], head + fa.len());
        let mut out = b"ttcf".to_vec();
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(head as u32).to_be_bytes());
        out.extend_from_slice(&((head + fa.len()) as u32).to_be_bytes());
        out.extend(fa);
        out.extend(fb);
        out
    }

    #[test]
    fn embedding_follows_the_os2_fs_type() {
        assert!(embedding_allowed(&sfnt(0x0000))); // 可安裝（Noto 等 OFL 字型）
        assert!(embedding_allowed(&sfnt(0x0004))); // 預覽與列印
        assert!(embedding_allowed(&sfnt(0x0008))); // 可編輯
        assert!(!embedding_allowed(&sfnt(0x0002))); // 受限授權
        assert!(embedding_allowed(&sfnt(0x0006))); // 舊字型同時標兩種：取寬的
        assert!(!embedding_allowed(&sfnt(0x0100))); // 不准取子集
        assert!(!embedding_allowed(&sfnt(0x0208))); // 只准嵌點陣
        assert!(!embedding_allowed(b"not a font"));
        assert!(!embedding_allowed(&[]));
        assert!(embedding_allowed(&ttc(0x0008, 0x0000)));
        assert!(!embedding_allowed(&ttc(0x0008, 0x0002)));
    }

    #[test]
    fn truetype_variable_fonts_are_not_used_but_cff2_ones_are() {
        // 一般的 TrueType 與 CFF 字型
        assert!(embedding_allowed(&face_at(0, &[b"glyf"], 0)));
        assert!(embedding_allowed(&face_at(0, &[b"CFF "], 0)));
        // TrueType 外框的可變字型（Windows 11 的 NotoSansTC-VF.ttf）：子集取不好，不用
        assert!(!embedding_allowed(&face_at(0, &[b"glyf", b"fvar"], 0)));
        // CFF2 的可變字型（Android 16 的 Noto Sans CJK）：畫面自己轉，照收
        assert!(embedding_allowed(&face_at(0, &[b"CFF2", b"fvar"], 0)));
    }

    /// 這台電腦上真的字型（只在 Windows 跑）：Noto Sans TC 是 TrueType 的可變字型，不用；
    /// 挑到的是允許嵌入的固定字型（這台是微軟正黑體）。
    #[cfg(windows)]
    #[test]
    fn on_this_windows_a_static_embeddable_font_is_picked() {
        if let Some(vf) = font_dirs()
            .into_iter()
            .map(|d| d.join("NotoSansTC-VF.ttf"))
            .find(|p| p.is_file())
        {
            assert!(!usable_font(&vf));
        }
        let Some(picked) = pick_font(&font_dirs(), usable_font) else {
            return;
        };
        let name = picked
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        assert!(!name.contains("-vf"), "{name}");
        assert!(usable_font(&picked));
    }
}
