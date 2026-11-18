//! 工作 4：加入譯本。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{AddMaterialOutcome, CheckLevel, ProblemKind};
use serde_json::{Value, json};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// 一個文獻庫與一篇文獻（資料夾 `Paper`）。
struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    src: tempfile::TempDir,
    paper: String,
    source_material: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source_material) = add_paper(&mut lib, 1, "Paper", Some(10));
    Fx {
        dir,
        lib,
        src: tempfile::tempdir().unwrap(),
        paper,
        source_material,
    }
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers/Paper")
    }
    fn paper_json(&self) -> PathBuf {
        self.folder().join("paper.json")
    }
    fn token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    fn pdf(&self, name: &str, seed: u64) -> PathBuf {
        fake_pdf_seeded(self.src.path(), name, 600, seed)
    }
    fn names(&self) -> Vec<String> {
        names_in(&self.folder())
    }
    fn value(&self) -> Value {
        serde_json::from_slice(&fs::read(self.paper_json()).unwrap()).unwrap()
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn added(o: AddMaterialOutcome) -> (String, VersionToken) {
    match o {
        AddMaterialOutcome::Added { material_id, token } => (material_id, token),
        other => panic!("expected Added, got {other:?}"),
    }
}

/// 第一次被讀時先做一件事的資料流。
struct HookedReader<F: FnOnce()> {
    data: Cursor<Vec<u8>>,
    hook: Option<F>,
}

impl<F: FnOnce()> Read for HookedReader<F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(h) = self.hook.take() {
            h();
        }
        self.data.read(buf)
    }
}

#[test]
fn add_translation_writes_spec_layout() {
    let mut fx = setup();
    let before = fx.value();
    let pdf = fx.pdf("t.pdf", 7);
    let src_bytes = fs::read(&pdf).unwrap();
    let t = fx.token();
    let (mid, _) = added(
        fx.lib
            .add_translation(&fx.paper.clone(), &pdf, Some(12), &t)
            .unwrap(),
    );
    assert_eq!(fx.names(), ["paper.json", "source.pdf", "translation.pdf"]);
    assert_eq!(
        fs::read(fx.folder().join("translation.pdf")).unwrap(),
        src_bytes
    );
    let after = fx.value();
    let m = &after["materials"][1];
    assert_eq!(m["id"], mid.as_str());
    assert_eq!(m["role"], "translation");
    assert_eq!(m["form"], "pdf");
    assert_eq!(m["path"], "translation.pdf");
    assert_eq!(m["sha256"], sha256_hex(&src_bytes).as_str());
    assert_eq!(m["bytes"], src_bytes.len());
    assert_eq!(m["pages"], 12);
    assert_eq!(m["pages_modified_by"], DEVICE);
    assert!(m["pages_modified"].is_string());
    assert_eq!(m["title"], Value::Null);
    let p = &m["provenance"];
    assert_eq!(p["method"], "added-by-user");
    assert!(p["time"].is_string());
    assert_eq!(p["origin_name"], "t.pdf");
    assert_eq!(p["proofread"], Value::Null);
    assert_eq!(p["page_map"], Value::Null);
    assert_eq!(p["source_sha256"], Value::Null);
    // 原文那一項與 bib 不變
    assert_eq!(before["materials"][0], after["materials"][0]);
    assert_eq!(before["bib"], after["bib"]);
    // 來源檔仍在
    assert_eq!(fs::read(&pdf).unwrap(), src_bytes);
}

#[test]
fn add_translation_without_pages_has_null_pages_time() {
    let mut fx = setup();
    let pdf = fx.pdf("t.pdf", 7);
    let t = fx.token();
    fx.lib
        .add_translation(&fx.paper.clone(), &pdf, None, &t)
        .unwrap();
    let m = &fx.value()["materials"][1];
    assert_eq!(m["pages"], Value::Null);
    assert_eq!(m["pages_modified"], Value::Null);
    assert_eq!(m["pages_modified_by"], Value::Null);
}

#[test]
fn second_translation_gets_suffix() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let a = fx.pdf("a.pdf", 7);
    let b = fx.pdf("b.pdf", 8);
    let (_, t) = added(fx.lib.add_translation(&id, &a, None, &fx.token()).unwrap());
    fx.lib.add_translation(&id, &b, None, &t).unwrap();
    assert_eq!(
        fx.names(),
        [
            "paper.json",
            "source.pdf",
            "translation-2.pdf",
            "translation.pdf"
        ]
    );
}

#[test]
fn user_file_with_same_name_is_not_overwritten() {
    let mut fx = setup();
    let mine = b"my own file".to_vec();
    fs::write(fx.folder().join("Translation.PDF"), &mine).unwrap();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    fx.lib
        .add_translation(&id, &pdf, None, &fx.token())
        .unwrap();
    let names = fx.names();
    assert!(
        names.contains(&"translation-2.pdf".to_string()),
        "{names:?}"
    );
    assert_eq!(fs::read(fx.folder().join("Translation.PDF")).unwrap(), mine);
}

