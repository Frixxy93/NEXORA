//! Material engine (spec §5, §13, §20, §24, §44), Phase 4.
//!
//! A material is a complete asset that **references** textures per slot rather
//! than copying them, so the same texture can back many materials (spec §44).
//! Materials are created two ways: importing a folder of maps (§13) or promoting
//! an existing texture set (Texture → Set → Material). A full 3D preview arrives
//! with the preview engine (Phase 6); here a material's preview is its base-color
//! map thumbnail.

use crate::ids::{new_id, AssetKind};
use crate::maptypes::MapTypeRegistry;
use crate::texture::{
    self, analyze, collect_files, import_texture, ImportOptions, ImportOutcome, EXPECTED_PBR,
};
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default material categories (spec §20). Matched against the material name.
pub const DEFAULT_CATEGORIES: &[&str] = &[
    "Concrete", "Wood", "Metal", "Stone", "Brick", "Plaster", "Tile", "Fabric", "Leather",
    "Plastic", "Glass", "Rubber", "Ground", "Organic", "Sci-Fi",
];

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One slot of a material and the texture that fills it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialMapDto {
    pub slot: String,
    pub texture_id: String,
    pub name: String,
}

/// A material shaped for the UI (spec §24).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDto {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub is_pbr: bool,
    pub tileable: Option<bool>,
    pub is_udim: bool,
    pub resolution: Option<String>,
    pub health: i64,
    pub status: String,
    pub favorite: bool,
    pub maps: Vec<MaterialMapDto>,
    pub missing_maps: Vec<String>,
    pub renderers: Vec<String>,
    /// Texture whose thumbnail represents the material (base color if present).
    pub preview_texture_id: Option<String>,
}

/// Outcome of importing a material folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialImportResult {
    pub id: String,
    pub name: String,
    pub map_count: usize,
    pub textures_imported: usize,
}

/// Guess a category from a material name by keyword (spec §20). Returns `Other`
/// when nothing matches.
pub fn detect_category(name: &str) -> String {
    let n = name.to_lowercase();
    for cat in DEFAULT_CATEGORIES {
        if n.contains(&cat.to_lowercase()) {
            return (*cat).to_string();
        }
    }
    "Other".to_string()
}

/// A friendly material name from a folder path (folder stem, prettified).
fn derive_material_name(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Material");
    let cleaned = raw.replace(['_', '-'], " ");
    cleaned
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which renderer adapters can build a shader for a material with these slots.
///
/// NEXORA ships three adapters. `generic_pbr` is the always-available baseline.
/// The dedicated V-Ray (VRayMtl) and Arnold (aiStandardSurface) networks are
/// anchored on the base color, so they apply to any material that has one — which
/// is every real PBR material. This is what populates the V-Ray/Arnold library
/// views and the "Renderers" chips.
fn supported_renderers(present: &std::collections::HashSet<&str>) -> Vec<&'static str> {
    let mut renderers = vec!["generic_pbr"];
    if present.contains("base_color") {
        renderers.push("vray");
        renderers.push("arnold");
    }
    renderers
}

/// A texture's tileable flag (`None` if unknown), for material metadata.
fn texture_tileable(conn: &Connection, texture_id: &str) -> Option<bool> {
    conn.query_row(
        "SELECT tileable FROM textures WHERE asset_id = ?1",
        [texture_id],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .map(|v| v != 0)
}

/// Width/height/UDIM flag for a texture, for material metadata.
fn texture_dims(conn: &Connection, texture_id: &str) -> Option<(Option<i64>, Option<i64>, bool)> {
    conn.query_row(
        "SELECT width, height, is_udim FROM textures WHERE asset_id = ?1",
        [texture_id],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, i64>(2)? != 0,
            ))
        },
    )
    .ok()
}

