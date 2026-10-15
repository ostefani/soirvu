//! Entry, search and tag commands (§6.1). Every one of them runs
//! through the state's guard: locked → the `locked` code.

use serde::{Deserialize, Serialize};
use soirvu_core::entries::{
    Cursor, Entry, EntryDraft, EntrySummary as CoreSummary, ListQuery, SearchHit, TagCount,
};

use crate::error::{ipc, AppError};
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
    /// Unix milliseconds; the UI formats them in the entry's time zone.
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
    pub tags: Vec<String>,
    pub updated_at: i64,
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
    date: Option<String>,
    limit: Option<u32>,
) -> Result<EntryPage, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    let after = cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|e| e.to_ipc())?;
    ipc(state.with_vault(|vault| {
        let page = vault.list_entries(&ListQuery { limit, after, tag, date })?;
        Ok(EntryPage {
            entries: page.items.iter().map(EntrySummary::from).collect(),
            next_cursor: page.next.as_ref().map(encode_cursor),
        })
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
    /// Plain text with \u{2}…\u{3} markers around the matches; the
    /// UI turns them into <mark> elements itself (§6.1) — this is
    /// never HTML.
    pub snippet: String,
}

impl From<&SearchHit> for HitDto {
    fn from(h: &SearchHit) -> Self {
        HitDto {
            id: h.id.clone(),
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
pub fn calendar_days(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<Vec<CalendarDay>, String> {
    ipc(state.with_vault(|vault| {
        let days = vault.entry_days(&from, &to)?;
        Ok(days.iter().map(CalendarDay::from).collect())
    }))
}

#[tauri::command]
pub fn tag_list(state: State<'_, AppState>) -> Result<Vec<TagDto>, String> {
    ipc(state.with_vault(|vault| {
        let tags = vault.list_tags()?;
        Ok(tags.iter().map(TagDto::from).collect())
    }))
}

/// Rename and merge return how many live entries carried the tag
/// (for a merge: the entries that had `from`). That is the core's
/// count, not the size of the target tag afterwards.
#[tauri::command]
pub fn tag_rename(state: State<'_, AppState>, from: String, to: String) -> Result<i64, String> {
    ipc(state.with_vault(|vault| {
        let count = vault.rename_tag(&from, &to)?;
        // A diary cannot hold 2^63 entries; saturate rather than wrap.
        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }))
}

#[tauri::command]
pub fn tag_merge(state: State<'_, AppState>, from: String, into: String) -> Result<i64, String> {
    ipc(state.with_vault(|vault| {
        let count = vault.merge_tags(&from, &into)?;
        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }))
}
