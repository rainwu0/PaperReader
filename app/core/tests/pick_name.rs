//! 新材料挑檔名時，資料夾裡實際的檔與清單裡每一項材料的檔名都算被占用
//! （清單列了、檔卻被刪掉的名稱不可以再被挑中，免得兩項材料指向同一個檔）。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use paperreader_core::store::AddMaterialOutcome;
use std::fs;

fn paths(lib: &LocalLibrary, paper: &str, role: &str) -> Vec<String> {
    lib.get_paper(paper)
        .unwrap()
        .0
        .materials
        .into_iter()
        .filter(|m| m.role == role)
        .map(|m| m.path)
        .collect()
}

#[test]
fn a_new_translation_does_not_reuse_a_listed_name_whose_file_is_gone() {
    let (dir, mut lib) = temp_library();
    let (paper, _) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    let src = tempfile::tempdir().unwrap();
    for seed in [7, 8] {
        if seed == 8 {
            fs::remove_file(dir.path().join("papers/Zhao Paper/translation.pdf")).unwrap();
        }
        let pdf = fake_pdf_seeded(src.path(), &format!("t{seed}.pdf"), 400, seed);
        let tok = lib.get_paper(&paper).unwrap().1;
        let r = lib.add_translation(&paper, &pdf, Some(10), &tok).unwrap();
        assert!(matches!(r, AddMaterialOutcome::Added { .. }), "{r:?}");
    }
    let p = paths(&lib, &paper, "translation");
    assert_eq!(p.len(), 2);
    assert_ne!(p[0].to_lowercase(), p[1].to_lowercase(), "{p:?}");
}

#[test]
fn a_new_overlay_does_not_reuse_a_listed_name_whose_file_is_gone() {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    for round in 0..2 {
        if round == 1 {
            fs::remove_file(
                dir.path()
                    .join("papers/Zhao Paper/translation-overlay.json"),
            )
            .unwrap();
        }
        let tok = lib.get_paper(&paper).unwrap().1;
        lib.create_overlay(
            &paper,
            OverlayDraft {
                source_material: source.clone(),
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
        .unwrap();
    }
    let p = paths(&lib, &paper, "translation");
    assert_eq!(p.len(), 2);
    assert_ne!(p[0].to_lowercase(), p[1].to_lowercase(), "{p:?}");
}

/// 同步帶來的材料挑檔名（`free_name`）：在最後一個點（不在開頭）之前加 `-2`、`-3` …；`taken` 是小寫的名稱。
#[test]
fn free_name_numbers_before_the_last_dot() {
    use paperreader_core::sync::merge::paper::free_name;
    use std::collections::HashSet;
    let taken =
        |names: &[&str]| -> HashSet<String> { names.iter().map(|s| s.to_string()).collect() };
    assert_eq!(free_name(&taken(&[]), "a.pdf"), "a.pdf");
    assert_eq!(free_name(&taken(&["a.pdf"]), "A.PDF"), "A-2.PDF");
    assert_eq!(free_name(&taken(&["a.pdf", "a-2.pdf"]), "a.pdf"), "a-3.pdf");
    assert_eq!(free_name(&taken(&["readme"]), "README"), "README-2");
    assert_eq!(free_name(&taken(&[".hidden"]), ".hidden"), ".hidden-2");
    assert_eq!(free_name(&taken(&["a.tar.gz"]), "a.tar.gz"), "a.tar-2.gz");
    assert_eq!(free_name(&taken(&["x."]), "x."), "x-2.");
    // 只比小寫：`taken` 裡不是小寫的名稱不算占用（呼叫的一方負責轉成小寫）
    assert_eq!(free_name(&taken(&["A.pdf"]), "a.pdf"), "a.pdf");
}
