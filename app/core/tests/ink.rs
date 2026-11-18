//! 筆跡 `ink.json`：讀、新增一筆、刪除一筆、清除一頁；原子寫入、版本記號、衝突、
//! 讀不懂不覆寫、`check` 認得它、備份包含它。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{CheckLevel, ProblemKind, StrokeDraft};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    material: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, material) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    Fx {
        dir,
        lib,
        paper,
        material,
    }
}

impl Fx {
    fn ink_path(&self) -> PathBuf {
        self.dir
            .path()
            .join("papers")
            .join("Zhao Paper")
            .join("ink.json")
    }
    fn draft(&self, page: u32) -> StrokeDraft {
        StrokeDraft {
            material: self.material.clone(),
            page_index: page,
            color: "#d93025".into(),
            width: 1.5,
            opacity: 1.0,
            points: vec![[10.0, 20.0], [11.5, 22.25], [30.0, 40.0]],
        }
    }
    fn add(&mut self, page: u32) -> (String, VersionToken) {
        let token = self.token();
        let (s, t) = self
            .lib
            .add_stroke(&self.paper, self.draft(page), token.as_ref())
            .unwrap();
        (s.id, t)
    }
    fn token(&self) -> Option<VersionToken> {
        fs::read(self.ink_path())
            .ok()
            .map(|b| VersionToken(sha256_hex(&b)))
    }
    fn raw(&self) -> Value {
        serde_json::from_slice(&fs::read(self.ink_path()).unwrap()).unwrap()
    }
}

#[test]
fn read_without_file_is_empty_and_none() {
    let fx = setup();
    let (doc, token) = fx.lib.read_ink(&fx.paper).unwrap();
    assert!(doc.strokes.is_empty());
    assert!(token.is_none());
    assert!(!fx.ink_path().exists());
}

