//! Actual local possession/RPC/Storage/native consumers; provider observations remain fixtures.
use super::*;
use crate::player_wire::{
    PlayerHandle, PlayerObjectIdentity, PlayerSnapshot, PlayerZone, PlayerZoneKind,
};
use crate::{
    controller_possession::{ControllerCredential, ControllerSecretStore, PossessedController},
    player_wire::{PlayerAction, PlayerViewRequest},
};
use serde_json::Value;
use spellwake_contracts::RulesInteger;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
const SUBMIT: &str = "/rest/v1/rpc/submit_native_controller_command";
const RECEIPTS: &str = "/rest/v1/rpc/read_native_controller_receipts";
struct LostAck {
    accepted: Mutex<Option<Value>>,
    lost: AtomicUsize,
    submit: AtomicUsize,
    no_native: AtomicBool,
    forbidden: AtomicUsize,
}
impl fault_proxy::Fault for LostAck {
    fn request(&self, _: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if path == SUBMIT {
            self.submit.fetch_add(1, Ordering::SeqCst);
        }
        if self.no_native.load(Ordering::SeqCst) && path != RECEIPTS {
            self.forbidden.fetch_add(1, Ordering::SeqCst);
            return Some(503);
        }
        None
    }
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if path == SUBMIT {
            assert_eq!(status, 200);
            let value: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            let mut saved = self.accepted.lock().unwrap();
            assert!(saved.is_none());
            *saved = Some(value);
        }
    }
    fn delivery(
        &self,
        _: &reqwest::Method,
        path: &str,
        _: u16,
        _: &[u8],
    ) -> fault_proxy::ResponseDelivery {
        if path == SUBMIT {
            assert!(self.accepted.lock().unwrap().is_some());
            assert_eq!(self.lost.fetch_add(1, Ordering::SeqCst), 0);
            fault_proxy::ResponseDelivery::Truncate
        } else {
            fault_proxy::ResponseDelivery::Forward
        }
    }
}
struct RevokeOnRecovery {
    secrets: Arc<ControllerSecretStore>,
    grant: Uuid,
    tripped: AtomicBool,
}
impl fault_proxy::Fault for RevokeOnRecovery {
    fn request(&self, _: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if path == "/rest/v1/rpc/read_native_game_source" {
            assert!(!self.tripped.swap(true, Ordering::SeqCst));
            self.secrets.revoke(self.grant).unwrap();
            Some(503)
        } else {
            None
        }
    }
    fn response(&self, _: &reqwest::Method, _: &str, _: u16, _: &mut Vec<u8>) {}
}
fn metadata(database: &str, r: &Reservation) -> Value {
    lifecycle_reads::sql(
        database,
        "SELECT jsonb_build_object('grant',g.id,'binding',b.id,'epoch',b.binding_epoch::text,'reference',g.grant_secret_ref,'revision',s.deck_revision_id,'configuration',room.configuration_id) FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room room ON room.id=s.room_id WHERE b.match_seat_id=$1 AND g.state='active' AND g.binding_epoch=b.binding_epoch",
        &[&r.seat_ids[1]],
    )
}
pub(super) fn install(
    store: &ControllerSecretStore,
    m: &Value,
) -> (ControllerCredential, PossessedController) {
    let grant: Uuid = m["grant"].as_str().unwrap().parse().unwrap();
    let binding = m["binding"].as_str().unwrap().parse().unwrap();
    let epoch: Amount = m["epoch"].as_str().unwrap().parse().unwrap();
    let credential = store
        .install(
            grant,
            binding,
            epoch,
            m["reference"].as_str().unwrap().parse().unwrap(),
        )
        .unwrap();
    let parsed = ControllerCredential::parse(&credential.expose_for_delivery()).unwrap();
    let proof = store.authenticate(grant, &parsed).unwrap().unwrap();
    (credential, proof)
}
fn rotate(database: &str, r: &Reservation, old: &Value) -> Value {
    let grant: Uuid = old["grant"].as_str().unwrap().parse().unwrap();
    let binding: Uuid = old["binding"].as_str().unwrap().parse().unwrap();
    let epoch = old["epoch"].as_str().unwrap().to_owned();
    let request = Uuid::new_v4();
    let reference = format!("fixture:controller-generation:{}", Uuid::new_v4());
    let _ = lifecycle_reads::sql(
        database,
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1 RETURNING to_jsonb(id)",
        &[&grant],
    );
    let result = lifecycle_reads::sql(
        database,
        "SELECT app.issue_ai_grant($1,$2::text::numeric,$3,$4)",
        &[&binding, &epoch, &reference, &request],
    );
    assert_eq!(result["status"], "success");
    let new_grant: Uuid = result["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let delivered = lifecycle_reads::sql(
        database,
        "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:controller-delivery',$2)",
        &[&new_grant, &Uuid::new_v4()],
    );
    assert_eq!(delivered["status"], "success");
    let revision: Uuid = old["revision"].as_str().unwrap().parse().unwrap();
    let configuration: Uuid = old["configuration"].as_str().unwrap().parse().unwrap();
    let ready = lifecycle_reads::sql(
        database,
        "SELECT app.commit_ai_ready_confirmation($1,$2::text::numeric,$3,'fixture:deck-qualified','fixture:controller-authentication',$4,$5)",
        &[
            &new_grant,
            &epoch,
            &revision,
            &configuration,
            &Uuid::new_v4(),
        ],
    );
    assert_eq!(ready["status"], "success");
    metadata(database, r)
}
fn assert_private_hand(
    snapshot: &PlayerSnapshot,
    viewer: Uuid,
    peer: Uuid,
    r: &Reservation,
) -> PlayerHandle {
    assert_eq!(snapshot.viewer_seat_id, viewer);
    let own = snapshot
        .zones
        .iter()
        .find(|z| z.zone_type() == PlayerZoneKind::Hand && z.owner_seat_id() == Some(viewer))
        .unwrap();
    assert!(matches!(own,PlayerZone::Visible{objects,..}if objects.len()==7));
    let other = snapshot
        .zones
        .iter()
        .find(|z| z.zone_type() == PlayerZoneKind::Hand && z.owner_seat_id() == Some(peer))
        .unwrap();
    assert!(
        matches!(other,PlayerZone::Hidden{hidden_count,..}if hidden_count.as_rules_integer()==&RulesInteger::from(7))
    );
    assert!(other.objects().is_empty());
    for seat in [viewer, peer] {
        let library = snapshot
            .zones
            .iter()
            .find(|z| z.zone_type() == PlayerZoneKind::Library && z.owner_seat_id() == Some(seat))
            .unwrap();
        assert!(
            matches!(library,PlayerZone::Hidden{hidden_count,..}if hidden_count.as_rules_integer()==&RulesInteger::from(13))
        );
        assert!(library.objects().is_empty());
        assert!(!library.order_known());
    }
    assert_eq!(snapshot.visible_objects.len(), 7);
    assert!(snapshot.visible_objects.iter().all(|o|o.owner_seat_id==viewer&&o.zone==*own.handle()&&matches!(&o.identity,PlayerObjectIdentity::Known{definition_id:Some(d),printing_id:Some(p),..}if *d==r.definition_id&&*p==r.printing_id)));
    own.objects()[0].clone()
}
fn authority_denials(
    repository: &NativeSourceRepository,
    database: &str,
    r: &Reservation,
    meta: &Value,
) -> Vec<String> {
    let epoch: Amount = meta["epoch"].as_str().unwrap().parse().unwrap();
    let mut unknown = meta.clone();
    unknown["grant"] = json!(Uuid::new_v4());
    let mut wrong_epoch = meta.clone();
    wrong_epoch["epoch"] = json!(epoch.checked_add(&Amount::from(1)).unwrap());
    let mut tokens = Vec::new();
    for invalid_meta in [&unknown, &wrong_epoch] {
        let store = ControllerSecretStore::default();
        let (token, proof) = install(&store, invalid_meta);
        assert!(store.current(&proof).unwrap());
        assert!(matches!(
            repository
                .controller_access(&store, &proof, r.game_id, r.context_id)
                .unwrap(),
            NativeControllerAccessOutcome::Unavailable
        ));
        tokens.push(token.expose_for_delivery());
    }
    let mut wrong_binding = meta.clone();
    wrong_binding["binding"] = json!(Uuid::new_v4());
    let store = ControllerSecretStore::default();
    let (token, proof) = install(&store, &wrong_binding);
    assert!(store.current(&proof).unwrap());
    assert!(
        repository
            .controller_access(&store, &proof, r.game_id, r.context_id)
            .is_err()
    );
    tokens.push(token.expose_for_delivery());
    let store = ControllerSecretStore::default();
    let (token, proof) = install(&store, meta);
    assert!(matches!(
        repository
            .controller_access(&store, &proof, r.game_id, Uuid::new_v4())
            .unwrap(),
        NativeControllerAccessOutcome::Unavailable
    ));
    tokens.push(token.expose_for_delivery());
    let foreign_match: Uuid = std::env::var("SPELLWAKE_NATIVE_FOREIGN_CONTROLLER_MATCH")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(foreign_match, r.match_id);
    let foreign = lifecycle_reads::sql(
        database,
        "SELECT jsonb_build_object('grant',g.id,'binding',b.id,'epoch',b.binding_epoch::text,'reference',g.grant_secret_ref) FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE seat.match_id=$1 AND g.state='active' AND g.binding_epoch=b.binding_epoch",
        &[&foreign_match],
    );
    let foreign_store = ControllerSecretStore::default();
    let (token, proof) = install(&foreign_store, &foreign);
    assert!(foreign_store.current(&proof).unwrap());
    let grant: Uuid = foreign["grant"].as_str().unwrap().parse().unwrap();
    let epoch = foreign["epoch"].as_str().unwrap().to_owned();
    let reference = foreign["reference"].as_str().unwrap().to_owned();
    let actor = lifecycle_reads::sql(
        database,
        "SELECT app.read_native_controller_actor($1,$2::text::numeric,$3,$4)",
        &[&grant, &epoch, &reference, &foreign_match],
    );
    assert_eq!(actor["kind"], "ai");
    assert!(matches!(
        repository
            .controller_access(&foreign_store, &proof, r.game_id, r.context_id)
            .unwrap(),
        NativeControllerAccessOutcome::Unavailable
    ));
    tokens.push(token.expose_for_delivery());
    tokens
}
fn superseded_generation(
    repository: &NativeSourceRepository,
    runtime: &NativeGameRuntime,
    database: &str,
    r: &Reservation,
    meta: &Value,
    original: &PlayerCommand,
    owner: &NativeHostOwner,
) -> String {
    // Expiry is canonical generation supersession. No wall-clock TTL is declared.
    let store = ControllerSecretStore::default();
    let (token, proof) = install(&store, meta);
    assert!(matches!(
        repository
            .controller_access(&store, &proof, r.game_id, r.context_id)
            .unwrap(),
        NativeControllerAccessOutcome::Granted(_)
    ));
    let binding: Uuid = meta["binding"].as_str().unwrap().parse().unwrap();
    let epoch = meta["epoch"].as_str().unwrap().to_owned();
    let command = crate::controller_ports::RebindRunner {
        binding_id: binding,
        basis: epoch.parse().unwrap(),
        proof_ref: "fixture:observed-generation-replacement".parse().unwrap(),
        request_id: Uuid::new_v4(),
    };
    let response = lifecycle_reads::sql(
        database,
        "SELECT app.rebind_ai_runner($1,$2::text::numeric,$3,$4)",
        &[
            &binding,
            &epoch,
            &command.proof_ref.as_str(),
            &command.request_id,
        ],
    );
    assert!(matches!(
        command
            .decode_response(&serde_json::to_vec(&response).unwrap())
            .unwrap(),
        ApiResponse::Success { .. }
    ));
    let grant: Uuid = meta["grant"].as_str().unwrap().parse().unwrap();
    let persisted = lifecycle_reads::sql(
        database,
        "SELECT jsonb_build_object('epoch',b.binding_epoch::text,'grantState',g.state) FROM app.ai_runner_binding b JOIN app.controller_grant g ON g.binding_id=b.id WHERE b.id=$1 AND g.id=$2",
        &[&binding, &grant],
    );
    assert_eq!(
        persisted["epoch"],
        json!(command.basis.checked_add(&Amount::from(1)).unwrap())
    );
    assert_eq!(persisted["grantState"], "revoked");
    assert!(store.current(&proof).unwrap());
    assert!(store.authenticate(proof.grant(), &token).unwrap().is_some());
    assert!(matches!(
        repository
            .controller_access(&store, &proof, r.game_id, r.context_id)
            .unwrap(),
        NativeControllerAccessOutcome::Unavailable
    ));
    assert!(
        matches!(repository.controller_original(&store, &proof, original).unwrap(),Some(ApiResponse::Fail{data})if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
    let fingerprint = || {
        lifecycle_reads::sql(
            database,
            "SELECT jsonb_build_object('revision',accepted_revision::text,'originals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND target_ref=($1::uuid)::text)) FROM app.game WHERE id=$1::uuid",
            &[&r.game_id],
        )
    };
    let before = fingerprint();
    let mut fresh = original.clone();
    fresh.request_id = Uuid::new_v4();
    assert!(
        matches!(runtime.controller_command(repository,&store,&proof,owner,&fresh).unwrap(),ApiResponse::Fail{data}if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
    assert_eq!(fingerprint(), before);
    token.expose_for_delivery()
}
pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    assert_eq!(
        r.structure,
        crate::rooms::configuration::MatchStructure::Bo3
    );
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let runtime = NativeGameRuntime::from_env(&base, &key).unwrap().unwrap();
    let selected = repository
        .recover_with_history(
            r.game_id,
            store,
            programs,
            NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    super::selected_origin(
        selected.checkpoint().provenance(),
        selected.checkpoint().state(),
        r,
    );
    assert_eq!(
        selected.history_evidence(),
        NativeHistoryEvidence::TransitionV1
    );
    let secrets = Arc::new(ControllerSecretStore::default());
    let meta = metadata(&database, r);
    let (token, proof) = install(&secrets, &meta);
    let mut extra_tokens = authority_denials(&repository, &database, r, &meta);
    let request = PlayerViewRequest {
        game_id: r.game_id,
        context_id: r.context_id,
    };
    let NativeViewOutcome::Snapshot(snapshot) = runtime
        .controller_view(&repository, &secrets, &proof, &request)
        .unwrap()
    else {
        panic!("actual AI live snapshot required")
    };
    assert_eq!(snapshot.viewer_seat_id, r.seat_ids[1]);
    let _ai_handle = assert_private_hand(&snapshot, r.seat_ids[1], r.seat_ids[0], r);
    let human = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    let human_token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let human_wire = human.read(Some(&human_token), r.game_id, r.context_id, 200);
    let human_snapshot: PlayerSnapshot =
        serde_json::from_value(human_wire["data"]["snapshot"].clone()).unwrap();
    let foreign_handle = assert_private_hand(&human_snapshot, r.seat_ids[0], r.seat_ids[1], r);
    assert!(
        !snapshot
            .visible_objects
            .iter()
            .any(|o| o.handle == foreign_handle)
    );
    let actor_shape = repository
        .controller_access(&secrets, &proof, r.game_id, r.context_id)
        .unwrap();
    let NativeControllerAccessOutcome::Granted(access) = actor_shape else {
        panic!("actual current controller access required")
    };
    let rejected = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: access.basis(),
        action: PlayerAction::PlayLand {
            source: foreign_handle,
        },
    };
    let owner = NativeHostOwner::new("fixture:source-reader").unwrap();
    assert!(
        matches!(runtime.controller_command(&repository,&secrets,&proof,&owner,&rejected).unwrap(),ApiResponse::Fail{data}if data.get("source").is_some_and(|v|v=="invalid"))
    );
    assert!(
        matches!(repository.controller_original(&secrets,&proof,&rejected).unwrap(),Some(ApiResponse::Fail{data})if data.get("source").is_some_and(|v|v=="invalid"))
    );
    let original = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: access.basis(),
        action: PlayerAction::Concede {},
    };
    assert!(
        repository
            .controller_original(&secrets, &proof, &original)
            .unwrap()
            .is_none()
    );
    let faults = Arc::new(LostAck {
        accepted: Mutex::new(None),
        lost: AtomicUsize::new(0),
        submit: AtomicUsize::new(0),
        no_native: AtomicBool::new(false),
        forbidden: AtomicUsize::new(0),
    });
    let proxy = fault_proxy::Proxy::new(&base, None, faults.clone());
    let proxied = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(proxy.url(), &key).unwrap(),
            "controller-connected",
            Duration::from_secs(60),
        )
        .unwrap(),
    );
    let owner = NativeHostOwner::new("fixture:source-reader").unwrap();
    let ApiResponse::Success { data: ack } = runtime
        .controller_command(&proxied, &secrets, &proof, &owner, &original)
        .unwrap()
    else {
        panic!("actual controller concession required")
    };
    ack.validate(&original, r.seat_ids[1]).unwrap();
    assert_eq!(ack.basis.revision.get(), 1);
    assert_eq!(faults.submit.load(Ordering::SeqCst), 1);
    assert_eq!(faults.lost.load(Ordering::SeqCst), 1);
    let outcome = lifecycle_reads::sql(
        &database,
        "SELECT app.consume_game_outcome((SELECT id FROM app.game_result WHERE game_id=$1),(SELECT revision FROM app.match WHERE id=$2))",
        &[&r.game_id, &r.match_id],
    );
    assert_eq!(outcome["status"], "success");
    assert_eq!(outcome["data"]["phase"], "between_games");
    let stale = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: original.basis.clone(),
        action: PlayerAction::Concede {},
    };
    assert!(
        matches!(runtime.controller_command(&repository,&secrets,&proof,&owner,&stale).unwrap(),ApiResponse::Fail{data}if data.get("basis").is_some_and(|v|v=="stale"))
    );
    faults.no_native.store(true, Ordering::SeqCst);
    let retry = runtime
        .controller_command(&proxied, &secrets, &proof, &owner, &original)
        .unwrap();
    assert!(matches!(retry,ApiResponse::Success{data}if data==ack));
    assert_eq!(faults.forbidden.load(Ordering::SeqCst), 0);
    assert_eq!(faults.submit.load(Ordering::SeqCst), 1);
    let next = rotate(&database, r, &meta);
    assert!(
        matches!(repository.controller_original(&secrets,&proof,&original).unwrap(),Some(ApiResponse::Fail{data})if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
    secrets
        .revoke(meta["grant"].as_str().unwrap().parse().unwrap())
        .unwrap();
    let (next_token, next_proof) = install(&secrets, &next);
    assert!(
        matches!(runtime.controller_command(&proxied,&secrets,&next_proof,&owner,&original).unwrap(),ApiResponse::Success{data}if data==ack)
    );
    assert_eq!(faults.forbidden.load(Ordering::SeqCst), 0);
    let canonical_grant: Uuid = next["grant"].as_str().unwrap().parse().unwrap();
    let canonical_database = database.clone();
    let canonical_late=NativeGameRuntime::from_env(&base,&key).unwrap().unwrap().with_delivery_hook(TestDeliveryStage::Refresh,move|_,_,_,_|{let _=lifecycle_reads::sql(&canonical_database,"UPDATE app.controller_grant SET state='revoked' WHERE id=$1 RETURNING to_jsonb(id)",&[&canonical_grant]);});
    assert!(matches!(
        canonical_late
            .controller_view(&repository, &secrets, &next_proof, &request)
            .unwrap(),
        NativeViewOutcome::Unavailable
    ));
    secrets.revoke(canonical_grant).unwrap();
    let third = rotate(&database, r, &next);
    let (third_token, third_proof) = install(&secrets, &third);
    let revoke = secrets.clone();
    let current_grant: Uuid = third["grant"].as_str().unwrap().parse().unwrap();
    let late = NativeGameRuntime::from_env(&base, &key)
        .unwrap()
        .unwrap()
        .with_delivery_hook(TestDeliveryStage::Refresh, move |_, _, _, _| {
            revoke.revoke(current_grant).unwrap();
        });
    assert!(matches!(
        late.controller_view(&repository, &secrets, &third_proof, &request)
            .unwrap(),
        NativeViewOutcome::Unavailable
    ));
    let fourth = rotate(&database, r, &third);
    let (fourth_token, fourth_proof) = install(&secrets, &fourth);
    let failure = Arc::new(RevokeOnRecovery {
        secrets: secrets.clone(),
        grant: fourth["grant"].as_str().unwrap().parse().unwrap(),
        tripped: AtomicBool::new(false),
    });
    let failing = fault_proxy::Proxy::new(&base, None, failure.clone());
    let failed_repository = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(failing.url(), &key).unwrap(),
            "controller-recovery-failure",
            Duration::from_secs(60),
        )
        .unwrap(),
    );
    assert!(matches!(
        runtime
            .controller_view(&failed_repository, &secrets, &fourth_proof, &request)
            .unwrap(),
        NativeViewOutcome::Unavailable
    ));
    assert!(failure.tripped.load(Ordering::SeqCst));
    extra_tokens.push(superseded_generation(
        &repository,
        &runtime,
        &database,
        r,
        &fourth,
        &original,
        &owner,
    ));
    let rows = lifecycle_reads::sql(
        &database,
        "SELECT coalesce(jsonb_agg(to_jsonb(c)),'[]'::jsonb) FROM app.command_receipt c",
        &[],
    );
    let text = serde_json::to_string(&rows).unwrap();
    assert!(!text.contains(&token.expose_for_delivery()));
    assert!(!text.contains(&next_token.expose_for_delivery()));
    assert!(!text.contains(&third_token.expose_for_delivery()));
    assert!(!text.contains(&fourth_token.expose_for_delivery()));
    for token in extra_tokens {
        assert!(!text.contains(&token));
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"controller","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualLocalPossession":true,"actualControllerView":true,"reciprocalPrivateHands":true,"humanConsumerPreserved":true,"sealedBridgeRejection":true,"actualStaleDispatch":true,"actualControllerCommand":true,"originalRetry":true,"replacementOriginal":true,"lostAckRecovered":true,"localLateRevocationDenied":true,"canonicalLateRevocationDenied":true,"compositionErrorRevocationDenied":true,"canonicalRevokedGrantDenied":true,"foreignExistingGrantDenied":true,"unknownGrantDenied":true,"foreignBindingDenied":true,"wrongEpochDenied":true,"foreignContextDenied":true,"supersededGenerationDenied":true,"supersededCommandNoWrite":true,"rawCredentialAbsentFromCanonicalRows":true,"providerActivated":false,"fullyQualifiedCards":0})
    );
}
