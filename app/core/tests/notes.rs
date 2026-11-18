//! 工作 6：筆記檔的讀寫。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::notes::{NotesLabels, Quote};
use paperreader_core::store::HighlightDraft;
use serde_json::{Value, json};
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
    fn notes_dir(&self) -> PathBuf {
        self.dir.path().join("notes")
    }
    fn notes_file(&self) -> PathBuf {
        self.notes_dir().join("Zhao Paper.md")
    }
    fn draft(&self, page: u32, quote: &str, comment: &str) -> HighlightDraft {
        HighlightDraft {
            material: self.material.clone(),
            page_index: page,
            page_label: Some((page + 1).to_string()),
            rects: vec![[10.0, 20.0, 30.0, 40.0]],
            next_page_rects: vec![],
            position: None,
            quote: Quote {
                exact: quote.into(),
                prefix: None,
                suffix: None,
                extra: Default::default(),
            },
            fingerprint: Some("fp".into()),
            comment: comment.into(),
        }
    }
    fn add(&mut self, page: u32, quote: &str, comment: &str) -> (String, VersionToken) {
        let d = self.draft(page, quote, comment);
        let (h, t) = self.lib.add_highlight(&self.paper, d, None).unwrap();
        (h.id, t)
    }
    fn file_token(&self) -> VersionToken {
        VersionToken(sha256_hex(&fs::read(self.notes_file()).unwrap()))
    }
}

fn set_notes_dir_at(root: &std::path::Path, value: &str) {
    let p = root.join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    v["notes_dir"] = json!(value);
    fs::write(&p, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

#[test]
fn read_without_file_is_empty_and_none() {
    let fx = setup();
    let (doc, token) = fx.lib.read_notes(&fx.paper).unwrap();
    assert!(token.is_none());
    assert_eq!(doc.path, None);
    assert_eq!(doc.modified, None);
    assert_eq!(doc.thoughts, "");
    assert!(doc.thoughts_complete);
    assert!(doc.highlights.is_empty());
    assert!(doc.unreadable.is_empty());
    assert!(!fx.notes_dir().exists());
}

#[test]
fn first_highlight_creates_file() {
    let mut fx = setup();
    let (id, token) = fx.add(2, "some quote", "my comment");
    assert!(fx.notes_file().is_file());
    let text = fs::read_to_string(fx.notes_file()).unwrap();
    assert!(text.starts_with("---\n"));
    assert!(text.contains(&format!("paperreader_paper: {}\n", fx.paper)));
    assert!(text.contains(&format!("paperreader_modified_by: {DEVICE}\n")));
    assert_eq!(token, fx.file_token());
    let (doc, t2) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(t2, Some(token));
    assert_eq!(doc.path.as_deref(), Some("notes/Zhao Paper.md"));
    assert_eq!(doc.modified_by.as_deref(), Some(DEVICE));
    assert_eq!(doc.highlights.len(), 1);
    let h = &doc.highlights[0];
    assert_eq!(h.id, id);
    assert!(h.id.starts_with("h-") && h.id.len() == 28);
    assert!(
        h.id[2..]
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
    );
    let (paper, _) = fx.lib.get_paper(&fx.paper).unwrap();
    assert_eq!(h.file.sha256, paper.materials[0].sha256);
    assert_eq!(h.file.fingerprint.as_deref(), Some("fp"));
    assert_eq!(h.comment, "my comment");
    assert_eq!(h.quote.exact, "some quote");
    assert_eq!(h.kind, "highlight");
    assert_eq!(h.color, None);
    assert_eq!(h.created, h.modified);
    assert!(h.complete);
    // 新建的檔案裡有篇名
    assert!(text.contains("\n# Zhao Paper\n"));
}

#[test]
fn labels_are_used_for_new_text() {
    let mut fx = setup();
    fx.lib
        .set_notes_labels(NotesLabels {
            thoughts: "整篇的想法".into(),
            highlights: "劃線".into(),
            back_link: "回到原文".into(),
        })
        .unwrap();
    fx.add(0, "q", "");
    let text = fs::read_to_string(fx.notes_file()).unwrap();
    assert!(text.contains("## 整篇的想法\n"));
    assert!(text.contains("## 劃線\n"));
    assert!(text.contains("[回到原文 p. 1]("));
    let bad = NotesLabels {
        thoughts: "a\nb".into(),
        highlights: "x".into(),
        back_link: "y".into(),
    };
    let r = fx.lib.set_notes_labels(bad);
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
}

#[test]
fn name_collision_gets_suffix() {
    let mut fx = setup();
    fs::create_dir_all(fx.notes_dir()).unwrap();
    let mine = "我自己的筆記，沒有屬性區。\n";
    fs::write(fx.notes_file(), mine).unwrap();
    fx.add(0, "q", "");
    assert_eq!(fs::read_to_string(fx.notes_file()).unwrap(), mine);
    let second = fx.notes_dir().join("Zhao Paper (2).md");
    assert!(second.is_file());
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.path.as_deref(), Some("notes/Zhao Paper (2).md"));
}

#[test]
fn file_found_by_front_matter_not_name() {
    let mut fx = setup();
    fx.add(0, "q", "c");
    let renamed = fx.notes_dir().join("whatever.md");
    fs::rename(fx.notes_file(), &renamed).unwrap();
    // 重新掃描之前
    let (doc, t) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.path.as_deref(), Some("notes/whatever.md"));
    assert_eq!(doc.highlights.len(), 1);
    assert!(t.is_some());
    // 重新掃描之後
    let again = fx.notes_dir().join("again.md");
    fs::rename(&renamed, &again).unwrap();
    fx.lib.rescan().unwrap();
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.path.as_deref(), Some("notes/again.md"));
}

