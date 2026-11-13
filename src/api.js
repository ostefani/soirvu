// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
// @ts-check
// The only module that calls invoke() (§6.1). Every wrapper is a
// thin, JSDoc-typed call to one Rust command. Errors arrive as
// stable code strings ("locked", "wrong_passphrase", "tag_exists",
// "backoff:12", ...) — the UI maps them to its own messages.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { scopeToWire } from "./lib/scope.js";

/**
 * @typedef {object} VaultInfo
 * @property {string} id
 * @property {string} name
 * @property {"available" | "volume_missing" | "not_found" | "unlocked"} state
 */

/**
 * @typedef {object} Picked
 * @property {string | null} vault_id Set for mode "open"; null for "create".
 * @property {string} name
 * @property {string | null} warning "cloud_sync" | "network_fs" when the
 *   folder looks unsuitable for SQLite (§3). A warning, never a refusal.
 */

/**
 * @typedef {object} Entry
 * @property {string} id
 * @property {string} entry_date "YYYY-MM-DD"
 * @property {string | null} title
 * @property {string} body_md
 * @property {number | null} mood 1–5
 * @property {string | null} tz IANA name the date is anchored to
 * @property {string[]} tags
 * @property {number} created_at Unix milliseconds
 * @property {number} updated_at Unix milliseconds
 * @property {number} rev
 * @property {string | null} folder_id Null is the vault root.
 */

/**
 * @typedef {object} EntrySummary
 * @property {string} id
 * @property {string} entry_date
 * @property {string | null} title
 * @property {string} excerpt
 * @property {string[]} tags
 * @property {number} updated_at
 * @property {string | null} folder_id Null is the vault root.
 */

/**
 * The entry fields both write commands take — no id, no folder
 * (creation files into a folder argument, updates never touch
 * membership). Rust refuses unknown fields, so a stale client
 * cannot smuggle an id or a folder through.
 * @typedef {object} DraftBody
 * @property {string} entry_date
 * @property {string | null} title
 * @property {string} body_md
 * @property {number | null} mood 1–5
 * @property {string | null} tz
 * @property {string[]} tags
 */

/**
 * @typedef {object} SearchHit
 * @property {string} id
 * @property {string} entry_date
 * @property {string | null} title
 * @property {string} snippet Plain text; \u0002…\u0003 mark the matches.
 * @property {string | null} folder_id Null is the vault root.
 */

/**
 * Which entries a scoped read may see. Defaults to All when
 * omitted — existing calls keep working unchanged.
 * The UI owns selections in JS casing; the wrappers convert to
 * the wire shape (notably the snake_case subtree flag) because
 * Rust deserializes nested structs strictly — Tauri's camelCase
 * conversion applies only to top-level command arguments.
 * @typedef {({ kind: "all" } | { kind: "root" } | { kind: "folder", id: string, includeDescendants?: boolean })} Scope
 */

/**
 * @typedef {object} Folder
 * @property {string} id
 * @property {string | null} parent_id Null is the vault root.
 * @property {string} name
 * @property {number} created_at Unix milliseconds
 * @property {number} updated_at Unix milliseconds
 * @property {number} rev
 * @property {number} entry_count Entries filed directly in it.
 * @property {boolean} recovered True when the stored parent is
 *   gone: the UI shows the folder at the root instead.
 */

/**
 * @typedef {object} LinkCandidate
 * @property {string} id
 * @property {string} entry_date
 * @property {string | null} title
 * @property {string | null} folder_id
 */

/**
 * @typedef {object} CandidatePage
 * @property {LinkCandidate[]} candidates
 * @property {string | null} next_cursor Opaque; pass back for more.
 */

/**
 * @typedef {object} BacklinkSource
 * @property {string} id
 * @property {string} entry_date
 * @property {string | null} title
 * @property {string | null} folder_id
 * @property {string | null} body Full body when it fits the inline
 *   budget, null for oversized bodies — never truncated.
 * @property {number} body_size Body length in bytes; the fetch
 *   budget and large-entry indicator read this, not the body.
 * @property {string} body_hash Hex SHA-256 over the body bytes;
 *   the verify cache keys on content, never on the revision.
 */

