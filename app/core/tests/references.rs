//! 參考文獻清單 `references.json`：寫入的檢查、重抽、背景寫入、判斷的加刪、讀不懂與較新的格式、
//! 不認得的欄位、自動補書目、跟著那篇走、目錄摘要、正規化與找回鍵、判斷的套用（10.1 第 1 到 8、12 到 15 項）。

mod common;

use common::*;
use paperreader_core::changes::LINK_JUDGMENT_DELETED;
use paperreader_core::error::LibError;
use paperreader_core::format::{BibDraft, BibFill};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::references::*;
use paperreader_core::store::{CheckLevel, ProblemKind};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PAGES: u32 = 12;

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
    fn file(&self) -> PathBuf {
        refs_path(&self.lib, self.dir.path(), &self.pid)
    }
    fn draft(&self, entries: Vec<EntryDraft>) -> ReferencesDraft {
        ReferencesDraft {
            extractor: "links".into(),
            version: 1,
            source_material: self.mid.clone(),
            source_sha256: self.sha.clone(),
            entries,
        }
    }
    fn write(&mut self, entries: Vec<EntryDraft>) -> VersionToken {
        let d = self.draft(entries);
        self.lib.write_references(&self.pid, d).unwrap()
    }
    fn token(&self) -> VersionToken {
        self.lib.read_references(&self.pid).unwrap().unwrap().1
    }
    fn doc(&self) -> ReferencesFile {
        self.lib.read_references(&self.pid).unwrap().unwrap().0
    }
    fn raw(&self) -> Value {
        serde_json::from_slice(&fs::read(self.file()).unwrap()).unwrap()
    }
    fn put_raw(&self, v: &Value) {
        fs::write(self.file(), serde_json::to_string_pretty(v).unwrap()).unwrap();
    }
}

fn refs_path(lib: &LocalLibrary, root: &Path, pid: &str) -> PathBuf {
    let folder = lib
        .list_papers()
        .into_iter()
        .find(|p| p.id == pid)
        .unwrap()
        .folder;
    root.join("papers").join(folder).join(REFERENCES_FILE)
}

/// 一篇有原文、頁數已知的文獻。
fn fx() -> Fx {
    fx_pages(Some(PAGES))
}

