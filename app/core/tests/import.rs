//! 整批匯入與匯入清單。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::{Bib, BibDraft, PaperFile};
use paperreader_core::import::*;
use paperreader_core::local::{AddOutcome, LocalLibrary};
use paperreader_core::store::CheckLevel;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// ---- 輔助 ----

/// 要匯入的資料夾。
struct Src {
    dir: TempDir,
}

impl Src {
    fn new() -> Src {
        Src {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let p = self.root().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    /// 假 PDF（`%PDF-` 開頭、3000 個位元組），交回內容。
    fn pdf(&self, rel: &str, seed: u64) -> Vec<u8> {
        let b = fake_bytes(seed, 3000);
        self.write(rel, &b);
        b
    }

    fn md(&self, rel: &str, text: &str) {
        self.write(rel, text.as_bytes());
    }

    fn manifest(&self, papers: Value) {
        self.manifest_value(&json!({"format": 1, "kind": "paperreader-import", "papers": papers}));
    }

    fn manifest_value(&self, v: &Value) {
        self.write(
            MANIFEST_NAME,
            serde_json::to_string_pretty(v).unwrap().as_bytes(),
        );
    }

    fn folder(&self) -> ImportContainer {
        ImportContainer::Folder(self.root().to_path_buf())
    }

    /// 把整個資料夾壓成 zip（Deflate，資料夾也有自己的項目，像一般的工具做的）。
    fn zip(&self, out: &TempDir) -> ImportContainer {
        let path = out.path().join("import.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut items = Vec::new();
        walk(self.root(), self.root(), &mut items);
        items.sort();
        for (rel, is_dir) in items {
            if is_dir {
                z.add_directory(format!("{rel}/"), opts).unwrap();
            } else {
                z.start_file(rel.as_str(), opts).unwrap();
                z.write_all(&fs::read(self.root().join(&rel)).unwrap())
                    .unwrap();
            }
        }
        z.finish().unwrap();
        ImportContainer::Zip(path)
    }
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, bool)>) {
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let rel = p
            .strip_prefix(base)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let is_dir = e.file_type().unwrap().is_dir();
        out.push((rel, is_dir));
        if is_dir {
            walk(base, &p, out);
        }
    }
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn src_pdf(path: &str) -> Value {
    json!({"path": path, "role": "source", "form": "pdf"})
}

fn file(path: &str, role: &str, form: &str) -> Value {
    json!({"path": path, "role": role, "form": form})
}

/// 只有一個原文的一篇。
fn one(path: &str) -> Value {
    json!({"files": [src_pdf(path)]})
}

fn scan(c: &ImportContainer) -> ImportScan {
    scan_import(c, &mut |_, _| true)
        .unwrap()
        .expect("not cancelled")
}

fn scan_err(c: &ImportContainer) -> LibError {
    scan_import(c, &mut |_, _| true).expect_err("expected an error")
}

fn assert_unreadable(c: &ImportContainer) -> String {
    match scan_err(c) {
        LibError::Unreadable { path, reason } => {
            assert_eq!(path, c.path().display().to_string());
            reason
        }
        other => panic!("expected Unreadable, got {other:?}"),
    }
}

fn preview(lib: &mut LocalLibrary, c: &ImportContainer) -> ImportPreview {
    let s = scan(c);
    lib.prepare_import(s).unwrap()
}

/// 一路寫完，交回每一步的結果。
fn steps(lib: &mut LocalLibrary, id: &str) -> Vec<ImportStep> {
    let mut out = Vec::new();
    loop {
        let s = lib.import_step(id).unwrap();
        let done = matches!(s, ImportStep::Finished(_));
        out.push(s);
        if done {
            return out;
        }
        assert!(out.len() < 100_000);
    }
}

fn run(lib: &mut LocalLibrary, id: &str) -> ImportReport {
    match steps(lib, id).pop().unwrap() {
        ImportStep::Finished(r) => r,
        other => panic!("expected Finished, got {other:?}"),
    }
}

fn import(lib: &mut LocalLibrary, c: &ImportContainer) -> (ImportPreview, ImportReport) {
    let p = preview(lib, c);
    let r = run(lib, &p.preview_id);
    (p, r)
}

fn skip(p: &ImportPreview, index: u32) -> (String, Option<String>) {
    let s = p
        .skip
        .iter()
        .find(|s| s.index == index)
        .unwrap_or_else(|| panic!("paper {index} is not skipped: {p:?}"));
    (s.reason.clone(), s.detail.clone())
}

fn accepted(p: &ImportPreview) -> Vec<u32> {
    p.accept.iter().map(|a| a.index).collect()
}

fn reason(code: &str, detail: Option<&str>) -> (String, Option<String>) {
    (code.to_string(), detail.map(String::from))
}

fn be(detail: &str) -> (String, Option<String>) {
    reason("bad-entry", Some(detail))
}

fn added_index(r: &ImportReport) -> Vec<u32> {
    r.added.iter().map(|(i, _)| *i).collect()
}

fn id_for(r: &ImportReport, index: u32) -> String {
    r.added
        .iter()
        .find(|(i, _)| *i == index)
        .unwrap_or_else(|| panic!("paper {index} was not added: {r:?}"))
        .1
        .clone()
}

fn paper(lib: &LocalLibrary, id: &str) -> PaperFile {
    lib.get_paper(id).unwrap().0
}

fn folder_of(root: &Path, lib: &LocalLibrary, id: &str) -> PathBuf {
    let s = lib.list_papers().into_iter().find(|p| p.id == id).unwrap();
    root.join("papers").join(s.folder)
}

/// `papers/` 底下以 `.pr-` 開頭的項目。
fn temp_items(root: &Path) -> Vec<String> {
    fs::read_dir(root.join("papers"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".pr-"))
        .collect()
}

/// 一個有 `a.pdf`、`t.pdf`、`t.md` 的資料夾；清單只有 `entry` 一篇。交回那一篇的略過原因。
fn bad(entry: Value) -> (String, Option<String>) {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("t.pdf", 2);
    s.md("t.md", "# t\n");
    s.manifest(json!([entry]));
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    assert!(p.accept.is_empty(), "{p:?}");
    skip(&p, 0)
}

/// 每篇一個不同的原文（`p0.pdf` …），書目依序是 `csls`；匯入之後交回每篇的書目（依序號）。
fn import_bibs(csls: Vec<Value>) -> Vec<Bib> {
    let s = Src::new();
    let mut papers = Vec::new();
    for (k, c) in csls.into_iter().enumerate() {
        let name = format!("p{k}.pdf");
        s.pdf(&name, 100 + k as u64);
        papers.push(json!({"csl": c, "files": [src_pdf(&name)]}));
    }
    let n = papers.len();
    s.manifest(Value::Array(papers));
    let (_d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    assert_eq!(r.added.len(), n, "{r:?}");
    (0..n as u32)
        .map(|i| paper(&lib, &id_for(&r, i)).bib)
        .collect()
}

fn preview_of(s: &Src) -> ImportPreview {
    let (_d, mut lib) = temp_library();
    preview(&mut lib, &s.folder())
}

/// 最小的一份重點筆記（屬性區有 `paperreader_digest`）。
fn digest_text(title: &str) -> String {
    format!("---\npaperreader_digest: 1\n---\n\n# {title}\n")
}

/// 屬性區寫了 `paperreader_proofread` 的重點筆記。
fn digest_proofread(title: &str, proofread: bool) -> String {
    format!("---\npaperreader_digest: 1\npaperreader_proofread: {proofread}\n---\n\n# {title}\n")
}

// ---- 1. 整份 ----

#[test]
fn manifest_missing_unreadable() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    assert_unreadable(&s.folder());
    // 資料夾本身不存在
    assert_unreadable(&ImportContainer::Folder(s.root().join("nope")));
    // 名稱只差大小寫：找清單用名稱，大小寫要相同
    s.write(
        "PaperReader-Import.json",
        json!({"format": 1, "kind": "paperreader-import", "papers": [one("a.pdf")]})
            .to_string()
            .as_bytes(),
    );
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_not_json_unreadable() {
    let s = Src::new();
    s.write(MANIFEST_NAME, b"{\"format\": 1,");
    assert_unreadable(&s.folder());
    s.write(MANIFEST_NAME, b"[1, 2]");
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_wrong_kind_unreadable() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest_value(&json!({"format": 1, "kind": "paperreader-backup", "papers": [one("a.pdf")]}));
    let r = assert_unreadable(&s.folder());
    assert!(r.contains("not an import manifest"), "{r}");
    s.manifest_value(&json!({"format": 1, "papers": [one("a.pdf")]}));
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_format_missing_unreadable() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    for f in [Value::Null, json!("1"), json!(1.5)] {
        s.manifest_value(
            &json!({"format": f, "kind": "paperreader-import", "papers": [one("a.pdf")]}),
        );
        assert_unreadable(&s.folder());
    }
    s.manifest_value(&json!({"kind": "paperreader-import", "papers": [one("a.pdf")]}));
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_format_too_new() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest_value(&json!({"format": 2, "kind": "paperreader-import", "papers": [one("a.pdf")]}));
    assert!(matches!(
        scan_err(&s.folder()),
        LibError::FormatTooNew { found: 2 }
    ));
}

#[test]
fn manifest_papers_empty_unreadable() {
    let s = Src::new();
    for papers in [json!([]), json!({}), Value::Null] {
        s.manifest(papers);
        assert_unreadable(&s.folder());
    }
    s.manifest_value(&json!({"format": 1, "kind": "paperreader-import"}));
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_papers_over_limit_unreadable() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(Value::Array(vec![one("a.pdf"); 10_001]));
    assert_unreadable(&s.folder());
    // 正好一萬篇照收（同一個路徑：第一篇以後都是 duplicate-path）
    s.manifest(Value::Array(vec![one("a.pdf"); 10_000]));
    let p = preview_of(&s);
    assert_eq!(p.total, 10_000);
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(skip(&p, 9_999), reason("duplicate-path", Some("a.pdf")));
}

#[test]
fn manifest_over_16mb_unreadable() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    let limit = 16 * 1024 * 1024;
    let body =
        json!({"format": 1, "kind": "paperreader-import", "papers": [one("a.pdf")]}).to_string();
    let mut exact = body.clone().into_bytes();
    exact.resize(limit, b' ');
    s.write(MANIFEST_NAME, &exact);
    assert_eq!(accepted(&preview_of(&s)), vec![0]);
    exact.push(b' ');
    s.write(MANIFEST_NAME, &exact);
    assert_unreadable(&s.folder());
}

#[test]
fn manifest_with_bom_ok() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(
        json!({"format": 1, "kind": "paperreader-import", "papers": [one("a.pdf")]})
            .to_string()
            .as_bytes(),
    );
    s.write(MANIFEST_NAME, &bytes);
    let p = preview_of(&s);
    assert_eq!(p.total, 1);
    assert_eq!(accepted(&p), vec![0]);
}

// ---- 2–6. 每篇與每個檔的欄位 ----

#[test]
fn entry_not_object_bad_entry() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!(["x", one("a.pdf"), null]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), be("papers[0]"));
    assert_eq!(skip(&p, 2), be("papers[2]"));
    assert_eq!(accepted(&p), vec![1]);
}

