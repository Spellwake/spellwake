//! Custom Magic card authoring, compilation, phrasing standardization, and binary art validation
//! for Spellwake Open Play.
//!
//! Custom cards are strictly scoped to Open Play (`mode = 'open'`, `competitive = false`) and
//! never enter Collection Play, Ranked, League, Limited, Trading, or public matchmaking.

pub mod compiler_pool;

pub use compiler_pool::{
    CompilerPoolError, CustomCardCompilerPool, load_custom_card_vocabulary_from_env,
    load_vocabulary_from_bytes,
};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_card_programs::custom_card::{
    CustomCardCompileInput, CustomCardCompileReport, CustomCardFaceRules,
};
use spellwake_oracle_compiler::standardize::StandardizeFaceInput;
use uuid::Uuid;

/// Maximum decoded binary art size per card face (128 KiB).
pub const MAX_FACE_ART_BYTES: usize = 131_072;

/// Maximum base64 character length for one face's art payload (~176 KiB + overhead).
pub const MAX_FACE_ART_BASE64_LEN: usize = 180_000;

/// Validated visual frame accent override (cosmetic-only, excluded from `rules_sha256`).
pub fn is_valid_frame_accent(value: &str) -> bool {
    matches!(
        value,
        "auto" | "W" | "U" | "B" | "R" | "G" | "gold" | "artifact" | "colorless" | "land"
    )
}

/// Validate one custom card face art upload payload and decode its raw PNG/WebP bytes.
///
/// # Errors
/// Rejects unsupported MIME types, oversized payloads, invalid base64, SVG/HTML content,
/// magic-byte mismatches, and non-finite or out-of-range crop/zoom parameters.
pub fn validate_and_decode_art_bytes(
    input: &CustomCardFaceArtInput,
) -> Result<ValidatedFaceArt, &'static str> {
    if input.face_index > 1 {
        return Err("invalid face index");
    }
    if !input.crop_x.is_finite()
        || !input.crop_y.is_finite()
        || !input.zoom.is_finite()
        || !(-1.0..=1.0).contains(&input.crop_x)
        || !(-1.0..=1.0).contains(&input.crop_y)
        || !(0.5..=4.0).contains(&input.zoom)
    {
        return Err("invalid art crop or zoom");
    }
    if let Some(artist) = input.artist.as_deref()
        && (artist.chars().count() > 80 || artist.contains('\0'))
    {
        return Err("invalid artist name");
    }
    let Some(raw_base64) = input.art_base64.as_deref() else {
        let Some(copy_from) = input.copy_from_revision_id else {
            return Err("art must provide either artBase64 or copyFromRevisionId");
        };
        if copy_from.is_nil() {
            return Err("invalid copyFromRevisionId");
        }
        return Ok(ValidatedFaceArt {
            face_index: input.face_index,
            mime_type: None,
            art_base64_canonical: None,
            art_sha256: None,
            copy_from_revision_id: Some(copy_from),
            crop_x: input.crop_x,
            crop_y: input.crop_y,
            zoom: input.zoom,
            artist: input
                .artist
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        });
    };
    let mime = input
        .mime_type
        .as_deref()
        .ok_or("art must declare mimeType")?;
    if !matches!(mime, "image/webp" | "image/png") {
        return Err("art must be image/webp or image/png");
    }
    if raw_base64.is_empty() || raw_base64.len() > MAX_FACE_ART_BASE64_LEN {
        return Err("art payload exceeds 128 KiB limit");
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw_base64.trim())
        .map_err(|_| "invalid base64 art payload")?;
    if bytes.is_empty() || bytes.len() > MAX_FACE_ART_BYTES {
        return Err("decoded art exceeds 128 KiB limit");
    }

    let matches_magic = match mime {
        "image/png" => {
            bytes.len() >= 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        }
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !matches_magic {
        return Err("art bytes do not match declared image format");
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let art_sha256 = hex_lower(&hasher.finalize());

    Ok(ValidatedFaceArt {
        face_index: input.face_index,
        mime_type: Some(mime.to_owned()),
        art_base64_canonical: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        art_sha256: Some(art_sha256),
        copy_from_revision_id: None,
        crop_x: input.crop_x,
        crop_y: input.crop_y,
        zoom: input.zoom,
        artist: input
            .artist
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

/// Decoded and validated art record ready for SQL base64 transport.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedFaceArt {
    pub face_index: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(rename = "artBase64", skip_serializing_if = "Option::is_none")]
    pub art_base64_canonical: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_from_revision_id: Option<Uuid>,
    pub crop_x: f64,
    pub crop_y: f64,
    pub zoom: f64,
    pub artist: Option<String>,
}

/// Client art payload supplied during custom card creation or revision save.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomCardFaceArtInput {
    pub face_index: u8,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub art_base64: Option<String>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub copy_from_revision_id: Option<Uuid>,
    pub crop_x: f64,
    pub crop_y: f64,
    pub zoom: f64,
    #[serde(default)]
    pub artist: Option<String>,
}

/// Request body for `POST /v1/custom-cards/standardize`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StandardizeRequest {
    pub faces: Vec<StandardizeFaceInput>,
}

