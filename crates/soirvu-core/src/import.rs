// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Import (DESIGN §8): bring entries INTO a vault from plain
//! files — a Markdown folder (our own export, or another app's
//! notes; front matter optional) or a SoirVu JSON export.
//!
//! Import only ever ADDS. An id that is already taken — by a
//! live entry or a tombstone — is never overwritten: JSON
//! entries with taken ids are skipped, Markdown entries get a
//! fresh id. Nothing is merged and nothing is deleted.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entries::EntryDraft;
use crate::error::{Error, Result};
use crate::export::civil_from_days;
use crate::folder::Vault;

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub entries: usize,
    pub attachments: usize,
    pub skipped_entries: usize,
    pub skipped_attachments: usize,
    pub skipped_folders: usize,
    /// Folders created (or adopted) by this import.
    pub folders: usize,
    /// `entry:` links rewritten to a remapped id.
    pub links_remapped: usize,
    /// Distinct `entry:` targets that name nothing imported or live.
    pub links_unresolved: usize,
}

impl Vault {
    /// Imports every `.md` file under `dir` (searched
    /// recursively, in path order). See the module docs for
    /// the id rules.
    ///
    /// Two passes. The first inventories every file, creates the
    /// folders and assigns every entry its final id, so the
    /// second pass can resolve forward links regardless of file
    /// order and remap references when collision rules generate
    /// fresh ids. A `soirvu-export.json` manifest (our own
    /// export) drives exact folder/entry placement; without one,
    /// year directories (`2026/…`) are legacy layout and land at
    /// the root, while other directories become folders.
    ///
    /// Link remapping is textual: a remapped UUID is replaced
    /// everywhere it appears as `entry:<uuid>`, including inside
    /// code samples. The CLI reports the counts; parser-accurate
    /// planning belongs to the desktop bridge, which shares the
    /// editor's Markdown parser.
    pub fn import_markdown(&mut self, dir: &Path) -> Result<ImportReport> {
        let manifest = load_manifest(dir);
        let mut files = Vec::new();
        collect_markdown(dir, &mut files);
        files.sort();
        // Pass 0: manifest folders first, so entries can reference
        // them. Final ids are decided before any row is written:
        // a live folder with the same id is adopted as-is, a
        // taken id (tombstone or otherwise) maps to a fresh id,
        // and parents resolve through the same map.
        let mut manifest_folders: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if let Some(manifest) = &manifest {
            for folder in &manifest.folders {
                let final_id = if self.folder_is_live(&folder.id) {
                    folder.id.clone()
                } else if self.folder_row_exists(&folder.id) {
                    Uuid::now_v7().to_string()
                } else {
                    folder.id.clone()
                };
                manifest_folders.insert(folder.id.clone(), final_id);
            }
            let device = self.device_id.clone();
            for folder in &manifest.folders {
                let final_id = &manifest_folders[&folder.id];
                if self.folder_is_live(final_id) {
                    continue;
                }
                let parent = folder.parent_id.as_deref().map(|p| {
                    manifest_folders
                        .get(p)
                        .cloned()
                        .unwrap_or_else(|| p.to_string())
                });
                let now = now_ms();
                if self
                    .insert_folder_exact(
                        final_id,
                        parent.as_deref(),
                        &folder.name,
                        folder.created_at,
                        folder.updated_at,
                        None,
                        1,
                        &device,
                        0,
                        "",
                    )
                    .is_err()
                {
                    // A concurrently taken id or a name the live
                    // rules reject: fall back to a fresh id so the
                    // entries below still have a home.
                    let fresh = Uuid::now_v7().to_string();
                    if self
                        .insert_folder_exact(
                            &fresh,
                            parent.as_deref(),
                            &folder.name,
                            now,
                            now,
                            None,
                            1,
                            &device,
                            0,
                            "",
                        )
                        .is_ok()
                    {
                        manifest_folders.insert(folder.id.clone(), fresh);
                    }
                }
            }
        }
        // Pass 1: parse every file, resolve its folder, assign its
        // final entry id. No entries are written yet, so forward
        // links resolve in pass 2.
        let mut folder_ids: std::collections::HashMap<PathBuf, String> =
            std::collections::HashMap::new();
        let mut plans: Vec<PlannedFile> = Vec::with_capacity(files.len());
        let mut id_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut skipped = 0usize;
        for file in &files {
            let text = match fs::read_to_string(file) {
                Ok(text) => text,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let (fm, body) = split_front_matter(&text);
            let folder = self.plan_entry_folder(
                dir,
                file,
                &fm,
                &manifest_folders,
                &mut folder_ids,
                manifest.is_some(),
            );
            let entry_id = match &fm.id {
                Some(id)
                    if crate::storage::is_canonical_uuid(id)
                        && !self.entry_exists(id).unwrap_or(true) =>
                {
                    id_map.insert(id.clone(), id.clone());
                    id.clone()
                }
                _ => {
                    let fresh = Uuid::now_v7().to_string();
                    if let Some(old) = &fm.id {
                        if !old.is_empty() {
                            id_map.insert(old.clone(), fresh.clone());
                        }
                    }
                    fresh
                }
            };
            plans.push(PlannedFile {
                file: file.clone(),
                fm,
                body,
                folder,
                entry_id,
            });
        }
        let mut report = ImportReport {
            entries: 0,
            attachments: 0,
            skipped_entries: skipped,
            skipped_attachments: 0,
            skipped_folders: 0,
            folders: folder_ids.len() + manifest_folders.len(),
            links_remapped: 0,
            links_unresolved: 0,
        };
        // Pass 2: rewrite remapped links, then write entries and
        // their attachments.
        let mut seen_targets: std::collections::HashSet<String> = std::collections::HashSet::new();
        for mut plan in plans {
            for (old, new) in &id_map {
                if old != new {
                    let from = format!("entry:{old}");
                    let matches = plan.body.matches(&from).count();
                    if matches > 0 {
                        plan.body = plan.body.replace(&from, &format!("entry:{new}"));
                        report.links_remapped += matches;
                    }
                }
            }
            for target in entry_link_targets(&plan.body) {
                if !seen_targets.contains(&target)
                    && !id_map.contains_key(&target)
                    && !self.entry_exists(&target).unwrap_or(true)
                {
                    seen_targets.insert(target);
                    report.links_unresolved += 1;
                }
            }
            match self.insert_planned_markdown(dir, &plan) {
                Ok((a, sa)) => {
                    report.entries += 1;
                    report.attachments += a;
                    report.skipped_attachments += sa;
                }
                Err(_) => report.skipped_entries += 1,
            }
        }
        Ok(report)
    }

    fn folder_is_live(&self, id: &str) -> bool {
        crate::folders::folder_is_live(self.db.connection(), id).unwrap_or(false)
    }

    fn folder_row_exists(&self, id: &str) -> bool {
        self.db
            .connection()
            .query_row("SELECT 1 FROM folder WHERE id = ?1", [id], |_| Ok(true))
            .unwrap_or(false)
    }

    /// Where a Markdown file's entry lives: the manifest-mapped
    /// `folder:` id first, then a `folder:` id that is live in
    /// this vault, then the file's directory chain (created on
    /// demand, except legacy year directories), else the root.
    /// Returns the live folder id, or `None` for root.
    fn plan_entry_folder(
        &mut self,
        root: &Path,
        file: &Path,
        fm: &FrontMatter,
        manifest_folders: &std::collections::HashMap<String, String>,
        dir_folders: &mut std::collections::HashMap<PathBuf, String>,
        has_manifest: bool,
    ) -> Option<String> {
        if let Some(want) = fm.folder.as_deref() {
            if let Some(mapped) = manifest_folders.get(want) {
                if self.folder_is_live(mapped) {
                    return Some(mapped.clone());
                }
            } else if self.folder_is_live(want) {
                return Some(want.to_string());
            }
        }
        if has_manifest {
            // Without a front-matter folder the manifest cannot
            // place the file: it lands at the root.
            return None;
        }
        let rel = file.parent().and_then(|p| p.strip_prefix(root).ok())?;
        if rel.as_os_str().is_empty() {
            return None;
        }
        let mut parent: Option<String> = None;
        let mut dir = PathBuf::new();
        for seg in rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
        {
            if is_legacy_year(&seg) {
                // Legacy year-organized exports predate folders:
                // their files are root entries, never year folders.
                return None;
            }
            dir.push(&seg);
            if let Some(known) = dir_folders.get(&dir) {
                parent = Some(known.clone());
                continue;
            }
            let name = strip_id_suffix(&seg);
            let id = match self.create_folder(parent.as_deref(), &name) {
                Ok(folder) => folder.id,
                Err(Error::FolderExists) => match self.find_sibling(parent.as_deref(), &name) {
                    Some(id) => id,
                    // Lost the race or a broken graph: the file
                    // lands under the nearest usable ancestor.
                    None => break,
                },
                // Invalid names and over-deep chains keep the
                // nearest usable ancestor instead of the root.
                Err(_) => break,
            };
            dir_folders.insert(dir.clone(), id.clone());
            parent = Some(id);
        }
        parent
    }

    /// The live child of `parent` with this display name, for the
    /// import's duplicate-sibling fallback.
    fn find_sibling(&self, parent: Option<&str>, name: &str) -> Option<String> {
        let key = crate::entries::normalize_key(&name.trim().to_lowercase());
        self.db
            .connection()
            .query_row(
                "SELECT id FROM folder
                  WHERE ((parent_id = ?1) OR (parent_id IS NULL AND ?1 IS NULL))
                    AND name_key = ?2 AND deleted_at IS NULL
                  ORDER BY id LIMIT 1",
                rusqlite::params![parent, key],
                |row| row.get(0),
            )
            .ok()
    }

    /// Writes one planned Markdown file: attachments first (their
    /// ids rewrite the body), then the entry itself in its planned
    /// folder. The body already carries remapped entry links.
    fn insert_planned_markdown(
        &mut self,
        root: &Path,
        plan: &PlannedFile,
    ) -> Result<(usize, usize)> {
        let file = &plan.file;
        let fm = &plan.fm;
        let mut body = plan.body.clone();

        // The entry date: filename prefix first (our export
        // names files YYYY-MM-DD-…), then the front-matter
        // creation date, then the file's modification time.
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let entry_date = date_prefix(&stem)
            .or_else(|| fm.created.map(|ms| date_of_ms(ms)))
            .or_else(|| {
                fs::metadata(file)
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| date_of_ms(d.as_millis() as i64))
            })
            .ok_or_else(|| Error::InvalidInput("no date for file".into()))?;

        let title = fm
            .title
            .clone()
            .or_else(|| first_heading(&body))
            .filter(|t| !t.trim().is_empty());

        // Attachments first (their ids rewrite the body), then
        // the entry itself.
        let mut skipped_attachments = 0;
        let mut attachments: Vec<(String, String, String, Vec<u8>)> = Vec::new();
        for rel in asset_refs(&body) {
            // No blanket `..` rejection: nested exports legitimately
            // climb to the root `assets/` dir, and `contained_file`
            // already refuses anything that resolves outside it.
            let md_dir = file.parent().unwrap_or(root);
            let path = [md_dir.join(&rel), root.join(&rel)]
                .into_iter()
                .find(|p| contained_file(root, p));
            let Some(path) = path else {
                skipped_attachments += 1;
                continue;
            };
            let Ok(bytes) = fs::read(&path) else {
                skipped_attachments += 1;
                continue;
            };
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (old_id, name) = match uuid_prefix(&file_name) {
                Some((uuid, rest)) => (Some(uuid.to_ascii_lowercase()), rest.to_string()),
                None => (None, file_name.clone()),
            };
            let att_id = match old_id {
                Some(id) if !self.attachment_exists(&id)? => id,
                _ => Uuid::now_v7().to_string(),
            };
            let mime = sniff_mime(&bytes)
                .map(str::to_string)
                .or_else(|| mime_by_extension(&name))
                .unwrap_or_else(|| "application/octet-stream".to_string());
            body = body.replace(&rel, &format!("attachment:{att_id}"));
            attachments.push((att_id, name, mime, bytes));
        }

        let now = now_ms();
        let (created, updated) = match (fm.created, fm.updated) {
            (Some(c), Some(u)) => (c, u),
            (Some(c), None) => (c, c),
            _ => (now, now),
        };
        let draft = EntryDraft {
            entry_date,
            title,
            body_md: body.trim_start_matches('\n').to_string(),
            mood: fm.mood,
            tz: None,
            tags: fm.tags.clone(),
        };
        let entry_id = &plan.entry_id;
        // The planned folder was resolved against live folders;
        // re-check liveness here so a concurrent tombstone cannot
        // file the entry under a dead folder.
        let folder = plan.folder.as_deref().filter(|id| self.folder_is_live(id));
        self.insert_entry_exact(entry_id, &draft, created, updated, None, folder)?;
        let mut imported = 0;
        for (att_id, name, mime, bytes) in &attachments {
            let sha = Sha256::digest(bytes);
            match self.insert_attachment_exact(
                att_id,
                &entry_id,
                name,
                mime,
                bytes.len() as i64,
                &sha,
                created,
                None,
                Some(bytes),
            ) {
                Ok(()) => imported += 1,
                Err(_) => skipped_attachments += 1,
            }
        }
        Ok((imported, skipped_attachments))
    }

