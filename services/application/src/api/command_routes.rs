//! Shared authenticated command transport; domain policy remains in canonical writers.
use super::*;
use crate::{
    commands::{
        CommandAck, CommandDisposition, CommandResponse, MarkDirectRead, SocialCommand,
        social_failure,
    },
    economy_commands::EconomyCommand,
    predictions::PredictionCommand,
    profile_art::ChangeProfileArt,
    rooms::RoomCommand,
};

pub(super) async fn prediction_execute(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::Prediction).await
}
pub(super) async fn prediction_disposition(
    State(state): State<ApiState>,
    input: Request,
) -> Response {
    command(state, input, true, WriteKind::Prediction).await
}

pub(super) async fn economy_execute(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::Economy).await
}
pub(super) async fn economy_disposition(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, true, WriteKind::Economy).await
}

pub(super) async fn social_execute(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::Social).await
}

pub(super) async fn social_disposition(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, true, WriteKind::Social).await
}

pub(super) async fn mark_read(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::Read).await
}
pub(super) async fn read_disposition(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, true, WriteKind::Read).await
}
pub(super) async fn room_execute(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::Room).await
}
pub(super) async fn room_disposition(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, true, WriteKind::Room).await
}
pub(super) async fn profile_art_execute(State(state): State<ApiState>, input: Request) -> Response {
    command(state, input, false, WriteKind::ProfileArt).await
}
pub(super) async fn profile_art_disposition(
    State(state): State<ApiState>,
    input: Request,
) -> Response {
    command(state, input, true, WriteKind::ProfileArt).await
}
enum WriteKind {
    ProfileArt,
    Room,
    Prediction,
    Social,
    Read,
    Economy,
}
async fn command(state: ApiState, input: Request, lookup: bool, kind: WriteKind) -> Response {
    private(command_inner(state, input, lookup, kind).await)
}
enum WriteCommand {
    ProfileArt(ChangeProfileArt),
    Room(RoomCommand),
    Prediction(PredictionCommand),
    Social(SocialCommand),
    Read(MarkDirectRead),
    Economy(EconomyCommand),
}
impl WriteCommand {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::ProfileArt(command) => command.validate(),
            Self::Room(command) => command.validate(),
            Self::Prediction(command) => command.validate(),
            Self::Social(command) => command.validate(),
            Self::Read(command) => command.validate(),
            Self::Economy(command) => command.validate(),
        }
    }
    fn failure(&self, errors: &BTreeMap<String, String>) -> bool {
        match self {
            Self::ProfileArt(command) => command.failure(errors),
            Self::Room(command) => command.failure(errors),
            Self::Prediction(command) => command.failure(errors),
            Self::Social(_) => social_failure(errors),
            Self::Economy(command) => command.failure(errors),
            Self::Read(_) => {
                errors
                    == &BTreeMap::from([("conversation".into(), "conversation unavailable".into())])
            }
        }
    }
    fn rpc(&self, subject: Uuid, session: Uuid, lookup: bool) -> (&'static str, Value) {
        match self {
            Self::ProfileArt(command) => (
                if lookup {
                    "read_profile_art_command"
                } else {
                    "change_profile_art_command"
                },
                json!({"p_subject":subject,"p_session":session,"p_request":command.request_id,"p_expected":command.expected,"p_art":command.card_art_profile}),
            ),
            Self::Room(command) => command.rpc(subject, session, lookup),
            Self::Prediction(command) => (
                if lookup {
                    "read_prediction_command"
                } else {
                    "execute_prediction_command"
                },
                json!({"p_subject":subject,"p_session":session,"p_input":command}),
            ),
            Self::Economy(command) => (
                if lookup {
                    "read_economy_command"
                } else {
                    "execute_economy_command"
                },
                json!({"p_subject":subject,"p_session":session,"p_input":command}),
            ),
            Self::Social(command) => (
                if lookup {
                    "read_social_command"
                } else {
                    "execute_social_command"
                },
                json!({"p_subject":subject,"p_session":session,"p_input":command}),
            ),
            Self::Read(command) if lookup => (
                "read_direct_read_command",
                json!({"p_subject":subject,"p_session":session,"p_input":command}),
            ),
            Self::Read(command) => (
                "mark_direct_messages_read",
                json!({"p_subject":subject,"p_session":session,"p_request":command.request_id,"p_conversation":command.conversation_id,"p_messages":command.message_ids}),
            ),
        }
    }
}

