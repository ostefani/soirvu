// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Contract snapshots for the IPC boundary (§13, Phase 2).
//!
//! These tests pin the exact JSON the webview sees: field names,
//! nullability, number types, and the stable error codes. A rename
//! like the SearchHit entry_id -> id drift now fails a test here
//! instead of breaking the UI silently. When a change is
//! deliberate, update the snapshots with `INSTA_UPDATE=always
//! cargo test` (or `cargo insta review`) and commit the diff —
//! the snapshot diff IS the contract change, review it like one.

use serde::Serialize;

use super::attachments::{AttachmentAdded, AttachmentInfo};
use super::entries::{
    BacklinkDto, BacklinkPageDto, CalendarDay, CandidatePageDto, DraftBody, EntryDto, EntryPage,
    EntrySummary, HitDto, ScopeInput, TagDto, TitleHitDto,
};
use super::folders::FolderDto;
use super::sync::{SyncBundleDto, SyncOverviewDto, SyncPeerDto, SyncPruneDto, SyncReportDto};
use super::vault::{Created, Forked, Picked, TouchIdStatus};
use crate::error::AppError;
use crate::state::{Settings, Status, VaultInfo};

fn snap<T: Serialize>(name: &str, value: &T) {
    let json = serde_json::to_string_pretty(value).expect("DTO serializes");
    insta::assert_snapshot!(name, json);
}

fn summary(id: &str, title: Option<&str>) -> EntrySummary {
    EntrySummary {
        id: id.into(),
        entry_date: "2026-01-02".into(),
        title: title.map(str::to_string),
        excerpt: "Hello, test…".into(),
        tags: vec!["golden".into()],
        updated_at: 1_767_229_200_000,
        folder_id: Some("folder-1".into()),
    }
}

#[test]
fn entry_dto() {
    snap(
        "entry_dto",
        &EntryDto {
            id: "entry-1".into(),
            entry_date: "2026-01-02".into(),
            title: Some("Morning pages".into()),
            body_md: "# Morning\n\nText with **bold**.".into(),
            body_hash: "68309e07d69cefbccd3d433bf64ad2e1cf250ed1e88e66c47a9cecff5b476857".into(),
            mood: Some(4),
            tz: Some("Europe/Berlin".into()),
            tags: vec!["golden".into(), "Работа".into()],
            created_at: 1_767_225_600_000,
            updated_at: 1_767_229_200_000,
            rev: 3,
            folder_id: Some("folder-1".into()),
        },
    );
}

#[test]
fn entry_dto_minimal() {
    snap(
        "entry_dto_minimal",
        &EntryDto {
            id: "entry-2".into(),
            entry_date: "2026-01-01".into(),
            title: None,
            body_md: String::new(),
            body_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            mood: None,
            tz: None,
            tags: vec![],
            created_at: 1_767_139_200_000,
            updated_at: 1_767_139_200_000,
            rev: 1,
            folder_id: None,
        },
    );
}

#[test]
fn entry_page() {
    snap(
        "entry_page",
        &EntryPage {
            entries: vec![summary("entry-1", Some("Test")), summary("entry-2", None)],
            next_cursor: Some("2026-01-02|1767225600000|entry-1".into()),
        },
    );
    snap(
        "entry_page_last",
        &EntryPage {
            entries: vec![],
            next_cursor: None,
        },
    );
}

#[test]
fn search_hits() {
    snap(
        "search_hits",
        &vec![
            HitDto {
                id: "entry-1".into(),
                entry_date: "2026-01-02".into(),
                title: Some("Test".into()),
                snippet: "…\u{2}test\u{3}…".into(),
                folder_id: Some("folder-1".into()),
            },
            HitDto {
                id: "entry-2".into(),
                entry_date: "2026-01-01".into(),
                title: None,
                snippet: "plain".into(),
                folder_id: None,
            },
        ],
    );
}

#[test]
fn tag_list() {
    snap(
        "tag_list",
        &vec![
            TagDto {
                name: "Работа".into(),
                entries: 2,
            },
            TagDto {
                name: "golden".into(),
                entries: 1,
            },
        ],
    );
}

#[test]
fn vault_info_states() {
    let info = |state: &str| VaultInfo {
        id: "0123456789abcdef0123456789abcdef".into(),
        name: "test_diar".into(),
        state: state.into(),
    };
    snap(
        "vault_info_states",
        &vec![info("available"), info("missing"), info("unlocked")],
    );
}

