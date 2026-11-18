use paperreader_core::error::{LibError, io_error};
use paperreader_core::ids::{new_id, now_compact, now_utc};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;

const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 'd' 代表一個數字、其餘字元照字面比對。
fn matches_shape(s: &str, shape: &str) -> bool {
    s.len() == shape.len()
        && s.chars().zip(shape.chars()).all(
            |(c, p)| {
                if p == 'd' { c.is_ascii_digit() } else { c == p }
            },
        )
}

#[test]
fn new_id_is_26_crockford_chars() {
    let id = new_id();
    assert_eq!(id.len(), 26);
    assert!(id.chars().all(|c| CROCKFORD.contains(c)), "bad id: {id}");
}

#[test]
fn new_ids_differ() {
    let ids: HashSet<String> = (0..1000).map(|_| new_id()).collect();
    assert_eq!(ids.len(), 1000);
}

#[test]
fn now_utc_matches_format() {
    let s = now_utc();
    assert!(matches_shape(&s, "dddd-dd-ddTdd:dd:dd.dddZ"), "bad: {s}");
}

#[test]
fn now_compact_matches_format() {
    let s = now_compact();
    assert!(matches_shape(&s, "ddddddddTddddddZ"), "bad: {s}");
}

#[test]
fn io_error_maps_kinds() {
    let p = Path::new("x");
    let mk = |k| io_error(p, std::io::Error::from(k));
    assert!(matches!(mk(ErrorKind::NotFound), LibError::NotFound { .. }));
    assert!(matches!(
        mk(ErrorKind::PermissionDenied),
        LibError::PermissionDenied { .. }
    ));
    assert!(matches!(mk(ErrorKind::StorageFull), LibError::NoSpace));
    assert!(matches!(mk(ErrorKind::Other), LibError::Io { .. }));
}
