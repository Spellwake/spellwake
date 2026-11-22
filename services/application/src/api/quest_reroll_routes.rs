//! Original human intent only; canonical SQL owns selection, allowance and recovery.
use super::*;
use crate::{
    commands::CommandResponse,
    qualified_intents::{
        MAX_QUALIFIED_INTENT_BYTES, QuestRerollAck, QuestRerollIntent, reroll_failure,
    },
};

pub(super) async fn execute(State(state): State<ApiState>, request: Request) -> Response {
    write(state, request, false).await
}

pub(super) async fn disposition(State(state): State<ApiState>, request: Request) -> Response {
    write(state, request, true).await
}

async fn write(state: ApiState, request: Request, lookup: bool) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let intent: QuestRerollIntent = match to_bytes(request.into_body(), MAX_QUALIFIED_INTENT_BYTES)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(intent) => intent,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if !intent.valid() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(error) => return actor_error(error),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend =
                match BackendConfig::new(&config.base, &config.secret_key).and_then(|config| {
                    BackendClient::new(config, "spellwake-quest-reroll", Duration::from_secs(60))
                }) {
                    Ok(backend) => backend,
                    Err(_) => return unavailable(),
                };
            // Never send a client actor, replacement, reward, policy or source day.
            // The facade refreshes session authority after its account lock wait.
            let result = backend.rpc_object::<ApiResponse<CommandResponse<QuestRerollAck>>>(
                if lookup {
                    "read_quest_reroll_command"
                } else {
                    "reroll_quest_command"
                },
                &json!({"p_subject":actor.subject(),"p_session":actor.session(),
                    "p_request":intent.request_id,"p_assignment":intent.assignment_id}),
            );
            match result {
                Ok(ApiResponse::Success {
                    data: CommandResponse::Ack(ack),
                }) if !lookup && intent.valid_ack(&ack) => (
                    StatusCode::ACCEPTED,
                    Json(ApiResponse::Success { data: ack }),
                )
                    .into_response(),
                Ok(ApiResponse::Success {
                    data: CommandResponse::Disposition(disposition),
                }) if lookup && intent.valid_disposition(&disposition) => success(disposition),
                Ok(ApiResponse::Fail { data }) if data.len() == 1 => {
                    match data
                        .iter()
                        .next()
                        .map(|(field, reason)| (field.as_str(), reason.as_str()))
                    {
                        Some(("session", "unauthorized")) => {
                            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                        }
                        Some((field, reason))
                            if reroll_failure(&data)
                                || matches!(
                                    (field, reason),
                                    ("request", "invalid" | "conflict")
                                ) =>
                        {
                            fail(StatusCode::CONFLICT, field, reason)
                        }
                        _ => unavailable(),
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}
