//! 應用內檢查更新的指令。只有 Android；電腦上 `update_status` 交回 `supported: false`，
//! 畫面不顯示這一節。
//!
//! 流程：同意→ 檢查（一個 GET，不帶資料）→ 使用者按「更新」→ 下載這台架構的安裝檔、
//! 核對 SHA-256 → 平台轉接器讀安裝檔：套件名稱相同、versionCode 比已安裝的新、簽章憑證相同，任何一項不符就刪掉
//! → 交給系統安裝。下載的檔放在應用的快取 `updates/`，裝好（下次打開時發現已是那一版）或放棄就刪。

use crate::error::{ApiError, ApiResult, ErrorKind};
use crate::state::{AppState, UpdateSettings};
use crate::update::{self, Available, AvailableView, UpdateSlot};
use paperreader_translate::Cancel;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_paperreader::{ApkInfo, PaperReaderExt};

/// 快取裡放下載的安裝檔的資料夾。
pub fn updates_dir(cache: &Path) -> PathBuf {
    cache.join("updates")
}

/// 啟動時清掉上次留下的：半份（`.part`）一律刪；沒有可用的 `ready.json` 時整個 `updates/` 刪掉
/// （寫紀錄之前被結束留下的安裝檔）。有紀錄的那一份留著，等 `update_status` 依版本決定。
pub fn clear_stale(cache: &Path) {
    let dir = updates_dir(cache);
    if read_ready(&dir).is_none() {
        discard(&dir);
        return;
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// 收回同意：回到沒同意的狀態，查到的版本、上次檢查、略過的版本一起忘掉。
/// `invited` 設成真：首頁的邀請不再出現（使用者剛做過決定）。
pub fn revoke(u: &mut UpdateSettings) {
    *u = UpdateSettings {
        invited: true,
        ..UpdateSettings::default()
    };
}

/// 會連網或會裝東西的指令，開頭都先查同意：畫面不顯示按鈕不夠，網頁的程式碼可以直接呼叫。
pub fn require_consent(u: &UpdateSettings) -> ApiResult<()> {
    if u.consented {
        Ok(())
    } else {
        Err(ApiError::new(ErrorKind::InvalidInput, "no consent"))
    }
}

/// 記下一次檢查的結果。檢查進行中被收回同意的話不記（不然收回後又出現「有新版」）。
pub fn record_check(u: &mut UpdateSettings, now: u64, found: Option<Available>) {
    if u.consented {
        u.last_check = Some(now);
        u.found = found;
    }
}

/// 已下載的那一版（`ready.json` 的 versionCode）是不是該丟：檢查查不到新版（發布被撤下），
/// 或查到的是另一版。
pub fn stale_download(ready: Option<u64>, found: Option<&Available>) -> bool {
    match (ready, found) {
        (None, _) => false,
        (Some(code), Some(a)) => a.code != code,
        (Some(_), None) => true,
    }
}

fn cache_dir(app: &AppHandle) -> ApiResult<PathBuf> {
    app.path()
        .app_cache_dir()
        .map_err(|_| ApiError::new(ErrorKind::Io, "cache dir"))
}

/// 核對過、可以安裝的那一份的紀錄（`updates/ready.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ready {
    version: String,
    code: u64,
    name: String,
    sha256: String,
}

fn ready_path(dir: &Path) -> PathBuf {
    dir.join("ready.json")
}

fn read_ready(dir: &Path) -> Option<Ready> {
    let r: Ready = serde_json::from_slice(&std::fs::read(ready_path(dir)).ok()?).ok()?;
    // 檔名只能是規格的安裝檔名，不讓紀錄指到別處
    let ok = r.name.starts_with("PaperReader-")
        && r.name.ends_with(".apk")
        && !r.name.contains(['/', '\\'])
        && dir.join(&r.name).is_file();
    ok.then_some(r)
}

fn discard(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// 這台能用應用內更新（Android，而且是有發布安裝檔的架構）。
    pub supported: bool,
    /// 已安裝的版本（系統記的 versionName）。
    pub current: String,
    pub consented: bool,
    pub auto_check: bool,
    pub invited: bool,
    /// 上次真的連到 GitHub 檢查的時間（Unix 秒）。
    pub last_check: Option<u64>,
    pub dismissed: Option<String>,
    /// 上次檢查查到、比已安裝的新的版本。
    pub available: Option<AvailableView>,
    /// 已下載、核對過、還沒裝的那一版。
    pub downloaded: Option<String>,
    pub download: Progress,
    /// 連到哪裡（同意的說明用）。
    pub host: &'static str,
    /// 現在是不是不計流量的網路（行動數據下先說大小再下載）；查不到時 `None`。
    pub unmetered: Option<bool>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub running: bool,
    pub done: u64,
    pub total: u64,
}

/// 這台已安裝的 versionCode；不是 Android 或讀不到時 `None`。
async fn installed(app: &AppHandle) -> Option<(String, u64)> {
    let v = app.paperreader().app_version().await.ok()?;
    Some((v.version_name, v.version_code))
}

#[tauri::command]
pub async fn update_status(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<UpdateSlot>>,
) -> ApiResult<UpdateStatus> {
    let s = state.update_settings_get();
    let inst = installed(&app).await;
    let supported = inst.is_some() && update::abi().is_some();
    let dir = updates_dir(&cache_dir(&app)?);
    let mut downloaded = None;
    if let Some((_, code)) = &inst
        && !slot.running.load(Ordering::SeqCst)
    {
        match read_ready(&dir) {
            // 已經裝上了（或裝了更新的）、或已收回同意：下載的那份沒用了
            Some(r) if r.code <= *code || !s.consented => discard(&dir),
            Some(r) => downloaded = Some(r.version),
            None => {}
        }
    }
    // 查到的那一版裝上了（或裝了更新的）就不算新；沒同意時不提示（收回同意之後）
    let available = s
        .found
        .as_ref()
        .filter(|a| s.consented && inst.as_ref().is_some_and(|(_, c)| a.code > *c))
        .map(Available::view);
    Ok(UpdateStatus {
        supported,
        current: inst.map(|(n, _)| n).unwrap_or_default(),
        consented: s.consented,
        auto_check: s.auto_check,
        invited: s.invited,
        last_check: s.last_check,
        dismissed: s.dismissed,
        available,
        downloaded,
        download: Progress {
            running: slot.running.load(Ordering::Relaxed),
            done: slot.done.load(Ordering::Relaxed),
            total: slot.total.load(Ordering::Relaxed),
        },
        host: update::source().host,
        unmetered: app
            .paperreader()
            .network_info()
            .await
            .ok()
            .map(|n| n.unmetered),
    })
}

/// 改檢查更新的設定；沒給的欄位不動。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePatch {
    #[serde(default)]
    pub consented: Option<bool>,
    #[serde(default)]
    pub auto_check: Option<bool>,
    #[serde(default)]
    pub invited: Option<bool>,
    #[serde(default)]
    pub dismissed: Option<String>,
}