/**
 * @typedef {object} BacklinkPage
 * @property {BacklinkSource[]} candidates
 * @property {string | null} next_cursor Opaque; pass back for more.
 */

/**
 * @typedef {object} Settings
 * @property {number} lock_timeout_secs
 * @property {boolean} strip_exif
 * @property {boolean} clipboard_clear
 * @property {"off" | "daily" | "weekly"} backup_schedule
 * @property {{ image_mb: number, pdf_mb: number, audio_mb: number, video_mb: number }} caps
 * @property {string | null} backup_location Full path of the backup
 *   folder. Read-only here: it changes only through
 *   settingsPickBackupDir / settingsClearBackupDir.
 * @property {string} platform The OS this build runs on
 *   ("macos", "linux", "windows"); the UI offers only what
 *   the platform supports.
 * @property {string | null} sync_location Display name of the
 *   open diary's sync folder (never a path). Read-only here:
 *   it changes only through syncPickFolder / syncClearFolder.
 */

// ---------------------------------------------------------------------------
// Vault lifecycle

/** @returns {Promise<VaultInfo[]>} */
export function vaultList() {
    return invoke("vault_list");
}

/**
 * Opens the native folder picker (Rust-side; the path never
 * reaches the webview). Mode "open" registers the vault found
 * there and returns its id; mode "create" only remembers the
 * folder for vaultCreate().
 * @param {"open" | "create"} mode
 * @returns {Promise<Picked | null>} Null when the user cancels.
 */
export function vaultPickFolder(mode) {
    return invoke("vault_pick_folder", { mode });
}

/**
 * Creates a vault in the folder picked with mode "create".
 * @param {string} passphrase
 * @returns {Promise<{ vault_id: string, recovery_key: string }>}
 *   The recovery key is shown once and never stored by the app.
 */
export function vaultCreate(passphrase) {
    return invoke("vault_create", { passphrase });
}

/**
 * @param {string} vaultId
 * @param {"passphrase" | "recovery"} kind
 * @param {string} secret
 * @returns {Promise<void>} Rejects with "wrong_passphrase" (either
 *   secret wrong — Rust cannot tell which), "backoff:<secs>" while
 *   the post-failure wait runs, or "storage" if the folder is gone.
 */
export function vaultUnlock(vaultId, kind, secret) {
    return invoke("vault_unlock", { vaultId, kind, secret });
}

/**
 * Touch ID for a vault (Phase 8). Pass a vault id (the lock
 * screen asks about the selected diary) or null (Settings
 * asks about the open one). `capable` is a property of this
 * build's signature; `enrolled` of the diary.
 * @returns {Promise<{built_in: boolean, capable: boolean, enrolled: boolean}>}
 */
export function touchIdStatus(vaultId) {
    return invoke("touch_id_status", { vaultId });
}

/**
 * Unlocks with the fingerprint prompt. Rejects with
 * "touch_id_cancelled" when the prompt is dismissed (not a
 * failed attempt), "touch_id_unavailable" when the diary has
 * no live device slot in this build, "wrong_passphrase"-style
 * codes never occur here.
 * @returns {Promise<void>}
 */
export function vaultUnlockTouchId(vaultId) {
    return invoke("vault_unlock_touch_id", { vaultId });
}

/** @returns {Promise<void>} Rejects with "touch_id_unavailable". */
export function touchIdEnroll() {
    return invoke("touch_id_enroll");
}

/** @returns {Promise<void>} */
export function touchIdDisable() {
    return invoke("touch_id_disable");
}

/** @returns {Promise<void>} */
export function vaultLock() {
    return invoke("vault_lock");
}

/**
 * Acknowledge the lock handshake (§6.2): the UI flushed (or
 * has nothing to flush) for this lock generation.
 * @param {number} generation
 * @param {boolean} [ok] false when the UI could not flush
 */
