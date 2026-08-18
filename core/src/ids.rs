//! Immutable asset identifiers.
//!
//! Per the NEXORA spec (§43), every asset carries a unique, immutable ID that
//! never changes when the underlying file is renamed or moved. Filenames are
//! **never** used as primary keys. IDs are human-readable so they can appear in
//! logs, the Maya bridge, and support tickets:
//!
//! ```text
//! NX-MAT-7F91-A2C8   (a material)
//! NX-TEX-81D4-9B22   (a texture)
//! NX-SET-4C0F-1E77   (a texture set)
//! ```

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The kind prefix embedded in an asset ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Material,
    Texture,
    TextureSet,
    Collection,
}

impl AssetKind {
    /// The 3-letter code used inside the ID.
    pub fn code(self) -> &'static str {
        match self {
            AssetKind::Material => "MAT",
            AssetKind::Texture => "TEX",
            AssetKind::TextureSet => "SET",
            AssetKind::Collection => "COL",
        }
    }

    /// Parse a kind back out of a 3-letter code.
    pub fn from_code(code: &str) -> Option<AssetKind> {
        match code {
            "MAT" => Some(AssetKind::Material),
            "TEX" => Some(AssetKind::Texture),
            "SET" => Some(AssetKind::TextureSet),
            "COL" => Some(AssetKind::Collection),
            _ => None,
        }
    }
}

/// A freshly generated NEXORA asset ID such as `NX-TEX-81D4-9B22`.
///
/// The two 16-bit groups give ~4.3 billion combinations per kind, which is far
/// beyond the 100k-asset performance target while staying short enough to read.
pub fn new_id(kind: AssetKind) -> String {
    let mut rng = rand::thread_rng();
    let a: u16 = rng.gen();
    let b: u16 = rng.gen();
    format!("NX-{}-{:04X}-{:04X}", kind.code(), a, b)
}

/// A parsed NEXORA ID, useful for validating input coming from the API/Maya.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetId {
    pub kind: AssetKind,
    pub group_a: u16,
    pub group_b: u16,
}

impl AssetId {
    /// Validate and parse an `NX-...` string.
    pub fn parse(s: &str) -> Option<AssetId> {
        let mut parts = s.split('-');
        if parts.next()? != "NX" {
            return None;
        }
        let kind = AssetKind::from_code(parts.next()?)?;
        let group_a = u16::from_str_radix(parts.next()?, 16).ok()?;
        let group_b = u16::from_str_radix(parts.next()?, 16).ok()?;
        if parts.next().is_some() {
            return None; // trailing garbage
        }
        Some(AssetId {
            kind,
            group_a,
            group_b,
        })
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "NX-{}-{:04X}-{:04X}",
            self.kind.code(),
            self.group_a,
            self.group_b
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_expected_shape() {
        let id = new_id(AssetKind::Material);
        assert!(id.starts_with("NX-MAT-"), "got {id}");
        assert_eq!(id.len(), "NX-MAT-XXXX-XXXX".len());
    }

    #[test]
    fn roundtrip_parse() {
        let id = new_id(AssetKind::Texture);
        let parsed = AssetId::parse(&id).expect("should parse");
        assert_eq!(parsed.kind, AssetKind::Texture);
        assert_eq!(parsed.to_string(), id);
    }

    #[test]
    fn rejects_garbage() {
        assert!(AssetId::parse("hello").is_none());
        assert!(AssetId::parse("NX-XXX-0000-0000").is_none());
        assert!(AssetId::parse("NX-TEX-ZZZZ-0000").is_none());
        assert!(AssetId::parse("NX-TEX-0000-0000-0000").is_none());
    }
}
