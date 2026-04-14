//! State Model module — SQLite-backed relational structure holding the answer
//! to "what is true right now".
//!
//! Consumes rows from `classified_artefacts` (where `state_model_processed = 0`)
//! and incrementally maintains the `work_items` table.
//!
//! Matching strategy:
//! - A classified artefact with a `stream_tag` is matched against existing
//!   active/blocked `work_items` on that same tag.
//! - On a match: `last_seen_at` is updated and status is re-evaluated.
//! - On no match: a new `work_item` is created.
//! - Artefacts without a `stream_tag` are marked processed but generate no
//!   work item — the model couldn't identify their stream.
//!
//! The `deep_link` field is always populated. Format for now:
//!   `angel://artefact/{source}/{artefact_id}`
//! This is a valid internal URI Angel's notification layer can resolve.
//! Richer link formats (session IDs, file paths) will be added when the
//! Capture module parses richer metadata from each source.

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::storage::Database;

/// How many classified artefacts to incorporate per cycle.
const BATCH_SIZE: usize = 20;

/// Poll interval when there is nothing to process.
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// StateModeler polls `classified_artefacts` and maintains `work_items`.
///
/// Runs as a background task owned by the tokio runtime.
pub struct StateModeler;

impl StateModeler {
    /// Spawn the state model background task. Returns immediately.
    pub fn start(db: Database) -> Self {
        tokio::spawn(async move {
            if let Err(e) = state_model_loop(db).await {
                error!("StateModel: loop exited: {:#}", e);
            }
        });
        Self
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn state_model_loop(db: Database) -> Result<()> {
    info!("StateModel: background task started");

    loop {
        let batch = fetch_unprocessed(&db, BATCH_SIZE)?;

        if batch.is_empty() {
            tokio::time::sleep(IDLE_POLL_INTERVAL).await;
            continue;
        }

        debug!("StateModel: processing batch of {}", batch.len());

        for row in &batch {
            if let Err(e) = incorporate(&db, row) {
                warn!(
                    "StateModel: failed to incorporate classified artefact {}: {:#}",
                    row.id, e
                );
                // Leave state_model_processed = 0 so it retries next cycle.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

/// Incorporate one classified artefact into the `work_items` table.
fn incorporate(db: &Database, row: &ClassifiedRow) -> Result<()> {
    let conn = db.conn();
    let mut conn = conn.lock().unwrap();

    // Use a transaction so the upsert + mark-processed are atomic.
    let tx = conn.transaction().context("failed to begin transaction")?;

    match row.stream_tag.as_deref() {
        None => {
            // No stream tag — nothing to add to the state model.
            debug!(
                "StateModel: skipping artefact {} (no stream_tag)",
                row.id
            );
        }
        Some(tag) => {
            // Try to find an active or blocked work item for this stream.
            let existing: Option<(String, String)> = tx
                .query_row(
                    "SELECT id, status FROM work_items \
                     WHERE stream_tag = ?1 AND status != 'completed' \
                     ORDER BY last_seen_at DESC \
                     LIMIT 1",
                    params![tag],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .context("failed to query existing work items")?;

            let now = chrono::Utc::now().to_rfc3339();

            if let Some((work_item_id, current_status)) = existing {
                // Update the existing work item.
                let new_status =
                    derive_status(&row.status_changes, &current_status);

                tx.execute(
                    "UPDATE work_items \
                     SET last_seen_at = ?1, status = ?2 \
                     WHERE id = ?3",
                    params![now, new_status, work_item_id],
                )
                .context("failed to update work item")?;

                debug!(
                    "StateModel: updated work_item {} (stream={}, status={}→{})",
                    work_item_id, tag, current_status, new_status
                );
            } else {
                // Create a new work item.
                let id = uuid::Uuid::new_v4().to_string();
                let title =
                    derive_title(Some(tag), row.related_component.as_deref());
                let description = derive_description(
                    &row.decisions,
                    &row.open_questions,
                );
                let status = derive_status(&row.status_changes, "active");
                let deep_link =
                    derive_deep_link(&row.source, &row.artefact_id);

                tx.execute(
                    "INSERT INTO work_items \
                     (id, title, description, status, stream_tag, \
                      deep_link, first_seen_at, last_seen_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        title,
                        description,
                        status,
                        tag,
                        deep_link,
                        now,
                        now,
                    ],
                )
                .context("failed to insert work item")?;

                info!(
                    "StateModel: created work_item {} — \"{}\" (stream={})",
                    id, title, tag
                );
            }
        }
    }

    // Mark this classified artefact as processed regardless of whether a
    // work item was created — we don't want to process it again.
    tx.execute(
        "UPDATE classified_artefacts \
         SET state_model_processed = 1 \
         WHERE id = ?1",
        params![row.id],
    )
    .context("failed to mark classified artefact as processed")?;

    tx.commit().context("failed to commit state model transaction")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

/// A classified artefact row joined with its parent artefact's source.
#[derive(Debug)]
struct ClassifiedRow {
    id: String,
    artefact_id: String,
    stream_tag: Option<String>,
    related_component: Option<String>,
    decisions: Vec<String>,
    open_questions: Vec<String>,
    status_changes: Vec<String>,
    source: String,
}

/// Fetch up to `limit` unprocessed classified artefacts, oldest first.
fn fetch_unprocessed(db: &Database, limit: usize) -> Result<Vec<ClassifiedRow>> {
    let conn = db.conn();
    let conn = conn.lock().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT ca.id, ca.artefact_id, ca.stream_tag, ca.related_component, \
                    ca.decisions, ca.open_questions, ca.status_changes, \
                    a.source \
             FROM classified_artefacts ca \
             JOIN artefacts a ON ca.artefact_id = a.id \
             WHERE ca.state_model_processed = 0 \
             ORDER BY ca.classified_at ASC \
             LIMIT ?1",
        )
        .context("failed to prepare fetch_unprocessed query")?;

    let rows: Vec<ClassifiedRow> = stmt
        .query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,  // decisions JSON
                row.get::<_, Option<String>>(5)?,  // open_questions JSON
                row.get::<_, Option<String>>(6)?,  // status_changes JSON
                row.get::<_, String>(7)?,
            ))
        })
        .context("failed to query unprocessed classified artefacts")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to collect query results")?
        .into_iter()
        .map(|(id, artefact_id, stream_tag, related_component, dec, oq, sc, source)| {
            ClassifiedRow {
                id,
                artefact_id,
                stream_tag,
                related_component,
                decisions: parse_json_array(dec.as_deref()),
                open_questions: parse_json_array(oq.as_deref()),
                status_changes: parse_json_array(sc.as_deref()),
                source,
            }
        })
        .collect();

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Derivation helpers
// ---------------------------------------------------------------------------

/// Derive a work item status from `status_changes` text, falling back to
/// `current` if nothing conclusive is found.
fn derive_status(status_changes: &[String], current: &str) -> &'static str {
    for change in status_changes {
        let lower = change.to_lowercase();
        if lower.contains("complet") || lower.contains("done") || lower.contains("finish") {
            return "completed";
        }
        if lower.contains("block") || lower.contains("wait") || lower.contains("stuck") {
            return "blocked";
        }
        if lower.contains("activ") || lower.contains("progress") || lower.contains("start") {
            return "active";
        }
    }
    // Preserve the current status if no signal in status_changes.
    match current {
        "completed" => "completed",
        "blocked" => "blocked",
        _ => "active",
    }
}

/// Build a human-readable work item title from the stream tag and component.
fn derive_title(stream_tag: Option<&str>, related_component: Option<&str>) -> String {
    match (stream_tag, related_component) {
        (Some(tag), Some(component)) => format!("{} — {}", tag, component),
        (Some(tag), None) => tag.to_string(),
        (None, Some(component)) => component.to_string(),
        (None, None) => "Untagged work item".to_string(),
    }
}

/// Build a short description from the first decision or open question.
fn derive_description(decisions: &[String], open_questions: &[String]) -> Option<String> {
    decisions
        .first()
        .or_else(|| open_questions.first())
        .cloned()
}

/// Build an internal deep link URI for this artefact.
///
/// Format: `angel://artefact/{source}/{artefact_id}`
///
/// Angel's notification layer resolves this URI in a later session when
/// richer metadata (session IDs, file paths) is available from Capture.
fn derive_deep_link(source: &str, artefact_id: &str) -> String {
    format!("angel://artefact/{}/{}", source, artefact_id)
}

/// Parse a stored JSON array string into a `Vec<String>`, returning an
/// empty vec on any parse error rather than propagating.
fn parse_json_array(json: Option<&str>) -> Vec<String> {
    json.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

/// Initialise the State Model module.
pub fn init() {}
