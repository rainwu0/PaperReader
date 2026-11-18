//! 應用內檢查更新：問 GitHub 的 `rainwu0/PaperReader` 有沒有比這台新的版本、
//! 下載這台架構的安裝檔、核對 SHA-256。簽章憑證的核對與交給系統安裝在平台轉接器（Android 的 Kotlin）。
//!
//! 只從 GitHub 下載：查詢的網址與下載網址的開頭都寫死（`SOURCE`）。除錯版可以在編譯時用環境變數
//! `PAPERREADER_UPDATE_BASE` 換成本機的假伺服器（`scripts/fake-github-server.mjs`）；正式版沒有這段程式碼。
//! 比較新舊用 versionCode（`規格/05` 第 5 節的公式），不比字串。

use crate::error::{ApiError, ApiResult, ErrorKind};
use paperreader_translate::Cancel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// 自動檢查的間隔：一天最多一次。
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// 發布清單最多收多少。20 筆發布的 JSON 一般在幾十 KB。
pub const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;
/// 核對碼清單最多收多少。每個安裝檔一行，不到 100 位元組。
pub const MAX_SUMS_BYTES: usize = 64 * 1024;
/// 正式版的安裝檔最多多大。正式版每個架構幾十 MB，留足餘裕。
pub const RELEASE_MAX_APK: u64 = 300 * 1024 * 1024;
/// 除錯版的上限：除錯版的安裝檔帶除錯符號，模擬器驗收用的是 389 MB 到 649 MB。
pub const DEBUG_MAX_APK: u64 = 1024 * 1024 * 1024;

/// 這個建置收的安裝檔大小上限。
pub fn max_apk() -> u64 {
    if cfg!(debug_assertions) {
        DEBUG_MAX_APK
    } else {
        RELEASE_MAX_APK
    }
}
/// 核對碼清單的檔名（`scripts/release-android.ps1` 產生）。
pub const SUMS_NAME: &str = "SHA256SUMS.txt";
/// 給 GitHub 的 `User-Agent`（GitHub 拒收沒有它的請求）。只寫應用的名稱，不帶版本與裝置。
const USER_AGENT: &str = "PaperReader-update-check";

/// 查詢與下載的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// 列出發布的網址。
    pub api: String,
    /// 下載網址必須以它開頭（後面接 `<標籤>/<檔名>`）。
    pub download_prefix: String,
    /// 給使用者看的主機名稱（同意的說明裡寫連到哪裡）。
    pub host: &'static str,
    /// 除錯版的假伺服器所在的主機（`PAPERREADER_UPDATE_BASE`）：轉址另外准到這台，可以是 http。正式版是 `None`。
    pub dev_host: Option<String>,
}

const API: &str = "https://api.github.com/repos/rainwu0/PaperReader/releases?per_page=20";
const DOWNLOAD_PREFIX: &str = "https://github.com/rainwu0/PaperReader/releases/download/";

/// 這個建置用的來源。正式版一律是 GitHub。
pub fn source() -> Source {
    #[cfg(debug_assertions)]
    if let Some(base) = option_env!("PAPERREADER_UPDATE_BASE") {
        let base = base.trim_end_matches('/');
        return Source {
            api: format!("{base}/repos/rainwu0/PaperReader/releases?per_page=20"),
            download_prefix: format!("{base}/rainwu0/PaperReader/releases/download/"),
            host: "api.github.com",
            dev_host: reqwest::Url::parse(base)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string)),
        };
    }
    Source {
        api: API.to_string(),
        download_prefix: DOWNLOAD_PREFIX.to_string(),
        host: "api.github.com",
        dev_host: None,
    }
}

