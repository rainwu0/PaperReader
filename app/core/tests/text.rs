//! 抽出的文字 `text.json`：寫入的檢查與上限、內容相同與版本較高時不寫、背景寫入、讀不懂與較新的
//! 格式、不認得的欄位（重抽）、`text_states`、目錄摘要的 `files`、`read_for_index`、跟著那篇走（10.1 第 1 到 8、12 項）。

mod common;

use common::*;
use paperreader_core::digest::{DigestDraft, DigestSection, PointDraft};
use paperreader_core::error::LibError;
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::overlay::{OverlayDraft, SegmentDraft};
use paperreader_core::store::{
    CheckLevel, FileStamp, IndexRead, IndexSource, PaperDigest, ProblemKind,
};
use paperreader_core::text::*;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

const PAGES: u32 = 3;

struct Fx {
    dir: TempDir,
    lib: LocalLibrary,
    pid: String,
    mid: String,
    sha: String,
}

impl Fx {
    fn root(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
    fn folder(&self) -> PathBuf {
        let name = self
            .lib
            .list_papers()
            .into_iter()
            .find(|p| p.id == self.pid)
            .unwrap()
            .folder;
        self.root().join("papers").join(name)
    }
    fn file(&self) -> PathBuf {
        self.folder().join(TEXT_FILE)
    }
    fn draft(&self, pages: &[&str]) -> TextDraft {
        TextDraft {
            extractor: "pdfjs-text".into(),
            version: 1,
            source_material: self.mid.clone(),
            source_sha256: self.sha.clone(),
            pages: pages.iter().map(|s| s.to_string()).collect(),
            truncated: false,
        }
    }
    fn write(&mut self, pages: &[&str]) -> VersionToken {
        let d = self.draft(pages);
        self.lib.write_text(&self.pid, d).unwrap()
    }
    fn doc(&self) -> TextFile {
        self.lib.read_text(&self.pid).unwrap().unwrap().0
    }
    fn raw(&self) -> Value {
        serde_json::from_slice(&fs::read(self.file()).unwrap()).unwrap()
    }
    fn put_raw(&self, v: &Value) {
        fs::write(self.file(), serde_json::to_string_pretty(v).unwrap()).unwrap();
    }
}

fn fx() -> Fx {
    fx_pages(Some(PAGES))
}

fn fx_pages(pages: Option<u32>) -> Fx {
    let (dir, mut lib) = temp_library();
    let (pid, mid) = add_paper(&mut lib, 1, "Zhao Paper", pages);
    let sha = lib.get_paper(&pid).unwrap().0.materials[0]
        .sha256
        .clone()
        .unwrap();
    Fx {
        dir,
        lib,
        pid,
        mid,
        sha,
    }
}

const P0: &str = "DETRs Beat YOLOs on Real-time Object Detection\nYian Zhao";
const P1: &str = "We propose the first real-time end-to-end object detector.";
const P2: &str = "";

fn invalid(r: Result<impl std::fmt::Debug, LibError>) -> String {
    match r {
        Err(LibError::InvalidInput { reason }) => reason,
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// ---------- 1. 寫入的檢查 ----------

#[test]
fn write_creates_the_file_with_core_stamps() {
    let mut f = fx();
    let tok = f.write(&[P0, P1, P2]);
    let (doc, read_tok) = f.lib.read_text(&f.pid).unwrap().unwrap();
    assert_eq!(read_tok, tok);
    assert_eq!(doc.format, 1);
    assert_eq!(doc.extraction.extractor, "pdfjs-text");
    assert_eq!(doc.extraction.version, 1);
    assert_eq!(doc.extraction.source_material, f.mid);
    assert_eq!(doc.extraction.source_sha256, f.sha);
    assert_eq!(doc.extraction.created_by, DEVICE);
    assert!(paperreader_core::ids::parse_utc_millis(&doc.extraction.created).is_some());
    let pages: Vec<(u32, &str)> = doc
        .pages
        .iter()
        .map(|p| (p.page, p.text.as_str()))
        .collect();
    assert_eq!(pages, [(0, P0), (1, P1), (2, P2)]);
    assert!(!doc.truncated);
    // 版本記號是整份檔的指紋；`truncated` 是假時不寫出
    let bytes = fs::read(f.file()).unwrap();
    assert_eq!(tok.0, paperreader_core::jsonio::sha256_hex(&bytes));
    assert!(f.raw().get("truncated").is_none());
    // 沒有留下暫存檔
    assert!(
        fs::read_dir(f.folder())
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().starts_with(".pr-"))
    );
}

#[test]
fn no_file_reads_as_none() {
    let f = fx();
    assert!(f.lib.read_text(&f.pid).unwrap().is_none());
    assert!(matches!(
        f.lib.read_text("01JCK3W2A2B3C4D5E6F7G8H9ZZ"),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn write_checks_the_structure() {
    let mut f = fx();
    let mut d = f.draft(&[P0, P1, P2]);
    d.extractor = String::new();
    invalid(f.lib.write_text(&f.pid, d));
    let mut d = f.draft(&[P0, P1, P2]);
    d.extractor = "x".repeat(41);
    invalid(f.lib.write_text(&f.pid, d));
    let mut d = f.draft(&[P0, P1, P2]);
    d.extractor = "字".repeat(40);
    f.lib.write_text(&f.pid, d).unwrap();
    let mut d = f.draft(&[P0, P1, P2]);
    d.version = 0;
    invalid(f.lib.write_text(&f.pid, d));
    // 頁數：沒截斷時要等於原文的頁數；截斷時不多於
    invalid(f.lib.write_text(&f.pid, f.draft(&[P0, P1])));
    invalid(f.lib.write_text(&f.pid, f.draft(&[P0, P1, P2, P2])));
    let mut d = f.draft(&[P0, P1, P2, P2]);
    d.truncated = true;
    invalid(f.lib.write_text(&f.pid, d));
    let mut d = f.draft(&[P0, P1]);
    d.truncated = true;
    f.lib.write_text(&f.pid, d).unwrap();
    assert!(f.doc().truncated);
    assert_eq!(f.raw()["truncated"], json!(true));
    assert_eq!(f.doc().pages.len(), 2);
}

#[test]
fn page_limit_counts_unicode_characters() {
    let mut f = fx();
    // 基本多文種平面以外的字（𝑥，4 個位元組）算一個字
    let full = "\u{1D465}".repeat(MAX_PAGE_CHARS);
    let over = "\u{1D465}".repeat(MAX_PAGE_CHARS + 1);
    let mut d = f.draft(&[&over, "", ""]);
    d.truncated = true;
    invalid(f.lib.write_text(&f.pid, d));
    let mut d = f.draft(&[&full, "", ""]);
    d.truncated = true;
    f.lib.write_text(&f.pid, d).unwrap();
    assert_eq!(f.doc().pages[0].text.chars().count(), MAX_PAGE_CHARS);
}

#[test]
fn whole_file_limits_characters_and_bytes() {
    let n = (MAX_TOTAL_CHARS / MAX_PAGE_CHARS) as u32; // 50 頁剛好 500 萬字
    let mut f = fx_pages(Some(n + 1));
    let page = "a".repeat(MAX_PAGE_CHARS);
    let mut pages: Vec<&str> = vec![page.as_str(); n as usize];
    pages.push("b");
    let mut d = f.draft(&pages);
    d.truncated = true;
    let reason = invalid(f.lib.write_text(&f.pid, d));
    assert!(reason.contains("characters"), "{reason}");
    pages.pop();
    pages.push("");
    f.lib.write_text(&f.pid, f.draft(&pages)).unwrap();
    // 字數在上限內、寫成 JSON 超過 24 MB（控制字元寫成 \u0001，一字 6 個位元組）
    let ctl = "\u{1}".repeat(MAX_PAGE_CHARS);
    let mut pages: Vec<&str> = vec![ctl.as_str(); n as usize];
    pages.push("");
    let before = fs::read(f.file()).unwrap();
    let reason = invalid(f.lib.write_text(&f.pid, f.draft(&pages)));
    assert!(reason.contains("bytes"), "{reason}");
    assert_eq!(fs::read(f.file()).unwrap(), before);
}

#[test]
fn source_fingerprint_that_is_not_current_is_a_conflict() {
    let mut f = fx();
    let mut d = f.draft(&[P0, P1, P2]);
    d.source_sha256 = "0".repeat(64);
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::Conflict)
    ));
    assert!(!f.file().exists());
}

#[test]
fn unknown_page_count_is_invalid_input() {
    let mut f = fx_pages(None);
    let reason = invalid(f.lib.write_text(&f.pid, f.draft(&[P0, P1, P2])));
    assert!(reason.starts_with("source pages unknown"), "{reason}");
    assert!(!f.file().exists());
    // 記下頁數之後就收
    f.lib.set_material_pages(&f.pid, &f.mid, 3).unwrap();
    f.write(&[P0, P1, P2]);
}

#[test]
fn material_that_is_not_the_source_or_paper_not_in_catalog_is_not_found() {
    let mut f = fx();
    let mut d = f.draft(&[P0, P1, P2]);
    d.source_material = "01JCK3W2A2B3C4D5E6F7G8H9ZZ".into();
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::NotFound { .. })
    ));
    let d = f.draft(&[P0, P1, P2]);
    assert!(matches!(
        f.lib.write_text("01JCK3W2A2B3C4D5E6F7G8H9ZZ", d),
        Err(LibError::NotFound { .. })
    ));
    // 這篇的材料、但不是原文（譯本）：也是 NotFound
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 700, 99);
    let tok = f.lib.get_paper(&f.pid).unwrap().1;
    f.lib.add_translation(&f.pid, &pdf, None, &tok).unwrap();
    let tr = f
        .lib
        .get_paper(&f.pid)
        .unwrap()
        .0
        .materials
        .into_iter()
        .find(|m| m.role != "source")
        .unwrap();
    let mut d = f.draft(&[P0, P1, P2]);
    d.source_material = tr.id;
    d.source_sha256 = tr.sha256.unwrap();
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::NotFound { .. })
    ));
    assert!(!f.file().exists());
}

