//! 重點筆記（`digest`）：建立、讀、刪一點、標為已校對；
//! 原子寫入、版本記號、衝突不覆寫、讀不懂不覆寫、不認得的內容保留、畫面交回的結果先檢查
//! （結構、大小、頁碼範圍）；`check`、備份還原、回收區、同步包認得它。

mod common;

use common::*;
use paperreader_core::backup::restore_backup;
use paperreader_core::changes::DIGEST_POINT_DELETED;
use paperreader_core::digest::*;
use paperreader_core::error::LibError;
use paperreader_core::jsonio::{VersionToken, sha256_hex};
use paperreader_core::local::LocalLibrary;
use paperreader_core::store::{ChangeFilter, CheckLevel};
use std::fs;
use std::path::PathBuf;

struct Fx {
    dir: tempfile::TempDir,
    lib: LocalLibrary,
    paper: String,
    source: String,
}

fn setup() -> Fx {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "Zhao Paper", Some(10));
    Fx {
        dir,
        lib,
        paper,
        source,
    }
}

fn section(id: &str, label: &str) -> DigestSection {
    DigestSection {
        id: id.into(),
        label: label.into(),
    }
}

/// 內建「標準」的五區（原先固定的格式）。
fn sections() -> Vec<DigestSection> {
    vec![
        section("question", "研究問題"),
        section("method", "方法"),
        section("results", "主要結果"),
        section("limitations", "限制"),
        section("quotes", "值得記下的引句或數字"),
    ]
}

fn point(section: &str, text: &str, pages: &[u32]) -> PointDraft {
    PointDraft {
        section: section.into(),
        text: text.into(),
        pages: pages.to_vec(),
        segments: vec!["g0001".into()],
    }
}

impl Fx {
    fn folder(&self) -> PathBuf {
        self.dir.path().join("papers").join("Zhao Paper")
    }
    fn draft(&self) -> DigestDraft {
        DigestDraft {
            source_material: self.source.clone(),
            service: "Claude（Anthropic）".into(),
            provider: "anthropic".into(),
            model: "claude-test".into(),
            lang: "zh-TW".into(),
            title: "Zhao Paper".into(),
            sections: sections(),
            points: vec![
                point("question", "即時偵測器的速度與準確率難以兼顧。", &[0]),
                point("method", "用混合編碼器把多尺度特徵分開處理。", &[2, 3]),
                point("results", "在 COCO 上 53.1 AP，比 YOLOv8 快 2 倍。", &[6]),
                point("limitations", "小物件的表現仍落後。", &[8]),
                point(
                    "quotes",
                    "「we propose the first real-time end-to-end detector」",
                    &[0],
                ),
            ],
        }
    }
    fn create(&mut self) -> CreatedDigest {
        let token = self.lib.get_paper(&self.paper).unwrap().1;
        let d = self.draft();
        self.lib.create_digest(&self.paper, d, &token).unwrap()
    }
    fn digest_path(&self) -> PathBuf {
        self.folder().join("digest.md")
    }
    fn text(&self) -> String {
        fs::read_to_string(self.digest_path()).unwrap()
    }
    fn material(&self, id: &str) -> paperreader_core::format::Material {
        self.lib
            .get_paper(&self.paper)
            .unwrap()
            .0
            .materials
            .into_iter()
            .find(|m| m.id == id)
            .unwrap()
    }
    fn paper_json(&self) -> Vec<u8> {
        fs::read(self.folder().join("paper.json")).unwrap()
    }
}

fn is_invalid(e: &LibError) -> bool {
    matches!(e, LibError::InvalidInput { .. })
}

// ---- 建立 ----

#[test]
fn create_writes_markdown_and_lists_the_material() {
    let mut fx = setup();
    let c = fx.create();
    let bytes = fs::read(fx.digest_path()).unwrap();
    let m = fx.material(&c.material_id);
    assert_eq!(m.role, ROLE_DIGEST);
    assert_eq!(m.form, FORM_MARKDOWN);
    assert_eq!(m.path, "digest.md");
    // 照譯文疊加檔的做法：檔案可以在筆記工具裡改，指紋與大小不記
    assert_eq!(m.sha256, None);
    assert_eq!(m.bytes, None);
    assert_eq!(m.pages, None);
    assert_eq!(m.provenance.method.as_deref(), Some("generated"));
    assert_eq!(m.provenance.proofread, Some(false));
    assert_eq!(m.provenance.page_map.as_deref(), Some("marked-in-text"));
    let src = fx.material(&fx.source);
    assert_eq!(m.provenance.source_sha256, src.sha256);
    assert_eq!(c.digest_token.0, sha256_hex(&bytes));
    assert_eq!(
        c.paper_token,
        fx.lib.get_paper(&fx.paper).unwrap().1,
        "the returned paper token is the current one"
    );
}

