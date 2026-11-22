//! Original human stake/consent only; actor seat and source authority are server-derived.
use super::*;
use crate::{
    ante::{AcceptAnte, AnteAck, AnteIntent, CancelAnte, OfferAnte},
    commands::CommandResponse,
};
#[derive(Clone, Copy)]
enum Kind {
    Offer,
    Accept,
    Cancel,
}
pub(super) async fn offer(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Offer, false).await
}
pub(super) async fn offer_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Offer, true).await
}
pub(super) async fn accept(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Accept, false).await
}
pub(super) async fn accept_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Accept, true).await
}
pub(super) async fn cancel(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    write(s, p, r, Kind::Cancel, false).await
}
pub(super) async fn cancel_disposition(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    write(s, p, r, Kind::Cancel, true).await
}
async fn write(
    state: ApiState,
    path: IdentifierPath,
    request: Request,
    kind: Kind,
    lookup: bool,
) -> Response {
    let field = if matches!(kind, Kind::Offer) {
        "match"
    } else {
        "commitment"
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
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 32768).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let input = match kind {
        Kind::Offer => parse_object::<OfferAnte>(&bytes).map(AnteIntent::Offer),
        Kind::Accept => parse_object::<AcceptAnte>(&bytes).map(AnteIntent::Accept),
        Kind::Cancel => parse_object::<CancelAnte>(&bytes).map(AnteIntent::Cancel),
    };
    let input = match input {
        Ok(i) if i.valid() => i,
        _ => return fail(StatusCode::BAD_REQUEST, "request", "invalid"),
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match BackendConfig::new(&config.base, &config.secret_key).and_then(|c| {
                BackendClient::new(c, "spellwake-human-ante", Duration::from_secs(60))
            }) {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<CommandResponse<AnteAck>>, _> = backend.rpc(
                if lookup {
                    "read_human_ante_command"
                } else {
                    "human_ante_command"
                },
                &input.parameters(actor.subject(), actor.session(), target),
            );
            command_routes::typed_reply(
                result,
                lookup,
                |ack| input.valid_ack(target, ack),
                |errors| input.terminal_failure(errors),
                |errors| {
                    errors.len() == 1
                        && errors.iter().all(|(k, v)| {
                            matches!((k.as_str(), v.as_str()), ("match", "unavailable"))
                                || (matches!(input, AnteIntent::Offer(_))
                                    && matches!(
                                        (k.as_str(), v.as_str()),
                                        ("assets", "ante bundle unavailable")
                                    ))
                        })
                },
            )
        })
        .await
}