#[test]
fn files_not_array_bad_entry() {
    assert_eq!(bad(json!({"files": "a.pdf"})), be("files"));
    assert_eq!(bad(json!({"files": []})), be("files"));
    assert_eq!(bad(json!({"files": null})), be("files"));
    assert_eq!(bad(json!({})), be("files"));
    assert_eq!(bad(json!({"files": ["a.pdf"]})), be("files[0]"));
}

#[test]
fn path_missing_bad_entry() {
    assert_eq!(
        bad(json!({"files": [{"role": "source", "form": "pdf"}]})),
        be("files[0].path")
    );
    assert_eq!(
        bad(json!({"files": [{"path": null, "role": "source", "form": "pdf"}]})),
        be("files[0].path")
    );
    assert_eq!(
        bad(json!({"files": [{"path": 5, "role": "source", "form": "pdf"}]})),
        be("files[0].path")
    );
    assert_eq!(
        bad(json!({"files": [{"path": "a.pdf", "form": "pdf"}]})),
        be("files[0].role")
    );
    assert_eq!(
        bad(json!({"files": [{"path": "a.pdf", "role": "source"}]})),
        be("files[0].form")
    );
}

#[test]
fn aliases_not_array_bad_entry() {
    assert_eq!(
        bad(json!({"aliases": "L001", "files": [src_pdf("a.pdf")]})),
        be("aliases")
    );
    assert_eq!(
        bad(json!({"aliases": ["L001", 3], "files": [src_pdf("a.pdf")]})),
        be("aliases[1]")
    );
}

#[test]
fn page_map_unknown_bad_entry() {
    for v in [json!("foo"), json!(1)] {
        let mut t = file("t.pdf", "translation", "pdf");
        t["page_map"] = v;
        assert_eq!(
            bad(json!({"files": [src_pdf("a.pdf"), t]})),
            be("files[1].page_map")
        );
    }
}

#[test]
fn proofread_not_bool_bad_entry() {
    let mut t = file("t.pdf", "translation", "pdf");
    t["proofread"] = json!("yes");
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), t]})),
        be("files[1].proofread")
    );
}

#[test]
fn sha256_malformed_bad_entry() {
    for v in [
        json!("abc"),
        json!("g".repeat(64)),
        json!("a".repeat(65)),
        json!(12),
    ] {
        let mut a = src_pdf("a.pdf");
        a["sha256"] = v;
        assert_eq!(bad(json!({"files": [a]})), be("files[0].sha256"));
    }
}

#[test]
fn csl_not_object_bad_entry() {
    assert_eq!(
        bad(json!({"csl": [], "files": [src_pdf("a.pdf")]})),
        be("csl")
    );
    assert_eq!(
        bad(json!({"csl": "x", "files": [src_pdf("a.pdf")]})),
        be("csl")
    );
}

#[test]
fn string_over_4kib_bad_entry() {
    let long = "x".repeat(4097);
    assert_eq!(
        bad(json!({"short_name": long, "files": [src_pdf("a.pdf")]})),
        be("short_name")
    );
    assert_eq!(
        bad(json!({"aliases": [long], "files": [src_pdf("a.pdf")]})),
        be("aliases[0]")
    );
    let mut t = file("t.pdf", "translation", "pdf");
    t["title"] = json!(long);
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), t]})),
        be("files[1].title")
    );
    assert_eq!(
        bad(json!({"csl": {"title": long}, "files": [src_pdf("a.pdf")]})),
        be("csl.title")
    );
    assert_eq!(
        bad(json!({"csl": {"author": [{"family": long}]}, "files": [src_pdf("a.pdf")]})),
        be("csl.author[0].family")
    );

    // 正好 4096 個位元組照收；CSL 裡不讀的欄位不受限制（只記成沒收的欄位）
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!([{
        "short_name": "x".repeat(4096),
        "csl": {"title": "T", "abstract": "y".repeat(5000)},
        "files": [src_pdf("a.pdf")]
    }]));
    let p = preview_of(&s);
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(
        p.accept[0].notices,
        vec![ImportNotice {
            kind: "bib-fields-dropped".into(),
            detail: Some("abstract".into())
        }]
    );
}

#[test]
fn one_bad_entry_others_accepted() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.manifest(json!([one("a.pdf"), {"files": 3}, one("b.pdf")]));
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0, 2]);
    assert_eq!(skip(&p, 1), be("files"));
    assert_eq!(added_index(&r), vec![0, 2]);
    assert_eq!(r.skipped, vec![p.skip[0].clone()]);
    assert_eq!(lib.list_papers().len(), 2);
}

#[test]
fn null_equals_absent_for_optional_fields() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.md("t.md", "text");
    s.manifest(json!([{
        "csl": null,
        "short_name": null,
        "aliases": null,
        "files": [
            {"path": "a.pdf", "role": "source", "form": "pdf",
             "title": null, "page_map": null, "proofread": null, "sha256": null},
            {"path": "t.md", "role": "translation", "form": "markdown", "title": null}
        ]
    }, {
        "csl": {"title": null, "DOI": null, "volume": null},
        "files": [src_pdf("a2.pdf")]
    }]));
    s.pdf("a2.pdf", 2);
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0, 1]);
    assert!(p.accept.iter().all(|a| a.notices.is_empty()), "{p:?}");
    let pf = paper(&lib, &id_for(&r, 0));
    assert_eq!(pf.short_name, None);
    assert!(pf.aliases.is_empty());
    assert_eq!(pf.bib.title.as_deref(), Some("a"));
    for m in &pf.materials {
        assert_eq!(m.title, None);
        assert_eq!(m.provenance.page_map, None);
        assert_eq!(m.provenance.proofread, None);
    }
    let pf = paper(&lib, &id_for(&r, 1));
    assert_eq!(pf.bib.title.as_deref(), Some("a2"));
    assert_eq!(pf.bib.doi, None);
}

#[test]
fn aliases_trimmed_and_deduped() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!([{
        "aliases": [" L001 ", "L001", "", "   ", "B"],
        "files": [src_pdf("a.pdf")]
    }]));
    let (_d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    assert_eq!(paper(&lib, &id_for(&r, 0)).aliases, vec!["L001", "B"]);
}

#[test]
fn short_name_trimmed() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.manifest(json!([
        {"short_name": "  RT-DETR ", "files": [src_pdf("a.pdf")]},
        {"short_name": "   ", "files": [src_pdf("b.pdf")]}
    ]));
    let (_d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    assert_eq!(
        paper(&lib, &id_for(&r, 0)).short_name.as_deref(),
        Some("RT-DETR")
    );
    assert_eq!(paper(&lib, &id_for(&r, 1)).short_name, None);
}

#[test]
fn too_many_files_skipped() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.pdf("c.pdf", 3);
    for k in 0..32 {
        s.md(&format!("m{k}.md"), &format!("note {k}"));
    }
    let mds = |n: usize| -> Vec<Value> {
        (0..n)
            .map(|k| file(&format!("m{k}.md"), "text", "markdown"))
            .collect()
    };
    let with = |src: &str, n: usize| {
        let mut f = vec![src_pdf(src)];
        f.extend(mds(n));
        json!({"files": f})
    };
    // 33 個：too-many-files；欄位錯的優先（bad-entry）；正好 32 個照收
    let mut worse = with("b.pdf", 32);
    worse["files"][5]["role"] = json!("supplement");
    s.manifest(json!([with("a.pdf", 32), worse, with("c.pdf", 31)]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("too-many-files", None));
    assert_eq!(skip(&p, 1), be("files[5].role"));
    assert_eq!(accepted(&p), vec![2]);
    assert_eq!(p.accept[0].files, 32);
}

// ---- 7–12. CSL ----