/// 轉址可以去的地方：只有 https 的 GitHub 主機。GitHub 的下載網址會轉到
/// `release-assets.githubusercontent.com`（目前是這個），舊的是 `objects.githubusercontent.com`，
/// 所以收 `*.githubusercontent.com`。除錯版另外准假伺服器那一台（http）。
pub fn redirect_allowed(url: &reqwest::Url, src: &Source) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if src.dev_host.as_deref() == Some(host) {
        return matches!(url.scheme(), "http" | "https");
    }
    url.scheme() == "https"
        && (host == "github.com"
            || host == "api.github.com"
            || host
                .strip_suffix(".githubusercontent.com")
                .is_some_and(|sub| !sub.is_empty() && !sub.contains('.')))
}

/// 這個安裝檔的架構在檔名裡的寫法（`規格/05` 第 7 節）。其他架構沒有發布安裝檔，不提供更新。
pub fn abi() -> Option<&'static str> {
    if cfg!(target_arch = "aarch64") {
        Some("arm64")
    } else if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else {
        None
    }
}

/// 對外的版號 `a.b.c` 或 `a.b.c.d`（`規格/05` 第 1、5 節）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub a: u64,
    pub b: u64,
    pub c: u64,
    pub d: u64,
}

impl Version {
    /// 讀對外的版號：三段或四段不補零的十進位整數，各段在上限內；第四段是 0 時不寫（寫了不收）。
    pub fn parse(s: &str) -> Option<Version> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let mut n = [0u64; 4];
        for (i, p) in parts.iter().enumerate() {
            let ok = !p.is_empty()
                && p.len() <= 4
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'));
            if !ok {
                return None;
            }
            n[i] = p.parse().ok()?;
        }
        let v = Version {
            a: n[0],
            b: n[1],
            c: n[2],
            d: n[3],
        };
        let within = v.a <= 20 && v.b <= 99 && v.c <= 9999 && v.d <= 99;
        if !within || (parts.len() == 4 && v.d == 0) {
            return None;
        }
        Some(v)
    }

    /// git 標籤：`v` 加對外的版號。
    pub fn from_tag(tag: &str) -> Option<Version> {
        Version::parse(tag.strip_prefix('v')?)
    }

    /// versionCode ＝ a×10⁸ ＋ b×10⁶ ＋ c×10² ＋ d。
    pub fn code(&self) -> u64 {
        self.a * 100_000_000 + self.b * 1_000_000 + self.c * 100 + self.d
    }

    pub fn display(&self) -> String {
        if self.d == 0 {
            format!("{}.{}.{}", self.a, self.b, self.c)
        } else {
            format!("{}.{}.{}.{}", self.a, self.b, self.c, self.d)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    /// GitHub 算的 `sha256:<十六進位>`；舊的附件沒有。
    #[serde(default)]
    pub digest: Option<String>,
}

/// 找到的新版本。記在設定檔裡（重開應用、一天之內不再查時，首頁照樣提示）；畫面只拿到 [`AvailableView`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Available {
    /// 對外的版號。
    pub version: String,
    pub code: u64,
    /// 那一版的變更摘要（Release 說明，原樣的文字）。
    pub notes: String,
    /// 安裝檔的大小。
    pub size: u64,
    pub apk_name: String,
    pub apk_url: String,
    pub sums_url: String,
    /// GitHub 給的 SHA-256（有的話也要相符）。
    pub digest: Option<String>,
}

/// 交給畫面的那一部分（不含網址）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableView {
    pub version: String,
    pub code: u64,
    pub notes: String,
    pub size: u64,
}

impl Available {
    pub fn view(&self) -> AvailableView {
        AvailableView {
            version: self.version.clone(),
            code: self.code,
            notes: self.notes.clone(),
            size: self.size,
        }
    }

    /// 設定檔裡讀回來的這一版，下載前再確認一次：版號合 `規格/05`、versionCode 照公式、
    /// 檔名是這台架構的安裝檔名，兩個網址正好是 `<下載位置>v<版號>/<檔名>` 與 `…/SHA256SUMS.txt`，大小在上限內。
    /// 和 [`pick`] 組出來的形狀一樣，所以設定檔被改過、或檔名帶 `../` 的都不收。
    pub fn matches_source(&self, src: &Source, abi: &str, max_apk: u64) -> bool {
        let Some(v) = Version::parse(&self.version) else {
            return false;
        };
        let base = format!("{}v{}/", src.download_prefix, self.version);
        v.display() == self.version
            && v.code() == self.code
            && self.apk_name == apk_name(&v, abi)
            && self.apk_url == format!("{base}{}", self.apk_name)
            && self.sums_url == format!("{base}{SUMS_NAME}")
            && self.size <= max_apk
    }
}