#[test]
fn externally_added_notes_file_is_found_without_rescan() {
    let mut fx = setup();
    fs::create_dir_all(fx.notes_dir()).unwrap();
    let text = paperreader_core::notes::new_file(
        &fx.paper,
        "Zhao Paper",
        &NotesLabels::default(),
        &paperreader_core::notes::Stamp {
            time: "2026-11-16T09:00:00.000Z".into(),
            device: DEVICE.into(),
        },
    );
    fs::write(fx.notes_dir().join("synced.md"), &text).unwrap();
    let (doc, t) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.path.as_deref(), Some("notes/synced.md"));
    assert!(t.is_some());
    // 帶 None 寫入：檔案已存在，一定是 Conflict，而且不另建 ` (2).md`
    let d = fx.draft(0, "q", "");
    let r = fx.lib.add_highlight(&fx.paper, d, None);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let names: Vec<_> = fs::read_dir(fx.notes_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["synced.md"]);
    assert_eq!(
        fs::read_to_string(fx.notes_dir().join("synced.md")).unwrap(),
        text
    );
}

#[test]
fn stale_token_is_conflict() {
    let mut fx = setup();
    let (id, token) = fx.add(0, "q", "c");
    let mut text = fs::read_to_string(fx.notes_file()).unwrap();
    text.push_str("\n外部加的一行\n");
    fs::write(fx.notes_file(), &text).unwrap();
    let d = fx.draft(1, "q2", "");
    let r = fx.lib.add_highlight(&fx.paper, d, Some(&token));
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let r = fx.lib.set_highlight_comment(&fx.paper, &id, "x", &token);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let r = fx.lib.delete_highlight(&fx.paper, &id, &token);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    let r = fx.lib.set_thoughts(&fx.paper, "t", Some(&token));
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fs::read_to_string(fx.notes_file()).unwrap(), text);
}

#[test]
fn none_token_when_file_exists_is_conflict() {
    let mut fx = setup();
    fx.add(0, "q", "");
    let before = fs::read(fx.notes_file()).unwrap();
    let r = fx.lib.set_thoughts(&fx.paper, "t", None);
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    // 帶 Some 而檔案不存在：同樣是 Conflict
    let other = {
        let (_, mut lib) = temp_library();
        let (p, _) = add_paper(&mut lib, 2, "Other", None);
        (lib, p)
    };
    let (mut lib2, p2) = other;
    let r = lib2.set_thoughts(&p2, "t", Some(&VersionToken("0".repeat(64))));
    assert!(matches!(r, Err(LibError::Conflict)), "{r:?}");
    assert_eq!(fs::read(fx.notes_file()).unwrap(), before);
}

#[test]
fn comment_edit_delete_and_thoughts() {
    let mut fx = setup();
    let (a, t) = fx.add(0, "qa", "ca");
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    let m0 = doc.modified.clone().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let d = fx.draft(1, "qb", "cb");
    let (hb, t) = fx.lib.add_highlight(&fx.paper, d, Some(&t)).unwrap();
    let t = fx
        .lib
        .set_highlight_comment(&fx.paper, &a, "ca2", &t)
        .unwrap();
    let t = fx
        .lib
        .set_thoughts(&fx.paper, "整篇想法\n兩行", Some(&t))
        .unwrap();
    let t = fx.lib.delete_highlight(&fx.paper, &hb.id, &t).unwrap();
    let (doc, t2) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(t2, Some(t));
    assert_eq!(doc.thoughts, "整篇想法\n兩行");
    assert!(doc.thoughts_complete);
    assert_eq!(doc.highlights.len(), 1);
    assert_eq!(doc.highlights[0].comment, "ca2");
    assert!(doc.highlights[0].modified > doc.highlights[0].created);
    assert!(doc.modified.unwrap() > m0);
}

