//! Genuine native bytes with explicitly synthetic selected-source admission metadata.
use super::*;
use crate::native_game::preparation_source::{
    NativePreparationRepository, NativeReservedGameSource,
};
fn entry_id(seat: usize, entry: usize) -> Uuid {
    Uuid::from_u128(0x99a0 + (seat as u128) * 10 + entry as u128)
}
fn printing_id(seat: usize, entry: usize) -> Uuid {
    Uuid::from_u128(0xaa00 + (seat as u128) * 10 + entry as u128)
}
fn reserved_value() -> Value {
    let mut value = crate::native_game::preparation_source::tests::reserved_fixture(false);
    let r = &mut value["data"];
    r["gameId"] = json!(ids().game_id);
    r["release"]["id"] = json!(ids().release_id);
    r["profile"]["id"] = json!(ids().profile_id);
    for (index, seat) in r["seats"].as_array_mut().unwrap().iter_mut().enumerate() {
        seat["seatId"] = json!(seats()[index]);
        seat["entries"] = json!((0..3).map(|n| json!({
            "entryId":entry_id(index,n),"definitionId":Uuid::from_u128(0xbb00+n as u128),
            "oracleRef":if n==2{FOREST}else{ISLAND},"definitionRevisionId":Uuid::from_u128(0xcc00+n as u128),
            "definitionRevisionRef":"fixture:immutable-definition","rulesTextRef":"fixture:rules","programRef":"fixture:program",
            "unlimitedBasicLand":true,"printing":if n==2{Value::Null}else{json!({"id":printing_id(index,n),"externalPrintingRef":"fixture:selected-not-source-snapshot","finish":if n==0{"foil"}else{"nonfoil"},"setId":Uuid::from_u128(0xdd00+n as u128)})},
            "section":"main","quantity":"100"
        })).collect::<Vec<_>>());
    }
    value
}
fn reservation(value: Value) -> NativeReservedGameSource {
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_native_admission_source",
        value,
    )]);
    let backend = BackendClient::new(
        BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
        "selected-source-test",
        Duration::from_secs(2),
    )
    .unwrap();
    let result = NativePreparationRepository::new(backend)
        .reserved_game(ids().game_id)
        .unwrap()
        .unwrap();
    assert_eq!(mock.finish().len(), 1);
    result
}
fn selection() -> Vec<NativeInitialSelection> {
    seats()
        .into_iter()
        .enumerate()
        .map(|(seat, seat_id)| NativeInitialSelection {
            seat_id,
            entry_ids: (0..21)
                .map(|n| {
                    entry_id(
                        seat,
                        if n % 2 == 1 {
                            2
                        } else if n % 4 == 0 {
                            0
                        } else {
                            1
                        },
                    )
                })
                .collect(),
        })
        .collect()
}
fn opening(source: &NativeProgramSource, r: &NativeReservedGameSource) -> PreparedNativeOpening {
    prepare_selected_initial(ids().context_id, source, r, spec(), selection()).unwrap()
}
fn assert_origins(checkpoint: &NativeCheckpoint) {
    let provenance = checkpoint.provenance().unwrap();
    assert_eq!(provenance.scope(), (ids().game_id, ids().context_id));
    let mut seen = 0;
    for object in checkpoint.state().execution().window().cards().iter() {
        let id = object.card_id().unwrap().0;
        let seat = usize::try_from((id - 1) / 21).unwrap();
        let n = usize::try_from((id - 1) % 21).unwrap();
        let index = if n % 2 == 1 {
            2
        } else if n % 4 == 0 {
            0
        } else {
            1
        };
        let physical = provenance.physical_card(object).unwrap();
        assert_eq!(physical.entry_id(), entry_id(seat, index));
        assert_eq!(
            physical.printing_id(),
            if index == 2 {
                None
            } else {
                Some(printing_id(seat, index))
            }
        );
        assert_eq!(
            physical.definition_id(),
            Some(Uuid::from_u128(0xbb00 + index as u128))
        );
        seen += 1;
    }
    assert_eq!(seen, 42);
}
fn assert_immutable_display(checkpoint: &NativeCheckpoint, source: &NativeProgramSource) {
    let display = NativeImmutableDisplay::new(source, checkpoint.provenance().unwrap()).unwrap();
    assert!(display.context() == &checkpoint.state().basis().context);
    assert!(
        display.scope()
            == NativeDisplayScope::Game {
                game_id: ids().game_id,
                context_id: ids().context_id,
            }
    );
    for object in checkpoint.state().execution().window().cards().iter() {
        let program = checkpoint
            .state()
            .execution()
            .copiable_program(object.id())
            .unwrap();
        let described = display.object(object, program).unwrap();
        let physical = checkpoint
            .provenance()
            .unwrap()
            .physical_card(object)
            .unwrap();
        assert_eq!(described.definition_id, physical.definition_id());
        assert_eq!(described.printing_id, physical.printing_id());
        assert!(
            described.characteristics
                == *source
                    .printed_card(program)
                    .unwrap()
                    .unwrap()
                    .characteristics()
        );
    }
    for viewer in seats() {
        let mut scope =
            NativeScope::new(checkpoint.state(), checkpoint.binding(viewer).unwrap()).unwrap();
        let snapshot = scope
            .project(
                &NativeProjectionMetadata {
                    through_event_sequence: crate::amount::Amount::from(0u64),
                    deadline_at: None,
                    disclosure: NativeDisclosure::LiveParticipant,
                },
                &display,
            )
            .unwrap();
        assert!(snapshot.visible_objects.iter().all(|object| matches!(
            object.identity,
            crate::player_wire::PlayerObjectIdentity::Known {
                definition_id: Some(_),
                ..
            }
        )));
    }
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary; exact adapter/template composition only"]
fn immutable_display_separates_copied_text_physical_printing_and_source_token_templates() {
    use spellwake_engine::{
        program::{Instruction, ProgramKind},
        token::{TokenBatchMember, TokenEntryPreview},
    };
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let checkpoint =
        NativeCheckpoint::initial(opening(source, &reservation(reserved_value()))).unwrap();
    let display = NativeImmutableDisplay::new(source, checkpoint.provenance().unwrap()).unwrap();
    let objects = checkpoint.state().execution().window().cards();
    let physical = objects
        .iter()
        .find(|object| object.card_id() == Some(CardId(1)))
        .unwrap();
    let forest = source
        .registry
        .seed(
            &OracleId::try_from(FOREST.to_owned()).unwrap(),
            CardId(1),
            SeatId(0),
            ObjectDestination::LibraryBottom,
        )
        .unwrap()
        .program;
    // Direct adapter copy contract. The native copying mechanism has a separate
    // real transition test; this call is not an admitted copy-card trajectory.
    let copied = display.object(physical, &forest).unwrap();
    assert_eq!(copied.characteristics.name, "Forest");
    assert_eq!(copied.printing_id, Some(printing_id(0, 0)));
    assert_eq!(copied.definition_id, Some(Uuid::from_u128(0xbb00)));
    let unknown = objects
        .seed_card(
            objects.revision(),
            CardId(999),
            SeatId(0),
            ObjectDestination::Hand,
        )
        .unwrap();
    assert!(matches!(
        display.object(&unknown.after, &forest),
        Err(NativeProjectionError::MissingDisplay)
    ));
    let token_copy = objects
        .insert_token(
            objects.revision(),
            SeatId(0),
            forest.clone(),
            &source.vocabulary,
        )
        .unwrap();
    let described = display.object(&token_copy.after, &forest).unwrap();
    assert!(described.definition_id.is_none() && described.printing_id.is_none());
    let mut templates = 0;
    for oracle in source.catalogue.keys() {
        let program = source
            .registry
            .seed(
                oracle,
                CardId(1),
                SeatId(0),
                ObjectDestination::LibraryBottom,
            )
            .unwrap()
            .program;
        if let ProgramKind::Spell(spell) = &program.kind {
            for instruction in &spell.body.instructions {
                if let Instruction::CreateTokens { creations } = instruction {
                    for creation in creations {
                        let definition =
                            creation.definition.materialize(&source.vocabulary).unwrap();
                        let token = objects
                            .insert_token(
                                objects.revision(),
                                SeatId(0),
                                definition.clone(),
                                &source.vocabulary,
                            )
                            .unwrap();
                        let described = display.object(&token.after, &definition).unwrap();
                        assert!(
                            described.definition_id.is_none() && described.printing_id.is_none()
                        );
                        assert_eq!(described.characteristics.name, "Soldier Token");
                        assert_eq!(
                            described.characteristics.printed_power.as_deref(),
                            Some("1")
                        );
                        assert_eq!(
                            described.characteristics.printed_toughness.as_deref(),
                            Some("1")
                        );
                        assert!(described.characteristics.rules_text.is_empty());
                        let preview = TokenEntryPreview {
                            enters_tapped: false,
                            member: TokenBatchMember::new(0),
                            creator: SeatId(0),
                            definition: definition.clone(),
                        };
                        assert_eq!(display.token_preview(&preview).unwrap(), "Soldier Token");
                        let mut foreign = definition;
                        foreign.names = spellwake_contracts::names::ObjectNames::nameless();
                        assert!(matches!(
                            display.object(&token.after, &foreign),
                            Err(NativeProjectionError::MissingDisplay)
                        ));
                        templates += 1;
                    }
                }
            }
        }
    }
    assert!(templates > 0);
}
fn assert_incarnation_controller_and_token_mechanisms(
    checkpoint: &NativeCheckpoint,
    source: &NativeProgramSource,
) {
    // Actual lower-level object mechanisms, not a qualified full-game trajectory.
    let provenance = checkpoint.provenance().unwrap();
    let objects = checkpoint.state().execution().window().cards();
    let original = objects
        .iter()
        .find(|object| object.card_id() == Some(CardId(1)))
        .unwrap();
    let moved = objects
        .apply_zone_move(
            objects.revision(),
            original.id(),
            ObjectDestination::Battlefield {
                controller: SeatId(0),
            },
        )
        .unwrap()
        .require_moved()
        .unwrap();
    assert_ne!(original.id(), moved.after.id());
    assert_eq!(
        provenance
            .physical_card(&moved.after)
            .unwrap()
            .printing_id(),
        Some(printing_id(0, 0))
    );
    let controlled = moved
        .next
        .apply_controller(moved.next.revision(), moved.after.id(), SeatId(1))
        .unwrap();
    assert_eq!(controlled.after.id(), moved.after.id());
    assert_eq!(controlled.after.controller(), Some(SeatId(1)));
    assert_eq!(
        provenance
            .physical_card(&controlled.after)
            .unwrap()
            .printing_id(),
        Some(printing_id(0, 0))
    );
    let definition = source
        .registry
        .seed(
            &OracleId::try_from(ISLAND.to_owned()).unwrap(),
            CardId(1),
            SeatId(0),
            ObjectDestination::LibraryBottom,
        )
        .unwrap()
        .program;
    let token = controlled
        .next
        .insert_token(
            controlled.next.revision(),
            SeatId(0),
            definition,
            &source.vocabulary,
        )
        .unwrap();
    assert!(token.after.card_id().is_none());
    assert!(provenance.physical_card(&token.after).is_none());
    let unknown = token
        .next
        .seed_card(
            token.next.revision(),
            CardId(999),
            SeatId(0),
            ObjectDestination::Hand,
        )
        .unwrap();
    assert!(provenance.physical_card(&unknown.after).is_none());
    assert_different_rules_copy_mechanism(provenance);
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn selected_origin_survives_initial_restore_and_same_oracle_distinct_printings() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let original = opening(source, &r);
    let expected = expected(original.reference().clone());
    let restored = read_root(original.original_bytes(), &expected, source).unwrap();
    assert_eq!(original.original_bytes(), restored.original_bytes());
    assert_eq!(original.candidate(), restored.candidate());
    assert!(original.provenance().unwrap().matches_reserved(&r));
    let legacy = prepare_initial(ids(), source, spec()).unwrap();
    assert_eq!(legacy.candidate(), original.candidate());
    assert!(legacy.provenance().is_none());
    let legacy_value: Value = serde_json::from_slice(legacy.original_bytes()).unwrap();
    assert!(legacy_value.get("selected").is_none());
    assert_eq!(legacy_value["version"], "native-initial-v1");
    let legacy_rebuilt = read_root(
        legacy.original_bytes(),
        &self::super::expected(legacy.reference().clone()),
        source,
    )
    .unwrap();
    assert_eq!(legacy_rebuilt.original_bytes(), legacy.original_bytes());
    assert!(legacy_rebuilt.provenance().is_none());
    let checkpoint = NativeCheckpoint::initial(restored).unwrap();
    assert_origins(&checkpoint);
    assert_immutable_display(&checkpoint, source);
    assert_incarnation_controller_and_token_mechanisms(&checkpoint, source);
    let mut changed = reserved_value();
    changed["data"]["seats"][0]["entries"][0]["printing"]["id"] = json!(Uuid::new_v4());
    assert!(
        !original
            .provenance()
            .unwrap()
            .matches_reserved(&reservation(changed))
    );
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn malformed_selected_roots_fail_during_cold_semantic_restore() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let original = opening(source, &r);
    let good: Value = serde_json::from_slice(original.original_bytes()).unwrap();
    let mut cases = vec![];
    for (path, value) in [
        ("/version", json!("native-initial-v1")),
        ("/selected", Value::Null),
        ("/selected/gameId", json!(Uuid::new_v4())),
        ("/selected/releaseId", json!(Uuid::new_v4())),
        ("/selected/profileId", json!(Uuid::new_v4())),
        ("/selected/seats/0/seatId", json!(seats()[1])),
        (
            "/selected/seats/0/selectedRevisionId",
            json!(Uuid::new_v4()),
        ),
        ("/selected/seats/0/entries/0/oracleRef", json!(FOREST)),
        (
            "/selected/seats/0/entries/0/oracleRef",
            json!(ISLAND.to_uppercase()),
        ),
        (
            "/selected/seats/0/entries/0/oracleRef",
            json!(format!(" {ISLAND}")),
        ),
        (
            "/selected/seats/0/entries/0/oracleRef",
            json!("fixture:opaque"),
        ),
        ("/selected/seats/0/entries/0/quantity", json!("0")),
        ("/selected/seats/0/entries/0/quantity", json!("1")),
        ("/selected/seats/0/entries/0/entryId", json!(entry_id(0, 1))),
        ("/selected/seats/0/entryIds/0", json!(entry_id(1, 0))),
        (
            "/selected/seats/0/entries/0/printing/id",
            json!(Uuid::nil()),
        ),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = value;
        cases.push(bad);
    }
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("selected");
    cases.push(missing);
    let mut short = good.clone();
    short["selected"]["seats"][0]["entryIds"]
        .as_array_mut()
        .unwrap()
        .pop();
    cases.push(short);
    let mut missing_print = good.clone();
    missing_print["selected"]["seats"][0]["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("printing");
    cases.push(missing_print);
    for bad in cases {
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(read_root(&bytes, &expected(reference(&bytes)), source).is_err());
    }
    let mut duplicate = String::from_utf8(original.original_bytes().to_vec()).unwrap();
    duplicate = duplicate.replacen("\"selected\":", "\"selected\":{},\"selected\":", 1);
    assert!(
        read_root(
            duplicate.as_bytes(),
            &expected(reference(duplicate.as_bytes())),
            source
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn selected_origin_survives_actual_native_append_and_cold_checkpoint_replay() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let fixture = checkpoint_fixture_with_opening(opening(source, &r), None);
    assert_origins(&fixture.tip);
    assert_immutable_display(&fixture.tip, source);
    let initial_ref = reference(&fixture.original);
    let expected =
        NativeCheckpointExpected::new(fixture.tip.reference().clone(), fixture.tip.basis().clone())
            .unwrap()
            .with_original_state(fixture.states[0].0.clone())
            .unwrap();
    let mut steps = vec![];
    for (r, b) in fixture.states.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&initial_ref, &fixture.original));
    for (r, b) in &fixture.inputs {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let count = steps.len();
    let mock = Mock::new(steps);
    let restored = load_checkpoint(
        &checkpoint_store(&mock),
        &expected,
        &self::super::expected(initial_ref),
        source,
        NativeRecoveryBudget::new(10, 10_000_000).unwrap(),
    )
    .unwrap();
    assert_origins(&restored);
    assert_immutable_display(&restored, source);
    assert_incarnation_controller_and_token_mechanisms(&restored, source);
    assert_eq!(restored.candidate(), fixture.tip.candidate());
    assert!(restored.provenance().unwrap().matches_reserved(&r));
    assert_eq!(mock.finish().len(), count);
}

#[test]
#[ignore = "requires explicitly owned real private Storage and genuine retained source"]
fn owned_native_selected_checkpoint_chain_recovery() {
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let store = NativeArtifactStore::from_env().unwrap();
    let fixture = checkpoint_fixture_with_opening(opening(source, &r), Some(&store));
    let initial_ref = reference(&fixture.original);
    for (index, candidate) in fixture.candidates.iter().enumerate() {
        let native = candidate.state().basis();
        let basis = crate::player_wire::PlayerBasis {
            game_id: ids().game_id,
            context_id: ids().context_id,
            epoch: native.epoch.into(),
            revision: native.revision.into(),
        };
        let expected = NativeCheckpointExpected::new(fixture.states[index].0.clone(), basis)
            .unwrap()
            .with_original_state(fixture.states[0].0.clone())
            .unwrap();
        let restored = load_checkpoint(
            &store,
            &expected,
            &self::super::expected(initial_ref.clone()),
            source,
            NativeRecoveryBudget::new(100, 10_000_000).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.candidate(), candidate);
        assert_origins(&restored);
        assert_incarnation_controller_and_token_mechanisms(&restored, source);
        assert!(restored.provenance().unwrap().matches_reserved(&r));
    }
    println!(
        "OWNED_NATIVE_SELECTED_CHECKPOINT_OK {}",
        json!({
            "stateManifests":fixture.states.len(),"orderedInputRecords":fixture.inputs.len(),
            "sameActualCandidate":true,"verifiedTips":fixture.candidates.len(),
            "selectedPrintingProvenance":true,"physicalOccurrences":42,
            "fullyQualifiedCards":0,"deckFormatAdmissionQualified":false,
            "selectedMetadataFixture":true
        })
    );
}

fn assert_different_rules_copy_mechanism(provenance: &NativeInitialProvenance) {
    // Separate native mechanism fixture: this is not an admitted source-card
    // trajectory or authority to use another game's physical IDs in production.
    use crate::native_game::tests::fixture;
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::ResolvedDuration,
        game::GameCommand,
        mana::{ManaCost, ManaPayment},
        program::{BodyTarget, ExecutableBody, Instruction, Program},
        targets::TargetRef,
    };
    let donor = Program::land(
        BTreeSet::from([spellwake_engine::land::LandType::Forest]),
        vec![],
    );
    let copier = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_PERMANENT,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::become_copy(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            spellwake_engine::program::EffectSubject::AbilitySource,
                        )),
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            spellwake_engine::program::MoveSubjectRef::TargetObject,
                        ),
                        ResolvedDuration::NoStatedDuration,
                        spellwake_engine::program::CopyExceptions::default(),
                    ),
                ))],
            ),
        }],
    );
    let state = fixture::main_state(vec![
        fixture::card(
            1,
            fixture::A,
            ObjectDestination::Battlefield {
                controller: fixture::A,
            },
            copier.clone(),
        ),
        fixture::card(
            2,
            fixture::A,
            ObjectDestination::Battlefield {
                controller: fixture::A,
            },
            donor.clone(),
        ),
    ]);
    let receiver = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let target = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    let advance = |state: &GameState, actor, command| {
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state()
    };
    let activated = advance(
        &state,
        fixture::A,
        GameCommand::ActivateAbility {
            source: receiver,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(target),
            payment: Some(ManaPayment::default()),
        },
    );
    let passed = advance(&activated, fixture::A, GameCommand::Pass);
    let copied = advance(&passed, fixture::B, GameCommand::Pass);
    assert_ne!(copier, donor);
    assert_eq!(copied.execution().copiable_program(receiver), Some(&donor));
    let physical = copied.execution().window().cards().get(receiver).unwrap();
    assert_eq!(physical.card_id(), Some(CardId(1)));
    assert_eq!(
        provenance.physical_card(physical).unwrap().printing_id(),
        Some(printing_id(0, 0))
    );
    assert_ne!(
        provenance.physical_card(physical).unwrap().printing_id(),
        None
    );
}