#[test]
fn csl_author_family_given_literal() {
    let bibs = import_bibs(vec![json!({
        "title": "T",
        "author": [
            {"family": "Zhao", "given": "Yian", "sequence": "first", "affiliation": [], "non-dropping-particle": "de"},
            {"literal": "RT-DETR Team"},
            {"family": "", "literal": "Yian Zhao"},
            {"given": "Only"},
            "a string",
            {"family": 5},
            {"family": " Wang ", "given": ""},
            {"family": "Li", "given": "Lei", "literal": "ignored"}
        ]
    })]);
    let a = &bibs[0].authors;
    let triple: Vec<(Option<&str>, Option<&str>, Option<&str>)> = a
        .iter()
        .map(|x| {
            (
                x.family.as_deref(),
                x.given.as_deref(),
                x.literal.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        triple,
        vec![
            (Some("Zhao"), Some("Yian"), None),
            (None, None, Some("RT-DETR Team")),
            (None, None, Some("Yian Zhao")),
            (Some("Wang"), None, None),
            (Some("Li"), Some("Lei"), None),
        ]
    );
    assert!(a.iter().all(|x| x.extra.is_empty()));

    // 子欄位不算沒收：沒有提醒
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!([{"csl": {"id": "x", "type": "article", "author": [{"family": "Z", "sequence": "first"}]},
        "files": [src_pdf("a.pdf")]}]));
    assert!(preview_of(&s).accept[0].notices.is_empty());
}

#[test]
fn csl_year_from_date_parts() {
    let bibs = import_bibs(vec![
        json!({"issued": {"date-parts": [[2024, 5, 1]]}}),
        json!({"issued": {"date-parts": [["2023"]]}}),
        json!({"issued": {"date-parts": [[999]]}}),
        json!({"issued": {"date-parts": [[10000]]}}),
        json!({"issued": {"raw": "2024"}}),
        json!({"issued": {"literal": "2024"}}),
        json!({"issued": {"date-parts": [[]]}}),
    ]);
    let years: Vec<Option<i32>> = bibs.iter().map(|b| b.year).collect();
    assert_eq!(
        years,
        vec![Some(2024), Some(2023), None, None, None, None, None]
    );
}

#[test]
fn csl_doi_uses_normalize_doi() {
    let bibs = import_bibs(vec![
        json!({"DOI": "https://doi.org/10.48550/arXiv.2304.08069"}),
        json!({"DOI": "  DOI:10.1145/ABC  "}),
        json!({"DOI": "doi:"}),
        json!({"DOI": "   "}),
    ]);
    let dois: Vec<Option<&str>> = bibs.iter().map(|b| b.doi.as_deref()).collect();
    assert_eq!(
        dois,
        vec![
            Some("10.48550/arxiv.2304.08069"),
            Some("10.1145/abc"),
            None,
            None
        ]
    );
}

#[test]
fn csl_wrong_type_field_dropped_not_fatal() {
    let s = Src::new();
    s.pdf("L001/2304.08069v3.pdf", 1);
    s.manifest(json!([{
        "csl": {"title": 5, "author": "Zhao", "issued": "2024", "container-title": ["x"],
                "DOI": 10, "URL": {}},
        "files": [src_pdf("L001/2304.08069v3.pdf")]
    }]));
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(
        p.accept[0].notices,
        vec![ImportNotice {
            kind: "bib-fields-dropped".into(),
            detail: Some("title, author, issued, container-title, DOI, URL".into())
        }]
    );
    let b = paper(&lib, &id_for(&r, 0)).bib;
    assert_eq!(b.title.as_deref(), Some("2304.08069v3"));
    assert!(b.authors.is_empty());
    assert_eq!((b.year, b.venue, b.doi, b.url), (None, None, None, None));
    assert_eq!(r.notices, 1);

    // date-parts 的年份不是數字
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(
        json!([{"csl": {"issued": {"date-parts": [["20x4"]]}}, "files": [src_pdf("a.pdf")]}]),
    );
    assert_eq!(
        preview_of(&s).accept[0].notices[0].detail.as_deref(),
        Some("issued")
    );
}

#[test]
fn csl_dropped_fields_notice_excludes_id_type() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.pdf("c.pdf", 3);
    let mut many = serde_json::Map::new();
    many.insert("id".into(), json!("x"));
    many.insert("type".into(), json!("article"));
    for k in 0..25 {
        many.insert(format!("k{k:02}"), json!("v"));
    }
    s.manifest(json!([
        {"csl": {"id": "zhao2024", "type": "paper-conference", "title": "T",
                 "volume": "3", "page": "1-10", "note": null},
         "files": [src_pdf("a.pdf")]},
        {"csl": {"id": "x", "type": "article", "title": "T"}, "files": [src_pdf("b.pdf")]},
        {"csl": Value::Object(many), "files": [src_pdf("c.pdf")]}
    ]));
    let p = preview_of(&s);
    assert_eq!(
        p.accept[0].notices,
        vec![ImportNotice {
            kind: "bib-fields-dropped".into(),
            detail: Some("volume, page".into())
        }]
    );
    assert!(p.accept[1].notices.is_empty());
    let twenty: Vec<String> = (0..20).map(|k| format!("k{k:02}")).collect();
    assert_eq!(
        p.accept[2].notices[0].detail.as_deref(),
        Some(twenty.join(", ").as_str())
    );
}

#[test]
fn no_csl_title_from_file_name() {
    let bib = &import_bibs(vec![Value::Null])[0];
    assert_eq!(bib.title.as_deref(), Some("p0"));
    assert!(bib.authors.is_empty());
    assert_eq!(
        (
            bib.year,
            bib.venue.as_deref(),
            bib.doi.as_deref(),
            bib.url.as_deref()
        ),
        (None, None, None, None)
    );
}

#[test]
fn csl_without_title_uses_file_name() {
    let bib = &import_bibs(vec![
        json!({"author": [{"family": "Zhao"}], "issued": {"date-parts": [[2024]]}}),
    ])[0];
    assert_eq!(bib.title.as_deref(), Some("p0"));
    assert_eq!(bib.year, Some(2024));
}

#[test]
fn empty_strings_become_null() {
    let bibs = import_bibs(vec![
        json!({"title": "   ", "container-title": "", "URL": " ", "DOI": ""}),
        json!({"title": "  X ", "container-title": " CVPR 2024 ", "URL": " https://a.b/c "}),
    ]);
    assert_eq!(bibs[0].title.as_deref(), Some("p0"));
    assert_eq!(
        (
            bibs[0].venue.as_deref(),
            bibs[0].url.as_deref(),
            bibs[0].doi.as_deref()
        ),
        (None, None, None)
    );
    assert_eq!(bibs[1].title.as_deref(), Some("X"));
    assert_eq!(bibs[1].venue.as_deref(), Some("CVPR 2024"));
    assert_eq!(bibs[1].url.as_deref(), Some("https://a.b/c"));
}

// ---- 13–20. 角色、形式、路徑 ----

#[test]
fn paper_without_source_skipped() {
    assert_eq!(
        bad(json!({"files": [file("t.pdf", "translation", "pdf")]})),
        reason("no-source", None)
    );
}

#[test]
fn paper_with_two_sources_skipped() {
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), src_pdf("t.pdf")]})),
        reason("multiple-sources", None)
    );
}

#[test]
fn bad_role_form_combination() {
    assert_eq!(
        bad(json!({"files": [file("t.md", "source", "markdown")]})),
        reason("bad-role", Some("files[0]"))
    );
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), file("t.pdf", "digest", "pdf")]})),
        reason("bad-role", Some("files[1]"))
    );
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), file("t.pdf", "text", "pdf")]})),
        reason("bad-role", Some("files[1]"))
    );
    // 不認得的值是 bad-entry
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), file("t.pdf", "supplement", "pdf")]})),
        be("files[1].role")
    );
    assert_eq!(
        bad(json!({"files": [file("a.pdf", "source", "epub")]})),
        be("files[0].form")
    );
    // bad-role 先於 no-source
    assert_eq!(
        bad(json!({"files": [file("t.pdf", "digest", "pdf")]})),
        reason("bad-role", Some("files[0]"))
    );
}

#[test]
fn path_rules() {
    let long = format!("{}.pdf", "a".repeat(1021));
    assert_eq!(long.len(), 1025);
    for p in [
        "/abs.pdf",
        "a/../b.pdf",
        "../a.pdf",
        "./a.pdf",
        "a/./b.pdf",
        "a//b.pdf",
        "a/",
        "a\\b.pdf",
        "C:x.pdf",
        "C:/x.pdf",
        "a\u{1}b.pdf",
        "a\u{7f}b.pdf",
        ".pr-tmp/a.pdf",
        long.as_str(),
    ] {
        assert_eq!(
            bad(json!({"files": [src_pdf(p)]})),
            reason("bad-path", Some(p)),
            "{p:?}"
        );
    }
    // 路徑不合規則先於檔案不存在、重複
    assert_eq!(
        bad(
            json!({"files": [src_pdf("a.pdf"), file("x/../t.md", "digest", "markdown"), file("A.PDF", "translation", "pdf")]})
        ),
        reason("bad-path", Some("x/../t.md"))
    );
}

#[cfg(unix)]
#[test]
fn symlink_below_root_skipped() {
    let s = Src::new();
    s.pdf("real.pdf", 1);
    s.pdf("d/b.pdf", 2);
    std::os::unix::fs::symlink(s.root().join("real.pdf"), s.root().join("link.pdf")).unwrap();
    std::os::unix::fs::symlink(s.root().join("d"), s.root().join("dlink")).unwrap();
    s.manifest(json!([
        one("link.pdf"),
        one("dlink/b.pdf"),
        one("real.pdf")
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("bad-path", Some("link.pdf")));
    assert_eq!(skip(&p, 1), reason("bad-path", Some("dlink/b.pdf")));
    assert_eq!(accepted(&p), vec![2]);
    // 根目錄本身是符號連結不受限制
    let outer = tempfile::tempdir().unwrap();
    let via = outer.path().join("via");
    std::os::unix::fs::symlink(s.root(), &via).unwrap();
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &ImportContainer::Folder(via));
    assert_eq!(accepted(&p), vec![2]);
}

#[test]
fn fifo_or_dir_as_file_skipped() {
    let s = Src::new();
    s.pdf("d/a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.manifest(json!([one("d"), {"files": [src_pdf("b.pdf"), file("d", "digest", "markdown")]}]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("bad-path", Some("d")));
    assert_eq!(skip(&p, 1), reason("bad-path", Some("d")));
    #[cfg(unix)]
    {
        let st = std::process::Command::new("mkfifo")
            .arg(s.root().join("f.pdf"))
            .status()
            .unwrap();
        assert!(st.success());
        s.manifest(json!([one("f.pdf"), one("b.pdf")]));
        let p = preview_of(&s);
        assert_eq!(skip(&p, 0), reason("bad-path", Some("f.pdf")));
        assert_eq!(accepted(&p), vec![1]);
    }
}

fn case_src() -> Src {
    let s = Src::new();
    s.pdf("L001/Source.pdf", 1);
    s.pdf("l002/a.pdf", 2);
    s.pdf("ok.pdf", 3);
    s.manifest(json!([
        one("L001/source.pdf"),
        one("L002/a.pdf"),
        one("ok.pdf")
    ]));
    s
}

#[test]
fn case_must_match_exactly_folder() {
    let p = preview_of(&case_src());
    assert_eq!(skip(&p, 0), reason("missing-file", Some("L001/source.pdf")));
    assert_eq!(skip(&p, 1), reason("missing-file", Some("L002/a.pdf")));
    assert_eq!(accepted(&p), vec![2]);
}

#[test]
fn case_must_match_exactly_zip() {
    let s = case_src();
    let out = tempfile::tempdir().unwrap();
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.zip(&out));
    assert_eq!(skip(&p, 0), reason("missing-file", Some("L001/source.pdf")));
    assert_eq!(skip(&p, 1), reason("missing-file", Some("L002/a.pdf")));
    assert_eq!(accepted(&p), vec![2]);
}

#[test]
fn duplicate_path_within_paper() {
    assert_eq!(
        bad(json!({"files": [src_pdf("a.pdf"), file("A.PDF", "translation", "pdf")]})),
        reason("duplicate-path", Some("A.PDF"))
    );
    assert_eq!(
        bad(
            json!({"files": [src_pdf("a.pdf"), file("t.md", "digest", "markdown"), file("t.md", "text", "markdown")]})
        ),
        reason("duplicate-path", Some("t.md"))
    );
}

#[test]
fn duplicate_path_across_papers_case_insensitive() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.md("t.md", "t");
    s.manifest(json!([
        {"files": [src_pdf("a.pdf"), file("t.md", "translation", "markdown")]},
        {"files": [src_pdf("b.pdf"), file("T.md", "translation", "markdown")]}
    ]));
    let p = preview_of(&s);
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(skip(&p, 1), reason("duplicate-path", Some("T.md")));
}

