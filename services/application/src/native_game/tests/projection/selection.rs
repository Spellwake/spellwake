//! Actual selected-action replacements and held orders over normalized originals.
use super::chooser_replacement::{bound, four_seat_root, mechanism_step};
use super::*;
use spellwake_engine::{
    effects::{PendingWorkKind, ZoneOptionalAnswer},
    program::{
        AffectedPlayers, InstructionAmount, PublicZoneDestination, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn prepare() -> GameState {
    let mut replacement = Program::enchantment(None);
    replacement.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    let mut cards = vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(2.into()),
                        },
                        Instruction::Discard {
                            players: AffectedPlayers::EachPlayer,
                            amount: InstructionAmount::Literal(2.into()),
                        },
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        },
                    ],
                ),
            ),
        ),
        card(
            2,
            D,
            ObjectDestination::Battlefield { controller: D },
            replacement,
        ),
    ];
    for (base, owner) in [(10, A), (20, B), (30, C), (40, D)] {
        for id in base..base + 2 {
            cards.push(card(
                id,
                owner,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let mut state = four_seat_root(cards);
    state = mechanism_step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    );
    for actor in [A, B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    state
}
fn viewer(source: &GameState, actor: SeatId) -> Uuid {
    bound(source, viewer_a())
        .seats
        .iter()
        .find(|seat| seat.native_seat == actor)
        .unwrap()
        .seat_id
}
fn project(scope: &mut NativeScope<'_>) -> NativeProjectedSnapshot {
    let value = scope
        .project_with_facts(&metadata(), &Display::default())
        .unwrap();
    let facts = value
        .facts(scope.source.execution().creature_vocabulary())
        .unwrap();
    value.snapshot().validate(&facts).unwrap();
    let decoded: PlayerSnapshot =
        crate::decode_wire_object(&serde_json::to_vec(value.snapshot()).unwrap()).unwrap();
    decoded.validate(&facts).unwrap();
    capture(value.snapshot());
    value
}
fn reply(
    source: &GameState,
    kind: DecisionKind,
    build: impl FnOnce(&NativeScope<'_>, &PlayerDecision) -> PlayerChoice,
) -> GameState {
    let actor = source.pending_decision().unwrap().chooser();
    let app = viewer(source, actor);
    let mut scope = NativeScope::new(source, bound(source, app)).unwrap();
    let snapshot = project(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == kind);
    assert!(
        matches!(crate::native_game::clocks::waiting(source,&scope.binding).unwrap(),crate::native_game::clocks::Waiting::Decision(seat) if seat==app)
    );
    let response = build(&scope, descriptor);
    let command = command(
        &scope,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response,
        },
    );
    let roster = snapshot
        .snapshot()
        .players
        .iter()
        .map(|seat| seat.seat_id)
        .collect::<Vec<_>>();
    descriptor
        .validate_response(&command, app, &roster, &[])
        .unwrap();
    let prepared = scope.propose(&command).unwrap();
    input_codec::verify(&scope, &prepared);
    let bytes = scope.encode_input(&prepared).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for row in record["transition"]["references"].as_array().unwrap() {
        if matches!(
            row["location"]["kind"].as_str(),
            Some("zone-application" | "zone-optional")
        ) {
            let mut wrong = record.clone();
            let entry = wrong["transition"]["references"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|entry| entry["handle"] == row["handle"])
                .unwrap();
            entry["location"] = serde_json::json!({"kind":"library-card","index":"0"});
            assert!(
                restore_input(
                    source,
                    scope.binding.clone(),
                    &serde_json::to_vec(&wrong).unwrap()
                )
                .is_err()
            );
        }
    }
    if kind == DecisionKind::ZoneReplacement {
        let mut alternate = record.clone();
        let row = alternate["transition"]["references"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| {
                matches!(
                    row["location"]["kind"].as_str(),
                    Some("zone-application" | "zone-optional")
                )
            })
            .unwrap();
        if row["location"]["kind"] == "zone-application" {
            // This fixture offers two applications and chose index zero.
            assert_eq!(row["location"]["index"], "0");
            row["location"]["index"] = serde_json::json!("1");
        } else {
            row["location"]["apply"] =
                serde_json::json!(!row["location"]["apply"].as_bool().unwrap());
        }
        let changed = restore_input(
            source,
            scope.binding.clone(),
            &serde_json::to_vec(&alternate).unwrap(),
        )
        .unwrap();
        assert_ne!(changed.native_input(), prepared.native_input());
        assert_ne!(
            changed.candidate(),
            prepared.candidate(),
            "valid alternative binding changes actual native result"
        );
    }
    assert!(
        restore_input(
            prepared.candidate().state(),
            bound(prepared.candidate().state(), app),
            &bytes
        )
        .is_err()
    );
    if prepared
        .candidate()
        .state()
        .execution()
        .window()
        .entries()
        .is_empty()
    {
        use spellwake_engine::occurrence::{RulesOccurrence, RulesRecord, ZoneReason};
        let mut actual = prepared
            .candidate()
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::Event(event) => Some(event),
                _ => None,
            })
            .flat_map(|event| &event.occurrences)
            .filter_map(|occurrence| match &occurrence.value {
                RulesOccurrence::ZoneChange(change) => match change.reason {
                    ZoneReason::Discard { player } => {
                        Some((change.before.object.card_id().unwrap(), player))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        actual.sort();
        assert_eq!(
            actual,
            vec![
                (CardId(10), A),
                (CardId(11), A),
                (CardId(40), D),
                (CardId(41), D)
            ]
        );
    }
    // Another surviving player cannot use this chooser's private options or hand.
    let observer = if actor == A { C } else { A };
    let mut other = NativeScope::new(source, bound(source, viewer(source, observer))).unwrap();
    let hidden = project(&mut other);
    assert!(hidden.snapshot().decisions.is_empty());
    for visible in &hidden.snapshot().visible_objects {
        let NativeReference::Object(id) = other.reference(&visible.handle).unwrap() else {
            panic!("physical object")
        };
        if let ObjectZone::Hand(owner) = source.execution().window().cards().get(id).unwrap().zone()
        {
            assert_eq!(
                owner, observer,
                "another player's chosen hand stays private"
            );
        }
    }
    for (handle, reference) in &scope.references {
        if matches!(
            reference,
            NativeReference::ZoneApplication(_) | NativeReference::ZoneOptional(_)
        ) {
            assert!(other.reference(handle).is_err());
        }
    }
    assert!(other.propose(&command).is_err());
    prepared.candidate().state().clone()
}
fn select(source: &GameState) -> GameState {
    reply(source, DecisionKind::SelectResolution, |_, decision| {
        let PlayerDomain::Objects { objects, .. } = &decision.domain else {
            panic!("selected hand")
        };
        assert_eq!(objects.len(), 2);
        PlayerChoice::Objects {
            objects: objects.clone(),
        }
    })
}
fn decline(source: &GameState) -> GameState {
    let source = if matches!(source.pending_decision().unwrap().frame(),spellwake_engine::game::RuleFrame::ZoneReplacement(frame) if matches!(frame.request(),spellwake_engine::effects::ZoneReplacementRequest::Select{..}))
    {
        reply(source, DecisionKind::ZoneReplacement, |_, decision| {
            let PlayerDomain::Options { options, .. } = &decision.domain else {
                panic!()
            };
            PlayerChoice::Option {
                option: options[0].handle.clone(),
            }
        })
    } else {
        source.clone()
    };
    reply(&source, DecisionKind::ZoneReplacement, |scope, decision| {
        let PlayerDomain::Options { options, .. } = &decision.domain else {
            panic!()
        };
        PlayerChoice::Option {
            option: options
                .iter()
                .find(|option| {
                    scope.reference(&option.handle).unwrap()
                        == NativeReference::ZoneOptional(ZoneOptionalAnswer::Decline)
                })
                .unwrap()
                .handle
                .clone(),
        }
    })
}
fn order(source: &GameState) -> GameState {
    reply(
        source,
        DecisionKind::OrderResolutionGraveyard,
        |_, decision| {
            let PlayerDomain::Ordering { objects, .. } = &decision.domain else {
                panic!()
            };
            PlayerChoice::Ordering {
                ordered: objects.iter().rev().cloned().collect(),
            }
        },
    )
}
fn depart(source: &GameState, actor: SeatId) -> GameState {
    let scope = NativeScope::new(source, bound(source, viewer(source, actor))).unwrap();
    let prepared = scope
        .propose(&command(&scope, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    prepared.candidate().state().clone()
}
#[test]
#[ignore = "requires genuine vocabulary; selected-action departure originals and private hand choices"]
fn selected_discard_departure_originals_preserve_private_choices_replacements_and_owner_order() {
    let source = prepare();
    let source = select(&source);
    assert_eq!(source.pending_decision().unwrap().chooser(), B);
    let source = depart(&source, B);
    assert_eq!(source.pending_decision().unwrap().chooser(), C);
    let source = select(&source);
    let source = select(&source);
    assert_eq!(
        source.execution().pending_work_kind(),
        Some(PendingWorkKind::ResolutionSelection)
    );
    let mut source = source;
    for actor in [A, A, C, C, D, D] {
        assert_eq!(source.pending_decision().unwrap().chooser(), actor);
        source = decline(&source);
    }
    assert_eq!(source.pending_decision().unwrap().chooser(), A);
    source = order(&source);
    assert_eq!(source.pending_decision().unwrap().chooser(), C);
    source = depart(&source, C);
    assert_eq!(source.pending_decision().unwrap().chooser(), D);
    assert_eq!(source.execution().life_total(A), Some(&22.into()));
    source = order(&source);
    assert!(source.execution().window().entries().is_empty());
    assert_eq!(source.execution().life_total(A), Some(&25.into()));
    let cards = source.execution().window().cards();
    let ids = |owner| {
        cards
            .graveyard(owner)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(A), vec![CardId(1), CardId(11), CardId(10)]);
    assert_eq!(ids(D), vec![CardId(41), CardId(40)]);
    for id in [20, 21, 30, 31] {
        assert!(cards.current_identity(CardId(id)).is_none());
    }
    assert_eq!(source.execution().window().participants(), &[A, D]);
}
