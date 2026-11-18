//! 命令列工具：包住 paperreader-core，供在模擬器上驗證用。
//!
//! 用法：`prcli <根目錄> <裝置代號檔> <指令> [參數]`。結果以一行 JSON 寫到標準輸出；
//! 失敗時寫 `{"error":"<錯誤項目名>","detail":"…"}`，結束碼 1。每次執行都重新開啟文獻庫。

use paperreader_core::error::{LibError, Result};
use paperreader_core::format::{Author, Bib, BibDraft};
use paperreader_core::ids::new_id;
use paperreader_core::import::{ImportContainer, ImportStep, scan_import};
use paperreader_core::jsonio::VersionToken;
use paperreader_core::local::LocalLibrary;
use paperreader_core::notes::Quote;
use paperreader_core::store::{
    AddMaterialOutcome, AddOutcome, ChangeFilter, CheckLevel, ConflictChoice, EventFilter,
    HighlightDraft, StrokeDraft, SyncMode,
};
use paperreader_core::sync::transport::{FolderTransport, SyncTransport};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(v) => println!("{v}"),
        Err(e) => {
            println!(
                "{}",
                json!({"error": e.kind_name(), "detail": e.to_string()})
            );
            std::process::exit(1);
        }
    }
}

fn invalid(reason: impl Into<String>) -> LibError {
    LibError::InvalidInput {
        reason: reason.into(),
    }
}

/// 位置參數與 `--名稱 值` 的旗標。同名的旗標可以重複（例如 `--rect`）：`flag` 取最後一個，
/// `flag_all` 依出現的順序取全部。
struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

/// 不帶值的旗標。
const BOOL_FLAGS: [&str; 3] = ["full", "dry-run", "defaults"];

fn parse(rest: &[String]) -> Result<Args> {
    let mut pos = Vec::new();
    let mut flags: HashMap<String, Vec<String>> = HashMap::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            // 不帶值的旗標
            if BOOL_FLAGS.contains(&name) {
                flags
                    .entry(name.to_string())
                    .or_default()
                    .push("true".to_string());
                continue;
            }
            let v = it
                .next()
                .ok_or_else(|| invalid(format!("--{name} needs a value")))?;
            flags.entry(name.to_string()).or_default().push(v.clone());
        } else {
            pos.push(a.clone());
        }
    }
    Ok(Args { pos, flags })
}

impl Args {
    fn flag_all(&self, name: &str) -> &[String] {
        self.flags.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
    /// 版本記號旗標：沒給是 `None`（由指令自己先讀）；`none` 是「我認為檔案不存在」。
    fn token_flag(&self) -> Option<Option<VersionToken>> {
        self.flag("token").map(|t| {
            if t == "none" {
                None
            } else {
                Some(VersionToken(t.to_string()))
            }
        })
    }
    fn pos(&self, i: usize, what: &str) -> Result<&str> {
        self.pos
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| invalid(format!("missing {what}")))
    }
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|v| v.last())
            .map(String::as_str)
    }
    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>> {
        self.flag(name)
            .map(|v| {
                v.parse()
                    .map_err(|_| invalid(format!("--{name} is not a number: {v}")))
            })
            .transpose()
    }
}

/// 裝置代號檔不存在（或是空的）就產生一個並寫入。
fn device_id(path: &Path) -> Result<String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let t = text.trim();
        if !t.is_empty() {
            // 代號會變成紀錄檔的檔名，內容不合法就報錯，不拿來組檔名
            if !paperreader_core::ids::is_valid_id(t) {
                return Err(invalid(format!(
                    "device file does not hold a valid device id: {}",
                    path.display()
                )));
            }
            return Ok(t.to_string());
        }
    }
    let id = new_id();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, format!("{id}\n"))
        .map_err(|e| paperreader_core::error::io_error(path, e))?;
    Ok(id)
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| invalid(e.to_string()))
}

fn draft_from(bib: &Bib) -> BibDraft {
    BibDraft {
        title: bib.title.clone(),
        authors: bib.authors.clone(),
        year: bib.year,
        venue: bib.venue.clone(),
        doi: bib.doi.clone(),
        url: bib.url.clone(),
    }
}

