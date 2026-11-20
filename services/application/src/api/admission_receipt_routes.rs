//! Recover the authenticated player's original room or between-game receipt.
use super::*;
use crate::{
    actor::RequestActor,
    qualified_intents::{
        BetweenGameIntent, MAX_QUALIFIED_INTENT_BYTES, RoomAdmissionIntent, StoredBetweenAck,
        between_failure, force_start_failure, ready_failure, valid_room_ack,
    },
};

#[derive(Clone, Copy)]
enum RoomCommand {
    Ready,
    ForceStart,
}

pub(super) async fn ready(
    State(s): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    room(s, path, request, RoomCommand::Ready, false).await
}

pub(super) async fn force_start(
    State(s): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    room(s, path, request, RoomCommand::ForceStart, false).await
}

pub(super) async fn execute_ready(
    State(s): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    room(s, path, request, RoomCommand::Ready, true).await
}
pub(super) async fn execute_force_start(
    State(s): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    room(s, path, request, RoomCommand::ForceStart, true).await
}

async fn room(
    state: ApiState,
    path: IdentifierPath,
    request: Request,
    command: RoomCommand,
    execute: bool,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "room", reason),
    };
    read(state, request, move |config, backend, actor, original: RoomAdmissionIntent| {
        use crate::commands::{CommandDisposition, CommandResponse, RevisionAck};
        let countdown = matches!(command, RoomCommand::ForceStart);
        let rpc = if countdown { "read_qualified_room_countdown_command" } else { "read_qualified_room_ready_command" };
        let mut args = json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_room":room,"p_original":original});
        let valid = |ack: &RevisionAck| valid_room_ack(&original, room, ack);
        let failure = |errors: &std::collections::BTreeMap<String,String>| match command {
            RoomCommand::Ready => ready_failure(errors), RoomCommand::ForceStart => force_start_failure(errors)
        };
        let read_prior = || backend.rpc_object::<ApiResponse<CommandResponse<RevisionAck>>>(rpc,&json!({
            "p_subject":actor.subject(),"p_session":actor.session(),"p_room":room,"p_original":original
        }));
        if !execute {return command_routes::typed_reply(read_prior(), true, valid, failure, |_|false);}
        let original_outcome = || match read_prior() {
            Ok(ApiResponse::Success {data:CommandResponse::Disposition(CommandDisposition::Accepted{ack})}) =>
                Some(command_routes::typed_reply(Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)}), false, valid, failure, |_|false)),
            Ok(ApiResponse::Success {data:CommandResponse::Disposition(CommandDisposition::Rejected{errors})}) if failure(&errors) =>
                Some(command_routes::typed_reply(Ok(ApiResponse::Fail{data:errors}), false, valid, failure, |_|false)),
            Ok(ApiResponse::Success {data:CommandResponse::Disposition(CommandDisposition::Unknown)}) => None,
            Ok(ApiResponse::Fail{data}) => Some(command_routes::typed_reply(Ok(ApiResponse::Fail{data}),false,valid,failure,|_|false)),
            _ => Some(unavailable()),
        };
        if let Some(response)=original_outcome() {return response;}
        let (Some(manifest),Some(runtime),Some(account))=(&config.admission,&config.native_views,actor.account()) else {return unavailable()};
        let repo=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-admission-source",Duration::from_secs(60))) {
            Ok(c)=>crate::native_game::preparation_source::NativePreparationRepository::new(c), Err(_)=>return unavailable()
        };
        match manifest.prepare_room(&repo,runtime.admission_programs(),room,&original,account,countdown) {
            Ok(Some(prepared))=>args["p_prepared"]=prepared,
            Ok(None)|Err(_)=>return original_outcome().unwrap_or_else(unavailable),
        }
        if let Err(error)=actor.ensure_unexpired() {return actor_error(error);}
        let rpc=if countdown {"qualify_room_countdown_command"} else {"qualify_room_ready_command"};
        let written=backend.rpc_object(rpc,&args);
        if written.is_err() && let Some(response)=original_outcome() {return response;}
        command_routes::typed_reply(written,false,valid,failure,|_|false)
    }).await
}

