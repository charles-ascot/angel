//! Secrets module — reads credentials from the macOS Keychain via the
//! `security` CLI tool.
//!
//! Using `security` instead of the `keyring` crate because the `keyring`
//! crate's macOS backend relies on the deprecated `SecKeychain` API which
//! silently fails on macOS 15+. The `security` CLI uses the modern SecItem
//! API and is always available on macOS.
//!
//! Keychain items (stored with `security add-generic-password`):
//! - service `angel-gcp-key`,       account `charlies-angel@chiops.iam.gserviceaccount.com`
//! - service `angel-anthropic-key`, account `angel`

use anyhow::{Context, Result};
use std::process::Command;

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
    keychain_read(KEYCHAIN_GCP_SERVICE, GCP_SERVICE_ACCOUNT)
        .context("GCP credentials not found in keychain — run setup first")
}

/// Retrieve the Anthropic API key from the macOS Keychain.
///
/// Returns an error if the item is absent or empty.
pub fn get_anthropic_key() -> Result<String> {
    keychain_read(KEYCHAIN_ANTHROPIC_SERVICE, KEYCHAIN_ANTHROPIC_ACCOUNT)
        .context("Anthropic API key not found in keychain — run setup first")
}

/// Read a generic password from the login keychain using the `security` CLI.
///
/// Equivalent to:
///   `security find-generic-password -s SERVICE -a ACCOUNT -w`
fn keychain_read(service: &str, account: &str) -> Result<String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s", service,
            "-a", account,
            "-w",
        ])
        .output()
        .context("failed to invoke `security` CLI — is Xcode Command Line Tools installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("No matching entry found in keychain. {}", stderr.trim());
    }

    let secret = String::from_utf8(output.stdout)
        .context("keychain value is not valid UTF-8")?
        .trim()
        .to_string();

    if secret.is_empty() {
        anyhow::bail!("keychain item exists but is empty");
    }

    Ok(secret)
}

/// Initialise the Secrets module.
pub fn init() {}