#[test]
fn read_only_library_refuses_to_write() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let lib_json = f.root().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lib_json).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&lib_json, v.to_string()).unwrap();
    let mut lib = LocalLibrary::open(&f.root(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    assert!(matches!(
        lib.write_text(&f.pid, f.draft(&[P1, P1, P1])),
        Err(LibError::ReadOnly)
    ));
    // 照樣讀得到
    assert_eq!(lib.read_text(&f.pid).unwrap().unwrap().0.pages[0].text, P0);
}

// ---------- 2. 內容相同、版本較高時不寫 ----------

#[test]
fn same_content_is_not_rewritten() {
    let mut f = fx();
    let t1 = f.write(&[P0, P1, P2]);
    let before = fs::read(f.file()).unwrap();
    let t2 = f.write(&[P0, P1, P2]);
    assert_eq!(t1, t2);
    assert_eq!(fs::read(f.file()).unwrap(), before);
    // 內容不同就寫
    let t3 = f.write(&[P1, P1, P2]);
    assert_ne!(t3, t1);
    assert_eq!(f.doc().pages[0].text, P1);
}

#[test]
fn higher_version_of_the_same_extractor_is_not_overwritten() {
    let mut f = fx();
    let mut d = f.draft(&[P0, P1, P2]);
    d.version = 2;
    let t2 = f.lib.write_text(&f.pid, d).unwrap();
    let before = fs::read(f.file()).unwrap();
    // 舊版應用抽的（版本 1）：不蓋新版抽的，交回現有的記號
    let t1 = f.lib.write_text(&f.pid, f.draft(&[P1, P1, P1])).unwrap();
    assert_eq!(t1, t2);
    assert_eq!(fs::read(f.file()).unwrap(), before);
    // 版本較高的照寫
    let mut d = f.draft(&[P1, P1, P1]);
    d.version = 3;
    f.lib.write_text(&f.pid, d).unwrap();
    assert_eq!(f.doc().extraction.version, 3);
    // 別的抽法不算「同一個抽法」：照寫
    let mut d = f.draft(&[P0, P0, P0]);
    d.extractor = "other-extractor".into();
    f.lib.write_text(&f.pid, d).unwrap();
    assert_eq!(f.doc().extraction.extractor, "other-extractor");
}

// ---------- 3. 背景寫入 ----------

#[test]
fn background_write_redoes_once_then_reports_conflict() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    f.lib.set_background_write_hook(Some(Box::new(|p: &Path| {
        let mut v: Value = serde_json::from_slice(&fs::read(p).unwrap()).unwrap();
        let n = v["touched"].as_u64().unwrap_or(0);
        v["touched"] = json!(n + 1);
        fs::write(p, v.to_string()).unwrap();
    })));
    let d = f.draft(&[P1, P1, P1]);
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::Conflict)
    ));
    assert_eq!(f.raw()["touched"], json!(2));
    assert_eq!(f.doc().pages[0].text, P0);
    // 只被改一次：重做之後寫成，別處加的欄位照樣留著
    let mut once = true;
    f.lib
        .set_background_write_hook(Some(Box::new(move |p: &Path| {
            if once {
                once = false;
                let mut v: Value = serde_json::from_slice(&fs::read(p).unwrap()).unwrap();
                v["touched"] = json!("once");
                fs::write(p, v.to_string()).unwrap();
            }
        })));
    let d = f.draft(&[P1, P1, P1]);
    f.lib.write_text(&f.pid, d).unwrap();
    f.lib.set_background_write_hook(None);
    assert_eq!(f.raw()["touched"], json!("once"));
    assert_eq!(f.doc().pages[0].text, P1);
}

