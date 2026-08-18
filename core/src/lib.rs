//! # NEXORA Core
//!
//! The renderer- and GUI-agnostic engine behind NEXORA. Everything the desktop
//! app needs to store, identify, and classify assets lives here so it can be
//! unit-tested without Tauri, Maya, or a running window.
//!
//! Phase 1 surface:
//! - [`ids`]      — immutable, human-readable asset IDs (`NX-MAT-7F91-A2C8`).
//! - [`db`]       — SQLite schema + migrations for the full asset graph.
//! - [`settings`] — typed application/library settings persisted in the DB.
//! - [`maptypes`] — configurable texture-map recognition registry (foundation
//!                  for Phase 2 import; not yet wired to an importer).
//! - [`models`]   — serde types shared with the frontend over IPC.

pub mod bridge;
pub mod db;
pub mod ids;
pub mod library;
pub mod maptypes;
pub mod material;
pub mod models;
pub mod settings;
pub mod texture;

use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Every fallible operation in the core surfaces one of these.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),
}

/// The build version of the core engine, surfaced to the UI and API `/status`.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