fn fx_pages(pages: Option<u32>) -> Fx {
    let (dir, mut lib) = temp_library();
    let (pid, mid) = add_paper(&mut lib, 1, "Citing Paper", pages);
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

fn entry(text: &str, page: u32) -> EntryDraft {
    EntryDraft {
        number: None,
        label: None,
        text: text.into(),
        page,
        bbox: None,
        doi: None,
        arxiv: None,
        year: None,
    }
}

fn numbered(n: u32, text: &str) -> EntryDraft {
    EntryDraft {
        number: Some(n),
        label: Some(format!("[{n}]")),
        ..entry(text, 9)
    }
}

const DETR: &str =
    "Y. Zhao, W. Lv, S. Xu, et al. DETRs beat YOLOs on real-time object detection. In CVPR, 2024.";
const ATTN: &str = "A. Vaswani et al. Attention is all you need. In NeurIPS, 2017.";
const RESNET: &str =
    "K. He, X. Zhang, S. Ren, J. Sun. Deep residual learning for image recognition. In CVPR, 2016.";

fn invalid(r: Result<impl std::fmt::Debug, LibError>) -> String {
    match r {
        Err(LibError::InvalidInput { reason }) => reason,
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// ---------- 1. 寫入的檢查 ----------

#[test]
fn write_creates_the_file_with_core_stamps_and_normalized_values() {
    let mut f = fx();
    let mut e = numbered(12, DETR);
    e.doi = Some(
        "https://doi.org/10.1109/CVPR.2024.01605."
            .trim_end_matches('.')
            .into(),
    );
    e.arxiv = Some("2304.08069".into());
    e.year = Some(2024);
    e.bbox = Some([72.004, 120.5049, 300.2, 150.0]);
    f.write(vec![e]);
    let doc = f.doc();
    assert_eq!(doc.format, 1);
    assert_eq!(doc.extraction.extractor, "links");
    assert_eq!(doc.extraction.source_material, f.mid);
    assert_eq!(doc.extraction.source_sha256, f.sha);
    assert_eq!(doc.extraction.created_by, DEVICE);
    assert!(!doc.extraction.created.is_empty());
    assert_eq!(doc.modified_by.as_deref(), Some(DEVICE));
    let e = &doc.entries[0];
    assert_eq!(e.doi.as_deref(), Some("10.1109/cvpr.2024.01605"));
    assert_eq!(e.bbox, Some([72.0, 120.5, 300.2, 150.0]));
    assert_eq!(e.number, Some(12));
    assert!(doc.judgments.is_empty());
    // 和 paper.json 同一層，不列在材料清單
    let paper = f.lib.get_paper(&f.pid).unwrap().0;
    assert_eq!(paper.materials.len(), 1);
}

#[test]
fn no_file_reads_as_none() {
    let f = fx();
    assert!(f.lib.read_references(&f.pid).unwrap().is_none());
    assert!(matches!(
        f.lib.read_references("01JCK3W2A2B3C4D5E6F7G8H9ZZ"),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn write_checks_the_structure() {
    let mut f = fx();
    let ok = || entry(ATTN, 0);
    type Change = Box<dyn Fn(&mut ReferencesDraft)>;
    let cases: Vec<Change> = vec![
        Box::new(|d| d.extractor = String::new()),
        Box::new(|d| d.extractor = "x".repeat(41)),
        Box::new(|d| d.version = 0),
        Box::new(|d| d.entries[0].text = "  ".into()),
        Box::new(|d| d.entries[0].text = "a".repeat(1001)),
        Box::new(|d| d.entries[0].label = Some("x".repeat(41))),
        Box::new(|d| d.entries[0].number = Some(0)),
        Box::new(|d| d.entries[0].number = Some(100_000)),
        Box::new(|d| d.entries[0].page = PAGES),
        Box::new(|d| d.entries[0].bbox = Some([f64::NAN, 0.0, 1.0, 1.0])),
        Box::new(|d| d.entries[0].bbox = Some([0.0, 0.0, 2e6, 1.0])),
        Box::new(|d| d.entries[0].doi = Some("not a doi".into())),
        Box::new(|d| d.entries[0].doi = Some("11.1000/x".into())),
        Box::new(|d| d.entries[0].doi = Some(format!("10.1000/{}", "x".repeat(300)))),
        Box::new(|d| d.entries[0].arxiv = Some("2304.08069v2".into())),
        Box::new(|d| d.entries[0].arxiv = Some("abc".into())),
        Box::new(|d| d.entries[0].arxiv = Some("HEP-TH/9901001".into())),
        Box::new(|d| d.entries[0].year = Some(999)),
        Box::new(|d| d.entries[0].year = Some(10000)),
    ];
    for (i, change) in cases.iter().enumerate() {
        let mut d = f.draft(vec![ok()]);
        change(&mut d);
        let r = f.lib.write_references(&f.pid, d);
        assert!(
            matches!(r, Err(LibError::InvalidInput { .. })),
            "case {i}: {r:?}"
        );
        assert!(!f.file().exists(), "case {i} wrote a file");
    }
    // 合規則的舊式與新式 arXiv 編號、文字剛好 1000 字、編號 99999 都收
    let mut d = f.draft(vec![ok(), ok(), ok()]);
    d.entries[0].arxiv = Some("hep-th/9901001".into());
    d.entries[1].arxiv = Some("math.GT/0309136".into());
    d.entries[2].text = "字".repeat(1000);
    d.entries[2].number = Some(99_999);
    f.lib.write_references(&f.pid, d).unwrap();
}

#[test]
fn write_limits_the_entry_count_and_the_extracted_bytes() {
    let mut f = fx();
    let many = |n: usize, text: &str| (0..n).map(|_| entry(text, 0)).collect::<Vec<_>>();
    let d = f.draft(many(MAX_ENTRIES + 1, "A. B. Title. 2020."));
    invalid(f.lib.write_references(&f.pid, d));
    // 2000 條、每條 1000 個四位元組的字：條數與字數都合，位元組超過 6 MB
    let d = f.draft(many(MAX_ENTRIES, &"𝐀".repeat(1000)));
    let reason = invalid(f.lib.write_references(&f.pid, d));
    assert!(reason.contains("bytes"), "{reason}");
    assert!(!f.file().exists());
    f.write(many(MAX_ENTRIES, "A. B. Title. 2020."));
    assert_eq!(f.doc().entries.len(), MAX_ENTRIES);
}

#[test]
fn source_fingerprint_that_is_not_current_is_a_conflict() {
    let mut f = fx();
    let mut d = f.draft(vec![entry(ATTN, 0)]);
    d.source_sha256 = "0".repeat(64);
    assert!(matches!(
        f.lib.write_references(&f.pid, d),
        Err(LibError::Conflict)
    ));
    assert!(!f.file().exists());
}

#[test]
fn unknown_page_count_is_invalid_input() {
    let mut f = fx_pages(None);
    let d = f.draft(vec![entry(ATTN, 0)]);
    let reason = invalid(f.lib.write_references(&f.pid, d));
    assert!(reason.starts_with("source pages unknown"), "{reason}");
    // 記下頁數之後就收
    f.lib.set_material_pages(&f.pid, &f.mid, 3).unwrap();
    f.write(vec![entry(ATTN, 2)]);
}

#[test]
fn material_that_is_not_the_source_is_not_found() {
    let mut f = fx();
    let mut d = f.draft(vec![entry(ATTN, 0)]);
    d.source_material = "01JCK3W2A2B3C4D5E6F7G8H9ZZ".into();
    assert!(matches!(
        f.lib.write_references(&f.pid, d),
        Err(LibError::NotFound { .. })
    ));
    let d = f.draft(vec![entry(ATTN, 0)]);
    assert!(matches!(
        f.lib.write_references("01JCK3W2A2B3C4D5E6F7G8H9ZZ", d),
        Err(LibError::NotFound { .. })
    ));
}

#[test]
fn read_only_library_refuses_every_write() {
    let mut f = fx();
    f.write(vec![entry(ATTN, 0)]);
    let tok = f.token();
    let lib_json = f.root().join("library.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&lib_json).unwrap()).unwrap();
    v["format"] = json!(2);
    fs::write(&lib_json, v.to_string()).unwrap();
    let mut lib = LocalLibrary::open(&f.root(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let d = f.draft(vec![entry(DETR, 0)]);
    assert!(matches!(
        lib.write_references(&f.pid, d),
        Err(LibError::ReadOnly)
    ));
    let other = add_other(&mut f);
    assert!(matches!(
        lib.add_link_judgment(&f.pid, 0, JudgmentKind::Reject, &other, &tok),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(
        lib.remove_link_judgments(&f.pid, &["j-x".to_string()], &tok),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(
        lib.fill_bib_blanks(&f.pid, BibFill::default()),
        Err(LibError::ReadOnly)
    ));
}

/// 庫裡另加一篇，交回代號（判斷的對象）。
fn add_other(f: &mut Fx) -> String {
    let n = f.lib.list_papers().len() as u64;
    add_paper(&mut f.lib, 100 + n, &format!("Cited {n}"), Some(5)).0
}

// ---------- 2. 重寫抽出的部分 ----------

#[test]
fn rewrite_keeps_judgments_and_moves_unknown_entry_fields_by_key() {
    let mut f = fx();
    let other = add_other(&mut f);
    f.write(vec![numbered(1, DETR), numbered(2, ATTN)]);
    let (j, _) = f
        .lib
        .add_link_judgment(&f.pid, 1, JudgmentKind::Reject, &other, &f.token())
        .unwrap();
    // 第二期連網取回的欄位、新版應用加的欄位
    let mut raw = f.raw();
    raw["entries"][1]["openalex"] = json!("W123");
    raw["entries"][0]["note"] = json!({"a": 1});
    raw["future_top"] = json!(true);
    raw["extraction"]["model"] = json!("v9");
    f.put_raw(&raw);
    // 重抽：順序換了、斷行與編號不同
    f.write(vec![
        numbered(
            5,
            "A. Vaswani et al. Atten- tion is all you need. In NeurIPS, 2017.",
        ),
        numbered(6, DETR),
        numbered(7, RESNET),
    ]);
    let raw = f.raw();
    assert_eq!(raw["entries"][0]["openalex"], json!("W123"));
    assert_eq!(raw["entries"][1]["note"], json!({"a": 1}));
    assert!(raw["entries"][2].get("openalex").is_none());
    assert_eq!(raw["future_top"], json!(true));
    assert_eq!(raw["extraction"]["model"], json!("v9"));
    let doc = f.doc();
    assert_eq!(doc.judgments, vec![j.clone()]);
    // 判斷照樣作用在「Attention」那一條（現在是第 0 條）
    let r = resolve_judgments(&doc.entries, &doc.judgments);
    assert_eq!(r.entries[0].rejected, vec![0]);
    assert!(r.lost.is_empty());
}

#[test]
fn same_extracted_part_is_not_rewritten() {
    let mut f = fx();
    let t1 = f.write(vec![numbered(1, DETR)]);
    let before = fs::read(f.file()).unwrap();
    let t2 = f.write(vec![numbered(1, DETR)]);
    assert_eq!(t1, t2);
    assert_eq!(fs::read(f.file()).unwrap(), before);
    // 抽取的版本不同就寫
    let mut d = f.draft(vec![numbered(1, DETR)]);
    d.version = 2;
    let t3 = f.lib.write_references(&f.pid, d).unwrap();
    assert_ne!(t3, t1);
    assert_eq!(f.doc().extraction.version, 2);
}

// ---------- 3. 背景寫入 ----------

#[test]
fn background_write_redoes_once_then_reports_conflict() {
    let mut f = fx();
    f.write(vec![numbered(1, DETR)]);
    // 每次「寫之前再讀一次」之前都被別處改過：重做一次仍不同
    f.lib.set_background_write_hook(Some(Box::new(|p: &Path| {
        let mut v: Value = serde_json::from_slice(&fs::read(p).unwrap()).unwrap();
        let n = v["touched"].as_u64().unwrap_or(0);
        v["touched"] = json!(n + 1);
        fs::write(p, v.to_string()).unwrap();
    })));
    let d = f.draft(vec![numbered(1, ATTN)]);
    assert!(matches!(
        f.lib.write_references(&f.pid, d),
        Err(LibError::Conflict)
    ));
    assert_eq!(f.raw()["touched"], json!(2));
    assert_eq!(f.doc().entries[0].text, DETR);
    // 只被改一次：重做之後寫成，別處改的照樣留著
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
    let d = f.draft(vec![numbered(1, ATTN)]);
    f.lib.write_references(&f.pid, d).unwrap();
    f.lib.set_background_write_hook(None);
    assert_eq!(f.raw()["touched"], json!("once"));
    assert_eq!(f.doc().entries[0].text, ATTN);
}

// ---------- 4. 加判斷 ----------

#[test]
fn add_judgment_copies_the_entry_and_computes_the_key() {
    let mut f = fx();
    let other = add_other(&mut f);
    let mut e = numbered(3, DETR);
    e.doi = Some("10.1109/cvpr.2024.01605".into());
    e.arxiv = Some("2304.08069".into());
    f.write(vec![numbered(1, ATTN), e]);
    let tok = f.token();
    let (j, t) = f
        .lib
        .add_link_judgment(&f.pid, 1, JudgmentKind::Manual, &other, &tok)
        .unwrap();
    assert!(j.id.starts_with("j-") && j.id.len() == 28 && j.id == j.id.to_lowercase());
    assert_eq!(j.kind, "manual");
    assert_eq!(j.paper, other);
    assert_eq!(j.key_version, ENTRY_KEY_VERSION);
    assert_eq!(j.entry_key, entry_key(DETR, 1).unwrap());
    assert_eq!(j.entry_text, DETR);
    assert_eq!(j.entry_doi.as_deref(), Some("10.1109/cvpr.2024.01605"));
    assert_eq!(j.entry_arxiv.as_deref(), Some("2304.08069"));
    assert_eq!(j.created_by, DEVICE);
    assert_eq!(t, f.token());
    assert_eq!(f.doc().judgments, vec![j]);
}

#[test]
fn add_judgment_rejects_bad_input() {
    let mut f = fx();
    let other = add_other(&mut f);
    // 沒有檔
    let fake = VersionToken("0".repeat(64));
    assert!(matches!(
        f.lib
            .add_link_judgment(&f.pid, 0, JudgmentKind::Reject, &other, &fake),
        Err(LibError::NotFound { .. })
    ));
    f.write(vec![numbered(1, ATTN)]);
    let tok = f.token();
    let pid = f.pid.clone();
    invalid(
        f.lib
            .add_link_judgment(&pid, 1, JudgmentKind::Reject, &other, &tok),
    );
    invalid(
        f.lib
            .add_link_judgment(&pid, 0, JudgmentKind::Manual, &pid, &tok),
    );
    invalid(f.lib.add_link_judgment(
        &pid,
        0,
        JudgmentKind::Manual,
        "01JCK3W2A2B3C4D5E6F7G8H9ZZ",
        &tok,
    ));
    // 記號不符
    assert!(matches!(
        f.lib
            .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &fake),
        Err(LibError::Conflict)
    ));
    assert!(f.doc().judgments.is_empty());
}

fn judgment_json(i: usize, paper: &str, text: &str) -> Value {
    json!({
        "id": format!("j-{i:026}"),
        "kind": "reject",
        "paper": paper,
        "entry_key": "x",
        "key_version": 1,
        "entry_text": text,
        "entry_doi": null,
        "entry_arxiv": null,
        "created": "2026-11-17T08:05:00.000Z",
        "created_by": DEVICE
    })
}

#[test]
fn judgments_are_limited_by_count_and_bytes() {
    let mut f = fx();
    let other = add_other(&mut f);
    f.write(vec![numbered(1, ATTN)]);
    let mut raw = f.raw();
    raw["judgments"] = Value::Array(
        (0..MAX_JUDGMENTS)
            .map(|i| judgment_json(i, &other, "t"))
            .collect(),
    );
    f.put_raw(&raw);
    let reason = invalid(f.lib.add_link_judgment(
        &f.pid.clone(),
        0,
        JudgmentKind::Reject,
        &other,
        &f.token(),
    ));
    assert!(reason.contains("judgments"), "{reason}");
    // 條數沒到上限、位元組超過 2 MB
    raw["judgments"] = Value::Array(
        (0..2200)
            .map(|i| judgment_json(i, &other, &"x".repeat(1000)))
            .collect(),
    );
    f.put_raw(&raw);
    invalid(
        f.lib
            .add_link_judgment(&f.pid.clone(), 0, JudgmentKind::Reject, &other, &f.token()),
    );
    // 重抽不受判斷多寡影響（兩部分各自檢查）
    f.write(vec![numbered(1, ATTN), numbered(2, DETR)]);
    assert_eq!(f.doc().judgments.len(), 2200);
}

#[test]
fn adding_the_same_kind_again_does_not_write() {
    let mut f = fx();
    let other = add_other(&mut f);
    f.write(vec![numbered(1, ATTN)]);
    let pid = f.pid.clone();
    let (j1, t1) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &f.token())
        .unwrap();
    let before = fs::read(f.file()).unwrap();
    let (j2, t2) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &t1)
        .unwrap();
    assert_eq!(j1, j2);
    assert_eq!(t1, t2);
    assert_eq!(fs::read(f.file()).unwrap(), before);
    // 換一種就寫：後面那一項較晚，現在是「連到」
    let (j3, _) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Manual, &other, &t2)
        .unwrap();
    let doc = f.doc();
    assert_eq!(doc.judgments.len(), 2);
    let r = resolve_judgments(&doc.entries, &doc.judgments);
    assert_eq!(
        r.entries[0].link.map(|i| doc.judgments[i].id.clone()),
        Some(j3.id)
    );
}

// ---------- 5. 刪判斷 ----------

fn with_two_judgments(f: &mut Fx) -> (String, String) {
    let a = add_other(f);
    let b = add_other(f);
    f.write(vec![numbered(1, ATTN), numbered(2, DETR)]);
    let pid = f.pid.clone();
    let (ja, _) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Reject, &a, &f.token())
        .unwrap();
    let (jb, _) = f
        .lib
        .add_link_judgment(&pid, 1, JudgmentKind::Manual, &b, &f.token())
        .unwrap();
    (ja.id, jb.id)
}

#[test]
fn remove_records_then_writes() {
    let mut f = fx();
    let (ja, jb) = with_two_judgments(&mut f);
    let pid = f.pid.clone();
    f.lib
        .remove_link_judgments(&pid, std::slice::from_ref(&ja), &f.token())
        .unwrap();
    let ids: Vec<String> = f.doc().judgments.into_iter().map(|j| j.id).collect();
    assert_eq!(ids, vec![jb.clone()]);
    let lines = change_lines(&f.root(), DEVICE);
    let rec = lines
        .iter()
        .find(|l| l["type"] == json!(LINK_JUDGMENT_DELETED))
        .unwrap();
    assert_eq!(rec["paper"], json!(pid));
    assert_eq!(rec["data"], json!({"judgments": [ja]}));
}

#[test]
fn remove_with_an_unknown_id_or_empty_list_writes_nothing() {
    let mut f = fx();
    let (ja, _) = with_two_judgments(&mut f);
    let before = fs::read(f.file()).unwrap();
    let pid = f.pid.clone();
    for ids in [vec![], vec![ja.clone(), "j-nope".to_string()]] {
        assert!(matches!(
            f.lib.remove_link_judgments(&pid, &ids, &f.token()),
            Err(LibError::NotFound { .. })
        ));
    }
    assert_eq!(fs::read(f.file()).unwrap(), before);
    assert!(
        change_lines(&f.root(), DEVICE)
            .iter()
            .all(|l| l["type"] != json!(LINK_JUDGMENT_DELETED))
    );
    // 記號不符
    assert!(matches!(
        f.lib
            .remove_link_judgments(&pid, &[ja], &VersionToken("0".repeat(64))),
        Err(LibError::Conflict)
    ));
}

#[test]
fn record_that_cannot_be_written_leaves_the_file() {
    let mut f = fx();
    let (ja, _) = with_two_judgments(&mut f);
    let before = fs::read(f.file()).unwrap();
    // 變更紀錄檔的位置被一個資料夾占住：附加失敗
    let log = changes_path(&f.root(), DEVICE);
    if log.exists() {
        fs::remove_file(&log).unwrap();
    }
    fs::create_dir_all(&log).unwrap();
    let pid = f.pid.clone();
    let tok = f.token();
    assert!(f.lib.remove_link_judgments(&pid, &[ja], &tok).is_err());
    assert_eq!(fs::read(f.file()).unwrap(), before);
}

#[test]
fn record_written_but_file_not_written_when_interrupted() {
    let mut f = fx();
    let (ja, _) = with_two_judgments(&mut f);
    let before = fs::read(f.file()).unwrap();
    f.lib.set_change_hook(Some(Box::new(|_| false)));
    let pid = f.pid.clone();
    let tok = f.token();
    assert!(
        f.lib
            .remove_link_judgments(&pid, std::slice::from_ref(&ja), &tok)
            .is_err()
    );
    f.lib.set_change_hook(None);
    assert_eq!(fs::read(f.file()).unwrap(), before);
    let rec = change_lines(&f.root(), DEVICE)
        .into_iter()
        .find(|l| l["type"] == json!(LINK_JUDGMENT_DELETED))
        .unwrap();
    assert_eq!(rec["data"]["judgments"], json!([ja]));
}

// ---------- 6. 讀不懂、超過上限、較新的格式 ----------

#[test]
fn unreadable_file_is_never_overwritten() {
    let mut f = fx();
    let other = add_other(&mut f);
    // 使用者自己放的同名檔：不是這個格式
    fs::write(f.file(), b"{\"hello\": \"mine\"}").unwrap();
    let before = fs::read(f.file()).unwrap();
    let pid = f.pid.clone();
    assert!(matches!(
        f.lib.read_references(&pid),
        Err(LibError::Unreadable { .. })
    ));
    let d = f.draft(vec![entry(ATTN, 0)]);
    assert!(matches!(
        f.lib.write_references(&pid, d),
        Err(LibError::Unreadable { .. })
    ));
    let tok = VersionToken(paperreader_core::jsonio::sha256_hex(&before));
    assert!(matches!(
        f.lib
            .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &tok),
        Err(LibError::Unreadable { .. })
    ));
    assert!(matches!(
        f.lib.remove_link_judgments(&pid, &["j-x".into()], &tok),
        Err(LibError::Unreadable { .. })
    ));
    assert_eq!(fs::read(f.file()).unwrap(), before);
    let problems = f.lib.check(CheckLevel::Full).unwrap();
    let kinds: Vec<ProblemKind> = problems.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, vec![ProblemKind::UnreadableReferencesFile]);
    assert!(problems[0].path.ends_with("/references.json"));
    assert_eq!(problems[0].paper.as_deref(), Some(pid.as_str()));
}

#[test]
fn file_over_nine_megabytes_is_unreadable() {
    let mut f = fx();
    f.write(vec![entry(ATTN, 0)]);
    let mut text = fs::read_to_string(f.file()).unwrap();
    text.push_str(&" ".repeat(MAX_FILE_BYTES as usize));
    fs::write(f.file(), text).unwrap();
    assert!(matches!(
        f.lib.read_references(&f.pid),
        Err(LibError::Unreadable { .. })
    ));
    let d = f.draft(vec![entry(DETR, 0)]);
    let pid = f.pid.clone();
    assert!(matches!(
        f.lib.write_references(&pid, d),
        Err(LibError::Unreadable { .. })
    ));
    let kinds: Vec<ProblemKind> = f
        .lib
        .check(CheckLevel::Quick)
        .unwrap()
        .iter()
        .map(|p| p.kind)
        .collect();
    assert_eq!(kinds, vec![ProblemKind::UnreadableReferencesFile]);
}

#[test]
fn newer_format_is_read_but_never_written() {
    let mut f = fx();
    let other = add_other(&mut f);
    f.write(vec![entry(ATTN, 0)]);
    let mut raw = f.raw();
    raw["format"] = json!(2);
    f.put_raw(&raw);
    let before = fs::read(f.file()).unwrap();
    let (doc, tok) = f.lib.read_references(&f.pid).unwrap().unwrap();
    assert_eq!(doc.format, 2);
    let pid = f.pid.clone();
    let d = f.draft(vec![entry(DETR, 0)]);
    assert!(matches!(
        f.lib.write_references(&pid, d),
        Err(LibError::FormatTooNew { found: 2 })
    ));
    assert!(matches!(
        f.lib
            .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &tok),
        Err(LibError::FormatTooNew { found: 2 })
    ));
    assert!(matches!(
        f.lib.remove_link_judgments(&pid, &["j-x".into()], &tok),
        Err(LibError::FormatTooNew { found: 2 })
    ));
    assert_eq!(fs::read(f.file()).unwrap(), before);
    assert!(f.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn check_knows_the_file() {
    let mut f = fx();
    f.write(vec![entry(ATTN, 0)]);
    // 不列成清單沒列的檔，也不列成大小不符（不是材料）
    assert!(f.lib.check(CheckLevel::Full).unwrap().is_empty());
}

// ---------- 7. 不認得的欄位經過加、刪判斷 ----------

#[test]
fn unknown_fields_survive_adding_and_removing_judgments() {
    let mut f = fx();
    let other = add_other(&mut f);
    f.write(vec![numbered(1, ATTN)]);
    let pid = f.pid.clone();
    let (j, _) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Reject, &other, &f.token())
        .unwrap();
    let mut raw = f.raw();
    raw["top"] = json!(1);
    raw["entries"][0]["e"] = json!(2);
    raw["extraction"]["x"] = json!(3);
    raw["judgments"][0]["k"] = json!(4);
    f.put_raw(&raw);
    let (j2, _) = f
        .lib
        .add_link_judgment(&pid, 0, JudgmentKind::Manual, &other, &f.token())
        .unwrap();
    f.lib
        .remove_link_judgments(&pid, &[j2.id], &f.token())
        .unwrap();
    let raw = f.raw();
    assert_eq!(raw["top"], json!(1));
    assert_eq!(raw["entries"][0]["e"], json!(2));
    assert_eq!(raw["extraction"]["x"], json!(3));
    assert_eq!(raw["judgments"][0]["k"], json!(4));
    assert_eq!(raw["judgments"][0]["id"], json!(j.id));
}

// ---------- 8. 自動補書目 ----------

fn bib_of(lib: &LocalLibrary, pid: &str) -> paperreader_core::format::Bib {
    lib.get_paper(pid).unwrap().0.bib
}

#[test]
fn fill_bib_blanks_only_fills_empty_fields_and_keeps_the_time() {
    let mut f = fx();
    let pid = f.pid.clone();
    let before = bib_of(&f.lib, &pid);
    assert_eq!(before.year, None);
    let fill = BibFill {
        year: Some(2023),
        doi: Some("https://doi.org/10.48550/ARXIV.2304.08069".into()),
        url: Some("https://arxiv.org/abs/2304.08069".into()),
    };
    assert!(f.lib.fill_bib_blanks(&pid, fill.clone()).unwrap());
    let after = bib_of(&f.lib, &pid);
    assert_eq!(after.year, Some(2023));
    assert_eq!(after.doi.as_deref(), Some("10.48550/arxiv.2304.08069"));
    assert_eq!(
        after.url.as_deref(),
        Some("https://arxiv.org/abs/2304.08069")
    );
    assert_eq!(after.modified, before.modified);
    assert_eq!(after.modified_by, before.modified_by);
    assert_eq!(after.title, before.title);
    // 已有值的不動；什麼都沒補到交回 false、不寫
    let path = f
        .root()
        .join("papers")
        .join(f.lib.list_papers()[0].folder.clone())
        .join("paper.json");
    let bytes = fs::read(&path).unwrap();
    let other = BibFill {
        year: Some(1999),
        doi: Some("10.1/other".into()),
        url: Some("https://example.org".into()),
    };
    assert!(!f.lib.fill_bib_blanks(&pid, other).unwrap());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!f.lib.fill_bib_blanks(&pid, BibFill::default()).unwrap());
}

#[test]
fn fill_bib_blanks_treats_empty_strings_as_blank() {
    let mut f = fx();
    let pid = f.pid.clone();
    let (p, tok) = f.lib.get_paper(&pid).unwrap();
    // 外部工具寫的空字串（應用自己的改書目不留空字串的 DOI）
    let path = f
        .root()
        .join("papers")
        .join(f.lib.list_papers()[0].folder.clone())
        .join("paper.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["bib"]["url"] = json!("");
    v["bib"]["doi"] = json!("");
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let _ = (p, tok);
    assert!(
        f.lib
            .fill_bib_blanks(
                &pid,
                BibFill {
                    url: Some("https://arxiv.org/abs/2304.08069".into()),
                    doi: Some("10.1/x".into()),
                    ..Default::default()
                }
            )
            .unwrap()
    );
    let b = bib_of(&f.lib, &pid);
    assert_eq!(b.url.as_deref(), Some("https://arxiv.org/abs/2304.08069"));
    assert_eq!(b.doi.as_deref(), Some("10.1/x"));
}

#[test]
fn fill_bib_blanks_checks_its_values() {
    let mut f = fx();
    let pid = f.pid.clone();
    for fill in [
        BibFill {
            year: Some(999),
            ..Default::default()
        },
        BibFill {
            doi: Some("nonsense".into()),
            ..Default::default()
        },
        BibFill {
            url: Some("ftp://example.org/x".into()),
            ..Default::default()
        },
    ] {
        invalid(f.lib.fill_bib_blanks(&pid, fill));
    }
    assert!(matches!(
        f.lib
            .fill_bib_blanks("01JCK3W2A2B3C4D5E6F7G8H9ZZ", BibFill::default()),
        Err(LibError::NotFound { .. })
    ));
}

/// 使用者清空的欄位記在 `cleared`，自動補不補回；之後填了值就拿掉。
#[test]
fn cleared_fields_are_not_filled_back() {
    let mut f = fx();
    let pid = f.pid.clone();
    f.lib
        .fill_bib_blanks(
            &pid,
            BibFill {
                year: Some(2023),
                doi: Some("10.1/wrong".into()),
                url: None,
            },
        )
        .unwrap();
    // 使用者刪掉錯的 DOI
    let (p, tok) = f.lib.get_paper(&pid).unwrap();
    let mut d = draft_of(&p.bib);
    d.doi = None;
    f.lib.update_bib(&pid, d, &tok).unwrap();
    let b = bib_of(&f.lib, &pid);
    assert_eq!(b.doi, None);
    assert_eq!(b.cleared, vec!["doi".to_string()]);
    assert!(
        !f.lib
            .fill_bib_blanks(
                &pid,
                BibFill {
                    doi: Some("10.1/wrong".into()),
                    ..Default::default()
                }
            )
            .unwrap()
    );
    assert_eq!(bib_of(&f.lib, &pid).doi, None);
    // 改書目時沒清空的欄位不記；填了值就拿掉
    let (p, tok) = f.lib.get_paper(&pid).unwrap();
    let mut d = draft_of(&p.bib);
    d.doi = Some("10.1/right".into());
    d.title = Some("Renamed".into());
    f.lib.update_bib(&pid, d, &tok).unwrap();
    let b = bib_of(&f.lib, &pid);
    assert!(b.cleared.is_empty());
    assert_eq!(b.doi.as_deref(), Some("10.1/right"));
    // 篇名清空不記（自動補不補篇名）
    let (p, tok) = f.lib.get_paper(&pid).unwrap();
    let mut d = draft_of(&p.bib);
    d.title = None;
    d.year = None;
    f.lib.update_bib(&pid, d, &tok).unwrap();
    assert_eq!(bib_of(&f.lib, &pid).cleared, vec!["year".to_string()]);
}

#[test]
fn bib_without_cleared_is_written_as_before() {
    let mut f = fx();
    let pid = f.pid.clone();
    let (p, tok) = f.lib.get_paper(&pid).unwrap();
    f.lib.update_bib(&pid, draft_of(&p.bib), &tok).unwrap();
    let path = f
        .root()
        .join("papers")
        .join(f.lib.list_papers()[0].folder.clone())
        .join("paper.json");
    let v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(v["bib"].get("cleared").is_none());
}

fn draft_of(b: &paperreader_core::format::Bib) -> BibDraft {
    BibDraft {
        title: b.title.clone(),
        authors: b.authors.clone(),
        year: b.year,
        venue: b.venue.clone(),
        doi: b.doi.clone(),
        url: b.url.clone(),
    }
}

// ---------- 12. 跟著那篇走 ----------

#[test]
fn judgments_survive_trash_restore_and_backup() {
    let mut f = fx();
    let (ja, jb) = with_two_judgments(&mut f);
    let pid = f.pid.clone();
    let ids = |lib: &LocalLibrary| -> Vec<String> {
        lib.read_references(&pid)
            .unwrap()
            .unwrap()
            .0
            .judgments
            .into_iter()
            .map(|j| j.id)
            .collect()
    };
    f.lib.remove_paper(&pid).unwrap();
    assert!(matches!(
        f.lib.read_references(&pid),
        Err(LibError::NotFound { .. })
    ));
    f.lib.restore_paper(&pid).unwrap();
    assert_eq!(ids(&f.lib), vec![ja.clone(), jb.clone()]);
    // 備份檔還原到別處
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    f.lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let to = out.path().join("restored");
    paperreader_core::backup::restore_backup(&zip, &to, DEVICE_B).unwrap();
    let lib2 = LocalLibrary::open(&to, DEVICE_B).unwrap();
    assert_eq!(ids(&lib2), vec![ja, jb]);
    assert!(lib2.check(CheckLevel::Full).unwrap().is_empty());
}

// ---------- 13. 目錄摘要 ----------

#[test]
fn paper_digests_show_bib_and_references_changes() {
    let mut f = fx();
    let other = add_other(&mut f);
    let d0 = f.lib.paper_digests();
    assert_eq!(d0.len(), 2);
    let of = |d: &[paperreader_core::store::PaperDigest], id: &str| {
        d.iter().find(|x| x.id == id).cloned().unwrap()
    };
    assert!(of(&d0, &f.pid).references.is_none());
    // 書目改了
    f.lib
        .fill_bib_blanks(
            &f.pid.clone(),
            BibFill {
                year: Some(2020),
                ..Default::default()
            },
        )
        .unwrap();
    let d1 = f.lib.paper_digests();
    assert_ne!(of(&d1, &f.pid).bib_sha256, of(&d0, &f.pid).bib_sha256);
    assert_eq!(of(&d1, &other), of(&d0, &other));
    // references.json 寫了
    f.write(vec![numbered(1, ATTN)]);
    let d2 = f.lib.paper_digests();
    let s2 = of(&d2, &f.pid).references.unwrap();
    assert!(s2.bytes > 0);
    // 在應用外改（大小變了）
    let mut raw = f.raw();
    raw["outside"] = json!("edit");
    f.put_raw(&raw);
    let d3 = f.lib.paper_digests();
    assert_ne!(of(&d3, &f.pid).references, Some(s2));
    // 移到回收區：不在摘要裡
    f.lib.remove_paper(&other).unwrap();
    let d4 = f.lib.paper_digests();
    assert!(d4.iter().all(|d| d.id != other));
    assert_eq!(d4.len(), 1);
}

// ---------- 14. 正規化與找回鍵 ----------

#[test]
fn fold_text_rules() {
    // NFKC：連字、全形
    assert_eq!(fold_text("ﬁnding ＡＢＣ"), "finding abc");
    // 軟連字號、斷行的連字號（後面接小寫才接回）
    assert_eq!(fold_text("detec\u{00AD}tion"), "detection");
    assert_eq!(fold_text("detec- tion"), "detection");
    assert_eq!(fold_text("detec-\ntion"), "detection");
    assert_eq!(fold_text("detec\u{2010} tion"), "detection");
    assert_eq!(fold_text("real-time"), "real-time");
    assert_eq!(fold_text("Bayes- Net"), "bayes- net");
    // 小寫、去重音、空白
    assert_eq!(
        fold_text("  Galvão   Rodríguez\tÉcole  "),
        "galvao rodriguez ecole"
    );
    assert_eq!(fold_text("Ångström"), "angstrom");
}

#[test]
fn entry_key_v1_is_stable() {
    // 這幾組是第 1 版的定值：判斷存著這個鍵，改了規則就要升版本（舊版的函式留著）
    let cases = [
        (
            "[12] Y. Zhao, W. Lv, S. Xu, et al. DETRs beat YOLOs on real-time object detection. In CVPR, 2024.",
            "yzhaowlvsxuetaldetrsbeatyolosonrealtimeobjectdetectionincvpr2024",
        ),
        (
            "12. Y. Zhao, W. Lv, S. Xu, et al. DETRs beat YOLOs on real- time object detection. In CVPR, 2024.",
            "yzhaowlvsxuetaldetrsbeatyolosonrealtimeobjectdetectionincvpr2024",
        ),
        ("(3) Müller, K. Über ﬂows. 2019.", "mullerkuberflows2019"),
        ("3) A.  B.", "ab"),
        ("2019. Smith", "smith"),
        ("Smith, J. (2019). Title", "smithj2019title"),
        ("[123456] X", "123456x"),
        ("注意力機制 [1]", "注意力機制1"),
    ];
    for (text, key) in cases {
        assert_eq!(entry_key(text, 1).as_deref(), Some(key), "{text}");
    }
    assert_eq!(ENTRY_KEY_VERSION, 1);
    assert_eq!(entry_key("x", 2), None);
    assert_eq!(entry_key("x", 0), None);
}

// ---------- 15. 判斷的套用 ----------

fn re(text: &str) -> RefEntry {
    RefEntry {
        number: None,
        label: None,
        text: text.into(),
        page: 0,
        bbox: None,
        doi: None,
        arxiv: None,
        year: None,
        extra: Default::default(),
    }
}

fn jd(id: &str, kind: &str, paper: &str, text: &str, created: &str, by: &str) -> Judgment {
    Judgment {
        id: id.into(),
        kind: kind.into(),
        paper: paper.into(),
        entry_key: entry_key(text, 1).unwrap(),
        key_version: 1,
        entry_text: text.into(),
        entry_doi: None,
        entry_arxiv: None,
        created: created.into(),
        created_by: by.into(),
        extra: Default::default(),
    }
}

const T1: &str = "2026-11-17T08:00:00.000Z";
const T2: &str = "2026-11-17T09:00:00.000Z";
const T3: &str = "2026-11-17T10:00:00.000Z";

#[test]
fn judgments_still_apply_after_reextraction_with_other_spacing() {
    let entries = vec![
        re("[4] A. Vaswani et al. Atten- tion is all  you need.\nIn NeurIPS, 2017."),
        re(DETR),
    ];
    let js = vec![jd("j-1", "manual", "P1", ATTN, T1, DEVICE)];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, Some(0));
    assert!(r.lost.is_empty());
}

#[test]
fn doi_and_arxiv_find_the_entry_when_the_key_does_not() {
    let mut a = re("Totally rewritten text one");
    a.doi = Some("10.1/abc".into());
    let mut b = re("Totally rewritten text two");
    b.arxiv = Some("2304.08069".into());
    let entries = vec![a, b];
    let mut j1 = jd("j-1", "reject", "P1", "old text one", T1, DEVICE);
    j1.entry_doi = Some("10.1/ABC".into());
    let mut j2 = jd("j-2", "manual", "P2", "old text two", T1, DEVICE);
    j2.entry_arxiv = Some("2304.08069".into());
    let j3 = jd("j-3", "manual", "P3", "nothing like it", T1, DEVICE);
    let r = resolve_judgments(&entries, &[j1, j2, j3]);
    assert_eq!(r.entries[0].rejected, vec![0]);
    assert_eq!(r.entries[1].link, Some(1));
    assert_eq!(r.lost, vec![2]);
}

#[test]
fn latest_judgment_per_target_wins() {
    let entries = vec![re(ATTN)];
    // 同一個對象：先連到、後否決 → 否決
    let js = vec![
        jd("j-1", "manual", "A", ATTN, T1, DEVICE),
        jd("j-2", "reject", "A", ATTN, T2, DEVICE),
    ];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, None);
    assert_eq!(r.entries[0].rejected, vec![1]);
    // 先否決、後確認 → 連到
    let js = vec![
        jd("j-1", "reject", "A", ATTN, T1, DEVICE),
        jd("j-2", "confirm", "A", ATTN, T2, DEVICE),
    ];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, Some(1));
    assert!(r.entries[0].rejected.is_empty());
}

#[test]
fn two_devices_each_pick_a_paper_the_later_wins() {
    let entries = vec![re(ATTN)];
    let js = vec![
        jd("j-2", "manual", "B", ATTN, T2, DEVICE_B),
        jd("j-1", "manual", "A", ATTN, T2, DEVICE),
    ];
    // 時間相同比裝置代號：DEVICE_B（01JD…）大於 DEVICE（01JC…）
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, Some(0));
    let js = vec![
        jd("j-1", "manual", "A", ATTN, T3, DEVICE),
        jd("j-2", "manual", "B", ATTN, T2, DEVICE_B),
    ];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, Some(0));
    assert_eq!(r.entries[0].by_paper, vec![0, 1]);
}

