// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Markdown export (DESIGN §8): the whole vault as plain files.
//!
//! ```text
//! out/
//! ├── 2026/2026-10-11-<slug>-<shortid>.md     one file per live entry
//! └── assets/<attachment-id>-<name>           the decrypted attachments
//! ```
//!
//! Export is PLAINTEXT. The caller (the app, the CLI) must warn the user and
//! must already hold an unlocked vault; this module only writes files.
//! Tombstones are not exported (the JSON export, later, is the lossless one
//! that includes them).

use std::fs;
use std::path::{Path, PathBuf};

use crate::entries::{EntryScope, ListQuery};
use crate::error::{Error, Result};
use crate::folder::Vault;

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub entries: usize,
    pub attachments: usize,
    pub folders: usize,
    pub dir: PathBuf,
}

impl Vault {
    /// Writes every live entry as a Markdown file with YAML front matter,
    /// and every attachment as a decrypted file in the top-level `assets/`.
    ///
    /// `out_dir` must be missing or empty — the same rule as creating a
    /// vault — so an export can never overwrite the user's files.
    /// Entries land in logical folder directories (`<slug>-<shortid>`
    /// per level, root entries at the export root); live folders with
    /// no entries still get their directory. `soirvu-export.json`
    /// at the root maps every id to its file, so a later import (or
    /// link-aware tooling) can resolve references without guessing.
    ///
    /// Links in the text are rewritten only where the target is
    /// unambiguous without parsing Markdown: `attachment:<id>`
    /// becomes a depth-relative `assets/<id>-<name>`. Internal
    /// `entry:<id>` links stay canonical: only the existing
    /// Markdown parser can tell a real link from a code sample,
    /// and core adds no Markdown parser (the desktop bridge
    /// rewrites them with that parser; the CLI documents the
    /// limitation instead of corrupting samples).
    pub fn export_markdown(&self, out_dir: &Path) -> Result<ExportReport> {
        prepare_out_dir(out_dir)?;
        let mut report = ExportReport {
            entries: 0,
            attachments: 0,
            folders: 0,
            dir: out_dir.to_path_buf(),
        };

        // Live folders with their export paths, parents first. Local
        // writes are acyclic; synced rows may not be, so the walk is
        // capped and a folder that cannot be placed lands at root
        // rather than looping or vanishing.
        let placed = self.export_folder_paths()?;
        for folder in placed.values() {
            if !folder.segments.is_empty() {
                fs::create_dir_all(out_dir.join(folder.segments.join("/")))?;
                report.folders += 1;
            }
        }

        // Summaries come paged; the full text is then loaded entry by entry.
        let mut ids: Vec<String> = Vec::new();
        let mut after = None;
        loop {
            let page = self.list_entries(&ListQuery {
                scope: EntryScope::All,
                limit: 200,
                after: after.clone(),
                tag: None,
                date: None,
            })?;
            ids.extend(page.items.iter().map(|item| item.id.clone()));
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }

        let mut manifest_entries = Vec::with_capacity(ids.len());
        for id in &ids {
            let entry = self.get_entry(id)?;
            let attachments = self.list_attachments(id)?;
            let dir_rel: PathBuf = placed
                .get(entry.folder_id.as_deref().unwrap_or(""))
                .map(|folder| folder.segments.iter().collect::<PathBuf>())
                .unwrap_or_default();

            let prefix = "../".repeat(dir_rel.components().count());
            let mut body = entry.body_md.clone();
            for att in &attachments {
                let file_name = format!("{}-{}", att.id, export_attachment_name(&att.name));
                let assets = out_dir.join("assets");
                fs::create_dir_all(&assets)?;
                fs::write(assets.join(&file_name), self.read_attachment(&att.id)?)?;
                body = body.replace(
                    &format!("attachment:{}", att.id),
                    &format!("{prefix}assets/{file_name}"),
                );
                report.attachments += 1;
            }

            let dir = if dir_rel.as_os_str().is_empty() {
                out_dir.to_path_buf()
            } else {
                fs::create_dir_all(out_dir.join(&dir_rel))?;
                out_dir.join(&dir_rel)
            };
            let short_id = &entry.id[..8];
            let file_name = format!(
                "{}-{}-{short_id}.md",
                entry.entry_date,
                slug(entry.title.as_deref())
            );
            fs::write(dir.join(&file_name), render_entry(&entry, &body))?;
            manifest_entries.push(ManifestEntry {
                id: entry.id.clone(),
                path: dir_rel
                    .join(&file_name)
                    .to_string_lossy()
                    .replace('\\', "/"),
            });
            report.entries += 1;
        }
        let manifest = ExportManifest {
            format: "soirvu-markdown-export",
            version: 2,
            vault_id: self.header().vault_id.clone(),
            exported_at: crate::storage::now_ms(),
            folders: placed
                .iter()
                .filter(|(_, folder)| !folder.segments.is_empty())
                .map(|(id, folder)| ManifestFolder {
                    id: (*id).to_string(),
                    parent_id: folder.parent_id.clone(),
                    name: folder.name.clone(),
                    created_at: folder.created_at,
                    updated_at: folder.updated_at,
                    path: folder.segments.join("/"),
                })
                .collect(),
            entries: manifest_entries,
        };
        fs::write(
            out_dir.join("soirvu-export.json"),
            serde_json::to_string_pretty(&manifest)
                .map_err(|e| Error::InvalidInput(e.to_string()))?,
        )?;
        Ok(report)
    }

