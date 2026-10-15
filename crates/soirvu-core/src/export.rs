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

use crate::entries::ListQuery;
use crate::error::{Error, Result};
use crate::folder::Vault;

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub entries: usize,
    pub attachments: usize,
    pub dir: PathBuf,
}

impl Vault {
    /// Writes every live entry as a Markdown file with YAML front matter,
    /// and every attachment as a decrypted file in `assets/`.
    ///
    /// `out_dir` must be missing or empty — the same rule as creating a
    /// vault — so an export can never overwrite the user's files.
    /// Links in the text are rewritten: `attachment:<id>` becomes
    /// `assets/<id>-<name>`, so the export opens in any Markdown reader.
    pub fn export_markdown(&self, out_dir: &Path) -> Result<ExportReport> {
        prepare_out_dir(out_dir)?;
        let mut report = ExportReport {
            entries: 0,
            attachments: 0,
            dir: out_dir.to_path_buf(),
        };

        // Summaries come paged; the full text is then loaded entry by entry.
        let mut ids: Vec<String> = Vec::new();
        let mut after = None;
        loop {
            let page = self.list_entries(&ListQuery {
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

        for id in &ids {
            let entry = self.get_entry(id)?;
            let attachments = self.list_attachments(id)?;

            let mut body = entry.body_md.clone();
            for att in &attachments {
                let file_name = format!("{}-{}", att.id, att.name);
                let assets = out_dir.join("assets");
                fs::create_dir_all(&assets)?;
                fs::write(assets.join(&file_name), self.read_attachment(&att.id)?)?;
                body = body.replace(
                    &format!("attachment:{}", att.id),
                    &format!("assets/{file_name}"),
                );
                report.attachments += 1;
            }

            let year_dir = out_dir.join(&entry.entry_date[..4]);
            fs::create_dir_all(&year_dir)?;
            let short_id = &entry.id[..8];
            let file = year_dir.join(format!(
                "{}-{}-{short_id}.md",
                entry.entry_date,
                slug(entry.title.as_deref())
            ));
            fs::write(&file, render_entry(&entry, &body))?;
            report.entries += 1;
        }
        Ok(report)
    }
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
                "SELECT id, entry_date, created_at, updated_at, title, body_md, mood, tz, deleted_at
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
                })
            })?;
            for row in rows {
                let mut entry = row?;
                entry.tags = tags.remove(&entry.id).unwrap_or_default();
                entries.push(entry);
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
            version: 1,
            vault_id: self.header().vault_id.clone(),
            exported_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default(),
            entries,
            attachments,
        };
        let text = serde_json::to_string_pretty(&export)
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        fs::write(out_file, text)?;
        Ok(ExportReport {
            entries: export.entries.len(),
            attachments: export.attachments.len(),
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

/// One exported file: front matter, then the entry text as stored.
fn render_entry(entry: &crate::entries::Entry, body: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("id: {}\n", entry.id));
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
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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
        let entry = vault.create_entry(&d).unwrap();
        let att = vault
            .add_attachment(&entry.id, "morning.jpg", "image/jpeg", b"jpeg bytes")
            .unwrap();
        // Put the real attachment id into the text, as the editor would.
        d.body_md = d.body_md.replace("PLACEHOLDER", &att.id);
        d.tags = vec!["идеи".into()];
        let entry = vault.update_entry(&entry.id, &d).unwrap();

        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!((report.entries, report.attachments), (1, 1));

        let file = out
            .join("2026")
            .join(format!(
                "2026-10-11-воскресенье-{}.md",
                &entry.id[..8]
            ));
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
        assert_eq!(fs::read(out.join("assets").join(asset_name)).unwrap(), b"jpeg bytes");
    }

    #[test]
    fn deleted_entries_are_not_exported() {
        let (_tmp, mut vault) = new_vault();
        let keep = vault.create_entry(&draft("2026-10-01", "keep", "k")).unwrap();
        let gone = vault.create_entry(&draft("2026-10-02", "gone", "g")).unwrap();
        vault.delete_entry(&gone.id).unwrap();

        let out = _tmp.path().join("export");
        let report = vault.export_markdown(&out).unwrap();
        assert_eq!(report.entries, 1);
        let names: Vec<String> = fs::read_dir(out.join("2026"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].contains(&keep.id[..8]));
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
        let live = vault.create_entry(&d).unwrap();
        let gone = vault
            .create_entry(&draft("2026-10-14", "gone", "secret text"))
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
