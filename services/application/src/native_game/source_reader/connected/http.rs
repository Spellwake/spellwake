//! Owned actual Rust HTTP boundary over the same real Auth/SQL/private Store.
use super::*;
use crate::player_wire::PlayerSnapshot;
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    time::Duration,
};

pub(super) struct HttpProbe {
    runtime: tokio::runtime::Runtime,
    server: tokio::task::JoinHandle<()>,
    url: String,
    client: Client,
}
impl HttpProbe {
    pub(super) fn new(base: &str, key: &str) -> Self {
        Self::configure(base, key, |runtime| runtime)
    }
    pub(super) fn configure(
        base: &str,
        key: &str,
        configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
    ) -> Self {
        Self::build(base, key, configure, None)
    }
    pub(super) fn owned(base: &str, key: &str, owner: &str) -> Self {
        Self::configure_owned(base, key, owner, |runtime| runtime)
    }
    pub(super) fn configure_owned(
        base: &str,
        key: &str,
        owner: &str,
        configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
    ) -> Self {
        Self::build(
            base,
            key,
            configure,
            Some(NativeHostOwner::new(owner).unwrap()),
        )
    }
    fn build(
        base: &str,
        key: &str,
        configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
        owner: Option<NativeHostOwner>,
    ) -> Self {
        Self::build_with_issuer(base, key, base, configure, owner)
    }
    pub(super) fn forwarded(real_base: &str, proxy_base: &str, key: &str) -> Self {
        Self::build_with_issuer(
            proxy_base,
            key,
            real_base,
            |runtime| runtime,
            Some(NativeHostOwner::new("fixture:source-reader").unwrap()),
        )
    }
    /// Configure a read-only endpoint through the owned transport while retaining
    /// the genuine Auth issuer; no native writer is installed.
    pub(super) fn forwarded_read(
        proxy_base: &str,
        real_base: &str,
        key: &str,
        configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
    ) -> Self {
        Self::build_with_issuer(proxy_base, key, real_base, configure, None)
    }
    fn build_with_issuer(
        base: &str,
        key: &str,
        issuer_base: &str,
        configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
        owner: Option<NativeHostOwner>,
    ) -> Self {
        Self::serve(configuration(base, key, issuer_base, configure, owner))
    }
    pub(super) fn serve(config: crate::api::ApiConfig) -> Self {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = runtime.spawn(async move {
            axum::serve(listener, crate::api::router(config))
                .await
                .unwrap();
        });
        Self {
            runtime,
            server,
            url,
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap(),
        }
    }
    pub(super) fn quests(&self, token: &str) -> Value {
        let response = self
            .client
            .get(format!("{}/v1/quests?limit=100", self.url))
            .bearer_auth(token)
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        response.json().unwrap()
    }
    pub(super) fn command(
        &self,
        token: Option<&str>,
        original: &crate::player_wire::PlayerCommand,
        expected: u16,
    ) -> Value {
        let mut request = self
            .client
            .post(format!(
                "{}/v1/games/{}/commands",
                self.url, original.basis.game_id
            ))
            .json(original);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: Value = response.json().unwrap();
        assert_eq!(
            value["status"],
            if expected == 202 {
                "success"
            } else if expected == 503 {
                "error"
            } else {
                "fail"
            }
        );
        value
    }
    /// Accept a genuine terminal command through the maintained HTTP owner.
    pub(super) fn concede(&self, token: &str, game: Uuid, context: Uuid) {
        let view = self.read(Some(token), game, context, 200);
        let command = crate::player_wire::PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: serde_json::from_value(view["data"]["snapshot"]["basis"].clone()).unwrap(),
            action: crate::player_wire::PlayerAction::Concede {},
        };
        self.command(Some(token), &command, 202);
    }
    /// Record a real session-bound deletion intent in the owned realm.
    pub(super) fn request_deletion(&self, token: &str) {
        let response = self
            .client
            .post(format!("{}/v1/account/deletion", self.url))
            .bearer_auth(token)
            .json(&json!({"requestId":Uuid::new_v4(),"statusKeyHash":"a".repeat(64)}))
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 202);
        let value: Value = response.json().unwrap();
        assert_eq!(value["status"], "success");
    }
    /// Read actual retained participant identity, including permanent tombstones.
    pub(super) fn match_seats(&self, token: &str, matched: Uuid) -> Value {
        let response = self
            .client
            .get(format!("{}/v1/matches/{matched}/seats", self.url))
            .bearer_auth(token)
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let value: Value = response.json().unwrap();
        assert_eq!(value["status"], "success");
        value
    }
    pub(super) fn read(
        &self,
        token: Option<&str>,
        game: Uuid,
        context: Uuid,
        expected_status: u16,
    ) -> serde_json::Value {
        let mut request = self
            .client
            .post(format!("{}/v1/games/{game}/view", self.url))
            .json(&json!({"gameId":game,"contextId":context}));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected_status);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: serde_json::Value = response.json().unwrap();
        assert_eq!(
            value["status"],
            if expected_status == 200 {
                "success"
            } else {
                "fail"
            }
        );
        match expected_status {
            401 => assert_eq!(value["data"]["session"], "unauthorized"),
            404 => assert_eq!(value["data"]["game"], "unavailable"),
            409 => assert_eq!(value["data"]["basis"], "stale"),
            200 => {}
            _ => panic!("unexpected owned HTTP status contract"),
        }
        value
    }
    pub(super) fn history(
        &self,
        token: Option<&str>,
        game: Uuid,
        body: &Value,
        expected: u16,
    ) -> Value {
        let mut request = self
            .client
            .post(format!("{}/v1/games/{game}/history", self.url))
            // Long retained games exercise every single-event archive page in debug builds.
            .timeout(Duration::from_secs(240))
            .json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: Value = response.json().unwrap();
        assert_eq!(
            value["status"],
            match expected {
                200 => "success",
                503 => "error",
                _ => "fail",
            }
        );
        value
    }
    /// Raw status of a POST, for routes expected to refuse in varied ways.
    pub(super) fn admission_receipt(
        &self,
        token: &str,
        path: &str,
        original: &Value,
        expected: u16,
    ) -> Value {
        let response = self
            .client
            .post(format!("{}{path}", self.url))
            .bearer_auth(token)
            .json(original)
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        response.json().unwrap()
    }
    pub(super) fn status(&self, token: Option<&str>, path: &str, body: &Value) -> u16 {
        let mut request = self.client.post(format!("{}{path}", self.url)).json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send().unwrap().status().as_u16()
    }
    pub(super) fn replay(
        &self,
        token: Option<&str>,
        game: Uuid,
        body: &Value,
        expected: u16,
    ) -> Value {
        let mut request = self
            .client
            .post(format!("{}/v1/games/{game}/replay", self.url))
            .json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: Value = response.json().unwrap();
        assert_eq!(
            value["status"],
            match expected {
                200 => "success",
                503 => "error",
                _ => "fail",
            }
        );
        value
    }
    pub(super) fn reconnect(
        &self,
        token: Option<&str>,
        game: Uuid,
        body: &Value,
        expected: u16,
    ) -> Value {
        let mut request = self
            .client
            .post(format!("{}/v1/games/{game}/reconnect", self.url))
            .json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: Value = response.json().unwrap();
        assert_eq!(
            value["status"],
            if expected == 200 {
                "success"
            } else if expected == 503 {
                "error"
            } else {
                "fail"
            }
        );
        value
    }
    fn stream(
        &self,
        token: Option<&str>,
        game: Uuid,
        body: &Value,
        expected: u16,
        count: usize,
    ) -> Vec<Value> {
        let mut request = self
            .client
            .post(format!("{}/v1/games/{game}/stream", self.url))
            .json(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        if expected != 200 {
            assert_eq!(response.headers()["content-type"], "application/json");
            let failure: Value = response.json().unwrap();
            assert_eq!(
                failure["status"],
                if expected == 503 { "error" } else { "fail" }
            );
            return vec![failure];
        }
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let mut reader = BufReader::new(response);
        let mut frames = Vec::new();
        for _ in 0..count {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            assert!(line.len() <= 2 * crate::player_wire::MAX_PLAYER_WIRE_BYTES + 8);
            let json = line
                .strip_prefix("data: ")
                .unwrap()
                .strip_suffix('\n')
                .unwrap();
            let value: Value = serde_json::from_str(json).unwrap();
            assert_eq!(value["status"], "success");
            assert_eq!(value["data"]["kind"], "snapshot");
            line.clear();
            assert_eq!(reader.read_line(&mut line).unwrap(), 1);
            assert_eq!(line, "\n");
            frames.push(value);
        }
        // Drop an incomplete HTTP response, exercising the server body cancellation path.
        frames
    }
    fn public_scope(
        &self,
        token: &str,
        reservation: &Reservation,
    ) -> (Uuid, Uuid, serde_json::Value, serde_json::Value) {
        let get = |path: String| {
            let response = self
                .client
                .get(format!("{}{}", self.url, path))
                .bearer_auth(token)
                .send()
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let value: serde_json::Value = response.json().unwrap();
            assert_eq!(value["status"], "success");
            value
        };
        // The known public match ID locates the match. Neither game nor context
        // comes from the private reservation when constructing the View request.
        let metadata = get(format!("/v1/matches/{}", reservation.match_id));
        let game =
            crate::identifiers::parse(metadata["data"]["match"]["currentGameId"].as_str().unwrap())
                .unwrap();
        let games = get(format!("/v1/matches/{}/games", reservation.match_id));
        let selected = games["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == json!(game))
            .unwrap();
        let context = crate::identifiers::parse(selected["contextId"].as_str().unwrap()).unwrap();
        // Independent owned fixture assertions, not the client input source.
        assert_eq!(game, reservation.game_id);
        assert_eq!(context, reservation.context_id);
        (game, context, metadata, games)
    }
    pub(super) fn check(
        &self,
        bearer: &str,
        peer: &str,
        outsider: &str,
        reservation: &Reservation,
        expected: &PlayerSnapshot,
    ) -> (Uuid, Uuid) {
        let (game, context, owner_metadata, owner_games) = self.public_scope(bearer, reservation);
        let (peer_game, peer_context, peer_metadata, peer_games) =
            self.public_scope(peer, reservation);
        let request = || self.read(Some(bearer), game, context, 200);
        let first = request();
        assert_eq!(
            first["data"]["snapshot"],
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(first, request());
        let peer_view = self.read(Some(peer), peer_game, peer_context, 200);
        assert_eq!(
            peer_view["data"]["snapshot"]["viewerSeatId"],
            json!(reservation.seat_ids[1])
        );
        assert_ne!(
            peer_view["data"]["snapshot"]["viewerSeatId"],
            first["data"]["snapshot"]["viewerSeatId"]
        );
        let empty =
            json!({"receipts":{"gameId":game,"contextId":context,"outstanding":[]},"cursor":null});
        let reconnected = self.reconnect(Some(bearer), game, &empty, 200);
        assert_eq!(reconnected["data"]["snapshot"], first["data"]["snapshot"]);
        assert_eq!(reconnected, self.reconnect(Some(bearer), game, &empty, 200));
        let peer_reconnected = self.reconnect(Some(peer), game, &empty, 200);
        assert_eq!(
            peer_reconnected["data"]["snapshot"],
            peer_view["data"]["snapshot"]
        );
        assert_ne!(
            peer_reconnected["data"]["cursor"]["token"],
            reconnected["data"]["cursor"]["token"]
        );
        let mut previous = empty.clone();
        previous["cursor"] = reconnected["data"]["cursor"].clone();
        let resumed = self.reconnect(Some(bearer), game, &previous, 200);
        assert_eq!(resumed, reconnected);
        let mut foreign = empty.clone();
        foreign["cursor"] = peer_reconnected["data"]["cursor"].clone();
        assert_eq!(
            self.reconnect(Some(bearer), game, &foreign, 409)["data"],
            json!({"cursor":"unavailable"})
        );
        let mut forged = previous.clone();
        forged["cursor"]["token"] = json!(format!("n1.{}", Uuid::new_v4()));
        assert_eq!(
            self.reconnect(Some(bearer), game, &forged, 409)["data"],
            json!({"cursor":"unavailable"})
        );
        let mut unknown = empty.clone();
        unknown["receipts"]["outstanding"] =
            json!([{"requestId":Uuid::new_v4(),"basis":expected.basis,"action":{"kind":"pass"}}]);
        let recovered_unknown = self.reconnect(Some(bearer), game, &unknown, 200);
        assert_eq!(
            recovered_unknown["data"]["snapshot"],
            first["data"]["snapshot"]
        );
        assert_eq!(
            recovered_unknown["data"]["receipts"][0]["original"],
            unknown["receipts"]["outstanding"][0]
        );
        assert_eq!(
            recovered_unknown["data"]["receipts"][0]["disposition"]["state"],
            "unknown"
        );
        self.reconnect(None, game, &empty, 401);
        self.reconnect(Some(outsider), game, &empty, 404);
        let mut other_context = empty.clone();
        other_context["receipts"]["contextId"] = json!(Uuid::new_v4());
        self.reconnect(Some(bearer), game, &other_context, 404);
        let stream_query = json!({"gameId":game,"contextId":context,"cursor":null});
        let started = std::time::Instant::now();
        let owner_frames = self.stream(Some(bearer), game, &stream_query, 200, 2);
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert_eq!(owner_frames[0], owner_frames[1]);
        assert_eq!(
            owner_frames[0]["data"]["snapshot"],
            first["data"]["snapshot"]
        );
        assert_eq!(
            owner_frames[0]["data"]["cursor"],
            reconnected["data"]["cursor"]
        );
        let peer_frames = self.stream(Some(peer), game, &stream_query, 200, 1);
        assert_eq!(
            peer_frames[0]["data"]["snapshot"],
            peer_view["data"]["snapshot"]
        );
        let mut resumed_query = stream_query.clone();
        resumed_query["cursor"] = owner_frames[1]["data"]["cursor"].clone();
        let resumed_frames = self.stream(Some(bearer), game, &resumed_query, 200, 1);
        assert_eq!(resumed_frames[0], owner_frames[1]);
        let mut foreign_query = stream_query.clone();
        foreign_query["cursor"] = peer_frames[0]["data"]["cursor"].clone();
        assert_eq!(
            self.stream(Some(bearer), game, &foreign_query, 409, 0)[0]["data"],
            json!({"cursor":"unavailable"})
        );
        let mut forged_query = resumed_query.clone();
        forged_query["cursor"]["token"] = json!(format!("n1.{}", Uuid::new_v4()));
        assert_eq!(
            self.stream(Some(bearer), game, &forged_query, 409, 0)[0]["data"],
            json!({"cursor":"unavailable"})
        );
        self.stream(None, game, &stream_query, 401, 0);
        self.stream(Some(outsider), game, &stream_query, 404, 0);
        let mut missing_query = stream_query.clone();
        missing_query["contextId"] = json!(Uuid::new_v4());
        self.stream(Some(bearer), game, &missing_query, 404, 0);
        let streams = vec![
            json!({"request":stream_query,"frames":owner_frames}),
            json!({"request":stream_query,"frames":peer_frames}),
            json!({"request":resumed_query,"frames":resumed_frames}),
        ];
        let reconnect = vec![
            json!({"request":empty,"response":reconnected}),
            json!({"request":previous,"response":resumed}),
            json!({"request":empty,"response":peer_reconnected}),
            json!({"request":unknown,"response":recovered_unknown}),
        ];
        // Private owned diagnostic input for the independent maintained Zod
        // consumer. This contains public HTTP payloads, never bearer credentials.
        let capture = std::env::var_os("SPELLWAKE_NATIVE_VIEW_HTTP_WIRE").unwrap();
        std::fs::write(
            capture,
            serde_json::to_vec(&json!({
                "request":{"gameId":game,"contextId":context},
                "metadata":[owner_metadata,peer_metadata],"gamePages":[owner_games,peer_games],"responses":[first,peer_view],"reconnect":reconnect,"streams":streams
            }))
            .unwrap(),
        )
        .unwrap();
        self.read(None, game, context, 401);
        self.read(Some(outsider), game, context, 404);
        self.read(Some(bearer), game, Uuid::new_v4(), 404);
        (game, context)
    }
}
/// One actual fixture configuration shared by TCP and controlled Axum body probes.
pub(super) fn configuration(
    base: &str,
    key: &str,
    issuer_base: &str,
    configure: impl FnOnce(NativeGameRuntime) -> NativeGameRuntime,
    owner: Option<NativeHostOwner>,
) -> crate::api::ApiConfig {
    let native = configure(NativeGameRuntime::from_env(base, key).unwrap().unwrap());
    let free: crate::cosmetics::AppearanceSelection =
        serde_json::from_str(&std::env::var("SPELLWAKE_NATIVE_PRESENTATION_FREE").unwrap())
            .unwrap();
    free.validate().unwrap();
    assert!(free.playmat_id.is_none());
    let mut config = crate::api::ApiConfig::new(
        base.into(),
        std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap(),
        key.into(),
        format!("{}/auth/v1", issuer_base.trim_end_matches('/')),
        [free.back_id, free.sleeve_id, free.dice_id],
        vec![],
    )
    .unwrap()
    .with_native_views(native);
    if let Some(owner) = owner {
        config = config.with_native_host_owner(owner);
    }

    config
}

impl Drop for HttpProbe {
    fn drop(&mut self) {
        self.server.abort();
        // Observe cancellation before the runtime and listener are dropped.
        let _ = self.runtime.block_on(&mut self.server);
    }
}

/// Actual trusted pause/restore at one immutable native cut; no fake native input.
pub(super) fn takeover(repository: &NativeSourceRepository, game: Uuid, owner: &str) {
    let source = repository.read(game).unwrap().unwrap();
    let (old_owner, epoch, revision, _, _) = source.operational_basis();
    let at = source.clocks().map(|row| row.4).max().unwrap();
    let ObjectResponse(ApiResponse::Success {data:paused}):ObjectResponse<serde_json::Value>=repository.backend.rpc("pause_game_incident",&json!({
        "p_game":game,"p_owner":old_owner,"p_epoch":epoch,"p_operational_revision":revision,"p_incident_at":at,
        "p_kind":"storage","p_proof":"fixture:http-same-cut-owner-change","p_request":Uuid::new_v4(),
    })).unwrap() else {panic!("actual owner pause required");};
    let ObjectResponse(ApiResponse::Success {..}):ObjectResponse<serde_json::Value>=repository.backend.rpc("restore_game_checkpoint",&json!({
        "p_game":game,"p_epoch":paused["epoch"],"p_operational_revision":paused["operationalRevision"],
        "p_checkpoint":source.checkpoint_ids().0,"p_new_owner":owner,"p_restoration_ref":"fixture:http-owner-restored","p_request":Uuid::new_v4(),
    })).unwrap() else {panic!("actual owner restore required");};
    let current = repository.read(game).unwrap().unwrap();
    assert_eq!(current.accepted_revision(), source.accepted_revision());
    assert_eq!(current.checkpoint_ids(), source.checkpoint_ids());
    assert_eq!(current.last_event_sequence(), source.last_event_sequence());
    assert_eq!(current.operational_basis().0, owner);
    assert!(current.operational_basis().1 > epoch);
}
