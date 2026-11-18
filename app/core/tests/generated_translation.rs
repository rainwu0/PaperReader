//! 產生的譯本 PDF：由譯文疊加檔產生、存成這篇的譯本材料，重新產生時原地更新。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use paperreader_core::store::{CheckLevel, ProblemKind, PutGenerated};
use serde_json::Value;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    overlay: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Paper", Some(10));
    let t = lib.get_paper(&paper).unwrap().1;
    let created = lib
        .create_overlay(
            &paper,
            OverlayDraft {
                source_material: source,
                provider: "anthropic".into(),
                model: "m".into(),
                source_lang: None,
                target_lang: "zh-TW".into(),
                segments: vec![SegmentDraft {
                    page: 0,
                    bbox: [10.0, 10.0, 100.0, 40.0],
                    kind: "body".into(),
                    size: Some(10.0),
                    source: "Hello world".into(),
                }],
            },
            &t,
        )
        .unwrap();
    Fx {
        dir,
        lib,
        paper,
        overlay: created.material_id,
    }
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers/Paper")
    }
    fn token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    fn value(&self) -> Value {
        serde_json::from_slice(&fs::read(self.folder().join("paper.json")).unwrap()).unwrap()
    }
    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.folder())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
    fn put(&mut self, bytes: Vec<u8>, pages: Option<u32>) -> Result<PutGenerated, LibError> {
        let t = self.token();
        self.lib.put_generated_translation(
            &self.paper.clone(),
            &self.overlay.clone(),
            &mut Cursor::new(bytes),
            pages,
            &t,
        )
    }
    /// 材料清單裡產生的譯本那一項。
    fn generated(&self) -> Value {
        self.value()["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "translation" && m["form"] == "pdf")
            .cloned()
            .expect("a generated translation")
    }
}

fn pdf_bytes(seed: u64) -> Vec<u8> {
    fake_bytes(seed, 900)
}

