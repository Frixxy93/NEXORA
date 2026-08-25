//! Poly Haven CC0 texture provider.
//!
//! Public API (no key needed): <https://api.polyhaven.com>. Every asset is CC0,
//! so downloading and re-storing it is unrestricted. We list texture assets,
//! read each asset's file manifest, pick a resolution + format per map, and name
//! the downloaded files with NEXORA-recognizable tokens so the existing
//! map-type detector and material importer handle them unchanged.

use crate::{CoreError, Result};
use std::io::Read;

pub const API: &str = "https://api.polyhaven.com";

/// A blocking HTTP agent with sane timeouts and a descriptive UA.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(180))
        .user_agent("NEXORA/1.0 (+https://github.com/Frixxy93/NEXORA)")
        .build()
}

fn net_err<E: std::fmt::Display>(ctx: &str, e: E) -> CoreError {
    CoreError::Provider(format!("{ctx}: {e}"))
}

/// Every texture asset id in the Poly Haven catalog.
pub fn list_texture_ids(agent: &ureq::Agent) -> Result<Vec<String>> {
    let body = agent
        .get(&format!("{API}/assets?type=textures"))
        .call()
        .map_err(|e| net_err("list assets", e))?
        .into_string()?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&body)?;
    Ok(map.into_iter().map(|(k, _)| k).collect())
}

