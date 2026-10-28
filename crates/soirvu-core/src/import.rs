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
}

impl Vault {
    /// Imports every `.md` file under `dir` (searched
    /// recursively, in path order). See the module docs for
    /// the id rules.
    pub fn import_markdown(&mut self, dir: &Path) -> Result<ImportReport> {
        let mut files = Vec::new();
        collect_markdown(dir, &mut files);
        files.sort();
        let mut report = ImportReport {
            entries: 0,
            attachments: 0,
            skipped_entries: 0,
            skipped_attachments: 0,
        };
        for file in &files {
            match self.import_markdown_file(dir, file) {
                Ok((e, a, sa)) => {
                    report.entries += e;
                    report.attachments += a;
                    report.skipped_attachments += sa;
                }
                Err(_) => report.skipped_entries += 1,
            }
        }
        Ok(report)
    }

    fn import_markdown_file(
        &mut self,
        root: &Path,
        file: &Path,
    ) -> Result<(usize, usize, usize)> {
        let text = fs::read_to_string(file)?;
        let (fm, mut body) = split_front_matter(&text);

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
            if rel.split('/').any(|seg| seg == "..") {
                skipped_attachments += 1;
                continue;
            }
            let md_dir = file.parent().unwrap_or(root);
            let path = [md_dir.join(&rel), root.join(&rel)]
                .into_iter()
                .find(|p| p.is_file());
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
                Some((uuid, rest)) => (Some(uuid), rest.to_string()),
                None => (None, file_name.clone()),
            };
            let att_id = match &old_id {
                Some(id) if !self.attachment_exists(id)? => id.to_string(),
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
        let entry_id = match &fm.id {
            Some(id) if !id.is_empty() && !self.entry_exists(id)? => id.clone(),
            _ => Uuid::now_v7().to_string(),
        };
        let draft = EntryDraft {
            entry_date,
            title,
            body_md: body.trim_start_matches('\n').to_string(),
            mood: fm.mood,
            tz: None,
            tags: fm.tags.clone(),
        };
        self.insert_entry_exact(&entry_id, &draft, created, updated, None)?;
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
        Ok((1, imported, skipped_attachments))
    }

    /// Imports a SoirVu JSON export (see `export_json`).
    /// Entries and attachments keep their ids; anything whose
    /// id is already taken is skipped, never overwritten.
    pub fn import_json(&mut self, file: &Path) -> Result<ImportReport> {
        let text = fs::read_to_string(file)?;
        let data: JsonImport = serde_json::from_str(&text)
            .map_err(|e| Error::InvalidInput(format!("not a SoirVu JSON export: {e}")))?;
        if data.format != "soirvu-json-export" || data.version != 1 {
            return Err(Error::InvalidInput(
                "not a SoirVu JSON export (bad format or version)".into(),
            ));
        }
        let mut report = ImportReport {
            entries: 0,
            attachments: 0,
            skipped_entries: 0,
            skipped_attachments: 0,
        };
        for entry in &data.entries {
            if self.entry_exists(&entry.id)? {
                report.skipped_entries += 1;
                continue;
            }
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
            ) {
                Ok(()) => report.entries += 1,
                Err(_) => report.skipped_entries += 1,
            }
        }
        for att in &data.attachments {
            let skip = self.attachment_exists(&att.id)?
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

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path
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
    let candidate = &stem[..10];
    if !candidate
        .chars()
        .all(|c| c.is_ascii_digit() || c == '-')
    {
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
    if text.len() < 10 || !valid_date(&text[..10]) {
        return None;
    }
    let days = days_from_civil(
        text[..4].parse().ok()?,
        text[5..7].parse().ok()?,
        text[8..10].parse().ok()?,
    );
    let mut secs = days * 86_400;
    if text.len() >= 19 && text.as_bytes()[13] == b':' {
        let h: i64 = text[11..13].parse().ok()?;
        let m: i64 = text[14..16].parse().ok()?;
        let s: i64 = text[17..19].parse().ok()?;
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

/// `assets/…` destinations of links and images, deduplicated.
fn asset_refs(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find("](assets/") {
        let after = &rest[pos + 2..];
        let Some(end) = after.find(')') else { break };
        let dest = after[..end].split_whitespace().next().unwrap_or("");
        if !dest.is_empty() && !out.iter().any(|d| d == dest) {
            out.push(dest.to_string());
        }
        rest = &after[end..];
    }
    out
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
    entries: Vec<JsonEntryIn>,
    attachments: Vec<JsonAttachmentIn>,
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
    use crate::entries::EntryDraft;

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
        let entry = source.create_entry(&d).unwrap();
        let att = source
            .add_attachment(&entry.id, "note.txt", "text/plain", b"attachment bytes")
            .unwrap();
        // The body references the attachment the way the editor stores it.
        let mut d2 = d.clone();
        d2.body_md = format!("look: [note](attachment:{})", att.id);
        source.update_entry(&entry.id, &d2).unwrap();

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
        assert_eq!(target.read_attachment(&atts[0].id).unwrap(), b"attachment bytes");
        assert!(got.body_md.contains(&format!("attachment:{}", att.id)));
    }

    #[test]
    fn json_round_trip_restores_tombstones_and_skips_duplicates() {
        let tmp = TempDir::new().unwrap();
        let mut source = new_vault(&tmp);
        let keep = source
            .create_entry(&EntryDraft {
                entry_date: "2026-10-15".into(),
                title: Some("keep".into()),
                body_md: "k".into(),
                ..Default::default()
            })
            .unwrap();
        let gone = source
            .create_entry(&EntryDraft {
                entry_date: "2026-10-14".into(),
                title: Some("gone".into()),
                body_md: "secret".into(),
                ..Default::default()
            })
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
}
