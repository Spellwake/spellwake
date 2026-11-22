//! Recover or submit the authenticated player's original between-game reply receipt.
use super::*;
use crate::{
    actor::RequestActor,
    qualified_intents::{
        BetweenGameIntent, MAX_QUALIFIED_INTENT_BYTES, StoredBetweenAck, between_failure,
    },
};

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
