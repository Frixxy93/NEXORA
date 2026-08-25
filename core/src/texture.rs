//! Texture import engine (spec §6–§10, Phase 2).
//!
//! Turns files on disk into `texture` asset records: reads metadata, hashes the
//! content for duplicate detection, classifies the map type via the
//! [`crate::maptypes`] registry, optionally copies into a managed library, and
//! generates a thumbnail. All logic is GUI-agnostic so it can be unit-tested;
//! the Tauri layer only supplies paths, options, and progress reporting.

use crate::ids::{new_id, AssetKind};
use crate::maptypes::MapTypeRegistry;
use crate::{CoreError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// File extensions NEXORA will import (spec §8). Lowercase, no dot.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "tif", "tiff", "tga", "bmp", "exr", "hdr", "webp", "tx",
];

/// Longest thumbnail edge in px.
const THUMB_MAX: u32 = 256;

/// True if `path` has an extension NEXORA imports.
pub fn is_supported(path: &Path) -> bool {
    ext_lower(path)
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Map slug → managed-library subfolder (spec §25).
pub fn library_subfolder(map_slug: Option<&str>) -> &'static str {
    match map_slug {
        Some("base_color") => "Textures/BaseColor",
        Some("roughness") | Some("glossiness") => "Textures/Roughness",
        Some("metallic") => "Textures/Metallic",
        Some("normal") | Some("bump") => "Textures/Normal",
        Some("height") | Some("displacement") => "Textures/Height",
        _ => "Textures/Other",
    }
}

/// The result of analyzing a file, before it becomes a DB record.
#[derive(Debug, Clone)]
pub struct AnalyzedTexture {
    pub name: String,
    pub file_path: String,
    pub map_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: String,
    pub channels: Option<u8>,
    pub color_space: Option<String>,
    pub file_size: u64,
    pub is_udim: bool,
    pub udim_tile: Option<u32>,
    pub hash: String,
}

/// Options controlling an import run.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Copy into the managed library (`true`) vs. reference in place (`false`).
    pub managed: bool,
    /// Root of the managed library; required when `managed` is true.
    pub library_root: Option<PathBuf>,
    /// Directory where thumbnails are cached (rebuildable, spec §50).
    pub thumbnail_dir: PathBuf,
    /// Generate a thumbnail on import.
    pub generate_preview: bool,
    /// Run filename → map-type detection.
    pub detect_maps: bool,
}

/// What happened to one file during import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ImportOutcome {
    Imported { id: String, name: String },
    /// The exact same path is already indexed.
    DuplicatePath { id: String },
    Skipped { reason: String },
}

/// A texture row shaped for the frontend/inspector (spec §23).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureDto {
    pub id: String,
    pub name: String,
    pub map_type: Option<String>,
    pub category: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub channels: Option<u8>,
    pub color_space: Option<String>,
    pub file_size: Option<u64>,
    pub is_udim: bool,
    pub tileable: Option<bool>,
    pub favorite: bool,
    pub managed: bool,
    pub file_path: String,
    pub thumbnail_path: Option<String>,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Inspect a file without touching the database. Metadata that requires
/// decoding (dimensions, channels) is best-effort — undecodable files (e.g. some
/// `.tx`) still yield a record from filename + size + hash.
pub fn analyze(path: &Path, registry: &MapTypeRegistry) -> Result<AnalyzedTexture> {
    if !is_supported(path) {
        return Err(CoreError::Config(format!(
            "unsupported file type: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let file_size = bytes.len() as u64;
    let format = ext_lower(path).unwrap_or_default();

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let map_type = registry.detect(&filename).map(|m| m.slug());
    let (is_udim, udim_tile) = detect_udim(&filename);

    // Cheap dimension probe (header only). Channels come later if we thumbnail.
    let (width, height) = image::image_dimensions(path)
        .ok()
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));

    let color_space = Some(guess_color_space(&format, map_type.as_deref()).to_string());
    let name = derive_name(&filename, registry);

    Ok(AnalyzedTexture {
        name,
        file_path: path.to_string_lossy().to_string(),
        map_type,
        width,
        height,
        format,
        // Channel count from the image header (cheap — no full pixel decode).
        channels: probe_channels(&bytes),
        color_space,
        file_size,
        is_udim,
        udim_tile,
        hash,
    })
}

/// Read an image's channel count from its header without decoding pixels.
/// Returns `None` for formats we can't introspect (e.g. some `.tx`).
fn probe_channels(bytes: &[u8]) -> Option<u8> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let decoder = reader.into_decoder().ok()?;
    Some(decoder.color_type().channel_count())
}

/// Heuristic seamless-tiling detection: a texture tiles when its opposite edges
/// wrap into each other. We compare the left/right columns and top/bottom rows of
/// the (already-decoded) thumbnail; a small average difference means the edges
/// meet cleanly. Operates on the small thumbnail, so it's cheap.
fn detect_tileable(thumb: &image::RgbaImage) -> bool {
    let (w, h) = thumb.dimensions();
    if w < 8 || h < 8 {
        return false;
    }
    let mut diff: u64 = 0;
    let mut count: u64 = 0;
    for y in 0..h {
        let l = thumb.get_pixel(0, y);
        let r = thumb.get_pixel(w - 1, y);
        for c in 0..3 {
            diff += (l[c] as i32 - r[c] as i32).unsigned_abs() as u64;
            count += 1;
        }
    }
    for x in 0..w {
        let t = thumb.get_pixel(x, 0);
        let b = thumb.get_pixel(x, h - 1);
        for c in 0..3 {
            diff += (t[c] as i32 - b[c] as i32).unsigned_abs() as u64;
            count += 1;
        }
    }
    // Mean absolute edge difference on a 0..255 scale. Seamless CC0 PBR textures
    // sit well under this; photos and cropped images sit well above.
    let mean = diff as f64 / count.max(1) as f64;
    mean < 12.0
}

/// Detect a UDIM tile number in a filename, e.g. `body.1002.exr` → (true, 1002).
fn detect_udim(filename: &str) -> (bool, Option<u32>) {
    // A 4-digit tile in the 1001–2100 UDIM range, delimited by `.` or `_`.
    let re = regex::Regex::new(r"[._](\d{4})\.[A-Za-z0-9]+$").unwrap();
    if let Some(cap) = re.captures(filename) {
        if let Ok(tile) = cap[1].parse::<u32>() {
            if (1001..=2100).contains(&tile) {
                return (true, Some(tile));
            }
        }
    }
    (false, None)
}

/// Best-effort color-space guess (spec stores it; renderers rely on it).
fn guess_color_space(format: &str, map_slug: Option<&str>) -> &'static str {
    match format {
        "exr" | "hdr" => "linear",
        _ => match map_slug {
            Some("base_color") | Some("emission") => "srgb",
            Some(_) => "linear",
            None => "srgb",
        },
    }
}

