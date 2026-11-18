//! 把回收區裡移除的材料放回那篇：先記 `material-restored`，再改 `paper.json`、
//! 把檔搬回去，最後刪掉回收區的那個資料夾；同步時移除與放回取時間較新的一筆。

mod common;

use common::*;
use paperreader_core::changes::{MATERIAL_REMOVED, MATERIAL_RESTORED};
use paperreader_core::error::LibError;
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{AddMaterialOutcome, ChangeFilter, CheckLevel};
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    material: String,
    bytes: Vec<u8>,
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers").join("Zhao Paper")
    }
    fn removed_folder(&self) -> String {
        self.lib.list_removed_materials().unwrap()[0].folder.clone()
    }
    fn listed(&self) -> bool {
        self.lib
            .get_paper(&self.paper)
            .unwrap()
            .0
            .materials
            .iter()
            .any(|m| m.id == self.material)
    }
}

/// 一篇加了一份譯本 PDF，再把它移除。
fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, _) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 400, 7);
    let bytes = fs::read(&pdf).unwrap();
    let tok = lib.get_paper(&paper).unwrap().1;
    let material = match lib.add_translation(&paper, &pdf, Some(10), &tok).unwrap() {
        AddMaterialOutcome::Added { material_id, .. } => material_id,
        other => panic!("{other:?}"),
    };
    let tok = lib.get_paper(&paper).unwrap().1;
    lib.remove_material(&paper, &material, &tok).unwrap();
    Fx {
        dir,
        lib,
        paper,
        material,
        bytes,
    }
}