    /// Live folder id to its export row: metadata plus path
    /// segments from the vault root down. Orphaned (parent
    /// tombstoned or missing) and cyclic folders export at the
    /// root: no entry is ever lost to a broken parent chain.
    fn export_folder_paths(&self) -> Result<std::collections::HashMap<String, ExportFolder>> {
        let rows: Vec<(String, Option<String>, String, i64, i64)> = self
            .db
            .connection()
            .prepare(
                "SELECT id, parent_id, name, created_at, updated_at
                   FROM folder WHERE deleted_at IS NULL",
            )?
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let by_id: std::collections::HashMap<&str, &(String, Option<String>, String, i64, i64)> =
            rows.iter().map(|row| (row.0.as_str(), row)).collect();
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for row in &rows {
            let mut segments = Vec::new();
            let mut seen = vec![row.0.as_str()];
            let mut cursor = row.1.as_deref();
            let mut chain = vec![row];
            while let Some(pid) = cursor {
                if seen.contains(&pid) || chain.len() > 32 {
                    chain.clear();
                    break;
                }
                seen.push(pid);
                match by_id.get(pid) {
                    Some(parent) => {
                        chain.push(parent);
                        cursor = parent.1.as_deref();
                    }
                    None => break,
                }
            }
            for link in chain.iter().rev() {
                segments.push(format!("{}-{}", slug(Some(&link.2)), &link.0[..8]));
            }
            out.insert(
                row.0.clone(),
                ExportFolder {
                    parent_id: row.1.clone(),
                    name: row.2.clone(),
                    created_at: row.3,
                    updated_at: row.4,
                    segments,
                },
            );
        }
        Ok(out)
    }
}

/// The export root's map: every folder and entry id to its
/// relative file path. Version 2 is the folder-aware layout;
/// imports (and link-aware tooling) resolve through it instead
/// of guessing from file names or year directories.
#[derive(serde::Serialize)]
struct ExportManifest {
    format: &'static str,
    version: u32,
    vault_id: String,
    exported_at: i64,
    folders: Vec<ManifestFolder>,
    entries: Vec<ManifestEntry>,
}

#[derive(serde::Serialize)]
struct ManifestFolder {
    id: String,
    parent_id: Option<String>,
    name: String,
    created_at: i64,
    updated_at: i64,
    path: String,
}

/// One live folder for the Markdown export: its metadata plus
/// the path segments from the vault root down.
struct ExportFolder {
    parent_id: Option<String>,
    name: String,
    created_at: i64,
    updated_at: i64,
    segments: Vec<String>,
}

#[derive(serde::Serialize)]
struct ManifestEntry {
    id: String,
    path: String,
}