export function vaultLockReady(generation, ok = true) {
    return invoke("vault_lock_ready", { generation, ok });
}

/** True once if the last automatic lock dropped unsaved edits. */
export function lockNoticeTake() {
    return invoke("lock_notice_take");
}

/** Tell Rust the user is active so the idle watcher measures inactivity, not time since unlock. */
export function vaultActivity() {
    return invoke("vault_activity");
}

/**
 * Removes a vault from the known list. Files are never touched (§3).
 * @param {string} vaultId
 */
export function vaultForget(vaultId) {
    return invoke("vault_forget", { vaultId });
}

/**
 * Re-points a moved vault: the user picks its new folder and Rust
 * checks the header's id matches.
 * @param {string} vaultId
 * @returns {Promise<boolean>} False when the user cancels.
 */
export function vaultRelocate(vaultId) {
    return invoke("vault_relocate", { vaultId });
}

/**
 * Gives a pending imported copy its own identity (Phase 6):
 * opens it with the passphrase, forks it in the core and
 * registers it next to the original.
 * @param {string} passphrase
 * @returns {Promise<{vault_id: string, recovery_key: string}>}
 *   The new recovery key is shown once.
 */
export function vaultForkImport(passphrase) {
    return invoke('vault_fork_import', { passphrase });
}

/**
 * Changes the open vault's passphrase; the core verifies the
 * current one. The session stays unlocked.
 * @param {string} currentPassphrase
 * @param {string} newPassphrase
 * @returns {Promise<void>}
 */
export function vaultChangePassphrase(currentPassphrase, newPassphrase) {
    return invoke('vault_change_passphrase', { currentPassphrase, newPassphrase });
}

/**
 * Replaces the open vault's recovery key; the current
 * passphrase is the re-auth proof.
 * @param {string} currentPassphrase
 * @returns {Promise<string>} The new recovery key — shown once.
 */
export function vaultRegenerateRecovery(currentPassphrase) {
    return invoke('vault_regenerate_recovery', { currentPassphrase });
}

/**
 * Changes the open vault's master key by fresh-vault
 * migration; the passphrase stays the same. Ends locked
 * (silently) so the caller can show the key first.
 * @param {string} currentPassphrase
 * @returns {Promise<string | null>} The NEW recovery key,
 *   shown once — or null when the user cancels the native
 *   confirmation.
 */
/**
 * §8 exports. Both resolve with { entries, attachments,
 * target } (target is a display name, never a path), or
 * null when the user backs out of the warning or the
 * folder pick. The passphrase re-authenticates in Rust.
 */
export function exportMarkdown(passphrase) {
    return invoke('export_markdown', { passphrase });
}

export function exportJson(passphrase) {
    return invoke('export_json', { passphrase });
}

/**
 * §8 imports. Both resolve with { entries, attachments,
 * skippedEntries, skippedAttachments, exportOnlyAttachments }, or null when the
 * user cancels the pick.
 */
export function importMarkdown() {
    return invoke('import_markdown');
}

export function importJson() {
    return invoke('import_json');
}

export function vaultRotateMasterKey(currentPassphrase) {
    return invoke('vault_rotate_master_key', { currentPassphrase });
}

// ---------------------------------------------------------------------------
// Entries, search, tags (all reject with "locked" unless Unlocked)

/**
 * @param {{ cursor?: string | null, tag?: string | null, date?: string | null, limit?: number, scope?: Scope | null }} [args]
 * @returns {Promise<{ entries: EntrySummary[], next_cursor: string | null }>}
 */
export function entryList(args = {}) {
    return invoke('entry_list', {
        cursor: args.cursor ?? null,
        tag: args.tag ?? null,
        date: args.date ?? null,
        limit: args.limit ?? null,
        scope: scopeToWire(args.scope),
    });
}

/**
 * Live-entry counts per day in [from, to), for the calendar.
 * @param {string} from - First day, YYYY-MM-DD (included).
 * @param {string} to - Last day, YYYY-MM-DD (excluded).
 * @param {Scope | null} [scope]
 * @returns {Promise<Array<{date: string, entries: number}>>}
 */