    /// Imports a SoirVu JSON export (see `export_json`).
    /// Entries and attachments keep their ids; anything whose
    /// id is already taken is skipped, never overwritten.
    /// Version 1 has no folders (everything lands at the root);
    /// version 2 carries folders — including scrubbed tombstones —
    /// and per-entry membership. Folders arrive before entries so
    /// membership resolves; a taken folder id maps to a fresh id
    /// (entries follow the map), while taken entry ids are skipped.
    /// Entry `entry:` links are never rewritten here: entry ids
    /// are never remapped, so bodies stay byte-identical.
    pub fn import_json(&mut self, file: &Path) -> Result<ImportReport> {
        let text = fs::read_to_string(file)?;
        let data: JsonImport = serde_json::from_str(&text)
            .map_err(|e| Error::InvalidInput(format!("not a SoirVu JSON export: {e}")))?;
        if data.format != "soirvu-json-export" || (data.version != 1 && data.version != 2) {
            return Err(Error::InvalidInput(
                "not a SoirVu JSON export (bad format or version)".into(),
            ));
        }
        let mut report = ImportReport {
            entries: 0,
            attachments: 0,
            skipped_entries: 0,
            skipped_attachments: 0,
            skipped_folders: 0,
            folders: 0,
            links_remapped: 0,
            links_unresolved: 0,
        };
        // Folders first, in file order (exports are oldest-first,
        // so parents usually precede children; raw references
        // tolerate the rest).
        let mut manifest_folders: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let device = self.device_id.clone();
        for folder in &data.folders {
            if !crate::storage::is_canonical_uuid(&folder.id) {
                report.skipped_folders += 1;
                continue;
            }
            if self.folder_is_live(&folder.id) {
                manifest_folders.insert(folder.id.clone(), folder.id.clone());
                report.folders += 1;
                continue;
            }
            let name = folder.name.clone().unwrap_or_default();
            let parent = folder.parent_id.as_deref().map(|p| {
                manifest_folders
                    .get(p)
                    .cloned()
                    .unwrap_or_else(|| p.to_string())
            });
            let now = now_ms();
            let mut final_id = folder.id.clone();
            let inserted = self
                .insert_folder_exact(
                    &final_id,
                    parent.as_deref(),
                    &name,
                    folder.created_at,
                    folder.updated_at,
                    folder.deleted_at,
                    folder.rev.max(1),
                    &folder.device_id,
                    folder.base_rev.max(0),
                    &folder.base_device,
                )
                .is_ok();
            if !inserted {
                // Taken id or rejected name: a fresh id keeps the
                // hierarchy importable; entries follow the map.
                final_id = Uuid::now_v7().to_string();
                if self
                    .insert_folder_exact(
                        &final_id,
                        parent.as_deref(),
                        &name,
                        now,
                        now,
                        folder.deleted_at,
                        1,
                        &device,
                        0,
                        "",
                    )
                    .is_err()
                {
                    report.skipped_folders += 1;
                    continue;
                }
            }
            manifest_folders.insert(folder.id.clone(), final_id);
            report.folders += 1;
        }
        for entry in &data.entries {
            // Untrusted ids must be canonical UUIDs, like folder ids
            // above: downstream code slices `id[..8]` for export
            // file names, which panics on short or multibyte ids.
            if !crate::storage::is_canonical_uuid(&entry.id) {
                report.skipped_entries += 1;
                continue;
            }
            if self.entry_exists(&entry.id)? {
                report.skipped_entries += 1;
                continue;
            }
            // Membership follows the folder map; an unknown folder
            // stays raw (the sync-accepted form) rather than
            // silently rooting the entry.
            let folder = entry.folder_id.as_deref().map(|f| {
                manifest_folders
                    .get(f)
                    .cloned()
                    .unwrap_or_else(|| f.to_string())
            });
            let draft = EntryDraft {
                entry_date: entry.entry_date.clone(),
                title: entry.title.clone(),
                body_md: entry.body_md.clone(),
                mood: entry.mood,
                tz: entry.tz.clone(),
                tags: entry.tags.clone(),
            };
            match self.insert_entry_exact(
                &entry.id,
                &draft,
                entry.created_at,
                entry.updated_at,
                entry.deleted_at,
                folder.as_deref(),
            ) {
                Ok(()) => report.entries += 1,
                Err(_) => report.skipped_entries += 1,
            }
        }
        for att in &data.attachments {
            let skip = !crate::storage::is_canonical_uuid(&att.id)
                || self.attachment_exists(&att.id)?
                || self.entry_exists(&att.entry_id).map(|e| !e).unwrap_or(true);
            if skip {
                report.skipped_attachments += 1;
                continue;
            }
            let tombstone = att.deleted_at.is_some();
            let data_bytes = if tombstone {
                None
            } else {
                match att
                    .data_base64
                    .as_deref()
                    .and_then(|b| data_encoding::BASE64.decode(b.as_bytes()).ok())
                {
                    Some(bytes)
                        if data_encoding::HEXLOWER.encode(&Sha256::digest(&bytes))
                            == att.sha256 =>
                    {
                        Some(bytes)
                    }
                    _ => {
                        report.skipped_attachments += 1;
                        continue;
                    }
                }
            };
            let sha = data_encoding::HEXLOWER
                .decode(att.sha256.as_bytes())
                .unwrap_or_default();
            match self.insert_attachment_exact(
                &att.id,
                &att.entry_id,
                &att.name,
                &att.mime,
                att.size,
                &sha,
                att.created_at,
                att.deleted_at,
                data_bytes.as_deref(),
            ) {
                Ok(()) => report.attachments += 1,
                Err(_) => report.skipped_attachments += 1,
            }
        }
        Ok(report)
    }

