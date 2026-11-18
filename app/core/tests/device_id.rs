//! 裝置代號是 ULID。它會變成紀錄檔的檔名，所以不合法的代號一律拒絕。

mod common;

use common::DEVICE;
use paperreader_core::error::LibError;
use paperreader_core::ids::{is_valid_id, new_id};
use paperreader_core::local::LocalLibrary;
use std::fs;

#[test]
fn path_like_device_id_is_rejected_and_nothing_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let r = LocalLibrary::open(&root, "../../x");
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert!(!root.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn malformed_device_ids_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [
        "",
        "01JCK3T1A2B3C4D5E6F7G8H9J",   // 25 個字元
        "01JCK3T1A2B3C4D5E6F7G8H9JKM", // 27 個字元
        "01jck3t1a2b3c4d5e6f7g8h9jk",  // 小寫
        "01JCK3T1A2B3C4D5E6F7G8H9JI",  // 含 I
        "01JCK3T1A2B3C4D5E6F7G8H9JL",  // 含 L
        "01JCK3T1A2B3C4D5E6F7G8H9JO",  // 含 O
        "01JCK3T1A2B3C4D5E6F7G8H9JU",  // 含 U
        "01JCK3T1A2B3C4D5E6F7G8H9J/",  // 含路徑分隔
        "01JCK3T1A2B3C4D5E6F7G8H9J ",  // 含空白
        "01JCK3T1A2B3C4D5E6F7G8H9J你", // 含非 ASCII
    ] {
        let root = dir.path().join("lib");
        let r = LocalLibrary::open(&root, bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad:?} -> {r:?}"
        );
        assert!(!root.exists(), "{bad:?}");
    }
}

#[test]
fn valid_device_ids_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    assert!(is_valid_id(DEVICE));
    assert!(LocalLibrary::open(dir.path(), DEVICE).is_ok());
    assert!(LocalLibrary::open(dir.path(), &new_id()).is_ok());
}
