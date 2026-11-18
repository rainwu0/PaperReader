//! 「列出文獻」的每一篇帶著 `has_notes`（首頁的「有筆記」篩選用）。

mod common;

use common::*;
use paperreader_core::local::LocalLibrary;
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    material: String,
    other: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, material) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    let (other, _) = add_paper(&mut lib, 2, "Other Paper", Some(10));
    Fx {
        dir,
        lib,
        paper,
        material,
        other,
    }
}

impl Fx {
    fn has_notes(&self, id: &str) -> bool {
        self.lib
            .list_papers()
            .into_iter()
            .find(|p| p.id == id)
            .expect("在目錄裡")
            .has_notes
    }
    fn notes_file(&self) -> PathBuf {
        self.dir.path().join("notes").join("Zhao Paper.md")
    }
    fn reopen(&mut self) {
        self.lib = LocalLibrary::open(self.dir.path(), DEVICE).unwrap();
    }
}

#[test]
fn a_paper_without_a_notes_file_has_no_notes() {
    let fx = setup();
    assert!(!fx.has_notes(&fx.paper));
    assert!(!fx.has_notes(&fx.other));
}

#[test]
fn a_highlight_counts_right_away_and_deleting_it_clears_the_flag() {
    let mut fx = setup();
    let (h, t) = fx
        .lib
        .add_highlight(&fx.paper, highlight_draft(&fx.material, 0, ""), None)
        .unwrap();
    // 不必重新掃描
    assert!(fx.has_notes(&fx.paper));
    assert!(!fx.has_notes(&fx.other));
    fx.lib.delete_highlight(&fx.paper, &h.id, &t).unwrap();
    assert!(!fx.has_notes(&fx.paper));
}

#[test]
fn an_empty_notes_file_is_not_notes_but_thoughts_are() {
    let mut fx = setup();
    // 「用其他應用打開」會先建一份空的筆記檔
    let t = fx.lib.set_thoughts(&fx.paper, "", None).unwrap();
    assert!(fx.notes_file().exists());
    assert!(!fx.has_notes(&fx.paper));
    let t = fx.lib.set_thoughts(&fx.paper, "   \n ", Some(&t)).unwrap();
    assert!(!fx.has_notes(&fx.paper), "只有空白的想法不算");
    let t = fx
        .lib
        .set_thoughts(&fx.paper, "這篇的方法值得一試", Some(&t))
        .unwrap();
    assert!(fx.has_notes(&fx.paper));
    fx.lib.set_thoughts(&fx.paper, "", Some(&t)).unwrap();
    assert!(!fx.has_notes(&fx.paper));
}

#[test]
fn an_edit_in_the_notes_tool_shows_after_a_rescan() {
    let mut fx = setup();
    fx.lib.set_thoughts(&fx.paper, "", None).unwrap();
    assert!(!fx.has_notes(&fx.paper));
    // 在筆記工具裡寫進想法（直接改檔）
    let text = fs::read_to_string(fx.notes_file()).unwrap();
    let marker = "<!-- paperreader:thoughts -->\n";
    assert!(text.contains(marker), "{text}");
    let edited = text.replace(marker, &format!("{marker}在筆記工具裡寫的\n"));
    fs::write(fx.notes_file(), edited).unwrap();
    fx.lib.rescan().unwrap();
    assert!(fx.has_notes(&fx.paper));
    // 再在筆記工具裡清掉
    fs::write(fx.notes_file(), text).unwrap();
    fx.lib.rescan().unwrap();
    assert!(!fx.has_notes(&fx.paper));
}

#[test]
fn a_block_that_could_not_be_read_counts_as_notes() {
    let mut fx = setup();
    fx.lib.set_thoughts(&fx.paper, "", None).unwrap();
    let text = fs::read_to_string(fx.notes_file()).unwrap();
    let marker = "<!-- paperreader:highlights -->\n";
    assert!(text.contains(marker), "{text}");
    let edited = text.replace(
        marker,
        &format!("{marker}<!-- paperreader:highlight {{not json -->\n使用者在別處寫的\n"),
    );
    fs::write(fx.notes_file(), edited).unwrap();
    fx.lib.rescan().unwrap();
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.unreadable.len(), 1, "{doc:?}");
    assert!(fx.has_notes(&fx.paper));
}

#[test]
fn a_highlight_written_in_the_notes_tool_shows_after_a_rescan() {
    let mut fx = setup();
    let t = fx.lib.set_thoughts(&fx.paper, "", None).unwrap();
    let without = fs::read_to_string(fx.notes_file()).unwrap();
    fx.lib
        .add_highlight(&fx.paper, highlight_draft(&fx.material, 0, ""), Some(&t))
        .unwrap();
    let with = fs::read_to_string(fx.notes_file()).unwrap();
    fs::write(fx.notes_file(), &without).unwrap();
    fx.lib.rescan().unwrap();
    assert!(!fx.has_notes(&fx.paper));
    // 在筆記工具裡貼進一條完整的劃線
    fs::write(fx.notes_file(), &with).unwrap();
    fx.lib.rescan().unwrap();
    assert!(fx.has_notes(&fx.paper));
}

#[test]
fn a_notes_file_that_is_not_utf8_has_no_notes() {
    let mut fx = setup();
    fx.lib.set_thoughts(&fx.paper, "有字的想法", None).unwrap();
    assert!(fx.has_notes(&fx.paper));
    let mut bytes = fs::read(fx.notes_file()).unwrap();
    bytes.extend_from_slice(&[0xFF, 0xFE, 0xFA, b'\n']);
    fs::write(fx.notes_file(), &bytes).unwrap();
    fx.lib.rescan().unwrap();
    assert!(
        fx.lib.read_notes(&fx.paper).is_err(),
        "這份檔仍認成這一篇的筆記檔，只是讀不了"
    );
    assert!(!fx.has_notes(&fx.paper));
}

#[test]
fn reopening_gives_the_same_answer() {
    let mut fx = setup();
    fx.lib
        .add_highlight(&fx.paper, highlight_draft(&fx.material, 0, "短評"), None)
        .unwrap();
    fx.lib.set_thoughts(&fx.other, "", None).unwrap();
    fx.reopen();
    assert!(fx.has_notes(&fx.paper));
    assert!(!fx.has_notes(&fx.other));
}

#[test]
fn with_two_files_for_one_paper_the_first_by_name_decides() {
    let mut fx = setup();
    fx.lib
        .add_highlight(&fx.paper, highlight_draft(&fx.material, 0, ""), None)
        .unwrap();
    let notes = fx.dir.path().join("notes");
    // 檔名排在前面的另一份空筆記檔也說自己屬於這篇：讀筆記的操作取它，「有筆記」也看它
    let empty = format!(
        "---\npaperreader_paper: {}\n---\n\n<!-- paperreader:thoughts -->\n<!-- paperreader:thoughts-end -->\n",
        fx.paper
    );
    fs::write(notes.join("A first.md"), empty).unwrap();
    fx.lib.rescan().unwrap();
    let (doc, _) = fx.lib.read_notes(&fx.paper).unwrap();
    assert_eq!(doc.path.as_deref(), Some("notes/A first.md"));
    assert!(!fx.has_notes(&fx.paper));
}