fn successor_value() -> Value {
    let mut value = reserved_value();
    value["data"]["gameIndex"] = json!("42");
    value["data"]["structure"] = json!("bo3");
    for (n, seat) in value["data"]["seats"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        seat["selectedRevisionId"] = json!(Uuid::from_u128(0xee00 + n as u128));
        seat["selectionDecisionId"] = json!(Uuid::from_u128(0xef00 + n as u128));
        seat["selectionReplyRef"] = json!("fixture:original-next-game-selection");
    }
    value
}
fn acceptance(
    checkpoint: &NativeCheckpoint,
    r: &NativeReservedGameSource,
) -> crate::admission_ports::AcceptReservedGame {
    use crate::admission_ports::{AcceptReservedGame, OpeningArtifacts};
    let fact = checkpoint.prepare_results().unwrap().reference().clone();
    let event = checkpoint.prepare_event().unwrap().reference().clone();
    AcceptReservedGame {match_id:r.metadata().match_id,expected_revision:r.metadata().match_revision.clone(),game_id:ids().game_id,
        owner:"fixture:selected-host".parse().unwrap(),original_accepted_basis:Some(r.metadata().match_revision.clone()),
        opening:OpeningArtifacts::from_bytes(&serde_json::to_vec(&json!({"eventSchemaRevision":"native-transition-v1","clockObservation":checkpoint.opening_clock_observation().unwrap(),"contextId":ids().context_id,"snapshotRef":checkpoint.initial_reference(),"contextRevision":"fixture:v2-context","stateRef":checkpoint.reference(),"factsRef":fact,"eventRef":event,"qualificationRef":"fixture:unqualified-admission"})).unwrap()).unwrap()}
}
fn opening_disposition(
    command: &crate::admission_ports::AcceptReservedGame,
    checkpoint: &NativeCheckpoint,
    source: &NativeProgramSource,
    response: Option<Value>,
) -> Step {
    let original = NativeSystemOriginal::prepare_opening(command, checkpoint, source).unwrap();
    let mut step = Step::json(
        "/rest/v1/rpc/read_native_system_disposition",
        json!({"status":"success","data":{"state":if response.is_some(){"accepted"}else{"unknown"},"response":response}}),
    );
    step.expect=Some(serde_json::to_vec(&json!({"p_game":command.game_id,"p_request":command.game_id,"p_original":serde_json::from_str::<Value>(original.original_json()).unwrap()})).unwrap());
    step
}
fn native_repo(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "selected-acceptance-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary; RPC acknowledgment is synthetic"]
fn selected_opening_acceptance_checks_first_and_later_game_sources_before_rpc() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    for value in [reserved_value(), successor_value()] {
        let r = reservation(value.clone());
        let checkpoint = NativeCheckpoint::initial(opening(source, &r)).unwrap();
        let command = acceptance(&checkpoint, &r);
        let mut step = Step::json(
            "/rest/v1/rpc/accept_native_opening",
            json!({"status":"success","data":{"resourceId":ids().game_id,"contextId":ids().context_id,"revision":"7"}}),
        );
        step.expect=Some(serde_json::to_vec(&json!({"p_match":r.metadata().match_id,"p_basis":r.metadata().match_revision,"p_game":ids().game_id,"p_owner":"fixture:selected-host","p_opening":serde_json::from_str::<Value>(command.opening.original_json()).unwrap(),"p_native_epoch":checkpoint.basis().epoch,"p_runtime_id":native_runtime_identity(),"p_program_source_id":source.identity().hex()})).unwrap());
        let mut steps = vec![opening_disposition(&command, &checkpoint, source, None)];
        let event = checkpoint.prepare_event().unwrap();
        let results = checkpoint.prepare_results().unwrap();
        steps.extend(checkpoint_read_steps(
            event.reference(),
            event.original_bytes(),
        ));
        steps.extend(checkpoint_read_steps(
            results.reference(),
            results.original_bytes(),
        ));
        steps.push(step);
        let mock = Mock::new(steps);
        let ack = native_repo(&mock)
            .accept_opening(
                &checkpoint_store(&mock),
                &r,
                &command,
                &checkpoint,
                source,
                &NativeSystemOriginal::prepare_opening(&command, &checkpoint, source).unwrap(),
            )
            .unwrap();
        assert_eq!(ack.resource_id, ids().game_id);
        assert_eq!(mock.finish().len(), 6);
        let mut changes = vec![
            ("/data/seats/0/entries/0/entryId", json!(Uuid::new_v4())),
            ("/data/seats/0/entries/0/oracleRef", json!(FOREST)),
            (
                "/data/seats/0/originalSnapshotRef",
                json!("fixture:changed-original"),
            ),
            (
                "/data/seats/0/entries/0/definitionId",
                json!(Uuid::new_v4()),
            ),
            (
                "/data/seats/0/entries/0/definitionRevisionId",
                json!(Uuid::new_v4()),
            ),
            (
                "/data/seats/0/entries/0/definitionRevisionRef",
                json!("fixture:changed-definition"),
            ),
            (
                "/data/seats/0/entries/0/rulesTextRef",
                json!("fixture:changed-rules"),
            ),
            (
                "/data/seats/0/entries/0/programRef",
                json!("fixture:changed-program"),
            ),
            ("/data/seats/0/entries/0/unlimitedBasicLand", json!(false)),
            ("/data/seats/0/entries/0/section", json!("sideboard")),
            ("/data/seats/0/entries/0/quantity", json!("99")),
            ("/data/seats/0/entries/0/printing/id", json!(Uuid::new_v4())),
            ("/data/seats/0/entries/0/printing", Value::Null),
            (
                "/data/seats/0/entries/0/printing/externalPrintingRef",
                json!("fixture:other-printing"),
            ),
            ("/data/seats/0/entries/0/printing/finish", json!("etched")),
            (
                "/data/seats/0/entries/0/printing/setId",
                json!(Uuid::new_v4()),
            ),
        ];
        if !r.metadata().game_index.is_zero() {
            changes.extend([
                ("/data/seats/0/selectedRevisionId", json!(Uuid::new_v4())),
                ("/data/seats/0/selectionDecisionId", json!(Uuid::new_v4())),
                (
                    "/data/seats/0/selectionReplyRef",
                    json!("fixture:other-reply"),
                ),
                ("/data/seats/0/originalRevisionId", json!(Uuid::new_v4())),
            ]);
        }
        let before = checkpoint.state().clone();
        for (path, change) in changes {
            let mut changed = value.clone();
            *changed.pointer_mut(path).unwrap() = change;
            let changed = reservation(changed);
            let mock = Mock::new(vec![opening_disposition(
                &command,
                &checkpoint,
                source,
                None,
            )]);
            assert!(
                native_repo(&mock)
                    .accept_opening(
                        &checkpoint_store(&mock),
                        &changed,
                        &command,
                        &checkpoint,
                        source,
                        &NativeSystemOriginal::prepare_opening(&command, &checkpoint, source)
                            .unwrap()
                    )
                    .is_err()
            );
            assert_eq!(mock.finish().len(), 1);
            assert_eq!(checkpoint.state(), &before);
            assert_origins(&checkpoint);
        }
        assert_different_rules_copy_mechanism(checkpoint.provenance().unwrap());
    }
}