/// Turn a filename into a friendly asset name by dropping the extension and any
/// segment that is map-type noise (`roughness`, `nrm`, …), a resolution token
/// (`4K`, `2048`), or a UDIM tile (`1002`) — wherever it appears. The remaining
/// words are title-cased. `Concrete_Roughness_4K.png` → `Concrete`.
fn derive_name(filename: &str, registry: &MapTypeRegistry) -> String {
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);

    // All recognized map tokens are noise for naming purposes.
    let noise: std::collections::HashSet<String> = registry
        .rules
        .iter()
        .flat_map(|r| r.tokens.iter().cloned())
        .collect();

    let kept: Vec<String> = stem
        .split(|c: char| c == '_' || c == '-' || c == '.' || c == ' ')
        .filter(|s| !s.is_empty())
        .filter(|s| {
            let l = s.to_lowercase();
            !noise.contains(&l) && !is_resolution_token(&l) && !is_udim_token(&l)
        })
        .map(title_word)
        .collect();

    if kept.is_empty() {
        // Everything was noise — fall back to the raw stem, prettified.
        title_word(stem.replace(['_', '-'], " ").trim())
    } else {
        kept.join(" ")
    }
}

/// `4k`, `8k`, `512`, `1024`, `2048`, `4096`, `8192`, `2048px` → resolution noise.
fn is_resolution_token(s: &str) -> bool {
    if let Some(k) = s.strip_suffix('k') {
        return k.chars().all(|c| c.is_ascii_digit()) && !k.is_empty();
    }
    let digits = s.strip_suffix("px").unwrap_or(s);
    matches!(digits, "256" | "512" | "1024" | "2048" | "4096" | "8192")
}

/// A bare 4-digit UDIM tile in the 1001–2100 range.
fn is_udim_token(s: &str) -> bool {
    s.len() == 4
        && s.chars().all(|c| c.is_ascii_digit())
        && s.parse::<u32>().map(|n| (1001..=2100).contains(&n)).unwrap_or(false)
}

/// Title-case a single space-separated phrase.
fn title_word(s: &str) -> String {
    s.split_whitespace()
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

// ---------------------------------------------------------------------------
// Packed maps (ARM / ORM / RMA …) — channel-packed grayscale maps
// ---------------------------------------------------------------------------

/// If the filename marks a channel-packed map, return the slot each RGB channel
/// carries (e.g. ARM → AO, Roughness, Metallic). These pack three grayscale maps
/// into one image and are common in game/PBR asset packs; NEXORA splits them into
/// usable single-channel maps on import.
pub fn detect_packed(filename: &str) -> Option<[&'static str; 3]> {
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);
    for tok in stem.split(|c| c == '_' || c == '-' || c == '.' || c == ' ') {
        match tok.to_lowercase().as_str() {
            // AO/Occlusion, Roughness, Metallic (ARM and glTF's ORM are the same).
            "arm" | "orm" => return Some(["ao", "roughness", "metallic"]),
            "rma" | "rmo" => return Some(["roughness", "metallic", "ao"]),
            "mra" | "mro" => return Some(["metallic", "roughness", "ao"]),
            _ => {}
        }
    }
    None
}