#[test]
fn skipped_paper_does_not_claim_path() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!([
        {"files": [src_pdf("a.pdf"), file("gone.md", "digest", "markdown")]},
        one("a.pdf")
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("missing-file", Some("gone.md")));
    assert_eq!(accepted(&p), vec![1]);
}

#[test]
fn form_mismatch_not_pdf() {
    let s = Src::new();
    s.write("a.pdf", b"hello, not a pdf");
    s.pdf("b.pdf", 2);
    s.md("t.pdf", "# a markdown file named .pdf");
    s.manifest(json!([
        one("a.pdf"),
        {"files": [src_pdf("b.pdf"), file("t.pdf", "translation", "pdf")]}
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("a.pdf")));
    assert_eq!(skip(&p, 1), reason("form-mismatch", Some("t.pdf")));
}

#[test]
fn form_mismatch_markdown_not_utf8() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.write("t.md", &[b'#', b' ', 0xff, 0xfe, b'x']);
    // 結尾停在一個字的中間
    s.write("u.md", &[b'#', 0xe4, 0xb8]);
    s.pdf("b.pdf", 2);
    s.manifest(json!([
        {"files": [src_pdf("a.pdf"), file("t.md", "translation", "markdown")]},
        {"files": [src_pdf("b.pdf"), file("u.md", "digest", "markdown")]}
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("t.md")));
    assert_eq!(skip(&p, 1), reason("form-mismatch", Some("u.md")));
}

#[test]
fn form_mismatch_empty_file() {
    let s = Src::new();
    s.write("a.pdf", b"");
    s.pdf("b.pdf", 2);
    s.write("t.md", b"");
    s.manifest(json!([
        one("a.pdf"),
        {"files": [src_pdf("b.pdf"), file("t.md", "text", "markdown")]}
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("a.pdf")));
    assert_eq!(skip(&p, 1), reason("form-mismatch", Some("t.md")));
}

#[test]
fn markdown_bom_kept() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    let mut md = vec![0xEF, 0xBB, 0xBF];
    md.extend_from_slice("# 譯本\n內文".as_bytes());
    s.write("t.md", &md);
    s.manifest(json!([{"files": [src_pdf("a.pdf"), file("t.md", "translation", "markdown")]}]));
    let (d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    let id = id_for(&r, 0);
    let dir = folder_of(d.path(), &lib, &id);
    assert_eq!(fs::read(dir.join("translation.md")).unwrap(), md);
    assert_eq!(
        paper(&lib, &id).materials[1].sha256.as_deref(),
        Some(sha_hex(&md).as_str())
    );
}

/// 一個字跨在兩段讀取之間（讀檔一次 1 MiB）：資料夾與 zip 都照收。
#[test]
fn markdown_multibyte_across_chunks() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    let text = "譯".repeat(400_000);
    assert!(text.len() > 1 << 20 && !(1usize << 20).is_multiple_of(3));
    s.md("t.md", &text);
    s.manifest(json!([{"files": [src_pdf("a.pdf"), file("t.md", "translation", "markdown")]}]));
    let out = tempfile::tempdir().unwrap();
    for c in [s.folder(), s.zip(&out)] {
        let (d, mut lib) = temp_library();
        let (p, r) = import(&mut lib, &c);
        assert_eq!(accepted(&p), vec![0], "{c:?}");
        let dir = folder_of(d.path(), &lib, &id_for(&r, 0));
        assert_eq!(
            fs::read(dir.join("translation.md")).unwrap(),
            text.as_bytes()
        );
    }
}

#[test]
fn checksum_mismatch_skipped() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.md("t.md", "text");
    let mut a = src_pdf("a.pdf");
    a["sha256"] = json!(sha_hex(b"something else"));
    let mut t = file("t.md", "translation", "markdown");
    t["sha256"] = json!(sha_hex(b"something else"));
    s.manifest(json!([{"files": [a]}, {"files": [src_pdf("b.pdf"), t]}]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("checksum-mismatch", Some("a.pdf")));
    assert_eq!(skip(&p, 1), reason("checksum-mismatch", Some("t.md")));
}

#[test]
fn checksum_uppercase_accepted() {
    let s = Src::new();
    let a_bytes = s.pdf("a.pdf", 1);
    s.md("t.md", "text");
    let mut a = src_pdf("a.pdf");
    a["sha256"] = json!(sha_hex(&a_bytes).to_uppercase());
    let mut t = file("t.md", "translation", "markdown");
    t["sha256"] = json!(sha_hex(b"text").to_uppercase());
    s.manifest(json!([{"files": [a, t]}]));
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0]);
    let pf = paper(&lib, &id_for(&r, 0));
    assert_eq!(pf.materials[0].sha256, Some(sha_hex(&a_bytes)));
    assert_eq!(pf.materials[1].sha256, Some(sha_hex(b"text")));
}

// ---- 21. 比對 ----

#[test]
fn already_present_and_in_trash_with_id() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.pdf("c.pdf", 3);
    let (_d, mut lib) = temp_library();
    let added = |lib: &mut LocalLibrary, p: &Path| match lib
        .add_paper(p, BibDraft::default(), None)
        .unwrap()
    {
        AddOutcome::Added(id) => id,
        o => panic!("{o:?}"),
    };
    let present = added(&mut lib, &s.root().join("a.pdf"));
    let trashed = added(&mut lib, &s.root().join("b.pdf"));
    lib.remove_paper(&trashed).unwrap();
    s.manifest(json!([one("a.pdf"), one("b.pdf"), one("c.pdf")]));
    let p = preview(&mut lib, &s.folder());
    assert_eq!(
        skip(&p, 0),
        reason("already-present", Some(present.as_str()))
    );
    assert_eq!(skip(&p, 1), reason("in-trash", Some(trashed.as_str())));
    assert_eq!(accepted(&p), vec![2]);
    assert_eq!(p.bytes, 3000);
}

#[test]
fn duplicate_source_within_manifest() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("copy/a.pdf", 1);
    s.manifest(json!([one("a.pdf"), one("copy/a.pdf")]));
    let p = preview_of(&s);
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(skip(&p, 1), reason("duplicate-source", Some("papers[0]")));
}

#[test]
fn skipped_paper_does_not_claim_source() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("copy/a.pdf", 1);
    s.manifest(json!([
        {"files": [src_pdf("a.pdf"), file("gone.md", "digest", "markdown")]},
        one("copy/a.pdf")
    ]));
    let p = preview_of(&s);
    assert_eq!(skip(&p, 0), reason("missing-file", Some("gone.md")));
    assert_eq!(accepted(&p), vec![1]);
}

#[test]
fn same_doi_is_notice_not_skip() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    let (_d, mut lib) = temp_library();
    let id = match lib
        .add_paper(
            &s.root().join("a.pdf"),
            BibDraft {
                doi: Some("10.1/X".into()),
                ..Default::default()
            },
            None,
        )
        .unwrap()
    {
        AddOutcome::Added(id) => id,
        o => panic!("{o:?}"),
    };
    s.manifest(json!([{"csl": {"DOI": "https://doi.org/10.1/x"}, "files": [src_pdf("b.pdf")]}]));
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(
        p.accept[0].notices,
        vec![ImportNotice {
            kind: "same-doi".into(),
            detail: Some(id)
        }]
    );
    assert_eq!(r.added.len(), 1);
    assert_eq!(r.notices, 1);
}

#[test]
fn prepare_on_read_only_library() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.manifest(json!([one("a.pdf")]));
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("library.json"),
        json!({"format": 2, "id": "01JCK3W2E4H6J8K0M2N4P6Q8RS"}).to_string(),
    )
    .unwrap();
    let mut lib = LocalLibrary::open(d.path(), DEVICE).unwrap();
    assert!(lib.is_read_only());
    let sc = scan(&s.folder());
    assert!(matches!(lib.prepare_import(sc), Err(LibError::ReadOnly)));
}

// ---- 22–30. 寫入 ----

#[test]
fn scan_and_prepare_write_nothing() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.md("t.md", "x");
    s.manifest(
        json!([one("a.pdf"), {"files": [src_pdf("b.pdf"), file("t.md", "text", "markdown")]}]),
    );
    let (d, mut lib) = temp_library();
    add_paper(&mut lib, 7, "existing", None);
    let (gone, _) = add_paper(&mut lib, 8, "gone", None);
    lib.remove_paper(&gone).unwrap();
    let lib_before = snapshot(d.path());
    let src_before = snapshot(s.root());
    let p = preview(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0, 1]);
    assert_eq!(snapshot(d.path()), lib_before);
    assert_eq!(snapshot(s.root()), src_before);
}

