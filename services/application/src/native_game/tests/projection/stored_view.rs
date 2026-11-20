//! Registry/projection integrity over genuine vocabulary and literal display data.
//! The loopback Store proves byte mechanics, not real Storage/Auth or SQL issuance.
use super::*;
use crate::{
    backend::{BackendClient, BackendConfig},
    test_http::{Mock, Step},
};
use std::time::Duration;

fn source() -> GameState {
    main_state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::land(BTreeSet::from([LandType::Island]), vec![]),
        ),
        card(
            2,
            B,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
    ])
}
fn restore<'a>(
    source: &'a GameState,
    viewer: Uuid,
    disclosure: NativeDisclosure,
    bytes: &[u8],
    display: &Display,
) -> Result<RestoredNativeView<'a>, NativeViewError> {
    let basis = scope(source, viewer).basis().clone();
    let reference = NativeArtifactRef::for_bytes(
        basis.game_id,
        basis.context_id,
        NativeArtifactRole::View,
        bytes,
    )
    .unwrap();
    let expected = NativeViewExpected::new(
        reference.clone(),
        basis,
        viewer,
        disclosure,
        Amount::from(source.basis().revision),
    )?;
    let mock = Mock::new(vec![
        Step::get(
            "/storage/v1/bucket/native-evidence".into(),
            200,
            br#"{"id":"native-evidence","public":false}"#.to_vec(),
        ),
        Step::get(
            format!(
                "/storage/v1/object/authenticated/native-evidence/{}",
                reference.as_str()
            ),
            200,
            bytes.to_vec(),
        ),
    ]);
    let store = NativeArtifactStore::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "stored-view-test",
            Duration::from_secs(2),
        )
        .unwrap(),
        "native-evidence".into(),
    )
    .unwrap();
    let result = load_view(&store, &expected, source, binding(source, viewer), display);
    assert_eq!(mock.finish().len(), 2);
    result
}
#[test]
#[ignore = "requires genuine vocabulary; stored registry composition, not source qualification"]
fn stored_view_preserves_snapshot_and_original_handles_for_proposal_and_input_replay() {
    let source = source();
    let before = source.clone();
    let display = Display::default();
    let prepared =
        prepare_view(&source, binding(&source, viewer_a()), &metadata(), &display).unwrap();
    let another =
        prepare_view(&source, binding(&source, viewer_a()), &metadata(), &display).unwrap();
    assert!(prepared.reference() != another.reference());
    let restored = restore(
        &source,
        viewer_a(),
        NativeDisclosure::LiveParticipant,
        prepared.original_bytes(),
        &display,
    )
    .unwrap();
    assert!(restored.snapshot() == prepared.snapshot());
    restored
        .snapshot()
        .validate(&restored.projection_facts().unwrap())
        .unwrap();
    assert!(restored.reference() == prepared.reference());
    let land = restored
        .snapshot()
        .visible_objects
        .iter()
        .find(|object| object.owner_seat_id == viewer_a())
        .unwrap()
        .handle
        .clone();
    let proposed = restored
        .scope()
        .propose(&command(
            restored.scope(),
            PlayerAction::PlayLand { source: land },
        ))
        .unwrap();
    let direct = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor: A,
            command: GameCommand::PlayLand {
                land: object(&source, 1),
            },
        })
        .unwrap();
    assert_eq!(proposed.candidate(), &direct);
    let retained = restored.scope().encode_input(&proposed).unwrap();
    let replay = restore_input(&source, binding(&source, viewer_a()), &retained).unwrap();
    assert_eq!(replay.candidate(), &direct);
    assert!(
        restore(
            &source,
            viewer_b(),
            NativeDisclosure::LiveParticipant,
            prepared.original_bytes(),
            &display
        )
        .is_err()
    );
    assert!(
        restore(
            &source,
            viewer_a(),
            NativeDisclosure::CompletedParticipant,
            prepared.original_bytes(),
            &display
        )
        .is_err()
    );
    assert_eq!(source, before);
    let terminal = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor: B,
            command: GameCommand::Concede,
        })
        .unwrap()
        .into_state();
    let mut completed = metadata();
    completed.disclosure = NativeDisclosure::CompletedParticipant;
    completed.through_event_sequence = Amount::from(terminal.basis().revision);
    let full = prepare_view(
        &terminal,
        binding(&terminal, viewer_a()),
        &completed,
        &display,
    )
    .unwrap();
    let loaded = restore(
        &terminal,
        viewer_a(),
        NativeDisclosure::CompletedParticipant,
        full.original_bytes(),
        &display,
    )
    .unwrap();
    assert!(loaded.snapshot() == full.snapshot());
    assert_eq!(loaded.snapshot().visible_objects.len(), 2);
    assert!(
        restore(
            &terminal,
            viewer_a(),
            NativeDisclosure::LiveParticipant,
            full.original_bytes(),
            &display
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires genuine vocabulary; stored view rejection composition only"]
fn stored_view_rejects_missing_alias_extra_hidden_and_altered_public_bindings() {
    let source = source();
    let before = source.clone();
    let display = Display::default();
    let prepared =
        prepare_view(&source, binding(&source, viewer_a()), &metadata(), &display).unwrap();
    let good: serde_json::Value = serde_json::from_slice(prepared.original_bytes()).unwrap();
    let mut cases = vec![];
    for key in [
        "version",
        "source",
        "viewerSeatId",
        "disclosure",
        "deadlineAt",
        "references",
        "snapshot",
    ] {
        let mut case = good.clone();
        case.as_object_mut().unwrap().remove(key);
        cases.push(case);
    }
    for (pointer, value) in [
        ("/version", serde_json::json!("native-view-artifact-v2")),
        ("/source/context/programs", serde_json::json!("foreign")),
        ("/snapshot/players/0/life", serde_json::json!("999")),
        ("/snapshot/throughEventSequence", serde_json::json!("1")),
        (
            "/references/0/handle",
            serde_json::json!("opaque-non-native-handle"),
        ),
    ] {
        let mut case = good.clone();
        *case.pointer_mut(pointer).unwrap() = value;
        cases.push(case);
    }
    let mut missing = good.clone();
    missing["references"].as_array_mut().unwrap().remove(0);
    cases.push(missing);
    let mut duplicate = good.clone();
    duplicate["references"]
        .as_array_mut()
        .unwrap()
        .push(good["references"][0].clone());
    cases.push(duplicate);
    let mut alias = good.clone();
    let mut row = good["references"][0].clone();
    row["handle"] = serde_json::json!(format!("n1.{}", Uuid::new_v4()));
    alias["references"].as_array_mut().unwrap().push(row);
    cases.push(alias);
    let other = prepare_view(&source, binding(&source, viewer_b()), &metadata(), &display).unwrap();
    let other_record: serde_json::Value = serde_json::from_slice(other.original_bytes()).unwrap();
    let hidden_handle = other
        .snapshot()
        .visible_objects
        .iter()
        .find(|object| object.owner_seat_id == viewer_b())
        .unwrap()
        .handle
        .as_str();
    let hidden_entry = other_record["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["handle"] == hidden_handle)
        .unwrap()
        .clone();
    let mut extra_hidden = good.clone();
    extra_hidden["references"]
        .as_array_mut()
        .unwrap()
        .push(hidden_entry);
    assert!(matches!(
        restore(
            &source,
            viewer_a(),
            NativeDisclosure::LiveParticipant,
            &serde_json::to_vec(&extra_hidden).unwrap(),
            &display
        ),
        Err(NativeViewError::Invalid)
    ));
    cases.push(extra_hidden);
    let mut invalid_zone = good.clone();
    assert_eq!(invalid_zone["references"][0]["location"]["kind"], "zone");
    invalid_zone["references"][0]["location"]["zone"] = serde_json::json!("battlefield");
    cases.push(invalid_zone);
    let mut extra = good.clone();
    extra["extra"] = serde_json::json!(true);
    cases.push(extra);
    let mut oversized = good.clone();
    oversized["references"] = serde_json::json!(vec![
        good["references"][0].clone();
        MAX_PLAYER_WIRE_BYTES / 39 + 1
    ]);
    cases.push(oversized);
    for case in cases {
        assert!(
            restore(
                &source,
                viewer_a(),
                NativeDisclosure::LiveParticipant,
                &serde_json::to_vec(&case).unwrap(),
                &display
            )
            .is_err()
        );
        assert_eq!(source, before);
    }
    let raw = std::str::from_utf8(prepared.original_bytes()).unwrap();
    let repeated = raw.replacen(
        "\"deadlineAt\":null",
        "\"deadlineAt\":null,\"deadlineAt\":null",
        1,
    );
    assert_ne!(raw, repeated);
    assert!(
        restore(
            &source,
            viewer_a(),
            NativeDisclosure::LiveParticipant,
            repeated.as_bytes(),
            &display
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires genuine vocabulary; passive ability registry composition only"]
fn stored_view_restores_triggered_and_continuous_descriptors_without_activating_them() {
    use spellwake_engine::{
        characteristics::{
            AbilityChange, KeywordAbility, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts,
        },
        program::{
            ObjectRelation, OrdinaryCondition, TriggerGrouping, TriggeredAbility,
            TriggeredAbilityId,
        },
    };
    let mut creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    creature.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(9),
        selector: PermanentSelector::default(),
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::Haste],
        ))]),
    });
    creature.triggered = vec![TriggeredAbility {
        id: TriggeredAbilityId(8),
        condition: OrdinaryCondition::BecomesTapped(ObjectRelation::SelfObject),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    }];
    let source = main_state(vec![card(
        1,
        A,
        ObjectDestination::Battlefield { controller: A },
        creature,
    )]);
    let display = Display::default();
    let prepared =
        prepare_view(&source, binding(&source, viewer_a()), &metadata(), &display).unwrap();
    let loaded = restore(
        &source,
        viewer_a(),
        NativeDisclosure::LiveParticipant,
        prepared.original_bytes(),
        &display,
    )
    .unwrap();
    assert!(loaded.snapshot() == prepared.snapshot());
    let object = &loaded.snapshot().visible_objects[0];
    assert_eq!(object.abilities.len(), 2);
    for ability in &object.abilities {
        assert!(matches!(
            loaded.scope().propose(&command(
                loaded.scope(),
                PlayerAction::ActivateAbility {
                    source: object.handle.clone(),
                    ability: ability.handle.clone(),
                    target: PlayerTarget::Seat {
                        seat_id: viewer_b()
                    },
                    payment: None
                }
            )),
            Err(NativeBridgeError::WrongReferenceKind)
        ));
    }
}