/// Split a channel-packed map into single-channel grayscale sibling files next to
/// the source (`{stem}_{slot}.png`), returning `(slot, path)` for each. Existing
/// siblings are reused (re-import stays idempotent). Sibling files sit beside the
/// source so both managed (copied in) and referenced (indexed in place) modes
/// work uniformly.
pub fn unpack_packed(src: &Path, layout: [&'static str; 3]) -> Result<Vec<(String, PathBuf)>> {
    let img = image::open(src)
        .map_err(|e| CoreError::Config(format!("decode packed map: {e}")))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let dir = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("packed");

    let mut out = Vec::new();
    for (ci, slot) in layout.iter().enumerate() {
        if slot.is_empty() {
            continue;
        }
        let dest = dir.join(format!("{stem}_{slot}.png"));
        if !dest.exists() {
            let mut gray = image::GrayImage::new(w, h);
            for (x, y, px) in img.enumerate_pixels() {
                gray.put_pixel(x, y, image::Luma([px[ci]]));
            }
            gray.save(&dest)
                .map_err(|e| CoreError::Config(format!("write channel: {e}")))?;
        }
        out.push((slot.to_string(), dest));
    }
    Ok(out)
}

/// Analyze a file for import, expanding a channel-packed map into its component
/// single-channel maps. Does NOT touch the DB — the heavy work (decode/unpack)
/// runs unlocked so the caller can keep DB locks brief. A non-packed file (or one
/// that fails to unpack) yields a single analysis.
pub fn prepare_import(
    path: &Path,
    opts: &ImportOptions,
    registry: &MapTypeRegistry,
) -> Result<Vec<AnalyzedTexture>> {
    if opts.detect_maps {
        if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(layout) = detect_packed(fname) {
                if let Ok(comps) = unpack_packed(path, layout) {
                    if !comps.is_empty() {
                        let mut out = Vec::with_capacity(comps.len());
                        for (_slot, comp) in comps {
                            out.push(analyze(&comp, registry)?);
                        }
                        return Ok(out);
                    }
                }
                // Unpack failed — fall through and import the packed file as-is.
            }
        }
    }
    Ok(vec![analyze(path, registry)?])
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Import one file into the database, returning its outcome.
///
/// Idempotent on path: importing the same absolute path twice reports
/// `DuplicatePath` rather than creating a second record (spec §27 — never
/// duplicate silently, never delete).
pub fn import_texture(
    conn: &Connection,
    analyzed: &AnalyzedTexture,
    opts: &ImportOptions,
) -> Result<ImportOutcome> {
    // Resolve the stored path (managed copy vs. reference in place).
    let mut stored_path = analyzed.file_path.clone();
    let mut managed = false;
    if opts.managed {
        if let Some(root) = &opts.library_root {
            let sub = library_subfolder(analyzed.map_type.as_deref());
            let dest_dir = root.join(sub);
            std::fs::create_dir_all(&dest_dir)?;
            let file_name = Path::new(&analyzed.file_path)
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            let dest = dest_dir.join(&file_name);
            // Don't clobber if the source already lives at the destination.
            if dest != Path::new(&analyzed.file_path) {
                std::fs::copy(&analyzed.file_path, &dest)?;
            }
            stored_path = dest.to_string_lossy().to_string();
            managed = true;
        }
    }

    // UDIM tiles collapse into a single texture record with per-tile rows
    // (spec §12) instead of one record per tile.
    if analyzed.is_udim && analyzed.udim_tile.is_some() {
        return import_udim_tile(conn, analyzed, &stored_path, managed, opts);
    }

    // Already indexed at this path?
    if let Some(existing) = find_texture_id_by_path(conn, &stored_path)? {
        return Ok(ImportOutcome::DuplicatePath { id: existing });
    }

    let id = new_id(AssetKind::Texture);
    let ts = now();
    let category = analyzed
        .map_type
        .clone()
        .unwrap_or_else(|| "other".to_string());

    // Thumbnail first (best-effort): its decode also yields tileability, which we
    // want in the texture row. Failure to thumbnail never blocks the import.
    let thumb = if opts.generate_preview {
        generate_thumbnail(Path::new(&stored_path), &opts.thumbnail_dir, &id).ok()
    } else {
        None
    };
    let tileable: Option<bool> = thumb.as_ref().and_then(|(_, t)| *t);

    conn.execute(
        "INSERT INTO assets (id, kind, name, category, favorite, created_at, updated_at)
         VALUES (?1, 'texture', ?2, ?3, 0, ?4, ?4)",
        params![id, analyzed.name, category, ts],
    )?;

    conn.execute(
        "INSERT INTO textures
           (asset_id, file_path, map_type, width, height, format, channels,
            color_space, file_size, is_udim, tileable, managed, created_at, modified_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
        params![
            id,
            stored_path,
            analyzed.map_type,
            analyzed.width,
            analyzed.height,
            analyzed.format,
            analyzed.channels,
            analyzed.color_space,
            analyzed.file_size as i64,
            analyzed.is_udim as i64,
            tileable,
            managed as i64,
            ts,
        ],
    )?;

    conn.execute(
        "INSERT INTO file_hashes (texture_id, hash, algo) VALUES (?1, ?2, 'blake3')",
        params![id, analyzed.hash],
    )?;

    if let Some((thumb_path, _)) = thumb {
        conn.execute(
            "INSERT INTO previews (asset_id, preview_path, kind, generated_at)
             VALUES (?1, ?2, 'thumbnail', ?3)",
            params![id, thumb_path.to_string_lossy(), ts],
        )?;
    }

    Ok(ImportOutcome::Imported {
        id,
        name: analyzed.name.clone(),
    })
}

fn find_texture_id_by_path(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT asset_id FROM textures WHERE file_path = ?1",
            [path],
            |row| row.get(0),
        )
        .optional()?)
}

/// Turn a tile path into its UDIM pattern: `body.1002.exr` → `body.<UDIM>.exr`.
/// Used as the stable `file_path` of the collapsed parent texture.
fn udim_pattern(path: &str) -> Option<String> {
    let re = regex::Regex::new(r"([._])(\d{4})(\.[A-Za-z0-9]+)$").unwrap();
    if re.is_match(path) {
        Some(re.replace(path, "${1}<UDIM>${3}").to_string())
    } else {
        None
    }
}