#[test]
fn draft_validation() {
    let mut fx = setup();
    let ok = fx.draft(0, "q", "");
    let mut cases: Vec<(&str, HighlightDraft)> = Vec::new();
    let mut d = ok.clone();
    d.rects = vec![];
    cases.push(("empty rects", d));
    let mut d = ok.clone();
    d.rects = vec![[30.0, 20.0, 10.0, 40.0]];
    cases.push(("left > right", d));
    let mut d = ok.clone();
    d.rects = vec![[10.0, f64::NAN, 30.0, 40.0]];
    cases.push(("nan", d));
    let mut d = ok.clone();
    d.page_index = 10; // 材料只有 10 頁（頁序 0 到 9）
    cases.push(("page beyond pages", d));
    let mut d = ok.clone();
    d.quote.exact = String::new();
    cases.push(("empty quote", d));
    let mut d = ok.clone();
    d.quote.exact = "a\0b".into();
    cases.push(("nul in quote", d));
    let mut d = ok.clone();
    d.position = Some(paperreader_core::notes::TextPosition {
        start: 1,
        end: 2,
        basis: "x".repeat(65),
        extra: Default::default(),
    });
    cases.push(("basis too long", d));
    let mut d = ok.clone();
    d.position = Some(paperreader_core::notes::TextPosition {
        start: 5,
        end: 2,
        basis: "ok".into(),
        extra: Default::default(),
    });
    cases.push(("start > end", d));
    for (name, d) in cases {
        let r = fx.lib.add_highlight(&fx.paper, d, None);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{name}: {r:?}"
        );
    }
    let mut d = ok.clone();
    d.material = "01JCKA1B2C3D4E5F6G7H8J9K0M".into();
    let r = fx.lib.add_highlight(&fx.paper, d, None);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert!(!fx.notes_dir().exists(), "nothing may be written");
    // 頁碼標籤不合規則：當成 null、不擋
    let mut d = ok.clone();
    d.page_label = Some("a*b".into());
    let (h, _) = fx.lib.add_highlight(&fx.paper, d, None).unwrap();
    assert_eq!(h.page.label, None);
}

#[test]
fn paper_in_trash_is_not_found_and_notes_survive_restore() {
    let mut fx = setup();
    let (id, t) = fx.add(0, "q", "keep me");
    fx.lib.remove_paper(&fx.paper.clone()).unwrap();
    let r = fx.lib.read_notes(&fx.paper);
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let d = fx.draft(1, "q", "");
    let r = fx.lib.add_highlight(&fx.paper, d, Some(&t));
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    let r = fx.lib.set_thoughts(&fx.paper, "x", Some(&t));
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
    assert!(fx.notes_file().is_file());
    fx.lib.restore_paper(&fx.paper.clone()).unwrap();
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.highlights.len(), 1);
    assert_eq!(doc.highlights[0].id, id);
}

