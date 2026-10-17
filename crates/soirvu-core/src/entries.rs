//! Entries, tags and search. Everything here is a method on `Vault`, so none of
//! it can be used without unlocking the vault first.
//!
//! Rules that hold for every write:
//! - it runs in one transaction;
//! - it bumps `rev`, `updated_at`, `device_id` and `lseq` of the entry it changes
//!   (tags belong to the entry, so changing tags counts as changing the entry);
//! - deleting never removes the row: it leaves a "tombstone" with the text scrubbed.

use rusqlite::{Connection, Transaction, params};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::folder::Vault;
use crate::storage::now_ms;

/// In search snippets the matched text is wrapped in these two control characters.
/// The UI turns them into `<mark>` elements; the core never produces HTML.
pub const MARK_START: char = '\u{2}';
pub const MARK_END: char = '\u{3}';

const MAX_TAG_CHARS: usize = 64;
const MIN_WORD_CHARS: usize = 3;
const PREVIEW_CHARS: usize = 200;
const MAX_PAGE: usize = 200;

/// Everything the user can edit in an entry. Saving always sends the whole draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryDraft {
    /// 'YYYY-MM-DD': the calendar day the entry is about, in the user's local time.
    /// The UI decides it (the core does not know the user's time zone).
    pub entry_date: String,
    pub title: Option<String>,
    pub body_md: String,
    pub mood: Option<i64>,
    /// IANA time zone name at creation, such as "Europe/Berlin". Only used when the
    /// entry is created; later saves keep the original.
    pub tz: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub entry_date: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub rev: i64,
    pub title: Option<String>,
    pub body_md: String,
    pub mood: Option<i64>,
    pub tz: Option<String>,
    /// Sorted by name, ignoring case.
    pub tags: Vec<String>,
}

/// What the entry list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    pub id: String,
    pub entry_date: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub title: Option<String>,
    /// The first 200 characters of the text.
    pub preview: String,
    pub tags: Vec<String>,
}

/// Where the previous page ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub entry_date: String,
    pub created_at: i64,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct ListQuery {
    /// 1 to 200.
    pub limit: usize,
    /// `None` for the first page; otherwise the `next` of the previous page.
    pub after: Option<Cursor>,
    /// Only entries with this tag (matched ignoring case).
    pub tag: Option<String>,
    /// Only entries on this day (YYYY-MM-DD).
    pub date: Option<String>,
}

impl Default for ListQuery {
    fn default() -> Self {
        ListQuery {
            limit: 50,
            after: None,
            tag: None,
            date: None,
        }
    }
}

/// One day's live-entry count, for the calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCount {
    pub date: String,
    pub entries: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Newest day first; within a day, newest first.
    pub items: Vec<EntrySummary>,
    /// `Some` when there may be more entries.
    pub next: Option<Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    /// Plain text around the match, with the match wrapped in MARK_START / MARK_END.
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub name: String,
    pub entries: i64,
}