    fn entry_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.db.connection().query_row(
            "SELECT COUNT(*) FROM entry WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    fn attachment_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.db.connection().query_row(
            "SELECT COUNT(*) FROM attachment WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// One parsed Markdown file awaiting the second pass: its final
/// entry id and resolved live folder (or `None` for root).
struct PlannedFile {
    file: PathBuf,
    fm: FrontMatter,
    body: String,
    folder: Option<String>,
    entry_id: String,
}

/// Our own export's root map, when the imported directory is one:
/// exact folder/entry placement instead of layout guessing.
#[derive(Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    folders: Vec<ManifestFolder>,
}

#[derive(Deserialize)]
struct ManifestFolder {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    updated_at: i64,
}

/// Reads the export manifest when the imported directory is our
/// own export. Unknown versions are ignored (layout guessing
/// takes over) rather than failing the whole import.
fn load_manifest(dir: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(dir.join("soirvu-export.json")).ok()?;
    let manifest: Manifest = serde_json::from_str(&text).ok()?;
    if manifest.format != "soirvu-markdown-export" || manifest.version != 2 {
        return None;
    }
    if manifest
        .folders
        .iter()
        .any(|f| !crate::storage::is_canonical_uuid(&f.id))
    {
        return None;
    }
    Some(manifest)
}

/// `2026`-style segments are legacy year layout (our pre-folder
/// exports grouped files that way), never folder names.
fn is_legacy_year(seg: &str) -> bool {
    seg.len() == 4 && seg.bytes().all(|b| b.is_ascii_digit())
}

/// Strips our stable export suffix (`<slug>-<8 hex id>`) back to
/// the display name for directory-derived folders. A segment
/// without that exact shape is kept whole; a foreign directory
/// that happens to share the shape loses its trailing suffix
/// (documented in the module docs).
fn strip_id_suffix(seg: &str) -> String {
    // Byte offsets are not char boundaries in multibyte text, so
    // every cut below is checked: a foreign name only loses its
    // tail when the tail is exactly `-` plus 8 ASCII hex digits.
    let Some(split) = seg.len().checked_sub(9) else {
        return seg.to_string();
    };
    let (Some(head), Some(tail)) = (seg.get(..split), seg.get(split..)) else {
        return seg.to_string();
    };
    if let Some(hex) = tail.strip_prefix('-')
        && hex.len() == 8
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
        && !head.is_empty()
    {
        return head.to_string();
    }
    seg.to_string()
}

/// Distinct `entry:<canonical uuid>` targets in a body, in order.
/// Textual, like the remap: code samples are included, and the
/// caller counts the unresolved rather than parsing Markdown.
fn entry_link_targets(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find("entry:") {
        let after = &rest[pos + 6..];
        // `get` fails when byte 36 lands inside a multibyte char;
        // such a prefix cannot be the 36 ASCII bytes of a UUID, so
        // it scans on exactly like any other non-link text.
        let linked = after.get(..36).filter(|head| Uuid::parse_str(head).is_ok());
        if let Some(head) = linked {
            let id = head.to_string();
            if !out.contains(&id) {
                out.push(id);
            }
            rest = &after[36..];
        } else {
            rest = after;
        }
    }
    out
}

/// A path is usable when it is a real file under `root`: the final
/// component must not be a symlink, no intermediate component may
/// be one either (a symlinked directory would otherwise redirect
/// resolution outside `root` while a lexical check still passes),
/// and the resolved parent must stay inside the resolved root.
/// Both sides resolve, so a symlinked root (macOS `/tmp`)
/// compares like with like. Anything unresolvable is unusable.
fn contained_file(root: &Path, path: &Path) -> bool {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return false;
    }
    if !path.is_file() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let (Ok(canonical_root), Ok(canonical_parent)) = (root.canonicalize(), parent.canonicalize())
    else {
        return false;
    };
    canonical_parent.starts_with(canonical_root)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(|e| e.ok()) {
        // Never follow symlinks: a linked file outside the root
        // is not part of the import, and a dir loop must not hang
        // the walk.
        let kind = entry
            .file_type()
            .map(|t| (t.is_symlink(), t.is_dir(), t.is_file()));
        let Ok((symlink, is_dir, is_file)) = kind else {
            continue;
        };
        if symlink {
            continue;
        }
        let path = entry.path();
        if is_dir {
            collect_markdown(&path, out);
        } else if is_file
            && path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// `YYYY-MM-DD` at the start of a file stem, when it is a real date.
fn date_prefix(stem: &str) -> Option<String> {
    let b = stem.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    // Byte 10 can sit inside a multibyte char even when bytes 4 and
    // 7 are dashes; `get` turns that into a miss, not a panic.
    let candidate = stem.get(..10)?;
    if !candidate.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return None;
    }
    valid_date(candidate).then(|| candidate.to_string())
}

fn valid_date(s: &str) -> bool {
    let parts: Vec<i64> = s.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return false;
    }
    let (y, m, d) = (parts[0], parts[1] as u32, parts[2] as u32);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return false;
    }
    let days = days_from_civil(y, m, d);
    civil_from_days(days) == (y, m, d)
}

