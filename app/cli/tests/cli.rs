use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    device: PathBuf,
    pdf_dir: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lib");
        let device = dir.path().join("device");
        let pdf_dir = dir.path().join("src");
        std::fs::create_dir_all(&pdf_dir).unwrap();
        Env {
            _dir: dir,
            root,
            device,
            pdf_dir,
        }
    }

    fn pdf(&self, name: &str, seed: u8, len: usize) -> PathBuf {
        let p = self.pdf_dir.join(name);
        let mut bytes: Vec<u8> = (0..len)
            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
            .collect();
        // 核心加入時驗檔頭：以 %PDF- 開頭（長度不變）
        assert!(len >= 5);
        bytes[..5].copy_from_slice(b"%PDF-");
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// 回傳（標準輸出解析成的 JSON，結束碼）。標準輸出必須恰好一行。
    fn run(&self, args: &[&str]) -> (Value, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_prcli"))
            .arg(&self.root)
            .arg(&self.device)
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.trim_end().lines().count(), 1, "stdout: {text:?}");
        assert!(text.ends_with('\n'));
        (
            serde_json::from_str(text.trim_end()).unwrap(),
            out.status.code().unwrap(),
        )
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (v, code) = self.run(args);
        assert_eq!(code, 0, "{args:?} -> {v}");
        v
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn init_then_add_then_list() {
    let env = Env::new();
    let v = env.ok(&["init"]);
    assert_eq!(v["read_only"], false);
    assert_eq!(v["library_id"].as_str().unwrap().len(), 26);

    let pdf = env.pdf("a.pdf", 1, 3000);
    let v = env.ok(&[
        "add",
        s(&pdf),
        "--title",
        "DETRs",
        "--family",
        "Zhao",
        "--year",
        "2024",
        "--pages",
        "14",
    ]);
    assert_eq!(v["outcome"], "added");
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 26);

    let list = env.ok(&["list"]);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id.as_str());
    assert_eq!(list[0]["folder"], "Zhao 2024 - DETRs");
    assert_eq!(list[0]["bib"]["title"], "DETRs");
    assert_eq!(list[0]["materials"][0]["status"], "Ok");
    assert_eq!(list[0]["materials"][0]["pages"], 14);
}

#[test]
fn device_file_is_created_once_and_reused() {
    let env = Env::new();
    env.ok(&["init"]);
    let first = std::fs::read_to_string(&env.device).unwrap();
    assert_eq!(first.trim().len(), 26);
    env.ok(&["init"]);
    assert_eq!(std::fs::read_to_string(&env.device).unwrap(), first);
}

#[test]
fn add_twice_reports_already_present() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let first = env.ok(&["add", s(&pdf), "--title", "A"]);
    let second = env.ok(&["add", s(&pdf), "--title", "B"]);
    assert_eq!(second["outcome"], "already_present");
    assert_eq!(second["id"], first["id"]);
    assert_eq!(env.ok(&["list"]).as_array().unwrap().len(), 1);
}

#[test]
fn error_is_json_with_exit_code_1() {
    let env = Env::new();
    let (v, code) = env.run(&["get", "01ZZZZZZZZZZZZZZZZZZZZZZZZ"]);
    assert_eq!(code, 1);
    assert_eq!(v["error"], "NotFound");
    assert!(v["detail"].is_string());
}

#[test]
fn unknown_command_is_an_error_too() {
    let env = Env::new();
    let (v, code) = env.run(&["frobnicate"]);
    assert_eq!(code, 1);
    assert_eq!(v["error"], "InvalidInput");
}

#[test]
fn stress_edit_100() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "start"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let v = env.ok(&["stress-edit", &id, "100"]);
    assert_eq!(v["ok"], 100);
    let got = env.ok(&["get", &id]);
    assert_eq!(got["paper"]["bib"]["title"], "edit-100");
    // 資料夾裡沒有 .pr- 項目
    for e in std::fs::read_dir(env.root.join("papers/start")).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        assert!(!name.starts_with(".pr-"), "{name}");
    }
}

