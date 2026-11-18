//! 檢查文獻庫。只回報，不修、不刪，不寫入任何東西；唯讀的文獻庫也能檢查。
//! 檢查直接看磁碟，不看記憶體裡的目錄（記憶體裡的可能已經過期）。

use super::notes::{inspect_notes_dir, is_valid_notes_dir};
use super::scan::scan_folders;
use super::{
    LIBRARY_FILE, LocalLibrary, PAPER_FILE, PAPER_SIDE_FILES, PAPERS_DIR, TEMP_PREFIX,
    is_valid_material_path, material_status, names_in,
};
use crate::digest::is_digest;
use crate::error::Result;
use crate::format::LibraryFile;
use crate::groups::{check_deleted_groups, parse_groups};
use crate::ink::InkFile;
use crate::jsonio::{hash_file, read_json};
use crate::overlay::{OVERLAY_FORM, OverlayFile};
use crate::references::{REFERENCES_FILE, read_references_file};
use crate::store::{CheckLevel, MaterialStatus, Problem, ProblemKind};
use crate::text::{TEXT_FILE, check_file as check_text_attributes, read_text_file};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// 根目錄裡應用認得的項目。
const KNOWN_ROOT: [&str; 5] = ["library.json", "papers", "logs", "notes", "jobs"];

