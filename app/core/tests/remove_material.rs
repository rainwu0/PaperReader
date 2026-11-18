//! 移除一項材料：先記 `material-removed`、再改 `paper.json`、材料檔搬進回收區；
//! 原文不能用它移除；回收區列得出來；檢查不列成問題；備份帶著；同步時另一台跟著搬。

mod common;

use common::*;
use paperreader_core::changes::MATERIAL_REMOVED;
use paperreader_core::error::LibError;
use paperreader_core::format::{REMOVED_MATERIAL_FILE, RemovedMaterialFile};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use paperreader_core::store::{AddMaterialOutcome, ChangeFilter, CheckLevel};
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    source: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    Fx {
        dir,
        lib,
        paper,
        source,
    }
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers").join("Zhao Paper")
    }
    fn trash(&self) -> PathBuf {
        self.dir.path().join(".trash")
    }
    fn token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    /// 加一份使用者自己的譯本 PDF，交回材料代號。
    fn add_translation(&mut self, seed: u64) -> String {
        let src = tempfile::tempdir().unwrap();
        let pdf = fake_pdf_seeded(src.path(), "t.pdf", 400, seed);
        let tok = self.token();
        match self
            .lib
            .add_translation(&self.paper, &pdf, Some(10), &tok)
            .unwrap()
        {
            AddMaterialOutcome::Added { material_id, .. } => material_id,
            other => panic!("{other:?}"),
        }
    }
    fn add_overlay(&mut self) -> String {
        let tok = self.token();
        self.lib
            .create_overlay(
                &self.paper,
                OverlayDraft {
                    source_material: self.source.clone(),
                    provider: "deepl".into(),
                    model: String::new(),
                    source_lang: Some("en".into()),
                    target_lang: "zh-TW".into(),
                    segments: vec![SegmentDraft {
                        page: 0,
                        bbox: [1.0, 1.0, 2.0, 2.0],
                        kind: "body".into(),
                        size: None,
                        source: "Hello.".into(),
                    }],
                },
                &tok,
            )
            .unwrap()
            .material_id
    }
    fn trash_folders(&self) -> Vec<String> {
        match fs::read_dir(self.trash()) {
            Ok(rd) => {
                let mut v: Vec<String> = rd
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                v.sort();
                v
            }
            Err(_) => Vec::new(),
        }
    }
}

#[test]
fn removing_a_translation_moves_its_file_to_the_trash_after_recording_it() {
    let mut fx = setup();
    let tid = fx.add_translation(7);
    let file_bytes = fs::read(fx.folder().join("translation.pdf")).unwrap();
    let entry = fx
        .lib
        .get_paper(&fx.paper)
        .unwrap()
        .0
        .materials
        .into_iter()
        .find(|m| m.id == tid)
        .unwrap();
    let tok = fx.token();

    let new_token = fx.lib.remove_material(&fx.paper, &tid, &tok).unwrap();

    // paper.json：那一項不見了，原文還在；交回的記號就是現在的記號
    let (paper, now) = fx.lib.get_paper(&fx.paper).unwrap();
    assert_eq!(new_token, now);
    assert!(paper.materials.iter().all(|m| m.id != tid));
    assert!(paper.materials.iter().any(|m| m.id == fx.source));
    assert!(!fx.folder().join("translation.pdf").exists());

    // 回收區：一個資料夾，裡面是說明檔與原本的檔
    let folders = fx.trash_folders();
    assert_eq!(folders.len(), 1, "{folders:?}");
    let tdir = fx.trash().join(&folders[0]);
    assert!(
        folders[0].ends_with("-Zhao Paper - translation.pdf"),
        "{folders:?}"
    );
    assert!(
        !tdir.join("paper.json").exists(),
        "not mistaken for a paper"
    );
    assert_eq!(fs::read(tdir.join("translation.pdf")).unwrap(), file_bytes);
    let marker: RemovedMaterialFile =
        serde_json::from_slice(&fs::read(tdir.join(REMOVED_MATERIAL_FILE)).unwrap()).unwrap();
    assert_eq!(marker.format, 1);
    assert_eq!(marker.paper, fx.paper);
    assert_eq!(marker.paper_folder, "Zhao Paper");
    assert_eq!(marker.removed_by, DEVICE);
    assert_eq!(marker.file.as_deref(), Some("translation.pdf"));
    assert_eq!(marker.material, entry, "the listed entry is kept verbatim");

    // 變更紀錄：一筆 material-removed
    let recs = fx
        .lib
        .read_changes(&ChangeFilter {
            paper: Some(fx.paper.clone()),
            kind: Some(MATERIAL_REMOVED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].data["material"], serde_json::json!(tid));
    assert_eq!(recs[0].device, DEVICE);

    // 回收區列得出來；不被當成一篇文獻；檢查沒有問題
    let removed = fx.lib.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].folder, folders[0]);
    assert_eq!(removed[0].paper_id, fx.paper);
    assert_eq!(removed[0].paper_folder, "Zhao Paper");
    assert_eq!(removed[0].material.id, tid);
    assert_eq!(
        removed[0].bytes,
        400 + {
            fs::metadata(tdir.join(REMOVED_MATERIAL_FILE))
                .unwrap()
                .len()
        }
    );
    assert_eq!(
        removed[0].removed_at.as_deref(),
        Some(marker.removed.as_str())
    );
    assert!(fx.lib.list_trash().unwrap().is_empty());
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn the_source_cannot_be_removed_on_its_own() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let tok = fx.token();
    let r = fx.lib.remove_material(&fx.paper, &fx.source, &tok);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(
        snapshot(fx.dir.path()),
        before,
        "nothing written, no record"
    );
}

