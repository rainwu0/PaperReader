//! 新材料挑好檔名之後、放進去之前，別處剛好放了同名的檔：回 `Conflict`，不蓋掉那個檔，清單與資料夾都不變
//! （建立疊加檔也要有這個檢查）。
//! 四個加材料的入口都經過同一個檢查，以測試掛勾在挑好檔名的那一刻放一個同名的檔來模擬。

mod common;

use common::*;
use paperreader_core::digest::{DigestDraft, DigestSection, PointDraft};
use paperreader_core::error::LibError;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const THEIRS: &[u8] = b"someone else's file";

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    source: String,
    _src: tempfile::TempDir,
    src: PathBuf,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Paper", Some(10));
    let src = tempfile::tempdir().unwrap();
    let path = src.path().to_path_buf();
    Fx {
        dir,
        lib,
        paper,
        source,
        _src: src,
        src: path,
    }
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

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers/Paper")
    }
    fn token(&self) -> VersionToken {
        self.lib.get_paper(&self.paper).unwrap().1
    }
    /// 下一次挑好檔名時，在那個位置放一個別人的檔；交回放的位置。
    fn plant_on_next_pick(&mut self) -> Arc<Mutex<Option<PathBuf>>> {
        let seen = Arc::new(Mutex::new(None));
        let s = seen.clone();
        self.lib
            .set_material_name_hook(Some(Box::new(move |p: &Path| {
                fs::write(p, THEIRS).unwrap();
                *s.lock().unwrap() = Some(p.to_path_buf());
            })));
        seen
    }
    /// 呼叫之後：回 `Conflict`、那個檔原封不動、`paper.json` 不變、資料夾裡沒有暫存檔。
    fn assert_refused<T: std::fmt::Debug>(
        &self,
        r: Result<T, LibError>,
        planted: &Arc<Mutex<Option<PathBuf>>>,
        paper_json: &[u8],
    ) {
        assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
        let p = planted.lock().unwrap().clone().expect("the hook ran");
        assert_eq!(fs::read(&p).unwrap(), THEIRS);
        assert_eq!(
            fs::read(self.folder().join("paper.json")).unwrap(),
            paper_json
        );
        let leftovers: Vec<String> = fs::read_dir(self.folder())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".pr-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}

#[test]
fn create_overlay_does_not_overwrite_a_file_placed_after_picking_the_name() {
    let mut fx = setup();
    let before = fs::read(fx.folder().join("paper.json")).unwrap();
    let planted = fx.plant_on_next_pick();
    let t = fx.token();
    let (paper, source) = (fx.paper.clone(), fx.source.clone());
    let r = fx.lib.create_overlay(&paper, overlay_draft(&source), &t);
    fx.assert_refused(r, &planted, &before);
}

#[test]
fn add_translation_does_not_overwrite_a_file_placed_after_picking_the_name() {
    let mut fx = setup();
    let pdf = fake_pdf_seeded(&fx.src, "t.pdf", 400, 7);
    let before = fs::read(fx.folder().join("paper.json")).unwrap();
    let planted = fx.plant_on_next_pick();
    let t = fx.token();
    let paper = fx.paper.clone();
    let r = fx.lib.add_translation(&paper, &pdf, Some(10), &t);
    fx.assert_refused(r, &planted, &before);
}

#[test]
fn put_generated_translation_does_not_overwrite_a_file_placed_after_picking_the_name() {
    let mut fx = setup();
    let t = fx.token();
    let (paper, source) = (fx.paper.clone(), fx.source.clone());
    let overlay = fx
        .lib
        .create_overlay(&paper, overlay_draft(&source), &t)
        .unwrap()
        .material_id;
    let before = fs::read(fx.folder().join("paper.json")).unwrap();
    let planted = fx.plant_on_next_pick();
    let t = fx.token();
    let r = fx.lib.put_generated_translation(
        &paper,
        &overlay,
        &mut Cursor::new(fake_bytes(9, 900)),
        Some(10),
        &t,
    );
    fx.assert_refused(r, &planted, &before);
}

#[test]
fn create_digest_does_not_overwrite_a_file_placed_after_picking_the_name() {
    let mut fx = setup();
    let before = fs::read(fx.folder().join("paper.json")).unwrap();
    let planted = fx.plant_on_next_pick();
    let t = fx.token();
    let (paper, source) = (fx.paper.clone(), fx.source.clone());
    let draft = DigestDraft {
        source_material: source,
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
    };
    let r = fx.lib.create_digest(&paper, draft, &t);
    fx.assert_refused(r, &planted, &before);
}
