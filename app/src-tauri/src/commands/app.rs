//! 應用本身：設定（深淺色）、版本、螢幕邊界與系統列、打開外部網址。
//!
//! 「關於」頁的組件與授權清單不在這裡：介面建置前由 `scripts/licenses.mjs` 從 Cargo.lock 與 package-lock.json
//! 產生 `licenses.json`，畫面直接讀那份檔。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::{AppState, Prefs, Theme};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_paperreader::{Insets, PaperReaderExt};

#[derive(Serialize)]
pub struct AppInfo {
    /// 對外的版號（規格/05 第 1 節）：設定檔裡的 `a.b.c+d` 在這裡是 `a.b.c.d`。
    pub version: String,
    /// 這個建置的作業系統：`android`、`windows` 或 `other`。「關於」頁依它只列這個安裝檔內含的組件。
    pub platform: &'static str,
}

const PLATFORM: &str = if cfg!(target_os = "android") {
    "android"
} else if cfg!(windows) {
    "windows"
} else {
    "other"
};

/// 對外的版號：Cargo 的版號 `a.b.c+d` 換成 `a.b.c.d`（規格/05 第 1 節；`+` 不出現在任何對外的地方）。
pub fn public_version() -> String {
    to_public_version(env!("CARGO_PKG_VERSION"))
}

fn to_public_version(tool_version: &str) -> String {
    tool_version.replace('+', ".")
}

#[cfg(test)]
mod version_tests {
    use super::to_public_version;

    #[test]
    fn plus_becomes_a_dot() {
        assert_eq!(to_public_version("0.1.12+3"), "0.1.12.3");
        assert_eq!(to_public_version("0.1.12"), "0.1.12");
    }
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: public_version(),
        platform: PLATFORM,
    }
}

/// 只開 `https://` 的網址（「關於」頁的 CSL 專案、各組件的原始碼位置）。別的種類一律拒絕。
fn web_url(url: &str) -> ApiResult<&str> {
    let ok = url
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace));
    if ok {
        Ok(url)
    } else {
        Err(ApiError::new(
            ErrorKind::InvalidInput,
            "only https links can be opened",
        ))
    }
}

/// 用系統的瀏覽器打開一個網頁（應用自己的網頁元件不離開）。
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> ApiResult<()> {
    let url = web_url(&url)?;
    app.paperreader().open_url(url).await.map_err(Into::into)
}

/// 啟動時要告知使用者的事：目前只有「設定檔壞了，已用預設值復原」。每次啟動只回報一次。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupNotice {
    pub settings_recovered: bool,
}

#[tauri::command]
pub async fn startup_notice(state: State<'_, AppState>) -> ApiResult<StartupNotice> {
    Ok(StartupNotice {
        settings_recovered: state.take_recovered(),
    })
}

/// 深淺色只動設定，不碰文獻庫的鎖。
#[tauri::command]
pub async fn get_theme(state: State<'_, AppState>) -> ApiResult<Theme> {
    Ok(state.theme())
}

#[tauri::command]
pub async fn set_theme(state: State<'_, AppState>, theme: Theme) -> ApiResult<()> {
    state.run("set_theme", move |s| s.set_theme(theme)).await
}

/// 偏好：首頁排序與閱讀的選項。只動設定，不碰文獻庫的鎖。
#[tauri::command]
pub async fn get_prefs(state: State<'_, AppState>) -> ApiResult<Prefs> {
    Ok(state.prefs())
}

#[tauri::command]
pub async fn set_prefs(state: State<'_, AppState>, prefs: Prefs) -> ApiResult<()> {
    state.run("set_prefs", move |s| s.set_prefs(prefs)).await
}

/// 閱讀時讓螢幕不自動關閉（開著閱讀畫面期間才要；離開閱讀就關掉）。
#[tauri::command]
pub async fn set_keep_awake(app: AppHandle, on: bool) -> ApiResult<()> {
    app.paperreader()
        .set_keep_screen_on(on)
        .await
        .map_err(Into::into)
}

/// 目前的螢幕邊界（狀態列、瀏海、手勢列占掉的部分）。
#[tauri::command]
pub async fn system_insets(app: AppHandle) -> ApiResult<Insets> {
    app.paperreader().insets().await.map_err(Into::into)
}

/// 讓狀態列與手勢列的圖示配合畫面的明暗。
#[tauri::command]
pub async fn system_bars(app: AppHandle, dark: bool) -> ApiResult<()> {
    app.paperreader()
        .set_dark_bars(dark)
        .await
        .map_err(Into::into)
}

/// 別的應用開了 `paperreader://` 連結：交回還沒處理的那一條（只交一次）。非 Android 沒有這個功能，交回 `null`。
#[tauri::command]
pub async fn take_link(app: AppHandle) -> ApiResult<Option<String>> {
    match app.paperreader().take_link().await {
        Ok(l) => Ok(l),
        Err(tauri_plugin_paperreader::Error::Unsupported) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_links_are_opened() {
        assert!(web_url("https://citationstyles.org/").is_ok());
        assert!(web_url("https://github.com/servo/stylo").is_ok());
        for bad in [
            "http://citationstyles.org/",
            "file:///data/data/io.github.rainwu0.paperreader/files/secrets/x.bin",
            "intent://scan/#Intent;scheme=zxing;end",
            "javascript:alert(1)",
            "https://",
            "https://a b",
            "",
        ] {
            assert_eq!(
                web_url(bad).unwrap_err().kind,
                ErrorKind::InvalidInput,
                "{bad}"
            );
        }
    }
}
