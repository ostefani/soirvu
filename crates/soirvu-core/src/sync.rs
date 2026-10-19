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
use rusqlite::{OptionalExtension, Transaction, params};
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
const ACK_AHEAD_KEY: &str = "sync_ack_ahead";
/// Meta key prefix for the per-peer applied cursors: how
/// far this vault has incorporated each peer's bundle
/// stream. Published into the folder as ack files, they
/// are what makes pruning safe.
const APPLIED_PREFIX: &str = "sync_applied:";
const SEEN_PREFIX: &str = "sync_seen:";
const CLAIMED_PREFIX: &str = "sync_claimed:";
const ACK_FORMAT: u32 = 1;
/// An ack is ids and counters — a few hundred bytes in
/// practice, a few KB with a hundred peers. Anything
/// larger is not an ack; it is not read into memory.
const ACK_MAX_BYTES: u64 = 65_536;
const ACK_DIR: &str = "acks";

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
    /// Notes that are not failures: the sync itself did
    /// its work (an ack that could not be published, say).
    /// Callers show them without treating the run as
    /// failed — the CLI, in particular, still exits 0.
    pub warnings: Vec<String>,
    /// The strongest shared-identity signal there is:
    /// the folder's ack for this device claimed more
    /// than this database holds, and was corrected. Kept
    /// apart from warnings on purpose — callers treat it
    /// as a failure (the CLI exits non-zero, the UI
    /// shows it as an error).
    pub ack_ahead: Option<String>,
}

/// What one prune removed from a sync folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncPruneReport {
    pub bundles_removed: usize,
    pub blobs_removed: usize,
    /// Every known peer has incorporated this vault's
    /// stream at least this far (0 = nothing is provably
    /// consumed, so nothing was removed).
    pub covered_through: i64,
}

/// Where this vault stands in the sync folder: its own
/// device identity, how far it has emitted, and how far
/// it has incorporated each peer's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOverview {
    pub device_id: String,
    pub emitted_lseq: i64,
    pub peers: Vec<SyncPeerCursor>,
    /// An ack ahead of this vault was detected at some
    /// point (see `SyncReport::ack_ahead`) and not yet
    /// dismissed or restamped away.
    pub ack_ahead_seen: bool,
}

/// How far this vault has incorporated one peer's
/// stream (the cursor its ack publishes for that peer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPeerCursor {
    pub device_id: String,
    pub applied_lseq: i64,
    /// The furthest point of that peer's stream this
    /// vault has EVIDENCE of: bundles it opened, or an
    /// ack claim its detector read. Kept apart from the
    /// cursor — a bundle across a gap still proves the
    /// emission, and the difference between the two is
    /// exactly what a re-emit heals.
    pub seen_emitted: i64,
}

/// One device's published cursors, written to
/// `acks/<device id>.json` in the sync folder after an
/// apply: which point of each peer's stream this device
/// has incorporated. Ids and counters only — the same
/// metadata class as a bundle header.
#[derive(Debug, Serialize, Deserialize)]
struct SyncAck {
    format: u32,
    vault_id: String,
    device_id: String,
    applied: std::collections::BTreeMap<String, i64>,
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
    let (vault, _device, from, to) = parse_bundle_name_full(name)?;
    Some((vault, from, to))
}