#[test]
fn apply_writes_paper_json_as_spec() {
    let s = Src::new();
    let src = s.pdf("L001/2304.08069v3.pdf", 1);
    let tr = s.pdf("L001/zh-TW.pdf", 2);
    // 重點筆記的已校對取檔案屬性區的值，清單寫 false 也一樣
    let dg = digest_proofread("重點", true);
    s.md("L001/digest.md", &dg);
    s.manifest(json!([{
        "csl": {"id": "zhao2024", "type": "paper-conference",
                "title": "DETRs Beat YOLOs on Real-time Object Detection",
                "author": [{"family": "Zhao", "given": "Yian"}, {"literal": "RT-DETR Team"}],
                "issued": {"date-parts": [[2024]]}, "container-title": "CVPR 2024",
                "DOI": "https://doi.org/10.48550/arXiv.2304.08069", "URL": "https://arxiv.org/abs/2304.08069"},
        "short_name": "RT-DETR",
        "aliases": ["L001"],
        "files": [
            {"path": "L001/2304.08069v3.pdf", "role": "source", "form": "pdf"},
            {"path": "L001/zh-TW.pdf", "role": "translation", "form": "pdf",
             "title": "繁體中文譯本", "page_map": "same-as-source", "proofread": false},
            {"path": "L001/digest.md", "role": "digest", "form": "markdown", "proofread": false,
             "page_map": "marked-in-text"}
        ]
    }]));
    let (d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(p.bytes, 3000 + 3000 + dg.len() as u64);
    let id = id_for(&r, 0);
    let pf = paper(&lib, &id);
    assert_eq!(pf.format, 1);
    assert_eq!(pf.id, id);
    assert_eq!(pf.short_name.as_deref(), Some("RT-DETR"));
    assert_eq!(pf.aliases, vec!["L001"]);
    let added = pf.added.clone().unwrap();
    assert_eq!(added.len(), 24);
    let b = &pf.bib;
    assert_eq!(
        b.title.as_deref(),
        Some("DETRs Beat YOLOs on Real-time Object Detection")
    );
    assert_eq!(b.authors[0].family.as_deref(), Some("Zhao"));
    assert_eq!(b.authors[0].given.as_deref(), Some("Yian"));
    assert_eq!(b.authors[1].literal.as_deref(), Some("RT-DETR Team"));
    assert_eq!(b.year, Some(2024));
    assert_eq!(b.venue.as_deref(), Some("CVPR 2024"));
    assert_eq!(b.doi.as_deref(), Some("10.48550/arxiv.2304.08069"));
    assert_eq!(b.url.as_deref(), Some("https://arxiv.org/abs/2304.08069"));
    assert_eq!(b.modified.as_deref(), Some(added.as_str()));
    assert_eq!(b.modified_by.as_deref(), Some(DEVICE));
    assert!(b.cleared.is_empty());
    assert!(pf.groups.is_empty());

    // 重點筆記：指紋與大小是 null（可以在筆記工具裡改）
    let want = [
        (
            "source",
            "pdf",
            "source.pdf",
            None,
            Some(sha_hex(&src)),
            Some(3000u64),
            None,
            None,
            "2304.08069v3.pdf",
        ),
        (
            "translation",
            "pdf",
            "translation.pdf",
            Some("繁體中文譯本"),
            Some(sha_hex(&tr)),
            Some(3000),
            Some(false),
            Some("same-as-source"),
            "zh-TW.pdf",
        ),
        (
            "digest",
            "markdown",
            "digest.md",
            None,
            None,
            None,
            Some(true),
            Some("marked-in-text"),
            "digest.md",
        ),
    ];
    assert_eq!(pf.materials.len(), 3);
    for (m, w) in pf.materials.iter().zip(want.iter()) {
        assert_eq!(m.id.len(), 26);
        assert_eq!(
            (m.role.as_str(), m.form.as_str(), m.path.as_str()),
            (w.0, w.1, w.2)
        );
        assert_eq!(m.title.as_deref(), w.3);
        assert_eq!(m.sha256, w.4);
        assert_eq!(m.bytes, w.5);
        assert_eq!(m.pages, None);
        assert_eq!(m.pages_modified, None);
        let pv = &m.provenance;
        assert_eq!(pv.method.as_deref(), Some("imported"));
        assert_eq!(pv.time.as_deref(), Some(added.as_str()));
        assert_eq!(pv.origin_name.as_deref(), Some(w.8));
        assert_eq!(pv.proofread, w.6);
        assert_eq!(pv.page_map.as_deref(), w.7);
        assert_eq!(pv.source_sha256, None);
    }
    // 檔案：只有 paper.json 與三個材料檔；不帶 cleared、不建 references.json
    let dir = folder_of(d.path(), &lib, &id);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["digest.md", "paper.json", "source.pdf", "translation.pdf"]
    );
    assert_eq!(fs::read(dir.join("source.pdf")).unwrap(), src);
    let raw: Value = serde_json::from_slice(&fs::read(dir.join("paper.json")).unwrap()).unwrap();
    assert!(raw["bib"].get("cleared").is_none());
    assert_eq!(
        raw["materials"][0]["provenance"]["source_sha256"],
        Value::Null
    );
    assert!(temp_items(d.path()).is_empty());
}

#[test]
fn material_file_names_as_table() {
    let s = Src::new();
    s.pdf("x/src.pdf", 1);
    s.pdf("x/t1.pdf", 2);
    s.pdf("x/t2.pdf", 3);
    s.md("x/t.md", "t");
    s.md("x/d1.md", &digest_text("d1"));
    s.md("x/d2.md", &digest_text("d2"));
    s.md("x/txt.md", "txt");
    s.manifest(json!([{"files": [
        src_pdf("x/src.pdf"),
        file("x/t1.pdf", "translation", "pdf"),
        file("x/t2.pdf", "translation", "pdf"),
        file("x/t.md", "translation", "markdown"),
        file("x/d1.md", "digest", "markdown"),
        file("x/d2.md", "digest", "markdown"),
        file("x/txt.md", "text", "markdown"),
    ]}]));
    let (d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    let id = id_for(&r, 0);
    let paths: Vec<String> = paper(&lib, &id)
        .materials
        .iter()
        .map(|m| m.path.clone())
        .collect();
    assert_eq!(
        paths,
        vec![
            "source.pdf",
            "translation.pdf",
            "translation-2.pdf",
            "translation.md",
            "digest.md",
            "digest-2.md",
            "text.md"
        ]
    );
    let dir = folder_of(d.path(), &lib, &id);
    assert_eq!(
        fs::read(dir.join("digest-2.md")).unwrap(),
        digest_text("d2").as_bytes()
    );
    assert_eq!(
        fs::read(dir.join("translation-2.pdf")).unwrap(),
        fake_bytes(3, 3000)
    );
}

/// 匯入的文獻庫經得起「檢查文獻庫」（完整）；重點筆記在筆記工具裡改過之後也是。
#[test]
fn imported_library_passes_full_check() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("t.pdf", 2);
    s.md("t.md", "# 譯本\n");
    s.md("d.md", &digest_text("重點"));
    s.md("x.md", "抽出的文字");
    s.manifest(json!([{"files": [
        src_pdf("a.pdf"),
        file("t.pdf", "translation", "pdf"),
        file("t.md", "translation", "markdown"),
        file("d.md", "digest", "markdown"),
        file("x.md", "text", "markdown"),
    ]}]));
    let (d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    let id = id_for(&r, 0);
    let all_ok = |lib: &LocalLibrary| {
        lib.list_papers()
            .iter()
            .flat_map(|p| p.materials.iter())
            .all(|m| m.status == paperreader_core::store::MaterialStatus::Ok)
    };
    assert_eq!(lib.check(CheckLevel::Full).unwrap(), vec![]);
    assert!(all_ok(&lib));
    let digest = folder_of(d.path(), &lib, &id).join("digest.md");
    let mut text = fs::read_to_string(&digest).unwrap();
    text.push_str("\n使用者在筆記工具裡加的一行。\n");
    fs::write(&digest, text).unwrap();
    assert_eq!(lib.check(CheckLevel::Full).unwrap(), vec![]);
    assert!(all_ok(&lib));
}

/// 重點筆記要是重點筆記的格式（核心的 `digest` 角色一律照它讀）：讀不懂 → `form-mismatch`，掃描與寫入都查。
#[test]
fn digest_must_be_in_digest_format() {
    let s = Src::new();
    for k in 0..3 {
        s.pdf(&format!("p{k}.pdf"), 30 + k);
    }
    s.md("free.md", "# 只是一份筆記\n\n- 一點\n");
    s.md("ok.md", &digest_text("A"));
    s.md("later.md", &digest_text("B"));
    s.manifest(json!([
        {"files": [src_pdf("p0.pdf"), file("free.md", "digest", "markdown")]},
        {"files": [src_pdf("p1.pdf"), file("ok.md", "digest", "markdown")]},
        {"files": [src_pdf("p2.pdf"), file("later.md", "digest", "markdown")]}
    ]));
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("free.md")));
    assert_eq!(accepted(&p), vec![1, 2]);
    s.md("later.md", "# 掃描之後換成一般的 Markdown\n");
    let r = run(&mut lib, &p.preview_id);
    assert_eq!(added_index(&r), vec![1]);
    assert_eq!(
        r.skipped.last().unwrap(),
        &ImportSkip {
            index: 2,
            reason: "form-mismatch".into(),
            detail: Some("later.md".into())
        }
    );
}