#[test]
fn markdown_is_readable_by_people_and_links_back_to_pages() {
    let mut fx = setup();
    let c = fx.create();
    let text = fx.text();
    assert!(text.starts_with("---\n"), "{text}");
    for key in [
        "paperreader_digest: 1",
        &format!("paperreader_digest_of: {}", fx.paper),
        &format!("paperreader_source_material: {}", fx.source),
        "paperreader_service: \"Claude（Anthropic）\"",
        "paperreader_provider: anthropic",
        "paperreader_model: \"claude-test\"",
        "paperreader_lang: zh-TW",
        "paperreader_proofread: false",
        &format!("paperreader_created_by: {DEVICE}"),
    ] {
        assert!(text.contains(key), "missing {key:?} in\n{text}");
    }
    assert!(text.contains("\n# Zhao Paper\n"));
    assert!(text.contains("\n## 研究問題\n"));
    assert!(text.contains("\n## 值得記下的引句或數字\n"));
    // 每一點：一行清單，後面是回原文那一頁的連結，再後面是看不見的資料
    let line = text
        .lines()
        .find(|l| l.contains("53.1 AP"))
        .expect("the results point");
    let link = format!(
        "[p. 7](paperreader://open?paper={}&material={}&page=7)",
        fx.paper, fx.source
    );
    assert!(line.starts_with("- 在 COCO 上 53.1 AP"), "{line}");
    assert!(line.contains(&link), "{line}");
    assert!(
        line.contains("<!-- paperreader:point {") && line.ends_with(" -->"),
        "{line}"
    );
    // 跨兩頁的一點有兩個連結
    let two = text.lines().find(|l| l.contains("混合編碼器")).unwrap();
    assert!(two.contains("page=3)") && two.contains("page=4)"), "{two}");
    // 依區段的順序：研究問題、方法、主要結果、限制、引句
    let at = |s: &str| text.find(s).unwrap();
    assert!(at("## 研究問題") < at("## 方法"));
    assert!(at("## 方法") < at("## 主要結果"));
    assert!(at("## 主要結果") < at("## 限制"));
    assert!(at("## 限制") < at("## 值得記下的引句或數字"));
    let _ = c;
}

#[test]
fn read_returns_points_in_order_with_ids_times_and_pages() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(token, c.digest_token);
    assert_eq!(doc.format, 1);
    assert_eq!(doc.paper.as_deref(), Some(fx.paper.as_str()));
    assert_eq!(doc.source_material.as_deref(), Some(fx.source.as_str()));
    assert_eq!(doc.service.as_deref(), Some("Claude（Anthropic）"));
    assert_eq!(doc.provider.as_deref(), Some("anthropic"));
    assert_eq!(doc.model.as_deref(), Some("claude-test"));
    assert_eq!(doc.lang.as_deref(), Some("zh-TW"));
    assert_eq!(doc.title.as_deref(), Some("Zhao Paper"));
    assert!(!doc.proofread);
    assert!(doc.created.is_some() && doc.created == doc.modified);
    assert_eq!(doc.created_by.as_deref(), Some(DEVICE));
    let sections: Vec<&str> = doc.points.iter().map(|p| p.section.as_str()).collect();
    assert_eq!(
        sections,
        ["question", "method", "results", "limitations", "quotes"]
    );
    let r = &doc.points[2];
    assert_eq!(r.text, "在 COCO 上 53.1 AP，比 YOLOv8 快 2 倍。");
    assert_eq!(r.pages, vec![6]);
    assert_eq!(r.segments, vec!["g0001".to_string()]);
    assert_eq!(doc.points[1].pages, vec![2, 3]);
    assert!(doc.unreadable.is_empty());
}

#[test]
fn each_point_has_its_own_id_and_time() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let mut ids: Vec<&str> = doc.points.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.iter().all(|i| i.starts_with("d-") && i.len() > 10));
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 5, "ids are unique");
    assert!(doc.points.iter().all(|p| !p.created.is_empty()));
}

#[test]
fn points_are_grouped_by_section_whatever_order_they_come_in() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let mut d = fx.draft();
    d.points.reverse();
    let c = fx.lib.create_digest(&fx.paper, d, &token).unwrap();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let sections: Vec<&str> = doc.points.iter().map(|p| p.section.as_str()).collect();
    assert_eq!(
        sections,
        ["question", "method", "results", "limitations", "quotes"]
    );
}

// ---- 自訂的區段----

fn custom_draft(fx: &Fx) -> DigestDraft {
    let mut d = fx.draft();
    d.sections = vec![
        section("question", "研究問題"),
        section("contradictions", "原文前後矛盾的地方"),
        section("results", "主要結果"),
    ];
    d.points = vec![
        point("results", "在 COCO 上 53.1 AP。", &[6]),
        point(
            "contradictions",
            "第 3 頁說 4 層，第 7 頁說 6 層。",
            &[2, 6],
        ),
        point("question", "速度與準確率難以兼顧。", &[0]),
    ];
    d
}

#[test]
fn custom_sections_are_recorded_in_order_and_read_back() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let d = custom_draft(&fx);
    let c = fx.lib.create_digest(&fx.paper, d.clone(), &token).unwrap();
    let text = fx.text();
    // 屬性區記下區段的代號與名稱（一行 JSON，依順序）
    let line = text
        .lines()
        .find(|l| l.starts_with("paperreader_sections: "))
        .expect("the sections line");
    assert_eq!(
        line,
        r#"paperreader_sections: [{"id":"question","label":"研究問題"},{"id":"contradictions","label":"原文前後矛盾的地方"},{"id":"results","label":"主要結果"}]"#
    );
    // 標題照區段的順序，沒有點的區段不寫標題
    let at = |s: &str| text.find(s).unwrap();
    assert!(at("\n## 研究問題\n") < at("\n## 原文前後矛盾的地方\n"));
    assert!(at("\n## 原文前後矛盾的地方\n") < at("\n## 主要結果\n"));
    assert!(!text.contains("## 方法"));
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.sections, d.sections);
    let order: Vec<&str> = doc.points.iter().map(|p| p.section.as_str()).collect();
    assert_eq!(order, ["question", "contradictions", "results"]);
    assert_eq!(doc.points[1].pages, vec![2, 6]);
    // 刪一點、標已校對都不動區段那一行
    let t = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    fx.lib
        .set_digest_proofread(&fx.paper, &c.material_id, true, &t)
        .unwrap();
    assert!(fx.text().contains(line));
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.sections, d.sections);
}