/// `consented: false` 是收回同意：設定回到沒同意的狀態（[`revoke`]），正在下載的停掉，
/// 下載的檔刪掉。之後一個請求都不送，直到再同意。
#[tauri::command]
pub async fn update_configure(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<UpdateSlot>>,
    patch: UpdatePatch,
) -> ApiResult<()> {
    let revoking = patch.consented == Some(false);
    state
        .run("update_configure", move |s| {
            s.update_settings_set(|u| {
                if revoking {
                    revoke(u);
                    return;
                }
                if let Some(v) = patch.consented {
                    u.consented = v;
                }
                if let Some(v) = patch.auto_check {
                    // 沒同意過不能打開（畫面先問；這裡再擋一次）
                    u.auto_check = v && u.consented;
                }
                if let Some(v) = patch.invited {
                    u.invited = v;
                }
                if let Some(v) = patch.dismissed {
                    u.dismissed = Some(v);
                }
            })
        })
        .await?;
    if revoking {
        // 正在下載的停掉（下載那一邊也會在寫紀錄前再查一次同意）；下載的檔刪掉
        if let Some(c) = slot
            .cancel
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            c.cancel();
        }
        discard(&updates_dir(&cache_dir(&app)?));
    }
    Ok(())
}

/// 檢查有沒有新版。`manual` 為假（打開應用時）只在自動檢查打開、上次查的超過一天時才連網，否則交回已知的結果。
/// 沒同意過一律不連網（`InvalidInput`）。
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<UpdateSlot>>,
    manual: bool,
) -> ApiResult<Option<AvailableView>> {
    let s = state.update_settings_get();
    require_consent(&s)?;
    let (Some((_, code)), Some(abi)) = (installed(&app).await, update::abi()) else {
        return Err(ApiError::new(ErrorKind::Unsupported, "update"));
    };
    let now = update::now_secs();
    if !manual && (!s.auto_check || !update::due(s.last_check, now)) {
        return Ok(s.found.filter(|a| a.code > code).map(|a| a.view()));
    }
    let src = update::source();
    let found = update::check(&src, abi, code)
        .await
        .map_err(|e| e.log("update_check"))?;
    let keep = found.clone();
    let still = state
        .run("update_check", move |s| {
            let mut still = false;
            s.update_settings_set(|u| {
                record_check(u, now, keep);
                still = u.consented;
            })?;
            Ok(still)
        })
        .await?;
    if !still {
        return Ok(None);
    }
    // 發布被撤下、或換了一版：已下載的那一份不留（下載進行中不動）
    let dir = updates_dir(&cache_dir(&app)?);
    if !slot.running.load(Ordering::SeqCst)
        && stale_download(read_ready(&dir).map(|r| r.code), found.as_ref())
    {
        discard(&dir);
    }
    Ok(found.map(|a| a.view()))
}

