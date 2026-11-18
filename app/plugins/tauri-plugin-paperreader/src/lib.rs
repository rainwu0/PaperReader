//! PaperReader 的平台轉接器。
//!
//! 做各平台才有的事：查「所有檔案存取」權限與各個儲存位置、帶使用者到授權頁、
//! 開系統的選檔畫面（可以一次選多份）、收下別的應用分享進來的 PDF、回報螢幕邊界、設定系統列的明暗、閱讀時讓螢幕不關、
//! 應用內更新的安裝檔核對與交給系統安裝（只有 Android）。網頁碰不到它：外掛沒有開放任何指令，
//! 應用的指令（`src-tauri`）經由 [`PaperReaderExt`] 呼叫它。
//!
//! Android 用自有的 Kotlin 程式（`android/`）。電腦在 `desktop.rs`：同一組方法、同樣的結果，
//! 對話框用 Tauri 官方的 dialog 外掛、開檔與網址用 opener；Windows 的金鑰與剪貼簿在 `win.rs`。
//! 電腦上沒有的事（權限頁、分享進來）交回 [`Error::Unsupported`] 或空的結果。

use serde::{Deserialize, Serialize};
use tauri::{
    Manager, Runtime,
    plugin::{Builder, TauriPlugin},
};

#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this platform is not supported")]
    Unsupported,
    #[error("platform call failed: {0}")]
    Platform(String),
    /// 複製選到的檔案時手機的空間不夠。
    #[error("no space left on device")]
    NoSpace,
}

pub type Result<T> = std::result::Result<T, Error>;

/// 儲存位置與權限（`storageInfo`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub sdk: u32,
    /// 有沒有「所有檔案存取」。
    pub all_files_access: bool,
    /// 共用空間裡的文獻庫位置：`Documents/PaperReader`。
    pub shared_dir: String,
    /// 應用私有的外部空間裡的文獻庫位置（解除安裝會一併刪除）。
    pub private_dir: String,
    /// 應用私有的暫存資料夾（選到的檔案先複製到這裡）。
    pub cache_dir: String,
}

/// 系統選檔畫面選到的一份 PDF。複製成功就有暫存檔的路徑；複製失敗（讀不到、空間不足）就有 `error`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedPdf {
    /// 使用者選的檔案的顯示名稱（給畫面在「哪幾份失敗」時用；不進日誌）。
    pub name: String,
    /// 暫存檔的路徑，檔名就是顯示名稱。用完要連同它所在的資料夾一起刪掉。複製失敗時沒有。
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub bytes: Option<u64>,
    /// 複製失敗的原因：`no_space`（手機空間不足）或 `failed`。
    #[serde(default)]
    pub error: Option<String>,
}

/// 系統的資料夾選擇畫面的結果（整批匯入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickedFolder {
    Cancelled,
    /// 主要儲存空間裡的資料夾，已轉成檔案路徑（`/storage/emulated/0/<相對路徑>`）。
    Path(String),
    /// 記憶卡、雲端等其他提供者的資料夾：這一版不收（請使用者改用 zip）。
    Unsupported,
}

/// 網路的狀況（「只在 Wi-Fi 下載」用）。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub online: bool,
    /// 不計流量的網路（Wi-Fi 之類）。
    pub unmetered: bool,
}

/// 已安裝的這個應用的版本（系統記的，不是編譯時寫進程式的）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersion {
    pub version_name: String,
    pub version_code: u64,
}

/// 下載的安裝檔讀出來的資訊（應用內更新）。讀不出來的欄位一律當成不符。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApkInfo {
    /// 安裝檔讀得懂（是 Android 的安裝檔）。
    pub readable: bool,
    /// 套件名稱和已安裝的這個應用相同。
    pub same_package: bool,
    pub version_code: u64,
    /// 簽章憑證和已安裝的相同（或是已安裝那把金鑰合法輪替後的新金鑰）。讀不到憑證時為假。
    pub same_signer: bool,
    /// 兩邊的簽章憑證都讀得到。為假時 `same_signer` 沒有意義：是讀不到，不是不同。
    #[serde(default)]
    pub signer_readable: bool,
}

/// 裝置與系統（應用內更新的診斷）。讀不到的欄位是 `None`；屬性沒設時是空字串。不含個人資料。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub brand: String,
    pub model: String,
    /// `Build.DISPLAY`（組建名稱）。
    pub display: String,
    /// `Build.VERSION.RELEASE`。
    pub release: String,
    pub sdk: i32,
    pub incremental: String,
    /// `ro.miui.ui.version.name`。
    pub miui_version: Option<String>,
    /// `ro.mi.os.version.name`（HyperOS）。
    pub hyper_os_version: Option<String>,
    /// `persist.sys.miui_optimization` 的原值；`0`／`false` 表示「MIUI 最佳化」關著。
    pub miui_optimization: Option<String>,
}

