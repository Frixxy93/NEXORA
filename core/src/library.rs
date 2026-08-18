//! Library features (spec §17–§22, §27, §30), Phase 5.
//!
//! Cross-cutting operations over the asset graph: full-text + tag search,
//! favorites, tags, collections, duplicate detection, and recent lists. These
//! reuse the texture/material read models and add the relationship tables the
//! schema already defines (tags, asset_tags, collections, collection_assets,
//! usage_history, file_hashes).

use crate::material::{self, MaterialDto};
use crate::texture::{self, TextureDto, TextureSetDto};
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A mixed bag of materials and textures (favorites, collection members, …).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MixedAssets {
    pub materials: Vec<MaterialDto>,
    pub textures: Vec<TextureDto>,
}

/// Global search results grouped by asset kind (spec §17).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResults {
    pub materials: Vec<MaterialDto>,
    pub textures: Vec<TextureDto>,
    pub sets: Vec<TextureSetDto>,
}

/// A tag with how many assets carry it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDto {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

/// A collection with its member count (spec §22).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDto {
    pub id: i64,
    pub name: String,
    pub icon: Option<String>,
    pub count: i64,
}

/// A set of textures with identical content (spec §27).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub textures: Vec<TextureDto>,
}

// ---------------------------------------------------------------------------
// Search (spec §17)
// ---------------------------------------------------------------------------

/// Build an FTS5 prefix query from free text: `"con wa"` → `"con* wa*"`.
fn fts_query(q: &str) -> Option<String> {
    let tokens: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|t| format!("{}*", t.to_lowercase()))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

/// Asset ids matching `q` by name/category/description (FTS) or by tag.
fn matching_ids(conn: &Connection, q: &str) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();

    if let Some(fts) = fts_query(q) {
        let mut stmt = conn.prepare(
            "SELECT a.id FROM assets a JOIN assets_fts f ON f.rowid = a.rowid
             WHERE assets_fts MATCH ?1",
        )?;
        let rows = stmt.query_map([&fts], |r| r.get::<_, String>(0))?;
        for r in rows {
            ids.insert(r?);
        }
    }

    let like = format!("%{}%", q.trim().to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT at.asset_id FROM asset_tags at JOIN tags t ON t.id = at.tag_id
         WHERE lower(t.name) LIKE ?1",
    )?;
    let rows = stmt.query_map([&like], |r| r.get::<_, String>(0))?;
    for r in rows {
        ids.insert(r?);
    }

    Ok(ids)
}

