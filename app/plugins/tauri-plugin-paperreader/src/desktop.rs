//! 電腦的平台轉接器：和 Android 同一組方法、同樣的結果。
//!
//! - 共用空間：系統「文件」已知資料夾底下的 `PaperReader`，不需要權限（`all_files_access` 恆為真，
//!   畫面因此不出現授權那一步）。專屬空間：應用的本機資料夾底下的 `PaperReader`。暫存：應用的快取資料夾
//!   （和 `src-tauri` 啟動時清理用的是同一個 `app_cache_dir`）。
//! - 選檔、選資料夾、存檔：Tauri 官方 dialog 外掛的系統對話框。選到的 PDF 直接交原路徑（核心加入時只讀它、
//!   自己複製一份；外殼的 `remove_pick` 只刪暫存資料夾底下的，碰不到原檔）。其他種類的檔（備份檔、同步包、
//!   術語表）會被呼叫端改名或刪掉，所以先複製到 `<暫存>/picks/<隨機>/<檔名>`，和 Android 一樣。
//! - 開網址、開筆記檔、列印：官方 opener 外掛交給系統預設的程式（列印由那個 PDF 程式做）。
//! - 金鑰與剪貼簿：Windows 在 `win.rs`；其他電腦作業系統不支援，交回 [`Error::Unsupported`]。
//! - 沒有的事：權限頁（`Unsupported`，畫面不會叫到）、分享進來與連結（空的，件二做）、
//!   螢幕不關與系統列明暗（不做事）、螢幕邊界（零）、網路（視為在線、不計流量）。

use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

/// 除錯版可以用這個環境變數把「文件」指到別處（測試時不碰使用者真正的「文件」資料夾）。正式版不讀它。
#[cfg(debug_assertions)]
const DOCUMENTS_OVERRIDE: &str = "PAPERREADER_DOCUMENTS_DIR";

fn platform(e: impl std::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

/// 複製或寫入時空間不足要讓畫面說「空間不足」；其餘一律是平台的問題。
fn io_failure(e: &std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::StorageFull {
        Error::NoSpace
    } else {
        Error::Platform(e.kind().to_string())
    }
}

/// 檔名：去掉路徑分隔與空字元；空的或只有點時用 `fallback`（同 Kotlin 的 `safeName`）。
fn safe_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

/// `path` 在 `root` 底下（兩者都換成實際路徑再比，`..` 與捷徑繞不出去）。不存在的一律算不在。
fn is_inside(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(p), Ok(r)) => p != r && p.starts_with(&r),
        _ => false,
    }
}

/// 暫存資料夾底下每次不同的資料夾名稱（同一個行程裡用計數器、不同行程靠時間與行程編號區分）。
fn unique_folder() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{nanos:x}-{:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// 一份選到的檔，交原路徑（PDF）。
fn picked_in_place(path: PathBuf) -> PickedPdf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = std::fs::metadata(&path).ok().map(|m| m.len());
    PickedPdf {
        name,
        path: Some(path.display().to_string()),
        bytes,
        error: None,
    }
}

/// 一份選到的檔，先複製到 `<picks>/<隨機>/<檔名>` 再交出（呼叫端會改名或刪掉它）。
/// 複製失敗時帶 `error`（`no_space` 或 `failed`），和 Android 的結果同一種形式。
fn picked_copy(path: &Path, picks: &Path) -> PickedPdf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = picks.join(unique_folder());
    let dest = dir.join(safe_name(&name, "chosen"));
    let copied = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(path, &dest));
    match copied {
        Ok(bytes) => PickedPdf {
            name,
            path: Some(dest.display().to_string()),
            bytes: Some(bytes),
            error: None,
        },
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            let error = if e.kind() == std::io::ErrorKind::StorageFull {
                "no_space"
            } else {
                "failed"
            };
            PickedPdf {
                name,
                path: None,
                bytes: None,
                error: Some(error.to_string()),
            }
        }
    }
}

