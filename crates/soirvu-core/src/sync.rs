//! Bundle sync (DESIGN §13 Phase 10a): the transport-free
//! core. A device EMITS its changes since the last emit as
//! one encrypted bundle file in a sync folder; a device
//! APPLIES every bundle in that folder it has not already
//! incorporated. Nothing here opens a socket or knows what
//! moves the folder — a synced folder, a USB stick, or a
//! future Bluetooth/LAN transport are all just couriers.
//!
//! A bundle is the full records (entries incl. tombstones,
//! attachment metadata) whose lseq passed the emit
//! watermark, JSON-encoded and sealed with XChaCha20-
//! Poly1305 under a key derived from the vault master
//! (HKDF label "soirvu/v1/sync"); the header values (vault,
//! device, lseq range) are bound as AAD. The filename
//! carries the vault's first 8 characters and the FULL
//! device id; at apply time the plaintext header line is
//! read before any decryption, so a bundle dropped in the
//! wrong vault's folder is skipped quietly, and a device's
//! own bundles are recognised by their full device id —
//! never by a prefix.
//! Attachment BLOBS travel next to the bundles as the plain
//! files `blobs/<id>` — they are already ciphertext.
//!
//! Merge rule per entry (deterministic, so every device
//! resolves the same pair of records the same way):
//! same author device — the higher rev wins (fast-forward);
//! different authors — the higher (rev, device_id) wins the
//! entry. Each record also carries its base: the (device,
//! rev) its author started from, which the core records on
//! every authorship change. If the incoming record's base is
//! exactly my current state, it is a clean fast-forward;
//! if both sides moved away from their bases, the loser's text is kept as
//! a conflict copy entry with a deterministic id, so the
//! same copy — never two — appears on every device. The
//! copy starts its own lineage: a deterministic synthetic
//! author derived from the divergence and a base pointing
//! at the copy itself, so every device builds the
//! identical record and an edit of the copy is an ordinary
//! authorship change that travels like any other edit. A
//! losing tombstone preserves nothing (it holds no text).
//! Applying is idempotent: re-running over the same folder
//! is a no-op, and a device's own bundles are skipped. One
//! case is loud instead: a bundle carrying MY device id
//! whose range reaches past my own emit watermark cannot
//! be mine — another copy of this vault (a folder copy
//! that was never restamped) is writing under my identity.
//! Its changes are reported, not applied; the remedy is
//! `Vault::restamp_device` on one of the copies.

use std::fs;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::crypto::{NONCE_LEN, random_bytes};
use crate::entries::next_lseq;
use crate::error::{Error, Result};
use crate::folder::{Vault, rename_retry};

const BUNDLE_FORMAT: u32 = 1;
const EMITTED_KEY: &str = "sync_emitted_lseq";
/// Bundle names already reported as a twin's (see
/// sync_apply): the warning is loud ONCE per file, not on
/// every run for as long as the stale file sits there.
const WARNED_KEY: &str = "sync_warned_bundles";

/// What one emit produced (or None when nothing changed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBundleInfo {
    pub file_name: String,
    pub from_lseq: i64,
    pub to_lseq: i64,
    pub entries: usize,
    pub attachments: usize,
}

/// What one apply over a sync folder did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub bundles_seen: usize,
    pub bundles_applied: usize,
    pub entries_applied: usize,
    pub entries_conflicted: usize,
    pub attachments_applied: usize,
    pub problems: Vec<String>,
}

/// The plaintext bundle header: metadata only (the same
/// values the filename carries). It is the AEAD's AAD, so
/// any tampering with it fails decryption.
#[derive(Serialize, Deserialize)]
struct BundleHeader {
    format: u32,
    vault_id: String,
    device_id: String,
    from_lseq: i64,
    to_lseq: i64,
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    entries: Vec<EntryRecord>,
    attachments: Vec<AttachmentRecord>,
}