/// 時間與裝置都相同（在應用以外抄過的判斷）：代號大的算較晚，不看在陣列裡的位置，所以兩台的順序不同也算出同一個結果。
#[test]
fn exact_same_stamp_larger_id_wins_in_any_order() {
    let entries = vec![re(ATTN)];
    let reject = jd("j-1", "reject", "A", ATTN, T1, DEVICE);
    let manual = jd("j-2", "manual", "A", ATTN, T1, DEVICE);
    for js in [
        vec![reject.clone(), manual.clone()],
        vec![manual.clone(), reject.clone()],
    ] {
        let r = resolve_judgments(&entries, &js);
        let link = r.entries[0].link.map(|i| js[i].id.as_str());
        assert_eq!(link, Some("j-2"), "{js:?}");
        assert!(r.entries[0].rejected.is_empty());
    }
    // 反過來：代號大的是否決
    let manual = jd("j-1", "manual", "A", ATTN, T1, DEVICE);
    let reject = jd("j-2", "reject", "A", ATTN, T1, DEVICE);
    for js in [
        vec![reject.clone(), manual.clone()],
        vec![manual.clone(), reject.clone()],
    ] {
        let r = resolve_judgments(&entries, &js);
        assert_eq!(r.entries[0].link, None, "{js:?}");
        let rejected: Vec<&str> = r.entries[0]
            .rejected
            .iter()
            .map(|&i| js[i].id.as_str())
            .collect();
        assert_eq!(rejected, ["j-2"]);
    }
    // 兩篇各連一項、時間裝置都相同：連到代號大的那一項
    let a = jd("j-1", "manual", "A", ATTN, T1, DEVICE);
    let b = jd("j-2", "manual", "B", ATTN, T1, DEVICE);
    for js in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
        let r = resolve_judgments(&entries, &js);
        assert_eq!(r.entries[0].link.map(|i| js[i].paper.as_str()), Some("B"));
    }
}

#[test]
fn rejecting_a_then_c_does_not_bring_a_back() {
    let entries = vec![re(ATTN)];
    let js = vec![
        jd("j-1", "reject", "A", ATTN, T1, DEVICE),
        jd("j-2", "reject", "C", ATTN, T2, DEVICE),
    ];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, None);
    assert_eq!(r.entries[0].rejected, vec![0, 1]);
}

#[test]
fn unknown_kinds_are_neither_applied_nor_lost() {
    let entries = vec![re(ATTN)];
    let js = vec![
        jd("j-1", "pin", "A", ATTN, T1, DEVICE),
        jd("j-2", "pin", "A", "gone", T1, DEVICE),
    ];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0], ResolvedEntry::default());
    assert!(r.lost.is_empty());
}

#[test]
fn a_judgment_applies_to_every_duplicate_entry() {
    let entries = vec![re(ATTN), re(DETR), re(ATTN)];
    let js = vec![jd("j-1", "manual", "A", ATTN, T1, DEVICE)];
    let r = resolve_judgments(&entries, &js);
    assert_eq!(r.entries[0].link, Some(0));
    assert_eq!(r.entries[1].link, None);
    assert_eq!(r.entries[2].link, Some(0));
}