#[test]
#[ignore = "requires genuine retained catalogue/vocabulary"]
fn selected_strict_raw_shapes_canonical_occurrences_and_prepare_limits() {
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let original = opening(source, &r);
    let good: Value = serde_json::from_slice(original.original_bytes()).unwrap();
    let mut cases = vec![];
    for field in ["selectionDecisionId", "selectionReplyRef"] {
        let mut missing = good.clone();
        missing["selected"]["seats"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        cases.push(missing);
    }
    for value in [
        json!(entry_id(0, 0).to_string().to_uppercase()),
        json!(entry_id(0, 0).simple().to_string()),
        json!(format!("urn:uuid:{}", entry_id(0, 0))),
        json!(format!("{{{}}}", entry_id(0, 0))),
        Value::Null,
        json!(7),
        json!(Uuid::nil()),
    ] {
        let mut bad = good.clone();
        bad["selected"]["seats"][0]["entryIds"][0] = value;
        cases.push(bad);
    }
    for path in [
        "/selected",
        "/selected/seats/0",
        "/selected/seats/0/entries/0",
        "/selected/seats/0/entries/0/printing",
    ] {
        let mut bad = good.clone();
        bad.pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unrecognized".into(), json!(true));
        cases.push(bad);
        let mut positional = good.clone();
        let fields = positional
            .pointer(path)
            .unwrap()
            .as_object()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        *positional.pointer_mut(path).unwrap() = json!(fields);
        cases.push(positional);
    }
    for quantity in [
        "1001",
        "999999999999999999999999999999999999999999999999999999999999999999999999999999999",
    ] {
        let mut bad = good.clone();
        bad["selected"]["seats"][0]["entries"][0]["quantity"] = json!(quantity);
        cases.push(bad);
    }
    let mut extra = good.clone();
    extra["selected"]["seats"][0]["entryIds"]
        .as_array_mut()
        .unwrap()
        .push(json!(entry_id(0, 0)));
    cases.push(extra);
    let mut extra = good.clone();
    let seat = extra["selected"]["seats"][0].clone();
    extra["selected"]["seats"]
        .as_array_mut()
        .unwrap()
        .push(seat);
    cases.push(extra);
    for bad in cases {
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(read_root(&bytes, &expected(reference(&bytes)), source).is_err());
    }
    for key in [
        "matchId",
        "originalRevisionId",
        "entryId",
        "externalPrintingRef",
    ] {
        let raw = String::from_utf8(original.original_bytes().to_vec()).unwrap();
        let bad = raw.replacen(
            &format!("\"{key}\":"),
            &format!("\"{key}\":null,\"{key}\":"),
            1,
        );
        assert!(read_root(bad.as_bytes(), &expected(reference(bad.as_bytes())), source).is_err());
    }
    for case in 0..7 {
        let mut assignment = selection();
        match case {
            0 => {
                assignment.pop();
            }
            1 => assignment.push(NativeInitialSelection {
                seat_id: Uuid::new_v4(),
                entry_ids: vec![],
            }),
            2 => assignment.swap(0, 1),
            3 => assignment[0].entry_ids.clear(),
            4 => assignment[0].entry_ids.push(entry_id(0, 0)),
            5 => assignment[0].entry_ids[0] = Uuid::nil(),
            _ => assignment[0].entry_ids[0] = entry_id(1, 0),
        }
        assert!(
            prepare_selected_initial(ids().context_id, source, &r, spec(), assignment).is_err()
        );
    }
    for oracle in [
        FOREST.to_owned(),
        ISLAND.to_uppercase(),
        format!(" {ISLAND}"),
        "fixture:opaque".into(),
    ] {
        let mut value = reserved_value();
        value["data"]["seats"][0]["entries"][0]["oracleRef"] = json!(oracle);
        assert!(
            prepare_selected_initial(
                ids().context_id,
                source,
                &reservation(value),
                spec(),
                selection()
            )
            .is_err()
        );
    }
    let mut too_few = reserved_value();
    too_few["data"]["seats"][0]["entries"][0]["quantity"] = json!("1");
    assert!(
        prepare_selected_initial(
            ids().context_id,
            source,
            &reservation(too_few),
            spec(),
            selection()
        )
        .is_err()
    );
    assert!(prepare_selected_initial(Uuid::nil(), source, &r, spec(), selection()).is_err());
    let mut oversized = reserved_value();
    oversized["data"]["seats"][0]["entries"][0]["definitionRevisionRef"] =
        json!(format!("{}fixture:body", " ".repeat(1_048_576)));
    assert!(matches!(
        prepare_selected_initial(
            ids().context_id,
            source,
            &reservation(oversized),
            spec(),
            selection()
        ),
        Err(NativeInitialError::Size)
    ));
}

#[test]
#[ignore = "requires genuine catalogue/vocabulary; authoritative original RPC responses are mocked"]
fn bo3_historical_opening_retry_defers_persistence_identity_to_sql_without_adding_observation() {
    use crate::admission_ports::OpeningArtifacts;
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let mut value = reserved_value();
    value["data"]["structure"] = json!("bo3");
    let r = reservation(value);
    let checkpoint = NativeCheckpoint::initial(opening(source, &r)).unwrap();
    let mut legacy = acceptance(&checkpoint, &r);
    let mut legacy_value: Value = serde_json::from_str(legacy.opening.original_json()).unwrap();
    legacy_value
        .as_object_mut()
        .unwrap()
        .remove("eventSchemaRevision");
    legacy_value
        .as_object_mut()
        .unwrap()
        .remove("clockObservation");
    legacy.opening =
        OpeningArtifacts::from_bytes(&serde_json::to_vec(&legacy_value).unwrap()).unwrap();
    assert!(
        serde_json::from_str::<Value>(legacy.opening.original_json())
            .unwrap()
            .get("clockObservation")
            .is_none()
    );
    // Original recovery performs no Storage/native admission preflight and never
    // rewrites the legacy opening with a marker or observation.
    let accepted = opening_disposition(
        &legacy,
        &checkpoint,
        source,
        Some(
            json!({"status":"success","data":{"resourceId":ids().game_id,"contextId":ids().context_id,"revision":"7"}}),
        ),
    );
    let mock = Mock::new(vec![accepted]);
    assert!(
        native_repo(&mock)
            .accept_opening(
                &checkpoint_store(&mock),
                &r,
                &legacy,
                &checkpoint,
                source,
                &NativeSystemOriginal::prepare_opening(&legacy, &checkpoint, source).unwrap()
            )
            .is_ok()
    );
    assert_eq!(mock.finish().len(), 1);
    // A saved original from a different historical runtime/package still returns
    // its stored acknowledgment before checking current compiled native identity.
    let mut historical: Value = serde_json::from_str(
        NativeSystemOriginal::prepare_opening(&legacy, &checkpoint, source)
            .unwrap()
            .original_json(),
    )
    .unwrap();
    historical["runtimeId"] = json!("1".repeat(64));
    historical["programSourceId"] = json!("2".repeat(64));
    let retained =
        NativeSystemOriginal::from_bytes(ids().game_id, &serde_json::to_vec(&historical).unwrap())
            .unwrap();
    let mut step = Step::json(
        "/rest/v1/rpc/read_native_system_disposition",
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":ids().game_id,"contextId":ids().context_id,"revision":"7"}}}}),
    );
    step.expect = Some(
        serde_json::to_vec(
            &json!({"p_game":ids().game_id,"p_request":ids().game_id,"p_original":historical}),
        )
        .unwrap(),
    );
    let mock = Mock::new(vec![step]);
    assert!(
        native_repo(&mock)
            .accept_opening(
                &checkpoint_store(&mock),
                &r,
                &legacy,
                &checkpoint,
                source,
                &retained
            )
            .is_ok()
    );
    assert_eq!(mock.finish().len(), 1);
    // Unknown legacy terms are not eligible for a new native acceptance. Rejection
    // happens before private bytes or a committing RPC, without any fallback.
    let mock = Mock::new(vec![opening_disposition(
        &legacy,
        &checkpoint,
        source,
        None,
    )]);
    assert!(
        native_repo(&mock)
            .accept_opening(
                &checkpoint_store(&mock),
                &r,
                &legacy,
                &checkpoint,
                source,
                &NativeSystemOriginal::prepare_opening(&legacy, &checkpoint, source).unwrap()
            )
            .is_err()
    );
    assert_eq!(mock.finish().len(), 1);
    let mut present: Value = serde_json::from_str(legacy.opening.original_json()).unwrap();
    present["clockObservation"] =
        serde_json::to_value(checkpoint.opening_clock_observation().unwrap()).unwrap();
    let chooser = present["clockObservation"]["seatId"].clone();
    present["clockObservation"]["seatId"] = json!(
        seats()
            .into_iter()
            .find(|seat| json!(seat) != chooser)
            .unwrap()
    );
    let mut changed = acceptance(&checkpoint, &r);
    changed.opening = OpeningArtifacts::from_bytes(&serde_json::to_vec(&present).unwrap()).unwrap();
    let mock = Mock::new(vec![opening_disposition(
        &changed,
        &checkpoint,
        source,
        None,
    )]);
    assert!(
        native_repo(&mock)
            .accept_opening(
                &checkpoint_store(&mock),
                &r,
                &changed,
                &checkpoint,
                source,
                &NativeSystemOriginal::prepare_opening(&changed, &checkpoint, source).unwrap()
            )
            .is_err()
    );
    assert_eq!(mock.finish().len(), 1);
}

#[test]
#[ignore = "requires genuine catalogue/vocabulary; exact fresh opening evidence with mocked SQL/Storage"]
fn matching_original_opening_rejects_event_and_results_substitutions_before_commit() {
    use crate::admission_ports::OpeningArtifacts;
    let source = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let r = reservation(reserved_value());
    let checkpoint = NativeCheckpoint::initial(opening(source, &r)).unwrap();
    let successor = checkpoint_fixture_with_opening(opening(source, &r), None).tip;
    assert!(successor.basis().revision.get() > 0);
    let mut cases = vec![];
    for (field, reference) in [
        (
            "eventRef",
            successor.prepare_event().unwrap().reference().clone(),
        ),
        (
            "eventRef",
            NativeArtifactRef::for_bytes(
                ids().game_id,
                ids().context_id,
                NativeArtifactRole::Event,
                b"wrong-opening-event",
            )
            .unwrap(),
        ),
        (
            "factsRef",
            NativeArtifactRef::for_bytes(
                ids().game_id,
                ids().context_id,
                NativeArtifactRole::Facts,
                b"wrong-opening-results",
            )
            .unwrap(),
        ),
    ] {
        let mut value: Value =
            serde_json::from_str(acceptance(&checkpoint, &r).opening.original_json()).unwrap();
        value[field] = json!(reference);
        cases.push(value);
    }
    let mut value: Value =
        serde_json::from_str(acceptance(&checkpoint, &r).opening.original_json()).unwrap();
    value.as_object_mut().unwrap().remove("eventSchemaRevision");
    cases.push(value);
    for value in cases {
        let mut command = acceptance(&checkpoint, &r);
        command.opening =
            OpeningArtifacts::from_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        let original =
            NativeSystemOriginal::prepare_opening(&command, &checkpoint, source).unwrap();
        let mock = Mock::new(vec![opening_disposition(
            &command,
            &checkpoint,
            source,
            None,
        )]);
        assert!(
            native_repo(&mock)
                .accept_opening(
                    &checkpoint_store(&mock),
                    &r,
                    &command,
                    &checkpoint,
                    source,
                    &original
                )
                .is_err()
        );
        // Exactly the read-only original lookup; no Storage or acceptance call.
        assert_eq!(mock.finish().len(), 1);
    }
    let command = acceptance(&checkpoint, &r);
    let original = NativeSystemOriginal::prepare_opening(&command, &checkpoint, source).unwrap();
    let event = checkpoint.prepare_event().unwrap();
    let results = checkpoint.prepare_results().unwrap();
    for corrupt_results in [false, true] {
        let mut steps = vec![opening_disposition(&command, &checkpoint, source, None)];
        let mut altered = if corrupt_results {
            results.original_bytes().to_vec()
        } else {
            event.original_bytes().to_vec()
        };
        altered[0] ^= 1;
        if corrupt_results {
            steps.extend(checkpoint_read_steps(
                event.reference(),
                event.original_bytes(),
            ));
            steps.extend(checkpoint_read_steps(results.reference(), &altered));
        } else {
            steps.extend(checkpoint_read_steps(event.reference(), &altered));
        }
        let mock = Mock::new(steps);
        assert!(
            native_repo(&mock)
                .accept_opening(
                    &checkpoint_store(&mock),
                    &r,
                    &command,
                    &checkpoint,
                    source,
                    &original
                )
                .is_err()
        );
        assert_eq!(mock.finish().len(), if corrupt_results { 5 } else { 3 });
    }
}
