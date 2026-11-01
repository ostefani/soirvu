// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Logical folders: named organizers inside one vault.
//!
//! A folder is a row, not a directory: the vault folder on disk
//! (`folder.rs`) stays the encryption, unlock, backup and sync
//! boundary, while these rows only say which entries belong where.
//! Root is represented by NULL, not a folder row. Entry membership
//! is assigned once at creation and never reassigned; only folders
//! move.

use rusqlite::{OptionalExtension, Transaction, params};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::Vault;
use crate::entries::next_lseq;
use crate::error::{Error, Result};
use crate::storage::now_ms;

/// A live folder: id, name and position. Membership and parent
/// references intentionally have no hard foreign keys: sync can
/// deliver children before parents, and the effective tree is
/// resolved at read time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub rev: i64,
}

/// Maximum nesting depth, counting root-level folders as depth 1.
const MAX_DEPTH: i64 = 32;
/// Longest folder name, in Unicode scalar values.
const MAX_NAME_CHARS: usize = 100;

/// The display name and its comparison key. Trimmed, NFC, non-empty,
/// bounded, with no path separators, control characters or bare dots.
/// Comparison uses the shared [`crate::entries::normalize_key`].
fn normalize_name(raw: &str) -> Result<(String, String)> {
    let display: String = raw.trim().nfc().collect::<String>();
    if display.is_empty() {
        return Err(Error::InvalidInput("a folder name is empty".into()));
    }
    if display.chars().count() > MAX_NAME_CHARS {
        return Err(Error::InvalidInput(format!(
            "a folder name is longer than {MAX_NAME_CHARS} characters"
        )));
    }
    if display == "." || display == ".." {
        return Err(Error::InvalidInput(
            "a folder cannot be named . or ..".into(),
        ));
    }
    if display
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return Err(Error::InvalidInput(
            "a folder name cannot contain separators or control characters".into(),
        ));
    }
    let key = crate::entries::normalize_key(&display);
    Ok((display, key))
}

