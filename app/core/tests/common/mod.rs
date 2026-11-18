#![allow(dead_code)]
//! 測試共用的輔助函式。測試不依賴真正的 PDF，任意位元組就是「原文」。

use paperreader_core::format::*;
use paperreader_core::ids::new_id;
use paperreader_core::jsonio::to_canonical_json;
use paperreader_core::local::LocalLibrary;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub const DEVICE: &str = "01JCK3T1A2B3C4D5E6F7G8H9JK";

/// 在空的暫存資料夾開啟一個文獻庫。
pub fn temp_library() -> (TempDir, LocalLibrary) {
    let dir = tempfile::tempdir().unwrap();
    let lib = LocalLibrary::open(dir.path(), DEVICE).unwrap();
    (dir, lib)
}

pub fn sample_paper(id: &str) -> PaperFile {
    PaperFile {
        format: 1,
        id: id.to_string(),
        short_name: None,
        aliases: vec![],
        added: Some("2026-11-13T08:30:11.045Z".into()),
        bib: Bib {
            title: Some("Sample".into()),
            authors: vec![Author {
                family: Some("Zhao".into()),
                given: Some("Yian".into()),
                ..Default::default()
            }],
            year: Some(2024),
            modified: Some("2026-11-13T08:30:11.045Z".into()),
            modified_by: Some(DEVICE.into()),
            ..Default::default()
        },
        materials: vec![source_material("source.pdf", None, None)],
        groups: vec![],
        groups_modified: None,
        groups_modified_by: None,
        extra: Map::new(),
    }
}

pub fn source_material(path: &str, bytes: Option<u64>, sha256: Option<&str>) -> Material {
    Material {
        id: new_id(),
        role: ROLE_SOURCE.into(),
        form: FORM_PDF.into(),
        path: path.into(),
        sha256: sha256.map(String::from),
        bytes,
        pages: None,
        pages_modified: None,
        pages_modified_by: None,
        title: None,
        provenance: Provenance {
            method: Some(METHOD_ADDED_BY_USER.into()),
            time: Some("2026-11-13T08:30:11.045Z".into()),
            origin_name: Some("orig.pdf".into()),
            ..Default::default()
        },
        extra: Map::new(),
    }
}

/// 寫 `<root>/papers/<folder>/paper.json`，回傳資料夾路徑。
pub fn write_paper(root: &Path, folder: &str, paper: &PaperFile) -> PathBuf {
    let dir = root.join("papers").join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("paper.json"), to_canonical_json(paper).unwrap()).unwrap();
    dir
}

/// 可重現的假位元組。`seed` 不同內容就不同。長度至少 5 時，前 5 個位元組是 `%PDF-`
/// （核心加入時驗檔頭；長度不變）。
pub fn fake_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut v: Vec<u8> = (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect();
    if len >= 5 {
        v[..5].copy_from_slice(b"%PDF-");
    }
    v
}

/// 在 `dir` 建立一個假 PDF（任意位元組），回傳路徑。
pub fn fake_pdf(dir: &Path, name: &str, len: usize) -> PathBuf {
    fake_pdf_seeded(dir, name, len, len as u64)
}

pub fn fake_pdf_seeded(dir: &Path, name: &str, len: usize, seed: u64) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, fake_bytes(seed, len)).unwrap();
    p
}

