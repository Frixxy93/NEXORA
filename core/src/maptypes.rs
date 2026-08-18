//! Configurable texture-map recognition registry (spec §9).
//!
//! Detection must **not** be a single hard-coded function. Instead the registry
//! holds one [`MapRule`] per map type, each with an ordered list of filename
//! tokens/patterns. New rules — including user-defined ones from Settings — can
//! be appended without touching detection logic.
//!
//! Phase 1 ships the registry and its matcher with tests. Phase 2 wires it into
//! the importer/scanner. Nothing here touches the filesystem.

use serde::{Deserialize, Serialize};

/// The canonical map types NEXORA understands. `Custom` carries a user label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapType {
    BaseColor,
    Roughness,
    Glossiness,
    Metallic,
    Normal,
    Height,
    Displacement,
    Bump,
    AmbientOcclusion,
    Specular,
    Opacity,
    Emission,
    Transmission,
    Thickness,
    Mask,
    Id,
    Custom(String),
}

impl MapType {
    /// Stable, storage-friendly slug.
    pub fn slug(&self) -> String {
        match self {
            MapType::BaseColor => "base_color".into(),
            MapType::Roughness => "roughness".into(),
            MapType::Glossiness => "glossiness".into(),
            MapType::Metallic => "metallic".into(),
            MapType::Normal => "normal".into(),
            MapType::Height => "height".into(),
            MapType::Displacement => "displacement".into(),
            MapType::Bump => "bump".into(),
            MapType::AmbientOcclusion => "ao".into(),
            MapType::Specular => "specular".into(),
            MapType::Opacity => "opacity".into(),
            MapType::Emission => "emission".into(),
            MapType::Transmission => "transmission".into(),
            MapType::Thickness => "thickness".into(),
            MapType::Mask => "mask".into(),
            MapType::Id => "id".into(),
            MapType::Custom(label) => format!("custom:{label}"),
        }
    }
}

/// A single recognition rule: a map type plus the tokens that indicate it.
///
/// `tokens` are matched case-insensitively against the `_`/`-`/`.`/space
/// separated segments of a filename stem. Order matters at the registry level:
/// earlier rules win ties, so more specific tokens should live in earlier rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRule {
    pub map_type: MapType,
    pub tokens: Vec<String>,
    /// User-added rules are flagged so Settings can edit/remove only these.
    #[serde(default)]
    pub user_defined: bool,
}

impl MapRule {
    fn builtin(map_type: MapType, tokens: &[&str]) -> Self {
        MapRule {
            map_type,
            tokens: tokens.iter().map(|s| s.to_string()).collect(),
            user_defined: false,
        }
    }
}

/// The ordered set of recognition rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapTypeRegistry {
    pub rules: Vec<MapRule>,
}

impl Default for MapTypeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl MapTypeRegistry {
    /// The default registry covering the naming conventions in spec §9.
    ///
    /// More specific tokens are listed before generic ones (e.g. `normalgl`
    /// before `normal`, `metalness` before `metal`) so a filename like
    /// `wood_normalgl.png` resolves to Normal on its first, most specific hit.
    pub fn builtin() -> Self {
        let rules = vec![
            MapRule::builtin(
                MapType::BaseColor,
                &["basecolor", "base_color", "albedo", "diffuse", "color", "col", "diff"],
            ),
            MapRule::builtin(
                MapType::AmbientOcclusion,
                &["ambientocclusion", "ao", "occlusion", "occ"],
            ),
            MapRule::builtin(
                MapType::Normal,
                &["normalgl", "normaldx", "normal", "nrm", "norm", "nor"],
            ),
            MapRule::builtin(
                MapType::Roughness,
                &["roughness", "rough", "rgh"],
            ),
            MapRule::builtin(
                MapType::Glossiness,
                &["glossiness", "gloss", "glossy"],
            ),
            MapRule::builtin(
                MapType::Metallic,
                &["metallic", "metalness", "metal", "mtl", "met"],
            ),
            MapRule::builtin(
                MapType::Displacement,
                &["displacement", "disp", "dsp"],
            ),
            MapRule::builtin(
                MapType::Height,
                &["height", "hght", "hgt"],
            ),
            MapRule::builtin(
                MapType::Bump,
                &["bump", "bmp"],
            ),
            MapRule::builtin(
                MapType::Specular,
                &["specular", "spec", "spc"],
            ),
            MapRule::builtin(
                MapType::Opacity,
                &["opacity", "alpha", "opac", "transparency"],
            ),
            MapRule::builtin(
                MapType::Emission,
                &["emission", "emissive", "emit", "glow"],
            ),
            MapRule::builtin(
                MapType::Transmission,
                &["transmission", "transmittance", "trans"],
            ),
            MapRule::builtin(
                MapType::Thickness,
                &["thickness", "thick"],
            ),
            MapRule::builtin(
                MapType::Mask,
                &["mask", "msk"],
            ),
            MapRule::builtin(
                MapType::Id,
                &["matid", "idmap", "id"],
            ),
        ];
        MapTypeRegistry { rules }
    }

