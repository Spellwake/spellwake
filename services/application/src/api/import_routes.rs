//! Resolved deck import executes and recovers one original verified-session intent.
use super::*;
use crate::decks::import::{ImportDeck, ImportDisposition, ImportResponse, valid_failure};

pub(super) async fn execute(State(state): State<ApiState>, request: Request) -> Response {
    handle(state, request, false).await
}
pub(super) async fn disposition(State(state): State<ApiState>, request: Request) -> Response {
    handle(state, request, true).await
}
async fn handle(state: ApiState, request: Request, lookup: bool) -> Response {
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
    let input: ImportDeck = match to_bytes(request.into_body(), 262144)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "import", "invalid");
    }
    state.blocking(move|config|{
        let actor=match config.actor().and_then(|client|client.resolve(&token)){Ok(actor)=>actor,Err(error)=>return actor_error(error)};
        if actor.account().is_none(){return fail(StatusCode::CONFLICT,"account","unavailable");}
        let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|config|BackendClient::new(config,"spellwake-deck-import",Duration::from_secs(60))){Ok(backend)=>backend,Err(_)=>return unavailable()};
        let function=if lookup{"read_deck_import_command"}else{"import_deck_command"};
        let result:Result<ApiResponse<ImportResponse>,_>=backend.rpc(function,&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_input":input}));
        match result{
            Ok(ApiResponse::Success{data:ImportResponse::Ack(ack)}) if !lookup=>(StatusCode::ACCEPTED,Json(ApiResponse::Success{data:ack})).into_response(),
            Ok(ApiResponse::Success{data:ImportResponse::Disposition(disposition)}) if lookup=>{
                if matches!(&disposition,ImportDisposition::Rejected{errors} if !valid_failure(errors)){unavailable()}else{success(disposition)}
            }
            Ok(ApiResponse::Fail{data}) if data.len()==1=>match data.iter().next().map(|(key,value)|(key.as_str(),value.as_str())){
                Some(("session","unauthorized"))=>fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
                Some((field,reason)) if valid_failure(&data)||matches!((field,reason),("request","invalid"|"conflict"))=>fail(StatusCode::CONFLICT,field,reason),
                _=>unavailable(),
            },
            _=>unavailable(),
        }
    }).await
}
