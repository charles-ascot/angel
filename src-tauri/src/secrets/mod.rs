//! Secrets module — macOS Keychain wrapper for credential access.
//!
//! All secrets are stored in the macOS Keychain. No secrets are ever stored
//! in environment variables, config files, or source code.
//!
//! Keychain items:
//! - `angel-gcp-key`: GCS service account JSON credentials
//! - `angel-anthropic-key`: Anthropic API key

use anyhow::{Context, Result};
use keyring::Entry;

/// Email of the GCP service account.
pub const GCP_SERVICE_ACCOUNT: &str = "charlies-angel@chiops.iam.gserviceaccount.com";

/// Keychain service name for GCP credentials.
pub const KEYCHAIN_GCP_SERVICE: &str = "angel-gcp-key";

/// Keychain service name for the Anthropic API key.
pub const KEYCHAIN_ANTHROPIC_SERVICE: &str = "angel-anthropic-key";

/// Keychain account name for the Anthropic API key.
pub const KEYCHAIN_ANTHROPIC_ACCOUNT: &str = "angel";

/// Retrieve the GCP service account JSON from the macOS Keychain.
///
/// Returns an error if the item is absent or empty — caller should log
/// a warning and disable GCS archiving rather than crashing.
pub fn get_gcp_credentials() -> Result<String> {
    let entry = Entry::new(KEYCHAIN_GCP_SERVICE, GCP_SERVICE_ACCOUNT)
        .context("failed to create keychain entry for GCP credentials")?;
    let creds = entry
        .get_password()
        .context("GCP credentials not found in keychain — run setup first")?;
    if creds.is_empty() {
        anyhow::bail!("GCP credentials keychain item exists but is empty");
    }
    Ok(creds)
}

/// Retrieve the Anthropic API key from the macOS Keychain.
///
/// Returns an error if the item is absent or empty.
pub fn get_anthropic_key() -> Result<String> {
    let entry = Entry::new(KEYCHAIN_ANTHROPIC_SERVICE, KEYCHAIN_ANTHROPIC_ACCOUNT)
        .context("failed to create keychain entry for Anthropic API key")?;
    let key = entry
        .get_password()
        .context("Anthropic API key not found in keychain — run setup first")?;
    if key.is_empty() {
        anyhow::bail!("Anthropic API key keychain item exists but is empty");
    }
    Ok(key)
}

/// Initialise the Secrets module.
pub fn init() {}