/// One exported file: front matter, then the entry text as stored.
/// `folder` names the entry's folder id (`soirvu-export.json`
/// maps it to a directory); root entries omit it.
fn render_entry(entry: &crate::entries::Entry, body: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("id: {}\n", entry.id));
    if let Some(folder_id) = &entry.folder_id {
        out.push_str(&format!("folder: {folder_id}\n"));
    }
    if let Some(title) = &entry.title {
        out.push_str(&format!("title: {}\n", yaml_string(title)));
    }
    out.push_str(&format!("created: {}\n", iso_utc(entry.created_at)));
    out.push_str(&format!("updated: {}\n", iso_utc(entry.updated_at)));
    if entry.tags.is_empty() {
        out.push_str("tags: []\n");
    } else {
        let tags: Vec<String> = entry.tags.iter().map(|t| yaml_string(t)).collect();
        out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    if let Some(mood) = entry.mood {
        out.push_str(&format!("mood: {mood}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// JSON export (§8): lossless, tombstones and ids included, made for
// re-import. Attachments travel base64 inside the one file.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct JsonEntry {
    id: String,
    entry_date: String,
    created_at: i64,
    updated_at: i64,
    title: Option<String>,
    body_md: String,
    mood: Option<i64>,
    tz: Option<String>,
    tags: Vec<String>,
    deleted_at: Option<i64>,
    folder_id: Option<String>,
}

/// A folder row. Tombstones are scrubbed (`name` is null): they
/// carry only the deletion, never the deleted name.
#[derive(serde::Serialize)]
struct JsonFolder {
    id: String,
    parent_id: Option<String>,
    name: Option<String>,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
    rev: i64,
    device_id: String,
    base_rev: i64,
    base_device: String,
}

#[derive(serde::Serialize)]
struct JsonAttachment {
    id: String,
    entry_id: String,
    name: String,
    mime: String,
    size: i64,
    sha256: String,
    created_at: i64,
    deleted_at: Option<i64>,
    /// Base64 plaintext. Null for tombstones (their blobs are gone).
    data_base64: Option<String>,
}

#[derive(serde::Serialize)]
struct JsonExport {
    format: &'static str,
    version: u32,
    vault_id: String,
    exported_at: i64,
    folders: Vec<JsonFolder>,
    entries: Vec<JsonEntry>,
    attachments: Vec<JsonAttachment>,
}

impl Vault {
    /// Writes the whole vault — tombstones included — as one
    /// JSON file. PLAINTEXT, like the Markdown export: the
    /// caller warns and re-authenticates (§8).
    pub fn export_json(&self, out_file: &Path) -> Result<ExportReport> {
        let conn = self.db.connection();
        // Tags per entry id (links join through the row id).
        let mut tags: std::collections::HashMap<String, Vec<String>> = Default::default();
        {
            let mut stmt = conn.prepare(
                "SELECT e.id, t.name FROM entry_tag et
                   JOIN tag t ON t.id = et.tag_id
                   JOIN entry e ON e.rid = et.entry_rid
                  ORDER BY t.name_key",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, name) = row?;
                tags.entry(id).or_default().push(name);
            }
        }
        let mut entries = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, entry_date, created_at, updated_at, title, body_md, mood, tz, deleted_at,
                        folder_id
                   FROM entry ORDER BY entry_date, created_at, id",
            )?;
            let rows = stmt.query_map([], |row| {
                // A tombstone's body is scrubbed to NULL; export
                // it as the empty string.
                let body: Option<String> = row.get(5)?;
                Ok(JsonEntry {
                    id: row.get(0)?,
                    entry_date: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    title: row.get(4)?,
                    body_md: body.unwrap_or_default(),
                    mood: row.get(6)?,
                    tz: row.get(7)?,
                    tags: Vec::new(),
                    deleted_at: row.get(8)?,
                    folder_id: row.get(9)?,
                })
            })?;
            for row in rows {
                let mut entry = row?;
                entry.tags = tags.remove(&entry.id).unwrap_or_default();
                entries.push(entry);
            }
        }
        let mut folders = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, parent_id, name, created_at, updated_at, deleted_at, rev,
                        device_id, base_rev, base_device
                   FROM folder ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], |row| {
                let deleted: Option<i64> = row.get(5)?;
                Ok(JsonFolder {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    // Tombstones are scrubbed on export.
                    name: if deleted.is_some() { None } else { row.get(2)? },
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    deleted_at: deleted,
                    rev: row.get(6)?,
                    device_id: row.get(7)?,
                    base_rev: row.get(8)?,
                    base_device: row.get(9)?,
                })
            })?;
            for row in rows {
                folders.push(row?);
            }
        }
        let mut attachments = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT id, entry_id, name, mime, size, sha256, created_at, deleted_at
                   FROM attachment ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], |row| {
                // sha256 is stored as raw bytes; export as hex.
                let sha: Vec<u8> = row.get(5)?;
                Ok(JsonAttachment {
                    id: row.get(0)?,
                    entry_id: row.get(1)?,
                    name: row.get(2)?,
                    mime: row.get(3)?,
                    size: row.get(4)?,
                    sha256: data_encoding::HEXLOWER.encode(&sha),
                    created_at: row.get(6)?,
                    deleted_at: row.get(7)?,
                    data_base64: None,
                })
            })?;
            for row in rows {
                let mut att = row?;
                if att.deleted_at.is_none() {
                    let data = self.read_attachment(&att.id)?;
                    att.data_base64 = Some(data_encoding::BASE64.encode(&data));
                }
                attachments.push(att);
            }
        }
        let _ = conn;
        let export = JsonExport {
            format: "soirvu-json-export",
            version: 2,
            vault_id: self.header().vault_id.clone(),
            exported_at: crate::storage::now_ms(),
            folders,
            entries,
            attachments,
        };
        let text = serde_json::to_string_pretty(&export)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        fs::write(out_file, text)?;
        Ok(ExportReport {
            entries: export.entries.len(),
            attachments: export.attachments.len(),
            folders: export.folders.len(),
            dir: out_file.to_path_buf(),
        })
    }
}

