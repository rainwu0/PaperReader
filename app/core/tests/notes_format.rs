//! 工作 5：筆記檔的解析與產生。純計算，不碰檔案。

use paperreader_core::error::LibError;
use paperreader_core::format::Map;
use paperreader_core::notes::*;

const EXAMPLE: &str = include_str!("data/notes_example.md");
const PAPER: &str = "01JCK3W2E4H6J8K0M2N4P6Q8RS";
const MATERIAL: &str = "01JCK3W2E7T9V1X3Z5B7D9F1GH";
const DEVICE: &str = "01JCK3T1A2B3C4D5E6F7G8H9JK";
const SHA: &str = "01018b33f4a1c2d3e4f5061728394a5b6c7d8e9f00112233445566778899aabb";
const H1: &str = "h-01jck5a7m3q9x2v8b4n6r0t1yz";
const H2: &str = "h-01jck5c2d8f0g4h6j8k0m2n4p6";
const NEW_ID: &str = "h-01jck5e9q2w4r6t8y0u2i4o6p8";

fn stamp(n: u32) -> Stamp {
    Stamp {
        time: format!("2026-11-16T09:00:{n:02}.000Z"),
        device: DEVICE.to_string(),
    }
}

fn labels() -> NotesLabels {
    NotesLabels::default()
}

fn hl(id: &str, page: u32, start: u32, quote: &str, comment: &str) -> Highlight {
    Highlight {
        id: id.into(),
        kind: "highlight".into(),
        material: MATERIAL.into(),
        file: FileRef {
            sha256: Some(SHA.into()),
            fingerprint: Some("fp-1".into()),
            extra: Map::new(),
        },
        page: PageRef {
            index: page,
            label: Some((page + 1).to_string()),
            extra: Map::new(),
        },
        rects: vec![[72.1, 512.4, 300.5, 524.0]],
        next_page_rects: vec![],
        position: Some(TextPosition {
            start,
            end: start + 10,
            basis: "pdfjs-6.5.438".into(),
            extra: Map::new(),
        }),
        quote: Quote {
            exact: quote.into(),
            prefix: Some("before ".into()),
            suffix: None,
            extra: Map::new(),
        },
        color: None,
        created: "2026-11-16T08:00:00.000Z".into(),
        modified: "2026-11-16T08:00:00.000Z".into(),
        extra: Map::new(),
        comment: comment.into(),
        complete: true,
    }
}

fn strip_stamps(s: &str) -> String {
    s.split_inclusive('\n')
        .filter(|l| !l.starts_with("paperreader_modified"))
        .collect()
}

/// 範例裡一條劃線（從資料行到結尾標記，含結尾標記的換行）的位元組範圍。
fn block_range(text: &str, id: &str) -> (usize, usize) {
    let key = format!("\"id\":\"{id}\"");
    let line_start = text[..text.find(&key).unwrap()].rfind('\n').unwrap() + 1;
    let end_marker = "<!-- paperreader:highlight-end -->\n";
    let end = text[line_start..].find(end_marker).unwrap() + line_start + end_marker.len();
    (line_start, end)
}

#[test]
fn example_parses() {
    let n = parse(EXAMPLE);
    assert_eq!(n.paper_id(), Some(PAPER));
    assert_eq!(n.modified(), Some("2026-11-15T14:03:27.004Z"));
    assert_eq!(n.modified_by(), Some(DEVICE));
    assert_eq!(
        n.thoughts(),
        "這篇的貢獻主要在推論速度，精度提升有限。\n\n和 RT-DETRv2 比較時要注意訓練的輪數不同。"
    );
    assert!(n.thoughts_complete());
    let hs = n.highlights();
    assert_eq!(hs.len(), 2);
    let a = &hs[0];
    assert_eq!(a.id, H1);
    assert_eq!(a.kind, "highlight");
    assert_eq!(a.material, MATERIAL);
    assert_eq!(a.file.sha256.as_deref(), Some(SHA));
    assert_eq!(a.file.fingerprint.as_deref(), Some("9f2c4e7a1b3d5f80"));
    assert_eq!(a.page.index, 6);
    assert_eq!(a.page.label.as_deref(), Some("7"));
    assert_eq!(a.rects.len(), 2);
    assert_eq!(a.rects[0], [72.1, 512.4, 300.5, 524.0]);
    assert!(a.next_page_rects.is_empty());
    let p = a.position.as_ref().unwrap();
    assert_eq!(
        (p.start, p.end, p.basis.as_str()),
        (1834, 1902, "pdfjs-6.5.438")
    );
    assert_eq!(
        a.quote.exact,
        "Transformer-based detectors suffer from high computational cost"
    );
    assert_eq!(a.quote.prefix.as_deref(), Some("In practice, "));
    assert_eq!(a.quote.suffix.as_deref(), Some(", which limits"));
    assert_eq!(a.color, None);
    assert_eq!(a.created, "2026-11-15T13:40:00.000Z");
    assert_eq!(a.modified, "2026-11-15T13:42:10.500Z");
    assert_eq!(a.comment, "這裡的成本指推論時間，不是訓練。");
    assert!(a.complete);
    let b = &hs[1];
    assert_eq!(b.id, H2);
    assert_eq!(b.page.index, 11);
    assert_eq!(b.quote.prefix, None);
    assert_eq!(b.comment, "");
    assert!(b.complete);
    assert!(n.unreadable().is_empty());
}