fn read_folder(conn: &rusqlite::Connection, id: &str) -> Result<Option<FolderRow>> {
    let found = conn
        .query_row(
            "SELECT id, parent_id, name, created_at, updated_at, deleted_at, rev
               FROM folder WHERE id = ?1",
            [id],
            |row| {
                Ok(FolderRow {
                    folder: Folder {
                        id: row.get(0)?,
                        parent_id: row.get(1)?,
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        rev: row.get(6)?,
                    },
                    deleted_at: row.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(found)
}

struct FolderRow {
    folder: Folder,
    deleted_at: Option<i64>,
}

impl From<FolderRow> for Folder {
    fn from(row: FolderRow) -> Folder {
        row.folder
    }
}

/// Whether a live (not tombstoned) folder with this id exists.
/// Whether a folder row exists and is not tombstoned. The Tauri
/// layer uses this for effective-hierarchy flags; core reads use it
/// for scope validation.
pub fn folder_is_live(conn: &rusqlite::Connection, id: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM folder WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false))
}

/// Ancestor ids from the immediate parent up, capped so a malformed
/// graph cannot loop forever. Returns an error on a cycle instead of
/// hanging: a cycle means the database is already inconsistent.
fn ancestors(conn: &rusqlite::Connection, mut parent: Option<String>) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    while let Some(id) = parent {
        if chain.len() > MAX_DEPTH as usize + 2 {
            return Err(Error::InvalidInput("folder ancestry is cyclic".into()));
        }
        if chain.contains(&id) {
            return Err(Error::FolderCycle);
        }
        chain.push(id.clone());
        parent = conn
            .query_row("SELECT parent_id FROM folder WHERE id = ?1", [&id], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten();
    }
    Ok(chain)
}

impl Vault {
    /// Creates a folder under `parent_id` (`None` is the vault root).
    pub fn create_folder(&mut self, parent_id: Option<&str>, name: &str) -> Result<Folder> {
        if let Some(parent) = parent_id {
            if !folder_is_live(self.db.connection(), parent)? {
                return Err(Error::FolderNotFound);
            }
        }
        if let Some(parent) = parent_id {
            let chain = ancestors(self.db.connection(), Some(parent.to_string()))?;
            if chain.len() as i64 + 1 > MAX_DEPTH {
                return Err(Error::FolderTooDeep);
            }
        }
        let (display, key) = normalize_name(name)?;
        let id = Uuid::now_v7().to_string();
        let now = now_ms();
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        reject_duplicate_sibling(&tx, parent_id, &key, &id)?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "INSERT INTO folder
               (id, parent_id, name, name_key, created_at, updated_at, rev, device_id, lseq, base_rev, base_device)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, ?6, ?7, 0, ?6)",
            params![id, parent_id, display, key, now, device, lseq],
        )?;
        tx.commit()?;
        Ok(Folder {
            id,
            parent_id: parent_id.map(str::to_string),
            name: display,
            created_at: now,
            updated_at: now,
            rev: 1,
        })
    }

    /// Inserts a folder row exactly as given (import/sync paths).
    /// Unlike `create_folder` this keeps the caller's id, revision
    /// and lineage, accepts raw parent references (a child may
    /// arrive before its parent; reads resolve them), and allows
    /// duplicate sibling names (independent offline creates collide
    /// legitimately). Live folders still need a valid name;
    /// scrubbed tombstones (empty name) pass through so the
    /// deletion survives the roundtrip.
    pub(crate) fn insert_folder_exact(
        &mut self,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        created_at: i64,
        updated_at: i64,
        deleted_at: Option<i64>,
        rev: i64,
        device_id: &str,
        base_rev: i64,
        base_device: &str,
    ) -> Result<()> {
        if !crate::storage::is_canonical_uuid(id)
            || !crate::storage::is_canonical_uuid(device_id)
            || (!base_device.is_empty() && !crate::storage::is_canonical_uuid(base_device))
            || parent_id.is_some_and(|p| !crate::storage::is_canonical_uuid(p))
            || rev < 1
            || base_rev < 0
        {
            return Err(Error::InvalidInput("folder record is invalid".into()));
        }
        let (display, key) = if deleted_at.is_some() && name.is_empty() {
            (String::new(), String::new())
        } else {
            let (display, key) = normalize_name(name)?;
            (display, key)
        };
        let tx = self.db.conn_mut().transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO device (id, name, created_at) VALUES (?1, 'import peer', ?2)",
            params![device_id, now_ms()],
        )?;
        let lseq = next_lseq(&tx)?;
        let changed = tx.execute(
            "INSERT INTO folder
               (id, parent_id, name, name_key, created_at, updated_at, deleted_at,
                rev, device_id, lseq, base_rev, base_device)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO NOTHING",
            params![
                id,
                parent_id,
                display,
                key,
                created_at,
                updated_at,
                deleted_at,
                rev,
                device_id,
                lseq,
                base_rev,
                base_device
            ],
        )?;
        tx.commit()?;
        if changed == 0 {
            return Err(Error::InvalidInput("folder id is already taken".into()));
        }
        Ok(())
    }

    /// How many live entries are filed directly in a folder (no
    /// descendants). The folder tree shows this per row.
    pub fn count_direct_entries(&self, folder_id: &str) -> Result<i64> {
        Ok(self.db.connection().query_row(
            "SELECT COUNT(*) FROM entry WHERE folder_id = ?1 AND deleted_at IS NULL",
            [folder_id],
            |row| row.get(0),
        )?)
    }

    /// All live folders, alphabetical by normalized name with the id as
    /// the stable tie-break, so every view lists them the same way.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.db.connection().prepare(
            "SELECT id, parent_id, name, created_at, updated_at, rev
               FROM folder WHERE deleted_at IS NULL
              ORDER BY name_key, id",
        )?;
        stmt.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                rev: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
    }

    /// A live folder by id.
    pub fn get_folder(&self, id: &str) -> Result<Folder> {
        match read_folder(self.db.connection(), id)? {
            Some(row) if row.deleted_at.is_none() => Ok(row.into()),
            _ => Err(Error::FolderNotFound),
        }
    }

    /// Renames a live folder. The name rules and the duplicate-sibling
    /// check are the same as on create. An unchanged name is a no-op:
    /// it is returned as-is without a revision bump. Any other change
    /// requires the revision the caller read, or fails revision_conflict.
    pub fn rename_folder(&mut self, id: &str, expected_rev: i64, name: &str) -> Result<Folder> {
        let row = read_folder(self.db.connection(), id)?;
        let row = match row {
            Some(row) if row.deleted_at.is_none() => row,
            _ => return Err(Error::FolderNotFound),
        };
        let (display, key) = normalize_name(name)?;
        if display == row.folder.name {
            return Ok(row.folder);
        }
        if row.folder.rev != expected_rev {
            return Err(Error::RevisionConflict);
        }
        let now = now_ms().max(row.folder.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        reject_duplicate_sibling(&tx, row.folder.parent_id.as_deref(), &key, id)?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "UPDATE folder
                SET name = ?1, name_key = ?2, updated_at = ?3, rev = rev + 1,
                    device_id = ?4, lseq = ?5
              WHERE id = ?6 AND deleted_at IS NULL",
            params![display, key, now, device, lseq, id],
        )?;
        tx.commit()?;
        let mut folder = row.folder;
        folder.name = display;
        folder.updated_at = now;
        folder.rev += 1;
        Ok(folder)
    }

    /// Moves a live folder under a new parent (`None` is the vault
    /// root). Only the folder's own parent/revision change: contained
    /// entries keep their folder, timestamps and revisions. An unchanged
    /// parent is a no-op. Any other change requires the revision the
    /// caller read, or fails revision_conflict.
    pub fn move_folder(
        &mut self,
        id: &str,
        expected_rev: i64,
        parent_id: Option<&str>,
    ) -> Result<Folder> {
        let row = read_folder(self.db.connection(), id)?;
        let row = match row {
            Some(row) if row.deleted_at.is_none() => row,
            _ => return Err(Error::FolderNotFound),
        };
        if row.folder.parent_id.as_deref() == parent_id {
            return Ok(row.folder);
        }
        if row.folder.rev != expected_rev {
            return Err(Error::RevisionConflict);
        }
        if let Some(parent) = parent_id {
            if !folder_is_live(self.db.connection(), parent)? {
                return Err(Error::FolderNotFound);
            }
        }
        let chain = ancestors(self.db.connection(), parent_id.map(str::to_string))?;
        if chain.iter().any(|ancestor| ancestor == id) {
            return Err(Error::FolderCycle);
        }
        if chain.len() as i64 + 1 > MAX_DEPTH {
            return Err(Error::FolderTooDeep);
        }
        let now = now_ms().max(row.folder.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        let name_key = crate::entries::normalize_key(&row.folder.name);
        reject_duplicate_sibling(&tx, parent_id, &name_key, id)?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "UPDATE folder
                SET parent_id = ?1, updated_at = ?2, rev = rev + 1,
                    device_id = ?3, lseq = ?4
              WHERE id = ?5 AND deleted_at IS NULL",
            params![parent_id, now, device, lseq, id],
        )?;
        tx.commit()?;
        let mut folder = row.folder;
        folder.parent_id = parent_id.map(str::to_string);
        folder.updated_at = now;
        folder.rev += 1;
        Ok(folder)
    }

    /// Renames and moves a live folder in one revision. The
    /// separate rename and move exist for callers that change one
    /// thing, but a combined call must not commit one half and fail
    /// the other — so everything is validated first (name, revision,
    /// parent liveness, cycles, depth, duplicate siblings) and a
    /// single statement writes both columns with one revision bump.
    /// A fully unchanged call is a no-op and skips the revision
    /// check, like the separate rename and move.
    pub fn update_folder(
        &mut self,
        id: &str,
        expected_rev: i64,
        parent_id: Option<&str>,
        name: &str,
    ) -> Result<Folder> {
        let row = read_folder(self.db.connection(), id)?;
        let row = match row {
            Some(row) if row.deleted_at.is_none() => row,
            _ => return Err(Error::FolderNotFound),
        };
        let (display, key) = normalize_name(name)?;
        let parent_changed = row.folder.parent_id.as_deref() != parent_id;
        if display == row.folder.name && !parent_changed {
            return Ok(row.folder);
        }
        if row.folder.rev != expected_rev {
            return Err(Error::RevisionConflict);
        }
        if parent_changed {
            if let Some(parent) = parent_id
                && !folder_is_live(self.db.connection(), parent)?
            {
                return Err(Error::FolderNotFound);
            }
            let chain = ancestors(self.db.connection(), parent_id.map(str::to_string))?;
            if chain.iter().any(|ancestor| ancestor == id) {
                return Err(Error::FolderCycle);
            }
            if chain.len() as i64 + 1 > MAX_DEPTH {
                return Err(Error::FolderTooDeep);
            }
        }
        let now = now_ms().max(row.folder.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        reject_duplicate_sibling(&tx, parent_id, &key, id)?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "UPDATE folder
                SET name = ?1, name_key = ?2, parent_id = ?3, updated_at = ?4, rev = rev + 1,
                    device_id = ?5, lseq = ?6
              WHERE id = ?7 AND deleted_at IS NULL",
            params![display, key, parent_id, now, device, lseq, id],
        )?;
        tx.commit()?;
        let mut folder = row.folder;
        folder.name = display;
        folder.parent_id = parent_id.map(str::to_string);
        folder.updated_at = now;
        folder.rev += 1;
        Ok(folder)
    }

    /// Deletes an empty live folder by writing a tombstone: the row
    /// stays (so a later sync knows the folder was deleted) but the
    /// name stops resolving. Non-empty means live entries or live
    /// subfolders, refused with FolderHasContents. Deleting a folder
    /// never touches its entries — but entries can only live in live
    /// folders, so a non-empty folder cannot be deleted at all.
    pub fn delete_folder(&mut self, id: &str, expected_rev: i64) -> Result<Folder> {
        let row = read_folder(self.db.connection(), id)?;
        let row = match row {
            Some(row) if row.deleted_at.is_none() => row,
            _ => return Err(Error::FolderNotFound),
        };
        if row.folder.rev != expected_rev {
            return Err(Error::RevisionConflict);
        }
        let conn = self.db.connection();
        let live_entries: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entry WHERE folder_id = ?1 AND deleted_at IS NULL",
            [id],
            |row| row.get(0),
        )?;
        let live_children: i64 = conn.query_row(
            "SELECT COUNT(*) FROM folder WHERE parent_id = ?1 AND deleted_at IS NULL",
            [id],
            |row| row.get(0),
        )?;
        if live_entries > 0 || live_children > 0 {
            return Err(Error::FolderHasContents);
        }
        let now = now_ms().max(row.folder.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        let lseq = next_lseq(&tx)?;
        let changed = tx.execute(
            "UPDATE folder
                SET deleted_at = ?1, updated_at = ?1, rev = rev + 1,
                    device_id = ?2, lseq = ?3
              WHERE id = ?4 AND deleted_at IS NULL",
            params![now, device, lseq, id],
        )?;
        debug_assert_eq!(changed, 1);
        tx.commit()?;
        let mut folder = row.folder;
        folder.updated_at = now;
        folder.rev += 1;
        Ok(folder)
    }

    /// Restores a tombstoned folder so its original structure shows up
    /// again: entries kept their assignment the whole time, so only the
    /// named folder is untombstoned. The parent chain must be live —
    /// otherwise the folder would restore into invisibility, so restore
    /// the parent first. Restoring a live folder is a no-op.
    pub fn restore_folder(&mut self, id: &str, expected_rev: i64) -> Result<Folder> {
        let row = read_folder(self.db.connection(), id)?.ok_or(Error::FolderNotFound)?;
        if row.deleted_at.is_none() {
            return Ok(row.folder);
        }
        if row.folder.rev != expected_rev {
            return Err(Error::RevisionConflict);
        }
        let mut ancestor = row.folder.parent_id.clone();
        while let Some(parent) = ancestor {
            match read_folder(self.db.connection(), &parent)? {
                Some(parent_row) if parent_row.deleted_at.is_none() => {
                    ancestor = parent_row.folder.parent_id;
                }
                _ => {
                    return Err(Error::InvalidInput(
                        "restore the parent folder first".into(),
                    ));
                }
            }
        }
        let now = now_ms().max(row.folder.updated_at + 1);
        let device = self.device_id.clone();
        let tx = self.db.conn_mut().transaction()?;
        let lseq = next_lseq(&tx)?;
        tx.execute(
            "UPDATE folder
                SET deleted_at = NULL, updated_at = ?1, rev = rev + 1,
                    device_id = ?2, lseq = ?3
              WHERE id = ?4",
            params![now, device, lseq, id],
        )?;
        tx.commit()?;
        let mut folder = row.folder;
        folder.updated_at = now;
        folder.rev += 1;
        Ok(folder)
    }
}