/// Import one tile of a UDIM set. The first tile creates the parent texture
/// record (keyed by the `<UDIM>` pattern); later tiles append to `udim_tiles`.
fn import_udim_tile(
    conn: &Connection,
    analyzed: &AnalyzedTexture,
    stored_path: &str,
    managed: bool,
    opts: &ImportOptions,
) -> Result<ImportOutcome> {
    let tile = analyzed.udim_tile.unwrap();
    let pattern = udim_pattern(stored_path).unwrap_or_else(|| stored_path.to_string());
    let ts = now();

    // Existing parent for this pattern? Add the tile if new.
    if let Some(parent_id) = find_texture_id_by_path(conn, &pattern)? {
        let exists = conn
            .query_row(
                "SELECT 1 FROM udim_tiles WHERE texture_id = ?1 AND tile = ?2",
                params![parent_id, tile],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            return Ok(ImportOutcome::DuplicatePath { id: parent_id });
        }
        conn.execute(
            "INSERT INTO udim_tiles (texture_id, tile, file_path) VALUES (?1, ?2, ?3)",
            params![parent_id, tile, stored_path],
        )?;
        return Ok(ImportOutcome::Imported {
            id: parent_id,
            name: analyzed.name.clone(),
        });
    }

    // Create the parent UDIM texture; its file_path is the pattern.
    let id = new_id(AssetKind::Texture);
    let category = analyzed
        .map_type
        .clone()
        .unwrap_or_else(|| "other".to_string());

    conn.execute(
        "INSERT INTO assets (id, kind, name, category, favorite, created_at, updated_at)
         VALUES (?1, 'texture', ?2, ?3, 0, ?4, ?4)",
        params![id, analyzed.name, category, ts],
    )?;
    conn.execute(
        "INSERT INTO textures
           (asset_id, file_path, map_type, width, height, format, channels,
            color_space, file_size, is_udim, tileable, managed, created_at, modified_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,NULL,?10,?11,?11)",
        params![
            id,
            pattern,
            analyzed.map_type,
            analyzed.width,
            analyzed.height,
            analyzed.format,
            analyzed.channels,
            analyzed.color_space,
            analyzed.file_size as i64,
            managed as i64,
            ts,
        ],
    )?;
    conn.execute(
        "INSERT INTO file_hashes (texture_id, hash, algo) VALUES (?1, ?2, 'blake3')",
        params![id, analyzed.hash],
    )?;
    conn.execute(
        "INSERT INTO udim_tiles (texture_id, tile, file_path) VALUES (?1, ?2, ?3)",
        params![id, tile, stored_path],
    )?;
    if opts.generate_preview {
        if let Ok((thumb, _)) = generate_thumbnail(Path::new(stored_path), &opts.thumbnail_dir, &id) {
            conn.execute(
                "INSERT INTO previews (asset_id, preview_path, kind, generated_at)
                 VALUES (?1, ?2, 'thumbnail', ?3)",
                params![id, thumb.to_string_lossy(), ts],
            )?;
        }
    }

    Ok(ImportOutcome::Imported {
        id,
        name: analyzed.name.clone(),
    })
}

/// The tile numbers of a UDIM texture, ascending.
pub fn udim_tiles(conn: &Connection, texture_id: &str) -> Result<Vec<u32>> {
    let mut stmt =
        conn.prepare("SELECT tile FROM udim_tiles WHERE texture_id = ?1 ORDER BY tile")?;
    let rows = stmt.query_map([texture_id], |r| r.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r? as u32);
    }
    Ok(out)
}

/// Gaps in a UDIM tile sequence, e.g. `[1001,1002,1004]` → `[1003]` (spec §12).
pub fn missing_udim_tiles(tiles: &[u32]) -> Vec<u32> {
    if tiles.is_empty() {
        return vec![];
    }
    let min = *tiles.iter().min().unwrap();
    let max = *tiles.iter().max().unwrap();
    let present: std::collections::HashSet<u32> = tiles.iter().copied().collect();
    (min..=max).filter(|t| !present.contains(t)).collect()
}

/// Decode `src`, write a PNG thumbnail into `dir` named `<id>.png`, return path.
/// Float formats (EXR/HDR) are clamped to 8-bit — a usable preview, not a grade.
/// Generate a thumbnail and, reusing the same decode, report whether the texture
/// tiles seamlessly. Returns `(thumbnail_path, tileable)`.
pub fn generate_thumbnail(src: &Path, dir: &Path, id: &str) -> Result<(PathBuf, Option<bool>)> {
    std::fs::create_dir_all(dir)?;
    let img = image::open(src)
        .map_err(|e| CoreError::Config(format!("decode failed: {e}")))?;
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX); // preserves aspect ratio
    let rgba = thumb.to_rgba8();
    let tileable = Some(detect_tileable(&rgba));
    let out = dir.join(format!("{id}.png"));
    rgba.save(&out)
        .map_err(|e| CoreError::Config(format!("thumbnail save failed: {e}")))?;
    Ok((out, tileable))
}

