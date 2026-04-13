//! Storage module — manages SQLite (local operational store) and Google Cloud
//! Storage (long-term archive).
//!
//! SQLite is the primary operational store at:
//!   `~/Library/Application Support/Angel/angel.db`
//!
//! GCS configuration:
//! - Bucket: `angel`
//! - Project: `chimera`
//! - Region: `europe-west1`
//! - Service account: `angel@chimera.iam.gserviceaccount.com`
//!
//! Angel writes to GCS asynchronously and best-effort. Angel never reads
//! from GCS at runtime.

/// GCS bucket name.
pub const GCS_BUCKET: &str = "charlies-angel";

/// GCP project ID.
pub const GCS_PROJECT: &str = "CHIOPS";

/// GCS region.
pub const GCS_REGION: &str = "eu";

/// Local SQLite database path (relative to Application Support).
pub const LOCAL_DB_NAME: &str = "angel.db";

/// Initialise the Storage module.
pub fn init() {}
