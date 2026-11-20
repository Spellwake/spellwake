//! Stored participant/owner metadata. None of these reads starts work or returns a native view.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreditQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    format_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    season_id: Option<Uuid>,
}
#[derive(Clone, Copy)]
enum OwnKind {
    Matches,
    Competitive,
    DailyWins,
}
pub(super) async fn matches(State(s): State<ApiState>, r: Request) -> Response {
    own(s, r, OwnKind::Matches).await
}
pub(super) async fn competitive(State(s): State<ApiState>, r: Request) -> Response {
    own(s, r, OwnKind::Competitive).await
}
pub(super) async fn daily_wins(State(s): State<ApiState>, r: Request) -> Response {
    own(s, r, OwnKind::DailyWins).await
}
async fn own(state: ApiState, request: Request, kind: OwnKind) -> Response {
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let (after, limit, format, season) = if matches!(kind, OwnKind::Competitive) {
        let q = match Query::<CreditQuery>::try_from_uri(request.uri()) {
            Ok(Query(q)) => q,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
        };
        (q.after, q.limit.unwrap_or(50), q.format_id, q.season_id)
    } else {
        let q = match Query::<PageQuery>::try_from_uri(request.uri()) {
            Ok(Query(q)) => q,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
        };
        (q.after, q.limit.unwrap_or(50), None, None)
    };
    if !(1..=100).contains(&limit)
        || [after, format, season]
            .into_iter()
            .flatten()
            .any(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| match kind {
            OwnKind::Matches => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.match_list(&token, after, limit)),
                None,
            ),
            OwnKind::Competitive => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.competitive_results(&token, after, limit, format, season)),
                None,
            ),
            OwnKind::DailyWins => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.daily_wins(&token, after, limit)),
                None,
            ),
        })
        .await
}
#[derive(Clone, Copy)]
enum TargetKind {
    Detail,
    Games,
    Seats,
    Decisions,
    Ante,
    Ballot,
    AiStatus,
}
impl TargetKind {
    fn field(self) -> &'static str {
        match self {
            Self::Ballot => "season",
            Self::AiStatus => "room",
            _ => "match",
        }
    }
    fn paged(self) -> bool {
        !matches!(self, Self::Detail | Self::Ante)
    }
}
pub(super) async fn detail(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    target(s, p, r, TargetKind::Detail).await
}
pub(super) async fn games(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    target(s, p, r, TargetKind::Games).await
}
pub(super) async fn seats(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    target(s, p, r, TargetKind::Seats).await
}
pub(super) async fn decisions(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    target(s, p, r, TargetKind::Decisions).await
}
pub(super) async fn ante(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    target(s, p, r, TargetKind::Ante).await
}
pub(super) async fn ballot(State(s): State<ApiState>, p: IdentifierPath, r: Request) -> Response {
    target(s, p, r, TargetKind::Ballot).await
}
pub(super) async fn ai_status(
    State(s): State<ApiState>,
    p: IdentifierPath,
    r: Request,
) -> Response {
    target(s, p, r, TargetKind::AiStatus).await
}
async fn target(
    state: ApiState,
    path: IdentifierPath,
    request: Request,
    kind: TargetKind,
) -> Response {
    let id = match identifier(path) {
        Ok(id) => id,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, kind.field(), reason),
    };
    let token = match bearer(request.headers()) {
        Ok(t) => t,
        Err(e) => return actor_error(e),
    };
    let (after, limit) = if kind.paged() {
        let q = match Query::<PageQuery>::try_from_uri(request.uri()) {
            Ok(Query(q)) => q,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
        };
        (q.after, q.limit.unwrap_or(50))
    } else {
        if request.uri().query().is_some() {
            return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
        }
        (None, 50)
    };
    if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| match kind {
            TargetKind::Detail => economy_routes::projection(
                config.actor().and_then(|c| c.match_detail(&token, id)),
                Some("match"),
            ),
            TargetKind::Games => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.match_games(&token, id, after, limit)),
                Some("match"),
            ),
            TargetKind::Seats => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.match_seats(&token, id, after, limit)),
                Some("match"),
            ),
            TargetKind::Decisions => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.match_decisions(&token, id, after, limit)),
                Some("match"),
            ),
            TargetKind::Ante => economy_routes::projection(
                config.actor().and_then(|c| c.match_ante(&token, id)),
                Some("match"),
            ),
            TargetKind::Ballot => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.ballot(&token, id, after, limit)),
                Some("ballot"),
            ),
            TargetKind::AiStatus => economy_routes::projection(
                config
                    .actor()
                    .and_then(|c| c.room_ai_status(&token, id, after, limit)),
                Some("room"),
            ),
        })
        .await
}