#[test]
fn newer_format_paper_can_have_notes() {
    let mut fx = setup();
    let pj = fx.dir.path().join("papers/Zhao Paper/paper.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&pj).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    fx.lib.rescan().unwrap();
    let before = fs::read(&pj).unwrap();
    fx.add(0, "q", "");
    assert_eq!(fs::read(&pj).unwrap(), before);
    assert_eq!(fx.lib.read_notes(&fx.paper).unwrap().0.highlights.len(), 1);
}

#[test]
fn read_only_refuses_writes_but_reads() {
    let mut fx = setup();
    let (id, t) = fx.add(0, "q", "c");
    let Fx {
        dir,
        paper,
        material,
        ..
    } = fx;
    let lp = dir.path().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lp).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&lp, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert_eq!(lib.read_notes(&paper).unwrap().0.highlights.len(), 1);
    let d = HighlightDraft {
        material,
        page_index: 0,
        page_label: None,
        rects: vec![[1.0, 1.0, 2.0, 2.0]],
        next_page_rects: vec![],
        position: None,
        quote: Quote {
            exact: "q".into(),
            prefix: None,
            suffix: None,
            extra: Default::default(),
        },
        fingerprint: None,
        comment: String::new(),
    };
    let r = lib.add_highlight(&paper, d, Some(&t));
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.set_highlight_comment(&paper, &id, "x", &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.delete_highlight(&paper, &id, &t);
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
    let r = lib.set_thoughts(&paper, "x", Some(&t));
    assert!(matches!(r, Err(LibError::ReadOnly)), "{r:?}");
}

#[test]
fn non_utf8_file_is_unreadable_and_untouched() {
    let mut fx = setup();
    fs::create_dir_all(fx.notes_dir()).unwrap();
    let mut bytes = format!("---\npaperreader_paper: {}\n---\n\n", fx.paper).into_bytes();
    bytes.extend_from_slice(&[0xFF, 0xFE, 0xFA, b'\n']);
    fs::write(fx.notes_dir().join("bad.md"), &bytes).unwrap();
    let r = fx.lib.read_notes(&fx.paper);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    let token = VersionToken(sha256_hex(&bytes));
    let d = fx.draft(0, "q", "");
    let r = fx.lib.add_highlight(&fx.paper, d, Some(&token));
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    let r = fx.lib.set_thoughts(&fx.paper, "x", Some(&token));
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
    assert_eq!(fs::read(fx.notes_dir().join("bad.md")).unwrap(), bytes);
}

#[test]
fn bad_entries_in_notes_dir_do_not_block_open() {
    let (dir, mut lib) = temp_library();
    let (p1, m1) = add_paper(&mut lib, 1, "One", None);
    let (p2, _) = add_paper(&mut lib, 2, "Two", None);
    drop(lib);
    let notes = dir.path().join("notes");
    fs::create_dir_all(notes.join("x.md")).unwrap();
    fs::write(notes.join("garbage.md"), [0xFFu8, 0xFE, 0x00, 0x80]).unwrap();
    let mut lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    lib.rescan().unwrap();
    // 別篇的筆記照常讀寫
    let d = HighlightDraft {
        material: m1,
        page_index: 0,
        page_label: None,
        rects: vec![[1.0, 1.0, 2.0, 2.0]],
        next_page_rects: vec![],
        position: None,
        quote: Quote {
            exact: "q".into(),
            prefix: None,
            suffix: None,
            extra: Default::default(),
        },
        fingerprint: None,
        comment: String::new(),
    };
    lib.add_highlight(&p1, d, None).unwrap();
    assert_eq!(lib.read_notes(&p1).unwrap().0.highlights.len(), 1);
    assert!(lib.read_notes(&p2).unwrap().1.is_none());
}

#[test]
fn open_cleans_temp_in_notes_dir() {
    let (dir, lib) = temp_library();
    drop(lib);
    let notes = dir.path().join("notes");
    fs::create_dir_all(notes.join(".obsidian")).unwrap();
    fs::write(notes.join(".obsidian/app.json"), "{}").unwrap();
    fs::write(notes.join(".pr-tmp-X"), "x").unwrap();
    fs::write(notes.join("mine.md"), "mine").unwrap();
    let _lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    assert!(!notes.join(".pr-tmp-X").exists());
    assert!(notes.join(".obsidian/app.json").is_file());
    assert!(notes.join("mine.md").is_file());
}

#[test]
fn invalid_notes_dir_is_refused_and_not_touched() {
    for bad in [
        "../x", ".", "papers", ".hidden", "logs", "a//b", "/abs", "a/../b",
    ] {
        // 文獻庫放在這個測試自己的暫存資料夾裡的 `lib/`，「文獻庫外面」就是同一個暫存資料夾裡的 `x/`
        // （`../x` 指到的地方）。不碰系統暫存資料夾的上一層，和別的測試平行執行時不會互撞。
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("lib");
        let mut lib = LocalLibrary::open(&root, DEVICE).unwrap();
        let (p, m) = add_paper(&mut lib, 1, "Paper", None);
        drop(lib);
        set_notes_dir_at(&root, bad);
        // 文獻庫外面事先放一個「暫存檔」：開啟時不可以被清掉
        let outside = outer.path().join("x");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join(".pr-y"), "keep").unwrap();
        let before_root: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();

        let mut lib = LocalLibrary::open(&root, DEVICE).unwrap();
        assert!(outside.join(".pr-y").is_file(), "{bad}");
        assert_eq!(lib.list_papers().len(), 1);
        let r = lib.read_notes(&p);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
        let d = HighlightDraft {
            material: m,
            page_index: 0,
            page_label: None,
            rects: vec![[1.0, 1.0, 2.0, 2.0]],
            next_page_rects: vec![],
            position: None,
            quote: Quote {
                exact: "q".into(),
                prefix: None,
                suffix: None,
                extra: Default::default(),
            },
            fingerprint: None,
            comment: String::new(),
        };
        let r = lib.add_highlight(&p, d, None);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "{bad}: {r:?}"
        );
        let after_root: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let mut a = after_root.clone();
        let mut b = before_root.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b, "{bad}");
    }
}
