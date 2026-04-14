//! Storage module — manages SQLite (local operational store) and Google Cloud
//! Storage (long-term archive).
//!
//! SQLite is the primary operational store, located at:
//!   `~/Library/Application Support/Angel/angel.db`
//!
//! GCS configuration:
//! - Bucket:  `charlies-angel`
//! - Project: `CHIOPS`
//! - Region:  `eu` (multi-region)
//! - Service account: `charlies-angel@chiops.iam.gserviceaccount.com`
//!
//! Angel writes to GCS asynchronously and best-effort via an unbounded
//! in-memory channel drained by a background task. Failures are logged
//! but never propagated to the caller. Angel never reads from GCS at runtime.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// --- Constants -----------------------------------------------------------

/// GCS bucket name.
pub const GCS_BUCKET: &str = "charlies-angel";

/// GCP project ID.
pub const GCS_PROJECT: &str = "CHIOPS";

/// GCS region.
pub const GCS_REGION: &str = "eu";

/// Local SQLite database filename.
pub const LOCAL_DB_NAME: &str = "angel.db";

/// Scopes required for GCS object writes.
const GCS_SCOPES: &[&str] = &["https://www.googleapis.com/auth/devstorage.read_write"];

// --- Database ------------------------------------------------------------

/// Thread-safe handle to the local SQLite database.
///
/// Clone is cheap — all clones share the same underlying connection.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Open (or create) the database at `app_data_dir/angel.db` and
    /// initialise the schema. Creates the directory if it does not exist.
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_data_dir)
            .context("failed to create app data directory")?;

        let db_path = app_data_dir.join(LOCAL_DB_NAME);
        info!("Opening database at {}", db_path.display());

        let conn = Connection::open(&db_path)
            .context("failed to open SQLite database")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(SCHEMA_SQL)
            .context("failed to initialise database schema")?;
        info!("Database schema initialised");
        Ok(())
    }

    /// Acquire the underlying connection for queries.
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }
}

/// SQL schema — tables and indexes created on first launch.
///
/// WAL mode for concurrent readers; foreign keys enforced.
const SCHEMA_SQL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Raw artefacts as captured by the Capture module.
-- Every artefact must be written here before any other processing.
CREATE TABLE IF NOT EXISTS artefacts (
    id          TEXT PRIMARY KEY,
    source      TEXT NOT NULL,        -- 'claude_code' | 'claude_ai' | 'gmail' | 'git' | 'filesystem'
    captured_at TEXT NOT NULL,        -- ISO-8601 UTC
    raw_content TEXT NOT NULL,        -- JSON blob
    classified  INTEGER NOT NULL DEFAULT 0,  -- 0 = pending, 1 = done
    gcs_synced  INTEGER NOT NULL DEFAULT 0   -- 0 = pending, 1 = uploaded
);

-- Structured output from the Classify module.
CREATE TABLE IF NOT EXISTS classified_artefacts (
    id                TEXT PRIMARY KEY,
    artefact_id       TEXT NOT NULL REFERENCES artefacts(id),
    classified_at     TEXT NOT NULL,
    stream_tag        TEXT,
    related_component TEXT,
    decisions         TEXT,    -- JSON array of strings
    open_questions    TEXT,    -- JSON array of strings
    status_changes    TEXT,    -- JSON array of strings
    cross_references  TEXT     -- JSON array of artefact IDs
);

-- State model: what is true right now across all watched sources.
CREATE TABLE IF NOT EXISTS work_items (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    description   TEXT,
    status        TEXT NOT NULL CHECK(status IN ('active', 'blocked', 'completed')),
    stream_tag    TEXT,
    deep_link     TEXT NOT NULL,   -- clickable link back to the source; NEVER empty
    first_seen_at TEXT NOT NULL,
    last_seen_at  TEXT NOT NULL
);

