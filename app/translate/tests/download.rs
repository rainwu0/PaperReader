//! 大檔下載（`download::fetch_to_file`）：邊收邊寫、核對通過才留下檔，失敗或取消不留下任何檔。

mod common;

use common::{Resp, serve};
use paperreader_translate::Cancel;
use paperreader_translate::download::{FetchFileError, RemoteFile, fetch_to_file};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn sha(s: &str) -> &'static str {
    let h: String = Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Box::leak(h.into_boxed_str())
}

fn remote(base: &str, body: &str, sha256: &'static str) -> RemoteFile {
    RemoteFile {
        url: Box::leak(format!("{base}/model.onnx").into_boxed_str()),
        size: body.len() as u64,
        zstd: false,
        sha256,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pr-dl-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plain(body: &'static str) -> impl Fn(&common::Req, usize) -> Resp {
    move |_, _| Resp {
        status: 200,
        headers: vec![("content-type".into(), "application/octet-stream".into())],
        body: body.to_string(),
    }
}

#[tokio::test]
async fn writes_the_file_after_the_checksum_matches() {
    let body = "pretend this is a layout model";
    let srv = serve(plain(body)).await;
    let dir = scratch("ok");
    let dest = dir.join("layout").join("PP-DocLayoutV3.onnx");
    let progress = AtomicU64::new(0);
    fetch_to_file(
        &remote(&srv.base, body, sha(body)),
        &dest,
        &progress,
        &Cancel::new(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), body);
    assert_eq!(progress.load(Ordering::Relaxed), body.len() as u64);
    assert!(!dest.with_file_name("PP-DocLayoutV3.onnx.part").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn checksum_mismatch_leaves_nothing() {
    let body = "pretend this is a layout model";
    let srv = serve(plain(body)).await;
    let dir = scratch("bad");
    let dest = dir.join("m.onnx");
    let e = fetch_to_file(
        &remote(&srv.base, body, sha("something else")),
        &dest,
        &AtomicU64::new(0),
        &Cancel::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, FetchFileError::Fetch(ref t) if t.kind == paperreader_translate::ErrorKind::BadResponse),
        "{e:?}"
    );
    assert!(!dest.exists());
    assert!(!dir.join("m.onnx.part").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn cancelled_download_leaves_nothing() {
    let body = "pretend this is a layout model";
    let srv = serve(plain(body)).await;
    let dir = scratch("cancel");
    let dest = dir.join("m.onnx");
    let cancel = Cancel::new();
    cancel.cancel();
    let e = fetch_to_file(
        &remote(&srv.base, body, sha(body)),
        &dest,
        &AtomicU64::new(0),
        &cancel,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, FetchFileError::Fetch(ref t) if t.kind == paperreader_translate::ErrorKind::Cancelled),
        "{e:?}"
    );
    assert!(!dest.exists());
    assert!(!dir.join("m.onnx.part").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn server_error_leaves_nothing() {
    let srv = serve(|_, _| Resp::status(404, "{}")).await;
    let dir = scratch("404");
    let dest = dir.join("m.onnx");
    assert!(
        fetch_to_file(
            &remote(&srv.base, "x", sha("x")),
            &dest,
            &AtomicU64::new(0),
            &Cancel::new()
        )
        .await
        .is_err()
    );
    assert!(!dest.exists());
    assert!(!dir.join("m.onnx.part").exists());
    let _ = std::fs::remove_dir_all(dir);
}