/// 把文字放進網址的一段（`obsidian://open?vault=…&file=…`）：英數字與 `-._~` 以外的位元組都編碼。
fn url_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 筆記檔的 Obsidian 連結：保存庫名稱取文獻庫資料夾的名稱，檔案取從文獻庫算起的路徑（`notes/<檔名>`），
/// 和 Android 的做法相同。筆記檔不在 `<文獻庫>/notes/` 底下時只給檔名。
fn obsidian_link(file: &Path) -> String {
    let library = file.parent().and_then(Path::parent);
    let vault = library
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rel = library
        .and_then(|lib| file.strip_prefix(lib).ok())
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|| {
            file.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    format!(
        "obsidian://open?vault={}&file={}",
        url_component(&vault),
        url_component(&rel)
    )
}

/// 只開 `https://` 而且有主機的網址（同 Kotlin 的 `openUrl`）。
fn is_web_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        !host.is_empty() && !url.chars().any(char::is_whitespace)
    })
}

impl<R: Runtime> PaperReader<R> {
    /// 系統「文件」已知資料夾（除錯版可用環境變數改指別處）。
    fn documents_dir(&self) -> Result<PathBuf> {
        #[cfg(debug_assertions)]
        if let Some(dir) = std::env::var_os(DOCUMENTS_OVERRIDE).filter(|d| !d.is_empty()) {
            return Ok(PathBuf::from(dir));
        }
        self.0.path().document_dir().map_err(platform)
    }

    fn cache_dir(&self) -> Result<PathBuf> {
        self.0.path().app_cache_dir().map_err(platform)
    }

    fn private_dir(&self) -> Result<PathBuf> {
        Ok(self
            .0
            .path()
            .app_local_data_dir()
            .map_err(platform)?
            .join("PaperReader"))
    }

    /// 對話框：掛在主視窗上（開著時主視窗不能操作）。除錯版改指「文件」時，對話框也從那裡開始。
    fn dialog(&self) -> FileDialogBuilder<R> {
        let mut d = self.0.dialog().file();
        if let Some(w) = self.0.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        #[cfg(debug_assertions)]
        if let Some(dir) = std::env::var_os(DOCUMENTS_OVERRIDE).filter(|d| !d.is_empty()) {
            d = d.set_directory(PathBuf::from(dir));
        }
        d
    }