#[test]
fn restoring_puts_the_file_and_the_entry_back_and_empties_the_trash_folder() {
    let mut fx = setup();
    let folder = fx.removed_folder();
    let marker_entry = fx.lib.list_removed_materials().unwrap()[0].material.clone();

    let paper = fx.lib.restore_material(&folder).unwrap();

    assert_eq!(paper, fx.paper);
    let p = fx.lib.get_paper(&fx.paper).unwrap().0;
    let m = p.materials.iter().find(|m| m.id == fx.material).unwrap();
    assert_eq!(*m, marker_entry, "the entry comes back verbatim");
    assert_eq!(fs::read(fx.folder().join(&m.path)).unwrap(), fx.bytes);
    assert!(!fx.dir.path().join(".trash").join(&folder).exists());
    assert!(fx.lib.list_removed_materials().unwrap().is_empty());
    let recs = fx
        .lib
        .read_changes(&ChangeFilter {
            paper: Some(fx.paper.clone()),
            kind: Some(MATERIAL_RESTORED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].data["material"], serde_json::json!(fx.material));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

/// 原本的檔名已經被別的檔占用：換一個不重複的名稱放回去，清單記新的路徑。
#[test]
fn a_taken_file_name_gets_a_new_name() {
    let mut fx = setup();
    fs::write(fx.folder().join("translation.pdf"), "someone else's").unwrap();
    let folder = fx.removed_folder();
    fx.lib.restore_material(&folder).unwrap();
    let p = fx.lib.get_paper(&fx.paper).unwrap().0;
    let m = p.materials.iter().find(|m| m.id == fx.material).unwrap();
    assert_ne!(m.path, "translation.pdf");
    assert_eq!(fs::read(fx.folder().join(&m.path)).unwrap(), fx.bytes);
    assert_eq!(
        fs::read(fx.folder().join("translation.pdf")).unwrap(),
        b"someone else's"
    );
}

/// 那篇不在目錄裡（在回收區或已經刪了）：`NotFound`；文獻庫什麼都不動。
#[test]
fn the_paper_must_be_in_the_catalog() {
    let mut fx = setup();
    let folder = fx.removed_folder();
    fx.lib.remove_paper(&fx.paper).unwrap();
    let before = snapshot(fx.dir.path());
    let r = fx.lib.restore_material(&folder);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

/// 回收區的文獻、名稱不合規則、不存在、唯讀：都不動。
#[test]
fn only_a_removed_material_can_be_restored() {
    let mut fx = setup();
    let (other, _) = add_paper(&mut fx.lib, 2, "Other Paper", None);
    fx.lib.remove_paper(&other).unwrap();
    let paper_folder = fx.lib.list_trash().unwrap()[0].folder.clone();
    let before = snapshot(fx.dir.path());
    for bad in [paper_folder.as_str(), "..", "E:x", ".pr-x"] {
        let r = fx.lib.restore_material(bad);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
    }
    assert!(matches!(
        fx.lib.restore_material("no such folder"),
        Err(LibError::NotFound { .. })
    ));
    assert_eq!(snapshot(fx.dir.path()), before);
}

/// 放回之後同一項已經在那篇裡（例如同步帶回來了）：`InvalidInput`，回收區的那一份不動。
#[test]
fn a_material_already_back_in_the_paper_is_refused() {
    let mut fx = setup();
    let folder = fx.removed_folder();
    let root = fx.dir.path().join(".trash").join(&folder);
    let copy = fx.dir.path().join(".trash").join("20200101T000000Z-copy");
    fs::create_dir_all(&copy).unwrap();
    for e in fs::read_dir(&root).unwrap().flatten() {
        fs::copy(e.path(), copy.join(e.file_name())).unwrap();
    }
    fx.lib.restore_material(&folder).unwrap();
    assert!(fx.listed());
    let r = fx.lib.restore_material("20200101T000000Z-copy");
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert!(copy.is_dir());
}

/// 先記錄後檔案：記錄寫不成時什麼都不動。
#[test]
fn the_record_is_written_before_any_file() {
    let mut fx = setup();
    let folder = fx.removed_folder();
    let before = snapshot(fx.dir.path());
    fx.lib.set_change_hook(Some(Box::new(|_| false)));
    assert!(fx.lib.restore_material(&folder).is_err());
    fx.lib.set_change_hook(None);
    let mut after = snapshot(fx.dir.path());
    // 只多了紀錄檔那一行（掛勾在附加之後才回報失敗）
    let log = format!("logs/changes-{DEVICE}.jsonl");
    after.remove(&log);
    let mut b = before.clone();
    b.remove(&log);
    assert_eq!(after, b);
    assert!(!fx.listed());
}

/// 同步：A 移除再放回；B 先前已經跟著移除（在回收區）。B 匯入之後那一項回到 B 的那篇。
#[test]
fn a_restore_on_one_device_brings_the_material_back_on_the_other() {
    let mut p = pair();
    let alpha =
        p.a.list_papers()
            .into_iter()
            .find(|x| x.bib.title.as_deref() == Some("Alpha Paper"))
            .unwrap();
    let tid = alpha
        .materials
        .iter()
        .find(|m| m.role == "translation")
        .unwrap()
        .id
        .clone();
    let tok = p.a.get_paper(&alpha.id).unwrap().1;
    p.a.remove_material(&alpha.id, &tid, &tok).unwrap();
    let out = p.out.path().to_path_buf();
    sync_once(&mut p.a, &mut p.b, &out);
    assert!(
        p.b.get_paper(&alpha.id)
            .unwrap()
            .0
            .materials
            .iter()
            .all(|m| m.id != tid)
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    let folder = p.a.list_removed_materials().unwrap()[0].folder.clone();
    p.a.restore_material(&folder).unwrap();
    sync_once(&mut p.a, &mut p.b, &out);
    let b_paper = p.b.get_paper(&alpha.id).unwrap().0;
    assert!(b_paper.materials.iter().any(|m| m.id == tid), "back on B");
    assert_eq!(p.b.check(CheckLevel::Quick).unwrap(), vec![]);
    // 反過來：B 之後再移除，A 匯入時拿掉（較新的一筆勝出）
    std::thread::sleep(std::time::Duration::from_millis(5));
    let tok = p.b.get_paper(&alpha.id).unwrap().1;
    p.b.remove_material(&alpha.id, &tid, &tok).unwrap();
    sync_once(&mut p.b, &mut p.a, &out);
    assert!(
        p.a.get_paper(&alpha.id)
            .unwrap()
            .0
            .materials
            .iter()
            .all(|m| m.id != tid)
    );
    let recs =
        p.a.read_changes(&ChangeFilter {
            paper: Some(alpha.id.clone()),
            kind: Some(MATERIAL_REMOVED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 2);
}
