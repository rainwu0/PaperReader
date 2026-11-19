//! 由畫面發起、帶著 `paper.json` 版本記號的八個寫入入口，寫入前檢查的先後：
//! 唯讀 → 不在目錄 → 記號不符 → 格式較新 → 各入口自己的檢查（頁數、疊加檔、來源材料、群組、要移除的材料…）。
//! 兩個條件同時成立時，先查的那一個算數，而且什麼都不寫。
//!
//! 每個入口自己的檢查都讓它不過（`Fx::own_checks_fail`），再疊上前面的條件，才測得出先後；
//! `own_checks_fail_on_their_own` 確認那些值單獨成立時真的會擋下。改書目沒有自己的檢查。

mod common;

use common::*;
use paperreader_core::digest::{DigestDraft, DigestSection, PointDraft};
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use paperreader_core::store::AddMaterialOutcome;
use serde_json::Value;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

/// 不存在的代號（文獻、材料、群組都用它）。
const NO_SUCH: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    group: String,
    pdf: PathBuf,
    _src: tempfile::TempDir,
}

/// 各入口自己的檢查要用的值。
struct Own {
    /// 加入譯本、存產生的譯本的頁數。
    pages: Option<u32>,
    /// 存產生的譯本：由哪一份疊加檔產生。
    overlay: String,
    /// 建立疊加檔、建立重點筆記：來源材料。
    source: String,
    /// 加入群組。
    add_group: String,
    /// 移出群組（不在群組裡時不寫，交回原本的記號）。
    remove_group: String,
    /// 移除材料。
    material: String,
}

fn overlay_draft(source: &str) -> OverlayDraft {
    OverlayDraft {
        layout_model_pages: Vec::new(),
        source_material: source.into(),
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
    }
}

fn digest_draft(source: &str) -> DigestDraft {
    DigestDraft {
        source_material: source.into(),
        service: "Claude（Anthropic）".into(),
        provider: "anthropic".into(),
        model: "claude-test".into(),
        lang: "zh-TW".into(),
        title: "Paper".into(),
        sections: vec![DigestSection {
            id: "question".into(),
            label: "研究問題".into(),
        }],
        points: vec![PointDraft {
            section: "question".into(),
            text: "一句話。".into(),
            pages: vec![0],
            segments: vec!["g0001".into()],
        }],
    }
}

/// 一篇（10 頁）有原文、一份疊加檔、一份加入的譯本 PDF；另有一個群組（那篇不在裡面）。
fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Paper", Some(10));
    let gt = lib.list_groups().unwrap().1;
    let group = lib.create_group("G", &gt).unwrap().0.id;
    let t = lib.get_paper(&paper).unwrap().1;
    lib.create_overlay(&paper, overlay_draft(&source), &t)
        .unwrap();
    let src = tempfile::tempdir().unwrap();
    let first = fake_pdf_seeded(src.path(), "t1.pdf", 400, 7);
    let t = lib.get_paper(&paper).unwrap().1;
    match lib.add_translation(&paper, &first, Some(10), &t).unwrap() {
        AddMaterialOutcome::Added { .. } => {}
        other => panic!("{other:?}"),
    }
    let pdf = fake_pdf_seeded(src.path(), "t2.pdf", 400, 8);
    Fx {
        dir,
        lib,
        paper,
        group,
        pdf,
        _src: src,
    }
}