#[test]
fn new_file_then_parse_roundtrip() {
    let text = new_file(PAPER, "A Paper", &labels(), &stamp(1));
    let n = parse(&text);
    assert_eq!(n.paper_id(), Some(PAPER));
    assert_eq!(n.modified(), Some(stamp(1).time.as_str()));
    assert_eq!(n.modified_by(), Some(DEVICE));
    assert_eq!(n.thoughts(), "");
    assert!(n.thoughts_complete());
    assert!(n.highlights().is_empty());
    assert!(text.contains("# A Paper"));
    assert!(text.contains("## Thoughts"));
    assert!(text.contains("## Highlights"));
    assert_eq!(read_paper_id(&text).as_deref(), Some(PAPER));
}

#[test]
fn add_then_parse_roundtrip() {
    let text = new_file(PAPER, "A Paper", &labels(), &stamp(1));
    let h = hl(NEW_ID, 3, 50, "quoted text", "my comment\nsecond line");
    let out = parse(&text)
        .with_new_highlight(&h, &labels(), PAPER, &stamp(2))
        .unwrap();
    let got = parse(&out).highlights();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], h);
    // 資料行的 JSON 沒有 comment、complete，有 "v":1
    let data_line = out
        .lines()
        .find(|l| l.starts_with("<!-- paperreader:highlight {"))
        .unwrap();
    assert!(data_line.contains("\"v\":1"));
    assert!(!data_line.contains("comment"));
    assert!(!data_line.contains("complete"));
    // 沒有短評時那一段與它後面的空行都不寫
    let h2 = hl("h-01jck5e9q2w4r6t8y0u2i4o6p9", 4, 5, "q2", "");
    let out2 = parse(&out)
        .with_new_highlight(&h2, &labels(), PAPER, &stamp(3))
        .unwrap();
    let got2 = parse(&out2).highlights();
    assert_eq!(got2.len(), 2);
    assert_eq!(got2[1].comment, "");
    assert!(out2.contains("^h-01jck5e9q2w4r6t8y0u2i4o6p9\n\n[Open in PaperReader p. 5]("));
}

#[test]
fn every_write_updates_file_stamp() {
    let h = hl(NEW_ID, 3, 50, "q", "c");
    let a = parse(EXAMPLE)
        .with_new_highlight(&h, &labels(), PAPER, &stamp(1))
        .unwrap();
    let b = parse(&a)
        .with_comment(H1, "new", &labels(), PAPER, &stamp(2))
        .unwrap();
    let c = parse(&b).without_highlight(H2, &stamp(3)).unwrap();
    let d = parse(&c).with_thoughts("t", &labels(), &stamp(4)).unwrap();
    for (i, text) in [&a, &b, &c, &d].iter().enumerate() {
        let n = parse(text);
        let s = stamp(i as u32 + 1);
        assert_eq!(n.modified(), Some(s.time.as_str()), "step {i}");
        assert_eq!(n.modified_by(), Some(s.device.as_str()), "step {i}");
        assert!(text.contains("tags: [paper]\n"), "step {i}");
    }
}

#[test]
fn stamp_lines_inserted_when_missing() {
    let text = format!("---\npaperreader_paper: {PAPER}\n---\n\n# T\n");
    let out = parse(&text)
        .with_thoughts("x", &labels(), &stamp(5))
        .unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines[1], format!("paperreader_paper: {PAPER}"));
    assert_eq!(lines[2], format!("paperreader_modified: {}", stamp(5).time));
    assert_eq!(lines[3], format!("paperreader_modified_by: {DEVICE}"));
    assert_eq!(lines[4], "---");
}