fn date_of_ms(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Inverse of `civil_from_days` (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let (m, d) = (i64::from(m), i64::from(d));
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// `2026-10-15T07:12:39Z` (the export's `iso_utc`) or a bare
/// `2026-10-15`, to epoch milliseconds.
fn parse_iso(text: &str) -> Option<i64> {
    let text = text.trim().trim_matches('"');
    // Checked cuts: multibyte frontmatter values used to panic on
    // the byte offsets below. A cut that fails — or a time part
    // that does not parse — rejects the value like any other
    // malformed date instead of crashing the import.
    let prefix = text.get(..10)?;
    if !valid_date(prefix) {
        return None;
    }
    let days = days_from_civil(
        text[..4].parse().ok()?,
        text[5..7].parse().ok()?,
        text[8..10].parse().ok()?,
    );
    let mut secs = days * 86_400;
    if text.len() >= 19 && text.as_bytes()[13] == b':' {
        let (Some(h), Some(m), Some(s)) = (text.get(11..13), text.get(14..16), text.get(17..19))
        else {
            return None;
        };
        let h: i64 = h.parse().ok()?;
        let m: i64 = m.parse().ok()?;
        let s: i64 = s.parse().ok()?;
        secs += h * 3600 + m * 60 + s;
    }
    Some(secs * 1000)
}

// ---------------------------------------------------------------------------
// Front matter and body scanning
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FrontMatter {
    id: Option<String>,
    folder: Option<String>,
    title: Option<String>,
    created: Option<i64>,
    updated: Option<i64>,
    tags: Vec<String>,
    mood: Option<i64>,
}

/// Splits a leading `---` block (the export's shape) off the
/// body. No front matter = everything is body.
fn split_front_matter(text: &str) -> (FrontMatter, String) {
    let mut fm = FrontMatter::default();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !text.starts_with("---\n") && !text.starts_with("---\r\n") {
        return (fm, text.to_string());
    }
    let rest = text[3..]
        .strip_prefix("\r\n")
        .or(text[3..].strip_prefix('\n'))
        .unwrap_or(&text[3..]);
    let mut offset = 0usize;
    let mut closed = false;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            offset += line.len();
            closed = true;
            break;
        }
        offset += line.len();
        parse_fm_line(&mut fm, trimmed);
    }
    if !closed {
        return (FrontMatter::default(), text.to_string());
    }
    (fm, rest[offset..].to_string())
}