#[test]
fn set_title_with_stale_token_is_conflict() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "start"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let token = env.ok(&["get", &id])["token"].as_str().unwrap().to_string();
    // 不帶記號：先讀再寫
    env.ok(&["set-title", &id, "second"]);
    let (v, code) = env.run(&["set-title", &id, "third", "--token", &token]);
    assert_eq!(code, 1);
    assert_eq!(v["error"], "Conflict");
    assert_eq!(env.ok(&["get", &id])["paper"]["bib"]["title"], "second");
}

#[test]
fn remove_add_again_restore_flow() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "Paper"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(env.ok(&["remove", &id])["ok"], true);
    let v = env.ok(&["add", s(&pdf)]);
    assert_eq!(v["outcome"], "in_trash");
    assert_eq!(v["id"], id.as_str());
    assert_eq!(env.ok(&["restore", &id])["ok"], true);
    assert_eq!(env.ok(&["list"])[0]["id"], id.as_str());
}

#[test]
fn events_and_check() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "Paper"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mid = env.ok(&["list"])[0]["materials"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let e = env.ok(&["open-paper", &id, &mid]);
    assert_eq!(e["type"], "open");
    assert_eq!(e["paper"], id.as_str());
    env.ok(&["position", &id, &mid, "3", "0.5"]);
    assert_eq!(env.ok(&["events"]).as_array().unwrap().len(), 2);
    let only = env.ok(&["events", "--paper", &id]);
    assert_eq!(only.as_array().unwrap().len(), 2);
    assert_eq!(env.ok(&["check", "quick"]), serde_json::json!([]));
    assert_eq!(env.ok(&["check", "full"]), serde_json::json!([]));
    std::fs::write(env.root.join("papers/Paper/mine.txt"), "x").unwrap();
    let probs = env.ok(&["check", "quick"]);
    assert_eq!(probs[0]["kind"], "UnlistedFile");
}

