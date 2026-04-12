mod anthropic;
mod capture;
mod classify;
mod pushback;
mod secrets;
mod state_model;
mod storage;

/// Path where Claude Code project transcripts are stored.
pub const CLAUDE_CODE_LOG_PATH: &str = "~/.claude/projects/";

/// Path to the Claude Code global history file.
pub const CLAUDE_CODE_HISTORY_PATH: &str = "~/.claude/history.jsonl";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|_app| {
            tracing::info!("Angel starting up");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Angel");
}
