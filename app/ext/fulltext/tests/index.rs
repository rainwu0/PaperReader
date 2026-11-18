//! 索引（真的 SQLite）。每種來源的增、改、刪只重做變了的鍵；移除一篇；版本不同、
//! 檔壞了：打不開；查詢的分組、排序、範圍、一批與下一批；FTS5 的命中與找位置一致。

use paperreader_core::digest::{DigestDoc, DigestPoint};
use paperreader_core::notes::{FileRef, Highlight, PageRef, Quote};
use paperreader_core::references::FOLD_VERSION;
use paperreader_core::store::{FileStamp, IndexSource, NotesDoc};
use paperreader_ext_fulltext::{
    FulltextIndex, INDEX_FILE, Stamps, UnitKind, is_corrupt, locate, parse_query, units_of,
    user_version,
};
use serde_json::json;

fn text_source(material: &str, pages: &[&str]) -> IndexSource {
    let v = json!({
        "format": 1,
        "extraction": {
            "extractor": "pdfjs-text", "version": 1,
            "source_material": material, "source_sha256": "00",
            "created": "2026-11-18T00:00:00.000Z", "created_by": "D"
        },
        "pages": pages.iter().enumerate().map(|(i, t)| json!({"page": i, "text": t})).collect::<Vec<_>>()
    });
    IndexSource::Text(serde_json::from_value(v).unwrap())
}

fn overlay_source(segs: &[(&str, u32, &str, Option<&str>)]) -> IndexSource {
    let v = json!({
        "format": 1, "source_material": "SRC", "provider": "p", "model": "m", "target_lang": "zh-TW",
        "segments": segs.iter().map(|(id, page, status, target)| json!({
            "id": id, "page": page, "bbox": [10.0, 20.0, 300.0, 40.0], "source": "src",
            "target": target, "status": status
        })).collect::<Vec<_>>()
    });
    IndexSource::Overlay(serde_json::from_value(v).unwrap())
}

fn digest_source(points: &[(&str, &str, &[u32])]) -> IndexSource {
    IndexSource::Digest(DigestDoc {
        format: 1,
        paper: None,
        source_material: None,
        source_sha256: None,
        service: None,
        provider: None,
        model: None,
        lang: None,
        created: None,
        created_by: None,
        modified: None,
        modified_by: None,
        proofread: false,
        sections: vec![],
        sections_unreadable: false,
        title: None,
        points: points
            .iter()
            .map(|(id, text, pages)| DigestPoint {
                id: (*id).into(),
                section: "s".into(),
                text: (*text).into(),
                pages: pages.to_vec(),
                segments: vec![],
                created: String::new(),
            })
            .collect(),
        unreadable: vec![],
    })
}

fn notes_source(highlights: &[(&str, u32, &str, &str)], thoughts: &str) -> IndexSource {
    IndexSource::Notes(NotesDoc {
        path: Some("notes/x.md".into()),
        modified: None,
        modified_by: None,
        thoughts: thoughts.into(),
        thoughts_complete: true,
        highlights: highlights
            .iter()
            .map(|(id, page, exact, comment)| Highlight {
                id: (*id).into(),
                kind: "highlight".into(),
                material: "SRC".into(),
                file: FileRef {
                    sha256: None,
                    fingerprint: None,
                    extra: Default::default(),
                },
                page: PageRef {
                    index: *page,
                    label: None,
                    extra: Default::default(),
                },
                rects: vec![[1.0, 2.0, 3.0, 4.0]],
                next_page_rects: vec![],
                position: None,
                quote: Quote {
                    exact: (*exact).into(),
                    prefix: None,
                    suffix: None,
                    extra: Default::default(),
                },
                color: None,
                created: String::new(),
                modified: String::new(),
                extra: Default::default(),
                comment: (*comment).into(),
                complete: true,
            })
            .collect(),
        unreadable: vec![],
    })
}

fn stamp(bytes: u64) -> FileStamp {
    FileStamp {
        bytes,
        modified_ms: Some(1000),
    }
}

fn stamps(keys: &[(&str, u64)]) -> Stamps {
    keys.iter()
        .map(|(k, b)| ((*k).to_string(), stamp(*b)))
        .collect()
}

fn fresh() -> (tempfile::TempDir, FulltextIndex) {
    let dir = tempfile::tempdir().unwrap();
    let ix = FulltextIndex::create(dir.path()).unwrap();
    (dir, ix)
}