#[test]
fn same_file_twice_is_already_present() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    let (mid, t) = added(
        fx.lib
            .add_translation(&id, &pdf, None, &fx.token())
            .unwrap(),
    );
    let bytes = fs::read(fx.paper_json()).unwrap();
    let names = fx.names();
    let out = fx.lib.add_translation(&id, &pdf, None, &t).unwrap();
    assert_eq!(out, AddMaterialOutcome::AlreadyPresent { material_id: mid });
    assert_eq!(fx.names(), names);
    assert_eq!(fs::read(fx.paper_json()).unwrap(), bytes);
}

#[test]
fn source_file_as_translation_is_already_present() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let source = fx.folder().join("source.pdf");
    let t = fx.token();
    let out = fx.lib.add_translation(&id, &source, None, &t).unwrap();
    assert_eq!(
        out,
        AddMaterialOutcome::AlreadyPresent {
            material_id: fx.source_material.clone()
        }
    );
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
}

#[test]
fn non_pdf_and_empty_are_rejected() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let t = fx.token();
    let text = fx.src.path().join("a.txt");
    fs::write(&text, b"just text").unwrap();
    let empty = fx.src.path().join("empty.pdf");
    fs::write(&empty, b"").unwrap();
    let missing = fx.src.path().join("missing.pdf");

    match fx.lib.add_translation(&id, &text, None, &t) {
        Err(LibError::InvalidInput { reason }) => assert!(reason.starts_with("not a PDF")),
        other => panic!("{other:?}"),
    }
    let r = fx.lib.add_translation(&id, &empty, None, &t);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = fx.lib.add_translation(&id, fx.src.path(), None, &t);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = fx.lib.add_translation(&id, &missing, None, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);

    // 資料流：空的與不是 PDF 的
    let r = fx
        .lib
        .add_translation_from_reader(&id, &mut Cursor::new(Vec::new()), None, None, &t);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = fx.lib.add_translation_from_reader(
        &id,
        &mut Cursor::new(b"no marker".to_vec()),
        None,
        None,
        &t,
    );
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
}

#[test]
fn pages_range_is_checked() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    let t = fx.token();
    let bytes = fs::read(fx.paper_json()).unwrap();
    for bad in [0u32, 100_001] {
        let r = fx.lib.add_translation(&id, &pdf, Some(bad), &t);
        assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    }
    assert_eq!(fs::read(fx.paper_json()).unwrap(), bytes);
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
}

#[test]
fn stale_token_is_conflict_before_copy() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let stale = fx.token();
    let mut v = fx.value();
    v["bib"]["title"] = json!("Changed");
    fs::write(fx.paper_json(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let pdf = fx.pdf("a.pdf", 7);
    let r = fx.lib.add_translation(&id, &pdf, None, &stale);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
}

#[test]
fn paper_changed_during_copy_is_conflict() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let t = fx.token();
    let pj = fx.paper_json();
    let mut v = fx.value();
    v["bib"]["title"] = json!("Changed During Copy");
    let text = serde_json::to_string_pretty(&v).unwrap();
    let mut reader = HookedReader {
        data: Cursor::new(fake_bytes(5, 800)),
        hook: Some(move || fs::write(&pj, text).unwrap()),
    };
    let r = fx
        .lib
        .add_translation_from_reader(&id, &mut reader, Some("x.pdf"), None, &t);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
    assert_eq!(fx.value()["bib"]["title"], "Changed During Copy");
}

#[test]
fn folder_renamed_during_copy_is_conflict() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let t = fx.token();
    let old = fx.folder();
    let new = fx.dir.path().join("papers/Renamed");
    let (o2, n2) = (old.clone(), new.clone());
    let renamed_ok = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = renamed_ok.clone();
    let mut reader = HookedReader {
        data: Cursor::new(fake_bytes(5, 800)),
        hook: Some(move || flag.set(fs::rename(&o2, &n2).is_ok())),
    };
    let r = fx
        .lib
        .add_translation_from_reader(&id, &mut reader, Some("x.pdf"), None, &t);
    if !renamed_ok.get() {
        // Windows 不允許改名一個裡面有開著的檔案的資料夾；這一項只能在別的平台驗證
        eprintln!("SKIPPED: this platform cannot rename a folder that holds an open file");
        return;
    }
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(names_in(&new), ["paper.json", "source.pdf"]);
    assert!(!old.exists());
}

