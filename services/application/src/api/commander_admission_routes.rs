//! Recover the authenticated player's original room or between-game receipt.
use super::*;
use crate::{
    actor::RequestActor,
    qualified_intents::{
        MAX_QUALIFIED_INTENT_BYTES, RoomAdmissionIntent, force_start_failure, ready_failure,
        valid_room_ack,
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
        let (Some(runtime), Some(account)) = (&config.native_views, actor.account()) else {return unavailable()};
        let repo=match BackendConfig::new(&config.base,&config.secret_key).and_then(|c|BackendClient::new(c,"spellwake-admission-source",Duration::from_secs(60))) {
            Ok(c)=>crate::native_game::preparation_source::NativePreparationRepository::new(c), Err(_)=>return unavailable()
        };
        let prepared = || {
            if let Some(manifest) = &config.commander_admission
                && let Some(observation) = repo.room(
                    room, &original.expected_revision, manifest.release_id, manifest.legality_revision_id,
                )?
                && observation.terms().room.format_id == manifest.format_id
            {
                return Ok(runtime.prepare_commander_room(manifest, &observation, account, countdown));
            }
            let Some(manifest) = &config.admission else { return Ok(None); };
            manifest.prepare_room(&repo, runtime.admission_programs(), room, &original, account, countdown)
        };
        let Ok(Some(prepared)) = prepared() else {
            return original_outcome().unwrap_or_else(unavailable);
        };
        args["p_prepared"] = prepared;
        if let Err(error)=actor.ensure_unexpired() {return actor_error(error);}
        let rpc=if countdown {"qualify_room_countdown_command"} else {"qualify_room_ready_command"};
        let written=backend.rpc_object(rpc,&args);
        if written.is_err() && let Some(response)=original_outcome() {return response;}
        command_routes::typed_reply(written,false,valid,failure,|_|false)
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
    if !is_json_content_type(request.headers()) {
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