export function calendarDays(from, to, scope = null) {
    return invoke('calendar_days', { from, to, scope: scopeToWire(scope) });
}

/**
 * Entries from this month-and-day in earlier years.
 * @param {string} date Today, YYYY-MM-DD.
 * @param {Scope | null} [scope]
 * @returns {Promise<EntrySummary[]>}
 */
export function onThisDay(date, scope = null) {
    return invoke('on_this_day', { date, scope: scopeToWire(scope) });
}

/**
 * @param {string} id
 * @returns {Promise<Entry>}
 */
export function entryGet(id) {
    return invoke("entry_get", { id });
}

/**
 * Files a new entry into a folder (null is the vault root). The
 * assignment is permanent: no update path can change it.
 * @param {string | null} folderId
 * @param {DraftBody} draft
 * @returns {Promise<Entry>}
 */
export function entryCreate(folderId, draft) {
    return invoke("entry_create", { folderId, draft });
}

/**
 * Replaces an entry's date, title, text, mood and tags. A stale
 * expectedRev fails with "revision_conflict" instead of
 * overwriting: the caller opened an older revision. Folder
 * membership is never touched.
 * @param {string} id
 * @param {number} expectedRev The rev the caller opened.
 * @param {DraftBody} draft
 * @returns {Promise<Entry>}
 */
export function entryUpdate(id, expectedRev, draft) {
    return invoke("entry_update", { id, expectedRev, draft });
}

/** @param {string} id */
export function entryDelete(id) {
    return invoke("entry_delete", { id });
}

/**
 * @param {string} query Words under 3 characters are dropped; if none
 *   remain, rejects with "query_too_short" (§4).
 * @param {{ limit?: number, scope?: Scope | null }} [args]
 * @returns {Promise<SearchHit[]>}
 */
export function search(query, args = {}) {
    return invoke("search", {
        query,
        limit: args.limit ?? null,
        scope: scopeToWire(args.scope),
    });
}

/**
 * Vault-wide title suggestions for the link picker: prefix
 * matches first, then FTS title matches, deduplicated. Empty
 * queries return recent entries; 1–2 character queries are
 * prefix-only.
 * @param {string} query
 * @param {{ cursor?: string | null, limit?: number }} [args]
 * @returns {Promise<CandidatePage>}
 */
export function entryLinkCandidates(query, args = {}) {
    return invoke("entry_link_candidates", {
        query,
        cursor: args.cursor ?? null,
        limit: args.limit ?? null,
    });
}

/**
 * Body-only UUID candidates for one link target: small bodies
 * inline, metadata (size, revision) plus a null body for oversized
 * ones, folder ids for the verifier. The target itself is
 * excluded; deleted sources never appear.
 * @param {string} id The link target.
 * @param {{ cursor?: string | null, limit?: number }} [args]
 * @returns {Promise<BacklinkPage>}
 */
export function backlinkCandidates(id, args = {}) {
    return invoke("backlink_candidates", {
        id,
        cursor: args.cursor ?? null,
        limit: args.limit ?? null,
    });
}

/**
 * @param {Scope | null} [scope]
 * @returns {Promise<{ name: string, entries: number }[]>}
 */
export function tagList(scope = null) {
    return invoke("tag_list", { scope: scopeToWire(scope) });
}

/**
 * One live folder per row, for the tree. `recovered` rows show at
 * the root; `entry_count` is direct entries only.
 * @returns {Promise<Folder[]>}
 */
export function folderList() {
    return invoke("folder_list");
}

/**
 * Creates a folder under parentId (null is the vault root).
 * Rejects with "folder_exists" on a duplicate sibling name.
 * @param {string | null} parentId
 * @param {string} name
 * @returns {Promise<Folder>}
 */
export function folderCreate(parentId, name) {
    return invoke("folder_create", { parentId, name });
}