#[test]
fn stale_token_unknown_material_and_read_only_write_nothing() {
    let mut fx = setup();
    let tid = fx.add_translation(7);
    let stale = fx.token();
    fx.lib
        .set_material_pages(&fx.paper, &fx.source, 12)
        .unwrap();
    let before = snapshot(fx.dir.path());
    assert!(matches!(
        fx.lib.remove_material(&fx.paper, &tid, &stale),
        Err(LibError::Conflict)
    ));
    let tok = fx.token();
    assert!(matches!(
        fx.lib
            .remove_material(&fx.paper, "01JNOSUCHMATERIAL000000000", &tok),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        fx.lib
            .remove_material("01JNOSUCHPAPER000000000000", &tid, &tok),
        Err(LibError::NotFound { .. })
    ));
    assert_eq!(snapshot(fx.dir.path()), before);

    // 唯讀的文獻庫
    let root = fx.dir.path().to_path_buf();
    drop(fx.lib);
    let lp = root.join("library.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = 2.into();
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let before = snapshot(&root);
    let mut ro = LocalLibrary::open(&root, DEVICE).unwrap();
    let tok = ro.get_paper(&fx.paper).unwrap().1;
    assert!(matches!(
        ro.remove_material(&fx.paper, &tid, &tok),
        Err(LibError::ReadOnly)
    ));
    assert_eq!(snapshot(&root), before);
}

/// 先記錄、後檔案：記錄寫好之後失敗，材料仍在清單裡、檔案沒動，記錄留著。
#[test]
fn the_record_is_written_before_any_file() {
    let mut fx = setup();
    let tid = fx.add_translation(7);
    let paper_before = fs::read(fx.folder().join("paper.json")).unwrap();
    fx.lib.set_change_hook(Some(Box::new(|_| false)));
    let tok = fx.token();
    assert!(fx.lib.remove_material(&fx.paper, &tid, &tok).is_err());
    fx.lib.set_change_hook(None);
    assert_eq!(
        fs::read(fx.folder().join("paper.json")).unwrap(),
        paper_before
    );
    assert!(fx.folder().join("translation.pdf").is_file());
    assert!(fx.trash_folders().is_empty());
    assert_eq!(change_lines(fx.dir.path(), DEVICE).len(), 1);
}

/// 譯文疊加檔也能移除；由它產生的譯本 PDF 是另一項材料，不跟著動。
#[test]
fn an_overlay_can_be_removed_and_the_generated_pdf_stays() {
    let mut fx = setup();
    let ov = fx.add_overlay();
    let tok = fx.token();
    let gen_id = match fx
        .lib
        .put_generated_translation(
            &fx.paper,
            &ov,
            &mut std::io::Cursor::new(fake_bytes(5, 300)),
            Some(10),
            &tok,
        )
        .unwrap()
    {
        paperreader_core::store::PutGenerated::Created { material_id, .. } => material_id,
        other => panic!("{other:?}"),
    };
    let tok = fx.token();
    fx.lib.remove_material(&fx.paper, &ov, &tok).unwrap();
    let paper = fx.lib.get_paper(&fx.paper).unwrap().0;
    assert!(paper.materials.iter().all(|m| m.id != ov));
    assert!(paper.materials.iter().any(|m| m.id == gen_id));
    assert!(fx.folder().join("translation-generated.pdf").is_file());
    assert!(!fx.folder().join("translation-overlay.json").exists());
    let removed = fx.lib.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].material.form, "overlay");
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