/// 整個資料夾的樣子：相對路徑（用 `/`）對內容；資料夾對 `None`。
pub fn snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if e.file_type().unwrap().is_dir() {
                out.insert(rel, None);
                walk(base, &p, out);
            } else {
                out.insert(rel, Some(fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// 路徑裡有任何一段以 `.` 開頭的項目（別的工具的隱藏檔）。
pub fn hidden_part(snap: &BTreeMap<String, Option<Vec<u8>>>) -> BTreeMap<String, Option<Vec<u8>>> {
    snap.iter()
        .filter(|(k, _)| k.split('/').any(|c| c.starts_with('.')))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// 加入一篇有一項原文材料的文獻，回傳（文獻代號，材料代號）。`seed` 不同內容就不同。
pub fn add_paper(
    lib: &mut LocalLibrary,
    seed: u64,
    title: &str,
    pages: Option<u32>,
) -> (String, String) {
    use paperreader_core::store::AddOutcome;
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "a.pdf", 300, seed);
    let draft = BibDraft {
        title: Some(title.into()),
        ..Default::default()
    };
    let id = match lib.add_paper(&pdf, draft, pages).unwrap() {
        AddOutcome::Added(id) => id,
        other => panic!("{other:?}"),
    };
    let mid = lib.get_paper(&id).unwrap().0.materials[0].id.clone();
    (id, mid)
}

/// 手寫一行閱讀事件（不經過核心的檢查），結尾有換行。
pub fn log_line(
    id: &str,
    t: &str,
    device: &str,
    paper: &str,
    kind: &str,
    data: serde_json::Value,
) -> String {
    serde_json::json!({"id": id, "t": t, "device": device, "paper": paper, "type": kind, "data": data})
        .to_string()
        + "\n"
}

/// 寫 `logs/reading-<裝置>-<年月>.jsonl`（覆蓋）。`month` 例如 `2026-11`。
pub fn write_log(root: &Path, device: &str, month: &str, text: &str) -> PathBuf {
    let dir = root.join("logs");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("reading-{device}-{month}.jsonl"));
    fs::write(&p, text).unwrap();
    p
}

/// 目前所有紀錄檔的內容，供「沒有多出任何一行」的比對。
pub fn logs_snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let dir = root.join("logs");
    if !dir.exists() {
        return BTreeMap::new();
    }
    snapshot(&dir)
}

// ---- 匯出與還原的測試共用（工作 7、8）----

use paperreader_core::notes::Quote;
use paperreader_core::store::HighlightDraft;
use std::io::Read;
use zip::CompressionMethod;

/// 建一個有兩篇文獻、一份譯本、群組、筆記檔、紀錄檔、回收區一篇的文獻庫。
pub fn build_library() -> (tempfile::TempDir, LocalLibrary) {
    let (dir, mut lib) = temp_library();
    let (p1, m1) = add_paper(&mut lib, 1, "Alpha Paper", Some(10));
    let (_p2, _m2) = add_paper(&mut lib, 2, "Beta Paper", Some(5));
    let (p3, _m3) = add_paper(&mut lib, 3, "Gamma Trash", None);
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf_seeded(src.path(), "t.pdf", 700, 99);
    let tok = lib.get_paper(&p1).unwrap().1;
    lib.add_translation(&p1, &pdf, None, &tok).unwrap();
    let (_, gt) = lib.list_groups().unwrap();
    let (g, _) = lib.create_group("Vision", &gt).unwrap();
    let tok = lib.get_paper(&p1).unwrap().1;
    lib.add_to_group(&p1, &g.id, &tok).unwrap();
    lib.add_highlight(
        &p1,
        HighlightDraft {
            material: m1.clone(),
            page_index: 1,
            page_label: Some("2".into()),
            rects: vec![[1.0, 2.0, 3.0, 4.0]],
            next_page_rects: vec![],
            position: None,
            quote: Quote {
                exact: "quoted".into(),
                prefix: None,
                suffix: None,
                extra: Default::default(),
            },
            fingerprint: None,
            comment: "筆記".into(),
        },
        None,
    )
    .unwrap();
    lib.record_open(&p1, &m1).unwrap();
    lib.remove_paper(&p3).unwrap();
    (dir, lib)
}

/// zip 裡每一項的（名稱 → 內容），依 zip 的順序。
pub fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>, CompressionMethod)> {
    let f = fs::File::open(path).unwrap();
    let mut z = zip::ZipArchive::new(f).unwrap();
    let mut out = Vec::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let name = String::from_utf8(e.name_raw().to_vec()).unwrap();
        let method = e.compression();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).unwrap();
        out.push((name, buf, method));
    }
    out
}

