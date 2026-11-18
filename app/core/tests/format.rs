use paperreader_core::error::LibError;
use paperreader_core::format::*;
use paperreader_core::jsonio::*;
use serde_json::{Value, json};
use std::fs;

const SHA: &str = "01018b33f4a1c2d3e4f5061728394a5b6c7d8e9f00112233445566778899aabb";

/// 檔案格式的範例，依全域限制（兩個空格縮排、欄位順序固定、結尾換行）排版。
fn spec_paper_json() -> String {
    format!(
        r#"{{
  "format": 1,
  "id": "01JCK3W2E4H6J8K0M2N4P6Q8RS",
  "short_name": null,
  "aliases": [],
  "added": "2026-11-13T08:30:11.045Z",
  "bib": {{
    "title": "DETRs Beat YOLOs on Real-time Object Detection",
    "authors": [
      {{
        "family": "Zhao",
        "given": "Yian",
        "literal": null
      }},
      {{
        "family": null,
        "given": null,
        "literal": "RT-DETR Team"
      }}
    ],
    "year": 2024,
    "venue": "CVPR 2024",
    "doi": "10.48550/arxiv.2304.08069",
    "url": "https://arxiv.org/abs/2304.08069",
    "modified": "2026-11-13T08:30:11.045Z",
    "modified_by": "01JCK3T1A2B3C4D5E6F7G8H9JK"
  }},
  "materials": [
    {{
      "id": "01JCK3W2E7T9V1X3Z5B7D9F1GH",
      "role": "source",
      "form": "pdf",
      "path": "source.pdf",
      "sha256": "{SHA}",
      "bytes": 5326548,
      "pages": 14,
      "pages_modified": null,
      "pages_modified_by": null,
      "title": null,
      "provenance": {{
        "method": "added-by-user",
        "time": "2026-11-13T08:30:11.045Z",
        "origin_name": "2304.08069v3.pdf",
        "proofread": null,
        "page_map": null,
        "source_sha256": null
      }}
    }}
  ],
  "groups": [],
  "groups_modified": null,
  "groups_modified_by": null
}}
"#
    )
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn paper_file_roundtrip_is_byte_identical() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let text = spec_paper_json();
    fs::write(&p, &text).unwrap();
    let (paper, _): (PaperFile, _) = read_json(&p).unwrap();
    assert_eq!(to_canonical_json(&paper).unwrap(), text);
}

#[test]
fn old_material_without_pages_time_reads_null() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let text = spec_paper_json()
        .replace("      \"pages_modified\": null,\n", "")
        .replace("      \"pages_modified_by\": null,\n", "");
    assert!(!text.contains("pages_modified"));
    fs::write(&p, text).unwrap();
    let (paper, _): (PaperFile, _) = read_json(&p).unwrap();
    assert_eq!(paper.materials[0].pages_modified, None);
    assert_eq!(paper.materials[0].pages_modified_by, None);
    assert_eq!(paper.materials[0].pages, Some(14));
}

#[test]
fn old_paper_json_reads_empty_groups() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let missing = spec_paper_json()
        .replace(",\n  \"groups\": []", "")
        .replace(",\n  \"groups_modified\": null", "")
        .replace(",\n  \"groups_modified_by\": null", "");
    assert!(!missing.contains("groups"));
    let nulls = missing.replace(
        "\n  ]\n}\n",
        "\n  ],\n  \"groups\": null,\n  \"groups_modified\": null,\n  \"groups_modified_by\": null\n}\n",
    );
    assert!(nulls.contains("\"groups\": null"));
    for text in [missing, nulls] {
        fs::write(&p, text).unwrap();
        let (paper, _): (PaperFile, _) = read_json(&p).unwrap();
        assert!(paper.groups.is_empty());
        assert_eq!(paper.groups_modified, None);
        assert_eq!(paper.groups_modified_by, None);
    }
}

