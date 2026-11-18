//! 加入的來源也可以是資料流（別的應用分享進來的檔案在 Android 上不一定有路徑）。

mod common;

use common::*;
use paperreader_core::error::LibError;
use paperreader_core::format::BibDraft;
use paperreader_core::jsonio::sha256_hex;
use paperreader_core::store::AddOutcome;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

fn title(t: &str) -> BibDraft {
    BibDraft {
        title: Some(t.into()),
        ..Default::default()
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// 讀了一半就出錯的資料流。
struct FailingReader {
    served: usize,
    fail_after: usize,
}

impl Read for FailingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.served >= self.fail_after {
            return Err(std::io::Error::other("stream broke"));
        }
        let n = buf.len().min(self.fail_after - self.served).min(4096);
        buf[..n].fill(7);
        self.served += n;
        Ok(n)
    }
}

#[test]
fn reader_that_fails_midway_leaves_nothing() {
    let (dir, mut lib) = temp_library();
    let mut r = FailingReader {
        served: 0,
        fail_after: 10_000,
    };
    let res = lib.add_paper_from_reader(&mut r, Some("shared.pdf"), title("x"), None);
    assert!(matches!(res, Err(LibError::Io { .. })), "{res:?}");
    // 暫存資料夾已經建立並寫入了一部分，出錯後必須整個刪掉
    assert!(r.served > 0);
    assert!(names_in(&dir.path().join("papers")).is_empty());
    assert!(names_in(dir.path()).iter().all(|n| !n.starts_with(".pr-")));
    assert!(lib.list_papers().is_empty());
}

#[test]
fn reader_adds_with_correct_content_and_fingerprint() {
    let (dir, mut lib) = temp_library();
    let bytes = fake_bytes(42, 3_500_000); // 超過一個緩衝區
    let out = lib
        .add_paper_from_reader(
            &mut Cursor::new(bytes.clone()),
            Some("2304.08069v3.pdf"),
            title("Streamed"),
            Some(9),
        )
        .unwrap();
    let AddOutcome::Added(id) = out else {
        panic!("{out:?}")
    };

    let folder = dir.path().join("papers/Streamed");
    assert_eq!(names_in(&folder), ["paper.json", "source.pdf"]);
    assert_eq!(fs::read(folder.join("source.pdf")).unwrap(), bytes);

    let (paper, _) = lib.get_paper(&id).unwrap();
    let m = &paper.materials[0];
    assert_eq!(m.sha256.as_deref(), Some(sha256_hex(&bytes).as_str()));
    assert_eq!(m.bytes, Some(bytes.len() as u64));
    assert_eq!(m.pages, Some(9));
    assert_eq!(
        m.provenance.origin_name.as_deref(),
        Some("2304.08069v3.pdf")
    );
}

#[test]
fn reader_without_origin_name_records_null_and_names_folder_untitled() {
    let (dir, mut lib) = temp_library();
    let out = lib
        .add_paper_from_reader(
            &mut Cursor::new(fake_bytes(1, 100)),
            None,
            BibDraft::default(),
            None,
        )
        .unwrap();
    let AddOutcome::Added(id) = out else {
        panic!("{out:?}")
    };
    assert_eq!(names_in(&dir.path().join("papers")), ["untitled"]);
    assert_eq!(
        lib.get_paper(&id).unwrap().0.materials[0]
            .provenance
            .origin_name,
        None
    );
}

#[test]
fn empty_reader_is_invalid_input_and_leaves_nothing() {
    let (dir, mut lib) = temp_library();
    let res = lib.add_paper_from_reader(
        &mut Cursor::new(Vec::new()),
        Some("e.pdf"),
        title("x"),
        None,
    );
    assert!(matches!(res, Err(LibError::InvalidInput { .. })), "{res:?}");
    assert!(names_in(&dir.path().join("papers")).is_empty());
}

#[test]
fn same_content_from_reader_and_from_file_is_one_paper() {
    let (_dir, mut lib) = temp_library();
    let src = tempfile::tempdir().unwrap();
    let pdf = fake_pdf(src.path(), "a.pdf", 5000);
    let bytes = fs::read(&pdf).unwrap();
    let AddOutcome::Added(id) = lib.add_paper(&pdf, title("A"), None).unwrap() else {
        panic!()
    };
    let again = lib
        .add_paper_from_reader(
            &mut Cursor::new(bytes),
            Some("shared.pdf"),
            title("A again"),
            None,
        )
        .unwrap();
    assert_eq!(again, AddOutcome::AlreadyPresent(id));
}

#[test]
fn reader_add_in_read_only_library_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("library.json"),
        "{\"format\":2,\"id\":\"01JCK3V7Q8Z9X5M2N4P6R8T0AB\"}",
    )
    .unwrap();
    let mut lib = paperreader_core::local::LocalLibrary::open(dir.path(), DEVICE).unwrap();
    let before = snapshot(dir.path());
    let res = lib.add_paper_from_reader(&mut Cursor::new(vec![1, 2, 3]), None, title("x"), None);
    assert!(matches!(res, Err(LibError::ReadOnly)), "{res:?}");
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn works_through_trait_object() {
    let (_dir, mut lib) = temp_library();
    let store = &mut lib;
    let out = store
        .add_paper_from_reader(
            &mut Cursor::new(fake_bytes(3, 64)),
            Some("x.pdf"),
            title("T"),
            None,
        )
        .unwrap();
    assert!(matches!(out, AddOutcome::Added(_)));
    assert_eq!(store.list_papers().len(), 1);
}