/// 文獻庫裡「該放進備份」的檔案：相對路徑（用 `/`）→ 內容。排除暫存與別的工具的隱藏項目（`.trash/` 例外）。
pub fn backup_worthy_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    snapshot(root)
        .into_iter()
        .filter_map(|(k, v)| {
            let v = v?;
            let segs: Vec<&str> = k.split('/').collect();
            let hidden = segs
                .iter()
                .enumerate()
                .any(|(i, s)| s.starts_with('.') && !(i == 0 && *s == ".trash"));
            (!hidden).then_some((k, v))
        })
        .collect()
}

// ---- 同步包的測試共用----

use paperreader_core::store::StrokeDraft;

/// 第二台裝置的代號。
pub const DEVICE_B: &str = "01JD9Z7Q1A2B3C4D5E6F7G8H9K";

pub fn highlight_draft(material: &str, page: u32, comment: &str) -> HighlightDraft {
    HighlightDraft {
        material: material.to_string(),
        page_index: page,
        page_label: None,
        rects: vec![[1.0, 2.0, 3.0, 4.0]],
        next_page_rects: vec![],
        position: None,
        quote: Quote {
            exact: format!("quote on page {page}"),
            prefix: None,
            suffix: None,
            extra: Default::default(),
        },
        fingerprint: None,
        comment: comment.to_string(),
    }
}

pub fn stroke_draft(material: &str, page: u32) -> StrokeDraft {
    StrokeDraft {
        material: material.to_string(),
        page_index: page,
        color: "#ff0000".into(),
        width: 2.0,
        opacity: 1.0,
        points: vec![[10.0, 10.0], [20.0, 20.0]],
    }
}

/// `logs/changes-<裝置>.jsonl` 的路徑。
pub fn changes_path(root: &Path, device: &str) -> PathBuf {
    root.join("logs").join(format!("changes-{device}.jsonl"))
}