fn parse_fm_line(fm: &mut FrontMatter, line: &str) {
    let Some((key, value)) = line.split_once(':') else {
        return;
    };
    let value = value.trim();
    match key.trim() {
        "id" => fm.id = Some(value.trim_matches('"').to_string()),
        "folder" => fm.folder = Some(value.trim_matches('"').to_string()),
        "title" => fm.title = Some(unquote(value)),
        "created" => fm.created = parse_iso(value),
        "updated" => fm.updated = parse_iso(value),
        "mood" => fm.mood = value.parse().ok(),
        "tags" => fm.tags = parse_tags(value),
        _ => {}
    }
}

/// The export's YAML double-quoted strings escape only
/// backslash and quote; newlines were folded to spaces.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some(next) => out.push(next),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        value.to_string()
    }
}

fn parse_tags(value: &str) -> Vec<String> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    if value.trim().is_empty() {
        return Vec::new();
    }
    // Split on commas that are outside quotes.
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '\\' if quoted => {
                cur.push(c);
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ',' if !quoted => {
                out.push(unquote(cur.trim()));
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(unquote(cur.trim()));
    }
    out
}

fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|t| t.trim().to_string()))
}

/// `assets/…` destinations of links and images, deduplicated. The
/// exporter writes `../assets/…` for nested entries, so leading
/// `../` segments are accepted here: resolution against the file's
/// own directory plus the root-containment check decide whether
/// such a path is usable (escapes still count as skipped).
/// Anything else — `entry:` targets, URLs, sibling notes — is not
/// an asset and is left alone.
fn asset_refs(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find("](") {
        let after = &rest[pos + 2..];
        let Some(end) = after.find(')') else { break };
        let dest = after[..end].split_whitespace().next().unwrap_or("");
        if is_asset_dest(dest) && !out.iter().any(|d| d == dest) {
            out.push(dest.to_string());
        }
        rest = &after[end..];
    }
    out
}

/// An asset destination: `assets/…`, optionally reached through
/// leading `../` segments (the nested export layout).
fn is_asset_dest(dest: &str) -> bool {
    let mut rest = dest;
    while let Some(stripped) = rest.strip_prefix("../") {
        rest = stripped;
    }
    rest.starts_with("assets/")
}