/// Backfill `channels` and `tileable` for already-imported textures that predate
/// those being recorded. Reads each file once (skipping missing/undecodable
/// ones), fills only the columns that are still NULL, and returns how many rows
/// were updated. UDIM parents are skipped (their `file_path` is a pattern).
pub fn backfill_texture_metadata(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT asset_id, file_path FROM textures
         WHERE is_udim = 0 AND (channels IS NULL OR tileable IS NULL)",
    )?;
    let targets: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut updated = 0usize;
    for (id, path) in targets {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue, // file moved/missing — leave as-is for relink
        };
        let channels = probe_channels(&bytes);
        let tileable = image::load_from_memory(&bytes)
            .ok()
            .map(|img| detect_tileable(&img.thumbnail(THUMB_MAX, THUMB_MAX).to_rgba8()));
        if channels.is_none() && tileable.is_none() {
            continue;
        }
        // COALESCE keeps any value already present; only fills the NULLs.
        conn.execute(
            "UPDATE textures
             SET channels = COALESCE(?2, channels),
                 tileable = COALESCE(?3, tileable)
             WHERE asset_id = ?1",
            params![id, channels, tileable],
        )?;
        updated += 1;
    }
    Ok(updated)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// List textures, newest first, optionally filtered by map-type slug.
/// The shared texture read model. Callers append a filter/order clause.
const TEXTURE_SELECT: &str = "SELECT a.id, a.name, t.map_type, a.category, t.width, t.height, t.format,
                       t.channels, t.color_space, t.file_size, t.is_udim, t.tileable,
                       a.favorite, t.managed, t.file_path, p.preview_path, a.created_at
                FROM assets a
                JOIN textures t ON t.asset_id = a.id
                LEFT JOIN previews p ON p.asset_id = a.id
                WHERE a.kind = 'texture'";

fn map_texture_row(row: &rusqlite::Row) -> rusqlite::Result<TextureDto> {
    Ok(TextureDto {
        id: row.get(0)?,
        name: row.get(1)?,
        map_type: row.get(2)?,
        category: row.get(3)?,
        width: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
        height: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        format: row.get(6)?,
        channels: row.get::<_, Option<i64>>(7)?.map(|v| v as u8),
        color_space: row.get(8)?,
        file_size: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
        is_udim: row.get::<_, i64>(10)? != 0,
        tileable: row.get::<_, Option<i64>>(11)?.map(|v| v != 0),
        favorite: row.get::<_, i64>(12)? != 0,
        managed: row.get::<_, i64>(13)? != 0,
        file_path: row.get(14)?,
        thumbnail_path: row.get(15)?,
        created_at: row.get(16)?,
    })
}

/// Run the texture read model with an extra filter/order clause appended.
fn query_textures(
    conn: &Connection,
    filter: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<TextureDto>> {
    let sql = format!("{TEXTURE_SELECT} {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, map_texture_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn list_textures(conn: &Connection, map_type: Option<&str>) -> Result<Vec<TextureDto>> {
    match map_type {
        // "other" means unclassified — textures whose map type wasn't detected.
        Some("other") => query_textures(
            conn,
            "AND t.map_type IS NULL ORDER BY a.created_at DESC",
            &[],
        ),
        Some(mt) => query_textures(
            conn,
            "AND t.map_type = ?1 ORDER BY a.created_at DESC",
            &[&mt],
        ),
        None => query_textures(conn, "ORDER BY a.created_at DESC", &[]),
    }
}

/// Fetch a single texture by id — an indexed primary-key lookup, not a scan.
pub fn get_texture(conn: &Connection, id: &str) -> Result<Option<TextureDto>> {
    Ok(query_textures(conn, "AND a.id = ?1", &[&id])?
        .into_iter()
        .next())
}

/// Fetch many textures by id in a single query (unordered input; results ordered
/// newest-first). Empty input returns empty without touching the DB.
pub fn list_textures_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<TextureDto>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("{TEXTURE_SELECT} AND a.id IN ({placeholders}) ORDER BY a.created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), map_texture_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Favorited textures only (filtered in SQL, no full-table load).
pub fn list_favorite_textures(conn: &Connection) -> Result<Vec<TextureDto>> {
    query_textures(conn, "AND a.favorite = 1 ORDER BY a.created_at DESC", &[])
}

/// The `limit` most recently added textures (LIMIT in SQL).
pub fn list_recent_textures(conn: &Connection, limit: usize) -> Result<Vec<TextureDto>> {
    let lim = limit as i64;
    query_textures(conn, "ORDER BY a.created_at DESC LIMIT ?1", &[&lim])
}

/// Collect importable files from a set of paths (files and/or directories).
pub fn collect_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            for entry in walkdir::WalkDir::new(p).into_iter().flatten() {
                let path = entry.path();
                if path.is_file() && is_supported(path) {
                    out.push(path.to_path_buf());
                }
            }
        } else if p.is_file() && is_supported(p) {
            out.push(p.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Texture sets (spec §10, §11, §31) — a relationship over individual textures.
// ---------------------------------------------------------------------------

/// Slots a complete PBR set is expected to fill, for missing-map reporting.
pub const EXPECTED_PBR: &[&str] = &["base_color", "roughness", "normal", "height", "ao"];

/// One member of a texture set: which slot, and which texture fills it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureSetMap {
    pub slot: String,
    pub texture_id: String,
    pub name: String,
}

/// A texture set shaped for the UI (spec §11/§31).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureSetDto {
    pub id: String,
    pub name: String,
    pub resolution: Option<String>,
    pub is_pbr: bool,
    pub tileable: Option<bool>,
    pub maps: Vec<TextureSetMap>,
    /// Expected PBR slots not present in this set.
    pub missing_maps: Vec<String>,
    pub member_count: usize,
}

fn parent_dir(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Friendly resolution label used for grouping/keys, e.g. 4096² → "4K".
pub fn res_label(w: Option<i64>, h: Option<i64>) -> String {
    match (w, h) {
        (Some(w), Some(h)) if w == h => match w {
            512 => "512".into(),
            1024 => "1K".into(),
            2048 => "2K".into(),
            4096 => "4K".into(),
            8192 => "8K".into(),
            _ => format!("{w}x{h}"),
        },
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => String::new(),
    }
}

/// Rebuild all texture sets from the current textures (spec §10). Sets are
/// derived data, so this clears and regenerates them. Textures are grouped by
/// (folder, base name, resolution); a group with ≥2 distinct map slots becomes a
/// set. Individual textures always remain independently searchable.
pub fn rebuild_texture_sets(conn: &Connection) -> Result<usize> {
    // Clear derived sets (cascades to texture_maps + the set's preview row).
    conn.execute("DELETE FROM assets WHERE kind = 'texture_set'", [])?;

    struct Cand {
        id: String,
        name: String,
        slot: String,
        w: Option<i64>,
        h: Option<i64>,
        dir: String,
        tileable: Option<i64>,
    }

    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, t.map_type, t.width, t.height, t.file_path, t.tileable
         FROM assets a JOIN textures t ON t.asset_id = a.id
         WHERE a.kind = 'texture' AND t.map_type IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        let path: String = r.get(5)?;
        Ok(Cand {
            id: r.get(0)?,
            name: r.get(1)?,
            slot: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            dir: parent_dir(&path),
            tileable: r.get(6)?,
        })
    })?;

    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String, String), Vec<Cand>> = BTreeMap::new();
    for r in rows {
        let c = r?;
        let key = (c.dir.clone(), c.name.to_lowercase(), res_label(c.w, c.h));
        groups.entry(key).or_default().push(c);
    }

    let ts = now();
    let mut count = 0usize;
    for ((_, _, res), members) in groups {
        // One texture per slot (first wins on a collision); a set needs ≥2 slots.
        let mut slots: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in &members {
            if seen.insert(m.slot.clone()) {
                slots.push((m.slot.clone(), m.id.clone()));
            }
        }
        if slots.len() < 2 {
            continue;
        }

        let name = members[0].name.clone();
        let tileable = members.iter().find_map(|m| m.tileable);
        let present: std::collections::HashSet<&str> = slots.iter().map(|(s, _)| s.as_str()).collect();
        let is_pbr = present.contains("base_color")
            && present.contains("normal")
            && (present.contains("roughness")
                || present.contains("glossiness")
                || present.contains("metallic"));

        let set_id = new_id(AssetKind::TextureSet);
        conn.execute(
            "INSERT INTO assets (id, kind, name, category, favorite, created_at, updated_at)
             VALUES (?1, 'texture_set', ?2, 'set', 0, ?3, ?3)",
            params![set_id, name, ts],
        )?;
        conn.execute(
            "INSERT INTO texture_sets (asset_id, resolution, is_pbr, tileable) VALUES (?1,?2,?3,?4)",
            params![
                set_id,
                if res.is_empty() { None } else { Some(res) },
                is_pbr as i64,
                tileable
            ],
        )?;
        for (slot, tex_id) in &slots {
            conn.execute(
                "INSERT INTO texture_maps (set_id, slot, texture_id) VALUES (?1, ?2, ?3)",
                params![set_id, slot, tex_id],
            )?;
        }
        count += 1;
    }
    Ok(count)
}

