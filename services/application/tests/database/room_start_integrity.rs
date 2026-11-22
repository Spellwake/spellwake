//! SQL countdown/start over source fixtures, not real native artifacts or game initialization.
use super::{TestResult, admission_integrity, connection, decks};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct Room {
    pub(super) source: admission_integrity::Admission,
    pub(super) id: Uuid,
    pub(super) other: Uuid,
    pub(super) other_revision: Uuid,
    pub(super) legality: Uuid,
}
fn resource(v: &Value) -> Uuid {
    assert_eq!(v["status"], "success");
    v["data"]["resourceId"].as_str().unwrap().parse().unwrap()
}
pub(super) async fn basis(c: &Client, r: Uuid) -> Result<String, tokio_postgres::Error> {
    c.query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&r])
        .await
        .map(|v| v.get(0))
}
pub(super) async fn room(c: &Client, humans: bool) -> FixtureResult<Room> {
    room_with_competition(c, humans, false).await
}
pub(super) async fn room_with_competition(
    c: &Client,
    humans: bool,
    competitive: bool,
) -> FixtureResult<Room> {
    room_with_capacity(c, humans, competitive, 2).await
}
/// Retain the ordinary lobby/deck producers while qualifying a larger roster.
pub(super) async fn room_with_capacity(
    c: &Client,
    humans: bool,
    competitive: bool,
    capacity: i64,
) -> FixtureResult<Room> {
    let f = admission_integrity::fixture_with_capacity(c, "collection", true, 0, capacity).await?;
    let meta = c
        .query_one(
            "SELECT format_id,legality_revision_id FROM app.match WHERE id=$1",
            &[&f.match_id],
        )
        .await?;
    let format: Uuid = meta.get(0);
    let legality = meta.get(1);
    let created: Value = c
        .query_one(
            "SELECT app.create_room($1,$2,$3,'collection',$5,'public',$4)",
            &[&f.actor, &format, &f.profile, &Uuid::new_v4(), &competitive],
        )
        .await?
        .get(0);
    let id = resource(&created);
    c.query_one(
        "SELECT app.select_room_deck($1,$2,0,$3,$4)",
        &[&f.actor, &id, &f.revision, &Uuid::new_v4()],
    )
    .await?;
    let (other, other_revision) = if humans {
        let (peer, _, deck, _, _) = decks::library(c).await?;
        c.execute(
            "UPDATE app.deck SET format_id=$1 WHERE id=$2",
            &[&format, &deck],
        )
        .await?;
        let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,NULL,$3,$4)", &[&peer,&deck,&json!([{"definitionId":f.definition,"printingId":f.printing,"section":"main","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
        let revision = resource(&saved);
        c.query_one(
            "SELECT app.join_room($1,$2,NULL,$3)",
            &[&peer, &id, &Uuid::new_v4()],
        )
        .await?;
        let current = basis(c, id).await?;
        c.query_one(
            "SELECT app.select_room_deck($1,$2,$3::text::numeric,$4,$5)",
            &[&peer, &id, &current, &revision, &Uuid::new_v4()],
        )
        .await?;
        (peer, revision)
    } else {
        let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,deck_revision_id,seat_index,controller_kind,ready) VALUES($1,$2,$3,1,'ai',false) RETURNING id", &[&id,&f.actor,&f.revision]).await?.get(0);
        let grant = bootstrap_ai(c, slot, f.actor, f.revision).await?;
        let confirmed:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck-qualified','fixture:authenticated-browser',(SELECT configuration_id FROM app.room WHERE id=$4),$3)", &[&grant,&f.revision,&Uuid::new_v4(),&id]).await?.get(0);
        assert_eq!(confirmed["status"], "success");
        (f.actor, f.revision)
    };
    Ok(Room {
        source: f,
        id,
        other,
        other_revision,
        legality,
    })
}
/// Normalized provider/delivery fixtures exercise SQL sources, without starting a real runner.
pub(super) async fn bootstrap_ai(
    c: &Client,
    slot: Uuid,
    sponsor: Uuid,
    revision: Uuid,
) -> FixtureResult<Uuid> {
    let room: Uuid = c
        .query_one("SELECT room_id FROM app.room_slot WHERE id=$1", &[&slot])
        .await?
        .get(0);
    let current = basis(c, room).await?;
    let boot: Value = c
        .query_one(
            "SELECT app.bootstrap_ai_slot($1,$2,$3::text::numeric,$4,$5)",
            &[&sponsor, &slot, &current, &revision, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let binding = resource(&boot);
    let start: Uuid = boot["data"]["startKey"].as_str().unwrap().parse()?;
    let work: Value = c
        .query_one(
            "SELECT coalesce(app.read_ai_start_work($1,1),'null'::jsonb)",
            &[&binding],
        )
        .await?
        .get(0);
    let work = spellwake_application::controller_ports::ReadStartWork {
        binding_id: binding,
        epoch: 1.into(),
    }
    .decode_response(work.to_string().as_bytes())?
    .expect("new bootstrap must expose its exact startup work");
    assert_eq!(work.start_id, start);
    let provider_ref = format!("fixture:process:{}", binding.simple());
    let provider_request = Uuid::new_v4();
    let observed: Value = c
        .query_one(
            "SELECT app.observe_ai_provider($1,1,$2,'running',$3,'fixture:provider-proof',$4)",
            &[&binding, &start, &provider_ref, &provider_request],
        )
        .await?
        .get(0);
    assert_eq!(observed["status"], "success", "{observed}");
    let no_work: Value = c
        .query_one(
            "SELECT coalesce(app.read_ai_start_work($1,1),'null'::jsonb)",
            &[&binding],
        )
        .await?
        .get(0);
    assert!(
        spellwake_application::controller_ports::ReadStartWork {
            binding_id: binding,
            epoch: 1.into()
        }
        .decode_response(no_work.to_string().as_bytes())?
        .is_none()
    );
    assert!(
        spellwake_application::controller_ports::ProviderObservation {
            binding_id: binding,
            epoch: 1.into(),
            start_id: start,
            outcome: spellwake_application::controller_ports::ProviderOutcome::Running,
            provider_ref: Some(provider_ref.parse()?),
            proof_ref: "fixture:provider-proof".parse()?,
            request_id: provider_request
        }
        .decode_response(observed.to_string().as_bytes())
        .is_ok()
    );
    let issue_request = Uuid::new_v4();
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,1,'fixture:private-secret-store-reference:'||$1::uuid::text,$2)",
            &[&binding, &issue_request],
        )
        .await?
        .get(0);
    let grant = resource(&issued);
    assert!(
        spellwake_application::controller_ports::IssueGrant {
            binding_id: binding,
            epoch: 1.into(),
            secret_ref: format!("fixture:private-secret-store-reference:{binding}").parse()?,
            request_id: issue_request
        }
        .decode_response(issued.to_string().as_bytes())
        .is_ok()
    );
    let delivery_request = Uuid::new_v4();
    let delivered: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:delivery-proof',$2)",
            &[&grant, &delivery_request],
        )
        .await?
        .get(0);
    assert_eq!(delivered["status"], "success", "{delivered}");
    assert!(
        spellwake_application::controller_ports::GrantDeliveryObservation {
            grant_id: grant,
            basis: 0.into(),
            outcome: spellwake_application::controller_ports::DeliveryOutcome::Delivered,
            proof_ref: "fixture:delivery-proof".parse()?,
            request_id: delivery_request
        }
        .decode_response(delivered.to_string().as_bytes())
        .is_ok()
    );
    Ok(grant)
}
/// Deliver and confirm the newly adopted grants. The old room grants are revoked.
pub(super) async fn confirm_adopted_ai(c: &Client, m: Uuid) -> FixtureResult<()> {
    let sources=c.query("SELECT g.id,b.binding_epoch::text,s.deck_revision_id,r.configuration_id,b.id FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE seat.match_id=$1 AND g.state='active' AND g.binding_epoch=b.binding_epoch AND g.delivery_state='pending'", &[&m]).await?;
    for row in sources {
        let grant: Uuid = row.get(0);
        let epoch: String = row.get(1);
        let revision: Uuid = row.get(2);
        let configuration: Uuid = row.get(3);
        let binding: Uuid = row.get(4);
        let delivery_request = Uuid::new_v4();
        let delivered:Value=c.query_one("SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:adopted-delivery',$2)", &[&grant,&delivery_request]).await?.get(0);
        assert_eq!(delivered["status"], "success", "{delivered}");
        assert!(
            spellwake_application::controller_ports::GrantDeliveryObservation {
                grant_id: grant,
                basis: 0.into(),
                outcome: spellwake_application::controller_ports::DeliveryOutcome::Delivered,
                proof_ref: "fixture:adopted-delivery".parse()?,
                request_id: delivery_request
            }
            .decode_response(delivered.to_string().as_bytes())
            .is_ok()
        );
        let ready_request = Uuid::new_v4();
        let confirmed:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,$2::text::numeric,$3,'fixture:deck-qualified','fixture:authenticated-browser',$4,$5)", &[&grant,&epoch,&revision,&configuration,&ready_request]).await?.get(0);
        assert_eq!(confirmed["status"], "success", "{confirmed}");
        assert!(
            spellwake_application::controller_ports::ReadyConfirmation {
                grant_id: grant,
                epoch: epoch.parse()?,
                revision_id: revision,
                qualification_ref: "fixture:deck-qualified".parse()?,
                authentication_ref: "fixture:authenticated-browser".parse()?,
                configuration_id: configuration,
                request_id: ready_request
            }
            .decode_response(confirmed.to_string().as_bytes(), binding)
            .is_ok()
        );
    }
    Ok(())
}
pub(super) async fn ready(
    c: &Client,
    f: &Room,
    actor: Uuid,
    revision: Uuid,
    value: bool,
) -> Result<Value, tokio_postgres::Error> {
    let current = basis(c, f.id).await?;
    let request = Uuid::new_v4();
    let response: Value = c
        .query_one(
            "SELECT app.set_room_ready($1,$2,$3::text::numeric,$4,$5,'fixture:validated',$6)",
            &[&actor, &f.id, &current, &value, &revision, &request],
        )
        .await
        .map(|v| v.get(0))?;
    spellwake_application::admission_ports::QualifiedReady {
        actor_id: actor,
        room_id: f.id,
        basis: current.parse().unwrap(),
        ready: value,
        selected_revision_id: Some(revision),
        qualification_ref: Some("fixture:validated".parse().unwrap()),
        request_id: request,
    }
    .decode_response(&serde_json::to_vec(&response).unwrap())
    .unwrap();
    Ok(response)
}
pub(super) async fn start(
    c: &Client,
    f: &Room,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let inputs:Vec<Value>=c.query("SELECT id,deck_revision_id FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL ORDER BY seat_index", &[&f.id]).await?.iter().map(|v|json!({"slotId":v.get::<_,Uuid>(0),"revisionId":v.get::<_,Uuid>(1),"snapshotRef":"fixture:accepted"})).collect();
    let current = basis(c, f.id).await?;
    let response: Value = c
        .query_one(
            "SELECT app.start_room_match($1,$2::text::numeric,$3,$4,'fixture:validated',$5,$6)",
            &[
                &f.id,
                &current,
                &f.source.release,
                &f.legality,
                &json!(inputs),
                &request,
            ],
        )
        .await
        .map(|v| v.get(0))?;
    let inputs = spellwake_application::admission_ports::RoomStartInputs::from_bytes(
        &serde_json::to_vec(&inputs).unwrap(),
    )
    .unwrap();
    spellwake_application::admission_ports::StartRoomMatch {
        room_id: f.id,
        basis: current.parse().unwrap(),
        release_id: f.source.release,
        legality_revision_id: f.legality,
        qualification_ref: "fixture:validated".parse().unwrap(),
        inputs,
        request_id: request,
    }
    .decode_response(&serde_json::to_vec(&response).unwrap())
    .unwrap();
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn every_ready_human_starts_the_full_room_countdown_and_unready_cancels_without_resetting_peers()
-> TestResult {
    let c = connection().await?;
    let f = room(&c, true).await?;
    assert_eq!(
        ready(&c, &f, f.source.actor, f.source.revision, true).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        "lobby"
    );
    ready(&c, &f, f.other, f.other_revision, true).await?;
    let row=c.query_one("SELECT state,countdown_at-clock_timestamp()>interval '9 seconds' FROM app.room WHERE id=$1", &[&f.id]).await?;
    assert_eq!(row.get::<_, String>(0), "countdown");
    assert!(row.get::<_, bool>(1));
    ready(&c, &f, f.other, f.other_revision, false).await?;
    assert!(
        c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.id, &f.source.actor]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        "lobby"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_can_start_with_valid_unready_humans_but_configuration_cancels_and_resets_every_human()
-> TestResult {
    let c = connection().await?;
    let f = room(&c, true).await?;
    let current = basis(&c, f.id).await?;
    let request = Uuid::new_v4();
    let forced: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:validated',$4)",
            &[&f.source.actor, &f.id, &current, &request],
        )
        .await?
        .get(0);
    assert_eq!(forced["status"], "success");
    let original = spellwake_application::admission_ports::QualifiedCountdown {
        actor_id: f.source.actor,
        room_id: f.id,
        basis: current.parse()?,
        qualification_ref: "fixture:validated".parse()?,
        request_id: request,
    };
    original.decode_response(&serde_json::to_vec(&forced)?)?;
    let replay: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:validated',$4)",
            &[&f.source.actor, &f.id, &current, &request],
        )
        .await?
        .get(0);
    assert_eq!(replay, forced);
    original.decode_response(&serde_json::to_vec(&replay)?)?;
    ready(&c, &f, f.source.actor, f.source.revision, true).await?;
    ready(&c, &f, f.other, f.other_revision, true).await?;
    let teams: Vec<Value> = c
        .query("SELECT id FROM app.room_slot WHERE room_id=$1", &[&f.id])
        .await?
        .iter()
        .map(|v| json!({"slotId":v.get::<_,Uuid>(0),"teamRef":null}))
        .collect();
    let current = basis(&c, f.id).await?;
    let configured: Value = c
        .query_one(
            "SELECT app.configure_room($1,$2,$3::text::numeric,$4,'bo3',$5,$6)",
            &[
                &f.source.actor,
                &f.id,
                &current,
                &f.source.profile,
                &json!(teams),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(configured["status"], "success");
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND controller_kind='human' AND ready", &[&f.id]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        "lobby"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_start_never_skips_the_countdown_then_reserves_one_match_game_and_handoff_atomically()
-> TestResult {
    let c = connection().await?;
    let f = room(&c, false).await?;
    ready(&c, &f, f.source.actor, f.source.revision, true).await?;
    assert_eq!(start(&c, &f, Uuid::new_v4()).await?["status"], "fail");
    // Owner-only deterministic clock fixture: distributed callers cannot write this field.
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let request = Uuid::new_v4();
    let accepted = start(&c, &f, request).await?;
    let matched = resource(&accepted);
    let row=c.query_one("SELECT m.state,m.reserved_game_id,m.started_at IS NOT NULL,(SELECT count(*) FROM app.match_seat WHERE match_id=m.id AND deck_accepted_at IS NOT NULL)::bigint,(SELECT count(*) FROM app.match_clock clock JOIN app.match_seat seat ON seat.id=clock.match_seat_id WHERE seat.match_id=m.id AND clock.remaining_ms=0 AND clock.paused)::bigint,(SELECT count(*) FROM app.durable_handoff WHERE match_id=m.id AND kind='ready_match')::bigint FROM app.match m WHERE m.id=$1", &[&matched]).await?;
    assert_eq!(row.get::<_, String>(0), "ready");
    assert!(row.get::<_, Option<Uuid>>(1).is_some());
    assert!(row.get::<_, bool>(2));
    assert_eq!(row.get::<_, i64>(3), 2);
    assert_eq!(row.get::<_, i64>(4), 2);
    assert_eq!(row.get::<_, i64>(5), 1);
    let after = start(&c, &f, Uuid::new_v4()).await?;
    assert_eq!(after["status"], "fail");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.match WHERE room_id=$1",
            &[&f.id]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}