    /// 在背景執行緒開對話框、等它關掉（對話框本身會被排到主執行緒）。
    async fn run_dialog<T: Send + 'static>(
        &self,
        f: impl FnOnce(FileDialogBuilder<R>) -> T + Send + 'static,
    ) -> Result<T> {
        let d = self.dialog();
        tauri::async_runtime::spawn_blocking(move || f(d))
            .await
            .map_err(platform)
    }

    /// 選檔對話框交回的路徑（取消是空的）。
    async fn pick_paths(&self, pdf_only: bool, multiple: bool) -> Result<Vec<PathBuf>> {
        let picked = self
            .run_dialog(move |d| {
                let d = if pdf_only {
                    d.add_filter("PDF", &["pdf"])
                } else {
                    d
                };
                if multiple {
                    d.blocking_pick_files().unwrap_or_default()
                } else {
                    d.blocking_pick_file().into_iter().collect()
                }
            })
            .await?;
        Ok(picked
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .collect())
    }

    pub async fn storage_info(&self) -> Result<StorageInfo> {
        Ok(StorageInfo {
            sdk: 0,
            all_files_access: true,
            shared_dir: self
                .documents_dir()?
                .join("PaperReader")
                .display()
                .to_string(),
            private_dir: self.private_dir()?.display().to_string(),
            cache_dir: self.cache_dir()?.display().to_string(),
        })
    }

    /// 電腦沒有「所有檔案存取」的授權頁（畫面看到 `all_files_access` 為真就不會叫它）。
    pub async fn open_all_files_settings(&self) -> Result<()> {
        Err(Error::Unsupported)
    }

    /// 選檔對話框，可以一次選多份 PDF。交原路徑；取消交回空清單。
    pub async fn pick_pdfs(&self) -> Result<Vec<PickedPdf>> {
        let paths = self.pick_paths(true, true).await?;
        Ok(paths.into_iter().map(picked_in_place).collect())
    }

    /// 只選一份 PDF（加入譯本用）。交原路徑。
    pub async fn pick_one_pdf(&self) -> Result<Vec<PickedPdf>> {
        let paths = self.pick_paths(true, false).await?;
        Ok(paths.into_iter().map(picked_in_place).collect())
    }

    /// 選一份任何種類的檔（備份檔、同步包、術語表），先複製到暫存再交出。
    pub async fn pick_any_file(&self) -> Result<Vec<PickedPdf>> {
        let paths = self.pick_paths(false, false).await?;
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let picks = self.cache_dir()?.join("picks");
        tauri::async_runtime::spawn_blocking(move || {
            paths.iter().map(|p| picked_copy(p, &picks)).collect()
        })
        .await
        .map_err(platform)
    }

    /// 選資料夾（整批匯入）。電腦上任何本機資料夾都收。
    pub async fn pick_folder(&self) -> Result<PickedFolder> {
        let picked = self.run_dialog(|d| d.blocking_pick_folder()).await?;
        Ok(match picked.and_then(|p| p.into_path().ok()) {
            Some(p) => PickedFolder::Path(p.display().to_string()),
            None => PickedFolder::Cancelled,
        })
    }

    /// 把暫存資料夾裡的一個檔交給使用者：開「存到…」對話框，複製過去。電腦沒有系統的分享畫面，
    /// `share` 也開「存到…」。交回 `saved` 或 `cancelled`。
    pub async fn export_file(
        &self,
        path: &str,
        name: &str,
        _share: bool,
        _mime: Option<&str>,
    ) -> Result<String> {
        let src = PathBuf::from(path);
        let cache = self.cache_dir()?;
        let private = self.private_dir()?;
        // 只交出應用暫存資料夾裡的檔（專屬空間的文獻庫也在應用的資料夾裡，不算）
        if !src.is_file() || !is_inside(&src, &cache) || is_inside(&src, &private) {
            return Err(Error::Platform("not an exportable file".into()));
        }
        let file_name = safe_name(name, "export");
        let ext = Path::new(&file_name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| !e.is_empty());
        let target = self
            .run_dialog(move |d| {
                let d = d.set_file_name(file_name);
                let d = match &ext {
                    Some(e) => d.add_filter(e.to_ascii_uppercase(), &[e.as_str()]),
                    None => d,
                };
                d.blocking_save_file()
            })
            .await?;
        let Some(target) = target.and_then(|p| p.into_path().ok()) else {
            return Ok("cancelled".into());
        };
        tauri::async_runtime::spawn_blocking(move || std::fs::copy(&src, &target))
            .await
            .map_err(platform)?
            .map_err(|e| io_failure(&e))?;
        Ok("saved".into())
    }

    /// 放一段文字到系統的剪貼簿；有 `html` 時一併放 HTML 版本。
    pub async fn copy_text(&self, text: &str, html: Option<&str>) -> Result<()> {
        #[cfg(windows)]
        {
            let owner = self
                .0
                .get_webview_window("main")
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0);
            crate::win::copy_text(owner, text, html.filter(|h| !h.is_empty()))
        }
        #[cfg(not(windows))]
        {
            let _ = (text, html);
            Err(Error::Unsupported)
        }
    }

    /// 用系統預設的 PDF 程式開暫存資料夾裡的一份 PDF，由它列印。程式開起來就交回。
    pub async fn print_file(&self, path: &str, _name: &str) -> Result<()> {
        let src = PathBuf::from(path);
        let cache = self.cache_dir()?;
        if !src.is_file() || !is_inside(&src, &cache) || is_inside(&src, &self.private_dir()?) {
            return Err(Error::Platform("not a printable file".into()));
        }
        tauri_plugin_opener::open_path(&src, None::<&str>).map_err(platform)
    }

    /// `paperreader://` 連結：電腦版件二才接上，現在一律沒有。
    pub async fn take_link(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// 分享進來：電腦沒有系統的分享（件二用拖放與檔案關聯），一律沒有。
    pub async fn shared_count(&self) -> Result<u32> {
        Ok(0)
    }

    pub async fn take_shared(&self) -> Result<Vec<PickedPdf>> {
        Ok(Vec::new())
    }

    /// 把筆記檔交給別的程式：`obsidian` 為真開 `obsidian://` 連結，否則用系統預設的程式開。
    /// 只交出文獻庫 `notes/` 底下與匯出暫存裡的 `.md`（同 Android）。
    /// 交回 `opened`、`no_app`（沒有可以處理的程式）或 `not_shareable`。
    pub async fn open_external(&self, path: &str, obsidian: bool) -> Result<String> {
        let file = PathBuf::from(path);
        let roots = [
            self.documents_dir()?.join("PaperReader").join("notes"),
            self.private_dir()?.join("notes"),
            self.cache_dir()?.join("exports"),
        ];
        let is_md = file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        if !file.is_file() || !is_md || !roots.iter().any(|r| is_inside(&file, r)) {
            return Ok("not_shareable".into());
        }
        let opened = if obsidian {
            tauri_plugin_opener::open_url(obsidian_link(&file), None::<&str>)
        } else {
            tauri_plugin_opener::open_path(&file, None::<&str>)
        };
        Ok(if opened.is_ok() { "opened" } else { "no_app" }.into())
    }

    /// 用系統的瀏覽器打開一個 `https://` 網址。
    pub async fn open_url(&self, url: &str) -> Result<()> {
        if !is_web_url(url) {
            return Err(Error::Platform("only https links".into()));
        }
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(platform)
    }

    /// 電腦的螢幕不會因為閱讀而關：不做事。
    pub async fn set_keep_screen_on(&self, _on: bool) -> Result<()> {
        Ok(())
    }

    /// 電腦的視窗沒有狀態列與手勢列。
    pub async fn insets(&self) -> Result<Insets> {
        Ok(Insets::default())
    }

    /// 把一把金鑰存進系統的憑證儲存（Windows 憑證管理員）。名稱只能是小寫英數字與底線。
    pub async fn secret_set(&self, name: &str, value: &str) -> Result<()> {
        #[cfg(windows)]
        {
            crate::win::secret_set(crate::win::SECRET_PREFIX, name, value)
        }
        #[cfg(not(windows))]
        {
            let _ = (name, value);
            Err(Error::Unsupported)
        }
    }

    /// 取出金鑰；沒有設過是 `None`。只給 Rust 端要送出請求時用，不經過網頁。
    pub async fn secret_get(&self, name: &str) -> Result<Option<String>> {
        #[cfg(windows)]
        {
            crate::win::secret_get(crate::win::SECRET_PREFIX, name)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Err(Error::Unsupported)
        }
    }

    pub async fn secret_delete(&self, name: &str) -> Result<()> {
        #[cfg(windows)]
        {
            crate::win::secret_delete(crate::win::SECRET_PREFIX, name)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Err(Error::Unsupported)
        }
    }

    /// 已設金鑰的名稱（不含內容）。只列這個應用自己的項目。
    pub async fn secret_names(&self) -> Result<Vec<String>> {
        #[cfg(windows)]
        {
            crate::win::secret_names(crate::win::SECRET_PREFIX)
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unsupported)
        }
    }

    /// 標題列已跟著系統的明暗：不做事。
    pub async fn set_dark_bars(&self, _dark: bool) -> Result<()> {
        Ok(())
    }

    /// 電腦不分網路種類：視為在線、不計流量（「只在 Wi-Fi 下載」在電腦上不擋）。
    pub async fn network_info(&self) -> Result<NetworkInfo> {
        Ok(NetworkInfo {
            online: true,
            unmetered: true,
        })
    }

    // 應用內更新只做 Android；電腦版之後用 Tauri 官方的更新外掛。畫面在電腦上不顯示這一節。

    pub async fn app_version(&self) -> Result<AppVersion> {
        Err(Error::Unsupported)
    }

    pub async fn inspect_apk(&self, _path: &str) -> Result<ApkInfo> {
        Err(Error::Unsupported)
    }

    pub async fn install_apk(&self, _path: &str, _sha256: &str) -> Result<String> {
        Err(Error::Unsupported)
    }

    pub async fn install_status(&self) -> Result<Option<InstallStatus>> {
        Ok(None)
    }

    pub async fn open_install_settings(&self) -> Result<()> {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_lose_path_separators_and_never_end_up_empty() {
        assert_eq!(safe_name("a/b\\c.pdf", "x"), "a_b_c.pdf");
        assert_eq!(safe_name("  ", "x"), "x");
        assert_eq!(safe_name("..", "x"), "x");
        assert_eq!(safe_name("報告.pdf", "x"), "報告.pdf");
    }

    #[test]
    fn only_files_below_the_root_are_inside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cache");
        std::fs::create_dir_all(root.join("exports")).unwrap();
        let inner = root.join("exports").join("a.pdf");
        std::fs::write(&inner, b"x").unwrap();
        let outer = dir.path().join("b.pdf");
        std::fs::write(&outer, b"x").unwrap();
        assert!(is_inside(&inner, &root));
        assert!(!is_inside(&outer, &root));
        // `..` 繞出去的不算
        assert!(!is_inside(
            &root.join("exports").join("..").join("..").join("b.pdf"),
            &root
        ));
        // 根目錄本身、不存在的檔都不算
        assert!(!is_inside(&root, &root));
        assert!(!is_inside(&root.join("missing.pdf"), &root));
    }

    #[test]
    fn other_files_are_copied_under_picks_and_the_original_stays() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("My backup.zip");
        std::fs::write(&original, b"zip body").unwrap();
        let picks = dir.path().join("cache").join("picks");
        let p = picked_copy(&original, &picks);
        assert_eq!(p.name, "My backup.zip");
        assert_eq!(p.bytes, Some(8));
        let copy = PathBuf::from(p.path.unwrap());
        // `<picks>/<隨機>/<檔名>`：外殼的 `remove_pick` 認得這個位置
        assert_eq!(copy.parent().unwrap().parent().unwrap(), picks);
        assert_eq!(std::fs::read(&copy).unwrap(), b"zip body");
        assert!(original.exists());
        // 兩次選同一份不會撞在一起
        let again = picked_copy(&original, &picks);
        assert_ne!(PathBuf::from(again.path.unwrap()), copy);
    }

    #[test]
    fn a_copy_that_fails_is_reported_without_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = picked_copy(&dir.path().join("gone.zip"), &dir.path().join("picks"));
        assert_eq!(p.name, "gone.zip");
        assert!(p.path.is_none());
        assert_eq!(p.error.as_deref(), Some("failed"));
    }

    #[test]
    fn pdfs_are_handed_over_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Paper.pdf");
        std::fs::write(&f, b"%PDF-1.4").unwrap();
        let p = picked_in_place(f.clone());
        assert_eq!(p.name, "Paper.pdf");
        assert_eq!(p.path.as_deref(), Some(f.display().to_string().as_str()));
        assert_eq!(p.bytes, Some(8));
    }

    #[test]
    fn obsidian_links_name_the_vault_and_the_note_inside_it() {
        let note = Path::new("C:/Users/me/Documents/PaperReader/notes/Smith 2020.md");
        assert_eq!(
            obsidian_link(note),
            "obsidian://open?vault=PaperReader&file=notes%2FSmith%202020.md"
        );
        let zh = Path::new("/lib/文獻/notes/筆記.md");
        assert_eq!(
            obsidian_link(zh),
            "obsidian://open?vault=%E6%96%87%E7%8D%BB&file=notes%2F%E7%AD%86%E8%A8%98.md"
        );
    }

    #[test]
    fn only_https_links_with_a_host_open() {
        assert!(is_web_url("https://citationstyles.org/"));
        assert!(is_web_url("https://example.com"));
        assert!(!is_web_url("http://example.com"));
        assert!(!is_web_url("https://"));
        assert!(!is_web_url("https:///path"));
        assert!(!is_web_url("https://a b"));
        assert!(!is_web_url("file:///C:/x"));
    }
}