/**
 * Renames and/or moves a folder; pass the current values for the
 * fields that stay. A stale expectedRev fails with
 * "revision_conflict" and writes nothing.
 * @param {string} id
 * @param {number} expectedRev
 * @param {string | null} parentId
 * @param {string} name
 * @returns {Promise<Folder>}
 */
export function folderUpdate(id, expectedRev, parentId, name) {
    return invoke("folder_update", { id, expectedRev, parentId, name });
}

/**
 * Deletes an empty folder at the caller's revision. Folders with
 * live subfolders or entries fail with "folder_not_empty".
 * @param {string} id
 * @param {number} expectedRev
 * @returns {Promise<void>}
 */
export function folderDelete(id, expectedRev) {
    return invoke("folder_delete", { id, expectedRev });
}

/**
 * Revives a tombstoned folder with its stored placement.
 * @param {string} id
 * @param {number} expectedRev
 * @returns {Promise<Folder>}
 */
export function folderRestore(id, expectedRev) {
    return invoke("folder_restore", { id, expectedRev });
}

/**
 * @returns {Promise<number>} Entries carrying the tag afterwards.
 *   Rejects with "tag_exists" when the target name is taken — the
 *   UI then offers tagMerge instead (§6.1).
 */
export function tagRename(from, to) {
    return invoke("tag_rename", { from, to });
}

/** @returns {Promise<number>} Entries the tag was removed from. */
export function tagDelete(name) {
    return invoke("tag_delete", { name });
}

/** @returns {Promise<number>} */
export function tagMerge(from, into) {
    return invoke("tag_merge", { from, into });
}

// ---------------------------------------------------------------------------
// Misc

/**
 * Opens an https or mailto link in the default browser, after a
 * native confirmation shown by Rust.
 * @param {string} url
 * @returns {Promise<boolean>} False when the user cancels.
 */
export function openLink(url) {
    return invoke("open_link", { url });
}

/** @returns {Promise<Settings>} */
export function settingsGet() {
    return invoke("settings_get");
}

/**
 * @typedef {object} AttachmentAdded
 * @property {string} id
 * @property {string} mime Sniffed in Rust, never from the file name.
 * @property {number} size
 * @property {number} rev The entry's post-commit revision — the
 *   editor adopts it so the next save builds on the fresh base.
 */

/**
 * @typedef {object} AttachmentInfo
 * @property {boolean} previewable
 * @property {string} id
 * @property {string} name
 * @property {string} mime
 * @property {number} size
 */

/**
 * The URL the webview loads an attachment from (§6.4). The id is
 * the only thing in it — never a path.
 * @param {string} id
 */
export function assetUrl(id) {
    // §12: on Windows the scheme is served as an
    // http://<scheme>.localhost origin; everywhere else
    // the scheme form. (Same rule as the editor's copy.)
    return navigator.userAgent.includes("Windows")
        ? `http://soirvu-asset.localhost/${id}`
        : `soirvu-asset://localhost/${id}`;
}

/**
 * Starts a chunked upload (§6.1).
 * @param {string} entryId
 * @param {string} name Original file name.
 * @param {string | null} mimeHint
 * @returns {Promise<string>} The upload id.
 */
export function attachmentBegin(entryId, name, mimeHint) {
    return invoke("attachment_begin", { entryId, name, mimeHint });
}

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** Base64 for one chunk (standard alphabet, padded). */
function encodeBase64(bytes) {
    let out = "";
    for (let i = 0; i < bytes.length; i += 3) {
        const b0 = bytes[i];
        const b1 = i + 1 < bytes.length ? bytes[i + 1] : 0;
        const b2 = i + 2 < bytes.length ? bytes[i + 2] : 0;
        const n = (b0 << 16) | (b1 << 8) | b2;
        out += B64[(n >> 18) & 63];
        out += B64[(n >> 12) & 63];
        out += i + 1 < bytes.length ? B64[(n >> 6) & 63] : "=";
        out += i + 2 < bytes.length ? B64[n & 63] : "=";
    }
    return out;
}