/// Search materials, textures, and sets by name/category/tags.
pub fn search(conn: &Connection, q: &str) -> Result<SearchResults> {
    if q.trim().is_empty() {
        return Ok(SearchResults::default());
    }
    let ids = matching_ids(conn, q)?;
    Ok(SearchResults {
        materials: material::list_materials(conn, None)?
            .into_iter()
            .filter(|m| ids.contains(&m.id))
            .collect(),
        textures: texture::list_textures(conn, None)?
            .into_iter()
            .filter(|t| ids.contains(&t.id))
            .collect(),
        sets: texture::list_texture_sets(conn)?
            .into_iter()
            .filter(|s| ids.contains(&s.id))
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Favorites (spec §21)
// ---------------------------------------------------------------------------

/// Toggle an asset's favorite flag.
pub fn set_favorite(conn: &Connection, id: &str, favorite: bool) -> Result<()> {
    conn.execute(
        "UPDATE assets SET favorite = ?2, updated_at = strftime('%s','now') WHERE id = ?1",
        params![id, favorite as i64],
    )?;
    Ok(())
}

/// All favorited materials and textures.
pub fn list_favorites(conn: &Connection) -> Result<MixedAssets> {
    Ok(MixedAssets {
        materials: material::list_materials(conn, None)?
            .into_iter()
            .filter(|m| m.favorite)
            .collect(),
        textures: texture::list_textures(conn, None)?
            .into_iter()
            .filter(|t| t.favorite)
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Recently added / used (spec §3 Smart)
// ---------------------------------------------------------------------------

/// The most recently added materials and textures (each capped at `limit`).
pub fn list_recent_added(conn: &Connection, limit: usize) -> Result<MixedAssets> {
    Ok(MixedAssets {
        materials: material::list_materials(conn, None)?
            .into_iter()
            .take(limit)
            .collect(),
        textures: texture::list_textures(conn, None)?
            .into_iter()
            .take(limit)
            .collect(),
    })
}

/// Record that an asset was used (viewed / sent to Maya / applied) — spec §42.
pub fn record_usage(conn: &Connection, id: &str, action: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_history (asset_id, action, at) VALUES (?1, ?2, strftime('%s','now'))",
        params![id, action],
    )?;
    Ok(())
}

/// Most recently used assets, distinct, newest first.
pub fn list_recent_used(conn: &Connection, limit: usize) -> Result<MixedAssets> {
    let mut stmt = conn.prepare(
        "SELECT asset_id FROM usage_history GROUP BY asset_id
         ORDER BY MAX(at) DESC LIMIT ?1",
    )?;
    let ordered: Vec<String> = stmt
        .query_map([limit as i64], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mats: HashMap<String, MaterialDto> = material::list_materials(conn, None)?
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    let texs: HashMap<String, TextureDto> = texture::list_textures(conn, None)?
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();

    let mut out = MixedAssets::default();
    for id in ordered {
        if let Some(m) = mats.get(&id) {
            out.materials.push(m.clone());
        } else if let Some(t) = texs.get(&id) {
            out.textures.push(t.clone());
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tags (spec §19)
// ---------------------------------------------------------------------------

/// All tags with usage counts.
pub fn list_tags(conn: &Connection) -> Result<Vec<TagDto>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, COUNT(at.asset_id)
         FROM tags t LEFT JOIN asset_tags at ON at.tag_id = t.id
         GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TagDto {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Tags on one asset (counts are not computed here; set to 0).
pub fn tags_for_asset(conn: &Connection, asset_id: &str) -> Result<Vec<TagDto>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name FROM tags t JOIN asset_tags at ON at.tag_id = t.id
         WHERE at.asset_id = ?1 ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([asset_id], |r| {
        Ok(TagDto {
            id: r.get(0)?,
            name: r.get(1)?,
            count: 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Add a tag to an asset (creating the tag if needed). Idempotent.
pub fn add_tag(conn: &Connection, asset_id: &str, name: &str) -> Result<TagDto> {
    let name = name.trim().trim_start_matches('#').trim();
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [name])?;
    let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?1", [name], |r| r.get(0))?;
    conn.execute(
        "INSERT OR IGNORE INTO asset_tags (asset_id, tag_id) VALUES (?1, ?2)",
        params![asset_id, tag_id],
    )?;
    Ok(TagDto {
        id: tag_id,
        name: name.to_string(),
        count: 0,
    })
}

/// Remove a tag from an asset (the tag itself remains).
pub fn remove_tag(conn: &Connection, asset_id: &str, tag_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM asset_tags WHERE asset_id = ?1 AND tag_id = ?2",
        params![asset_id, tag_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Collections (spec §22)
// ---------------------------------------------------------------------------

/// Create a collection, returning it.
pub fn create_collection(conn: &Connection, name: &str, icon: Option<&str>) -> Result<CollectionDto> {
    conn.execute(
        "INSERT INTO collections (name, icon, created_at) VALUES (?1, ?2, strftime('%s','now'))",
        params![name, icon],
    )?;
    let id = conn.last_insert_rowid();
    Ok(CollectionDto {
        id,
        name: name.to_string(),
        icon: icon.map(|s| s.to_string()),
        count: 0,
    })
}

/// All collections with member counts.
pub fn list_collections(conn: &Connection) -> Result<Vec<CollectionDto>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.icon, COUNT(ca.asset_id)
         FROM collections c LEFT JOIN collection_assets ca ON ca.collection_id = c.id
         GROUP BY c.id ORDER BY c.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CollectionDto {
            id: r.get(0)?,
            name: r.get(1)?,
            icon: r.get(2)?,
            count: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Delete a collection (its physical assets are untouched — spec §22).
pub fn delete_collection(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM collections WHERE id = ?1", [id])?;
    Ok(())
}

/// Add an asset to a collection. Idempotent.
pub fn add_to_collection(conn: &Connection, collection_id: i64, asset_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO collection_assets (collection_id, asset_id) VALUES (?1, ?2)",
        params![collection_id, asset_id],
    )?;
    Ok(())
}

/// Remove an asset from a collection.
pub fn remove_from_collection(conn: &Connection, collection_id: i64, asset_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM collection_assets WHERE collection_id = ?1 AND asset_id = ?2",
        params![collection_id, asset_id],
    )?;
    Ok(())
}

/// The materials and textures in a collection.
pub fn collection_members(conn: &Connection, collection_id: i64) -> Result<MixedAssets> {
    let mut stmt =
        conn.prepare("SELECT asset_id FROM collection_assets WHERE collection_id = ?1")?;
    let ids: HashSet<String> = stmt
        .query_map([collection_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(MixedAssets {
        materials: material::list_materials(conn, None)?
            .into_iter()
            .filter(|m| ids.contains(&m.id))
            .collect(),
        textures: texture::list_textures(conn, None)?
            .into_iter()
            .filter(|t| ids.contains(&t.id))
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Duplicates (spec §27)
// ---------------------------------------------------------------------------

/// Groups of textures whose content hashes match (never deletes anything).
pub fn list_duplicates(conn: &Connection) -> Result<Vec<DuplicateGroup>> {
    let mut stmt = conn
        .prepare("SELECT hash FROM file_hashes GROUP BY hash HAVING COUNT(*) > 1 ORDER BY hash")?;
    let hashes: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;

    let by_id: HashMap<String, TextureDto> = texture::list_textures(conn, None)?
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();

    let mut out = Vec::new();
    for hash in hashes {
        let mut ids = conn.prepare("SELECT texture_id FROM file_hashes WHERE hash = ?1")?;
        let tex_ids: Vec<String> = ids
            .query_map([&hash], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        let textures: Vec<TextureDto> =
            tex_ids.into_iter().filter_map(|id| by_id.get(&id).cloned()).collect();
        if textures.len() > 1 {
            out.push(DuplicateGroup { hash, textures });
        }
    }
    Ok(out)
}

/// Count of redundant copies (total hashed files minus distinct hashes).
pub fn duplicate_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) - COUNT(DISTINCT hash) FROM file_hashes",
        [],
        |r| r.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Remove from library (spec §26 — never touches the user's actual files)
// ---------------------------------------------------------------------------

/// Remove an asset's record from the library. This deletes the DB row (which
/// cascades to previews, tags, collection/set/material links, hashes, and UDIM
/// tiles) but NEVER deletes the underlying texture/material files on disk. The
/// cache thumbnail path is returned so the caller can drop that (rebuildable)
/// cache file; the original asset file is left untouched.
pub fn remove_asset(conn: &Connection, id: &str) -> Result<Option<String>> {
    // The cache thumbnail is safe to delete — it is not the user's file.
    let preview_path: Option<String> = conn
        .query_row(
            "SELECT preview_path FROM previews WHERE asset_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    conn.execute("DELETE FROM assets WHERE id = ?1", [id])?;
    Ok(preview_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::maptypes::MapTypeRegistry;
    use crate::texture::{analyze, import_texture, ImportOptions};
    use image::{Rgb, RgbImage};
    use std::path::{Path, PathBuf};

    fn png(dir: &Path, name: &str, shade: u8) -> PathBuf {
        let mut img = RgbImage::new(32, 32);
        for px in img.pixels_mut() {
            *px = Rgb([shade, shade, shade]);
        }
        let p = dir.join(name);
        img.save(&p).unwrap();
        p
    }

    fn import(db: &Database, dir: &Path, name: &str, shade: u8) -> String {
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&png(dir, name, shade), &reg).unwrap();
        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.join("_t"),
            generate_preview: false,
            detect_maps: true,
        };
        match import_texture(db.conn(), &a, &opts).unwrap() {
            crate::texture::ImportOutcome::Imported { id, .. } => id,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn favorites_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let id = import(&db, dir.path(), "wood_basecolor.png", 100);
        import(&db, dir.path(), "wood_roughness.png", 110);

        assert_eq!(list_favorites(db.conn()).unwrap().textures.len(), 0);
        set_favorite(db.conn(), &id, true).unwrap();
        let fav = list_favorites(db.conn()).unwrap();
        assert_eq!(fav.textures.len(), 1);
        assert_eq!(fav.textures[0].id, id);
        set_favorite(db.conn(), &id, false).unwrap();
        assert_eq!(list_favorites(db.conn()).unwrap().textures.len(), 0);
    }

    #[test]
    fn tags_add_search_remove() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let id = import(&db, dir.path(), "wood_basecolor.png", 100);

        let tag = add_tag(db.conn(), &id, "#industrial").unwrap();
        assert_eq!(tag.name, "industrial"); // hash + spaces stripped
        assert_eq!(tags_for_asset(db.conn(), &id).unwrap().len(), 1);
        assert_eq!(list_tags(db.conn()).unwrap()[0].count, 1);

        // Searchable by tag.
        assert_eq!(search(db.conn(), "industrial").unwrap().textures.len(), 1);

        remove_tag(db.conn(), &id, tag.id).unwrap();
        assert_eq!(tags_for_asset(db.conn(), &id).unwrap().len(), 0);
    }

    #[test]
    fn search_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        import(&db, dir.path(), "Concrete_BaseColor.png", 100);
        import(&db, dir.path(), "Wood_BaseColor.png", 120);

        let r = search(db.conn(), "concrete").unwrap();
        assert_eq!(r.textures.len(), 1);
        assert_eq!(r.textures[0].name, "Concrete");
        // Prefix match.
        assert_eq!(search(db.conn(), "conc").unwrap().textures.len(), 1);
    }

    #[test]
    fn collections_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let id = import(&db, dir.path(), "metal_normal.png", 90);

        let col = create_collection(db.conn(), "Sci-Fi", Some("🚀")).unwrap();
        add_to_collection(db.conn(), col.id, &id).unwrap();
        let listed = list_collections(db.conn()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].count, 1);
        assert_eq!(collection_members(db.conn(), col.id).unwrap().textures.len(), 1);

        remove_from_collection(db.conn(), col.id, &id).unwrap();
        assert_eq!(collection_members(db.conn(), col.id).unwrap().textures.len(), 0);
        delete_collection(db.conn(), col.id).unwrap();
        assert_eq!(list_collections(db.conn()).unwrap().len(), 0);
    }

    #[test]
    fn duplicate_detection_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        // Same content, different names → identical hash.
        import(&db, dir.path(), "copy_a.png", 128);
        import(&db, dir.path(), "copy_b.png", 128);
        import(&db, dir.path(), "unique.png", 200);

        let groups = list_duplicates(db.conn()).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].textures.len(), 2);
        assert_eq!(duplicate_count(db.conn()).unwrap(), 1);
    }

    #[test]
    fn remove_asset_deletes_record_but_keeps_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let id = import(&db, dir.path(), "stone_basecolor.png", 140);
        let file = dir.path().join("stone_basecolor.png");

        assert!(file.exists());
        assert_eq!(texture::list_textures(db.conn(), None).unwrap().len(), 1);

        remove_asset(db.conn(), &id).unwrap();

        assert_eq!(texture::list_textures(db.conn(), None).unwrap().len(), 0);
        assert!(file.exists(), "the original file must never be deleted (spec §26)");
        // Hash row cascaded away too.
        let hashes: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM file_hashes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hashes, 0);
    }

    #[test]
    fn recent_used_orders_by_last_use() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let a = import(&db, dir.path(), "a_basecolor.png", 100);
        let b = import(&db, dir.path(), "b_roughness.png", 110);
        record_usage(db.conn(), &a, "viewed").unwrap();
        record_usage(db.conn(), &b, "viewed").unwrap();
        let recent = list_recent_used(db.conn(), 10).unwrap();
        assert_eq!(recent.textures.len(), 2);
    }
}
