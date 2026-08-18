//! Database open/init/migration wrapper.
//!
//! Wraps a single `rusqlite::Connection`. NEXORA is a desktop app with one
//! writer, so a single pooled connection guarded by the app state is sufficient;
//! WAL mode keeps reads snappy during background scans (Phase 2+).

mod schema;

pub use schema::SCHEMA_VERSION;

use crate::Result;
use rusqlite::Connection;
use std::path::Path;

/// Owns the SQLite connection and enforces schema version on open.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (creating if needed) the DB at `path` and apply migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Database> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// In-memory DB for tests.
    pub fn open_in_memory() -> Result<Database> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Database> {
        // Pragmas tuned for a local desktop workload.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        // Migrate while we still own the connection exclusively.
        Self::migrate(&mut conn)?;
        Ok(Database { conn })
    }

    /// Apply forward migrations based on `PRAGMA user_version`.
    fn migrate(conn: &mut Connection) -> Result<()> {
        let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if current < 1 {
            let tx = conn.transaction()?;
            tx.execute_batch(schema::SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        // Future: `if current < 2 { ... }` numbered migrations.
        Ok(())
    }

    /// Borrow the raw connection (read/write).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The applied schema version.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_sets_schema_version() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn expected_tables_exist() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name IN
                 ('assets','textures','materials','material_maps','texture_sets',
                  'texture_maps','udim_tiles','tags','asset_tags','collections',
                  'collection_assets','previews','file_hashes','usage_history',
                  'settings','renderer_presets','material_versions')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 17, "all core tables should be created");
    }

    #[test]
    fn fts_search_finds_asset() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let now = 1_700_000_000i64;
        conn.execute(
            "INSERT INTO assets (id, kind, name, category, created_at, updated_at)
             VALUES ('NX-MAT-0001-0002','material','Concrete Industrial','Concrete',?1,?1)",
            [now],
        )
        .unwrap();

        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM assets_fts WHERE assets_fts MATCH 'concrete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }

    #[test]
    fn migration_is_idempotent_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexora.db");
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        }
        // Reopen: migrate() should be a no-op and not error.
        let db2 = Database::open(&path).unwrap();
        assert_eq!(db2.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