#[test]
fn a_file_without_the_sections_line_reads_as_the_five_default_sections() {
    // 這一版之前寫的重點筆記沒有 `paperreader_sections`：照原本的五區讀，名稱留空（畫面用自己的文字表）
    let mut fx = setup();
    let c = fx.create();
    let text = fx.text();
    let line = text
        .lines()
        .find(|l| l.starts_with("paperreader_sections: "))
        .unwrap()
        .to_string();
    fs::write(fx.digest_path(), text.replace(&format!("{line}\n"), "")).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let ids: Vec<&str> = doc.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, DEFAULT_SECTIONS);
    assert!(doc.sections.iter().all(|s| s.label.is_empty()));
    assert_eq!(doc.points.len(), 5);
    // 照樣可以改，改了也不補上區段那一行
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    assert!(!fx.text().contains("paperreader_sections"));
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn an_unreadable_sections_line_is_listed_and_the_headings_are_used() {
    // 讀不懂時不默默換成五區：列進「讀不懂」，區段照每一點前面的 `## ` 標題與順序
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let c = fx
        .lib
        .create_digest(&fx.paper, custom_draft(&fx), &token)
        .unwrap();
    let text = fx.text();
    let line = text
        .lines()
        .find(|l| l.starts_with("paperreader_sections: "))
        .unwrap()
        .to_string();
    fs::write(
        fx.digest_path(),
        text.replace(&line, "paperreader_sections: [broken"),
    )
    .unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert!(doc.sections_unreadable);
    assert_eq!(doc.unreadable.len(), 1);
    assert!(doc.unreadable[0].text.starts_with("paperreader_sections"));
    assert_eq!(
        doc.sections,
        vec![
            section("question", "研究問題"),
            section("contradictions", "原文前後矛盾的地方"),
            section("results", "主要結果"),
        ]
    );
    assert_eq!(doc.points.len(), 3, "the points are still read");
    // 改檔不動那一行
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    assert!(fx.text().contains("paperreader_sections: [broken"));
}

#[test]
fn a_sections_list_rewritten_as_block_yaml_is_still_read() {
    // 筆記工具（Obsidian）改屬性時可能把一行 JSON 改寫成區塊式的 YAML
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let d = custom_draft(&fx);
    let c = fx.lib.create_digest(&fx.paper, d.clone(), &token).unwrap();
    let text = fx.text();
    let line = text
        .lines()
        .find(|l| l.starts_with("paperreader_sections: "))
        .unwrap()
        .to_string();
    let block = "paperreader_sections:\n  - id: question\n    label: 研究問題\n  - id: \"contradictions\"\n    label: '原文前後矛盾的地方'\n  - label: \"主要結果\"\n    id: results";
    fs::write(fx.digest_path(), text.replace(&line, block)).unwrap();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert!(!doc.sections_unreadable, "{:?}", doc.unreadable);
    assert_eq!(doc.sections, d.sections);
    assert!(doc.unreadable.is_empty());
}

#[test]
fn section_labels_must_differ() {
    // 同名的兩區在筆記工具裡分不出來，合併時也可能接錯標題
    let mut fx = setup();
    let mut d = fx.draft();
    d.sections[1].label = d.sections[0].label.clone();
    rejects(&mut fx, d);
}

#[test]
fn section_labels_with_quotes_and_backslashes_round_trip() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let mut d = fx.draft();
    d.sections[4].label = r#"「值得記」的 "quotes" \ 與 #號"#.into();
    let c = fx.lib.create_digest(&fx.paper, d.clone(), &token).unwrap();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.sections, d.sections);
    assert!(
        fx.text()
            .contains(&format!("\n## {}\n", d.sections[4].label))
    );
}

#[test]
fn a_point_is_checked_against_the_sections_of_its_digest() {
    let ss = sections();
    let p = point("contradictions", "x。", &[0]);
    assert!(validate_point(&p, &ss, 10).is_err());
    let mut more = ss.clone();
    more.push(section("contradictions", "原文前後矛盾的地方"));
    assert!(validate_point(&p, &more, 10).is_ok());
    assert!(validate_point(&point("question", "x。", &[10]), &ss, 10).is_err());
}

#[test]
fn text_with_markdown_characters_round_trips() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let mut d = fx.draft();
    d.points = vec![point(
        "results",
        "F1 = 0.9 [12], *p* < 0.05 | a_b \\ 「引號」 <b>",
        &[1],
    )];
    let c = fx.lib.create_digest(&fx.paper, d, &token).unwrap();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(
        doc.points[0].text,
        "F1 = 0.9 [12], *p* < 0.05 | a_b \\ 「引號」 <b>"
    );
}

// ---- 建立時的檢查（架構 5.3 規則 4）----

type Edit = Box<dyn Fn(&mut DigestDraft)>;

fn rejects(fx: &mut Fx, d: DigestDraft) {
    let before = snapshot(fx.dir.path());
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let e = fx.lib.create_digest(&fx.paper, d, &token).unwrap_err();
    assert!(is_invalid(&e), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before, "nothing is written");
}

#[test]
fn create_rejects_a_page_beyond_the_source() {
    let mut fx = setup();
    let mut d = fx.draft();
    d.points[0].pages = vec![10]; // 原文 10 頁：頁序 0..=9
    rejects(&mut fx, d);
}

