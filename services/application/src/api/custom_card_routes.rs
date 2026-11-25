//! Authenticated HTTP routes for Spellwake Open Play Custom Card Studio, binary art
//! delivery, Vision OCR fallback import, in-match revealed custom cards, and Quick Test.

use super::*;
use crate::{
    ai_runtime::VisionImportError,
    custom_cards::{
        CreateCustomCardRequest, PatchCustomCardRequest, QuickTestRequest,
        SaveCustomCardRevisionRequest, SavedCustomCardResponse, StandardizeRequest,
        VisionImportRequest, validate_card_presentation,
    },
};
use base64::Engine as _;
use spellwake_card_programs::custom_card::{CustomCardCompileInput, CustomCardFaceRules};
use spellwake_oracle_compiler::standardize::StandardizeFaceInput;

/// 16 KiB body limit for compile and standardize requests.
const MAX_COMPILE_BODY_BYTES: usize = 16 * 1024;

/// 512 KiB body limit for create/save custom card requests (supports up to 2x 128 KiB base64 art).
const MAX_SAVE_CARD_BODY_BYTES: usize = 512 * 1024;

/// 768 KiB body limit for Vision OCR fallback image uploads (512 KiB decoded + base64 overhead).
const MAX_VISION_BODY_BYTES: usize = 768 * 1024;

/// Immutable private browser cache header applied strictly to `200 OK` binary art responses.
pub(super) const IMMUTABLE_PRIVATE_ART_CACHE: &str = "private, max-age=31536000, immutable";

/// Identify revision-addressed binary art paths that may keep their immutable cache headers on 200.
pub(super) fn is_immutable_art_path(path: &str) -> bool {
    (path.starts_with("/v1/custom-cards/revisions/") && path.contains("/art/"))
        || (path.starts_with("/v1/rooms/")
            && path.contains("/lent-decks/")
            && path.contains("/face-art/"))
        || (path.starts_with("/v1/games/")
            && path.contains("/custom-cards/")
            && path.contains("/art/"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListCustomCardsQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
    #[serde(default)]
    include_archived: Option<bool>,
    #[serde(default)]
    search: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextQuery {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteCardQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    request_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinaryArtRow {
    mime_type: String,
    art_sha256: String,
    art_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedRevisionAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    revision_id: Uuid,
    revision_number: String,
}

/// `POST /v1/custom-cards/compile`: live Stage 1 AST + Stage 2 engine lowering check.
pub(super) async fn compile(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_COMPILE_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: CustomCardCompileInput = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            match config.custom_compiler.compile(input) {
                Ok(compiled) => success(compiled.report),
                Err(_) => unavailable(),
            }
        })
        .await
}

/// `POST /v1/custom-cards/standardize`: deterministic Foundations/M25+ Oracle phrasing rewriter.
pub(super) async fn standardize(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_COMPILE_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: StandardizeRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.faces.is_empty() || input.faces.len() > 2 {
        return fail(StatusCode::BAD_REQUEST, "faces", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            match config.custom_compiler.standardize(input.faces) {
                Ok(output) => success(output),
                Err(_) => unavailable(),
            }
        })
        .await
}

/// `POST /v1/custom-cards/import-vision`: Codex Vision OCR fallback for external card images.
pub(super) async fn import_vision(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_VISION_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: VisionImportRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let Some(vision) = config.codex_vision.as_ref() else {
                return unavailable();
            };
            if !matches!(
                input.source_kind.as_str(),
                "external_png" | "external_image"
            ) {
                return fail(StatusCode::BAD_REQUEST, "source", "invalid");
            }
            let Some((mime_type, image_base64)) = parse_vision_data_url(&input.image_data_url)
            else {
                return fail(StatusCode::BAD_REQUEST, "image", "invalid");
            };
            let extracted = match vision.extract_card(account_id, mime_type, image_base64) {
                Ok(card) => card,
                Err(VisionImportError::InvalidImage(_)) => {
                    return fail(StatusCode::BAD_REQUEST, "image", "invalid");
                }
                Err(VisionImportError::RateLimited) => {
                    return fail(StatusCode::TOO_MANY_REQUESTS, "rate", "limited");
                }
                Err(VisionImportError::MalformedOutput(_)) => {
                    return fail(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "vision",
                        "could_not_extract_card",
                    );
                }
                Err(
                    VisionImportError::Busy
                    | VisionImportError::Unavailable(_)
                    | VisionImportError::Timeout
                    | VisionImportError::Transport(_),
                ) => {
                    return unavailable();
                }
            };

            let std_inputs: Vec<StandardizeFaceInput> = extracted
                .faces
                .iter()
                .map(|face| StandardizeFaceInput {
                    name: face.name.clone(),
                    mana_cost: face.mana_cost.clone(),
                    type_line: face.type_line.clone(),
                    oracle_text: face.oracle_text.clone(),
                    power: face.power.clone(),
                    toughness: face.toughness.clone(),
                    loyalty: face.loyalty.clone(),
                    defense: face.defense.clone(),
                    color_indicator: face.color_indicator.clone(),
                })
                .collect();

            let standardized = match config.custom_compiler.standardize(std_inputs) {
                Ok(out) => out,
                Err(_) => return unavailable(),
            };

            let compile_faces: Vec<CustomCardFaceRules> = standardized
                .faces
                .iter()
                .map(|face| CustomCardFaceRules {
                    name: face.name.clone(),
                    mana_cost: face.mana_cost.clone(),
                    type_line: face.type_line.clone(),
                    oracle_text: face.oracle_text.clone(),
                    power: face.power.clone(),
                    toughness: face.toughness.clone(),
                    loyalty: face.loyalty.clone(),
                    defense: face.defense.clone(),
                    color_indicator: face.color_indicator.clone(),
                })
                .collect();

            let compile_report = match config.custom_compiler.compile(CustomCardCompileInput {
                layout: extracted.layout.clone(),
                rarity: extracted.rarity.clone(),
                faces: compile_faces.clone(),
            }) {
                Ok(compiled) => compiled.report,
                Err(_) => return unavailable(),
            };

            let flavor_texts: Vec<Option<String>> = extracted
                .faces
                .iter()
                .map(|face| face.flavor_text.clone())
                .collect();
            let artist = extracted.faces.first().and_then(|face| face.artist.clone());

            success(json!({
                "cards": [
                    {
                        "layout": extracted.layout,
                        "rarity": extracted.rarity,
                        "faces": compile_faces,
                        "flavorTexts": flavor_texts,
                        "collectorNote": null,
                        "artist": artist,
                        "artCropBox": extracted.art_crop_box,
                        "standardized": standardized,
                        "compiled": compile_report,
                    }
                ],
                "warnings": extracted.warnings,
            }))
        })
        .await
}

