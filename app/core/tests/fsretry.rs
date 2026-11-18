//! 改名與刪除遇到 Windows 上暫時的鎖時短暫重試。
//!
//! 鎖用另一個執行緒開著檔、過一小段時間放開來重現：不共用（`share_mode(0)`）開著的檔改名或刪除回
//! ERROR_SHARING_VIOLATION（32）；資料夾裡有檔開著時改名那個資料夾回 ERROR_ACCESS_DENIED（5）。

use paperreader_core::fsretry::{is_transient, retry_transient_within};
use std::cell::Cell;
use std::io;
use std::time::Duration;

#[test]
fn other_errors_fail_at_once() {
    let calls = Cell::new(0);
    let r: io::Result<()> = retry_transient_within(Duration::from_secs(2), || {
        calls.set(calls.get() + 1);
        Err(io::Error::from(io::ErrorKind::NotFound))
    });
    assert_eq!(r.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(calls.get(), 1);
}

#[test]
fn success_is_returned_at_once() {
    let calls = Cell::new(0);
    let r = retry_transient_within(Duration::from_secs(2), || {
        calls.set(calls.get() + 1);
        Ok(7)
    });
    assert_eq!(r.unwrap(), 7);
    assert_eq!(calls.get(), 1);
}

/// 其他平台沒有這種暫時的鎖：存取被拒照舊立刻失敗。
#[cfg(not(windows))]
#[test]
fn permission_denied_is_not_retried_elsewhere() {
    let calls = Cell::new(0);
    let r: io::Result<()> = retry_transient_within(Duration::from_secs(2), || {
        calls.set(calls.get() + 1);
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    });
    assert!(r.is_err());
    assert_eq!(calls.get(), 1);
    assert!(!is_transient(&io::Error::from_raw_os_error(13)));
}

#[cfg(windows)]
mod windows {
    use super::*;
    use paperreader_core::fsretry;
    use std::fs;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    const ACCESS_DENIED: i32 = 5;
    const SHARING_VIOLATION: i32 = 32;

    /// 另一個執行緒開著的檔。放開的時間由測試決定（`release_after`），不靠開檔之後經過多久：
    /// 主機很慢時，測試確認鎖還在的那一步也不會落在鎖放開之後。
    struct Held {
        release: mpsc::Sender<Duration>,
        handle: thread::JoinHandle<()>,
    }

    impl Held {
        /// 從現在起 `after` 之後放開。
        fn release_after(&self, after: Duration) {
            self.release.send(after).unwrap();
        }

        fn join(self) {
            let _ = self.release.send(Duration::ZERO);
            self.handle.join().unwrap();
        }
    }

    /// 在另一個執行緒開著 `path`（`share` 是共用方式；`None` 用標準的開法），開好之後才交回。
    fn hold(path: &Path, share: Option<u32>) -> Held {
        let path: PathBuf = path.to_path_buf();
        let (opened_tx, opened_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<Duration>();
        let handle = thread::spawn(move || {
            let mut o = fs::OpenOptions::new();
            o.read(true);
            if let Some(s) = share {
                o.share_mode(s);
            }
            let f = o.open(&path).unwrap();
            opened_tx.send(()).unwrap();
            let after = release_rx.recv().unwrap_or(Duration::ZERO);
            thread::sleep(after);
            drop(f);
        });
        opened_rx.recv().unwrap();
        Held { release, handle }
    }

    #[test]
    fn transient_errors_are_access_denied_and_sharing_violation() {
        assert!(is_transient(&io::Error::from_raw_os_error(ACCESS_DENIED)));
        assert!(is_transient(&io::Error::from_raw_os_error(
            SHARING_VIOLATION
        )));
        assert!(!is_transient(&io::Error::from_raw_os_error(2))); // 找不到檔
        assert!(!is_transient(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn a_rename_waits_for_a_file_held_without_sharing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        fs::write(&a, b"x").unwrap();
        let h = hold(&a, Some(0));
        // 鎖確實在：不重試的改名失敗
        let raw = fs::rename(&a, &b).unwrap_err();
        assert_eq!(raw.raw_os_error(), Some(SHARING_VIOLATION));
        h.release_after(Duration::from_millis(150));
        retry_transient_within(Duration::from_secs(2), || fs::rename(&a, &b)).unwrap();
        assert!(b.is_file() && !a.exists());
        h.join();
    }

    /// 這次在 GitHub 上失敗的那一種：資料夾裡有東西開著時改名資料夾。
    #[test]
    fn a_folder_rename_waits_for_an_open_file_inside() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("staging");
        let to = dir.path().join("library");
        fs::create_dir_all(from.join("papers")).unwrap();
        fs::write(from.join("papers/x.pdf"), b"%PDF").unwrap();
        let h = hold(&from.join("papers/x.pdf"), None);
        let raw = fs::rename(&from, &to).unwrap_err();
        assert_eq!(raw.raw_os_error(), Some(ACCESS_DENIED));
        h.release_after(Duration::from_millis(150));
        fsretry::rename(&from, &to).unwrap();
        assert!(to.join("papers/x.pdf").is_file() && !from.exists());
        h.join();
    }

    #[test]
    fn a_folder_removal_waits_for_a_file_held_without_sharing() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        fs::create_dir_all(old.join("papers")).unwrap();
        fs::write(old.join("papers/x.pdf"), b"%PDF").unwrap();
        let h = hold(&old.join("papers/x.pdf"), Some(0));
        h.release_after(Duration::from_millis(150));
        fsretry::remove_dir_all(&old).unwrap();
        assert!(!old.exists());
        h.join();
    }

    #[test]
    fn a_lock_held_past_the_limit_returns_the_original_error() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        fs::write(&a, b"x").unwrap();
        // 測試做完才放開
        let h = hold(&a, Some(0));
        let start = Instant::now();
        let e =
            retry_transient_within(Duration::from_millis(300), || fs::rename(&a, &b)).unwrap_err();
        let took = start.elapsed();
        assert_eq!(e.raw_os_error(), Some(SHARING_VIOLATION));
        assert_eq!(e.kind(), fs::rename(&a, &b).unwrap_err().kind());
        assert!(
            took >= Duration::from_millis(100),
            "gave up too early: {took:?}"
        );
        assert!(
            took < Duration::from_millis(2000),
            "waited too long: {took:?}"
        );
        assert!(a.is_file() && !b.exists());
        h.join();
    }
}