/// Core creator shared by folder-import and create-from-set. `slot_tex` is the
/// ordered, de-duplicated (slot → texture id) mapping.
fn create_material(
    conn: &Connection,
    name: &str,
    category: &str,
    folder_path: Option<&str>,
    slot_tex: &[(String, String)],
) -> Result<String> {
    let present: std::collections::HashSet<&str> = slot_tex.iter().map(|(s, _)| s.as_str()).collect();
    let is_pbr = present.contains("base_color")
        && present.contains("normal")
        && (present.contains("roughness")
            || present.contains("glossiness")
            || present.contains("metallic"));

    // Resolution + UDIM come from the members (prefer base color for resolution).
    let base = slot_tex
        .iter()
        .find(|(s, _)| s == "base_color")
        .or_else(|| slot_tex.first());
    let resolution = base
        .and_then(|(_, tid)| texture_dims(conn, tid))
        .map(|(w, h, _)| texture::res_label(w, h))
        .filter(|s| !s.is_empty());
    let is_udim = slot_tex
        .iter()
        .any(|(_, tid)| texture_dims(conn, tid).map(|(_, _, u)| u).unwrap_or(false));
    // A material tiles if its base-color (or first) texture tiles.
    let tileable = base.and_then(|(_, tid)| texture_tileable(conn, tid));

    // Health = share of expected PBR slots present (spec §31).
    let present_expected = EXPECTED_PBR.iter().filter(|s| present.contains(**s)).count();
    let health = ((present_expected as f64 / EXPECTED_PBR.len() as f64) * 100.0).round() as i64;
    let status = if health >= 100 {
        "healthy"
    } else if health == 0 {
        "broken"
    } else {
        "incomplete"
    };

    let id = new_id(AssetKind::Material);
    let ts = now();

    conn.execute(
        "INSERT INTO assets (id, kind, name, category, favorite, created_at, updated_at)
         VALUES (?1, 'material', ?2, ?3, 0, ?4, ?4)",
        params![id, name, category, ts],
    )?;
    conn.execute(
        "INSERT INTO materials
           (asset_id, folder_path, is_pbr, tileable, is_udim, resolution, health, status)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            id,
            folder_path,
            is_pbr as i64,
            tileable,
            is_udim as i64,
            resolution,
            health,
            status
        ],
    )?;
    for (slot, tex_id) in slot_tex {
        conn.execute(
            "INSERT INTO material_maps (material_id, slot, texture_id) VALUES (?1, ?2, ?3)",
            params![id, slot, tex_id],
        )?;
    }
    // Record every renderer this material can be applied in, so the V-Ray/Arnold
    // library views and the inspector "Renderers" chips reflect reality.
    for renderer in supported_renderers(&present) {
        conn.execute(
            "INSERT OR IGNORE INTO renderer_presets (material_id, renderer, params)
             VALUES (?1, ?2, NULL)",
            params![id, renderer],
        )?;
    }

    Ok(id)
}

/// Import a directory of maps as one material (spec §13). Each texture is
/// imported (referenced, not duplicated) and linked to the material by slot.
pub fn import_material_folder(
    conn: &Connection,
    dir: &Path,
    opts: &ImportOptions,
    registry: &MapTypeRegistry,
) -> Result<MaterialImportResult> {
    let files = collect_files(&[dir.to_path_buf()]);

    let mut slot_tex: Vec<(String, String)> = Vec::new();
    let mut seen_slots = std::collections::HashSet::new();
    let mut textures_imported = 0usize;

    for file in &files {
        let a = analyze(file, registry)?;
        let tex_id = match import_texture(conn, &a, opts)? {
            ImportOutcome::Imported { id, .. } => {
                textures_imported += 1;
                id
            }
            ImportOutcome::DuplicatePath { id } => id,
            ImportOutcome::Skipped { .. } => continue,
        };
        if let Some(slot) = &a.map_type {
            if seen_slots.insert(slot.clone()) {
                slot_tex.push((slot.clone(), tex_id));
            }
        }
    }

    let name = derive_material_name(dir);
    let category = detect_category(&name);
    let folder = dir.to_string_lossy().to_string();
    let id = create_material(conn, &name, &category, Some(&folder), &slot_tex)?;

    Ok(MaterialImportResult {
        id,
        name,
        map_count: slot_tex.len(),
        textures_imported,
    })
}