#[test]
fn groups_wrong_type_makes_paper_unreadable() {
    use paperreader_core::local::LocalLibrary;
    use paperreader_core::store::{CheckLevel, ProblemKind};
    for (from, to) in [
        ("\"groups\": []", "\"groups\": \"x\""),
        ("\"groups\": []", "\"groups\": [1]"),
        ("\"groups_modified\": null", "\"groups_modified\": 5"),
    ] {
        let dir = tmp();
        let papers = dir.path().join("papers/Bad");
        fs::create_dir_all(&papers).unwrap();
        fs::write(
            papers.join("paper.json"),
            spec_paper_json().replace(from, to),
        )
        .unwrap();
        let lib = LocalLibrary::open(dir.path(), "01JCK3T1A2B3C4D5E6F7G8H9JK").unwrap();
        assert!(lib.list_papers().is_empty(), "{to}");
        let ps = lib.check(CheckLevel::Quick).unwrap();
        assert!(
            ps.iter()
                .any(|p| p.kind == ProblemKind::UnreadablePaperFile),
            "{to}: {ps:?}"
        );
    }
}

#[test]
fn old_paper_json_gains_groups_on_write() {
    use paperreader_core::local::LocalLibrary;
    let dir = tmp();
    let id = "01JCK3W2E4H6J8K0M2N4P6Q8RS";
    let folder = dir.path().join("papers/Old");
    fs::create_dir_all(&folder).unwrap();
    // 沒有 groups 三個欄位、有一個不認得的欄位
    let mut v: Value = serde_json::from_str(&spec_paper_json()).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("groups");
    o.remove("groups_modified");
    o.remove("groups_modified_by");
    o.insert("x_future".into(), json!(1));
    fs::write(
        folder.join("paper.json"),
        serde_json::to_string_pretty(&v).unwrap(),
    )
    .unwrap();

    let mut lib = LocalLibrary::open(dir.path(), "01JCK3T1A2B3C4D5E6F7G8H9JK").unwrap();
    let (_paper, token) = lib.get_paper(id).unwrap();
    let draft = BibDraft {
        title: Some("New title".into()),
        ..Default::default()
    };
    lib.update_bib(id, draft, &token).unwrap();
    let after: Value =
        serde_json::from_slice(&fs::read(folder.join("paper.json")).unwrap()).unwrap();
    let keys: Vec<&str> = after
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let pos = |k: &str| keys.iter().position(|x| *x == k).unwrap();
    assert_eq!(pos("groups"), pos("materials") + 1);
    assert_eq!(pos("groups_modified"), pos("materials") + 2);
    assert_eq!(pos("groups_modified_by"), pos("materials") + 3);
    assert!(pos("x_future") > pos("groups_modified_by"));
    assert_eq!(after["groups"], json!([]));
}

#[test]
fn unknown_fields_survive_at_every_level() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let mut v: Value = serde_json::from_str(&spec_paper_json()).unwrap();
    v["x_future"] = json!(1);
    v["bib"]["x_future"] = json!(1);
    v["bib"]["authors"][0]["x_future"] = json!(1);
    v["materials"][0]["x_future"] = json!(1);
    v["materials"][0]["provenance"]["x_future"] = json!(1);
    fs::write(&p, serde_json::to_string_pretty(&v).unwrap()).unwrap();

    let (paper, _): (PaperFile, _) = read_json(&p).unwrap();
    let out: Value = serde_json::from_str(&to_canonical_json(&paper).unwrap()).unwrap();
    assert_eq!(out["x_future"], json!(1));
    assert_eq!(out["bib"]["x_future"], json!(1));
    assert_eq!(out["bib"]["authors"][0]["x_future"], json!(1));
    assert_eq!(out["materials"][0]["x_future"], json!(1));
    assert_eq!(out["materials"][0]["provenance"]["x_future"], json!(1));
}

#[test]
fn null_fields_are_written_not_omitted() {
    let prov = Provenance::default();
    let s = to_canonical_json(&prov).unwrap();
    assert_eq!(s.matches("null").count(), 6, "{s}");
}

#[test]
fn bom_is_tolerated_on_read() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(spec_paper_json().as_bytes());
    fs::write(&p, &bytes).unwrap();
    let (paper, _): (PaperFile, _) = read_json(&p).unwrap();
    assert_eq!(paper.id, "01JCK3W2E4H6J8K0M2N4P6Q8RS");

    let out = dir.path().join("out.json");
    write_json_atomic(&out, &paper).unwrap();
    let written = fs::read(&out).unwrap();
    assert!(!written.starts_with(&[0xEF, 0xBB, 0xBF]));
}

#[test]
fn wrong_type_is_unreadable() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let text = spec_paper_json().replace("\"year\": 2024", "\"year\": \"2024\"");
    fs::write(&p, text).unwrap();
    let r: Result<(PaperFile, _), _> = read_json(&p);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
}

