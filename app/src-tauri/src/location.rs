//! 文獻庫放在哪裡的決定。純函式，不碰檔案，方便測試。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// 共用空間的 `Documents/PaperReader`，需要「所有檔案存取」。
    Shared,
    /// 應用專屬空間（應用私有的外部空間），解除安裝會一併刪除。
    Private,
}

/// 啟動時要做什麼。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// 文獻庫還沒建立：什麼都不開、什麼都不寫。
    NotCreated,
    /// 在這個位置開啟（那裡已經有 `library.json`）。
    Open(Storage),
    /// 以前選了共用空間，但現在沒有權限了：先不開，等使用者重新授權。
    AccessLost,
    /// 記得文獻庫放在這個位置，但那裡沒有 `library.json` 了（例如被搬走或改名）：
    /// 不自動建新的，由畫面說明並讓使用者選「在這裡建一個新的」。
    Missing(Storage),
}

/// - 記得選過位置：共用空間而且現在沒有權限，是 `AccessLost`；那裡有文獻庫就開，沒有是 `Missing`。
/// - 沒記：有權限而且共用空間裡已經有文獻庫（例如重新安裝之後），接回去；否則還沒建立。
///
/// `chosen_has_library`：記得的那個位置裡有沒有 `library.json`。
pub fn plan(
    chosen: Option<Storage>,
    all_files_access: bool,
    shared_has_library: bool,
    chosen_has_library: bool,
) -> Plan {
    match chosen {
        Some(Storage::Shared) if !all_files_access => Plan::AccessLost,
        Some(s) if chosen_has_library => Plan::Open(s),
        Some(s) => Plan::Missing(s),
        None if all_files_access && shared_has_library => Plan::Open(Storage::Shared),
        None => Plan::NotCreated,
    }
}

/// 使用者按下「去開啟」回來之後，要建立在哪裡：有權限放共用空間，否則當作選了「不要」。
pub fn storage_after_permission_step(all_files_access: bool) -> Storage {
    if all_files_access {
        Storage::Shared
    } else {
        Storage::Private
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_chosen_and_nothing_there_means_not_created() {
        assert_eq!(plan(None, false, false, false), Plan::NotCreated);
        assert_eq!(plan(None, true, false, false), Plan::NotCreated);
        // 有舊文獻庫但沒有權限：看不到它，不能開
        assert_eq!(plan(None, false, true, false), Plan::NotCreated);
    }

    #[test]
    fn reinstall_adopts_the_existing_shared_library() {
        assert_eq!(plan(None, true, true, false), Plan::Open(Storage::Shared));
    }

    #[test]
    fn remembered_choice_is_opened_when_it_is_there() {
        assert_eq!(
            plan(Some(Storage::Private), false, true, true),
            Plan::Open(Storage::Private)
        );
        assert_eq!(
            plan(Some(Storage::Shared), true, false, true),
            Plan::Open(Storage::Shared)
        );
    }

    #[test]
    fn remembered_place_without_a_library_is_reported_not_recreated() {
        assert_eq!(
            plan(Some(Storage::Shared), true, false, false),
            Plan::Missing(Storage::Shared)
        );
        assert_eq!(
            plan(Some(Storage::Private), false, false, false),
            Plan::Missing(Storage::Private)
        );
    }

    #[test]
    fn revoked_permission_is_reported_not_silently_switched() {
        assert_eq!(
            plan(Some(Storage::Shared), false, true, true),
            Plan::AccessLost
        );
    }

    #[test]
    fn permission_step_falls_back_to_private() {
        assert_eq!(storage_after_permission_step(true), Storage::Shared);
        assert_eq!(storage_after_permission_step(false), Storage::Private);
    }
}