#[test]
fn first_put_adds_a_translation_pdf_material_with_provenance() {
    let mut fx = setup();
    let bytes = pdf_bytes(7);
    let r = fx.put(bytes.clone(), Some(10)).unwrap();
    let PutGenerated::Created { material_id, .. } = r else {
        panic!("expected Created, got {r:?}");
    };
    assert!(
        fx.names()
            .contains(&"translation-generated.pdf".to_string())
    );
    assert_eq!(
        fs::read(fx.folder().join("translation-generated.pdf")).unwrap(),
        bytes
    );
    let m = fx.generated();
    assert_eq!(m["id"], material_id.as_str());
    assert_eq!(m["role"], "translation");
    assert_eq!(m["form"], "pdf");
    assert_eq!(m["path"], "translation-generated.pdf");
    assert_eq!(m["sha256"], sha256_hex(&bytes).as_str());
    assert_eq!(m["bytes"], bytes.len());
    assert_eq!(m["pages"], 10);
    let p = &m["provenance"];
    assert_eq!(p["method"], "generated");
    assert!(p["time"].is_string());
    assert_eq!(p["proofread"], false);
    assert_eq!(p["generated_from"], fx.overlay.as_str());
    // 來源原文的指紋跟著疊加檔那一項
    let overlay = fx.value()["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["form"] == "overlay")
        .cloned()
        .unwrap();
    assert_eq!(p["source_sha256"], overlay["provenance"]["source_sha256"]);
    // 沒有暫存檔留下、檢查沒有問題
    assert!(fx.names().iter().all(|n| !n.starts_with(".pr-")));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn second_put_updates_the_same_material_in_place() {
    let mut fx = setup();
    let PutGenerated::Created { material_id, .. } = fx.put(pdf_bytes(1), Some(10)).unwrap() else {
        panic!("expected Created");
    };
    let before = fx.generated();
    let newer = pdf_bytes(2);
    let r = fx.put(newer.clone(), Some(10)).unwrap();
    let PutGenerated::Updated {
        material_id: again, ..
    } = r
    else {
        panic!("expected Updated, got {r:?}");
    };
    assert_eq!(again, material_id, "同一份材料原地更新，代號不變");
    let after = fx.generated();
    assert_eq!(after["id"], before["id"]);
    assert_eq!(after["path"], before["path"]);
    assert_ne!(after["sha256"], before["sha256"], "新的版本記號");
    assert_eq!(after["sha256"], sha256_hex(&newer).as_str());
    assert_eq!(after["bytes"], newer.len());
    assert_eq!(
        fs::read(fx.folder().join("translation-generated.pdf")).unwrap(),
        newer
    );
    // 只有一份產生的譯本，沒有多出檔案
    let n = fx.value()["materials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["form"] == "pdf" && m["role"] == "translation")
        .count();
    assert_eq!(n, 1);
    assert!(fx.names().iter().all(|n| !n.starts_with(".pr-")));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn identical_content_changes_nothing() {
    let mut fx = setup();
    fx.put(pdf_bytes(3), Some(10)).unwrap();
    let before = fx.value();
    let r = fx.put(pdf_bytes(3), Some(10)).unwrap();
    assert!(matches!(r, PutGenerated::Unchanged { .. }), "{r:?}");
    assert_eq!(fx.value(), before);
}

#[test]
fn a_users_own_translation_pdf_is_never_touched() {
    let mut fx = setup();
    // 使用者自己加入的譯本
    let src = tempfile::tempdir().unwrap();
    let theirs = fake_pdf_seeded(src.path(), "t.pdf", 600, 99);
    let t = fx.token();
    fx.lib
        .add_translation(&fx.paper.clone(), &theirs, Some(10), &t)
        .unwrap();
    let mine = fs::read(fx.folder().join("translation.pdf")).unwrap();
    fx.put(pdf_bytes(4), Some(10)).unwrap();
    fx.put(pdf_bytes(5), Some(10)).unwrap();
    assert_eq!(fs::read(fx.folder().join("translation.pdf")).unwrap(), mine);
    let names = fx.names();
    assert!(names.contains(&"translation.pdf".to_string()));
    assert!(names.contains(&"translation-generated.pdf".to_string()));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn not_a_pdf_or_empty_is_refused_and_leaves_nothing() {
    let mut fx = setup();
    let before = fx.value();
    let r = fx.put(vec![b'x'; 2000], Some(10));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = fx.put(vec![], Some(10));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert_eq!(fx.value(), before);
    assert!(
        fx.names()
            .iter()
            .all(|n| !n.starts_with(".pr-") && !n.contains("generated"))
    );
}

#[test]
fn unknown_overlay_or_stale_token_is_refused() {
    let mut fx = setup();
    let t = fx.token();
    let r = fx.lib.put_generated_translation(
        &fx.paper.clone(),
        "no-such-material",
        &mut Cursor::new(pdf_bytes(6)),
        Some(10),
        &t,
    );
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    // 版本記號過期
    let stale = fx.token();
    fx.put(pdf_bytes(6), Some(10)).unwrap();
    let r = fx.lib.put_generated_translation(
        &fx.paper.clone(),
        &fx.overlay.clone(),
        &mut Cursor::new(pdf_bytes(8)),
        Some(10),
        &stale,
    );
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert!(fx.names().iter().all(|n| !n.starts_with(".pr-")));
}

#[test]
fn read_only_library_refuses() {
    let fx = setup();
    let t = fx.token();
    // library.json 的 format 較新 → 唯讀
    let lp = fx.dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = serde_json::json!(2);
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut ro = LocalLibrary::open(fx.dir.path(), DEVICE).unwrap();
    let r = ro.put_generated_translation(
        &fx.paper.clone(),
        &fx.overlay.clone(),
        &mut Cursor::new(pdf_bytes(9)),
        Some(10),
        &t,
    );
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}

#[test]
fn page_count_out_of_range_is_refused() {
    let mut fx = setup();
    let r = fx.put(pdf_bytes(10), Some(0));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    assert!(fx.names().iter().all(|n| !n.contains("generated")));
}

#[test]
fn generated_pdf_is_a_normal_library_file_for_backup() {
    let mut fx = setup();
    let bytes = pdf_bytes(11);
    fx.put(bytes.clone(), Some(10)).unwrap();
    // 是文獻庫裡一般的檔案，備份（依資料夾內容）自然帶著它
    let files = backup_worthy_files(fx.dir.path());
    assert_eq!(
        files.get("papers/Paper/translation-generated.pdf"),
        Some(&bytes)
    );
    let _ = ProblemKind::MaterialMissing;
}

#[test]
fn update_when_the_old_file_is_gone_puts_the_new_one_back() {
    let mut fx = setup();
    fx.put(pdf_bytes(20), Some(10)).unwrap();
    // 使用者在檔案管理員把它刪了
    fs::remove_file(fx.folder().join("translation-generated.pdf")).unwrap();
    // 內容相同也不能當作「沒變」：檔案不在
    let r = fx.put(pdf_bytes(20), Some(10)).unwrap();
    assert!(matches!(r, PutGenerated::Updated { .. }), "{r:?}");
    assert_eq!(
        fs::read(fx.folder().join("translation-generated.pdf")).unwrap(),
        pdf_bytes(20)
    );
    // 內容不同也一樣
    fs::remove_file(fx.folder().join("translation-generated.pdf")).unwrap();
    let r = fx.put(pdf_bytes(21), Some(10)).unwrap();
    assert!(matches!(r, PutGenerated::Updated { .. }), "{r:?}");
    assert!(fx.names().iter().all(|n| !n.starts_with(".pr-")));
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn a_corrupted_material_path_is_refused_and_nothing_is_touched() {
    for bad in ["source.pdf", "../escape.pdf", "/abs.pdf", "@hidden.pdf"] {
        let mut fx = setup();
        fx.put(pdf_bytes(30), Some(10)).unwrap();
        let source_before = fs::read(fx.folder().join("source.pdf")).unwrap();
        // paper.json 被改壞：產生的譯本那一項的 path 指到別處
        let mut v = fx.value();
        for m in v["materials"].as_array_mut().unwrap() {
            if m["role"] == "translation" && m["form"] == "pdf" {
                m["path"] = Value::String(bad.to_string());
            }
        }
        fs::write(
            fx.folder().join("paper.json"),
            serde_json::to_string_pretty(&v).unwrap(),
        )
        .unwrap();
        fx.lib.rescan().unwrap();
        let before = fx.value();
        let r = fx.put(pdf_bytes(31), Some(10));
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
        assert_eq!(fx.value(), before, "{bad}");
        assert_eq!(
            fs::read(fx.folder().join("source.pdf")).unwrap(),
            source_before,
            "{bad}: 原文不能被動到"
        );
        assert!(fx.names().iter().all(|n| !n.starts_with(".pr-")), "{bad}");
    }
}

#[test]
fn highlights_and_strokes_on_the_generated_pdf_survive_an_in_place_update() {
    use paperreader_core::notes::Quote;
    use paperreader_core::store::{HighlightDraft, StrokeDraft};
    let mut fx = setup();
    let PutGenerated::Created { material_id, .. } = fx.put(pdf_bytes(40), Some(10)).unwrap() else {
        panic!("expected Created");
    };
    let (h, _) = fx
        .lib
        .add_highlight(
            &fx.paper.clone(),
            HighlightDraft {
                material: material_id.clone(),
                page_index: 0,
                page_label: None,
                rects: vec![[10.0, 10.0, 80.0, 24.0]],
                next_page_rects: vec![],
                position: None,
                quote: Quote {
                    exact: "譯文的一句話".into(),
                    prefix: None,
                    suffix: None,
                    extra: Default::default(),
                },
                fingerprint: None,
                comment: "短評".into(),
            },
            None,
        )
        .unwrap();
    let (s, _) = fx
        .lib
        .add_stroke(
            &fx.paper.clone(),
            StrokeDraft {
                material: material_id.clone(),
                page_index: 0,
                color: "#d93025".into(),
                width: 1.8,
                opacity: 1.0,
                points: vec![[10.0, 10.0], [50.0, 30.0]],
            },
            None,
        )
        .unwrap();
    // 原地更新
    let r = fx.put(pdf_bytes(41), Some(10)).unwrap();
    assert!(matches!(r, PutGenerated::Updated { .. }), "{r:?}");
    let (notes, _) = fx.lib.read_notes(&fx.paper.clone()).unwrap();
    let kept = notes
        .highlights
        .iter()
        .find(|x| x.id == h.id)
        .expect("劃線還在");
    assert_eq!(kept.material, material_id);
    assert_eq!(kept.comment, "短評");
    assert_eq!(kept.quote.exact, "譯文的一句話");
    let (ink, _) = fx.lib.read_ink(&fx.paper.clone()).unwrap();
    let st = ink.strokes.iter().find(|x| x.id == s.id).expect("筆跡還在");
    assert_eq!(st.material, material_id);
    assert_eq!(fx.lib.check(CheckLevel::Full).unwrap(), vec![]);
}

#[test]
fn a_listed_generated_pdf_whose_file_is_gone_keeps_its_name() {
    // 清單裡別的材料用著的檔名也算占用
    let mut fx = setup();
    fx.put(pdf_bytes(1), Some(10)).unwrap();
    fs::remove_file(fx.folder().join("translation-generated.pdf")).unwrap();
    // 第二份譯文疊加檔產生的譯本 PDF：不可以拿走第一份的檔名
    let source = fx.value()["materials"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let t = fx.token();
    let second = fx
        .lib
        .create_overlay(
            &fx.paper.clone(),
            OverlayDraft {
                source_material: source,
                provider: "anthropic".into(),
                model: "m".into(),
                source_lang: None,
                target_lang: "zh-TW".into(),
                segments: vec![SegmentDraft {
                    page: 0,
                    bbox: [10.0, 10.0, 100.0, 40.0],
                    kind: "body".into(),
                    size: Some(10.0),
                    source: "Hi".into(),
                }],
            },
            &t,
        )
        .unwrap();
    let t = fx.token();
    let r = fx
        .lib
        .put_generated_translation(
            &fx.paper.clone(),
            &second.material_id,
            &mut Cursor::new(pdf_bytes(2)),
            Some(10),
            &t,
        )
        .unwrap();
    let PutGenerated::Created { material_id, .. } = r else {
        panic!("{r:?}")
    };
    let mats = fx.value()["materials"].as_array().unwrap().clone();
    let path = mats
        .iter()
        .find(|m| m["id"] == material_id.as_str())
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(
        path, "translation-generated.pdf",
        "two materials never share a file"
    );
}
