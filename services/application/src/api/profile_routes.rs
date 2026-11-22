//! Explicit account-default commands share transport, never a caller-selected scope.
use super::*;
use crate::{
    commands::{CommandDisposition, CommandResponse},
    cosmetics::ChangeProfileDefaults,
};

#[derive(Clone, Copy)]
enum Operation {
    Change,
    Disposition,
}

pub(super) async fn change(State(state): State<ApiState>, request: Request) -> Response {
    execute(state, request, Operation::Change).await
}
pub(super) async fn disposition(State(state): State<ApiState>, request: Request) -> Response {
    execute(state, request, Operation::Disposition).await
}

async fn execute(state: ApiState, request: Request, operation: Operation) -> Response {
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
    let input: ChangeProfileDefaults = match to_bytes(request.into_body(), 2048)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "defaults", "invalid");
    }
    state.blocking(move |config| {
        let actor = match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor,Err(error)=>return actor_error(error) };
        if actor.account().is_none() { return fail(StatusCode::CONFLICT,"account","unavailable"); }
        let backend = match BackendConfig::new(&config.base,&config.secret_key).and_then(|config| BackendClient::new(config,"spellwake-profile-defaults",Duration::from_secs(60))) {
            Ok(backend)=>backend,Err(_)=>return unavailable(),
        };
        let (function,parameters)=match operation {
            Operation::Change=>("change_profile_defaults_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request_id,"p_expected":input.expected,"p_appearance":input.appearance})),
            Operation::Disposition=>("read_profile_defaults_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_command":input})),
        };
        let response: Result<ApiResponse<CommandResponse>,_> = backend.rpc(function,&parameters);
        match response {
            Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)})
                if matches!(operation,Operation::Change) && !ack.resource_id.is_nil() =>
                    (StatusCode::ACCEPTED,Json(ApiResponse::Success{data:ack})).into_response(),
            Ok(ApiResponse::Success{data:CommandResponse::Disposition(disposition)})
                if matches!(operation,Operation::Disposition) && valid_disposition(&disposition)=>success(disposition),
            Ok(ApiResponse::Fail{data}) if data.len()==1=>match data.iter().next().map(|(key,value)|(key.as_str(),value.as_str())) {
                Some(("session","unauthorized"))=>fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
                Some((field,reason)) if canonical_failure(&data) || matches!((field,reason),("request","invalid"|"conflict"))=>fail(StatusCode::CONFLICT,field,reason),
                _=>unavailable(),
            },
            _=>unavailable(),
        }
    }).await
}

fn canonical_failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors
                .iter()
                .next()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            Some(
                ("account", "account unavailable")
                    | ("defaults", "account defaults changed; reload before saving")
                    | ("cosmetics", "cosmetic selection unavailable")
            )
        )
}
fn valid_disposition(disposition: &CommandDisposition) -> bool {
    match disposition {
        CommandDisposition::Unknown | CommandDisposition::Pending => true,
        CommandDisposition::Accepted { ack } => !ack.resource_id.is_nil(),
        CommandDisposition::Rejected { errors } => canonical_failure(errors),
    }
}

/// Caller-owned profile crop metadata; image delivery has its own qualification boundary.
pub(super) async fn art_own(State(state): State<ApiState>, request: Request) -> Response {
    read_art(state, None, request).await
}
pub(super) async fn art_public(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let target = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "player", reason),
    };
    read_art(state, Some(target), request).await
}
async fn read_art(state: ApiState, target: Option<Uuid>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| match target {
            Some(target) => economy_routes::projection(
                config
                    .actor()
                    .and_then(|a| a.public_profile_art(&token, target)),
                Some("player"),
            ),
            None => {
                economy_routes::projection(config.actor().and_then(|a| a.profile_art(&token)), None)
            }
        })
        .await
}
