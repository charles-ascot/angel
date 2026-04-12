//! Anthropic API client module — handles communication with the Anthropic API
//! for the reasoning engine.
//!
//! Model: Claude Opus 4.6 (`claude-opus-4-6`)
//!
//! API key is retrieved from the macOS Keychain via the `secrets` module.
//! This module never stores or caches the API key.

/// The Anthropic model string used for all reasoning calls.
pub const ANTHROPIC_MODEL: &str = "claude-opus-4-6";

/// Initialise the Anthropic client module.
pub fn init() {}
