//! Owned actual RPC/Storage transition composition with genuine source replay.
//! Fixture release/profile do not qualify full cards, deck legality or feature metrics.
use super::view_runtime::TestDeliveryStage;
use super::*;
use std::fs;
mod active_replay;
mod archive_integrity;
mod bank_expiry;
mod between_receipts;
mod cast_activity;
mod command_failures;
mod completed_history;
mod controller;
mod delivery;
mod delivery_deletion;
mod delivery_expiry;
mod delivery_failures;
mod delivery_handoff;
mod delivery_logout;
mod delivery_presentation;
mod delivery_sources;
mod fault_proxy;
mod history_delivery;
mod history_failures;
mod history_pages;
mod history_scans;
mod http;
mod lifecycle_reads;
mod mixed_replay;
mod no_contest;
mod opening_delivery;
mod publication_budget;
mod read_account;
mod read_controls;
mod replay_frames;
mod replay_lost_ack;
mod replay_publication;
mod response_expiry;
mod results;
mod rule_errors;
mod source_failures;
mod turn_expiry;
mod unsupported;
mod write_failures;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reservation {
    room_receipts: Option<RoomReceipts>,
    main_entries: Option<std::collections::BTreeMap<Uuid, FixtureMainEntry>>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    profile_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    format: Option<crate::account::FormatSummary>,
    expected_match_revision: Amount,
    structure: crate::rooms::configuration::MatchStructure,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    seat_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    definition_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    printing_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomReceipts {
    room: Uuid,
    ready: OriginalReceipt,
    countdown: OriginalReceipt,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalReceipt {
    original: crate::qualified_intents::RoomAdmissionIntent,
    ack: crate::commands::RevisionAck,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureMainEntry {
    definition_id: Uuid,
    printing_id: Uuid,
    name: String,
    quantity: usize,
}
impl Reservation {
    fn deck_size(&self) -> usize {
        if self.seat_ids.len() == 3 { 60 } else { 20 }
    }
    fn bank_policy(&self) -> &'static str {
        match self.structure {
            crate::rooms::configuration::MatchStructure::Bo1 => "bo1-bank-inactive-v1",
            crate::rooms::configuration::MatchStructure::Bo3 => "bo3-overall-bank-v1",
        }
    }
}
fn selected_origin(
    provenance: Option<&NativeInitialProvenance>,
    state: &GameState,
    reservation: &Reservation,
) {
    let provenance = provenance.expect("actual v2 provenance required");
    assert_eq!(
        provenance.scope(),
        (reservation.game_id, reservation.context_id)
    );
    let mut count = 0;
    let mut occurrences = std::collections::BTreeMap::<Uuid, usize>::new();
    for object in state.execution().window().cards().iter() {
        let original = provenance.physical_card(object).unwrap();
        if let Some(entries) = &reservation.main_entries {
            let expected = &entries[&original.entry_id()];
            assert_eq!(original.definition_id(), expected.definition_id);
            assert_eq!(original.printing_id(), Some(expected.printing_id));
            *occurrences.entry(original.entry_id()).or_default() += 1;
        } else {
            assert_eq!(original.definition_id(), reservation.definition_id);
            assert_eq!(original.printing_id(), Some(reservation.printing_id));
        }
        count += 1;
    }
    if let Some(entries) = &reservation.main_entries {
        assert_eq!(occurrences.len(), entries.len());
        for (id, entry) in entries {
            assert_eq!(occurrences[id], entry.quantity);
        }
    }
    assert_eq!(count, reservation.seat_ids.len() * reservation.deck_size());
}
#[test]
#[ignore = "requires owned managed RPC/Storage realm and genuine retained source"]
fn owned_native_source_reader_reconstructs_original_and_advanced_checkpoint() {
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let file = std::env::var_os("SPELLWAKE_NATIVE_SOURCE_RESERVATION").unwrap();
    let reservation: Reservation = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    let multiplayer = std::env::var("SPELLWAKE_NATIVE_OPENING_RESPONSE").is_ok_and(|s| s == "1");
    assert_eq!(reservation.seat_ids.len(), if multiplayer { 3 } else { 2 });
    let deck_size = reservation.deck_size();
    let read = |name| fs::read(std::env::var_os(name).expect("genuine source required")).unwrap();
    let programs = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        &read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
    .unwrap();
    let phase = std::env::var("SPELLWAKE_NATIVE_SOURCE_PHASE").unwrap();
    let (fixture_oracle, fixture_name) =
        if std::env::var("SPELLWAKE_NATIVE_RULE_ERRORS").is_ok_and(|v| v == "1") {
            ("058917c1-21ab-488a-9f9c-591c55f3c596", "Diabolic Edict")
        } else {
            ("b2c6aa39-2d2a-459c-a555-fb48ba993373", "Island")
        };
    let output = std::env::var_os("SPELLWAKE_NATIVE_SOURCE_OUTPUT").unwrap();
    let ids = NativeInitialIds {
        game_id: reservation.game_id,
        context_id: reservation.context_id,
        release_id: reservation.release_id,
        profile_id: reservation.profile_id,
    };
    let store = NativeArtifactStore::from_env().unwrap();
    if phase == "prepare" {
        let source =
            crate::native_game::preparation_source::NativePreparationRepository::from_env()
                .unwrap()
                .reserved_game(ids.game_id)
                .unwrap()
                .unwrap();
        let original = if multiplayer {
            let format = reservation.format.as_ref().unwrap();
            let open = |selected_format: &crate::account::FormatSummary| {
                programs.prepare_selected_constructed_opening(
                    ids.context_id,
                    &source,
                    selected_format,
                    reservation.seat_ids[0],
                    vec![0.into(); 2048],
                )
            };
            for (id, key) in [
                (Uuid::new_v4(), format.key.as_str()),
                (format.id, "commander"),
                (format.id, "modern"),
            ] {
                let foreign = crate::account::FormatSummary {
                    id,
                    key: key.to_owned(),
                    name: format.name.clone(),
                };
                assert!(open(&foreign).is_err());
            }
            open(format)
        } else {
            let selection = source
                .metadata()
                .seats
                .iter()
                .map(|seat| {
                    let [entry] = seat.entries.as_slice() else {
                        panic!("owned one-entry fixture required")
                    };
                    assert_eq!(
                        entry.quantity,
                        Amount::from(u64::try_from(deck_size).unwrap())
                    );
                    assert_eq!(entry.oracle_ref.as_str(), fixture_oracle);
                    assert_eq!(entry.definition_id, reservation.definition_id);
                    assert_eq!(entry.printing.as_ref().unwrap().id, reservation.printing_id);
                    NativeInitialSelection {
                        seat_id: seat.seat_id,
                        entry_ids: vec![entry.entry_id; deck_size],
                    }
                })
                .collect();
            prepare_selected_initial(
                ids.context_id,
                &programs,
                &source,
                NativeInitialSpec {
                    first_player_seat_id: reservation.seat_ids[0],
                    participants: reservation
                        .seat_ids
                        .iter()
                        .map(|seat| NativeInitialParticipant {
                            seat_id: *seat,
                            life: 20.into(),
                            cards: (0..deck_size)
                                .map(|_| NativeInitialCard {
                                    oracle_id: spellwake_card_programs::OracleId::try_from(
                                        fixture_oracle.to_owned(),
                                    )
                                    .unwrap(),
                                    zone: NativeInitialZone::Library,
                                })
                                .collect(),
                        })
                        .collect(),
                    entropy: vec![
                        0.into();
                        if std::env::var("SPELLWAKE_NATIVE_UNSUPPORTED_WORK")
                            .is_ok_and(|v| v == "1")
                        {
                            38
                        } else {
                            2048
                        }
                    ],
                },
                selection,
            )
        }
        .unwrap();
        assert!(original.provenance().unwrap().matches_reserved(&source));
        selected_origin(
            original.provenance(),
            original.candidate().state(),
            &reservation,
        );
        let original_json: serde_json::Value =
            serde_json::from_slice(original.original_bytes()).unwrap();
        assert_eq!(original_json["version"], "native-initial-v2");
        let root = store
            .retain(
                ids.game_id,
                ids.context_id,
                NativeArtifactRole::Initial,
                original.original_bytes(),
            )
            .unwrap();
        assert!(root == *original.reference());
        let first = NativeCheckpoint::initial(original).unwrap();
        if std::env::var("SPELLWAKE_NATIVE_UNSUPPORTED_WORK").is_ok_and(|v| v == "1") {
            assert_eq!(
                first.state().entropy().unwrap().remaining(),
                0,
                "two twenty-card initial shuffles consume38words"
            );
        }
        let display = NativeImmutableDisplay::new(&programs, first.provenance().unwrap()).unwrap();
        for object in first.state().execution().window().cards().iter() {
            let definition = first
                .state()
                .execution()
                .copiable_program(object.id())
                .unwrap();
            let described = display.object(object, definition).unwrap();
            if let Some(entries) = &reservation.main_entries {
                let origin = first.provenance().unwrap().physical_card(object).unwrap();
                let expected = &entries[&origin.entry_id()];
                assert_eq!(described.definition_id, Some(expected.definition_id));
                assert_eq!(described.printing_id, Some(expected.printing_id));
                assert_eq!(described.characteristics.name, expected.name);
            } else {
                assert_eq!(described.definition_id, Some(reservation.definition_id));
                assert_eq!(described.printing_id, Some(reservation.printing_id));
                assert_eq!(described.characteristics.name, fixture_name);
            }
        }
        assert!(first.retain(&store).unwrap() == *first.reference());
        let facts = first.prepare_results().unwrap().retain(&store).unwrap();
        let event = first.prepare_event().unwrap().retain(&store).unwrap();
        let opening = json!({"eventSchemaRevision":"native-transition-v1","contextId":ids.context_id,"snapshotRef":root.as_str(),"contextRevision":"fixture:native-source-reader-v1",
            "stateRef":first.reference().as_str(),"factsRef":facts.as_str(),"eventRef":event.as_str(),"qualificationRef":"fixture:partial-registry-not-admission","clockObservation":first.opening_clock_observation().unwrap()});
        let mut command = crate::admission_ports::AcceptReservedGame {
            match_id: reservation.match_id,
            expected_revision: reservation.expected_match_revision.clone(),
            game_id: ids.game_id,
            owner: "fixture:source-reader".parse().unwrap(),
            opening: crate::admission_ports::OpeningArtifacts::from_bytes(
                &serde_json::to_vec(&opening).unwrap(),
            )
            .unwrap(),
            original_accepted_basis: Some(reservation.expected_match_revision.clone()),
        };
        let opening_original =
            NativeSystemOriginal::prepare_opening(&command, &first, &programs).unwrap();
        let repository = NativeSourceRepository::from_env().unwrap();
        command.match_id = Uuid::new_v4();
        assert!(
            repository
                .accept_opening(
                    &store,
                    &source,
                    &command,
                    &first,
                    &programs,
                    &opening_original
                )
                .is_err()
        );
        command.match_id = reservation.match_id;
        command.expected_revision = command
            .expected_revision
            .checked_add(&Amount::from(1))
            .unwrap();
        assert!(
            repository
                .accept_opening(
                    &store,
                    &source,
                    &command,
                    &first,
                    &programs,
                    &opening_original
                )
                .is_err()
        );
        command.expected_revision = reservation.expected_match_revision.clone();
        let mut foreign = opening.clone();
        foreign["stateRef"] = json!(event.as_str());
        foreign["clockObservation"]["proofRef"] = json!(event.as_str());
        command.opening = crate::admission_ports::OpeningArtifacts::from_bytes(
            &serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        assert!(
            repository
                .accept_opening(
                    &store,
                    &source,
                    &command,
                    &first,
                    &programs,
                    &opening_original
                )
                .is_err()
        );
        command.opening = crate::admission_ports::OpeningArtifacts::from_bytes(
            &serde_json::to_vec(&opening).unwrap(),
        )
        .unwrap();
        let mut wrong_chooser = opening.clone();
        wrong_chooser["clockObservation"]["seatId"] = json!(
            reservation
                .seat_ids
                .iter()
                .find(|seat| json!(seat) != opening["clockObservation"]["seatId"])
                .unwrap()
        );
        command.opening = crate::admission_ports::OpeningArtifacts::from_bytes(
            &serde_json::to_vec(&wrong_chooser).unwrap(),
        )
        .unwrap();
        let false_clock_original =
            NativeSystemOriginal::prepare_opening(&command, &first, &programs).unwrap();
        assert!(
            repository
                .accept_opening(
                    &store,
                    &source,
                    &command,
                    &first,
                    &programs,
                    &false_clock_original
                )
                .is_err()
        );
        command.opening = crate::admission_ports::OpeningArtifacts::from_bytes(
            &serde_json::to_vec(&opening).unwrap(),
        )
        .unwrap();
        let ack = repository
            .accept_opening(
                &store,
                &source,
                &command,
                &first,
                &programs,
                &opening_original,
            )
            .unwrap();
        let retry = repository
            .accept_opening(
                &store,
                &source,
                &command,
                &first,
                &programs,
                &opening_original,
            )
            .unwrap();
        assert_eq!(ack.resource_id, retry.resource_id);
        assert_eq!(ack.context_id, retry.context_id);
        assert_eq!(ack.revision, retry.revision);
        command.owner = "fixture:changed-original-owner".parse().unwrap();
        assert!(
            repository
                .accept_opening(
                    &store,
                    &source,
                    &command,
                    &first,
                    &programs,
                    &opening_original
                )
                .is_err()
        );
        let retained = repository.opening(ids.game_id).unwrap().unwrap();
        assert_eq!(retained.original_basis().0, "fixture:source-reader");
        fs::write(output, serde_json::to_vec(&opening).unwrap()).unwrap();
        println!(
            "OWNED_NATIVE_SOURCE_READER_OK {}",
            json!({"phase":"prepare","stateManifests":1,"fullyQualifiedCards":0,"actualNativeTransitionEvidence":true,"actualRustOpeningAcceptance":true,"actualInitialClockObservation":true,"constructedFormatControls":multiplayer,"falseOpeningClockRejected":true,"sameOriginalRetry":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true})
        );
        return;
    }
    if phase == "results" {
        if std::env::var("SPELLWAKE_NATIVE_BETWEEN_RECEIPTS").is_ok_and(|v| v == "1") {
            between_receipts::run(&reservation);
        } else if multiplayer {
            response_expiry::run(&reservation, &store, &programs);
        } else if std::env::var("SPELLWAKE_NATIVE_BANK_EXPIRY").is_ok_and(|v| v == "1") {
            bank_expiry::run(&reservation, &store, &programs);
        } else {
            results::run(&reservation, &store, &programs);
        }
        return;
    }
    if phase == "replay-lost-ack" {
        replay_lost_ack::run(&reservation);
        return;
    }
    if phase == "controller" {
        controller::run(&reservation, &store, &programs);
        return;
    }
    if phase == "mixed-replay" {
        mixed_replay::run(&reservation, &store);
        return;
    }
    if phase == "publication-budget" {
        publication_budget::run(&reservation, &store, &programs);
        return;
    }
    if phase.starts_with("publication-") {
        replay_publication::run(&reservation, &phase);
        return;
    }
    if phase.starts_with("archive-prepare-") {
        let incident = phase == "archive-prepare-incident";
        assert!(incident || phase == "archive-prepare-completed");
        lifecycle_reads::prepare_archive(&reservation, incident);
        println!(
            "OWNED_NATIVE_SOURCE_READER_OK {}",
            json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"canonicalArchivePrepared":true,"fixtureObligationsEmpty":true,"fixtureShapeChecked":true,"incidentDeniedModesChecked":if incident {3} else {0},"incident":incident,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
        );
        return;
    }
    if phase.starts_with("archive-session-") {
        history_delivery::run(&reservation, &phase);
        return;
    }
    if phase.starts_with("archive-handoff-") {
        delivery_handoff::run(&reservation, &store, &programs, &phase);
        return;
    }
    if phase.starts_with("archive-unavailable-") {
        read_controls::unavailable(&reservation, &phase);
        return;
    }
    if phase.starts_with("read-account-") {
        read_account::run(&reservation, &phase);
        return;
    }
    if phase.starts_with("read-unavailable-") {
        read_controls::unavailable(&reservation, &phase);
        return;
    }
    if phase.starts_with("read-cancellation-") {
        read_controls::cancellation(&reservation, &phase);
        return;
    }
    if phase.starts_with("lifecycle-read-") {
        lifecycle_reads::run(&reservation, &phase);
        return;
    }
    if phase.starts_with("handoff-") {
        delivery_handoff::run(&reservation, &store, &programs, &phase);
        return;
    }
    if phase.starts_with("history-error-expiry-") {
        history_failures::expiry(&reservation, &phase);
        return;
    }
    if phase.starts_with("history-error-") {
        history_failures::run(&reservation, &phase);
        return;
    }
    if phase.starts_with("history-") {
        history_delivery::run(&reservation, &phase);
        return;
    }
    if phase == "delivery-failures" {
        delivery_failures::run(&reservation, &store, &programs);
        return;
    }
    if phase == "sources" {
        delivery_sources::run(&reservation, &store, &programs);
        return;
    }
    if phase.starts_with("deletion-") {
        delivery_deletion::run(&reservation, &store, &programs, &phase);
        return;
    }
    if phase == "presentation" {
        delivery_presentation::run(&reservation, &store, &programs);
        return;
    }
    if phase.starts_with("logout-") {
        delivery_logout::run(&reservation, &store, &programs, &phase);
        return;
    }
    if phase.starts_with("expiry-") {
        delivery_expiry::run(&reservation, &store, &programs, &phase);
        return;
    }
    if phase == "delivery" {
        delivery::run(&reservation, &store, &programs);
        return;
    }
    assert!(matches!(phase.as_str(), "advance" | "verify"));
    let repository = NativeSourceRepository::from_env().unwrap();
    let observed = repository.read(ids.game_id).unwrap().unwrap();
    assert_eq!(observed.match_id(), reservation.match_id);
    for (seat, _, _, _, _) in observed.clocks() {
        assert_eq!(observed.clock_policy(seat), Some(reservation.bank_policy()));
    }
    assert_eq!(observed.clock_policy(Uuid::new_v4()), None);
    assert_eq!(
        observed.context_ids(),
        (ids.context_id, ids.release_id, ids.profile_id)
    );
    assert_eq!(
        observed
            .controllers()
            .map(|(id, _, _)| id)
            .collect::<Vec<_>>(),
        reservation.seat_ids
    );
    let initial_expected = observed.initial_expected().unwrap();
    let restored_original = load_initial(&store, &initial_expected, &programs).unwrap();
    selected_origin(
        restored_original.provenance(),
        restored_original.candidate().state(),
        &reservation,
    );
    let recovered = repository
        .recover_with_history(
            ids.game_id,
            &store,
            &programs,
            NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.history_evidence(),
        NativeHistoryEvidence::TransitionV1
    );
    assert_eq!(
        recovered.opening().ids(),
        (reservation.match_id, ids.game_id, ids.context_id)
    );
    assert_eq!(
        recovered.source().accepted_revision(),
        observed.accepted_revision()
    );
    let restored = recovered.checkpoint();
    selected_origin(restored.provenance(), restored.state(), &reservation);
    let display = NativeImmutableDisplay::new(&programs, restored.provenance().unwrap()).unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let bearer = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(
            &base,
            &public,
            &format!("{}/auth/v1", base.trim_end_matches('/')),
        )
        .unwrap(),
    )
    .unwrap();
    let actor = auth.resolve(&bearer).unwrap();
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(&actor, ids.game_id, ids.context_id)
        .unwrap()
    else {
        panic!("actual human participant required")
    };
    assert_eq!(access.viewer_seat_id(), reservation.seat_ids[0]);
    assert_eq!(access.basis(), *restored.basis());
    let bound = access.bind(&recovered).unwrap();
    assert_eq!(bound.public_basis(), restored.basis());
    assert!(access.completed_disclosure().is_none());
    assert!(matches!(
        repository
            .human_access(&actor, ids.game_id, Uuid::new_v4())
            .unwrap(),
        NativeHumanAccessOutcome::Unavailable
    ));
    let peer_bearer = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let peer = auth.resolve(&peer_bearer).unwrap();
    let NativeHumanAccessOutcome::Granted(peer_access) = repository
        .human_access(&peer, ids.game_id, ids.context_id)
        .unwrap()
    else {
        panic!("other admitted participant required")
    };
    assert_eq!(peer_access.viewer_seat_id(), reservation.seat_ids[1]);
    assert_eq!(
        peer_access.bind(&recovered).unwrap().public_basis(),
        restored.basis()
    );
    let outsider_bearer = std::env::var("SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER").unwrap();
    let outsider = auth.resolve(&outsider_bearer).unwrap();
    assert!(matches!(
        repository
            .human_access(&outsider, ids.game_id, ids.context_id)
            .unwrap(),
        NativeHumanAccessOutcome::Unavailable
    ));
    let view_metadata = NativeProjectionMetadata {
        through_event_sequence: observed.last_event_sequence().clone(),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    };
    let issued = repository
        .issue_human_view(&store, &access, &recovered, &view_metadata, &display)
        .unwrap();
    let repeated = repository
        .issue_human_view(&store, &access, &recovered, &view_metadata, &display)
        .unwrap();
    assert_eq!(issued.receipt_id(), repeated.receipt_id());
    // Actual SQL-accepted historical cuts join native Event/result bytes and
    // reuse their own immutable first-winner Views, not a copy of the current tip.
    let historical = repository
        .project_human_history(
            &store,
            &programs,
            &access,
            NativeRecoveryBudget::new(1_000, 16_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    let historical_count = usize::try_from(restored.basis().revision.get()).unwrap() + 1;
    assert_eq!(historical.observations().len(), historical_count);
    assert!(historical.recovered().checkpoint().basis() == restored.basis());
    for (index, observation) in historical.observations().iter().enumerate() {
        let projection = observation.projection();
        let snapshot = projection.snapshot();
        assert_eq!(snapshot.basis.revision.get(), u64::try_from(index).unwrap());
        assert_eq!(
            snapshot.through_event_sequence,
            Amount::from(u64::try_from(index).unwrap())
        );
        assert_eq!(snapshot.viewer_seat_id, access.viewer_seat_id());
        assert!(!observation.receipt_id().is_nil());
        assert!(!observation.checkpoint_id().is_nil());
        assert!(!observation.creator_receipt_id().is_nil());
        let facts = projection
            .facts(restored.state().execution().creature_vocabulary())
            .unwrap();
        assert!(!facts.authorized_full_disclosure);
        snapshot.validate(&facts).unwrap();
    }
    let tip = historical.observations().last().unwrap();
    assert_eq!(tip.receipt_id(), issued.receipt_id());
    assert!(tip.projection().snapshot() == issued.view().snapshot());
    let historical_retry = repository
        .project_human_history(
            &store,
            &programs,
            &access,
            NativeRecoveryBudget::new(1_000, 16_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    for (original, retry) in historical
        .observations()
        .iter()
        .zip(historical_retry.observations())
    {
        assert_eq!(original.receipt_id(), retry.receipt_id());
        assert!(original.projection().snapshot() == retry.projection().snapshot());
    }
    assert!(
        repository
            .project_human_history(
                &store,
                &programs,
                &access,
                NativeRecoveryBudget::new(1, 1).unwrap(),
            )
            .is_err()
    );
    let cursor = repository.issue_human_snapshot_cursor(&issued).unwrap();
    let repeated_cursor = repository.issue_human_snapshot_cursor(&repeated).unwrap();
    let public_cursor = cursor.cursor();
    assert!(public_cursor == repeated_cursor.cursor());
    history_scans::run(&repository, &historical, &access, &public_cursor);
    cursor.validate_current(&access).unwrap();
    let stored = repository
        .read_human_cursor(&access, &public_cursor)
        .unwrap()
        .unwrap();
    assert!(stored.cursor() == public_cursor);
    assert!(
        repository
            .read_human_cursor(&peer_access, &public_cursor)
            .unwrap()
            .is_none()
    );
    let mut substituted = public_cursor.clone();
    substituted.token = format!("n1.{}", Uuid::new_v4()).try_into().unwrap();
    assert!(
        repository
            .read_human_cursor(&access, &substituted)
            .unwrap()
            .is_none()
    );
    substituted = public_cursor.clone();
    substituted.through_event_sequence = Amount::from(u64::MAX);
    assert!(
        repository
            .read_human_cursor(&access, &substituted)
            .unwrap()
            .is_none()
    );
    let free: crate::cosmetics::AppearanceSelection =
        serde_json::from_str(&std::env::var("SPELLWAKE_NATIVE_PRESENTATION_FREE").unwrap())
            .unwrap();
    let NativePresentationOutcome::Observed(presentation) = repository
        .human_presentation(&actor, ids.game_id, ids.context_id, &free)
        .unwrap()
    else {
        panic!("actual current presentation required")
    };
    let roster: Vec<_> = observed.controllers().map(|(seat, _, _)| seat).collect();
    presentation.validate_against(&access, &roster).unwrap();
    assert_eq!(presentation.presentation().seats.len(), 2);

    for object in &issued.view().snapshot().visible_objects {
        match &object.identity {
            crate::player_wire::PlayerObjectIdentity::Known {
                definition_id,
                printing_id,
                characteristics,
                ..
            } => {
                assert_eq!(*definition_id, Some(reservation.definition_id));
                assert_eq!(*printing_id, Some(reservation.printing_id));
                assert_eq!(characteristics.name, fixture_name);
            }
            crate::player_wire::PlayerObjectIdentity::Concealed {} => {
                panic!("visible-object fixture must have a known physical source")
            }
        }
    }
    assert!(issued.view().reference() == repeated.view().reference());
    assert!(issued.view().snapshot() == repeated.view().snapshot());
    let http = http::HttpProbe::new(
        &base,
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
    );
    let (public_game, public_context) = http.check(
        &bearer,
        &peer_bearer,
        &outsider_bearer,
        &reservation,
        issued.view().snapshot(),
    );
    let after_reads = repository.read(ids.game_id).unwrap().unwrap();
    assert_eq!(
        after_reads.accepted_revision(),
        observed.accepted_revision()
    );
    assert_eq!(after_reads.checkpoint_ids(), observed.checkpoint_ids());
    assert_eq!(
        after_reads.last_event_sequence(),
        observed.last_event_sequence()
    );
    assert_eq!(
        issued.view().snapshot().viewer_seat_id,
        reservation.seat_ids[0]
    );
    let decision = issued.view().snapshot().decisions.first().unwrap();
    let crate::player_wire::PlayerDomain::Options { options, .. } = &decision.domain else {
        panic!("actual mulligan options required")
    };
    let command = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: issued.view().scope().basis().clone(),
        action: crate::player_wire::PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: crate::player_wire::PlayerChoice::Option {
                option: options[0].handle.clone(),
            },
        },
    };
    let proposed = issued.view().scope().propose(&command).unwrap();
    let retained = issued.view().scope().encode_input(&proposed).unwrap();
    let replay = restore_input(
        restored.state(),
        access.bind(&recovered).unwrap(),
        &retained,
    )
    .unwrap();
    assert_eq!(replay.candidate(), proposed.candidate());
    let full = NativeProjectionMetadata {
        through_event_sequence: observed.last_event_sequence().clone(),
        deadline_at: None,
        disclosure: NativeDisclosure::CompletedParticipant,
    };
    assert!(
        repository
            .issue_human_view(&store, &access, &recovered, &full, &display)
            .is_err()
    );
    let wrong_anchor = NativeArtifactRef::for_bytes(
        ids.game_id,
        ids.context_id,
        NativeArtifactRole::State,
        b"{\"fixture\":\"foreign-initial-state\"}",
    )
    .unwrap();
    let wrong_expected = observed
        .checkpoint_expected(&restored_original)
        .unwrap()
        .with_original_state(wrong_anchor)
        .unwrap();
    assert!(matches!(
        load_checkpoint(
            &store,
            &wrong_expected,
            &initial_expected,
            &programs,
            NativeRecoveryBudget::new(2, 4_000_000).unwrap()
        ),
        Err(NativeCheckpointError::Invalid)
    ));
    let viewer = reservation.seat_ids[0];
    if phase == "advance" {
        assert_eq!(observed.accepted_revision(), &Amount::from(0));
        assert_eq!(restored.candidate(), restored_original.candidate());
        let scope = NativeScope::new(restored.state(), restored.binding(viewer).unwrap()).unwrap();
        let (bytes, actual) = scope
            .prepare_entropy(vec![u64::MAX, 1, 2])
            .unwrap()
            .into_parts();
        let input = store
            .retain(
                ids.game_id,
                ids.context_id,
                NativeArtifactRole::Input,
                &bytes,
            )
            .unwrap();
        let next = restored
            .append(&store, input, NativeCheckpointInput::Entropy, viewer)
            .unwrap();
        assert_eq!(next.candidate(), &actual);
        selected_origin(next.provenance(), next.state(), &reservation);
        assert!(next.retain(&store).unwrap() == *next.reference());
        let facts = next.prepare_results().unwrap().retain(&store).unwrap();
        let event = next.prepare_event().unwrap().retain(&store).unwrap();
        let candidate = std::sync::Arc::new(recovered.prepare_candidate(&next).unwrap());
        let body: serde_json::Value = serde_json::from_str(candidate.original_json()).unwrap();
        assert_eq!(body["factsRef"], json!(facts));
        assert_eq!(body["events"], json!([event.as_str()]));
        let request = Uuid::new_v4();
        let entropy_original = std::sync::Arc::new(
            NativeSystemOriginal::prepare_entropy(request, &recovered, &next).unwrap(),
        );
        assert!(
            repository
                .commit_entropy(
                    &store,
                    &recovered,
                    Uuid::nil(),
                    &next,
                    &candidate,
                    &entropy_original
                )
                .is_err()
        );
        let wrong_facts = store
            .retain(
                ids.game_id,
                ids.context_id,
                NativeArtifactRole::Facts,
                br#"{"fixture":"not-entropy-results"}"#,
            )
            .unwrap();
        let mut wrong = body.clone();
        wrong["factsRef"] = json!(wrong_facts.as_str());
        let mut fabricated = body.clone();
        fabricated["terminal"] = json!({"outcome":"draw","winnerSeatIds":[],"reason":"native_checked_losses","winnersRef":facts.as_str()});
        fabricated["participation"] = json!(
            reservation
                .seat_ids
                .iter()
                .map(|seat| json!({"seatId":seat,"ownTurns":"3","reason":"native_turn_starts_v1"}))
                .collect::<Vec<_>>()
        );
        let mut wrong_bank = body.clone();
        wrong_bank["clocks"][0]["remainingMs"] = json!("1");
        let mut wrong_pause = body.clone();
        let previous = wrong_pause["clocks"][0]["paused"].as_bool().unwrap();
        wrong_pause["clocks"][0]["paused"] = json!(!previous);
        let mut wrong_event = body.clone();
        wrong_event["events"] = json!([next.reference()]);
        let mut foreign_event = body.clone();
        foreign_event["events"] = json!([NativeArtifactRef::for_bytes(
            Uuid::new_v4(),
            ids.context_id,
            NativeArtifactRole::Event,
            b"foreign"
        )
        .unwrap()]);
        let mut changed_event = body.clone();
        changed_event["events"] = json!([NativeArtifactRef::for_bytes(
            ids.game_id,
            ids.context_id,
            NativeArtifactRole::Event,
            b"not-native-transition"
        )
        .unwrap()]);
        let mut extra_event = body.clone();
        extra_event["events"] = json!([event.as_str(), event.as_str()]);
        let mut missing_marker = body.clone();
        missing_marker
            .as_object_mut()
            .unwrap()
            .remove("eventSchemaRevision");
        for invalid in [
            wrong,
            fabricated,
            wrong_bank,
            wrong_pause,
            wrong_event,
            foreign_event,
            changed_event,
            extra_event,
            missing_marker,
        ] {
            let invalid = crate::admission_ports::HostCandidate::from_bytes(
                &serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
            assert!(
                repository
                    .commit_entropy(
                        &store,
                        &recovered,
                        request,
                        &next,
                        &invalid,
                        &entropy_original
                    )
                    .is_err()
            );
        }
        let next_ref = next.reference().clone();
        let next_basis = next.basis().clone();
        let hook_candidate = std::sync::Arc::clone(&candidate);
        let hook_original = std::sync::Arc::clone(&entropy_original);
        let stale = http::HttpProbe::configure(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            move |runtime| {
                runtime.with_delivery_hook(
                    TestDeliveryStage::Issue,
                    move |repository, recovered, store, programs| {
                        // Deterministic external canonical advancement at the real HTTP
                        // read boundary; no clock sleeps or speculative native commit.
                        let expected =
                            NativeCheckpointExpected::new(next_ref.clone(), next_basis.clone())
                                .unwrap()
                                .with_original_state(
                                    recovered.opening().artifacts().state().clone(),
                                )
                                .unwrap();
                        let next = load_checkpoint(
                            store,
                            &expected,
                            &recovered.source().initial_expected().unwrap(),
                            programs,
                            NativeRecoveryBudget::new(3, 4_000_000).unwrap(),
                        )
                        .unwrap();
                        repository
                            .commit_entropy(
                                store,
                                recovered,
                                request,
                                &next,
                                &hook_candidate,
                                &hook_original,
                            )
                            .unwrap();
                    },
                )
            },
        );
        stale.read(Some(&bearer), public_game, public_context, 409);
        let accepted = repository
            .commit_entropy(
                &store,
                &recovered,
                request,
                &next,
                &candidate,
                &entropy_original,
            )
            .unwrap();
        let retry = repository
            .commit_entropy(
                &store,
                &recovered,
                request,
                &next,
                &candidate,
                &entropy_original,
            )
            .unwrap();
        assert_eq!(accepted.resource_id, retry.resource_id);
        assert_eq!(accepted.revision, retry.revision);
        assert_eq!(accepted.revision, Amount::from(1));
        let NativeHumanAccessOutcome::Granted(new_access) = repository
            .human_access(&actor, ids.game_id, ids.context_id)
            .unwrap()
        else {
            panic!("fresh human cut required")
        };
        assert_eq!(new_access.basis().revision.get(), 1);
        assert!(new_access.bind(&recovered).is_err());
        assert!(
            repository
                .commit_entropy(
                    &store,
                    &recovered,
                    Uuid::new_v4(),
                    &next,
                    &candidate,
                    &entropy_original
                )
                .is_err()
        );
        fs::write(output, serde_json::to_vec(&body).unwrap()).unwrap();
    } else {
        assert_eq!(observed.accepted_revision(), &Amount::from(1));
        let scope = NativeScope::new(
            restored_original.candidate().state(),
            restored_original.binding(viewer).unwrap(),
        )
        .unwrap();
        let (_, actual) = scope
            .prepare_entropy(vec![u64::MAX, 1, 2])
            .unwrap()
            .into_parts();
        assert_eq!(restored.candidate(), &actual);
        assert_eq!(
            restored.basis().epoch.get(),
            restored_original.candidate().state().basis().epoch
        );
        assert_ne!(
            restored.basis().epoch.get().to_string(),
            observed.operational_basis().1.to_string()
        );
        assert_eq!(observed.last_event_sequence(), &Amount::from(1));
        let query = crate::player_wire::PlayerReceiptQuery {
            game_id: ids.game_id,
            context_id: ids.context_id,
            outstanding: vec![command.clone()],
        };
        let ApiResponse::Success { data: unknown } =
            repository.read_human_receipts(&actor, &query).unwrap()
        else {
            panic!("unknown original required")
        };
        query
            .validate_receipts(&unknown, access.viewer_seat_id())
            .unwrap();
        assert!(matches!(
            unknown[0].disposition,
            crate::commands::CommandDisposition::Unknown
        ));
        let player_input = store
            .retain(
                ids.game_id,
                ids.context_id,
                NativeArtifactRole::Input,
                &retained,
            )
            .unwrap();
        let player_next = restored
            .append(&store, player_input, NativeCheckpointInput::Player, viewer)
            .unwrap();
        assert_eq!(player_next.candidate(), proposed.candidate());
        player_next.retain(&store).unwrap();
        let player_facts = player_next
            .prepare_results()
            .unwrap()
            .retain(&store)
            .unwrap();
        let player_event = player_next.prepare_event().unwrap().retain(&store).unwrap();
        let player_candidate = recovered.prepare_candidate(&player_next).unwrap();
        let player_body: serde_json::Value =
            serde_json::from_str(player_candidate.original_json()).unwrap();
        assert_eq!(player_body["factsRef"], json!(player_facts));
        assert_eq!(player_body["events"], json!([player_event.as_str()]));
        let commit = || {
            repository.commit_human(
                &store,
                NativeHumanCommit {
                    actor: &actor,
                    access: &access,
                    recovered: &recovered,
                    issued: &issued,
                    original: &command,
                    next: &player_next,
                    candidate: &player_candidate,
                },
            )
        };
        let command_http = http::HttpProbe::owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
        );
        // Both requests execute and retain from revision one. The inner original
        // wins after the outer proposal is prepared; SQL must reject only the
        // outer original as stale, without accepting a second native successor.
        let competing_original = PlayerCommand {
            request_id: Uuid::new_v4(),
            ..command.clone()
        };
        let race_base = base.clone();
        let race_key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
        let race_token = bearer.clone();
        let winner = command.clone();
        let competing_http = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            move |runtime| {
                runtime.with_delivery_hook(TestDeliveryStage::CommandCommit, move |_, _, _, _| {
                    let inner =
                        http::HttpProbe::owned(&race_base, &race_key, "fixture:source-reader");
                    inner.command(Some(&race_token), &winner, 202);
                })
            },
        );
        let competing_rejection = competing_http.command(Some(&bearer), &competing_original, 409);
        assert_eq!(competing_rejection["data"], json!({"basis":"stale"}));
        assert_eq!(
            http.command(Some(&bearer), &competing_original, 409),
            competing_rejection
        );
        let public_ack = command_http.command(Some(&bearer), &command, 202);
        let ack: crate::player_wire::PlayerCommandAck =
            serde_json::from_value(public_ack["data"].clone()).unwrap();
        ack.validate(&command, access.viewer_seat_id()).unwrap();
        assert_eq!(ack.basis.revision.get(), 2);
        assert_eq!(
            command_http.command(Some(&bearer), &command, 202),
            public_ack
        );
        // Stored originals survive missing write configuration, without execution.
        assert_eq!(http.command(Some(&bearer), &command, 202), public_ack);
        let ApiResponse::Success { data: retry } = commit().unwrap() else {
            panic!("original retry required");
        };
        assert!(ack == retry);
        let ApiResponse::Success { data: receipts } =
            repository.read_human_receipts(&actor, &query).unwrap()
        else {
            panic!("historical accepted original required")
        };
        query
            .validate_receipts(&receipts, access.viewer_seat_id())
            .unwrap();
        assert!(
            matches!(&receipts[0].disposition,crate::commands::CommandDisposition::Accepted {ack:stored} if stored==&ack)
        );
        let mut changed = query.clone();
        changed.outstanding[0].action = crate::player_wire::PlayerAction::Concede {};
        assert!(
            matches!(repository.read_human_receipts(&actor,&changed).unwrap(),ApiResponse::Fail {data} if data.get("request").is_some_and(|value|value=="conflict"))
        );
        let ApiResponse::Success {
            data: peer_receipts,
        } = repository.read_human_receipts(&peer, &query).unwrap()
        else {
            panic!("peer's own namespace required")
        };
        query
            .validate_receipts(&peer_receipts, peer_access.viewer_seat_id())
            .unwrap();
        assert!(matches!(
            peer_receipts[0].disposition,
            crate::commands::CommandDisposition::Unknown
        ));
        let current = repository
            .recover_with_history(
                ids.game_id,
                &store,
                &programs,
                NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            current.history_evidence(),
            NativeHistoryEvidence::TransitionV1
        );
        assert_eq!(current.source().accepted_revision(), &Amount::from(2));
        assert_eq!(current.checkpoint().candidate(), player_next.candidate());
        selected_origin(player_next.provenance(), player_next.state(), &reservation);
        selected_origin(
            current.checkpoint().provenance(),
            current.checkpoint().state(),
            &reservation,
        );
        let mut fresh = command.clone();
        fresh.request_id = Uuid::new_v4();
        fresh.basis = current.checkpoint().basis().clone();
        fresh.action = crate::player_wire::PlayerAction::Pass {};
        http.command(Some(&bearer), &fresh, 503); // Views alone grant no writer.
        let wrong_owner = http::HttpProbe::owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:another-owner",
        );
        wrong_owner.command(Some(&bearer), &fresh, 503);
        let missing_store = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            |runtime| {
                let backend = BackendClient::new(
                    BackendConfig::new(
                        &base,
                        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
                    )
                    .unwrap(),
                    "missing-native-store-control",
                    Duration::from_secs(10),
                )
                .unwrap();
                runtime.with_test_store(
                    NativeArtifactStore::new(backend, format!("missing-{}", Uuid::new_v4()))
                        .unwrap(),
                )
            },
        );
        missing_store.command(Some(&bearer), &fresh, 503);
        let fresh_query = crate::player_wire::PlayerReceiptQuery {
            game_id: ids.game_id,
            context_id: ids.context_id,
            outstanding: vec![fresh.clone()],
        };
        let ApiResponse::Success { data } = repository
            .read_human_receipts(&actor, &fresh_query)
            .unwrap()
        else {
            panic!("receipt query");
        };
        assert!(matches!(
            data[0].disposition,
            crate::commands::CommandDisposition::Unknown
        ));
        // Known accepted past cuts can be rejected without any writer runtime.
        let mut stale_original = command.clone();
        stale_original.request_id = Uuid::new_v4();
        let stale_response = http.command(Some(&bearer), &stale_original, 409);
        assert_eq!(stale_response["data"], json!({"basis":"stale"}));
        assert_eq!(
            http.command(Some(&bearer), &stale_original, 409),
            stale_response
        );
        // Future revision and foreign epoch are ingress refusals, not receipts.
        for future_epoch in [false, true] {
            let mut future = fresh.clone();
            future.request_id = Uuid::new_v4();
            if future_epoch {
                future.basis.epoch = 1.into();
            } else {
                future.basis.revision = 3.into();
            }
            assert_eq!(
                command_http.command(Some(&bearer), &future, 409)["data"],
                json!({"basis":"invalid"})
            );
            assert!(
                repository
                    .human_original(&actor, &future)
                    .unwrap()
                    .is_none()
            );
        }
        // Limits exhaust before any candidate can become a player rejection.
        for budget in [
            NativeRecoveryBudget::new(1, 4_000_000).unwrap(),
            NativeRecoveryBudget::new(100, 1).unwrap(),
        ] {
            let limited = http::HttpProbe::configure_owned(
                &base,
                &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
                "fixture:source-reader",
                |runtime| runtime.with_test_recovery(budget),
            );
            limited.command(Some(&bearer), &fresh, 503);
            assert!(repository.human_original(&actor, &fresh).unwrap().is_none());
        }
        write_failures::run(&repository, &base, &bearer, &actor, &fresh);
        let mut rejected = fresh.clone();
        rejected.request_id = Uuid::new_v4();
        rejected.action = crate::player_wire::PlayerAction::Choose {
            decision: crate::player_wire::PlayerHandle::try_from("unissued-decision".to_owned())
                .unwrap(),
            response: crate::player_wire::PlayerChoice::Option {
                option: crate::player_wire::PlayerHandle::try_from("unissued-option".to_owned())
                    .unwrap(),
            },
        };
        source_failures::run(
            &repository,
            &base,
            &bearer,
            &actor,
            &fresh,
            reservation.structure,
        );
        let denied = command_http.command(Some(&bearer), &rejected, 409);
        assert_eq!(denied["data"], json!({"source":"invalid"}));
        assert_eq!(http.command(Some(&bearer), &rejected, 409), denied);
        assert_eq!(
            repository
                .read(ids.game_id)
                .unwrap()
                .unwrap()
                .accepted_revision(),
            &Amount::from(2)
        );
        // The issued revision-two View contains its exact private registry.
        // Exercise real gateway absence/corruption, restoring every original key.
        let current_access = match repository
            .human_access(&actor, ids.game_id, ids.context_id)
            .unwrap()
        {
            NativeHumanAccessOutcome::Granted(access) => access,
            _ => panic!("current artifact failure participant required"),
        };
        let ObjectResponse(ApiResponse::Success {data:view_source}):ObjectResponse<serde_json::Value> = repository.backend.rpc(
            "read_native_view_source",
            &json!({"p_game":ids.game_id,"p_context":ids.context_id,"p_epoch":fresh.basis.epoch,"p_revision":fresh.basis.revision,
                "p_viewer":current_access.viewer_seat_id(),"p_disclosure":"live"}),
        ).unwrap() else {panic!("issued current View required");};
        let view_ref =
            NativeArtifactRef::try_from(view_source["viewRef"].as_str().unwrap().to_owned())
                .unwrap();
        command_failures::artifacts(
            &repository,
            &store,
            &command_http,
            &actor,
            &bearer,
            (&fresh, &command),
            &[
                current.opening().artifacts().initial().clone(),
                current.checkpoint().reference().clone(),
                current.checkpoint().retained_input().unwrap().1.clone(),
                player_event.clone(),
                player_facts.clone(),
                view_ref,
            ],
        );
        command_http.command(None, &fresh, 401);
        command_http.command(Some(&outsider_bearer), &fresh, 404);
        let mut conflicting = command.clone();
        conflicting.action = crate::player_wire::PlayerAction::Concede {};
        assert_eq!(
            command_http.command(Some(&bearer), &conflicting, 409)["data"],
            json!({"request":"conflict"})
        );
        let race_game = ids.game_id;
        let owner_before = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            move |runtime| {
                runtime.with_before_command_recovery(move |repository| {
                    http::takeover(repository, race_game, "fixture:other-http-host")
                })
            },
        );
        let mut owner_attempt = fresh.clone();
        owner_attempt.request_id = Uuid::new_v4();
        owner_before.command(Some(&bearer), &owner_attempt, 503);
        http::takeover(&repository, ids.game_id, "fixture:source-reader");
        let owner_after = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            move |runtime| {
                runtime.with_delivery_hook(
                    TestDeliveryStage::CommandCommit,
                    move |repository, _, _, _| {
                        http::takeover(repository, race_game, "fixture:other-http-host")
                    },
                )
            },
        );
        owner_attempt.request_id = Uuid::new_v4();
        owner_attempt.action = crate::player_wire::PlayerAction::Concede {};
        owner_after.command(Some(&bearer), &owner_attempt, 503);
        let owner_query = crate::player_wire::PlayerReceiptQuery {
            game_id: ids.game_id,
            context_id: ids.context_id,
            outstanding: vec![owner_attempt.clone()],
        };
        let ApiResponse::Success { data } = repository
            .read_human_receipts(&actor, &owner_query)
            .unwrap()
        else {
            panic!("owner race original");
        };
        assert!(matches!(
            data[0].disposition,
            crate::commands::CommandDisposition::Unknown
        ));
        http::takeover(&repository, ids.game_id, "fixture:source-reader");
        let reject_after = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            move |runtime| {
                runtime.with_delivery_hook(
                    TestDeliveryStage::CommandReject,
                    move |repository, _, _, _| {
                        http::takeover(repository, race_game, "fixture:other-http-host")
                    },
                )
            },
        );
        let mut stable_reject = rejected.clone();
        stable_reject.request_id = Uuid::new_v4();
        assert_eq!(
            reject_after.command(Some(&bearer), &stable_reject, 409)["data"],
            json!({"source":"invalid"})
        );
        assert_eq!(
            repository
                .read(ids.game_id)
                .unwrap()
                .unwrap()
                .accepted_revision(),
            &Amount::from(2)
        );
        http::takeover(&repository, ids.game_id, "fixture:source-reader");
        // The original signed bearer is retained, but current Auth authority ends.
        let origin = reqwest::Url::parse(&base).unwrap();
        assert!(origin.scheme() == "http" && origin.host_str() == Some("127.0.0.1"));
        let revoke_base = base.clone();
        let revoke_public = public.clone();
        let revoke_bearer = bearer.clone();
        let late = http::HttpProbe::configure(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            move |runtime| {
                runtime.with_delivery_hook(TestDeliveryStage::Refresh, move |_, _, _, _| {
                    let revoked = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_secs(10))
                        .build()
                        .unwrap()
                        .post(format!(
                            "{}/auth/v1/logout?scope=local",
                            revoke_base.trim_end_matches('/')
                        ))
                        .header("apikey", &revoke_public)
                        .bearer_auth(&revoke_bearer)
                        .send()
                        .unwrap();
                    assert_eq!(revoked.status(), reqwest::StatusCode::NO_CONTENT);
                })
            },
        );
        late.read(Some(&bearer), public_game, public_context, 401);
        assert!(matches!(
            repository
                .human_access(&actor, ids.game_id, ids.context_id)
                .unwrap(),
            NativeHumanAccessOutcome::Unauthorized
        ));
        assert!(matches!(
            auth.resolve(&bearer),
            Err(crate::actor::ActorError::Unauthorized)
        ));
        http.read(Some(&bearer), public_game, public_context, 401);
        assert!(
            matches!(repository.read_human_receipts(&actor,&query).unwrap(),ApiResponse::Fail {data} if data.get("session").is_some_and(|value|value=="unauthorized"))
        );
        assert!(
            matches!(commit().unwrap(),ApiResponse::Fail {data} if data.get("session").is_some_and(|value|value=="unauthorized"))
        );
        assert!(matches!(
            repository
                .human_access(&peer, ids.game_id, ids.context_id)
                .unwrap(),
            NativeHumanAccessOutcome::Granted(_)
        ));
        let logout_base = base.clone();
        let logout_key = public.clone();
        let logout_peer = peer_bearer.clone();
        let late_command = http::HttpProbe::configure_owned(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
            "fixture:source-reader",
            move |runtime| {
                runtime.with_delivery_hook(TestDeliveryStage::CommandCommit, move |_, _, _, _| {
                    let response = reqwest::blocking::Client::new()
                        .post(format!(
                            "{}/auth/v1/logout?scope=local",
                            logout_base.trim_end_matches('/')
                        ))
                        .header("apikey", &logout_key)
                        .bearer_auth(&logout_peer)
                        .send()
                        .unwrap();
                    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
                })
            },
        );
        let late_original = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: current.checkpoint().basis().clone(),
            action: crate::player_wire::PlayerAction::Concede {},
        };
        late_command.command(Some(&peer_bearer), &late_original, 401);
        assert_eq!(
            repository
                .read(ids.game_id)
                .unwrap()
                .unwrap()
                .accepted_revision(),
            &Amount::from(2)
        );
    }
    let mut metrics = json!({"phase":phase,"actualNativeTransitionEvidence":true,"verifiedTips":if phase=="verify" {2} else {1},"sameActualCandidate":true,"actualManagedReadRpc":true,"actualPrivateStorage":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,"actualRustEntropyCommit":phase=="advance","sameOriginalRetry":phase=="advance","actualHumanAccessAndBinding":true,"peerSeatAndOutsiderChecked":true,"revokedSessionDenied":phase=="verify","actualIssuedViewAndRegistry":true,"actualNativeViewHttp":true,"publicNativeViewScopeDiscovered":true,"actualSelectedClockPolicyObserved":true,"actualClockIntentDerived":true,"actualNativeViewLateStale":phase=="advance","actualNativeViewLateRevocation":phase=="verify","displayQualified":false,"actualNativePlayerCommitAndReceipts":phase=="verify","actualNativeHumanCommandHttp":phase=="verify","actualFreshCommandStoreFailure":phase=="verify","actualRuleRejectHttp":phase=="verify","actualCommandOwnerRaces":phase=="verify","actualCompetingCommands":phase=="verify","actualCommandBasisControls":phase=="verify","actualCommandBudgetRefusals":phase=="verify","actualCommandArtifactFailures":phase=="verify","actualCommandWriteFailures":phase=="verify","actualCommandSourceProjectionFaults":phase=="verify","actualCommandSqlClockRefusal":phase=="verify","actualCommandExhaustedBankProjection":phase=="verify" && matches!(reservation.structure,crate::rooms::configuration::MatchStructure::Bo3),"actualCommandLateRevocation":phase=="verify","initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true});
    metrics["actualHistoricalSeatCutCount"] = json!(historical_count);
    metrics["actualHistoricalViewFirstWinnerRetry"] = json!(true);
    metrics["actualHistoricalRawScansAndPositionalCursors"] = json!(true);
    metrics["actualHistoricalScanFaultCases"] = json!(9);
    metrics["completedFullHistoricalReplayQualified"] = json!(false);
    metrics["actualNativeReconnectHttp"] = json!(true);
    metrics["actualNativeStreamHttp"] = json!(true);
    metrics["actualReconnectUnknownNotResubmitted"] = json!(true);
    metrics["actualTypedSnapshotCursor"] = json!(true);
    metrics["actualTypedCursorCorrelation"] = json!(true);
    metrics["actualTypedCurrentPresentation"] = json!(true);
    metrics["currentPresentationFree"] = json!(free);
    println!("OWNED_NATIVE_SOURCE_READER_OK {}", metrics);
}