    /// Add a user pattern (from Settings) for an existing or custom map type.
    pub fn add_user_rule(&mut self, map_type: MapType, tokens: Vec<String>) {
        self.rules.push(MapRule {
            map_type,
            tokens,
            user_defined: true,
        });
    }

    /// Detect the map type of a filename, if any rule matches.
    ///
    /// The stem is lower-cased and split on separators; the first rule with a
    /// token appearing as a whole segment (or as a suffix of the last segment,
    /// to catch `wood4k_rough`) wins.
    pub fn detect(&self, filename: &str) -> Option<MapType> {
        let stem = strip_extension(filename).to_lowercase();
        let segments: Vec<&str> = stem
            .split(|c: char| c == '_' || c == '-' || c == '.' || c == ' ')
            .filter(|s| !s.is_empty())
            .collect();

        for rule in &self.rules {
            for token in &rule.tokens {
                // Whole-segment match is the strongest signal.
                if segments.iter().any(|seg| *seg == token) {
                    return Some(rule.map_type.clone());
                }
            }
        }
        // Fallback: substring match against the final segment (handles glued
        // names like `woodrough`), still respecting rule specificity order.
        if let Some(last) = segments.last() {
            for rule in &self.rules {
                for token in &rule.tokens {
                    if token.len() >= 3 && last.ends_with(token.as_str()) {
                        return Some(rule.map_type.clone());
                    }
                }
            }
        }
        None
    }
}

/// Strip a single trailing extension (`.jpg`, `.exr`, ...) from a filename.
fn strip_extension(filename: &str) -> &str {
    match filename.rfind('.') {
        Some(idx) if idx > 0 => &filename[..idx],
        _ => filename,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_conventions() {
        let r = MapTypeRegistry::builtin();
        assert_eq!(r.detect("wood_basecolor.jpg"), Some(MapType::BaseColor));
        assert_eq!(r.detect("wood_albedo.jpg"), Some(MapType::BaseColor));
        assert_eq!(r.detect("wood_diffuse.jpg"), Some(MapType::BaseColor));
        assert_eq!(r.detect("wood_roughness.jpg"), Some(MapType::Roughness));
        assert_eq!(r.detect("wood_rough.jpg"), Some(MapType::Roughness));
        assert_eq!(r.detect("wood_metalness.jpg"), Some(MapType::Metallic));
        assert_eq!(r.detect("wood_nrm.jpg"), Some(MapType::Normal));
        assert_eq!(r.detect("wood_normalgl.jpg"), Some(MapType::Normal));
        assert_eq!(r.detect("wood_disp.exr"), Some(MapType::Displacement));
        assert_eq!(r.detect("wood_height.exr"), Some(MapType::Height));
        assert_eq!(r.detect("wood_ao.jpg"), Some(MapType::AmbientOcclusion));
        assert_eq!(r.detect("wood_opacity.png"), Some(MapType::Opacity));
        assert_eq!(r.detect("wood_alpha.png"), Some(MapType::Opacity));
        assert_eq!(r.detect("wood_emission.jpg"), Some(MapType::Emission));
    }

    #[test]
    fn specificity_normalgl_not_confused() {
        let r = MapTypeRegistry::builtin();
        // "normalgl" must resolve to Normal, not be missed.
        assert_eq!(r.detect("Concrete_NormalGL_4K.png"), Some(MapType::Normal));
    }

    #[test]
    fn unknown_returns_none() {
        let r = MapTypeRegistry::builtin();
        assert_eq!(r.detect("random_photo.jpg"), None);
    }

    #[test]
    fn user_rule_is_matched() {
        let mut r = MapTypeRegistry::builtin();
        r.add_user_rule(MapType::Custom("cavity".into()), vec!["cavity".into()]);
        assert_eq!(
            r.detect("rock_cavity.png"),
            Some(MapType::Custom("cavity".into()))
        );
    }
}
