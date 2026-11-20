//! Original receipt recovery after both seats select and the BO3 match advances.
use super::lifecycle_reads::sql;
use super::*;
use serde_json::Value;

pub(super) fn run(reservation: &Reservation) {
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let http = http::HttpProbe::owned(
        &base,
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
        "fixture:source-reader",
    );
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap(),
            &format!("{}/auth/v1", base.trim_end_matches('/')),
        )
        .unwrap(),
    )
    .unwrap();
    let tokens = [
        std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap(),
        std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap(),
    ];
    http.concede(&tokens[0], reservation.game_id, reservation.context_id);
    let result = sql(
        &database,
        "SELECT app.consume_game_outcome((SELECT id FROM app.game_result WHERE game_id=$1),(SELECT revision FROM app.match WHERE id=$2))",
        &[&reservation.game_id, &reservation.match_id],
    );
    assert_eq!(result["status"], "success");
    let path = "/v1/between-game/replies/disposition";
    let mut retained = Vec::new();
    let mixed = std::env::var("SPELLWAKE_NATIVE_CONTROLLER_BETWEEN").is_ok_and(|v| v == "1");
    let humans = if mixed { &tokens[..1] } else { &tokens[..] };
    for (index, token) in humans.iter().enumerate() {
        let actor = auth.resolve(token).unwrap();
        let terms = sql(
            &database,
            "SELECT jsonb_build_object('decisionId',d.id,'selectedRevisionId',s.source_revision_id,'expectedRevision',m.revision::text) FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat s ON s.id=d.seat_id WHERE m.id=$1 AND s.account_id=$2 AND d.state='pending'",
            &[&reservation.match_id, &actor.account().unwrap()],
        );
        let mut intent = terms;
        intent["requestId"] = json!(Uuid::new_v4());
        assert_eq!(
            http.admission_receipt(token, path, &intent, 200)["data"],
            json!({"state":"unknown","response":null})
        );
        assert_eq!(
            http.admission_receipt(&tokens[1 - index], path, &intent, 409)["data"],
            json!({"actor":"unavailable"})
        );
        let response = sql(
            &database,
            "WITH source AS (SELECT app.read_native_between_source(($3::jsonb->>'decisionId')::uuid,($3->>'expectedRevision')::numeric,($3->>'selectedRevisionId')::uuid) AS body) SELECT app.qualify_between_game_command($1,$2,$3,jsonb_build_object('version','native-between-prepared-v1','sourceHash',body->'data'->>'sourceHash','replyRef','fixture:original-reply','qualificationRef','fixture:sql-only')) FROM source",
            &[&actor.subject(), &actor.session(), &intent],
        );
        assert_eq!(response["status"], "success");
        let mut projected = response;
        if index == 0 {
            assert_eq!(projected["data"]["reservedGameId"], "");
            projected["data"]["reservedGameId"] = Value::Null;
        } else {
            let reserved =
                Uuid::parse_str(projected["data"]["reservedGameId"].as_str().unwrap()).unwrap();
            assert_eq!(
                sql(
                    &database,
                    "SELECT to_jsonb(reserved_game_id) FROM app.match WHERE id=$1",
                    &[&reservation.match_id]
                ),
                json!(reserved)
            );
        }
        retained.push((intent, projected));
    }
    if mixed {
        controller_reply(reservation, &database, &tokens[0], &http);
    }
    for (token, (mut intent, projected)) in humans.iter().zip(retained) {
        let read = http.admission_receipt(token, path, &intent, 200);
        assert_eq!(
            read["data"],
            json!({"state":"accepted","response":projected})
        );
        intent["expectedRevision"] = json!("987654321");
        assert_eq!(
            http.admission_receipt(token, path, &intent, 409)["data"],
            json!({"request":"conflict"})
        );
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"between-receipts","historicalOwnReceipts":humans.len(),"possessedControllerReceipt":mixed,"foreignActorRejected":true,"reservedNextGame":true})
    );
}