#[test]
fn create_rejects_bad_structure() {
    let mut fx = setup();
    let base = fx.draft();
    let cases: Vec<Edit> = vec![
        Box::new(|d| d.points[0].section = "summary".into()),
        Box::new(|d| d.points[0].text = "   ".into()),
        Box::new(|d| d.points[0].text = "a\nb".into()),
        Box::new(|d| d.points[0].text = "x <!-- paperreader:point {} --> y".into()),
        Box::new(|d| d.points[0].text = "x --> y".into()),
        Box::new(|d| d.points[0].text = "長".repeat(MAX_POINT_CHARS + 1)),
        Box::new(|d| d.points[0].pages = vec![]),
        Box::new(|d| d.points[0].pages = (0..9).collect()),
        Box::new(|d| d.points[0].segments = vec!["bad id!".into()]),
        Box::new(|d| d.points.clear()),
        Box::new(|d| {
            let p = d.points[0].clone();
            d.points = vec![p; MAX_POINTS + 1];
        }),
        Box::new(|d| d.sections[1].label = String::new()),
        Box::new(|d| d.sections[4].label = "a\nb".into()),
        // 區段：點的區段要在清單裡；代號的寫法、不重複、數量、標題的內容都檢查
        Box::new(|d| {
            d.sections.remove(1);
        }),
        Box::new(|d| d.sections.clear()),
        Box::new(|d| d.sections[0].id = "Question".into()),
        Box::new(|d| d.sections[0].id = "a b".into()),
        Box::new(|d| d.sections[0].id = "x".repeat(MAX_SECTION_ID + 1)),
        Box::new(|d| d.sections[1].id = "question".into()),
        Box::new(|d| d.sections[2].label = "x --> y".into()),
        Box::new(|d| d.sections[2].label = "長".repeat(101)),
        Box::new(|d| {
            for i in 0..MAX_SECTIONS {
                d.sections.push(section(&format!("extra_{i}"), "多的"));
            }
        }),
        Box::new(|d| d.provider = String::new()),
        Box::new(|d| d.lang = String::new()),
        Box::new(|d| d.source_material = "nope".into()),
    ];
    for (i, f) in cases.iter().enumerate() {
        let mut d = base.clone();
        f(&mut d);
        let before = snapshot(fx.dir.path());
        let token = fx.lib.get_paper(&fx.paper).unwrap().1;
        let e = fx.lib.create_digest(&fx.paper, d, &token).unwrap_err();
        assert!(
            is_invalid(&e) || matches!(e, LibError::NotFound { .. }),
            "case {i}: {e:?}"
        );
        assert_eq!(snapshot(fx.dir.path()), before, "case {i}: nothing written");
    }
}