#[derive(Serialize, Deserialize, Clone)]
struct EntryRecord {
    id: String,
    entry_date: String,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
    rev: i64,
    device_id: String,
    base_rev: i64,
    base_device: String,
    title: Option<String>,
    body_md: Option<String>,
    mood: Option<i64>,
    tz: Option<String>,
    tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AttachmentRecord {
    id: String,
    entry_id: String,
    name: String,
    mime: String,
    size: i64,
    sha256: String,
    created_at: i64,
    deleted_at: Option<i64>,
}

fn header_line(header: &BundleHeader) -> Vec<u8> {
    serde_json::to_vec(header).expect("the bundle header always serializes")
}

fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

/// Splits a bundle file name, `<vault8>-<device>-<from>-
/// <to>.sync`. The device part is a full id and contains
/// '-' itself, so the range is split off from the right.
/// Returns (vault prefix, from, to).
fn parse_bundle_name(name: &str) -> Option<(String, i64, i64)> {
    let stem = name.strip_suffix(".sync")?;
    let (rest, to) = stem.rsplit_once('-')?;
    let (rest, from) = rest.rsplit_once('-')?;
    let (vault, _device) = rest.split_once('-')?;
    Some((
        vault.to_string(),
        from.parse().ok()?,
        to.parse().ok()?,
    ))
}

/// Reads just the plaintext header line of a bundle — the
/// same bytes `open_bundle` authenticates as AAD, so
/// anything decided from it in `sync_apply` (skip a
/// foreign vault, recognise my own bundle) is re-verified
/// there before a single record is trusted.
fn header_of(bytes: &[u8]) -> Option<BundleHeader> {
    let newline = bytes.iter().position(|b| *b == b'\n')?;
    serde_json::from_slice(&bytes[..newline]).ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The deterministic id of the conflict copy for a losing
/// record: every device resolving the same pair derives
/// the same id, so the copies merge instead of multiplying.
fn conflict_id(entry_id: &str, loser: &EntryRecord) -> String {
    conflict_digest("soirvu-conflict\n", entry_id, loser)
}

/// The shared derivation behind `conflict_id` and
/// `conflict_author`: a hash of the divergence — the
/// entry, the loser's author and rev, and the loser's
/// CONTENT. The content matters: two copies of one vault
/// share a device id, so their losers can agree on
/// (author, rev) while carrying different text; without
/// the content they would map to one copy id, and the
/// second text would be dropped as "already existing".
fn conflict_digest(domain: &str, entry_id: &str, loser: &EntryRecord) -> String {
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update(entry_id.as_bytes());
    hash.update(b"\n");
    hash.update(loser.device_id.as_bytes());
    hash.update(b"\n");
    hash.update(loser.rev.to_string().as_bytes());
    hash.update(b"\n");
    // Serialised as JSON so the field boundaries are
    // unambiguous. Timestamps are provenance, not
    // content, and stay out.
    let content = serde_json::to_vec(&(
        &loser.entry_date,
        &loser.title,
        &loser.body_md,
        loser.mood,
        &loser.tz,
        loser.deleted_at.is_some(),
        &loser.tags,
    ))
    .expect("record content always serializes");
    hash.update(&content);
    let digest = hash.finalize();
    let h = hex(&digest[..16]);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// The deterministic author of a conflict copy, derived
/// from the same inputs as the copy's id: every device
/// resolving this divergence stamps the copy with this
/// author — never with its own device id — so the records
/// they build are identical.
fn conflict_author(entry_id: &str, loser: &EntryRecord) -> String {
    conflict_digest("soirvu-conflict-author\n", entry_id, loser)
}

impl Vault {
    /// Emits one bundle with every change since the last
    /// emit into `sync_dir` (created if missing), copying
    /// any new attachment blobs next to it. Returns None
    /// when there is nothing new.
    pub fn sync_emit(&mut self, sync_dir: &Path) -> Result<Option<SyncBundleInfo>> {
        let watermark: i64 = self
            .db
            .meta(EMITTED_KEY)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let (entries, attachments, to_lseq) = {
            let conn = self.db.connection();
            let mut stmt = conn.prepare(
                "SELECT id, entry_date, created_at, updated_at, deleted_at, rev,
                        device_id, title, body_md, mood, tz, base_rev, base_device
                   FROM entry WHERE lseq > ?1 ORDER BY lseq",
            )?;
            let entries = stmt
                .query_map([watermark], |row| {
                    Ok(EntryRecord {
                        id: row.get(0)?,
                        entry_date: row.get(1)?,
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                        deleted_at: row.get(4)?,
                        rev: row.get(5)?,
                        device_id: row.get(6)?,
                        title: row.get(7)?,
                        body_md: row.get(8)?,
                        mood: row.get(9)?,
                        tz: row.get(10)?,
                        base_rev: row.get(11)?,
                        base_device: row.get(12)?,
                        tags: Vec::new(),
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);
            let mut entries = entries;
            for entry in &mut entries {
                entry.tags = entry_tags(conn, &entry.id)?;
            }
            let mut stmt = conn.prepare(
                "SELECT id, entry_id, name, mime, size, sha256, created_at, deleted_at
                   FROM attachment WHERE lseq > ?1 ORDER BY lseq",
            )?;
            let attachments = stmt
                .query_map([watermark], |row| {
                    let sha: Vec<u8> = row.get(5)?;
                    Ok(AttachmentRecord {
                        id: row.get(0)?,
                        entry_id: row.get(1)?,
                        name: row.get(2)?,
                        mime: row.get(3)?,
                        size: row.get(4)?,
                        sha256: hex(&sha),
                        created_at: row.get(6)?,
                        deleted_at: row.get(7)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(stmt);
            let max_lseq: i64 = conn.query_row(
                "SELECT COALESCE(MAX(lseq), ?1) FROM (
                     SELECT lseq FROM entry WHERE lseq > ?1
                     UNION ALL SELECT lseq FROM attachment WHERE lseq > ?1)",
                [watermark],
                |row| row.get(0),
            )?;
            (entries, attachments, max_lseq)
        };
        if entries.is_empty() && attachments.is_empty() {
            return Ok(None);
        }
        let from_lseq = watermark + 1;

        // Blobs first: a bundle must never reference a blob
        // that has not arrived next to it.
        let blobs_dir = sync_dir.join("blobs");
        for record in &attachments {
            if record.deleted_at.is_some() {
                continue;
            }
            let dest = blobs_dir.join(&record.id);
            if !dest.exists() {
                let src = crate::attachments::blob_path(self.dir(), &record.id);
                if src.exists() {
                    fs::create_dir_all(&blobs_dir)?;
                    let tmp = blobs_dir.join(format!(".tmp-{}", record.id));
                    fs::copy(&src, &tmp)?;
                    rename_retry(&tmp, &dest)?;
                }
            }
        }

        let header = BundleHeader {
            format: BUNDLE_FORMAT,
            vault_id: self.header().vault_id.clone(),
            device_id: self.device_id.clone(),
            from_lseq,
            to_lseq,
        };
        let bundle = Bundle { entries, attachments };
        let json = serde_json::to_vec(&bundle)?;
        let key = self.sync_key();
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).expect("sync key is 32 bytes");
        let nonce = random_bytes::<NONCE_LEN>()?;
        let aad = header_line(&header);
        let sealed = cipher
            .encrypt(&XNonce::from(nonce), Payload { msg: &json, aad: &aad })
            .map_err(|_| Error::Unlock)?;
        let mut bytes = aad.clone();
        bytes.push(b'\n');
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&sealed);

        fs::create_dir_all(sync_dir)?;
        // The FULL device id goes in the name: its first 8
        // characters are the top of a UUIDv7 timestamp and
        // stay the same for ~65 seconds, so two devices
        // created in the same window would share a prefix —
        // and a shared filename — under the short form.
        let file_name = format!(
            "{}-{}-{:08}-{:08}.sync",
            short(&header.vault_id),
            header.device_id,
            from_lseq,
            to_lseq
        );
        let dest = sync_dir.join(&file_name);
        if dest.exists() {
            // A bundle with this name is already in the
            // folder: my own earlier write (a crash between
            // the write and the watermark commit below makes
            // this emit a re-emit, which must be a no-op), a
            // torn copy of it — or another copy of this
            // vault writing under my device id, whose bundle
            // must never be silently overwritten.
            let existing = fs::read(&dest)?;
            match open_bundle(&cipher, &existing) {
                Ok((old_header, old_bundle)) => {
                    let identical = old_header.vault_id == header.vault_id
                        && old_header.device_id == header.device_id
                        && old_header.from_lseq == from_lseq
                        && old_header.to_lseq == to_lseq
                        && serde_json::to_vec(&old_bundle)? == json;
                    if !identical {
                        return Err(Error::Sync(format!(
                            "bundle {file_name} is already in the sync folder with \
                             different contents — another copy of this vault is using \
                             this device id; run restamp-device on one of the copies"
                        )));
                    }
                    // Identical: already emitted. Fall
                    // through to the watermark commit.
                }
                Err(_) => {
                    // Undecryptable: a torn copy of my own
                    // bundle (its header line still names
                    // this vault and device) may be replaced;
                    // anything else may not.
                    let mine = header_of(&existing).is_some_and(|h| {
                        h.vault_id == header.vault_id && h.device_id == header.device_id
                    });
                    if !mine {
                        return Err(Error::Sync(format!(
                            "bundle {file_name} is already in the sync folder and is \
                             not this device's bundle — another copy of this vault may \
                             be using this device id; run restamp-device on one of the copies"
                        )));
                    }
                    let tmp = sync_dir.join(format!(".tmp-{file_name}"));
                    fs::write(&tmp, &bytes)?;
                    rename_retry(&tmp, &dest)?;
                }
            }
        } else {
            let tmp = sync_dir.join(format!(".tmp-{file_name}"));
            fs::write(&tmp, &bytes)?;
            rename_retry(&tmp, &dest)?;
        }

        // The watermark moves only after the bundle is
        // safely in the folder: a crash before this just
        // re-emits (a no-op to apply).
        {
            let tx = self.db.conn_mut().transaction()?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![EMITTED_KEY, to_lseq.to_string()],
            )?;
            tx.commit()?;
        }
        Ok(Some(SyncBundleInfo {
            file_name,
            from_lseq,
            to_lseq,
            entries: bundle.entries.len(),
            attachments: bundle.attachments.len(),
        }))
    }

    /// Applies every bundle in `sync_dir` this vault has not
    /// incorporated yet. Safe to re-run: incorporated records
    /// are recognised and skipped.
    pub fn sync_apply(&mut self, sync_dir: &Path) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        if !sync_dir.is_dir() {
            return Ok(report);
        }
        let my_vault = self.header().vault_id.clone();
        let my_vault_short = short(&my_vault).to_string();
        let emitted: i64 = self
            .db
            .meta(EMITTED_KEY)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut warned: std::collections::BTreeSet<String> = self
            .db
            .meta(WARNED_KEY)?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let mut warned_dirty = false;
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        for item in fs::read_dir(sync_dir)? {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            // The filename's vault prefix IS read: a bundle
            // for another vault is not even opened. (The
            // plaintext header below is the authority; this
            // is the cheap filter in front of it.)
            if let Some((prefix, _, _)) = parse_bundle_name(&name) {
                if prefix == my_vault_short {
                    files.push((name, item.path()));
                }
            }
        }
        // The names sort by (device, zero-padded range):
        // one device's bundles are read in lseq order.
        files.sort();
        let key = self.sync_key();
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).expect("sync key is 32 bytes");

        for (name, path) in files {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    report.problems.push(format!("bundle {name}: cannot read"));
                    continue;
                }
            };
            // The plaintext header line decides whose bundle
            // this is — exactly, by full ids. open_bundle
            // re-verifies the same values as AAD below, so
            // nothing here trusts the line beyond skipping.
            if let Some(h) = header_of(&bytes) {
                if h.vault_id != my_vault {
                    // Another vault sharing the folder: not
                    // a problem, not mine to report.
                    continue;
                }
                if h.device_id == self.device_id {
                    // My own bundle — unless it reaches past
                    // everything I ever emitted, which I
                    // cannot have written: another copy of
                    // this vault is using my device id.
                    if h.to_lseq > emitted && !warned.contains(&name) {
                        warned.insert(name.clone());
                        warned_dirty = true;
                        report.problems.push(format!(
                            "bundle {name}: another copy of this vault is using this \
                             device id — its changes were not applied; run \
                             restamp-device on one of the copies"
                        ));
                    }
                    continue;
                }
            }
            report.bundles_seen += 1;
            let (header, bundle) = match open_bundle(&cipher, &bytes) {
                Ok(opened) => opened,
                Err(_) => {
                    report
                        .problems
                        .push(format!("bundle {name}: cannot decrypt (wrong vault or damaged)"));
                    continue;
                }
            };
            if header.format != BUNDLE_FORMAT || header.vault_id != my_vault {
                report.problems.push(format!("bundle {name}: not for this vault"));
                continue;
            }
            report.bundles_applied += 1;
            let tx = self.db.conn_mut().transaction()?;
            // Entry rows reference their author device
            // (entry.device_id is a foreign key, and
            // foreign_keys is ON), so EVERY author named
            // in the bundle needs a device row before its
            // records can land — not just the header's
            // device: bundles relay, and a bundle from A
            // can carry B-authored records to a device that
            // has never seen a bundle from B.
            let mut authors: Vec<&str> = vec![header.device_id.as_str()];
            for record in &bundle.entries {
                if !authors.contains(&record.device_id.as_str()) {
                    authors.push(record.device_id.as_str());
                }
            }
            for author in authors {
                tx.execute(
                    "INSERT OR IGNORE INTO device (id, name, created_at) VALUES (?1, 'sync peer', ?2)",
                    params![author, crate::storage::now_ms()],
                )?;
            }
            for record in &bundle.entries {
                apply_entry(&tx, record, &mut report)?;
            }
            tx.commit()?;
            for record in &bundle.attachments {
                self.apply_attachment(sync_dir, record, &mut report)?;
            }
        }
        if warned_dirty {
            let tx = self.db.conn_mut().transaction()?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![WARNED_KEY, serde_json::to_string(&warned)?],
            )?;
            tx.commit()?;
        }
        Ok(report)
    }

    /// Gives this vault a NEW device identity and returns
    /// it: a fresh device id, and the emit watermark reset
    /// (everything this vault holds will be emitted again
    /// under the new id, where peers take it as a new
    /// device's stream). This is the step a folder copy of
    /// a vault needs before it can sync as a second device:
    /// without it the copy shares the original's id, its
    /// bundles are skipped as "own" by every peer (and by
    /// the original), and its emits can collide with the
    /// original's bundle names.
    pub fn restamp_device(&mut self) -> Result<String> {
        let new_id = uuid::Uuid::now_v7().to_string();
        let tx = self.db.conn_mut().transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO device (id, name, created_at) VALUES (?1, 'this device', ?2)",
            params![new_id, crate::storage::now_ms()],
        )?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'device_id'",
            [&new_id],
        )?;
        tx.execute("DELETE FROM meta WHERE key = ?1", [EMITTED_KEY])?;
        tx.commit()?;
        self.device_id = new_id.clone();
        Ok(new_id)
    }

}