impl LocalLibrary {
    pub fn check(&self, level: CheckLevel) -> Result<Vec<Problem>> {
        let mut out: Vec<Problem> = Vec::new();
        let problem = |kind, path: String, paper: Option<&str>| Problem {
            kind,
            path,
            paper: paper.map(String::from),
        };

        // 根目錄：不認得、也不是隱藏的項目。`notes_dir` 的第一段也是認得的。
        let notes_root = self.library.notes_dir.split('/').next().unwrap_or("");
        for name in list_dir(&self.root)? {
            if name.starts_with('.') || KNOWN_ROOT.contains(&name.as_str()) || name == notes_root {
                continue;
            }
            out.push(problem(ProblemKind::UnknownEntry, name, None));
        }

        // papers/：直接一層。
        let papers = self.root.join(PAPERS_DIR);
        let folders = scan_folders(&papers, None)?;
        if papers.is_dir() {
            for name in list_dir(&papers)? {
                if name.starts_with('.') {
                    continue;
                }
                // 檔案，或沒有 paper.json 的資料夾
                if !folders.contains_key(&name) {
                    out.push(problem(
                        ProblemKind::UnknownEntry,
                        format!("{PAPERS_DIR}/{name}"),
                        None,
                    ));
                }
            }
        }

        // 殘留的暫存項目（`.pr-` 開頭）：根目錄、papers/、各文獻資料夾這三層。
        // 開啟時清不掉的，或唯讀開啟時沒有清的，都在這裡列出。
        let unfinished = self.unfinished_sync_name();
        for name in list_dir(&self.root)? {
            if Some(&name) == unfinished.as_ref() {
                out.push(problem(ProblemKind::UnfinishedSyncImport, name, None));
            } else if name.starts_with(TEMP_PREFIX) {
                out.push(problem(ProblemKind::LeftoverTemp, name, None));
            }
        }
        if papers.is_dir() {
            for name in list_dir(&papers)? {
                if name.starts_with(TEMP_PREFIX) {
                    out.push(problem(
                        ProblemKind::LeftoverTemp,
                        format!("{PAPERS_DIR}/{name}"),
                        None,
                    ));
                } else if !name.starts_with('.') && papers.join(&name).is_dir() {
                    let id = folders
                        .get(&name)
                        .and_then(|f| f.paper.as_ref())
                        .map(|p| p.id.as_str());
                    for inner in list_dir(&papers.join(&name))? {
                        if inner.starts_with(TEMP_PREFIX) {
                            out.push(problem(
                                ProblemKind::LeftoverTemp,
                                format!("{PAPERS_DIR}/{name}/{inner}"),
                                id,
                            ));
                        }
                    }
                }
            }
        }

        // logs/：讀不了的紀錄檔（其餘的功能都略過它，這裡列成問題）。
        for name in super::events::unreadable_log_files(&self.root)? {
            out.push(problem(
                ProblemKind::UnreadableLogFile,
                format!("logs/{name}"),
                None,
            ));
        }

        // 筆記檔：`notes_dir` 不合規則就只列一項、不檢查它底下；否則逐份看
        // （使用者自己的 .md 與子資料夾不是問題）。
        let notes_dir = self.library.notes_dir.as_str();
        if !is_valid_notes_dir(notes_dir) {
            out.push(problem(
                ProblemKind::InvalidNotesDir,
                LIBRARY_FILE.to_string(),
                None,
            ));
        } else {
            let dir = self.root.join(notes_dir);
            if dir.is_dir() {
                for name in list_dir(&dir)? {
                    if name.starts_with(TEMP_PREFIX) {
                        out.push(problem(
                            ProblemKind::LeftoverTemp,
                            format!("{notes_dir}/{name}"),
                            None,
                        ));
                    }
                }
            }
            let mut claimed: HashSet<String> = HashSet::new();
            for f in inspect_notes_dir(&self.root, notes_dir) {
                let path = format!("{notes_dir}/{}", f.name);
                if f.unreadable {
                    out.push(problem(
                        ProblemKind::UnreadableNotesFile,
                        path,
                        f.paper.as_deref(),
                    ));
                    continue;
                }
                let Some(paper) = f.paper.as_deref() else {
                    continue;
                };
                if !claimed.insert(paper.to_string()) {
                    out.push(problem(
                        ProblemKind::DuplicateNotesFile,
                        path.clone(),
                        Some(paper),
                    ));
                }
                if f.bad_blocks {
                    out.push(problem(ProblemKind::UnreadableNoteBlock, path, Some(paper)));
                }
            }
        }

        // 群組：直接看磁碟上的 library.json。讀不懂就列一項，並且不檢查懸空的代號
        // （不知道哪些群組存在）。library.json 本身讀不了，開啟時就已經失敗，這裡略過。
        let known_groups: Option<HashSet<String>> =
            match read_json::<LibraryFile>(&self.root.join(LIBRARY_FILE)) {
                Ok((lf, _)) => {
                    match (
                        parse_groups(&lf.groups),
                        check_deleted_groups(&lf.deleted_groups),
                    ) {
                        (Ok(defs), Ok(_)) => Some(defs.into_iter().map(|d| d.id).collect()),
                        _ => {
                            out.push(problem(
                                ProblemKind::UnreadableGroups,
                                LIBRARY_FILE.to_string(),
                                None,
                            ));
                            None
                        }
                    }
                }
                Err(_) => None,
            };

        let mut seen: HashSet<&str> = HashSet::new();
        for (name, folder) in &folders {
            let rel = format!("{PAPERS_DIR}/{name}");
            let Some(paper) = &folder.paper else {
                out.push(problem(
                    ProblemKind::UnreadablePaperFile,
                    format!("{rel}/{PAPER_FILE}"),
                    None,
                ));
                continue;
            };
            if !seen.insert(paper.id.as_str()) {
                out.push(problem(ProblemKind::DuplicateId, rel, Some(&paper.id)));
                continue;
            }

            if let Some(known) = &known_groups
                && paper.groups.iter().any(|g| !known.contains(g))
            {
                out.push(problem(
                    ProblemKind::UnknownGroupRef,
                    format!("{rel}/{PAPER_FILE}"),
                    Some(&paper.id),
                ));
            }

            let dir = papers.join(name);
            let mut listed: HashSet<String> = HashSet::new();
            listed.insert(PAPER_FILE.to_string());
            // 每篇固定一個的檔（筆跡、參考文獻清單、抽出的文字）：認得；讀不懂時列成問題（不修、不刪）
            for name in PAPER_SIDE_FILES {
                listed.insert(name.to_string());
                let path = dir.join(name);
                if let Some(kind) = unreadable_side_file(name, &path, level) {
                    out.push(problem(kind, format!("{rel}/{name}"), Some(&paper.id)));
                }
            }
            for m in &paper.materials {
                if is_valid_material_path(&m.path) {
                    let first = m.path.replace('\\', "/");
                    if let Some(seg) = first.split('/').next() {
                        listed.insert(seg.to_string());
                    }
                }
                // 譯文疊加檔：檔案每翻完一批就會改（材料沒有指紋與大小），所以只確認讀得懂
                if m.form == OVERLAY_FORM
                    && is_valid_material_path(&m.path)
                    && dir.join(&m.path).is_file()
                    && read_json::<OverlayFile>(&dir.join(&m.path)).is_err()
                {
                    out.push(problem(
                        ProblemKind::UnreadableOverlayFile,
                        format!("{rel}/{}", m.path),
                        Some(&paper.id),
                    ));
                }
                // 重點筆記：可以在筆記工具裡改，所以只確認讀得懂，不比大小與指紋
                let digest = is_digest(m);
                if digest
                    && is_valid_material_path(&m.path)
                    && dir.join(&m.path).is_file()
                    && !digest_readable(&dir.join(&m.path))
                {
                    out.push(problem(
                        ProblemKind::UnreadableDigestFile,
                        format!("{rel}/{}", m.path),
                        Some(&paper.id),
                    ));
                }
                let mut status = material_status(&dir, m);
                if digest && status == MaterialStatus::SizeMismatch {
                    status = MaterialStatus::Ok;
                }
                let file_rel = format!("{rel}/{}", m.path);
                match status {
                    MaterialStatus::InvalidPath => out.push(problem(
                        ProblemKind::MaterialInvalidPath,
                        format!("{rel}/{PAPER_FILE}"),
                        Some(&paper.id),
                    )),
                    MaterialStatus::Missing => out.push(problem(
                        ProblemKind::MaterialMissing,
                        file_rel,
                        Some(&paper.id),
                    )),
                    MaterialStatus::SizeMismatch => out.push(problem(
                        ProblemKind::MaterialSizeMismatch,
                        file_rel,
                        Some(&paper.id),
                    )),
                    MaterialStatus::Ok => {
                        if level == CheckLevel::Full
                            && !digest
                            && let Some(want) = &m.sha256
                        {
                            let (got, _) = hash_file(&dir.join(&m.path))?;
                            if &got != want {
                                out.push(problem(
                                    ProblemKind::MaterialHashMismatch,
                                    file_rel,
                                    Some(&paper.id),
                                ));
                            }
                        }
                    }
                }
            }
            for entry in list_dir(&dir)? {
                if entry.starts_with('.') || listed.contains(&entry) {
                    continue;
                }
                // 同步時另存的重點筆記衝突副本（屬性區有 `paperreader_conflict_of`）是給人看的，不是問題
                if entry.ends_with(".md") && is_conflict_copy(&dir.join(&entry)) {
                    continue;
                }
                out.push(problem(
                    ProblemKind::UnlistedFile,
                    format!("{rel}/{entry}"),
                    Some(&paper.id),
                ));
            }
        }

        out.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then((a.kind as u8).cmp(&(b.kind as u8)))
        });
        Ok(out)
    }
}