/// 一台裝置的變更紀錄檔的每一行（JSON 值）；檔案不存在是空的。
pub fn change_lines(root: &Path, device: &str) -> Vec<serde_json::Value> {
    match fs::read_to_string(changes_path(root, device)) {
        Ok(t) => t
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 把一台裝置的變更紀錄檔整份換掉（測試直接改時間用）。
pub fn write_change_lines(root: &Path, device: &str, lines: &[serde_json::Value]) {
    let mut s = String::new();
    for l in lines {
        s.push_str(&l.to_string());
        s.push('\n');
    }
    fs::create_dir_all(root.join("logs")).unwrap();
    fs::write(changes_path(root, device), s).unwrap();
}

/// 現在往前推 `minutes` 分鐘的時間字串。
pub fn minutes_ago(minutes: i64) -> String {
    use paperreader_core::ids::{format_utc_millis, now_utc_millis};
    format_utc_millis(now_utc_millis() - minutes * 60_000).unwrap()
}

/// 兩台裝置上的同一個文獻庫：A 用 `build_library()` 建，B 從 A 的備份檔還原（裝置代號 `DEVICE_B`）。
pub struct Pair {
    pub a_dir: TempDir,
    pub a: LocalLibrary,
    pub b_dir: TempDir,
    pub b: LocalLibrary,
    pub out: TempDir,
}

pub fn pair() -> Pair {
    let (a_dir, a) = build_library();
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("seed.zip");
    a.export_backup(&zip, &mut |_, _| {}).unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let b_root = b_dir.path().join("lib");
    paperreader_core::backup::restore_backup(&zip, &b_root, DEVICE_B).unwrap();
    let b = LocalLibrary::open(&b_root, DEVICE_B).unwrap();
    Pair {
        a_dir,
        a,
        b_dir,
        b,
        out,
    }
}

impl Pair {
    pub fn a_root(&self) -> PathBuf {
        self.a_dir.path().to_path_buf()
    }
    pub fn b_root(&self) -> PathBuf {
        self.b_dir.path().join("lib")
    }
}

/// 從 `from` 匯出（只帶變更）一份同步包。
pub fn export_pkg(from: &mut LocalLibrary, out: &Path) -> PathBuf {
    let p = out.join(format!("{}.zip", new_id()));
    from.export_sync_package(
        &p,
        paperreader_core::store::SyncMode::Changes,
        None,
        &mut |_, _| {},
    )
    .unwrap();
    p
}

/// 匯入，衝突全部照預設。
pub fn import_defaults(
    to: &mut LocalLibrary,
    pkg: &Path,
) -> paperreader_core::store::SyncImportReport {
    let preview = to.prepare_sync_import(pkg, &mut |_, _| {}).unwrap();
    let choices: Vec<(String, paperreader_core::store::ConflictChoice)> = preview
        .conflicts
        .iter()
        .map(|c| (c.id.clone(), c.default))
        .collect();
    to.apply_sync_import(&preview.preview_id, &choices).unwrap()
}

/// `from` 匯出、`to` 匯入（預設的選擇）。
pub fn sync_once(
    from: &mut LocalLibrary,
    to: &mut LocalLibrary,
    out: &Path,
) -> paperreader_core::store::SyncImportReport {
    let pkg = export_pkg(from, out);
    import_defaults(to, &pkg)
}

/// 「兩邊相同」的比法：可同步的檔的內容，不比紀錄檔與資料夾名稱。
/// 文獻依代號：位置、`paper.json` 的 JSON 值、每個材料檔的指紋、`ink.json` 與譯文疊加檔去掉檔層級的
/// `modified`、`modified_by`；筆記檔依 `paperreader_paper` 去掉屬性區的兩行時間；`library.json` 的 JSON 值。
pub fn sync_state(lib: &LocalLibrary, root: &Path) -> BTreeMap<String, serde_json::Value> {
    use paperreader_core::jsonio::sha256_hex;
    use serde_json::{Value, json};
    let mut out = BTreeMap::new();
    let read = |p: &Path| -> Value { serde_json::from_slice(&fs::read(p).unwrap()).unwrap() };
    out.insert("library".to_string(), read(&root.join("library.json")));
    let strip = |mut v: Value| -> Value {
        if let Some(o) = v.as_object_mut() {
            o.remove("modified");
            o.remove("modified_by");
        }
        v
    };
    let mut dirs: Vec<(String, String, PathBuf)> = Vec::new();
    for p in lib.list_papers() {
        dirs.push((
            p.id.clone(),
            "papers".into(),
            root.join("papers").join(&p.folder),
        ));
    }
    let mut seen: std::collections::HashSet<String> = dirs.iter().map(|d| d.0.clone()).collect();
    for t in lib.list_trash().unwrap() {
        if seen.insert(t.paper_id.clone()) {
            dirs.push((
                t.paper_id.clone(),
                "trash".into(),
                root.join(".trash").join(&t.folder),
            ));
        }
    }
    for (id, place, dir) in dirs {
        let paper = read(&dir.join("paper.json"));
        let mut files = serde_json::Map::new();
        for m in paper["materials"].as_array().unwrap() {
            let path = m["path"].as_str().unwrap();
            let f = dir.join(path);
            let v = if m["form"] == json!("overlay") {
                let mut v = read(&f);
                for s in v["segments"].as_array_mut().unwrap() {
                    let _ = s;
                }
                strip(v)
            } else {
                json!(fs::read(&f).map(|b| sha256_hex(&b)).ok())
            };
            files.insert(m["id"].as_str().unwrap().to_string(), v);
        }
        let ink = dir.join("ink.json");
        let ink = if ink.is_file() {
            strip(read(&ink))
        } else {
            Value::Null
        };
        let refs = dir.join("references.json");
        let refs = if refs.is_file() {
            strip(read(&refs))
        } else {
            Value::Null
        };
        let text = dir.join("text.json");
        let text = if text.is_file() {
            read(&text)
        } else {
            Value::Null
        };
        out.insert(
            format!("paper:{id}"),
            json!({"place": place, "paper": paper, "files": files, "ink": ink, "references": refs, "text": text}),
        );
    }
    let notes_dir = root.join(&lib.library_file().notes_dir);
    if let Ok(rd) = fs::read_dir(&notes_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".md") {
                continue;
            }
            let text = fs::read_to_string(e.path()).unwrap();
            let Some(pid) = paperreader_core::notes::read_paper_id(&text) else {
                continue;
            };
            let kept: Vec<&str> = text
                .lines()
                .filter(|l| {
                    !l.starts_with("paperreader_modified:")
                        && !l.starts_with("paperreader_modified_by:")
                })
                .collect();
            out.insert(format!("notes:{pid}"), json!(kept.join("\n")));
        }
    }
    out
}

