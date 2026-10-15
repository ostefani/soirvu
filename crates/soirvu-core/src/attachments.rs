//! Attachment blobs: encrypted files on disk, metadata in the database.
//!
//! Everything here is a method on `Vault`, so none of it can be used without
//! unlocking the vault first — the same rule as `entries.rs`.
//!
//! Blob format (DESIGN §5, "Attachments"):
//!
//! ```text
//! header   magic "SOIRVUAT" (8 bytes) | version (1 byte) | nonce prefix (19 bytes)
//! chunk n  XChaCha20-Poly1305 ciphertext of up to 64 KiB of plaintext, plus the
//!          16-byte authentication tag. The 24-byte nonce is the header's prefix,
//!          a 32-bit big-endian chunk counter and a last-chunk flag byte — the
//!          STREAM layout (BE32 variant), so chunk n sits at a fixed file offset
//!          and can be decrypted alone, which is what Range reads need (§6.4).
//! ```
//!
//! The per-blob key is `HKDF(attachment_key, info = attachment_id)` and every
//! chunk's AAD binds it to the attachment id, the format version and the chunk
//! number, so a blob copied onto another attachment's row does not decrypt.
//!
//! Write order (§7): the blob is written to a temporary file, fsynced, renamed
//! into place, and only then is the database row committed. If the commit
//! fails the blob is removed again. Deleting an attachment tombstones the row
//! (and bumps its entry, like a tag change) and removes the blob: deleted
//! content must be gone from the disk, not just hidden.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use data_encoding::HEXLOWER;
use hkdf::Hkdf;
use rusqlite::{Connection, Transaction, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::{Key, NONCE_LEN, random_bytes};
use crate::entries::next_lseq;
use crate::error::{Error, Result};
use crate::folder::Vault;
use crate::storage::now_ms;

/// Plaintext bytes per chunk. Every chunk except the last holds exactly this
/// many, so a plaintext offset maps to exactly one chunk index and one offset.
pub const CHUNK_PLAIN: usize = 64 * 1024;

const TAG_LEN: usize = 16;
const MAGIC: &[u8; 8] = b"SOIRVUAT";
const BLOB_VERSION: u8 = 1;
/// Random per blob; the nonce is prefix (19) + counter (4) + last flag (1).
const PREFIX_LEN: usize = 19;
const HEADER_LEN: u64 = (8 + 1 + PREFIX_LEN) as u64;
const MAX_NAME_CHARS: usize = 255;

/// One attachment, as the rest of the app sees it. The metadata lives in the
/// encrypted database; the content lives in the blob file named by `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub entry_id: String,
    /// Original file name. Safe to store: the database is encrypted.
    pub name: String,
    /// Sniffed by the caller in Rust, never taken from the UI (§6.1).
    pub mime: String,
    /// Plaintext size in bytes.
    pub size: i64,
    /// SHA-256 of the plaintext, as lowercase hex.
    pub sha256: String,
    pub created_at: i64,
}

/// What the startup sweep (§7) found in the attachments folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSweep {
    /// Blob files no live attachment row points to. Removed.
    pub orphans_removed: usize,
    /// Temporary files from uploads that never finished. Removed.
    pub incomplete_removed: usize,
    /// Ids of live rows whose blob file is not there. Reported, never deleted:
    /// the row is the only record that the attachment ever existed.
    pub missing: Vec<String>,
}