/**
 * Sends one slice of an upload. Slices travel as base64: the
 * isolation pattern carries JSON, not binary bodies.
 * @param {string} uploadId
 * @param {Uint8Array} bytes
 */
export function attachmentChunk(uploadId, bytes) {
    return invoke("attachment_chunk", { uploadId, dataBase64: encodeBase64(bytes) });
}

/**
 * Sniffs, caps, strips (per settings) and commits an upload.
 * Refused with "revision_conflict" when the entry moved under the
 * caller: committing then would strand the attachment on unseen
 * content, so the editor adopts nothing and surfaces the conflict.
 * @param {string} uploadId
 * @param {number} expectedRev The entry rev the editor opened.
 * @returns {Promise<AttachmentAdded>}
 */
export function attachmentFinish(uploadId, expectedRev) {
    return invoke("attachment_finish", { uploadId, expectedRev });
}

/** @param {string} uploadId */
export function attachmentAbort(uploadId) {
    return invoke("attachment_abort", { uploadId });
}

/**
 * @param {string} id
 * @returns {Promise<AttachmentInfo>}
 */
export function attachmentInfo(id) {
    return invoke("attachment_info", { id });
}

/**
 * Rust-side picker + streaming import for very large files (§6.1).
 * Resolves to null when the user cancels the picker.
 * @param {string} entryId
 * @param {number} expectedRev The entry rev the caller opened.
 * @returns {Promise<AttachmentAdded | null>}
 */
export function attachmentImport(entryId, expectedRev) {
    return invoke("attachment_import", { entryId, expectedRev });
}

/**
 * Copies the open vault to the backup location now (§13
 * Phase 6). Resolves with the new folder's display name.
 * @returns {Promise<string>}
 */
export function backupNow() {
    return invoke('backup_now', {});
}

/**
 * Reports a copy made in the diary (§9 clipboard clearing).
 * Rust checks the setting and schedules the clear.
 * @returns {Promise<void>}
 */
export function clipboardNoteCopy() {
    return invoke('clipboard_note_copy', {});
}

/**
 * Opens the attachment in the OS default app. Rust writes a
 * decrypted copy into the app-managed opened folder (wiped when
 * the diary locks) and hands it to the OS; no path crosses IPC.
 * @param {string} id
 * @returns {Promise<boolean>}
 */
export function attachmentOpen(id) {
    return invoke('attachment_open', { id });
}

/** @param {Partial<Pick<Settings, "lock_timeout_secs" | "strip_exif" | "clipboard_clear" | "backup_schedule" | "caps">>} settings */
export function settingsSet(settings) {
    return invoke("settings_set", { settings });
}

/**
 * Opens the native folder picker for the backup location; the
 * path stays in Rust. Resolves to the updated Settings, or null
 * when the user cancels the picker.
 * @returns {Promise<Settings | null>}
 */
export function settingsPickBackupDir() {
    return invoke("settings_pick_backup_dir");
}

/** @returns {Promise<Settings>} */
export function settingsClearBackupDir() {
    return invoke("settings_clear_backup_dir");
}

// ---------------------------------------------------------------------------
// Sync (Phase 10): bundle files in a folder another tool
// carries between devices. The folder path stays in Rust.

/**
 * @typedef {object} SyncPeer
 * @property {string} device_id
 * @property {number} applied_lseq How far this device has
 *   incorporated that peer's stream (0 when it never has).
 * @property {number} waiting That peer's bundles in the
 *   folder past the cursor.
 * @property {number | null} emitted_lseq How far that
 *   peer's stream is evidenced to have been emitted;
 *   null with no evidence.
 * @property {boolean} needs_reemit This device is
 *   provably behind that peer with no covering bundle
 *   in the folder: the peer needs to re-emit.
 */

/**
 * @typedef {object} SyncOverview
 * @property {string | null} folder_name Display name of the
 *   sync folder (never a path); null when not set.
 * @property {string} device_id This device's identity in
 *   the sync folder.
 * @property {number} emitted_lseq
 * @property {number} bundles_in_folder
 * @property {SyncPeer[]} peers
 * @property {boolean} ack_ahead_seen An ack ahead of
 *   this vault was detected and not yet dismissed.
 */

