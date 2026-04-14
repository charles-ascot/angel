mod anthropic;
mod capture;
mod classify;
mod pushback;
mod secrets;
mod state_model;
mod storage;

use storage::{Database, GcsWriter};
use tauri::Manager;

/// Path where Claude Code project transcripts are stored.
pub const CLAUDE_CODE_LOG_PATH: &str = "~/.claude/projects/";

/// Path to the Claude Code global history file.
pub const CLAUDE_CODE_HISTORY_PATH: &str = "~/.claude/history.jsonl";

/// Application state shared across all Tauri commands and background tasks.
pub struct AppState {
    pub db: Database,
    pub gcs: GcsWriter,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|app| {
            // Resolve the platform app data directory:
            //   macOS: ~/Library/Application Support/Angel/
            let app_data_dir = app.path().app_data_dir()?;

            // Open (or create) the local SQLite database.
            let db = Database::open(&app_data_dir)?;

            // Retrieve GCP credentials and start the GCS archive writer.
            // If credentials are missing we degrade gracefully — archiving is
            // best-effort and must never prevent the app from starting.
            let gcs = match secrets::get_gcp_credentials() {
                Ok(creds) => {
                    tracing::info!("GCP credentials loaded, GCS archiving active");
                    GcsWriter::spawn(creds)
                }
                Err(e) => {
                    tracing::warn!(
                        "GCP credentials unavailable, GCS archiving disabled: {:#}",
                        e
                    );
                    GcsWriter::disabled()
                }
            };

            app.manage(AppState { db, gcs });
            tracing::info!("Angel initialised");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Angel");
}
