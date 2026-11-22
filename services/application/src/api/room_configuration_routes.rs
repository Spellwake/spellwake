//! Host configuration transport uses existing canonical room transactions.
use super::*;
use crate::rooms::configuration::{
    ConfigurationDisposition, ConfigurationResponse, ConfigureRoom, failure,
};
type RoomPath = IdentifierPath;
pub(super) async fn read(
    State(state): State<ApiState>,
    path: RoomPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "room", reason),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    state
        .blocking(move |config| {
            let result = config
                .actor()
                .and_then(|client| client.room_configuration(&token, room));
            match result {
                Ok(ApiResponse::Fail { data })
                    if data
                        .get("configuration")
                        .is_some_and(|s| s == "unavailable") =>
                {
                    fail(StatusCode::CONFLICT, "configuration", "unavailable")
                }
                result => economy_routes::projection(result, Some("room")),
            }
        })
        .await
}
pub(super) async fn execute(
    State(state): State<ApiState>,
    path: RoomPath,
    request: Request,
) -> Response {
    write(state, path, request, false).await
}
pub(super) async fn disposition(
    State(state): State<ApiState>,
    path: RoomPath,
    request: Request,
) -> Response {
    write(state, path, request, true).await
}
async fn write(state: ApiState, path: RoomPath, request: Request, lookup: bool) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "room", reason),
    };
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
    let command: ConfigureRoom = match to_bytes(request.into_body(), 32768)
        .await
        .ok()
        .and_then(|b| parse_object(&b).ok())
    {
        Some(value) => value,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if command.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "configuration", "invalid");
    }
    state.blocking(move|config|{
        let actor=match config.actor().and_then(|client|client.resolve(&token)){Ok(actor)=>actor,Err(error)=>return actor_error(error)};
        if actor.account().is_none(){return fail(StatusCode::CONFLICT,"account","unavailable");}
        let backend=match BackendConfig::new(&config.base,&config.secret_key).and_then(|config|BackendClient::new(config,"spellwake-room-configuration",Duration::from_secs(60))){Ok(client)=>client,Err(_)=>return unavailable()};
        let result:Result<ApiResponse<ConfigurationResponse>,_>=backend.rpc(if lookup{"read_room_configuration_command"}else{"configure_room_command"},&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":command.request_id,"p_room":room,"p_expected":command.expected_revision,"p_profile":command.profile_id,"p_structure":command.structure,"p_teams":command.teams}));
        match result {
            Ok(ApiResponse::Success{data:ConfigurationResponse::Ack(ack)}) if !lookup && command.valid_ack(room,&ack)=>(StatusCode::ACCEPTED,Json(ApiResponse::Success{data:ack})).into_response(),
            Ok(ApiResponse::Success{data:ConfigurationResponse::Disposition(disposition)}) if lookup=>{
                let valid=match &disposition{ConfigurationDisposition::Accepted{ack}=>command.valid_ack(room,ack),ConfigurationDisposition::Rejected{errors}=>failure(errors),_=>true};
                if valid{success(disposition)}else{unavailable()}
            }
            Ok(ApiResponse::Fail{data}) if data.len()==1=>match data.iter().next().map(|(k,v)|(k.as_str(),v.as_str())){
                Some(("session","unauthorized"))=>fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
                Some((field,reason)) if failure(&data)||matches!((field,reason),("request","invalid"|"conflict"))=>fail(StatusCode::CONFLICT,field,reason),
                _=>unavailable(),
            },
            _=>unavailable(),
        }
    }).await
}
