//! Authenticated adapters delegate to existing immutable domain writers.
use super::*;
use crate::{
    commands::{CommandAck, CommandDisposition},
    decks::{ChangeDeckSettings, CreateDeck, DeckCommand, SaveDeck},
};

pub(super) async fn create(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let input: CreateDeck = match to_bytes(request.into_body(), 4096)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "deck", "invalid");
    }
    state.blocking(move |config| {
        let actor = match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor, Err(error)=>return actor_error(error) };
        if actor.account().is_none() { return fail(StatusCode::CONFLICT,"account","unavailable"); }
        let backend = match backend(config) { Ok(client)=>client, Err(_)=>return unavailable() };
        command_response(backend.rpc("create_deck_command", &json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request_id,"p_name":input.name,"p_mode":input.mode,"p_format":input.format_id})))
    }).await
}
pub(super) async fn save(
    State(state): State<ApiState>,
    deck: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let deck = match identifier(deck) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "deck", "invalid"),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let input: SaveDeck = match to_bytes(request.into_body(), 262144)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.deck_id != deck || input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "deck", "invalid");
    }
    state.blocking(move |config| {
        let actor = match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor, Err(error)=>return actor_error(error) };
        if actor.account().is_none() { return fail(StatusCode::CONFLICT,"account","unavailable"); }
        let backend = match backend(config) { Ok(client)=>client, Err(_)=>return unavailable() };
        command_response(backend.rpc("save_deck_command", &json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request_id,"p_deck":input.deck_id,"p_expected_revision":input.expected_revision_id,"p_entries":input.entries})))
    }).await
}
/// One settings command updates metadata and entitled appearance atomically.
pub(super) async fn settings(
    State(state): State<ApiState>,
    deck: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let deck = match identifier(deck) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "deck", "invalid"),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let input: ChangeDeckSettings = match to_bytes(request.into_body(), 4096)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.deck_id != deck || input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "deck", "invalid");
    }
    state.blocking(move |config| {
        let actor = match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor, Err(error)=>return actor_error(error) };
        if actor.account().is_none() { return fail(StatusCode::CONFLICT,"account","unavailable"); }
        let backend = match backend(config) { Ok(client)=>client, Err(_)=>return unavailable() };
        let result: Result<ApiResponse<CommandAck>,_> = backend.rpc("change_deck_settings_command", &json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request_id,"p_deck":input.deck_id,"p_expected":input.expected,"p_settings":input.settings}));
        if matches!(&result, Ok(ApiResponse::Success{data}) if data.resource_id != deck) { return unavailable(); }
        command_response(result)
    }).await
}
fn backend(config: &ApiConfig) -> Result<BackendClient, crate::backend::BackendError> {
    BackendClient::new(
        BackendConfig::new(&config.base, &config.secret_key)?,
        "spellwake-deck-command",
        Duration::from_secs(60),
    )
}
fn command_response(
    result: Result<ApiResponse<CommandAck>, crate::backend::BackendError>,
) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) if !data.resource_id.is_nil() => {
            (StatusCode::ACCEPTED, Json(ApiResponse::Success { data })).into_response()
        }
        Ok(ApiResponse::Fail { data }) if data.len() == 1 => match data
            .iter()
            .next()
            .map(|(key, value)| (key.as_str(), value.as_str()))
        {
            Some(("session", "unauthorized")) => {
                fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
            }
            Some((field, reason))
                if canonical_failure(&data)
                    || matches!((field, reason), ("request", "invalid" | "conflict")) =>
            {
                fail(StatusCode::CONFLICT, field, reason)
            }
            _ => unavailable(),
        },
        _ => unavailable(),
    }
}
fn canonical_failure(data: &BTreeMap<String, String>) -> bool {
    data.len() == 1
        && matches!(
            data.iter()
                .next()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            Some(
                ("account", "account unavailable")
                    | ("deck", "invalid deck metadata" | "deck unavailable")
                    | ("revision", "deck changed; reload before saving")
                    | (
                        "entries",
                        "expected deck entry array"
                            | "too many deck entries"
                            | "invalid deck entries"
                    )
                    | (
                        "cosmetics",
                        "default cosmetic unavailable" | "cosmetic selection unavailable"
                    )
                    | ("settings", "deck settings changed; reload before saving")
            )
        )
}

pub(super) async fn disposition(State(state): State<ApiState>, request: Request) -> Response {
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
    let input: DeckCommand = match to_bytes(request.into_body(), 264192)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state.blocking(move |config| {
        let actor=match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor,Err(error)=>return actor_error(error) };
        if actor.account().is_none() { return fail(StatusCode::CONFLICT,"account","unavailable"); }
        let backend=match backend(config) { Ok(backend)=>backend,Err(_)=>return unavailable() };
        let expected_resource=match &input { DeckCommand::Settings(command)=>Some(command.deck_id),_=>None };
        let (kind,command)=match input {
            DeckCommand::Create(command)=>("create",serde_json::to_value(command)),
            DeckCommand::Save(command)=>("save",serde_json::to_value(command)),
            DeckCommand::Settings(command)=>("settings",serde_json::to_value(command)),
        };
        let command=match command {Ok(command)=>command,Err(_)=>return unavailable()};
        let result: Result<ApiResponse<CommandDisposition>,_>=backend.rpc("read_deck_command",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_kind":kind,"p_command":command}));
        match result {
            Ok(ApiResponse::Success{data})=>{
                let valid=match &data {
                    CommandDisposition::Unknown | CommandDisposition::Pending=>true,
                    CommandDisposition::Accepted{ack}=>!ack.resource_id.is_nil() && expected_resource.is_none_or(|id| ack.resource_id==id),
                    CommandDisposition::Rejected{errors}=>canonical_failure(errors),
                };
                if valid {success(data)} else {unavailable()}
            },
            Ok(ApiResponse::Fail{data}) if data.len()==1=>match data.iter().next().map(|(key,value)|(key.as_str(),value.as_str())) {
                Some(("session","unauthorized"))=>fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
                Some(("request",reason @ ("invalid" | "conflict")))=>fail(StatusCode::CONFLICT,"request",reason),
                _=>unavailable(),
            },
            _=>unavailable(),
        }
    }).await
}
pub(super) async fn detail(
    State(state): State<ApiState>,
    deck: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let deck = match identifier(deck) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "deck", "invalid"),
    };
    state
        .blocking(
            move |config| match config.actor().and_then(|client| client.deck(&token, deck)) {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { data }) if data.contains_key("deck") => {
                    fail(StatusCode::NOT_FOUND, "deck", "unavailable")
                }
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::CONFLICT, "account", "unavailable")
                }
                Ok(ApiResponse::Error { .. }) => unavailable(),
                Err(error) => actor_error(error),
            },
        )
        .await
}
pub(super) async fn formats(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            match config
                .actor()
                .and_then(|client| client.formats(&token, query.after, limit))
            {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::CONFLICT, "account", "unavailable")
                }
                Ok(ApiResponse::Error { .. }) => unavailable(),
                Err(error) => actor_error(error),
            }
        })
        .await
}