#[test]
fn vault_status_event_values() {
    // state.status() travels inside the vault-status event payload.
    snap(
        "vault_status_event_values",
        &vec![
            Status::Unavailable,
            Status::Locked,
            Status::Unlocking,
            Status::Unlocked,
            Status::Locking,
        ],
    );
}

#[test]
fn forked_shape() {
    snap(
        "forked",
        &Forked {
            vault_id: "a54c38f6a54c38f6a54c38f6a54c38f6".into(),
            recovery_key: "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ-2345-567".into(),
        },
    );
}

#[test]
fn touch_id_status_shape() {
    snap(
        "touch_id_status",
        &TouchIdStatus {
            built_in: true,
            capable: true,
            enrolled: false,
        },
    );
}

#[test]
fn export_outcome_shape() {
    snap(
        "export_outcome",
        &crate::commands::misc::ExportOutcome {
            entries: 12,
            attachments: 3,
            target: "Exports/soirvu-export.json".into(),
        },
    );
}

#[test]
fn import_outcome_shape() {
    snap(
        "import_outcome",
        &crate::commands::misc::ImportOutcome {
            entries: 12,
            attachments: 3,
            skipped_entries: 1,
            skipped_attachments: 2,
            export_only_attachments: 1,
            folders: 2,
            skipped_folders: 0,
        },
    );
}

#[test]
fn calendar_day_shape() {
    snap(
        "calendar_day",
        &CalendarDay {
            date: "2026-10-15".into(),
            entries: 2,
        },
    );
}

#[test]
fn attachment_shapes() {
    snap(
        "attachment_added",
        &AttachmentAdded {
            id: "0190a1b2-7c3d-7abc-8def-0123456789ab".into(),
            mime: "image/png".into(),
            size: 12345,
            rev: 7,
        },
    );
    snap(
        "attachment_info",
        &AttachmentInfo {
            id: "0190a1b2-7c3d-7abc-8def-0123456789ab".into(),
            name: "photo.png".into(),
            mime: "image/png".into(),
            size: 12345,
            previewable: true,
        },
    );
}

#[test]
fn picked_and_created() {
    snap(
        "picked_open",
        &Picked {
            vault_id: Some("0123456789abcdef0123456789abcdef".into()),
            name: "test_diar".into(),
            warning: None,
            fork_required: false,
        },
    );
    snap(
        "picked_create",
        &Picked {
            vault_id: None,
            name: "new_diary".into(),
            warning: None,
            fork_required: false,
        },
    );
    snap(
        "created",
        &Created {
            vault_id: "0123456789abcdef0123456789abcdef".into(),
            recovery_key: "sample recovery key words".into(),
        },
    );
}

#[test]
fn sync_shapes() {
    snap(
        "sync_overview",
        &SyncOverviewDto {
            folder_name: Some("DiarySync".into()),
            device_id: "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa".into(),
            emitted_lseq: 42,
            bundles_in_folder: 3,
            peers: vec![SyncPeerDto {
                device_id: "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb".into(),
                applied_lseq: 40,
                waiting: 1,
                emitted_lseq: Some(57),
                needs_reemit: true,
            }],
            ack_ahead_seen: false,
        },
    );
    snap(
        "sync_bundle",
        &SyncBundleDto {
            file_name: "01234567-aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa-41-42.sync".into(),
            from_lseq: 41,
            to_lseq: 42,
            entries: 2,
            attachments: 1,
        },
    );
    snap(
        "sync_report",
        &SyncReportDto {
            bundles_seen: 2,
            bundles_applied: 2,
            folders_applied: 1,
            folders_conflicted: 0,
            entries_applied: 5,
            entries_conflicted: 1,
            attachments_applied: 1,
            problems: vec!["attachment 0190a1b2: its blob is missing from the sync folder".into()],
            warnings: vec![],
            ack_ahead: None,
        },
    );
    snap(
        "sync_prune",
        &SyncPruneDto {
            bundles_removed: 1,
            blobs_removed: 0,
            covered_through: 40,
        },
    );
}

#[test]
fn settings_default() {
    snap("settings_default", &Settings::default());
}