fn prepare_out_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        let has_content = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name() != ".DS_Store");
        if has_content {
            return Err(Error::FolderNotEmpty);
        }
    } else {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// A YAML double-quoted string: backslash and quote are escaped, control
/// characters cannot appear (titles and tags were checked on the way in,
/// but a title is free text, so newlines are folded to spaces).
fn yaml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

// Stored names can come from older or foreign clients. Reserve space for a
// conventional extension and keep the UUID-prefixed filename under 255 bytes.
fn export_attachment_name(name: &str) -> String {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

    fn sanitize(text: &str, limit: usize) -> String {
        let mut safe = String::new();
        for c in text.nfc() {
            let c = if c.is_alphanumeric() || is_combining_mark(c) || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            };
            if safe.len() + c.len_utf8() > limit {
                break;
            }
            safe.push(c);
        }
        safe.trim_matches('.').into()
    }

    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty()
            && !ext.is_empty()
            && ext.len() <= 32
            && ext.bytes().all(|b| b.is_ascii_alphanumeric()) => (stem, ext),
        _ => (name, ""),
    };
    let extension_len = extension.len() + usize::from(!extension.is_empty());
    let mut safe = sanitize(stem, 200 - extension_len);
    if safe.is_empty() {
        safe.push_str("attachment");
    }
    if !extension.is_empty() {
        safe.push('.');
        safe.push_str(extension);
    }
    safe
}