/// 下載並核對查到的新版。完成才交回；進度用 `update_status` 的 `download` 看。
/// 核對不過（SHA-256、套件名稱、版本、簽章憑證）時刪掉下載的檔，交回 `UpdateChecksum` 或 `UpdateSignature`。
#[tauri::command]
pub async fn update_download(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<UpdateSlot>>,
) -> ApiResult<String> {
    let s = state.update_settings_get();
    require_consent(&s)?;
    let a = s
        .found
        .ok_or_else(|| ApiError::new(ErrorKind::InvalidInput, "nothing to download"))?;
    let abi = update::abi().ok_or_else(|| ApiError::new(ErrorKind::Unsupported, "update"))?;
    // 設定檔裡讀回來的這一版：網址、檔名、版號要彼此一致，仍在這個建置的下載位置底下
    if !a.matches_source(&update::source(), abi, update::max_apk()) {
        return Err(ApiError::new(
            ErrorKind::UpdateBadRelease,
            "remembered release",
        ));
    }
    if slot.running.swap(true, Ordering::SeqCst) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "downloading"));
    }
    let cancel = Cancel::new();
    *slot.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
    let r = download_and_verify(&app, &state, &a, &slot, &cancel).await;
    slot.running.store(false, Ordering::SeqCst);
    if let Err(e) = &r
        && e.kind == ErrorKind::UpdateGone
    {
        // 那一版已從 GitHub 撤下：查到的版本忘掉，不再提示
        let gone = a.code;
        let _ = state
            .run("update_download", move |s| {
                s.update_settings_set(|u| {
                    if u.found.as_ref().is_some_and(|f| f.code == gone) {
                        u.found = None;
                    }
                })
            })
            .await;
    }
    r.map(|()| a.version).map_err(|e| e.log("update_download"))
}

async fn download_and_verify(
    app: &AppHandle,
    state: &AppState,
    a: &Available,
    slot: &UpdateSlot,
    cancel: &Cancel,
) -> ApiResult<()> {
    let dir = updates_dir(&cache_dir(app)?);
    // 只留這一份：先清掉之前的
    discard(&dir);
    let src = update::source();
    let sha = match update::expected_sha256(&src, a, cancel).await {
        Ok(s) => s,
        Err(e) => {
            discard(&dir);
            return Err(e);
        }
    };
    let apk = dir.join(&a.apk_name);
    if let Err(e) = update::download(&src, a, &sha, &apk, slot, cancel).await {
        discard(&dir);
        return Err(e);
    }
    if let Err(e) = verify_apk(app, &apk, a.code).await {
        discard(&dir);
        return Err(e);
    }
    // 下載途中收回了同意：不留下可以安裝的檔
    if cancel.is_cancelled() || require_consent(&state.update_settings_get()).is_err() {
        discard(&dir);
        return Err(ApiError::new(ErrorKind::InvalidInput, "cancelled"));
    }
    let ready = Ready {
        version: a.version.clone(),
        code: a.code,
        name: a.apk_name.clone(),
        sha256: sha,
    };
    let written = serde_json::to_vec(&ready)
        .map_err(|_| ApiError::new(ErrorKind::Io, "ready"))
        .and_then(|body| {
            std::fs::write(ready_path(&dir), body)
                .map_err(|_| ApiError::new(ErrorKind::Io, "ready"))
        });
    if written.is_err() {
        // 沒有紀錄的安裝檔沒人會刪
        discard(&dir);
    }
    written
}