#[test]
fn rescan_after_reports_changes_made_while_waiting() {
    let env = Env::new();
    let a = env.pdf("a.pdf", 1, 3000);
    let b = env.pdf("b.pdf", 2, 3000);
    let c = env.pdf("c.pdf", 3, 3000);
    let id_a = env.ok(&["add", s(&a), "--title", "First"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id_b = env.ok(&["add", s(&b), "--title", "Second"])["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 子程序開好文獻庫（在標準錯誤寫 ready）之後才改，改完再叫它重新掃描：照順序做，不靠時間
    // （原本固定等 0.8 秒、子程序等 2.5 秒，電腦忙時子程序還沒開好文獻庫就被改了，偶發失敗）
    let mut child = Command::new(env!("CARGO_BIN_EXE_prcli"))
        .arg(&env.root)
        .arg(&env.device)
        .args(["rescan-after", "stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(child.stderr.take().unwrap()),
        &mut ready,
    )
    .unwrap();
    assert_eq!(ready.trim(), "ready");
    // 等待期間：改一個資料夾名稱、把另一篇的篇名換成等長的字串、加入一篇新的
    std::fs::rename(
        env.root.join("papers/First"),
        env.root.join("papers/Renamed"),
    )
    .unwrap();
    let pj = env.root.join("papers/Second/paper.json");
    let text = std::fs::read_to_string(&pj).unwrap();
    std::fs::write(
        &pj,
        text.replace("\"title\": \"Second\"", "\"title\": \"Secnod\""),
    )
    .unwrap();
    let id_c = env.ok(&["add", s(&c), "--title", "Third"])["id"]
        .as_str()
        .unwrap()
        .to_string();

    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let has = |key: &str, id: &str| v[key].as_array().unwrap().iter().any(|x| x == id);
    assert!(has("changed", &id_a), "{v}");
    assert!(has("changed", &id_b), "{v}");
    assert!(has("added", &id_c), "{v}");
    assert!(v["removed"].as_array().unwrap().is_empty(), "{v}");
}

#[test]
fn reading_commands() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "Paper", "--pages", "10"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mid = env.ok(&["list"])[0]["materials"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(env.ok(&["progress", &id])["status"], "Unread");

    let e = env.ok(&["position", &id, &mid, "3", "0.5", "--zoom", "1.5"]);
    assert_eq!(e["type"], "position");
    assert_eq!(e["data"]["page"], 3);
    // 完全相同：沒有寫
    assert_eq!(
        env.ok(&["position", &id, &mid, "3", "0.5", "--zoom", "1.5"]),
        serde_json::json!({"written": false})
    );
    let pr = env.ok(&["progress", &id]);
    assert_eq!(pr["status"], "Reading");
    assert_eq!(pr["position"]["page"], 3);
    assert_eq!(env.ok(&["continue"]), serde_json::json!([id.clone()]));
    assert_eq!(env.ok(&["continue", "0"]), serde_json::json!([]));
    assert_eq!(env.ok(&["list"])[0]["progress"]["status"], "Reading");

    assert_eq!(env.ok(&["set-read", &id, "true"])["data"]["read"], true);
    assert_eq!(env.ok(&["progress", &id])["status"], "Read");
    assert_eq!(env.ok(&["continue"]), serde_json::json!([]));
    env.ok(&["set-read", &id, "false"]);
    assert_eq!(env.ok(&["progress", &id])["status"], "Reading");

    // 輸入不合法
    let (v, code) = env.run(&["position", &id, &mid, "0", "0.5"]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
    let (v, code) = env.run(&["set-read", &id, "maybe"]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
    let (v, code) = env.run(&["open-paper", "01ZZZZZZZZZZZZZZZZZZZZZZZZ", &mid]);
    assert_eq!((code, v["error"].as_str()), (1, Some("NotFound")));

    // 回收區
    assert_eq!(env.ok(&["trash-list"]), serde_json::json!([]));
    env.ok(&["remove", &id]);
    let t = env.ok(&["trash-list"]);
    assert_eq!(t.as_array().unwrap().len(), 1);
    assert_eq!(t[0]["paper_id"], id.as_str());
    assert_eq!(t[0]["name"], "Paper");
    assert_eq!(t[0]["bib"]["title"], "Paper");
    env.ok(&["restore", &id]);
    assert_eq!(env.ok(&["progress", &id])["status"], "Reading");
}

// ---- 全局功能----

fn add_titled(env: &Env, name: &str, seed: u8, title: &str) -> (String, String) {
    let pdf = env.pdf(name, seed, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", title, "--pages", "10"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = env.ok(&["list"]);
    let mid = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.as_str())
        .unwrap()["materials"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    (id, mid)
}

#[test]
fn set_pages_command() {
    let env = Env::new();
    let pdf = env.pdf("a.pdf", 1, 3000);
    let id = env.ok(&["add", s(&pdf), "--title", "P"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mid = env.ok(&["list"])[0]["materials"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        env.ok(&["set-pages", &id, &mid, "12"]),
        serde_json::json!({"written": true})
    );
    assert_eq!(
        env.ok(&["set-pages", &id, &mid, "12"]),
        serde_json::json!({"written": false})
    );
    assert_eq!(env.ok(&["list"])[0]["materials"][0]["pages"], 12);
    assert!(env.ok(&["list"])[0]["added"].is_string());
}

#[test]
fn groups_flow() {
    let env = Env::new();
    let (id, _) = add_titled(&env, "a.pdf", 1, "Alpha");
    let g = env.ok(&["group-create", "Vision"]);
    let gid = g["group"]["id"].as_str().unwrap().to_string();
    assert_eq!(g["group"]["name"], "Vision");
    assert!(g["token"].is_string());
    let v = env.ok(&["group-rename", &gid, "Vision 2"]);
    assert!(v["token"].is_string());
    env.ok(&["group-add", &id, &gid]);
    assert_eq!(
        env.ok(&["list"])[0]["groups"],
        serde_json::json!([gid.clone()])
    );
    let groups = env.ok(&["groups"]);
    assert_eq!(groups["groups"][0]["name"], "Vision 2");
    assert!(groups["token"].is_string());
    env.ok(&["group-remove", &id, &gid]);
    assert_eq!(env.ok(&["list"])[0]["groups"], serde_json::json!([]));
    env.ok(&["group-add", &id, &gid]);
    let del = env.ok(&["group-delete", &gid]);
    assert_eq!(del["skipped"], serde_json::json!([]));
    assert!(del["token"].is_string());
    assert_eq!(env.ok(&["list"])[0]["groups"], serde_json::json!([]));
    assert_eq!(env.ok(&["groups"])["groups"], serde_json::json!([]));
    // 舊的記號：Conflict
    let (v, code) = env.run(&["group-create", "X", "--token", &"0".repeat(64)]);
    assert_eq!((code, v["error"].as_str()), (1, Some("Conflict")));
}

#[test]
fn translation_flow() {
    let env = Env::new();
    let (id, _) = add_titled(&env, "a.pdf", 1, "Alpha");
    let t = env.pdf("t.pdf", 9, 3500);
    let v = env.ok(&["add-translation", &id, s(&t), "--pages", "11"]);
    assert_eq!(v["outcome"], "added");
    let mid = v["material_id"].as_str().unwrap().to_string();
    let list = env.ok(&["list"]);
    let mats = list[0]["materials"].as_array().unwrap();
    assert_eq!(mats.len(), 2);
    assert_eq!(mats[1]["role"], "translation");
    assert_eq!(mats[1]["id"], mid.as_str());
    let v2 = env.ok(&["add-translation", &id, s(&t)]);
    assert_eq!(v2["outcome"], "already_present");
    assert_eq!(v2["material_id"], mid.as_str());
    let bad = env.pdf_dir.join("not.txt");
    std::fs::write(&bad, b"plain text").unwrap();
    let (v, code) = env.run(&["add-translation", &id, s(&bad)]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
}

/// 移除一項材料、回收區的材料、永久刪除一項、清空回收區。
#[test]
fn remove_material_and_purge() {
    let env = Env::new();
    let (id, source) = add_titled(&env, "a.pdf", 1, "Alpha");
    let t = env.pdf("t.pdf", 9, 3100);
    let tid = env.ok(&["add-translation", &id, s(&t)])["material_id"]
        .as_str()
        .unwrap()
        .to_string();
    // 原文不能這樣移除
    let (v, code) = env.run(&["remove-material", &id, &source]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
    let v = env.ok(&["remove-material", &id, &tid]);
    assert_eq!(v["token"].as_str().unwrap().len(), 64);
    let removed = env.ok(&["trash-materials"]);
    assert_eq!(removed.as_array().unwrap().len(), 1);
    assert_eq!(removed[0]["paper_id"], id.as_str());
    assert_eq!(removed[0]["material"]["id"], tid.as_str());
    let folder = removed[0]["folder"].as_str().unwrap().to_string();
    // 放回、再移除一次
    let v = env.ok(&["restore-material", &folder]);
    assert_eq!(v["paper"], id.as_str());
    assert_eq!(env.ok(&["trash-materials"]), serde_json::json!([]));
    env.ok(&["remove-material", &id, &tid]);
    let folder = env.ok(&["trash-materials"])[0]["folder"]
        .as_str()
        .unwrap()
        .to_string();
    env.ok(&["purge", &folder]);
    assert_eq!(env.ok(&["trash-materials"]), serde_json::json!([]));
    let (v, code) = env.run(&["purge", &folder]);
    assert_eq!((code, v["error"].as_str()), (1, Some("NotFound")));
    // 清空：一篇文獻
    env.ok(&["remove", &id]);
    let r = env.ok(&["empty-trash"]);
    assert_eq!(r, serde_json::json!({"papers": 1, "materials": 0}));
    assert_eq!(env.ok(&["trash-list"]), serde_json::json!([]));
}

#[test]
fn notes_flow() {
    let env = Env::new();
    let (id, mid) = add_titled(&env, "a.pdf", 1, "Alpha");
    let empty = env.ok(&["notes", &id]);
    assert_eq!(empty["token"], serde_json::Value::Null);
    assert_eq!(empty["path"], serde_json::Value::Null);

    let h = env.ok(&[
        "highlight-add",
        &id,
        &mid,
        "--page",
        "0",
        "--rect",
        "10,20,30,40",
        "--quote",
        "a quote",
        "--comment",
        "first",
    ]);
    let hid = h["highlight"]["id"].as_str().unwrap().to_string();
    assert!(h["token"].is_string());
    let n = env.ok(&["notes", &id]);
    assert_eq!(n["highlights"][0]["id"], hid.as_str());
    assert_eq!(n["highlights"][0]["comment"], "first");
    assert!(n["token"].is_string());
    assert!(n["path"].as_str().unwrap().ends_with("Alpha.md"));

    env.ok(&["highlight-comment", &id, &hid, "changed"]);
    assert_eq!(
        env.ok(&["notes", &id])["highlights"][0]["comment"],
        "changed"
    );
    env.ok(&["thoughts", &id, "整篇的想法"]);
    assert_eq!(env.ok(&["notes", &id])["thoughts"], "整篇的想法");
    // 筆記檔已存在時 `--token none` 是 Conflict
    let (v, code) = env.run(&["thoughts", &id, "x", "--token", "none"]);
    assert_eq!((code, v["error"].as_str()), (1, Some("Conflict")));
    env.ok(&["highlight-delete", &id, &hid]);
    assert_eq!(env.ok(&["notes", &id])["highlights"], serde_json::json!([]));
}

#[test]
fn ink_flow() {
    let env = Env::new();
    let (id, mid) = add_titled(&env, "a.pdf", 1, "Alpha");
    let empty = env.ok(&["ink", &id]);
    assert_eq!(empty["token"], serde_json::Value::Null);
    assert_eq!(empty["strokes"], serde_json::json!([]));
    let a = env.ok(&[
        "stroke-add",
        &id,
        &mid,
        "--page",
        "0",
        "--points",
        "1,2;3.5,4;5,6",
    ]);
    let sid = a["stroke"]["id"].as_str().unwrap().to_string();
    env.ok(&[
        "stroke-add",
        &id,
        &mid,
        "--page",
        "1",
        "--points",
        "1,2",
        "--color",
        "#1a56db",
        "--width",
        "4",
        "--opacity",
        "0.45",
    ]);
    let ink = env.ok(&["ink", &id]);
    assert_eq!(ink["strokes"].as_array().unwrap().len(), 2);
    assert_eq!(
        ink["strokes"][0]["points"][1],
        serde_json::json!([3.5, 4.0])
    );
    assert_eq!(ink["strokes"][1]["opacity"], 0.45);
    env.ok(&["stroke-delete", &id, &sid]);
    assert_eq!(
        env.ok(&["ink", &id])["strokes"].as_array().unwrap().len(),
        1
    );
    env.ok(&["ink-clear-page", &id, &mid, "--page", "1"]);
    assert_eq!(
        env.ok(&["ink", &id])["strokes"].as_array().unwrap().len(),
        0
    );
    let (v, code) = env.run(&["stroke-add", &id, &mid, "--page", "0", "--points", "1;2"]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
}

#[test]
fn rect_flag_repeats() {
    let env = Env::new();
    let (id, mid) = add_titled(&env, "a.pdf", 1, "Alpha");
    let h = env.ok(&[
        "highlight-add",
        &id,
        &mid,
        "--page",
        "1",
        "--rect",
        "1,2,3,4",
        "--rect",
        "5,6,7,8",
        "--rect",
        "9,10,11,12",
        "--quote",
        "q",
        "--label",
        "ii",
    ]);
    let rects = h["highlight"]["rects"].as_array().unwrap();
    assert_eq!(rects.len(), 3);
    assert_eq!(rects[0], serde_json::json!([1.0, 2.0, 3.0, 4.0]));
    assert_eq!(rects[2], serde_json::json!([9.0, 10.0, 11.0, 12.0]));
    assert_eq!(h["highlight"]["page"]["label"], "ii");
    let (v, code) = env.run(&[
        "highlight-add",
        &id,
        &mid,
        "--page",
        "1",
        "--rect",
        "1,2,3",
        "--quote",
        "q",
    ]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
}

#[test]
fn export_then_restore_backup() {
    let env = Env::new();
    let (id, mid) = add_titled(&env, "a.pdf", 1, "Alpha");
    add_titled(&env, "b.pdf", 2, "Beta");
    let g = env.ok(&["group-create", "G"])["group"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    env.ok(&["group-add", &id, &g]);
    env.ok(&[
        "highlight-add",
        &id,
        &mid,
        "--page",
        "0",
        "--rect",
        "1,2,3,4",
        "--quote",
        "q",
    ]);
    env.ok(&["open-paper", &id, &mid]);

    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("backup.zip");
    let report = env.ok(&["export-backup", s(&out)]);
    assert!(report["files"].as_u64().unwrap() > 0);
    assert_eq!(report["skipped"], serde_json::json!([]));
    assert!(out.is_file());

    let env2 = Env::new();
    assert!(
        !env2.root.exists(),
        "the target must not exist before the restore"
    );
    let r = env2.ok(&["restore-backup", s(&out)]);
    assert_eq!(r["manifest_mismatches"], serde_json::json!([]));
    assert_eq!(r["problems"], serde_json::json!([]));
    assert_eq!(r["library_id"], env.ok(&["init"])["library_id"]);
    assert_eq!(env.ok(&["list"]), env2.ok(&["list"]));
    assert_eq!(
        env.ok(&["groups"])["groups"],
        env2.ok(&["groups"])["groups"]
    );
    assert_eq!(env.ok(&["notes", &id]), env2.ok(&["notes", &id]));
    assert_eq!(env.ok(&["check", "full"]), env2.ok(&["check", "full"]));

    // 目標不是空的：拒絕，原本的內容不變
    let before = env2.ok(&["list"]);
    let (v, code) = env2.run(&["restore-backup", s(&out)]);
    assert_eq!((code, v["error"].as_str()), (1, Some("InvalidInput")));
    assert_eq!(env2.ok(&["list"]), before);
}

#[test]
fn restore_backup_does_not_open_target_first() {
    let env = Env::new();
    add_titled(&env, "a.pdf", 1, "Alpha");
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("backup.zip");
    env.ok(&["export-backup", s(&out)]);
    let env2 = Env::new();
    // 對不存在的目標 restore-backup 成功，而不是因為「開啟時先建出 library.json、目標不是空的」而失敗
    let r = env2.ok(&["restore-backup", s(&out)]);
    assert!(r["files"].as_u64().unwrap() > 0);
    assert_eq!(env2.ok(&["list"]).as_array().unwrap().len(), 1);
}

// ---- 同步包----

impl Env {
    /// 帶環境變數執行。
    fn run_env(&self, args: &[&str], envs: &[(&str, &str)]) -> (Value, i32) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_prcli"));
        cmd.arg(&self.root).arg(&self.device).args(args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.trim_end().lines().count(), 1, "stdout: {text:?}");
        (
            serde_json::from_str(text.trim_end()).unwrap(),
            out.status.code().unwrap(),
        )
    }

    fn err(&self, args: &[&str]) -> Value {
        let (v, code) = self.run(args);
        assert_eq!(code, 1, "{args:?} -> {v}");
        v
    }
}

/// A 有兩篇；B 從 A 的完整同步包加入。交回（A, B, 放同步包的資料夾）。
fn joined_pair() -> (Env, Env, tempfile::TempDir) {
    let a = Env::new();
    add_titled(&a, "a.pdf", 1, "Alpha");
    add_titled(&a, "b.pdf", 2, "Beta");
    let out = tempfile::tempdir().unwrap();
    let full = out.path().join("full.zip");
    let r = a.ok(&["sync-export", s(&full), "--full", "--label", "Phone A"]);
    assert_eq!(r["mode"], "full");
    let b = Env::new();
    let j = b.ok(&["sync-join", s(&full)]);
    assert!(j["files"].as_u64().unwrap() > 0);
    (a, b, out)
}

fn titles(env: &Env) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = env
        .ok(&["list"])
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["bib"]["title"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn sync_export_import_flow() {
    let (a, b, out) = joined_pair();
    assert_eq!(titles(&a), titles(&b));
    let (alpha, _) = titles(&a)[0].clone();
    a.ok(&["set-title", &alpha, "Alpha on A"]);
    add_titled(&b, "c.pdf", 3, "Gamma on B");
    let pa = out.path().join("a1.zip");
    a.ok(&["sync-export", s(&pa)]);
    b.ok(&["sync-import", s(&pa), "--defaults"]);
    let pb = out.path().join("b1.zip");
    b.ok(&["sync-export", s(&pb)]);
    let r = a.ok(&["sync-import", s(&pb)]);
    assert_eq!(r["added"][0]["kind"], "Paper");
    assert_eq!(titles(&a), titles(&b));
    assert!(titles(&a).iter().any(|(_, t)| t == "Alpha on A"));
}

#[test]
fn sync_import_dry_run_writes_nothing() {
    let (a, b, out) = joined_pair();
    add_titled(&a, "c.pdf", 3, "New on A");
    let pa = out.path().join("a1.zip");
    a.ok(&["sync-export", s(&pa)]);
    let before = titles(&b);
    let v = b.ok(&["sync-import", s(&pa), "--dry-run"]);
    assert_eq!(v["added"][0]["kind"], "Paper");
    assert_eq!(titles(&b), before);
    assert!(
        !std::fs::read_dir(&b.root)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".pr-sync-"))
    );
}

/// 兩邊各改同一篇的想法：一個 `Thoughts` 衝突。
fn thoughts_conflict(a: &Env, b: &Env, out: &Path) -> (String, PathBuf) {
    let (alpha, _) = titles(a)[0].clone();
    a.ok(&["thoughts", &alpha, "A thinks"]);
    b.ok(&["thoughts", &alpha, "B thinks"]);
    let pa = out.join(format!("a-{}.zip", std::process::id()));
    a.ok(&["sync-export", s(&pa)]);
    (alpha, pa)
}

#[test]
fn sync_import_requires_choices() {
    let (a, b, out) = joined_pair();
    let (alpha, pa) = thoughts_conflict(&a, &b, out.path());
    let v = b.err(&["sync-import", s(&pa)]);
    assert_eq!(v["error"], "InvalidInput");
    assert!(v["detail"].as_str().unwrap().contains("Thoughts"), "{v}");
    let n = b.ok(&["notes", &alpha]);
    assert_eq!(n["thoughts"], "B thinks");
}

#[test]
fn sync_import_choose_and_defaults() {
    let (a, b, out) = joined_pair();
    let (alpha, pa) = thoughts_conflict(&a, &b, out.path());
    let r = b.ok(&["sync-import", s(&pa), "--choose", "c1=incoming"]);
    assert!(r["conflict_copies"].as_array().unwrap().is_empty());
    assert_eq!(b.ok(&["notes", &alpha])["thoughts"], "A thinks");
    // 再來一次衝突，用預設（both：留本機的、另存副本）
    a.ok(&["thoughts", &alpha, "A again"]);
    b.ok(&["thoughts", &alpha, "B again"]);
    let pa2 = out.path().join("a2.zip");
    a.ok(&["sync-export", s(&pa2)]);
    let r = b.ok(&["sync-import", s(&pa2), "--defaults"]);
    assert_eq!(r["conflict_copies"].as_array().unwrap().len(), 1);
    assert_eq!(b.ok(&["notes", &alpha])["thoughts"], "B again");
}

#[test]
fn sync_join_does_not_open_target_first() {
    let (a, b, _out) = joined_pair();
    assert_eq!(titles(&a), titles(&b));
    assert_eq!(a.ok(&["init"])["library_id"], b.ok(&["init"])["library_id"]);
}

#[test]
fn changes_command_filters() {
    let env = Env::new();
    let (p1, _) = add_titled(&env, "a.pdf", 1, "One");
    let (p2, _) = add_titled(&env, "b.pdf", 2, "Two");
    env.ok(&["remove", &p1]);
    env.ok(&["remove", &p2]);
    env.ok(&["restore", &p2]);
    let all = env.ok(&["changes"]);
    assert_eq!(all["changes"].as_array().unwrap().len(), 3);
    let one = env.ok(&["changes", "--paper", &p2]);
    assert_eq!(one["changes"].as_array().unwrap().len(), 2);
    let restored = env.ok(&["changes", "--type", "paper-restored"]);
    assert_eq!(restored["changes"].as_array().unwrap().len(), 1);
    assert_eq!(restored["changes"][0]["paper"], p2.as_str());
}

#[test]
fn sync_folder_flow() {
    let (a, b, out) = joined_pair();
    add_titled(&a, "c.pdf", 3, "Via folder");
    let shared = out.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let r = a.ok(&["sync-folder-put", s(&shared)]);
    let name = r["name"].as_str().unwrap().to_string();
    assert!(name.starts_with("paperreader-sync-"));
    let list = b.ok(&["sync-folder-list", s(&shared)]);
    assert_eq!(list["packages"][0]["name"], name.as_str());
    let got = out.path().join("got.zip");
    b.ok(&["sync-folder-fetch", s(&shared), &name, s(&got)]);
    b.ok(&["sync-import", s(&got), "--defaults"]);
    assert!(titles(&b).iter().any(|(_, t)| t == "Via folder"));
}

/// 資料夾裡每個檔的相對路徑與內容（`import` 的 `--dry-run` 前後比對用）。
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                out.push((p.strip_prefix(base).unwrap().display().to_string(), vec![]));
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().display().to_string();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// 整批匯入：`--dry-run` 不寫任何檔、輸出一份 JSON；不帶就一路寫完；重跑同一份清單全部略過。
#[test]
fn import_dry_run_writes_nothing_then_imports() {
    let env = Env::new();
    env.ok(&["init"]);
    let src = env.pdf_dir.join("batch");
    std::fs::create_dir_all(src.join("L001")).unwrap();
    std::fs::rename(env.pdf("a.pdf", 1, 3000), src.join("L001/a.pdf")).unwrap();
    std::fs::rename(env.pdf("b.pdf", 2, 3000), src.join("b.pdf")).unwrap();
    std::fs::write(src.join("L001/t.md"), "# 譯本\n").unwrap();
    let manifest = serde_json::json!({
        "format": 1,
        "kind": "paperreader-import",
        "papers": [
            {"csl": {"title": "Alpha", "author": [{"family": "Zhao"}], "issued": {"date-parts": [[2024]]}},
             "files": [{"path": "L001/a.pdf", "role": "source", "form": "pdf"},
                       {"path": "L001/t.md", "role": "translation", "form": "markdown"}]},
            {"files": [{"path": "b.pdf", "role": "source", "form": "pdf"}]},
            {"files": [{"path": "gone.pdf", "role": "source", "form": "pdf"}]}
        ]
    });
    std::fs::write(
        src.join("paperreader-import.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let before = tree(&env.root);
    let v = env.ok(&["import", s(&src), "--dry-run"]);
    assert_eq!(tree(&env.root), before);
    assert!(v.get("report").is_none());
    assert_eq!(v["preview"]["total"], 3);
    assert_eq!(v["preview"]["accept"].as_array().unwrap().len(), 2);
    assert_eq!(v["preview"]["skip"][0]["reason"], "missing-file");
    assert_eq!(v["preview"]["skip"][0]["detail"], "gone.pdf");

    let v = env.ok(&["import", s(&src)]);
    assert_eq!(v["report"]["added"].as_array().unwrap().len(), 2);
    assert_eq!(v["report"]["skipped"].as_array().unwrap().len(), 1);
    let list = env.ok(&["list"]);
    let folders: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["folder"].as_str().unwrap())
        .collect();
    assert_eq!(folders, vec!["Zhao 2024 - Alpha", "b"]);

    let v = env.ok(&["import", s(&src)]);
    assert!(v["report"]["added"].as_array().unwrap().is_empty());
    assert_eq!(v["preview"]["skip"][0]["reason"], "already-present");
    assert_eq!(v["report"]["skipped"].as_array().unwrap().len(), 3);
}

#[test]
fn writes_refused_while_unfinished() {
    let (a, b, out) = joined_pair();
    let (alpha, _) = titles(&a)[0].clone();
    a.ok(&["set-title", &alpha, "From A"]);
    let pa = out.path().join("a1.zip");
    a.ok(&["sync-export", s(&pa)]);
    let (v, code) = b.run_env(&["sync-import", s(&pa)], &[("PRCLI_SYNC_STOP_AT", "2")]);
    assert_eq!(code, 1, "{v}");
    let (v, code) = b.run_env(&["set-title", &alpha, "x"], &[("PRCLI_SKIP_RESUME", "1")]);
    assert_eq!(code, 1);
    assert_eq!(v["error"], "ReadOnly");
    let r = b.ok(&["sync-resume"]);
    assert!(r["package"].as_str().is_some());
    b.ok(&["set-title", &alpha, "writable again"]);
    // 沒有寫到一半的合併時 NotFound
    assert_eq!(b.err(&["sync-resume"])["error"], "NotFound");
}