/// Request body for `POST /v1/custom-cards`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCustomCardRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub expected_revision_id: Option<Uuid>,
    pub layout: String,
    pub rarity: String,
    pub faces: Vec<CustomCardFaceRules>,
    #[serde(default)]
    pub visual_frame_accents: Vec<String>,
    #[serde(default)]
    pub flavor_texts: Vec<Option<String>>,
    #[serde(default)]
    pub collector_note: Option<String>,
    #[serde(default)]
    pub art: Vec<CustomCardFaceArtInput>,
}

/// Request body for `POST /v1/custom-cards/{id}/revisions`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCustomCardRevisionRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub expected_revision_id: Option<Uuid>,
    pub layout: String,
    pub rarity: String,
    pub faces: Vec<CustomCardFaceRules>,
    #[serde(default)]
    pub visual_frame_accents: Vec<String>,
    #[serde(default)]
    pub flavor_texts: Vec<Option<String>>,
    #[serde(default)]
    pub collector_note: Option<String>,
    #[serde(default)]
    pub art: Vec<CustomCardFaceArtInput>,
}

/// Request body for `PATCH /v1/custom-cards/{id}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchCustomCardRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub archived: bool,
}

/// Request body for `POST /v1/custom-cards/import-vision`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisionImportRequest {
    pub source_kind: String,
    pub image_data_url: String,
}

/// Request body for `POST /v1/open-play/quick-test`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuickTestRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub custom_card_revision_id: Uuid,
    #[serde(default)]
    pub ai_plays_custom_card: bool,
}

/// Normalized visual frame accents, flavor texts, and collector note validated before persistence.
pub struct ValidatedCardPresentation {
    pub compile_input: CustomCardCompileInput,
    pub visual_frame_accents: Vec<String>,
    pub flavor_texts: Vec<Option<String>>,
    pub collector_note: Option<String>,
    pub arts: Vec<ValidatedFaceArt>,
}