async fn command_inner(state: ApiState, input: Request, lookup: bool, kind: WriteKind) -> Response {
    let read = matches!(kind, WriteKind::Read);
    let room = matches!(kind, WriteKind::Room);
    let economy = matches!(kind, WriteKind::Economy);
    let prediction = matches!(kind, WriteKind::Prediction);
    if input.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(input.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if !is_json_content_type(input.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(
        input.into_body(),
        if prediction {
            4096
        } else if read {
            2048
        } else if economy {
            65536
        } else {
            32768
        },
    )
    .await
    {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let command = if matches!(kind, WriteKind::ProfileArt) {
        parse_object::<ChangeProfileArt>(&bytes).map(WriteCommand::ProfileArt)
    } else if room {
        parse_object::<RoomCommand>(&bytes).map(WriteCommand::Room)
    } else if prediction {
        parse_object::<PredictionCommand>(&bytes).map(WriteCommand::Prediction)
    } else if economy {
        parse_object::<EconomyCommand>(&bytes).map(WriteCommand::Economy)
    } else if read {
        parse_object::<MarkDirectRead>(&bytes).map(WriteCommand::Read)
    } else {
        parse_object::<SocialCommand>(&bytes).map(WriteCommand::Social)
    };
    let command = match command {
        Ok(command) => command,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if command.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(error) => return actor_error(error),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend =
                match BackendConfig::new(&config.base, &config.secret_key).and_then(|config| {
                    BackendClient::new(config, "spellwake-commands", Duration::from_secs(60))
                }) {
                    Ok(backend) => backend,
                    Err(_) => return unavailable(),
                };
            let (function, parameters) = command.rpc(actor.subject(), actor.session(), lookup);
            let response: Result<ApiResponse<CommandResponse>, _> =
                backend.rpc(function, &parameters);
            match response {
                Ok(ApiResponse::Success {
                    data: CommandResponse::Disposition(disposition),
                }) if lookup && valid_disposition(&command, &disposition, actor.account()) => {
                    success(disposition)
                }
                Ok(ApiResponse::Success {
                    data: CommandResponse::Ack(ack),
                }) if !lookup && valid_ack(&command, &ack, actor.account()) => (
                    StatusCode::ACCEPTED,
                    Json(ApiResponse::Success { data: ack }),
                )
                    .into_response(),
                Ok(ApiResponse::Fail { data }) if data.len() == 1 => {
                    match data
                        .iter()
                        .next()
                        .map(|(key, value)| (key.as_str(), value.as_str()))
                    {
                        Some(("session", "unauthorized")) => {
                            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                        }
                        Some((field, reason))
                            if command.failure(&data)
                                || matches!(
                                    (field, reason),
                                    ("request", "invalid" | "conflict")
                                ) =>
                        {
                            fail(StatusCode::CONFLICT, field, reason)
                        }
                        _ => unavailable(),
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}

fn valid_ack(command: &WriteCommand, ack: &CommandAck, owner: Option<Uuid>) -> bool {
    !ack.resource_id.is_nil()
        && match command {
            WriteCommand::ProfileArt(_) => Some(ack.resource_id) == owner,
            WriteCommand::Social(SocialCommand::Block(command)) => {
                ack.resource_id == command.other_account_id
            }
            WriteCommand::Read(command) => ack.resource_id == command.conversation_id,
            WriteCommand::Room(command) => command.valid_ack(ack),
            WriteCommand::Prediction(command) => command.valid_ack(ack),
            WriteCommand::Economy(command) => command.valid_ack(ack),
            _ => true,
        }
}

fn valid_disposition(
    command: &WriteCommand,
    disposition: &CommandDisposition,
    owner: Option<Uuid>,
) -> bool {
    match disposition {
        CommandDisposition::Unknown | CommandDisposition::Pending => true,
        CommandDisposition::Accepted { ack } => valid_ack(command, ack, owner),
        CommandDisposition::Rejected { errors } => command.failure(errors),
    }
}

pub(super) fn private(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// One response protocol for specialized acknowledgments and exact durable recovery.
/// Domain closures bind original context and separate terminal from outer-only failures.
pub(super) fn typed_reply<A: Serialize + Clone>(
    result: Result<ApiResponse<crate::commands::CommandResponse<A>>, crate::backend::BackendError>,
    lookup: bool,
    valid_ack: impl Fn(&A) -> bool,
    terminal_failure: impl Fn(&BTreeMap<String, String>) -> bool,
    outer_failure: impl Fn(&BTreeMap<String, String>) -> bool,
) -> Response {
    match result {
        Ok(ApiResponse::Success {
            data: crate::commands::CommandResponse::Ack(ack),
        }) if !lookup && valid_ack(&ack) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::Success { data: ack }),
        )
            .into_response(),
        Ok(ApiResponse::Success {
            data: crate::commands::CommandResponse::Disposition(disposition),
        }) if lookup => {
            let valid = match &disposition {
                crate::commands::CommandDisposition::Accepted { ack } => valid_ack(ack),
                crate::commands::CommandDisposition::Rejected { errors } => {
                    terminal_failure(errors)
                }
                _ => true,
            };
            if valid {
                success(disposition)
            } else {
                unavailable()
            }
        }
        Ok(ApiResponse::Fail { data }) if data.len() == 1 => {
            match data.iter().next().map(|(k, v)| (k.as_str(), v.as_str())) {
                Some(("session", "unauthorized")) => {
                    fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                }
                Some((field, reason))
                    if terminal_failure(&data)
                        || outer_failure(&data)
                        || matches!((field, reason), ("request", "invalid" | "conflict")) =>
                {
                    fail(StatusCode::CONFLICT, field, reason)
                }
                _ => unavailable(),
            }
        }
        _ => unavailable(),
    }
}