/// 安裝檔的檔名（`規格/05` 第 7 節）。
pub fn apk_name(v: &Version, abi: &str) -> String {
    format!("PaperReader-{}-{abi}.apk", v.display())
}

/// 從發布清單挑出比 `installed` 新、而且有這台架構的安裝檔與核對碼清單的最新一版。
/// 略過草稿、標籤不合規格的、下載網址不在 `prefix/<標籤>/` 底下的、安裝檔大於 `max_apk` 的。
pub fn pick(
    releases: &[GhRelease],
    abi: &str,
    installed: u64,
    prefix: &str,
    max_apk: u64,
) -> Option<Available> {
    let mut best: Option<Available> = None;
    for r in releases {
        if r.draft {
            continue;
        }
        let Some(v) = Version::from_tag(&r.tag_name) else {
            continue;
        };
        if v.code() <= installed || best.as_ref().is_some_and(|b| b.code >= v.code()) {
            continue;
        }
        let under = format!("{prefix}{}/", r.tag_name);
        let find = |name: &str| {
            r.assets
                .iter()
                .find(|a| a.name == name && a.browser_download_url == format!("{under}{name}"))
        };
        let name = apk_name(&v, abi);
        let (Some(apk), Some(sums)) = (find(&name), find(SUMS_NAME)) else {
            continue;
        };
        if apk.size > max_apk {
            continue;
        }
        best = Some(Available {
            version: v.display(),
            code: v.code(),
            notes: r.body.clone().unwrap_or_default().trim().to_string(),
            size: apk.size,
            apk_name: name,
            apk_url: apk.browser_download_url.clone(),
            sums_url: sums.browser_download_url.clone(),
            digest: apk.digest.clone(),
        });
    }
    best
}

/// 從核對碼清單（`<十六進位>  <檔名>`，每行一筆；`*<檔名>` 也收）取出某個檔的 SHA-256（小寫）。
pub fn sum_for(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        let ok = file == name
            && it.next().is_none()
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit());
        ok.then(|| hash.to_ascii_lowercase())
    })
}

/// GitHub 的 `digest` 欄位（`sha256:<十六進位>`）取出十六進位；別的演算法或格式不對交回 `None`。
pub fn digest_sha256(d: &str) -> Option<String> {
    let hex = d.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 自動檢查是不是該查了（上次查的超過一天，或時鐘被往回調）。
pub fn due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(t) => now < t || now - t >= CHECK_INTERVAL_SECS,
    }
}

// ---------- 連網 ----------

/// 更新專用的用戶端：TLS 設定和翻譯共用，另外不帶 `Referer`、轉址只准到
/// [`redirect_allowed`] 的主機（最多 10 次；不准的停在 3xx，交回 [`ErrorKind::UpdateBadRelease`]）。
/// 正式版只准 https；除錯版的假伺服器是 http，所以只有設了 `dev_host` 時才放寬。
fn client(src: &Source) -> reqwest::Client {
    let allowed = src.clone();
    paperreader_translate::http::builder()
        .referer(false)
        .https_only(src.dev_host.is_none())
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if redirect_allowed(attempt.url(), &allowed) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .expect("static client configuration")
}

fn net_error(e: &reqwest::Error) -> ApiError {
    // 細節不帶網址以外的東西；網址是公開的 GitHub 位置
    ApiError::new(ErrorKind::UpdateNetwork, format!("network: {e}"))
}