/// Validate presentation fields and decode art payloads shared by create and revision save routes.
///
/// # Errors
/// Rejects invalid face counts, invalid frame accents, overly long flavor/notes, or bad art.
#[allow(
    clippy::too_many_arguments,
    reason = "Explicit presentation validation avoids constructing an intermediate DTO copy."
)]
pub fn validate_card_presentation(
    layout: String,
    rarity: String,
    faces: Vec<CustomCardFaceRules>,
    visual_frame_accents: Vec<String>,
    flavor_texts: Vec<Option<String>>,
    collector_note: Option<String>,
    arts: Vec<CustomCardFaceArtInput>,
) -> Result<ValidatedCardPresentation, &'static str> {
    if faces.is_empty() || faces.len() > 2 {
        return Err("card must have 1 or 2 faces");
    }
    let expected_faces = match layout.as_str() {
        "normal" | "planeswalker" | "saga" => 1,
        "transform" | "modal_dfc" | "adventure" | "split" => 2,
        _ => return Err("unsupported card layout"),
    };
    if faces.len() != expected_faces {
        return Err("face count does not match layout");
    }
    if !matches!(rarity.as_str(), "common" | "uncommon" | "rare" | "mythic") {
        return Err("unsupported rarity");
    }

    let mut accents = if visual_frame_accents.is_empty() {
        vec!["auto".to_owned(); faces.len()]
    } else {
        visual_frame_accents
    };
    if accents.len() != faces.len() || accents.iter().any(|a| !is_valid_frame_accent(a)) {
        return Err("invalid visual frame accent");
    }
    // Preserve exact length matching faces.
    accents.truncate(faces.len());

    let flavors = if flavor_texts.is_empty() {
        vec![None; faces.len()]
    } else {
        if flavor_texts.len() != faces.len() {
            return Err("flavor text count does not match face count");
        }
        let mut cleaned = Vec::with_capacity(faces.len());
        for text in flavor_texts {
            match text {
                Some(raw) => {
                    if raw.chars().count() > 512 || raw.contains('\0') {
                        return Err("flavor text exceeds 512 characters");
                    }
                    let trimmed = raw.trim();
                    cleaned.push((!trimmed.is_empty()).then(|| trimmed.to_owned()));
                }
                None => cleaned.push(None),
            }
        }
        cleaned
    };

    let note = match collector_note {
        Some(raw) => {
            if raw.chars().count() > 120 || raw.contains('\0') {
                return Err("collector note exceeds 120 characters");
            }
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        None => None,
    };

    let mut seen_faces = std::collections::BTreeSet::new();
    let mut validated_arts = Vec::with_capacity(arts.len());
    for art in &arts {
        if usize::from(art.face_index) >= faces.len() || !seen_faces.insert(art.face_index) {
            return Err("duplicate or out-of-range face art");
        }
        validated_arts.push(validate_and_decode_art_bytes(art)?);
    }

    Ok(ValidatedCardPresentation {
        compile_input: CustomCardCompileInput {
            layout,
            rarity,
            faces,
        },
        visual_frame_accents: accents,
        flavor_texts: flavors,
        collector_note: note,
        arts: validated_arts,
    })
}

/// Response payload returned after creating or saving a custom card revision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedCustomCardResponse {
    pub custom_card_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: String,
    pub compile: CustomCardCompileReport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_png_and_webp_magic_bytes_and_rejects_svg_or_oversize() {
        let mut png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png_bytes.extend_from_slice(&[0_u8; 32]);
        let valid_png = CustomCardFaceArtInput {
            face_index: 0,
            mime_type: Some("image/png".to_owned()),
            art_base64: Some(base64::engine::general_purpose::STANDARD.encode(&png_bytes)),
            copy_from_revision_id: None,
            crop_x: 0.5,
            crop_y: 0.4,
            zoom: 1.2,
            artist: Some("Elena Vance".to_owned()),
        };
        let decoded = validate_and_decode_art_bytes(&valid_png).expect("valid png passes");
        assert_eq!(decoded.art_sha256.as_deref().map(str::len), Some(64));
        assert_eq!(decoded.artist.as_deref(), Some("Elena Vance"));

        let mut webp_bytes = Vec::new();
        webp_bytes.extend_from_slice(b"RIFF\x24\x00\x00\x00WEBP");
        webp_bytes.extend_from_slice(&[0_u8; 24]);
        let valid_webp = CustomCardFaceArtInput {
            face_index: 0,
            mime_type: Some("image/webp".to_owned()),
            art_base64: Some(base64::engine::general_purpose::STANDARD.encode(&webp_bytes)),
            copy_from_revision_id: None,
            crop_x: 0.5,
            crop_y: 0.5,
            zoom: 1.0,
            artist: None,
        };
        assert!(validate_and_decode_art_bytes(&valid_webp).is_ok());

        let svg_attack = CustomCardFaceArtInput {
            face_index: 0,
            mime_type: Some("image/png".to_owned()),
            art_base64: Some(
                base64::engine::general_purpose::STANDARD
                    .encode(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            ),
            copy_from_revision_id: None,
            crop_x: 0.5,
            crop_y: 0.5,
            zoom: 1.0,
            artist: None,
        };
        assert!(validate_and_decode_art_bytes(&svg_attack).is_err());

        let oversized = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
            .into_iter()
            .chain(std::iter::repeat_n(0_u8, MAX_FACE_ART_BYTES))
            .collect::<Vec<_>>();
        let too_big = CustomCardFaceArtInput {
            face_index: 0,
            mime_type: Some("image/png".to_owned()),
            art_base64: Some(base64::engine::general_purpose::STANDARD.encode(&oversized)),
            copy_from_revision_id: None,
            crop_x: 0.5,
            crop_y: 0.5,
            zoom: 1.0,
            artist: None,
        };
        assert!(validate_and_decode_art_bytes(&too_big).is_err());
    }
}
