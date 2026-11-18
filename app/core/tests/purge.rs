//! 永久刪除：回收區裡的一篇先記 `paper-purged` 再刪；移除的材料直接刪；全部清空；
//! 使用者自己放進 `.trash/` 的東西不動；刪了之後備份與同步包不再帶著。

mod common;

use common::*;
use paperreader_core::changes::PAPER_PURGED;
use paperreader_core::error::LibError;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{AddMaterialOutcome, AddOutcome, ChangeFilter, CheckLevel, SyncMode};
use std::fs;
use std::path::{Path, PathBuf};

fn trash(root: &Path) -> PathBuf {
    root.join(".trash")
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

/// 一篇在回收區（有筆記與想法）、另一篇在目錄裡並移除過一項譯本。交回（文獻庫, 回收區那篇, 目錄那篇）。
fn setup() -> (tempfile::TempDir, LocalLibrary, String, String) {
    let (dir, mut lib) = temp_library();
    let (gone, gm) = add_paper(&mut lib, 1, "Gone Paper", Some(3));
    let (kept, _) = add_paper(&mut lib, 2, "Kept Paper", Some(3));
    let (_, nt) = lib
        .add_highlight(&gone, highlight_draft(&gm, 0, "note"), None)
        .unwrap();
    lib.set_thoughts(&gone, "my thoughts", Some(&nt)).unwrap();
    lib.record_open(&gone, &gm).unwrap();
    lib.remove_paper(&gone).unwrap();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 500, 9);
    let tok = lib.get_paper(&kept).unwrap().1;
    let tid = match lib.add_translation(&kept, &pdf, None, &tok).unwrap() {
        AddMaterialOutcome::Added { material_id, .. } => material_id,
        other => panic!("{other:?}"),
    };
    let tok = lib.get_paper(&kept).unwrap().1;
    lib.remove_material(&kept, &tid, &tok).unwrap();
    (dir, lib, gone, kept)
}

#[test]
fn purging_a_paper_records_it_then_deletes_its_folder() {
    let (dir, mut lib, gone, _) = setup();
    let root = dir.path();
    let entry = lib.list_trash().unwrap().remove(0);
    assert_eq!(entry.paper_id, gone);
    let notes_before = snapshot(&root.join("notes"));
    let reading_logs: Vec<String> = names(&root.join("logs"))
        .into_iter()
        .filter(|n| n.starts_with("reading-"))
        .collect();
    let material_folder = lib.list_removed_materials().unwrap()[0].folder.clone();

    lib.purge_trash_entry(&entry.folder).unwrap();

    assert!(!trash(root).join(&entry.folder).exists());
    assert!(lib.list_trash().unwrap().is_empty());
    // 別的東西不動：移除的材料、筆記檔、閱讀紀錄
    assert!(trash(root).join(&material_folder).is_dir());
    assert_eq!(snapshot(&root.join("notes")), notes_before);
    for n in &reading_logs {
        assert!(root.join("logs").join(n).is_file());
    }
    // 一筆 paper-purged
    let recs = lib
        .read_changes(&ChangeFilter {
            paper: Some(gone.clone()),
            kind: Some(PAPER_PURGED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].data, serde_json::json!({}));
    assert_eq!(lib.check(CheckLevel::Full).unwrap(), vec![]);
    // 同一份 PDF 再加入：不再說「在回收區」
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "a.pdf", 300, 1);
    let r = lib.add_paper(&pdf, Default::default(), None).unwrap();
    assert!(matches!(r, AddOutcome::Added(_)), "{r:?}");
}

#[test]
fn purging_a_removed_material_deletes_it_without_a_record() {
    let (dir, mut lib, _, _) = setup();
    let lines_before = change_lines(dir.path(), DEVICE).len();
    let folder = lib.list_removed_materials().unwrap()[0].folder.clone();
    lib.purge_trash_entry(&folder).unwrap();
    assert!(!trash(dir.path()).join(&folder).exists());
    assert!(lib.list_removed_materials().unwrap().is_empty());
    assert_eq!(
        lib.list_trash().unwrap().len(),
        1,
        "the paper in the trash stays"
    );
    assert_eq!(change_lines(dir.path(), DEVICE).len(), lines_before);
}

