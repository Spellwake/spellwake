//! Human pregame intents and daily Advanced AI quota reads; never exposes provider credentials.
use super::*;
use crate::{
    ai_runtime::{AiQuotaStatus, SetAiTierRequest},
    commands::CommandResponse,
    room_controls::{BootstrapRequest, ControlAck, ControlKind, RevisionRequest, SetAiDeckRequest},
};
pub(super) async fn add(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, ControlKind::Add, false).await
}
pub(super) async fn add_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::Add, true).await
}
pub(super) async fn bootstrap(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::Bootstrap, false).await
}
pub(super) async fn bootstrap_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::Bootstrap, true).await
}
pub(super) async fn remove(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, ControlKind::Remove, false).await
}
pub(super) async fn remove_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::Remove, true).await
}
pub(super) async fn unready(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, ControlKind::Unready, false).await
}
pub(super) async fn unready_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::Unready, true).await
}
pub(super) async fn set_tier(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, ControlKind::SetTier, false).await
}
pub(super) async fn set_tier_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::SetTier, true).await
}
pub(super) async fn set_deck(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, ControlKind::SetDeck, false).await
}
pub(super) async fn set_deck_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, ControlKind::SetDeck, true).await
}

/// `GET /v1/account/ai-quota`: caller's daily Advanced AI allowance and UTC reset instant.
pub(super) async fn ai_quota(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
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
            let Some(account_id) = actor.account() else {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            };
            let resp = match client.authenticated_rpc(&token, "read_my_ai_quota", &json!({})) {
                Ok(v) => v,
                Err(e) => return actor_error(e),
            };
            match resp {
                ApiResponse::Success { data } => {
                    match serde_json::from_value::<AiQuotaStatus>(data) {
                        Ok(quota) if quota.validate(account_id).is_ok() => {
                            (StatusCode::OK, Json(ApiResponse::Success { data: quota }))
                                .into_response()
                        }
                        _ => unavailable(),
                    }
                }
                ApiResponse::Fail { data } => {
                    let code = if data.get("session").is_some_and(|v| v == "unauthorized") {
                        StatusCode::UNAUTHORIZED
                    } else {
                        StatusCode::CONFLICT
                    };
                    (code, Json(ApiResponse::<AiQuotaStatus>::Fail { data })).into_response()
                }
                ApiResponse::Error { .. } => unavailable(),
            }
        })
        .await
}

async fn write(
    state: ApiState,
    path: IdentifierPath,
    request: Request,
    kind: ControlKind,
    lookup: bool,
) -> Response {
    let field = if matches!(kind, ControlKind::Add | ControlKind::Unready) {
        "room"
    } else {
        "slot"
    };
    let target = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, field, reason),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let (request, basis, revision, tier) = match kind {
        ControlKind::Bootstrap => match parse_object::<BootstrapRequest>(&bytes) {
            Ok(i) => (i.request_id, i.expected_revision, i.deck_revision_id, None),
            Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
        },
        ControlKind::SetTier => match parse_object::<SetAiTierRequest>(&bytes) {
            Ok(i) => (i.request_id, i.expected_revision, None, Some(i.tier)),
            Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
        },
        ControlKind::SetDeck => match parse_object::<SetAiDeckRequest>(&bytes) {
            Ok(i) => (i.request_id, i.expected_revision, i.deck_revision_id, None),
            Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
        },
        ControlKind::Add | ControlKind::Remove | ControlKind::Unready => {
            match parse_object::<RevisionRequest>(&bytes) {
                Ok(i) => (i.request_id, i.expected_revision, None, None),
                Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
            }
        }
    };
    if request.is_nil() {
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
            let backend = match BackendConfig::new(&config.base, &config.secret_key).and_then(|c| {
                BackendClient::new(c, "spellwake-room-control", Duration::from_secs(60))
            }) {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let (name, params) = match kind {
                ControlKind::Add => (
                    if lookup {
                        "read_add_room_ai_slot_command"
                    } else {
                        "add_room_ai_slot_command"
                    },
                    json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_room":target,"p_expected":basis}),
                ),
                ControlKind::Unready => (
                    if lookup {
                        "read_unready_room_command"
                    } else {
                        "unready_room_command"
                    },
                    json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_room":target,"p_expected":basis}),
                ),
                ControlKind::Bootstrap | ControlKind::Remove => (
                    if lookup {
                        "read_ai_slot_command"
                    } else {
                        "manage_ai_slot_command"
                    },
                    json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_kind":if matches!(kind,ControlKind::Bootstrap){"bootstrap"}else{"remove"},"p_slot":target,"p_expected":basis,"p_revision":revision}),
                ),
                ControlKind::SetTier => (
                    if lookup {
                        "read_set_ai_slot_tier_command"
                    } else {
                        "set_ai_slot_tier_command"
                    },
                    json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_slot":target,"p_expected":basis,"p_tier":tier.map(|t|t.as_str())}),
                ),
                ControlKind::SetDeck => (
                    if lookup {
                        "read_set_ai_slot_deck_command"
                    } else {
                        "set_ai_slot_deck_command"
                    },
                    json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_slot":target,"p_expected":basis,"p_revision":revision}),
                ),
            };
            let result: Result<ApiResponse<CommandResponse<ControlAck>>, _> =
                backend.rpc(name, &params);
            command_routes::typed_reply(
                result,
                lookup,
                |ack| kind.valid_ack(target, &basis, ack),
                |errors| kind.failure(errors),
                |_| false,
            )
        })
        .await
}