#[test]
fn missing_id_is_unreadable() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    let text = spec_paper_json().replace("  \"id\": \"01JCK3W2E4H6J8K0M2N4P6Q8RS\",\n", "");
    fs::write(&p, text).unwrap();
    let r: Result<(PaperFile, _), _> = read_json(&p);
    assert!(matches!(r, Err(LibError::Unreadable { .. })), "{r:?}");
}

#[test]
fn atomic_write_leaves_no_temp() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    write_json_atomic(&p, &json!({"a": 1})).unwrap();
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".pr-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn atomic_write_replaces_existing() {
    let dir = tmp();
    let p = dir.path().join("paper.json");
    write_json_atomic(&p, &json!({"a": 1})).unwrap();
    write_json_atomic(&p, &json!({"a": 2})).unwrap();
    let (v, _): (Value, _) = read_json(&p).unwrap();
    assert_eq!(v, json!({"a": 2}));
}

#[test]
fn version_token_tracks_content() {
    let dir = tmp();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, "{\"t\": \"abc\"}\n").unwrap();
    fs::write(&b, "{\"t\": \"abc\"}\n").unwrap();
    let (_, ta): (Value, _) = read_json(&a).unwrap();
    let (_, tb): (Value, _) = read_json(&b).unwrap();
    assert_eq!(ta, tb);
    fs::write(&b, "{\"t\": \"abd\"}\n").unwrap();
    let (_, tb2): (Value, _) = read_json(&b).unwrap();
    assert_ne!(ta, tb2);
}

#[test]
fn write_returns_the_token_a_later_read_gets() {
    let dir = tmp();
    let p = dir.path().join("x.json");
    let t = write_json_atomic(&p, &json!({"a": 1})).unwrap();
    let (_, t2): (Value, _) = read_json(&p).unwrap();
    assert_eq!(t, t2);
}

#[test]
fn event_type_field_name() {
    let e = Event {
        id: "i".into(),
        t: "2026-11-13T09:02:44.310Z".into(),
        device: "d".into(),
        paper: "p".into(),
        kind: "open".into(),
        data: json!({}),
        extra: Map::new(),
    };
    let v: Value = serde_json::from_str(&to_canonical_json(&e).unwrap()).unwrap();
    assert_eq!(v["type"], json!("open"));
    assert!(v.get("kind").is_none());
}

#[test]
fn normalize_doi_cases() {
    let want = "10.48550/arxiv.2304.08069";
    assert_eq!(
        normalize_doi(" https://doi.org/10.48550/arXiv.2304.08069 "),
        want
    );
    assert_eq!(normalize_doi("doi:10.48550/ARXIV.2304.08069"), want);
    assert_eq!(normalize_doi("10.48550/arxiv.2304.08069"), want);
}

macro_rules! doi_prefix_test {
    ($name:ident, $prefix:expr) => {
        #[test]
        fn $name() {
            let want = "10.48550/arxiv.2304.08069";
            let body = "10.48550/arXiv.2304.08069";
            assert_eq!(normalize_doi(&format!("{}{}", $prefix, body)), want);
            // 前綴不分大小寫
            assert_eq!(
                normalize_doi(&format!("{}{}", $prefix.to_uppercase(), body)),
                want
            );
        }
    };
}

doi_prefix_test!(doi_prefix_https_doi_org, "https://doi.org/");
doi_prefix_test!(doi_prefix_http_doi_org, "http://doi.org/");
doi_prefix_test!(doi_prefix_https_www_doi_org, "https://www.doi.org/");
doi_prefix_test!(doi_prefix_http_www_doi_org, "http://www.doi.org/");
doi_prefix_test!(doi_prefix_https_dx_doi_org, "https://dx.doi.org/");
doi_prefix_test!(doi_prefix_http_dx_doi_org, "http://dx.doi.org/");
doi_prefix_test!(doi_prefix_bare_doi_org, "doi.org/");
doi_prefix_test!(doi_prefix_bare_dx_doi_org, "dx.doi.org/");
doi_prefix_test!(doi_prefix_doi_colon, "doi:");

#[test]
fn doi_prefix_with_space_after_colon() {
    assert_eq!(normalize_doi("DOI: 10.1/ABC"), "10.1/abc");
}