#[test]
fn only_entries_the_app_put_in_the_trash_can_be_purged() {
    let (dir, mut lib, _, _) = setup();
    let t = trash(dir.path());
    // 使用者自己放進回收區的資料夾與檔
    fs::create_dir_all(t.join("my stuff")).unwrap();
    fs::write(t.join("my stuff").join("x.txt"), "keep").unwrap();
    fs::write(t.join("loose.txt"), "keep").unwrap();
    let before = snapshot(dir.path());
    for bad in ["my stuff", "loose.txt"] {
        let r = lib.purge_trash_entry(bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
    }
    for bad in ["no such folder", ""] {
        let r = lib.purge_trash_entry(bad);
        assert!(
            matches!(
                r,
                Err(LibError::NotFound { .. } | LibError::InvalidInput { .. })
            ),
            "{bad}: {r:?}"
        );
    }
    for bad in ["..", ".", "../papers", "a/b", "a\\b", ".hidden", "/abs"] {
        let r = lib.purge_trash_entry(bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
    }
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn emptying_the_trash_purges_papers_and_materials_but_not_the_users_own_things() {
    let (dir, mut lib, gone, kept) = setup();
    // 再移除一篇，回收區有兩篇
    lib.remove_paper(&kept).unwrap();
    let t = trash(dir.path());
    fs::create_dir_all(t.join("my stuff")).unwrap();
    fs::write(t.join("my stuff").join("x.txt"), "keep").unwrap();
    fs::create_dir_all(t.join(".stfolder")).unwrap();

    let r = lib.empty_trash().unwrap();
    assert_eq!(r.papers, 2);
    assert_eq!(r.materials, 1);
    assert_eq!(
        names(&t),
        vec![".stfolder".to_string(), "my stuff".to_string()]
    );
    assert!(lib.list_trash().unwrap().is_empty());
    assert!(lib.list_removed_materials().unwrap().is_empty());
    let purged: Vec<String> = lib
        .read_changes(&ChangeFilter {
            paper: None,
            kind: Some(PAPER_PURGED.into()),
        })
        .unwrap()
        .into_iter()
        .filter_map(|r| r.paper)
        .collect();
    assert!(
        purged.contains(&gone) && purged.contains(&kept),
        "{purged:?}"
    );
    // 空的回收區再清一次：什麼都不做
    let r = lib.empty_trash().unwrap();
    assert_eq!((r.papers, r.materials), (0, 0));
}

/// 先記錄後刪除：記錄寫好之後失敗，資料夾還在。
#[test]
fn the_record_is_written_before_the_folder_is_deleted() {
    let (dir, mut lib, _, _) = setup();
    let folder = lib.list_trash().unwrap()[0].folder.clone();
    let lines_before = change_lines(dir.path(), DEVICE).len();
    lib.set_change_hook(Some(Box::new(|_| false)));
    assert!(lib.purge_trash_entry(&folder).is_err());
    lib.set_change_hook(None);
    assert!(trash(dir.path()).join(&folder).join("paper.json").is_file());
    assert_eq!(change_lines(dir.path(), DEVICE).len(), lines_before + 1);
}

/// 回收區裡同一篇有兩個資料夾：以資料夾指定，只刪那一個；另一個照樣放得回去。
#[test]
fn purging_one_of_two_copies_leaves_the_other() {
    let (dir, mut lib, gone, _) = setup();
    let t = trash(dir.path());
    let first = lib.list_trash().unwrap()[0].folder.clone();
    // 複製一份成較早移除的資料夾
    let copy = "20200101T000000Z-Gone Paper";
    fs::create_dir_all(t.join(copy)).unwrap();
    for n in names(&t.join(&first)) {
        fs::copy(t.join(&first).join(&n), t.join(copy).join(&n)).unwrap();
    }
    lib.purge_trash_entry(&first).unwrap();
    let left = lib.list_trash().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].folder, copy);
    lib.restore_paper(&gone).unwrap();
    assert!(lib.get_paper(&gone).is_ok());
    assert_eq!(purged_records(&lib, &gone), 0);
}

#[test]
fn read_only_library_refuses_to_purge() {
    let (dir, lib, _, _) = setup();
    let folder = lib.list_trash().unwrap()[0].folder.clone();
    let root = dir.path().to_path_buf();
    drop(lib);
    let lp = root.join("library.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = 2.into();
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let before = snapshot(&root);
    let mut ro = LocalLibrary::open(&root, DEVICE).unwrap();
    assert!(matches!(
        ro.purge_trash_entry(&folder),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(ro.empty_trash(), Err(LibError::ReadOnly)));
    assert_eq!(snapshot(&root), before);
}

/// 刪到一半被中斷留下的 `.trash/.pr-` 暫存，下次開啟時清掉。
#[test]
fn a_half_deleted_folder_is_cleaned_on_the_next_open() {
    let (dir, lib, _, _) = setup();
    let left = trash(dir.path()).join(".pr-purge-01JTEST");
    fs::create_dir_all(left.join("sub")).unwrap();
    fs::write(left.join("sub").join("a.pdf"), "x").unwrap();
    drop(lib);
    let _lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(!left.exists());
}

/// 永久刪除之後，備份檔與同步包都不再帶著那些檔。
#[test]
fn backups_and_sync_packages_no_longer_carry_purged_files() {
    let (dir, mut lib, _, _) = setup();
    lib.empty_trash().unwrap();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let in_backup: Vec<String> = zip_entries(&zip).into_iter().map(|e| e.0).collect();
    assert!(
        !in_backup.iter().any(|n| n.starts_with(".trash/")),
        "{in_backup:?}"
    );
    let pkg = out.path().join("s.zip");
    lib.export_sync_package(&pkg, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let in_pkg: Vec<String> = zip_entries(&pkg).into_iter().map(|e| e.0).collect();
    assert!(
        !in_pkg.iter().any(|n| n.starts_with(".trash/")),
        "{in_pkg:?}"
    );
    let _ = dir;
}

/// 名稱只收恰好一段的一般名稱：Windows 上磁碟機相對的 `E:x` 不算絕對路徑，`join` 之後卻會換掉
/// 整個 `.trash`；應用放的名稱不會有 `:`（`naming.rs` 拿掉了）。
#[test]
fn drive_relative_and_other_multi_part_names_are_refused() {
    let (dir, mut lib, _, _) = setup();
    let before = snapshot(dir.path());
    for bad in [
        "E:x",
        "E:",
        "C:\\x",
        "C:x",
        "x:y",
        "\\\\?\\C:\\x",
        "//server/share",
        "a/./b",
    ] {
        let r = lib.purge_trash_entry(bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
    }
    assert_eq!(snapshot(dir.path()), before);
}

fn purged_records(lib: &LocalLibrary, paper: &str) -> usize {
    lib.read_changes(&ChangeFilter {
        paper: Some(paper.to_string()),
        kind: Some(PAPER_PURGED.into()),
    })
    .unwrap()
    .len()
}

/// 永久刪除的記錄只在那篇在這台完全不見時才寫：目錄裡或回收區另一個資料夾還有同一個代號時，
/// 只刪那個資料夾，不記；最後一份刪掉時才記。
#[test]
fn the_purge_is_recorded_only_when_no_copy_of_the_paper_is_left() {
    let (dir, mut lib, gone, _) = setup();
    let t = trash(dir.path());
    let first = lib.list_trash().unwrap()[0].folder.clone();
    let copy = "20200101T000000Z-Gone Paper";
    fs::create_dir_all(t.join(copy)).unwrap();
    for n in names(&t.join(&first)) {
        fs::copy(t.join(&first).join(&n), t.join(copy).join(&n)).unwrap();
    }
    lib.purge_trash_entry(&first).unwrap();
    assert_eq!(purged_records(&lib, &gone), 0, "another copy is left");
    // 放回目錄之後，再把一份舊的副本放進回收區刪掉：目錄裡還有，不記
    lib.restore_paper(&gone).unwrap();
    let catalog = dir.path().join("papers").join("Gone Paper");
    let stale = "20190101T000000Z-Gone Paper";
    fs::create_dir_all(t.join(stale)).unwrap();
    for n in names(&catalog) {
        if catalog.join(&n).is_file() {
            fs::copy(catalog.join(&n), t.join(stale).join(&n)).unwrap();
        }
    }
    lib.purge_trash_entry(stale).unwrap();
    assert_eq!(purged_records(&lib, &gone), 0, "still in the catalog");
    assert!(lib.get_paper(&gone).is_ok());
    // 移到回收區、刪掉最後一份：記一筆
    lib.remove_paper(&gone).unwrap();
    let last = lib.list_trash().unwrap()[0].folder.clone();
    lib.purge_trash_entry(&last).unwrap();
    assert_eq!(purged_records(&lib, &gone), 1);
}

fn trash_ids(lib: &LocalLibrary) -> Vec<String> {
    lib.list_trash()
        .unwrap()
        .into_iter()
        .map(|e| e.paper_id)
        .collect()
}

/// 永久刪除在同步時生效。A 永久刪除了回收區的一篇；匯入 B 的完整同步包時不再帶回來；
/// B 匯入 A 的同步包之後，B 回收區裡的那篇也刪掉。
#[test]
fn a_purged_paper_leaves_both_trashes_and_does_not_come_back() {
    let mut p = pair();
    let gamma = p.a.list_trash().unwrap()[0].clone();
    assert_eq!(trash_ids(&p.b), vec![gamma.paper_id.clone()]);
    p.a.purge_trash_entry(&gamma.folder).unwrap();

    let full = p.out.path().join("full-b.zip");
    p.b.export_sync_package(&full, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    import_defaults(&mut p.a, &full);
    assert!(trash_ids(&p.a).is_empty(), "{:?}", trash_ids(&p.a));

    let out = p.out.path().to_path_buf();
    let report = sync_once(&mut p.a, &mut p.b, &out);
    assert!(trash_ids(&p.b).is_empty(), "{report:?}");
    let b_trash = p.b_root().join(".trash");
    assert!(
        !names(&b_trash).iter().any(|n| n.contains("Gamma")),
        "{:?}",
        names(&b_trash)
    );
    assert_eq!(p.b.check(CheckLevel::Quick).unwrap(), vec![]);
    // 再來回一次：不再有變動
    let again = sync_once(&mut p.b, &mut p.a, &out);
    assert!(trash_ids(&p.a).is_empty(), "{again:?}");
}

/// 永久刪除之後另一台把它放回目錄（或一直在目錄裡用著）：目錄裡的一律不刪，提醒
/// `PurgedButRestored`；兩台最後都在目錄裡。
#[test]
fn a_purged_paper_still_in_a_catalog_is_kept_with_a_notice() {
    use paperreader_core::store::NoticeKind;
    let mut p = pair();
    let gamma = p.a.list_trash().unwrap()[0].clone();
    p.b.restore_paper(&gamma.paper_id).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    p.a.purge_trash_entry(&gamma.folder).unwrap();
    let out = p.out.path().to_path_buf();

    // A 匯入 B：B 的目錄裡有，帶進 A 的目錄
    let r = sync_once(&mut p.b, &mut p.a, &out);
    assert!(
        r.notices
            .iter()
            .any(|n| n.kind == NoticeKind::PurgedButRestored
                && n.paper.as_deref() == Some(gamma.paper_id.as_str())),
        "{r:?}"
    );
    assert!(p.a.get_paper(&gamma.paper_id).is_ok());
    assert!(trash_ids(&p.a).is_empty());

    // B 匯入 A：B 的目錄裡那份不刪
    let r = sync_once(&mut p.a, &mut p.b, &out);
    assert!(p.b.get_paper(&gamma.paper_id).is_ok(), "{r:?}");
    assert!(trash_ids(&p.b).is_empty());
    // 之後再同步不再提醒
    let r = sync_once(&mut p.b, &mut p.a, &out);
    assert!(
        !r.notices
            .iter()
            .any(|n| n.kind == NoticeKind::PurgedButRestored),
        "{r:?}"
    );
}

/// 另一種先後：A 永久刪除之後，B 才把它移到回收區（B 還沒同步過）。較晚的移除不抵銷
/// 永久刪除：A 匯入 B 的完整同步包時不帶回來；B 匯入 A 之後，B 回收區裡的那份也刪掉。
#[test]
fn a_removal_after_the_purge_does_not_bring_the_paper_back() {
    let mut p = pair();
    let beta =
        p.a.list_papers()
            .into_iter()
            .find(|x| x.bib.title.as_deref() == Some("Beta Paper"))
            .unwrap();
    p.a.remove_paper(&beta.id).unwrap();
    let folder =
        p.a.list_trash()
            .unwrap()
            .into_iter()
            .find(|e| e.paper_id == beta.id)
            .unwrap()
            .folder;
    p.a.purge_trash_entry(&folder).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    p.b.remove_paper(&beta.id).unwrap();

    let full = p.out.path().join("full-b.zip");
    p.b.export_sync_package(&full, SyncMode::Full, None, &mut |_, _| {})
        .unwrap();
    let r = import_defaults(&mut p.a, &full);
    assert!(
        !trash_ids(&p.a).contains(&beta.id),
        "{:?} {r:?}",
        trash_ids(&p.a)
    );
    assert!(p.a.get_paper(&beta.id).is_err());

    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert!(!trash_ids(&p.b).contains(&beta.id));
    assert!(p.b.get_paper(&beta.id).is_err());
}

/// 先後：A 永久刪除之後，B 才把它從回收區放回目錄。留在目錄，兩台各提醒一次
/// `PurgedButRestored`，之後不再提醒。
#[test]
fn a_restore_after_the_purge_keeps_the_paper_and_notices_once_on_each_side() {
    use paperreader_core::store::NoticeKind;
    let notified = |r: &paperreader_core::store::SyncImportReport, id: &str| {
        r.notices
            .iter()
            .any(|n| n.kind == NoticeKind::PurgedButRestored && n.paper.as_deref() == Some(id))
    };
    let mut p = pair();
    let gamma = p.a.list_trash().unwrap()[0].clone();
    p.a.purge_trash_entry(&gamma.folder).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    p.b.restore_paper(&gamma.paper_id).unwrap();
    let out = p.out.path().to_path_buf();

    let r = sync_once(&mut p.b, &mut p.a, &out);
    assert!(p.a.get_paper(&gamma.paper_id).is_ok(), "{r:?}");
    assert!(notified(&r, &gamma.paper_id), "{r:?}");
    let r = sync_once(&mut p.a, &mut p.b, &out);
    assert!(p.b.get_paper(&gamma.paper_id).is_ok());
    assert!(notified(&r, &gamma.paper_id), "{r:?}");
    let r = sync_once(&mut p.b, &mut p.a, &out);
    assert!(!notified(&r, &gamma.paper_id), "{r:?}");
    let r = sync_once(&mut p.a, &mut p.b, &out);
    assert!(!notified(&r, &gamma.paper_id), "{r:?}");
}

/// 一直留在目錄裡的那台（從沒移除過）匯入另一台「移除再永久刪除」的同步包：目錄裡的不刪，提醒。
#[test]
fn a_catalog_copy_is_not_deleted_by_a_purge_from_another_device() {
    use paperreader_core::store::NoticeKind;
    let mut p = pair();
    let beta =
        p.a.list_papers()
            .into_iter()
            .find(|x| x.bib.title.as_deref() == Some("Beta Paper"))
            .unwrap();
    p.a.remove_paper(&beta.id).unwrap();
    let folder =
        p.a.list_trash()
            .unwrap()
            .into_iter()
            .find(|e| e.paper_id == beta.id)
            .unwrap()
            .folder;
    p.a.purge_trash_entry(&folder).unwrap();
    let out = p.out.path().to_path_buf();
    let r = sync_once(&mut p.a, &mut p.b, &out);
    assert!(p.b.get_paper(&beta.id).is_ok(), "{r:?}");
    assert!(
        r.notices
            .iter()
            .any(|n| n.kind == NoticeKind::PurgedButRestored),
        "{r:?}"
    );
}