// ---------- 4. 讀不懂、超過 32 MB、較新的格式 ----------

#[test]
fn unreadable_file_is_never_overwritten() {
    let mut f = fx();
    // 使用者自己放的同名檔：不是這個格式
    fs::write(f.file(), b"{\"hello\": \"mine\"}").unwrap();
    let before = fs::read(f.file()).unwrap();
    assert!(matches!(
        f.lib.read_text(&f.pid),
        Err(LibError::Unreadable { .. })
    ));
    let d = f.draft(&[P0, P1, P2]);
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::Unreadable { .. })
    ));
    assert_eq!(fs::read(f.file()).unwrap(), before);
    let problems = f.lib.check(CheckLevel::Full).unwrap();
    let kinds: Vec<ProblemKind> = problems.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, vec![ProblemKind::UnreadableTextFile]);
    assert!(problems[0].path.ends_with("/text.json"));
    assert_eq!(problems[0].paper.as_deref(), Some(f.pid.as_str()));
}

#[test]
fn file_over_32_megabytes_is_unreadable() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut text = fs::read_to_string(f.file()).unwrap();
    text.push_str(&" ".repeat(MAX_FILE_BYTES as usize));
    fs::write(f.file(), text).unwrap();
    assert!(matches!(
        f.lib.read_text(&f.pid),
        Err(LibError::Unreadable { .. })
    ));
    let d = f.draft(&[P1, P1, P1]);
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::Unreadable { .. })
    ));
    let kinds: Vec<ProblemKind> = f
        .lib
        .check(CheckLevel::Quick)
        .unwrap()
        .iter()
        .map(|p| p.kind)
        .collect();
    assert_eq!(kinds, vec![ProblemKind::UnreadableTextFile]);
}