impl Fx {
    fn token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    fn paper_json(&self) -> PathBuf {
        self.dir.path().join("papers/Paper/paper.json")
    }
    /// 把那篇的 `format` 改成 2（比這一版新）。
    fn make_paper_newer(&mut self) {
        let mut v: Value = serde_json::from_slice(&fs::read(self.paper_json()).unwrap()).unwrap();
        v["format"] = serde_json::json!(2);
        fs::write(self.paper_json(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        self.lib.rescan().unwrap();
    }
    /// 把整個文獻庫的 `format` 改成 2（唯讀開啟）。
    fn make_library_read_only(&mut self) {
        let lp = self.dir.path().join("library.json");
        let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
        v["format"] = serde_json::json!(2);
        fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        self.lib = LocalLibrary::open(self.dir.path(), DEVICE).unwrap();
        assert!(self.lib.is_read_only());
    }

    /// 讓每個入口自己的檢查都不過：頁數不合、疊加檔不存在、來源材料不存在、群組不存在、要移除的材料不存在；
    /// 移出一個那篇本來就不在的群組（走到那一步就不寫、交回原本的記號）。
    fn own_checks_fail(&self) -> Own {
        Own {
            pages: Some(0),
            overlay: NO_SUCH.into(),
            source: NO_SUCH.into(),
            add_group: NO_SUCH.into(),
            remove_group: self.group.clone(),
            material: NO_SUCH.into(),
        }
    }

    /// 八個入口，都帶 `token` 呼叫 `paper` 這篇。改書目放最後：它沒有自己的檢查，走到那一步就會寫。
    fn call_all(
        &mut self,
        paper: &str,
        token: &VersionToken,
        own: &Own,
    ) -> Vec<(&'static str, Result<(), LibError>)> {
        let lib = &mut self.lib;
        vec![
            (
                "add_translation",
                lib.add_translation(paper, &self.pdf, own.pages, token)
                    .map(|_| ()),
            ),
            (
                "put_generated_translation",
                lib.put_generated_translation(
                    paper,
                    &own.overlay,
                    &mut Cursor::new(fake_bytes(9, 900)),
                    own.pages,
                    token,
                )
                .map(|_| ()),
            ),
            (
                "create_overlay",
                lib.create_overlay(paper, overlay_draft(&own.source), token)
                    .map(|_| ()),
            ),
            (
                "create_digest",
                lib.create_digest(paper, digest_draft(&own.source), token)
                    .map(|_| ()),
            ),
            (
                "add_to_group",
                lib.add_to_group(paper, &own.add_group, token).map(|_| ()),
            ),
            (
                "remove_from_group",
                lib.remove_from_group(paper, &own.remove_group, token)
                    .map(|_| ()),
            ),
            (
                "remove_material",
                lib.remove_material(paper, &own.material, token).map(|_| ()),
            ),
            (
                "update_bib",
                lib.update_bib(
                    paper,
                    BibDraft {
                        title: Some("Changed".into()),
                        ..Default::default()
                    },
                    token,
                )
                .map(|_| ()),
            ),
        ]
    }
}

fn stale() -> VersionToken {
    VersionToken("0".repeat(64))
}

fn assert_all(results: &[(&str, Result<(), LibError>)], want: fn(&LibError) -> bool, what: &str) {
    assert_eq!(results.len(), 8);
    for (name, r) in results {
        match r {
            Err(e) if want(e) => {}
            other => panic!("{name}: expected {what}, got {other:?}"),
        }
    }
}

#[test]
fn own_checks_fail_on_their_own() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    let t = fx.token();
    let own = fx.own_checks_fail();
    let r = fx.call_all(&paper, &t, &own);
    let kind =
        |name: &str| -> &Result<(), LibError> { &r.iter().find(|(n, _)| *n == name).unwrap().1 };
    let invalid = |e: &Result<(), LibError>| matches!(e, Err(LibError::InvalidInput { .. }));
    let not_found = |e: &Result<(), LibError>| matches!(e, Err(LibError::NotFound { .. }));
    assert!(
        invalid(kind("add_translation")),
        "{:?}",
        kind("add_translation")
    );
    // 存產生的譯本：頁數不合先於疊加檔不存在
    assert!(
        invalid(kind("put_generated_translation")),
        "{:?}",
        kind("put_generated_translation")
    );
    assert!(
        not_found(kind("create_overlay")),
        "{:?}",
        kind("create_overlay")
    );
    assert!(
        not_found(kind("create_digest")),
        "{:?}",
        kind("create_digest")
    );
    assert!(
        not_found(kind("add_to_group")),
        "{:?}",
        kind("add_to_group")
    );
    assert!(
        not_found(kind("remove_material")),
        "{:?}",
        kind("remove_material")
    );
    // 移出一個本來就不在的群組：不寫、交回原本的記號；改書目沒有自己的檢查（照常寫，這裡不看）
    assert!(
        kind("remove_from_group").is_ok(),
        "{:?}",
        kind("remove_from_group")
    );
    let after = snapshot(fx.dir.path());
    let changed: Vec<&String> = after
        .keys()
        .filter(|k| after.get(*k) != before.get(*k))
        .collect();
    // 只有改書目寫了 paper.json
    assert_eq!(changed, ["papers/Paper/paper.json"], "{changed:?}");
}

#[test]
fn generated_translation_checks_pages_before_the_overlay() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    let t = fx.token();
    let r = fx.lib.put_generated_translation(
        &paper,
        NO_SUCH,
        &mut Cursor::new(fake_bytes(9, 900)),
        Some(0),
        &t,
    );
    assert!(
        matches!(&r, Err(LibError::InvalidInput { reason }) if reason.starts_with("pages must be between")),
        "{r:?}"
    );
    // 頁數合理時才輪到疊加檔不存在
    let r = fx.lib.put_generated_translation(
        &paper,
        NO_SUCH,
        &mut Cursor::new(fake_bytes(9, 900)),
        Some(10),
        &t,
    );
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn read_only_comes_before_everything_else() {
    let mut fx = setup();
    fx.make_library_read_only();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    let own = fx.own_checks_fail();
    // 記號過期、各入口自己的檢查不過：仍是唯讀
    let r = fx.call_all(&paper, &stale(), &own);
    assert_all(&r, |e| matches!(e, LibError::ReadOnly), "ReadOnly");
    // 不在目錄的文獻：仍是唯讀
    let r = fx.call_all(NO_SUCH, &stale(), &own);
    assert_all(&r, |e| matches!(e, LibError::ReadOnly), "ReadOnly");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn an_unknown_paper_is_not_found_and_writes_nothing() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let own = fx.own_checks_fail();
    let r = fx.call_all(NO_SUCH, &stale(), &own);
    assert_all(&r, |e| matches!(e, LibError::NotFound { .. }), "NotFound");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn a_stale_token_comes_before_a_newer_format_and_the_entrys_own_checks() {
    let mut fx = setup();
    let old = fx.token();
    fx.make_paper_newer();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    let own = fx.own_checks_fail();
    let r = fx.call_all(&paper, &old, &own);
    assert_all(&r, |e| matches!(e, LibError::Conflict), "Conflict");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn a_newer_format_comes_before_the_entrys_own_checks() {
    let mut fx = setup();
    fx.make_paper_newer();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    let t = fx.token();
    let own = fx.own_checks_fail();
    let r = fx.call_all(&paper, &t, &own);
    assert_all(
        &r,
        |e| matches!(e, LibError::FormatTooNew { found: 2 }),
        "FormatTooNew",
    );
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn bad_pages_are_refused_and_write_nothing() {
    let mut fx = setup();
    let before = snapshot(fx.dir.path());
    let paper = fx.paper.clone();
    for pages in [0, 100_001] {
        let t = fx.token();
        let own = Own {
            pages: Some(pages),
            ..fx.own_checks_fail()
        };
        let r = fx.lib.add_translation(&paper, &fx.pdf, own.pages, &t);
        assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
        let r = fx.lib.put_generated_translation(
            &paper,
            &own.overlay,
            &mut Cursor::new(fake_bytes(9, 900)),
            own.pages,
            &t,
        );
        assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    }
    assert_eq!(snapshot(fx.dir.path()), before);
}