/// `GET /v1/custom-cards`: paginated owner vault listing.
pub(super) async fn list(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<ListCustomCardsQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
        || query
            .search
            .as_deref()
            .is_some_and(|s| s.chars().count() > 100 || s.contains('\0'))
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            let result = config.actor().and_then(|c| {
                c.authenticated_rpc(
                    &token,
                    "read_my_custom_cards",
                    &json!({
                        "p_after": query.after,
                        "p_limit": limit,
                        "p_include_archived": query.include_archived.unwrap_or(false),
                        "p_search": query.search,
                    }),
                )
            });
            relay_custom_rpc(result, StatusCode::OK)
        })
        .await
}

/// `POST /v1/custom-cards`: authoritatively recompile and persist a new custom card (rev 1).
pub(super) async fn create(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_SAVE_CARD_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: CreateCustomCardRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    let validated = match validate_card_presentation(
        input.layout,
        input.rarity,
        input.faces,
        input.visual_frame_accents,
        input.flavor_texts,
        input.collector_note,
        input.art,
    ) {
        Ok(v) => v,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "card", reason),
    };
    if validated
        .arts
        .iter()
        .any(|art| art.art_base64_canonical.is_none())
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "art",
            "new card requires art bytes",
        );
    }

    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let compiled = match config
                .custom_compiler
                .compile(validated.compile_input.clone())
            {
                Ok(c) => c,
                Err(_) => return unavailable(),
            };
            let card_name = canonical_card_name(&validated.compile_input.faces);
            let payload = json!({
                "name": card_name,
                "layout": validated.compile_input.layout,
                "rarity": validated.compile_input.rarity,
                "rulesSha256": compiled.report.rules_sha256,
                "compilerRevision": compiled.report.compiler_revision,
                "facesRules": validated.compile_input.faces,
                "visualFrameAccents": validated.visual_frame_accents,
                "flavorTexts": validated.flavor_texts,
                "collectorNote": validated.collector_note,
                "stage1Parsed": compiled.report.stage1_parsed,
                "stage2Playable": compiled.report.stage2_playable,
                "art": validated.arts,
            });
            let backend = match config.backend("spellwake-custom-card-create") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<SavedRevisionAck>, _> = backend.rpc(
                "create_custom_card_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": input.request_id,
                    "p_payload": payload,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => (
                    StatusCode::CREATED,
                    Json(ApiResponse::Success {
                        data: SavedCustomCardResponse {
                            custom_card_id: data.resource_id,
                            revision_id: data.revision_id,
                            revision_number: data.revision_number,
                            compile: compiled.report,
                        },
                    }),
                )
                    .into_response(),
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

/// `POST /v1/custom-cards/{card}/revisions`: authoritatively recompile and save an immutable rev.
pub(super) async fn save_revision(
    State(state): State<ApiState>,
    card_path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let card_id = match identifier(card_path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "card", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_SAVE_CARD_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: SaveCustomCardRevisionRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.request_id.is_nil() || input.expected_revision_id.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    let validated = match validate_card_presentation(
        input.layout,
        input.rarity,
        input.faces,
        input.visual_frame_accents,
        input.flavor_texts,
        input.collector_note,
        input.art,
    ) {
        Ok(v) => v,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "card", reason),
    };

    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let compiled = match config
                .custom_compiler
                .compile(validated.compile_input.clone())
            {
                Ok(c) => c,
                Err(_) => return unavailable(),
            };
            let card_name = canonical_card_name(&validated.compile_input.faces);
            let payload = json!({
                "name": card_name,
                "layout": validated.compile_input.layout,
                "rarity": validated.compile_input.rarity,
                "rulesSha256": compiled.report.rules_sha256,
                "compilerRevision": compiled.report.compiler_revision,
                "facesRules": validated.compile_input.faces,
                "visualFrameAccents": validated.visual_frame_accents,
                "flavorTexts": validated.flavor_texts,
                "collectorNote": validated.collector_note,
                "stage1Parsed": compiled.report.stage1_parsed,
                "stage2Playable": compiled.report.stage2_playable,
                "art": validated.arts,
            });
            let backend = match config.backend("spellwake-custom-card-save") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<SavedRevisionAck>, _> = backend.rpc(
                "save_custom_card_revision_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": input.request_id,
                    "p_card_id": card_id,
                    "p_expected_revision": input.expected_revision_id,
                    "p_payload": payload,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => success(SavedCustomCardResponse {
                    custom_card_id: data.resource_id,
                    revision_id: data.revision_id,
                    revision_number: data.revision_number,
                    compile: compiled.report,
                }),
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

/// `PATCH /v1/custom-cards/{card}`: archive or restore a custom card.
pub(super) async fn patch(
    State(state): State<ApiState>,
    card_path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let card_id = match identifier(card_path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "card", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: PatchCustomCardRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match config.backend("spellwake-custom-card-patch") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "archive_custom_card_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": input.request_id,
                    "p_card_id": card_id,
                    "p_archived": input.archived,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

/// `DELETE /v1/custom-cards/{card}`: hard-delete unreferenced custom card or archive if referenced.
pub(super) async fn delete(
    State(state): State<ApiState>,
    card_path: IdentifierPath,
    request: Request,
) -> Response {
    let card_id = match identifier(card_path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "card", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<DeleteCardQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let request_id = query.request_id.unwrap_or_else(Uuid::new_v4);
    if request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match config.backend("spellwake-custom-card-delete") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "delete_custom_card_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": request_id,
                    "p_card_id": card_id,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

/// `GET /v1/custom-cards/revisions/{revision}/art/{face}`: owner-only immutable binary art.
pub(super) async fn owner_art(
    State(state): State<ApiState>,
    path: Result<Path<(String, u8)>, axum::extract::rejection::PathRejection>,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let (rev_raw, face_index) = match path {
        Ok(Path(pair)) => pair,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "art", "invalid"),
    };
    let Ok(revision_id) = crate::identifiers::parse(&rev_raw) else {
        return fail(StatusCode::BAD_REQUEST, "revision", "invalid");
    };
    if face_index > 1 {
        return fail(StatusCode::BAD_REQUEST, "face", "invalid");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let backend = match config.backend("spellwake-custom-card-art") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<BinaryArtRow>, _> = backend.rpc(
                "read_custom_card_art_for_owner",
                &json!({
                    "p_actor": account_id,
                    "p_revision_id": revision_id,
                    "p_face_index": face_index,
                }),
            );
            binary_art_response(result)
        })
        .await
}

/// `GET /v1/rooms/{room}/lent-decks/{deck}/face-art/{revision}`: guest-accessible lent face art.
pub(super) async fn lent_deck_face_art(
    State(state): State<ApiState>,
    path: Result<Path<(String, String, String)>, axum::extract::rejection::PathRejection>,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let (room_raw, deck_raw, rev_raw) = match path {
        Ok(Path(triple)) => triple,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "art", "invalid"),
    };
    let (Ok(room_id), Ok(deck_id), Ok(rev_id)) = (
        crate::identifiers::parse(&room_raw),
        crate::identifiers::parse(&deck_raw),
        crate::identifiers::parse(&rev_raw),
    ) else {
        return fail(StatusCode::BAD_REQUEST, "art", "invalid");
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let backend = match config.backend("spellwake-lent-deck-art") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<BinaryArtRow>, _> = backend.rpc(
                "read_lent_deck_face_art",
                &json!({
                    "p_actor": account_id,
                    "p_room_id": room_id,
                    "p_deck_id": deck_id,
                    "p_custom_card_revision_id": rev_id,
                }),
            );
            binary_art_response(result)
        })
        .await
}

/// `GET /v1/games/{game}/custom-cards/{revision}?contextId={ctx}`: revealed in-match metadata.
pub(super) async fn match_revealed_card(
    State(state): State<ApiState>,
    path: Result<Path<(String, String)>, axum::extract::rejection::PathRejection>,
    request: Request,
) -> Response {
    let (game_raw, rev_raw) = match path {
        Ok(Path(pair)) => pair,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "card", "invalid"),
    };
    let (Ok(game_id), Ok(rev_id)) = (
        crate::identifiers::parse(&game_raw),
        crate::identifiers::parse(&rev_raw),
    ) else {
        return fail(StatusCode::BAD_REQUEST, "card", "invalid");
    };
    let query = match Query::<ContextQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) if !q.context_id.is_nil() => q,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let backend = match config.backend("spellwake-match-custom-card") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "read_revealed_match_custom_card",
                &json!({
                    "p_actor": account_id,
                    "p_game_id": game_id,
                    "p_context_id": query.context_id,
                    "p_custom_card_revision_id": rev_id,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { data }) if data.contains_key("card") => {
                    fail(StatusCode::NOT_FOUND, "card", "unavailable")
                }
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

/// `GET /v1/games/{game}/custom-cards/{revision}/art/{face}?contextId={ctx}`: revealed art bytes.
pub(super) async fn match_revealed_art(
    State(state): State<ApiState>,
    path: Result<Path<(String, String, u8)>, axum::extract::rejection::PathRejection>,
    request: Request,
) -> Response {
    let (game_raw, rev_raw, face_index) = match path {
        Ok(Path(triple)) => triple,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "art", "invalid"),
    };
    let (Ok(game_id), Ok(rev_id)) = (
        crate::identifiers::parse(&game_raw),
        crate::identifiers::parse(&rev_raw),
    ) else {
        return fail(StatusCode::BAD_REQUEST, "art", "invalid");
    };
    if face_index > 1 {
        return fail(StatusCode::BAD_REQUEST, "face", "invalid");
    }
    let query = match Query::<ContextQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) if !q.context_id.is_nil() => q,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let backend = match config.backend("spellwake-match-custom-art") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<BinaryArtRow>, _> = backend.rpc(
                "read_revealed_match_custom_card_art",
                &json!({
                    "p_actor": account_id,
                    "p_game_id": game_id,
                    "p_context_id": query.context_id,
                    "p_custom_card_revision_id": rev_id,
                    "p_face_index": face_index,
                }),
            );
            binary_art_response(result)
        })
        .await
}