/// Splits `<uuid>-<name>` (the export's asset file names).
fn uuid_prefix(file_name: &str) -> Option<(&str, &str)> {
    let b = file_name.as_bytes();
    if b.len() > 37
        && b[8] == b'-'
        && b[13] == b'-'
        && b[18] == b'-'
        && b[23] == b'-'
        && b[36] == b'-'
        && Uuid::parse_str(&file_name[..36]).is_ok()
    {
        Some((&file_name[..36], &file_name[37..]))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// MIME sniffing: the same magic-byte rules the bridge applies
// to interactive attachments (§6.4), so an imported file gets
// the type the interactive path would have given it.
// ---------------------------------------------------------------------------

fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.len() >= magic.len() && &bytes[..magic.len()] == magic;
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if starts(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if starts(b"GIF8") {
        return Some("image/gif");
    }
    if starts(b"%PDF-") {
        return Some("application/pdf");
    }
    if starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) {
        return Some("audio/mpeg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
        match &bytes[8..12] {
            b"WEBP" => return Some("image/webp"),
            b"WAVE" => return Some("audio/wav"),
            _ => {}
        }
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return match brand {
            b"M4A " | b"M4B " => Some("audio/x-m4a"),
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" => Some("image/heic"),
            b"qt  " => Some("video/quicktime"),
            _ => Some("video/mp4"),
        };
    }
    None
}

fn mime_by_extension(name: &str) -> Option<String> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(
        match ext.as_str() {
            "txt" | "md" => "text/plain",
            "json" => "application/json",
            "csv" => "text/csv",
            _ => return None,
        }
        .to_string(),
    )
}

// ---------------------------------------------------------------------------
// JSON mirror of the export format
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonImport {
    format: String,
    version: u32,
    #[serde(default)]
    folders: Vec<JsonFolderIn>,
    entries: Vec<JsonEntryIn>,
    attachments: Vec<JsonAttachmentIn>,
}

#[derive(Deserialize)]
struct JsonFolderIn {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    updated_at: i64,
    #[serde(default)]
    deleted_at: Option<i64>,
    #[serde(default = "one")]
    rev: i64,
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    base_rev: i64,
    #[serde(default)]
    base_device: String,
}

fn one() -> i64 {
    1
}

#[derive(Deserialize)]
struct JsonEntryIn {
    id: String,
    entry_date: String,
    created_at: i64,
    updated_at: i64,
    title: Option<String>,
    #[serde(default)]
    body_md: String,
    mood: Option<i64>,
    tz: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    deleted_at: Option<i64>,
    #[serde(default)]
    folder_id: Option<String>,
}

#[derive(Deserialize)]
struct JsonAttachmentIn {
    id: String,
    entry_id: String,
    name: String,
    mime: String,
    size: i64,
    sha256: String,
    created_at: i64,
    deleted_at: Option<i64>,
    data_base64: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::entries::{EntryDraft, EntryScope};

    fn fast() -> KdfParams {
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        }
    }

    fn new_vault(tmp: &TempDir) -> Vault {
        let (vault, _recovery) =
            Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
        vault
    }

    use tempfile::TempDir;

    #[test]
    fn markdown_round_trip_restores_entries_and_attachments() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let mut d = EntryDraft {
            entry_date: "2026-10-15".into(),
            title: Some("round trip".into()),
            body_md: "with a file".into(),
            ..Default::default()
        };
        d.tags = vec!["work".into()];
        let entry = source.create_entry(None, &d).unwrap();
        let att = source
            .add_attachment(&entry.id, "note.txt", "text/plain", b"attachment bytes")
            .unwrap();
        // The body references the attachment the way the editor stores it.
        let mut d2 = d.clone();
        d2.body_md = format!("look: [note](attachment:{})", att.id);
        source.update_entry(&entry.id, source.get_entry(&entry.id).unwrap().rev, &d2).unwrap();

        let out = tmp.path().join("export");
        source.export_markdown(&out).unwrap();
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut target = new_vault(&tmp2);
        let report = target.import_markdown(&out).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.attachments, 1);
        assert_eq!(report.skipped_entries, 0);
        let page = target
            .list_entries(&crate::entries::ListQuery {
                scope: EntryScope::All,
                limit: 10,
                after: None,
                tag: None,
                date: None,
            })
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let got = target.get_entry(&page.items[0].id).unwrap();
        assert_eq!(got.id, entry.id); // front-matter id preserved
        assert_eq!(got.title.as_deref(), Some("round trip"));
        assert_eq!(got.tags, vec!["work".to_string()]);
        let atts = target.list_attachments(&got.id).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].id, att.id); // asset filename id preserved
        assert_eq!(atts[0].name, "note.txt");
        assert_eq!(
            target.read_attachment(&atts[0].id).unwrap(),
            b"attachment bytes"
        );
        assert!(got.body_md.contains(&format!("attachment:{}", att.id)));
    }

    /// P2: the exporter writes `../assets/…` destinations for
    /// nested entries, but the importer only recognized `](assets/`
    /// and rejected every `..` segment — so a nested roundtrip
    /// imported zero attachments and skipped zero. Relative asset
    /// paths must resolve within the export root.
    #[test]
    fn nested_entry_attachments_survive_a_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let dev = source.create_folder(None, "Dev").unwrap();
        let inner = source.create_folder(Some(&dev.id), "inner").unwrap();
        let d = EntryDraft {
            entry_date: "2026-10-15".into(),
            title: Some("nested".into()),
            body_md: "with a file".into(),
            ..Default::default()
        };
        let entry = source.create_entry(Some(&inner.id), &d).unwrap();
        let att = source
            .add_attachment(&entry.id, "note.txt", "text/plain", b"attachment bytes")
            .unwrap();
        let mut d2 = d.clone();
        d2.body_md = format!("look: [note](attachment:{})", att.id);
        source
            .update_entry(&entry.id, source.get_entry(&entry.id).unwrap().rev, &d2)
            .unwrap();

        let out = tmp.path().join("export");
        source.export_markdown(&out).unwrap();
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut target = new_vault(&tmp2);
        let report = target.import_markdown(&out).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.attachments, 1);
        assert_eq!(report.skipped_attachments, 0);
        let page = target
            .list_entries(&crate::entries::ListQuery {
                scope: EntryScope::All,
                limit: 10,
                after: None,
                tag: None,
                date: None,
            })
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let got = target.get_entry(&page.items[0].id).unwrap();
        assert!(got.folder_id.is_some());
        let atts = target.list_attachments(&got.id).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].name, "note.txt");
        assert_eq!(
            target.read_attachment(&atts[0].id).unwrap(),
            b"attachment bytes"
        );
        assert!(got.body_md.contains("attachment:"));
    }

    #[test]
    fn json_round_trip_restores_tombstones_and_skips_duplicates() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let keep = source
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-15".into(),
                    title: Some("keep".into()),
                    body_md: "k".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let gone = source
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-14".into(),
                    title: Some("gone".into()),
                    body_md: "secret".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        source.delete_entry(&gone.id).unwrap();
        let out = tmp.path().join("export.json");
        source.export_json(&out).unwrap();
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut target = new_vault(&tmp2);
        let report = target.import_json(&out).unwrap();
        assert_eq!(report.entries, 2); // the tombstone imports too
        let got = target.get_entry(&keep.id).unwrap();
        assert_eq!(got.title.as_deref(), Some("keep"));
        // Second import: everything is a duplicate, nothing is added.
        let report2 = target.import_json(&out).unwrap();
        assert_eq!(report2.entries, 0);
        assert_eq!(report2.skipped_entries, 2);
    }

    #[test]
    fn plain_markdown_without_front_matter_imports() {
        let tmp = TempDir::new().unwrap();
        let folder = tmp.path().join("notes");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("2026-09-01-first.md"),
            "# A morning\n\nSome text.\n",
        )
        .unwrap();
        let mut vault = new_vault(&tmp);
        let report = vault.import_markdown(&folder).unwrap();
        assert_eq!(report.entries, 1);
        let page = vault
            .list_entries(&crate::entries::ListQuery {
                scope: EntryScope::All,
                limit: 10,
                after: None,
                tag: None,
                date: None,
            })
            .unwrap();
        let got = vault.get_entry(&page.items[0].id).unwrap();
        assert_eq!(got.entry_date, "2026-09-01");
        assert_eq!(got.title.as_deref(), Some("A morning"));
    }

    fn all_entries(vault: &Vault) -> Vec<crate::entries::EntrySummary> {
        vault
            .list_entries(&crate::entries::ListQuery {
                scope: EntryScope::All,
                limit: 200,
                after: None,
                tag: None,
                date: None,
            })
            .unwrap()
            .items
    }

    #[test]
    fn markdown_roundtrip_preserves_folders_and_forward_links() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let health = source.create_folder(None, "Health").unwrap();
        let target = source
            .create_entry(
                Some(&health.id),
                &EntryDraft {
                    entry_date: "2026-10-30".into(),
                    title: Some("Training notes".into()),
                    body_md: "lift heavy".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        // The summary sorts before the folder file, so the link
        // is a forward reference at import time.
        let summary = source
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-31".into(),
                    title: Some("Summary".into()),
                    body_md: format!("see [notes](entry:{})", target.id),
                    ..Default::default()
                },
            )
            .unwrap();
        let out = tmp.path().join("export");
        source.export_markdown(&out).unwrap();
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp2);
        let report = vault.import_markdown(&out).unwrap();
        assert_eq!(report.entries, 2);
        assert_eq!(report.folders, 1);
        assert_eq!(report.links_unresolved, 0);
        // Ids survive a fresh import, so the link needs no remap.
        let got = vault.get_entry(&summary.id).unwrap();
        assert_eq!(got.folder_id, None);
        assert!(got.body_md.contains(&format!("entry:{}", target.id)));
        let notes = vault.get_entry(&target.id).unwrap();
        assert_eq!(notes.folder_id.as_deref(), Some(health.id.as_str()));
        assert_eq!(notes.title.as_deref(), Some("Training notes"));
        let folders = vault.list_folders().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Health");
    }

    #[test]
    fn legacy_year_layout_imports_to_root_without_year_folders() {
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        fs::create_dir_all(notes.join("2026")).unwrap();
        fs::write(
            notes.join("2026").join("2026-01-01-old.md"),
            "# Old\n\nLegacy text.\n",
        )
        .unwrap();
        let mut vault = new_vault(&tmp);
        let report = vault.import_markdown(&notes).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.folders, 0);
        assert!(vault.list_folders().unwrap().is_empty());
        let items = all_entries(&vault);
        assert_eq!(items.len(), 1);
        assert_eq!(vault.get_entry(&items[0].id).unwrap().folder_id, None);
    }

    #[test]
    fn subdirectory_without_manifest_becomes_folders() {
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        fs::create_dir_all(notes.join("Work").join("Deep")).unwrap();
        fs::write(
            notes.join("Work").join("Deep").join("2026-02-02-deep.md"),
            "Deep work.\n",
        )
        .unwrap();
        let mut vault = new_vault(&tmp);
        let report = vault.import_markdown(&notes).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.folders, 2);
        let items = all_entries(&vault);
        let got = vault.get_entry(&items[0].id).unwrap();
        let deep = vault.get_folder(got.folder_id.as_deref().unwrap()).unwrap();
        assert_eq!(deep.name, "Deep");
        let work = vault
            .get_folder(deep.parent_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(work.name, "Work");
        assert_eq!(work.parent_id, None);
    }

    #[test]
    fn duplicate_import_remaps_ids_and_rewrites_links() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let target = source
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-30".into(),
                    title: Some("Target".into()),
                    body_md: "target text".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        source
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-31".into(),
                    title: Some("Source".into()),
                    body_md: format!("link [t](entry:{}) here", target.id),
                    ..Default::default()
                },
            )
            .unwrap();
        let out = tmp.path().join("export");
        source.export_markdown(&out).unwrap();
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp2);
        let first = vault.import_markdown(&out).unwrap();
        assert_eq!(first.links_remapped, 0);
        // Every id is taken now: the second import remaps both
        // entries and rewrites the source body to the new target.
        let second = vault.import_markdown(&out).unwrap();
        assert_eq!(second.entries, 2);
        assert!(second.links_remapped >= 1, "{second:?}");
        assert_eq!(second.links_unresolved, 0);
        let items = all_entries(&vault);
        assert_eq!(items.len(), 4);
        let sources: Vec<_> = items
            .iter()
            .filter(|i| i.title.as_deref() == Some("Source"))
            .collect();
        assert_eq!(sources.len(), 2);
        let bodies: Vec<String> = sources
            .iter()
            .map(|s| vault.get_entry(&s.id).unwrap().body_md)
            .collect();
        assert!(
            bodies
                .iter()
                .any(|b| b.contains(&format!("entry:{}", target.id)))
        );
        assert!(
            bodies
                .iter()
                .any(|b| !b.contains(&format!("entry:{}", target.id)) && b.contains("entry:"))
        );
        let targets: Vec<_> = items
            .iter()
            .filter(|i| i.title.as_deref() == Some("Target"))
            .collect();
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|t| t.id != target.id));
    }

    #[test]
    fn json_v2_roundtrip_keeps_folders_membership_and_tombstones() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let health = source.create_folder(None, "Health").unwrap();
        let doomed = source.create_folder(None, "Doomed").unwrap();
        source.delete_folder(&doomed.id, 1).unwrap();
        let entry = source
            .create_entry(
                Some(&health.id),
                &EntryDraft {
                    entry_date: "2026-10-30".into(),
                    title: Some("Filed".into()),
                    body_md: "in health".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let out = tmp.path().join("export.json");
        source.export_json(&out).unwrap();
        // Tombstones are scrubbed: the deleted name never reaches
        // the plaintext file.
        let text = fs::read_to_string(&out).unwrap();
        assert!(!text.contains("Doomed"));
        drop(source);

        let tmp2 = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp2);
        let report = vault.import_json(&out).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.folders, 2);
        assert_eq!(
            vault.get_entry(&entry.id).unwrap().folder_id.as_deref(),
            Some(health.id.as_str())
        );
        assert_eq!(vault.get_folder(&health.id).unwrap().name, "Health");
        assert!(vault.get_folder(&doomed.id).is_err());
        // …but the tombstone row survives, so a later sync still
        // knows the folder was deleted.
        let deleted: Option<i64> = vault
            .database()
            .connection()
            .query_row(
                "SELECT deleted_at FROM folder WHERE id = ?1",
                [&doomed.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deleted.is_some());
    }

    #[test]
    fn json_v1_still_imports_as_root() {
        let tmp = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp);
        let id = "01900000-0000-7000-8000-000000000001";
        let file = tmp.path().join("v1.json");
        fs::write(
            &file,
            format!(
                r#"{{"format":"soirvu-json-export","version":1,"entries":[{{"id":"{id}","entry_date":"2026-10-01","created_at":1,"updated_at":1,"title":"Old","body_md":"text","tags":[],"deleted_at":null}}],"attachments":[]}}"#
            ),
        )
        .unwrap();
        let report = vault.import_json(&file).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.folders, 0);
        assert_eq!(vault.get_entry(id).unwrap().folder_id, None);
    }

    /// Entry ids from untrusted JSON need the same canonical-UUID
    /// gate as folder ids: a short or multibyte id would otherwise
    /// be stored and later panic the markdown export's `id[..8]`.
    /// Skipped entries count, like skipped folders.
    #[test]
    fn json_import_skips_non_canonical_entry_ids() {
        let tmp = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp);
        let good = "01900000-0000-7000-8000-000000000001";
        let file = tmp.path().join("bad-ids.json");
        fs::write(
            &file,
            format!(
                r#"{{"format":"soirvu-json-export","version":1,"entries":[{{"id":"é","entry_date":"2026-10-01","created_at":1,"updated_at":1,"title":"Bad","body_md":"text","tags":[],"deleted_at":null}},{{"id":"{good}","entry_date":"2026-10-01","created_at":1,"updated_at":1,"title":"Good","body_md":"text","tags":[],"deleted_at":null}}],"attachments":[]}}"#
            ),
        )
        .unwrap();
        let report = vault.import_json(&file).unwrap();
        assert_eq!(report.entries, 1);
        assert_eq!(report.skipped_entries, 1);
        assert_eq!(
            vault.get_entry(good).unwrap().title.as_deref(),
            Some("Good")
        );
    }

    #[test]
    fn json_import_skips_noncanonical_attachment_ids() {
        let tmp = TempDir::new().unwrap();
        let mut vault = new_vault(&tmp);
        let entry = vault
            .create_entry(
                None,
                &EntryDraft {
                    entry_date: "2026-10-01".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let canonical = "01900000-0000-7000-8000-00000000000a";
        let attachment = |id: &str, deleted_at: Option<i64>| {
            serde_json::json!({
                "id": id,
                "entry_id": entry.id,
                "name": "note.txt",
                "mime": "text/plain",
                "size": 4,
                "sha256": data_encoding::HEXLOWER.encode(&Sha256::digest(b"note")),
                "created_at": 1,
                "deleted_at": deleted_at,
                "data_base64": deleted_at.is_none().then(|| data_encoding::BASE64.encode(b"note")),
            })
        };
        let file = tmp.path().join("attachment-ids.json");
        let export = serde_json::json!({
            "format": "soirvu-json-export",
            "version": 2,
            "entries": [],
            "attachments": [
                attachment(&canonical.to_ascii_uppercase(), None),
                attachment("legacy-id", Some(2)),
                attachment(canonical, None),
            ],
        });
        fs::write(&file, serde_json::to_vec(&export).unwrap()).unwrap();

        let report = vault.import_json(&file).unwrap();
        assert_eq!(report.skipped_attachments, 2);
        assert_eq!(report.attachments, 1);
        assert!(!vault.attachment_exists("legacy-id").unwrap());
        assert_eq!(vault.read_attachment(canonical).unwrap(), b"note");
    }

    /// P2: byte-offset slicing panicked on multibyte input. Every
    /// case below panicked before checked slicing: a folder name,
    /// a link scan, a file stem and frontmatter dates.
    #[test]
    fn multibyte_input_never_panics_string_slicing() {
        assert_eq!(strip_id_suffix("розробка"), "розробка");
        assert_eq!(strip_id_suffix("розробка-12345678"), "розробка");
        assert_eq!(strip_id_suffix("розробка-не-суфікс"), "розробка-не-суфікс");
        // Emoji, short names and the exact suffix boundary: the
        // tail only strips on `-` plus 8 ASCII hex digits.
        assert_eq!(strip_id_suffix("🎉-12345678"), "🎉");
        assert_eq!(strip_id_suffix("🎉-not-a-suffix"), "🎉-not-a-suffix");
        assert_eq!(strip_id_suffix("short"), "short");
        assert_eq!(strip_id_suffix("name-1234abcd"), "name");
        assert_eq!(strip_id_suffix("name-1234abc"), "name-1234abc");
        let body = format!("entry:{}é", "x".repeat(35));
        assert!(entry_link_targets(&body).is_empty());
        // A bad prefix scans on: the later valid target still counts.
        let valid = "01900000-0000-7000-8000-000000000001";
        let mixed = format!("entry:{}é then entry:{valid}!", "x".repeat(35));
        assert_eq!(entry_link_targets(&mixed), vec![valid.to_string()]);
        assert_eq!(date_prefix("2026-12-3éx"), None);
        assert_eq!(date_prefix("2026"), None);
        assert_eq!(parse_iso("\"abcdefghié\""), None);
        assert_eq!(parse_iso("2026-10-15T12:Xéyz"), None);
        assert_eq!(parse_iso("2026"), None);
        // The dash positions prove byte 36 is a boundary, so the
        // UUID parse below cannot panic even with multibyte text
        // in the prefix: it just misses.
        assert_eq!(
            uuid_prefix("12345678-1234-1234-1234-123456789012-x"),
            Some(("12345678-1234-1234-1234-123456789012", "x"))
        );
        assert_eq!(uuid_prefix("12345678-1234-1234-1234-1234567890é-x"), None);
        // Well-formed values keep working through the same paths.
        assert_eq!(
            entry_link_targets("see entry:0190a1b2-7c3d-7abc-8def-0123456789ab!"),
            vec!["0190a1b2-7c3d-7abc-8def-0123456789ab".to_string()]
        );
        assert_eq!(date_prefix("2026-12-31-note"), Some("2026-12-31".into()));
        assert!(parse_iso("2026-10-15T07:12:39Z").is_some());
    }

    /// Follow-up: the lexical containment check normalized `..`
    /// without touching the filesystem, so an intermediate symlink
    /// redirected resolution outside the root while the check still
    /// passed. The resolved parent must stay inside the resolved
    /// root now.
    #[cfg(unix)]
    #[test]
    fn intermediate_symlink_directories_are_skipped() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(notes.join("assets")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "not yours\n").unwrap();
        symlink(&outside, notes.join("assets").join("evildir")).unwrap();
        fs::write(
            notes.join("2026-03-03-note.md"),
            "![x](assets/evildir/secret.txt)\n",
        )
        .unwrap();
        let mut vault = new_vault(&tmp);
        let report = vault.import_markdown(&notes).unwrap();
        // The entry imports; the smuggled file does not.
        assert_eq!(report.entries, 1);
        assert_eq!(report.attachments, 0);
        assert_eq!(report.skipped_attachments, 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_files_and_escaping_assets_are_skipped() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let notes = tmp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("2026-03-03-real.md"), "Real.\n").unwrap();
        symlink(notes.join("2026-03-03-real.md"), notes.join("link.md")).unwrap();
        fs::write(
            notes.join("2026-03-04-escape.md"),
            "![x](assets/../../secret)\n",
        )
        .unwrap();
        let mut vault = new_vault(&tmp);
        let report = vault.import_markdown(&notes).unwrap();
        // The symlink is not followed and the escaping asset is
        // not read: two entries, one skipped attachment.
        assert_eq!(report.entries, 2);
        assert_eq!(report.skipped_attachments, 1);
    }
}
