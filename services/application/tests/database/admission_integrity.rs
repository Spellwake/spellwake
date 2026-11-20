//! Source-qualified SQL admission fixtures. Qualification/artifact strings do not
//! establish native construction legality, executable card coverage or storage persistence.
use super::{TestResult, connection, decks, game_scope, operation, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct Admission {
    pub(super) actor: Uuid,
    pub(super) match_id: Uuid,
    pub(super) release: Uuid,
    pub(super) human: Uuid,
    pub(super) ai: Uuid,
    pub(super) revision: Uuid,
    pub(super) printing: Uuid,
    pub(super) definition: Uuid,
    pub(super) inventory: Uuid,
    pub(super) profile: Uuid,
}
pub(super) async fn fixture(
    c: &Client,
    mode: &str,
    basic: bool,
    owned: i64,
) -> FixtureResult<Admission> {
    fixture_with_capacity(c, mode, basic, owned, 2).await
}
/// Build the same qualified dictionaries with an explicit supported seat cap.
pub(super) async fn fixture_with_capacity(
    c: &Client,
    mode: &str,
    basic: bool,
    owned: i64,
    capacity: i64,
) -> FixtureResult<Admission> {
    let (actor, _, deck, definition, format) = decks::library(c).await?;
    c.execute("UPDATE app.deck SET mode=$1 WHERE id=$2", &[&mode, &deck])
        .await?;
    let set: Uuid = c
        .query_one(
            "INSERT INTO app.card_set(external_set_ref,name) VALUES($1,'Fixture') RETURNING id",
            &[&Uuid::new_v4().to_string()],
        )
        .await?
        .get(0);
    let printing:Uuid=c.query_one("INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,'common','nonfoil','fixture') RETURNING id", &[&definition,&set,&Uuid::new_v4().to_string()]).await?.get(0);
    let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,NULL,$3,$4)", &[&actor,&deck,&json!([{"definitionId":definition,"printingId":printing,"section":"main","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
    assert_eq!(saved["status"], "success");
    let revision = saved["data"]["resourceId"].as_str().unwrap().parse()?;
    let body:Uuid=c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,$2,'fixture') RETURNING id", &[&definition,&Uuid::new_v4().to_string()]).await?.get(0);
    let release:Uuid=c.query_one("INSERT INTO app.qualified_release(release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,'fixture','fixture','fixture','fixture') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land) VALUES($1,$2,'fixture',$3)", &[&release,&body,&basic]).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,$2,'fixture') RETURNING id", &[&Uuid::new_v4().to_string(),&capacity]).await?.get(0);
    let legality:Uuid=c.query_one("INSERT INTO app.legality_revision(source_ref,effective_at,imported_at) VALUES('fixture',clock_timestamp(),clock_timestamp()) RETURNING id", &[]).await?.get(0);
    c.execute("INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES($1,$2,$3,'legal')", &[&definition,&format,&legality]).await?;
    let match_id:Uuid=c.query_one("INSERT INTO app.match(format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES($1,$2,$3,$4,false,'bo1','admitting',0) RETURNING id", &[&format,&profile,&legality,&mode]).await?.get(0);
    let human:Uuid=c.query_one("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,0,'human') RETURNING id", &[&match_id,&actor]).await?.get(0);
    let ai:Uuid=c.query_one("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai') RETURNING id", &[&match_id,&actor]).await?.get(0);
    let asset: Uuid = c
        .query_one(
            "INSERT INTO app.asset_definition(kind,printing_id) VALUES('card',$1) RETURNING id",
            &[&printing],
        )
        .await?
        .get(0);
    let inventory: Uuid = c
        .query_one("SELECT app.ensure_inventory($1,$2)", &[&actor, &asset])
        .await?
        .get(0);
    if owned > 0 {
        let op = operation(c, actor).await?;
        c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',$3::bigint,'fixture',$1::uuid::text)", &[&op,&inventory,&owned]).await?;
        c.execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&op],
        )
        .await?;
    }
    Ok(Admission {
        actor,
        match_id,
        release,
        human,
        ai,
        revision,
        printing,
        definition,
        inventory,
        profile,
    })
}
fn inputs(f: &Admission) -> Value {
    json!([{"seatId":f.human,"revisionId":f.revision,"snapshotRef":"fixture:human-copy"},{"seatId":f.ai,"revisionId":f.revision,"snapshotRef":"fixture:ai-copy"}])
}
async fn accept(
    c: &Client,
    f: &Admission,
    body: &Value,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.accept_match_decks($1,0,$2,'fixture:validated',$3,$4)",
            &[&f.match_id, &f.release, &body, &request],
        )
        .await
        .map(|r| r.get(0))?;
    match spellwake_application::admission_ports::AdmissionInputs::from_bytes(
        &serde_json::to_vec(body).unwrap(),
    ) {
        Ok(inputs) => {
            spellwake_application::admission_ports::AcceptMatchDecks {
                match_id: f.match_id,
                basis: 0.into(),
                release_id: f.release,
                qualification_ref: "fixture:validated".parse().unwrap(),
                inputs,
                request_id: request,
            }
            .decode_response(&serde_json::to_vec(&response).unwrap())
            .unwrap();
        }
        Err(_) => assert_ne!(response["status"], "success"),
    }
    Ok(response)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn match_admission_copies_all_seats_once_and_library_inventory_changes_do_not_change_the_snapshot()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "collection", false, 2).await?;
    let request = Uuid::new_v4();
    let body = inputs(&f);
    let accepted = accept(&c, &f, &body, request).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(accept(&c, &f, &body, request).await?, accepted);
    let row=c.query_one("SELECT count(*)::bigint,min(deck_accepted_at)=max(deck_accepted_at) FROM app.match_seat WHERE match_id=$1 AND source_revision_id=$2 AND deck_snapshot_ref IS NOT NULL", &[&f.match_id,&f.revision]).await?;
    assert_eq!(row.get::<_, i64>(0), 2);
    assert!(row.get::<_, bool>(1));
    let op = operation(&c, f.actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',2,'fixture',$1::uuid::text)", &[&op,&f.inventory]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    assert_eq!(accept(&c, &f, &body, request).await?, accepted);
    assert_eq!(
        c.execute("DELETE FROM app.match_seat WHERE id=$1", &[&f.human])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match SET mode='open' WHERE id=$1",
            &[&f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let mut changed = body;
    changed[0]["snapshotRef"] = json!("changed");
    assert_eq!(
        accept(&c, &f, &changed, request).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn collection_admission_rejects_missing_or_held_copies_while_open_and_basic_permission_need_no_inventory()
-> TestResult {
    let c = connection().await?;
    for (mode, basic, owned, expected) in [
        ("collection", false, 1, "fail"),
        ("collection", true, 0, "success"),
        ("open", false, 0, "success"),
    ] {
        let f = fixture(&c, mode, basic, owned).await?;
        assert_eq!(
            accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["status"],
            expected
        );
    }
    let f = fixture(&c, "collection", false, 2).await?;
    let op = operation(&c, f.actor).await?;
    let hold:Uuid=c.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2,'held',$3) RETURNING id", &[&f.actor,&Uuid::new_v4().to_string(),&op]).await?.get(0);
    c.execute(
        "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
        &[&hold, &f.inventory],
    )
    .await?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["data"]["collection"],
        "not enough available copies"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admission_rejects_foreign_repeated_or_incomplete_seat_inputs_and_preserves_every_unaccepted_seat()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "collection", false, 2).await?;
    let other = game_scope::seed(&c).await?;
    for body in [
        json!([inputs(&f)[0]]),
        json!([inputs(&f)[0], inputs(&f)[0]]),
        json!([inputs(&f)[0],{"seatId":other.seat,"revisionId":f.revision,"snapshotRef":"fixture"}]),
    ] {
        assert_eq!(
            accept(&c, &f, &body, Uuid::new_v4()).await?["status"],
            "fail"
        );
    }
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.match_seat WHERE match_id=$1 AND deck_accepted_at IS NOT NULL", &[&f.match_id]).await?.get::<_,i64>(0),0);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.actor]).await?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["data"]["account"],
        "unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admission_waits_for_a_competing_inventory_debit_and_rechecks_availability() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "collection", false, 2).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let op = operation(&c, f.actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',1,'fixture',$1::uuid::text)", &[&op,&f.inventory]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    let task = tokio::spawn(async move { accept(&waiting, &f, &inputs(&f), Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??["data"]["collection"],
        "not enough available copies"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn qualified_profile_fields_are_all_present_and_admitted_release_dictionary_cannot_expand()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "collection", false, 2).await?;
    let row = c
        .query_one(
            "SELECT minimum_seats,seat_capacity FROM app.table_profile_revision WHERE id=$1",
            &[&f.profile],
        )
        .await?;
    assert_eq!(row.get::<_, i64>(0), 2);
    assert_eq!(row.get::<_, i64>(1), 2);
    assert_eq!(c.execute("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats) VALUES($1,'fixture','fixture',2)", &[&Uuid::new_v4().to_string()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["status"],
        "success"
    );
    let body:Uuid=c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,$2,'fixture') RETURNING id", &[&f.definition,&Uuid::new_v4().to_string()]).await?.get(0);
    assert_eq!(c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'fixture')", &[&f.release,&body]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.query_one("SELECT owned_quantity::text FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.id=$1 AND d.printing_id=$2", &[&f.inventory,&f.printing]).await?.get::<_,String>(0),"2");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admitted_match_release_is_the_only_release_allowed_for_its_game_context() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "collection", false, 2).await?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["status"],
        "success"
    );
    let game:Uuid=c.query_one("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,0,'active',0) RETURNING id", &[&f.match_id]).await?.get(0);
    let other:Uuid=c.query_one("INSERT INTO app.qualified_release(release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,'fixture','fixture','fixture','fixture') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    assert_eq!(c.execute("INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES($1,$2,$3,'fixture','fixture')", &[&game,&other,&f.profile]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.execute("INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES($1,$2,$3,'fixture','fixture')", &[&game,&f.release,&f.profile]).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match SET accepted_release_id=$1 WHERE id=$2",
            &[&other, &f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.execute("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,7,'fixture')", &[&Uuid::new_v4().to_string()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unsupported_ambiguous_and_banned_card_inputs_never_receive_an_accepted_snapshot()
-> TestResult {
    let c = connection().await?;
    for case in [
        "missing_program",
        "ambiguous_program",
        "missing_legality",
        "banned",
    ] {
        let mut f = fixture(&c, "collection", false, 2).await?;
        match case {
            "missing_program" => {
                f.release=c.query_one("INSERT INTO app.qualified_release(release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,'fixture','fixture','fixture','fixture') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
            }
            "ambiguous_program" => {
                let body:Uuid=c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,$2,'fixture') RETURNING id", &[&f.definition,&Uuid::new_v4().to_string()]).await?.get(0);
                c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'fixture')", &[&f.release,&body]).await?;
            }
            _ => {
                let revision:Uuid=c.query_one("INSERT INTO app.legality_revision(source_ref,effective_at,imported_at) VALUES('fixture',clock_timestamp(),clock_timestamp()) RETURNING id", &[]).await?.get(0);
                if case == "banned" {
                    c.execute("INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) SELECT $1,format_id,$2,'banned' FROM app.match WHERE id=$3", &[&f.definition,&revision,&f.match_id]).await?;
                }
                c.execute(
                    "UPDATE app.match SET legality_revision_id=$1 WHERE id=$2",
                    &[&revision, &f.match_id],
                )
                .await?;
            }
        }
        assert_eq!(
            accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["data"]["cards"],
            "unsupported or not legal in accepted revision",
            "{case}"
        );
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.match_seat WHERE match_id=$1 AND deck_accepted_at IS NOT NULL", &[&f.match_id]).await?.get::<_,i64>(0),0);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn mode_format_ownership_unsealed_and_split_section_quantities_are_checked_at_admission()
-> TestResult {
    let c = connection().await?;
    let mut f = fixture(&c, "collection", false, 2).await?;
    let row = c
        .query_one(
            "SELECT deck_id FROM app.deck_revision WHERE id=$1",
            &[&f.revision],
        )
        .await?;
    let deck: Uuid = row.get(0);
    c.execute("UPDATE app.deck SET mode='open' WHERE id=$1", &[&deck])
        .await?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["data"]["deck"],
        "unavailable or incompatible"
    );
    c.execute(
        "UPDATE app.deck SET mode='collection' WHERE id=$1",
        &[&deck],
    )
    .await?;
    let format: Uuid = c
        .query_one(
            "SELECT format_id FROM app.match WHERE id=$1",
            &[&f.match_id],
        )
        .await?
        .get(0);
    let (other, _, foreign, _, foreign_format) = decks::library(&c).await?;
    c.execute(
        "UPDATE app.deck SET format_id=$1 WHERE id=$2",
        &[&foreign_format, &deck],
    )
    .await?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["status"],
        "fail"
    );
    c.execute(
        "UPDATE app.deck SET format_id=$1 WHERE id=$2",
        &[&format, &deck],
    )
    .await?;
    c.execute(
        "UPDATE app.deck SET format_id=$1 WHERE id=$2",
        &[&format, &foreign],
    )
    .await?;
    let foreign_saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,NULL,$3,$4)", &[&other,&foreign,&json!([{"definitionId":f.definition,"printingId":f.printing,"section":"main","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
    let mut body = inputs(&f);
    body[0]["revisionId"] = foreign_saved["data"]["resourceId"].clone();
    assert_eq!(
        accept(&c, &f, &body, Uuid::new_v4()).await?["data"]["deck"],
        "unavailable or incompatible"
    );
    c.batch_execute("BEGIN").await?;
    let unsealed:Uuid=c.query_one("INSERT INTO app.deck_revision(deck_id,revision_number,created_at) VALUES($1,99,clock_timestamp()) RETURNING id", &[&deck]).await?.get(0);
    body = inputs(&f);
    body[0]["revisionId"] = json!(unsealed);
    assert_eq!(
        accept(&c, &f, &body, Uuid::new_v4()).await?["status"],
        "fail"
    );
    c.batch_execute("ROLLBACK").await?;
    let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,$3,$4,$5)", &[&f.actor,&deck,&f.revision,&json!([{"definitionId":f.definition,"printingId":f.printing,"section":"main","quantity":"1"},{"definitionId":f.definition,"printingId":f.printing,"section":"sideboard","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
    f.revision = saved["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        accept(&c, &f, &inputs(&f), Uuid::new_v4()).await?["data"]["collection"],
        "not enough available copies"
    );
    Ok(())
}