/// `POST /v1/open-play/quick-test`: Patron-only 1v1 60-card Open Play Quick Test vs. Normal Bot.
pub(super) async fn quick_test(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: QuickTestRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.request_id.is_nil() || input.custom_card_revision_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let client = match config.actor() {
                Ok(c) => c,
                Err(e) => return actor_error(e),
            };
            let actor = match client.resolve(&token) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            // Verify Patron supporter entitlement first through canonical SQL authority.
            let patreon =
                match client.authenticated_rpc(&token, "read_my_patreon_status", &json!({})) {
                    Ok(ApiResponse::Success { data }) => data,
                    Ok(ApiResponse::Fail { data }) => return map_custom_card_fail(data),
                    Ok(ApiResponse::Error { .. }) => return unavailable(),
                    Err(e) => return actor_error(e),
                };
            let is_patron = patreon
                .get("supporter")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !is_patron {
                return fail(StatusCode::FORBIDDEN, "supporter", "supporter_required");
            }
            // Delegate room/deck quick-test orchestration to SQL if present, else report unavailable.
            let backend = match config.backend("spellwake-open-play-quick-test") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "create_open_play_quick_test_room_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": input.request_id,
                    "p_custom_card_revision_id": input.custom_card_revision_id,
                    "p_ai_plays_custom_card": input.ai_plays_custom_card,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => {
                    (StatusCode::ACCEPTED, Json(ApiResponse::Success { data })).into_response()
                }
                Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
                _ => unavailable(),
            }
        })
        .await
}