#[test]
fn folder_renamed_after_copy_is_conflict_and_cleans_temp() {
    // 複製途中資料夾被改名。Windows 不允許在複製途中（暫存檔開著）改名，所以用測試掛勾
    // 在複製完之後、檢查檔頭之前改名；暫存檔因此跟著搬到新名稱的資料夾。確認 cc388d4 的兩件事：
    // 檔頭不再按（已不存在的）舊路徑檢查，所以不是 NotFound 而是 Conflict；改名後資料夾裡的暫存檔被清掉。
    let mut fx = setup();
    let id = fx.paper.clone();
    let t = fx.token();
    let old = fx.folder();
    let new = fx.dir.path().join("papers/Renamed");
    let (o, n) = (old.clone(), new.clone());
    fx.lib
        .set_translation_copy_hook(Some(Box::new(move || fs::rename(&o, &n).unwrap())));
    let mut reader = Cursor::new(fake_bytes(5, 800));
    let r = fx
        .lib
        .add_translation_from_reader(&id, &mut reader, Some("x.pdf"), None, &t);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert!(!old.exists());
    assert_eq!(names_in(&new), ["paper.json", "source.pdf"]);
    // 掛勾只呼叫一次：之後（記號要重新取）的加入照常成功
    let t2 = fx.lib.get_paper(&id).unwrap().1;
    let ok = fx
        .lib
        .add_translation_from_reader(&id, &mut Cursor::new(fake_bytes(6, 800)), None, None, &t2)
        .unwrap();
    assert!(matches!(ok, AddMaterialOutcome::Added { .. }), "{ok:?}");
    assert_eq!(
        names_in(&new),
        ["paper.json", "source.pdf", "translation.pdf"]
    );
}

#[test]
fn unknown_paper_and_trash_are_not_found() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    let t = fx.token();
    let r = fx
        .lib
        .add_translation("01JCKA1B2C3D4E5F6G7H8J9K0M", &pdf, None, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    fx.lib.remove_paper(&id).unwrap();
    let r = fx.lib.add_translation(&id, &pdf, None, &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn read_only_is_refused() {
    let fx = setup();
    let Fx {
        dir, paper, src, ..
    } = fx;
    let pdf = fake_pdf_seeded(src.path(), "a.pdf", 600, 7);
    let t = {
        let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
        lib.get_paper(&paper).unwrap().1
    };
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let r = lib.add_translation(&paper, &pdf, None, &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}

#[test]
fn newer_format_is_refused() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let mut v = fx.value();
    v["format"] = json!(2);
    fs::write(fx.paper_json(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    fx.lib.rescan().unwrap();
    let pdf = fx.pdf("a.pdf", 7);
    let t = fx.token();
    let r = fx.lib.add_translation(&id, &pdf, None, &t);
    assert!(matches!(r, Err(LibError::FormatTooNew { .. })), "{r:?}");
    assert_eq!(fx.names(), ["paper.json", "source.pdf"]);
}

#[test]
fn interrupted_after_rename_leaves_unlisted_file() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    fs::copy(&pdf, fx.folder().join("translation.pdf")).unwrap();
    let problems = fx.lib.check(CheckLevel::Quick).unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.kind == ProblemKind::UnlistedFile && p.path.ends_with("translation.pdf")),
        "{problems:?}"
    );
    let out = fx
        .lib
        .add_translation(&id, &pdf, None, &fx.token())
        .unwrap();
    assert!(matches!(out, AddMaterialOutcome::Added { .. }), "{out:?}");
    assert!(fx.names().contains(&"translation-2.pdf".to_string()));
}

#[test]
fn translation_visible_without_rescan() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    fx.lib
        .add_translation(&id, &pdf, None, &fx.token())
        .unwrap();
    let s = fx
        .lib
        .list_papers()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(s.materials.len(), 2);
    assert_eq!(s.materials[1].role, "translation");
    assert_eq!(
        s.materials[1].status,
        paperreader_core::store::MaterialStatus::Ok
    );
}

#[test]
fn material_path_and_events_work_for_translation() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    let (mid, _) = added(
        fx.lib
            .add_translation(&id, &pdf, Some(5), &fx.token())
            .unwrap(),
    );
    let p = fx.lib.material_path(&id, &mid).unwrap();
    assert_eq!(p, fx.folder().join("translation.pdf"));
    fx.lib.record_open(&id, &mid).unwrap();
    fx.lib.record_position(&id, &mid, 2, 0.5, None).unwrap();
}

#[test]
fn returned_token_allows_next_write() {
    let mut fx = setup();
    let id = fx.paper.clone();
    let pdf = fx.pdf("a.pdf", 7);
    let (_, t) = added(
        fx.lib
            .add_translation(&id, &pdf, None, &fx.token())
            .unwrap(),
    );
    let draft = BibDraft {
        title: Some("After".into()),
        ..Default::default()
    };
    fx.lib.update_bib(&id, draft, &t).unwrap();
}
