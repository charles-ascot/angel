//! Secrets module — macOS Keychain wrapper for credential access.
//!
//! All secrets are stored in the macOS Keychain. No secrets are ever stored
//! in environment variables, config files, or source code.
//!
//! Keychain items:
//! - `angel-gcp-key`: GCS service account JSON credentials
//! - `angel-anthropic-key`: Anthropic API key

/// Keychain service name for GCP credentials.
pub const KEYCHAIN_GCP_SERVICE: &str = "angel-gcp-key";

/// Keychain service name for the Anthropic API key.
pub const KEYCHAIN_ANTHROPIC_SERVICE: &str = "angel-anthropic-key";

/// Initialise the Secrets module.
pub fn init() {}