/// 重點筆記的指紋與大小不記；清單寫了指紋照樣比對。
#[test]
fn digest_sha256_and_bytes_null_declared_checked() {
    let s = Src::new();
    s.pdf("p0.pdf", 40);
    s.pdf("p1.pdf", 41);
    let text = digest_text("A");
    s.md("d.md", &text);
    let mut good = file("d.md", "digest", "markdown");
    good["sha256"] = json!(sha_hex(text.as_bytes()));
    let mut wrong = file("d.md", "digest", "markdown");
    wrong["sha256"] = json!(sha_hex(b"other"));
    s.manifest(json!([
        {"files": [src_pdf("p0.pdf"), wrong]},
        {"files": [src_pdf("p1.pdf"), good]}
    ]));
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &s.folder());
    assert_eq!(skip(&p, 0), reason("checksum-mismatch", Some("d.md")));
    let m = &paper(&lib, &id_for(&r, 1)).materials[1];
    assert_eq!(
        (m.role.as_str(), m.sha256.as_deref(), m.bytes),
        ("digest", None, None)
    );
}

#[test]
fn folder_name_from_mapped_bib() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.pdf("c.pdf", 3);
    s.manifest(json!([
        {"csl": {"title": "DETRs Beat YOLOs", "author": [{"family": "Zhao", "given": "Yian"}],
                 "issued": {"date-parts": [[2024]]}}, "files": [src_pdf("a.pdf")]},
        {"csl": {"title": "Same", "author": [{"literal": "RT-DETR Team"}]}, "files": [src_pdf("b.pdf")]},
        {"files": [src_pdf("c.pdf")]}
    ]));
    let (_d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    let folder = |i: u32| {
        let id = id_for(&r, i);
        lib.list_papers()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .folder
    };
    assert_eq!(folder(0), "Zhao 2024 - DETRs Beat YOLOs");
    assert_eq!(folder(1), "RT-DETR Team - Same");
    assert_eq!(folder(2), "c");
}

/// 三篇，各有原文與一份 Markdown 譯本。
fn three_papers() -> Src {
    let s = Src::new();
    for k in 0..3 {
        s.pdf(&format!("p{k}/a.pdf"), 10 + k);
        s.md(&format!("p{k}/t.md"), &format!("t{k}"));
    }
    s.manifest(Value::Array(
        (0..3)
            .map(|k| {
                json!({"files": [src_pdf(&format!("p{k}/a.pdf")),
                                 file(&format!("p{k}/t.md"), "translation", "markdown")]})
            })
            .collect(),
    ));
    s
}

#[test]
fn step_io_error_marks_failed_and_continues() {
    let s = three_papers();
    let (d, mut lib) = temp_library();
    lib.set_import_copy_hook(Some(Box::new(|i, k| {
        (i == 1 && k == 1).then(|| LibError::Io {
            path: "x".into(),
            reason: "boom".into(),
        })
    })));
    let p = preview(&mut lib, &s.folder());
    let all = steps(&mut lib, &p.preview_id);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], ImportStep::Progress { done: 1, total: 3 });
    assert_eq!(all[1], ImportStep::Progress { done: 2, total: 3 });
    let ImportStep::Finished(r) = &all[2] else {
        panic!("{all:?}")
    };
    assert_eq!(added_index(r), vec![0, 2]);
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].index, 1);
    assert_eq!(r.failed[0].reason, "failed");
    let detail = r.failed[0].detail.clone().unwrap();
    assert!(
        detail.starts_with("Io") && detail.contains("boom"),
        "{detail}"
    );
    assert!(r.not_done.is_empty());
    assert!(temp_items(d.path()).is_empty());
    assert_eq!(lib.list_papers().len(), 2);
}