/// Like `parse_bundle_name`, but also returns the device
/// part (a full id in current names, its first 8
/// characters in names written before identity fix).
fn parse_bundle_name_full(name: &str) -> Option<(String, String, i64, i64)> {
    let stem = name.strip_suffix(".sync")?;
    let (rest, to) = stem.rsplit_once('-')?;
    let (rest, from) = rest.rsplit_once('-')?;
    let (vault, device) = rest.split_once('-')?;
    Some((
        vault.to_string(),
        device.to_string(),
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

/// A device id in its one canonical form — the form
/// `restamp_device` and vault creation mint. Anything
/// else in a peer position is junk from a plaintext
/// file, not a peer.
fn is_canonical_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|u| u.to_string() == s)
}

/// Reads every ack file for this vault in the folder.
/// Files that do not parse — a torn write from a peer, a
/// stranger's file — are skipped: a missing ack only
/// ever makes pruning more conservative.
fn read_acks(sync_dir: &Path, vault_id: &str) -> Vec<SyncAck> {
    let mut out = Vec::new();
    let acks_dir = sync_dir.join(ACK_DIR);
    if let Ok(items) = fs::read_dir(&acks_dir) {
        for item in items.flatten() {
            let path = item.path();
            if path.extension().is_some_and(|e| e == "json")
                && fs::metadata(&path).map(|m| m.len() <= ACK_MAX_BYTES).unwrap_or(false)
                && let Ok(bytes) = fs::read(&path)
                && let Ok(ack) = serde_json::from_slice::<SyncAck>(&bytes)
                && ack.format == ACK_FORMAT
                && ack.vault_id == vault_id
            {
                out.push(ack);
            }
        }
    }
    out
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
    // Tags enter by their name KEY, not their display
    // spelling: the display is per-device — the first
    // spelling a device saw wins its tag table, and emit
    // reads tags back through that table — so the same
    // record can carry "Foo" from one device and "foo"
    // from another, and the same loser would derive a
    // different copy id on each. The key (lowercase,
    // NFC) is the derivation write_record_tags stores by.
    let mut tag_keys: Vec<String> = loser
        .tags
        .iter()
        .map(|t| t.to_lowercase().nfc().collect())
        .collect();
    tag_keys.sort();
    tag_keys.dedup();
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
        &tag_keys,
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
        self.emit_from(sync_dir, watermark)
    }

    /// Emits every record past `watermark` as one bundle
    /// and, on success only, commits `to_lseq` as the new
    /// watermark. A failed emit — an identity collision, a
    /// disk error — changes nothing, which is what makes
    /// re-emit safe to retry.
    fn emit_from(
        &mut self,
        sync_dir: &Path,
        watermark: i64,
    ) -> Result<Option<SyncBundleInfo>> {
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
                        return Err(Error::SharedDeviceId);
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
                        // Not readable as a bundle of this
                        // vault at all — a foreign or
                        // damaged file squatting at my
                        // bundle's name. That is not an
                        // identity clash (a twin's bundle
                        // would decrypt, and lands in the
                        // branch above): the remedy is
                        // removing the file, not restamp.
                        return Err(Error::SyncUnreadable(file_name.clone()));
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

    /// Re-emits this vault's whole sync state as one
    /// bundle covering every record from lseq 1. The
    /// recovery when a peer lost bundles to a prune (an
    /// overstated ack, a restore): the full-range bundle
    /// satisfies the peer's contiguity check, so its
    /// cursor unsticks and the missing records land.
    /// Identity is NOT touched — that is restamp's job,
    /// and restamp as a recovery tool strands the old
    /// identity's ack file and cursor in the folder as a
    /// phantom peer. Just an emit from watermark 0: the
    /// stored watermark moves only when the emit lands.
    pub fn sync_reemit(&mut self, sync_dir: &Path) -> Result<Option<SyncBundleInfo>> {
        self.emit_from(sync_dir, 0)
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
        let mut cursors_dirty = false;
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
            // Emission evidence: this bundle proves its
            // author emitted through to_lseq — even when
            // it lands across a gap and the cursor below
            // cannot move. The difference is the overview's
            // re-emit signal.
            {
                let tx = self.db.conn_mut().transaction()?;
                Self::note_seen(&tx, &header.device_id, header.to_lseq)?;
                tx.commit()?;
            }
            let problems_before = report.problems.len();
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
            // The cursor may advance over this bundle only
            // when it was FULLY incorporated and continues
            // the peer's stream. Fully: the entries
            // committed and every attachment record applied
            // — attachment failures are problems in the
            // report, not errors (a missing blob, an entry
            // that has not arrived yet, a hash mismatch), so
            // the problem count is the test. Continues: the
            // bundle starts at or below the point already
            // held (an overlapping re-emit is fine). A
            // bundle that fails either test leaves the
            // cursor where it was: apply re-reads the whole
            // folder, so it is retried on a later run, and
            // the ack never claims what this database does
            // not hold.
            let cursor_key = format!("{APPLIED_PREFIX}{}", header.device_id);
            let previous: i64 = self
                .db
                .meta(&cursor_key)?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if header.to_lseq > previous
                && header.from_lseq <= previous + 1
                && report.problems.len() == problems_before
            {
                let tx = self.db.conn_mut().transaction()?;
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![cursor_key, header.to_lseq.to_string()],
                )?;
                tx.commit()?;
                cursors_dirty = true;
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
        // Every apply republishes the ack — the write is
        // skipped when the file is already current, so
        // this also repairs a lost ack on an apply where
        // no cursor moved. Best effort: a missing ack only
        // ever makes peers prune less — the safe direction
        // — so a failure never fails the apply. It is not
        // silent, and not one-shot: the note repeats on
        // every apply until the publish lands, because an
        // unexplained prune stall is worse than a repeated
        // note.
        // If the publish itself fails, no check ran: a
        // folder that cannot be written cannot be
        // corrected either, so the failure warning below
        // is the whole signal for that case.
        // Read before the publish: nothing that happens
        // after the ack file is corrected may take the
        // detection down with it. A failed read degrades
        // to an empty set (no claims persisted) — the
        // detection itself is unaffected.
        let known = self.known_peers().unwrap_or_default();
        match self.publish_ack(sync_dir) {
            Ok(Some((warning, ahead_peers))) => {
                // The report carries the detection first;
                // persisting it durably is best-effort on
                // top (see persist_ack_ahead).
                // The warning names the claimed peers
                // (short ids, at most five, known peers
                // first): a claim about a peer this vault
                // has never seen persists no row, so this
                // message is the only place that says who
                // must re-emit.
                report.ack_ahead = Some(if ahead_peers.is_empty() {
                    warning
                } else {
                    let mut named: Vec<&str> = Vec::new();
                    for (peer, _) in ahead_peers
                        .iter()
                        .filter(|(p, _)| known.contains(p.as_str()))
                        .chain(
                            ahead_peers
                                .iter()
                                .filter(|(p, _)| !known.contains(p.as_str())),
                        )
                    {
                        if named.len() == 5 {
                            break;
                        }
                        named.push(&peer[..8]);
                    }
                    let suffix = if ahead_peers.len() > 5 { ", …" } else { "" };
                    format!("{warning} (claimed for: {}{suffix})", named.join(", "))
                });
                if let Err(e) = self.persist_ack_ahead(&known, &ahead_peers) {
                    report.warnings.push(format!(
                        "the ahead note could not be saved ({e}); \
                         it is in this report, but will not be shown again"
                    ));
                }
            }
            Ok(None) => {}
            Err(e) => report.warnings.push(format!(
                "this device's ack could not be published ({e}); \
                 peers will prune less until it can be"
            )),
        }
        Ok(report)
    }

    /// Every peer emission point this vault has evidence
    /// of, by peer (see `SyncPeerCursor::seen_emitted`):
    /// the further of the bundle-derived point and any
    /// claim-derived point.
    fn seen_emitted(&self) -> Result<std::collections::BTreeMap<String, i64>> {
        let mut seen = std::collections::BTreeMap::new();
        for prefix in [SEEN_PREFIX, CLAIMED_PREFIX] {
            let conn = self.db.connection();
            let mut stmt = conn.prepare(&format!(
                "SELECT key, value FROM meta WHERE key LIKE '{prefix}%'"
            ))?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (key, value) = row?;
                if let (Some(peer), Ok(to)) =
                    (key.strip_prefix(prefix), value.parse::<i64>())
                {
                    let entry = seen.entry(peer.to_string()).or_insert(0);
                    *entry = (*entry).max(to);
                }
            }
        }
        Ok(seen)
    }

    /// Peers this vault has a first-hand tie to: a
    /// cursor or evidence, both of which come from
    /// bundles it opened. Claim evidence is persisted
    /// only for these — an unknown id in a plaintext
    /// file is not a peer, it is just a string. (A file
    /// NAME in the folder proves nothing: anyone who
    /// can write the folder can name a file.)
    fn known_peers(&self) -> Result<std::collections::BTreeSet<String>> {
        let mut known: std::collections::BTreeSet<String> =
            self.applied_cursors()?.into_keys().collect();
        known.extend(self.seen_emitted()?.into_keys());
        Ok(known)
    }

    /// Records bundle-derived evidence for one peer:
    /// a bundle this vault opened. Claim-derived
    /// evidence has its own writer (note_claimed).
    fn note_seen(tx: &rusqlite::Transaction<'_>, peer: &str, to_lseq: i64) -> Result<()> {
        Self::note_point(tx, &format!("{SEEN_PREFIX}{peer}"), to_lseq)
    }

    /// Claim-derived evidence, kept apart from the
    /// bundle-derived kind: a claim comes from a
    /// plaintext file anyone with folder write access
    /// can plant, so dismiss and restamp clear this
    /// kind — a garbled claim must not wedge the
    /// re-emit hint forever. Bundle evidence needs no
    /// clearing: the bundle existed and was opened.
    fn note_claimed(tx: &rusqlite::Transaction<'_>, peer: &str, to_lseq: i64) -> Result<()> {
        Self::note_point(tx, &format!("{CLAIMED_PREFIX}{peer}"), to_lseq)
    }

    fn note_point(tx: &rusqlite::Transaction<'_>, key: &str, to_lseq: i64) -> Result<()> {
        let key = key.to_string();
        let previous: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key = ?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        let previous = previous.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        if to_lseq > previous {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, to_lseq.to_string()],
            )?;
        }
        Ok(())
    }

    /// Every per-peer cursor this vault holds, by peer.
    fn applied_cursors(&self) -> Result<std::collections::BTreeMap<String, i64>> {
        let mut applied = std::collections::BTreeMap::new();
        let conn = self.db.connection();
        let mut stmt =
            conn.prepare("SELECT key, value FROM meta WHERE key LIKE 'sync_applied:%'")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (key, value) = row?;
            if let (Some(peer), Ok(to)) =
                (key.strip_prefix(APPLIED_PREFIX), value.parse::<i64>())
            {
                applied.insert(peer.to_string(), to);
            }
        }
        Ok(applied)
    }

    /// This vault's sync standing (see `SyncOverview`):
    /// identity, emit point, and the per-peer cursors.
    pub fn sync_overview(&self) -> Result<SyncOverview> {
        Ok(SyncOverview {
            device_id: self.device_id.clone(),
            emitted_lseq: self
                .db
                .meta(EMITTED_KEY)?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            peers: {
                let applied = self.applied_cursors()?;
                let seen = self.seen_emitted()?;
                let mut ids: std::collections::BTreeSet<&String> =
                    applied.keys().collect();
                ids.extend(seen.keys());
                ids.into_iter()
                    .map(|device_id| SyncPeerCursor {
                        device_id: device_id.clone(),
                        applied_lseq: applied.get(device_id).copied().unwrap_or(0),
                        seen_emitted: seen.get(device_id).copied().unwrap_or(0),
                    })
                    .collect()
            },
            ack_ahead_seen: self.db.meta(ACK_AHEAD_KEY)?.is_some(),
        })
    }

    /// Clears the durable ahead-of-vault flag (the one
    /// `sync_overview` reports) without restamping.
    pub fn clear_ack_ahead(&mut self) -> Result<()> {
        let tx = self.db.conn_mut().transaction()?;
        tx.execute("DELETE FROM meta WHERE key = ?1", [ACK_AHEAD_KEY])?;
        // Dismissing the note retires the claims it was
        // built on: they were never verified, and left
        // in place a garbled one would hold the re-emit
        // hint open forever.
        tx.execute(
            &format!("DELETE FROM meta WHERE key LIKE '{CLAIMED_PREFIX}%'"),
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Writes this vault's ack file into the sync folder:
    /// the per-peer cursors, so peers can tell which of
    /// their bundles every device has consumed.
    ///
    /// The ack cannot be trusted while a device identity
    /// is shared. A bit-identical copy's cursors are
    /// indistinguishable from this device's, so under an
    /// unrestamped copy the file can claim consumption
    /// that never happened on the original — and nothing
    /// in the ack can tell the copies apart, because a
    /// copy holds everything the original holds. Restamp
    /// ends the sharing; the durable fix is the device
    /// id moving out of the database (Phase 10b step 3).
    /// Until then this is a known gap, not a guarantee.
    ///
    /// The invariant is that the ack never claims more
    /// than this database holds, so the file is always
    /// rewritten to this vault's cursors. A file found
    /// AHEAD of this database can only have been written
    /// by another copy sharing the identity, left behind
    /// by a restore from backup, or planted — the caller
    /// is told (the return value) and the file is
    /// corrected downward in the same write. That makes
    /// the apply path a detector the bundle index can't
    /// be: an apply-only copy leaves no bundle behind,
    /// but it moves the ack, and the move shows here.
    /// Makes an ack-ahead detection durable: the flag,
    /// plus the claims as evidence for known peers (a
    /// copy of this vault consumed those bundles, so
    /// their authors emitted at least that far — even
    /// though THIS vault never saw them; claims about
    /// unknown peers raised the flag and go no further).
    fn persist_ack_ahead(
        &mut self,
        known: &std::collections::BTreeSet<String>,
        ahead_peers: &[(String, i64)],
    ) -> Result<()> {
        let tx = self.db.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = '1'",
            params![ACK_AHEAD_KEY],
        )?;
        for (peer, to) in ahead_peers {
            if known.contains(peer.as_str()) {
                Self::note_claimed(&tx, peer, *to)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn publish_ack(&self, sync_dir: &Path) -> Result<Option<(String, Vec<(String, i64)>)>> {
        let applied = self.applied_cursors()?;
        let ack = SyncAck {
            format: ACK_FORMAT,
            vault_id: self.header().vault_id.clone(),
            device_id: self.device_id.clone(),
            applied,
        };
        let bytes = serde_json::to_vec(&ack)?;
        let acks_dir = sync_dir.join(ACK_DIR);
        let dest = acks_dir.join(format!("{}.json", self.device_id));
        let mut warning = None;
        let oversized = fs::metadata(&dest)
            .map(|m| m.len() > ACK_MAX_BYTES)
            .unwrap_or(false);
        if oversized {
            // Not readable as an ack at all: it is
            // replaced below like any stale file, and
            // the replacement is reported — without
            // claims, since none could be read.
            warning = Some((
                "an ack file in the folder is too large to be an ack and \
                 was replaced with this device's own"
                    .to_string(),
                Vec::new(),
            ));
        } else if let Ok(existing) = fs::read(&dest) {
            // Write only what changed: republishing an
            // identical ack would make folder mirrors
            // re-copy the file on every sync, and the
            // repair case (missing or stale file) is
            // exactly the differing one.
            if existing == bytes {
                return Ok(None);
            }
            if let Ok(file_ack) = serde_json::from_slice::<SyncAck>(&existing) {
                // Detection and persistence are separate
                // gates. Detection — the warning and the
                // flag — takes any claim that is a
                // canonical device id, not this device,
                // and ahead of the truth: a peer this
                // vault has never seen can still have
                // lost entries to a copy's claim, and
                // silence there is the worst outcome. The
                // flag is a single row, so forged ids
                // cannot spam anything with it.
                // Persistence of the claims as evidence
                // is the narrow gate, applied by the
                // caller: known peers only, so junk ids
                // never become meta rows or phantom
                // peers. (No cap on this list: it is
                // transient, and a sorted cap would cut
                // real peers out from behind forged ones;
                // persistence is the bounded step.)
                // A file ahead only in junk or
                // self claims raises nothing: there is
                // nothing actionable to report.
                let ahead_peers: Vec<(String, i64)> = if file_ack.vault_id == ack.vault_id {
                    file_ack
                        .applied
                        .iter()
                        .filter(|(peer, to)| {
                            **to > ack.applied.get(peer.as_str()).copied().unwrap_or(0)
                                && peer.as_str() != self.device_id
                                && is_canonical_uuid(peer)
                        })
                        .map(|(peer, to)| (peer.clone(), *to))
                        .collect()
                } else {
                    Vec::new()
                };
                let ahead = !ahead_peers.is_empty();
                if ahead {
                    warning = Some((
                        "this device's ack in the folder is ahead of this vault: \
                         another copy may share its identity, or the vault was \
                         restored. Bundles a peer already pruned on the strength \
                         of it are lost to this vault until that peer re-emits \
                         everything. If a copy shares this identity, restamp \
                         the copy — restamping is not a recovery tool: it \
                         strands the old identity's ack and cursor in the folder"
                            .to_string(),
                        ahead_peers,
                    ));
                }
            }
        }
        fs::create_dir_all(&acks_dir)?;
        let tmp = acks_dir.join(format!(".tmp-{}", self.device_id));
        fs::write(&tmp, &bytes)?;
        rename_retry(&tmp, &dest)?;
        Ok(warning)
    }

    /// Removes this vault's own consumed bundles — and
    /// the blobs of attachments whose deletion is equally
    /// consumed — from the sync folder.
    ///
    /// A bundle is removed only when EVERY peer known to
    /// the folder has published an ack covering it: the
    /// peers are the devices that publish acks here plus
    /// the authors of bundles present here, and a peer
    /// with no ack counts as having consumed nothing.
    /// With no peers, or no acks, nothing is removed —
    /// pruning must never guess. A device that joins the
    /// folder later is NOT covered by this rule: it can
    /// only receive what the folder and its peers still
    /// hold, so prune only once the peer set is settled.
    ///
    /// Blobs of LIVE attachments are never removed (the
    /// folder is the only place a peer that has not
    /// fetched yet can get the bytes); a blob whose
    /// attachment is a tombstone in this vault, with the
    /// tombstone itself covered by every ack, is dead
    /// weight on every device. That a peer's coverage of
    /// this stream implies it holds the tombstone leans
    /// on relay: applied records are re-stamped with a
    /// local lseq and re-emitted, so the tombstone also
    /// travels in every holder's own stream.
    pub fn sync_prune(&mut self, sync_dir: &Path) -> Result<SyncPruneReport> {
        let mut report = SyncPruneReport::default();
        if !sync_dir.is_dir() {
            return Ok(report);
        }
        let my_vault = self.header().vault_id.clone();
        let my_vault_short = short(&my_vault).to_string();
        let acks = read_acks(sync_dir, &my_vault);
        let mut peers: std::collections::BTreeSet<String> =
            acks.iter().map(|a| a.device_id.clone()).collect();
        let mut mine: Vec<(PathBuf, i64)> = Vec::new();
        for item in fs::read_dir(sync_dir)? {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            if let Some((prefix, device, _from, to)) = parse_bundle_name_full(&name) {
                if prefix != my_vault_short {
                    continue;
                }
                if device == self.device_id || device == short(&self.device_id) {
                    mine.push((item.path(), to));
                } else if device.len() == 36 {
                    peers.insert(device);
                }
            }
        }
        peers.remove(&self.device_id);
        if peers.is_empty() {
            return Ok(report);
        }
        let covered = peers
            .iter()
            .map(|peer| {
                acks.iter()
                    .find(|a| &a.device_id == peer)
                    .and_then(|a| a.applied.get(&self.device_id).copied())
                    .unwrap_or(0)
            })
            .min()
            .unwrap_or(0);
        report.covered_through = covered;
        if covered == 0 {
            return Ok(report);
        }
        for (path, to) in mine {
            if to <= covered && fs::remove_file(&path).is_ok() {
                report.bundles_removed += 1;
            }
        }
        let blobs_dir = sync_dir.join("blobs");
        if blobs_dir.is_dir() {
            for item in fs::read_dir(&blobs_dir)? {
                let item = item?;
                let path = item.path();
                if !path.is_file() {
                    continue;
                }
                let id = item.file_name().to_string_lossy().into_owned();
                let row: Option<(Option<i64>, i64)> = self
                    .db
                    .connection()
                    .query_row(
                        "SELECT deleted_at, lseq FROM attachment WHERE id = ?1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .ok();
                if let Some((Some(_deleted_at), lseq)) = row
                    && lseq <= covered
                    && fs::remove_file(&path).is_ok()
                {
                    report.blobs_removed += 1;
                }
            }
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
        tx.execute("DELETE FROM meta WHERE key = ?1", [ACK_AHEAD_KEY])?;
        tx.execute(
            &format!("DELETE FROM meta WHERE key LIKE '{CLAIMED_PREFIX}%'"),
            [],
        )?;
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
        assert!(matches!(err, Error::SharedDeviceId), "{err}");
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

    #[test]
    fn one_loser_one_copy_regardless_of_tag_spelling() {
        const DEVICE_C: &str = "cccccccc-3333-4333-8333-cccccccccccc";
        fn tagged(body: &str, tag: &str) -> EntryDraft {
            EntryDraft {
                entry_date: "2026-10-16".into(),
                title: Some("Note".into()),
                body_md: body.into(),
                mood: None,
                tz: None,
                tags: vec![tag.into()],
            }
        }
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        // C: a third device whose tag table learns "foo"
        // (lowercase) before it ever sees the entry.
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
        c.create_entry(&tagged("C's own", "foo")).unwrap();
        // A authors the entry with the spelling "Foo".
        let entry = a.create_entry(&tagged("base", "Foo")).unwrap();
        a.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        c.sync_apply(sync.path()).unwrap();
        // A and B diverge on it; B resolves (loser seen with
        // A's spelling), then C resolves (loser is C's local
        // record, whose tag reads "foo" from C's table).
        a.update_entry(&entry.id, &tagged("A's edited", "Foo")).unwrap();
        b.update_entry(&entry.id, &tagged("B's words", "Foo")).unwrap();
        exchange(&mut a, &mut b, sync.path());
        b.sync_emit(sync.path()).unwrap();
        c.sync_apply(sync.path()).unwrap();
        c.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        // One loser, one text — exactly one copy of it on B,
        // because the copy's identity is derived from the
        // tag's name key, not from any device's spelling.
        let page = b.list_entries(&crate::entries::ListQuery::default()).unwrap();
        let copies = page
            .items
            .iter()
            .filter(|i| b.get_entry(&i.id).unwrap().body_md == "A's edited")
            .count();
        assert_eq!(copies, 1, "the same loser produced {copies} copies");
    }

    fn bundle_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|i| i.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".sync"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn apply_records_cursors_and_publishes_an_ack() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.create_entry(&draft("one")).unwrap();
        let info = a.sync_emit(sync.path()).unwrap().unwrap();
        b.sync_apply(sync.path()).unwrap();
        let cursor = b.db.meta(&format!("sync_applied:{}", a.device_id)).unwrap();
        assert_eq!(cursor.as_deref(), Some(info.to_lseq.to_string().as_str()));
        // The ack file is in the folder, addressed by B's
        // full device id, carrying the same cursor.
        let ack_path = sync
            .path()
            .join("acks")
            .join(format!("{}.json", b.device_id));
        let ack: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ack_path).unwrap()).unwrap();
        assert_eq!(ack["device_id"], b.device_id.as_str());
        assert_eq!(ack["applied"][a.device_id.as_str()], info.to_lseq);
    }

    #[test]
    fn prune_waits_for_every_peer_then_removes_bundles() {
        const DEVICE_C: &str = "cccccccc-3333-4333-8333-cccccccccccc";
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
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

        let entry = a.create_entry(&draft("one")).unwrap();
        let first = a.sync_emit(sync.path()).unwrap().unwrap();
        b.sync_apply(sync.path()).unwrap();
        c.sync_apply(sync.path()).unwrap();
        a.update_entry(&entry.id, &draft("two")).unwrap();
        let second = a.sync_emit(sync.path()).unwrap().unwrap();
        b.sync_apply(sync.path()).unwrap();
        // C has not consumed the second bundle: only the
        // first is covered by every ack.
        let report = a.sync_prune(sync.path()).unwrap();
        assert_eq!(report.covered_through, first.to_lseq);
        assert_eq!(report.bundles_removed, 1);
        let names = bundle_names(sync.path());
        assert!(!names.contains(&first.file_name), "{names:?}");
        assert!(names.contains(&second.file_name), "{names:?}");
        // Once C catches up, the second bundle goes too —
        // and the entry itself lives on in C's database.
        c.sync_apply(sync.path()).unwrap();
        let report = a.sync_prune(sync.path()).unwrap();
        assert_eq!(report.bundles_removed, 1);
        assert!(bundle_names(sync.path()).is_empty());
        assert_eq!(c.get_entry(&entry.id).unwrap().body_md, "two");
    }

    #[test]
    fn prune_without_peers_removes_nothing() {
        let (_da, _db, mut a, _b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.create_entry(&draft("one")).unwrap();
        a.sync_emit(sync.path()).unwrap();
        let report = a.sync_prune(sync.path()).unwrap();
        assert_eq!(report.bundles_removed, 0);
        assert_eq!(report.covered_through, 0);
        assert_eq!(bundle_names(sync.path()).len(), 1);
    }

    #[test]
    fn prune_removes_only_covered_tombstone_blobs() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("with file")).unwrap();
        let att = a
            .add_attachment(&entry.id, "note.txt", "text/plain", b"blob bytes")
            .unwrap();
        a.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        let blob = sync.path().join("blobs").join(&att.id);
        assert!(blob.exists());
        // Covered by B's ack — but the attachment is LIVE,
        // so its blob stays: the folder is where a peer
        // that has not fetched yet gets the bytes.
        let report = a.sync_prune(sync.path()).unwrap();
        assert_eq!(report.blobs_removed, 0);
        assert!(blob.exists());
        // Delete it, sync the tombstone, and the blob is
        // dead weight everywhere: pruned.
        a.delete_attachment(&att.id).unwrap();
        a.sync_emit(sync.path()).unwrap();
        b.sync_apply(sync.path()).unwrap();
        let report = a.sync_prune(sync.path()).unwrap();
        assert_eq!(report.blobs_removed, 1);
        assert!(!blob.exists());
    }

    #[test]
    fn a_failed_attachment_is_not_acknowledged_and_heals_later() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let entry = a.create_entry(&draft("with file")).unwrap();
        let att = a
            .add_attachment(&entry.id, "note.txt", "text/plain", b"blob bytes")
            .unwrap();
        let info = a.sync_emit(sync.path()).unwrap().unwrap();
        // The blob has not reached the folder yet (the
        // mover is still copying it): the attachment record
        // fails as a problem, so the bundle must NOT be
        // acknowledged — pruning has to leave it alone.
        let blob = sync.path().join("blobs").join(&att.id);
        let blob_bytes = std::fs::read(&blob).unwrap();
        std::fs::remove_file(&blob).unwrap();
        let report = b.sync_apply(sync.path()).unwrap();
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(b.list_attachments(&entry.id).unwrap().is_empty());
        // The invariant itself, at its source: no cursor
        // was recorded for A. The prune assertions below
        // follow from this — they must not be the only
        // thing standing between the cursor and a claim it
        // never earned.
        assert_eq!(
            b.db.meta(&format!("sync_applied:{}", a.device_id)).unwrap(),
            None
        );
        let pruned = a.sync_prune(sync.path()).unwrap();
        assert_eq!(pruned.bundles_removed, 0);
        assert!(sync.path().join(&info.file_name).exists());
        // When the blob arrives, the retry heals the apply.
        std::fs::write(&blob, &blob_bytes).unwrap();
        let report = b.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(b.list_attachments(&entry.id).unwrap().len(), 1);
    }

    #[test]
    fn a_gap_in_a_peers_stream_is_not_acknowledged() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        let e1 = a.create_entry(&draft("first")).unwrap();
        let first = a.sync_emit(sync.path()).unwrap().unwrap();
        let e2 = a.create_entry(&draft("second")).unwrap();
        let second = a.sync_emit(sync.path()).unwrap().unwrap();
        // The first bundle arrives torn (the mover is mid-
        // copy); the second is intact but must not be
        // acknowledged over the gap.
        let p1 = sync.path().join(&first.file_name);
        let intact = std::fs::read(&p1).unwrap();
        std::fs::write(&p1, &intact[..intact.len() / 2]).unwrap();
        b.sync_apply(sync.path()).unwrap();
        assert!(b.get_entry(&e1.id).is_err(), "e1 cannot be there yet");
        assert!(b.get_entry(&e2.id).is_ok());
        // The intact second bundle must not be acknowledged
        // over the gap either: still no cursor for A.
        assert_eq!(
            b.db.meta(&format!("sync_applied:{}", a.device_id)).unwrap(),
            None
        );
        a.sync_prune(sync.path()).unwrap();
        assert!(
            sync.path().join(&first.file_name).exists(),
            "the unapplied bundle was pruned on an ack that jumped the gap"
        );
        // Once the bundle arrives whole, the stream heals:
        // both bundles apply in order and the cursor
        // covers them.
        std::fs::write(&p1, &intact).unwrap();
        let report = b.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(b.get_entry(&e1.id).is_ok());
        let cursor = b.db.meta(&format!("sync_applied:{}", a.device_id)).unwrap();
        assert_eq!(cursor.as_deref(), Some(second.to_lseq.to_string().as_str()));
    }

    #[test]
    fn overview_reports_identity_emit_point_and_cursors() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.create_entry(&draft("one")).unwrap();
        let info = a.sync_emit(sync.path()).unwrap().unwrap();
        let over_a = a.sync_overview().unwrap();
        assert_eq!(over_a.device_id, a.device_id);
        assert_eq!(over_a.emitted_lseq, info.to_lseq);
        assert!(over_a.peers.is_empty());
        b.sync_apply(sync.path()).unwrap();
        let over_b = b.sync_overview().unwrap();
        assert_eq!(over_b.device_id, b.device_id);
        assert_eq!(over_b.emitted_lseq, 0);
        assert_eq!(
            over_b.peers,
            vec![SyncPeerCursor {
                device_id: a.device_id.clone(),
                applied_lseq: info.to_lseq,
                seen_emitted: info.to_lseq,
            }]
        );
    }

    #[test]
    fn apply_publishes_and_repairs_this_devices_ack() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.create_entry(&draft("one")).unwrap();
        // Emit alone does not publish: the ack belongs to
        // apply, the only operation that moves cursors.
        a.sync_emit(sync.path()).unwrap();
        let a_ack = sync.path().join("acks").join(format!("{}.json", a.device_id));
        assert!(!a_ack.exists());
        // B applies: B's ack appears, naming A's stream.
        b.sync_apply(sync.path()).unwrap();
        let b_ack_path = sync.path().join("acks").join(format!("{}.json", b.device_id));
        let b_ack = fs::read(&b_ack_path).unwrap();
        assert!(String::from_utf8(b_ack).unwrap().contains(&a.device_id));
        // A lost ack is repaired by the next apply, even
        // one where no cursor moves.
        fs::remove_file(&b_ack_path).unwrap();
        let report = b.sync_apply(sync.path()).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(b_ack_path.exists());
    }

    #[test]
    fn a_no_op_emit_writes_nothing() {
        let (_da, _db, mut a, _b) = two_devices();
        let sync = TempDir::new().unwrap();
        // Into an existing folder: no bundles, no acks dir.
        assert!(a.sync_emit(sync.path()).unwrap().is_none());
        assert!(!sync.path().join("acks").exists());
        // Into a missing path: still nothing, no phantom
        // folder tree where a drive used to be mounted.
        let missing = sync.path().join("gone");
        assert!(a.sync_emit(&missing).unwrap().is_none());
        assert!(!missing.exists());
    }

    #[test]
    fn a_failed_ack_publish_is_noted_in_the_apply_report() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        // "acks" exists as a FILE: publishing must fail.
        // A's apply carries that as a warning, not a
        // problem — the data itself applied fine.
        fs::write(sync.path().join("acks"), b"not a dir").unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert_eq!(report.entries_applied, 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("ack could not be published")),
            "{:?}",
            report.warnings
        );
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn an_ack_ahead_of_this_vault_is_reported_and_corrected() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        let info1 = b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        // The file claims consumption this vault never
        // made (a copy, a restore, or a plant). The next
        // apply must say so AND put the file back to this
        // database's truth: the ack never claims more
        // than this vault holds.
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        let mut file_ack: SyncAck =
            serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
        file_ack.applied.insert(b.device_id.clone(), 99);
        fs::write(&ack_path, serde_json::to_vec(&file_ack).unwrap()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        let ahead = report.ack_ahead.as_deref().unwrap_or_default();
        assert!(ahead.contains("ahead of this vault"), "{ahead}");
        // The detection is durable, not one-shot.
        assert!(a.sync_overview().unwrap().ack_ahead_seen);
        a.clear_ack_ahead().unwrap();
        assert!(!a.sync_overview().unwrap().ack_ahead_seen);
        let after: SyncAck =
            serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
        assert_eq!(after.applied.get(&b.device_id), Some(&info1.to_lseq));
    }

    #[test]
    fn an_ack_moved_by_a_twin_is_detected_and_corrected() {
        // The residual gap, until the device id leaves
        // the database (step 3): a bit-identical copy that
        // applies a peer's bundle still publishes its
        // cursor under the shared id, and until the
        // original next applies, a prune on the peer
        // would trust the claim. What changed: the
        // original's next apply now DETECTS the moved
        // ack (the bundle index never could — an
        // apply-only copy writes no bundle), warns, and
        // corrects the file back to its own cursors.
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        let info1 = b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        b.create_entry(&draft("y2")).unwrap();
        let info2 = b.sync_emit(sync.path()).unwrap().unwrap();
        twin.sync_apply(sync.path()).unwrap();
        // The harm lands through the real chain: B
        // prunes, trusting the ack the twin moved, and
        // y2's bundle goes with it.
        let pruned = b.sync_prune(sync.path()).unwrap();
        assert!(pruned.bundles_removed >= 1);
        assert!(!sync.path().join(&info2.file_name).exists());
        let ack: SyncAck = serde_json::from_slice(
            &fs::read(sync.path().join("acks").join(format!("{}.json", a.device_id)))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ack.applied.get(&b.device_id), Some(&(info1.to_lseq + 1)));
        // ...while the original's own cursor never moved.
        let over_a = a.sync_overview().unwrap();
        assert_eq!(
            over_a.peers,
            vec![SyncPeerCursor {
                device_id: b.device_id.clone(),
                applied_lseq: info1.to_lseq,
                seen_emitted: info1.to_lseq,
            }]
        );
        // The original's next apply detects the ahead
        // file, reports it in the failure channel, and
        // corrects it downward.
        let report = a.sync_apply(sync.path()).unwrap();
        let ahead = report.ack_ahead.as_deref().unwrap_or_default();
        assert!(ahead.contains("ahead of this vault"), "{ahead}");
        assert!(a.sync_overview().unwrap().ack_ahead_seen);
        let ack: SyncAck = serde_json::from_slice(
            &fs::read(sync.path().join("acks").join(format!("{}.json", a.device_id)))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ack.applied.get(&b.device_id), Some(&info1.to_lseq));
    }

    #[test]
    fn a_reemit_heals_what_a_prune_took() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        a.sync_apply(sync.path()).unwrap();
        // A twin of A moves A's ack past y2; B prunes on
        // the strength of it; y2 never reaches A.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        b.create_entry(&draft("y2")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        twin.sync_apply(sync.path()).unwrap();
        b.sync_prune(sync.path()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.ack_ahead.is_some());
        let count = |v: &Vault| {
            v.list_entries(&crate::entries::ListQuery::default())
                .unwrap()
                .items
                .len()
        };
        assert_eq!(count(&a), 1, "y2 must be lost");
        // The peer re-emits everything, same identity:
        // the full-range bundle lands, y2 arrives, and
        // A's cursor for B unsticks to the end.
        let full = b.sync_reemit(sync.path()).unwrap().unwrap();
        assert_eq!(full.from_lseq, 1);
        let healed = a.sync_apply(sync.path()).unwrap();
        assert_eq!(healed.entries_applied, 1);
        assert_eq!(count(&a), 2, "y2 must be back");
        let over_a = a.sync_overview().unwrap();
        assert_eq!(
            over_a.peers,
            vec![SyncPeerCursor {
                device_id: b.device_id.clone(),
                applied_lseq: full.to_lseq,
                seen_emitted: full.to_lseq,
            }]
        );
    }

    #[test]
    fn two_reemits_in_a_row_agree() {
        let (_da, _db, _a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        let first = b.sync_reemit(sync.path()).unwrap().unwrap();
        let bytes = fs::read(sync.path().join(&first.file_name)).unwrap();
        // Nothing changed since: the same range name and
        // the identical payload, so the guard passes.
        let second = b.sync_reemit(sync.path()).unwrap().unwrap();
        assert_eq!(first.file_name, second.file_name);
        assert_eq!(fs::read(sync.path().join(&second.file_name)).unwrap(), bytes);
    }

    #[test]
    fn a_failed_reemit_keeps_the_watermark() {
        let (_da, _db, _a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.create_entry(&draft("y2")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        assert_eq!(b.sync_overview().unwrap().emitted_lseq, 2);
        b.create_entry(&draft("y3")).unwrap();
        // A garbage file already sits at the full-range
        // name: the re-emit must fail with SyncUnreadable
        // (not an identity clash — nothing there decrypts
        // or even parses) and leave the stored watermark
        // exactly as it was.
        let name = format!(
            "{}-{}-00000001-00000003.sync",
            &b.header().vault_id[..8],
            b.device_id
        );
        fs::write(sync.path().join(&name), b"torn\n").unwrap();
        let err = b.sync_reemit(sync.path()).unwrap_err();
        assert!(matches!(err, Error::SyncUnreadable(_)), "{err}");
        assert_eq!(b.sync_overview().unwrap().emitted_lseq, 2);
        // And the next ordinary emit is an ordinary emit:
        // just the new record, from the intact watermark.
        fs::remove_file(sync.path().join(&name)).unwrap();
        let info = b.sync_emit(sync.path()).unwrap().unwrap();
        assert_eq!(info.from_lseq, 3);
    }

    #[test]
    fn a_twin_reemit_with_a_colliding_name_is_shared_device_id() {
        let (_da, _db, _a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap();
        // A bit-identical copy adds its own record and
        // re-emits the full range under the shared name.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(b.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        twin.create_entry(&draft("twin's")).unwrap();
        twin.sync_reemit(sync.path()).unwrap();
        // The original's diverging record takes the same
        // lseq, so its full-range bundle takes the same
        // name with a different payload: the guard fires.
        b.create_entry(&draft("original's")).unwrap();
        let err = b.sync_reemit(sync.path()).unwrap_err();
        assert!(matches!(err, Error::SharedDeviceId), "{err}");
    }

    #[test]
    fn a_torn_own_bundle_is_replaced_silently() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.create_entry(&draft("y2")).unwrap();
        let info = b.sync_emit(sync.path()).unwrap().unwrap();
        // Tear the bundle: the header line survives (it
        // still names this vault and device), the sealed
        // body does not. A re-emit over the same range
        // replaces it without a word — the crash-between-
        // write-and-commit case from the emit guard.
        let path = sync.path().join(&info.file_name);
        let bytes = fs::read(&path).unwrap();
        let head_end = bytes.iter().position(|c| *c == b'\n').unwrap();
        let mut torn = bytes[..=head_end].to_vec();
        torn.extend_from_slice(b"garbagegarbagegarbagegarbage12");
        fs::write(&path, torn).unwrap();
        let info2 = b.sync_reemit(sync.path()).unwrap().unwrap();
        assert_eq!(info2.file_name, info.file_name);
        // And the replacement is a working bundle.
        let report = a.sync_apply(sync.path()).unwrap();
        assert_eq!(report.entries_applied, 2);
    }

    #[test]
    fn seen_emitted_tracks_bundles_and_ack_claims() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        // Direct evidence: a bundle A opens.
        b.create_entry(&draft("y1")).unwrap();
        let info1 = b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let peer = a.sync_overview().unwrap().peers.remove(0);
        assert_eq!(peer.applied_lseq, info1.to_lseq);
        assert_eq!(peer.seen_emitted, info1.to_lseq);
        // Detector evidence: a twin consumes y2, the ack
        // it moves claims the consumption, B prunes the
        // bundle — A never sees it, yet the emission is
        // on record here.
        let dir_t = TempDir::new().unwrap();
        let t_dir = dir_t.path().join("v");
        copy_dir(a.dir(), &t_dir);
        let mut twin = Vault::open_with_passphrase(&t_dir, PASSPHRASE).unwrap();
        b.create_entry(&draft("y2")).unwrap();
        let info2 = b.sync_emit(sync.path()).unwrap().unwrap();
        twin.sync_apply(sync.path()).unwrap();
        b.sync_prune(sync.path()).unwrap();
        a.sync_apply(sync.path()).unwrap();
        let peer = a.sync_overview().unwrap().peers.remove(0);
        assert_eq!(peer.applied_lseq, info1.to_lseq);
        assert_eq!(peer.seen_emitted, info2.to_lseq);
    }

    #[test]
    fn dismiss_and_restamp_clear_claimed_evidence() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        let plant = |ack_path: &std::path::Path, b_id: &str| {
            let mut file_ack: SyncAck =
                serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
            file_ack.applied.insert(b_id.to_string(), 99999);
            fs::write(ack_path, serde_json::to_vec(&file_ack).unwrap()).unwrap();
        };
        // The claim lands: seen jumps to the claimed point.
        plant(&ack_path, &b.device_id);
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.ack_ahead.is_some());
        assert_eq!(a.sync_overview().unwrap().peers[0].seen_emitted, 99999);
        // Dismiss retires it with the flag: only the
        // bundle-derived point (1) remains.
        a.clear_ack_ahead().unwrap();
        assert_eq!(a.sync_overview().unwrap().peers[0].seen_emitted, 1);
        // Restamp clears claims the same way.
        plant(&ack_path, &b.device_id);
        a.sync_apply(sync.path()).unwrap();
        a.restamp_device().unwrap();
        let over = a.sync_overview().unwrap();
        assert_eq!(over.peers[0].seen_emitted, 1);
        assert!(!over.ack_ahead_seen);
    }

    #[test]
    fn unknown_claims_flag_but_do_not_persist() {
        let (_da, _db, mut a, _b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        let mut file_ack: SyncAck =
            serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
        file_ack.applied.insert("not-a-uuid".to_string(), 50);
        for _ in 0..100 {
            file_ack
                .applied
                .insert(uuid::Uuid::now_v7().to_string(), 50);
        }
        fs::write(&ack_path, serde_json::to_vec(&file_ack).unwrap()).unwrap();
        // Detection is the wide gate: canonical claims
        // ahead of the truth raise the flag — a peer
        // this vault has never seen can still have lost
        // entries to a copy's claim. Persistence is the
        // narrow gate: no rows, no phantom peers.
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.ack_ahead.is_some());
        assert!(a.sync_overview().unwrap().ack_ahead_seen);
        assert!(a.sync_overview().unwrap().peers.is_empty());
    }

    #[test]
    fn a_known_peer_persists_among_strangers() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        let mut file_ack: SyncAck =
            serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
        file_ack.applied.insert(b.device_id.clone(), 999);
        for _ in 0..100 {
            file_ack
                .applied
                .insert(uuid::Uuid::now_v7().to_string(), 50);
        }
        fs::write(&ack_path, serde_json::to_vec(&file_ack).unwrap()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report
            .ack_ahead
            .as_deref()
            .is_some_and(|t| t.contains(&b.device_id[..8])));
        let over = a.sync_overview().unwrap();
        assert_eq!(over.peers.len(), 1);
        assert_eq!(over.peers[0].device_id, b.device_id);
        assert_eq!(over.peers[0].seen_emitted, 999);
    }

    #[test]
    fn a_claim_about_this_device_itself_is_not_persisted() {
        let (_da, _db, mut a, mut b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.create_entry(&draft("x")).unwrap();
        a.sync_emit(sync.path()).unwrap().unwrap();
        b.create_entry(&draft("y1")).unwrap();
        b.sync_emit(sync.path()).unwrap().unwrap();
        a.sync_apply(sync.path()).unwrap();
        // A copy's ack claims this device consumed its
        // OWN stream to 50. This device's bundle is in
        // the folder and its cursor for B exists — only
        // the explicit self rule keeps the claim out of
        // both detection and persistence.
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        let mut file_ack: SyncAck =
            serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
        file_ack.applied.insert(a.device_id.clone(), 50);
        fs::write(&ack_path, serde_json::to_vec(&file_ack).unwrap()).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        assert!(report.ack_ahead.is_none());
        assert!(a
            .sync_overview()
            .unwrap()
            .peers
            .iter()
            .all(|p| p.device_id != a.device_id));
    }

    #[test]
    fn an_oversized_ack_is_replaced_and_reported() {
        let (_da, _db, mut a, _b) = two_devices();
        let sync = TempDir::new().unwrap();
        a.sync_apply(sync.path()).unwrap();
        let ack_path = sync.path().join("acks").join(format!("{}.json", a.device_id));
        // A real ack's shape, padded past the size bound
        // with a junk field serde would otherwise ignore:
        // the bound is on bytes, before any parsing.
        let raw = format!(
            "{{\"format\":1,\"vault_id\":\"{}\",\"device_id\":\"{}\",\"applied\":{{}},\"pad\":\"{}\"}}",
            a.header().vault_id,
            a.device_id,
            "x".repeat(70_000)
        );
        fs::write(&ack_path, raw).unwrap();
        let report = a.sync_apply(sync.path()).unwrap();
        let ahead = report.ack_ahead.expect("oversized must be reported");
        assert!(ahead.contains("too large"), "{ahead}");
        // The file is corrected: this device's own ack,
        // parseable and back under the bound.
        let now = fs::read(&ack_path).unwrap();
        assert!(now.len() <= ACK_MAX_BYTES as usize);
        assert!(serde_json::from_slice::<SyncAck>(&now).is_ok());
    }
}