fn apply_entry(
    tx: &Transaction,
    record: &EntryRecord,
    report: &mut SyncReport,
) -> Result<()> {
    let local: Option<EntryRecord> = {
            let found = tx
                .query_row(
                    "SELECT entry_date, created_at, updated_at, deleted_at, rev,
                            device_id, title, body_md, mood, tz, base_rev, base_device
                       FROM entry WHERE id = ?1",
                    [&record.id],
                    |row| {
                        Ok(EntryRecord {
                            id: record.id.clone(),
                            entry_date: row.get(0)?,
                            created_at: row.get(1)?,
                            updated_at: row.get(2)?,
                            deleted_at: row.get(3)?,
                            rev: row.get(4)?,
                            device_id: row.get(5)?,
                            title: row.get(6)?,
                            body_md: row.get(7)?,
                            mood: row.get(8)?,
                            tz: row.get(9)?,
                            base_rev: row.get(10)?,
                            base_device: row.get(11)?,
                            tags: Vec::new(),
                        })
                    },
                )
                .ok();
            match found {
                Some(mut row) => {
                    row.tags = entry_tags(tx, &record.id)?;
                    Some(row)
                }
                None => None,
            }
        };
        let Some(local) = local else {
            insert_record(tx, record)?;
            report.entries_applied += 1;
            return Ok(());
        };
        if record.device_id == local.device_id {
            // Same lineage: the higher rev is simply newer.
            if record.rev > local.rev {
                replace_record(tx, record)?;
                report.entries_applied += 1;
            }
            return Ok(());
        }
        // Different authors. A record whose base is exactly
        // my current state descends from it: clean
        // fast-forward, no conflict. Symmetrically, if my
        // state descends from the record, it is stale.
        if (record.base_device.as_str(), record.base_rev) == (local.device_id.as_str(), local.rev) {
            replace_record(tx, record)?;
            report.entries_applied += 1;
            return Ok(());
        }
        if (local.base_device.as_str(), local.base_rev) == (record.device_id.as_str(), record.rev) {
            return Ok(());
        }
        // Both sides moved away from their bases: a true
        // divergence. The higher (rev, device_id) wins the
        // entry; both sides compute the same winner.
        let incoming_wins =
            (record.rev, record.device_id.as_str()) > (local.rev, local.device_id.as_str());
        let loser = if incoming_wins { &local } else { record };
        if incoming_wins {
            replace_record(tx, record)?;
            report.entries_applied += 1;
        }
        // The loser's text survives as a conflict copy
        // (a losing tombstone holds no text to keep). If
        // both texts are identical there is nothing to
        // preserve — which is also what stops two devices'
        // independently created copies of the SAME loser
        // from breeding copies of copies: the twins merge.
        // The copy starts its OWN lineage — a deterministic
        // synthetic author and a base pointing at itself —
        // because a base inside the original entry's
        // history would share that entry's coordinates: an
        // edit of such a copy would read as stale on one
        // peer and as a fast-forward on the other, and
        // vanish between them.
        let winner_rec = if incoming_wins { record } else { &local };
        let text_differs = loser.body_md != winner_rec.body_md
            || loser.title != winner_rec.title
            || loser.deleted_at.is_some() != winner_rec.deleted_at.is_some();
        if loser.body_md.is_some() && text_differs {
            let copy_id = conflict_id(&record.id, loser);
            let exists: bool = tx
                .query_row("SELECT 1 FROM entry WHERE id = ?1", [&copy_id], |_| Ok(true))
                .unwrap_or(false);
            if !exists {
                let mut copy = loser.clone();
                copy.id = copy_id;
                copy.rev = 1;
                copy.device_id = conflict_author(&record.id, loser);
                copy.base_device = copy.device_id.clone();
                copy.base_rev = 1;
                copy.deleted_at = None;
                copy.title = Some(match &loser.title {
                    Some(title) => format!("{title} (conflict)"),
                    None => "Conflict copy".to_string(),
                });
                tx.execute(
                    "INSERT OR IGNORE INTO device (id, name, created_at) VALUES (?1, 'conflict copy', ?2)",
                    params![copy.device_id, crate::storage::now_ms()],
                )?;
                insert_record(tx, &copy)?;
                report.entries_conflicted += 1;
            }
        }
    Ok(())
}