impl Vault {
    /// Stores `data` as a new attachment of a live entry.
    /// A thin wrapper over `add_attachment_from_reader` for data that is
    /// already in memory; large files should use the reader form.
    pub fn add_attachment(
        &mut self,
        entry_id: &str,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<Attachment> {
        self.add_attachment_from_reader(entry_id, name, mime, &mut io::Cursor::new(data))
    }

    /// Streams an attachment from `reader` without ever holding the whole
    /// plaintext (or ciphertext) in memory: one 64 KiB chunk at a time is
    /// hashed, encrypted and written.
    ///
    /// Adding an attachment changes its entry (DESIGN §4: `updated_at` is
    /// bumped on every change, "including tag and attachment changes"), so the
    /// entry gets a new `rev`, `updated_at`, `device_id` and `lseq` inside the
    /// same transaction as the attachment row. The row gets its own `lseq`.
    pub fn add_attachment_from_reader(
        &mut self,
        entry_id: &str,
        name: &str,
        mime: &str,
        reader: &mut impl Read,
    ) -> Result<Attachment> {
        let name = validate_name(name)?;
        let mime = validate_mime(mime)?;
        let entry_rid = live_entry_rid(self.db.connection(), entry_id)?;

        let id = Uuid::now_v7().to_string();
        let key = blob_key(&self.attachment_key(), &id);
        let (size, sha256) = write_blob(self.dir(), &id, &key, reader)?;

        let device = self.device_id.clone();
        let commit = (|| -> Result<Attachment> {
            let tx = self.db.conn_mut().transaction()?;
            let lseq = next_lseq(&tx)?;
            let created_at = now_ms();
            tx.execute(
                "INSERT INTO attachment
                   (id, entry_id, name, mime, size, sha256, created_at, deleted_at, lseq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)",
                params![id, entry_id, name, mime, size, &sha256[..], created_at, lseq],
            )?;
            bump_entry(&tx, entry_rid, &device)?;
            tx.commit()?;
            Ok(Attachment {
                id: id.clone(),
                entry_id: entry_id.to_string(),
                name,
                mime,
                size,
                sha256: HEXLOWER.encode(&sha256),
                created_at,
            })
        })();
        if commit.is_err() {
            // The row was not committed, so the blob is an orphan. Remove it
            // now; if even that fails, the startup sweep will find it.
            remove_blob(self.dir(), &id);
        }
        commit
    }

    /// Metadata for one live attachment. A tombstoned or unknown id is
    /// `AttachmentNotFound`.
    pub fn get_attachment(&self, id: &str) -> Result<Attachment> {
        Ok(load_attachment(self.db.connection(), id)?.1)
    }

    /// All live attachments of one live entry, oldest first (the order they
    /// were added to the entry).
    pub fn list_attachments(&self, entry_id: &str) -> Result<Vec<Attachment>> {
        live_entry_rid(self.db.connection(), entry_id)?;
        let mut stmt = self.db.connection().prepare(
            "SELECT id, entry_id, name, mime, size, sha256, created_at
               FROM attachment
              WHERE entry_id = ?1 AND deleted_at IS NULL
              ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([entry_id], row_to_attachment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The whole plaintext of one attachment, verified end to end: every
    /// chunk must authenticate, the total size must match the row, and the
    /// SHA-256 of the plaintext must match the row. A truncated, extended or
    /// tampered blob is `CorruptAttachment`, never partial plaintext.
    pub fn read_attachment(&self, id: &str) -> Result<Vec<u8>> {
        let (_, att) = load_attachment(self.db.connection(), id)?;
        let root = self.attachment_key();
        let (mut blob, prefix) = open_blob(self.dir(), &att)?;
        let mut out = Vec::with_capacity(att.size as usize);
        let mut hasher = Sha256::new();
        for counter in 0..chunk_count(att.size as u64) {
            let plain = read_chunk(&mut blob, &att, &root, &prefix, counter)?;
            hasher.update(&plain);
            out.extend_from_slice(&plain);
        }
        if out.len() as i64 != att.size || HEXLOWER.encode(&hasher.finalize()) != att.sha256 {
            return Err(corrupt(&att.id, "content does not match its row"));
        }
        Ok(out)
    }

    /// The plaintext bytes in `range` (clamped to the attachment), decrypting
    /// only the chunks the range touches. This is what the `soirvu-asset://`
    /// protocol (§6.4) answers HTTP Range requests with. Each touched chunk
    /// is authenticated; the whole-file hash is not (and cannot be) checked
    /// for a partial read.
    pub fn read_attachment_range(&self, id: &str, range: Range<u64>) -> Result<Vec<u8>> {
        let (_, att) = load_attachment(self.db.connection(), id)?;
        if range.start > range.end {
            return Err(Error::InvalidInput("range start is after its end".into()));
        }
        let size = att.size as u64;
        if range.start > size {
            return Err(Error::InvalidInput(
                "range starts past the end of the attachment".into(),
            ));
        }
        let end = range.end.min(size);
        if range.start == end {
            return Ok(Vec::new());
        }
        let chunk = CHUNK_PLAIN as u64;
        let first = range.start / chunk;
        let last = (end - 1) / chunk;
        let root = self.attachment_key();
        let (mut blob, prefix) = open_blob(self.dir(), &att)?;
        blob.seek(SeekFrom::Start(
            HEADER_LEN + first * (CHUNK_PLAIN + TAG_LEN) as u64,
        ))?;
        let mut out = Vec::with_capacity((end - range.start) as usize);
        for counter in first..=last {
            let plain = read_chunk(&mut blob, &att, &root, &prefix, counter as u32)?;
            let chunk_start = counter * chunk;
            let from = (range.start.max(chunk_start) - chunk_start) as usize;
            let to = (end.min(chunk_start + chunk) - chunk_start) as usize;
            out.extend_from_slice(&plain[from..to]);
        }
        Ok(out)
    }

    /// Deletes an attachment: the row is tombstoned (its identity and `lseq`
    /// stay, so a later sync learns about the deletion) and its entry is
    /// bumped, in one transaction; then the blob file is removed, so the
    /// content is gone from the disk and not just hidden.
    pub fn delete_attachment(&mut self, id: &str) -> Result<()> {
        let (_, att) = load_attachment(self.db.connection(), id)?;
        let entry_rid = live_entry_rid(self.db.connection(), &att.entry_id)?;
        let device = self.device_id.clone();
        {
            let tx = self.db.conn_mut().transaction()?;
            let lseq = next_lseq(&tx)?;
            tx.execute(
                "UPDATE attachment SET deleted_at = ?1, lseq = ?2
                  WHERE id = ?3 AND deleted_at IS NULL",
                params![now_ms(), lseq, att.id],
            )?;
            bump_entry(&tx, entry_rid, &device)?;
            tx.commit()?;
        }
        remove_blob(self.dir(), &att.id);
        Ok(())
    }

    /// Startup sweep (§7): removes blob files no live row points to (which
    /// includes the blobs of tombstoned attachments whose removal was
    /// interrupted) and temporary files from unfinished uploads, and reports
    /// live rows whose blob is missing. Removes nothing else.
    pub fn sweep_attachments(&self) -> Result<AttachmentSweep> {
        let live: HashSet<String> = {
            let mut stmt = self
                .db
                .connection()
                .prepare("SELECT id FROM attachment WHERE deleted_at IS NULL")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<HashSet<_>>>()?
        };
        let mut sweep = AttachmentSweep {
            missing: live
                .iter()
                .filter(|id| !blob_path(self.dir(), id).exists())
                .cloned()
                .collect(),
            ..Default::default()
        };
        sweep.missing.sort();

        let root = self.dir().join("attachments");
        if !root.exists() {
            return Ok(sweep);
        }
        for shard in fs::read_dir(&root)? {
            let shard = shard?.path();
            if !shard.is_dir() {
                continue;
            }
            for file in fs::read_dir(&shard)? {
                let file = file?;
                let name = file.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') && name.ends_with(".tmp") {
                    fs::remove_file(file.path())?;
                    sweep.incomplete_removed += 1;
                } else if !live.contains(&name) {
                    fs::remove_file(file.path())?;
                    sweep.orphans_removed += 1;
                }
            }
        }
        Ok(sweep)
    }
}

// ---------------------------------------------------------------------------
// Paths and files
// ---------------------------------------------------------------------------

/// `attachments/ab/<id>` — sharded by the id's first two characters so no
/// folder ever holds every blob. The file name is the id and nothing else:
/// no original name or type leaks (§3).
pub(crate) fn blob_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("attachments").join(&id[..2]).join(id)
}

fn tmp_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("attachments")
        .join(&id[..2])
        .join(format!(".{id}.tmp"))
}

/// Best-effort blob removal: a failure here is not fatal, because the row is
/// already tombstoned (or was never committed) and the startup sweep removes
/// whatever is left.
pub(crate) fn remove_blob(dir: &Path, id: &str) {
    let _ = fs::remove_file(blob_path(dir, id));
}

/// Encrypts everything from `reader` into the blob file for `id`.
/// Returns the plaintext size and its SHA-256 (raw bytes).
pub(crate) fn write_blob(dir: &Path, id: &str, key: &Key, reader: &mut impl Read) -> Result<(i64, [u8; 32])> {
    let final_path = blob_path(dir, id);
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(dir, id);
    let written = (|| -> Result<(i64, [u8; 32])> {
        let prefix: [u8; PREFIX_LEN] = random_bytes()?;
        let mut file = File::create(&tmp)?;
        file.write_all(MAGIC)?;
        file.write_all(&[BLOB_VERSION])?;
        file.write_all(&prefix)?;

        let mut hasher = Sha256::new();
        let mut size: i64 = 0;
        let mut counter: u32 = 0;
        // A chunk can only be marked "last" once the reader proves there is
        // no next chunk, so one chunk is always held back.
        let mut current = read_up_to(reader, CHUNK_PLAIN)?;
        loop {
            let next = read_up_to(reader, CHUNK_PLAIN)?;
            let last = next.is_empty();
            hasher.update(&current);
            size = size
                .checked_add(current.len() as i64)
                .ok_or_else(|| Error::InvalidInput("attachment is too large".into()))?;
            let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).expect("blob key is 32 bytes");
            let encrypted = cipher
                .encrypt(
                    &XNonce::from(chunk_nonce(&prefix, counter, last)),
                    Payload {
                        msg: &current,
                        aad: &chunk_aad(id, counter),
                    },
                )
                .map_err(|_| corrupt(id, "encryption failed"))?;
            file.write_all(&encrypted)?;
            if last {
                break;
            }
            current = next;
            counter = counter
                .checked_add(1)
                .ok_or_else(|| Error::InvalidInput("attachment is too large".into()))?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &final_path)?;
        #[cfg(unix)]
        if let Some(parent) = final_path.parent() {
            File::open(parent)?.sync_all()?;
        }
        let digest = hasher.finalize();
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&digest);
        Ok((size, sha256))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// Opens a blob and checks it against its row before any chunk is read: the
/// header must be ours, and the file length must be exactly what the row's
/// size implies — the cheapest possible truncation/extension check.
/// Returns the file (positioned at the first chunk) and the nonce prefix.
fn open_blob(dir: &Path, att: &Attachment) -> Result<(File, [u8; PREFIX_LEN])> {
    let path = blob_path(dir, &att.id);
    let mut file = File::open(&path).map_err(|_| corrupt(&att.id, "blob file is missing"))?;
    if file.metadata()?.len() != expected_len(att.size) {
        return Err(corrupt(&att.id, "blob file has the wrong length"));
    }
    let mut header = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC || header[8] != BLOB_VERSION {
        return Err(corrupt(&att.id, "blob header is not ours"));
    }
    let prefix: [u8; PREFIX_LEN] = header[9..].try_into().expect("prefix length");
    Ok((file, prefix))
}

/// Reads and decrypts the chunk the file is currently positioned at.
/// `counter` must be that chunk's number: a wrong position is caught anyway,
/// because the counter is part of the nonce and the AAD.
fn read_chunk(
    file: &mut File,
    att: &Attachment,
    attachment_key: &Key,
    prefix: &[u8; PREFIX_LEN],
    counter: u32,
) -> Result<Vec<u8>> {
    let size = att.size as u64;
    let plain_len = plain_len_of_chunk(size, counter);
    let mut encrypted = vec![0u8; plain_len + TAG_LEN];
    file.read_exact(&mut encrypted)
        .map_err(|_| corrupt(&att.id, "blob file ends inside a chunk"))?;

    let last = counter as u64 + 1 == chunk_count(size) as u64;
    let key = blob_key(attachment_key, &att.id);
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).expect("blob key is 32 bytes");
    cipher
        .decrypt(
            &XNonce::from(chunk_nonce(prefix, counter, last)),
            Payload {
                msg: &encrypted,
                aad: &chunk_aad(&att.id, counter),
            },
        )
        .map_err(|_| corrupt(&att.id, "a chunk failed authentication"))
}

fn chunk_count(size: u64) -> u32 {
    if size == 0 {
        1 // an empty attachment is one empty final chunk
    } else {
        size.div_ceil(CHUNK_PLAIN as u64) as u32
    }
}

fn plain_len_of_chunk(size: u64, counter: u32) -> usize {
    let start = counter as u64 * CHUNK_PLAIN as u64;
    if size == 0 {
        0
    } else {
        (size - start).min(CHUNK_PLAIN as u64) as usize
    }
}

fn expected_len(size: i64) -> u64 {
    let size = size as u64;
    HEADER_LEN + size + chunk_count(size) as u64 * TAG_LEN as u64
}

fn chunk_nonce(prefix: &[u8; PREFIX_LEN], counter: u32, last: bool) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[..PREFIX_LEN].copy_from_slice(prefix);
    nonce[PREFIX_LEN..PREFIX_LEN + 4].copy_from_slice(&counter.to_be_bytes());
    nonce[NONCE_LEN - 1] = u8::from(last);
    nonce
}

