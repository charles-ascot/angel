//! Pushback module — intercepts new Claude Code session starts via filesystem
//! watch and runs similarity checks against the State Model.
//!
//! On strong matches, surfaces a native macOS notification with a clickable
//! deep link to the related work. Every notification *must* include a
//! clickable deep link — "there is a related chat" without a link is
//! forbidden.
//!
//! Three escalation rungs:
//! 1. Passive registry note
//! 2. Session-start notification (default)
//! 3. Hard halt (rare)

/// Initialise the Pushback module.
pub fn init() {}