fn controller_reply(r: &Reservation, database: &str, sponsor: &str, http: &http::HttpProbe) {
    use crate::{
        commands::CommandDisposition, controller_possession::ControllerSecretStore,
        qualified_intents::BetweenGameIntent,
    };
    let terms = sql(
        database,
        "SELECT jsonb_build_object('requestId',gen_random_uuid(),'decisionId',d.id,'expectedRevision',m.revision::text,'selectedRevisionId',s.source_revision_id) FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat s ON s.id=d.seat_id WHERE m.id=$1 AND s.id=$2 AND d.state='pending'",
        &[&r.match_id, &r.seat_ids[1]],
    );
    let original: BetweenGameIntent = serde_json::from_value(terms.clone()).unwrap();
    let meta = super::controller::metadata(database, r);
    let store = ControllerSecretStore::default();
    let (_, proof) = super::controller::install(&store, &meta);
    let repository = NativeSourceRepository::from_env().unwrap();
    let read = |original| {
        repository
            .controller_between_disposition(&store, &proof, original)
            .unwrap()
    };
    assert!(matches!(
        read(&original),
        ApiResponse::Success {
            data: CommandDisposition::Unknown
        }
    ));
    let path = "/v1/between-game/replies/disposition";
    assert_eq!(
        http.admission_receipt(sponsor, path, &terms, 409)["data"],
        json!({"actor":"unavailable"})
    );
    let result = sql(
        database,
        "WITH source AS (SELECT app.read_native_between_source(($4::jsonb->>'decisionId')::uuid,($4->>'expectedRevision')::numeric,($4->>'selectedRevisionId')::uuid) AS body) SELECT app.qualify_controller_between_game_command($1,$2::text::numeric,$3,$4,jsonb_build_object('version','native-between-prepared-v1','sourceHash',body->'data'->>'sourceHash','replyRef','fixture:controller-reply','qualificationRef','fixture:sql-only')) FROM source",
        &[
            &proof.grant(),
            &proof.epoch().to_string(),
            &proof.reference(),
            &terms,
        ],
    );
    assert_eq!(result["status"], "success");
    let next: Uuid = result["data"]["reservedGameId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        sql(
            database,
            "SELECT to_jsonb(reserved_game_id) FROM app.match WHERE id=$1",
            &[&r.match_id]
        ),
        json!(next)
    );
    assert!(
        matches!(read(&original),ApiResponse::Success{data:CommandDisposition::Accepted{ack}} if ack.reserved_game_id==Some(next) && ack.resource_id==original.decision_id)
    );
    let mut changed = original.clone();
    changed.selected_revision_id = Uuid::new_v4();
    assert!(
        matches!(read(&changed),ApiResponse::Fail{data} if data.get("request").is_some_and(|v|v=="conflict"))
    );
    assert!(store.revoke(proof.grant()).unwrap());
    assert!(
        matches!(read(&original),ApiResponse::Fail{data} if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
}

/// Exercise the actual private consumer in an owned realm with the sealed mixed-roster release.
#[test]
#[ignore = "requires an isolated authenticated BO3 controller decision and a scoped admission release"]
fn possessed_controller_submits_its_qualified_between_game_deck() {
    use crate::{
        commands::CommandDisposition, controller_possession::ControllerSecretStore,
        qualified_intents::BetweenGameIntent,
    };
    let input: Value = serde_json::from_slice(
        &std::fs::read(std::env::var_os("SPELLWAKE_CONTROLLER_REPLY_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    let original: BetweenGameIntent = serde_json::from_value(input["original"].clone()).unwrap();
    let repository = NativeSourceRepository::from_env().unwrap();
    let store = ControllerSecretStore::default();
    let (_, proof) = super::controller::install(&store, &input["grant"]);
    let read = |name| std::fs::read(std::env::var_os(name).unwrap()).unwrap();
    let programs = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        &read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
    .unwrap();
    assert!(matches!(
        repository
            .controller_between_disposition(&store, &proof, &original)
            .unwrap(),
        ApiResponse::Success {
            data: CommandDisposition::Unknown
        }
    ));
    let submit = || {
        repository
            .execute_controller_between(&store, &proof, &original, &programs)
            .unwrap()
    };
    let ApiResponse::Success { data: ack } = submit() else {
        panic!("Controller could not submit its selected deck");
    };
    assert_eq!(ack.resource_id, original.decision_id);
    let game = ack
        .reserved_game_id
        .expect("The second reply reserves game two");
    assert!(
        matches!(submit(),ApiResponse::Success{data} if data.reserved_game_id==Some(game) && data.revision==ack.revision)
    );
    assert!(store.revoke(proof.grant()).unwrap());
    assert!(
        matches!(submit(),ApiResponse::Fail{data} if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
    println!("VERIFIED_CONTROLLER_BETWEEN_GAME {game}");
}