#[test]
fn create_needs_the_page_count_of_the_source() {
    let (dir, mut lib) = temp_library();
    let (paper, source) = add_paper(&mut lib, 1, "No Pages", None);
    let token = lib.get_paper(&paper).unwrap().1;
    let mut fx = Fx {
        dir,
        lib,
        paper,
        source,
    };
    let d = fx.draft();
    let before = snapshot(fx.dir.path());
    let e = fx.lib.create_digest(&fx.paper, d, &token).unwrap_err();
    assert!(is_invalid(&e), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn create_with_a_stale_token_is_a_conflict() {
    let mut fx = setup();
    let old = fx.lib.get_paper(&fx.paper).unwrap().1;
    fx.lib
        .set_material_pages(&fx.paper, &fx.source, 11)
        .unwrap();
    let d = fx.draft();
    let e = fx.lib.create_digest(&fx.paper, d, &old).unwrap_err();
    assert!(matches!(e, LibError::Conflict), "{e:?}");
    assert!(!fx.digest_path().exists());
}

#[test]
fn a_taken_name_gets_a_number() {
    let mut fx = setup();
    fs::write(fx.folder().join("Digest.md"), "使用者自己的檔").unwrap();
    let c = fx.create();
    assert_eq!(fx.material(&c.material_id).path, "digest-2.md");
    assert_eq!(
        fs::read_to_string(fx.folder().join("Digest.md")).unwrap(),
        "使用者自己的檔"
    );
    let c2 = fx.create();
    assert_eq!(fx.material(&c2.material_id).path, "digest-3.md");
}

// ---- 刪一點 ----

#[test]
fn deleting_a_point_removes_only_its_line() {
    let mut fx = setup();
    let c = fx.create();
    let before = fx.text();
    let pj = fx.paper_json();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let gone = doc.points[2].id.clone();
    let t2 = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, &gone, &token)
        .unwrap();
    let after = fx.text();
    assert_eq!(t2.0, sha256_hex(after.as_bytes()));
    let removed: Vec<&str> = before
        .lines()
        .filter(|l| !after.lines().any(|a| a == *l))
        .collect();
    // 刪掉的那一行，加上屬性區的修改時間（同一台裝置，`paperreader_modified_by` 不變）
    assert_eq!(removed.len(), 2, "{removed:?}");
    assert!(removed.iter().any(|l| l.contains(&gone)));
    assert!(
        removed
            .iter()
            .filter(|l| !l.contains(&gone))
            .all(|l| l.starts_with("paperreader_modified"))
    );
    let (doc2, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc2.points.len(), 4);
    assert!(doc2.points.iter().all(|p| p.id != gone));
    assert_eq!(fx.paper_json(), pj, "only the digest file is written");
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn deleting_a_point_goes_through_one_operation_that_records_it() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let gone = doc.points[0].id.clone();
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &gone, &token)
        .unwrap();
    let recs = fx
        .lib
        .read_changes(&ChangeFilter {
            paper: Some(fx.paper.clone()),
            kind: Some(DIGEST_POINT_DELETED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].data_str("material"), Some(c.material_id.as_str()));
    assert_eq!(recs[0].data_str("point"), Some(gone.as_str()));
}

#[test]
fn deleting_an_unknown_point_writes_nothing() {
    let mut fx = setup();
    let c = fx.create();
    let (_, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let before = snapshot(fx.dir.path());
    let e = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, "d-nope", &token)
        .unwrap_err();
    assert!(matches!(e, LibError::NotFound { .. }), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn a_stale_token_is_a_conflict_and_writes_nothing() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    let before = snapshot(fx.dir.path());
    let e = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[1].id, &token)
        .unwrap_err();
    assert!(matches!(e, LibError::Conflict), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn a_bib_edit_in_between_does_not_block_the_digest() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let pt = fx.lib.get_paper(&fx.paper).unwrap().1;
    fx.lib
        .update_bib(
            &fx.paper,
            paperreader_core::format::BibDraft {
                title: Some("Renamed".into()),
                ..Default::default()
            },
            &pt,
        )
        .unwrap();
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    let p = fx.lib.get_paper(&fx.paper).unwrap().0;
    assert_eq!(
        p.bib.title.as_deref(),
        Some("Renamed"),
        "the bib edit is kept"
    );
}

// ---- 標為已校對 ----

#[test]
fn marking_proofread_updates_the_file_and_the_material() {
    let mut fx = setup();
    let c = fx.create();
    let (_, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let t2 = fx
        .lib
        .set_digest_proofread(&fx.paper, &c.material_id, true, &token)
        .unwrap();
    assert!(fx.text().contains("\npaperreader_proofread: true\n"));
    let (doc, t3) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert!(doc.proofread);
    assert_eq!(t2, t3);
    let m = fx.material(&c.material_id);
    assert_eq!(m.provenance.proofread, Some(true));
    assert_eq!(m.sha256, None);
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
    // 已經是這個狀態：不寫
    let before = snapshot(fx.dir.path());
    let t4 = fx
        .lib
        .set_digest_proofread(&fx.paper, &c.material_id, true, &t3)
        .unwrap();
    assert_eq!(t4, t3);
    assert_eq!(snapshot(fx.dir.path()), before);
    // 可以改回未校對
    fx.lib
        .set_digest_proofread(&fx.paper, &c.material_id, false, &t4)
        .unwrap();
    assert!(
        !fx.lib
            .read_digest(&fx.paper, &c.material_id)
            .unwrap()
            .0
            .proofread
    );
}

// ---- 讀不懂不覆寫、新格式、不認得的內容保留 ----

#[test]
fn an_unreadable_file_is_not_overwritten() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let mut bad = fx.text().into_bytes();
    bad.push(0xff);
    fs::write(fx.digest_path(), &bad).unwrap();
    let e = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap_err();
    assert!(matches!(e, LibError::Unreadable { .. }), "{e:?}");
    // 用壞檔自己的指紋當記號：走到「讀不懂」那一支，而不是先被當成衝突
    let bad_token = VersionToken(sha256_hex(&bad));
    let before = snapshot(fx.dir.path());
    let e = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &bad_token)
        .unwrap_err();
    assert!(matches!(e, LibError::Unreadable { .. }), "{e:?}");
    let e = fx
        .lib
        .set_digest_proofread(&fx.paper, &c.material_id, true, &bad_token)
        .unwrap_err();
    assert!(matches!(e, LibError::Unreadable { .. }), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
    // 屬性區沒有 paperreader_digest：不是我們的格式，也不寫
    fs::write(fx.digest_path(), "# just a note\n").unwrap();
    let e = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap_err();
    assert!(matches!(e, LibError::Unreadable { .. }), "{e:?}");
    let e = fx
        .lib
        .delete_digest_point(
            &fx.paper,
            &c.material_id,
            &doc.points[0].id,
            &VersionToken(sha256_hex(b"# just a note\n")),
        )
        .unwrap_err();
    assert!(matches!(e, LibError::Unreadable { .. }), "{e:?}");
}

#[test]
fn the_deletion_record_is_written_before_the_file() {
    // 先記錄、後寫檔。記錄寫完就停下（模擬被強制結束）：檔案沒動，記錄在
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let text = fx.text();
    fx.lib.set_change_hook(Some(Box::new(|_| false)));
    assert!(
        fx.lib
            .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
            .is_err()
    );
    fx.lib.set_change_hook(None);
    assert_eq!(fx.text(), text, "the file is not touched");
    let recs = fx
        .lib
        .read_changes(&ChangeFilter {
            paper: Some(fx.paper.clone()),
            kind: Some(DIGEST_POINT_DELETED.into()),
        })
        .unwrap();
    assert_eq!(recs.len(), 1, "the record is already there");
}

#[test]
fn a_newer_format_is_read_only() {
    let mut fx = setup();
    let c = fx.create();
    let text = fx
        .text()
        .replace("paperreader_digest: 1", "paperreader_digest: 2");
    fs::write(fx.digest_path(), &text).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.format, 2);
    assert_eq!(doc.points.len(), 5, "still readable");
    let before = snapshot(fx.dir.path());
    let e = fx
        .lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap_err();
    assert!(matches!(e, LibError::FormatTooNew { found: 2 }), "{e:?}");
    let e = fx
        .lib
        .set_digest_proofread(&fx.paper, &c.material_id, true, &token)
        .unwrap_err();
    assert!(matches!(e, LibError::FormatTooNew { found: 2 }), "{e:?}");
    assert_eq!(snapshot(fx.dir.path()), before);
}

#[test]
fn unknown_content_is_kept_byte_for_byte() {
    let mut fx = setup();
    let c = fx.create();
    // 使用者（或別的工具）加的屬性、自己的段落、資料裡不認得的鍵、改過的文字
    let text = fx
        .text()
        .replace(
            "paperreader_proofread: false\n",
            "paperreader_proofread: false\ntags: [paper]\n",
        )
        .replace("\"v\":1,", "\"v\":1,\"color\":\"red\",")
        .replace("小物件的表現仍落後。", "小物件的表現仍落後（我改的）。");
    let text = format!("{text}\n## 我自己加的\n\n這段應用不讀、不改。\r\n");
    fs::write(fx.digest_path(), &text).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points[3].text, "小物件的表現仍落後（我改的）。");
    let gone = doc.points[0].id.clone();
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &gone, &token)
        .unwrap();
    let after = fx.text();
    assert!(after.contains("tags: [paper]\n"));
    assert!(after.contains("\"color\":\"red\""));
    assert!(after.contains("小物件的表現仍落後（我改的）。"));
    assert!(after.ends_with("\n## 我自己加的\n\n這段應用不讀、不改。\r\n"));
    assert!(!after.contains(&gone));
}