impl Vault {
    fn apply_attachment(
        &mut self,
        sync_dir: &Path,
        record: &AttachmentRecord,
        report: &mut SyncReport,
    ) -> Result<()> {
        enum State {
            Missing,
            Live,
            Deleted,
        }
        let state = {
            let conn = self.db.connection();
            let deleted: Option<Option<i64>> = conn
                .query_row("SELECT deleted_at FROM attachment WHERE id = ?1", [&record.id], |row| row.get(0))
                .ok();
            match deleted {
                None => State::Missing,
                Some(None) => State::Live,
                Some(Some(_)) => State::Deleted,
            }
        };
        match state {
            State::Live => {
                if record.deleted_at.is_some() {
                    // Their delete wins for attachments (the
                    // bytes are immutable, so there is no
                    // content conflict to preserve).
                    let tx = self.db.conn_mut().transaction()?;
                    let lseq = next_lseq(&tx)?;
                    tx.execute(
                        "UPDATE attachment SET deleted_at = ?1, lseq = ?2 WHERE id = ?3",
                        params![record.deleted_at, lseq, record.id],
                    )?;
                    tx.commit()?;
                    crate::attachments::remove_blob(self.dir(), &record.id);
                    report.attachments_applied += 1;
                }
            }
            State::Deleted => {}
            State::Missing => {
                if record.deleted_at.is_some() {
                    // A delete for something never held: keep
                    // a tombstone row so a later, older add
                    // cannot resurrect it.
                    let entry_exists = self.db.connection().query_row(
                        "SELECT 1 FROM entry WHERE id = ?1",
                        [&record.entry_id],
                        |_| Ok(true),
                    ).unwrap_or(false);
                    if entry_exists {
                        let tx = self.db.conn_mut().transaction()?;
                        let lseq = next_lseq(&tx)?;
                        insert_attachment_row(&tx, record, lseq, &record.sha256)?;
                        tx.commit()?;
                    }
                    return Ok(());
                }
                let src = sync_dir.join("blobs").join(&record.id);
                if !src.exists() {
                    report
                        .problems
                        .push(format!("attachment {}: its blob is missing from the sync folder", record.id));
                    return Ok(());
                }
                let entry_exists = self.db.connection().query_row(
                    "SELECT 1 FROM entry WHERE id = ?1",
                    [&record.entry_id],
                    |_| Ok(true),
                ).unwrap_or(false);
                if !entry_exists {
                    report
                        .problems
                        .push(format!("attachment {}: its entry has not arrived yet", record.id));
                    return Ok(());
                }
                let dest = crate::attachments::blob_path(self.dir(), &record.id);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let tmp = dest.with_extension("sync-tmp");
                fs::copy(&src, &tmp)?;
                rename_retry(&tmp, &dest)?;
                {
                    let tx = self.db.conn_mut().transaction()?;
                    let lseq = next_lseq(&tx)?;
                    insert_attachment_row(&tx, record, lseq, &record.sha256)?;
                    tx.commit()?;
                }
                // Authenticate what arrived: the plaintext
                // hash must match the record (the same check
                // JSON import makes, §8).
                let verified = self
                    .read_attachment(&record.id)
                    .map(|bytes| hex(&Sha256::digest(&bytes)) == record.sha256)
                    .unwrap_or(false);
                if verified {
                    report.attachments_applied += 1;
                } else {
                    let tx = self.db.conn_mut().transaction()?;
                    tx.execute("DELETE FROM attachment WHERE id = ?1", [&record.id])?;
                    tx.commit()?;
                    crate::attachments::remove_blob(self.dir(), &record.id);
                    report
                        .problems
                        .push(format!("attachment {}: content does not match its hash; rejected", record.id));
                }
            }
        }
        Ok(())
    }
}

