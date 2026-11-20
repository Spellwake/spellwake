//! Replace original ordered approvals and recover the same source; never choose a trusted ballot.
use super::*;
use crate::{
    commands::CommandResponse,
    supply_ballot::{self, BallotAck, ReplaceBallot},
};
pub(super) async fn replace(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, false).await
}
pub(super) async fn disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, true).await
}
async fn write(state: ApiState, path: IdentifierPath, request: Request, lookup: bool) -> Response {
    let season = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "season", reason),
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
    let input = match to_bytes(request.into_body(), 4096)
        .await
        .ok()
        .and_then(|b| parse_object::<ReplaceBallot>(&b).ok())
    {
        Some(i) if i.validate().is_ok() => i,
        _ => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
    };
    state.blocking(move|config|{
  let actor=match config.actor().and_then(|c|c.resolve(&token)){Ok(a)=>a,Err(e)=>return actor_error(e)};
  if actor.account().is_none(){return fail(StatusCode::CONFLICT,"account","unavailable");}
  let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-supply-ballot",Duration::from_secs(60))){Ok(b)=>b,Err(_)=>return unavailable()};
  let result:Result<ApiResponse<CommandResponse<BallotAck>>,_>=backend.rpc(if lookup{"read_supply_ballot_command"}else{"replace_supply_ballot_command"},&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request_id,"p_season":season,"p_sets":input.set_ids}));
  command_routes::typed_reply(result,lookup,|ack|ack.valid(season,&input),supply_ballot::terminal_failure,|_|false)
 }).await
}