fn status_error(status: u16) -> ApiError {
    match status {
        403 | 429 => ApiError::new(ErrorKind::UpdateRateLimited, format!("http {status}")),
        // 附件或核對碼清單不見了：那一版被撤下
        404 | 410 => ApiError::new(ErrorKind::UpdateGone, format!("http {status}")),
        _ => ApiError::new(ErrorKind::UpdateBadRelease, format!("http {status}")),
    }
}

async fn get(src: &Source, url: &str, accept: &str) -> ApiResult<reqwest::Response> {
    let resp = client(src)
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, accept)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| net_error(&e))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status_error(status));
    }
    Ok(resp)
}

/// 把一塊收進 `buf`；收了會超過 `cap` 就不收，交回 [`ErrorKind::UpdateBadRelease`]。
pub fn push_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) -> ApiResult<()> {
    if buf.len().saturating_add(chunk.len()) > cap {
        return Err(ApiError::new(ErrorKind::UpdateBadRelease, "too large"));
    }
    buf.extend_from_slice(chunk);
    Ok(())
}

/// 讀完一個回應，最多 `cap` 位元組。公布的長度已經超過就不讀。
async fn body_capped(mut resp: reqwest::Response, cap: usize) -> ApiResult<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(ApiError::new(ErrorKind::UpdateBadRelease, "too large"));
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| net_error(&e))? {
        push_capped(&mut buf, &chunk, cap)?;
    }
    Ok(buf)
}

/// 問 GitHub 有沒有比 `installed` 新的版本（一個 GET，不帶任何資料）。
pub async fn check(src: &Source, abi: &str, installed: u64) -> ApiResult<Option<Available>> {
    let resp = get(src, &src.api, "application/vnd.github+json")
        .await
        // 清單本身不會被撤下；404 是倉庫的位置不對
        .map_err(|e| {
            if e.kind == ErrorKind::UpdateGone {
                ApiError::new(ErrorKind::UpdateBadRelease, e.detail)
            } else {
                e
            }
        })?;
    let body = body_capped(resp, MAX_LIST_BYTES).await?;
    let releases: Vec<GhRelease> = serde_json::from_slice(&body)
        .map_err(|_| ApiError::new(ErrorKind::UpdateBadRelease, "release list"))?;
    Ok(pick(
        &releases,
        abi,
        installed,
        &src.download_prefix,
        max_apk(),
    ))
}

/// 下載的進度（畫面輪詢）。
#[derive(Default)]
pub struct UpdateSlot {
    pub running: AtomicBool,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: Mutex<Option<Cancel>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 算一個檔的 SHA-256（小寫十六進位）。
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn io_error(e: &std::io::Error) -> ApiError {
    if e.raw_os_error() == Some(28) {
        ApiError::new(ErrorKind::NoSpace, "no space")
    } else {
        ApiError::new(ErrorKind::Io, "write")
    }
}

/// 取核對碼清單，找出安裝檔的 SHA-256。GitHub 也給了 `digest` 時兩者必須相同。
pub async fn expected_sha256(src: &Source, a: &Available, cancel: &Cancel) -> ApiResult<String> {
    let body = tokio::select! {
        r = async {
            let resp = get(src, &a.sums_url, "application/octet-stream").await?;
            body_capped(resp, MAX_SUMS_BYTES).await
        } => r?,
        () = cancel.cancelled() => return Err(ApiError::new(ErrorKind::InvalidInput, "cancelled")),
    };
    let text = String::from_utf8(body)
        .map_err(|_| ApiError::new(ErrorKind::UpdateBadRelease, "sums not utf-8"))?;
    let sum = sum_for(&text, &a.apk_name)
        .ok_or_else(|| ApiError::new(ErrorKind::UpdateBadRelease, "no sum for apk"))?;
    if let Some(d) = a.digest.as_deref().and_then(digest_sha256)
        && d != sum
    {
        return Err(ApiError::new(ErrorKind::UpdateChecksum, "digest differs"));
    }
    Ok(sum)
}

/// 下載安裝檔到 `dest`：邊收邊寫進 `<dest>.part`、邊算 SHA-256；和 `sha256` 相符才改名成 `dest`。
/// 不符、取消、連線中斷、寫不進去時刪掉 `.part`，不留下看起來完整的檔。
pub async fn download(
    src: &Source,
    a: &Available,
    sha256: &str,
    dest: &Path,
    slot: &UpdateSlot,
    cancel: &Cancel,
) -> ApiResult<()> {
    let part = dest.with_extension("apk.part");
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io_error(&e))?;
    }
    let r = stream(src, a, sha256, &part, slot, cancel).await;
    match r {
        Ok(()) => std::fs::rename(&part, dest).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            io_error(&e)
        }),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