fn run(argv: &[String]) -> Result<Value> {
    if argv.len() < 3 {
        return Err(invalid(
            "usage: prcli <root> <device-file> <command> [args]",
        ));
    }
    let root = Path::new(&argv[0]);
    let device = device_id(Path::new(&argv[1]))?;
    let command = argv[2].as_str();
    let args = parse(&argv[3..])?;

    // 從備份檔還原：`<根目錄>` 就是還原的目標，必須在 `LocalLibrary::open` 之前處理，
    // 否則開啟會先建出 `library.json`，目標就不是空的了。
    if command == "restore-backup" {
        let archive = args.pos(0, "backup file")?;
        let report = paperreader_core::backup::restore_backup(Path::new(archive), root, &device)?;
        return to_value(&report);
    }

    // 從同步包加入：同上，`<根目錄>` 是加入的目標
    if command == "sync-join" {
        let pkg = args.pos(0, "sync package")?;
        let report =
            paperreader_core::sync::join::join_from_sync_package(Path::new(pkg), root, &device)?;
        return to_value(&report);
    }

    // 測試用：`PRCLI_SKIP_RESUME=1` 時開啟不把寫到一半的合併做完（文獻庫唯讀）。
    // 這兩個環境變數只在除錯版（`cargo test` 用的）有效，模擬器用的發行版不理它們
    let test_hooks = cfg!(debug_assertions);
    let mut lib = if test_hooks && std::env::var_os("PRCLI_SKIP_RESUME").is_some() {
        LocalLibrary::open_without_resume(root, &device)?
    } else {
        LocalLibrary::open(root, &device)?
    };
    // 測試用：`PRCLI_SYNC_STOP_AT=k` 讓匯入在計畫的第 k 項之前停下（0 是寫計畫檔之前）
    if test_hooks
        && let Some(k) = std::env::var("PRCLI_SYNC_STOP_AT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
    {
        lib.set_sync_apply_hook(Some(Box::new(move |i| i != k)));
    }
    let library_id = lib.library_file().id.clone();
    let store: &mut LocalLibrary = &mut lib;

    match command {
        "init" => Ok(json!({"read_only": store.is_read_only(), "library_id": library_id})),
        "add" => {
            let file = args.pos(0, "file")?;
            let mut authors = Vec::new();
            if let Some(f) = args.flag("family") {
                authors.push(Author {
                    family: Some(f.into()),
                    ..Default::default()
                });
            }
            if let Some(l) = args.flag("literal") {
                authors.push(Author {
                    literal: Some(l.into()),
                    ..Default::default()
                });
            }
            let draft = BibDraft {
                title: args.flag("title").map(String::from),
                authors,
                year: args.number("year")?,
                venue: None,
                doi: args.flag("doi").map(String::from),
                url: None,
            };
            let pages = args.number("pages")?;
            let (name, id) = match store.add_paper(Path::new(file), draft, pages)? {
                AddOutcome::Added(id) => ("added", id),
                AddOutcome::AlreadyPresent(id) => ("already_present", id),
                AddOutcome::InTrash(id) => ("in_trash", id),
            };
            Ok(json!({"outcome": name, "id": id}))
        }
        "list" => to_value(&store.list_papers()),
        "get" => {
            let (paper, token) = store.get_paper(args.pos(0, "paper id")?)?;
            Ok(json!({"paper": to_value(&paper)?, "token": token.0}))
        }
        "set-title" => {
            let id = args.pos(0, "paper id")?;
            let title = args.pos(1, "title")?;
            let (paper, current) = store.get_paper(id)?;
            let token = match args.flag("token") {
                Some(t) => VersionToken(t.to_string()),
                None => current,
            };
            let mut draft = draft_from(&paper.bib);
            draft.title = Some(title.to_string());
            let new = store.update_bib(id, draft, &token)?;
            Ok(json!({"token": new.0}))
        }
        "stress-edit" => {
            let id = args.pos(0, "paper id")?;
            let n: u32 = args
                .pos(1, "count")?
                .parse()
                .map_err(|_| invalid("count is not a number"))?;
            let (paper, mut token) = store.get_paper(id)?;
            let mut draft = draft_from(&paper.bib);
            for i in 1..=n {
                draft.title = Some(format!("edit-{i}"));
                token = store.update_bib(id, draft.clone(), &token)?;
            }
            Ok(json!({"ok": n}))
        }
        "remove" => {
            store.remove_paper(args.pos(0, "paper id")?)?;
            Ok(json!({"ok": true}))
        }
        "restore" => {
            store.restore_paper(args.pos(0, "paper id")?)?;
            Ok(json!({"ok": true}))
        }
        // 閱讀事件只能經由下面三個操作寫入（核心會檢查欄位）
        "open-paper" => {
            let e = store.record_open(args.pos(0, "paper id")?, args.pos(1, "material id")?)?;
            to_value(&e)
        }
        // position <文獻代號> <材料代號> <頁碼> <頁內位置> [--zoom 縮放]；與上一筆相同時輸出 {"written":false}
        "position" => {
            let page: i64 = args
                .pos(2, "page")?
                .parse()
                .map_err(|_| invalid("page is not an integer"))?;
            let offset: f64 = args
                .pos(3, "offset")?
                .parse()
                .map_err(|_| invalid("offset is not a number"))?;
            let zoom: Option<f64> = args.number("zoom")?;
            match store.record_position(
                args.pos(0, "paper id")?,
                args.pos(1, "material id")?,
                page,
                offset,
                zoom,
            )? {
                Some(e) => to_value(&e),
                None => Ok(json!({"written": false})),
            }
        }
        "set-read" => {
            let read = match args.pos(1, "true|false")? {
                "true" => true,
                "false" => false,
                other => return Err(invalid(format!("expected true or false, got {other}"))),
            };
            to_value(&store.set_read(args.pos(0, "paper id")?, read)?)
        }
        "progress" => to_value(&store.reading_progress(args.pos(0, "paper id")?)?),
        // continue [N]：繼續閱讀的文獻代號，預設最多 10 篇
        "continue" => {
            let limit: usize = match args.pos.first() {
                Some(n) => n.parse().map_err(|_| invalid("N is not a number"))?,
                None => 10,
            };
            to_value(&store.continue_reading(limit))
        }
        "trash-list" => to_value(&store.list_trash()?),
        // remove-material <文獻代號> <材料代號>：原文以外的一項材料移到回收區
        "remove-material" => {
            let id = args.pos(0, "paper id")?;
            let (_, token) = store.get_paper(id)?;
            let t = store.remove_material(id, args.pos(1, "material id")?, &token)?;
            Ok(json!({"token": t.0}))
        }
        "trash-materials" => to_value(&store.list_removed_materials()?),
        // restore-material <回收區裡的資料夾名稱>：把移除的材料放回那篇
        "restore-material" => {
            let paper = store.restore_material(args.pos(0, "trash folder")?)?;
            Ok(json!({"paper": paper}))
        }
        // purge <回收區裡的資料夾名稱>：永久刪除一篇或一項移除的材料
        "purge" => {
            store.purge_trash_entry(args.pos(0, "trash folder")?)?;
            Ok(json!({"ok": true}))
        }
        "empty-trash" => to_value(&store.empty_trash()?),
        "events" => {
            let filter = EventFilter {
                paper: args.flag("paper").map(String::from),
                ..Default::default()
            };
            to_value(&store.read_events(&filter)?)
        }
        // 開啟（已掃描一次）、等指定的毫秒數、呼叫 rescan()，印出與開啟時相比的差異。
        // 等待期間由別的程序（例如 adb shell）改動文獻庫，用來驗證 rescan 在真正的裝置檔案系統上看得到。
        // rescan-after <毫秒|stdin>：開好文獻庫之後等一下再重新掃描（等待期間別處改了文獻庫）。
        // `stdin`：先在標準錯誤寫一行 `ready`（文獻庫已經開好），再等標準輸入的一行；測試用它照順序做，不靠時間
        "rescan-after" => {
            let wait = args.pos(0, "milliseconds or stdin")?;
            if wait == "stdin" {
                eprintln!("ready");
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| invalid(format!("cannot read standard input: {e}")))?;
            } else {
                let ms: u64 = wait
                    .parse()
                    .map_err(|_| invalid("milliseconds is not a number"))?;
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            to_value(&store.rescan()?)
        }
        "check" => {
            let level = match args.pos(0, "quick|full")? {
                "quick" => CheckLevel::Quick,
                "full" => CheckLevel::Full,
                other => return Err(invalid(format!("unknown check level: {other}"))),
            };
            to_value(&store.check(level)?)
        }
        // set-pages <文獻> <材料> <頁數>
        "set-pages" => {
            let pages: u32 = args
                .pos(2, "pages")?
                .parse()
                .map_err(|_| invalid("pages is not a number"))?;
            let written = store.set_material_pages(
                args.pos(0, "paper id")?,
                args.pos(1, "material id")?,
                pages,
            )?;
            Ok(json!({"written": written}))
        }
        "groups" => {
            let (groups, token) = store.list_groups()?;
            Ok(json!({"groups": to_value(&groups)?, "token": token.0}))
        }
        // group-create <名稱> [--token T]（T 是 library.json 的記號）
        "group-create" => {
            let token = group_token(store, &args)?;
            let (g, token) = store.create_group(args.pos(0, "name")?, &token)?;
            Ok(json!({"group": to_value(&g)?, "token": token.0}))
        }
        "group-rename" => {
            let token = group_token(store, &args)?;
            let t = store.rename_group(args.pos(0, "group id")?, args.pos(1, "name")?, &token)?;
            Ok(json!({"token": t.0}))
        }
        "group-delete" => {
            let token = group_token(store, &args)?;
            let out = store.delete_group(args.pos(0, "group id")?, &token)?;
            Ok(json!({"token": out.token.0, "skipped": out.skipped}))
        }
        // group-add / group-remove <文獻> <群組> [--token T]（T 是那篇 paper.json 的記號）
        "group-add" | "group-remove" => {
            let paper = args.pos(0, "paper id")?;
            let group = args.pos(1, "group id")?;
            let token = paper_token(store, paper, &args)?;
            let t = if command == "group-add" {
                store.add_to_group(paper, group, &token)?
            } else {
                store.remove_from_group(paper, group, &token)?
            };
            Ok(json!({"token": t.0}))
        }
        // add-translation <文獻> <檔案> [--pages N] [--token T]
        "add-translation" => {
            let paper = args.pos(0, "paper id")?;
            let token = paper_token(store, paper, &args)?;
            let pages = args.number("pages")?;
            match store.add_translation(paper, Path::new(args.pos(1, "file")?), pages, &token)? {
                AddMaterialOutcome::Added { material_id, .. } => {
                    Ok(json!({"outcome": "added", "material_id": material_id}))
                }
                AddMaterialOutcome::AlreadyPresent { material_id } => {
                    Ok(json!({"outcome": "already_present", "material_id": material_id}))
                }
            }
        }
        // notes <文獻>：NotesDoc 加 "token"（null 表示沒有筆記檔）
        "notes" => {
            let (doc, token) = store.read_notes(args.pos(0, "paper id")?)?;
            let mut v = to_value(&doc)?;
            v["token"] = token.map_or(Value::Null, |t| Value::String(t.0));
            Ok(v)
        }
        // highlight-add <文獻> <材料> --page N（頁序，從 0 起算）--rect l,b,r,t [--rect …] --quote Q
        //   [--label L] [--comment C] [--token T|none]
        "highlight-add" => {
            let paper = args.pos(0, "paper id")?;
            let mut rects = Vec::new();
            for r in args.flag_all("rect") {
                rects.push(parse_rect(r)?);
            }
            let page_index: u32 = args
                .number("page")?
                .ok_or_else(|| invalid("missing --page"))?;
            let draft = HighlightDraft {
                material: args.pos(1, "material id")?.to_string(),
                page_index,
                page_label: args.flag("label").map(String::from),
                rects,
                next_page_rects: vec![],
                position: None,
                quote: Quote {
                    exact: args
                        .flag("quote")
                        .ok_or_else(|| invalid("missing --quote"))?
                        .to_string(),
                    prefix: None,
                    suffix: None,
                    extra: Default::default(),
                },
                fingerprint: None,
                comment: args.flag("comment").unwrap_or("").to_string(),
            };
            let token = notes_token(store, paper, &args)?;
            let (h, t) = store.add_highlight(paper, draft, token.as_ref())?;
            Ok(json!({"highlight": to_value(&h)?, "token": t.0}))
        }
        "highlight-comment" => {
            let paper = args.pos(0, "paper id")?;
            let token = notes_token(store, paper, &args)?
                .ok_or_else(|| invalid("this command needs an existing notes file"))?;
            let t = store.set_highlight_comment(
                paper,
                args.pos(1, "highlight id")?,
                args.pos(2, "comment")?,
                &token,
            )?;
            Ok(json!({"token": t.0}))
        }
        "highlight-delete" => {
            let paper = args.pos(0, "paper id")?;
            let token = notes_token(store, paper, &args)?
                .ok_or_else(|| invalid("this command needs an existing notes file"))?;
            let t = store.delete_highlight(paper, args.pos(1, "highlight id")?, &token)?;
            Ok(json!({"token": t.0}))
        }
        // thoughts <文獻> <文字> [--token T|none]
        "thoughts" => {
            let paper = args.pos(0, "paper id")?;
            let token = notes_token(store, paper, &args)?;
            let t = store.set_thoughts(paper, args.pos(1, "text")?, token.as_ref())?;
            Ok(json!({"token": t.0}))
        }
        // ink <文獻>：筆跡檔的內容加 "token"（null 表示沒有 ink.json）
        "ink" => {
            let (doc, token) = store.read_ink(args.pos(0, "paper id")?)?;
            let mut v = to_value(&doc)?;
            v["token"] = token.map_or(Value::Null, |t| Value::String(t.0));
            Ok(v)
        }
        // stroke-add <文獻> <材料> --page N（頁序，從 0 起算）--points x,y;x,y;… [--color #rrggbb]
        //   [--width W] [--opacity O] [--token T|none]
        "stroke-add" => {
            let paper = args.pos(0, "paper id")?;
            let mut points = Vec::new();
            for pair in args
                .flag("points")
                .ok_or_else(|| invalid("missing --points"))?
                .split(';')
            {
                let (x, y) = pair
                    .split_once(',')
                    .ok_or_else(|| invalid(format!("--points needs x,y;x,y: {pair}")))?;
                let num = |v: &str| {
                    v.trim()
                        .parse::<f64>()
                        .map_err(|_| invalid(format!("--points has a non-number: {pair}")))
                };
                points.push([num(x)?, num(y)?]);
            }
            let draft = StrokeDraft {
                material: args.pos(1, "material id")?.to_string(),
                page_index: args
                    .number("page")?
                    .ok_or_else(|| invalid("missing --page"))?,
                color: args.flag("color").unwrap_or("#d93025").to_string(),
                width: args.number("width")?.unwrap_or(1.5),
                opacity: args.number("opacity")?.unwrap_or(1.0),
                points,
            };
            let token = ink_token(store, paper, &args)?;
            let (st, t) = store.add_stroke(paper, draft, token.as_ref())?;
            Ok(json!({"stroke": to_value(&st)?, "token": t.0}))
        }
        // stroke-delete <文獻> <筆代號> [--token T]
        "stroke-delete" => {
            let paper = args.pos(0, "paper id")?;
            let token = ink_token(store, paper, &args)?
                .ok_or_else(|| invalid("this command needs an existing ink file"))?;
            let t = store.delete_stroke(paper, args.pos(1, "stroke id")?, &token)?;
            Ok(json!({"token": t.0}))
        }
        // ink-clear-page <文獻> <材料> --page N [--token T]
        "ink-clear-page" => {
            let paper = args.pos(0, "paper id")?;
            let token = ink_token(store, paper, &args)?
                .ok_or_else(|| invalid("this command needs an existing ink file"))?;
            let page: u32 = args
                .number("page")?
                .ok_or_else(|| invalid("missing --page"))?;
            let t = store.clear_ink_page(paper, args.pos(1, "material id")?, page, &token)?;
            Ok(json!({"token": t.0}))
        }
        // export-backup <輸出檔>
        "export-backup" => {
            let report =
                store.export_backup(Path::new(args.pos(0, "output file")?), &mut |_, _| {})?;
            to_value(&report)
        }
        // ---- 同步包----
        // changes [--paper P] [--type T]
        "changes" => {
            let filter = ChangeFilter {
                paper: args.flag("paper").map(String::from),
                kind: args.flag("type").map(String::from),
            };
            Ok(json!({"changes": to_value(&store.read_changes(&filter)?)?}))
        }
        // sync-export <輸出檔> [--full] [--label L]
        "sync-export" => {
            let mode = if args.flag("full").is_some() {
                SyncMode::Full
            } else {
                SyncMode::Changes
            };
            let report = store.export_sync_package(
                Path::new(args.pos(0, "output file")?),
                mode,
                args.flag("label"),
                &mut |_, _| {},
            )?;
            to_value(&report)
        }
        // sync-import <同步包> [--dry-run] [--choose c1=local|incoming|both …] [--defaults]
        // 準備與寫入在同一個行程裡（命令列每次都重新開啟文獻庫，開啟時會清掉沒有計畫檔的暫存）
        "sync-import" => {
            let preview = store
                .prepare_sync_import(Path::new(args.pos(0, "sync package")?), &mut |_, _| {})?;
            if args.flag("dry-run").is_some() {
                store.discard_sync_import(&preview.preview_id)?;
                return to_value(&preview);
            }
            let mut given: HashMap<String, ConflictChoice> = HashMap::new();
            for c in args.flag_all("choose") {
                let (id, choice) = c
                    .split_once('=')
                    .ok_or_else(|| invalid(format!("--choose needs id=choice: {c}")))?;
                let choice = match choice {
                    "local" => ConflictChoice::Local,
                    "incoming" => ConflictChoice::Incoming,
                    "both" => ConflictChoice::Both,
                    other => return Err(invalid(format!("unknown choice: {other}"))),
                };
                given.insert(id.to_string(), choice);
            }
            let defaults = args.flag("defaults").is_some();
            let mut choices = Vec::new();
            let mut missing = Vec::new();
            for c in &preview.conflicts {
                match given.remove(&c.id) {
                    Some(ch) => choices.push((c.id.clone(), ch)),
                    None if defaults => choices.push((c.id.clone(), c.default)),
                    None => missing.push(c.clone()),
                }
            }
            // 多給了不存在的衝突代號：交給核心回報
            choices.extend(given);
            if !missing.is_empty() {
                store.discard_sync_import(&preview.preview_id)?;
                return Err(invalid(format!(
                    "conflicts need a choice (--choose or --defaults): {}",
                    serde_json::to_string(&missing).unwrap_or_default()
                )));
            }
            to_value(&store.apply_sync_import(&preview.preview_id, &choices)?)
        }
        "sync-resume" => to_value(&store.resume_sync_import()?),
        // import <資料夾或zip> [--dry-run]：整批匯入。只印一份 JSON：`--dry-run` 是
        // `{"preview": …}`；否則一路寫完，印 `{"preview": …, "report": …}`
        "import" => {
            let given = Path::new(args.pos(0, "folder or zip")?);
            let container = if given.is_dir() {
                ImportContainer::Folder(given.to_path_buf())
            } else {
                ImportContainer::Zip(given.to_path_buf())
            };
            let scan = scan_import(&container, &mut |_, _| true)?
                .ok_or_else(|| invalid("scan cancelled"))?;
            let preview = store.prepare_import(scan)?;
            if args.flag("dry-run").is_some() {
                return Ok(json!({"preview": to_value(&preview)?}));
            }
            let report = loop {
                match store.import_step(&preview.preview_id)? {
                    ImportStep::Progress { .. } => {}
                    ImportStep::Finished(r) => break r,
                }
            };
            Ok(json!({"preview": to_value(&preview)?, "report": to_value(&report)?}))
        }
        // sync-folder-list <資料夾>
        "sync-folder-list" => {
            let mut t = FolderTransport::new(Path::new(args.pos(0, "folder")?));
            Ok(json!({"packages": to_value(&t.list()?)?}))
        }
        // sync-folder-put <資料夾> [--full] [--label L]：匯出到暫存再放成
        // paperreader-sync-<文獻庫代號>-<裝置代號>.zip
        "sync-folder-put" => {
            let dir = Path::new(args.pos(0, "folder")?);
            let mode = if args.flag("full").is_some() {
                SyncMode::Full
            } else {
                SyncMode::Changes
            };
            let tmp = std::env::temp_dir().join(format!("prcli-sync-{}.zip", new_id()));
            let report =
                match store.export_sync_package(&tmp, mode, args.flag("label"), &mut |_, _| {}) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(e);
                    }
                };
            let name = format!("paperreader-sync-{library_id}-{device}.zip");
            let put = FolderTransport::new(dir).put(&tmp, &name);
            let _ = std::fs::remove_file(&tmp);
            put?;
            let mut v = to_value(&report)?;
            v["name"] = json!(name);
            Ok(v)
        }
        // sync-folder-fetch <資料夾> <名稱> <輸出檔>
        "sync-folder-fetch" => {
            let mut t = FolderTransport::new(Path::new(args.pos(0, "folder")?));
            let name = args.pos(1, "name")?;
            let out = args.pos(2, "output file")?;
            let p = t
                .list()?
                .into_iter()
                .find(|p| p.name == name)
                .ok_or_else(|| LibError::NotFound {
                    what: format!("package {name}"),
                })?;
            t.fetch(&p, Path::new(out))?;
            Ok(json!({"fetched": out}))
        }
        other => Err(invalid(format!("unknown command: {other}"))),
    }
}

