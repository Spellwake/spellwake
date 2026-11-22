//! Original matchmaking consent and owner metadata; no public matcher/roster API.
use super::*;
use crate::{
    commands::{CommandDisposition, CommandResponse},
    matchmaking::{
        self, CancelMatchmaking, EnqueueDisposition, EnqueueMatchmaking, EnqueueResponse,
    },
};
pub(super) async fn enqueue(State(s): State<ApiState>, r: Request) -> Response {
    write(s, None, r, false).await
}
pub(super) async fn enqueue_disposition(State(s): State<ApiState>, r: Request) -> Response {
    write(s, None, r, true).await
}
pub(super) async fn cancel(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    let entry = match identifier(p) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "entry", reason),
    };
    write(s, Some(entry), r, false).await
}
pub(super) async fn cancel_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    let entry = match identifier(p) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "entry", reason),
    };
    write(s, Some(entry), r, true).await
}
enum Intent {
    Enqueue(EnqueueMatchmaking),
    Cancel(CancelMatchmaking),
}
impl Intent {
    fn request(&self) -> Uuid {
        match self {
            Self::Enqueue(i) => i.request_id,
            Self::Cancel(i) => i.request_id,
        }
    }
}
async fn write(state: ApiState, entry: Option<Uuid>, request: Request, lookup: bool) -> Response {
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
    let bytes = match to_bytes(
        request.into_body(),
        if entry.is_some() { 4096 } else { 264192 },
    )
    .await
    {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let input = if entry.is_some() {
        parse_object::<CancelMatchmaking>(&bytes).map(Intent::Cancel)
    } else {
        parse_object::<EnqueueMatchmaking>(&bytes).map(Intent::Enqueue)
    };
    let input = match input {
        Ok(input) if !input.request().is_nil() => input,
        _ => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
    };
    if matches!(&input,Intent::Enqueue(i) if i.validate().is_err()) {
        return fail(StatusCode::BAD_REQUEST, "intent", "invalid");
    }
    state.blocking(move|config|{
  let actor=match config.actor().and_then(|c|c.resolve(&token)){Ok(a)=>a,Err(e)=>return actor_error(e)};
  if actor.account().is_none(){return fail(StatusCode::CONFLICT,"account","unavailable");}
  let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-matchmaking",Duration::from_secs(60))){Ok(c)=>c,Err(_)=>return unavailable()};
  match input{
   Intent::Enqueue(i)=>{
    let (name,params)=if lookup{("read_matchmaking_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_kind":"enqueue","p_request":i.request_id,"p_entry":null,"p_mode":i.mode,"p_competitive":i.competitive,"p_alternatives":i.alternatives}))}else{("enqueue_matchmaking_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":i.request_id,"p_mode":i.mode,"p_competitive":i.competitive,"p_alternatives":i.alternatives}))};
    let result:Result<ApiResponse<EnqueueResponse>,_>=backend.rpc(name,&params);
    match result{
     Ok(ApiResponse::Success{data:EnqueueResponse::Ack(ack)}) if !lookup && ack.valid()=>accepted(ack),
     Ok(ApiResponse::Success{data:EnqueueResponse::Disposition(d)}) if lookup=>{
      let valid=match &d{EnqueueDisposition::Accepted{ack}=>ack.valid(),EnqueueDisposition::Rejected{errors}=>matchmaking::enqueue_failure(errors),_=>true};if valid{success(d)}else{unavailable()}
     },Ok(ApiResponse::Fail{data})=>rejection(data,false),_=>unavailable()
    }
   },
   Intent::Cancel(i)=>{
    let Some(entry)=entry else{return unavailable()};
    let (name,params)=if lookup{("read_matchmaking_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_kind":"cancel","p_request":i.request_id,"p_entry":entry,"p_mode":null,"p_competitive":null,"p_alternatives":null}))}else{("cancel_matchmaking_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":i.request_id,"p_entry":entry}))};
    let result:Result<ApiResponse<CommandResponse>,_>=backend.rpc(name,&params);
    match result{
     Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)}) if !lookup && ack.resource_id==entry=>accepted(ack),
     Ok(ApiResponse::Success{data:CommandResponse::Disposition(d)}) if lookup=>{
      let valid=match &d{CommandDisposition::Accepted{ack}=>ack.resource_id==entry,CommandDisposition::Rejected{errors}=>matchmaking::cancel_failure(errors),_=>true};if valid{success(d)}else{unavailable()}
     },Ok(ApiResponse::Fail{data})=>rejection(data,true),_=>unavailable()
    }
   }
  }
 }).await
}
fn accepted(value: impl Serialize) -> Response {
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse::Success { data: value }),
    )
        .into_response()
}
fn rejection(data: BTreeMap<String, String>, cancel: bool) -> Response {
    if data.len() != 1 {
        return unavailable();
    }
    match data.iter().next().map(|(k, v)| (k.as_str(), v.as_str())) {
        Some(("session", "unauthorized")) => {
            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
        }
        Some((field, reason))
            if (if cancel {
                matchmaking::cancel_failure(&data)
            } else {
                matchmaking::enqueue_failure(&data)
            }) || matches!((field, reason), ("request", "invalid" | "conflict")) =>
        {
            fail(StatusCode::CONFLICT, field, reason)
        }
        _ => unavailable(),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    intent_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigQuery {
    after_index: Option<u16>,
    limit: Option<usize>,
}
pub(super) async fn queue(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<QueueQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if [query.intent_id, query.after]
        .into_iter()
        .flatten()
        .any(|id| id.is_nil())
        || !(1..=100).contains(&limit)
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor()
                    .and_then(|a| a.queue(&token, query.intent_id, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn intent(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let id = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "intent", reason),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor().and_then(|a| a.queue_intent(&token, id)),
                Some("intent"),
            )
        })
        .await
}
pub(super) async fn profiles(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if query.after.is_some_and(|id| id.is_nil()) || !(1..=100).contains(&limit) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor()
                    .and_then(|a| a.table_profiles(&token, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn configurations(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let id = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "profile", reason),
    };
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<ConfigQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if query.after_index.is_some_and(|index| index > 511) || !(1..=100).contains(&limit) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor()
                    .and_then(|a| a.profile_configurations(&token, id, query.after_index, limit)),
                Some("profile"),
            )
        })
        .await
}