#[test]
fn edit_preserves_other_bytes() {
    let orig = strip_stamps(EXAMPLE);

    // 改第二條的短評
    let out = parse(EXAMPLE)
        .with_comment(H2, "新的短評", &labels(), PAPER, &stamp(1))
        .unwrap();
    let out = strip_stamps(&out);
    let (s2, e2) = block_range(&orig, H2);
    assert!(out.starts_with(&orig[..s2]), "prefix changed");
    assert!(out.ends_with(&orig[e2..]), "suffix changed");
    assert!(out.len() > s2 + (orig.len() - e2));

    // 刪第二條
    let out = parse(EXAMPLE).without_highlight(H2, &stamp(1)).unwrap();
    let out = strip_stamps(&out);
    // 刪掉那一段與緊接在後面的一個空行
    let expect = format!("{}{}", &orig[..s2], &orig[e2 + 1..]);
    assert_eq!(out, expect);

    // 改想法
    let out = parse(EXAMPLE)
        .with_thoughts("只有一行", &labels(), &stamp(1))
        .unwrap();
    let out = strip_stamps(&out);
    let ts = orig.find("<!-- paperreader:thoughts -->\n").unwrap()
        + "<!-- paperreader:thoughts -->\n".len();
    let te = orig.find("<!-- paperreader:thoughts-end -->").unwrap();
    assert_eq!(out, format!("{}只有一行\n{}", &orig[..ts], &orig[te..]));
}

#[test]
fn crlf_file_stays_crlf() {
    let crlf = EXAMPLE.replace('\n', "\r\n");
    let n = parse(&crlf);
    assert_eq!(n.paper_id(), Some(PAPER));
    assert_eq!(n.highlights().len(), 2);
    let h = hl(NEW_ID, 3, 50, "line1\nline2", "c1\nc2");
    let out = n
        .with_new_highlight(&h, &labels(), PAPER, &stamp(1))
        .unwrap();
    // 檔案裡沒有單獨的 \n
    let bytes = out.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            assert!(i > 0 && bytes[i - 1] == b'\r', "bare LF at {i}");
        }
    }
    let got = parse(&out).highlights();
    assert_eq!(got.len(), 3);
    assert_eq!(
        got.iter().find(|x| x.id == NEW_ID).unwrap().comment,
        "c1\nc2"
    );
}

#[test]
fn bom_is_kept() {
    let with_bom = format!("\u{FEFF}{EXAMPLE}");
    let n = parse(&with_bom);
    assert_eq!(n.paper_id(), Some(PAPER));
    assert_eq!(n.highlights().len(), 2);
    let out = n.with_thoughts("x", &labels(), &stamp(1)).unwrap();
    assert!(out.starts_with('\u{FEFF}'));
    assert_eq!(parse(&out).paper_id(), Some(PAPER));
}

#[test]
fn quote_with_comment_terminator() {
    let text = new_file(PAPER, "T", &labels(), &stamp(1));
    let q = "a --> b <!-- c";
    let h = hl(NEW_ID, 0, 0, q, "");
    let out = parse(&text)
        .with_new_highlight(&h, &labels(), PAPER, &stamp(2))
        .unwrap();
    let data_line = out
        .lines()
        .find(|l| l.starts_with("<!-- paperreader:highlight {"))
        .unwrap();
    // 只在結尾有一個 -->，除了開頭的 <!-- 之外沒有別的
    assert_eq!(data_line.matches("-->").count(), 1);
    assert_eq!(data_line.matches("<!--").count(), 1);
    assert_eq!(parse(&out).highlights()[0].quote.exact, q);
}

#[test]
fn comment_with_heading_stays_in_block() {
    let h = hl(NEW_ID, 20, 0, "q", "# 小標\n內文");
    let out = parse(EXAMPLE)
        .with_new_highlight(&h, &labels(), PAPER, &stamp(1))
        .unwrap();
    let got = parse(&out).highlights();
    assert_eq!(got.len(), 3);
    assert_eq!(
        got.iter().find(|x| x.id == NEW_ID).unwrap().comment,
        "# 小標\n內文"
    );
    // 它前面那一條仍讀得到
    assert!(got.iter().any(|x| x.id == H2));
}