fn chunk_aad(id: &str, counter: u32) -> Vec<u8> {
    format!("soirvu-attachment\nversion={BLOB_VERSION}\nid={id}\nchunk={counter}").into_bytes()
}

/// Per-blob key (DESIGN §5): `HKDF(attachment_key, info = attachment_id)`.
pub(crate) fn blob_key(attachment_key: &Key, id: &str) -> Key {
    let hkdf = Hkdf::<Sha256>::new(None, &attachment_key[..]);
    let mut out = Zeroizing::new([0u8; 32]);
    hkdf.expand(id.as_bytes(), &mut *out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

fn corrupt(id: &str, what: &str) -> Error {
    Error::CorruptAttachment(format!("attachment {id}: {what}"))
}

/// Reads until `buf` is full or the reader ends; a short read is not an error.
fn read_up_to(reader: &mut impl Read, n: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut filled = 0;
    while filled < n {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(m) => filled += m,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::Io(e)),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

fn row_to_attachment(row: &rusqlite::Row) -> rusqlite::Result<Attachment> {
    let sha: Vec<u8> = row.get(5)?;
    Ok(Attachment {
        id: row.get(0)?,
        entry_id: row.get(1)?,
        name: row.get(2)?,
        mime: row.get(3)?,
        size: row.get(4)?,
        sha256: HEXLOWER.encode(&sha),
        created_at: row.get(6)?,
    })
}

/// Reads one live attachment and its internal row id.
fn load_attachment(conn: &Connection, id: &str) -> Result<(i64, Attachment)> {
    let found = conn.query_row(
        "SELECT rid, id, entry_id, name, mime, size, sha256, created_at
           FROM attachment WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |row| {
            let sha: Vec<u8> = row.get(6)?;
            Ok((
                row.get::<_, i64>(0)?,
                Attachment {
                    id: row.get(1)?,
                    entry_id: row.get(2)?,
                    name: row.get(3)?,
                    mime: row.get(4)?,
                    size: row.get(5)?,
                    sha256: HEXLOWER.encode(&sha),
                    created_at: row.get(7)?,
                },
            ))
        },
    );
    match found {
        Ok(found) => Ok(found),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(Error::AttachmentNotFound),
        Err(other) => Err(other.into()),
    }
}

fn live_entry_rid(conn: &Connection, entry_id: &str) -> Result<i64> {
    let found = conn.query_row(
        "SELECT rid FROM entry WHERE id = ?1 AND deleted_at IS NULL",
        [entry_id],
        |row| row.get(0),
    );
    match found {
        Ok(rid) => Ok(rid),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(Error::EntryNotFound),
        Err(other) => Err(other.into()),
    }
}

/// Records "this entry changed" for an attachment change, exactly like
/// `update_entry` does: new `rev`, `updated_at` that never goes backwards,
/// the writing device, and a change counter of the entry's own.
fn bump_entry(tx: &Transaction, entry_rid: i64, device: &str) -> Result<()> {
    let lseq = next_lseq(tx)?;
    tx.execute(
        "UPDATE entry
            SET updated_at = max(?1, updated_at + 1), rev = rev + 1, device_id = ?2, lseq = ?3
          WHERE rid = ?4 AND deleted_at IS NULL",
        params![now_ms(), device, lseq, entry_rid],
    )?;
    Ok(())
}

fn validate_name(name: &str) -> Result<String> {
    let name: String = name.trim().into();
    if name.is_empty() {
        return Err(Error::InvalidInput("attachment name is empty".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::InvalidInput(format!(
            "attachment name is longer than {MAX_NAME_CHARS} characters"
        )));
    }
    // The name is only ever stored in the encrypted database and shown in the
    // UI, but one day it becomes a file name again ("Save a copy…", export),
    // so path separators and control characters are refused here, once.
    if name.chars().any(char::is_control) || name.contains('/') || name.contains('\\') {
        return Err(Error::InvalidInput(
            "attachment name contains a forbidden character".into(),
        ));
    }
    Ok(name)
}

fn validate_mime(mime: &str) -> Result<String> {
    let mime = mime.trim().to_ascii_lowercase();
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    let mut parts = mime.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(top), Some(sub), None) if token(top) && token(sub) => Ok(mime),
        _ => Err(Error::InvalidInput(
            "mime must look like type/subtype".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::entries::{EntryDraft, ListQuery};
    use tempfile::TempDir;

    fn fast() -> KdfParams {
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        }
    }

    fn new_vault_with_entry() -> (TempDir, Vault, String) {
        let tmp = TempDir::new().unwrap();
        let (mut vault, _recovery) =
            Vault::create(&tmp.path().join("v"), "pass phrase", fast()).unwrap();
        let entry = vault
            .create_entry(&EntryDraft {
                entry_date: "2026-10-13".into(),
                title: Some("День".into()),
                body_md: "текст".into(),
                ..Default::default()
            })
            .unwrap();
        (tmp, vault, entry.id)
    }

    /// Deterministic bytes that are not all the same, so chunk offsets show up.
    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    fn sha256_hex(data: &[u8]) -> String {
        HEXLOWER.encode(&Sha256::digest(data))
    }

    #[test]
    fn an_attachment_can_be_stored_and_read_back() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let data = b"not really a jpeg";
        let att = vault
            .add_attachment(&entry_id, "photo.jpg", "image/jpeg", data)
            .unwrap();
        assert_eq!(att.entry_id, entry_id);
        assert_eq!(att.name, "photo.jpg");
        assert_eq!(att.mime, "image/jpeg");
        assert_eq!(att.size, data.len() as i64);
        assert_eq!(att.sha256, sha256_hex(data));
        assert_eq!(vault.get_attachment(&att.id).unwrap(), att.clone());
        assert_eq!(vault.list_attachments(&entry_id).unwrap(), [att.clone()]);
        assert_eq!(vault.read_attachment(&att.id).unwrap(), data);
        // The blob is a file named by id, in a shard folder named by its
        // first two characters — the original name appears nowhere on disk.
        let path = blob_path(vault.dir(), &att.id);
        assert!(path.exists());
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            att.id.as_str()
        );
        let on_disk = fs::read(&path).unwrap();
        assert!(!on_disk
            .windows(b"photo.jpg".len())
            .any(|w| w == b"photo.jpg"));
    }

    #[test]
    fn an_empty_attachment_round_trips() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let att = vault
            .add_attachment(&entry_id, "empty.bin", "application/octet-stream", b"")
            .unwrap();
        assert_eq!(att.size, 0);
        assert_eq!(att.sha256, sha256_hex(b""));
        assert!(vault.read_attachment(&att.id).unwrap().is_empty());
        assert!(
            vault
                .read_attachment_range(&att.id, 0..0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn chunk_boundaries_round_trip() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        for n in [
            CHUNK_PLAIN - 1,
            CHUNK_PLAIN,
            CHUNK_PLAIN + 1,
            3 * CHUNK_PLAIN + 123,
        ] {
            let data = pattern(n);
            let att = vault
                .add_attachment(&entry_id, "a.bin", "application/octet-stream", &data)
                .unwrap();
            assert_eq!(vault.read_attachment(&att.id).unwrap(), data, "size {n}");
        }
    }

    #[test]
    fn the_reader_form_streams_the_same_result() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let data = pattern(200_000);
        let mut cursor = io::Cursor::new(&data);
        let att = vault
            .add_attachment_from_reader(
                &entry_id,
                "big.bin",
                "application/octet-stream",
                &mut cursor,
            )
            .unwrap();
        assert_eq!(att.size, data.len() as i64);
        assert_eq!(vault.read_attachment(&att.id).unwrap(), data);
    }

    #[test]
    fn range_reads_return_the_right_bytes() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let data = pattern(3 * CHUNK_PLAIN + 500);
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", &data)
            .unwrap();
        let size = data.len() as u64;
        for (start, end) in [
            (0, 10),
            (100, 100), // empty
            (CHUNK_PLAIN as u64 - 5, CHUNK_PLAIN as u64 + 5), // crosses a boundary
            (CHUNK_PLAIN as u64, 2 * CHUNK_PLAIN as u64),     // exactly one chunk
            (size - 7, size),                                 // the tail
            (0, size),                                        // everything
            (size, size),                                     // empty at the end
            (10, size + 10_000),                              // clamped
        ] {
            let got = vault.read_attachment_range(&att.id, start..end).unwrap();
            let want = &data[start as usize..end.min(size) as usize];
            assert_eq!(got, want, "range {start}..{end}");
        }
        assert!(matches!(
            vault.read_attachment_range(&att.id, size + 1..size + 2),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn nothing_readable_is_in_the_blob() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let canary = "секретное вложение Ж7Qx".repeat(5000);
        let att = vault
            .add_attachment(&entry_id, "note.txt", "text/plain", canary.as_bytes())
            .unwrap();
        let on_disk = fs::read(blob_path(vault.dir(), &att.id)).unwrap();
        assert!(!on_disk
            .windows("секретное".len())
            .any(|w| w == "секретное".as_bytes()));
        assert!(on_disk.starts_with(MAGIC));
    }

    #[test]
    fn a_tampered_blob_fails_closed() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let data = pattern(2 * CHUNK_PLAIN);
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", &data)
            .unwrap();
        let path = blob_path(vault.dir(), &att.id);
        let mut bytes = fs::read(&path).unwrap();
        // Flip one byte inside the SECOND chunk.
        bytes[HEADER_LEN as usize + CHUNK_PLAIN + TAG_LEN + 10] ^= 0x01;
        fs::write(&path, &bytes).unwrap();

        assert!(matches!(
            vault.read_attachment(&att.id),
            Err(Error::CorruptAttachment(_))
        ));
        // A range inside the damaged chunk fails…
        assert!(matches!(
            vault.read_attachment_range(&att.id, CHUNK_PLAIN as u64..CHUNK_PLAIN as u64 + 100),
            Err(Error::CorruptAttachment(_))
        ));
        // …but a range that only touches the intact first chunk still reads:
        // chunks really are independent.
        let got = vault.read_attachment_range(&att.id, 0..100).unwrap();
        assert_eq!(got, &data[..100]);
    }

    #[test]
    fn a_truncated_or_extended_blob_fails_closed() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", b"hello")
            .unwrap();
        let path = blob_path(vault.dir(), &att.id);
        let bytes = fs::read(&path).unwrap();

        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(matches!(
            vault.read_attachment(&att.id),
            Err(Error::CorruptAttachment(_))
        ));

        let mut longer = bytes.clone();
        longer.push(0);
        fs::write(&path, &longer).unwrap();
        assert!(matches!(
            vault.read_attachment(&att.id),
            Err(Error::CorruptAttachment(_))
        ));
    }

    #[test]
    fn a_blob_swapped_between_attachments_does_not_decrypt() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let a = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", b"same size!")
            .unwrap();
        let b = vault
            .add_attachment(&entry_id, "b.bin", "application/octet-stream", b"same size?")
            .unwrap();
        let pa = blob_path(vault.dir(), &a.id);
        let pb = blob_path(vault.dir(), &b.id);
        let (ba, bb) = (fs::read(&pa).unwrap(), fs::read(&pb).unwrap());
        fs::write(&pa, &bb).unwrap();
        fs::write(&pb, &ba).unwrap();
        // The per-blob key and the AAD both name the attachment id.
        assert!(matches!(
            vault.read_attachment(&a.id),
            Err(Error::CorruptAttachment(_))
        ));
        assert!(matches!(
            vault.read_attachment(&b.id),
            Err(Error::CorruptAttachment(_))
        ));
    }

    #[test]
    fn adding_and_deleting_bump_the_entry() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let before = vault.get_entry(&entry_id).unwrap();
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", b"x")
            .unwrap();
        let after_add = vault.get_entry(&entry_id).unwrap();
        assert_eq!(after_add.rev, before.rev + 1);
        assert!(after_add.updated_at > before.updated_at);
        assert_eq!(after_add.body_md, before.body_md);

        vault.delete_attachment(&att.id).unwrap();
        let after_delete = vault.get_entry(&entry_id).unwrap();
        assert_eq!(after_delete.rev, after_add.rev + 1);
        assert!(after_delete.updated_at > after_add.updated_at);
        assert_eq!(
            vault
                .list_entries(&ListQuery::default())
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn deleting_an_attachment_tombstones_it_and_removes_the_blob() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", b"gone soon")
            .unwrap();
        let path = blob_path(vault.dir(), &att.id);
        vault.delete_attachment(&att.id).unwrap();

        assert!(matches!(
            vault.get_attachment(&att.id),
            Err(Error::AttachmentNotFound)
        ));
        assert!(matches!(
            vault.read_attachment(&att.id),
            Err(Error::AttachmentNotFound)
        ));
        assert!(matches!(
            vault.delete_attachment(&att.id),
            Err(Error::AttachmentNotFound)
        ));
        assert!(vault.list_attachments(&entry_id).unwrap().is_empty());
        assert!(!path.exists(), "the blob must be gone, not just hidden");

        // The row is still there, tombstoned, with no live link from lists.
        let deleted_at: Option<i64> = vault
            .database()
            .connection()
            .query_row(
                "SELECT deleted_at FROM attachment WHERE id = ?1",
                [&att.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
    }

    #[test]
    fn deleting_an_entry_tombstones_its_attachments_and_removes_their_blobs() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", b"with entry")
            .unwrap();
        let path = blob_path(vault.dir(), &att.id);
        vault.delete_entry(&entry_id).unwrap();

        assert!(matches!(
            vault.get_attachment(&att.id),
            Err(Error::AttachmentNotFound)
        ));
        assert!(!path.exists());
        let deleted_at: Option<i64> = vault
            .database()
            .connection()
            .query_row(
                "SELECT deleted_at FROM attachment WHERE id = ?1",
                [&att.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
    }

    #[test]
    fn unknown_and_misplaced_attachments_are_refused() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        assert!(matches!(
            vault.get_attachment("no-such-id"),
            Err(Error::AttachmentNotFound)
        ));
        assert!(matches!(
            vault.add_attachment("no-such-entry", "a.bin", "text/plain", b"x"),
            Err(Error::EntryNotFound)
        ));
        assert!(matches!(
            vault.list_attachments("no-such-entry"),
            Err(Error::EntryNotFound)
        ));
        // A deleted entry takes no new attachments.
        vault.delete_entry(&entry_id).unwrap();
        assert!(matches!(
            vault.add_attachment(&entry_id, "a.bin", "text/plain", b"x"),
            Err(Error::EntryNotFound)
        ));
    }

    #[test]
    fn bad_names_and_mime_types_are_refused_and_write_nothing() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let before = vault.get_entry(&entry_id).unwrap();
        for name in ["", "   ", "a/b.bin", "a\\b.bin", "a\nb.bin", &"x".repeat(256)] {
            assert!(
                matches!(
                    vault.add_attachment(&entry_id, name, "text/plain", b"x"),
                    Err(Error::InvalidInput(_))
                ),
                "name {name:?}"
            );
        }
        for mime in ["", "text", "text/", "/plain", "text/plain/extra", "text pl/ain"] {
            assert!(
                matches!(
                    vault.add_attachment(&entry_id, "a.bin", mime, b"x"),
                    Err(Error::InvalidInput(_))
                ),
                "mime {mime:?}"
            );
        }
        assert!(vault.list_attachments(&entry_id).unwrap().is_empty());
        assert_eq!(vault.get_entry(&entry_id).unwrap(), before); // no bump
        assert!(!vault.dir().join("attachments").exists()); // no files either
    }

    #[test]
    fn mime_is_stored_lowercase() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let att = vault
            .add_attachment(&entry_id, "a.JPG", "IMAGE/JPEG", b"x")
            .unwrap();
        assert_eq!(att.mime, "image/jpeg");
        assert_eq!(att.name, "a.JPG"); // the name keeps its spelling
    }

    #[test]
    fn the_sweep_removes_orphans_and_temporary_files_and_reports_missing() {
        let (_tmp, mut vault, entry_id) = new_vault_with_entry();
        let keep = vault
            .add_attachment(&entry_id, "keep.bin", "application/octet-stream", b"keep")
            .unwrap();
        let lost = vault
            .add_attachment(&entry_id, "lost.bin", "application/octet-stream", b"lost")
            .unwrap();
        let shard = blob_path(vault.dir(), &keep.id)
            .parent()
            .unwrap()
            .to_path_buf();
        fs::write(shard.join("orphan-file"), b"nobody points here").unwrap();
        fs::write(shard.join(".deadbeef.tmp"), b"half an upload").unwrap();
        fs::remove_file(blob_path(vault.dir(), &lost.id)).unwrap();

        let sweep = vault.sweep_attachments().unwrap();
        assert_eq!(sweep.orphans_removed, 1);
        assert_eq!(sweep.incomplete_removed, 1);
        assert_eq!(sweep.missing, [lost.id.clone()]);
        // The live blob is untouched and the missing row is NOT deleted.
        assert_eq!(vault.read_attachment(&keep.id).unwrap(), b"keep");
        assert_eq!(vault.get_attachment(&lost.id).unwrap(), lost);
        assert!(matches!(
            vault.read_attachment(&lost.id),
            Err(Error::CorruptAttachment(_))
        ));
    }

    #[test]
    fn attachments_survive_locking_and_reopening() {
        let (tmp, mut vault, entry_id) = new_vault_with_entry();
        let data = pattern(150_000);
        let att = vault
            .add_attachment(&entry_id, "a.bin", "application/octet-stream", &data)
            .unwrap();
        vault.lock();

        let reopened = Vault::open_with_passphrase(&tmp.path().join("v"), "pass phrase").unwrap();
        assert_eq!(reopened.get_attachment(&att.id).unwrap(), att);
        assert_eq!(reopened.read_attachment(&att.id).unwrap(), data);
        assert_eq!(
            reopened
                .read_attachment_range(&att.id, 65_000..66_000)
                .unwrap(),
            &data[65_000..66_000]
        );
    }
}