fn parse_vision_data_url(data_url: &str) -> Option<(&str, &str)> {
    let rest = data_url.strip_prefix("data:")?;
    let (mime, payload) = rest.split_once(";base64,")?;
    if !matches!(mime, "image/png" | "image/webp" | "image/jpeg") || payload.is_empty() {
        return None;
    }
    Some((mime, payload))
}

fn canonical_card_name(faces: &[CustomCardFaceRules]) -> String {
    let raw = if faces.len() == 2 {
        format!("{} // {}", faces[0].name.trim(), faces[1].name.trim())
    } else {
        faces
            .first()
            .map(|f| f.name.trim().to_owned())
            .unwrap_or_default()
    };
    raw.chars().take(120).collect::<String>().trim().to_owned()
}

fn binary_art_response(
    result: Result<ApiResponse<BinaryArtRow>, crate::backend::BackendError>,
) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) => {
            if !matches!(data.mime_type.as_str(), "image/webp" | "image/png") {
                return unavailable();
            }
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(
                data.art_base64
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .collect::<String>(),
            ) else {
                return unavailable();
            };
            let Ok(content_type) = HeaderValue::from_str(&data.mime_type) else {
                return unavailable();
            };
            let mut resp = (StatusCode::OK, bytes).into_response();
            let headers = resp.headers_mut();
            headers.insert(header::CONTENT_TYPE, content_type);
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(IMMUTABLE_PRIVATE_ART_CACHE),
            );
            if let Ok(etag) = HeaderValue::from_str(&format!("\"{}\"", data.art_sha256)) {
                headers.insert(header::ETAG, etag);
            }
            resp
        }
        Ok(ApiResponse::Fail { data }) if data.contains_key("art") => {
            fail(StatusCode::NOT_FOUND, "art", "unavailable")
        }
        Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
        _ => unavailable(),
    }
}