/// 一次安裝的最終結果（系統的原話、確認畫面、環境資訊），平台轉接器存在應用私有空間的那一筆。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstallReport {
    /// `PackageInstaller.STATUS_*`：0 成功；1 失敗、2 被擋、3 中止、4 無效、5 衝突、6 空間不足、7 不相容、8 逾時。
    pub status: i32,
    /// `EXTRA_STATUS_MESSAGE` 原文。
    pub status_message: Option<String>,
    /// 舊式代碼（`INSTALL_FAILED_*` 的數字），系統有給才有。
    pub legacy_status: Option<i32>,
    pub session_id: Option<i64>,
    /// 交給系統、打開確認畫面、收到結果的時間（Unix 毫秒）。
    pub commit_at: Option<i64>,
    pub confirm_at: Option<i64>,
    pub at: i64,
    /// 要裝的 versionCode。
    pub target_code: Option<i64>,
    /// 確認意圖指向的套件、元件、動作。
    pub confirm_package: Option<String>,
    pub confirm_component: Option<String>,
    pub confirm_action: Option<String>,
    pub installed_name: Option<String>,
    pub installed_code: Option<i64>,
    /// 安裝者紀錄（`getInstallSourceInfo`）。
    pub installer: Option<String>,
    pub initiator: Option<String>,
    pub can_request_installs: Option<bool>,
    pub device: DeviceInfo,
}

/// 平台轉接器存的最新一筆結果，以及是不是還有一次交給系統、還沒有結果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstallState {
    pub report: Option<InstallReport>,
    pub pending: bool,
    /// 還沒有結果的那一次（狀態是 -1，`at` 是現在，附環境資訊）。
    pub attempt: Option<InstallReport>,
}

/// 螢幕邊界（單位：CSS 像素，也就是 dp）：狀態列、瀏海、手勢列占掉的部分。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

#[cfg(target_os = "android")]
pub struct PaperReader<R: Runtime>(PluginHandle<R>);

#[cfg(not(target_os = "android"))]
pub struct PaperReader<R: Runtime>(tauri::AppHandle<R>);

#[cfg(target_os = "android")]
mod android_impl {
    use super::*;
    use serde_json::{Value, json};

    /// Kotlin 端用 `reject(訊息, "NO_SPACE")` 回報空間不足；其他失敗一律是 `Platform`。
    fn map(e: tauri::plugin::mobile::PluginInvokeError) -> Error {
        use tauri::plugin::mobile::PluginInvokeError;
        match e {
            PluginInvokeError::InvokeRejected(resp) if resp.code.as_deref() == Some("NO_SPACE") => {
                Error::NoSpace
            }
            other => Error::Platform(other.to_string()),
        }
    }

    impl<R: Runtime> PaperReader<R> {
        pub(super) async fn call<T: serde::de::DeserializeOwned>(
            &self,
            command: &str,
            payload: Value,
        ) -> Result<T> {
            self.0
                .run_mobile_plugin_async(command, payload)
                .await
                .map_err(map)
        }

        pub async fn storage_info(&self) -> Result<StorageInfo> {
            self.call("storageInfo", Value::Null).await
        }

        /// 開啟系統的「所有檔案存取」授權頁；回到應用後再用 `storage_info` 查結果。
        pub async fn open_all_files_settings(&self) -> Result<()> {
            self.call::<Value>("openAllFilesSettings", Value::Null)
                .await
                .map(|_| ())
        }

        /// 開系統選檔畫面，可以一次選多份 PDF。使用者取消交回空清單。
        /// 每一份各自複製成暫存檔；有一份複製失敗，不影響其他份（那一份帶 `error`）。
        pub async fn pick_pdfs(&self) -> Result<Vec<PickedPdf>> {
            self.pick("pdf", true).await
        }

        /// 只選一份 PDF（加入譯本用）。
        pub async fn pick_one_pdf(&self) -> Result<Vec<PickedPdf>> {
            self.pick("pdf", false).await
        }

        /// 選一份備份檔（任何種類的檔案，由核心驗證它是不是備份檔）。
        pub async fn pick_any_file(&self) -> Result<Vec<PickedPdf>> {
            self.pick("any", false).await
        }

        /// 開系統的資料夾選擇畫面（整批匯入）。只收主要儲存空間的資料夾，轉成檔案路徑。
        pub async fn pick_folder(&self) -> Result<PickedFolder> {
            #[derive(Deserialize)]
            struct Raw {
                cancelled: bool,
                #[serde(default)]
                path: Option<String>,
                #[serde(default)]
                unsupported: bool,
            }
            let raw: Raw = self.call("pickFolder", Value::Null).await?;
            Ok(match (raw.cancelled, raw.path) {
                (true, _) => PickedFolder::Cancelled,
                (false, Some(p)) if !raw.unsupported => PickedFolder::Path(p),
                _ => PickedFolder::Unsupported,
            })
        }

