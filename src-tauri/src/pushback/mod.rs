//! Pushback module — intercepts new Claude Code session starts via filesystem
//! watch and runs similarity checks against the State Model.
//!
//! Trigger: a new `.jsonl` file appears under `~/.claude/projects/`.
//!
//! On a match, Angel logs to `pushback_log` and optionally surfaces a native
//! macOS notification or dialog containing a clickable deep link.
//!
//! Every pushback_log row *must* have a deep link — "there is a related chat"
//! without a link is forbidden.
//!
//! Escalation rungs (based on similarity score):
//!
//! | Score   | Rung           | Action                                  |
//! |---------|----------------|-----------------------------------------|
//! | ≥ 0.60  | `halt`         | Blocking osascript dialog               |
//! | ≥ 0.30  | `notification` | macOS notification banner               |
//! | ≥ 0.15  | `registry`     | Silent DB log only                      |
//! | < 0.15  | —              | Ignored entirely                        |
//!
//! Similarity is computed as Jaccard over word tokens extracted from the new
//! session's first three lines vs. each work item's title + stream_tag +
//! description.

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use rusqlite::params;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::storage::Database;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// Minimum similarity to log a registry entry (silent).
const REGISTRY_THRESHOLD: f32 = 0.15;

/// Minimum similarity to send an OS notification.
const NOTIFY_THRESHOLD: f32 = 0.30;

/// Minimum similarity to show a blocking halt dialog.
const HALT_THRESHOLD: f32 = 0.60;

/// How many lines to read from the new session file for similarity matching.
const SAMPLE_LINES: usize = 3;

/// How long to wait after file creation before reading content (lets the
/// writing process flush its first line).
const CREATION_DELAY: Duration = Duration::from_millis(600);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// PushbackWatcher watches for new Claude Code sessions and runs pushback
/// logic. Runs as a background task owned by the tokio runtime.
pub struct PushbackWatcher;