fn relay_custom_rpc(
    result: Result<ApiResponse<Value>, ActorError>,
    success_status: StatusCode,
) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) => {
            (success_status, Json(ApiResponse::Success { data })).into_response()
        }
        Ok(ApiResponse::Fail { data }) => map_custom_card_fail(data),
        Ok(ApiResponse::Error { .. }) => unavailable(),
        Err(e) => actor_error(e),
    }
}

fn map_custom_card_fail(data: BTreeMap<String, String>) -> Response {
    if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") {
        return fail(StatusCode::UNAUTHORIZED, "session", "unauthorized");
    }
    if data.len() == 1
        && data
            .get("supporter")
            .is_some_and(|v| v == "supporter_required")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::<Value>::Fail { data }),
        )
            .into_response();
    }
    if data.len() == 1 && data.get("card").is_some_and(|v| v == "unavailable") {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<Value>::Fail { data }),
        )
            .into_response();
    }
    if data.len() == 1
        && data
            .values()
            .next()
            .is_some_and(|v| matches!(v.as_str(), "invalid" | "unsupported"))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<Value>::Fail { data }),
        )
            .into_response();
    }
    (
        StatusCode::CONFLICT,
        Json(ApiResponse::<Value>::Fail { data }),
    )
        .into_response()
}