impl Vault {
    pub fn create_entry(&mut self, draft: &EntryDraft) -> Result<Entry> {
        validate_draft(draft)?;
        let tags = normalize_tags(&draft.tags)?;
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let device = self.device_id.clone();

        let tx = self.db.conn_mut().transaction()?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "INSERT INTO entry
               (id, entry_date, created_at, updated_at, rev, device_id, lseq, title, body_md, mood, tz, base_rev, base_device)
             VALUES (?1, ?2, ?3, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?4)",
            params![
                id,
                draft.entry_date,
                now,
                device,
                lseq,
                draft.title,
                draft.body_md,
                draft.mood,
                draft.tz
            ],
        )?;
        let rid = tx.last_insert_rowid();
        write_tags(&tx, rid, &tags)?;
        tx.commit()?;
        self.get_entry(&id)
    }

    /// Inserts an entry with a caller-chosen id and timestamps —
    /// the import path (§8). The caller has checked the id is
    /// free. A `deleted_at` entry is stored scrubbed, exactly
    /// like a locally deleted one: body NULL, no search text.
    pub(crate) fn insert_entry_exact(
        &mut self,
        id: &str,
        draft: &EntryDraft,
        created_at: i64,
        updated_at: i64,
        deleted_at: Option<i64>,
    ) -> Result<()> {
        validate_draft(draft)?;
        let tags = normalize_tags(&draft.tags)?;
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "INSERT INTO entry
               (id, entry_date, created_at, updated_at, rev, device_id, lseq, title, body_md, mood, tz, deleted_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                draft.entry_date,
                created_at,
                updated_at,
                device,
                lseq,
                draft.title,
                if deleted_at.is_some() {
                    None
                } else {
                    Some(&draft.body_md)
                },
                draft.mood,
                draft.tz,
                deleted_at
            ],
        )?;
        let rid = tx.last_insert_rowid();
        write_tags(&tx, rid, &tags)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_entry(&self, id: &str) -> Result<Entry> {
        load(self.db.connection(), id).map(|(_, entry)| entry)
    }

    /// Replaces the entry's date, title, text, mood and tags with the draft.
    /// If nothing differs, nothing is written (no new revision).
    pub fn update_entry(&mut self, id: &str, draft: &EntryDraft) -> Result<Entry> {
        validate_draft(draft)?;
        let tags = normalize_tags(&draft.tags)?;
        let (rid, current) = load(self.db.connection(), id)?;

        let current_keys: Vec<String> = current.tags.iter().map(|t| tag_key(t)).collect();
        let new_keys: Vec<&String> = tags.iter().map(|(_, key)| key).collect();
        let unchanged = current.entry_date == draft.entry_date
            && current.title == draft.title
            && current.body_md == draft.body_md
            && current.mood == draft.mood
            && current_keys.iter().eq(new_keys.iter().copied());
        if unchanged {
            return Ok(current);
        }

        // Never let updated_at go backwards, even if the clock does.
        let now = now_ms().max(current.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "UPDATE entry
                SET entry_date = ?1, title = ?2, body_md = ?3, mood = ?4,
                    updated_at = ?5, base_rev = CASE WHEN device_id <> ?6 THEN rev ELSE base_rev END, base_device = CASE WHEN device_id <> ?6 THEN device_id ELSE base_device END, rev = rev + 1, device_id = ?6, lseq = ?7
              WHERE rid = ?8 AND deleted_at IS NULL",
            params![
                draft.entry_date,
                draft.title,
                draft.body_md,
                draft.mood,
                now,
                device,
                lseq,
                rid
            ],
        )?;
        write_tags(&tx, rid, &tags)?;
        tx.commit()?;
        self.get_entry(id)
    }

    /// Deletes an entry by writing a tombstone: the row stays (so a later sync knows
    /// the entry was deleted) but title, text, mood and tags are erased.
    /// Its attachments are tombstoned in the same transaction and their blob
    /// files are removed after the commit, so deleted content does not stay
    /// readable through the attachment API or on disk.
    pub fn delete_entry(&mut self, id: &str) -> Result<()> {
        let (rid, current) = load(self.db.connection(), id)?;
        let attachment_ids: Vec<String> = {
            let mut stmt = self.db.connection().prepare(
                "SELECT id FROM attachment WHERE entry_id = ?1 AND deleted_at IS NULL",
            )?;
            let rows = stmt.query_map([id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };
        let now = now_ms().max(current.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        for attachment_id in &attachment_ids {
            let lseq = next_lseq(&tx)?;
            tx.execute(
                "UPDATE attachment SET deleted_at = ?1, lseq = ?2
                  WHERE id = ?3 AND deleted_at IS NULL",
                params![now, lseq, attachment_id],
            )?;
        }
        let lseq = next_lseq(&tx)?;
        tx.execute("DELETE FROM entry_tag WHERE entry_rid = ?1", [rid])?;
        tx.execute(
            "UPDATE entry
                SET deleted_at = ?1, updated_at = ?1, base_rev = CASE WHEN device_id <> ?2 THEN rev ELSE base_rev END, base_device = CASE WHEN device_id <> ?2 THEN device_id ELSE base_device END, rev = rev + 1, device_id = ?2, lseq = ?3,
                    title = NULL, body_md = NULL, mood = NULL
              WHERE rid = ?4",
            params![now, device, lseq, rid],
        )?;
        tx.commit()?;
        for attachment_id in &attachment_ids {
            crate::attachments::remove_blob(self.dir(), attachment_id);
        }
        Ok(())
    }

    pub fn list_entries(&self, query: &ListQuery) -> Result<Page> {
        let limit = query.limit.clamp(1, MAX_PAGE);
        let tag_key = match &query.tag {
            Some(name) => Some(normalize_tag(name)?.1),
            None => None,
        };
        let (after_date, after_created, after_id) = match &query.after {
            Some(c) => (
                Some(c.entry_date.clone()),
                Some(c.created_at),
                Some(c.id.clone()),
            ),
            None => (None, None, None),
        };

        // One extra row tells us whether another page exists.
        let mut stmt = self.db.connection().prepare(
            "SELECT e.id, e.entry_date, e.created_at, e.updated_at, e.title,
                    substr(e.body_md, 1, ?6),
                    (SELECT group_concat(t.name, char(31))
                       FROM entry_tag et JOIN tag t ON t.id = et.tag_id
                      WHERE et.entry_rid = e.rid)
               FROM entry e
              WHERE e.deleted_at IS NULL
                AND (?1 IS NULL OR (e.entry_date, e.created_at, e.id) < (?1, ?2, ?3))
                AND (?4 IS NULL OR EXISTS (
                       SELECT 1 FROM entry_tag et JOIN tag t ON t.id = et.tag_id
                        WHERE et.entry_rid = e.rid AND t.name_key = ?4))
                AND (?7 IS NULL OR e.entry_date = ?7)
              ORDER BY e.entry_date DESC, e.created_at DESC, e.id DESC
              LIMIT ?5",
        )?;
        let rows = stmt.query_map(
            params![
                after_date,
                after_created,
                after_id,
                tag_key,
                (limit + 1) as i64,
                PREVIEW_CHARS as i64,
                query.date
            ],
            |row| {
                let tags: Option<String> = row.get(6)?;
                Ok(EntrySummary {
                    id: row.get(0)?,
                    entry_date: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    title: row.get(4)?,
                    preview: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    tags: split_tags(tags),
                })
            },
        )?;
        let mut items = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        let next = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|last| Cursor {
                entry_date: last.entry_date.clone(),
                created_at: last.created_at,
                id: last.id.clone(),
            })
        } else {
            None
        };
        Ok(Page { items, next })
    }

    /// How many live entries each day in `[from, to)` has — the
    /// calendar's dots. Days with no entries are absent.
    pub fn entry_days(&self, from: &str, to: &str) -> Result<Vec<DayCount>> {
        let mut stmt = self.db.connection().prepare(
            "SELECT entry_date, COUNT(*) FROM entry
              WHERE deleted_at IS NULL AND entry_date >= ?1 AND entry_date < ?2
              GROUP BY entry_date ORDER BY entry_date",
        )?;
        let rows = stmt.query_map(params![from, to], |row| {
            Ok(DayCount {
                date: row.get(0)?,
                entries: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Entries written on this month-and-day in earlier years —
    /// the "on this day" memory lane. `date` is the reference
    /// day (YYYY-MM-DD); only strictly earlier entries match,
    /// newest year first.
    pub fn on_this_day(&self, date: &str) -> Result<Vec<EntrySummary>> {
        let mut stmt = self.db.connection().prepare(
            "SELECT e.id, e.entry_date, e.created_at, e.updated_at, e.title,
                    substr(e.body_md, 1, ?2),
                    (SELECT group_concat(t.name, char(31))
                       FROM entry_tag et JOIN tag t ON t.id = et.tag_id
                      WHERE et.entry_rid = e.rid)
               FROM entry e
              WHERE e.deleted_at IS NULL
                AND substr(e.entry_date, 6, 5) = substr(?1, 6, 5)
                AND e.entry_date < ?1
              ORDER BY e.entry_date DESC, e.created_at DESC, e.id DESC",
        )?;
        let rows = stmt.query_map(params![date, PREVIEW_CHARS as i64], |row| {
            let tags: Option<String> = row.get(6)?;
            Ok(EntrySummary {
                id: row.get(0)?,
                entry_date: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                title: row.get(4)?,
                preview: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                tags: split_tags(tags),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Finds entries containing every word of `query` (words of 3 or more characters;
    /// shorter words are ignored). Matching is by substring and ignores case, so
    /// «книг» finds «Книгу», «книги» and «книга». Newest entries come first.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let normalized: String = query.nfc().collect();
        let words: Vec<String> = normalized
            .split_whitespace()
            .filter(|word| word.chars().count() >= MIN_WORD_CHARS)
            .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
            .collect();
        if words.is_empty() {
            return Err(Error::QueryTooShort);
        }
        // Every word becomes a quoted phrase, so characters like * or - or the
        // word NEAR are plain text and cannot be read as search operators.
        let fts_query = words.join(" ");

        let mut stmt = self.db.connection().prepare(
            "SELECT e.id, e.entry_date, e.title,
                    snippet(entry_fts, -1, char(2), char(3), '…', 64)
               FROM entry_fts JOIN entry e ON e.rid = entry_fts.rowid
              WHERE entry_fts MATCH ?1 AND e.deleted_at IS NULL
              ORDER BY e.entry_date DESC, e.created_at DESC, e.id DESC
              LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit.clamp(1, MAX_PAGE) as i64], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                entry_date: row.get(1)?,
                title: row.get(2)?,
                snippet: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Tags used by at least one live entry, with how many entries use them.
    pub fn list_tags(&self) -> Result<Vec<TagCount>> {
        let mut stmt = self.db.connection().prepare(
            "SELECT t.name, count(*)
               FROM tag t
               JOIN entry_tag et ON et.tag_id = t.id
               JOIN entry e ON e.rid = et.entry_rid
              WHERE e.deleted_at IS NULL
              GROUP BY t.id
              ORDER BY t.name_key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TagCount {
                name: row.get(0)?,
                entries: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Renames a tag on every live entry that uses it, and returns how many entries
    /// that was.
    ///
    /// - Only the capitalization or spelling of the same tag may change freely
    ///   («работа» to «Работа»). Any other new name must not belong to a different
    ///   tag that is in use; that would silently merge two tags, so it is refused
    ///   with `TagExists` and the caller has to call `merge_tags` on purpose.
    /// - The tag set belongs to the entry, so each changed entry gets a new `rev`,
    ///   `updated_at`, `device_id` and its own `lseq`, exactly like `update_entry`.
    ///   Without this, sync would never learn about the change. Tombstoned
    ///   entries have no tags and are not touched.
    /// - Renaming a tag to the name it already has changes nothing and returns 0.
    /// - A tag that no live entry uses does not exist as far as this method is
    ///   concerned (`TagNotFound`), and a leftover row of such a tag never blocks
    ///   a new name.
    pub fn rename_tag(&mut self, from: &str, to: &str) -> Result<usize> {
        let (_, from_key) = normalize_tag(from)?;
        let (to_name, to_key) = normalize_tag(to)?;
        let device = self.device_id.clone();

        let tx = self.db.conn_mut().transaction()?;
        let source = find_used_tag(&tx, &from_key)?.ok_or(Error::TagNotFound)?;
        if to_key == from_key {
            if to_name == source.name {
                return Ok(0);
            }
        } else {
            if find_used_tag(&tx, &to_key)?.is_some() {
                return Err(Error::TagExists);
            }
            // The name may still be held by the row of a tag nobody uses any more
            // (no entry, live or tombstoned, links to it), which is invisible to
            // callers. Remove it so that the unique key is free.
            tx.execute("DELETE FROM tag WHERE name_key = ?1", [&to_key])?;
        }
        tx.execute(
            "UPDATE tag SET name = ?1, name_key = ?2 WHERE id = ?3",
            params![to_name, to_key, source.id],
        )?;
        let rids = live_entries_with_tag(&tx, source.id)?;
        bump_entries(&tx, &rids, &device)?;
        tx.commit()?;
        Ok(rids.len())
    }

    /// Replaces the tag `from` by the tag `into` on every live entry that has
    /// `from`, deletes `from`, and returns how many entries that was.
    ///
    /// - Both tags must be in use (`TagNotFound` otherwise) and must be different
    ///   tags (`InvalidInput` otherwise: «Работа» and «работа» are one tag).
    /// - `into` keeps its own spelling.
    /// - An entry that already has both ends up with `into` once. Its tag set
    ///   still changed (it lost `from`), so it is counted and gets a new revision
    ///   like all the others: see `rename_tag` for what that means.
    pub fn merge_tags(&mut self, from: &str, into: &str) -> Result<usize> {
        let (_, from_key) = normalize_tag(from)?;
        let (_, into_key) = normalize_tag(into)?;
        if from_key == into_key {
            return Err(Error::InvalidInput(
                "a tag cannot be merged into itself".into(),
            ));
        }
        let device = self.device_id.clone();

        let tx = self.db.conn_mut().transaction()?;
        let source = find_used_tag(&tx, &from_key)?.ok_or(Error::TagNotFound)?;
        let target = find_used_tag(&tx, &into_key)?.ok_or(Error::TagNotFound)?;
        let rids = live_entries_with_tag(&tx, source.id)?;

        // Entries that have both tags only lose `from`; the others get `into` in
        // its place. Doing the first step first keeps the primary key
        // (entry_rid, tag_id) from ever seeing a duplicate.
        tx.execute(
            "DELETE FROM entry_tag
              WHERE tag_id = ?1
                AND entry_rid IN (SELECT entry_rid FROM entry_tag WHERE tag_id = ?2)",
            params![source.id, target.id],
        )?;
        tx.execute(
            "UPDATE entry_tag SET tag_id = ?2 WHERE tag_id = ?1",
            params![source.id, target.id],
        )?;
        tx.execute("DELETE FROM tag WHERE id = ?1", [source.id])?;
        bump_entries(&tx, &rids, &device)?;
        tx.commit()?;
        Ok(rids.len())
    }

    /// Removes the tag `name` from every live entry that has it
    /// and deletes the tag, returning how many entries that was.
    /// The tag must be in use (`TagNotFound` otherwise, the same
    /// rule as rename and merge). Affected entries get a new
    /// revision, like a rename or a merge; tombstones keep
    /// nothing of the tag — their links go with its row.
    pub fn delete_tag(&mut self, name: &str) -> Result<usize> {
        let (_, key) = normalize_tag(name)?;
        let device = self.device_id.clone();

        let tx = self.db.conn_mut().transaction()?;
        let source = find_used_tag(&tx, &key)?.ok_or(Error::TagNotFound)?;
        let rids = live_entries_with_tag(&tx, source.id)?;
        tx.execute("DELETE FROM entry_tag WHERE tag_id = ?1", [source.id])?;
        tx.execute("DELETE FROM tag WHERE id = ?1", [source.id])?;
        bump_entries(&tx, &rids, &device)?;
        tx.commit()?;
        Ok(rids.len())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reads one live entry and its internal row id.
fn load(conn: &Connection, id: &str) -> Result<(i64, Entry)> {
    let found = conn.query_row(
        "SELECT rid, id, entry_date, created_at, updated_at, rev, title, body_md, mood, tz
           FROM entry WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Entry {
                    id: row.get(1)?,
                    entry_date: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    rev: row.get(5)?,
                    title: row.get(6)?,
                    body_md: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    mood: row.get(8)?,
                    tz: row.get(9)?,
                    tags: vec![],
                },
            ))
        },
    );
    let (rid, mut entry) = match found {
        Ok(found) => found,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Err(Error::EntryNotFound),
        Err(other) => return Err(other.into()),
    };

    let mut stmt = conn.prepare(
        "SELECT t.name FROM entry_tag et JOIN tag t ON t.id = et.tag_id
          WHERE et.entry_rid = ?1 ORDER BY t.name_key",
    )?;
    entry.tags = stmt
        .query_map([rid], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok((rid, entry))
}

/// The change counter: one number that goes up by one with every write in this vault.
pub(crate) fn next_lseq(tx: &Transaction) -> Result<i64> {
    tx.execute(
        "UPDATE meta SET value = CAST(value AS INTEGER) + 1 WHERE key = 'lseq'",
        [],
    )?;
    Ok(tx.query_row(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'lseq'",
        [],
        |row| row.get(0),
    )?)
}

/// A tag as the public API sees it.
struct UsedTag {
    id: i64,
    name: String,
}

/// Finds a tag by its key, but only if at least one live entry uses it. A tag
/// nobody uses stays in the table (see `list_tags`) yet does not exist for callers.
fn find_used_tag(tx: &Transaction, key: &str) -> Result<Option<UsedTag>> {
    let found = tx.query_row(
        "SELECT t.id, t.name FROM tag t
          WHERE t.name_key = ?1
            AND EXISTS (SELECT 1 FROM entry_tag et JOIN entry e ON e.rid = et.entry_rid
                         WHERE et.tag_id = t.id AND e.deleted_at IS NULL)",
        [key],
        |row| {
            Ok(UsedTag {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        },
    );
    match found {
        Ok(tag) => Ok(Some(tag)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(other) => Err(other.into()),
    }
}

/// Row ids of the live entries that carry this tag, in creation order, so that
/// the change counter is assigned in a predictable order.
fn live_entries_with_tag(tx: &Transaction, tag_id: i64) -> Result<Vec<i64>> {
    let mut stmt = tx.prepare(
        "SELECT e.rid FROM entry_tag et JOIN entry e ON e.rid = et.entry_rid
          WHERE et.tag_id = ?1 AND e.deleted_at IS NULL
          ORDER BY e.rid",
    )?;
    let rids = stmt
        .query_map([tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(rids)
}

/// Records "this entry changed" for entries whose tag set was changed by a
/// tag-wide operation: new `rev`, `updated_at` (never backwards), `device_id`,
/// and a change counter of its own for each entry, so that a reader paging by
/// `lseq` can never split a group of entries that share one value.
/// Title, text and mood are not touched, so the search index is not either.
fn bump_entries(tx: &Transaction, rids: &[i64], device: &str) -> Result<()> {
    let now = now_ms();
    for rid in rids {
        let lseq = next_lseq(tx)?;
        tx.execute(
            "UPDATE entry
                SET updated_at = max(?1, updated_at + 1), base_rev = CASE WHEN device_id <> ?2 THEN rev ELSE base_rev END, base_device = CASE WHEN device_id <> ?2 THEN device_id ELSE base_device END, rev = rev + 1,
                    device_id = ?2, lseq = ?3
              WHERE rid = ?4 AND deleted_at IS NULL",
            params![now, device, lseq, rid],
        )?;
    }
    Ok(())
}

/// Makes the entry's tag set exactly `tags`, creating tags that do not exist yet.
/// When a tag already exists, its original spelling is kept.
fn write_tags(tx: &Transaction, rid: i64, tags: &[(String, String)]) -> Result<()> {
    tx.execute("DELETE FROM entry_tag WHERE entry_rid = ?1", [rid])?;
    for (name, key) in tags {
        tx.execute(
            "INSERT INTO tag (name, name_key) VALUES (?1, ?2) ON CONFLICT(name_key) DO NOTHING",
            params![name, key],
        )?;
        let tag_id: i64 = tx.query_row("SELECT id FROM tag WHERE name_key = ?1", [key], |row| {
            row.get(0)
        })?;
        tx.execute(
            "INSERT INTO entry_tag (entry_rid, tag_id) VALUES (?1, ?2)",
            params![rid, tag_id],
        )?;
    }
    Ok(())
}

fn split_tags(joined: Option<String>) -> Vec<String> {
    let mut tags: Vec<String> = match joined {
        Some(text) => text.split('\u{1f}').map(str::to_string).collect(),
        None => vec![],
    };
    tags.sort_by_key(|name| tag_key(name));
    tags
}

/// How tags are compared: composed Unicode form, then lowercase. This is why
/// «Работа» and «работа» are the same tag (SQLite's own NOCASE only knows ASCII).
fn tag_key(name: &str) -> String {
    name.nfc()
        .collect::<String>()
        .to_lowercase()
        .nfc()
        .collect()
}

/// Returns (the name to display, the key to compare by).
fn normalize_tag(name: &str) -> Result<(String, String)> {
    let display: String = name.trim().nfc().collect();
    if display.is_empty() {
        return Err(Error::InvalidInput("a tag name is empty".into()));
    }
    if display.chars().count() > MAX_TAG_CHARS {
        return Err(Error::InvalidInput(format!(
            "a tag name is longer than {MAX_TAG_CHARS} characters"
        )));
    }
    if display.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "a tag name contains a control character".into(),
        ));
    }
    let key = tag_key(&display);
    Ok((display, key))
}

/// Normalizes a list of tag names: drops duplicates (ignoring case), sorts by key.
fn normalize_tags(names: &[String]) -> Result<Vec<(String, String)>> {
    let mut tags: Vec<(String, String)> = Vec::new();
    for name in names {
        let tag = normalize_tag(name)?;
        if !tags.iter().any(|(_, key)| *key == tag.1) {
            tags.push(tag);
        }
    }
    tags.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(tags)
}

fn validate_draft(draft: &EntryDraft) -> Result<()> {
    validate_date(&draft.entry_date)?;
    if let Some(tz) = &draft.tz
        && (tz.is_empty() || tz.chars().count() > 64)
    {
        return Err(Error::InvalidInput(
            "time zone name has a bad length".into(),
        ));
    }
    Ok(())
}

/// Accepts only real calendar dates written as YYYY-MM-DD.
fn validate_date(date: &str) -> Result<()> {
    let bad = || Error::InvalidInput("entry_date must be a real date written YYYY-MM-DD".into());
    let bytes = date.as_bytes();
    if !date.is_ascii() || bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(bad());
    }
    let number = |text: &str| -> Result<u32> {
        if text.bytes().all(|b| b.is_ascii_digit()) {
            text.parse().map_err(|_| bad())
        } else {
            Err(bad())
        }
    };
    let (year, month, day) = (
        number(&date[0..4])?,
        number(&date[5..7])?,
        number(&date[8..10])?,
    );
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if year == 0 || day == 0 || day > days_in_month {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
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

    fn tagged(mut d: EntryDraft, tags: &[&str]) -> EntryDraft {
        d.tags = tags.iter().map(|t| t.to_string()).collect();
        d
    }

    fn ids(hits: &[SearchHit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    fn lseq(vault: &Vault) -> i64 {
        vault
            .database()
            .connection()
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='lseq'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// FTS5 compares its index with the entry table and reports any difference.
    fn assert_search_index_is_consistent(vault: &Vault) {
        vault
            .database()
            .connection()
            .execute(
                "INSERT INTO entry_fts(entry_fts, rank) VALUES ('integrity-check', 1)",
                [],
            )
            .expect("the search index disagrees with the entry table");
    }

    #[test]
    fn an_entry_can_be_created_and_read_back() {
        let (_tmp, mut vault) = new_vault();
        let mut d = draft("2026-10-11", "Воскресенье", "Сегодня **хороший** день.");
        d.mood = Some(4);
        d.tz = Some("Europe/Berlin".into());
        d.tags = vec!["Работа".into(), "идеи".into()];
        let created = vault.create_entry(&d).unwrap();

        assert_eq!(created.rev, 1);
        assert_eq!(created.created_at, created.updated_at);
        assert_eq!(created.entry_date, "2026-10-11");
        assert_eq!(created.title.as_deref(), Some("Воскресенье"));
        assert_eq!(created.body_md, "Сегодня **хороший** день.");
        assert_eq!(created.mood, Some(4));
        assert_eq!(created.tz.as_deref(), Some("Europe/Berlin"));
        assert_eq!(created.tags, ["идеи", "Работа"]); // sorted ignoring case
        assert_eq!(vault.get_entry(&created.id).unwrap(), created);

        let writer: String = vault
            .database()
            .connection()
            .query_row(
                "SELECT device_id FROM entry WHERE id = ?1",
                [&created.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(writer, vault.device_id);
    }

    #[test]
    fn an_update_makes_a_new_revision_and_an_identical_save_does_not() {
        let (_tmp, mut vault) = new_vault();
        let created = vault
            .create_entry(&draft("2026-10-11", "t", "first"))
            .unwrap();
        let counter = lseq(&vault);

        // Saving the same content again writes nothing.
        let same = vault
            .update_entry(&created.id, &draft("2026-10-11", "t", "first"))
            .unwrap();
        assert_eq!(same, created);
        assert_eq!(lseq(&vault), counter);

        let changed = vault
            .update_entry(&created.id, &draft("2026-10-11", "t", "second"))
            .unwrap();
        assert_eq!(changed.rev, 2);
        assert!(changed.updated_at > created.updated_at);
        assert_eq!(changed.created_at, created.created_at);
        assert_eq!(changed.body_md, "second");
        assert_eq!(lseq(&vault), counter + 1);
    }

    #[test]
    fn the_original_time_zone_is_kept_on_update() {
        let (_tmp, mut vault) = new_vault();
        let mut d = draft("2026-10-11", "t", "x");
        d.tz = Some("Europe/Berlin".into());
        let created = vault.create_entry(&d).unwrap();
        d.tz = Some("Asia/Tokyo".into());
        d.body_md = "y".into();
        let updated = vault.update_entry(&created.id, &d).unwrap();
        assert_eq!(updated.tz.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn the_change_counter_goes_up_with_every_write() {
        let (_tmp, mut vault) = new_vault();
        let start = lseq(&vault);
        let a = vault.create_entry(&draft("2026-10-11", "a", "a")).unwrap();
        let b = vault.create_entry(&draft("2026-10-11", "b", "b")).unwrap();
        vault
            .update_entry(&a.id, &draft("2026-10-11", "a", "a2"))
            .unwrap();
        vault.delete_entry(&b.id).unwrap();
        assert_eq!(lseq(&vault), start + 4);
    }

    #[test]
    fn deleting_leaves_a_scrubbed_tombstone() {
        let (_tmp, mut vault) = new_vault();
        let d = tagged(draft("2026-10-11", "секрет", "тайный текст"), &["личное"]);
        let created = vault.create_entry(&d).unwrap();
        vault.delete_entry(&created.id).unwrap();

        // The entry is gone for every reader...
        assert!(matches!(
            vault.get_entry(&created.id),
            Err(Error::EntryNotFound)
        ));
        assert!(matches!(
            vault.delete_entry(&created.id),
            Err(Error::EntryNotFound)
        ));
        assert!(matches!(
            vault.update_entry(&created.id, &d),
            Err(Error::EntryNotFound)
        ));
        assert!(
            vault
                .list_entries(&ListQuery::default())
                .unwrap()
                .items
                .is_empty()
        );
        assert!(vault.search("тайный", 10).unwrap().is_empty());
        assert!(vault.list_tags().unwrap().is_empty());

        // ...but the row is still there, with its identity and without its content.
        let row = vault
            .database()
            .connection()
            .query_row(
                "SELECT deleted_at, rev, title, body_md, mood,
                        (SELECT count(*) FROM entry_tag WHERE entry_rid = entry.rid)
                   FROM entry WHERE id = ?1",
                [&created.id],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();
        assert!(row.0.is_some());
        assert_eq!(row.1, 2);
        assert_eq!((row.2, row.3, row.4, row.5), (None, None, None, 0));
        assert_search_index_is_consistent(&vault);
    }

    #[test]
    fn russian_and_english_search() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&draft(
                "2026-10-01",
                "Покупки",
                "Сегодня купила Книгу о море.",
            ))
            .unwrap();
        let b = vault
            .create_entry(&draft("2026-10-02", "Вечер", "Читала книги до ночи."))
            .unwrap();
        let c = vault
            .create_entry(&draft("2026-10-03", "Список", "Одна книга лежит на столе."))
            .unwrap();
        let d = vault
            .create_entry(&draft(
                "2026-10-04",
                "English",
                "I bought a Book about the sea.",
            ))
            .unwrap();

        // Newest day first.
        assert_eq!(
            ids(&vault.search("книг", 10).unwrap()),
            [&c.id, &b.id, &a.id]
        );
        assert_eq!(
            ids(&vault.search("КНИГ", 10).unwrap()),
            [&c.id, &b.id, &a.id]
        );
        assert_eq!(ids(&vault.search("книгу", 10).unwrap()), [&a.id]);
        assert_eq!(ids(&vault.search("book", 10).unwrap()), [&d.id]);
        // The title is searched too.
        assert_eq!(ids(&vault.search("вечер", 10).unwrap()), [&b.id]);
        // All words must match; words under 3 characters are ignored.
        assert_eq!(ids(&vault.search("книг мор", 10).unwrap()), [&a.id]);
        assert_eq!(ids(&vault.search("книг о мор", 10).unwrap()), [&a.id]);
        assert!(vault.search("книг sea", 10).unwrap().is_empty());
        // Limit.
        assert_eq!(vault.search("книг", 2).unwrap().len(), 2);
        // Nothing to search for.
        assert!(matches!(vault.search("кн", 10), Err(Error::QueryTooShort)));
        assert!(matches!(
            vault.search("  о  я ", 10),
            Err(Error::QueryTooShort)
        ));
        assert!(matches!(vault.search("", 10), Err(Error::QueryTooShort)));
    }

    #[test]
    fn ё_and_й_are_not_folded_into_е_and_и() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&draft("2026-10-01", "t", "Нашёл ёлку, читал про войну."))
            .unwrap();
        assert_eq!(ids(&vault.search("нашёл", 10).unwrap()), [&a.id]);
        assert!(vault.search("нашел", 10).unwrap().is_empty());
        assert_eq!(ids(&vault.search("войну", 10).unwrap()), [&a.id]);
        assert!(vault.search("воину", 10).unwrap().is_empty());
    }

    #[test]
    fn a_decomposed_query_still_finds_composed_text() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&draft("2026-10-01", "t", "про войну"))
            .unwrap();
        // «войну» typed with «й» as «и» + combining breve.
        let decomposed = "во\u{438}\u{306}ну";
        assert_ne!(decomposed, "войну");
        assert_eq!(ids(&vault.search(decomposed, 10).unwrap()), [&a.id]);
    }

    #[test]
    fn search_operators_are_plain_text() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&draft(
                "2026-10-01",
                "t",
                "цена: 5* \"кавычки\" и NEAR(слово)",
            ))
            .unwrap();
        for query in [
            "5*",
            "\"кавычки\"",
            "NEAR(слово)",
            "кав*",
            "AND OR NOT",
            "\"",
            "title:t",
        ] {
            // None of these may raise a database error.
            let result = vault.search(query, 10);
            assert!(
                result.is_ok() || matches!(result, Err(Error::QueryTooShort)),
                "query {query:?} failed"
            );
        }
        assert_eq!(ids(&vault.search("\"кавычки\"", 10).unwrap()), [&a.id]);
        assert_eq!(ids(&vault.search("NEAR(слово)", 10).unwrap()), [&a.id]);
    }

    #[test]
    fn snippets_mark_the_match_with_control_characters() {
        let (_tmp, mut vault) = new_vault();
        vault
            .create_entry(&draft("2026-10-01", "t", "Сегодня купила Книгу о море."))
            .unwrap();
        let hits = vault.search("книг", 10).unwrap();
        println!("snippet: {:?}", hits[0].snippet);
        assert!(hits[0].snippet.contains(MARK_START));
        assert!(hits[0].snippet.contains(MARK_END));
        let marked: String = hits[0]
            .snippet
            .split(MARK_START)
            .nth(1)
            .unwrap()
            .split(MARK_END)
            .next()
            .unwrap()
            .to_string();
        assert_eq!(marked, "Книг");
    }

    #[test]
    fn a_long_body_gives_a_short_snippet_around_the_match() {
        let (_tmp, mut vault) = new_vault();
        let body = format!("{} жасмин {}", "слово ".repeat(200), "конец ".repeat(200));
        vault
            .create_entry(&draft("2026-10-01", "t", &body))
            .unwrap();
        let hits = vault.search("жасмин", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippet;
        assert!(
            snippet.chars().count() < 200,
            "snippet too long: {}",
            snippet.chars().count()
        );
        assert!(
            snippet.contains(&format!("{MARK_START}жасмин{MARK_END}")),
            "{snippet:?}"
        );
    }

    #[test]
    fn search_follows_edits_and_deletes() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&draft("2026-10-01", "t", "про яблоки"))
            .unwrap();
        assert_eq!(vault.search("яблок", 10).unwrap().len(), 1);

        vault
            .update_entry(&a.id, &draft("2026-10-01", "t", "про груши"))
            .unwrap();
        assert!(vault.search("яблок", 10).unwrap().is_empty());
        assert_eq!(vault.search("груш", 10).unwrap().len(), 1);

        // Changing only the tags does not disturb the index.
        vault
            .update_entry(
                &a.id,
                &tagged(draft("2026-10-01", "t", "про груши"), &["еда"]),
            )
            .unwrap();
        assert_eq!(vault.search("груш", 10).unwrap().len(), 1);

        vault.delete_entry(&a.id).unwrap();
        assert!(vault.search("груш", 10).unwrap().is_empty());
        assert_search_index_is_consistent(&vault);
    }

    #[test]
    fn tags_are_compared_ignoring_case_in_any_language() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(
                draft("2026-10-01", "a", "a"),
                &["Работа", "работа", " РАБОТА ", "Work"],
            ))
            .unwrap();
        assert_eq!(a.tags, ["Work", "Работа"]); // one «работа» (first spelling kept), sorted by case-folded name
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["работа"]))
            .unwrap();
        assert_eq!(b.tags, ["Работа"]); // the existing spelling is reused

        let tags = vault.list_tags().unwrap();
        assert_eq!(
            tags,
            [
                // Sorted by the case-folded name, so Latin comes before Cyrillic.
                TagCount {
                    name: "Work".into(),
                    entries: 1
                },
                TagCount {
                    name: "Работа".into(),
                    entries: 2
                }
            ]
        );
    }

    #[test]
    fn tag_changes_make_a_new_revision() {
        let (_tmp, mut vault) = new_vault();
        let a = vault.create_entry(&draft("2026-10-01", "a", "a")).unwrap();
        let counter = lseq(&vault);
        let b = vault
            .update_entry(&a.id, &tagged(draft("2026-10-01", "a", "a"), &["x1"]))
            .unwrap();
        assert_eq!(b.rev, 2);
        assert_eq!(b.tags, ["x1"]);
        assert_eq!(lseq(&vault), counter + 1);
        // Same tags in a different case: not a change.
        let c = vault
            .update_entry(&a.id, &tagged(draft("2026-10-01", "a", "a"), &["X1"]))
            .unwrap();
        assert_eq!(c.rev, 2);
        // Removing the tags is a change; the tag no longer shows in the list.
        let d = vault
            .update_entry(&a.id, &draft("2026-10-01", "a", "a"))
            .unwrap();
        assert_eq!(d.rev, 3);
        assert!(d.tags.is_empty());
        assert!(vault.list_tags().unwrap().is_empty());
    }

    #[test]
    fn bad_tag_names_are_refused() {
        let (_tmp, mut vault) = new_vault();
        for bad in ["", "   ", "a\nb", &"x".repeat(65)] {
            let result = vault.create_entry(&tagged(draft("2026-10-01", "a", "a"), &[bad]));
            assert!(matches!(result, Err(Error::InvalidInput(_))), "tag {bad:?}");
        }
        // Nothing was written by the failed attempts.
        assert!(
            vault
                .list_entries(&ListQuery::default())
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn bad_dates_are_refused() {
        let (_tmp, mut vault) = new_vault();
        for bad in [
            "",
            "2026-1-1",
            "26-10-11",
            "2026/10/11",
            "2026-13-01",
            "2026-00-10",
            "2026-10-00",
            "2026-02-29",
            "2026-04-31",
            "2026-10-32",
            "20261011xx",
            "2026-10-1é",
            "٢٠٢٦-١٠-١١",
        ] {
            let result = vault.create_entry(&draft(bad, "a", "a"));
            assert!(
                matches!(result, Err(Error::InvalidInput(_))),
                "date {bad:?}"
            );
        }
        for good in ["2026-10-11", "2024-02-29", "2000-02-29", "2026-12-31"] {
            assert!(
                vault.create_entry(&draft(good, "a", "a")).is_ok(),
                "date {good:?}"
            );
        }
        assert!(vault.create_entry(&draft("1900-02-29", "a", "a")).is_err()); // 1900 was not a leap year
    }

    #[test]
    fn on_this_day_reaches_back_years_not_sideways() {
        let (_tmp, mut vault) = new_vault();
        for (date, title) in [
            ("2023-10-15", "deleted"),
            ("2024-10-15", "two years back"),
            ("2025-10-15", "last year"),
            ("2025-10-16", "wrong day"),
            ("2026-10-15", "today itself"),
        ] {
            let saved = vault.create_entry(&draft(date, title, "text")).unwrap();
            if title == "deleted" {
                vault.delete_entry(&saved.id).unwrap();
            }
        }
        let hits = vault.on_this_day("2026-10-15").unwrap();
        let titles: Vec<&str> = hits.iter().map(|h| h.title.as_deref().unwrap_or("")).collect();
        assert_eq!(titles, ["last year", "two years back"]);
    }


    #[test]
    fn days_count_live_entries_and_the_list_filters_by_date() {
        let (_tmp, mut vault) = new_vault();
        let mut ids = Vec::new();
        for (date, title) in [
            ("2026-10-03", "a"),
            ("2026-10-03", "b"),
            ("2026-10-05", "c"),
            ("2026-11-01", "d"),
        ] {
            ids.push(vault.create_entry(&draft(date, title, "text")).unwrap());
        }
        vault.delete_entry(&ids[3].id).unwrap(); // tombstones do not count

        let days = vault.entry_days("2026-10-01", "2026-11-01").unwrap();
        assert_eq!(
            days,
            vec![
                DayCount { date: "2026-10-03".into(), entries: 2 },
                DayCount { date: "2026-10-05".into(), entries: 1 },
            ]
        );

        let page = vault
            .list_entries(&ListQuery {
                limit: 50,
                after: None,
                tag: None,
                date: Some("2026-10-03".into()),
            })
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.items.iter().all(|e| e.entry_date == "2026-10-03"));
    }

    #[test]
    fn the_list_is_paged_newest_first_without_gaps_or_repeats() {
        let (_tmp, mut vault) = new_vault();
        let mut created = Vec::new();
        for (date, title) in [
            ("2026-10-01", "a"),
            ("2026-10-03", "b"),
            ("2026-10-03", "c"), // same day as b, created later
            ("2026-10-02", "d"),
            ("2026-10-05", "e"),
        ] {
            created.push(
                vault
                    .create_entry(&draft(date, title, &format!("text {title}")))
                    .unwrap(),
            );
        }
        let expected = ["e", "c", "b", "d", "a"];

        let mut seen = Vec::new();
        let mut after = None;
        let mut pages = 0;
        loop {
            let page = vault
                .list_entries(&ListQuery {
                    limit: 2,
                    after: after.clone(),
                    tag: None,
                    date: None,
                })
                .unwrap();
            pages += 1;
            seen.extend(page.items.iter().map(|i| i.title.clone().unwrap()));
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }
        assert_eq!(seen, expected);
        assert_eq!(pages, 3);

        // A page that ends exactly at the last entry has no `next`.
        let all = vault
            .list_entries(&ListQuery {
                limit: 5,
                after: None,
                tag: None,
                date: None,
            })
            .unwrap();
        assert_eq!(all.items.len(), 5);
        assert!(all.next.is_none());
    }

    #[test]
    fn the_list_can_be_filtered_by_tag_and_shows_previews_and_tags() {
        let (_tmp, mut vault) = new_vault();
        let long_text = "я".repeat(500);
        vault
            .create_entry(&tagged(
                draft("2026-10-01", "one", &long_text),
                &["Работа", "идеи"],
            ))
            .unwrap();
        vault
            .create_entry(&tagged(draft("2026-10-02", "two", "short"), &["идеи"]))
            .unwrap();
        vault
            .create_entry(&draft("2026-10-03", "three", "none"))
            .unwrap();

        let by_tag = vault
            .list_entries(&ListQuery {
                tag: Some("РАБОТА".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_tag.items.len(), 1);
        let item = &by_tag.items[0];
        assert_eq!(item.title.as_deref(), Some("one"));
        assert_eq!(item.tags, ["идеи", "Работа"]);
        assert_eq!(item.preview.chars().count(), 200);

        let ideas = vault
            .list_entries(&ListQuery {
                tag: Some("идеи".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ideas.items.len(), 2);
        assert!(
            vault
                .list_entries(&ListQuery {
                    tag: Some("нет такого".into()),
                    ..Default::default()
                })
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn entries_survive_locking_and_reopening() {
        let (tmp, mut vault) = new_vault();
        let created = vault
            .create_entry(&tagged(draft("2026-10-11", "t", "про книги"), &["идеи"]))
            .unwrap();
        vault.lock();

        let reopened = Vault::open_with_passphrase(&tmp.path().join("v"), "pass phrase").unwrap();
        assert_eq!(reopened.get_entry(&created.id).unwrap(), created);
        assert_eq!(ids(&reopened.search("книг", 10).unwrap()), [&created.id]);
        assert_eq!(reopened.list_tags().unwrap().len(), 1);
    }

    #[test]
    fn the_search_index_stays_consistent_through_a_mixed_workload() {
        let (_tmp, mut vault) = new_vault();
        let mut live = Vec::new();
        for n in 0..30 {
            let e = vault
                .create_entry(&tagged(
                    draft(
                        "2026-10-01",
                        &format!("заголовок {n}"),
                        &format!("текст номер {n} про книгу"),
                    ),
                    &["a-tag"],
                ))
                .unwrap();
            live.push(e.id);
        }
        for (n, id) in live.iter().enumerate() {
            match n % 3 {
                0 => {
                    vault
                        .update_entry(
                            id,
                            &draft("2026-10-02", "новый", &format!("другой текст {n}")),
                        )
                        .unwrap();
                }
                1 => vault.delete_entry(id).unwrap(),
                _ => {}
            }
        }
        assert_search_index_is_consistent(&vault);
        assert_eq!(
            vault
                .list_entries(&ListQuery {
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .items
                .len(),
            20
        );
        // Only the 10 untouched entries still contain «книгу».
        assert_eq!(vault.search("книгу", 200).unwrap().len(), 10);
    }

    // -----------------------------------------------------------------------
    // Tag rename and merge
    // -----------------------------------------------------------------------

    type RowMeta = (i64, i64, i64, String, Option<i64>);

    /// (rev, updated_at, lseq, device_id, deleted_at) straight from the table,
    /// so that tombstones can be inspected too.
    fn row_meta(vault: &Vault, id: &str) -> RowMeta {
        vault
            .database()
            .connection()
            .query_row(
                "SELECT rev, updated_at, lseq, device_id, deleted_at FROM entry WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
    }

    /// Every name in the tag table, used or not, ordered by key.
    fn tag_rows(vault: &Vault) -> Vec<String> {
        let conn = vault.database().connection();
        let mut stmt = conn
            .prepare("SELECT name FROM tag ORDER BY name_key")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    fn tag_filter(name: &str) -> ListQuery {
        ListQuery {
            tag: Some(name.into()),
            ..Default::default()
        }
    }

    fn tc(name: &str, entries: i64) -> TagCount {
        TagCount {
            name: name.into(),
            entries,
        }
    }

    #[test]
    fn renaming_a_tag_changes_it_on_every_entry_that_uses_it() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["Работа", "идеи"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["работа"]))
            .unwrap();
        let c = vault
            .create_entry(&tagged(draft("2026-10-03", "c", "c"), &["идеи"]))
            .unwrap();

        // The old name is found ignoring case, the new one is trimmed.
        assert_eq!(vault.rename_tag("РАБОТА", "  Дело ").unwrap(), 2);

        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["Дело", "идеи"]);
        assert_eq!(vault.get_entry(&b.id).unwrap().tags, ["Дело"]);
        assert_eq!(vault.get_entry(&c.id).unwrap().tags, ["идеи"]);
        assert_eq!(vault.list_tags().unwrap(), [tc("Дело", 2), tc("идеи", 2)]);
        assert_eq!(tag_rows(&vault), ["Дело", "идеи"]);
        assert_eq!(
            vault.list_entries(&tag_filter("дело")).unwrap().items.len(),
            2
        );
        assert!(
            vault
                .list_entries(&tag_filter("работа"))
                .unwrap()
                .items
                .is_empty()
        );

        // Nothing but the tag changed.
        let a_now = vault.get_entry(&a.id).unwrap();
        assert_eq!(a_now.title, a.title);
        assert_eq!(a_now.body_md, a.body_md);
        assert_eq!(a_now.entry_date, a.entry_date);
        assert_eq!(a_now.created_at, a.created_at);
    }

    #[test]
    fn a_rename_makes_a_new_revision_of_exactly_the_entries_it_changes() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["x"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["x"]))
            .unwrap();
        let c = vault.create_entry(&draft("2026-10-03", "c", "c")).unwrap();
        let d = vault
            .create_entry(&tagged(draft("2026-10-04", "d", "d"), &["y"]))
            .unwrap();
        let before: Vec<RowMeta> = [&a, &b, &c, &d]
            .iter()
            .map(|e| row_meta(&vault, &e.id))
            .collect();
        let counter = lseq(&vault);

        assert_eq!(vault.rename_tag("x", "z").unwrap(), 2);

        for (entry, old) in [(&a, &before[0]), (&b, &before[1])] {
            let now = row_meta(&vault, &entry.id);
            assert_eq!(now.0, old.0 + 1, "rev");
            assert!(now.1 > old.1, "updated_at must move forward");
            assert_eq!(now.3, vault.device_id);
            assert!(now.2 > counter, "lseq must be newer than any earlier write");
            assert_eq!(now.4, None);
        }
        // Each entry has its own change number, and the counter moved once per entry.
        let (la, lb) = (row_meta(&vault, &a.id).2, row_meta(&vault, &b.id).2);
        assert_ne!(la, lb);
        assert_eq!(lseq(&vault), counter + 2);
        assert_eq!(la.max(lb), counter + 2);

        // Entries without the tag are exactly as they were.
        assert_eq!(row_meta(&vault, &c.id), before[2]);
        assert_eq!(row_meta(&vault, &d.id), before[3]);
    }

    #[test]
    fn a_rename_that_only_changes_the_capitalization_is_allowed() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["работа"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["работа"]))
            .unwrap();
        let counter = lseq(&vault);

        assert_eq!(vault.rename_tag("работа", "Работа").unwrap(), 2);
        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["Работа"]);
        assert_eq!(vault.get_entry(&b.id).unwrap().tags, ["Работа"]);
        assert_eq!(vault.list_tags().unwrap(), [tc("Работа", 2)]);
        assert_eq!(tag_rows(&vault), ["Работа"]); // renamed in place, no second tag
        // The new spelling is a change other devices must see.
        assert_eq!(row_meta(&vault, &a.id).0, 2);
        assert_eq!(row_meta(&vault, &b.id).0, 2);
        assert_eq!(lseq(&vault), counter + 2);

        // Saying the same thing again, in whatever case the old name is typed, is a no-op.
        let meta = row_meta(&vault, &a.id);
        assert_eq!(vault.rename_tag("РАБОТА", " Работа ").unwrap(), 0);
        assert_eq!(row_meta(&vault, &a.id), meta);
        assert_eq!(lseq(&vault), counter + 2);
    }

    #[test]
    fn a_rename_onto_another_tag_in_use_is_refused_and_changes_nothing() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["alpha"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["Beta"]))
            .unwrap();
        let counter = lseq(&vault);
        let (meta_a, meta_b) = (row_meta(&vault, &a.id), row_meta(&vault, &b.id));

        for target in ["Beta", "beta", " BETA "] {
            assert!(
                matches!(vault.rename_tag("alpha", target), Err(Error::TagExists)),
                "target {target:?}"
            );
        }
        assert_eq!(lseq(&vault), counter);
        assert_eq!(row_meta(&vault, &a.id), meta_a);
        assert_eq!(row_meta(&vault, &b.id), meta_b);
        assert_eq!(tag_rows(&vault), ["alpha", "Beta"]);
        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["alpha"]);
    }

    #[test]
    fn a_tag_nobody_uses_cannot_be_renamed_and_does_not_block_a_name() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["old"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["y"]))
            .unwrap();
        // Removing the tag from its only entry leaves an unused row behind (§4).
        vault
            .update_entry(&a.id, &draft("2026-10-01", "a", "a"))
            .unwrap();
        assert_eq!(tag_rows(&vault), ["old", "y"]);

        assert!(matches!(
            vault.rename_tag("old", "other"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("old", "y"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("y", "old"),
            Err(Error::TagNotFound)
        ));
        assert_eq!(tag_rows(&vault), ["old", "y"]);

        // The invisible leftover does not stand in the way of the name.
        assert_eq!(vault.rename_tag("y", "OLD").unwrap(), 1);
        assert_eq!(vault.get_entry(&b.id).unwrap().tags, ["OLD"]);
        assert_eq!(tag_rows(&vault), ["OLD"]);
    }

    #[test]
    fn unknown_and_invalid_tag_names_are_refused_and_change_nothing() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["x"]))
            .unwrap();
        vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["y"]))
            .unwrap();
        let counter = lseq(&vault);
        let meta = row_meta(&vault, &a.id);

        assert!(matches!(
            vault.rename_tag("nope", "z"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("nope", "x"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("x", "nope"),
            Err(Error::TagNotFound)
        ));
        // «Работа» and «работа» are one tag, so it cannot be merged into itself.
        for same in ["x", "X", "  x "] {
            assert!(
                matches!(vault.merge_tags("x", same), Err(Error::InvalidInput(_))),
                "{same:?}"
            );
        }
        let too_long = "x".repeat(65);
        for bad in ["", "   ", "a\nb", too_long.as_str()] {
            assert!(matches!(
                vault.rename_tag("x", bad),
                Err(Error::InvalidInput(_))
            ));
            assert!(matches!(
                vault.rename_tag(bad, "z"),
                Err(Error::InvalidInput(_))
            ));
            assert!(matches!(
                vault.merge_tags("x", bad),
                Err(Error::InvalidInput(_))
            ));
            assert!(matches!(
                vault.merge_tags(bad, "x"),
                Err(Error::InvalidInput(_))
            ));
        }
        assert_eq!(lseq(&vault), counter);
        assert_eq!(row_meta(&vault, &a.id), meta);
        assert_eq!(tag_rows(&vault), ["x", "y"]);
    }

    #[test]
    fn merging_moves_the_entries_and_removes_the_old_tag() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["old"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["old", "Keep"]))
            .unwrap();
        let c = vault
            .create_entry(&tagged(draft("2026-10-03", "c", "c"), &["Keep"]))
            .unwrap();
        let d = vault.create_entry(&draft("2026-10-04", "d", "d")).unwrap();
        let meta_c = row_meta(&vault, &c.id);
        let meta_d = row_meta(&vault, &d.id);
        let counter = lseq(&vault);

        assert_eq!(vault.merge_tags("OLD", "keep").unwrap(), 2);

        // The target keeps its own spelling; the entry that had both has it once.
        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["Keep"]);
        assert_eq!(vault.get_entry(&b.id).unwrap().tags, ["Keep"]);
        assert_eq!(vault.get_entry(&c.id).unwrap().tags, ["Keep"]);
        assert_eq!(vault.list_tags().unwrap(), [tc("Keep", 3)]);
        assert_eq!(tag_rows(&vault), ["Keep"]); // the old tag is gone for good
        assert!(
            vault
                .list_entries(&tag_filter("old"))
                .unwrap()
                .items
                .is_empty()
        );
        assert_eq!(
            vault.list_entries(&tag_filter("keep")).unwrap().items.len(),
            3
        );

        // Both changed entries are new revisions, including the one that already had `Keep`:
        // its tag set shrank.
        assert_eq!(row_meta(&vault, &a.id).0, 2);
        assert_eq!(row_meta(&vault, &b.id).0, 2);
        let (la, lb) = (row_meta(&vault, &a.id).2, row_meta(&vault, &b.id).2);
        assert_ne!(la, lb);
        assert!(la > counter && lb > counter);
        assert_eq!(lseq(&vault), counter + 2);
        // The others were not touched.
        assert_eq!(row_meta(&vault, &c.id), meta_c);
        assert_eq!(row_meta(&vault, &d.id), meta_d);
    }

    #[test]
    fn tombstoned_entries_are_never_touched_by_rename_or_merge() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["t"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "секрет", "тайна"), &["t", "u"]))
            .unwrap();
        let c = vault
            .create_entry(&tagged(draft("2026-10-03", "c", "c"), &["keep"]))
            .unwrap();
        vault.delete_entry(&b.id).unwrap();
        let dead = row_meta(&vault, &b.id);
        assert!(dead.4.is_some());
        let counter = lseq(&vault);

        // `u` was only ever used by the deleted entry, so the name is free.
        assert_eq!(vault.rename_tag("t", "u").unwrap(), 1);
        assert_eq!(row_meta(&vault, &b.id), dead);
        assert_eq!(lseq(&vault), counter + 1);
        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["u"]);

        assert_eq!(vault.merge_tags("u", "keep").unwrap(), 1);
        assert_eq!(row_meta(&vault, &b.id), dead);
        assert_eq!(lseq(&vault), counter + 2);
        assert_eq!(vault.list_tags().unwrap(), [tc("keep", 2)]);
        assert_eq!(vault.get_entry(&c.id).unwrap().tags, ["keep"]);

        // Still a scrubbed tombstone: no text, no tag links.
        let (title, body, links): (Option<String>, Option<String>, i64) = vault
            .database()
            .connection()
            .query_row(
                "SELECT title, body_md, (SELECT count(*) FROM entry_tag WHERE entry_rid = entry.rid)
                   FROM entry WHERE id = ?1",
                [&b.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((title, body, links), (None, None, 0));
        assert!(matches!(vault.get_entry(&b.id), Err(Error::EntryNotFound)));
    }

    #[test]
    fn a_tag_used_only_by_a_deleted_entry_does_not_exist_for_rename_or_merge() {
        let (_tmp, mut vault) = new_vault();
        vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["x"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["gone"]))
            .unwrap();
        vault.delete_entry(&b.id).unwrap();
        let counter = lseq(&vault);

        assert!(matches!(
            vault.rename_tag("gone", "z"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("gone", "x"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("x", "gone"),
            Err(Error::TagNotFound)
        ));
        assert_eq!(lseq(&vault), counter);
    }

    /// Tombstoning removes an entry's tag links, so a link from a tombstone should
    /// never exist. If one ever appears (a bug, or a future merge from another
    /// device), rename and merge must still treat the tombstone as not there.
    #[test]
    fn a_stray_tag_link_on_a_tombstone_is_ignored() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["t"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["t", "only-dead"]))
            .unwrap();
        vault.delete_entry(&b.id).unwrap();
        vault
            .database()
            .connection()
            .execute(
                "INSERT INTO entry_tag (entry_rid, tag_id)
                 SELECT e.rid, t.id FROM entry e, tag t WHERE e.id = ?1",
                [&b.id],
            )
            .unwrap();
        let dead = row_meta(&vault, &b.id);
        let counter = lseq(&vault);

        // A tag whose only link comes from the tombstone is not in use.
        assert!(matches!(
            vault.rename_tag("only-dead", "z"),
            Err(Error::TagNotFound)
        ));
        assert!(matches!(
            vault.merge_tags("only-dead", "t"),
            Err(Error::TagNotFound)
        ));
        // And the tombstone is not bumped along with the live entry.
        assert_eq!(vault.rename_tag("t", "u").unwrap(), 1);
        assert_eq!(row_meta(&vault, &b.id), dead);
        assert_eq!(lseq(&vault), counter + 1);
        assert_eq!(vault.get_entry(&a.id).unwrap().tags, ["u"]);
    }

    #[test]
    fn a_rename_never_moves_updated_at_backwards() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["x"]))
            .unwrap();
        // A clock that was once far ahead.
        let future = now_ms() + 86_400_000;
        vault
            .database()
            .connection()
            .execute(
                "UPDATE entry SET updated_at = ?1 WHERE id = ?2",
                params![future, a.id],
            )
            .unwrap();

        vault.rename_tag("x", "y").unwrap();
        assert_eq!(row_meta(&vault, &a.id).1, future + 1);
    }

    #[test]
    fn renaming_and_merging_leave_the_text_and_the_search_index_alone() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(
                draft("2026-10-01", "Вечер", "Читала книгу."),
                &["old"],
            ))
            .unwrap();
        let b = vault
            .create_entry(&tagged(
                draft("2026-10-02", "Утро", "Купила книги."),
                &["old", "other"],
            ))
            .unwrap();

        vault.rename_tag("old", "new").unwrap();
        vault.merge_tags("new", "other").unwrap();

        for before in [&a, &b] {
            let now = vault.get_entry(&before.id).unwrap();
            assert_eq!(now.title, before.title);
            assert_eq!(now.body_md, before.body_md);
        }
        // Newest first, both still found.
        assert_eq!(ids(&vault.search("книг", 10).unwrap()), [&b.id, &a.id]);
        assert_search_index_is_consistent(&vault);
    }

    #[test]
    fn renamed_and_merged_tags_survive_locking_and_reopening() {
        let (tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-01", "a", "a"), &["a1"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-02", "b", "b"), &["b1", "c1"]))
            .unwrap();
        vault.rename_tag("a1", "A2").unwrap();
        vault.merge_tags("b1", "c1").unwrap();
        let (a_now, b_now) = (
            vault.get_entry(&a.id).unwrap(),
            vault.get_entry(&b.id).unwrap(),
        );
        assert_eq!((a_now.rev, b_now.rev), (2, 2));
        vault.lock();

        let reopened = Vault::open_with_passphrase(&tmp.path().join("v"), "pass phrase").unwrap();
        assert_eq!(reopened.get_entry(&a.id).unwrap(), a_now);
        assert_eq!(reopened.get_entry(&b.id).unwrap(), b_now);
        assert_eq!(reopened.list_tags().unwrap(), [tc("A2", 1), tc("c1", 1)]);
    }

    #[test]
    fn delete_tag_removes_it_from_live_entries_only() {
        let (_tmp, mut vault) = new_vault();
        let a = vault
            .create_entry(&tagged(draft("2026-10-15", "a", "x"), &["work", "keep"]))
            .unwrap();
        let b = vault
            .create_entry(&tagged(draft("2026-10-14", "b", "x"), &["work"]))
            .unwrap();
        let gone = vault
            .create_entry(&tagged(draft("2026-10-13", "gone", "x"), &["work"]))
            .unwrap();
        vault.delete_entry(&gone.id).unwrap();

        let before = vault.get_entry(&a.id).unwrap();
        let removed = vault.delete_tag("work").unwrap();
        assert_eq!(removed, 2); // the tombstone does not count

        let a_after = vault.get_entry(&a.id).unwrap();
        assert_eq!(a_after.tags, vec!["keep".to_string()]);
        assert!(a_after.rev > before.rev); // a real change bumps the revision
        assert_eq!(vault.get_entry(&b.id).unwrap().tags, Vec::<String>::new());
        // The tag is gone from the list, and deleting it again fails.
        let counts = vault.list_tags().unwrap();
        assert!(counts.iter().all(|t| t.name != "work"));
        assert!(matches!(
            vault.delete_tag("work"),
            Err(Error::TagNotFound)
        ));
    }
}