fn papers(ix: &FulltextIndex, q: &str) -> Vec<(String, usize)> {
    let q = parse_query(q).unwrap();
    ix.search(&q, None, 0, 100)
        .unwrap()
        .papers
        .into_iter()
        .map(|p| (p.paper, p.total))
        .collect()
}

// ---------- 各種來源的索引與搜尋 ----------

#[test]
fn each_source_kind_is_indexed_and_searchable() {
    let (_d, mut ix) = fresh();
    let now = stamps(&[
        ("text", 1),
        ("overlay:OV", 2),
        ("digest:DG", 3),
        ("notes", 4),
    ]);
    ix.put(
        "P",
        &now,
        vec![
            (
                "text".into(),
                Some(text_source(
                    "SRC",
                    &["object detection on page zero", "nothing"],
                )),
            ),
            (
                "overlay:OV".into(),
                Some(overlay_source(&[
                    ("g1", 0, "done", Some("即時物件偵測")),
                    ("g2", 1, "pending", None),
                    ("g3", 1, "failed", Some("不應該被找到的譯文")),
                ])),
            ),
            (
                "digest:DG".into(),
                Some(digest_source(&[("d1", "要點：注意力機制", &[3, 4])])),
            ),
            (
                "notes".into(),
                Some(notes_source(
                    &[("h1", 2, "a quoted sentence", "我的短評 bananas")],
                    "first para\n\nsecond kiwi para",
                )),
            ),
        ],
    )
    .unwrap();
    assert_eq!(ix.len(), 1);
    type Place = (UnitKind, Option<String>, Option<u32>, Option<String>);
    let kinds = |q: &str| -> Vec<Place> {
        let q = parse_query(q).unwrap();
        ix.hits(&q, "P", 200)
            .unwrap()
            .into_iter()
            .map(|h| (h.kind, h.material, h.page, h.target))
            .collect()
    };
    assert_eq!(
        kinds("detection"),
        vec![(UnitKind::Source, Some("SRC".into()), Some(0), None)]
    );
    assert_eq!(
        kinds("物件偵測"),
        vec![(
            UnitKind::Translation,
            Some("OV".into()),
            Some(0),
            Some("g1".into())
        )]
    );
    assert!(kinds("不應該").is_empty(), "沒翻完的段落不進索引");
    assert_eq!(
        kinds("注意力"),
        vec![(
            UnitKind::Digest,
            Some("DG".into()),
            Some(3),
            Some("d1".into())
        )]
    );
    assert_eq!(
        kinds("bananas"),
        vec![(
            UnitKind::Note,
            Some("SRC".into()),
            Some(2),
            Some("h1".into())
        )]
    );
    assert_eq!(
        kinds("kiwi"),
        vec![(UnitKind::Note, None, None, Some("t1".into()))]
    );
    // 譯文那一段帶方框
    let q = parse_query("物件").unwrap();
    assert_eq!(
        ix.hits(&q, "P", 10).unwrap()[0].bbox,
        Some([10.0, 20.0, 300.0, 40.0])
    );
}

#[test]
fn only_changed_keys_are_stale_and_redone() {
    let (_d, mut ix) = fresh();
    let now = stamps(&[("text", 1), ("notes", 4)]);
    assert_eq!(
        ix.stale("P", &now),
        vec!["notes".to_string(), "text".to_string()]
    );
    ix.put(
        "P",
        &now,
        vec![
            ("text".into(), Some(text_source("SRC", &["alpha"]))),
            ("notes".into(), Some(notes_source(&[], "gamma"))),
        ],
    )
    .unwrap();
    assert!(ix.stale("P", &now).is_empty());
    // notes 改了、加了一份疊加檔、text 沒變
    let now2 = stamps(&[("text", 1), ("notes", 5), ("overlay:OV", 9)]);
    assert_eq!(
        ix.stale("P", &now2),
        vec!["notes".to_string(), "overlay:OV".to_string()]
    );
    ix.put(
        "P",
        &now2,
        vec![
            ("notes".into(), Some(notes_source(&[], "delta"))),
            (
                "overlay:OV".into(),
                Some(overlay_source(&[("g1", 0, "done", Some("epsilon"))])),
            ),
        ],
    )
    .unwrap();
    assert_eq!(papers(&ix, "alpha"), vec![("P".into(), 1)]);
    assert!(papers(&ix, "gamma").is_empty());
    assert_eq!(papers(&ix, "delta"), vec![("P".into(), 1)]);
    assert_eq!(papers(&ix, "epsilon"), vec![("P".into(), 1)]);
    // 疊加檔不見了：stale 說出它，put 只刪
    let now3 = stamps(&[("text", 1), ("notes", 5)]);
    assert_eq!(ix.stale("P", &now3), vec!["overlay:OV".to_string()]);
    ix.put("P", &now3, vec![("overlay:OV".into(), None)])
        .unwrap();
    assert!(papers(&ix, "epsilon").is_empty());
    assert!(ix.stale("P", &now3).is_empty());
    // 讀不懂（None）但檔在：記下檔案屬性，不再重讀
    let now4 = stamps(&[("text", 2), ("notes", 5)]);
    ix.put("P", &now4, vec![("text".into(), None)]).unwrap();
    assert!(ix.stale("P", &now4).is_empty());
    assert!(papers(&ix, "alpha").is_empty());
}

