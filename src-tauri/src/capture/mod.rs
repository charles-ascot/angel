//! Capture module — reads from external sources and persists raw artefacts.
//!
//! Sources watched in this implementation:
//! - Claude Code transcripts  (`~/.claude/projects/**/*.jsonl`)
//! - Claude Code global history (`~/.claude/history.jsonl`)
//! - Claude desktop app data   (`~/Library/Application Support/Claude/`)
//!
//! Deferred sources (mechanism TBD): claude.ai web chats, Gmail, git activity,
//! arbitrary filesystem paths.
//!
//! Hard rule: every captured artefact is written to local SQLite *before*
//! anything else happens. This module never reasons or decides — it only
//! persists.

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode, Watcher};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::storage::Database;

/// Path to the Claude desktop application data directory.
pub const CLAUDE_DESKTOP_PATH: &str = "~/Library/Application Support/Claude";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Capturer spawns background tasks that watch all configured sources.
///
/// Tasks are owned by the tokio runtime — the `Capturer` value is a
/// lightweight marker; dropping it does not cancel the watchers.
pub struct Capturer;

impl Capturer {
    /// Spawn all capture tasks. Returns immediately; work happens in the
    /// background under the tokio runtime.
    pub fn start(db: Database) -> Self {
        // (path, source label, is_dir)
        let watchers: &[(&str, &str, bool)] = &[
            (crate::CLAUDE_CODE_LOG_PATH, "claude_code_transcript", true),
            (crate::CLAUDE_CODE_HISTORY_PATH, "claude_code_history", false),
            (CLAUDE_DESKTOP_PATH, "claude_desktop", true),
        ];

        for &(raw_path, source, is_dir) in watchers {
            let db = db.clone();
            let path = expand_home(raw_path);
            let source = source.to_string();

            if is_dir {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = watch_dir(db, path, source).await {
                        error!("Capture dir-watcher exited: {:#}", e);
                    }
                });
            } else {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = watch_file(db, path, source).await {
                        error!("Capture file-watcher exited: {:#}", e);
                    }
                });
            }
        }

        Self
    }
}

// ---------------------------------------------------------------------------
// Watcher tasks
// ---------------------------------------------------------------------------

/// Watch `dir` recursively. On startup, catch up on all existing `.jsonl`
/// files; then stream new lines as they are appended.
async fn watch_dir(db: Database, dir: PathBuf, source: String) -> Result<()> {
    if !dir.exists() {
        warn!(
            "Capture: watch directory does not exist, skipping: {}",
            dir.display()
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
    .context("failed to create filesystem watcher")?;

    watcher
        .watch(&dir, RecursiveMode::Recursive)
        .context("failed to start watching directory")?;

    info!(
        "Capture: watching {} (source={})",
        dir.display(),
        source
    );

    let mut positions: HashMap<PathBuf, u64> = HashMap::new();

    // Catch up on content that already existed before we started watching.
    if let Err(e) = read_dir_initial(&db, &dir, &source, &mut positions) {
        warn!(
            "Capture: initial read of {} failed: {:#}",
            dir.display(),
            e
        );
    }

    while let Some(result) = rx.recv().await {
        match result {
            Ok(event) => {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in &event.paths {
                        if is_jsonl(path) {
                            if let Err(e) =
                                read_new_lines(&db, path, &source, &mut positions)
                            {
                                warn!(
                                    "Capture: error reading {}: {:#}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => warn!("Capture: watch error on {}: {}", dir.display(), e),
        }
    }

    Ok(())
}

/// Watch a single file. Catches up on existing content, then streams new
/// lines. Watches the parent directory so creation of the file is detected
/// even if it does not exist when Angel starts.
async fn watch_file(db: Database, file: PathBuf, source: String) -> Result<()> {
    let parent = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));

    if !parent.exists() {
        warn!(
            "Capture: parent directory does not exist, skipping: {}",
            parent.display()
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
    .context("failed to create filesystem watcher")?;

    watcher
        .watch(&parent, RecursiveMode::NonRecursive)
        .context("failed to start watching file parent")?;

    info!(
        "Capture: watching {} (source={})",
        file.display(),
        source
    );

    let mut positions: HashMap<PathBuf, u64> = HashMap::new();

    if file.exists() {
        if let Err(e) = read_new_lines(&db, &file, &source, &mut positions) {
            warn!(
                "Capture: initial read of {} failed: {:#}",
                file.display(),
                e
            );
        }
    }

    while let Some(result) = rx.recv().await {
        match result {
            Ok(event) => {
                for path in &event.paths {
                    if path == &file {
                        if let Err(e) =
                            read_new_lines(&db, path, &source, &mut positions)
                        {
                            warn!(
                                "Capture: error reading {}: {:#}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => warn!(
                "Capture: watch error on {}: {}",
                file.display(),
                e
            ),
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// File reading
// ---------------------------------------------------------------------------

/// Recursively walk `dir` and call `read_new_lines` on every `.jsonl` file.
fn read_dir_initial(
    db: &Database,
    dir: &Path,
    source: &str,
    positions: &mut HashMap<PathBuf, u64>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).context("failed to read directory")? {
        let path = entry?.path();
        if path.is_dir() {
            if let Err(e) = read_dir_initial(db, &path, source, positions) {
                warn!(
                    "Capture: skipping subdirectory {}: {:#}",
                    path.display(),
                    e
                );
            }
        } else if is_jsonl(&path) {
            if let Err(e) = read_new_lines(db, &path, source, positions) {
                warn!("Capture: skipping file {}: {:#}", path.display(), e);
            }
        }
    }
    Ok(())
}

/// Read bytes appended to `path` since the last call, parse each line as a
/// JSON object, and persist valid lines to SQLite.
///
/// Uses a byte-offset map (`positions`) so each line is persisted exactly
/// once regardless of how many events arrive.
fn read_new_lines(
    db: &Database,
    path: &Path,
    source: &str,
    positions: &mut HashMap<PathBuf, u64>,
) -> Result<()> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    let file_size = file.metadata()?.len();
    let last_pos = *positions.entry(path.to_path_buf()).or_insert(0);

    if file_size <= last_pos {
        // Nothing new, or the file was truncated/rotated — reset position.
        if file_size < last_pos {
            positions.insert(path.to_path_buf(), 0);
        }
        return Ok(());
    }

    file.seek(SeekFrom::Start(last_pos))?;

    let new_len = (file_size - last_pos) as usize;
    let mut buf = vec![0u8; new_len];
    file.read_exact(&mut buf)?;

    positions.insert(path.to_path_buf(), file_size);

    let text = String::from_utf8_lossy(&buf);
    let mut count = 0usize;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Only persist valid JSON objects — skip malformed / partial lines.
        if let Ok(Value::Object(_)) = serde_json::from_str::<Value>(line) {
            persist_artefact(db, source, line)
                .with_context(|| format!("failed to persist artefact from {}", path.display()))?;
            count += 1;
        }
    }

    if count > 0 {
        debug!(
            "Capture: persisted {} artefact(s) from {}",
            count,
            path.display()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Insert one raw artefact into the `artefacts` table.
fn persist_artefact(db: &Database, source: &str, raw_content: &str) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let captured_at = chrono::Utc::now().to_rfc3339();

    let conn = db.conn();
    let conn = conn.lock().unwrap();

    conn.execute(
        "INSERT INTO artefacts (id, source, captured_at, raw_content) \
         VALUES (?1, ?2, ?3, ?4)",
        params![id, source, captured_at, raw_content],
    )
    .context("failed to insert artefact into database")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Initialise the Capture module.
pub fn init() {}