#[test]
fn step_no_space_stops_rest_not_done() {
    let s = three_papers();
    let (d, mut lib) = temp_library();
    lib.set_import_copy_hook(Some(Box::new(|i, k| {
        (i == 1 && k == 0).then_some(LibError::NoSpace)
    })));
    let p = preview(&mut lib, &s.folder());
    let all = steps(&mut lib, &p.preview_id);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], ImportStep::Progress { done: 1, total: 3 });
    let ImportStep::Finished(r) = &all[1] else {
        panic!("{all:?}")
    };
    assert_eq!(added_index(r), vec![0]);
    assert_eq!(r.not_done, vec![1, 2]);
    assert!(r.failed.is_empty());
    assert!(temp_items(d.path()).is_empty());
    assert_eq!(lib.list_papers().len(), 1);
    // 計畫已丟掉
    assert!(matches!(
        lib.import_step(&p.preview_id),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn manifest_changed_since_scan_is_conflict() {
    let s = three_papers();
    let (d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    let before = snapshot(d.path());
    let mut m = fs::read(s.root().join(MANIFEST_NAME)).unwrap();
    m.push(b'\n');
    s.write(MANIFEST_NAME, &m);
    assert!(matches!(
        lib.import_step(&p.preview_id),
        Err(LibError::Conflict)
    ));
    assert_eq!(snapshot(d.path()), before);
    assert!(matches!(
        lib.import_step(&p.preview_id),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn source_added_meanwhile_skipped_at_write() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.pdf("c.pdf", 3);
    s.manifest(json!([one("a.pdf"), one("b.pdf"), one("c.pdf")]));
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0, 1, 2]);
    let added = |lib: &mut LocalLibrary, n: &str| match lib
        .add_paper(&s.root().join(n), BibDraft::default(), None)
        .unwrap()
    {
        AddOutcome::Added(id) => id,
        o => panic!("{o:?}"),
    };
    let a = added(&mut lib, "a.pdf");
    let b = added(&mut lib, "b.pdf");
    lib.remove_paper(&b).unwrap();
    let r = run(&mut lib, &p.preview_id);
    assert_eq!(added_index(&r), vec![2]);
    assert_eq!(
        r.skipped,
        vec![
            ImportSkip {
                index: 0,
                reason: "already-present".into(),
                detail: Some(a)
            },
            ImportSkip {
                index: 1,
                reason: "in-trash".into(),
                detail: Some(b)
            },
        ]
    );
}

#[test]
fn file_changed_after_scan_checksum_mismatch_at_write() {
    let s = Src::new();
    for k in 0..5 {
        s.pdf(&format!("p{k}.pdf"), 20 + k);
    }
    s.md("t1.md", "declared");
    s.md("t2.md", "plain");
    s.md("t3.md", "will vanish");
    let mut t1 = file("t1.md", "text", "markdown");
    t1["sha256"] = json!(sha_hex(b"declared"));
    s.manifest(json!([
        one("p0.pdf"),
        {"files": [src_pdf("p1.pdf"), t1]},
        {"files": [src_pdf("p2.pdf"), file("t2.md", "text", "markdown")]},
        {"files": [src_pdf("p3.pdf"), file("t3.md", "text", "markdown")]},
        one("p4.pdf")
    ]));
    let (d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    assert_eq!(accepted(&p), vec![0, 1, 2, 3, 4]);
    // 掃描之後：原文換了內容、宣告指紋的檔改了、Markdown 不再是 UTF-8、檔案不見了
    s.write("p0.pdf", &fake_bytes(99, 3000));
    s.md("t1.md", "changed");
    s.write("t2.md", &[0xff, 0xfe]);
    fs::remove_file(s.root().join("t3.md")).unwrap();
    let r = run(&mut lib, &p.preview_id);
    assert_eq!(added_index(&r), vec![4]);
    let got: Vec<(u32, &str, Option<&str>)> = r
        .skipped
        .iter()
        .map(|x| (x.index, x.reason.as_str(), x.detail.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, "checksum-mismatch", Some("p0.pdf")),
            (1, "checksum-mismatch", Some("t1.md")),
            (2, "form-mismatch", Some("t2.md")),
            (3, "missing-file", Some("t3.md")),
        ]
    );
    assert!(temp_items(d.path()).is_empty());
    assert_eq!(lib.list_papers().len(), 1);
}

#[test]
fn finish_keeps_done_rest_not_done() {
    let s = three_papers();
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &s.folder());
    assert_eq!(
        lib.import_step(&p.preview_id).unwrap(),
        ImportStep::Progress { done: 1, total: 3 }
    );
    let r = lib.finish_import(&p.preview_id).unwrap();
    assert_eq!(added_index(&r), vec![0]);
    assert_eq!(r.not_done, vec![1, 2]);
    assert!(r.skipped.is_empty() && r.failed.is_empty());
    assert_eq!(lib.list_papers().len(), 1);
    assert!(matches!(
        lib.import_step(&p.preview_id),
        Err(LibError::InvalidInput { .. })
    ));
    assert!(matches!(
        lib.finish_import(&p.preview_id),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn unknown_preview_id_invalid_input() {
    let (_d, mut lib) = temp_library();
    match lib.import_step("01ZZZZZZZZZZZZZZZZZZZZZZZZ") {
        Err(LibError::InvalidInput { reason }) => assert!(reason.contains("unknown preview")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        lib.finish_import("nope"),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn new_preview_replaces_old() {
    let s = three_papers();
    let (_d, mut lib) = temp_library();
    let p1 = preview(&mut lib, &s.folder());
    let p2 = preview(&mut lib, &s.folder());
    assert_ne!(p1.preview_id, p2.preview_id);
    assert!(matches!(
        lib.import_step(&p1.preview_id),
        Err(LibError::InvalidInput { .. })
    ));
    assert_eq!(run(&mut lib, &p2.preview_id).added.len(), 3);
}

#[test]
fn rerun_same_manifest_adds_nothing() {
    let s = three_papers();
    let (d, mut lib) = temp_library();
    let (_, first) = import(&mut lib, &s.folder());
    assert_eq!(first.added.len(), 3);
    let before = snapshot(d.path());
    let (p, second) = import(&mut lib, &s.folder());
    assert!(p.accept.is_empty());
    for (i, id) in &first.added {
        assert_eq!(skip(&p, *i), reason("already-present", Some(id.as_str())));
    }
    assert!(second.added.is_empty());
    assert_eq!(second.skipped.len(), 3);
    assert_eq!(snapshot(d.path()), before);
}

#[test]
fn scan_progress_reported_and_cancel_returns_none() {
    let s = three_papers();
    let total: u64 = 3 * 3000 + "t0".len() as u64 * 3;
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let sc = scan_import(&s.folder(), &mut |d, t| {
        calls.push((d, t));
        true
    })
    .unwrap();
    assert!(sc.is_some());
    assert!(!calls.is_empty());
    // 第一輪（檢查欄位）還不知道總量，回報 (0, 0)；之後每次都帶總量
    let first = calls.iter().position(|c| c.1 != 0).unwrap();
    assert!(first >= 3, "{calls:?}");
    assert!(calls[..first].iter().all(|c| *c == (0, 0)), "{calls:?}");
    assert!(calls[first..].iter().all(|c| c.1 == total), "{calls:?}");
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0), "{calls:?}");
    assert_eq!(*calls.last().unwrap(), (total, total));

    // 取消：交回 None，什麼都沒寫
    let before = snapshot(s.root());
    for stop_at in [1usize, 3] {
        let mut n = 0;
        let r = scan_import(&s.folder(), &mut |_, _| {
            n += 1;
            n < stop_at
        })
        .unwrap();
        assert!(r.is_none(), "stop at {stop_at}");
    }
    assert_eq!(snapshot(s.root()), before);

    // 掃描的結果可以交給別的執行緒、放進外殼的狀態
    fn send_static<T: Send + 'static>() {}
    send_static::<ImportScan>();
}

// ---- 31–34. zip ----

#[test]
fn zip_deflate_accepted() {
    let s = Src::new();
    let a = s.pdf("L001/a.pdf", 1);
    s.md("L001/t.md", &"翻譯 ".repeat(1000));
    s.manifest(
        json!([{"files": [src_pdf("L001/a.pdf"), file("L001/t.md", "translation", "markdown")]}]),
    );
    let out = tempfile::tempdir().unwrap();
    let c = s.zip(&out);
    let entries = zip_entries(c.path());
    assert!(
        entries
            .iter()
            .any(|e| e.0 == "L001/t.md" && e.2 == zip::CompressionMethod::Deflated)
    );
    let (d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &c);
    assert_eq!(accepted(&p), vec![0]);
    let id = id_for(&r, 0);
    let dir = folder_of(d.path(), &lib, &id);
    assert_eq!(fs::read(dir.join("source.pdf")).unwrap(), a);
    assert_eq!(
        fs::read(dir.join("translation.md")).unwrap(),
        "翻譯 ".repeat(1000).as_bytes()
    );
    assert_eq!(paper(&lib, &id).materials[0].sha256, Some(sha_hex(&a)));
}

/// 寫進去的一篇：書目，與每項材料的（角色, 檔名, 指紋）。
type Imported = (Bib, Vec<(String, String, Option<String>)>);

#[test]
fn zip_same_result_as_folder() {
    let s = Src::new();
    s.pdf("ok/a.pdf", 1);
    s.md("ok/t.md", "t");
    s.md("ok/d.md", &digest_text("d"));
    s.pdf("miss/a.pdf", 2);
    s.write("form/a.pdf", b"not a pdf");
    s.pdf("dup/a.pdf", 3);
    s.pdf("sum/a.pdf", 4);
    s.pdf("Case/A.pdf", 5);
    s.pdf("copy/a.pdf", 1);
    let mut sum = src_pdf("sum/a.pdf");
    sum["sha256"] = json!("0".repeat(64));
    s.manifest(json!([
        {"csl": {"title": "OK", "volume": "1"}, "short_name": "ok", "aliases": ["L1"],
         "files": [src_pdf("ok/a.pdf"), file("ok/t.md", "translation", "markdown"),
                   file("ok/d.md", "digest", "markdown")]},
        {"files": [src_pdf("miss/a.pdf"), file("miss/gone.md", "digest", "markdown")]},
        one("form/a.pdf"),
        {"files": [src_pdf("dup/a.pdf"), file("OK/T.md", "digest", "markdown")]},
        {"files": [sum]},
        one("Case/a.pdf"),
        one("bad\\a.pdf"),
        one("copy/a.pdf"),
        one("ok")
    ]));
    let out = tempfile::tempdir().unwrap();
    let mut results = Vec::new();
    for c in [s.folder(), s.zip(&out)] {
        let (_d, mut lib) = temp_library();
        let mut p = preview(&mut lib, &c);
        let r = run(&mut lib, &p.preview_id);
        p.preview_id.clear();
        let papers: Vec<Imported> = r
            .added
            .iter()
            .map(|(_, id)| {
                let mut pf = paper(&lib, id);
                pf.bib.modified = None;
                let ms = pf
                    .materials
                    .iter()
                    .map(|m| (m.role.clone(), m.path.clone(), m.sha256.clone()))
                    .collect();
                (pf.bib, ms)
            })
            .collect();
        results.push((p, papers));
    }
    assert_eq!(results[0], results[1]);
    let p = &results[0].0;
    assert_eq!(accepted(p), vec![0]);
    let reasons: Vec<&str> = p.skip.iter().map(|x| x.reason.as_str()).collect();
    assert_eq!(
        reasons,
        vec![
            "missing-file",
            "form-mismatch",
            "duplicate-path",
            "checksum-mismatch",
            "missing-file",
            "bad-path",
            "duplicate-source",
            "bad-path"
        ]
    );
}

/// 自己寫一份 zip：`(名稱, 內容, 加密)`，不壓縮。
fn raw_zip(
    out: &TempDir,
    entries: &[(&str, &[u8], bool)],
    symlinks: &[(&str, &str)],
) -> ImportContainer {
    use zip::unstable::write::FileOptionsExt;
    let path = out.path().join("raw.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let plain =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes, encrypted) in entries {
        let opts = if *encrypted {
            plain.with_deprecated_encryption(b"secret").unwrap()
        } else {
            plain
        };
        z.start_file(*name, opts).unwrap();
        z.write_all(bytes).unwrap();
    }
    for (name, target) in symlinks {
        z.add_symlink(*name, *target, plain).unwrap();
    }
    z.finish().unwrap();
    ImportContainer::Zip(path)
}

fn manifest_bytes(papers: Value) -> Vec<u8> {
    json!({"format": 1, "kind": "paperreader-import", "papers": papers})
        .to_string()
        .into_bytes()
}

#[test]
fn zip_unreferenced_bad_entries_ignored() {
    let out = tempfile::tempdir().unwrap();
    let a = fake_bytes(1, 3000);
    let m = manifest_bytes(json!([one("a.pdf")]));
    let c = raw_zip(
        &out,
        &[
            (MANIFEST_NAME, &m, false),
            ("a.pdf", &a, false),
            ("__MACOSX/._a.pdf", b"\x00\x05junk", false),
            ("C:evil.txt", b"x", false),
            (".pr-temp", b"x", false),
            ("x/../y.pdf", b"x", false),
            ("secret.pdf", &fake_bytes(2, 3000), true),
        ],
        &[("link.pdf", "a.pdf")],
    );
    let (_d, mut lib) = temp_library();
    let (p, r) = import(&mut lib, &c);
    assert_eq!(accepted(&p), vec![0]);
    assert_eq!(r.added.len(), 1);
}

#[test]
fn zip_referenced_bad_name_bad_path() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one(".pr-x/a.pdf"), one("d"), one("ok.pdf")]));
    let c = raw_zip(
        &out,
        &[
            (MANIFEST_NAME, &m, false),
            (".pr-x/a.pdf", &fake_bytes(1, 3000), false),
            ("d/x.pdf", &fake_bytes(2, 3000), false),
            ("ok.pdf", &fake_bytes(3, 3000), false),
        ],
        &[],
    );
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &c);
    assert_eq!(skip(&p, 0), reason("bad-path", Some(".pr-x/a.pdf")));
    assert_eq!(skip(&p, 1), reason("bad-path", Some("d")));
    assert_eq!(accepted(&p), vec![2]);
}

/// zip 的項目名稱用了反斜線（Windows PowerShell 5.1 的 `Compress-Archive` 這樣寫，zip 格式不允許），
/// 整份不收；原因以 `zip entry names use backslashes` 開頭，外殼靠它告訴使用者改用別的工具重新壓縮。
/// 引用不到的項目有反斜線也一樣（看到就是用錯工具做的）。
#[test]
fn zip_backslash_entry_names_unreadable() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one("pdf/a.pdf")]));
    let a = fake_bytes(1, 3000);
    let c = raw_zip(
        &out,
        &[(MANIFEST_NAME, &m, false), ("pdf\\a.pdf", &a, false)],
        &[],
    );
    let r = assert_unreadable(&c);
    assert!(r.starts_with("zip entry names use backslashes"), "{r}");
    let c = raw_zip(
        &out,
        &[
            (MANIFEST_NAME, &manifest_bytes(json!([one("a.pdf")])), false),
            ("a.pdf", &a, false),
            ("junk\\x.txt", b"x", false),
        ],
        &[],
    );
    let r = assert_unreadable(&c);
    assert!(r.starts_with("zip entry names use backslashes"), "{r}");
}

#[test]
fn zip_encrypted_referenced_bad_path() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one("secret.pdf"), one("link.pdf"), one("ok.pdf")]));
    let c = raw_zip(
        &out,
        &[
            (MANIFEST_NAME, &m, false),
            ("secret.pdf", &fake_bytes(1, 3000), true),
            ("ok.pdf", &fake_bytes(3, 3000), false),
        ],
        &[("link.pdf", "ok.pdf")],
    );
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &c);
    assert_eq!(skip(&p, 0), reason("bad-path", Some("secret.pdf")));
    assert_eq!(skip(&p, 1), reason("bad-path", Some("link.pdf")));
    assert_eq!(accepted(&p), vec![2]);
}

/// 把 zip 裡名為 `name` 的項目宣告的未壓縮大小改成 `size`（本地檔頭與中央目錄兩處）。
fn patch_declared_size(path: &Path, name: &str, size: u32) {
    let mut b = fs::read(path).unwrap();
    let n = name.as_bytes();
    let mut patched = 0;
    for i in 0..b.len().saturating_sub(46) {
        if b[i..i + 4] == [0x50, 0x4b, 0x03, 0x04] && b[i + 30..].starts_with(n) {
            b[i + 22..i + 26].copy_from_slice(&size.to_le_bytes());
            patched += 1;
        }
        if b[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] && b[i + 46..].starts_with(n) {
            b[i + 24..i + 28].copy_from_slice(&size.to_le_bytes());
            patched += 1;
        }
    }
    assert_eq!(patched, 2);
    fs::write(path, b).unwrap();
}