/// 群組的記號（`library.json` 的）：`--token` 沒給就先讀。群組的記號一定存在，不接受 `none`。
fn group_token(store: &LocalLibrary, args: &Args) -> Result<VersionToken> {
    match args.token_flag() {
        Some(Some(t)) => Ok(t),
        Some(None) => Err(invalid("a group token cannot be none")),
        None => Ok(store.list_groups()?.1),
    }
}

/// 一篇文獻的 `paper.json` 記號：`--token` 沒給就先讀。
fn paper_token(store: &LocalLibrary, paper: &str, args: &Args) -> Result<VersionToken> {
    match args.token_flag() {
        Some(Some(t)) => Ok(t),
        Some(None) => Err(invalid("a paper token cannot be none")),
        None => Ok(store.get_paper(paper)?.1),
    }
}

/// 筆記檔的記號：`--token` 沒給就先讀（檔案不存在時是 `None`）；`--token none` 明確表示
/// 「我認為筆記檔不存在」。
fn notes_token(store: &LocalLibrary, paper: &str, args: &Args) -> Result<Option<VersionToken>> {
    match args.token_flag() {
        Some(t) => Ok(t),
        None => Ok(store.read_notes(paper)?.1),
    }
}

/// 筆跡檔的記號：`--token` 沒給就先讀（檔案不存在時是 `None`）。
fn ink_token(store: &LocalLibrary, paper: &str, args: &Args) -> Result<Option<VersionToken>> {
    match args.token_flag() {
        Some(t) => Ok(t),
        None => Ok(store.read_ink(paper)?.1),
    }
}

/// `l,b,r,t` → 矩形。
fn parse_rect(s: &str) -> Result<[f64; 4]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(invalid(format!("--rect needs l,b,r,t: {s}")));
    }
    let mut out = [0.0; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .trim()
            .parse()
            .map_err(|_| invalid(format!("--rect has a non-number: {s}")))?;
    }
    Ok(out)
}
