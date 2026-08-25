//! ambientCG CC0 texture provider.
//!
//! Public API (no key needed): <https://ambientcg.com/api/v2>. Every asset is
//! CC0, so downloading and re-storing is unrestricted. Unlike Poly Haven (which
//! serves one file per map), ambientCG ships each material as a single ZIP
//! bundle. We page the catalog for Material assets, pick the ZIP download for the
//! chosen resolution, then extract the individual map images from the archive.
//! ambientCG's own map filenames (Color, NormalGL, Roughness, Displacement,
//! AmbientOcclusion, Metalness) are already recognized by the map-type registry,
//! so extracted files import unchanged.

use crate::{CoreError, Result};
use std::io::{Cursor, Read};
use std::path::Path;

pub const API: &str = "https://ambientcg.com/api/v2";

fn net_err<E: std::fmt::Display>(ctx: &str, e: E) -> CoreError {
    CoreError::Provider(format!("{ctx}: {e}"))
}

/// One material bundle to fetch: its id and the ZIP download URL.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialBundle {
    pub asset_id: String,
    pub url: String,
    pub size: u64,
}

/// From one asset's `downloadData`, pick the ZIP download whose `attribute`
/// matches the wanted resolution, preferring JPG then falling back to PNG.
/// `res_prefix` is e.g. "1K".
fn pick_zip_download(asset: &serde_json::Value, res_prefix: &str) -> Option<(String, u64)> {
    let downloads = asset
        .pointer("/downloadFolders/default/downloadFiletypeCategories/zip/downloads")?
        .as_array()?;

    // Try JPG first (smaller), then PNG.
    for suffix in ["-JPG", "-PNG"] {
        let want = format!("{res_prefix}{suffix}");
        for d in downloads {
            let attr = d.get("attribute").and_then(|a| a.as_str()).unwrap_or("");
            if attr.eq_ignore_ascii_case(&want) {
                let url = d.get("downloadLink").and_then(|u| u.as_str())?;
                let size = d.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                return Some((url.to_string(), size));
            }
        }
    }
    None
}

/// Parse one page of `full_json` into the material bundles it contains.
pub fn parse_page(body: &serde_json::Value, res_prefix: &str) -> Vec<MaterialBundle> {
    let mut out = Vec::new();
    let Some(assets) = body.get("foundAssets").and_then(|a| a.as_array()) else {
        return out;
    };
    for asset in assets {
        let Some(id) = asset.get("assetId").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some((url, size)) = pick_zip_download(asset, res_prefix) {
            out.push(MaterialBundle {
                asset_id: id.to_string(),
                url,
                size,
            });
        }
    }
    out
}

/// Predictable 256px thumbnail URL for an asset (fallback when previewImage is
/// absent). Served by ambientCG's media CDN.
fn thumb_url(asset_id: &str) -> String {
    format!(
        "https://acg-media.struffelproductions.com/file/ambientCG-Web/media/thumbnail/256-PNG/{asset_id}.png"
    )
}

