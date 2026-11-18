use serde_json::Value;
use std::process::Command;

#[test]
fn invalid_device_file_is_rejected_and_nothing_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let device = dir.path().join("device");
    std::fs::write(&device, "../../x\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_prcli"))
        .arg(&root)
        .arg(&device)
        .arg("init")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "InvalidInput");
    assert!(!root.exists());
    // 代號檔原封不動
    assert_eq!(std::fs::read_to_string(&device).unwrap(), "../../x\n");
}
