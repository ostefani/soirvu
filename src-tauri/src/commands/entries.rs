// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! Entry, search and tag commands (§6.1). Every one of them runs
//! through the state's guard: locked → the `locked` code.

use serde::{Deserialize, Serialize};
use soirvu_core::entries::{
    BacklinkPage, CandidateCursor, CandidatePage, Cursor, Entry, EntryDraft, EntryScope,
    EntrySummary as CoreSummary, ListQuery, SearchHit, TagCount, TitleHit,
};

use crate::error::{AppError, ipc};
use soirvu_core::entries::body_hash;
use tauri::AppHandle;

#[derive(Serialize)]
pub struct EntryDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    pub body_md: String,
    /// Hex SHA-256 over the body bytes, from the same row read as
    /// `body_md`: a fetched body and its hash always describe the
    /// same content, so caches can key on it.
    pub body_hash: String,
    pub mood: Option<i64>,
    pub tz: Option<String>,
    pub tags: Vec<String>,
    /// Unix milliseconds; the UI formats them in the entry's time zone.
    pub created_at: i64,
    pub updated_at: i64,
    pub rev: i64,
    /// The folder holding this entry; null is the vault root.
    pub folder_id: Option<String>,
}

impl From<&Entry> for EntryDto {
    fn from(e: &Entry) -> Self {
        EntryDto {
            id: e.id.clone(),
            entry_date: e.entry_date.clone(),
            title: e.title.clone(),
            body_md: e.body_md.clone(),
            body_hash: body_hash(&e.body_md),
            mood: e.mood,
            tz: e.tz.clone(),
            tags: e.tags.clone(),
            created_at: e.created_at,
            updated_at: e.updated_at,
            rev: e.rev,
            folder_id: e.folder_id.clone(),
        }
    }
}

/// The list shows a summary; full bodies come from entry_get.
#[derive(Serialize)]
pub struct EntrySummary {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    pub excerpt: String,
    pub tags: Vec<String>,
    pub updated_at: i64,
    pub folder_id: Option<String>,
}

impl From<&CoreSummary> for EntrySummary {
    fn from(e: &CoreSummary) -> Self {
        EntrySummary {
            id: e.id.clone(),
            entry_date: e.entry_date.clone(),
            title: e.title.clone(),
            excerpt: e.preview.clone(),
            tags: e.tags.clone(),
            updated_at: e.updated_at,
            folder_id: e.folder_id.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct EntryPage {
    pub entries: Vec<EntrySummary>,
    /// Pass back as `cursor` for the next page; null at the end.
    pub next_cursor: Option<String>,
}

/// The core's cursor is a (entry_date, created_at, id) triple; across
/// the IPC boundary it travels as one opaque string.
fn encode_cursor(cursor: &Cursor) -> String {
    format!("{}|{}|{}", cursor.entry_date, cursor.created_at, cursor.id)
}

fn decode_cursor(text: &str) -> Result<Cursor, AppError> {
    let invalid = || AppError::Code("invalid_input".into());
    if text.len() > 80 {
        return Err(invalid());
    }
    let mut parts = text.splitn(3, '|');
    let entry_date = parts.next().filter(|s| !s.is_empty()).ok_or_else(invalid)?;
    let created_at: i64 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    let id = parts.next().filter(|s| !s.is_empty()).ok_or_else(invalid)?;
    Ok(Cursor {
        entry_date: entry_date.to_string(),
        created_at,
        id: id.to_string(),
    })
}

/// The entry fields both write commands take. No id, no folder:
/// creation files into a folder argument, updates never touch
/// membership (there is deliberately no folder field to reassign
/// with, so a missing field cannot reset the entry to root).
/// Unknown fields are refused: a stale client cannot smuggle an
/// id or a folder through a shape the backend would ignore.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftBody {
    pub entry_date: String,
    pub title: Option<String>,
    pub body_md: String,
    pub mood: Option<i64>,
    pub tz: Option<String>,
    pub tags: Vec<String>,
}

impl DraftBody {
    fn into_draft(self) -> EntryDraft {
        EntryDraft {
            entry_date: self.entry_date,
            title: self.title,
            body_md: self.body_md,
            mood: self.mood,
            tz: self.tz,
            tags: self.tags,
        }
    }
}

/// Which entries a scoped read may see: the whole vault, only
/// directly unassigned entries, or one folder with or without its
/// subtree. Unknown folder ids fail loudly, never silently empty.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ScopeInput {
    All,
    Root,
    Folder {
        id: String,
        include_descendants: bool,
    },
}

impl ScopeInput {
    fn resolve(input: Option<ScopeInput>) -> EntryScope {
        match input.unwrap_or(ScopeInput::All) {
            ScopeInput::All => EntryScope::All,
            ScopeInput::Root => EntryScope::Root,
            ScopeInput::Folder {
                id,
                include_descendants,
            } => EntryScope::Folder {
                id,
                include_descendants,
            },
        }
    }
}

#[tauri::command]
pub async fn entry_list(
    app: AppHandle,
    cursor: Option<String>,
    tag: Option<String>,
    date: Option<String>,
    limit: Option<u32>,
    scope: Option<ScopeInput>,
) -> Result<EntryPage, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    let after = cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|e| e.to_ipc())?;
    let scope = ScopeInput::resolve(scope);
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let page = vault.list_entries(&ListQuery {
                scope,
                limit,
                after,
                tag,
                date,
            })?;
            Ok(EntryPage {
                entries: page.items.iter().map(EntrySummary::from).collect(),
                next_cursor: page.next.as_ref().map(encode_cursor),
            })
        }))
    })
    .await
}