/**
 * @typedef {object} SyncBundle
 * @property {string} file_name
 * @property {number} from_lseq
 * @property {number} to_lseq
 * @property {number} entries
 * @property {number} attachments
 */

/**
 * @typedef {object} SyncReport
 * @property {number} bundles_seen
 * @property {number} bundles_applied
 * @property {number} entries_applied
 * @property {number} entries_conflicted
 * @property {number} attachments_applied
 * @property {string[]} problems
 * @property {string[]} warnings Notes that are not
 *   failures (the ack could not be published, say); the
 *   sync itself still succeeded.
 * @property {string | null} ack_ahead Set when the
 *   folder's ack claimed more than this vault holds: a
 *   shared identity or a restore. A failure, not a
 *   warning.
 */

/**
 * @typedef {object} SyncPrune
 * @property {number} bundles_removed
 * @property {number} blobs_removed
 * @property {number} covered_through
 */

/** @returns {Promise<SyncOverview>} */
export function syncOverview() {
    return invoke('sync_overview');
}

/**
 * Opens the native folder picker for the open diary's sync
 * folder; the path stays in Rust. Resolves to the updated
 * Settings, or null when the user cancels the picker.
 * @returns {Promise<Settings | null>}
 */
export function syncPickFolder() {
    return invoke('sync_pick_folder');
}

/** @returns {Promise<Settings>} */
export function syncClearFolder() {
    return invoke('sync_clear_folder');
}

/**
 * Writes this device's new changes as one bundle into the
 * sync folder. Resolves null when nothing changed. Rejects
 * with "sync_not_configured" when no folder is set.
 * @returns {Promise<SyncBundle | null>}
 */
export function syncEmit() {
    return invoke('sync_emit');
}

/** @returns {Promise<SyncReport>} */
export function syncApply() {
    return invoke('sync_apply');
}

/** @returns {Promise<SyncPrune>} */
export function syncPrune() {
    return invoke('sync_prune');
}

/**
 * Gives THIS copy of the diary a new device identity — the
 * remedy when a vault folder was copied from another
 * device and both copies share one identity.
 * @returns {Promise<string>} The new device id.
 */
export function restampDevice() {
    return invoke('restamp_device');
}

/**
 * Clears the durable ahead-of-vault flag (see
 * syncOverview's ack_ahead_seen) without restamping.
 * @returns {Promise<SyncOverview>}
 */
export function syncDismissAckAhead() {
    return invoke('sync_dismiss_ack_ahead');
}

/**
 * Re-emits this vault's whole sync state as one
 * full-range bundle, under the same identity: the
 * recovery when a peer lost bundles to a prune.
 * @returns {Promise<SyncBundle | null>} Null when there
 *   is nothing to emit.
 */
export function syncReemit() {
    return invoke('sync_reemit');
}

/**
 * Fires whenever the lock status changes (unlock, manual lock,
 * idle auto-lock), so the UI never polls.
 * @param {(payload: { status: string, vault_id: string | null }) => void} cb
 * @returns {Promise<() => void>} The unlisten function.
 */
export function onVaultStatus(cb) {
    return listen("vault-status", (event) => cb(event.payload));
}

/**
 * Fires when Rust asks the UI to flush before it drops the
 * keys (§6.2). The handler must flush, then answer with
 * vaultLockReady.
 * @param {(payload: { generation: number }) => void} cb
 * @returns {Promise<() => void>} The unlisten function.
 */
export function onLockRequested(cb) {
    return listen("lock-requested", (event) => cb(event.payload));
}

/**
 * Fires when a master-key rotation begins, so the settings view
 * can show progress instead of a hung button.
 * @param {(payload: unknown) => void} cb
 * @returns {Promise<() => void>} The unlisten function.
 */
export function onRotationStarted(cb) {
    return listen("rotation-started", (event) => cb(event.payload));
}
