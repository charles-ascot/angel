//! Classify module — consumes captured artefacts and tags them via the
//! Anthropic API (Claude Opus 4.6).
//!
//! Produces structured records containing:
//! - Stream tag (which work stream this belongs to)
//! - Related component (specific module or system)
//! - Decisions made
//! - Open questions raised
//! - Status changes noted
//! - Cross-references to other work items
//!
//! Runs as a background polling task. Processes up to `BATCH_SIZE`
//! unclassified artefacts per cycle with a short inter-call gap to
//! stay well inside Anthropic API rate limits.

use anyhow::{Context, Result};
use rusqlite::params;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::anthropic::ANTHROPIC_MODEL;
use crate::secrets;
use crate::storage::Database;

/// How many artefacts to classify per polling cycle.
const BATCH_SIZE: usize = 5;

/// How long to wait between cycles when there is nothing to classify.
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// How long to wait between individual Anthropic API calls within a batch.
const INTER_CALL_DELAY: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Classifier polls SQLite for unclassified artefacts, calls the Anthropic
/// API, and writes the results back to SQLite.
///
/// Tasks are owned by the tokio runtime — dropping `Classifier` does not
/// cancel the background work.
pub struct Classifier;

impl Classifier {
    /// Spawn the classification background task. Returns immediately.
    pub fn start(db: Database) -> Self {
        tauri::async_runtime::spawn(async move {
            if let Err(e) = classify_loop(db).await {
                error!("Classify: loop exited with error: {:#}", e);
            }
        });
        Self
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn classify_loop(db: Database) -> Result<()> {
    // Retrieve the Anthropic API key from the macOS Keychain once at startup.
    let api_key = match secrets::get_anthropic_key() {
        Ok(k) => k,
        Err(e) => {
            error!(
                "Classify: Anthropic API key unavailable — classification disabled: {:#}",
                e
            );
            return Ok(());
        }
    };

    let client = reqwest::Client::new();
    info!("Classify: background task started");

    loop {
        let artefacts = fetch_unclassified(&db, BATCH_SIZE)?;

        if artefacts.is_empty() {
            tokio::time::sleep(IDLE_POLL_INTERVAL).await;
            continue;
        }

        debug!("Classify: processing batch of {} artefacts", artefacts.len());

        for artefact in &artefacts {
            match classify_one(&client, &api_key, artefact).await {
                Ok(result) => {
                    if let Err(e) = persist_classified(&db, &artefact.id, &result) {
                        warn!(
                            "Classify: failed to persist classification for {}: {:#}",
                            artefact.id, e
                        );
                        // Leave classified=0 so it retries next cycle.
                        continue;
                    }
                    if let Err(e) = mark_artefact_classified(&db, &artefact.id) {
                        warn!(
                            "Classify: failed to mark artefact {} as classified: {:#}",
                            artefact.id, e
                        );
                    }
                    debug!("Classify: classified artefact {}", artefact.id);
                }
                Err(e) => {
                    warn!(
                        "Classify: API call failed for artefact {} — will retry: {:#}",
                        artefact.id, e
                    );
                }
            }

            tokio::time::sleep(INTER_CALL_DELAY).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

/// A raw artefact row as read from the `artefacts` table.
#[derive(Debug)]
struct RawArtefact {
    id: String,
    source: String,
    captured_at: String,
    raw_content: String,
}

/// Fetch up to `limit` unclassified artefacts, oldest first.
fn fetch_unclassified(db: &Database, limit: usize) -> Result<Vec<RawArtefact>> {
    let conn = db.conn();
    let conn = conn.lock().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT id, source, captured_at, raw_content \
             FROM artefacts \
             WHERE classified = 0 \
             ORDER BY captured_at ASC \
             LIMIT ?1",
        )
        .context("failed to prepare fetch_unclassified query")?;

    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(RawArtefact {
                id: row.get(0)?,
                source: row.get(1)?,
                captured_at: row.get(2)?,
                raw_content: row.get(3)?,
            })
        })
        .context("failed to query unclassified artefacts")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to collect unclassified artefacts")?;

    Ok(rows)
}

/// Insert a classification result into `classified_artefacts`.
fn persist_classified(
    db: &Database,
    artefact_id: &str,
    result: &ClassificationResult,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let classified_at = chrono::Utc::now().to_rfc3339();

    let decisions = serde_json::to_string(&result.decisions)
        .context("failed to serialise decisions")?;
    let open_questions = serde_json::to_string(&result.open_questions)
        .context("failed to serialise open_questions")?;
    let status_changes = serde_json::to_string(&result.status_changes)
        .context("failed to serialise status_changes")?;
    let cross_references = serde_json::to_string(&result.cross_references)
        .context("failed to serialise cross_references")?;

    let conn = db.conn();
    let conn = conn.lock().unwrap();

    conn.execute(
        "INSERT INTO classified_artefacts \
         (id, artefact_id, classified_at, stream_tag, related_component, \
          decisions, open_questions, status_changes, cross_references) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            artefact_id,
            classified_at,
            result.stream_tag,
            result.related_component,
            decisions,
            open_questions,
            status_changes,
            cross_references,
        ],
    )
    .context("failed to insert classified artefact")?;

    Ok(())
}

/// Set `classified = 1` on the given artefact row.
fn mark_artefact_classified(db: &Database, artefact_id: &str) -> Result<()> {
    let conn = db.conn();
    let conn = conn.lock().unwrap();

    conn.execute(
        "UPDATE artefacts SET classified = 1 WHERE id = ?1",
        params![artefact_id],
    )
    .context("failed to mark artefact as classified")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Anthropic API
// ---------------------------------------------------------------------------

/// The structured classification produced by the model.
#[derive(Debug, serde::Deserialize)]
struct ClassificationResult {
    stream_tag: Option<String>,
    related_component: Option<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    open_questions: Vec<String>,
    #[serde(default)]
    status_changes: Vec<String>,
    #[serde(default)]
    cross_references: Vec<String>,
}

/// Minimal Anthropic Messages API request shape.
#[derive(serde::Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(serde::Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Minimal Anthropic Messages API response shape.
#[derive(serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(serde::Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

const SYSTEM_PROMPT: &str = "\
You are Angel, a workload intelligence system. You receive raw JSON artefacts \
captured from a developer's workflow (Claude Code sessions, desktop app logs, \
git activity, email, etc.) and produce a structured classification.

Return ONLY a valid JSON object matching this exact schema — no prose, no \
markdown, no code fences:

{
  \"stream_tag\": \"short-slug identifying the work stream, e.g. 'angel-capture' or 'chiops-infra' — null if unclear\",
  \"related_component\": \"specific module or component, e.g. 'capture-module' — null if unclear\",
  \"decisions\": [\"any decision made or recorded\"],
  \"open_questions\": [\"any question raised but not yet resolved\"],
  \"status_changes\": [\"any status transition noted, e.g. 'task moved to in-progress'\"],
  \"cross_references\": [\"name or description of any other referenced work item\"]
}

All array fields default to []. All string fields default to null if unclear.";

/// Call the Anthropic Messages API to classify a single artefact.
async fn classify_one(
    client: &reqwest::Client,
    api_key: &str,
    artefact: &RawArtefact,
) -> Result<ClassificationResult> {
    let user_content = format!(
        "Source: {}\nCaptured at: {}\n\n{}",
        artefact.source, artefact.captured_at, artefact.raw_content
    );

    let request = AnthropicRequest {
        model: ANTHROPIC_MODEL,
        max_tokens: 1024,
        system: SYSTEM_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user",
            content: &user_content,
        }],
    };

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await
        .context("Anthropic API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic API returned {}: {}", status, body);
    }

    let api_resp: AnthropicResponse = resp
        .json()
        .await
        .context("failed to parse Anthropic API response")?;

    let text = api_resp
        .content
        .into_iter()
        .find(|c| c.kind == "text")
        .map(|c| c.text)
        .context("no text content in Anthropic API response")?;

    let json_str = extract_json(&text);

    serde_json::from_str::<ClassificationResult>(json_str)
        .with_context(|| format!("failed to parse classification JSON: {}", json_str))
}

/// Extract the JSON object from a model response that may contain prose or
/// markdown code fences.
fn extract_json(text: &str) -> &str {
    // Strip ```json ... ``` or ``` ... ``` fences
    for fence in &["```json", "```"] {
        if let Some(start) = text.find(fence) {
            let after_fence = start + fence.len();
            if let Some(end) = text[after_fence..].find("```") {
                return text[after_fence..after_fence + end].trim();
            }
        }
    }
    // Fall back to the first { ... } block
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        return &text[start..=end];
    }
    text.trim()
}

/// Initialise the Classify module.
pub fn init() {}
