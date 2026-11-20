//! Human room intents call narrow verified-session facades; no private worker scope is exposed.
use super::*;
use crate::{
    commands::CommandResponse,
    room_controls::{BootstrapRequest, ControlAck, ControlKind, RevisionRequest},
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
    if !single_content_type(request.headers()).is_some_and(|v| v == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let (request, basis, revision) = if matches!(kind, ControlKind::Bootstrap) {
        match parse_object::<BootstrapRequest>(&bytes) {
            Ok(i) => (i.request_id, i.expected_revision, i.deck_revision_id),
            Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
        }
    } else {
        match parse_object::<RevisionRequest>(&bytes) {
            Ok(i) => (i.request_id, i.expected_revision, None),
            Err(_) => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
        }
    };
    if request.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state.blocking(move|config|{
  let actor=match config.actor().and_then(|c|c.resolve(&token)){Ok(a)=>a,Err(e)=>return actor_error(e)};
  if actor.account().is_none(){return fail(StatusCode::CONFLICT,"account","unavailable");}
  let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-room-control",Duration::from_secs(60))){Ok(b)=>b,Err(_)=>return unavailable()};
  let (name,params)=match kind {
   ControlKind::Add=>(if lookup{"read_add_room_ai_slot_command"}else{"add_room_ai_slot_command"},json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_room":target,"p_expected":basis})),
   ControlKind::Unready=>(if lookup{"read_unready_room_command"}else{"unready_room_command"},json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_room":target,"p_expected":basis})),
   ControlKind::Bootstrap|ControlKind::Remove=>(if lookup{"read_ai_slot_command"}else{"manage_ai_slot_command"},json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_kind":if matches!(kind,ControlKind::Bootstrap){"bootstrap"}else{"remove"},"p_slot":target,"p_expected":basis,"p_revision":revision})),
  };
  let result:Result<ApiResponse<CommandResponse<ControlAck>>,_>=backend.rpc(name,&params);
  command_routes::typed_reply(result,lookup,|ack|kind.valid_ack(target,&basis,ack),|errors|kind.failure(errors),|_|false)
 }).await
}