async fn stream(
    src: &Source,
    a: &Available,
    sha256: &str,
    part: &Path,
    slot: &UpdateSlot,
    cancel: &Cancel,
) -> ApiResult<()> {
    use std::io::Write;
    let cancelled = || ApiError::new(ErrorKind::InvalidInput, "cancelled");
    let mut resp = tokio::select! {
        r = client(src)
            .get(&a.apk_url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            // 幾十 MB 在慢的網路上要很久；連線本身仍有 15 秒的上限
            .timeout(Duration::from_secs(60 * 60))
            .send() => r.map_err(|e| net_error(&e))?,
        () = cancel.cancelled() => return Err(cancelled()),
    };
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status_error(status));
    }
    slot.done.store(0, Ordering::Relaxed);
    slot.total.store(a.size, Ordering::Relaxed);
    let mut out = std::io::BufWriter::new(std::fs::File::create(part).map_err(|e| io_error(&e))?);
    let mut h = Sha256::new();
    let mut got: u64 = 0;
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c.map_err(|e| net_error(&e))?,
            () = cancel.cancelled() => return Err(cancelled()),
        };
        let Some(chunk) = chunk else { break };
        got += chunk.len() as u64;
        // 比公布的大小還大就不再收（不讓一個錯的回應塞滿手機）
        if got > a.size {
            return Err(ApiError::new(
                ErrorKind::UpdateChecksum,
                "larger than listed",
            ));
        }
        h.update(&chunk);
        out.write_all(&chunk).map_err(|e| io_error(&e))?;
        slot.done.store(got, Ordering::Relaxed);
    }
    out.flush().map_err(|e| io_error(&e))?;
    out.get_ref().sync_all().map_err(|e| io_error(&e))?;
    if hex(&h.finalize()) != sha256 {
        return Err(ApiError::new(ErrorKind::UpdateChecksum, "sha256 mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_follow_spec_05() {
        let v = Version::parse("0.1.12").unwrap();
        assert_eq!(v.code(), 1_001_200);
        assert_eq!(v.display(), "0.1.12");
        assert_eq!(Version::parse("0.1.1.1").unwrap().code(), 1_000_101);
        assert_eq!(Version::parse("0.2.0").unwrap().code(), 2_000_000);
        assert_eq!(Version::parse("1.0.0").unwrap().code(), 100_000_000);
        assert_eq!(
            Version::parse("20.99.9999.99").unwrap().code(),
            2_099_999_999
        );
        for bad in [
            "0.1",
            "0.1.0.0",
            "0.01.1",
            "0.1.1.",
            "a.b.c",
            "21.0.0",
            "0.100.0",
            "0.1.10000",
            "0.1.1.100",
            "0.1.1+1",
            "",
            "0.1.1.1.1",
            " 0.1.1",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(Version::from_tag("v0.1.12.3").unwrap().code(), 1_001_203);
        assert!(Version::from_tag("0.1.12").is_none());
    }

    fn rel(tag: &str, draft: bool, assets: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            draft,
            body: Some(format!("notes {tag}\n")),
            assets: assets
                .iter()
                .map(|n| GhAsset {
                    name: (*n).into(),
                    size: 10,
                    browser_download_url: format!("{DOWNLOAD_PREFIX}{tag}/{n}"),
                    digest: None,
                })
                .collect(),
        }
    }

    #[test]
    fn picks_newest_release_with_this_abi() {
        let all = [
            "PaperReader-0.1.2-x86_64.apk",
            "PaperReader-0.1.2-arm64.apk",
            SUMS_NAME,
        ];
        let releases = vec![
            rel("v0.1.3", true, &["PaperReader-0.1.3-arm64.apk", SUMS_NAME]),
            rel("nightly", false, &[SUMS_NAME]),
            rel("v0.1.2", false, &all),
            rel("v0.1.1", false, &["PaperReader-0.1.1-arm64.apk", SUMS_NAME]),
            rel("v0.1.0", false, &[]),
        ];
        let a = pick(
            &releases,
            "arm64",
            1_000_100,
            DOWNLOAD_PREFIX,
            RELEASE_MAX_APK,
        )
        .unwrap();
        assert_eq!(a.version, "0.1.2");
        assert_eq!(a.code, 1_000_200);
        assert_eq!(a.notes, "notes v0.1.2");
        assert_eq!(a.apk_name, "PaperReader-0.1.2-arm64.apk");
        assert!(a.sums_url.ends_with("/v0.1.2/SHA256SUMS.txt"));
        // 已經是最新
        assert!(
            pick(
                &releases,
                "arm64",
                1_000_200,
                DOWNLOAD_PREFIX,
                RELEASE_MAX_APK
            )
            .is_none()
        );
        // 新的一版沒有這個架構的安裝檔：取有的那一版
        let only_arm = vec![
            rel("v0.1.4", false, &["PaperReader-0.1.4-arm64.apk", SUMS_NAME]),
            rel("v0.1.2", false, &all),
        ];
        assert_eq!(
            pick(
                &only_arm,
                "x86_64",
                1_000_100,
                DOWNLOAD_PREFIX,
                RELEASE_MAX_APK
            )
            .unwrap()
            .version,
            "0.1.2"
        );
        // 缺核對碼清單的不算
        let no_sums = vec![rel("v0.1.2", false, &["PaperReader-0.1.2-arm64.apk"])];
        assert!(pick(&no_sums, "arm64", 1000, DOWNLOAD_PREFIX, RELEASE_MAX_APK).is_none());
    }

    #[test]
    fn rejects_assets_outside_the_repository() {
        let mut r = rel("v0.1.2", false, &["PaperReader-0.1.2-arm64.apk", SUMS_NAME]);
        r.assets[0].browser_download_url = "https://example.com/PaperReader-0.1.2-arm64.apk".into();
        assert!(
            pick(
                &[r.clone()],
                "arm64",
                1000,
                DOWNLOAD_PREFIX,
                RELEASE_MAX_APK
            )
            .is_none()
        );
        // 別的標籤底下的檔也不收
        r.assets[0].browser_download_url =
            format!("{DOWNLOAD_PREFIX}v0.1.1/PaperReader-0.1.2-arm64.apk");
        assert!(pick(&[r], "arm64", 1000, DOWNLOAD_PREFIX, RELEASE_MAX_APK).is_none());
    }

    #[test]
    fn remembered_release_must_still_point_at_the_repository() {
        let src = github();
        let all = ["PaperReader-0.1.2-arm64.apk", SUMS_NAME];
        let mut a = pick(
            &[rel("v0.1.2", false, &all)],
            "arm64",
            1000,
            DOWNLOAD_PREFIX,
            RELEASE_MAX_APK,
        )
        .unwrap();
        assert!(a.matches_source(&src, "arm64", RELEASE_MAX_APK));
        // 設定檔裡記的網址被改到別處，不下載
        a.apk_url = "https://example.com/x.apk".into();
        assert!(!a.matches_source(&src, "arm64", RELEASE_MAX_APK));
        a.apk_url = format!("{DOWNLOAD_PREFIX}v0.1.2/../../x.apk");
        assert!(!a.matches_source(&src, "arm64", RELEASE_MAX_APK));
        // 記在設定檔的形狀讀得回來
        let b: Available = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn reads_sha256sums() {
        let h = "a".repeat(64);
        let text = format!(
            "{h}  PaperReader-0.1.2-arm64.apk\n{}  PaperReader-0.1.2-x86_64.apk\n",
            "B".repeat(64)
        );
        assert_eq!(
            sum_for(&text, "PaperReader-0.1.2-arm64.apk"),
            Some(h.clone())
        );
        assert_eq!(
            sum_for(&text, "PaperReader-0.1.2-x86_64.apk"),
            Some("b".repeat(64))
        );
        assert_eq!(sum_for(&format!("{h} *x.apk"), "x.apk"), Some(h.clone()));
        assert_eq!(sum_for(&text, "PaperReader-0.1.2"), None);
        assert_eq!(sum_for("abc  x.apk", "x.apk"), None);
        assert_eq!(digest_sha256(&format!("sha256:{h}")), Some(h));
        assert_eq!(digest_sha256("sha512:abc"), None);
    }

    #[test]
    fn checks_at_most_once_a_day() {
        assert!(due(None, 100));
        assert!(!due(Some(100), 100 + CHECK_INTERVAL_SECS - 1));
        assert!(due(Some(100), 100 + CHECK_INTERVAL_SECS));
        // 時鐘被往回調：照樣可以查
        assert!(due(Some(100), 50));
    }

    // ---------- 安全相關 ----------

    fn github() -> Source {
        Source {
            api: API.into(),
            download_prefix: DOWNLOAD_PREFIX.into(),
            host: "api.github.com",
            dev_host: None,
        }
    }

    /// 轉址只准到 GitHub 的主機，而且是 https。
    #[test]
    fn redirects_only_to_github_over_https() {
        let ok = |u: &str| redirect_allowed(&reqwest::Url::parse(u).unwrap(), &github());
        assert!(ok(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/x"
        ));
        assert!(ok("https://objects.githubusercontent.com/x"));
        assert!(ok(
            "https://github.com/rainwu0/PaperReader/releases/download/v0.1.2/x"
        ));
        assert!(ok(
            "https://api.github.com/repos/rainwu0/PaperReader/releases"
        ));
        // 不是 https
        assert!(!ok("http://release-assets.githubusercontent.com/x"));
        assert!(!ok("http://github.com/x"));
        // 別的主機，或長得像的
        assert!(!ok("https://example.com/x"));
        assert!(!ok("https://evilgithubusercontent.com/x"));
        assert!(!ok("https://githubusercontent.com.example.com/x"));
        assert!(!ok("https://github.com.example.com/x"));
        assert!(!ok("https://gist.github.com/x"));
        assert!(!ok("https://127.0.0.1/x"));
        // 除錯版的假伺服器：只多准那一台的 http
        let dev = Source {
            dev_host: Some("10.0.2.2".into()),
            ..github()
        };
        let dok = |u: &str| redirect_allowed(&reqwest::Url::parse(u).unwrap(), &dev);
        assert!(dok("http://10.0.2.2:8791/assets/x.apk"));
        assert!(dok("https://release-assets.githubusercontent.com/x"));
        assert!(!dok("http://10.0.2.3:8791/assets/x.apk"));
        assert!(!dok("http://objects.githubusercontent.com/x"));
    }

    /// 讀回應有上限，超過就停。
    #[test]
    fn bodies_have_a_size_cap() {
        let mut buf = Vec::new();
        assert!(push_capped(&mut buf, &[1; 6], 10).is_ok());
        assert!(push_capped(&mut buf, &[2; 4], 10).is_ok());
        assert_eq!(buf.len(), 10);
        let e = push_capped(&mut buf, &[3], 10).unwrap_err();
        assert_eq!(e.kind, ErrorKind::UpdateBadRelease);
        assert_eq!(buf.len(), 10, "超過的部分不收");
        const { assert!(MAX_LIST_BYTES <= 2 * 1024 * 1024) };
        const { assert!(MAX_SUMS_BYTES <= 64 * 1024) };
        const { assert!(RELEASE_MAX_APK <= 300 * 1024 * 1024) };
    }

    /// 公布的安裝檔大小超過上限的那一版不收。
    #[test]
    fn rejects_apks_larger_than_the_cap() {
        let mut r = rel("v0.1.2", false, &["PaperReader-0.1.2-arm64.apk", SUMS_NAME]);
        r.assets[0].size = RELEASE_MAX_APK + 1;
        assert!(
            pick(
                &[r.clone()],
                "arm64",
                1000,
                DOWNLOAD_PREFIX,
                RELEASE_MAX_APK
            )
            .is_none()
        );
        r.assets[0].size = RELEASE_MAX_APK;
        assert!(pick(&[r], "arm64", 1000, DOWNLOAD_PREFIX, RELEASE_MAX_APK).is_some());
    }

    /// 設定檔讀回的那一版，檔名、版號、網址要彼此一致，而且合版號規則。
    #[test]
    fn remembered_release_must_be_self_consistent() {
        let all = ["PaperReader-0.1.2-arm64.apk", SUMS_NAME];
        let good = pick(
            &[rel("v0.1.2", false, &all)],
            "arm64",
            1000,
            DOWNLOAD_PREFIX,
            RELEASE_MAX_APK,
        )
        .unwrap();
        let src = github();
        assert!(good.matches_source(&src, "arm64", RELEASE_MAX_APK));
        let bad = |f: &dyn Fn(&mut Available)| {
            let mut a = good.clone();
            f(&mut a);
            !a.matches_source(&src, "arm64", RELEASE_MAX_APK)
        };
        assert!(bad(&|a| a.apk_name = "../../shared_prefs/x.apk".into()));
        assert!(bad(&|a| a.apk_name = "/data/x.apk".into()));
        assert!(bad(&|a| a.apk_name = "PaperReader-0.1.2-x86_64.apk".into()));
        assert!(bad(&|a| a.apk_name = "PaperReader-0.1.3-arm64.apk".into()));
        assert!(bad(&|a| a.code = 1_000_300));
        assert!(bad(&|a| a.version = "0.1.02".into()));
        assert!(bad(&|a| a.version = "0.1.3".into()));
        assert!(bad(
            &|a| a.sums_url = format!("{DOWNLOAD_PREFIX}v0.1.2/other.txt")
        ));
        assert!(bad(
            &|a| a.sums_url = format!("{DOWNLOAD_PREFIX}v0.1.1/{SUMS_NAME}")
        ));
        assert!(bad(&|a| a.size = RELEASE_MAX_APK + 1));
        // 別的架構的手機不收這台的
        assert!(!good.matches_source(&src, "x86_64", RELEASE_MAX_APK));
    }

    /// 發布被撤下（404）和「內容不完整」分開，畫面才能清掉舊的提示。
    #[test]
    fn not_found_means_the_release_was_withdrawn() {
        assert_eq!(status_error(404).kind, ErrorKind::UpdateGone);
        assert_eq!(status_error(410).kind, ErrorKind::UpdateGone);
        assert_eq!(status_error(403).kind, ErrorKind::UpdateRateLimited);
        assert_eq!(status_error(500).kind, ErrorKind::UpdateBadRelease);
        // 被擋下的轉址（不准的主機）停在 3xx
        assert_eq!(status_error(302).kind, ErrorKind::UpdateBadRelease);
    }

    #[test]
    fn release_build_points_at_github() {
        let s = source();
        if option_env!("PAPERREADER_UPDATE_BASE").is_none() {
            assert_eq!(s.api, API);
            assert_eq!(s.download_prefix, DOWNLOAD_PREFIX);
        }
        assert!(API.starts_with("https://api.github.com/repos/rainwu0/PaperReader/"));
        assert!(DOWNLOAD_PREFIX.starts_with("https://github.com/rainwu0/PaperReader/"));
    }
}