#[test]
fn newer_format_is_read_but_never_written() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    raw["format"] = json!(2);
    f.put_raw(&raw);
    let before = fs::read(f.file()).unwrap();
    assert_eq!(f.doc().format, 2);
    let d = f.draft(&[P1, P1, P1]);
    assert!(matches!(
        f.lib.write_text(&f.pid, d),
        Err(LibError::FormatTooNew { found: 2 })
    ));
    assert_eq!(fs::read(f.file()).unwrap(), before);
    assert!(f.lib.check(CheckLevel::Full).unwrap().is_empty());
}

/// 快速檢查（開文獻庫、回到前景、同步與加入之後）只看檔案屬性，不開 `text.json`；
/// 讀不懂由完整檢查列出。超過 32 MB、不是一般檔案只看屬性就知道，快速檢查照樣列。
#[test]
fn quick_check_only_looks_at_file_attributes() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    fs::write(f.file(), b"not json at all").unwrap();
    let kinds = |level| -> Vec<ProblemKind> {
        f.lib.check(level).unwrap().iter().map(|p| p.kind).collect()
    };
    assert!(
        kinds(CheckLevel::Quick).is_empty(),
        "{:?}",
        kinds(CheckLevel::Quick)
    );
    assert_eq!(
        kinds(CheckLevel::Full),
        vec![ProblemKind::UnreadableTextFile]
    );
    // 不是一般檔案（同名的資料夾）：只看屬性就知道
    fs::remove_file(f.file()).unwrap();
    fs::create_dir(f.file()).unwrap();
    assert_eq!(
        kinds(CheckLevel::Quick),
        vec![ProblemKind::UnreadableTextFile]
    );
}