impl PushbackWatcher {
    /// Spawn the pushback background task. Returns immediately.
    pub fn start(db: Database) -> Self {
        tokio::spawn(async move {
            if let Err(e) = pushback_loop(db).await {
                error!("Pushback: loop exited: {:#}", e);
            }
        });
        Self
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn pushback_loop(db: Database) -> Result<()> {
    let projects_dir = expand_home(crate::CLAUDE_CODE_LOG_PATH);

    if !projects_dir.exists() {
        warn!(
            "Pushback: projects directory does not exist, skipping: {}",
            projects_dir.display()
        );
        return Ok(());
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::RecommendedWatcher::new(
        move |result| {
            tx.send(result).ok();
        },
        notify::Config::default(),
    )
    .context("failed to create pushback filesystem watcher")?;

    watcher
        .watch(&projects_dir, RecursiveMode::Recursive)
        .context("failed to start watching projects directory")?;

    info!(
        "Pushback: watching {} for new sessions",
        projects_dir.display()
    );

    while let Some(result) = rx.recv().await {
        match result {
            Ok(event) => {
                if matches!(event.kind, EventKind::Create(_)) {
                    for path in &event.paths {
                        if is_jsonl(path) {
                            let db = db.clone();
                            let path = path.clone();
                            tokio::spawn(async move {
                                // Brief pause so the writer flushes its first line.
                                tokio::time::sleep(CREATION_DELAY).await;
                                if let Err(e) = on_new_session(&db, &path).await {
                                    warn!(
                                        "Pushback: error processing new session {}: {:#}",
                                        path.display(),
                                        e
                                    );
                                }
                            });
                        }
                    }
                }
            }
            Err(e) => warn!("Pushback: watch error: {}", e),
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Session event handler
// ---------------------------------------------------------------------------

/// Called when a new session `.jsonl` file is created. Runs the similarity
/// check and triggers the appropriate escalation if warranted.
async fn on_new_session(db: &Database, session_path: &Path) -> Result<()> {
    debug!("Pushback: new session detected: {}", session_path.display());

    // Read the first few lines from the new session for matching.
    let session_text = read_sample_lines(session_path, SAMPLE_LINES);

    if session_text.trim().is_empty() {
        debug!(
            "Pushback: no readable content in new session, skipping: {}",
            session_path.display()
        );
        return Ok(());
    }

    // Fetch all active/blocked work items from the State Model.
    let work_items = fetch_active_work_items(db)?;

    if work_items.is_empty() {
        debug!("Pushback: no active work items, nothing to match against");
        return Ok(());
    }

    // Find the best-matching work item.
    let best = work_items
        .iter()
        .map(|item| {
            let score = similarity_score(&session_text, item);
            (score, item)
        })
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let (score, matched_item) = match best {
        Some(pair) if pair.0 >= REGISTRY_THRESHOLD => pair,
        _ => {
            debug!(
                "Pushback: no significant match for session {} (best score below threshold)",
                session_path.display()
            );
            return Ok(());
        }
    };

    let escalation = escalation_level(score);

    info!(
        "Pushback: session {} matched work_item '{}' (score={:.2}, escalation={})",
        session_path.display(),
        matched_item.title,
        score,
        escalation
    );

    // Log the pushback event.
    log_pushback(
        db,
        session_path,
        &matched_item.id,
        escalation,
        &matched_item.deep_link,
    )?;

    // Surface the notification / dialog based on escalation level.
    match escalation {
        "notification" => {
            notify_user(&matched_item.title, &matched_item.deep_link);
        }
        "halt" => {
            halt_user(&matched_item.title, &matched_item.deep_link);
        }
        _ => { /* registry: silent */ }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Similarity
// ---------------------------------------------------------------------------

/// A minimal work item row for matching purposes.
struct WorkItemRow {
    id: String,
    title: String,
    stream_tag: Option<String>,
    description: Option<String>,
    deep_link: String,
}

/// Jaccard similarity between the session text and a work item's indexed text.
fn similarity_score(session_text: &str, item: &WorkItemRow) -> f32 {
    let item_text = format!(
        "{} {} {}",
        item.title,
        item.stream_tag.as_deref().unwrap_or(""),
        item.description.as_deref().unwrap_or("")
    );

    let session_tokens = tokenize(session_text);
    let item_tokens = tokenize(&item_text);

    if session_tokens.is_empty() || item_tokens.is_empty() {
        return 0.0;
    }

    let intersection = session_tokens.intersection(&item_tokens).count();
    let union = session_tokens.union(&item_tokens).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f32 / union as f32
}

/// Tokenize `text` into a set of lowercase words, filtering stop words and
/// very short tokens (≤ 2 chars).
fn tokenize(text: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "for", "with", "from", "this", "that", "have", "been",
        "will", "are", "was", "not", "but", "can", "all", "its", "new", "one",
        "you", "any", "out", "get", "has", "via", "use", "used",
    ];

    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 2 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Map a similarity score to an escalation rung string.
fn escalation_level(score: f32) -> &'static str {
    if score >= HALT_THRESHOLD {
        "halt"
    } else if score >= NOTIFY_THRESHOLD {
        "notification"
    } else {
        "registry"
    }
}

// ---------------------------------------------------------------------------
// macOS notification / dialog
// ---------------------------------------------------------------------------

/// Show a macOS notification banner via osascript.
///
/// The deep link is included in the body so it is visible to the user even
/// before the Angel frontend registers the `angel://` URL scheme handler.
fn notify_user(work_item_title: &str, deep_link: &str) {
    let script = format!(
        r#"display notification "Related work: {title}\n{link}" with title "Angel" subtitle "Possible duplicate detected" sound name "Frog""#,
        title = escape_applescript(work_item_title),
        link = escape_applescript(deep_link),
    );

    match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    {
        Ok(out) if out.status.success() => {
            debug!("Pushback: notification sent for '{}'", work_item_title);
        }
        Ok(out) => {
            warn!(
                "Pushback: osascript notification failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            warn!("Pushback: failed to run osascript: {}", e);
        }
    }
}

/// Show a blocking macOS dialog via osascript (halt escalation).
///
/// Blocks until the user dismisses. This is intentional — halt is reserved
/// for very high-confidence matches where awareness before continuing is
/// critical.
fn halt_user(work_item_title: &str, deep_link: &str) {
    let script = format!(
        r#"display dialog "Angel detected a very likely duplicate of existing work:\n\n\"{title}\"\n\nDeep link: {link}\n\nYou may still proceed — this is advisory only." with title "Angel — Strong Match Detected" buttons {{"OK"}} default button "OK" with icon caution"#,
        title = escape_applescript(work_item_title),
        link = escape_applescript(deep_link),
    );

    match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    {
        Ok(out) if out.status.success() => {
            info!(
                "Pushback: halt dialog dismissed for '{}'",
                work_item_title
            );
        }
        Ok(out) => {
            warn!(
                "Pushback: osascript halt dialog failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => {
            warn!("Pushback: failed to run osascript: {}", e);
        }
    }
}

/// Escape single quotes and backslashes for inline AppleScript strings.
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

/// Fetch all active and blocked work items for matching.
fn fetch_active_work_items(db: &Database) -> Result<Vec<WorkItemRow>> {
    let conn = db.conn();
    let conn = conn.lock().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT id, title, stream_tag, description, deep_link \
             FROM work_items \
             WHERE status IN ('active', 'blocked') \
             ORDER BY last_seen_at DESC",
        )
        .context("failed to prepare fetch_active_work_items query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(WorkItemRow {
                id: row.get(0)?,
                title: row.get(1)?,
                stream_tag: row.get(2)?,
                description: row.get(3)?,
                deep_link: row.get(4)?,
            })
        })
        .context("failed to query active work items")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to collect active work items")?;

    Ok(rows)
}

/// Insert a row into `pushback_log`.
fn log_pushback(
    db: &Database,
    session_path: &Path,
    work_item_id: &str,
    escalation_level: &str,
    deep_link: &str,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let triggered_at = chrono::Utc::now().to_rfc3339();
    let session_path_str = session_path.to_string_lossy();

    let conn = db.conn();
    let conn = conn.lock().unwrap();

    conn.execute(
        "INSERT INTO pushback_log \
         (id, triggered_at, new_session_path, matched_work_item_id, \
          escalation_level, deep_link) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            triggered_at,
            session_path_str.as_ref(),
            work_item_id,
            escalation_level,
            deep_link,
        ],
    )
    .context("failed to insert pushback log entry")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// File helpers
// ---------------------------------------------------------------------------

/// Read up to `n` lines from `path`, concatenating them into a single string.
/// Returns an empty string if the file cannot be read.
fn read_sample_lines(path: &Path, n: usize) -> String {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            debug!(
                "Pushback: could not open session file {}: {}",
                path.display(),
                e
            );
            return String::new();
        }
    };

    BufReader::new(file)
        .lines()
        .take(n)
        .filter_map(|l| l.ok())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Expand a leading `~/` to the user's home directory.
fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Return `true` if `path` has a `.jsonl` extension.
fn is_jsonl(path: &Path) -> bool {
    path.extension().map_or(false, |e| e == "jsonl")
}
