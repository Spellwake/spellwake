//! Admitted human chat and recipient-owned notifications with exact receipt recovery.
use super::*;
use crate::{
    commands::{CommandDisposition, CommandResponse},
    game_chat::SendGameMessage,
    notifications::ReadNotifications,
    social::MessageCursor,
};
use chrono::{DateTime, Utc};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatQuery {
    before_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    before_id: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
pub(super) async fn chat(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let match_id = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "match", reason),
    };
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<ChatQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(16);
    if !(1..=16).contains(&limit)
        || query.before_at.is_some() != query.before_id.is_some()
        || query.before_id.is_some_and(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    let before = query
        .before_at
        .zip(query.before_id)
        .map(|(sent_at, id)| MessageCursor { sent_at, id });
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor()
                    .and_then(|a| a.game_chat(&token, match_id, before.as_ref(), limit)),
                Some("conversation"),
            )
        })
        .await
}
pub(super) async fn notifications(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<NotificationQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |c| {
            economy_routes::projection(
                c.actor()
                    .and_then(|a| a.notifications(&token, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn send(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    chat_write(s, p, r, false).await
}
pub(super) async fn send_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    chat_write(s, p, r, true).await
}
async fn chat_write(s: ApiState, p: IdentifierPath, r: Request, lookup: bool) -> Response {
    let match_id = match identifier(p) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "match", reason),
    };
    write(s, Some(match_id), r, lookup).await
}
pub(super) async fn mark_read(State(s): State<ApiState>, r: Request) -> Response {
    write(s, None, r, false).await
}
pub(super) async fn read_disposition(State(s): State<ApiState>, r: Request) -> Response {
    write(s, None, r, true).await
}
enum Intent {
    Chat(SendGameMessage),
    Read(ReadNotifications),
}
async fn write(
    state: ApiState,
    match_id: Option<Uuid>,
    request: Request,
    lookup: bool,
) -> Response {
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
    let bytes = match to_bytes(request.into_body(), 32768).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let input = if match_id.is_some() {
        parse_object::<SendGameMessage>(&bytes).map(Intent::Chat)
    } else {
        parse_object::<ReadNotifications>(&bytes).map(Intent::Read)
    };
    let input = match input {
        Ok(Intent::Chat(i)) if !i.request_id.is_nil() => Intent::Chat(i),
        Ok(Intent::Read(i)) if i.validate().is_ok() => Intent::Read(i),
        _ => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
    };
    state.blocking(move|config|{
        let actor=match config.actor().and_then(|c|c.resolve(&token)){Ok(a)=>a,Err(e)=>return actor_error(e)};
        let Some(owner)=actor.account() else{return fail(StatusCode::CONFLICT,"account","unavailable")};
        let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-chat-notifications",Duration::from_secs(60))){Ok(c)=>c,Err(_)=>return unavailable()};
        let chat=matches!(&input,Intent::Chat(_));
        let (name,params)=match input {
            Intent::Chat(i)=>(if lookup{"read_game_message_command"}else{"send_game_message_command"},json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":i.request_id,"p_match":match_id,"p_body":i.body})),
            Intent::Read(i)=>(if lookup{"read_notifications_command"}else{"mark_notifications_read"},json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":i.request_id,"p_notifications":i.notification_ids})),
        };
        let valid_ack=|id:Uuid| !id.is_nil() && (chat || id==owner);
        let response:Result<ApiResponse<CommandResponse>,_>=backend.rpc(name,&params);
        match response {
            Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)}) if !lookup && valid_ack(ack.resource_id)=>(StatusCode::ACCEPTED,Json(ApiResponse::Success{data:ack})).into_response(),
            Ok(ApiResponse::Success{data:CommandResponse::Disposition(d)}) if lookup=>{
                let valid=match &d{CommandDisposition::Accepted{ack}=>valid_ack(ack.resource_id),CommandDisposition::Rejected{errors}=>known_failure(errors,chat),_=>true};
                if valid{success(d)}else{unavailable()}
            },
            Ok(ApiResponse::Fail{data})=> {
                if data.len()==1 && data.get("session").is_some_and(|s|s=="unauthorized"){return fail(StatusCode::UNAUTHORIZED,"session","unauthorized");}
                if known_failure(&data,chat) || (data.len()==1 && data.get("request").is_some_and(|v|matches!(v.as_str(),"invalid"|"conflict"))){(StatusCode::CONFLICT,Json(ApiResponse::<Value>::Fail{data})).into_response()}else{unavailable()}
            },_=>unavailable(),
        }
    }).await
}
fn known_failure(errors: &BTreeMap<String, String>, chat: bool) -> bool {
    errors.len() == 1
        && errors.iter().all(|(k, v)| match (k.as_str(), v.as_str()) {
            ("conversation", "conversation unavailable")
            | ("body", "message must contain 1 to 4000 characters") => chat,
            ("notifications", "unavailable") => !chat,
            _ => false,
        })
}