#[test]
fn read_unknown_paper_is_not_found() {
    let fx = setup();
    assert!(matches!(
        fx.lib.read_ink("01JCK3T1A2B3C4D5E6F7G8H9ZZ"),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn first_stroke_creates_file_with_page_coordinates() {
    let mut fx = setup();
    let (id, token) = fx.add(2);
    let v = fx.raw();
    assert_eq!(v["format"], 1);
    assert_eq!(v["modified_by"], DEVICE);
    assert!(v["modified"].as_str().unwrap().ends_with('Z'));
    let s = &v["strokes"][0];
    assert_eq!(s["id"], id.as_str());
    assert_eq!(s["material"], fx.material.as_str());
    assert_eq!(s["page"], 2);
    assert_eq!(s["color"], "#d93025");
    assert_eq!(s["width"], 1.5);
    assert!(s["created"].as_str().unwrap().ends_with('Z'));
    assert_eq!(s["points"][1][0], 11.5);
    assert_eq!(s["points"][1][1], 22.25);
    // 回傳的記號就是檔案現在的內容
    assert_eq!(token, fx.token().unwrap());
    // 沒有留下暫存檔
    let folder = fx.ink_path().parent().unwrap().to_path_buf();
    assert!(
        fs::read_dir(folder).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pr-"))
    );
}

#[test]
fn strokes_accumulate_and_read_back_in_order() {
    let mut fx = setup();
    let (a, _) = fx.add(0);
    let (b, _) = fx.add(0);
    let (c, _) = fx.add(3);
    let (doc, token) = fx.lib.read_ink(&fx.paper).unwrap();
    let ids: Vec<&str> = doc.strokes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, [a.as_str(), b.as_str(), c.as_str()]);
    assert_eq!(token, fx.token());
    assert_eq!(doc.strokes[2].page, 3);
    assert_eq!(doc.strokes[0].points.len(), 3);
}

#[test]
fn stale_token_is_conflict_and_file_untouched() {
    let mut fx = setup();
    let (_, stale) = fx.add(0);
    fx.add(0);
    let before = fs::read(fx.ink_path()).unwrap();
    let r = fx.lib.add_stroke(&fx.paper, fx.draft(0), Some(&stale));
    assert!(matches!(r, Err(LibError::Conflict)));
    assert_eq!(fs::read(fx.ink_path()).unwrap(), before);
}

#[test]
fn none_token_when_file_exists_is_conflict() {
    let mut fx = setup();
    fx.add(0);
    let r = fx.lib.add_stroke(&fx.paper, fx.draft(0), None);
    assert!(matches!(r, Err(LibError::Conflict)));
}

#[test]
fn token_when_file_missing_is_conflict() {
    let mut fx = setup();
    let r = fx
        .lib
        .add_stroke(&fx.paper, fx.draft(0), Some(&VersionToken("0".repeat(64))));
    assert!(matches!(r, Err(LibError::Conflict)));
    assert!(!fx.ink_path().exists());
}

#[test]
fn delete_one_stroke() {
    let mut fx = setup();
    let (a, _) = fx.add(0);
    let (b, t) = fx.add(1);
    fx.lib.delete_stroke(&fx.paper, &a, &t).unwrap();
    let (doc, _) = fx.lib.read_ink(&fx.paper).unwrap();
    assert_eq!(doc.strokes.len(), 1);
    assert_eq!(doc.strokes[0].id, b);
}

#[test]
fn delete_missing_stroke_is_not_found_and_does_not_write() {
    let mut fx = setup();
    let (_, t) = fx.add(0);
    let before = fs::read(fx.ink_path()).unwrap();
    let r = fx.lib.delete_stroke(&fx.paper, "s-nope", &t);
    assert!(matches!(r, Err(LibError::NotFound { .. })));
    assert_eq!(fs::read(fx.ink_path()).unwrap(), before);
}

#[test]
fn delete_without_file_is_not_found() {
    let mut fx = setup();
    let r = fx
        .lib
        .delete_stroke(&fx.paper, "s-x", &VersionToken("0".repeat(64)));
    assert!(matches!(r, Err(LibError::NotFound { .. })));
    assert!(!fx.ink_path().exists());
}

#[test]
fn delete_with_stale_token_is_conflict() {
    let mut fx = setup();
    let (a, stale) = fx.add(0);
    fx.add(0);
    let r = fx.lib.delete_stroke(&fx.paper, &a, &stale);
    assert!(matches!(r, Err(LibError::Conflict)));
    assert_eq!(fx.lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 2);
}

#[test]
fn clear_page_removes_only_that_page_of_that_material() {
    let mut fx = setup();
    fx.add(0);
    fx.add(1);
    fx.add(1);
    let (_, t) = {
        let (id, t) = fx.add(2);
        (id, t)
    };
    fx.lib
        .clear_ink_page(&fx.paper, &fx.material, 1, &t)
        .unwrap();
    let (doc, _) = fx.lib.read_ink(&fx.paper).unwrap();
    let pages: Vec<u32> = doc.strokes.iter().map(|s| s.page).collect();
    assert_eq!(pages, [0, 2]);
}

#[test]
fn clear_page_with_nothing_there_does_not_write() {
    let mut fx = setup();
    let (_, t) = fx.add(0);
    let before = fs::read(fx.ink_path()).unwrap();
    let t2 = fx
        .lib
        .clear_ink_page(&fx.paper, &fx.material, 5, &t)
        .unwrap();
    assert_eq!(t2, t);
    assert_eq!(fs::read(fx.ink_path()).unwrap(), before);
}

#[test]
fn draft_validation_writes_nothing() {
    let mut fx = setup();
    let bad: Vec<(&str, StrokeDraft)> = vec![
        (
            "unknown material",
            StrokeDraft {
                material: "01JCK3T1A2B3C4D5E6F7G8H9ZZ".into(),
                ..fx.draft(0)
            },
        ),
        (
            "page beyond pages",
            StrokeDraft {
                page_index: 10,
                ..fx.draft(0)
            },
        ),
        (
            "no points",
            StrokeDraft {
                points: vec![],
                ..fx.draft(0)
            },
        ),
        (
            "nan point",
            StrokeDraft {
                points: vec![[f64::NAN, 1.0]],
                ..fx.draft(0)
            },
        ),
        (
            "infinite point",
            StrokeDraft {
                points: vec![[1.0, f64::INFINITY]],
                ..fx.draft(0)
            },
        ),
        (
            "bad color",
            StrokeDraft {
                color: "red".into(),
                ..fx.draft(0)
            },
        ),
        (
            "zero width",
            StrokeDraft {
                width: 0.0,
                ..fx.draft(0)
            },
        ),
        (
            "huge width",
            StrokeDraft {
                width: 1000.0,
                ..fx.draft(0)
            },
        ),
        (
            "zero opacity",
            StrokeDraft {
                opacity: 0.0,
                ..fx.draft(0)
            },
        ),
        (
            "opacity over 1",
            StrokeDraft {
                opacity: 1.5,
                ..fx.draft(0)
            },
        ),
        (
            "too many points",
            StrokeDraft {
                points: vec![[1.0, 1.0]; 20_001],
                ..fx.draft(0)
            },
        ),
    ];
    for (name, d) in bad {
        let r = fx.lib.add_stroke(&fx.paper, d, None);
        if name == "unknown material" {
            assert!(matches!(r, Err(LibError::NotFound { .. })), "{name}: {r:?}");
        } else {
            assert!(
                matches!(r, Err(LibError::InvalidInput { .. })),
                "{name}: {r:?}"
            );
        }
        assert!(!fx.ink_path().exists(), "{name} wrote a file");
    }
}

#[test]
fn points_are_rounded_to_two_decimals() {
    let mut fx = setup();
    let d = StrokeDraft {
        points: vec![[1.23456, 9.87654]],
        ..fx.draft(0)
    };
    let (s, _) = fx.lib.add_stroke(&fx.paper, d, None).unwrap();
    assert_eq!(s.points, vec![[1.23, 9.88]]);
}

#[test]
fn coordinates_at_the_limit_are_accepted_and_file_stays_readable() {
    let mut fx = setup();
    let d = StrokeDraft {
        points: vec![[1_000_000.0, -1_000_000.0]],
        ..fx.draft(0)
    };
    fx.lib.add_stroke(&fx.paper, d, None).unwrap();
    assert_eq!(fx.lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 1);
}

#[test]
fn tiny_width_is_kept_at_least_one_hundredth() {
    let mut fx = setup();
    let d = StrokeDraft {
        width: 0.001,
        ..fx.draft(0)
    };
    let (s, _) = fx.lib.add_stroke(&fx.paper, d, None).unwrap();
    assert_eq!(s.width, 0.01);
    // 檔案裡也是 0.01，讀回來仍合規則（寬度大於 0）
    assert_eq!(fx.raw()["strokes"][0]["width"], 0.01);
}

#[test]
fn unknown_fields_survive_a_write() {
    let mut fx = setup();
    fx.add(0);
    let mut v = fx.raw();
    v["x_future"] = serde_json::json!({"a": 1});
    v["strokes"][0]["x_pressure"] = serde_json::json!([1, 2]);
    fs::write(fx.ink_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    fx.add(1);
    let after = fx.raw();
    assert_eq!(after["x_future"]["a"], 1);
    assert_eq!(after["strokes"][0]["x_pressure"][1], 2);
    assert_eq!(after["strokes"].as_array().unwrap().len(), 2);
}

#[test]
fn bom_file_is_readable() {
    let mut fx = setup();
    fx.add(0);
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(fs::read(fx.ink_path()).unwrap());
    fs::write(fx.ink_path(), bytes).unwrap();
    assert_eq!(fx.lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 1);
}

#[test]
fn unreadable_file_is_reported_and_never_overwritten() {
    let mut fx = setup();
    fs::write(fx.ink_path(), "{ not json").unwrap();
    let before = fs::read(fx.ink_path()).unwrap();
    assert!(matches!(
        fx.lib.read_ink(&fx.paper),
        Err(LibError::Unreadable { .. })
    ));
    let t = VersionToken(sha256_hex(&before));
    assert!(matches!(
        fx.lib.add_stroke(&fx.paper, fx.draft(0), Some(&t)),
        Err(LibError::Unreadable { .. })
    ));
    assert!(matches!(
        fx.lib.add_stroke(&fx.paper, fx.draft(0), None),
        Err(LibError::Conflict) | Err(LibError::Unreadable { .. })
    ));
    assert!(matches!(
        fx.lib.delete_stroke(&fx.paper, "s-x", &t),
        Err(LibError::Unreadable { .. })
    ));
    assert_eq!(fs::read(fx.ink_path()).unwrap(), before);
}

#[test]
fn newer_format_can_be_read_but_not_written() {
    let mut fx = setup();
    fx.add(0);
    let mut v = fx.raw();
    v["format"] = serde_json::json!(99);
    fs::write(fx.ink_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    assert_eq!(fx.lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 1);
    let before = fs::read(fx.ink_path()).unwrap();
    let t = fx.token();
    assert!(matches!(
        fx.lib.add_stroke(&fx.paper, fx.draft(0), t.as_ref()),
        Err(LibError::FormatTooNew { found: 99 })
    ));
    assert_eq!(fs::read(fx.ink_path()).unwrap(), before);
}

#[test]
fn read_only_library_refuses_writes_but_reads() {
    let mut fx = setup();
    fx.add(0);
    let lib_json = fx.dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lib_json).unwrap()).unwrap();
    v["format"] = serde_json::json!(99);
    fs::write(&lib_json, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = LocalLibrary::open(fx.dir.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    assert_eq!(lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 1);
    let t = fx.token();
    assert!(matches!(
        lib.add_stroke(&fx.paper, fx.draft(0), t.as_ref()),
        Err(LibError::ReadOnly)
    ));
}

#[test]
fn check_accepts_ink_file_and_flags_unreadable_one() {
    let mut fx = setup();
    fx.add(0);
    let ps = fx.lib.check(CheckLevel::Quick).unwrap();
    assert!(ps.is_empty(), "{ps:?}");
    fs::write(fx.ink_path(), "{ not json").unwrap();
    let ps = fx.lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert_eq!(ps[0].kind, ProblemKind::UnreadableInkFile);
    assert_eq!(ps[0].path, "papers/Zhao Paper/ink.json");
    assert_eq!(ps[0].paper.as_deref(), Some(fx.paper.as_str()));
}

#[test]
fn ink_survives_trash_and_restore() {
    let mut fx = setup();
    fx.add(0);
    fx.lib.remove_paper(&fx.paper).unwrap();
    fx.lib.restore_paper(&fx.paper).unwrap();
    assert_eq!(fx.lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 1);
}

#[test]
fn backup_contains_ink_file_and_restores_it() {
    let mut fx = setup();
    fx.add(0);
    fx.add(1);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("b.zip");
    fx.lib.export_backup(&out, &mut |_, _| {}).unwrap();
    let target = out_dir.path().join("restored");
    let report = paperreader_core::backup::restore_backup(&out, &target, DEVICE).unwrap();
    assert!(report.manifest_mismatches.is_empty());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let lib = LocalLibrary::open(&target, DEVICE).unwrap();
    assert_eq!(lib.read_ink(&fx.paper).unwrap().0.strokes.len(), 2);
}