#[test]
fn removing_a_paper_drops_everything() {
    let (_d, mut ix) = fresh();
    for p in ["A", "B"] {
        ix.put(
            "P".replace('P', p).as_str(),
            &stamps(&[("text", 1)]),
            vec![("text".into(), Some(text_source("S", &["shared words"])))],
        )
        .unwrap();
    }
    assert_eq!(ix.len(), 2);
    ix.remove("A").unwrap();
    assert_eq!(ix.len(), 1);
    assert_eq!(papers(&ix, "shared"), vec![("B".into(), 1)]);
    assert_eq!(
        ix.stale("A", &stamps(&[("text", 1)])),
        vec!["text".to_string()]
    );
}

#[test]
fn reopening_keeps_the_index_and_a_different_version_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut ix = FulltextIndex::create(dir.path()).unwrap();
        ix.put(
            "P",
            &stamps(&[("text", 1)]),
            vec![("text".into(), Some(text_source("S", &["kept"])))],
        )
        .unwrap();
    }
    let ix = FulltextIndex::open(dir.path()).expect("同版本打得開");
    assert_eq!(papers(&ix, "kept"), vec![("P".into(), 1)]);
    drop(ix);
    // user_version 不同（含只有 FOLD_VERSION 變了）：打不開
    assert_eq!(user_version(), 1000 + i64::from(FOLD_VERSION));
    for v in [user_version() + 1, 2000 + i64::from(FOLD_VERSION), 0] {
        let c = rusqlite::Connection::open(dir.path().join(INDEX_FILE)).unwrap();
        c.pragma_update(None, "user_version", v).unwrap();
        drop(c);
        assert!(
            FulltextIndex::open(dir.path()).is_none(),
            "user_version {v}"
        );
    }
    // 沒有檔
    let empty = tempfile::tempdir().unwrap();
    assert!(FulltextIndex::open(empty.path()).is_none());
}

#[test]
fn a_damaged_file_does_not_open_and_reports_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(INDEX_FILE), vec![0x5Au8; 8192]).unwrap();
    assert!(FulltextIndex::open(dir.path()).is_none());
    let c = rusqlite::Connection::open(dir.path().join(INDEX_FILE)).unwrap();
    let e = c
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_err();
    assert!(is_corrupt(&e), "{e:?}");
    // 別種錯誤不算損壞
    let e = c.execute("SELEC nonsense", []).unwrap_err();
    assert!(!is_corrupt(&e));
}

// ---------- 依篇分組、排序、範圍與分頁 ----------

#[test]
fn papers_are_grouped_ranked_scoped_and_paged() {
    let (_d, mut ix) = fresh();
    // 12 篇；第 n 篇有 n 頁含 needle，其餘的字讓分數有差
    for n in 1..=12u32 {
        let pages: Vec<String> = (0..12)
            .map(|i| {
                if i < n {
                    format!("needle {}", "filler ".repeat(i as usize))
                } else {
                    "other".into()
                }
            })
            .collect();
        let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
        ix.put(
            &format!("P{n:02}"),
            &stamps(&[("text", 1)]),
            vec![("text".into(), Some(text_source("S", &pages)))],
        )
        .unwrap();
    }
    let q = parse_query("needle").unwrap();
    let first = ix.search(&q, None, 0, 10).unwrap();
    assert_eq!(first.papers.len(), 10);
    assert!(first.more);
    let second = ix.search(&q, None, 10, 10).unwrap();
    assert_eq!(second.papers.len(), 2);
    assert!(!second.more);
    let mut all: Vec<String> = first
        .papers
        .iter()
        .chain(&second.papers)
        .map(|p| p.paper.clone())
        .collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 12, "兩批不重複");
    for p in first.papers.iter().chain(&second.papers) {
        let n: usize = p.paper[1..].parse().unwrap();
        assert_eq!(p.total, n, "一處等於一個單位（一頁）");
        assert_eq!(p.candidates.len(), n.min(10));
    }
    // 範圍
    let scope = vec!["P03".to_string(), "P07".to_string(), "Pxx".to_string()];
    let r = ix.search(&q, Some(&scope), 0, 10).unwrap();
    let mut got: Vec<String> = r.papers.into_iter().map(|p| p.paper).collect();
    got.sort();
    assert_eq!(got, vec!["P03", "P07"]);
    // hits：最多 limit 處
    assert_eq!(ix.hits(&q, "P12", 200).unwrap().len(), 12);
    assert_eq!(ix.hits(&q, "P12", 5).unwrap().len(), 5);
}

