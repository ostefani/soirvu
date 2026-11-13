// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//
// Settings-screen copy for backend error codes, one table per area —
// the same code means different things for security, export, and sync,
// so the tables stay separate. Only codes with a curated message live
// here; everything else falls back to the shared unknown rule, so raw
// IPC codes never reach settings.

export const SETTINGS_SECURITY_ERRORS = {
    passphrase_too_short: "Use at least 8 characters for the new passphrase.",
    passphrase_too_long: "The passphrase exceeds the 64 KiB limit.",
    wrong_passphrase: "Wrong current passphrase.",
};

export const SETTINGS_EXPORT_ERRORS = {
    passphrase_too_long: "The passphrase exceeds the 64 KiB limit.",
    wrong_passphrase: "Wrong passphrase.",
    folder_not_empty: "That folder is not empty — pick an empty folder for the export.",
    export_target_exists:
        "That folder already contains soirvu-export.json — pick another folder.",
    locked: "The diary is locked.",
};

export const SETTINGS_SYNC_ERRORS = {
    sync_shared_identity:
        "Another copy of this diary is using this device identity — restamp this copy below to fix it.",
    sync_unreadable:
        "A file in the sync folder sits at a bundle name and cannot be read — remove it there and try again.",
    sync_folder_missing: "The sync folder is not there — is the drive mounted?",
    sync_not_configured: "Choose a sync folder first.",
    locked: "The diary is locked.",
    sync: "Sync failed.",
};

export const SETTINGS_BACKUP_ERRORS = {
    backup_unavailable: "The backup folder is not available.",
    vault_mismatch:
        "The open vault no longer matches this folder. Lock and re-open it, then try again.",
};

export const SETTINGS_GENERAL_ERRORS = {
    locked: "The diary is locked.",
};
