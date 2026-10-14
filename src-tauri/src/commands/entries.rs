//! Entry, search and tag commands (§6.1). Every one of them runs
//! through the state's guard: locked → the `locked` code.

use serde::{Deserialize, Serialize};
use soirvu_core::entries::{Entry, EntryDraft, SearchHit, TagInfo};

use crate::error::ipc;
use crate::state::AppState;
use tauri::State;

#[derive(Serialize)]
pub struct EntryDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    pub body_md: String,
    pub mood: Option<i64>,
    pub tz: Option<String>,
    pub tags: Vec<String>,
    /// Unix seconds; the UI formats them in the entry's time zone.
    pub created_at: i64,
    pub updated_at: i64,
    pub rev: i64,
}

impl From<&Entry> for EntryDto {
    fn from(e: &Entry) -> Self {
        EntryDto {
            id: e.id.clone(),
            entry_date: e.entry_date.clone(),
            title: e.title.clone(),
            body_md: e.body_md.clone(),
            mood: e.mood,
            tz: e.tz.clone(),
            tags: e.tags.clone(),
            created_at: e.created_at,
            updated_at: e.updated_at,
            rev: e.rev,
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
    pub mood: Option<i64>,
    pub tags: Vec<String>,
    pub updated_at: i64,
}

impl From<&Entry> for EntrySummary {
    fn from(e: &Entry) -> Self {
        EntrySummary {
            id: e.id.clone(),
            entry_date: e.entry_date.clone(),
            title: e.title.clone(),
            excerpt: e.body_md.chars().take(140).collect(),
            mood: e.mood,
            tags: e.tags.clone(),
            updated_at: e.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct EntryPage {
    pub entries: Vec<EntrySummary>,
    /// Pass back as `cursor` for the next page; null at the end.
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveDraft {
    /// Null creates a new entry; a value updates that entry.
    pub id: Option<String>,
    pub entry_date: String,
    pub title: Option<String>,
    pub body_md: String,
    pub mood: Option<i64>,
    pub tz: Option<String>,
    pub tags: Vec<String>,
}

#[tauri::command]
pub fn entry_list(
    state: State<'_, AppState>,
    cursor: Option<String>,
    tag: Option<String>,
    limit: Option<u32>,
) -> Result<EntryPage, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    ipc(state.with_vault(|vault| {
        let all = vault.list_entries(tag.as_deref())?;
        // Keyset pagination over the core's sort order
        // (entry_date, created_at, id — all descending): the cursor
        // is the id of the last entry of the previous page.
        let start = match &cursor {
            None => 0,
            Some(id) => all
                .iter()
                .position(|e| &e.id == id)
                .map(|pos| pos + 1)
                .unwrap_or(all.len()),
        };
        let page: Vec<EntrySummary> =
            all.iter().skip(start).take(limit).map(EntrySummary::from).collect();
        let next_cursor = if start + page.len() < all.len() {
            page.last().map(|e| e.id.clone())
        } else {
            None
        };
        Ok(EntryPage { entries: page, next_cursor })
    }))
}

#[tauri::command]
pub fn entry_get(state: State<'_, AppState>, id: String) -> Result<EntryDto, String> {
    ipc(state.with_vault(|vault| {
        let entry = vault.get_entry(&id)?;
        Ok(EntryDto::from(&entry))
    }))
}

#[tauri::command]
pub fn entry_save(state: State<'_, AppState>, draft: SaveDraft) -> Result<EntryDto, String> {
    ipc(state.with_vault(|vault| {
        let core_draft = EntryDraft {
            entry_date: draft.entry_date,
            title: draft.title,
            body_md: draft.body_md,
            mood: draft.mood,
            tz: draft.tz,
            tags: draft.tags,
        };
        let entry = match &draft.id {
            None => vault.create_entry(&core_draft)?,
            Some(id) => vault.update_entry(id, &core_draft)?,
        };
        Ok(EntryDto::from(&entry))
    }))
}

#[tauri::command]
pub fn entry_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ipc(state.with_vault(|vault| {
        vault.delete_entry(&id)?;
        Ok(())
    }))
}

#[derive(Serialize)]
pub struct HitDto {
    pub id: String,
    pub entry_date: String,
    pub title: Option<String>,
    /// Plain text with \u{1}…\u{2} markers around the matches; the
    /// UI turns them into <mark> elements itself (§6.1) — this is
    /// never HTML.
    pub snippet: String,
}

impl From<&SearchHit> for HitDto {
    fn from(h: &SearchHit) -> Self {
        HitDto {
            id: h.entry_id.clone(),
            entry_date: h.entry_date.clone(),
            title: h.title.clone(),
            snippet: h.snippet.clone(),
        }
    }
}

#[tauri::command]
pub fn search(state: State<'_, AppState>, query: String) -> Result<Vec<HitDto>, String> {
    ipc(state.with_vault(|vault| {
        let hits = vault.search(&query, 50)?;
        Ok(hits.iter().map(HitDto::from).collect())
    }))
}

#[derive(Serialize)]
pub struct TagDto {
    pub name: String,
    pub entries: i64,
}

impl From<&TagInfo> for TagDto {
    fn from(t: &TagInfo) -> Self {
        TagDto { name: t.name.clone(), entries: t.entries }
    }
}

#[tauri::command]
pub fn tag_list(state: State<'_, AppState>) -> Result<Vec<TagDto>, String> {
    ipc(state.with_vault(|vault| {
        let tags = vault.list_tags()?;
        Ok(tags.iter().map(TagDto::from).collect())
    }))
}

/// Rename and merge return the number of entries carrying the tag
/// afterwards (§6.1).
#[tauri::command]
pub fn tag_rename(state: State<'_, AppState>, from: String, to: String) -> Result<i64, String> {
    ipc(state.with_vault(|vault| {
        let tag = vault.rename_tag(&from, &to)?;
        Ok(tag.entries)
    }))
}

#[tauri::command]
pub fn tag_merge(state: State<'_, AppState>, from: String, into: String) -> Result<i64, String> {
    ipc(state.with_vault(|vault| {
        let tag = vault.merge_tags(&from, &into)?;
        Ok(tag.entries)
    }))
}