/// 平台轉接器讀出來的資訊合不合格：套件名稱相同、versionCode 等於公布的那一版而且比已安裝的新、簽章憑證相同。
/// 讀不出安裝檔或它的簽章是 [`ErrorKind::UpdateUnverifiable`]（Android 9、10、13 初版曾讀不到），
/// 其餘不符是 [`ErrorKind::UpdateSignature`]。兩種都不裝。
pub fn judge(info: &ApkInfo, code: u64, installed: u64) -> ApiResult<()> {
    let detail = format!(
        "readable={} package={} code={} signer_readable={} signer={}",
        info.readable,
        info.same_package,
        info.version_code == code,
        info.signer_readable,
        info.same_signer
    );
    if !info.readable {
        return Err(ApiError::new(ErrorKind::UpdateUnverifiable, detail));
    }
    if !info.same_package || info.version_code != code || info.version_code <= installed {
        return Err(ApiError::new(ErrorKind::UpdateSignature, detail));
    }
    if !info.signer_readable {
        return Err(ApiError::new(ErrorKind::UpdateUnverifiable, detail));
    }
    if !info.same_signer {
        return Err(ApiError::new(ErrorKind::UpdateSignature, detail));
    }
    Ok(())
}

/// 請平台轉接器讀安裝檔，照 [`judge`] 判斷。
async fn verify_apk(app: &AppHandle, apk: &Path, code: u64) -> ApiResult<()> {
    let installed = installed(app)
        .await
        .map(|(_, c)| c)
        .ok_or_else(|| ApiError::new(ErrorKind::Unsupported, "update"))?;
    let info = app
        .paperreader()
        .inspect_apk(&apk.display().to_string())
        .await
        .map_err(|_| ApiError::new(ErrorKind::UpdateUnverifiable, "inspect failed"))?;
    judge(&info, code, installed)
}

#[tauri::command]
pub async fn update_download_cancel(slot: State<'_, Arc<UpdateSlot>>) -> ApiResult<()> {
    if let Some(c) = slot
        .cancel
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
    {
        c.cancel();
    }
    Ok(())
}

/// 把核對過的那一份交給系統安裝。裝之前再核對一次（SHA-256 與簽章），快取裡的檔被換掉也不會裝上；
/// 寫進系統安裝 session時平台轉接器再邊寫邊算一次 SHA-256，核對的就是系統要裝的那份內容。
/// 下載進行中不裝（同一個資料夾正在被換掉）。沒同意不裝。
/// 交回 `needs_permission`（還沒允許「安裝不明應用程式」）或 `committed`。
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: State<'_, Arc<UpdateSlot>>,
) -> ApiResult<String> {
    require_consent(&state.update_settings_get())?;
    if slot.running.load(Ordering::SeqCst) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "downloading"));
    }
    let dir = updates_dir(&cache_dir(&app)?);
    let r = read_ready(&dir).ok_or_else(|| ApiError::new(ErrorKind::NotFound, "no download"))?;
    let apk = dir.join(&r.name);
    let p = apk.clone();
    let sha = tauri::async_runtime::spawn_blocking(move || update::file_sha256(&p))
        .await
        .map_err(|_| ApiError::new(ErrorKind::Io, "hash"))?
        .map_err(|_| ApiError::new(ErrorKind::Io, "hash"))?;
    if sha != r.sha256 {
        discard(&dir);
        return Err(ApiError::new(ErrorKind::UpdateChecksum, "changed in cache"));
    }
    if let Err(e) = verify_apk(&app, &apk, r.code).await {
        discard(&dir);
        return Err(e.log("update_install"));
    }
    let result = app
        .paperreader()
        .install_apk(&apk.display().to_string(), &r.sha256)
        .await
        .map_err(|_| ApiError::new(ErrorKind::UpdateInstall, "install").log("update_install"))?;
    if result == "mismatch" {
        discard(&dir);
        return Err(
            ApiError::new(ErrorKind::UpdateChecksum, "changed while installing")
                .log("update_install"),
        );
    }
    Ok(result)
}

