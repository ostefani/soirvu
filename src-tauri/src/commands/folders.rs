// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Folder commands (§6.1). Folders are created once and filed
//! permanently: entries never move, renames and moves carry
//! `expected_rev`, and confirmed deletion removes the stored subtree.
//! Empty-only deletion remains available in core for recovery workflows. Every command runs through the state's
//! guard: locked → the `locked` code.

use serde::Serialize;
use soirvu_core::folders::Folder;

use crate::error::ipc;
use tauri::AppHandle;

/// One live folder for the tree: metadata, direct-entry count,
/// and a recovery flag. `parent_id` is the stored placement; when
/// it names a missing or tombstoned folder, `recovered` is set and
/// the UI shows the folder at the root instead of dropping it.
#[derive(Serialize)]
pub struct FolderDto {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub rev: i64,
    pub entry_count: i64,
    pub recovered: bool,
}

fn to_dto(
    vault: &soirvu_core::Vault,
    folder: &Folder,
) -> Result<FolderDto, crate::error::AppError> {
    let entry_count = vault.count_direct_entries(&folder.id)?;
    let recovered = match &folder.parent_id {
        None => false,
        Some(parent) => {
            !soirvu_core::folders::folder_is_live(vault.database().connection(), parent)?
        }
    };
    Ok(FolderDto {
        id: folder.id.clone(),
        parent_id: folder.parent_id.clone(),
        name: folder.name.clone(),
        created_at: folder.created_at,
        updated_at: folder.updated_at,
        rev: folder.rev,
        entry_count,
        recovered,
    })
}

#[tauri::command]
pub async fn folder_list(app: AppHandle) -> Result<Vec<FolderDto>, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let folders = vault.list_folders()?;
            folders
                .iter()
                .map(|folder| to_dto(vault, folder))
                .collect::<Result<Vec<_>, _>>()
        }))
    })
    .await
}

/// Creates a folder under `parent_id` (null is the vault root).
/// Duplicate sibling names fail with `folder_exists`.
#[tauri::command]
pub async fn folder_create(
    app: AppHandle,
    parent_id: Option<String>,
    name: String,
) -> Result<FolderDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let folder = vault.create_folder(parent_id.as_deref(), &name)?;
            to_dto(vault, &folder)
        }))
    })
    .await
}

/// Renames and/or moves a folder in one core transaction: the
/// rename and move are validated together and written with a
/// single revision bump, so a failed move cannot leave a rename
/// persisted. A stale `expected_rev` fails with
/// `revision_conflict`; nothing is written. Unchanged names and
/// parents are no-ops.
#[tauri::command]
pub async fn folder_update(
    app: AppHandle,
    id: String,
    expected_rev: i64,
    parent_id: Option<String>,
    name: String,
) -> Result<FolderDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let current = vault.update_folder(&id, expected_rev, parent_id.as_deref(), &name)?;
            to_dto(vault, &current)
        }))
    })
    .await
}

/// Deletes the folder, its stored descendants and their entries atomically.
/// A stale root revision fails with `revision_conflict`.
#[tauri::command]
pub async fn folder_delete(app: AppHandle, id: String, expected_rev: i64) -> Result<(), String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            vault.delete_folder_tree(&id, expected_rev)?;
            Ok(())
        }))
    })
    .await
}

/// Revives a tombstoned folder with its stored placement: entries
/// filed under it stay filed. Restoring under a deleted parent
/// fails until the parent returns first.
#[tauri::command]
pub async fn folder_restore(
    app: AppHandle,
    id: String,
    expected_rev: i64,
) -> Result<FolderDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let folder = vault.restore_folder(&id, expected_rev)?;
            to_dto(vault, &folder)
        }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use soirvu_core::Vault;
    use soirvu_core::crypto::KdfParams;
    use soirvu_core::entries::EntryDraft;

    fn test_vault() -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::TempDir::new().unwrap();
        let (vault, _) = Vault::create(
            &tmp.path().join("v"),
            "pass phrase",
            KdfParams {
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
            mood: None,
            tz: None,
            tags: vec![],
        }
    }

    /// The tree's count is direct entries only: nested folders and
    /// the root never leak into it.
    #[test]
    fn folder_dto_counts_direct_entries_only() {
        let (_tmp, mut vault) = test_vault();
        let folder = vault.create_folder(None, "Dev").unwrap();
        vault.create_entry(Some(&folder.id), &draft("One")).unwrap();
        vault.create_entry(Some(&folder.id), &draft("Two")).unwrap();
        vault.create_entry(None, &draft("Root")).unwrap();
        let dto = to_dto(&vault, &vault.get_folder(&folder.id).unwrap()).unwrap();
        assert_eq!(dto.entry_count, 2);
        assert!(!dto.recovered);
        assert_eq!(dto.parent_id, None);
    }

    /// Restoring a child before its parent is refused (parent-first
    /// rule), so the tree never has to place an orphan through the
    /// public API: `recovered` only fires on sync-ordered or
    /// foreign data. Pin the refusal code the UI depends on.
    #[test]
    fn restore_child_first_is_refused() {
        let (_tmp, mut vault) = test_vault();
        let parent = vault.create_folder(None, "A").unwrap();
        let child = vault.create_folder(Some(&parent.id), "B").unwrap();
        vault.delete_folder(&child.id, child.rev).unwrap();
        vault.delete_folder(&parent.id, parent.rev).unwrap();
        let err = vault.restore_folder(&child.id, child.rev + 1).unwrap_err();
        assert_eq!(AppError::from(err).to_ipc(), "invalid_input");
    }
}