/// Fetch the browsable catalog: every Material with its display name, category,
/// and 256px thumbnail URL (for the Discover "Browse" grid). Pages the API;
/// `synced` is left false — the caller flags already-imported assets.
pub fn list_catalog(agent: &ureq::Agent) -> Result<Vec<super::CatalogAsset>> {
    const LIMIT: usize = 100;
    let mut offset = 0usize;
    let mut out: Vec<super::CatalogAsset> = Vec::new();

    loop {
        let url = format!(
            "{API}/full_json?type=Material&include=displayData,previewData&limit={LIMIT}&offset={offset}"
        );
        let body = agent
            .get(&url)
            .call()
            .map_err(|e| net_err("list catalog", e))?
            .into_string()?;
        let json: serde_json::Value = serde_json::from_str(&body)?;
        let Some(assets) = json.get("foundAssets").and_then(|a| a.as_array()) else {
            break;
        };
        let len = assets.len();
        if len == 0 {
            break;
        }
        for asset in assets {
            let Some(id) = asset.get("assetId").and_then(|v| v.as_str()) else {
                continue;
            };
            let name = asset
                .get("displayName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(id)
                .to_string();
            let thumbnail_url = asset
                .pointer("/previewImage/256-PNG")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| thumb_url(id));
            let categories = asset
                .get("displayCategory")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|c| vec![c.to_lowercase()])
                .unwrap_or_default();
            out.push(super::CatalogAsset {
                source: super::SOURCE_AMBIENTCG.to_string(),
                id: id.to_string(),
                name,
                thumbnail_url,
                categories,
                synced: false,
            });
        }
        offset += len;
        if len < LIMIT {
            break;
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Resolve the ZIP bundle URL for a single asset at `res_prefix` (used to
/// download a specific pick from the Browse grid, JPG→PNG fallback).
pub fn bundle_url(agent: &ureq::Agent, id: &str, res_prefix: &str) -> Result<Option<String>> {
    let url = format!("{API}/full_json?id={id}&include=downloadData");
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| net_err("bundle url", e))?
        .into_string()?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    let asset = json
        .get("foundAssets")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first());
    Ok(asset.and_then(|a| pick_zip_download(a, res_prefix)).map(|(u, _)| u))
}

/// List every Material bundle available at `res_prefix` (e.g. "1K"), paging the
/// catalog. Materials without a matching ZIP are skipped.
pub fn list_material_bundles(agent: &ureq::Agent, res_prefix: &str) -> Result<Vec<MaterialBundle>> {
    const LIMIT: usize = 100;
    let mut offset = 0usize;
    let mut out: Vec<MaterialBundle> = Vec::new();

    loop {
        let url = format!(
            "{API}/full_json?type=Material&include=downloadData&limit={LIMIT}&offset={offset}"
        );
        let body = agent
            .get(&url)
            .call()
            .map_err(|e| net_err("list materials", e))?
            .into_string()?;
        let json: serde_json::Value = serde_json::from_str(&body)?;

        let page_len = json
            .get("foundAssets")
            .and_then(|a| a.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if page_len == 0 {
            break;
        }

        out.extend(parse_page(&json, res_prefix));
        offset += page_len;

        // Safety valve: the catalog is a few thousand assets, not tens of
        // thousands. Stop if a page returns fewer than the limit (last page).
        if page_len < LIMIT {
            break;
        }
    }

    Ok(out)
}

/// True for a real PBR map image we want to import; false for previews, spheres,
/// thumbnails, and non-image sidecars (usage docs, etc.).
pub fn is_map_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext_ok = [".png", ".jpg", ".jpeg", ".tif", ".tiff", ".exr", ".bmp", ".tga"]
        .iter()
        .any(|e| lower.ends_with(e));
    if !ext_ok {
        return false;
    }
    // ambientCG bundles include preview renders and sphere/cube renders we don't
    // want as material maps.
    let skip = ["preview", "sphere", "cube", "thumb", "_var"];
    !skip.iter().any(|s| lower.contains(s))
}

/// Download the material ZIP at `url` and extract its map images into `dest_dir`.
/// Returns the number of bytes downloaded. Non-map entries are skipped.
pub fn download_and_extract(agent: &ureq::Agent, url: &str, dest_dir: &Path) -> Result<u64> {
    let resp = agent.get(url).call().map_err(|e| net_err("download", e))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(500 * 1024 * 1024)
        .read_to_end(&mut buf)?;
    let bytes = buf.len() as u64;

    let mut zip = zip::ZipArchive::new(Cursor::new(buf))
        .map_err(|e| CoreError::Provider(format!("open zip: {e}")))?;

    let mut extracted = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| CoreError::Provider(format!("zip entry: {e}")))?;
        if !entry.is_file() {
            continue;
        }
        // Use only the base filename (defend against any path components).
        let raw = entry.name().to_string();
        let base = raw.rsplit(['/', '\\']).next().unwrap_or(&raw);
        if !is_map_image(base) {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        std::fs::write(dest_dir.join(base), &data)?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err(CoreError::Provider("no map images in bundle".into()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed real `full_json` page shape.
    fn page() -> serde_json::Value {
        serde_json::json!({
            "foundAssets": [
                {
                    "assetId": "Bricks075A",
                    "downloadFolders": { "default": { "downloadFiletypeCategories": { "zip": { "downloads": [
                        { "attribute": "1K-JPG", "downloadLink": "https://dl/Bricks075A_1K-JPG.zip", "size": 1000 },
                        { "attribute": "1K-PNG", "downloadLink": "https://dl/Bricks075A_1K-PNG.zip", "size": 3000 },
                        { "attribute": "2K-JPG", "downloadLink": "https://dl/Bricks075A_2K-JPG.zip", "size": 4000 }
                    ] } } } }
                },
                {
                    // Only PNG at 1K → falls back to PNG.
                    "assetId": "Metal001",
                    "downloadFolders": { "default": { "downloadFiletypeCategories": { "zip": { "downloads": [
                        { "attribute": "1K-PNG", "downloadLink": "https://dl/Metal001_1K-PNG.zip", "size": 2000 }
                    ] } } } }
                },
                {
                    // No 1K at all → skipped.
                    "assetId": "Wood002",
                    "downloadFolders": { "default": { "downloadFiletypeCategories": { "zip": { "downloads": [
                        { "attribute": "4K-JPG", "downloadLink": "https://dl/Wood002_4K-JPG.zip", "size": 9000 }
                    ] } } } }
                }
            ]
        })
    }

    #[test]
    fn parse_prefers_jpg_and_falls_back_to_png() {
        let bundles = parse_page(&page(), "1K");
        assert_eq!(bundles.len(), 2);

        let bricks = bundles.iter().find(|b| b.asset_id == "Bricks075A").unwrap();
        assert_eq!(bricks.url, "https://dl/Bricks075A_1K-JPG.zip"); // jpg over png
        assert_eq!(bricks.size, 1000);

        let metal = bundles.iter().find(|b| b.asset_id == "Metal001").unwrap();
        assert_eq!(metal.url, "https://dl/Metal001_1K-PNG.zip"); // png fallback

        // Wood002 has no 1K variant → not present.
        assert!(!bundles.iter().any(|b| b.asset_id == "Wood002"));
    }

    #[test]
    fn map_image_filter_skips_previews_and_nonimages() {
        assert!(is_map_image("Bricks075A_1K_Color.jpg"));
        assert!(is_map_image("Bricks075A_1K_NormalGL.png"));
        assert!(is_map_image("Bricks075A_1K_Roughness.jpg"));
        assert!(!is_map_image("Bricks075A_PREVIEW.png"));
        assert!(!is_map_image("Bricks075A_1K_Sphere.png"));
        assert!(!is_map_image("Bricks075A.usda"));
        assert!(!is_map_image("readme.txt"));
    }
}