/// `pages[i].page` 要等於 i，不合的檔當成讀不懂：不覆寫、完整檢查列出。
#[test]
fn pages_out_of_sequence_are_unreadable() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    raw["pages"][1]["page"] = json!(7);
    f.put_raw(&raw);
    let before = fs::read(f.file()).unwrap();
    assert!(matches!(
        f.lib.read_text(&f.pid),
        Err(LibError::Unreadable { .. })
    ));
    assert!(matches!(
        f.lib.write_text(&f.pid, f.draft(&[P1, P1, P1])),
        Err(LibError::Unreadable { .. })
    ));
    assert_eq!(fs::read(f.file()).unwrap(), before);
    let kinds: Vec<ProblemKind> = f
        .lib
        .check(CheckLevel::Full)
        .unwrap()
        .iter()
        .map(|p| p.kind)
        .collect();
    assert_eq!(kinds, vec![ProblemKind::UnreadableTextFile]);
    assert_eq!(
        f.lib.read_for_index(&f.pid, "text", None).unwrap(),
        IndexRead::Missing
    );
}

#[test]
fn check_knows_the_file() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    // 不列成清單沒列的檔，也不列成大小不符（不是材料）
    assert!(f.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn scanned_source_is_written_with_empty_pages() {
    let mut f = fx();
    f.write(&["", "", ""]);
    assert!(f.doc().pages.iter().all(|p| p.text.is_empty()));
}

// ---------- 5. 不認得的欄位（重抽） ----------

#[test]
fn reextraction_of_the_same_source_moves_page_extras_by_index() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    raw["x_top"] = json!("top");
    raw["extraction"]["x_ext"] = json!("ext");
    raw["pages"][0]["x_p0"] = json!("zero");
    raw["pages"][1]["x_p1"] = json!("one");
    raw["pages"][2]["x_p2"] = json!("two");
    f.put_raw(&raw);
    // 版本 2、只剩兩頁（截斷）：第 0、1 頁的搬過去，第 2 頁的丟掉；頂層與檔頭的留著
    let mut d = f.draft(&[P1, P0]);
    d.version = 2;
    d.truncated = true;
    f.lib.write_text(&f.pid, d).unwrap();
    let raw = f.raw();
    assert_eq!(raw["x_top"], json!("top"));
    assert_eq!(raw["extraction"]["x_ext"], json!("ext"));
    assert_eq!(raw["extraction"]["version"], json!(2));
    assert_eq!(raw["pages"][0]["x_p0"], json!("zero"));
    assert_eq!(raw["pages"][0]["text"], json!(P1));
    assert_eq!(raw["pages"][1]["x_p1"], json!("one"));
    assert_eq!(raw["pages"].as_array().unwrap().len(), 2);
    assert!(!fs::read_to_string(f.file()).unwrap().contains("x_p2"));
}

#[test]
fn page_extras_do_not_replace_known_keys() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    // 新版應用在頁上多記的欄位；名稱和這一版認得的不同
    raw["pages"][0]["lang"] = json!("en");
    f.put_raw(&raw);
    f.write(&[P1, P1, P2]);
    let raw = f.raw();
    assert_eq!(raw["pages"][0]["lang"], json!("en"));
    assert_eq!(raw["pages"][0]["text"], json!(P1));
    assert_eq!(raw["pages"][0]["page"], json!(0));
}

#[test]
fn reextraction_after_the_source_changed_drops_page_extras() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    raw["x_top"] = json!("top");
    raw["extraction"]["x_ext"] = json!("ext");
    raw["extraction"]["source_sha256"] = json!("1".repeat(64));
    raw["pages"][0]["x_p0"] = json!("zero");
    f.put_raw(&raw);
    f.write(&[P1, P1, P2]);
    let raw = f.raw();
    assert_eq!(raw["x_top"], json!("top"));
    assert_eq!(raw["extraction"]["x_ext"], json!("ext"));
    assert_eq!(raw["extraction"]["source_sha256"], json!(f.sha));
    assert!(raw["pages"][0].get("x_p0").is_none());
}