        async fn pick(&self, kind: &str, multiple: bool) -> Result<Vec<PickedPdf>> {
            #[derive(Deserialize)]
            struct Raw {
                cancelled: bool,
                #[serde(default)]
                files: Vec<PickedPdf>,
            }
            let raw: Raw = self
                .call("pickFiles", json!({ "kind": kind, "multiple": multiple }))
                .await?;
            Ok(if raw.cancelled { Vec::new() } else { raw.files })
        }

        /// 把應用暫存資料夾裡的一個檔交給使用者：`share` 為真開系統的分享畫面，否則開「存到…」。
        /// 交回 `saved`、`shared` 或 `cancelled`。
        pub async fn export_file(
            &self,
            path: &str,
            name: &str,
            share: bool,
            mime: Option<&str>,
        ) -> Result<String> {
            #[derive(Deserialize)]
            struct Raw {
                result: String,
            }
            let raw: Raw = self
                .call(
                    "exportFile",
                    json!({ "path": path, "name": name, "share": share, "mime": mime }),
                )
                .await?;
            Ok(raw.result)
        }

        /// 放一段文字到系統的剪貼簿；有 `html` 時一併放 HTML 版本。
        pub async fn copy_text(&self, text: &str, html: Option<&str>) -> Result<()> {
            self.call::<Value>("copyText", json!({ "text": text, "html": html }))
                .await
                .map(|_| ())
        }

        /// 用系統的列印服務印應用暫存資料夾裡的一份 PDF。列印畫面開出來就交回（之後印不印由使用者決定）。
        pub async fn print_file(&self, path: &str, name: &str) -> Result<()> {
            self.call::<Value>("printFile", json!({ "path": path, "name": name }))
                .await
                .map(|_| ())
        }

        /// 別的應用（例如筆記工具）開了 `paperreader://` 連結：交回還沒處理的那一條（只交一次）。
        pub async fn take_link(&self) -> Result<Option<String>> {
            #[derive(Deserialize)]
            struct Raw {
                link: Option<String>,
            }
            let raw: Raw = self.call("takeLink", Value::Null).await?;
            Ok(raw.link)
        }

        /// 別的應用分享進來、還沒領取的份數（含還在複製的）。
        pub async fn shared_count(&self) -> Result<u32> {
            #[derive(Deserialize)]
            struct Raw {
                count: u32,
            }
            let raw: Raw = self.call("sharedCount", Value::Null).await?;
            Ok(raw.count)
        }

        /// 領取別的應用分享進來的 PDF（只交一次）。每一份和選檔的結果同一種形式：
        /// 暫存檔在 `<暫存>/shared/<隨機>/<檔名>`，用完連同那個資料夾一起刪；複製失敗的帶 `error`。
        pub async fn take_shared(&self) -> Result<Vec<PickedPdf>> {
            #[derive(Deserialize)]
            struct Raw {
                #[serde(default)]
                files: Vec<PickedPdf>,
            }
            let raw: Raw = self.call("takeShared", Value::Null).await?;
            Ok(raw.files)
        }

        /// 把筆記檔交給別的應用：`obsidian` 為真用 `obsidian://open?vault=…&file=…` 連結，否則開系統的
        /// 「用其他應用打開」選單。交回 `opened`、`no_app`（沒有可以處理的應用，例如沒裝 Obsidian），或 `not_shareable`（筆記檔不在
        /// 可以交出去的位置：只有 `Documents/PaperReader/notes` 與應用專屬空間的 `PaperReader/notes`）。
        pub async fn open_external(&self, path: &str, obsidian: bool) -> Result<String> {
            #[derive(Deserialize)]
            struct Raw {
                result: String,
            }
            let mode = if obsidian { "obsidian" } else { "chooser" };
            let raw: Raw = self
                .call("openExternal", json!({ "path": path, "mode": mode }))
                .await?;
            Ok(raw.result)
        }

        /// 用系統的瀏覽器打開一個網址（呼叫的一方先確認是 `https://`）。
        pub async fn open_url(&self, url: &str) -> Result<()> {
            self.call::<Value>("openUrl", json!({ "url": url }))
                .await
                .map(|_| ())
        }

        /// 閱讀時讓螢幕不自動關閉（`on` 為假就取消）。
        pub async fn set_keep_screen_on(&self, on: bool) -> Result<()> {
            self.call::<Value>("setKeepScreenOn", json!({ "on": on }))
                .await
                .map(|_| ())
        }

        pub async fn insets(&self) -> Result<Insets> {
            self.call("insets", Value::Null).await
        }

