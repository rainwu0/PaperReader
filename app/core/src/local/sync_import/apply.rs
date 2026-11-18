//! 寫入：組好暫存、寫計畫檔、逐項執行；寫到一半的合併的重做。

use super::*;

const PLAN_FORMAT: u32 = 1;

impl LocalLibrary {
    /// 寫入。`choices` 是每個衝突的選擇（預覽裡的代號 → 選項）。
    pub fn apply_sync_import(
        &mut self,
        preview_id: &str,
        choices: &[(String, ConflictChoice)],
    ) -> Result<SyncImportReport> {
        // 1.
        match &self.pending_sync {
            Some(p) if p.preview_id == preview_id => {}
            _ => {
                return Err(LibError::NotFound {
                    what: format!("sync preview {preview_id}"),
                });
            }
        }
        self.require_writable()?;
        // 2. 每一個衝突都要有一個選擇，而且是它提供的
        let pending = self.pending_sync.as_ref().expect("checked");
        let mut by_key: HashMap<String, ConflictChoice> = HashMap::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (cid, choice) in choices {
            let Some((_, key, options)) = pending.keys.iter().find(|(c, _, _)| c == cid) else {
                return Err(LibError::InvalidInput {
                    reason: format!("no such conflict: {cid}"),
                });
            };
            if !options.contains(choice) {
                return Err(LibError::InvalidInput {
                    reason: format!("conflict {cid} does not offer {choice:?}"),
                });
            }
            if !seen.insert(cid.as_str()) {
                return Err(LibError::InvalidInput {
                    reason: format!("conflict {cid} is given twice"),
                });
            }
            by_key.insert(key.clone(), *choice);
        }
        if let Some((cid, _, _)) = pending
            .keys
            .iter()
            .find(|(c, _, _)| !seen.contains(c.as_str()))
        {
            return Err(LibError::InvalidInput {
                reason: format!("no choice for conflict {cid}"),
            });
        }
        // 3. 計畫記下的本機檔還是那時的樣子
        for (rel, sha) in &pending.tokens {
            if file_sha(&self.root.join(rel)) != *sha {
                let p = self.pending_sync.take().expect("checked");
                let _ = fs::remove_dir_all(&p.staging);
                return Err(LibError::Conflict);
            }
        }
        let pending = self.pending_sync.take().expect("checked");
        let staging = pending.staging.clone();
        let result = self.build_and_run(&pending, &by_key);
        if result.is_err() && self.unfinished_sync.is_none() {
            // 計畫檔寫成之前失敗：什麼都沒改，刪掉暫存
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    fn build_and_run(
        &mut self,
        pending: &PendingSync,
        choices: &HashMap<String, ConflictChoice>,
    ) -> Result<SyncImportReport> {
        let staging = &pending.staging;
        let inn = staging.join("in");
        let outd = staging.join("out");
        let tree = RemoteTree::new(inn.clone(), pending.manifest.clone());
        let computed = self.compute_merge(&tree, choices)?;
        // 預覽之後才出現（或消失）的衝突：使用者沒有看過，不照預設寫
        let now_keys: Vec<&str> = computed
            .ctx
            .conflicts
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        let seen_keys: Vec<&str> = pending.keys.iter().map(|(_, k, _)| k.as_str()).collect();
        if now_keys != seen_keys {
            return Err(LibError::Conflict);
        }

        // 4. 算出全部的結果，放進暫存 out/
        fs::create_dir_all(&outd).map_err(|e| io_error(&outd, e))?;
        let mut n = 0usize;
        let mut moved: HashMap<String, PathBuf> = HashMap::new();
        let mut dirs: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        // 每一個暫存檔都 `sync_all`，所在的資料夾在寫計畫檔之前 `sync_dir`
        let mut place_src = |src: &Src, dest: &Path, want: &str| -> Result<()> {
            if let Some(d) = dest.parent() {
                fs::create_dir_all(d).map_err(|e| io_error(d, e))?;
                let mut a = Some(d);
                while let Some(x) = a {
                    if !x.starts_with(&outd) {
                        break;
                    }
                    dirs.insert(x.to_path_buf());
                    a = x.parent();
                }
            }
            match src {
                Src::Bytes(b) => {
                    let mut f = fs::File::create(dest).map_err(|e| io_error(dest, e))?;
                    f.write_all(b).map_err(|e| io_error(dest, e))?;
                    f.sync_all().map_err(|e| io_error(dest, e))?;
                }
                Src::Remote(rel) => {
                    let from = inn.join(rel);
                    if from.exists() {
                        fs::rename(&from, dest).map_err(|e| io_error(&from, e))?;
                        moved.insert(rel.clone(), dest.to_path_buf());
                    } else if let Some(prev) = moved.get(rel) {
                        fs::copy(prev, dest).map_err(|e| io_error(prev, e))?;
                        sync_file(dest)?;
                    } else {
                        return Err(io_error(
                            &from,
                            std::io::Error::from(std::io::ErrorKind::NotFound),
                        ));
                    }
                }
                Src::Local(p) => {
                    fs::copy(p, dest).map_err(|e| io_error(p, e))?;
                    sync_file(dest)?;
                    if file_sha(dest).as_deref() != Some(want) {
                        return Err(LibError::Conflict);
                    }
                }
            }
            Ok(())
        };
        let mut items: Vec<PlanItem> = Vec::new();
        // 1 段：附加記錄
        let log_dir = outd.join("logs");
        for name in &computed.logs {
            let from = inn.join("logs").join(name);
            let dest = log_dir.join(name);
            place_src(&Src::Remote(format!("logs/{name}")), &dest, "")
                .or_else(|e| if from.exists() { Err(e) } else { Ok(()) })?;
        }
        items.push(PlanItem::Append {
            records: computed.ctx.records.clone(),
            logs: computed.logs.clone(),
        });
        // 2 段：新的文獻
        for np in &computed.new_papers {
            let dir = outd.join("new").join(&np.paper);
            for (rel, src, sha) in &np.files {
                place_src(src, &dir.join(rel), sha)?;
            }
            items.push(PlanItem::NewPaper {
                paper: np.paper.clone(),
                to: np.to,
                name: np.name.clone(),
                source: format!("out/new/{}", np.paper),
                files: np
                    .files
                    .iter()
                    .map(|(rel, _, sha)| StagedFile {
                        path: rel.clone(),
                        sha256: sha.clone(),
                    })
                    .collect(),
            });
        }
        // 3 段：回收區的移動
        for (paper, to, name) in &computed.moves {
            items.push(PlanItem::Move {
                paper: paper.clone(),
                to: *to,
                name: name.clone(),
            });
        }
        // 3a 段：永久刪除了的，回收區裡的資料夾刪掉
        for paper in &computed.purges {
            items.push(PlanItem::PurgeTrash {
                paper: paper.clone(),
            });
        }
        // 4 到 6 段：每篇的檔、筆記檔與衝突副本、library.json
        let mut writes: Vec<&WriteItem> = computed.paper_writes.iter().collect();
        // 每一篇依序：材料檔，然後 ink.json、譯文疊加檔，最後 paper.json
        writes.sort_by_key(|w| (w.paper.clone(), w.target == PAPER_FILE));
        let lib_last = |w: &&WriteItem| w.target == crate::local::LIBRARY_FILE;
        let mut root_writes: Vec<&WriteItem> = computed.root_writes.iter().collect();
        root_writes.sort_by_key(lib_last);
        for w in writes.into_iter().chain(root_writes) {
            n += 1;
            let rel = format!("out/f{n}");
            place_src(&w.src, &staging.join(&rel), &w.after)?;
            items.push(PlanItem::Write {
                paper: w.paper.clone(),
                target: w.target.clone(),
                source: rel,
                before: w.before.clone(),
                after: w.after.clone(),
                notes: w.notes,
            });
        }
        // 6a 段：刪除記錄拿掉的材料，檔搬進回收區（paper.json 已經寫好）
        for (paper, material) in &computed.trash_materials {
            items.push(PlanItem::TrashMaterial {
                paper: paper.clone(),
                material: Box::new(material.clone()),
            });
        }
        // 7 段：sync-imported
        let rec = ChangeRecord {
            id: new_id(),
            t: now_utc(),
            device: self.device_id.clone(),
            paper: None,
            kind: SYNC_IMPORTED.to_string(),
            data: serde_json::json!({
                "package": pending.manifest.package,
                "from_device": pending.manifest.device,
                "package_created": pending.manifest.created,
            }),
            extra: Default::default(),
        };
        items.push(PlanItem::SyncImported { record: rec });
        items.push(PlanItem::Finish);
        // 組好之後刪掉 in/；暫存的資料夾落盤
        fs::remove_dir_all(&inn).map_err(|e| io_error(&inn, e))?;
        for d in &dirs {
            sync_dir(d);
        }
        sync_dir(staging);

        let plan = Plan {
            format: PLAN_FORMAT,
            package: pending.manifest.package.clone(),
            from_device: pending.manifest.device.clone(),
            items,
            report: PlanReport {
                added: Summary::items(&computed.ctx.summary.added),
                updated: Summary::items(&computed.ctx.summary.updated),
                removed: Summary::items(&computed.ctx.summary.removed),
                conflict_copies: computed.copies.clone(),
                skipped_papers: computed.skipped.clone(),
                notices: notices_with_conflict_ids(computed.ctx.notices.clone(), &pending.keys),
            },
        };
        // 5. 寫計畫檔：這一刻就是確定要做
        if self.hook_says_stop(0) {
            return Err(stopped());
        }
        write_json_atomic(&staging.join(PLAN_FILE), &plan)?;
        // 6–7.
        self.run_plan(staging, &plan)
    }

    /// 依序執行計畫的每一項。中途失敗：停在那裡、交回錯誤，文獻庫轉成唯讀，直到計畫做完。
    fn run_plan(&mut self, staging: &Path, plan: &Plan) -> Result<SyncImportReport> {
        let mut notices: Vec<SyncNotice> = Vec::new();
        let mut copies: Vec<String> = Vec::new();
        let mut problems = Vec::new();
        let mut dirs: Option<HashMap<String, (Place, String)>> = None;
        for (k, item) in plan.items.iter().enumerate() {
            let step = if self.hook_says_stop(k + 1) {
                Err(stopped())
            } else {
                self.run_item(staging, plan, item, &mut notices, &mut copies, &mut dirs)
            };
            match step {
                Ok(Some(p)) => problems = p,
                Ok(None) => {}
                Err(e) => {
                    // 收尾已刪掉暫存資料夾之後的錯誤（重新掃描、檢查）不是「計畫沒做完」
                    if staging.exists() {
                        self.unfinished_sync = Some(staging.to_path_buf());
                    } else {
                        self.unfinished_sync = None;
                    }
                    let _ = self.reload_after_sync();
                    return Err(e);
                }
            }
        }
        let mut report_notices = plan.report.notices.clone();
        report_notices.extend(notices);
        let mut conflict_copies = plan.report.conflict_copies.clone();
        conflict_copies.extend(copies);
        Ok(SyncImportReport {
            package: plan.package.clone(),
            added: plan.report.added.clone(),
            updated: plan.report.updated.clone(),
            removed: plan.report.removed.clone(),
            conflict_copies,
            skipped_papers: plan.report.skipped_papers.clone(),
            notices: report_notices,
            problems,
        })
    }

    /// 執行一項。收尾那一項交回快速檢查的結果。
    fn run_item(
        &mut self,
        staging: &Path,
        plan: &Plan,
        item: &PlanItem,
        notices: &mut Vec<SyncNotice>,
        copies: &mut Vec<String>,
        dirs: &mut Option<HashMap<String, (Place, String)>>,
    ) -> Result<Option<Vec<crate::store::Problem>>> {
        match item {
            PlanItem::Append { records, logs } => {
                self.append_missing_records(records)?;
                for name in logs {
                    let remote = match fs::read(staging.join("out/logs").join(name)) {
                        Ok(b) => b,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(io_error(&staging.join("out/logs").join(name), e)),
                    };
                    let path = self.root.join(crate::local::events::LOGS_DIR).join(name);
                    let local = match fs::read(&path) {
                        Ok(b) => Some(b),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                        Err(e) => return Err(io_error(&path, e)),
                    };
                    if let LogMerge::Append(bytes) = merge_log(local.as_deref(), &remote) {
                        append_bytes(&path, &bytes)?;
                    }
                }
            }
            PlanItem::NewPaper {
                paper,
                to,
                name,
                source,
                files,
            } => {
                let src = staging.join(source);
                if self.paper_dir(dirs, paper)?.is_some() {
                    if src.exists() {
                        let _ = fs::remove_dir_all(&src);
                    }
                } else if src.exists() {
                    // 暫存的每一個檔都要等於計畫記的指紋
                    for f in files {
                        verify_staged(&src.join(&f.path), &f.sha256)?;
                    }
                    *dirs = None;
                    let base = self.root.join(base_of(*to));
                    fs::create_dir_all(&base).map_err(|e| io_error(&base, e))?;
                    let name = unique_name(&names_in_or_empty(&base), name);
                    fs::rename(&src, base.join(&name)).map_err(|e| io_error(&src, e))?;
                    sync_dir(&base);
                }
            }
            PlanItem::Move { paper, to, name } => {
                if let Some((place, folder)) = self.paper_dir(dirs, paper)?
                    && place != *to
                {
                    *dirs = None;
                    let from = self.root.join(base_of(place)).join(&folder);
                    let base = self.root.join(base_of(*to));
                    fs::create_dir_all(&base).map_err(|e| io_error(&base, e))?;
                    let name = unique_name(&names_in_or_empty(&base), name);
                    fs::rename(&from, base.join(&name)).map_err(|e| io_error(&from, e))?;
                    sync_dir(&base);
                    sync_dir(&self.root.join(base_of(place)));
                }
            }
            PlanItem::Write {
                paper,
                target,
                source,
                before,
                after,
                notes,
            } => {
                let src = staging.join(source);
                let path = match paper {
                    Some(id) => match self.paper_dir(dirs, id)? {
                        Some((place, folder)) => {
                            self.root.join(base_of(place)).join(folder).join(target)
                        }
                        None => {
                            notices.push(SyncNotice {
                                kind: NoticeKind::UnmergedWrite,
                                paper: Some(id.clone()),
                                path: Some(target.clone()),
                                other: None,
                                conflicts: Vec::new(),
                            });
                            return Ok(None);
                        }
                    },
                    None => self.root.join(target),
                };
                let cur = file_sha(&path);
                if cur.as_deref() == Some(after.as_str()) {
                    if src.exists() {
                        let _ = fs::remove_file(&src);
                    }
                } else if cur == *before && src.exists() {
                    // 暫存檔要等於計畫記的 `after`；不符就停下、維持計畫沒做完
                    verify_staged(&src, after)?;
                    if let Some(d) = path.parent() {
                        fs::create_dir_all(d).map_err(|e| io_error(d, e))?;
                    }
                    fs::rename(&src, &path).map_err(|e| io_error(&path, e))?;
                    if let Some(d) = path.parent() {
                        sync_dir(d);
                    }
                } else {
                    // 計畫之後有人在別處改過它：不覆寫
                    let rel = self.rel_of(&path);
                    if *notes && src.exists() {
                        let text = fs::read_to_string(&src).map_err(|e| io_error(&src, e))?;
                        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
                        let file = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let stem = file.strip_suffix(".md").unwrap_or(&file);
                        let dev6: String = plan.from_device.chars().take(6).collect();
                        let base = unique_stem(
                            &names_in_or_empty(&dir),
                            ".md",
                            &format!("{stem} (conflict {dev6} {})", now_compact()),
                        );
                        let cp = dir.join(format!("{base}.md"));
                        crate::jsonio::write_bytes_atomic(&cp, as_conflict_copy(&text).as_bytes())?;
                        copies.push(self.rel_of(&cp));
                        let _ = fs::remove_file(&src);
                    }
                    let n = SyncNotice {
                        kind: NoticeKind::UnmergedWrite,
                        paper: paper.clone(),
                        path: Some(rel),
                        other: None,
                        conflicts: Vec::new(),
                    };
                    if !notices.contains(&n) {
                        notices.push(n);
                    }
                }
            }
            PlanItem::TrashMaterial { paper, material } => {
                if let Some((place, folder)) = self.paper_dir(dirs, paper)? {
                    let dir = self.root.join(base_of(place)).join(&folder);
                    // paper.json 還列著它（寫入沒做成）或還有別的材料用同一個檔：不搬，免得清單指向不見的檔
                    let listed = read_json::<crate::format::PaperFile>(&dir.join(PAPER_FILE))
                        .map(|(p, _)| {
                            p.materials
                                .iter()
                                .any(|m| m.id == material.id || m.path == material.path)
                        })
                        .unwrap_or(true);
                    if !listed {
                        crate::local::remove::trash_material_file(
                            &self.root,
                            &self.device_id,
                            paper,
                            &folder,
                            &dir,
                            material,
                        )?;
                    }
                }
            }
            PlanItem::PurgeTrash { paper } => {
                // 目錄裡有同一篇（計畫之後被放回）就不刪；回收區裡同一篇的每一個資料夾都刪
                let in_catalog = self
                    .paper_dir(dirs, paper)?
                    .is_some_and(|(place, _)| place == Place::Papers);
                if !in_catalog {
                    for e in self.list_trash()? {
                        if &e.paper_id == paper {
                            crate::local::remove::delete_trash_folder(&self.root, &e.folder)?;
                            *dirs = None;
                        }
                    }
                }
            }
            PlanItem::SyncImported { record } => {
                self.append_missing_records(std::slice::from_ref(record))?;
            }
            PlanItem::Finish => {
                fs::remove_dir_all(staging).map_err(|e| io_error(staging, e))?;
                self.unfinished_sync = None;
                self.reload_after_sync()?;
                return Ok(Some(self.check(CheckLevel::Quick)?));
            }
        }
        Ok(None)
    }

    /// 附加這台變更紀錄檔裡還沒有的記錄（以 `id` 判斷）。
    fn append_missing_records(&mut self, records: &[ChangeRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let path = self
            .root
            .join(crate::local::events::LOGS_DIR)
            .join(changes_file_name(&self.device_id));
        let have: HashSet<String> = match fs::read(&path) {
            Ok(b) => b
                .split(|c| *c == b'\n')
                .filter_map(parse_change_line)
                .map(|r| r.id)
                .collect(),
            Err(_) => HashSet::new(),
        };
        for r in records {
            if !have.contains(&r.id) {
                let mut text = serde_json::to_string(r).map_err(|e| LibError::InvalidInput {
                    reason: e.to_string(),
                })?;
                text.push('\n');
                crate::local::events::append_line(&path, &text)?;
            }
        }
        Ok(())
    }

    /// 依文獻代號找那篇的資料夾：先 `papers/`，再 `.trash/`（最晚移除的那一份）。用一份對照表（執行計畫時一次建好；新的文獻與移動之後重建）。
    fn paper_dir(
        &self,
        dirs: &mut Option<HashMap<String, (Place, String)>>,
        id: &str,
    ) -> Result<Option<(Place, String)>> {
        if dirs.is_none() {
            let mut map: HashMap<String, (Place, String)> = HashMap::new();
            let fresh = crate::local::scan::scan_folders(&self.papers_dir(), None)?;
            for f in fresh.values() {
                if let Some(p) = &f.paper {
                    map.entry(p.id.clone())
                        .or_insert((Place::Papers, f.name.clone()));
                }
            }
            for e in self.list_trash()? {
                map.entry(e.paper_id.clone())
                    .or_insert((Place::Trash, e.folder.clone()));
            }
            *dirs = Some(map);
        }
        Ok(dirs.as_ref().and_then(|m| m.get(id).cloned()))
    }

    /// 寫完（或寫到一半）之後，讓記憶體裡的目錄、`library.json`、筆記檔的索引、閱讀事件反映磁碟上的樣子。
    fn reload_after_sync(&mut self) -> Result<()> {
        if let Ok((lf, _)) = read_json::<LibraryFile>(&self.root.join(crate::local::LIBRARY_FILE)) {
            self.library = lf;
        }
        self.folders = crate::local::scan::scan_folders(&self.papers_dir(), None)?;
        self.notes_index =
            crate::local::notes::scan_notes_dir(&self.root, &self.library.notes_dir, None);
        self.events = crate::local::events::EventIndex::load(&self.root, &self.device_id)?;
        Ok(())
    }

    /// 唯讀是因為有寫到一半的合併（不是格式較新）：畫面給「重試」。
    pub fn has_unfinished_sync_import(&self) -> bool {
        self.unfinished_sync.is_some()
    }

    /// 開啟文獻庫時自動做完了上一次沒做完的合併：那一次的回報。留到畫面說了「做完了」、呼叫
    /// `clear_resumed_sync_report` 為止（問狀況的那一次被畫面丟掉也不會不見）。
    pub fn resumed_sync_report(&self) -> Option<&SyncImportReport> {
        self.resumed_report.as_ref()
    }

    /// 畫面說了「做完了」之後清掉開啟時做完的合併的回報。
    pub fn clear_resumed_sync_report(&mut self) {
        self.resumed_report = None;
    }

    /// 有寫到一半的合併時，把它做完（不必重開文獻庫）。沒有時 `NotFound`。
    /// 開啟文獻庫時已經自動做完的，交回那一次的回報。
    pub fn resume_sync_import(&mut self) -> Result<SyncImportReport> {
        if self.read_only {
            return Err(LibError::ReadOnly);
        }
        // 先看有沒有寫到一半的：它比開啟時自動做完的那一份新
        if let Some(dir) = self.unfinished_sync.clone() {
            self.resumed_report = None;
            return self.resume_at(&dir);
        }
        if let Some(r) = self.resumed_report.take() {
            return Ok(r);
        }
        match find_plan_dir(&self.root) {
            Some(dir) => self.resume_at(&dir),
            None => Err(LibError::NotFound {
                what: "unfinished sync import".to_string(),
            }),
        }
    }

    /// 照計畫從頭重做一遍（每一項都認得出已經做過的）。
    pub(crate) fn resume_at(&mut self, dir: &Path) -> Result<SyncImportReport> {
        let (plan, _) = match read_json::<Plan>(&dir.join(PLAN_FILE)) {
            Ok(v) => v,
            Err(e) => {
                self.unfinished_sync = Some(dir.to_path_buf());
                return Err(e);
            }
        };
        self.run_plan(dir, &plan)
    }

    /// 有寫到一半的合併時，它的暫存資料夾（相對於根目錄的名稱）。
    pub(crate) fn unfinished_sync_name(&self) -> Option<String> {
        self.unfinished_sync
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }
}

/// 檔案內容落盤。
fn sync_file(path: &Path) -> Result<()> {
    // Windows 上 `sync_all` 要有寫入權限，所以以可寫的方式打開（不截斷）
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| io_error(path, e))
}

/// 暫存檔的指紋要等於計畫記的；不符（例如斷電留下空檔）就交回錯誤，不改名。
fn verify_staged(path: &Path, want: &str) -> Result<()> {
    if file_sha(path).as_deref() == Some(want) {
        Ok(())
    } else {
        Err(LibError::Io {
            path: path.display().to_string(),
            reason: "staged file does not match the plan".to_string(),
        })
    }
}

fn stopped() -> LibError {
    LibError::Io {
        path: String::new(),
        reason: "sync import stopped by test hook".to_string(),
    }
}

/// 附加一段位元組到檔案（不存在就建立）並 `sync_data`。
fn append_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| io_error(d, e))?;
    }
    let mut f = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|e| io_error(path, e))?;
    f.write_all(bytes).map_err(|e| io_error(path, e))?;
    f.sync_data().map_err(|e| io_error(path, e))
}