/// 每篇固定一個的檔存在而讀不懂時，交回要列的問題種類。
fn unreadable_side_file(name: &str, path: &Path, level: CheckLevel) -> Option<ProblemKind> {
    if name == TEXT_FILE {
        // 每篇都有、是文獻庫裡最大的一批 JSON，快速檢查又常跑（回到前景、同步與加入之後）：快速檢查只看檔案屬性
        // （不是一般檔案、超過 32 MB），不開檔；整份讀、解析留給完整檢查
        let bad = match level {
            CheckLevel::Quick => check_text_attributes(path).is_err(),
            CheckLevel::Full => read_text_file(path).is_err(),
        };
        return bad.then_some(ProblemKind::UnreadableTextFile);
    }
    if name == REFERENCES_FILE {
        // 不存在是 Ok(None)；超過 9 MB、不是一般檔案、讀不懂都是錯誤
        return read_references_file(path)
            .is_err()
            .then_some(ProblemKind::UnreadableReferencesFile);
    }
    (path.is_file() && read_json::<InkFile>(path).is_err())
        .then_some(ProblemKind::UnreadableInkFile)
}

/// 重點筆記讀得懂：UTF-8、屬性區有 `paperreader_digest`。
fn digest_readable(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .is_some_and(|t| crate::digest::parse(&t).is_ok())
}

/// 衝突副本：屬性區有 `paperreader_conflict_of`。
fn is_conflict_copy(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .is_some_and(|t| crate::notes::conflict_of(&t).is_some())
}

/// 資料夾裡每一個項目的名稱，依名稱排序（回報的順序固定）。
fn list_dir(dir: &Path) -> Result<Vec<String>> {
    let mut v = names_in(dir)?;
    v.sort();
    Ok(v)
}
