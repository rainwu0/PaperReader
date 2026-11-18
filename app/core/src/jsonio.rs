//! JSON 檔案的讀取與不留半成品的寫入。

use crate::error::{LibError, Result, io_error};
use crate::ids::new_id;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

/// 分段讀檔時一次讀多少位元組。
const HASH_BUF: usize = 1 << 20;

/// 版本記號：檔案內容的 SHA-256（64 個小寫十六進位字元）。
///
/// 序列化成一個字串（交給畫面的型別裡帶著它時用）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct VersionToken(pub String);

pub fn sha256_hex(bytes: &[u8]) -> String {
    SHA256_CALLS.with(|c| c.set(c.get() + 1));
    hex(&Sha256::digest(bytes))
}

thread_local! {
    static SHA256_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// 只給測試用：這個執行緒到現在呼叫了幾次 `sha256_hex`。驗「不算版本記號」的讀法（`read_for_index`）真的沒算。
#[doc(hidden)]
pub fn sha256_calls() -> u64 {
    SHA256_CALLS.with(|c| c.get())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 分段讀完 `input`，邊讀邊算 SHA-256，不把整份讀進記憶體。核心裡算檔案指紋的迴圈只有這一個。
///
/// 每讀到一段先交給 `each`（寫到別處、留下檔頭…），它的錯誤原樣交回；讀到 `Interrupted` 重讀，
/// 其他讀取的錯誤經 `read_err` 轉成錯誤（讀的一方與寫的一方各自對應到不同的路徑或錯誤種類）。
/// 交回（指紋, 位元組數）。
pub(crate) fn hash_reader(
    input: &mut dyn Read,
    read_err: impl FnOnce(std::io::Error) -> LibError,
    mut each: impl FnMut(&[u8]) -> Result<()>,
) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_BUF];
    let mut total = 0u64;
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(read_err(e)),
        };
        each(&buf[..n])?;
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex(&hasher.finalize()), total))
}

/// 分段讀一個檔算 SHA-256。打不開、讀不了都是錯誤。交回（指紋, 位元組數）。
pub(crate) fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut f = fs::File::open(path).map_err(|e| io_error(path, e))?;
    hash_reader(&mut f, |e| io_error(path, e), |_| Ok(()))
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// 讀 JSON。開頭的位元組順序記號（記事本常加）略過；記號依檔案的原始內容算。
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<(T, VersionToken)> {
    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    let token = VersionToken(sha256_hex(&bytes));
    Ok((parse_json(path, &bytes)?, token))
}

/// 同 `read_json`，但不算版本記號（只讀不寫的一方用，例如給全文搜尋的索引讀檔）。
pub fn read_json_untokened<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
    parse_json(path, &bytes)
}

fn parse_json<T: DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T> {
    let body = bytes.strip_prefix(&BOM).unwrap_or(bytes);
    serde_json::from_slice(body).map_err(|e| LibError::Unreadable {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

/// 兩個空格縮排、欄位順序照結構、結尾一個換行。
pub fn to_canonical_json<T: Serialize>(value: &T) -> Result<String> {
    let mut s = serde_json::to_string_pretty(value).map_err(|e| LibError::InvalidInput {
        reason: e.to_string(),
    })?;
    s.push('\n');
    Ok(s)
}

/// 先在同一資料夾寫 `.pr-tmp-<代號>`，`sync_all`，再改名蓋過目標。
/// 最後對資料夾做一次 `sync_all`，失敗不算錯（斷電只盡力而為）。
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<VersionToken> {
    let text = to_canonical_json(value)?;
    write_bytes_atomic(path, text.as_bytes())
}

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<VersionToken> {
    let dir = path.parent().ok_or_else(|| LibError::InvalidInput {
        reason: format!("no parent folder: {}", path.display()),
    })?;
    let tmp = dir.join(format!(".pr-tmp-{}", new_id()));
    let result = (|| -> Result<()> {
        let mut f = fs::File::create(&tmp).map_err(|e| io_error(&tmp, e))?;
        f.write_all(bytes).map_err(|e| io_error(&tmp, e))?;
        f.sync_all().map_err(|e| io_error(&tmp, e))?;
        drop(f);
        fs::rename(&tmp, path).map_err(|e| io_error(path, e))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(dir);
    Ok(VersionToken(sha256_hex(bytes)))
}

/// 盡力讓資料夾項目（改名）寫進儲存裝置；在不支援的系統上無作用。
pub fn sync_dir(dir: &Path) {
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}
