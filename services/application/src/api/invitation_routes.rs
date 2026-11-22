//! Secret-safe invitation delivery, issuer release and caller-bound room joins.
use super::*;
use crate::{
    commands::{CommandAck, CommandDisposition, CommandResponse},
    rooms::invitations::{
        self, InvitationToken, IssueInvitation, IssuedInvitation, JoinResponse, JoinRoom,
        RevokeInvitation,
    },
};
#[derive(Clone, Copy)]
enum Kind {
    Issue,
    Join,
    Revoke,
}
impl Kind {
    fn field(self) -> &'static str {
        if matches!(self, Self::Revoke) {
            "invitation"
        } else {
            "room"
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Join => "join",
            Self::Revoke => "revoke",
        }
    }
    fn failure(self, data: &BTreeMap<String, String>) -> bool {
        match self {
            Self::Issue => invitations::issue_failure(data),
            Self::Join => invitations::join_failure(data),
            Self::Revoke => invitations::revoke_failure(data),
        }
    }
}
enum Intent {
    Issue(IssueInvitation),
    Join(JoinRoom),
    Revoke(RevokeInvitation),
}
impl Intent {
    fn request(&self) -> Uuid {
        match self {
            Self::Issue(i) => i.request_id,
            Self::Join(i) => i.request_id,
            Self::Revoke(i) => i.request_id,
        }
    }
    fn basis(&self) -> Option<&crate::amount::Amount> {
        match self {
            Self::Issue(i) => Some(&i.expected_revision),
            _ => None,
        }
    }
}
pub(super) async fn issue(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Issue, false).await
}
pub(super) async fn issue_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Issue, true).await
}
pub(super) async fn join(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Join, false).await
}
pub(super) async fn join_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Join, true).await
}
pub(super) async fn revoke(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Revoke, false).await
}
pub(super) async fn revoke_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Revoke, true).await
}
async fn write(
    state: ApiState,
    path: IdentifierPath,
    request: Request,
    kind: Kind,
    lookup: bool,
) -> Response {
    let target = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, kind.field(), reason),
    };
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
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let input = match kind {
        Kind::Issue => parse_object::<IssueInvitation>(&bytes).map(Intent::Issue),
        Kind::Join => parse_object::<JoinRoom>(&bytes).map(Intent::Join),
        Kind::Revoke => parse_object::<RevokeInvitation>(&bytes).map(Intent::Revoke),
    };
    let input = match input {
        Ok(input) if !input.request().is_nil() => input,
        _ => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
    };
    state.blocking(move|config|{
  let actor=match config.actor().and_then(|c|c.resolve(&token)){Ok(actor)=>actor,Err(e)=>return actor_error(e)};
  let Some(owner)=actor.account() else{return fail(StatusCode::CONFLICT,"account","unavailable")};
  // Authority reads still occur when the dedicated key is absent; no mutation is attempted.
  let delivery=match &input{Intent::Issue(i)=>match config.invitation_key.as_ref().and_then(|key|key.derive(owner,target,i.request_id,&i.expected_revision).ok()){Some(t)=>Some(t),None=>return unavailable()},_=>None};
  let hash=match &input{Intent::Issue(_)=>delivery.as_ref().map(InvitationToken::hash).transpose(),Intent::Join(i)=>i.token.as_ref().map(InvitationToken::hash).transpose(),Intent::Revoke(_)=>Ok(None)};
  let hash=match hash{Ok(hash)=>hash,Err(_)=>return unavailable()};
  let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|cfg|BackendClient::new(cfg,"spellwake-room-invitations",Duration::from_secs(60))){Ok(c)=>c,Err(_)=>return unavailable()};
  let (name,params)=if lookup{("read_room_invitation_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_kind":kind.name(),"p_request":input.request(),"p_target":target,"p_expected":input.basis(),"p_token_hash":hash}))}else{match kind{
    Kind::Issue=>("issue_room_invitation_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request(),"p_room":target,"p_expected":input.basis(),"p_token_hash":hash})),
    Kind::Join=>("join_room_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request(),"p_room":target,"p_token_hash":hash})),
    Kind::Revoke=>("revoke_room_invitation_command",json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":input.request(),"p_invitation":target})),
  }};
  if matches!(kind,Kind::Join){
   let result:Result<ApiResponse<JoinResponse>,_>=backend.rpc(name,&params);
   match result{
    Ok(ApiResponse::Success{data:JoinResponse::Ack(ack)}) if !lookup && ack.valid(target)=>accepted(ack),
    Ok(ApiResponse::Success{data:JoinResponse::Disposition(d)}) if lookup=>{
     let valid=match &d{CommandDisposition::Accepted{ack}=>ack.valid(target),CommandDisposition::Rejected{errors}=>kind.failure(errors),_=>true};if valid{success(d)}else{unavailable()}
    },Ok(ApiResponse::Fail{data})=>rejection(kind,data),_=>unavailable()
   }
  }else{
   let result:Result<ApiResponse<CommandResponse>,_>=backend.rpc(name,&params);
   match result{
    Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)}) if !lookup && valid_ack(kind,target,&ack)=>match delivery{Some(token)=>match invitations::deliver(ack,token){Some(ack)=>accepted(ack),None=>unavailable()},None=>accepted(ack)},
    Ok(ApiResponse::Success{data:CommandResponse::Disposition(d)}) if lookup=>{
     let valid=match &d{CommandDisposition::Accepted{ack}=>valid_ack(kind,target,ack),CommandDisposition::Rejected{errors}=>kind.failure(errors),_=>true};
     if !valid{return unavailable();}
     match delivery{Some(token)=>match deliver_disposition(d,token){Some(d)=>success(d),None=>unavailable()},None=>success(d)}
    },Ok(ApiResponse::Fail{data})=>rejection(kind,data),_=>unavailable()
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
fn valid_ack(kind: Kind, target: Uuid, ack: &CommandAck) -> bool {
    !ack.resource_id.is_nil() && (!matches!(kind, Kind::Revoke) || ack.resource_id == target)
}
fn deliver_disposition(
    d: CommandDisposition,
    token: InvitationToken,
) -> Option<CommandDisposition<IssuedInvitation>> {
    Some(match d {
        CommandDisposition::Unknown => CommandDisposition::Unknown,
        CommandDisposition::Pending => CommandDisposition::Pending,
        CommandDisposition::Rejected { errors } => CommandDisposition::Rejected { errors },
        CommandDisposition::Accepted { ack } => CommandDisposition::Accepted {
            ack: invitations::deliver(ack, token)?,
        },
    })
}
fn rejection(kind: Kind, data: BTreeMap<String, String>) -> Response {
    if data.len() != 1 {
        return unavailable();
    }
    match data.iter().next().map(|(k, v)| (k.as_str(), v.as_str())) {
        Some(("session", "unauthorized")) => {
            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
        }
        Some((field, reason))
            if kind.failure(&data)
                || matches!((field, reason), ("request", "invalid" | "conflict")) =>
        {
            fail(StatusCode::CONFLICT, field, reason)
        }
        _ => unavailable(),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    room_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
pub(super) async fn list(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<ListQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || [query.room_id, query.after]
            .into_iter()
            .flatten()
            .any(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.invitations(&token, query.room_id, query.after, limit)),
                None,
            )
        })
        .await
}
