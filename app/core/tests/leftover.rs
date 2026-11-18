//! 清不掉的暫存項目不擋住開啟，由「檢查文獻庫」列出。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{CheckLevel, ProblemKind};
use std::fs;

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn check_lists_leftover_temp_items_in_a_read_only_library() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    write_paper(root, "A", &sample_paper(A));
    fs::write(root.join("papers/A/source.pdf"), b"").unwrap();
    // sample_paper 的材料大小未記錄，空檔也算 Ok
    fs::write(root.join(".pr-tmp-Z"), b"x").unwrap();
    fs::create_dir_all(root.join("papers/.pr-adding-X")).unwrap();
    fs::write(root.join("papers/.pr-adding-X/source.pdf"), b"half").unwrap();
    fs::write(root.join("papers/A/.pr-tmp-Y"), b"x").unwrap();

    let lib = LocalLibrary::open(root, DEVICE).unwrap();
    assert!(lib.is_read_only());
    // 唯讀開啟不清理
    assert!(root.join(".pr-tmp-Z").exists());

    let ps = lib.check(CheckLevel::Quick).unwrap();
    let got: Vec<_> = ps
        .iter()
        .map(|p| (p.kind, p.path.as_str(), p.paper.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            (ProblemKind::LeftoverTemp, ".pr-tmp-Z", None),
            (ProblemKind::LeftoverTemp, "papers/.pr-adding-X", None),
            (ProblemKind::LeftoverTemp, "papers/A/.pr-tmp-Y", Some(A)),
        ]
    );
}

#[test]
fn check_does_not_list_other_tools_hidden_items_as_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_paper(root, "A", &sample_paper(A));
    fs::write(root.join("papers/A/source.pdf"), b"").unwrap();
    fs::create_dir_all(root.join(".stfolder")).unwrap();
    fs::write(root.join("papers/A/.DS_Store"), b"x").unwrap();
    let lib = LocalLibrary::open(root, DEVICE).unwrap();
    assert!(lib.check(CheckLevel::Full).unwrap().is_empty());
}

/// 讓一個暫存項目「清不掉」：Windows 上開著不准刪除的控制代碼；其他系統拿掉資料夾的寫入權限。
/// 回傳的東西要保留到測試結束。
#[cfg(windows)]
fn make_undeletable(root: &std::path::Path) -> Box<dyn std::any::Any> {
    use std::os::windows::fs::OpenOptionsExt;
    let p = root.join(".pr-tmp-stuck");
    fs::write(&p, b"x").unwrap();
    // share_mode(0)：不與任何人共用，別人就刪不掉也改不了名
    Box::new(
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&p)
            .unwrap(),
    )
}

#[cfg(not(windows))]
fn make_undeletable(root: &std::path::Path) -> Box<dyn std::any::Any> {
    use std::os::unix::fs::PermissionsExt;
    let d = root.join(".pr-tmp-stuck");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("f"), b"x").unwrap();
    fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    Box::new(Restore(d))
}

#[test]
fn open_succeeds_when_a_temp_item_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_paper(root, "A", &sample_paper(A));
    fs::write(root.join("papers/A/source.pdf"), b"").unwrap();
    let _guard = make_undeletable(root);
    // 清得掉的照樣清掉
    fs::write(root.join(".pr-tmp-removable"), b"x").unwrap();
    fs::create_dir_all(root.join("papers/.pr-adding-gone")).unwrap();

    let lib = LocalLibrary::open(root, DEVICE).expect("清不掉的暫存項目不該讓文獻庫打不開");
    assert!(!lib.is_read_only());
    assert_eq!(lib.list_papers().len(), 1);
    assert!(!root.join(".pr-tmp-removable").exists());
    assert!(!root.join("papers/.pr-adding-gone").exists());
    assert!(root.join(".pr-tmp-stuck").exists());

    let ps = lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert_eq!(ps[0].kind, ProblemKind::LeftoverTemp);
    assert_eq!(ps[0].path, ".pr-tmp-stuck");
}