// ---------- 6. text_states ----------

#[test]
fn text_states_reads_only_the_header() {
    let (_dir, mut lib) = temp_library();
    let mut ids = Vec::new();
    for (seed, title) in [(1, "Has"), (2, "None"), (3, "Bad"), (4, "Newer")] {
        ids.push(add_paper(&mut lib, seed, title, Some(1)).0);
    }
    let sha = |lib: &LocalLibrary, id: &str| {
        let m = lib.get_paper(id).unwrap().0.materials[0].clone();
        (m.id, m.sha256.unwrap())
    };
    for i in [0, 2, 3] {
        let (mid, s) = sha(&lib, &ids[i]);
        lib.write_text(
            &ids[i],
            TextDraft {
                extractor: "pdfjs-text".into(),
                version: 1,
                source_material: mid,
                source_sha256: s,
                pages: vec!["p".into()],
                truncated: false,
            },
        )
        .unwrap();
    }
    let path_of = |lib: &LocalLibrary, id: &str, root: &Path| {
        let folder = lib
            .list_papers()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .folder;
        root.join("papers").join(folder).join(TEXT_FILE)
    };
    let root = _dir.path().to_path_buf();
    fs::write(path_of(&lib, &ids[2], &root), b"not json").unwrap();
    let newer = path_of(&lib, &ids[3], &root);
    let mut v: Value = serde_json::from_slice(&fs::read(&newer).unwrap()).unwrap();
    v["format"] = json!(2);
    v["pages"] = json!("a newer app may store pages differently");
    fs::write(&newer, v.to_string()).unwrap();
    let mut ask = ids.clone();
    ask.push("01JCK3W2A2B3C4D5E6F7G8H9ZZ".into());
    let got = lib.text_states(&ask);
    assert_eq!(got.len(), 5);
    let st = |i: usize| got[i].1.clone();
    assert_eq!(got[0].0, ids[0]);
    let s0 = st(0).expect("has a file");
    assert_eq!(s0.extractor, "pdfjs-text");
    assert_eq!(s0.version, 1);
    assert_eq!(s0.source_sha256, sha(&lib, &ids[0]).1);
    assert!(st(1).is_none(), "no file");
    assert!(st(2).is_none(), "unreadable");
    let s3 = st(3).expect("newer format still answers with the header");
    assert_eq!(s3.source_sha256, sha(&lib, &ids[3]).1);
    assert!(st(4).is_none(), "not in the catalog");
}

// ---------- 7. 目錄摘要的 files ----------

fn digest_of(lib: &LocalLibrary, id: &str) -> PaperDigest {
    lib.paper_digests()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap()
}

fn overlay_draft(mid: &str) -> OverlayDraft {
    OverlayDraft {
        source_material: mid.into(),
        provider: "anthropic".into(),
        model: "x".into(),
        source_lang: None,
        target_lang: "zh-TW".into(),
        segments: (0..2)
            .map(|i| SegmentDraft {
                page: i,
                bbox: [1.0, 1.0, 2.0, 2.0],
                kind: "body".into(),
                size: None,
                source: format!("source text {i}"),
            })
            .collect(),
    }
}

fn digest_draft(mid: &str) -> DigestDraft {
    DigestDraft {
        source_material: mid.into(),
        service: "test".into(),
        provider: "anthropic".into(),
        model: "claude-test".into(),
        lang: "zh-TW".into(),
        title: "Zhao Paper".into(),
        sections: vec![DigestSection {
            id: "method".into(),
            label: "方法".into(),
        }],
        points: vec![PointDraft {
            section: "method".into(),
            text: "用混合編碼器分開處理多尺度特徵。".into(),
            pages: vec![1],
            segments: vec![],
        }],
    }
}

