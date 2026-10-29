// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Sync commands (§13 Phase 10b): the open vault emits and
//! applies bundle files in its sync folder, and prunes what
//! every peer has consumed. Another tool carries the folder
//! between devices — these commands only read and write
//! files in it; nothing here uses the network.
//!
//! The folder's path lives in the registry (state.rs) and
//! never crosses IPC: the UI sees its display name through
//! Settings (`sync_location`) and the DTOs below.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use soirvu_core::sync::{SyncBundleInfo, SyncOverview, SyncPruneReport, SyncReport};

use crate::error::ipc;
use crate::state::{AppState, Settings};

/// What the UI may know about the open vault's sync state.
/// The folder-derived numbers are counted from file NAMES
/// (the bundle name is format 1: `<vault8>-<device>-<from>-
/// <to>.sync`); no bundle is opened for this.
#[derive(Debug, Clone, Serialize)]
pub struct SyncOverviewDto {
    /// Display name of the sync folder; null when unset.
    pub folder_name: Option<String>,
    /// This vault's device identity in the folder.
    pub device_id: String,
    pub emitted_lseq: i64,
    /// Bundle files for this vault currently in the folder.
    pub bundles_in_folder: usize,
    pub peers: Vec<SyncPeerDto>,
    pub ack_ahead_seen: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPeerDto {
    pub device_id: String,
    /// How far this vault has incorporated that peer's
    /// stream (0 when it never has).
    pub applied_lseq: i64,
    /// That peer's bundles in the folder past the cursor.
    /// A peer whose cursor never moves is held back by a
    /// bundle that will not apply — the apply report's
    /// problems name it.
    pub waiting: usize,
    /// How far that peer's stream is evidenced to have
    /// been emitted (bundles this vault opened, or an ack
    /// claim its detector read); null with no evidence.
    pub emitted_lseq: Option<i64>,
    /// Provably owed a re-emit: this vault's cursor is
    /// behind the peer's emitted point and no bundle in
    /// the folder covers the gap (the covering ones were
    /// pruned). Self-clearing once the gap is covered.
    pub needs_reemit: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncBundleDto {
    pub file_name: String,
    pub from_lseq: i64,
    pub to_lseq: i64,
    pub entries: usize,
    pub attachments: usize,
}

impl From<SyncBundleInfo> for SyncBundleDto {
    fn from(info: SyncBundleInfo) -> Self {
        SyncBundleDto {
            file_name: info.file_name,
            from_lseq: info.from_lseq,
            to_lseq: info.to_lseq,
            entries: info.entries,
            attachments: info.attachments,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReportDto {
    pub bundles_seen: usize,
    pub bundles_applied: usize,
    pub entries_applied: usize,
    pub entries_conflicted: usize,
    pub attachments_applied: usize,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub ack_ahead: Option<String>,
}

impl From<SyncReport> for SyncReportDto {
    fn from(report: SyncReport) -> Self {
        SyncReportDto {
            bundles_seen: report.bundles_seen,
            bundles_applied: report.bundles_applied,
            entries_applied: report.entries_applied,
            entries_conflicted: report.entries_conflicted,
            attachments_applied: report.attachments_applied,
            problems: report.problems,
            warnings: report.warnings,
            ack_ahead: report.ack_ahead,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPruneDto {
    pub bundles_removed: usize,
    pub blobs_removed: usize,
    pub covered_through: i64,
}

impl From<SyncPruneReport> for SyncPruneDto {
    fn from(report: SyncPruneReport) -> Self {
        SyncPruneDto {
            bundles_removed: report.bundles_removed,
            blobs_removed: report.blobs_removed,
            covered_through: report.covered_through,
        }
    }
}

/// The open vault's sync folder, or a code saying why
/// not: the action commands need one, the picker sets it.
/// The app always picks an existing folder, so a missing
/// one here means it vanished (an unmounted drive). Core
/// cannot tell that from a first use — its emit creates
/// a fresh folder on purpose — but the app can, so the
/// check lives here.
fn configured_dir(state: &AppState) -> Result<PathBuf, String> {
    let dir = state
        .current_sync_dir()
        .ok_or_else(|| "sync_not_configured".to_string())?;
    if !dir.is_dir() {
        return Err("sync_folder_missing".to_string());
    }
    Ok(dir)
}

/// Clears the durable ahead-of-vault flag without
/// restamping; the refreshed overview comes back.
#[tauri::command]
pub async fn sync_dismiss_ack_ahead(app: AppHandle) -> Result<SyncOverviewDto, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = state.current_sync_dir();
        ipc(state.with_vault(|vault| {
            vault.clear_ack_ahead()?;
            let overview = vault.sync_overview()?;
            let vault_id = vault.header().vault_id.clone();
            Ok(assemble_overview(&overview, dir.as_deref(), &vault_id))
        }))
    })
    .await
}

#[tauri::command]
pub async fn sync_overview(app: AppHandle) -> Result<SyncOverviewDto, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = state.current_sync_dir();
        ipc(state.with_vault(|vault| {
            let overview = vault.sync_overview()?;
            let vault_id = vault.header().vault_id.clone();
            Ok(assemble_overview(&overview, dir.as_deref(), &vault_id))
        }))
    })
    .await
}

/// Pairs the core's cursors with what the folder holds:
/// a peer with bundles present but no cursor yet appears
/// with applied 0 and everything waiting.
fn assemble_overview(
    overview: &SyncOverview,
    dir: Option<&Path>,
    vault_id: &str,
) -> SyncOverviewDto {
    let folder_name = dir.map(|d| {
        d.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Folder".into())
    });
    let mut peers: Vec<SyncPeerDto> = overview
        .peers
        .iter()
        .map(|p| SyncPeerDto {
            device_id: p.device_id.clone(),
            applied_lseq: p.applied_lseq,
            waiting: 0,
            emitted_lseq: (p.seen_emitted > 0).then_some(p.seen_emitted),
            needs_reemit: false,
        })
        .collect();
    let mut bundles_in_folder = 0;
    let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(dir) = dir {
        let prefix = &vault_id[..8.min(vault_id.len())];
        if let Ok(items) = std::fs::read_dir(dir) {
            for item in items.flatten() {
                let name = item.file_name().to_string_lossy().into_owned();
                let Some((vault, device, from, to)) = bundle_name_parts(&name) else {
                    continue;
                };
                if vault != prefix {
                    continue;
                }
                bundles_in_folder += 1;
                // Own bundles: the full id, or its first 8
                // characters in names written before the
                // identity fix.
                if overview.device_id.starts_with(&device) {
                    continue;
                }
                let applied = peers
                    .iter()
                    .find(|p| p.device_id == device)
                    .map(|p| p.applied_lseq)
                    .unwrap_or(0);
                if from <= applied + 1 && to > applied {
                    covered.insert(device.clone());
                }
                match peers.iter_mut().find(|p| p.device_id == device) {
                    Some(peer) => {
                        if to > peer.applied_lseq {
                            peer.waiting += 1;
                        }
                    }
                    None => peers.push(SyncPeerDto {
                        device_id: device,
                        applied_lseq: 0,
                        waiting: 1,
                        emitted_lseq: None,
                        needs_reemit: false,
                    }),
                }
            }
        }
    }
    for peer in peers.iter_mut() {
        peer.needs_reemit = matches!(peer.emitted_lseq, Some(e) if e > peer.applied_lseq)
            && !covered.contains(&peer.device_id);
    }
    peers.sort_by(|a, b| a.device_id.cmp(&b.device_id));
    SyncOverviewDto {
        folder_name,
        device_id: overview.device_id.clone(),
        emitted_lseq: overview.emitted_lseq,
        bundles_in_folder,
        peers,
        ack_ahead_seen: overview.ack_ahead_seen,
    }
}

/// Splits a bundle file name into (vault prefix, device,
/// from, to). The device part contains '-' itself, so the
/// range splits off from the right — the same reading the
/// core does.
fn bundle_name_parts(name: &str) -> Option<(String, String, i64, i64)> {
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

/// Picks the open vault's sync folder through the native
/// picker; the path stays in Rust. Resolves to the updated
/// Settings, or null when the user cancels.
#[tauri::command]
pub async fn sync_pick_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Settings>, String> {
    let Some(id) = state.current_id() else {
        return Err("locked".into());
    };
    let Some(path) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let settings = crate::commands::run_blocking_state(app, move |state| {
        ipc(state.set_sync_dir(&id, Some(path)))?;
        Ok(state.settings())
    })
    .await?;
    Ok(Some(settings))
}

/// Clears the open vault's sync folder. Bundles already in
/// the folder are not touched — only the setting goes.
#[tauri::command]
pub async fn sync_clear_folder(app: AppHandle) -> Result<Settings, String> {
    crate::commands::run_blocking_state(app, |state| {
        let Some(id) = state.current_id() else {
            return Err("locked".into());
        };
        ipc(state.set_sync_dir(&id, None))?;
        Ok(state.settings())
    })
    .await
}

// Emit, apply and prune read and write the whole sync folder. They run on
// the blocking pool, and the vault operation gate keeps each complete sync
// action serialized against writes, lock, and other sync commands.
#[tauri::command]
pub async fn sync_emit(app: AppHandle) -> Result<Option<SyncBundleDto>, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = configured_dir(state)?;
        ipc(state.with_vault(|vault| Ok(vault.sync_emit(&dir)?.map(SyncBundleDto::from))))
    })
    .await
}

