//! 合併的計算：讀兩邊的紀錄檔、`library.json`、目錄與回收區、筆記檔，逐篇合併（`units.rs`），
//! 彙總出衝突、提醒、要寫的東西。準備（預覽）與寫入都從這裡算起。

use super::*;

impl LocalLibrary {
    pub(super) fn compute_merge(
        &self,
        tree: &RemoteTree,
        choices: &HashMap<String, ConflictChoice>,
    ) -> Result<Computed> {
        let manifest = &tree.manifest;
        let mut ctx = MergeCtx::new(
            now_utc(),
            now_compact(),
            self.device_id.clone(),
            manifest.device.clone(),
            choices.clone(),
            Box::new(new_id),
        );
        let mut out = Computed {
            ctx: MergeCtx::new(
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                HashMap::new(),
                Box::new(new_id),
            ),
            skipped: Vec::new(),
            new_papers: Vec::new(),
            moves: Vec::new(),
            paper_writes: Vec::new(),
            root_writes: Vec::new(),
            trash_materials: Vec::new(),
            purges: Vec::new(),
            copies: Vec::new(),
            tokens: BTreeMap::new(),
            logs: Vec::new(),
            already_imported: false,
        };

        // 紀錄檔：先合（在記憶體裡）
        let local_records = crate::local::changes::read_all_changes(&self.root)?;
        let remote_logs = tree.read_logs();
        let mut remote_records = Vec::new();
        for (name, bytes) in &remote_logs {
            if crate::changes::is_changes_file(name) {
                remote_records.extend(crate::local::changes::parse_change_bytes(bytes));
            }
        }
        let records = union_records(&local_records, &remote_records);
        let local_ids: HashSet<&str> = local_records.iter().map(|r| r.id.as_str()).collect();
        let thoughts_papers = |v: &[ChangeRecord]| -> HashSet<String> {
            v.iter()
                .filter(|r| r.kind == crate::changes::THOUGHTS_WRITTEN)
                .filter_map(|r| r.paper.clone())
                .collect()
        };
        ctx.local_thoughts = thoughts_papers(&local_records);
        ctx.remote_thoughts = thoughts_papers(&remote_records);
        out.already_imported = records.iter().any(|r| {
            r.kind == SYNC_IMPORTED && r.data_str("package") == Some(manifest.package.as_str())
        });
        out.logs = remote_logs.keys().cloned().collect();

        // library.json
        let lib_path = self.root.join(crate::local::LIBRARY_FILE);
        let (local_lib, _) = read_json::<LibraryFile>(&lib_path)?;
        let (remote_lib, _) = read_json::<LibraryFile>(&tree.path(crate::local::LIBRARY_FILE))?;
        let lm = merge_library(&local_lib, &remote_lib, &mut ctx);
        let deleted_groups = lm.deleted.clone();

        // 本機的文獻：目錄（代號重複的第一個）與回收區（每個代號最晚移除的那一份，目錄裡有的不算）
        let mut local: BTreeMap<String, (Place, String, PaperFile)> = BTreeMap::new();
        let fresh = crate::local::scan::scan_folders(&self.papers_dir(), None)?;
        for f in fresh.values() {
            if let Some(p) = &f.paper
                && !local.contains_key(&p.id)
            {
                local.insert(p.id.clone(), (Place::Papers, f.name.clone(), p.clone()));
            }
        }
        for e in self.list_trash()? {
            if local.contains_key(&e.paper_id) {
                continue;
            }
            if let Ok((p, _)) =
                read_json::<PaperFile>(&self.root.join(TRASH_DIR).join(&e.folder).join(PAPER_FILE))
            {
                local.insert(p.id.clone(), (Place::Trash, e.folder.clone(), p));
            }
        }
        let remote = tree.read_papers();
        let mut bad_remote: HashSet<String> = HashSet::new();
        for (id, reason) in &remote.bad {
            bad_remote.insert(id.clone());
            out.skipped.push(SkippedPaper {
                paper: id.clone(),
                title: None,
                reason: reason.clone(),
            });
        }

        // 筆記檔
        let notes_dir = self.library.notes_dir.clone();
        let notes_ok = notes_dir_path(&self.root, &notes_dir).is_some();
        let local_index = crate::local::notes::scan_notes_heads(&self.root, &notes_dir);
        let mut local_notes: BTreeMap<String, String> = BTreeMap::new();
        for (name, pid) in local_index.entries() {
            local_notes.entry(pid).or_insert(name);
        }
        let remote_notes = tree.read_notes(&remote_lib.notes_dir);
        let mut note_names: HashSet<String> = names_in_or_empty(&self.root.join(&notes_dir))
            .into_iter()
            .map(|n| n.to_lowercase())
            .collect();
        let existing_copies = self.existing_conflict_copies(&notes_dir);

        // 省略的材料：本機依指紋找
        let mut finder = CandidateFinder::new(self, &local);

        let mut ids: Vec<String> = local.keys().cloned().collect();
        ids.extend(remote.papers.keys().cloned());
        ids.extend(local_notes.keys().cloned());
        ids.extend(remote_notes.keys().cloned());
        ids.sort();
        ids.dedup();

        let mut papers_names: HashSet<String> = names_in_or_empty(&self.papers_dir())
            .into_iter()
            .map(|n| n.to_lowercase())
            .collect();
        let mut trash_names: HashSet<String> = names_in_or_empty(&self.root.join(TRASH_DIR))
            .into_iter()
            .map(|n| n.to_lowercase())
            .collect();

        // 本機原文的指紋（`SameSource` 用）
        let mut local_sources: HashMap<String, String> = HashMap::new();
        for (id, (_, _, p)) in &local {
            for m in &p.materials {
                if m.role == crate::format::ROLE_SOURCE
                    && let Some(s) = &m.sha256
                {
                    local_sources.insert(s.clone(), id.clone());
                }
            }
        }

        for id in ids {
            if bad_remote.contains(&id) {
                continue;
            }
            let lp = local.get(&id);
            let rp = remote.papers.get(&id);
            if let Some((_, _, p)) = lp
                && p.format > FORMAT_VERSION
            {
                out.skipped.push(SkippedPaper {
                    paper: id.clone(),
                    title: p.bib.title.clone(),
                    reason: "format-too-new".to_string(),
                });
                continue;
            }
            let title = lp
                .and_then(|(_, _, p)| p.bib.title.clone())
                .or_else(|| rp.and_then(|r| r.paper.bib.title.clone()));
            let mut sub = ctx.fork(Box::new(new_id));
            // 永久刪除之後有人放回、結果在目錄裡：這次匯入才知道的（那兩筆有一筆不在
            // 這台的紀錄裡）提醒一次，之後不再提醒
            if let Some((purge, restore)) = restored_after_purge(&records, &id)
                && latest_place_record(&records, &id).is_some_and(|r| r.kind == PAPER_RESTORED)
                && (!local_ids.contains(purge.id.as_str())
                    || !local_ids.contains(restore.id.as_str()))
            {
                sub.notice(NoticeKind::PurgedButRestored, Some(&id), None, None);
            }
            let mut unit = Unit::default();
            let r = self.merge_unit(
                &id,
                title.as_deref(),
                lp,
                rp,
                tree,
                &records,
                deleted_groups.as_ref(),
                &mut finder,
                &mut sub,
                &mut unit,
                &mut UnitNames {
                    papers: &mut papers_names,
                    trash: &mut trash_names,
                    notes: &mut note_names,
                },
                &NotesCtx {
                    ok: notes_ok,
                    dir: &notes_dir,
                    local: &local_notes,
                    remote: &remote_notes,
                    existing_copies: &existing_copies,
                },
                &local_sources,
            );
            match r {
                Ok(()) => {
                    if !sub.conflicts.is_empty() {
                        for (k, v) in &unit.read {
                            out.tokens.insert(k.clone(), v.clone());
                        }
                    }
                    ctx.absorb(sub);
                    for w in &unit.paper_writes {
                        if w.before.is_some()
                            && let Some(rel) = &unit.dir_rel
                        {
                            out.tokens
                                .insert(format!("{rel}/{}", w.target), w.before.clone());
                        }
                    }
                    for w in &unit.root_writes {
                        if w.before.is_some() {
                            out.tokens.insert(w.target.clone(), w.before.clone());
                        }
                    }
                    if !unit.moves.is_empty()
                        && let Some(rel) = &unit.dir_rel
                    {
                        let pj = format!("{rel}/{PAPER_FILE}");
                        let sha = file_sha(&self.root.join(&pj));
                        out.tokens.insert(pj, sha);
                    }
                    out.new_papers.extend(unit.new_papers);
                    out.moves.extend(unit.moves);
                    out.paper_writes.extend(unit.paper_writes);
                    out.root_writes.extend(unit.root_writes);
                    out.trash_materials.extend(unit.trash_materials);
                    out.purges.extend(unit.purges);
                    out.copies.extend(unit.copies);
                }
                Err(reason) => out.skipped.push(SkippedPaper {
                    paper: id.clone(),
                    title,
                    reason,
                }),
            }
        }

        if lm.changed {
            let bytes = canonical(&lm.file)?;
            let before = file_sha(&lib_path);
            out.tokens
                .insert(crate::local::LIBRARY_FILE.to_string(), before.clone());
            out.root_writes.push(WriteItem {
                paper: None,
                target: crate::local::LIBRARY_FILE.to_string(),
                after: sha256_hex(&bytes),
                src: Src::Bytes(bytes),
                before,
                notes: false,
            });
        }
        out.ctx = ctx;
        Ok(out)
    }

    /// 本機 `notes_dir` 裡的衝突副本：文獻代號 →（檔名, 內容）。
    fn existing_conflict_copies(&self, notes_dir: &str) -> HashMap<String, Vec<(String, String)>> {
        let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let Some(dir) = notes_dir_path(&self.root, notes_dir) else {
            return out;
        };
        let mut names = names_in_or_empty(&dir);
        names.sort();
        for n in names {
            if !n.ends_with(".md") || n.starts_with('.') {
                continue;
            }
            if let Ok(t) = fs::read_to_string(dir.join(&n))
                && let Some(pid) = conflict_of(&t)
            {
                out.entry(pid).or_default().push((n, t));
            }
        }
        out
    }
}