#[test]
fn a_broken_point_line_is_listed_and_kept() {
    let mut fx = setup();
    let c = fx.create();
    let text = fx.text();
    let broken: String = text
        .lines()
        .map(|l| {
            if l.contains("小物件") {
                l.replace("\"v\":1", "\"v\":")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(fx.digest_path(), &broken).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points.len(), 4);
    assert_eq!(doc.unreadable.len(), 1);
    assert!(doc.unreadable[0].text.contains("小物件"));
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    assert!(
        fx.text().contains("小物件"),
        "the broken line is not touched"
    );
}

#[test]
fn a_read_only_library_refuses_every_write() {
    let mut fx = setup();
    let c = fx.create();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    let root = fx.dir.path().to_path_buf();
    let mut v: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("library.json")).unwrap()).unwrap();
    v["format"] = serde_json::json!(99);
    fs::write(
        root.join("library.json"),
        serde_json::to_string(&v).unwrap(),
    )
    .unwrap();
    let mut ro = LocalLibrary::open(&root, DEVICE).unwrap();
    assert!(ro.is_read_only());
    let before = snapshot(&root);
    assert!(matches!(
        ro.delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token),
        Err(LibError::ReadOnly)
    ));
    assert!(matches!(
        ro.set_digest_proofread(&fx.paper, &c.material_id, true, &token),
        Err(LibError::ReadOnly)
    ));
    let pt = ro.get_paper(&fx.paper).unwrap().1;
    assert!(matches!(
        ro.create_digest(&fx.paper, fx.draft(), &pt),
        Err(LibError::ReadOnly)
    ));
    assert_eq!(snapshot(&root), before);
    assert!(ro.read_digest(&fx.paper, &c.material_id).is_ok());
}

#[test]
fn reading_another_kind_of_material_is_refused() {
    let mut fx = setup();
    let e = fx.lib.read_digest(&fx.paper, &fx.source).unwrap_err();
    assert!(is_invalid(&e), "{e:?}");
    let _ = &mut fx;
}

// ---- check、備份還原、回收區、同步包 ----

#[test]
fn check_backup_restore_and_trash_know_the_digest() {
    let mut fx = setup();
    let c = fx.create();
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("b.zip");
    fx.lib.export_backup(&zip, &mut |_, _| {}).unwrap();
    let root2 = out.path().join("restored");
    restore_backup(&zip, &root2, DEVICE).unwrap();
    let lib2 = LocalLibrary::open(&root2, DEVICE).unwrap();
    let (doc, _) = lib2.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points.len(), 5);
    assert!(lib2.check(CheckLevel::Full).unwrap().is_empty());
    fx.lib.remove_paper(&fx.paper).unwrap();
    fx.lib.restore_paper(&fx.paper).unwrap();
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points.len(), 5);
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn a_sync_package_carries_the_digest_and_its_edits() {
    let mut p = pair();
    // A 的第一篇（Alpha Paper，10 頁）
    let paper =
        p.a.list_papers()
            .into_iter()
            .find(|s| s.bib.title.as_deref() == Some("Alpha Paper"))
            .unwrap()
            .id;
    let (pf, token) = p.a.get_paper(&paper).unwrap();
    let source = pf.materials[0].id.clone();
    let draft = DigestDraft {
        source_material: source,
        service: "Claude".into(),
        provider: "anthropic".into(),
        model: "m".into(),
        lang: "zh-TW".into(),
        title: "Alpha Paper".into(),
        sections: sections(),
        points: vec![
            point("question", "問題一。", &[0]),
            point("results", "結果一。", &[3]),
        ],
    };
    let c = p.a.create_digest(&paper, draft, &token).unwrap();
    // A → B：新材料隨同步包過去；再 B → A，A 才有「B 匯入過 A 的哪一份」的記錄，
    // 之後 A 的匯出才是只帶變更
    sync_once(&mut p.a, &mut p.b, p.out.path());
    let (doc_b, _) = p.b.read_digest(&paper, &c.material_id).unwrap();
    assert_eq!(doc_b.points.len(), 2);
    assert!(p.b.check(CheckLevel::Full).unwrap().is_empty());
    sync_once(&mut p.b, &mut p.a, p.out.path());
    // A 刪一點、標已校對，只帶變更地同步一次：B 跟上
    let (doc_a, t) = p.a.read_digest(&paper, &c.material_id).unwrap();
    let t =
        p.a.delete_digest_point(&paper, &c.material_id, &doc_a.points[0].id, &t)
            .unwrap();
    p.a.set_digest_proofread(&paper, &c.material_id, true, &t)
        .unwrap();
    let pkg = p.out.path().join("changes.zip");
    let rep =
        p.a.export_sync_package(
            &pkg,
            paperreader_core::store::SyncMode::Changes,
            None,
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(rep.mode, paperreader_core::store::SyncMode::Changes);
    assert!(
        rep.included < rep.listed,
        "unchanged material files are left out"
    );
    import_defaults(&mut p.b, &pkg);
    let (doc_b, _) = p.b.read_digest(&paper, &c.material_id).unwrap();
    assert_eq!(doc_b.points.len(), 1);
    assert_eq!(doc_b.points[0].text, "結果一。");
    assert!(doc_b.proofread);
    assert!(p.b.check(CheckLevel::Full).unwrap().is_empty());
    let (doc_a, _) = p.a.read_digest(&paper, &c.material_id).unwrap();
    assert_eq!(doc_a.points, doc_b.points);
}

#[test]
fn the_paper_json_is_untouched_when_the_digest_write_fails_validation() {
    // 交回的結果先檢查才寫：`paper.json` 的位元組在拒絕之後完全一樣
    let mut fx = setup();
    let before = fx.paper_json();
    let mut d = fx.draft();
    d.points[1].pages = vec![99];
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    assert!(fx.lib.create_digest(&fx.paper, d, &token).is_err());
    assert_eq!(fx.paper_json(), before);
}

// ---- 補充的測試 ----

/// 把檔案裡含 `find` 的那一行換掉（模擬在筆記工具裡改字），交回新的內容。
fn edit_outside(path: &std::path::Path, find: &str, to: &str) -> String {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(find), "{find} not in the file");
    let text = text.replace(find, to);
    fs::write(path, &text).unwrap();
    text
}

