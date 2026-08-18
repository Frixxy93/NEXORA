//! Serde DTOs shared with the frontend/API.
//!
//! Phase 1 only needs a few read models (dashboard counts, library/connection
//! status). Full asset records arrive in Phase 2+ as the importer lands.

use serde::{Deserialize, Serialize};

/// Home-dashboard headline counts (spec §4).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryStats {
    pub materials: u64,
    pub textures: u64,
    pub texture_sets: u64,
    pub favorites: u64,
    pub recently_added: u64,
}

/// Aggregate library-health snapshot (spec §30). Phase 1 returns zeros until the
/// scanner exists; the shape is fixed now so the UI never changes later.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryHealth {
    pub assets: u64,
    pub healthy: u64,
    pub missing_files: u64,
    pub duplicates: u64,
    pub incomplete_materials: u64,
    pub broken_references: u64,
}

/// Whether the local library is configured and reachable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryStatus {
    pub configured: bool,
    pub location: Option<String>,
    /// True if the configured location exists on disk.
    pub reachable: bool,
    pub storage_mode: String,
}

/// Maya bridge connectivity (spec §4 "Maya connection status"). Phase 1 always
/// reports disconnected; the Bridge server lands in Phase 7.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MayaStatus {
    pub connected: bool,
    pub version: Option<String>,
    pub bridge_port: Option<u16>,
}