#[test]
fn zip_entry_over_declared_size_form_mismatch() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one("a.pdf"), one("b.pdf")]));
    let c = raw_zip(
        &out,
        &[
            (MANIFEST_NAME, &m, false),
            ("a.pdf", &fake_bytes(1, 3000), false),
            ("b.pdf", &fake_bytes(2, 3000), false),
        ],
        &[],
    );
    patch_declared_size(c.path(), "a.pdf", 2000);
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &c);
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("a.pdf")));
    assert_eq!(accepted(&p), vec![1]);
}

#[test]
fn zip_too_many_entries_unreadable() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("many.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file(MANIFEST_NAME, opts).unwrap();
    z.write_all(&manifest_bytes(json!([one("e0")]))).unwrap();
    for k in 0..50_000 {
        z.start_file(format!("e{k}"), opts).unwrap();
    }
    z.finish().unwrap();
    assert_unreadable(&ImportContainer::Zip(path));
}

#[test]
fn zip_manifest_not_at_root_unreadable() {
    let s = Src::new();
    s.pdf("pkg/a.pdf", 1);
    s.write(
        &format!("pkg/{MANIFEST_NAME}"),
        &manifest_bytes(json!([one("a.pdf")])),
    );
    let out = tempfile::tempdir().unwrap();
    assert_unreadable(&s.zip(&out));
    // 不是 zip
    let not_zip = out.path().join("not.zip");
    fs::write(&not_zip, b"PK nope").unwrap();
    assert_unreadable(&ImportContainer::Zip(not_zip));
    // 不存在
    assert_unreadable(&ImportContainer::Zip(out.path().join("missing.zip")));
}

// ---- 效能與上限 ----

/// zip 的項目名稱很深很長時，掃描的時間與記憶體和名稱長度成正比。舊寫法替每個 `/` 前面那一段各配置一份
/// 字串（平方）：3 個約 6 萬位元組的名稱，會吃到 2.6 GB、16.7 秒。
#[test]
fn zip_long_deep_entry_names_are_cheap() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one("a.pdf")]));
    let a = fake_bytes(1, 3000);
    let deep: Vec<String> = (0..3)
        .map(|k| format!("{k}/{}x", "a/".repeat(29_990)))
        .collect();
    let mut entries: Vec<(&str, &[u8], bool)> =
        vec![(MANIFEST_NAME, &m, false), ("a.pdf", &a, false)];
    for d in &deep {
        entries.push((d.as_str(), b"x", false));
    }
    let c = raw_zip(&out, &entries, &[]);
    let started = Instant::now();
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &c);
    let r = run(&mut lib, &p.preview_id);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(added_index(&r), vec![0]);
}

/// 別名去重是線性的。舊寫法逐一比對前面收過的（平方）：8 萬個別名要 4.45 秒，期間取消不了。
#[test]
fn many_aliases_are_cheap() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    let aliases: Vec<String> = (0..60_000).map(|k| format!("L{k:05}")).collect();
    let mut given = aliases.clone();
    given.push(" L00000 ".into());
    s.manifest(json!([{"aliases": given, "files": [src_pdf("a.pdf")]}]));
    let started = Instant::now();
    let sc = scan(&s.folder());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let (_d, mut lib) = temp_library();
    let p = lib.prepare_import(sc).unwrap();
    let r = run(&mut lib, &p.preview_id);
    assert_eq!(paper(&lib, &id_for(&r, 0)).aliases, aliases);
}

/// 掃描的第一輪（檢查欄位）每篇回報一次進度（還不知道總量，交 (0, 0)），取消在這一輪就有效。
#[test]
fn first_pass_reports_progress_and_can_cancel() {
    let s = Src::new();
    s.manifest(Value::Array(vec![json!({"files": 1}); 200]));
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let r = scan_import(&s.folder(), &mut |d, t| {
        calls.push((d, t));
        true
    })
    .unwrap();
    assert!(r.is_some());
    assert!(calls.len() > 200, "{}", calls.len());
    assert!(calls.iter().all(|c| *c == (0, 0)));
    let mut n = 0;
    let r = scan_import(&s.folder(), &mut |_, _| {
        n += 1;
        n < 10
    })
    .unwrap();
    assert!(r.is_none());
    assert_eq!(n, 10);
}

/// 匯入的重點筆記，`proofread` 取檔案屬性區的 `paperreader_proofread`（清單寫的不同時以檔案為準；
/// 沒寫是 false），`page_map` 一律 `marked-in-text`，`method` 是 `imported`，`source_sha256` 是 null。
/// 其他角色沒有屬性區，照清單。
#[test]
fn digest_provenance_follows_file_and_spec3() {
    let s = Src::new();
    s.pdf("p0.pdf", 50);
    s.pdf("p1.pdf", 51);
    s.md("yes.md", &digest_proofread("Y", true));
    s.md("plain.md", &digest_text("N"));
    s.md("t.md", "譯文");
    s.manifest(json!([
        {"files": [
            src_pdf("p0.pdf"),
            {"path": "yes.md", "role": "digest", "form": "markdown", "proofread": false, "page_map": "none"},
            {"path": "t.md", "role": "translation", "form": "markdown", "proofread": true, "page_map": "none"}
        ]},
        {"files": [
            src_pdf("p1.pdf"),
            {"path": "plain.md", "role": "digest", "form": "markdown", "proofread": true}
        ]}
    ]));
    let (_d, mut lib) = temp_library();
    let (_, r) = import(&mut lib, &s.folder());
    let got = |i: u32, k: usize| {
        let m = paper(&lib, &id_for(&r, i)).materials[k].clone();
        (
            m.provenance.method,
            m.provenance.proofread,
            m.provenance.page_map,
            m.provenance.source_sha256,
        )
    };
    let imported = Some("imported".to_string());
    let marked = Some("marked-in-text".to_string());
    assert_eq!(
        got(0, 1),
        (imported.clone(), Some(true), marked.clone(), None)
    );
    assert_eq!(
        got(0, 2),
        (imported.clone(), Some(true), Some("none".to_string()), None)
    );
    assert_eq!(got(1, 1), (imported, Some(false), marked, None));
}

/// `name` 的項目在中央目錄裡的 zip64 延伸欄位：未壓縮大小改成 `size`（寫的時候要 `large_file(true)`）。
fn patch_zip64_size(path: &Path, name: &str, size: u64) {
    let mut b = fs::read(path).unwrap();
    let n = name.as_bytes();
    let mut patched = 0;
    for i in 0..b.len().saturating_sub(46) {
        if b[i..i + 4] != [0x50, 0x4b, 0x01, 0x02] || !b[i + 46..].starts_with(n) {
            continue;
        }
        let le16 = |b: &[u8], at: usize| u16::from_le_bytes([b[at], b[at + 1]]) as usize;
        let (name_len, extra_len) = (le16(&b, i + 28), le16(&b, i + 30));
        let mut at = i + 46 + name_len;
        let end = at + extra_len;
        while at + 4 <= end {
            let (id, len) = (le16(&b, at), le16(&b, at + 2));
            if id == 1 {
                b[at + 4..at + 12].copy_from_slice(&size.to_le_bytes());
                patched += 1;
            }
            at += 4 + len;
        }
    }
    assert_eq!(patched, 1);
    fs::write(path, b).unwrap();
}

/// 清單以外的項目宣告的未壓縮大小超過 4 GiB → 整份不收。
#[test]
fn zip_entry_declared_over_4gib_unreadable() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("big.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file(MANIFEST_NAME, opts).unwrap();
    z.write_all(&manifest_bytes(json!([one("a.pdf")]))).unwrap();
    z.start_file("a.pdf", opts).unwrap();
    z.write_all(&fake_bytes(1, 3000)).unwrap();
    z.start_file("big.bin", opts.large_file(true)).unwrap();
    z.write_all(b"x").unwrap();
    z.finish().unwrap();
    let c = ImportContainer::Zip(path.clone());
    // 改之前照常收（這份 zip 讀得懂）
    let (_d, mut lib) = temp_library();
    assert_eq!(accepted(&preview(&mut lib, &c)), vec![0]);
    patch_zip64_size(&path, "big.bin", (4 << 30) + 1);
    let r = assert_unreadable(&c);
    assert!(r.contains("too large"), "{r}");
}

/// 清單以外各項宣告的未壓縮大小加起來超過 64 GiB → 整份不收。
#[test]
fn zip_entries_declared_over_64gib_total_unreadable() {
    let out = tempfile::tempdir().unwrap();
    let m = manifest_bytes(json!([one("a.pdf")]));
    let a = fake_bytes(1, 3000);
    let names: Vec<String> = (0..17).map(|k| format!("e{k:02}")).collect();
    let mut entries: Vec<(&str, &[u8], bool)> =
        vec![(MANIFEST_NAME, &m, false), ("a.pdf", &a, false)];
    for n in &names {
        entries.push((n.as_str(), b"x", false));
    }
    let c = raw_zip(&out, &entries, &[]);
    for n in &names {
        // 每項 4 GiB 減 2：每一項都在 4 GiB 以內，17 項加起來超過 64 GiB
        patch_declared_size(c.path(), n, u32::MAX - 1);
    }
    let r = assert_unreadable(&c);
    assert!(r.contains("too large in total"), "{r}");
}

/// Deflate 的項目實際解出來比宣告的大 → `form-mismatch`（原本的測試只用了不壓縮的項目）。
#[test]
fn zip_deflate_entry_over_declared_size_form_mismatch() {
    let s = Src::new();
    s.pdf("a.pdf", 1);
    s.pdf("b.pdf", 2);
    s.manifest(json!([one("a.pdf"), one("b.pdf")]));
    let out = tempfile::tempdir().unwrap();
    let c = s.zip(&out);
    patch_declared_size(c.path(), "a.pdf", 2000);
    let (_d, mut lib) = temp_library();
    let p = preview(&mut lib, &c);
    assert_eq!(skip(&p, 0), reason("form-mismatch", Some("a.pdf")));
    assert_eq!(accepted(&p), vec![1]);
}
