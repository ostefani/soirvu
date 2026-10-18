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
use super::sync::{SyncBundleDto, SyncOverviewDto, SyncPeerDto, SyncPruneDto, SyncReportDto};
use super::entries::{CalendarDay, EntryDto, EntryPage, EntrySummary, HitDto, SaveDraft, TagDto};
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
            mood: Some(4),
            tz: Some("Europe/Berlin".into()),
            tags: vec!["golden".into(), "Работа".into()],
            created_at: 1_767_225_600_000,
            updated_at: 1_767_229_200_000,
            rev: 3,
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
            mood: None,
            tz: None,
            tags: vec![],
            created_at: 1_767_139_200_000,
            updated_at: 1_767_139_200_000,
            rev: 1,
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
            },
            HitDto {
                id: "entry-2".into(),
                entry_date: "2026-01-01".into(),
                title: None,
                snippet: "plain".into(),
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
        },
    );
    snap(
        "attachment_info",
        &AttachmentInfo {
            id: "0190a1b2-7c3d-7abc-8def-0123456789ab".into(),
            name: "photo.png".into(),
            mime: "image/png".into(),
            size: 12345,
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
            entries_applied: 5,
            entries_conflicted: 1,
            attachments_applied: 1,
            problems: vec![
                "attachment 0190a1b2: its blob is missing from the sync folder".into(),
            ],
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
fn save_draft_input() {
    // The exact JSON the UI sends to entry_save (§6.1). Parsing it
    // into SaveDraft must keep working field for field.
    let sent = serde_json::json!({
        "id": "entry-1",
        "entry_date": "2026-01-02",
        "title": "Morning pages",
        "body_md": "# Morning",
        "mood": 4,
        "tz": "Europe/Berlin",
        "tags": ["golden", "Работа"]
    });
    let draft: SaveDraft =
        serde_json::from_value(sent.clone()).expect("the UI's draft JSON parses");
    assert_eq!(draft.entry_date, "2026-01-02");
    assert_eq!(draft.tags.len(), 2);
    insta::assert_snapshot!(
        "save_draft_input",
        serde_json::to_string_pretty(&sent).expect("value serializes")
    );

    // A create sends id: null; that must parse too.
    let create = serde_json::json!({
        "id": null,
        "entry_date": "2026-01-02",
        "title": null,
        "body_md": "",
        "mood": null,
        "tz": "America/Los_Angeles",
        "tags": []
    });
    let draft: SaveDraft = serde_json::from_value(create).expect("the UI's create JSON parses");
    assert!(draft.id.is_none());
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