#[test]
fn several_terms_must_be_in_the_same_unit_but_need_not_be_adjacent() {
    let (_d, mut ix) = fresh();
    ix.put(
        "P",
        &stamps(&[("text", 1)]),
        vec![(
            "text".into(),
            Some(text_source(
                "S",
                &[
                    "detection with transformers",
                    "only detection here",
                    "transformers alone",
                ],
            )),
        )],
    )
    .unwrap();
    assert_eq!(papers(&ix, "transformers detection"), vec![("P".into(), 1)]);
    assert!(papers(&ix, "\"transformers detection\"").is_empty());
}

/// 檔沒變時每一筆 FTS5 的命中都找得到位置；反過來找得到位置的單位也都在命中裡（兩邊的比法相同）。
#[test]
fn every_fts_hit_is_located_and_vice_versa() {
    let texts = [
        "DETRs Beat YOLOs on Real-time Object Detection. We propose RT-DETR, the first real-\ntime end-to-\nend object detec-\ntion model.",
        "自監督學習（self-\nsupervised learning）在 COCO 上達到 53.1 AP；非極大值抑制（NMS）被移除。",
        "スーパー・モデルの表現。Transformer模型的表現很好。state-of-the-\nart 方法。",
        "Let \u{1D465} denote the input; the \u{FB01}ne-tuning stage uses ＡＰ５０ and naïve baselines.",
        "Σ ΟΔΟΣ ΣΑ. Real-time, real time, realtime and real-\ntime detection for real-\ntime use.",
        "我們在COCO上達到最好；我們在 COCO 上也達到。示意圖見圖 3。",
    ];
    let queries = [
        "detect",
        "detection",
        "\"object detection\"",
        "object detection",
        "real-time",
        "\"real-time detection\"",
        "\"for real-time\"",
        "end-to-end",
        "self-supervised",
        "selfsupervised",
        "極大值",
        "抑制 NMS",
        "在COCO上",
        "Transformer模型",
        "state-of-the-art",
        "圖 3",
        "\u{1D465} denote",
        "fine-tuning",
        "ap50",
        "naive",
        "ΟΔΟΣ",
        "οδος σα",
        "スーパー",
        "ー",
        "模型 表現",
        "real time",
        "realtime",
        "\"real time\"",
        "示意圖",
        "COCO 達到",
        "rt-detr",
        "53.1",
        "1 ap",
        "detec",
        "tion",
        "the first",
        "AND",
        "-",
        "\"\"",
        "NEAR(a b)",
    ];
    let (_d, mut ix) = fresh();
    let pages: Vec<&str> = texts.to_vec();
    ix.put(
        "P",
        &stamps(&[("text", 1)]),
        vec![("text".into(), Some(text_source("S", &pages)))],
    )
    .unwrap();
    let src = text_source("S", &pages);
    let units = units_of("text", &src);
    let mut checked = 0;
    for qs in queries {
        let Some(q) = parse_query(qs) else { continue };
        let hit_pages: Vec<u32> = ix
            .hits(&q, "P", 200)
            .unwrap()
            .into_iter()
            .filter_map(|h| h.page)
            .collect();
        for u in &units {
            let located = locate(&q, &u.text);
            let in_fts = hit_pages.contains(&u.page.unwrap());
            assert_eq!(
                located.is_some(),
                in_fts,
                "查詢 {qs:?} 在第 {:?} 頁：找位置 {:?}、FTS5 {in_fts}",
                u.page,
                located
            );
            if let Some(l) = located {
                // range 對回原字串：UTF-16 位置的那一段就是 quote
                let u16: Vec<u16> = u.text.encode_utf16().collect();
                assert_eq!(
                    String::from_utf16(&u16[l.range.0..l.range.1]).unwrap(),
                    l.quote
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 30, "{checked}");
}