        /// 把一把金鑰加密存進應用私有的空間（Android Keystore）。名稱只能是小寫英數字與底線。
        pub async fn secret_set(&self, name: &str, value: &str) -> Result<()> {
            self.call::<Value>("secretSet", json!({ "name": name, "value": value }))
                .await
                .map(|_| ())
        }

        /// 取出金鑰；沒有設過是 `None`。只給 Rust 端要送出請求時用，不經過網頁。
        pub async fn secret_get(&self, name: &str) -> Result<Option<String>> {
            #[derive(Deserialize)]
            struct Raw {
                value: Option<String>,
            }
            let raw: Raw = self.call("secretGet", json!({ "name": name })).await?;
            Ok(raw.value)
        }

        pub async fn secret_delete(&self, name: &str) -> Result<()> {
            self.call::<Value>("secretDelete", json!({ "name": name }))
                .await
                .map(|_| ())
        }

        /// 已設金鑰的名稱（不含內容）。
        pub async fn secret_names(&self) -> Result<Vec<String>> {
            #[derive(Deserialize)]
            struct Raw {
                names: Vec<String>,
            }
            let raw: Raw = self.call("secretNames", Value::Null).await?;
            Ok(raw.names)
        }

        /// 讓狀態列與手勢列的圖示配合畫面的明暗：`dark` 為真表示畫面是深色。
        pub async fn set_dark_bars(&self, dark: bool) -> Result<()> {
            self.call::<Value>("setDarkBars", json!({ "dark": dark }))
                .await
                .map(|_| ())
        }

        pub async fn network_info(&self) -> Result<NetworkInfo> {
            self.call("networkInfo", Value::Null).await
        }

        // ---------- 應用內更新 ----------

        pub async fn app_version(&self) -> Result<AppVersion> {
            self.call("appVersion", Value::Null).await
        }

        /// 讀下載的安裝檔：套件名稱、versionCode、簽章憑證是否和已安裝的相同。
        pub async fn inspect_apk(&self, path: &str) -> Result<ApkInfo> {
            self.call("inspectApk", json!({ "path": path })).await
        }

        /// 交給系統安裝。交回 `needs_permission`（還沒允許「安裝不明應用程式」，沒有開始）、
        /// `mismatch`（寫進安裝 session的內容和 `sha256` 不符，已放棄，沒有交給系統）或
        /// `committed`（已交給系統；要使用者確認時系統的確認畫面由外掛打開）。
        pub async fn install_apk(&self, path: &str, sha256: &str) -> Result<String> {
            #[derive(Deserialize)]
            struct Raw {
                result: String,
            }
            let raw: Raw = self
                .call("installApk", json!({ "path": path, "sha256": sha256 }))
                .await?;
            Ok(raw.result)
        }

        /// 最近一次安裝的最終結果（不取走），以及是不是還在等系統回報。
        pub async fn install_status(&self) -> Result<InstallState> {
            self.call("installStatus", Value::Null).await
        }

        pub async fn device_info(&self) -> Result<DeviceInfo> {
            self.call("deviceInfo", Value::Null).await
        }

        /// 退路：把下載、核對過的安裝檔交給系統的安裝程式（`ACTION_VIEW`）。交回 `opened` 或 `no_app`。
        pub async fn open_installer(&self, path: &str) -> Result<String> {
            #[derive(Deserialize)]
            struct Raw {
                result: String,
            }
            let raw: Raw = self.call("openInstaller", json!({ "path": path })).await?;
            Ok(raw.result)
        }

        /// 開系統的「安裝不明應用程式」頁（只有這個應用的那一項）。
        pub async fn open_install_settings(&self) -> Result<()> {
            self.call::<Value>("openInstallSettings", Value::Null)
                .await
                .map(|_| ())
        }
    }
}

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(windows)]
mod win;

pub trait PaperReaderExt<R: Runtime> {
    fn paperreader(&self) -> &PaperReader<R>;
}

impl<R: Runtime, T: Manager<R>> PaperReaderExt<R> for T {
    fn paperreader(&self) -> &PaperReader<R> {
        self.state::<PaperReader<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("paperreader")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin(
                    "io.github.rainwu0.paperreader.plugin",
                    "PaperReaderPlugin",
                )?;
                app.manage(PaperReader(handle));
            }
            #[cfg(not(target_os = "android"))]
            {
                app.manage(PaperReader::<R>(app.clone()));
            }
            Ok(())
        })
        .build()
}

/// 把平台轉接器加進應用。電腦上一併加 Tauri 官方的 dialog 外掛（選檔、存檔對話框要它的狀態）；
/// 它的指令網頁用不到（`capabilities` 沒有開），只由這個外掛從 Rust 端呼叫。
/// 不在這個外掛的 `setup` 裡加：那時應用正在逐一初始化外掛，再加一個會卡住。
pub fn register<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let builder = builder.plugin(init());
    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_dialog::init());
    builder
}