/// Re-emits the whole sync state under the SAME
/// identity: the recovery when a peer lost bundles to
/// a prune. (Restamp is the shared-identity remedy.)
#[tauri::command]
pub async fn sync_reemit(app: AppHandle) -> Result<Option<SyncBundleDto>, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = configured_dir(state)?;
        ipc(state.with_vault(|vault| Ok(vault.sync_reemit(&dir)?.map(SyncBundleDto::from))))
    })
    .await
}

#[tauri::command]
pub async fn sync_apply(app: AppHandle) -> Result<SyncReportDto, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = configured_dir(state)?;
        ipc(state.with_vault(|vault| Ok(SyncReportDto::from(vault.sync_apply(&dir)?))))
    })
    .await
}

#[tauri::command]
pub async fn sync_prune(app: AppHandle) -> Result<SyncPruneDto, String> {
    crate::commands::run_blocking_state(app, |state| {
        let dir = configured_dir(state)?;
        ipc(state.with_vault(|vault| Ok(SyncPruneDto::from(vault.sync_prune(&dir)?))))
    })
    .await
}

/// Gives THIS copy of the vault a new device identity —
/// the remedy when a vault folder was copied from another
/// device and both copies share one identity. Returns the
/// new device id. The old identity's ack file is NOT
/// touched: while two copies share an id they also share
/// that file, and it belongs to the surviving original,
/// which keeps publishing under the old id.
#[tauri::command]
pub async fn restamp_device(app: AppHandle) -> Result<String, String> {
    crate::commands::run_blocking_state(app, |state| {
        // The new identity is persisted in the registry
        // FIRST, then the live vault switches to it: if
        // the switch were interrupted, the next open
        // assigns the persisted id and writes its device
        // row then — the two cannot drift apart.
        let new_id = soirvu_core::new_device_id();
        ipc(state.restamp_current_device(new_id.clone()))?;
        Ok(new_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use soirvu_core::sync::SyncPeerCursor;

    fn overview_with_peer(applied: i64, seen: i64) -> SyncOverview {
        SyncOverview {
            device_id: "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa".to_string(),
            emitted_lseq: 0,
            peers: vec![SyncPeerCursor {
                device_id: "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb".to_string(),
                applied_lseq: applied,
                seen_emitted: seen,
            }],
            ack_ahead_seen: false,
        }
    }

    #[test]
    fn needs_reemit_until_a_covering_bundle_is_in_the_folder() {
        let vault_id = "0123456789abcdef0123456789abcdef";
        // Evidence past the cursor, nothing in the
        // folder: the peer is owed a re-emit.
        let dto = assemble_overview(&overview_with_peer(5, 10), None, vault_id);
        assert_eq!(dto.peers[0].emitted_lseq, Some(10));
        assert!(dto.peers[0].needs_reemit);
        // A bundle covering the gap (6..10) sits in the
        // folder: the peer is only behind the courier.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path()
                .join("01234567-bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb-00000006-00000010.sync"),
            b"",
        )
        .unwrap();
        let dto = assemble_overview(&overview_with_peer(5, 10), Some(dir.path()), vault_id);
        assert!(!dto.peers[0].needs_reemit);
    }
}
