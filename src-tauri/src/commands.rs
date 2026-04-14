//! Tauri commands exposed to the frontend.
//!
//! All commands are read-only — the frontend never writes to the database
//! directly. Mutations happen exclusively in background tasks.

use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// A work item row serialised for the frontend.
#[derive(Debug, Serialize)]
pub struct WorkItemDto {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub stream_tag: Option<String>,
    pub deep_link: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

/// A pushback log row serialised for the frontend.
#[derive(Debug, Serialize)]
pub struct PushbackEntryDto {
    pub id: String,
    pub triggered_at: String,
    pub new_session_path: String,
    pub matched_work_item_id: Option<String>,
    pub escalation_level: String,
    pub deep_link: String,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Return all work items, ordered by last_seen_at descending.
#[tauri::command]
pub fn list_work_items(state: State<AppState>) -> Result<Vec<WorkItemDto>, String> {
    let conn = state.db.conn();
    let conn = conn.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, status, stream_tag, deep_link, \
                    first_seen_at, last_seen_at \
             FROM work_items \
             ORDER BY last_seen_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(WorkItemDto {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status: row.get(3)?,
                stream_tag: row.get(4)?,
                deep_link: row.get(5)?,
                first_seen_at: row.get(6)?,
                last_seen_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

/// Return the 25 most recent pushback log entries, newest first.
#[tauri::command]
pub fn list_pushback_log(state: State<AppState>) -> Result<Vec<PushbackEntryDto>, String> {
    let conn = state.db.conn();
    let conn = conn.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, triggered_at, new_session_path, matched_work_item_id, \
                    escalation_level, deep_link \
             FROM pushback_log \
             ORDER BY triggered_at DESC \
             LIMIT 25",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![], |row| {
            Ok(PushbackEntryDto {
                id: row.get(0)?,
                triggered_at: row.get(1)?,
                new_session_path: row.get(2)?,
                matched_work_item_id: row.get(3)?,
                escalation_level: row.get(4)?,
                deep_link: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;

    Ok(rows)
}