/// 一篇有抽出的文字、疊加檔、重點筆記、筆記檔的文獻。交回（疊加檔、重點筆記的材料代號）。
fn with_all_sources(f: &mut Fx) -> (String, String) {
    f.write(&[P0, P1, P2]);
    let tok = f.lib.get_paper(&f.pid).unwrap().1;
    let ov = f
        .lib
        .create_overlay(&f.pid, overlay_draft(&f.mid), &tok)
        .unwrap()
        .material_id;
    let tok = f.lib.get_paper(&f.pid).unwrap().1;
    let dg = f
        .lib
        .create_digest(&f.pid, digest_draft(&f.mid), &tok)
        .unwrap()
        .material_id;
    let mid = f.mid.clone();
    f.lib
        .add_highlight(&f.pid, highlight_draft(&mid, 0, "我的短評"), None)
        .unwrap();
    (ov, dg)
}

#[test]
fn paper_digests_list_the_files_of_full_text_search() {
    let mut f = fx();
    let d0 = digest_of(&f.lib, &f.pid);
    assert!(d0.files.is_empty(), "no source yet: {:?}", d0.files);
    let (ov, dg) = with_all_sources(&mut f);
    let d1 = digest_of(&f.lib, &f.pid);
    let keys: Vec<&str> = d1.files.keys().map(String::as_str).collect();
    let (ok, dk) = (format!("overlay:{ov}"), format!("digest:{dg}"));
    let mut want = vec!["text", "notes", ok.as_str(), dk.as_str()];
    want.sort();
    assert_eq!(keys, want);
    assert_eq!(
        d1.files["text"].bytes,
        fs::metadata(f.file()).unwrap().len()
    );
    assert!(d1.references.is_none(), "references stays as before");
    // 每一份在應用外改了（大小變了）都看得出來
    let notes_dir = f.root().join("notes");
    let notes = fs::read_dir(&notes_dir)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".md"))
        .unwrap()
        .path();
    let folder = f.folder();
    let paper = f.lib.get_paper(&f.pid).unwrap().0;
    let path_of =
        |id: &str| folder.join(&paper.materials.iter().find(|m| m.id == id).unwrap().path);
    for (key, path) in [
        ("text".to_string(), f.file()),
        ("notes".to_string(), notes),
        (ok.clone(), path_of(&ov)),
        (dk.clone(), path_of(&dg)),
    ] {
        let before = digest_of(&f.lib, &f.pid).files[&key];
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n");
        fs::write(&path, &bytes).unwrap();
        let after = digest_of(&f.lib, &f.pid).files[&key];
        assert_ne!(before, after, "{key} changed outside the app");
    }
    // 疊加檔裡翻好一段（應用內寫）也看得出來
    let before = digest_of(&f.lib, &f.pid).files["text"];
    f.write(&[P1, P1, P2]);
    assert_ne!(digest_of(&f.lib, &f.pid).files["text"], before);
}

// ---------- 8. read_for_index ----------

fn stamp_now(path: &Path) -> FileStamp {
    let m = fs::metadata(path).unwrap();
    FileStamp {
        bytes: m.len(),
        modified_ms: m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64),
    }
}

#[test]
fn read_for_index_reads_each_kind_of_source() {
    let mut f = fx();
    let (ov, dg) = with_all_sources(&mut f);
    let files = digest_of(&f.lib, &f.pid).files;
    let read = |key: &str| match f.lib.read_for_index(&f.pid, key, None).unwrap() {
        IndexRead::Read(stamp, src) => {
            assert_eq!(stamp, files[key], "{key}: the stamp of the digest");
            *src
        }
        other => panic!("{key}: {other:?}"),
    };
    match read("text") {
        IndexSource::Text(t) => assert_eq!(t.pages[1].text, P1),
        other => panic!("{other:?}"),
    }
    match read(&format!("overlay:{ov}")) {
        IndexSource::Overlay(o) => assert_eq!(o.segments.len(), 2),
        other => panic!("{other:?}"),
    }
    match read(&format!("digest:{dg}")) {
        IndexSource::Digest(d) => assert_eq!(d.points.len(), 1),
        other => panic!("{other:?}"),
    }
    match read("notes") {
        IndexSource::Notes(n) => assert_eq!(n.highlights[0].comment, "我的短評"),
        other => panic!("{other:?}"),
    }
}