-- Record of every pushback notification surfaced to the user.
CREATE TABLE IF NOT EXISTS pushback_log (
    id                   TEXT PRIMARY KEY,
    triggered_at         TEXT NOT NULL,
    new_session_path     TEXT NOT NULL,
    matched_work_item_id TEXT REFERENCES work_items(id),
    escalation_level     TEXT NOT NULL CHECK(escalation_level IN ('registry', 'notification', 'halt')),
    deep_link            TEXT NOT NULL   -- always present; a notification without a link is forbidden
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_work_items_status
    ON work_items(status);
CREATE INDEX IF NOT EXISTS idx_artefacts_source
    ON artefacts(source, captured_at);
CREATE INDEX IF NOT EXISTS idx_artefacts_unclassified
    ON artefacts(classified) WHERE classified = 0;
CREATE INDEX IF NOT EXISTS idx_artefacts_unsynced
    ON artefacts(gcs_synced) WHERE gcs_synced = 0;
";

// --- GCS Writer ----------------------------------------------------------

struct GcsPayload {
    object_name: String,
    data: Vec<u8>,
}

enum GcsWriterInner {
    Active(mpsc::UnboundedSender<GcsPayload>),
    Disabled,
}

/// Async, best-effort GCS writer.
///
/// Enqueue writes with [`GcsWriter::write_async`] — the call returns
/// immediately and the background task handles the actual upload.
/// Failures are logged but never returned to the caller.
pub struct GcsWriter(GcsWriterInner);

impl GcsWriter {
    /// Spawn the background upload task using the provided service account
    /// JSON (retrieved from the macOS Keychain).
    pub fn spawn(credentials_json: String) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(gcs_upload_loop(rx, credentials_json));
        Self(GcsWriterInner::Active(tx))
    }

    /// No-op writer used when GCP credentials are unavailable.
    pub fn disabled() -> Self {
        Self(GcsWriterInner::Disabled)
    }

    /// Queue `data` for upload to `object_name` in the GCS bucket.
    ///
    /// Returns immediately — never blocks the caller. If the writer is
    /// disabled the call is silently dropped.
    pub fn write_async(&self, object_name: impl Into<String>, data: Vec<u8>) {
        match &self.0 {
            GcsWriterInner::Active(tx) => {
                let payload = GcsPayload {
                    object_name: object_name.into(),
                    data,
                };
                if let Err(e) = tx.send(payload) {
                    warn!("GCS writer channel closed, dropping payload: {}", e);
                }
            }
            GcsWriterInner::Disabled => {
                debug!("GCS writer disabled, skipping upload");
            }
        }
    }
}

async fn gcs_upload_loop(
    mut rx: mpsc::UnboundedReceiver<GcsPayload>,
    credentials_json: String,
) {
    let sa = match gcp_auth::CustomServiceAccount::from_json(&credentials_json) {
        Ok(sa) => sa,
        Err(e) => {
            error!(
                "Failed to parse GCP service account JSON — GCS uploads disabled: {}",
                e
            );
            return;
        }
    };
    let auth = gcp_auth::AuthenticationManager::from(sa);

    let client = reqwest::Client::new();

    while let Some(payload) = rx.recv().await {
        if let Err(e) = upload_object(&client, &auth, &payload).await {
            warn!(
                "GCS upload failed for '{}' — continuing: {:#}",
                payload.object_name, e
            );
        } else {
            debug!("GCS upload OK: {}", payload.object_name);
        }
    }
}

async fn upload_object(
    client: &reqwest::Client,
    auth: &gcp_auth::AuthenticationManager,
    payload: &GcsPayload,
) -> Result<()> {
    let token = auth
        .get_token(GCS_SCOPES)
        .await
        .context("failed to obtain GCS access token")?;

    let url = format!(
        "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}",
        GCS_BUCKET, payload.object_name,
    );

    let resp = client
        .post(&url)
        .bearer_auth(token.as_str())
        .header("Content-Type", "application/octet-stream")
        .body(payload.data.clone())
        .send()
        .await
        .context("GCS HTTP request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GCS returned {}: {}", status, body);
    }

    Ok(())
}

/// Initialise the Storage module.
pub fn init() {}
