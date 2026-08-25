//! Shared application state held by Tauri and injected into commands.

use nexora_core::bridge::{MayaLink, Outbox};
use nexora_core::db::Database;
use nexora_core::providers::SyncProgress;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Shared, cloneable handle to the single database.
pub type Db = Arc<Mutex<Database>>;

/// App-wide state. The database sits behind `Arc<Mutex<…>>` so the background
/// import thread and the localhost Bridge API server can share it with the
/// foreground commands. The bridge outbox + Maya link are also shared so the
/// desktop "Send to Maya" action and connection status stay in sync.
pub struct AppState {
    pub db: Db,
    /// Rebuildable thumbnail cache directory (spec §50).
    pub thumbnail_dir: PathBuf,
    /// Assets queued to send into Maya (drained by the plug-in via /api/pull).
    pub outbox: Outbox,
    /// Liveness of the connected Maya, updated by heartbeats.
    pub maya: Arc<Mutex<MayaLink>>,
    /// Bridge auth token (shared with the plug-in via the config file).
    pub bridge_token: String,
    /// The port the Bridge API bound to.
    pub bridge_port: u16,
    /// Discover (free-texture sync): set to stop a running sync.
    pub discover_stop: Arc<AtomicBool>,
    /// True while a Discover sync thread is running (guards double-start).
    pub discover_running: Arc<AtomicBool>,
    /// Latest sync progress, read by the status command.
    pub discover_progress: Arc<Mutex<SyncProgress>>,
    /// True while a library scan is running (guards overlapping scans from the
    /// manual "Scan now" and the auto-scan timer).
    pub scan_running: Arc<AtomicBool>,
}
