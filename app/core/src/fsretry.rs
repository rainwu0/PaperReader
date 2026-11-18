//! 改名與刪除遇到 Windows 上「暫時的」鎖時，短暫重試。
//!
//! Windows 上別的程式（即時防毒、索引、檔案總管的預覽）剛好開著一個檔時，改名或刪除那個檔、或改名
//! 含有它的資料夾，會回 ERROR_ACCESS_DENIED（5）或 ERROR_SHARING_VIOLATION（32），對方放開後就好了。
//! 這裡只認這兩個代碼、只在 Windows 上等，最多等 [`TRANSIENT_LIMIT`]；等不到交回最後一次的錯誤
//! （種類與不重試時相同）。其他平台、其他錯誤一律第一次就交回。
//!
//! 做法參考 Go 工具鏈的 `cmd/internal/robustio`（同樣兩個代碼、總時間上限 2 秒、間隔逐次加長）、
//! rustup 的 `utils::rename`（存取被拒時退避重試）與 node 的 graceful-fs（Windows 上改名重試），
//! 依它們的文件與原始碼的做法自己寫，沒有沿用程式碼。
//!
//! 核心自己的寫入目前不用這裡（這一層不自己重試）；由外殼在搬文獻庫這類整個資料夾的
//! 改名與刪除上使用。

use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

/// 總共最多等多久（Go 的 robustio 也是 2 秒）。真的沒有權限時，失敗也只晚這麼久回報。
pub const TRANSIENT_LIMIT: Duration = Duration::from_secs(2);

/// 第一次等的時間；之後每次加倍，單次最多 [`MAX_PAUSE`]。
const FIRST_PAUSE: Duration = Duration::from_millis(10);
const MAX_PAUSE: Duration = Duration::from_millis(250);

/// 是不是可能等一下就好的錯誤：只有 Windows 上的 ERROR_ACCESS_DENIED（5）與
/// ERROR_SHARING_VIOLATION（32）。只看作業系統的代碼，不看 `ErrorKind`（自己造的
/// `PermissionDenied` 不算）。
pub fn is_transient(e: &io::Error) -> bool {
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        matches!(
            e.raw_os_error(),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
        )
    }
    #[cfg(not(windows))]
    {
        let _ = e;
        false
    }
}

/// 執行 `op`；遇到 [`is_transient`] 的錯誤時等一下再試，到累計的等待時間會超過 `limit` 為止。
/// 上限只算等待，不算 `op` 本身花的時間：刪一個大資料夾第一次就花掉兩秒以上時，仍然可以重試
/// 呼叫端握著文獻庫的鎖時，最壞多等 `limit`。
/// `op` 要能重做（改名、刪除這類失敗時沒有改到任何東西，或重做會從停下的地方接著做）。
pub fn retry_transient_within<T>(
    limit: Duration,
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut waited = Duration::ZERO;
    let mut pause = FIRST_PAUSE;
    loop {
        match op() {
            Err(e) if is_transient(&e) && waited + pause <= limit => {
                thread::sleep(pause);
                waited += pause;
                pause = (pause * 2).min(MAX_PAUSE);
            }
            other => return other,
        }
    }
}

/// [`retry_transient_within`]，上限 [`TRANSIENT_LIMIT`]。
pub fn retry_transient<T>(op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    retry_transient_within(TRANSIENT_LIMIT, op)
}

/// `std::fs::rename`，遇到暫時的鎖時短暫重試。
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    retry_transient(|| std::fs::rename(from, to))
}

/// `std::fs::remove_dir_all`，遇到暫時的鎖時短暫重試（重做時從還沒刪的接著刪）。
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    retry_transient(|| std::fs::remove_dir_all(path))
}