#[tauri::command]
pub async fn entry_get(app: AppHandle, id: String) -> Result<EntryDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let entry = vault.get_entry(&id)?;
            Ok(EntryDto::from(&entry))
        }))
    })
    .await
}

/// Files a new entry into a folder (null is the vault root). The
/// folder must be live; the assignment is permanent — no update
/// path can change it afterwards.
#[tauri::command]
pub async fn entry_create(
    app: AppHandle,
    folder_id: Option<String>,
    draft: DraftBody,
) -> Result<EntryDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let entry = vault.create_entry(folder_id.as_deref(), &draft.into_draft())?;
            Ok(EntryDto::from(&entry))
        }))
    })
    .await
}

/// Replaces an entry's date, title, text, mood and tags. A stale
/// `expected_rev` fails with `revision_conflict` instead of
/// overwriting: the caller opened an older revision. Folder
/// membership is never touched — reassignment is rejected
/// structurally, by having no folder field at all.
#[tauri::command]
pub async fn entry_update(
    app: AppHandle,
    id: String,
    expected_rev: i64,
    draft: DraftBody,
) -> Result<EntryDto, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let entry = vault.update_entry(&id, expected_rev, &draft.into_draft())?;
            Ok(EntryDto::from(&entry))
        }))
    })
    .await
}

#[tauri::command]
pub async fn entry_delete(app: AppHandle, id: String) -> Result<(), String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            vault.delete_entry(&id)?;
            Ok(())
        }))
    })
    .await
}

#[derive(Serialize)]
pub struct HitDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    /// Plain text with \u{2}…\u{3} markers around the matches; the
    /// UI turns them into <mark> elements itself (§6.1) — this is
    /// never HTML.
    pub snippet: String,
    pub folder_id: Option<String>,
}

impl From<&SearchHit> for HitDto {
    fn from(h: &SearchHit) -> Self {
        HitDto {
            id: h.id.clone(),
            entry_date: h.entry_date.clone(),
            title: h.title.clone(),
            snippet: h.snippet.clone(),
            folder_id: h.folder_id.clone(),
        }
    }
}

#[tauri::command]
pub async fn search(
    app: AppHandle,
    query: String,
    limit: Option<u32>,
    scope: Option<ScopeInput>,
) -> Result<Vec<HitDto>, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    let scope = ScopeInput::resolve(scope);
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let hits = vault.search(&query, limit, &scope)?;
            Ok(hits.iter().map(HitDto::from).collect())
        }))
    })
    .await
}

/// Vault-wide title suggestions for the link picker: `candidates`
/// newest-first (prefix matches, then FTS title matches,
/// deduplicated), `next_cursor` opaque for the next page. Empty
/// queries return recent entries; 1–2 character queries are
/// prefix-only.
#[derive(Serialize)]
pub struct CandidatePageDto {
    pub candidates: Vec<TitleHitDto>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct TitleHitDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    pub folder_id: Option<String>,
}

impl From<&TitleHit> for TitleHitDto {
    fn from(hit: &TitleHit) -> Self {
        TitleHitDto {
            id: hit.id.clone(),
            entry_date: hit.entry_date.clone(),
            title: hit.title.clone(),
            folder_id: hit.folder_id.clone(),
        }
    }
}

