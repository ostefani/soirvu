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
}