/// A file-name version of the title: letters and digits (any alphabet) are
/// kept and lowered, everything else becomes one dash. At most 40 chars.
fn slug(title: Option<&str>) -> String {
    let mut out = String::new();
    let mut last_was_dash = true; // also trims a leading dash
    for ch in title.unwrap_or("").chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
        if out.chars().count() >= 40 {
            break;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "entry".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Milliseconds since 1970 -> `2026-10-13T07:31:03Z` (UTC).
/// No date crate: the civil calendar math below is the standard
/// days-from-epoch conversion, checked by the tests.
pub(crate) fn iso_utc(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let day_ms = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1_000 % 60
    )
}

/// (year, month, day) for a count of days since 1970-01-01.
/// Howard Hinnant's `civil_from_days` algorithm.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // year of era, [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year, [0, 365]
    let mp = (5 * doy + 2) / 153; // month index, [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::entries::EntryDraft;
    use tempfile::TempDir;

    fn fast() -> KdfParams {
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        }
    }

    fn new_vault() -> (TempDir, Vault) {
        let tmp = TempDir::new().unwrap();
        let (vault, _recovery) =
            Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
        (tmp, vault)
    }

    fn draft(date: &str, title: &str, body: &str) -> EntryDraft {
        EntryDraft {
            entry_date: date.into(),
            title: Some(title.into()),
            body_md: body.into(),
            ..Default::default()
        }
    }

    #[test]
    fn iso_dates_are_right() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(86_400_000), "1970-01-02T00:00:00Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z"); // leap day
        assert_eq!(iso_utc(1_762_000_000_000), "2025-11-01T12:26:40Z");
        assert_eq!(iso_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn attachment_export_names_are_safe_and_keep_extensions() {
        assert_eq!(export_attachment_name(""), "attachment");
        assert_eq!(export_attachment_name("meeting notes"), "meeting_notes");
        assert_eq!(export_attachment_name("CON.txt"), "CON.txt");
        assert_eq!(export_attachment_name("re\u{301}sume\u{301}.pdf"), "résumé.pdf");
        assert_eq!(export_attachment_name("../folder\\note: draft.txt"), "_folder_note__draft.txt");
        let name = export_attachment_name(&format!("{}.png", "é".repeat(150)));
        assert!(name.len() <= 200);
        assert!(name.ends_with(".png"));
    }

    #[test]
    fn slugs_are_file_name_safe() {
        assert_eq!(slug(Some("Мой День!")), "мой-день");
        assert_eq!(slug(Some("  Hello,  World ")), "hello-world");
        assert_eq!(slug(Some("!!!")), "entry");
        assert_eq!(slug(None), "entry");
        assert_eq!(slug(Some(&"а".repeat(100))).chars().count(), 40);
    }

    #[test]
    fn an_export_has_front_matter_text_and_assets() {
        let (_tmp, mut vault) = new_vault();
        let mut d = draft(
            "2026-10-11",
            "Воскресенье",
            "Фото дня: ![утро](attachment:PLACEHOLDER)",
        );
        let entry = vault.create_entry(None, &d).unwrap();
        let att = vault
            .add_attachment(&entry.id, "morning.jpg", "image/jpeg", b"jpeg bytes")
            .unwrap();
        // Put the real attachment id into the text, as the editor would.
        d.body_md = d.body_md.replace("PLACEHOLDER", &att.id);
        d.tags = vec!["идеи".into()];
        let entry = vault.update_entry(&entry.id, vault.get_entry(&entry.id).unwrap().rev, &d).unwrap();

        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!((report.entries, report.attachments), (1, 1));

        let file = out.join(format!("2026-10-11-воскресенье-{}.md", &entry.id[..8]));
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("---\n"));
        assert!(text.contains(&format!("id: {}", entry.id)));
        assert!(text.contains("title: \"Воскресенье\""));
        assert!(text.contains("tags: [\"идеи\"]"));
        assert!(text.contains("created: 20")); // an ISO date, not a number
        // The link now points at the exported file…
        let asset_name = format!("{}-morning.jpg", att.id);
        assert!(text.contains(&format!("![утро](assets/{asset_name})")));
        assert!(!text.contains("attachment:"));
        // …and the asset is the decrypted content.
        assert_eq!(
            fs::read(out.join("assets").join(asset_name)).unwrap(),
            b"jpeg bytes"
        );
    }

    #[test]
    fn deleted_entries_are_not_exported() {
        let (_tmp, mut vault) = new_vault();
        let keep = vault
            .create_entry(None, &draft("2026-10-01", "keep", "k"))
            .unwrap();
        let gone = vault
            .create_entry(None, &draft("2026-10-02", "gone", "g"))
            .unwrap();
        vault.delete_entry(&gone.id).unwrap();

        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!(report.entries, 1);
        let names: Vec<String> = fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        // One entry file plus the export manifest; no year directory.
        assert!(!out.join("2026").exists());
        let files: Vec<&String> = names.iter().filter(|n| n.ends_with(".md")).collect();
        assert_eq!(files.len(), 1);
        assert!(files[0].contains(&keep.id[..8]));
    }

    #[test]
    fn markdown_export_uses_folder_layout_with_manifest() {
        let (_tmp, mut vault) = new_vault();
        let health = vault.create_folder(None, "Health").unwrap();
        let nested = vault.create_folder(Some(&health.id), "Training").unwrap();
        let empty = vault.create_folder(None, "Empty").unwrap();
        let root = vault
            .create_entry(None, &draft("2026-10-29", "Morning", "plain"))
            .unwrap();
        let deep = vault
            .create_entry(
                Some(&nested.id),
                &draft(
                    "2026-10-31",
                    "Run",
                    &format!(
                        "see [morning](entry:{}) and ![pic](attachment:PLACEHOLDER)",
                        root.id
                    ),
                ),
            )
            .unwrap();
        let att = vault
            .add_attachment(&deep.id, "pic.png", "image/png", b"png bytes")
            .unwrap();
        let mut d = vault.get_entry(&deep.id).unwrap();
        d.body_md = d.body_md.replace("PLACEHOLDER", &att.id);
        let deep_rev = vault.get_entry(&deep.id).unwrap().rev;
        vault
            .update_entry(
                &deep.id,
                deep_rev,
                &EntryDraft {
                    title: d.title.clone(),
                    body_md: d.body_md.clone(),
                    ..draft("2026-10-31", "Run", "")
                },
            )
            .unwrap();

        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!(report.entries, 2);
        assert_eq!(report.folders, 3);

        // Nested entries live under slug-suffixed directories;
        // root entries stay at the export root.
        let deep_dir = out.join(format!("health-{}", &health.id[..8]));
        assert!(deep_dir.is_dir());
        assert!(out.join(format!("empty-{}", &empty.id[..8])).is_dir());
        let files: Vec<String> = fs::read_dir(&deep_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 1, "only the Training subdir: {files:?}");
        let nested_dir = deep_dir.join(format!("training-{}", &nested.id[..8]));
        assert!(nested_dir.is_dir());
        let nested_files: Vec<String> = fs::read_dir(&nested_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(nested_files.len(), 1);
        assert!(nested_files[0].starts_with("2026-10-31-run-"));
        // Front matter carries the folder id; the root file omits it.
        let text = fs::read_to_string(nested_dir.join(&nested_files[0])).unwrap();
        assert!(text.contains(&format!("folder: {}", nested.id)));
        let root_file = out.join(format!("2026-10-29-morning-{}.md", &root.id[..8]));
        let root_text = fs::read_to_string(&root_file).unwrap();
        assert!(!root_text.contains("folder:"));
        // Attachments rewrite depth-relatively; entry links stay canonical.
        assert!(text.contains(&format!("../../assets/{}-pic.png", att.id)));
        assert!(text.contains(&format!("(entry:{})", root.id)));
        // The manifest maps every id to its file.
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(out.join("soirvu-export.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["version"], 2);
        let entry_paths: Vec<&str> = manifest["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert_eq!(entry_paths.len(), 2);
        assert!(entry_paths.iter().any(|p| p.ends_with(&nested_files[0])));
        let folder_paths: Vec<&str> = manifest["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect();
        assert_eq!(folder_paths.len(), 3);
    }

    #[test]
    fn export_refuses_a_folder_that_is_not_empty() {
        let (_tmp, vault) = new_vault();
        let out = _tmp.path().join("export");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("mine.txt"), b"precious").unwrap();
        assert!(matches!(
            vault.export_markdown(&out),
            Err(Error::FolderNotEmpty)
        ));
        assert_eq!(fs::read(out.join("mine.txt")).unwrap(), b"precious");
    }

    #[test]
    fn an_empty_vault_exports_to_an_empty_folder() {
        let (_tmp, vault) = new_vault();
        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!((report.entries, report.attachments), (0, 0));
        assert!(out.exists());
    }

    #[test]
    fn json_export_is_lossless_and_includes_tombstones() {
        let (tmp, mut vault) = new_vault();
        let mut d = draft("2026-10-15", "live", "still here");
        d.mood = Some(3);
        d.tags = vec!["work".into()];
        let live = vault.create_entry(None, &d).unwrap();
        let gone = vault
            .create_entry(None, &draft("2026-10-14", "gone", "secret text"))
            .unwrap();
        vault
            .add_attachment(&live.id, "note.txt", "text/plain", b"attachment bytes")
            .unwrap();
        vault.delete_entry(&gone.id).unwrap();

        let out = tmp.path().join("export.json");
        let report = vault.export_json(&out).unwrap();
        assert_eq!(report.entries, 2);
        assert_eq!(report.attachments, 1);

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(value["format"], "soirvu-json-export");
        let entries = value["entries"].as_array().unwrap();
        let tomb = entries.iter().find(|e| e["id"] == gone.id).unwrap();
        assert!(tomb["deleted_at"].is_number());
        assert_eq!(tomb["body_md"], ""); // scrubbed at delete; nothing leaks back
        let live_json = entries.iter().find(|e| e["id"] == live.id).unwrap();
        assert_eq!(live_json["tags"], serde_json::json!(["work"]));
        assert_eq!(live_json["mood"], 3);
        let att = &value["attachments"][0];
        assert_eq!(
            data_encoding::BASE64
                .decode(att["data_base64"].as_str().unwrap().as_bytes())
                .unwrap(),
            b"attachment bytes"
        );
    }
}
