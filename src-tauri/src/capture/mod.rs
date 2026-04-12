//! Capture module — reads from external sources and persists raw artefacts.
//!
//! Sources include:
//! - Claude Code transcripts (`~/.claude/projects/**/*.jsonl`)
//! - Claude Code global history (`~/.claude/history.jsonl`)
//! - Claude desktop app data (`~/Library/Application Support/Claude/`)
//! - claude.ai web chats (mechanism TBD)
//! - Git activity on configured repositories
//! - Gmail via OAuth
//! - Configured filesystem paths
//!
//! Hard rule: every captured artefact is written to local SQLite *before*
//! anything else happens. This module never reasons or decides — it only
//! persists.

/// Initialise the Capture module.
pub fn init() {}