pub(super) async fn between(State(state): State<ApiState>, request: Request) -> Response {
    read(state,request,|_,backend,actor,original:BetweenGameIntent| {
        let result = backend.rpc_object("read_qualified_between_game_command",
            &json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_original":original}));
        command_routes::typed_reply(result,true,|ack:&StoredBetweenAck|original.valid_ack(&ack.0),
            between_failure,|errors| errors.len()==1 && errors.get("actor").is_some_and(|s|s=="unavailable"))
    }).await
}

pub(super) async fn execute_between(State(state): State<ApiState>, request: Request) -> Response {
    use crate::commands::{CommandDisposition, CommandResponse};
    use crate::qualified_intents::BetweenDecisionOwner;
    read(state, request, |config, backend, actor, original: BetweenGameIntent| {
        let args = json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_original":original});
        let valid = |ack:&StoredBetweenAck| original.valid_ack(&ack.0);
        let foreign = |errors: &std::collections::BTreeMap<String,String>| errors.len()==1 && errors.get("actor").is_some_and(|s|s=="unavailable");
        let previous = || {
            let lookup = backend.rpc_object::<ApiResponse<CommandResponse<StoredBetweenAck>>>("read_qualified_between_game_command",&args);
            let result = match lookup {
                Ok(ApiResponse::Success{data:CommandResponse::Disposition(CommandDisposition::Accepted{ack})}) => Ok(ApiResponse::Success{data:CommandResponse::Ack(ack)}),
                Ok(ApiResponse::Success{data:CommandResponse::Disposition(CommandDisposition::Rejected{errors})}) if between_failure(&errors) => Ok(ApiResponse::Fail{data:errors}),
                Ok(ApiResponse::Success{data:CommandResponse::Disposition(CommandDisposition::Unknown)}) => return None,
                Ok(ApiResponse::Fail{data}) => Ok(ApiResponse::Fail{data}),
                _ => return Some(unavailable()),
            };
            Some(command_routes::typed_reply(result,false,valid,between_failure,foreign))
        };
        if let Some(response)=previous() { return response; }
        // SQL227 returns unknown only after verifying this human owns this decision.
        let (Some(account), Some(manifest), Some(runtime))=(actor.account(), &config.admission, &config.native_views) else { return unavailable(); };
        let owner = BetweenDecisionOwner { decision_id:original.decision_id, account_id:account };
        let Ok(client) = BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-between-source",Duration::from_secs(60))) else {return unavailable();};
        let repo = crate::native_game::preparation_source::NativePreparationRepository::new(client);
        let Ok(Some(prepared)) = manifest.prepare_between(&repo,runtime.admission_programs(),&original,&owner,account) else {
            return previous().unwrap_or_else(unavailable);
        };
        if let Err(error)=actor.ensure_unexpired() {return actor_error(error);}
        let mut submission=args.clone();
        submission["p_prepared"]=prepared;
        let written=backend.rpc_object("qualify_between_game_command",&submission);
        if written.is_err() && let Some(response)=previous() {return response;}
        command_routes::typed_reply(written,false,valid,between_failure,foreign)
    }).await
}

async fn read<I, F>(state: ApiState, request: Request, lookup: F) -> Response
where
    I: serde::de::DeserializeOwned + Send + 'static,
    F: FnOnce(&ApiConfig, &BackendClient, &RequestActor, I) -> Response + Send + 'static,
{
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if !single_content_type(request.headers()).is_some_and(|v| v == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let original = match to_bytes(request.into_body(), MAX_QUALIFIED_INTENT_BYTES)
        .await
        .ok()
        .and_then(|bytes| parse_object::<I>(&bytes).ok())
    {
        Some(intent) => intent,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(error) => return actor_error(error),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match BackendConfig::new(&config.base, &config.secret_key).and_then(|c| {
                BackendClient::new(c, "spellwake-admission-receipt", Duration::from_secs(60))
            }) {
                Ok(client) => client,
                Err(_) => return unavailable(),
            };
            let response = lookup(config, &backend, &actor, original);
            match actor.ensure_unexpired() {
                Ok(()) => response,
                Err(error) => actor_error(error),
            }
        })
        .await
}
