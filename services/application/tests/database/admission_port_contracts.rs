//! Source-to-contract checks over normalized SQL fixtures, not native/provider qualification.
use super::{TestResult, connection};
use serde_json::{Value, json};
use spellwake_application::admission_ports::*;
use uuid::Uuid;
type ContractResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn ready_receipt(
    c: &tokio_postgres::Client,
    original: &QualifiedReady,
) -> ContractResult<Value> {
    let basis = original.basis.to_string();
    let qualification = original.qualification_ref.as_ref().map(|x| x.as_str());
    let value: Value = c
        .query_one(
            "SELECT app.set_room_ready($1,$2,$3::text::numeric,$4,$5,$6,$7)",
            &[
                &original.actor_id,
                &original.room_id,
                &basis,
                &original.ready,
                &original.selected_revision_id,
                &qualification,
                &original.request_id,
            ],
        )
        .await?
        .get(0);
    original.decode_response(&serde_json::to_vec(&value)?)?;
    let intent = spellwake_application::qualified_intents::RoomAdmissionIntent {
        request_id: original.request_id,
        expected_revision: original.basis.clone(),
    };
    let state = if value["status"] == "success" {
        "accepted"
    } else {
        "rejected"
    };
    let disposition = json!({"state":state,"response":value});
    let d = spellwake_application::qualified_intents::decode_intent(&serde_json::to_vec(
        &disposition,
    )?)?;
    assert!(
        spellwake_application::qualified_intents::valid_room_disposition(
            &intent,
            original.room_id,
            false,
            &d
        )
    );
    Ok(value)
}
async fn countdown_receipt(
    c: &tokio_postgres::Client,
    original: &QualifiedCountdown,
) -> ContractResult<Value> {
    let basis = original.basis.to_string();
    let value: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,$4,$5)",
            &[
                &original.actor_id,
                &original.room_id,
                &basis,
                &original.qualification_ref.as_str(),
                &original.request_id,
            ],
        )
        .await?
        .get(0);
    original.decode_response(&serde_json::to_vec(&value)?)?;
    let intent = spellwake_application::qualified_intents::RoomAdmissionIntent {
        request_id: original.request_id,
        expected_revision: original.basis.clone(),
    };
    let state = if value["status"] == "success" {
        "accepted"
    } else {
        "rejected"
    };
    let disposition = json!({"state":state,"response":value});
    let d = spellwake_application::qualified_intents::decode_intent(&serde_json::to_vec(
        &disposition,
    )?)?;
    assert!(
        spellwake_application::qualified_intents::valid_room_disposition(
            &intent,
            original.room_id,
            true,
            &d
        )
    );
    Ok(value)
}
async fn start_receipt(
    c: &tokio_postgres::Client,
    original: &StartRoomMatch,
) -> ContractResult<Value> {
    let basis = original.basis.to_string();
    let inputs: Value = serde_json::from_str(original.inputs.original_json())?;
    let value: Value = c
        .query_one(
            "SELECT app.start_room_match($1,$2::text::numeric,$3,$4,$5,$6,$7)",
            &[
                &original.room_id,
                &basis,
                &original.release_id,
                &original.legality_revision_id,
                &original.qualification_ref.as_str(),
                &inputs,
                &original.request_id,
            ],
        )
        .await?
        .get(0);
    original.decode_response(&serde_json::to_vec(&value)?)?;
    Ok(value)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retained_room_source_receipts_survive_ready_countdown_and_start_frontier_changes()
-> TestResult {
    use super::room_start_integrity::{basis, ready, room};
    let c = connection().await?;
    let f = room(&c, true).await?;
    let accepted_ready = QualifiedReady {
        actor_id: f.source.actor,
        room_id: f.id,
        basis: basis(&c, f.id).await?.parse()?,
        ready: true,
        selected_revision_id: Some(f.source.revision),
        qualification_ref: Some("fixture:validated".parse()?),
        request_id: Uuid::new_v4(),
    };
    let accepted = ready_receipt(&c, &accepted_ready).await?;
    assert_eq!(accepted["status"], "success");
    ready(&c, &f, f.other, f.other_revision, true).await?;
    assert_eq!(ready_receipt(&c, &accepted_ready).await?, accepted);
    let rejected_ready = QualifiedReady {
        request_id: Uuid::new_v4(),
        ..accepted_ready
    };
    let rejected = ready_receipt(&c, &rejected_ready).await?;
    assert_eq!(rejected["status"], "fail");
    ready(&c, &f, f.source.actor, f.source.revision, false).await?;
    assert_eq!(ready_receipt(&c, &rejected_ready).await?, rejected);

    let accepted_countdown = QualifiedCountdown {
        actor_id: f.source.actor,
        room_id: f.id,
        basis: basis(&c, f.id).await?.parse()?,
        qualification_ref: "fixture:validated".parse()?,
        request_id: Uuid::new_v4(),
    };
    let accepted = countdown_receipt(&c, &accepted_countdown).await?;
    assert_eq!(accepted["status"], "success");
    ready(&c, &f, f.other, f.other_revision, false).await?;
    assert_eq!(countdown_receipt(&c, &accepted_countdown).await?, accepted);
    let rejected_countdown = QualifiedCountdown {
        request_id: Uuid::new_v4(),
        ..accepted_countdown
    };
    let rejected = countdown_receipt(&c, &rejected_countdown).await?;
    assert_eq!(rejected["status"], "fail");
    ready(&c, &f, f.source.actor, f.source.revision, true).await?;
    assert_eq!(countdown_receipt(&c, &rejected_countdown).await?, rejected);

    let inputs:Vec<Value>=c.query("SELECT id,deck_revision_id FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL ORDER BY seat_index",&[&f.id]).await?.iter().map(|r|json!({"slotId":r.get::<_,Uuid>(0),"revisionId":r.get::<_,Uuid>(1),"snapshotRef":"fixture:accepted"})).collect();
    let rejected_start = StartRoomMatch {
        room_id: f.id,
        basis: basis(&c, f.id).await?.parse()?,
        release_id: f.source.release,
        legality_revision_id: f.legality,
        qualification_ref: "fixture:validated".parse()?,
        inputs: RoomStartInputs::from_bytes(&serde_json::to_vec(&inputs)?)?,
        request_id: Uuid::new_v4(),
    };
    let rejected = start_receipt(&c, &rejected_start).await?;
    assert_eq!(rejected["status"], "fail");
    let force = QualifiedCountdown {
        actor_id: f.source.actor,
        room_id: f.id,
        basis: basis(&c, f.id).await?.parse()?,
        qualification_ref: "fixture:validated".parse()?,
        request_id: Uuid::new_v4(),
    };
    assert_eq!(countdown_receipt(&c, &force).await?["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    assert_eq!(start_receipt(&c, &rejected_start).await?, rejected);
    let accepted_start = StartRoomMatch {
        basis: basis(&c, f.id).await?.parse()?,
        request_id: Uuid::new_v4(),
        ..rejected_start
    };
    let accepted = start_receipt(&c, &accepted_start).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(start_receipt(&c, &accepted_start).await?, accepted);
    Ok(())
}

pub(super) fn candidate_response(
    game: Uuid,
    owner: &str,
    epoch: &str,
    basis: &str,
    request: Uuid,
    body: &Value,
    response: &Value,
) {
    match HostCandidate::from_bytes(&serde_json::to_vec(body).unwrap()) {
        Ok(candidate) => {
            let original = AcceptHostCandidate {
                game_id: game,
                owner: owner.parse().unwrap(),
                epoch: epoch.parse().unwrap(),
                basis: basis.parse().unwrap(),
                request_id: request,
                candidate,
            };
            original
                .decode_response(&serde_json::to_vec(response).unwrap())
                .unwrap();
        }
        // Low-level malformed-source tests intentionally bypass the normalized
        // port. None may turn into a successful accepted native frame.
        Err(_) => assert_ne!(response["status"], "success"),
    }
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_source_jsonb_budget_matches_postgres_strings_lists_objects_and_bounds()
-> TestResult {
    let c = connection().await?;
    let seat = Uuid::new_v4();
    let context = Uuid::new_v4();
    let opening = json!({"contextId":context,"snapshotRef":"fixture:snapshot","contextRevision":"v1","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:event","qualificationRef":"fixture:qualification"});
    let mut nested = json!({"contextId":context,"stateRef":"🂡\u{0001}\n\\\"é","factsRef":"fixture:facts","events":["fixture:event"],"facts":[{"eventIndex":"0","accountId":Uuid::new_v4(),"kind":"fixture","revision":"v1","value":"0"}],"clocks":[{"seatId":seat,"clockRevision":"0","remainingMs":"0","paused":true}],"participation":[],"terminal":null,"bankTimeout":null,"nativeEliminations":[]});
    for paused in [false, true] {
        nested["clocks"][0]["paused"] = json!(paused);
        let bytes = serde_json::to_vec(&nested)?;
        let frame = HostCandidate::from_bytes(&bytes)?;
        let actual: i32 = c
            .query_one("SELECT octet_length($1::jsonb::text)", &[&nested])
            .await?
            .get(0);
        assert_eq!(
            frame.original_terms_octets(),
            Some(usize::try_from(actual)?)
        );
        assert_eq!(frame.original_json().as_bytes(), bytes);
    }
    fn octets(
        kind: &str,
        bytes: &[u8],
    ) -> Result<Option<usize>, spellwake_application::source_protocol::SourceResponseError> {
        match kind {
            "opening" => OpeningArtifacts::from_bytes(bytes).map(|x| x.original_terms_octets()),
            "queue" => QueueSelection::from_bytes(bytes).map(|x| x.original_terms_octets()),
            "admission" => AdmissionInputs::from_bytes(bytes).map(|x| x.original_terms_octets()),
            "room_start" => RoomStartInputs::from_bytes(bytes).map(|x| x.original_terms_octets()),
            "candidate" => HostCandidate::from_bytes(bytes).map(|x| x.original_terms_octets()),
            _ => panic!("unexpected source family"),
        }
    }
    let queue_id = Uuid::new_v4();
    for (kind, value, limit, field) in [
        ("opening", opening, 16_384, "stateRef"),
        (
            "queue",
            json!({"profileId":Uuid::new_v4(),"configurationKey":"fixture:duel","hostEntryId":queue_id,"humanSeats":[{"queueEntryId":queue_id,"seatIndex":"0"}],"aiSeats":[]}),
            262_144,
            "configurationKey",
        ),
        (
            "admission",
            json!([{"seatId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:snapshot"}]),
            1_048_576,
            "snapshotRef",
        ),
        (
            "room_start",
            json!([{"slotId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:snapshot"}]),
            1_048_576,
            "snapshotRef",
        ),
        ("candidate", nested, 1_048_576, "stateRef"),
    ] {
        let base = octets(kind, &serde_json::to_vec(&value)?)?.unwrap();
        for desired in [limit, limit + 1] {
            let mut terms = value.clone();
            let record = if terms.is_array() {
                &mut terms[0]
            } else {
                &mut terms
            };
            record[field] = json!(format!(
                "{}{}",
                " ".repeat(desired - base),
                record[field].as_str().unwrap()
            ));
            let actual: i32 = c
                .query_one("SELECT octet_length($1::jsonb::text)", &[&terms])
                .await?
                .get(0);
            assert_eq!(usize::try_from(actual)?, desired, "{kind}");
            let parsed = octets(kind, &serde_json::to_vec(&terms)?);
            if desired == limit {
                assert_eq!(parsed?, Some(limit), "{kind}");
            } else {
                assert!(parsed.is_err(), "{kind}");
            }
        }
    }
    Ok(())
}