fn set_maps(conn: &Connection, set_id: &str) -> Result<Vec<TextureSetMap>> {
    let mut stmt = conn.prepare(
        "SELECT m.slot, m.texture_id, a.name
         FROM texture_maps m JOIN assets a ON a.id = m.texture_id
         WHERE m.set_id = ?1 ORDER BY m.slot",
    )?;
    let rows = stmt.query_map([set_id], |r| {
        Ok(TextureSetMap {
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

/// List all texture sets, with present/missing map slots computed.
pub fn list_texture_sets(conn: &Connection) -> Result<Vec<TextureSetDto>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, s.resolution, s.is_pbr, s.tileable
         FROM assets a JOIN texture_sets s ON s.asset_id = a.id
         WHERE a.kind = 'texture_set' ORDER BY a.created_at DESC, a.name",
    )?;
    let heads: Vec<(String, String, Option<String>, bool, Option<bool>)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)? != 0,
                r.get::<_, Option<i64>>(4)?.map(|v| v != 0),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out = Vec::new();
    for (id, name, resolution, is_pbr, tileable) in heads {
        let maps = set_maps(conn, &id)?;
        let present: std::collections::HashSet<&str> = maps.iter().map(|m| m.slot.as_str()).collect();
        let missing_maps = EXPECTED_PBR
            .iter()
            .filter(|s| !present.contains(**s))
            .map(|s| s.to_string())
            .collect();
        let member_count = maps.len();
        out.push(TextureSetDto {
            id,
            name,
            resolution,
            is_pbr,
            tileable,
            maps,
            missing_maps,
            member_count,
        });
    }
    Ok(out)
}

/// Fetch a single texture set by id.
pub fn get_texture_set(conn: &Connection, id: &str) -> Result<Option<TextureSetDto>> {
    Ok(list_texture_sets(conn)?.into_iter().find(|s| s.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use image::{Rgb, RgbImage};

    fn write_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let mut img = RgbImage::new(w, h);
        for px in img.pixels_mut() {
            *px = Rgb([120, 90, 60]);
        }
        let p = dir.join(name);
        img.save(&p).unwrap();
        p
    }

    #[test]
    fn analyze_reads_metadata_and_map_type() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_png(dir.path(), "Concrete_Roughness_4K.png", 64, 32);
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&file, &reg).unwrap();
        assert_eq!(a.map_type.as_deref(), Some("roughness"));
        assert_eq!(a.width, Some(64));
        assert_eq!(a.height, Some(32));
        assert_eq!(a.format, "png");
        assert_eq!(a.color_space.as_deref(), Some("linear"));
        assert_eq!(a.name, "Concrete"); // map token + res stripped
        assert!(!a.hash.is_empty());
    }

    #[test]
    fn udim_is_detected() {
        let (is_udim, tile) = detect_udim("character_body.1002.exr");
        assert!(is_udim);
        assert_eq!(tile, Some(1002));
        assert_eq!(detect_udim("plain_texture.png"), (false, None));
    }

    #[test]
    fn import_creates_records_and_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_png(dir.path(), "wood_basecolor.png", 40, 40);
        let thumbs = dir.path().join("thumbs");

        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&file, &reg).unwrap();
        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: thumbs.clone(),
            generate_preview: true,
            detect_maps: true,
        };
        let outcome = import_texture(db.conn(), &a, &opts).unwrap();
        let id = match outcome {
            ImportOutcome::Imported { id, .. } => id,
            other => panic!("expected import, got {other:?}"),
        };

        // Record + hash + preview present.
        let list = list_textures(db.conn(), None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].map_type.as_deref(), Some("base_color"));
        assert!(list[0].thumbnail_path.is_some());
        assert!(thumbs.join(format!("{id}.png")).exists());

        // Re-importing the same path is a no-op duplicate.
        let again = import_texture(db.conn(), &a, &opts).unwrap();
        assert!(matches!(again, ImportOutcome::DuplicatePath { .. }));
        assert_eq!(list_textures(db.conn(), None).unwrap().len(), 1);
    }

    #[test]
    fn channels_and_tileable_populated_on_import() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();

        // analyze() reads channels from the header (RGB PNG → 3 channels).
        let a = analyze(&write_png(dir.path(), "wall_basecolor.png", 64, 64), &reg).unwrap();
        assert_eq!(a.channels, Some(3));

        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.path().join("_t"),
            generate_preview: true,
            detect_maps: true,
        };
        let id = match import_texture(db.conn(), &a, &opts).unwrap() {
            ImportOutcome::Imported { id, .. } => id,
            other => panic!("{other:?}"),
        };
        let t = get_texture(db.conn(), &id).unwrap().unwrap();
        assert_eq!(t.channels, Some(3));
        // A solid image has identical opposite edges → detected as tileable.
        assert_eq!(t.tileable, Some(true));
    }

    #[test]
    fn backfill_fills_tileable_when_preview_was_off() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&write_png(dir.path(), "floor_basecolor.png", 48, 48), &reg).unwrap();
        // No preview → tileable not computed at import (stays unknown).
        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.path().join("_t"),
            generate_preview: false,
            detect_maps: true,
        };
        let id = match import_texture(db.conn(), &a, &opts).unwrap() {
            ImportOutcome::Imported { id, .. } => id,
            other => panic!("{other:?}"),
        };
        assert_eq!(get_texture(db.conn(), &id).unwrap().unwrap().tileable, None);

        // Backfill decodes the file and fills the missing flag.
        let n = backfill_texture_metadata(db.conn()).unwrap();
        assert_eq!(n, 1);
        assert_eq!(get_texture(db.conn(), &id).unwrap().unwrap().tileable, Some(true));
    }

    #[test]
    fn packed_arm_map_unpacks_into_component_maps() {
        let dir = tempfile::tempdir().unwrap();
        let reg = MapTypeRegistry::builtin();
        // ARM: R = AO (50), G = Roughness (150), B = Metallic (250).
        let path = dir.path().join("Rock_ARM_2K.png");
        let mut img = RgbImage::new(16, 16);
        for px in img.pixels_mut() {
            *px = Rgb([50, 150, 250]);
        }
        img.save(&path).unwrap();

        assert_eq!(detect_packed("Rock_ARM_2K.png"), Some(["ao", "roughness", "metallic"]));

        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.path().join("_t"),
            generate_preview: false,
            detect_maps: true,
        };
        let prepared = prepare_import(&path, &opts, &reg).unwrap();
        assert_eq!(prepared.len(), 3);
        let slots: std::collections::HashSet<_> =
            prepared.iter().filter_map(|a| a.map_type.clone()).collect();
        assert!(slots.contains("ao"));
        assert!(slots.contains("roughness"));
        assert!(slots.contains("metallic"));
        // Component siblings were written next to the source.
        assert!(dir.path().join("Rock_ARM_2K_roughness.png").exists());

        // With map detection off, the packed file is imported as-is (no split).
        let opts_off = ImportOptions {
            detect_maps: false,
            ..opts.clone()
        };
        assert_eq!(prepare_import(&path, &opts_off, &reg).unwrap().len(), 1);
    }

    #[test]
    fn scoped_fetches_return_only_requested_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.path().join("_t"),
            generate_preview: false,
            detect_maps: true,
        };
        let mut ids = Vec::new();
        for n in ["a_basecolor.png", "b_roughness.png", "c_normal.png"] {
            let a = analyze(&write_png(dir.path(), n, 16, 16), &reg).unwrap();
            match import_texture(db.conn(), &a, &opts).unwrap() {
                ImportOutcome::Imported { id, .. } => ids.push(id),
                other => panic!("{other:?}"),
            }
        }

        // get_texture returns exactly the requested row (indexed lookup).
        let got = get_texture(db.conn(), &ids[1]).unwrap().unwrap();
        assert_eq!(got.id, ids[1]);
        assert!(get_texture(db.conn(), "NX-TEX-0000-0000").unwrap().is_none());

        // by_ids returns just the subset; empty input is empty without a query.
        let subset = list_textures_by_ids(db.conn(), &[ids[0].clone(), ids[2].clone()]).unwrap();
        assert_eq!(subset.len(), 2);
        let returned: std::collections::HashSet<_> = subset.iter().map(|t| t.id.clone()).collect();
        assert!(returned.contains(&ids[0]) && returned.contains(&ids[2]));
        assert!(!returned.contains(&ids[1]));
        assert!(list_textures_by_ids(db.conn(), &[]).unwrap().is_empty());
    }

    #[test]
    fn managed_import_copies_into_library() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_png(dir.path(), "metal_normal.png", 20, 20);
        let lib = dir.path().join("LIB");

        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&file, &reg).unwrap();
        let opts = ImportOptions {
            managed: true,
            library_root: Some(lib.clone()),
            thumbnail_dir: dir.path().join("thumbs"),
            generate_preview: false,
            detect_maps: true,
        };
        import_texture(db.conn(), &a, &opts).unwrap();

        let copied = lib.join("Textures/Normal/metal_normal.png");
        assert!(copied.exists(), "file should be copied into managed library");
        let list = list_textures(db.conn(), None).unwrap();
        assert!(list[0].managed);
        assert_eq!(list[0].file_path, copied.to_string_lossy());
    }

    #[test]
    fn filter_by_map_type() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = dir.path().join("t");
        let db = Database::open_in_memory().unwrap();
        let reg = MapTypeRegistry::builtin();
        for n in ["a_basecolor.png", "a_roughness.png", "a_normal.png"] {
            let f = write_png(dir.path(), n, 8, 8);
            let a = analyze(&f, &reg).unwrap();
            let opts = ImportOptions {
                managed: false,
                library_root: None,
                thumbnail_dir: thumbs.clone(),
                generate_preview: false,
                detect_maps: true,
            };
            import_texture(db.conn(), &a, &opts).unwrap();
        }
        assert_eq!(list_textures(db.conn(), Some("roughness")).unwrap().len(), 1);
        assert_eq!(list_textures(db.conn(), None).unwrap().len(), 3);
    }

    fn quick_import(db: &Database, dir: &Path, name: &str, w: u32, h: u32) {
        let f = write_png(dir, name, w, h);
        let reg = MapTypeRegistry::builtin();
        let a = analyze(&f, &reg).unwrap();
        let opts = ImportOptions {
            managed: false,
            library_root: None,
            thumbnail_dir: dir.join("_t"),
            generate_preview: false,
            detect_maps: true,
        };
        import_texture(db.conn(), &a, &opts).unwrap();
    }

    #[test]
    fn udim_tiles_collapse_into_one_texture_with_missing_detection() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        // Tiles 1001, 1002, 1004 — 1003 is missing.
        for name in ["body.1001.png", "body.1002.png", "body.1004.png"] {
            quick_import(&db, dir.path(), name, 16, 16);
        }
        // One collapsed texture, flagged UDIM.
        let list = list_textures(db.conn(), None).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_udim);
        assert!(list[0].file_path.contains("<UDIM>"));

        let tiles = udim_tiles(db.conn(), &list[0].id).unwrap();
        assert_eq!(tiles, vec![1001, 1002, 1004]);
        assert_eq!(missing_udim_tiles(&tiles), vec![1003]);

        // Re-importing an existing tile is a duplicate, not a new tile.
        quick_import(&db, dir.path(), "body.1001.png", 16, 16);
        assert_eq!(udim_tiles(db.conn(), &list[0].id).unwrap().len(), 3);
    }

    #[test]
    fn texture_set_grouping_and_pbr_missing_maps() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        for name in [
            "Concrete_BaseColor_2K.png",
            "Concrete_Roughness_2K.png",
            "Concrete_Normal_2K.png",
        ] {
            quick_import(&db, dir.path(), name, 2048, 2048);
        }
        // A lone texture in a different family shouldn't form a set.
        quick_import(&db, dir.path(), "Brick_Normal_2K.png", 2048, 2048);

        let sets = rebuild_texture_sets(db.conn()).unwrap();
        assert_eq!(sets, 1, "only Concrete has ≥2 slots");

        let listed = list_texture_sets(db.conn()).unwrap();
        assert_eq!(listed.len(), 1);
        let s = &listed[0];
        assert_eq!(s.name, "Concrete");
        assert_eq!(s.resolution.as_deref(), Some("2K"));
        assert!(s.is_pbr, "base_color + roughness + normal = PBR");
        assert_eq!(s.member_count, 3);
        // height and ao are the expected-but-missing slots.
        assert!(s.missing_maps.contains(&"height".to_string()));
        assert!(s.missing_maps.contains(&"ao".to_string()));
        assert!(!s.missing_maps.contains(&"base_color".to_string()));

        // Individual textures remain independently searchable.
        assert_eq!(list_textures(db.conn(), Some("roughness")).unwrap().len(), 1);

        // Rebuild is idempotent (no duplicate sets).
        let again = rebuild_texture_sets(db.conn()).unwrap();
        assert_eq!(again, 1);
    }
}