fn open_bundle(cipher: &XChaCha20Poly1305, bytes: &[u8]) -> Result<(BundleHeader, Bundle)> {
    let Some(newline) = bytes.iter().position(|b| *b == b'\n') else {
        return Err(Error::Unlock);
    };
    let (line, rest) = bytes.split_at(newline);
    let header: BundleHeader = serde_json::from_slice(line).map_err(|_| Error::Unlock)?;
    let rest = &rest[1..];
    if rest.len() <= NONCE_LEN {
        return Err(Error::Unlock);
    }
    let (nonce, sealed) = rest.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("split_at NONCE_LEN");
    let plain = cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: sealed, aad: line })
        .map_err(|_| Error::Unlock)?;
    let bundle: Bundle = serde_json::from_slice(&plain).map_err(|_| Error::Unlock)?;
    Ok((header, bundle))
}

fn entry_tags(conn: &rusqlite::Connection, entry_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM entry_tag et
           JOIN tag t ON t.id = et.tag_id
           JOIN entry e ON e.rid = et.entry_rid
          WHERE e.id = ?1 ORDER BY t.name_key",
    )?;
    let tags = stmt
        .query_map([entry_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

fn write_record_tags(tx: &Transaction, rid: i64, tags: &[String]) -> Result<()> {
    tx.execute("DELETE FROM entry_tag WHERE entry_rid = ?1", [rid])?;
    for name in tags {
        let display: String = name.trim().nfc().collect();
        if display.is_empty() || display.chars().any(char::is_control) {
            continue;
        }
        let key: String = display.to_lowercase().nfc().collect();
        tx.execute(
            "INSERT INTO tag (name, name_key) VALUES (?1, ?2) ON CONFLICT(name_key) DO NOTHING",
            params![display, key],
        )?;
        let tag_id: i64 = tx.query_row("SELECT id FROM tag WHERE name_key = ?1", [&key], |row| row.get(0))?;
        tx.execute("INSERT INTO entry_tag (entry_rid, tag_id) VALUES (?1, ?2)", params![rid, tag_id])?;
    }
    Ok(())
}

fn insert_record(tx: &Transaction, record: &EntryRecord) -> Result<()> {
    let lseq = next_lseq(tx)?;
    tx.execute(
        "INSERT INTO entry
           (id, entry_date, created_at, updated_at, deleted_at, rev, device_id, lseq,
            title, body_md, mood, tz, base_rev, base_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            record.id,
            record.entry_date,
            record.created_at,
            record.updated_at,
            record.deleted_at,
            record.rev,
            record.device_id,
            lseq,
            record.title,
            record.body_md,
            record.mood,
            record.tz,
            record.base_rev,
            record.base_device
        ],
    )?;
    let rid = tx.last_insert_rowid();
    if record.deleted_at.is_none() {
        write_record_tags(tx, rid, &record.tags)?;
    }
    Ok(())
}

fn replace_record(tx: &Transaction, record: &EntryRecord) -> Result<()> {
    let lseq = next_lseq(tx)?;
    tx.execute(
        "UPDATE entry
            SET entry_date = ?1, created_at = ?2, updated_at = ?3, deleted_at = ?4,
                rev = ?5, device_id = ?6, lseq = ?7, title = ?8, body_md = ?9,
                mood = ?10, tz = ?11, base_rev = ?12, base_device = ?13
          WHERE id = ?14",
        params![
            record.entry_date,
            record.created_at,
            record.updated_at,
            record.deleted_at,
            record.rev,
            record.device_id,
            lseq,
            record.title,
            record.body_md,
            record.mood,
            record.tz,
            record.base_rev,
            record.base_device,
            record.id
        ],
    )?;
    let rid: i64 = tx.query_row("SELECT rid FROM entry WHERE id = ?1", [&record.id], |row| row.get(0))?;
    if record.deleted_at.is_none() {
        write_record_tags(tx, rid, &record.tags)?;
    } else {
        tx.execute("DELETE FROM entry_tag WHERE entry_rid = ?1", [rid])?;
    }
    Ok(())
}

fn insert_attachment_row(tx: &Transaction, record: &AttachmentRecord, lseq: i64, sha_hex: &str) -> Result<()> {
    let sha: Vec<u8> = (0..sha_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&sha_hex[i..i + 2], 16).unwrap_or(0))
        .collect();
    tx.execute(
        "INSERT INTO attachment
           (id, entry_id, name, mime, size, sha256, created_at, deleted_at, lseq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            record.id,
            record.entry_id,
            record.name,
            record.mime,
            record.size,
            sha,
            record.created_at,
            record.deleted_at,
            lseq
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use crate::entries::EntryDraft;
    use tempfile::TempDir;

    const PASSPHRASE: &str = "correct horse battery staple";
    const DEVICE_A: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
    const DEVICE_B: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";

    fn fast() -> KdfParams {
        KdfParams { m_kib: 8, t: 1, p: 1 }
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for item in fs::read_dir(from).unwrap() {
            let item = item.unwrap();
            let dest = to.join(item.file_name());
            if item.file_type().unwrap().is_dir() {
                copy_dir(&item.path(), &dest);
            } else {
                fs::copy(item.path(), &dest).unwrap();
            }
        }
    }

    /// Two devices of ONE vault: B starts as a byte copy of
    /// A's folder with its device id re-stamped.
    fn two_devices() -> (TempDir, TempDir, Vault, Vault) {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let (a, _) = Vault::create(&dir_a.path().join("v"), PASSPHRASE, fast()).unwrap();
        let a_dir = a.dir().to_path_buf();
        drop(a);
        copy_dir(&a_dir, &dir_b.path().join("v"));
        let mut b = Vault::open_with_passphrase(&dir_b.path().join("v"), PASSPHRASE).unwrap();
        {
            let conn = b.db.conn_mut();
            conn.execute(
                "INSERT INTO device (id, name, created_at) VALUES (?1, 'test', 0)",
                [DEVICE_B],
            )
            .unwrap();
            conn.execute("UPDATE meta SET value = ?1 WHERE key = 'device_id'", [DEVICE_B])
                .unwrap();
        }
        drop(b);
        let b = Vault::open_with_passphrase(&dir_b.path().join("v"), PASSPHRASE).unwrap();
        assert_eq!(b.device_id, DEVICE_B);
        let mut a = Vault::open_with_passphrase(&a_dir, PASSPHRASE).unwrap();
        {
            let conn = a.db.conn_mut();
            conn.execute(
                "INSERT INTO device (id, name, created_at) VALUES (?1, 'test', 0)",
                [DEVICE_A],
            )
            .unwrap();
            conn.execute("UPDATE meta SET value = ?1 WHERE key = 'device_id'", [DEVICE_A])
                .unwrap();
        }
        drop(a);
        let a = Vault::open_with_passphrase(&a_dir, PASSPHRASE).unwrap();
        assert_eq!(a.device_id, DEVICE_A);
        (dir_a, dir_b, a, b)
    }

    fn draft(body: &str) -> EntryDraft {
        EntryDraft {
            entry_date: "2026-10-16".into(),
            title: Some("Note".into()),
            body_md: body.into(),
            mood: None,
            tz: None,
            tags: vec!["sync".into()],
        }
    }

    fn exchange(a: &mut Vault, b: &mut Vault, dir: &Path) {
        a.sync_emit(dir).unwrap();
        b.sync_emit(dir).unwrap();
        let ra = a.sync_apply(dir).unwrap();
        let rb = b.sync_apply(dir).unwrap();
        assert!(ra.problems.is_empty(), "{:?}", ra.problems);
        assert!(rb.problems.is_empty(), "{:?}", rb.problems);
    }

    #[test]
    fn entries_flow_both_ways_and_reapply_is_a_no_op() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("from A")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        assert_eq!(b.get_entry(&entry.id).unwrap().body_md, "from A");
        assert_eq!(b.get_entry(&entry.id).unwrap().tags, vec!["sync".to_string()]);

        b.update_entry(&entry.id, &draft("edited on B")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        assert_eq!(a.get_entry(&entry.id).unwrap().body_md, "edited on B");

        // Everything is incorporated once the re-broadcast
        // of what B taught A has also settled (this vault
        // repeats what it applied, for multi-hop couriers):
        // one more exchange, then nothing moves at all.
        exchange(&mut a, &mut b, sync.path());
        let before = a.sync_emit(sync.path()).unwrap();
        assert!(before.is_none());
        let report = a.sync_apply(sync.path()).unwrap();
        assert_eq!(report.entries_applied, 0);
    }

    #[test]
    fn diverged_edits_converge_with_one_conflict_copy() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        exchange(&mut a, &mut b, sync.path());

        a.update_entry(&entry.id, &draft("A's words")).unwrap();
        b.update_entry(&entry.id, &draft("B's words")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        // A second exchange: the copies themselves travel.
        exchange(&mut a, &mut b, sync.path());

        let page_a = a.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let page_b = b.list_entries(&crate::entries::ListQuery::default()).unwrap();
        assert_eq!(page_a.items.len(), 2, "winner + one conflict copy");
        assert_eq!(page_b.items.len(), 2);
        let winner_a = a.get_entry(&entry.id).unwrap();
        let winner_b = b.get_entry(&entry.id).unwrap();
        assert_eq!(winner_a.body_md, winner_b.body_md, "both sides pick the same winner");
        let loser_text = if winner_a.body_md == "A's words" { "B's words" } else { "A's words" };
        let copy_a = page_a.items.iter().find(|i| i.id != entry.id).unwrap();
        let copy_b = page_b.items.iter().find(|i| i.id != entry.id).unwrap();
        assert_eq!(copy_a.id, copy_b.id, "the same deterministic copy on both sides");
        assert_eq!(a.get_entry(&copy_a.id).unwrap().body_md, loser_text);
    }

    #[test]
    fn delete_against_edit_keeps_the_text_as_a_copy() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        exchange(&mut a, &mut b, sync.path());

        b.delete_entry(&entry.id).unwrap();
        a.update_entry(&entry.id, &draft("still writing")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        exchange(&mut a, &mut b, sync.path());

        assert!(a.get_entry(&entry.id).is_err(), "the delete won the entry on A too");
        assert!(b.get_entry(&entry.id).is_err());
        let page = a.list_entries(&crate::entries::ListQuery::default()).unwrap();
        assert_eq!(page.items.len(), 1, "only the conflict copy remains");
        assert_eq!(a.get_entry(&page.items[0].id).unwrap().body_md, "still writing");
    }

    #[test]
    fn attachments_travel_with_their_blob() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("with a file")).unwrap();
        let att = a
            .add_attachment(&entry.id, "note.txt", "text/plain", b"attachment canary")
            .unwrap();
        exchange(&mut a, &mut b, sync.path());
        let got = b.read_attachment(&att.id).unwrap();
        assert_eq!(got, b"attachment canary");
    }

    #[test]
    fn a_bundle_from_another_vault_is_ignored_quietly() {
        let (_da, _db, mut a, _b) = two_devices();
        let other_dir = TempDir::new().unwrap();
        let (mut other, _) = Vault::create(&other_dir.path().join("v"), PASSPHRASE, fast()).unwrap();
        other.create_entry(&draft("someone else's diary")).unwrap();
        let sync = TempDir::new().unwrap();
        other.sync_emit(sync.path()).unwrap();
        // Two vaults sharing one folder is a normal setup:
        // the foreign bundle is skipped on its filename
        // prefix, never decrypted, never a "problem" (a
        // problem would make every CLI run exit non-zero).
        let report = a.sync_apply(sync.path()).unwrap();
        assert_eq!(report.entries_applied, 0);
        assert_eq!(report.bundles_seen, 0);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn devices_sharing_a_name_prefix_still_exchange() {
        // Device ids are UUIDv7: their first 8 characters
        // are timestamp bits, identical for ~65 seconds at
        // a time. Two devices created in that window share
        // the prefix the old filename/skip logic used —
        // they must still see each other's bundles.
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let sync = TempDir::new().unwrap();
        let (a, _) = Vault::create(&dir_a.path().join("v"), PASSPHRASE, fast()).unwrap();
        let a_dir = a.dir().to_path_buf();
        let a_id = a.device_id.clone();
        drop(a);
        copy_dir(&a_dir, &dir_b.path().join("v"));
        let twin_id = format!("{}-2222-4222-8222-222222222222", &a_id[..8]);
        let mut b = Vault::open_with_passphrase(&dir_b.path().join("v"), PASSPHRASE).unwrap();
        {
            let conn = b.db.conn_mut();
            conn.execute(
                "INSERT INTO device (id, name, created_at) VALUES (?1, 'test', 0)",
                [&twin_id],
            )
            .unwrap();
            conn.execute("UPDATE meta SET value = ?1 WHERE key = 'device_id'", [&twin_id])
                .unwrap();
        }
        drop(b);
        let mut b = Vault::open_with_passphrase(&dir_b.path().join("v"), PASSPHRASE).unwrap();
        let mut a = Vault::open_with_passphrase(&a_dir, PASSPHRASE).unwrap();
        assert_eq!(short(&a.device_id), short(&b.device_id), "the prefixes collide");
        assert_ne!(a.device_id, b.device_id);
        let entry = a.create_entry(&draft("from A")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        assert_eq!(b.get_entry(&entry.id).unwrap().body_md, "from A");
    }

    #[test]
    fn a_copied_vault_is_loud_and_restamp_fixes_it() {
        let (_da, _db, mut a, _b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("original")).unwrap();
        a.sync_emit(sync.path()).unwrap();
        // A raw folder copy: same device id, same watermark.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        assert_eq!(twin.device_id, a.device_id);
        twin.update_entry(&entry.id, &draft("twin edit one")).unwrap();
        twin.update_entry(&entry.id, &draft("twin edit two")).unwrap();
        twin.sync_emit(sync.path()).unwrap();
        // A hears about it — loudly — and applies nothing.
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(
            report.problems.iter().any(|p| p.contains("restamp-device")),
            "{:?}",
            report.problems
        );
        assert_eq!(a.get_entry(&entry.id).unwrap().body_md, "original");
        // The same stale file does not re-warn on the next run.
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        // Restamp the copy: it becomes a new device and its
        // whole state flows.
        let new_id = twin.restamp_device().unwrap();
        assert_ne!(new_id, a.device_id);
        twin.sync_emit(sync.path()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(a.get_entry(&entry.id).unwrap().body_md, "twin edit two");
    }

    #[test]
    fn emit_never_overwrites_a_twins_bundle() {
        let dir_a = TempDir::new().unwrap();
        let sync = TempDir::new().unwrap();
        let (mut a, _) = Vault::create(&dir_a.path().join("v"), PASSPHRASE, fast()).unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        // A raw copy taken after the create: same id,
        // watermark 0 on both sides.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        // Both sides edit the entry once, so both emits want
        // the same range (1..=2) — with different records.
        a.update_entry(&entry.id, &draft("A's text")).unwrap();
        a.sync_emit(sync.path()).unwrap();
        let bundle_name = format!(
            "{}-{}-00000001-00000002.sync",
            short(&a.header().vault_id),
            a.device_id
        );
        let before = fs::read(sync.path().join(&bundle_name)).unwrap();
        twin.update_entry(&entry.id, &draft("twin's text")).unwrap();
        let err = twin.sync_emit(sync.path()).unwrap_err();
        assert!(err.to_string().contains("restamp-device"), "{err}");
        let after = fs::read(sync.path().join(&bundle_name)).unwrap();
        assert_eq!(before, after, "the original's bundle is untouched");
    }

    #[test]
    fn a_reemit_after_a_crash_is_a_no_op() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("from A")).unwrap();
        let first = a.sync_emit(sync.path()).unwrap().unwrap();
        // The crash: the bundle reached the folder, the
        // watermark commit did not.
        a.db
            .conn_mut()
            .execute("DELETE FROM meta WHERE key = 'sync_emitted_lseq'", [])
            .unwrap();
        let second = a.sync_emit(sync.path()).unwrap().unwrap();
        assert_eq!(first.file_name, second.file_name);
        let report = b.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(b.get_entry(&entry.id).unwrap().body_md, "from A");
    }

    #[test]
    fn editing_a_conflict_copy_survives() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        a.update_entry(&entry.id, &draft("A's words")).unwrap();
        b.update_entry(&entry.id, &draft("B's words")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        // The user fixes up the copy on A before any further
        // exchange: the edit is an authorship change on the
        // copy's own lineage and must travel like one.
        let page = a.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let copy_id = page.items.iter().find(|i| i.id != entry.id).unwrap().id.clone();
        a.update_entry(&copy_id, &draft("merged by hand")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        assert_eq!(a.get_entry(&copy_id).unwrap().body_md, "merged by hand", "edit lost on A");
        assert_eq!(b.get_entry(&copy_id).unwrap().body_md, "merged by hand", "edit never reached B");
    }

    #[test]
    fn relayed_records_do_not_break_a_third_device() {
        const DEVICE_C: &str = "cccccccc-3333-4333-8333-cccccccccccc";
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        // C: a copy of A's vault stamped to a third id,
        // taken before A has ever seen a B bundle (so C's
        // device table has no row for B).
        let dir_c = TempDir::new().unwrap();
        let c_dir = dir_c.path().join("v");
        copy_dir(a.dir(), &c_dir);
        let mut c = Vault::open_with_passphrase(&c_dir, PASSPHRASE).unwrap();
        {
            let conn = c.db.conn_mut();
            conn.execute(
                "INSERT INTO device (id, name, created_at) VALUES (?1, 'test', 0)",
                [DEVICE_C],
            )
            .unwrap();
            conn.execute("UPDATE meta SET value = ?1 WHERE key = 'device_id'", [DEVICE_C])
                .unwrap();
        }
        drop(c);
        let mut c = Vault::open_with_passphrase(&c_dir, PASSPHRASE).unwrap();
        assert_eq!(c.device_id, DEVICE_C);
        // A creates, B edits: the entry is B-authored.
        let entry = a.create_entry(&draft("from A")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        b.update_entry(&entry.id, &draft("edited on B")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        // A re-emits into a fresh folder: its bundle carries
        // the B-authored record under A's header. C must
        // register B as an author, not die on the foreign
        // key.
        let relay = TempDir::new().unwrap();
        a.sync_emit(relay.path()).unwrap();
        let report = c.sync_apply(relay.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(c.get_entry(&entry.id).unwrap().body_md, "edited on B");
    }

    #[test]
    fn twin_copies_of_one_vault_lose_no_loser_text() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        // A twin: a raw copy of A's vault, still DEVICE_A.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        assert_eq!(twin.device_id, a.device_id);
        // Three-way divergence at the same rev: A/2, twin's
        // A/2 (different text, same author and rev), B/2.
        // The twin makes a filler entry first so its emit
        // range differs from A's and the emit guard stays
        // out of this scenario.
        a.update_entry(&entry.id, &draft("A one")).unwrap();
        twin.create_entry(&draft("filler")).unwrap();
        twin.update_entry(&entry.id, &draft("twin words")).unwrap();
        b.update_entry(&entry.id, &draft("B words")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        exchange(&mut b, &mut twin, sync.path());
        // Both losers survive as copies: their ids differ
        // because the content is part of the derivation.
        let page = b.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let bodies: Vec<String> = page
            .items
            .iter()
            .map(|i| b.get_entry(&i.id).unwrap().body_md)
            .collect();
        assert!(bodies.iter().any(|x| x == "A one"), "A's loser kept: {bodies:?}");
        assert!(bodies.iter().any(|x| x == "twin words"), "twin's loser kept: {bodies:?}");
    }

    #[test]
    fn editing_a_conflict_copy_on_the_winner_device_survives() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        a.update_entry(&entry.id, &draft("A's words")).unwrap();
        b.update_entry(&entry.id, &draft("B's words")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        // The copy is edited on B — the device whose edit
        // WON the entry (A edited it in the other test).
        let page = b.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let copy_id = page.items.iter().find(|i| i.id != entry.id).unwrap().id.clone();
        b.update_entry(&copy_id, &draft("merged by hand")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        assert_eq!(a.get_entry(&copy_id).unwrap().body_md, "merged by hand");
        assert_eq!(b.get_entry(&copy_id).unwrap().body_md, "merged by hand");
    }

    #[test]
    fn an_edited_copy_arriving_before_resolution_converges() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("base")).unwrap();
        a.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        a.update_entry(&entry.id, &draft("A's words")).unwrap();
        b.update_entry(&entry.id, &draft("B's words")).unwrap();
        // B resolves first and edits the copy.
        a.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        let page = b.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let copy_id = page.items.iter().find(|i| i.id != entry.id).unwrap().id.clone();
        b.update_entry(&copy_id, &draft("merged by hand")).unwrap();
        // B edits the entry once more, so in B's next
        // bundle the edited copy precedes the winning
        // entry record (by lseq): A meets the copy before
        // it has resolved the entry at all.
        b.update_entry(&entry.id, &draft("B words v2")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(a.get_entry(&entry.id).unwrap().body_md, "B words v2");
        assert_eq!(a.get_entry(&copy_id).unwrap().body_md, "merged by hand");
        let page = a.list_entries(&crate::entries::ListQuery::default()).unwrap();
        assert_eq!(page.items.len(), 2, "no duplicate copy");
    }
}