/// Create a material from an explicit list of (slot, file path) pairs — used by
/// the Maya capture flow (spec §39), where the shader already tells us which map
/// fills which slot. Each path is imported (referenced) and linked by slot.
pub fn create_material_from_maps(
    conn: &Connection,
    name: &str,
    slot_paths: &[(String, String)],
    opts: &ImportOptions,
    registry: &MapTypeRegistry,
) -> Result<String> {
    let mut slot_tex: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (slot, path) in slot_paths {
        let p = Path::new(path);
        if !texture::is_supported(p) {
            continue;
        }
        let a = analyze(p, registry)?;
        let tex_id = match import_texture(conn, &a, opts)? {
            ImportOutcome::Imported { id, .. } => id,
            ImportOutcome::DuplicatePath { id } => id,
            ImportOutcome::Skipped { .. } => continue,
        };
        if seen.insert(slot.clone()) {
            slot_tex.push((slot.clone(), tex_id));
        }
    }
    let category = detect_category(name);
    create_material(conn, name, &category, None, &slot_tex)
}

/// Promote an existing texture set into a material referencing the same textures.
pub fn create_material_from_set(
    conn: &Connection,
    set_id: &str,
    name_override: Option<&str>,
) -> Result<String> {
    let set = texture::get_texture_set(conn, set_id)?
        .ok_or_else(|| crate::CoreError::NotFound(format!("texture set {set_id}")))?;
    let slot_tex: Vec<(String, String)> = set
        .maps
        .iter()
        .map(|m| (m.slot.clone(), m.texture_id.clone()))
        .collect();
    let name = name_override.unwrap_or(&set.name).to_string();
    let category = detect_category(&name);
    create_material(conn, &name, &category, None, &slot_tex)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

fn material_maps(conn: &Connection, material_id: &str) -> Result<Vec<MaterialMapDto>> {
    let mut stmt = conn.prepare(
        "SELECT m.slot, m.texture_id, a.name
         FROM material_maps m JOIN assets a ON a.id = m.texture_id
         WHERE m.material_id = ?1 ORDER BY m.slot",
    )?;
    let rows = stmt.query_map([material_id], |r| {
        Ok(MaterialMapDto {
            slot: r.get(0)?,
            texture_id: r.get(1)?,
            name: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn material_renderers(conn: &Connection, material_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT renderer FROM renderer_presets WHERE material_id = ?1 ORDER BY renderer")?;
    let rows = stmt.query_map([material_id], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// A material's own columns, before maps/renderers are attached.
struct MaterialHead {
    id: String,
    name: String,
    category: Option<String>,
    is_pbr: bool,
    tileable: Option<bool>,
    is_udim: bool,
    resolution: Option<String>,
    health: i64,
    status: String,
    favorite: bool,
}

fn read_head(r: &rusqlite::Row) -> rusqlite::Result<MaterialHead> {
    Ok(MaterialHead {
        id: r.get(0)?,
        name: r.get(1)?,
        category: r.get(2)?,
        is_pbr: r.get::<_, i64>(3)? != 0,
        tileable: r.get::<_, Option<i64>>(4)?.map(|v| v != 0),
        is_udim: r.get::<_, i64>(5)? != 0,
        resolution: r.get(6)?,
        health: r.get(7)?,
        status: r.get(8)?,
        favorite: r.get::<_, i64>(9)? != 0,
    })
}

/// The shared material-head read model. Callers append a filter/order clause.
const MATERIAL_HEAD_SELECT: &str = "SELECT a.id, a.name, a.category, m.is_pbr, m.tileable, m.is_udim,
                       m.resolution, m.health, m.status, a.favorite
                FROM assets a JOIN materials m ON m.asset_id = a.id
                WHERE a.kind = 'material'";

/// Run the material-head query with an extra filter/order clause appended.
fn query_material_heads(
    conn: &Connection,
    filter: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<MaterialHead>> {
    let sql = format!("{MATERIAL_HEAD_SELECT} {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, read_head)?;
    let mut heads = Vec::new();
    for r in rows {
        heads.push(r?);
    }
    Ok(heads)
}

/// Attach each head's maps, renderers, missing slots, and preview into a DTO.
/// The per-head sub-queries run only for the heads actually selected, so scoped
/// callers (get/by-ids/favorites/recent) never load the whole library.
fn attach_material_details(conn: &Connection, heads: Vec<MaterialHead>) -> Result<Vec<MaterialDto>> {
    let mut out = Vec::new();
    for head in heads {
        let MaterialHead {
            id,
            name,
            category,
            is_pbr,
            tileable,
            is_udim,
            resolution,
            health,
            status,
            favorite,
        } = head;
        let maps = material_maps(conn, &id)?;
        let renderers = material_renderers(conn, &id)?;
        let present: std::collections::HashSet<&str> = maps.iter().map(|m| m.slot.as_str()).collect();
        let missing_maps = EXPECTED_PBR
            .iter()
            .filter(|s| !present.contains(**s))
            .map(|s| s.to_string())
            .collect();
        let preview_texture_id = maps
            .iter()
            .find(|m| m.slot == "base_color")
            .or_else(|| maps.first())
            .map(|m| m.texture_id.clone());
        out.push(MaterialDto {
            id,
            name,
            category,
            is_pbr,
            tileable,
            is_udim,
            resolution,
            health,
            status,
            favorite,
            maps,
            missing_maps,
            renderers,
            preview_texture_id,
        });
    }
    Ok(out)
}

/// List materials, newest first, optionally filtered by category.
pub fn list_materials(conn: &Connection, category: Option<&str>) -> Result<Vec<MaterialDto>> {
    let heads = match category {
        Some(cat) => query_material_heads(
            conn,
            "AND a.category = ?1 ORDER BY a.created_at DESC",
            &[&cat],
        )?,
        None => query_material_heads(conn, "ORDER BY a.created_at DESC", &[])?,
    };
    attach_material_details(conn, heads)
}

/// Fetch many materials by id in a single head query (results newest-first).
/// Empty input returns empty without touching the DB.
pub fn list_materials_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<MaterialDto>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "{MATERIAL_HEAD_SELECT} AND a.id IN ({placeholders}) ORDER BY a.created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), read_head)?;
    let mut heads = Vec::new();
    for r in rows {
        heads.push(r?);
    }
    attach_material_details(conn, heads)
}

/// Favorited materials only (filtered in SQL, no full-table load).
pub fn list_favorite_materials(conn: &Connection) -> Result<Vec<MaterialDto>> {
    let heads = query_material_heads(conn, "AND a.favorite = 1 ORDER BY a.created_at DESC", &[])?;
    attach_material_details(conn, heads)
}

/// The `limit` most recently added materials (LIMIT in SQL).
pub fn list_recent_materials(conn: &Connection, limit: usize) -> Result<Vec<MaterialDto>> {
    let lim = limit as i64;
    let heads = query_material_heads(conn, "ORDER BY a.created_at DESC LIMIT ?1", &[&lim])?;
    attach_material_details(conn, heads)
}

/// Fetch a single material by id — an indexed primary-key lookup, not a scan.
pub fn get_material(conn: &Connection, id: &str) -> Result<Option<MaterialDto>> {
    let heads = query_material_heads(conn, "AND a.id = ?1", &[&id])?;
    Ok(attach_material_details(conn, heads)?.into_iter().next())
}

/// Backfill `tileable` for materials that don't have it yet, deriving each from
/// its base-color (or first) texture. Returns how many materials were updated.
/// Run this after [`crate::texture::backfill_texture_metadata`] so the source
/// textures already carry their tileable flag.
pub fn recompute_material_tileable(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT asset_id FROM materials WHERE tileable IS NULL")?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut updated = 0usize;
    for id in ids {
        // Prefer the base-color slot; fall back to any slot's texture.
        let tid: Option<String> = conn
            .query_row(
                "SELECT texture_id FROM material_maps
                 WHERE material_id = ?1 AND texture_id IS NOT NULL
                 ORDER BY (slot = 'base_color') DESC LIMIT 1",
                [&id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        if let Some(tid) = tid {
            if let Some(t) = texture_tileable(conn, &tid) {
                conn.execute(
                    "UPDATE materials SET tileable = ?2 WHERE asset_id = ?1",
                    params![id, t],
                )?;
                updated += 1;
            }
        }
    }
    Ok(updated)
}

/// Count materials that aren't fully healthy (for library health, spec §30).
pub fn incomplete_material_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM materials WHERE status <> 'healthy'",
        [],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use image::{Rgb, RgbImage};
    use std::path::PathBuf;

    fn write_png(dir: &Path, name: &str) -> PathBuf {
        let mut img = RgbImage::new(2048, 2048);
        for px in img.pixels_mut() {
            *px = Rgb([100, 100, 100]);
        }
        let p = dir.join(name);
        img.save(&p).unwrap();
        p
    }

    fn opts(dir: &Path) -> ImportOptions {
        ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.join("_t"),
            generate_preview: false,
            detect_maps: true,
        }
    }

    #[test]
    fn detect_category_matches_keywords() {
        assert_eq!(detect_category("Concrete Industrial"), "Concrete");
        assert_eq!(detect_category("OldWood_Planks"), "Wood");
        assert_eq!(detect_category("Whatever"), "Other");
    }

    #[test]
    fn import_material_folder_links_textures() {
        let root = tempfile::tempdir().unwrap();
        let mat_dir = root.path().join("Concrete");
        std::fs::create_dir_all(&mat_dir).unwrap();
        write_png(&mat_dir, "BaseColor.jpg");
        write_png(&mat_dir, "Roughness.jpg");
        write_png(&mat_dir, "Normal.jpg");

        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        let res = import_material_folder(db.conn(), &mat_dir, &opts(root.path()), &reg).unwrap();

        assert_eq!(res.name, "Concrete");
        assert_eq!(res.map_count, 3);
        assert_eq!(res.textures_imported, 3);

        let mats = list_materials(db.conn(), None).unwrap();
        assert_eq!(mats.len(), 1);
        let m = &mats[0];
        assert_eq!(m.category.as_deref(), Some("Concrete"));
        assert!(m.is_pbr);
        assert_eq!(m.resolution.as_deref(), Some("2K"));
        assert_eq!(m.maps.len(), 3);
        assert!(m.missing_maps.contains(&"height".to_string()));
        // Has a base color → applicable in all three renderer adapters, so the
        // V-Ray/Arnold library views and inspector chips are populated.
        assert!(m.renderers.contains(&"generic_pbr".to_string()));
        assert!(m.renderers.contains(&"vray".to_string()));
        assert!(m.renderers.contains(&"arnold".to_string()));
        assert!(m.preview_texture_id.is_some());
        // health = 3 of 5 expected slots = 60
        assert_eq!(m.health, 60);
        assert_eq!(m.status, "incomplete");

        // Textures also exist independently and are referenced, not duplicated.
        assert_eq!(texture::list_textures(db.conn(), None).unwrap().len(), 3);
    }

    #[test]
    fn create_material_from_set_reuses_textures() {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        for n in ["Brick_BaseColor_2K.png", "Brick_Normal_2K.png", "Brick_Roughness_2K.png"] {
            let f = write_png(root.path(), n);
            let a = analyze(&f, &reg).unwrap();
            import_texture(db.conn(), &a, &opts(root.path())).unwrap();
        }
        texture::rebuild_texture_sets(db.conn()).unwrap();
        let set = texture::list_texture_sets(db.conn()).unwrap().remove(0);

        let mid = create_material_from_set(db.conn(), &set.id, None).unwrap();
        let m = get_material(db.conn(), &mid).unwrap().unwrap();
        assert_eq!(m.name, "Brick");
        assert_eq!(m.category.as_deref(), Some("Brick"));
        assert_eq!(m.maps.len(), 3);
        assert!(m.is_pbr);

        // Material maps point at the same texture ids as the set.
        let set_tex: std::collections::HashSet<_> =
            set.maps.iter().map(|x| x.texture_id.clone()).collect();
        for mm in &m.maps {
            assert!(set_tex.contains(&mm.texture_id));
        }
    }
}
