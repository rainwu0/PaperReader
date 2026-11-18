//! 工作 1：是不是 PDF、預設篇名。

mod common;

use paperreader_core::error::LibError;
use paperreader_core::pdf::{PDF_HEADER_WINDOW, is_pdf_file, looks_like_pdf, title_from_file_name};
use std::fs;

#[test]
fn header_at_start() {
    assert!(looks_like_pdf(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n1 0 obj"));
}

#[test]
fn header_after_junk_within_window() {
    assert_eq!(PDF_HEADER_WINDOW, 1024);
    let mut v = vec![0xEF, 0xBB, 0xBF];
    v.extend(std::iter::repeat_n(b'x', 100));
    v.extend_from_slice(b"%PDF-1.4");
    assert!(looks_like_pdf(&v));

    // `%PDF-` 從位移 1019 起：最後一個字元在位移 1023，剛好在窗內
    let mut at_1019 = vec![b'x'; 1019];
    at_1019.extend_from_slice(b"%PDF-");
    assert!(looks_like_pdf(&at_1019));

    // 從位移 1020 起：最後一個字元在 1024，超出
    let mut at_1020 = vec![b'x'; 1020];
    at_1020.extend_from_slice(b"%PDF-");
    assert!(!looks_like_pdf(&at_1020));
}

#[test]
fn zip_and_empty_are_not_pdf() {
    assert!(!looks_like_pdf(b"PK\x03\x04 some zip bytes"));
    assert!(!looks_like_pdf(b""));
}

#[test]
fn is_pdf_file_cases() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.pdf");
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.resize(10 * 1024 * 1024, b'z');
    fs::write(&big, &bytes).unwrap();
    assert!(is_pdf_file(&big).unwrap());

    let plain = dir.path().join("a.txt");
    fs::write(&plain, b"hello").unwrap();
    assert!(!is_pdf_file(&plain).unwrap());

    let missing = dir.path().join("nope.pdf");
    assert!(matches!(
        is_pdf_file(&missing),
        Err(LibError::NotFound { .. })
    ));
    assert!(matches!(
        is_pdf_file(dir.path()),
        Err(LibError::InvalidInput { .. })
    ));
}

#[test]
fn title_from_file_name_cases() {
    let cases: &[(&str, Option<&str>)] = &[
        ("Zhao 2024.pdf", Some("Zhao 2024")),
        ("Alpha.PDF", Some("Alpha")),
        ("notes.txt", Some("notes.txt")),
        ("論文.pdf", Some("論文")),
        ("a.pdf.pdf", Some("a.pdf")),
        ("  b .pdf ", Some("b")),
        (".pdf", None),
        ("", None),
    ];
    for (input, want) in cases {
        assert_eq!(
            title_from_file_name(input).as_deref(),
            *want,
            "input {input:?}"
        );
    }
}