#[test]
fn comment_with_marker_is_rejected() {
    let n = parse(EXAMPLE);
    let bad = "ok\n  <!-- paperreader:thoughts -->\nmore";
    let r = n.with_comment(H1, bad, &labels(), PAPER, &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = n.with_thoughts(bad, &labels(), &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = n.with_comment(H1, "nul\0", &labels(), PAPER, &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
}

#[test]
fn comment_crlf_is_normalized() {
    let out = parse(EXAMPLE)
        .with_comment(H1, "a\r\nb\rc", &labels(), PAPER, &stamp(1))
        .unwrap();
    assert!(!out.contains('\r'));
    assert_eq!(parse(&out).highlights()[0].comment, "a\nb\nc");
}

#[test]
fn user_edited_comment_is_read() {
    let edited = EXAMPLE.replace(
        "這裡的成本指推論時間，不是訓練。",
        "使用者在筆記工具裡改過了。",
    );
    assert_eq!(
        parse(&edited).highlights()[0].comment,
        "使用者在筆記工具裡改過了。"
    );
}

#[test]
fn missing_block_id_line_falls_back() {
    let edited = EXAMPLE.replace(&format!("^{H1}\n\n"), "");
    assert_ne!(edited, EXAMPLE);
    let n = parse(&edited);
    assert_eq!(
        n.highlights()[0].comment,
        "這裡的成本指推論時間，不是訓練。"
    );
}

#[test]
fn back_link_is_not_part_of_comment() {
    // 範例第一條的短評不含連結
    assert!(
        !parse(EXAMPLE).highlights()[0]
            .comment
            .contains("paperreader://")
    );
}

#[test]
fn foreign_paperreader_link_is_kept_in_comment() {
    let link = "[別處](paperreader://open?paper=P&material=M&page=3&highlight=h-other)";
    let edited = EXAMPLE.replace(
        "這裡的成本指推論時間，不是訓練。\n",
        &format!("這裡的成本指推論時間，不是訓練。\n\n{link}\n"),
    );
    let c = parse(&edited).highlights()[0].comment.clone();
    assert!(c.ends_with(link), "{c:?}");
}

#[test]
fn user_text_after_last_highlight_survives_delete() {
    let edited = EXAMPLE.replace("<!-- paperreader:highlights-end -->\n", "");
    let edited = format!("{edited}\n我自己寫的一段，在最後一條劃線之後。\n");
    let n = parse(&edited);
    assert_eq!(n.highlights().len(), 2);
    let out = n.without_highlight(H2, &stamp(1)).unwrap();
    assert!(out.contains("我自己寫的一段，在最後一條劃線之後。\n"));
    assert!(out.contains("這段在標記之外，應用不讀、不改。"));
    assert!(!out.contains(H2));
}

#[test]
fn moved_highlight_before_user_text() {
    // 把第二條搬到「## 我自己加的」那一段之後
    let orig = strip_stamps(EXAMPLE);
    let (s2, e2) = block_range(&orig, H2);
    let block = orig[s2..e2].to_string();
    let without = format!("{}{}", &orig[..s2], &orig[e2..]);
    let user_start = without.find("## 我自己加的").unwrap();
    let moved = format!(
        "{}{}\n{}",
        &without[..user_start],
        block,
        &without[user_start..]
    );
    let n = parse(&moved);
    let hs = n.highlights();
    assert_eq!(hs.len(), 2);
    assert!(hs.iter().all(|h| h.complete));
    let out = n
        .with_comment(H2, "改過", &labels(), PAPER, &stamp(1))
        .unwrap();
    assert!(out.contains("## 我自己加的\n\n這段在標記之外，應用不讀、不改。\n"));
    assert_eq!(
        parse(&out)
            .highlights()
            .iter()
            .find(|h| h.id == H2)
            .unwrap()
            .comment,
        "改過"
    );
}

#[test]
fn missing_highlight_end_is_incomplete() {
    // 刪掉第一條的 highlight-end
    let marker = "<!-- paperreader:highlight-end -->\n";
    let edited = EXAMPLE.replacen(marker, "", 1);
    let n = parse(&edited);
    let hs = n.highlights();
    assert_eq!(hs.len(), 2);
    let first = hs.iter().find(|h| h.id == H1).unwrap();
    assert!(!first.complete);
    assert_eq!(first.page.index, 6);
    let r = n.with_comment(H1, "x", &labels(), PAPER, &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    let r = n.without_highlight(H1, &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    // 第二條照常可以改
    assert!(
        n.with_comment(H2, "ok", &labels(), PAPER, &stamp(1))
            .is_ok()
    );
}

#[test]
fn highlight_moved_into_thoughts() {
    let orig = strip_stamps(EXAMPLE);
    let (s2, e2) = block_range(&orig, H2);
    let block = orig[s2..e2].to_string();
    let without = format!("{}{}", &orig[..s2], &orig[e2..]);
    let te = without.find("<!-- paperreader:thoughts-end -->").unwrap();
    let moved = format!("{}{}{}", &without[..te], block, &without[te..]);
    let n = parse(&moved);
    assert!(!n.thoughts_complete());
    assert!(!n.thoughts().contains("\"v\":1"));
    let r = n.with_thoughts("x", &labels(), &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
    // 那條劃線照樣讀得到
    assert!(n.highlights().iter().any(|h| h.id == H2));
}

#[test]
fn missing_thoughts_end() {
    let edited = EXAMPLE.replace("<!-- paperreader:thoughts-end -->\n", "");
    let n = parse(&edited);
    assert!(!n.thoughts_complete());
    assert!(n.thoughts().starts_with("這篇的貢獻主要在推論速度"));
    let r = n.with_thoughts("x", &labels(), &stamp(1));
    assert!(matches!(r, Err(LibError::InvalidInput { .. })), "{r:?}");
}

#[test]
fn unknown_marker_is_boundary_and_kept() {
    let edited = EXAMPLE.replacen(
        "<!-- paperreader:highlight-end -->\n",
        "<!-- paperreader:future -->\n<!-- paperreader:highlight-end -->\n",
        1,
    );
    let n = parse(&edited);
    let hs = n.highlights();
    assert!(!hs.iter().find(|h| h.id == H1).unwrap().complete);
    // 改別的地方，未知標記逐位元組保留
    let out = n
        .with_comment(H2, "x", &labels(), PAPER, &stamp(1))
        .unwrap();
    assert!(out.contains("<!-- paperreader:future -->\n"));
}

#[test]
fn broken_data_line_is_unreadable_and_kept() {
    let edited = EXAMPLE.replacen(
        "{\"v\":1,\"id\":\"h-01jck5a7",
        "{\"v\":1,\"id\":\"h-01jck5a7 BROKEN",
        1,
    );
    let broken_line_no = EXAMPLE
        .lines()
        .position(|l| l.contains("h-01jck5a7m3q9x2v8b4n6r0t1yz\",\"kind\""))
        .unwrap()
        + 1;
    let n = parse(&edited);
    let un = n.unreadable();
    assert_eq!(un.len(), 1);
    assert_eq!(un[0].line, broken_line_no);
    assert!(un[0].text.contains("BROKEN"));
    assert_eq!(n.highlights().len(), 1);
    let out = n
        .with_comment(H2, "x", &labels(), PAPER, &stamp(1))
        .unwrap();
    let broken_text: String = edited
        .lines()
        .skip(broken_line_no - 1)
        .take(11)
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(
        out.contains(&broken_text),
        "broken block must be kept verbatim"
    );
}

#[test]
fn newer_version_and_duplicate_id_are_unreadable() {
    let v2 = EXAMPLE.replacen(
        "{\"v\":1,\"id\":\"h-01jck5a7",
        "{\"v\":2,\"id\":\"h-01jck5a7",
        1,
    );
    let n = parse(&v2);
    assert_eq!(n.unreadable().len(), 1);
    assert_eq!(n.highlights().len(), 1);
    // 第二條的 id 改成與第一條相同
    let dup = EXAMPLE.replacen(H2, H1, 2);
    let n = parse(&dup);
    assert_eq!(n.highlights().len(), 1);
    assert_eq!(n.unreadable().len(), 1);
    assert!(n.unreadable()[0].reason.contains("duplicate"));
}

#[test]
fn unknown_keys_survive_comment_edit() {
    let edited = EXAMPLE.replacen(
        "\"modified\":\"2026-11-15T13:42:10.500Z\"}",
        "\"modified\":\"2026-11-15T13:42:10.500Z\",\"x_future\":1}",
        1,
    );
    let n = parse(&edited);
    assert_eq!(
        n.highlights()[0].extra.get("x_future"),
        Some(&serde_json::json!(1))
    );
    let out = n
        .with_comment(H1, "new", &labels(), PAPER, &stamp(1))
        .unwrap();
    let h = parse(&out)
        .highlights()
        .into_iter()
        .find(|h| h.id == H1)
        .unwrap();
    assert_eq!(h.extra.get("x_future"), Some(&serde_json::json!(1)));
    assert_eq!(h.modified, stamp(1).time);
    assert_eq!(h.created, "2026-11-15T13:40:00.000Z");
    assert_eq!(h.comment, "new");
}

#[test]
fn missing_sections_are_appended() {
    let base = format!("---\npaperreader_paper: {PAPER}\n---\n\n我的筆記\n");
    let n = parse(&base);
    let h = hl(NEW_ID, 1, 0, "q", "");
    let out = n
        .with_new_highlight(&h, &labels(), PAPER, &stamp(1))
        .unwrap();
    assert!(strip_stamps(&out).starts_with(&strip_stamps(&base)));
    assert!(out.contains("## Highlights"));
    assert!(out.contains("<!-- paperreader:highlights -->"));
    assert!(
        out.trim_end()
            .ends_with("<!-- paperreader:highlights-end -->")
    );
    assert_eq!(parse(&out).highlights().len(), 1);

    let out2 = parse(&base)
        .with_thoughts("想法", &labels(), &stamp(1))
        .unwrap();
    assert!(strip_stamps(&out2).starts_with(&strip_stamps(&base)));
    assert!(out2.contains("## Thoughts"));
    assert_eq!(parse(&out2).thoughts(), "想法");
}

#[test]
fn region_without_end_marker_gets_new_highlight_after_start_marker() {
    // 已有劃線區就不另開新區；沒有結尾標記時放在開頭標記之後
    let edited = EXAMPLE.replace("<!-- paperreader:highlights-end -->\n", "");
    let n = parse(&edited);
    let h = hl(NEW_ID, 20, 0, "q", "c");
    let out = n
        .with_new_highlight(&h, &labels(), PAPER, &stamp(1))
        .unwrap();
    assert_eq!(out.matches("<!-- paperreader:highlights -->").count(), 1);
    assert_eq!(out.matches("## Highlights").count(), 0);
    let marker = "<!-- paperreader:highlights -->\n";
    let at = out.find(marker).unwrap() + marker.len();
    assert!(
        out[at..].starts_with("<!-- paperreader:highlight {"),
        "inserted right after the start marker"
    );
    // 原本的每一個位元組都還在（除了屬性區的兩行時間與新插入的那一段）
    let new_block_start = at;
    let new_block_end = block_range(&out, NEW_ID).1 + 1; // 加上後面的空行
    let without_new = format!("{}{}", &out[..new_block_start], &out[new_block_end..]);
    assert_eq!(strip_stamps(&without_new), strip_stamps(&edited));
    assert_eq!(parse(&out).highlights().len(), 3);
}

#[test]
fn incomplete_highlight_in_region_does_not_open_new_regions() {
    // 劃線區裡有一條缺結尾標記的劃線時，連加三條，劃線區仍只有一個
    let marker = "<!-- paperreader:highlight-end -->\n";
    let edited = EXAMPLE.replacen(marker, "", 1); // 第一條缺結尾標記
    let mut cur = edited.clone();
    let ids = [
        "h-01jck5e9q2w4r6t8y0u2i4o6p1",
        "h-01jck5e9q2w4r6t8y0u2i4o6p2",
        "h-01jck5e9q2w4r6t8y0u2i4o6p3",
    ];
    for (i, id) in ids.iter().enumerate() {
        cur = parse(&cur)
            .with_new_highlight(
                &hl(id, 20 + i as u32, 0, "q", "c"),
                &labels(),
                PAPER,
                &stamp(2),
            )
            .unwrap();
    }
    assert_eq!(cur.matches("<!-- paperreader:highlights -->").count(), 1);
    assert_eq!(
        cur.matches("<!-- paperreader:highlights-end -->").count(),
        1
    );
    assert_eq!(cur.matches("## Highlights").count(), 0);
    let n = parse(&cur);
    let hs = n.highlights();
    assert_eq!(hs.len(), 5);
    assert!(!hs.iter().find(|h| h.id == H1).unwrap().complete);
    assert!(
        ids.iter()
            .all(|id| hs.iter().any(|h| h.id == *id && h.complete))
    );
    // 使用者自己的段落與不完整的那一條逐位元組還在
    assert!(cur.contains("## 我自己加的\n\n這段在標記之外，應用不讀、不改。\n"));
    let first_data = edited
        .lines()
        .find(|l| l.contains(&format!("\"id\":\"{H1}\"")))
        .unwrap();
    assert!(cur.contains(first_data));
}

#[test]
fn comment_edit_keeps_every_other_data_line_value() {
    // 使用者在筆記工具裡手改成不合規則的 page.label、整數座標、
    // 不認得的鍵；改短評之後除了 modified，資料行的每個值與寫法都不變
    let edited = EXAMPLE
        .replacen("\"label\":\"7\"", "\"label\":\"7*\"", 1)
        .replacen("[[310.0,640.2,540.8,651.9]]", "[[310,640,540,651]]", 1)
        .replacen(
            "\"modified\":\"2026-11-15T13:42:10.500Z\"}",
            "\"modified\":\"2026-11-15T13:42:10.500Z\",\"x_future\":{\"a\":[1,2]}}",
            1,
        );
    assert_ne!(edited, EXAMPLE);
    let data_of = |text: &str, id: &str| -> serde_json::Value {
        let line = text
            .lines()
            .find(|l| {
                l.starts_with("<!-- paperreader:highlight {")
                    && l.contains(&format!("\"id\":\"{id}\""))
            })
            .unwrap();
        let json = line
            .strip_prefix("<!-- paperreader:highlight ")
            .unwrap()
            .strip_suffix(" -->")
            .unwrap();
        serde_json::from_str(json).unwrap()
    };
    let raw_line = |text: &str, id: &str| -> String {
        text.lines()
            .find(|l| {
                l.starts_with("<!-- paperreader:highlight {")
                    && l.contains(&format!("\"id\":\"{id}\""))
            })
            .unwrap()
            .to_string()
    };
    let n = parse(&edited);
    let out = n
        .with_comment(H1, "新的", &labels(), PAPER, &stamp(3))
        .unwrap();
    let out = n
        .with_comment(H2, "也改", &labels(), PAPER, &stamp(4))
        .map(|o| {
            // 兩次改的是同一份原文上的不同條；各自驗證
            (o, out)
        });
    let (out2, out1) = out.unwrap();

    let before = data_of(&edited, H1);
    let mut after = data_of(&out1, H1);
    assert_eq!(after["modified"], stamp(3).time);
    after["modified"] = before["modified"].clone();
    assert_eq!(before, after);
    assert_eq!(after["page"]["label"], "7*");
    assert_eq!(after["x_future"], serde_json::json!({"a": [1, 2]}));
    // 看得到的頁碼：不合規則的標籤改用頁序加 1
    assert!(out1.contains("**p. 7**"));

    // 整數座標的寫法不變（不被改成 310.0）
    let raw2 = raw_line(&out2, H2);
    assert!(raw2.contains("[[310,640,540,651]]"), "{raw2}");
    // 另一條沒被改的資料行逐位元組不變
    assert_eq!(raw_line(&out1, H2), raw_line(&edited, H2));
}

#[test]
fn extra_comment_key_in_data_line_is_not_duplicated() {
    // 資料行裡多出 `comment`、`complete` 鍵，交給畫面的 JSON 不可以有兩個
    let edited = EXAMPLE.replacen(
        "\"modified\":\"2026-11-15T13:42:10.500Z\"}",
        "\"modified\":\"2026-11-15T13:42:10.500Z\",\"comment\":\"sneaky\",\"complete\":false}",
        1,
    );
    let h = parse(&edited).highlights().remove(0);
    assert_eq!(h.comment, "這裡的成本指推論時間，不是訓練。");
    assert!(h.complete);
    assert!(!h.extra.contains_key("comment") && !h.extra.contains_key("complete"));
    let text = serde_json::to_string(&h).unwrap();
    assert_eq!(text.matches("\"comment\"").count(), 1, "{text}");
    assert_eq!(text.matches("\"complete\"").count(), 1, "{text}");
}

#[test]
fn highlights_start_marker_on_last_line_without_newline() {
    let base = format!("---\npaperreader_paper: {PAPER}\n---\n\n<!-- paperreader:highlights -->");
    let out = parse(&base)
        .with_new_highlight(&hl(NEW_ID, 1, 0, "q", ""), &labels(), PAPER, &stamp(1))
        .unwrap();
    assert_eq!(out.matches("<!-- paperreader:highlights -->").count(), 1);
    assert_eq!(parse(&out).highlights().len(), 1);
}

#[test]
fn link_and_label_escaping() {
    let mut l = labels();
    l.back_link = "回到[原文]".to_string();
    let mut h = hl(NEW_ID, 4, 0, "q", "");
    h.page.label = Some("iv".into());
    let text = new_file(PAPER, "T", &l, &stamp(1));
    let out = parse(&text)
        .with_new_highlight(&h, &l, PAPER, &stamp(2))
        .unwrap();
    assert!(out.contains("**p. iv**"));
    assert!(out.contains("[回到\\[原文\\] p. iv]("));
    assert!(out.contains("&page=5&"));

    for bad in ["a\nb", "x*y", "", &"z".repeat(33)] {
        let mut h = hl(NEW_ID, 4, 0, "q", "");
        h.page.label = Some(bad.into());
        let out = parse(&text)
            .with_new_highlight(&h, &l, PAPER, &stamp(2))
            .unwrap();
        assert!(out.contains("**p. 5**"), "{bad:?}");
        assert_eq!(parse(&out).highlights()[0].page.label, None);
    }
}

#[test]
fn labels_and_title_are_sanitized() {
    let mut l = labels();
    l.thoughts = "a\nb".into();
    assert!(matches!(l.validate(), Err(LibError::InvalidInput { .. })));
    let mut l = labels();
    l.highlights = "x".repeat(101);
    assert!(matches!(l.validate(), Err(LibError::InvalidInput { .. })));
    let mut l = labels();
    l.back_link = String::new();
    assert!(matches!(l.validate(), Err(LibError::InvalidInput { .. })));
    assert!(labels().validate().is_ok());

    let text = new_file(PAPER, "Line1\nLine2\u{7}\r\n  Line3", &labels(), &stamp(1));
    assert!(text.contains("\n# Line1 Line2 Line3\n"), "{text}");
    // 篇名裡的換行與控制字元不會讓檔案多出一行
    let plain = new_file(PAPER, "x", &labels(), &stamp(1));
    assert_eq!(text.lines().count(), plain.lines().count());
}

#[test]
fn sorting() {
    let text = new_file(PAPER, "T", &labels(), &stamp(1));
    let mut cur = text;
    let items = [
        ("h-01jck5e9q2w4r6t8y0u2i4o6p1", 3u32, 5u32),
        ("h-01jck5e9q2w4r6t8y0u2i4o6p2", 1, 50),
        ("h-01jck5e9q2w4r6t8y0u2i4o6p3", 1, 10),
    ];
    for (id, page, start) in items {
        cur = parse(&cur)
            .with_new_highlight(&hl(id, page, start, "q", ""), &labels(), PAPER, &stamp(2))
            .unwrap();
    }
    let ids: Vec<String> = parse(&cur).highlights().into_iter().map(|h| h.id).collect();
    assert_eq!(
        ids,
        [
            "h-01jck5e9q2w4r6t8y0u2i4o6p3",
            "h-01jck5e9q2w4r6t8y0u2i4o6p2",
            "h-01jck5e9q2w4r6t8y0u2i4o6p1"
        ]
    );
}

#[test]
fn remove_highlight_removes_whole_block() {
    let n = parse(EXAMPLE);
    let out = n.without_highlight(H1, &stamp(1)).unwrap();
    for gone in [H1, "這裡的成本指推論時間", "Transformer-based", "**p. 7**"] {
        assert!(!out.contains(gone), "{gone}");
    }
    assert!(out.contains(H2));
    let r = n.without_highlight("h-nosuch", &stamp(1));
    assert!(matches!(r, Err(LibError::NotFound { .. })), "{r:?}");
}

#[test]
fn front_matter_variants() {
    for text in [
        format!("---\npaperreader_paper: '{PAPER}'\n---\nbody\n"),
        format!("---\npaperreader_paper: \"{PAPER}\"\n---\nbody\n"),
        format!("---\npaperreader_paper:   {PAPER}   \n---\nbody\n"),
        format!("---\r\npaperreader_paper: {PAPER}\r\n---\r\nbody\r\n"),
        format!("---\npaperreader_paper: {PAPER}\n...\nbody\n"),
    ] {
        assert_eq!(parse(&text).paper_id(), Some(PAPER), "{text:?}");
        assert_eq!(read_paper_id(&text).as_deref(), Some(PAPER));
    }
    for text in [
        "no front matter\n".to_string(),
        format!("---\npaperreader_paper: {PAPER}\nbody never closed\n"),
    ] {
        assert_eq!(parse(&text).paper_id(), None, "{text:?}");
        assert_eq!(read_paper_id(&text), None);
    }
}

#[test]
fn read_paper_id_head_cut_mid_char() {
    let mut text = format!("---\npaperreader_paper: {PAPER}\n---\n");
    while text.len() < 8200 {
        text.push('字');
    }
    let bytes = text.as_bytes();
    // 8192 正好切在一個三位元組的字中間
    let cut = &bytes[..8192];
    assert!(std::str::from_utf8(cut).is_err());
    assert_eq!(read_paper_id_from_bytes(cut).as_deref(), Some(PAPER));
}