/// 檔案已經不在（使用者在別處刪了）：照樣拿掉清單項、記錄；回收區不放空的資料夾。
#[test]
fn a_material_whose_file_is_gone_is_still_removed() {
    let mut fx = setup();
    let tid = fx.add_translation(7);
    fs::remove_file(fx.folder().join("translation.pdf")).unwrap();
    let tok = fx.token();
    fx.lib.remove_material(&fx.paper, &tid, &tok).unwrap();
    assert!(
        fx.lib
            .get_paper(&fx.paper)
            .unwrap()
            .0
            .materials
            .iter()
            .all(|m| m.id != tid)
    );
    assert!(fx.trash_folders().is_empty());
    assert_eq!(change_lines(fx.dir.path(), DEVICE).len(), 1);
}

/// 同一秒移除兩項：各自一個資料夾，不互相蓋掉；列出時由新到舊。
#[test]
fn two_removals_get_separate_trash_folders() {
    let mut fx = setup();
    let a = fx.add_translation(7);
    let b = fx.add_translation(8);
    let tok = fx.token();
    let tok = fx.lib.remove_material(&fx.paper, &a, &tok).unwrap();
    fx.lib.remove_material(&fx.paper, &b, &tok).unwrap();
    assert_eq!(fx.trash_folders().len(), 2);
    let removed = fx.lib.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 2);
    let ids: Vec<&str> = removed.iter().map(|r| r.material.id.as_str()).collect();
    assert!(ids.contains(&a.as_str()) && ids.contains(&b.as_str()));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

/// 備份帶著移除的材料；還原之後照樣列得出來。
#[test]
fn backups_carry_removed_materials() {
    let mut fx = setup();
    let tid = fx.add_translation(7);
    let tok = fx.token();
    fx.lib.remove_material(&fx.paper, &tid, &tok).unwrap();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    fx.lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let target = out.path().join("restored");
    paperreader_core::backup::restore_backup(&zip, &target, DEVICE_B).unwrap();
    let lib2 = LocalLibrary::open(&target, DEVICE_B).unwrap();
    let removed = lib2.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].material.id, tid);
}

/// 同步：A 移除一項材料，B 匯入後那一項也拿掉，B 的檔搬進 B 的回收區（不留成清單沒列的檔）。
#[test]
fn sync_moves_the_removed_material_into_the_other_devices_trash() {
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
    let report = sync_once(&mut p.a, &mut p.b, &out);
    assert!(
        report
            .removed
            .iter()
            .any(|i| i.kind == paperreader_core::store::ItemKind::Material),
        "{report:?}"
    );
    let b_paper = p.b.get_paper(&alpha.id).unwrap().0;
    assert!(b_paper.materials.iter().all(|m| m.id != tid));
    let b_root = p.b_root();
    let b_folder = b_root.join("papers").join(&alpha.folder);
    assert!(!b_folder.join("translation.pdf").exists());
    let removed = p.b.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 1, "{removed:?}");
    assert_eq!(removed[0].material.id, tid);
    assert_eq!(removed[0].paper_id, alpha.id);
    assert_eq!(p.b.check(CheckLevel::Quick).unwrap(), vec![]);

    // 再匯入一次：什麼都不多做
    let again = sync_once(&mut p.a, &mut p.b, &out);
    assert!(again.removed.is_empty(), "{again:?}");
    assert_eq!(p.b.list_removed_materials().unwrap().len(), 1);
}

/// 搬檔失敗（Windows 上檔案被別的程式以不分享的方式開著）時，材料留在清單裡、回收區不留
/// 空的資料夾；記錄已經寫了。關掉之後再移除一次就成功。
#[cfg(windows)]
#[test]
fn a_failed_move_keeps_the_material_listed_and_leaves_no_trash_folder() {
    use std::os::windows::fs::OpenOptionsExt;
    let mut fx = setup();
    let tid = fx.add_translation(7);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fx.folder().join("translation.pdf"))
        .unwrap();
    let tok = fx.token();
    assert!(fx.lib.remove_material(&fx.paper, &tid, &tok).is_err());
    let paper = fx.lib.get_paper(&fx.paper).unwrap().0;
    assert!(paper.materials.iter().any(|m| m.id == tid), "still listed");
    assert!(fx.folder().join("translation.pdf").is_file());
    assert!(fx.trash_folders().is_empty(), "{:?}", fx.trash_folders());
    assert!(fx.lib.list_removed_materials().unwrap().is_empty());
    drop(held);
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
    let tok = fx.token();
    fx.lib.remove_material(&fx.paper, &tid, &tok).unwrap();
    assert_eq!(fx.lib.list_removed_materials().unwrap().len(), 1);
}

