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
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if current < 2 {
            // Backfill V-Ray/Arnold renderer availability for materials created
            // before renderer presets recorded them (see schema::MIGRATE_V2).
            let tx = conn.transaction()?;
            tx.execute_batch(schema::MIGRATE_V2_RENDERER_PRESETS)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if current < 3 {
            // Add the local `users` table backing the offline app lock (auth.rs).
            let tx = conn.transaction()?;
            tx.execute_batch(schema::MIGRATE_V3_USERS)?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        // Future: `if current < 4 { ... }` numbered migrations.
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
                  'settings','renderer_presets','material_versions','users')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 18, "all core tables should be created");
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

    #[test]
    fn v2_backfills_renderer_presets_for_existing_materials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexora.db");

        // Simulate a pre-v2 library: a material with a base color that only ever
        // recorded a generic_pbr preset, and user_version pinned at 1.
        {
            let db = Database::open(&path).unwrap();
            let conn = db.conn();
            let now = 1_700_000_000i64;
            conn.execute(
                "INSERT INTO assets (id, kind, name, created_at, updated_at)
                 VALUES ('NX-MAT-AAAA-BBBB','material','Old Concrete',?1,?1)",
                [now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO assets (id, kind, name, created_at, updated_at)
                 VALUES ('NX-TEX-CCCC-DDDD','texture','Old Concrete BaseColor',?1,?1)",
                [now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO textures (asset_id, file_path, map_type, created_at)
                 VALUES ('NX-TEX-CCCC-DDDD','/x/basecolor.png','base_color',?1)",
                [now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO materials (asset_id, is_pbr, is_udim, health, status)
                 VALUES ('NX-MAT-AAAA-BBBB',1,0,100,'healthy')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO material_maps (material_id, slot, texture_id)
                 VALUES ('NX-MAT-AAAA-BBBB','base_color','NX-TEX-CCCC-DDDD')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO renderer_presets (material_id, renderer, params)
                 VALUES ('NX-MAT-AAAA-BBBB','generic_pbr',NULL)",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }

        // Reopen → the v2 migration runs and backfills V-Ray + Arnold.
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let renderers: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM renderer_presets
                 WHERE material_id = 'NX-MAT-AAAA-BBBB'
                   AND renderer IN ('vray','arnold')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(renderers, 2, "v2 should backfill vray + arnold presets");
    }
}