/// Rejects a live sibling with the same normalized name under the same
/// parent. Independent offline creates can legitimately collide, so
/// there is no unique database constraint: the check runs here, where
/// the caller gets a proper error instead of a constraint failure.
fn reject_duplicate_sibling(
    tx: &Transaction,
    parent_id: Option<&str>,
    name_key: &str,
    self_id: &str,
) -> Result<()> {
    let clash: Option<String> = tx
        .query_row(
            "SELECT id FROM folder
              WHERE ((parent_id = ?1) OR (parent_id IS NULL AND ?1 IS NULL))
                AND name_key = ?2 AND deleted_at IS NULL AND id <> ?3",
            params![parent_id, name_key, self_id],
            |row| row.get(0),
        )
        .optional()?;
    if clash.is_some() {
        return Err(Error::FolderExists);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::entries::{EntryDraft, normalize_key};
    use crate::storage::MIGRATIONS;
    use tempfile::TempDir;

    fn test_vault() -> (TempDir, crate::Vault) {
        let tmp = TempDir::new().unwrap();
        let (vault, _) = crate::Vault::create(
            &tmp.path().join("v"),
            "pass phrase",
            crate::crypto::KdfParams {
                m_kib: 64 * 1024,
                t: 3,
                p: 1,
            },
        )
        .unwrap();
        (tmp, vault)
    }

    fn draft(title: &str) -> EntryDraft {
        EntryDraft {
            entry_date: "2026-10-31".into(),
            title: Some(title.into()),
            body_md: "body".into(),
            ..Default::default()
        }
    }

    #[test]
    fn create_list_and_reject_bad_names() {
        let (_tmp, mut vault) = test_vault();
        let root = vault.create_folder(None, "Personal").unwrap();
        assert_eq!(root.parent_id, None);
        assert_eq!(root.rev, 1);
        let child = vault.create_folder(Some(&root.id), "Health").unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(root.id.as_str()));

        // Names sort normalized for display; listing is deterministic.
        let names: Vec<_> = vault
            .list_folders()
            .unwrap()
            .iter()
            .map(|f| f.name.clone())
            .collect();
        assert_eq!(names, vec!["Health".to_string(), "Personal".to_string()]);

        for bad in ["", "   ", ".", "..", "a/b", "a\\b", "a\x00b"] {
            assert!(
                vault.create_folder(None, bad).is_err(),
                "name {bad:?} must be rejected"
            );
        }
        // Case-insensitive duplicate siblings are refused; different
        // parents may reuse a name.
        assert!(vault.create_folder(None, "personal").is_err());
        let other = vault.create_folder(Some(&child.id), "Personal").unwrap();
        assert_eq!(other.parent_id.as_deref(), Some(child.id.as_str()));
        // Unknown parents are refused.
        assert!(vault.create_folder(Some("nope"), "X").is_err());
    }

    #[test]
    fn rename_and_move_keep_entries_put() {
        let (_tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let b = vault.create_folder(None, "B").unwrap();
        let moved = vault.move_folder(&b.id, 1, Some(&a.id)).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(moved.rev, 2);
        let renamed = vault.rename_folder(&b.id, 2, "Bee").unwrap();
        assert_eq!(renamed.name, "Bee");
        assert_eq!(renamed.rev, 3);
        // Moving into itself, into a descendant, or into a missing
        // folder is refused.
        assert!(vault.move_folder(&a.id, 1, Some(&a.id)).is_err());
        assert!(vault.move_folder(&a.id, 1, Some(&b.id)).is_err());
        assert!(vault.move_folder(&a.id, 1, Some("nope")).is_err());
        // Beyond the depth limit is refused: A plus 31 levels is depth
        // 32, so one more move would exceed it.
        let mut parent = a.id.clone();
        for i in 0..31 {
            let f = vault
                .create_folder(Some(&parent), &format!("L{i}"))
                .unwrap();
            parent = f.id;
        }
        assert!(vault.move_folder(&b.id, 3, Some(&parent)).is_err());
    }

    /// P2: the IPC adapter used to commit a rename before
    /// attempting the move, so a failed move left the rename
    /// persisted. The combined update validates everything first
    /// and writes once: a cyclic move fails with the name, parent
    /// and revision all untouched.
    #[test]
    fn combined_rename_and_move_is_atomic() {
        let (_tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let b = vault.create_folder(Some(&a.id), "B").unwrap();
        let err = vault
            .update_folder(&b.id, b.rev, Some(&b.id), "Bee")
            .unwrap_err();
        assert!(matches!(err, crate::Error::FolderCycle));
        let kept = vault.get_folder(&b.id).unwrap();
        assert_eq!(kept.name, "B");
        assert_eq!(kept.parent_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(kept.rev, b.rev);
        // The same call with a legal move applies both at once
        // with a single revision bump.
        let done = vault.update_folder(&b.id, b.rev, None, "Bee").unwrap();
        assert_eq!(done.name, "Bee");
        assert_eq!(done.parent_id, None);
        assert_eq!(done.rev, b.rev + 1);
        // A fully unchanged call is a no-op, like the separate
        // rename and move, and skips the revision check.
        let same = vault
            .update_folder(&done.id, done.rev + 99, None, "Bee")
            .unwrap();
        assert_eq!(same.rev, done.rev);
    }

    #[test]
    fn entries_keep_folder_through_update() {
        let (_tmp, mut vault) = test_vault();
        let folder = vault.create_folder(None, "Dev").unwrap();
        let entry = vault.create_entry(Some(&folder.id), &draft("One")).unwrap();
        assert_eq!(entry.folder_id.as_deref(), Some(folder.id.as_str()));
        let updated = vault
            .update_entry(
                &entry.id,
                entry.rev,
                &EntryDraft {
                    title: Some("Two".into()),
                    ..draft("ignored")
                },
            )
            .unwrap();
        assert_eq!(updated.folder_id.as_deref(), Some(folder.id.as_str()));
        // A stale revision fails instead of overwriting: the failed
        // edit must not touch the stored folder either.
        assert!(matches!(
            vault.update_entry(
                &entry.id,
                entry.rev,
                &EntryDraft {
                    title: Some("Stale".into()),
                    ..draft("ignored")
                },
            ),
            Err(crate::Error::RevisionConflict)
        ));
        assert_eq!(
            vault.get_entry(&entry.id).unwrap().folder_id.as_deref(),
            Some(folder.id.as_str())
        );
        // The search key follows the title.
        let key: Option<String> = vault
            .database()
            .connection()
            .query_row(
                "SELECT title_key FROM entry WHERE id = ?1",
                [&entry.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(key, Some(normalize_key("Two")));
        // Unknown folders are refused at creation.
        assert!(vault.create_entry(Some("nope"), &draft("X")).is_err());
    }

    #[test]
    fn move_folder_reopen_keeps_assignments() {
        let (tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let e = vault.create_entry(Some(&a.id), &draft("Day one")).unwrap();
        vault.move_folder(&a.id, 1, None).unwrap();
        drop(vault);
        let vault =
            crate::Vault::open_with_passphrase(&tmp.path().join("v"), "pass phrase").unwrap();
        let folders = vault.list_folders().unwrap();
        assert_eq!(folders.len(), 1);
        let entry = vault.get_entry(&e.id).unwrap();
        assert_eq!(entry.folder_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(entry.title.as_deref(), Some("Day one"));
    }

    /// Unchanged names and parents return the folder as-is: no
    /// revision bump, no conflict, even with a revision the folder
    /// has outgrown. Real changes with a stale revision fail before
    /// anything is written.
    #[test]
    fn stale_revisions_conflict_but_no_ops_ignore_them() {
        let (_tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let same = vault.rename_folder(&a.id, 99, "A").unwrap();
        assert_eq!(same.rev, 1);
        let same = vault.move_folder(&a.id, 99, None).unwrap();
        assert_eq!(same.rev, 1);
        assert!(matches!(
            vault.rename_folder(&a.id, 99, "B"),
            Err(crate::Error::RevisionConflict)
        ));
        assert!(matches!(
            vault.move_folder(&a.id, 99, Some(&a.id)),
            Err(crate::Error::RevisionConflict)
        ));
        // With the current revision the cycle is reported, not hidden
        // behind a conflict: the revision gate only guards writes.
        let b = vault.create_folder(Some(&a.id), "B").unwrap();
        assert!(matches!(
            vault.move_folder(&a.id, 1, Some(&b.id)),
            Err(crate::Error::FolderCycle)
        ));
        assert_eq!(vault.get_folder(&a.id).unwrap().rev, 1);
    }

    /// Deleting a folder needs it empty and at the caller's revision;
    /// restoring needs the parent chain live. Tombstones keep the row
    /// (sync replays them in Step 4) while reads behave as if the
    /// folder were gone.
    #[test]
    fn delete_needs_empty_and_restore_brings_back() {
        let (_tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let b = vault.create_folder(Some(&a.id), "B").unwrap();
        // A folder with a live subfolder cannot be deleted, nor can
        // one holding a live entry.
        assert!(matches!(
            vault.delete_folder(&a.id, 1),
            Err(crate::Error::FolderHasContents)
        ));
        vault.create_entry(Some(&b.id), &draft("Day one")).unwrap();
        assert!(matches!(
            vault.delete_folder(&b.id, 1),
            Err(crate::Error::FolderHasContents)
        ));
        // A stale revision fails before the emptiness check matters.
        let c = vault.create_folder(None, "C").unwrap();
        assert!(matches!(
            vault.delete_folder(&c.id, 7),
            Err(crate::Error::RevisionConflict)
        ));
        let tomb = vault.delete_folder(&c.id, 1).unwrap();
        assert_eq!(tomb.rev, 2);
        assert!(vault.list_folders().unwrap().iter().all(|f| f.id != c.id));
        assert!(matches!(
            vault.get_folder(&c.id),
            Err(crate::Error::FolderNotFound)
        ));
        assert!(vault.create_entry(Some(&c.id), &draft("X")).is_err());
        // A stale restore revision fails while the tombstone stands.
        assert!(matches!(
            vault.restore_folder(&c.id, 7),
            Err(crate::Error::RevisionConflict)
        ));
        // Restoring an already-live folder is a no-op without a bump.
        let live = vault.restore_folder(&a.id, 1).unwrap();
        assert_eq!(live.rev, 1);
        let restored = vault.restore_folder(&c.id, 2).unwrap();
        assert_eq!(restored.rev, 3);
        assert_eq!(vault.get_folder(&c.id).unwrap().rev, 3);
        // A restored folder is live again, so restoring once more
        // is a no-op that ignores even a stale revision.
        let again = vault.restore_folder(&c.id, 2).unwrap();
        assert_eq!(again.rev, 3);
        // An unknown id still fails.
        assert!(matches!(
            vault.restore_folder("nope", 1),
            Err(crate::Error::FolderNotFound)
        ));
    }

    /// A deleted parent hides its whole subtree: restoring the child
    /// first is refused, restoring the parent revives both levels
    /// with the hierarchy intact.
    #[test]
    fn restore_parent_first_then_child() {
        let (_tmp, mut vault) = test_vault();
        let a = vault.create_folder(None, "A").unwrap();
        let b = vault.create_folder(Some(&a.id), "B").unwrap();
        vault.delete_folder(&b.id, 1).unwrap();
        vault.delete_folder(&a.id, 1).unwrap();
        assert!(matches!(
            vault.restore_folder(&b.id, 2),
            Err(crate::Error::InvalidInput(_))
        ));
        vault.restore_folder(&a.id, 2).unwrap();
        let b = vault.restore_folder(&b.id, 2).unwrap();
        assert_eq!(b.parent_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(b.rev, 3);
        assert_eq!(vault.list_folders().unwrap().len(), 2);
    }

    /// A schema-4 database has no folders and no title keys. Opening it
    /// upgrades to 5, roots every existing entry, backfills title keys
    /// with the same normalizer live writes use, and leaves a pre-migration
    /// backup behind.
    #[test]
    fn schema4_upgrades_to_folders_and_title_keys() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("v");
        let key = test_key();
        // Build a schema-4 vault by hand: old migrations only.
        let db = crate::storage::create_with(&path, &key, "vault-1", &MIGRATIONS[..4]).unwrap();
        db.connection()
            .execute(
                "INSERT INTO device (id, name, created_at) VALUES ('dev-1', 'test', 1)",
                [],
            )
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO entry
                   (id, entry_date, created_at, updated_at, rev, device_id, lseq,
                    title, body_md, mood, tz, base_rev, base_device)
                 VALUES ('e1', '2026-10-01', 1, 1, 1, 'dev-1', 1,
                    'Training Notes', 'lift heavy', NULL, NULL, 0, 'dev-1')",
                [],
            )
            .unwrap();
        drop(db);

        let backup = tmp.path().join("backup");
        std::fs::create_dir(&backup).unwrap();
        let db = crate::storage::open_with(&path, &key, &backup, MIGRATIONS).unwrap();
        let version: i64 = db
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let (folder_id, title_key): (Option<String>, Option<String>) = db
            .connection()
            .query_row(
                "SELECT folder_id, title_key FROM entry WHERE id = 'e1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(folder_id, None);
        assert_eq!(title_key, Some(normalize_key("Training Notes")));
        // The pre-migration backup survived the upgrade.
        assert!(std::fs::read_dir(&backup).unwrap().next().is_some());
        // The upgraded database opens through the normal path.
        drop(db);
        crate::storage::Database::open(&path, &key, &backup).unwrap();
    }

    #[test]
    fn broken_migration_leaves_schema4_usable() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("v");
        let key = test_key();
        crate::storage::create_with(&path, &key, "vault-1", &MIGRATIONS[..4]).unwrap();
        let backup = tmp.path().join("backup");
        std::fs::create_dir(&backup).unwrap();
        let broken: Vec<&str> = MIGRATIONS[..4]
            .iter()
            .copied()
            .chain(std::iter::once("THIS IS NOT SQL;"))
            .collect();
        assert!(crate::storage::open_with(&path, &key, &backup, &broken).is_err());
        // Still schema 4: the failed upgrade changed nothing.
        let db = crate::storage::open_with(&path, &key, &backup, &MIGRATIONS[..4]).unwrap();
        let version: i64 = db
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    fn test_key() -> crate::crypto::Key {
        zeroize::Zeroizing::new([7u8; 32])
    }
}
