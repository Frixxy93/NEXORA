//! The SQLite schema for NEXORA (spec §42).
//!
//! Design notes:
//! - `assets` is the shared spine: every material, texture, and texture-set has
//!   a row here keyed by its immutable `NX-...` id. `textures` and `materials`
//!   hold kind-specific columns and reference `assets(id)` 1:1.
//! - Relationships (a material's maps, a set's members, collection membership,
//!   tags) are join tables that reference asset **ids**, never file paths — so a
//!   texture can be reused by many materials without duplicating files (§44).
//! - `assets_fts` is an FTS5 index kept in sync by triggers for fast search.
//!
//! All statements are idempotent (`IF NOT EXISTS`) so applying the schema to an
//! existing DB is safe. Structural changes go through numbered migrations.

/// Current schema version, stored in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 3;

/// The full v1 schema, applied inside a single transaction.
pub const SCHEMA_V1: &str = r#"
-- ---------------------------------------------------------------------------
-- Core spine
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS assets (
    id           TEXT PRIMARY KEY,              -- NX-MAT-.. / NX-TEX-.. / NX-SET-..
    kind         TEXT NOT NULL CHECK (kind IN ('material','texture','texture_set')),
    name         TEXT NOT NULL,
    category     TEXT,
    description  TEXT,
    favorite     INTEGER NOT NULL DEFAULT 0,     -- 0/1
    created_at   INTEGER NOT NULL,               -- unix seconds
    updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_assets_kind      ON assets(kind);
CREATE INDEX IF NOT EXISTS idx_assets_category  ON assets(category);
CREATE INDEX IF NOT EXISTS idx_assets_favorite  ON assets(favorite);
CREATE INDEX IF NOT EXISTS idx_assets_created   ON assets(created_at);

-- ---------------------------------------------------------------------------
-- Textures (first-class; may exist with no material) — spec §6
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS textures (
    asset_id     TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    file_path    TEXT NOT NULL,
    map_type     TEXT,                            -- slug from MapTypeRegistry
    width        INTEGER,
    height       INTEGER,
    format       TEXT,                            -- jpg/png/exr/...
    channels     INTEGER,
    color_space  TEXT,                            -- srgb/linear/raw
    file_size    INTEGER,                         -- bytes
    is_udim      INTEGER NOT NULL DEFAULT 0,
    tileable     INTEGER,                         -- 0/1/NULL (unknown)
    managed      INTEGER NOT NULL DEFAULT 1,      -- 1 managed, 0 referenced (§47)
    created_at   INTEGER NOT NULL,
    modified_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_textures_map     ON textures(map_type);
CREATE INDEX IF NOT EXISTS idx_textures_format  ON textures(format);
CREATE INDEX IF NOT EXISTS idx_textures_udim    ON textures(is_udim);
CREATE UNIQUE INDEX IF NOT EXISTS idx_textures_path ON textures(file_path);

-- ---------------------------------------------------------------------------
-- Materials — spec §5
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS materials (
    asset_id     TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    folder_path  TEXT,                            -- source dir if imported as one
    is_pbr       INTEGER NOT NULL DEFAULT 1,
    tileable     INTEGER,
    is_udim      INTEGER NOT NULL DEFAULT 0,
    resolution   TEXT,                            -- friendly label e.g. "4K"
    health       INTEGER NOT NULL DEFAULT 100,    -- 0..100 (§31)
    status       TEXT NOT NULL DEFAULT 'healthy'  -- healthy/incomplete/missing/broken
);

-- Which texture fills each slot of a material — references texture ids (§44)
CREATE TABLE IF NOT EXISTS material_maps (
    material_id  TEXT NOT NULL REFERENCES materials(asset_id) ON DELETE CASCADE,
    slot         TEXT NOT NULL,                   -- base_color/roughness/normal/...
    texture_id   TEXT REFERENCES textures(asset_id) ON DELETE SET NULL,
    PRIMARY KEY (material_id, slot)
);
CREATE INDEX IF NOT EXISTS idx_material_maps_tex ON material_maps(texture_id);

-- Renderer availability/presets per material (§37/§38)
CREATE TABLE IF NOT EXISTS renderer_presets (
    material_id  TEXT NOT NULL REFERENCES materials(asset_id) ON DELETE CASCADE,
    renderer     TEXT NOT NULL,                   -- generic_pbr/vray/arnold
    params       TEXT,                            -- JSON blob of adapter params
    PRIMARY KEY (material_id, renderer)
);

-- Material history for future versioning (§42 material_versions)
CREATE TABLE IF NOT EXISTS material_versions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    material_id  TEXT NOT NULL REFERENCES materials(asset_id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    snapshot     TEXT NOT NULL,                   -- JSON snapshot
    created_at   INTEGER NOT NULL
);

-- ---------------------------------------------------------------------------
-- Texture sets — spec §11 (relationship over individual textures)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS texture_sets (
    asset_id     TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    resolution   TEXT,
    is_pbr       INTEGER NOT NULL DEFAULT 0,
    tileable     INTEGER
);

CREATE TABLE IF NOT EXISTS texture_maps (
    set_id       TEXT NOT NULL REFERENCES texture_sets(asset_id) ON DELETE CASCADE,
    slot         TEXT NOT NULL,                   -- base_color/roughness/...
    texture_id   TEXT NOT NULL REFERENCES textures(asset_id) ON DELETE CASCADE,
    PRIMARY KEY (set_id, slot)
);

-- UDIM tiles belonging to a texture (§12)
CREATE TABLE IF NOT EXISTS udim_tiles (
    texture_id   TEXT NOT NULL REFERENCES textures(asset_id) ON DELETE CASCADE,
    tile         INTEGER NOT NULL,                -- 1001, 1002, ...
    file_path    TEXT NOT NULL,
    PRIMARY KEY (texture_id, tile)
);

-- ---------------------------------------------------------------------------
-- Tags & collections — spec §19/§22
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tags (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    name  TEXT NOT NULL UNIQUE COLLATE NOCASE
);
CREATE TABLE IF NOT EXISTS asset_tags (
    asset_id  TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (asset_id, tag_id)
);

CREATE TABLE IF NOT EXISTS collections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    icon        TEXT,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS collection_assets (
    collection_id  INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    asset_id       TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    PRIMARY KEY (collection_id, asset_id)
);

-- ---------------------------------------------------------------------------
-- Previews, hashes, usage — spec §14/§27/§42
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS previews (
    asset_id     TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    preview_path TEXT NOT NULL,                   -- cached thumbnail/preview file
    kind         TEXT NOT NULL DEFAULT 'thumbnail',
    generated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS file_hashes (
    texture_id  TEXT PRIMARY KEY REFERENCES textures(asset_id) ON DELETE CASCADE,
    hash        TEXT NOT NULL,                    -- content hash (e.g. blake3/sha256)
    algo        TEXT NOT NULL DEFAULT 'blake3'
);
CREATE INDEX IF NOT EXISTS idx_file_hashes_hash ON file_hashes(hash);

CREATE TABLE IF NOT EXISTS usage_history (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id   TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    action     TEXT NOT NULL,                     -- viewed/sent_to_maya/applied
    at         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_asset ON usage_history(asset_id);
CREATE INDEX IF NOT EXISTS idx_usage_at    ON usage_history(at);

-- ---------------------------------------------------------------------------
-- Settings — single JSON row keyed by 'app' (see settings.rs)
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

-- ---------------------------------------------------------------------------
-- Full-text search over assets — spec §17/§42 (FTS5)
-- ---------------------------------------------------------------------------
CREATE VIRTUAL TABLE IF NOT EXISTS assets_fts USING fts5(
    name,
    category,
    description,
    tags,
    content=''                                    -- external-content-less index
);

-- Keep the FTS index in step with `assets`. Tags are added by the app layer on
-- tag changes; name/category/description are covered by these triggers.
CREATE TRIGGER IF NOT EXISTS assets_ai AFTER INSERT ON assets BEGIN
    INSERT INTO assets_fts(rowid, name, category, description, tags)
    VALUES (new.rowid, new.name, COALESCE(new.category,''), COALESCE(new.description,''), '');
END;
CREATE TRIGGER IF NOT EXISTS assets_ad AFTER DELETE ON assets BEGIN
    INSERT INTO assets_fts(assets_fts, rowid, name, category, description, tags)
    VALUES('delete', old.rowid, old.name, COALESCE(old.category,''), COALESCE(old.description,''), '');
END;
CREATE TRIGGER IF NOT EXISTS assets_au AFTER UPDATE ON assets BEGIN
    INSERT INTO assets_fts(assets_fts, rowid, name, category, description, tags)
    VALUES('delete', old.rowid, old.name, COALESCE(old.category,''), COALESCE(old.description,''), '');
    INSERT INTO assets_fts(rowid, name, category, description, tags)
    VALUES (new.rowid, new.name, COALESCE(new.category,''), COALESCE(new.description,''), '');
END;
"#;

/// v2 — renderer availability backfill.
///
/// Early builds wrote only a `generic_pbr` preset per material, so the V-Ray and
/// Arnold library views (which filter on `renderer_presets`) and the inspector
/// chips were always empty. NEXORA's V-Ray and Arnold adapters build a surface
/// shader anchored on the base color, so any material with a base color supports
/// them. Backfill those rows for existing materials. Idempotent via OR IGNORE.
pub const MIGRATE_V2_RENDERER_PRESETS: &str = r#"
INSERT OR IGNORE INTO renderer_presets (material_id, renderer, params)
SELECT DISTINCT material_id, 'vray', NULL FROM material_maps WHERE slot = 'base_color';
INSERT OR IGNORE INTO renderer_presets (material_id, renderer, params)
SELECT DISTINCT material_id, 'arnold', NULL FROM material_maps WHERE slot = 'base_color';
"#;

/// v3 — local user accounts for the app lock (see auth.rs).
///
/// Credentials for NEXORA's offline login live entirely in the local DB. The
/// password is never stored in plaintext: `password_hash` holds an Argon2id PHC
/// string (algorithm, parameters, salt, and hash together). No server, no
/// network — the whole auth system is self-contained on the user's machine.
pub const MIGRATE_V3_USERS: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,                    -- Argon2id PHC string
    created_at    INTEGER NOT NULL,
    last_login    INTEGER
);
"#;