/// 系統安裝程式最後回報的結果：`waiting`（還沒回報，或正等使用者在確認畫面按）、`failed`、`cancelled`。
/// 裝成功時應用會被系統結束，所以收不到成功。
#[tauri::command]
pub async fn update_install_status(app: AppHandle) -> ApiResult<String> {
    let s = app.paperreader().install_status().await?;
    Ok(match s.map(|s| s.status) {
        None | Some(-1) | Some(0) => "waiting",
        // PackageInstaller.STATUS_FAILURE_ABORTED：使用者在確認畫面按了取消
        Some(3) => "cancelled",
        Some(_) => "failed",
    }
    .to_string())
}

/// 開系統的「安裝不明應用程式」頁（只有 PaperReader 那一項）。回到應用後由畫面接著裝。
#[tauri::command]
pub async fn update_open_install_settings(app: AppHandle) -> ApiResult<()> {
    app.paperreader()
        .open_install_settings()
        .await
        .map_err(Into::into)
}

/// 不裝了：刪掉下載的檔。下載進行中不動（要停下載用 `update_download_cancel`）。
#[tauri::command]
pub async fn update_discard(app: AppHandle, slot: State<'_, Arc<UpdateSlot>>) -> ApiResult<()> {
    if slot.running.load(Ordering::SeqCst) {
        return Err(ApiError::new(ErrorKind::TranslateBusy, "downloading"));
    }
    discard(&updates_dir(&cache_dir(&app)?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_record_only_names_an_apk_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("PaperReader-0.1.2-arm64.apk"), b"x").unwrap();
        let write = |name: &str| write_ready(d, name, 1_000_200);
        write("PaperReader-0.1.2-arm64.apk");
        assert_eq!(read_ready(d).unwrap().code, 1_000_200);
        write("../PaperReader-0.1.2-arm64.apk");
        assert!(read_ready(d).is_none());
        write("PaperReader-0.1.3-arm64.apk");
        assert!(read_ready(d).is_none());
    }

    fn write_ready(d: &Path, name: &str, code: u64) {
        let r = Ready {
            version: "0.1.2".into(),
            code,
            name: name.into(),
            sha256: "0".repeat(64),
        };
        std::fs::write(ready_path(d), serde_json::to_vec(&r).unwrap()).unwrap();
    }

    /// 啟動時，半份一律刪；沒有可用的 `ready.json` 對應的安裝檔也刪（寫紀錄前被結束留下的）。
    #[test]
    fn leftovers_are_removed_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let u = updates_dir(dir.path());
        std::fs::create_dir_all(&u).unwrap();
        std::fs::write(u.join("PaperReader-0.1.2-arm64.apk.part"), b"x").unwrap();
        std::fs::write(u.join("PaperReader-0.1.2-arm64.apk"), b"x").unwrap();
        clear_stale(dir.path());
        assert!(!u.exists(), "沒有紀錄的安裝檔不留");

        // 有紀錄的那一份留著（等 `update_status` 依版本決定）；半份照樣刪
        std::fs::create_dir_all(&u).unwrap();
        std::fs::write(u.join("PaperReader-0.1.2-arm64.apk.part"), b"x").unwrap();
        std::fs::write(u.join("PaperReader-0.1.2-arm64.apk"), b"x").unwrap();
        write_ready(&u, "PaperReader-0.1.2-arm64.apk", 1_000_200);
        clear_stale(dir.path());
        assert!(!u.join("PaperReader-0.1.2-arm64.apk.part").exists());
        assert!(u.join("PaperReader-0.1.2-arm64.apk").exists());

        // 沒有 updates/ 也不出錯
        let empty = tempfile::tempdir().unwrap();
        clear_stale(empty.path());
    }

    fn found(version: &str) -> Available {
        let v = update::Version::parse(version).unwrap();
        let name = update::apk_name(&v, "arm64");
        Available {
            version: version.into(),
            code: v.code(),
            notes: String::new(),
            size: 10,
            apk_url: format!(
                "https://github.com/rainwu0/PaperReader/releases/download/v{version}/{name}"
            ),
            sums_url: format!(
                "https://github.com/rainwu0/PaperReader/releases/download/v{version}/SHA256SUMS.txt"
            ),
            apk_name: name,
            digest: None,
        }
    }

    fn consented() -> UpdateSettings {
        UpdateSettings {
            consented: true,
            auto_check: true,
            invited: true,
            last_check: Some(100),
            dismissed: Some("0.1.2".into()),
            found: Some(found("0.1.3")),
        }
    }

    /// 收回同意回到沒同意的狀態，查到的版本一起忘掉；首頁的邀請不再出現。
    #[test]
    fn revoking_consent_forgets_everything() {
        let mut u = consented();
        revoke(&mut u);
        assert_eq!(
            u,
            UpdateSettings {
                consented: false,
                auto_check: false,
                invited: true,
                last_check: None,
                dismissed: None,
                found: None,
            }
        );
        // 還沒被邀請過就收回（從設定直接進去同意的）：一樣不再邀請
        let mut v = UpdateSettings {
            invited: false,
            ..consented()
        };
        revoke(&mut v);
        assert!(v.invited);
    }

    /// 沒同意時，會連網或會裝東西的指令一律在外殼這一側拒絕。
    #[test]
    fn network_and_install_need_consent() {
        assert!(require_consent(&consented()).is_ok());
        let mut u = consented();
        revoke(&mut u);
        assert_eq!(
            require_consent(&u).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            require_consent(&UpdateSettings::default())
                .unwrap_err()
                .kind,
            ErrorKind::InvalidInput
        );
    }

    /// 收回同意時正在查的那一次，結果不寫回（不然收回後又出現「有新版」）。
    #[test]
    fn a_check_finishing_after_revoke_is_not_recorded() {
        let mut u = consented();
        record_check(&mut u, 500, Some(found("0.1.4")));
        assert_eq!(u.last_check, Some(500));
        assert_eq!(u.found.as_ref().unwrap().version, "0.1.4");
        revoke(&mut u);
        record_check(&mut u, 600, Some(found("0.1.5")));
        assert_eq!(u.last_check, None);
        assert_eq!(u.found, None);
    }

    /// 查不到新版（發布被撤下）或查到的不是已下載的那一版時，下載的檔不留。
    #[test]
    fn withdrawn_or_replaced_release_drops_the_download() {
        let f = found("0.1.3");
        assert!(!stale_download(None, None));
        assert!(!stale_download(None, Some(&f)));
        assert!(!stale_download(Some(f.code), Some(&f)));
        assert!(stale_download(Some(f.code), None));
        assert!(stale_download(Some(f.code), Some(&found("0.1.4"))));
    }

    fn info(readable: bool, signer_readable: bool, same_signer: bool) -> ApkInfo {
        ApkInfo {
            readable,
            same_package: true,
            version_code: 1_000_300,
            same_signer,
            signer_readable,
        }
    }

    /// 讀不到簽章和簽章不同是兩種錯誤；前者不說「可能不是正式版本」。
    #[test]
    fn unreadable_signature_is_not_a_different_signature() {
        assert!(judge(&info(true, true, true), 1_000_300, 1_000_200).is_ok());
        assert_eq!(
            judge(&info(true, true, false), 1_000_300, 1_000_200)
                .unwrap_err()
                .kind,
            ErrorKind::UpdateSignature
        );
        assert_eq!(
            judge(&info(true, false, false), 1_000_300, 1_000_200)
                .unwrap_err()
                .kind,
            ErrorKind::UpdateUnverifiable
        );
        assert_eq!(
            judge(&info(false, false, false), 1_000_300, 1_000_200)
                .unwrap_err()
                .kind,
            ErrorKind::UpdateUnverifiable
        );
        // 套件名稱或版本不對：仍是「不是這個應用的新版」
        let mut other = info(true, true, true);
        other.same_package = false;
        assert_eq!(
            judge(&other, 1_000_300, 1_000_200).unwrap_err().kind,
            ErrorKind::UpdateSignature
        );
        assert_eq!(
            judge(&info(true, true, true), 1_000_400, 1_000_200)
                .unwrap_err()
                .kind,
            ErrorKind::UpdateSignature
        );
        assert_eq!(
            judge(&info(true, true, true), 1_000_300, 1_000_300)
                .unwrap_err()
                .kind,
            ErrorKind::UpdateSignature
        );
    }
}