/// The candidate cursor carries its section: `p|` walks prefix
/// matches, `f|` FTS title matches, then the shared keyset triple.
fn encode_candidate_cursor(cursor: &CandidateCursor) -> String {
    format!(
        "{}|{}|{}|{}",
        if cursor.fts { "f" } else { "p" },
        cursor.cursor.entry_date,
        cursor.cursor.created_at,
        cursor.cursor.id
    )
}

fn decode_candidate_cursor(text: &str) -> Result<CandidateCursor, AppError> {
    let invalid = || AppError::Code("invalid_input".into());
    let (section, rest) = text.split_once('|').ok_or_else(invalid)?;
    let fts = match section {
        "f" => true,
        "p" => false,
        _ => return Err(invalid()),
    };
    Ok(CandidateCursor {
        fts,
        cursor: decode_cursor(rest)?,
    })
}

fn finish_candidate_page_dto(page: &CandidatePage) -> CandidatePageDto {
    CandidatePageDto {
        candidates: page.items.iter().map(TitleHitDto::from).collect(),
        next_cursor: page.next.as_ref().map(encode_candidate_cursor),
    }
}

#[tauri::command]
pub async fn entry_link_candidates(
    app: AppHandle,
    query: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<CandidatePageDto, String> {
    let limit = limit.unwrap_or(20).clamp(1, 200) as usize;
    let after = cursor
        .as_deref()
        .map(decode_candidate_cursor)
        .transpose()
        .map_err(|e| e.to_ipc())?;
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let page = vault.link_candidates(&query, limit, after)?;
            Ok(finish_candidate_page_dto(&page))
        }))
    })
    .await
}

/// Body-only UUID candidates for one link target: full bounded
/// bodies plus folder ids for the verifier. The target itself is
/// excluded; deleted sources never appear.
#[derive(Serialize)]
pub struct BacklinkPageDto {
    pub candidates: Vec<BacklinkDto>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct BacklinkDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    pub folder_id: Option<String>,
    /// The full body when it fits the inline budget, `None` for
    /// oversized bodies (see `body_size`): never truncated.
    pub body: Option<String>,
    pub body_size: i64,
    /// Hex SHA-256 over the body bytes: the verify cache keys on
    /// content, never on the revision.
    pub body_hash: String,
}

impl From<&soirvu_core::entries::BacklinkCandidate> for BacklinkDto {
    fn from(hit: &soirvu_core::entries::BacklinkCandidate) -> Self {
        BacklinkDto {
            id: hit.id.clone(),
            entry_date: hit.entry_date.clone(),
            title: hit.title.clone(),
            folder_id: hit.folder_id.clone(),
            body: hit.body.clone(),
            body_size: hit.body_size,
            body_hash: hit.body_hash.clone(),
        }
    }
}

#[tauri::command]
pub async fn backlink_candidates(
    app: AppHandle,
    id: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<BacklinkPageDto, String> {
    let limit = limit.unwrap_or(20).clamp(1, 200) as usize;
    let after = cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|e| e.to_ipc())?;
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let page: BacklinkPage = vault.backlink_candidates(&id, limit, after)?;
            Ok(BacklinkPageDto {
                candidates: page.items.iter().map(BacklinkDto::from).collect(),
                next_cursor: page.next.as_ref().map(encode_cursor),
            })
        }))
    })
    .await
}

#[derive(Serialize)]
pub struct TagDto {
    pub name: String,
    pub entries: i64,
}

impl From<&TagCount> for TagDto {
    fn from(t: &TagCount) -> Self {
        TagDto {
            name: t.name.clone(),
            entries: t.entries,
        }
    }
}

/// One day's live-entry count, for the calendar (§13 Phase 6).
#[derive(Serialize)]
pub struct CalendarDay {
    pub date: String,
    pub entries: i64,
}

impl From<&soirvu_core::entries::DayCount> for CalendarDay {
    fn from(d: &soirvu_core::entries::DayCount) -> Self {
        CalendarDay {
            date: d.date.clone(),
            entries: d.entries,
        }
    }
}

#[tauri::command]
pub async fn on_this_day(
    app: AppHandle,
    date: String,
    scope: Option<ScopeInput>,
) -> Result<Vec<EntrySummary>, String> {
    let scope = ScopeInput::resolve(scope);
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let hits = vault.on_this_day(&date, &scope)?;
            Ok(hits.iter().map(EntrySummary::from).collect())
        }))
    })
    .await
}

#[tauri::command]
pub async fn calendar_days(
    app: AppHandle,
    from: String,
    to: String,
    scope: Option<ScopeInput>,
) -> Result<Vec<CalendarDay>, String> {
    let scope = ScopeInput::resolve(scope);
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let days = vault.entry_days(&from, &to, &scope)?;
            Ok(days.iter().map(CalendarDay::from).collect())
        }))
    })
    .await
}