// ---- 合併層的純計算測試用 ----

use paperreader_core::store::{ConflictChoice, ConflictKind};
use paperreader_core::sync::merge::{MergeCtx, conflict_key};

/// 依篇名找文獻的代號。
pub fn find(lib: &LocalLibrary, title: &str) -> String {
    lib.list_papers()
        .into_iter()
        .find(|p| p.bib.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no paper {title}"))
        .id
}

/// 同步包裡的項目（名稱, 內容），不含 `manifest.json`。
pub type Entries = Vec<(String, Vec<u8>)>;

/// 照原本的同步包重做一份 zip：`f` 可以改清單與項目。
pub fn rewrite(src: &Path, dst: &Path, f: impl FnOnce(&mut serde_json::Value, &mut Entries)) {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut entries: Entries = zip_entries(src)
        .into_iter()
        .map(|(n, b, _)| (n, b))
        .collect();
    let mi = entries.iter().position(|e| e.0 == "manifest.json").unwrap();
    let (_, mb) = entries.remove(mi);
    let mut manifest: serde_json::Value = serde_json::from_slice(&mb).unwrap();
    f(&mut manifest, &mut entries);
    let file = fs::File::create(dst).unwrap();
    let mut z = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, b) in &entries {
        z.start_file(n.as_str(), opts).unwrap();
        z.write_all(b).unwrap();
    }
    z.start_file("manifest.json", opts).unwrap();
    z.write_all(manifest.to_string().as_bytes()).unwrap();
    z.finish().unwrap();
}

/// 寫入的時間（測試裡固定）。
pub const NOW: &str = "2026-12-01T00:00:00.000Z";

/// 本機是 `DEVICE_B`、匯入的是 `DEVICE`。記錄代號依序 `01REC…`。
pub fn mctx(choices: &[(ConflictKind, Option<&str>, Option<&str>, ConflictChoice)]) -> MergeCtx {
    let mut n = 0u32;
    MergeCtx::new(
        NOW.into(),
        "20261201T000000Z".into(),
        DEVICE_B.into(),
        DEVICE.into(),
        choices
            .iter()
            .map(|(k, p, i, c)| (conflict_key(*k, *p, *i), *c))
            .collect(),
        Box::new(move || {
            n += 1;
            format!("01REC{n:021}")
        }),
    )
}

/// 一筆手寫的變更記錄。
pub fn rec(
    id: &str,
    t: &str,
    device: &str,
    paper: Option<&str>,
    kind: &str,
    data: serde_json::Value,
) -> paperreader_core::changes::ChangeRecord {
    paperreader_core::changes::ChangeRecord {
        id: id.into(),
        t: t.into(),
        device: device.into(),
        paper: paper.map(String::from),
        kind: kind.into(),
        data,
        extra: Default::default(),
    }
}