/// `read_for_index` 不算版本記號（SHA-256），四種鍵都是。
#[test]
fn read_for_index_does_not_compute_fingerprints() {
    use paperreader_core::jsonio::sha256_calls;
    let mut f = fx();
    let (ov, dg) = with_all_sources(&mut f);
    for key in [
        "text".to_string(),
        "notes".to_string(),
        format!("overlay:{ov}"),
        format!("digest:{dg}"),
    ] {
        let before = sha256_calls();
        let got = f.lib.read_for_index(&f.pid, &key, None).unwrap();
        assert!(matches!(got, IndexRead::Read(..)), "{key}: {got:?}");
        assert_eq!(sha256_calls(), before, "{key}: computed a fingerprint");
    }
}

#[test]
fn read_for_index_does_not_read_an_unchanged_file() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let path = f.file();
    let known = stamp_now(&path);
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    // 換成同樣長度的垃圾，修改時間設回去：檔案屬性和記下的相同，不讀內容
    let len = fs::metadata(&path).unwrap().len() as usize;
    fs::write(&path, vec![b'#'; len]).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    assert_eq!(stamp_now(&path), known);
    assert_eq!(
        f.lib.read_for_index(&f.pid, "text", Some(known)).unwrap(),
        IndexRead::Unchanged
    );
    // 屬性不同（另一個時間）：讀了，讀不懂交回 Missing
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        f.lib.read_for_index(&f.pid, "text", Some(known)).unwrap(),
        IndexRead::Missing
    );
}

#[test]
fn read_for_index_missing_newer_and_bad_keys() {
    let mut f = fx();
    let pid = f.pid.clone();
    // 沒有檔
    for key in [
        "text",
        "notes",
        "overlay:01JCK3W2A2B3C4D5E6F7G8H9ZZ",
        "digest:01JCK3W2A2B3C4D5E6F7G8H9ZZ",
    ] {
        assert_eq!(
            f.lib.read_for_index(&pid, key, None).unwrap(),
            IndexRead::Missing,
            "{key}"
        );
    }
    // 讀不懂、format 較新
    f.write(&[P0, P1, P2]);
    let mut raw = f.raw();
    raw["format"] = json!(2);
    f.put_raw(&raw);
    assert_eq!(
        f.lib.read_for_index(&pid, "text", None).unwrap(),
        IndexRead::Missing
    );
    fs::write(f.file(), b"garbage").unwrap();
    assert_eq!(
        f.lib.read_for_index(&pid, "text", None).unwrap(),
        IndexRead::Missing
    );
    // 不認得的鍵、不在目錄
    for key in ["", "pages", "overlay", "summary:x"] {
        invalid(f.lib.read_for_index(&pid, key, None));
    }
    assert!(matches!(
        f.lib
            .read_for_index("01JCK3W2A2B3C4D5E6F7G8H9ZZ", "text", None),
        Err(LibError::NotFound { .. })
    ));
}

// ---------- 12. 跟著那篇走 ----------

#[test]
fn text_follows_the_paper_through_trash_restore_backup_and_purge() {
    let mut f = fx();
    f.write(&[P0, P1, P2]);
    let pid = f.pid.clone();
    f.lib.remove_paper(&pid).unwrap();
    assert!(matches!(
        f.lib.read_text(&pid),
        Err(LibError::NotFound { .. })
    ));
    let trash = f.lib.list_trash().unwrap();
    let in_trash = f
        .root()
        .join(".trash")
        .join(&trash[0].folder)
        .join(TEXT_FILE);
    assert!(in_trash.is_file());
    f.lib.restore_paper(&pid).unwrap();
    assert_eq!(f.doc().pages[1].text, P1);
    // 備份檔還原到別處
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    f.lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let to = out.path().join("restored");
    paperreader_core::backup::restore_backup(&zip, &to, DEVICE_B).unwrap();
    let lib2 = LocalLibrary::open(&to, DEVICE_B).unwrap();
    assert_eq!(lib2.read_text(&pid).unwrap().unwrap().0.pages[0].text, P0);
    assert!(lib2.check(CheckLevel::Full).unwrap().is_empty());
    // 永久刪除：跟著不見
    f.lib.remove_paper(&pid).unwrap();
    let folder = f.lib.list_trash().unwrap()[0].folder.clone();
    f.lib.purge_trash_entry(&folder).unwrap();
    assert!(!f.root().join(".trash").join(&folder).exists());
}