/// 說明檔先用 `.pr-` 的暫存名寫好，搬好檔才改成正式的名稱。被強制結束留下的半成品，
/// 下次開啟時：檔已經搬進去的補上正式名稱；只有暫存說明檔或空的資料夾清掉；使用者自己的東西不動。
#[test]
fn half_finished_removals_are_finished_or_cleaned_on_open() {
    let mut fx = setup();
    let a = fx.add_translation(7);
    let b = fx.add_translation(8);
    let tok = fx.token();
    let tok = fx.lib.remove_material(&fx.paper, &a, &tok).unwrap();
    fx.lib.remove_material(&fx.paper, &b, &tok).unwrap();
    let t = fx.trash();
    let folders = fx.trash_folders();
    assert_eq!(folders.len(), 2);
    let tmp = format!(".pr-{REMOVED_MATERIAL_FILE}");
    // 1：檔已經搬進去、說明檔還是暫存名
    fs::rename(
        t.join(&folders[0]).join(REMOVED_MATERIAL_FILE),
        t.join(&folders[0]).join(&tmp),
    )
    .unwrap();
    // 2：只有暫存說明檔（檔還沒搬）
    let only_marker = "20200101T000000Z-Zhao Paper - x.pdf";
    fs::create_dir_all(t.join(only_marker)).unwrap();
    fs::copy(
        t.join(&folders[1]).join(REMOVED_MATERIAL_FILE),
        t.join(only_marker).join(&tmp),
    )
    .unwrap();
    // 3：空的資料夾（建好資料夾就被中斷）
    let empty = "20200101T000001Z-Zhao Paper - y.pdf";
    fs::create_dir_all(t.join(empty)).unwrap();
    // 4：使用者自己的東西
    fs::create_dir_all(t.join("my stuff")).unwrap();
    fs::write(t.join("my stuff").join("keep.txt"), "keep").unwrap();
    assert_eq!(
        fx.lib.list_removed_materials().unwrap().len(),
        1,
        "a temporary marker is not listed"
    );

    let root = fx.dir.path().to_path_buf();
    drop(fx.lib);
    let lib = LocalLibrary::open(&root, DEVICE).unwrap();
    assert!(t.join(&folders[0]).join(REMOVED_MATERIAL_FILE).is_file());
    assert!(!t.join(&folders[0]).join(&tmp).exists());
    assert!(!t.join(only_marker).exists());
    assert!(!t.join(empty).exists());
    assert!(t.join("my stuff").join("keep.txt").is_file());
    assert_eq!(lib.list_removed_materials().unwrap().len(), 2);
}

/// 同步寫入在 `TrashMaterial` 這一項之前被強制結束，而且回收區留著上次搬到一半的暫存說明檔：
/// 下次開啟時做完計畫，回收區裡那項材料只有一份。
#[test]
fn an_interrupted_trash_material_step_is_finished_once_on_the_next_open() {
    use paperreader_core::sync::plan::{PLAN_FILE, Plan, PlanItem};
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
    let pkg = export_pkg(&mut p.a, p.out.path());
    let preview = p.b.prepare_sync_import(&pkg, &mut |_, _| {}).unwrap();
    // 停在 TrashMaterial 那一項之前
    let b_root = p.b_root();
    let stopped_at = std::sync::Arc::new(std::sync::Mutex::new(None::<usize>));
    {
        let root = b_root.clone();
        let stopped_at = stopped_at.clone();
        p.b.set_sync_apply_hook(Some(Box::new(move |k| {
            if k == 0 {
                return true;
            }
            let staging = fs::read_dir(&root)
                .unwrap()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
                .unwrap()
                .path();
            let plan: Plan =
                serde_json::from_slice(&fs::read(staging.join(PLAN_FILE)).unwrap()).unwrap();
            let at = plan
                .items
                .iter()
                .position(|i| matches!(i, PlanItem::TrashMaterial { .. }))
                .unwrap()
                + 1;
            if k == at {
                *stopped_at.lock().unwrap() = Some(at);
            }
            k < at
        })));
    }
    assert!(p.b.apply_sync_import(&preview.preview_id, &[]).is_err());
    assert!(stopped_at.lock().unwrap().is_some());
    // 上一次搬到一半：回收區有一個只有暫存說明檔的資料夾
    let t = b_root.join(".trash");
    let half = "20200101T000000Z-Alpha Paper - translation.pdf";
    fs::create_dir_all(t.join(half)).unwrap();
    fs::write(
        t.join(half).join(format!(".pr-{REMOVED_MATERIAL_FILE}")),
        "{}",
    )
    .unwrap();
    drop(p.b);
    let lib = LocalLibrary::open(&b_root, DEVICE_B).unwrap();
    let removed = lib.list_removed_materials().unwrap();
    assert_eq!(removed.len(), 1, "{removed:?}");
    assert_eq!(removed[0].material.id, tid);
    assert!(!t.join(half).exists());
    assert_eq!(lib.check(CheckLevel::Quick).unwrap(), vec![]);
}