/// Fetch the browsable catalog: every texture asset with its name, thumbnail URL,
/// and categories (for the Discover "Browse" grid). `synced` is left false here;
/// the caller flags already-imported ones.
pub fn list_catalog(agent: &ureq::Agent) -> Result<Vec<super::CatalogAsset>> {
    let body = agent
        .get(&format!("{API}/assets?type=textures"))
        .call()
        .map_err(|e| net_err("list catalog", e))?
        .into_string()?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&body)?;

    let mut out = Vec::with_capacity(map.len());
    for (id, v) in map {
        let name = v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&id)
            .to_string();
        let thumbnail_url = v
            .get("thumbnail_url")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let categories = v
            .get("categories")
            .and_then(|c| c.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        out.push(super::CatalogAsset {
            source: super::SOURCE_POLYHAVEN.to_string(),
            id,
            name,
            thumbnail_url,
            categories,
            synced: false,
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// One file to fetch for an asset.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedFile {
    /// e.g. `rock_wall_10_diffuse.jpg` — the token drives map-type detection.
    pub filename: String,
    pub url: String,
    pub size: u64,
}

/// Poly Haven map key → the NEXORA filename token that the map registry detects.
/// Returns `None` for keys we don't import (normal-DX duplicate, packed ARM,
/// mesh formats, etc.).
fn map_token(key: &str) -> Option<&'static str> {
    match key {
        "Diffuse" => Some("diffuse"),
        "nor_gl" => Some("normal"), // OpenGL normal (three.js/Maya convention)
        "Rough" => Some("roughness"),
        "AO" => Some("ao"),
        "Displacement" => Some("displacement"),
        "Metal" | "Metalness" => Some("metallic"),
        "Emission" => Some("emission"),
        _ => None, // nor_dx, arm, blend, gltf, mtlx, spec, ...
    }
}

/// Pick a format node for a resolution: jpg (small) → png → exr.
fn pick_format(res_node: &serde_json::Value) -> Option<(&'static str, &serde_json::Value)> {
    for fmt in ["jpg", "png", "exr"] {
        if let Some(f) = res_node.get(fmt) {
            if f.get("url").and_then(|u| u.as_str()).is_some() {
                return Some((fmt, f));
            }
        }
    }
    None
}

/// Build the download plan from an asset's `/files/{id}` JSON, targeting
/// `res_key` (e.g. "1k"), falling back to the smallest available resolution.
pub fn plan_from_files_json(id: &str, files: &serde_json::Value, res_key: &str) -> Vec<PlannedFile> {
    let mut plan = Vec::new();
    let Some(obj) = files.as_object() else { return plan };
    for (key, node) in obj {
        let Some(token) = map_token(key) else { continue };
        let res_node = node.get(res_key).or_else(|| {
            ["1k", "2k", "4k", "8k"].iter().find_map(|r| node.get(*r))
        });
        let Some(res_node) = res_node else { continue };
        if let Some((fmt, f)) = pick_format(res_node) {
            let url = f.get("url").and_then(|u| u.as_str()).unwrap_or_default();
            if url.is_empty() {
                continue;
            }
            let size = f.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
            plan.push(PlannedFile {
                filename: format!("{id}_{token}.{fmt}"),
                url: url.to_string(),
                size,
            });
        }
    }
    plan
}

/// Fetch the file manifest for `id` and build a download plan.
pub fn download_plan(agent: &ureq::Agent, id: &str, res_key: &str) -> Result<Vec<PlannedFile>> {
    let body = agent
        .get(&format!("{API}/files/{id}"))
        .call()
        .map_err(|e| net_err("files", e))?
        .into_string()?;
    let files: serde_json::Value = serde_json::from_str(&body)?;
    Ok(plan_from_files_json(id, &files, res_key))
}

/// Download one URL's bytes (capped at 250 MB to bound memory).
pub fn download_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let resp = agent.get(url).call().map_err(|e| net_err("download", e))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(250 * 1024 * 1024)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed real `/files/rock_wall_10` shape: map keys → res → format → meta.
    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "Diffuse": {
                "1k": { "jpg": { "size": 100, "url": "https://dl/rock_wall_10_diff_1k.jpg" },
                        "png": { "size": 200, "url": "https://dl/rock_wall_10_diff_1k.png" } },
                "4k": { "jpg": { "size": 900, "url": "https://dl/rock_wall_10_diff_4k.jpg" } }
            },
            "nor_gl": {
                "1k": { "png": { "size": 300, "url": "https://dl/rock_wall_10_nor_gl_1k.png" } }
            },
            "nor_dx": {
                "1k": { "png": { "size": 300, "url": "https://dl/rock_wall_10_nor_dx_1k.png" } }
            },
            "Rough": {
                "1k": { "jpg": { "size": 120, "url": "https://dl/rock_wall_10_rough_1k.jpg" } }
            },
            "AO":   { "1k": { "jpg": { "size": 110, "url": "https://dl/rock_wall_10_ao_1k.jpg" } } },
            "Displacement": { "1k": { "png": { "size": 400, "url": "https://dl/rock_wall_10_disp_1k.png" } } },
            "arm":  { "1k": { "jpg": { "size": 130, "url": "https://dl/rock_wall_10_arm_1k.jpg" } } },
            "blend": { "blend": { "size": 5, "url": "https://dl/x.blend" } }
        })
    }

    #[test]
    fn plan_picks_pbr_maps_at_resolution_and_prefers_jpg() {
        let plan = plan_from_files_json("rock_wall_10", &fixture(), "1k");
        let names: Vec<&str> = plan.iter().map(|p| p.filename.as_str()).collect();
        // Imports the PBR set with recognizable tokens; skips nor_dx/arm/blend.
        assert!(names.contains(&"rock_wall_10_diffuse.jpg")); // jpg preferred over png
        assert!(names.contains(&"rock_wall_10_normal.png")); // nor_gl only in png
        assert!(names.contains(&"rock_wall_10_roughness.jpg"));
        assert!(names.contains(&"rock_wall_10_ao.jpg"));
        assert!(names.contains(&"rock_wall_10_displacement.png"));
        assert!(!names.iter().any(|n| n.contains("arm")));
        assert!(!names.iter().any(|n| n.contains("nor_dx")));
        assert!(!names.iter().any(|n| n.contains("blend")));
        assert_eq!(plan.len(), 5);
    }

    #[test]
    fn plan_falls_back_when_resolution_missing() {
        // Diffuse has no 2k node here → falls back to an available resolution.
        let plan = plan_from_files_json("rock_wall_10", &fixture(), "2k");
        assert!(plan.iter().any(|p| p.filename == "rock_wall_10_diffuse.jpg"));
    }
}