#[tauri::command]
pub async fn tag_list(app: AppHandle, scope: Option<ScopeInput>) -> Result<Vec<TagDto>, String> {
    let scope = ScopeInput::resolve(scope);
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let tags = vault.list_tags(&scope)?;
            Ok(tags.iter().map(TagDto::from).collect())
        }))
    })
    .await
}

/// Rename and merge return how many live entries carried the tag
/// (for a merge: the entries that had `from`). That is the core's
/// count, not the size of the target tag afterwards.

#[tauri::command]
pub async fn tag_rename(app: AppHandle, from: String, to: String) -> Result<i64, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let count = vault.rename_tag(&from, &to)?;
            // A diary cannot hold 2^63 entries; saturate rather than wrap.
            Ok(i64::try_from(count).unwrap_or(i64::MAX))
        }))
    })
    .await
}

#[tauri::command]
pub async fn tag_merge(app: AppHandle, from: String, into: String) -> Result<i64, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let count = vault.merge_tags(&from, &into)?;
            Ok(i64::try_from(count).unwrap_or(i64::MAX))
        }))
    })
    .await
}

#[tauri::command]
pub async fn tag_delete(app: AppHandle, name: String) -> Result<i64, String> {
    crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| {
            let count = vault.delete_tag(&name)?;
            Ok(i64::try_from(count).unwrap_or(i64::MAX))
        }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use soirvu_core::Error as CoreError;

    fn valid_draft() -> serde_json::Value {
        serde_json::json!({
            "entry_date": "2026-01-02",
            "title": "Test",
            "body_md": "hello",
            "mood": null,
            "tz": null,
            "tags": [],
        })
    }

    /// A stale webview cannot smuggle an id or a folder through a
    /// shape the backend would otherwise ignore.
    #[test]
    fn draft_body_refuses_smuggled_id_and_folder() {
        let mut smuggled = valid_draft();
        smuggled["id"] = serde_json::json!("entry-1");
        smuggled["folder_id"] = serde_json::json!("folder-1");
        match serde_json::from_value::<DraftBody>(smuggled) {
            Err(err) => assert!(
                err.to_string().contains("unknown field"),
                "unexpected: {err}"
            ),
            Ok(_) => panic!("an id and folder must not parse as a DraftBody"),
        }
        // And the plain shape still parses.
        let draft = serde_json::from_value::<DraftBody>(valid_draft()).unwrap();
        assert_eq!(draft.entry_date, "2026-01-02");
    }

    /// The UI keys its conflict banner off this exact code.
    #[test]
    fn stale_rev_maps_to_revision_conflict() {
        let result: Result<EntryDto, String> =
            ipc(Err(AppError::from(CoreError::RevisionConflict)));
        assert_eq!(result.err(), Some("revision_conflict".to_string()));
    }

    #[test]
    fn candidate_cursor_roundtrips_its_section() {
        for fts in [false, true] {
            let cursor = CandidateCursor {
                fts,
                cursor: Cursor {
                    entry_date: "2026-01-02".into(),
                    created_at: 1,
                    id: "e1".into(),
                },
            };
            let decoded = decode_candidate_cursor(&encode_candidate_cursor(&cursor)).unwrap();
            assert_eq!(decoded.fts, fts);
            assert_eq!(decoded.cursor, cursor.cursor);
        }
    }

    #[test]
    fn cursor_decoders_reject_garbage() {
        for bad in ["", "nope", "2026-01-02|notanumber|e1", "2026-01-02|1"] {
            assert_eq!(
                decode_cursor(bad).unwrap_err().to_ipc(),
                "invalid_input",
                "{bad:?}"
            );
        }
        for bad in ["", "x|2026-01-02|1|e1", "p|nope", "f|2026-01-02|1"] {
            assert_eq!(
                decode_candidate_cursor(bad).unwrap_err().to_ipc(),
                "invalid_input",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn scope_resolves_its_shapes() {
        assert_eq!(ScopeInput::resolve(None), EntryScope::All);
        assert_eq!(
            ScopeInput::resolve(Some(ScopeInput::Root)),
            EntryScope::Root
        );
        assert_eq!(
            ScopeInput::resolve(Some(ScopeInput::Folder {
                id: "f".into(),
                include_descendants: true,
            })),
            EntryScope::Folder {
                id: "f".into(),
                include_descendants: true,
            },
        );
    }
}
