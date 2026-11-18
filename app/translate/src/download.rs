//! 下載離線翻譯的模型檔：取回、（需要時）解開 zstd 壓縮、核對 SHA-256。
//!
//! 只交回核對過的位元組，寫到哪裡由外殼決定。

use crate::error::{ErrorKind, Result, TranslateError};
use crate::{Cancel, http};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

/// 一個要下載的檔。
#[derive(Debug, Clone, Copy)]
pub struct RemoteFile {
    pub url: &'static str,
    /// 下載的大小（壓縮後）。
    pub size: u64,
    /// 是不是 zstd 壓縮過。
    pub zstd: bool,
    /// 解開後的 SHA-256（十六進位小寫）。
    pub sha256: &'static str,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 下載一個檔，每收到一段就把累計位元組加進 `progress`。交回解開、核對過的內容。
pub async fn fetch_verified(
    file: &RemoteFile,
    progress: &AtomicU64,
    cancel: &Cancel,
) -> Result<Vec<u8>> {
    let client = http::build_client();
    let mut resp = client
        .get(file.url)
        // 幾十 MB 的模型檔在慢的網路上要很久；連線本身仍有 15 秒的上限
        .timeout(std::time::Duration::from_secs(30 * 60))
        .send()
        .await
        .map_err(|e| TranslateError::network(&e))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(TranslateError::from_status(status, "", None));
    }
    let mut raw: Vec<u8> = Vec::with_capacity(file.size as usize);
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c.map_err(|e| TranslateError::network(&e))?,
            () = cancel.cancelled() => {
                return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled"));
            }
        };
        let Some(chunk) = chunk else { break };
        progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        raw.extend_from_slice(&chunk);
    }
    let data = if file.zstd {
        let mut out = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(raw.as_slice())
            .map_err(|_| TranslateError::new(ErrorKind::BadResponse, "bad zstd header"))?
            .read_to_end(&mut out)
            .map_err(|_| TranslateError::new(ErrorKind::BadResponse, "bad zstd data"))?;
        out
    } else {
        raw
    };
    if hex(&Sha256::digest(&data)) != file.sha256 {
        return Err(TranslateError::new(
            ErrorKind::BadResponse,
            "checksum mismatch",
        ));
    }
    Ok(data)
}

/// `fetch_to_file` 的錯誤：下載本身（連不上、核對不合、取消）或寫檔。
#[derive(Debug, thiserror::Error)]
pub enum FetchFileError {
    #[error(transparent)]
    Fetch(#[from] TranslateError),
    #[error("write: {0}")]
    Io(#[from] std::io::Error),
}

/// 下載一個不壓縮的大檔（例如一百多 MB 的版面模型），邊收邊寫進 `<dest>.part`、邊算 SHA-256，
/// 核對通過才改名成 `dest`；取消、連線中斷、核對不合、寫不進去時刪掉 `.part`，不留下看起來完整的檔。
/// 不把整個檔放在記憶體裡（`fetch_verified` 會）。
pub async fn fetch_to_file(
    file: &RemoteFile,
    dest: &std::path::Path,
    progress: &AtomicU64,
    cancel: &Cancel,
) -> std::result::Result<(), FetchFileError> {
    if file.zstd {
        return Err(
            TranslateError::new(ErrorKind::Config, "zstd files go through fetch_verified").into(),
        );
    }
    let part = {
        let mut name = dest.file_name().unwrap_or_default().to_os_string();
        name.push(".part");
        dest.with_file_name(name)
    };
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let r = stream_to(file, &part, progress, cancel).await;
    match r {
        Ok(()) => {
            std::fs::rename(&part, dest).inspect_err(|_| {
                let _ = std::fs::remove_file(&part);
            })?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

async fn stream_to(
    file: &RemoteFile,
    part: &std::path::Path,
    progress: &AtomicU64,
    cancel: &Cancel,
) -> std::result::Result<(), FetchFileError> {
    use std::io::Write;
    let client = http::build_client();
    let mut resp = tokio::select! {
        r = client
            .get(file.url)
            // 一百多 MB 的檔在慢的網路上要很久；連線本身仍有 15 秒的上限
            .timeout(std::time::Duration::from_secs(60 * 60))
            .send() => r.map_err(|e| TranslateError::network(&e))?,
        () = cancel.cancelled() => {
            return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled").into());
        }
    };
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(TranslateError::from_status(status, "", None).into());
    }
    // 寫檔是同步的：一次一小段，比網路快得多，不另開執行緒
    let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(part)?);
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c.map_err(|e| TranslateError::network(&e))?,
            () = cancel.cancelled() => {
                return Err(TranslateError::new(ErrorKind::Cancelled, "cancelled").into());
            }
        };
        let Some(chunk) = chunk else { break };
        total += chunk.len() as u64;
        // 比申報的大小大很多：不是要的檔，不必收完
        if total > file.size.saturating_mul(2).max(1 << 20) {
            return Err(TranslateError::new(ErrorKind::BadResponse, "larger than expected").into());
        }
        hasher.update(&chunk);
        out.write_all(&chunk)?;
        progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    if hex(&hasher.finalize()) != file.sha256 {
        return Err(TranslateError::new(ErrorKind::BadResponse, "checksum mismatch").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }
}