#[test]
fn write_input_shapes() {
    // The exact JSON the UI sends to entry_create (§6.1): a folder
    // plus a body with no id and no folder. Parsing it into
    // DraftBody must keep working field for field, and the body
    // must not smuggle membership in.
    let sent = serde_json::json!({
        "entry_date": "2026-01-02",
        "title": "Morning pages",
        "body_md": "# Morning",
        "mood": 4,
        "tz": "Europe/Berlin",
        "tags": ["golden", "Работа"]
    });
    let draft: DraftBody =
        serde_json::from_value(sent.clone()).expect("the UI's draft JSON parses");
    assert_eq!(draft.entry_date, "2026-01-02");
    assert_eq!(draft.tags.len(), 2);
    insta::assert_snapshot!(
        "write_draft_input",
        serde_json::to_string_pretty(&sent).expect("value serializes")
    );

    // Unknown fields (a stale id, a folder) are refused: updates
    // carry id and expected_rev as command arguments, never in
    // the body, so a missing field cannot reset the entry to root.
    assert!(
        serde_json::from_value::<DraftBody>(serde_json::json!({
            "id": "entry-1",
            "entry_date": "2026-01-02",
            "title": null,
            "body_md": "",
            "mood": null,
            "tz": null,
            "tags": []
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<DraftBody>(serde_json::json!({
            "folder_id": "folder-1",
            "entry_date": "2026-01-02",
            "title": null,
            "body_md": "",
            "mood": null,
            "tz": null,
            "tags": []
        }))
        .is_err()
    );
}

#[test]
fn scope_input_shapes() {
    // The wire shape for every scoped read: the whole vault, only
    // unassigned entries, or one folder with a subtree flag.
    // Deserialize-only: the UI sends these, never receives them.
    let all: ScopeInput =
        serde_json::from_value(serde_json::json!({"kind": "all"})).expect("all parses");
    assert!(matches!(all, ScopeInput::All));
    let root: ScopeInput =
        serde_json::from_value(serde_json::json!({"kind": "root"})).expect("root parses");
    assert!(matches!(root, ScopeInput::Root));
    let folder: ScopeInput = serde_json::from_value(serde_json::json!({
        "kind": "folder",
        "id": "folder-1",
        "include_descendants": true
    }))
    .expect("folder parses");
    assert!(matches!(
        folder,
        ScopeInput::Folder {
            include_descendants: true,
            ..
        }
    ));
    assert!(serde_json::from_value::<ScopeInput>(serde_json::json!({"kind": "vault"})).is_err());
    // Nested struct fields deserialize strictly: Tauri converts
    // only top-level command arguments to camelCase, so the UI
    // must send the snake_case flag (see scopeToWire in
    // src/lib/scope.js). A camelCase twin fails loudly here
    // instead of at runtime in front of the user.
    assert!(
        serde_json::from_value::<ScopeInput>(serde_json::json!({
            "kind": "folder",
            "id": "folder-1",
            "includeDescendants": true
        }))
        .is_err()
    );
}

#[test]
fn folder_dto() {
    snap(
        "folder_dto",
        &FolderDto {
            id: "folder-1".into(),
            parent_id: Some("folder-0".into()),
            name: "Health".into(),
            created_at: 1_767_225_600_000,
            updated_at: 1_767_229_200_000,
            rev: 4,
            entry_count: 3,
            recovered: false,
        },
    );
    snap(
        "folder_dto_recovered",
        &FolderDto {
            id: "folder-2".into(),
            parent_id: Some("folder-gone".into()),
            name: "Orphan".into(),
            created_at: 1_767_225_600_000,
            updated_at: 1_767_229_200_000,
            rev: 2,
            entry_count: 1,
            recovered: true,
        },
    );
}

#[test]
fn candidate_dtos() {
    snap(
        "candidate_page",
        &CandidatePageDto {
            candidates: vec![
                TitleHitDto {
                    id: "entry-1".into(),
                    entry_date: "2026-01-02".into(),
                    title: Some("Training day".into()),
                    folder_id: Some("folder-1".into()),
                },
                TitleHitDto {
                    id: "entry-2".into(),
                    entry_date: "2026-01-01".into(),
                    title: None,
                    folder_id: None,
                },
            ],
            next_cursor: Some("f|2026-01-01|1767225600000|entry-2".into()),
        },
    );
    snap(
        "backlink_page",
        &BacklinkPageDto {
            candidates: vec![BacklinkDto {
                id: "entry-3".into(),
                entry_date: "2026-01-03".into(),
                title: Some("Summary".into()),
                folder_id: None,
                body: Some("see [notes](entry:entry-1) for details".into()),
                body_size: 38,
                body_hash: "d9150e63f33a8b72c75cc8996a505195ce6cfee5a4c07832bd4126d130ba0201"
                    .into(),
            }],
            next_cursor: None,
        },
    );
}

#[test]
fn error_codes() {
    use soirvu_core::Error as Core;
    let code = |e: Core| AppError::from(e).to_ipc();
    let lines = [
        format!("Unlock -> {}", code(Core::Unlock)),
        format!("EmptyPassphrase -> {}", code(Core::EmptyPassphrase)),
        format!(
            "InvalidRecoveryKey -> {}",
            code(Core::InvalidRecoveryKey("x".into()))
        ),
        format!(
            "UnsupportedFormat -> {}",
            code(Core::UnsupportedFormat("x".into()))
        ),
        format!("InvalidHeader -> {}", code(Core::InvalidHeader("x".into()))),
        format!("BadKdfParams -> {}", code(Core::BadKdfParams("x".into()))),
        format!("FolderNotEmpty -> {}", code(Core::FolderNotEmpty)),
        format!("FolderNotFound -> {}", code(Core::FolderNotFound)),
        format!("FolderExists -> {}", code(Core::FolderExists)),
        format!("FolderHasContents -> {}", code(Core::FolderHasContents)),
        format!("FolderCycle -> {}", code(Core::FolderCycle)),
        format!("FolderTooDeep -> {}", code(Core::FolderTooDeep)),
        format!("RevisionConflict -> {}", code(Core::RevisionConflict)),
        format!("VaultMismatch -> {}", code(Core::VaultMismatch)),
        format!(
            "Io -> {}",
            code(Core::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "x"
            )))
        ),
        format!("EntryNotFound -> {}", code(Core::EntryNotFound)),
        format!("AttachmentNotFound -> {}", code(Core::AttachmentNotFound)),
        format!(
            "CorruptAttachment -> {}",
            code(Core::CorruptAttachment("x".into()))
        ),
        format!("InvalidInput -> {}", code(Core::InvalidInput("x".into()))),
        format!("TagNotFound -> {}", code(Core::TagNotFound)),
        format!("TagExists -> {}", code(Core::TagExists)),
        format!("QueryTooShort -> {}", code(Core::QueryTooShort)),
        format!("Random -> {}", code(Core::Random("x".into()))),
        format!("Backoff -> {}", AppError::Backoff(30).to_ipc()),
        format!("Code -> {}", AppError::Code("locked".into()).to_ipc()),
    ];
    insta::assert_snapshot!("error_codes", lines.join("\n"));
}

/// The IPC surface is declared in three places that must agree:
/// the build.rs manifest (what Rust exposes), the capability
/// allow-list (what the window may call), and the isolation
/// frame's allow-list (what the sandbox passes through). A new
/// command missing anywhere fails closed at runtime with a
/// confusing rejection, so this test fails loudly instead.
#[test]
fn ipc_surface_lists_agree() {
    use std::collections::BTreeSet;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // One `"command",` string literal per line in both files;
    // the manifest's last entry has no trailing comma.
    fn literals(text: &str) -> BTreeSet<String> {
        text.lines()
            .map(str::trim)
            .filter_map(|line| {
                line.strip_prefix('"').and_then(|rest| {
                    rest.strip_suffix("\",")
                        .or_else(|| rest.strip_suffix('"'))
                        .map(str::to_string)
                })
            })
            .collect()
    }

    let manifest_src = std::fs::read_to_string(root.join("build.rs")).unwrap();
    let manifest = literals(&manifest_src);
    assert!(!manifest.is_empty());

    let isolation_src = std::fs::read_to_string(root.join("isolation/index.html")).unwrap();
    let allowed_block = isolation_src
        .split("const ALLOWED = new Set([")
        .nth(1)
        .expect("isolation frame keeps its ALLOWED set")
        .split("]);")
        .next()
        .unwrap();
    let isolation = literals(allowed_block);

    let capabilities_src = std::fs::read_to_string(root.join("capabilities/default.json")).unwrap();
    let capabilities: serde_json::Value = serde_json::from_str(&capabilities_src).unwrap();
    let capabilities: BTreeSet<String> = capabilities["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|name| name.starts_with("allow-"))
        .map(|name| name.trim_start_matches("allow-").replace('-', "_"))
        .collect();

    assert_eq!(manifest, isolation, "build.rs manifest vs isolation frame");
    assert_eq!(manifest, capabilities, "build.rs manifest vs capabilities");
}