#[test]
fn h1_edits_made_outside_are_not_a_problem_and_are_kept() {
    let mut fx = setup();
    let c = fx.create();
    edit_outside(
        &fx.digest_path(),
        "小物件的表現仍落後。",
        "小物件的表現仍落後很多（我在筆記工具裡改的）。",
    );
    // 檢查不列大小或內容不符
    assert!(fx.lib.check(CheckLevel::Quick).unwrap().is_empty());
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
    // 讀到的就是改過的；之後在應用裡刪一點，改過的字留著
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(
        doc.points[3].text,
        "小物件的表現仍落後很多（我在筆記工具裡改的）。"
    );
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    assert!(fx.text().contains("我在筆記工具裡改的"));
    let m = fx.material(&c.material_id);
    assert_eq!((m.sha256, m.bytes), (None, None));
    assert!(fx.lib.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn h1_an_unreadable_digest_is_listed_by_check() {
    let mut fx = setup();
    fx.create();
    fs::write(fx.digest_path(), "# no front matter\n").unwrap();
    let p = fx.lib.check(CheckLevel::Quick).unwrap();
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(
        p[0].kind,
        paperreader_core::store::ProblemKind::UnreadableDigestFile
    );
}

/// 兩台都有同一份重點筆記：A 建、A → B、B → A。
fn digest_pair() -> (Pair, String, String) {
    let mut p = pair();
    let paper =
        p.a.list_papers()
            .into_iter()
            .find(|s| s.bib.title.as_deref() == Some("Alpha Paper"))
            .unwrap()
            .id;
    let (pf, token) = p.a.get_paper(&paper).unwrap();
    let draft = DigestDraft {
        source_material: pf.materials[0].id.clone(),
        service: "Claude".into(),
        provider: "anthropic".into(),
        model: "m".into(),
        lang: "zh-TW".into(),
        title: "Alpha Paper".into(),
        sections: sections(),
        points: vec![
            point("question", "問題一。", &[0]),
            point("method", "方法一。", &[1]),
            point("results", "結果一。", &[3]),
        ],
    };
    let c = p.a.create_digest(&paper, draft, &token).unwrap();
    sync_once(&mut p.a, &mut p.b, p.out.path());
    sync_once(&mut p.b, &mut p.a, p.out.path());
    (p, paper, c.material_id)
}

fn texts(lib: &LocalLibrary, paper: &str, mid: &str) -> Vec<String> {
    let (d, _) = lib.read_digest(paper, mid).unwrap();
    d.points.into_iter().map(|p| p.text).collect()
}

fn conflict_copies(lib: &LocalLibrary, paper: &str, mid: &str) -> Vec<String> {
    let dir = lib.material_path(paper, mid).unwrap();
    let dir = dir.parent().unwrap();
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("digest (conflict ") && n.ends_with(".md"))
        .collect();
    v.sort();
    v
}

#[test]
fn m1_sync_merges_points_by_id_and_deletions_win() {
    let (mut p, paper, mid) = digest_pair();
    // A 刪「方法一」，B 之後標已校對
    let (d, t) = p.a.read_digest(&paper, &mid).unwrap();
    let gone = d.points[1].id.clone();
    p.a.delete_digest_point(&paper, &mid, &gone, &t).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let (_, t) = p.b.read_digest(&paper, &mid).unwrap();
    p.b.set_digest_proofread(&paper, &mid, true, &t).unwrap();
    // B → A：刪掉的不回來；已校對過來
    sync_once(&mut p.b, &mut p.a, p.out.path());
    assert_eq!(texts(&p.a, &paper, &mid), ["問題一。", "結果一。"]);
    assert!(p.a.read_digest(&paper, &mid).unwrap().0.proofread);
    // A → B：B 也拿掉那一點
    sync_once(&mut p.a, &mut p.b, p.out.path());
    assert_eq!(texts(&p.b, &paper, &mid), ["問題一。", "結果一。"]);
    assert!(p.b.read_digest(&paper, &mid).unwrap().0.proofread);
    assert!(conflict_copies(&p.a, &paper, &mid).is_empty());
    assert!(conflict_copies(&p.b, &paper, &mid).is_empty());
    assert!(p.a.check(CheckLevel::Full).unwrap().is_empty());
    assert!(p.b.check(CheckLevel::Full).unwrap().is_empty());
}

#[test]
fn h1_a_point_deleted_outside_the_app_comes_back() {
    // 沒有刪除記錄的「只在一邊」：留下
    let (mut p, paper, mid) = digest_pair();
    let path = p.b.material_path(&paper, &mid).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let kept: String = text
        .split_inclusive('\n')
        .filter(|l| !l.contains("方法一。"))
        .collect();
    fs::write(&path, kept).unwrap();
    sync_once(&mut p.a, &mut p.b, p.out.path());
    assert_eq!(
        texts(&p.b, &paper, &mid),
        ["問題一。", "方法一。", "結果一。"]
    );
}

#[test]
fn h1_the_same_point_edited_on_both_sides_keeps_local_and_copies_incoming() {
    let (mut p, paper, mid) = digest_pair();
    let pa = p.a.material_path(&paper, &mid).unwrap();
    let pb = p.b.material_path(&paper, &mid).unwrap();
    edit_outside(&pa, "結果一。", "結果一（A 改的）。");
    edit_outside(&pb, "結果一。", "結果一（B 改的）。");
    let report = sync_once(&mut p.b, &mut p.a, p.out.path());
    assert_eq!(
        texts(&p.a, &paper, &mid),
        ["問題一。", "方法一。", "結果一（A 改的）。"],
        "the local text is kept"
    );
    assert!(
        report
            .notices
            .iter()
            .any(|n| n.kind == paperreader_core::store::NoticeKind::DigestPointConflict)
    );
    let copies = conflict_copies(&p.a, &paper, &mid);
    assert_eq!(copies.len(), 1, "{copies:?}");
    let dir = pa.parent().unwrap();
    let copy = fs::read_to_string(dir.join(&copies[0])).unwrap();
    assert!(
        copy.contains("結果一（B 改的）。"),
        "the incoming copy is kept"
    );
    assert!(copy.contains("paperreader_conflict_of: "));
    assert!(!copy.contains("paperreader_digest_of: "));
    assert!(
        p.a.check(CheckLevel::Full).unwrap().is_empty(),
        "a conflict copy is not an unlisted file"
    );
    // 再同步一次同一份差異，不再多寫一份副本
    sync_once(&mut p.b, &mut p.a, p.out.path());
    assert_eq!(conflict_copies(&p.a, &paper, &mid).len(), 1);
}

#[test]
fn h1_text_outside_the_points_that_differs_copies_incoming() {
    let (mut p, paper, mid) = digest_pair();
    let pb = p.b.material_path(&paper, &mid).unwrap();
    let text = fs::read_to_string(&pb).unwrap();
    fs::write(&pb, format!("{text}\n## 我自己加的\n\nB 這邊寫的段落。\n")).unwrap();
    sync_once(&mut p.b, &mut p.a, p.out.path());
    let pa = p.a.material_path(&paper, &mid).unwrap();
    assert!(!fs::read_to_string(&pa).unwrap().contains("B 這邊寫的段落"));
    let copies = conflict_copies(&p.a, &paper, &mid);
    assert_eq!(copies.len(), 1, "{copies:?}");
}

#[test]
fn m2_a_listed_digest_whose_file_is_gone_keeps_its_name() {
    let mut fx = setup();
    let c1 = fx.create();
    fs::remove_file(fx.digest_path()).unwrap();
    let c2 = fx.create();
    let p1 = fx.material(&c1.material_id).path;
    let p2 = fx.material(&c2.material_id).path;
    assert_eq!(p1, "digest.md");
    assert_ne!(
        p2.to_lowercase(),
        "digest.md",
        "two materials never share a file"
    );
    assert!(fx.folder().join(&p2).is_file());
    assert!(!fx.digest_path().exists());
}

#[test]
fn l2_a_title_with_comment_markers_is_not_read_as_a_point() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let mut d = fx.draft();
    d.title = "Title <!-- paperreader:point {\"v\":1,\"id\":\"d-x1\",\"section\":\"question\",\"pages\":[0],\"segments\":[],\"created\":\"t\"} -->".into();
    let c = fx.lib.create_digest(&fx.paper, d, &token).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points.len(), 5);
    assert!(doc.unreadable.is_empty());
    assert!(
        !fx.text().contains("<!--paperreader")
            && fx.text().matches("<!-- paperreader:point").count() == 5
    );
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &doc.points[0].id, &token)
        .unwrap();
    assert!(fx.text().contains("\n# Title"), "the title line stays");
}

#[test]
fn l4_images_html_and_links_in_text_are_written_as_plain_text() {
    let mut fx = setup();
    let token = fx.lib.get_paper(&fx.paper).unwrap().1;
    let mut d = fx.draft();
    let odd = r"看 ![x](https://e.com/a.png) 與 <img src=x> 和 [連結](https://e.com) \ 結尾 [p. 1](paperreader://open?page=1)";
    d.points = vec![point("results", odd, &[1])];
    let c = fx.lib.create_digest(&fx.paper, d, &token).unwrap();
    let text = fx.text();
    let line = text.lines().find(|l| l.contains("看 ")).unwrap();
    // 圖片、HTML、連結都跳脫成純文字：`!\[x\](…)`、`\<img`、`\[連結\](…)`
    assert!(
        !line.contains("![x](") && !line.contains(" <img") && !line.contains("[連結]("),
        "{line}"
    );
    assert!(line.contains(r"!\[x\](") && line.contains(r"\<img") && line.contains(r"\[連結\]("));
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.points[0].text, odd, "read back unchanged");
    assert_eq!(doc.points[0].pages, vec![1]);
}

#[test]
fn l5_deleting_a_point_removes_a_duplicated_line_too() {
    let mut fx = setup();
    let c = fx.create();
    let text = fx.text();
    let dup = text
        .lines()
        .find(|l| l.contains("53.1 AP"))
        .unwrap()
        .to_string();
    fs::write(fx.digest_path(), format!("{text}{dup}\n")).unwrap();
    let (doc, token) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert_eq!(doc.unreadable.len(), 1, "the copy is listed as a duplicate");
    let id = doc.points[2].id.clone();
    fx.lib
        .delete_digest_point(&fx.paper, &c.material_id, &id, &token)
        .unwrap();
    assert!(!fx.text().contains(&id), "both lines are gone");
    let (doc, _) = fx.lib.read_digest(&fx.paper, &c.material_id).unwrap();
    assert!(doc.points.iter().all(|p| p.id != id));
    assert!(doc.unreadable.is_empty());
}
