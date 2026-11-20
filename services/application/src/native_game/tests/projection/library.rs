//! Private library choices and real normalized-original replay after departure.
use super::chooser_replacement::{bound, finish_original_stack, four_seat_root, mechanism_step};
use super::*;
use spellwake_engine::{
    effects::{LibraryActionKind, ZoneOptionalAnswer, ZoneReplacementRequest},
    game::RuleFrame,
    program::{
        InstructionAmount, PublicZoneDestination, ResolvingPlayer, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn prepare(
    kind: LibraryActionKind,
    recipient: SeatId,
    replacement: bool,
    flipped: bool,
    lower: bool,
) -> GameState {
    let player = if recipient == A {
        ResolvingPlayer::Controller
    } else {
        ResolvingPlayer::TargetPlayer
    };
    let amount = InstructionAmount::Literal(2.into());
    let instruction = match kind {
        LibraryActionKind::Scry => Instruction::Scry { player, amount },
        LibraryActionKind::Surveil => Instruction::Surveil { player, amount },
        LibraryActionKind::Mill => Instruction::Mill { player, amount },
    };
    let mut cards = vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                if recipient == A {
                    BodyTarget::None
                } else {
                    BodyTarget::SinglePlayer
                },
                vec![
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(2.into()),
                    },
                    instruction,
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    },
                ],
            ),
        ),
    )];
    for id in (10..13).rev() {
        cards.push(card(
            id,
            recipient,
            ObjectDestination::LibraryTop,
            if flipped {
                Program::creature(None, 7.into(), 8.into(), BTreeSet::new())
            } else {
                Program::artifact(None, vec![])
            },
        ));
    }
    if replacement {
        let mut source = Program::enchantment(None);
        source.zone_replacements = vec![ZoneReplacementAbility {
            id: ZoneReplacementAbilityId(0),
            actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::EffectMill,
                ZoneActionKind::EffectSurveil,
            ])),
            objects: ZoneObjectKind::Card,
            subject: ZoneReplacementSubject::AnyObject,
            destination: PublicZoneDestination::Graveyard,
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::OptionalAffectedPlayer,
        }];
        cards.push(card(
            200,
            B,
            ObjectDestination::Battlefield { controller: B },
            source,
        ));
    }
    if lower {
        cards.push(card(
            2,
            D,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(5.into()),
                    }],
                ),
            ),
        ));
    }
    let mut source = four_seat_root(cards);
    if lower {
        for actor in [A, B, C] {
            source = mechanism_step(&source, actor, GameCommand::Pass);
        }
        source = mechanism_step(
            &source,
            D,
            GameCommand::CastUntargeted {
                spell: object(&source, 2),
                payment: None,
            },
        );
        source = mechanism_step(&source, D, GameCommand::Pass);
    }
    source = mechanism_step(
        &source,
        A,
        if recipient == A {
            GameCommand::CastUntargeted {
                spell: object(&source, 1),
                payment: None,
            }
        } else {
            GameCommand::Cast {
                spell: object(&source, 1),
                target: TargetRef::Player(recipient),
            }
        },
    );
    for actor in [A, B, C, D] {
        source = mechanism_step(&source, actor, GameCommand::Pass);
    }
    source
}
fn project(view: &mut NativeScope<'_>, display: &Display) -> NativeProjectedSnapshot {
    let result = view.project_with_facts(&metadata(), display).unwrap();
    let facts = result
        .facts(view.source.execution().creature_vocabulary())
        .unwrap();
    result.snapshot().validate(&facts).unwrap();
    let decoded: PlayerSnapshot =
        crate::decode_wire_object(&serde_json::to_vec(result.snapshot()).unwrap()).unwrap();
    decoded.validate(&facts).unwrap();
    capture(result.snapshot());
    result
}
fn assert_private(view: &NativeScope<'_>, snapshot: &PlayerSnapshot, display: &Display) {
    assert!(snapshot.visible_objects.iter().all(|object| {
        let NativeReference::Object(id) = view.reference(&object.handle).unwrap() else {
            panic!("physical")
        };
        !matches!(
            view.source
                .execution()
                .window()
                .cards()
                .get(id)
                .unwrap()
                .zone(),
            ObjectZone::Library(_)
        )
    }));
    assert!(
        snapshot
            .zones
            .iter()
            .filter(|zone| matches!(
                zone,
                PlayerZone::Hidden {
                    zone_type: PlayerZoneKind::Library,
                    ..
                }
            ))
            .count()
            == 4
    );
    for card in [10, 11, 12] {
        let cards = view.source.execution().window().cards();
        let current = cards.current_identity(CardId(card)).unwrap();
        assert!(
            matches!(cards.get(current).unwrap().zone(), ObjectZone::Library(_)),
            "no partial library movement at an actual wait"
        );
    }
    if view.source.execution().pending_library_kind() == Some(LibraryActionKind::Mill) {
        assert!(display.calls.borrow().iter().all(|call|!matches!(call,DisplayCall::Object(object,_) if matches!(object.zone(),ObjectZone::Library(_)))));
    }
}
fn verify_receipts(candidate: &spellwake_engine::game::NativeCandidate) -> usize {
    use spellwake_engine::occurrence::{RulesOccurrence, RulesRecord};
    let mut count = 0;
    for record in &candidate.rules_journal().records {
        let RulesRecord::Event(event) = record else {
            continue;
        };
        for occurrence in &event.occurrences {
            let RulesOccurrence::LibraryAction(action) = &occurrence.value else {
                continue;
            };
            count += 1;
            assert_eq!(
                action.selected_before_top_first,
                action
                    .public_zone_moves
                    .iter()
                    .map(|movement| movement.before)
                    .collect::<Vec<_>>()
            );
            for movement in &action.public_zone_moves {
                let changes = event
                    .occurrences
                    .iter()
                    .filter_map(|occurrence| match &occurrence.value {
                        RulesOccurrence::ZoneChange(change)
                            if change.before.object.id() == movement.before =>
                        {
                            Some(change)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0].after.object.id(), movement.after);
                let expected = match movement.destination {
                    PublicZoneDestination::Graveyard => ObjectZone::Graveyard(action.player),
                    PublicZoneDestination::Exile => ObjectZone::Exile,
                };
                assert_eq!(
                    changes[0].before.object.zone(),
                    ObjectZone::Library(action.player)
                );
                assert_eq!(changes[0].after.object.zone(), expected);
                assert_eq!(
                    candidate
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .get(movement.after)
                        .unwrap()
                        .zone(),
                    expected
                );
            }
        }
    }
    count
}
fn depart(source: &GameState, viewer: Uuid) -> GameState {
    let view = NativeScope::new(source, bound(source, viewer)).unwrap();
    let prepared = view
        .propose(&command(&view, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&view, &prepared);
    let count = verify_receipts(prepared.candidate());
    let omitted = source
        .pending_decision()
        .is_some_and(|pending| pending.chooser() == view.binding.actor)
        || source
            .execution()
            .window()
            .resolving()
            .is_some_and(|entry| entry.controller() == view.binding.actor);
    let completed = !omitted
        && prepared
            .candidate()
            .state()
            .execution()
            .pending_library_kind()
            .is_none();
    assert_eq!(count, usize::from(completed));
    if source
        .pending_decision()
        .is_some_and(|pending| pending.chooser() == view.binding.actor)
    {
        assert!(prepared.candidate().rules_journal().records.iter().all(|record|!matches!(record,spellwake_engine::occurrence::RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence|matches!(occurrence.value,spellwake_engine::occurrence::RulesOccurrence::LibraryAction(_))))));
    }
    prepared.candidate().state().clone()
}
fn respond(
    source: &GameState,
    build: impl FnOnce(&NativeScope<'_>, &PlayerDecision) -> PlayerChoice,
) -> GameState {
    let viewer = source.pending_decision().unwrap().chooser();
    let b = bound(source, viewer_a());
    let app = b
        .seats
        .iter()
        .find(|seat| seat.native_seat == viewer)
        .unwrap()
        .seat_id;
    let mut view = NativeScope::new(source, bound(source, app)).unwrap();
    let display = Display::default();
    let projected = project(&mut view, &display);
    assert_private(&view, projected.snapshot(), &display);
    let descriptor = &projected.snapshot().decisions[0];
    assert!(
        matches!(crate::native_game::clocks::waiting(source,&view.binding).unwrap(),crate::native_game::clocks::Waiting::Decision(waiter) if waiter==app)
    );
    let choice = build(&view, descriptor);
    let command = command(
        &view,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: choice,
        },
    );
    let roster = projected
        .snapshot()
        .players
        .iter()
        .map(|seat| seat.seat_id)
        .collect::<Vec<_>>();
    descriptor
        .validate_response(&command, app, &roster, &[])
        .unwrap();
    let prepared = view.propose(&command).unwrap();
    input_codec::verify(&view, &prepared);
    let count = verify_receipts(prepared.candidate());
    assert_eq!(
        count,
        usize::from(
            prepared
                .candidate()
                .state()
                .execution()
                .pending_library_kind()
                .is_none()
        )
    );
    let bytes = view.encode_input(&prepared).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for row in record["transition"]["references"].as_array().unwrap() {
        let kind = row["location"]["kind"].as_str().unwrap();
        if ["library-card", "zone-application", "zone-optional"].contains(&kind) {
            for location in [
                serde_json::json!({"kind":"library-card","index":"999"}),
                serde_json::json!({"kind":"object","index":"0"}),
            ] {
                let mut wrong = record.clone();
                let entry = wrong["transition"]["references"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|entry| entry["handle"] == row["handle"])
                    .unwrap();
                entry["location"] = location;
                assert!(
                    restore_input(
                        source,
                        view.binding.clone(),
                        &serde_json::to_vec(&wrong).unwrap()
                    )
                    .is_err()
                );
            }
        }
    }
    if descriptor.kind == DecisionKind::Scry {
        let mut swapped = record.clone();
        let entries = swapped["transition"]["references"].as_array_mut().unwrap();
        let indices = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry["location"]["kind"] == "library-card")
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert_eq!(indices.len(), 2);
        let first = entries[indices[0]]["location"].clone();
        entries[indices[0]]["location"] = entries[indices[1]]["location"].clone();
        entries[indices[1]]["location"] = first;
        let changed = restore_input(
            source,
            view.binding.clone(),
            &serde_json::to_vec(&swapped).unwrap(),
        )
        .unwrap();
        assert_ne!(changed.native_input(), prepared.native_input());
        assert_ne!(
            changed.candidate(),
            prepared.candidate(),
            "valid alternative binding changes the actual partition result"
        );
    }
    let mut observer = NativeScope::new(source, bound(source, Uuid::from_u128(302))).unwrap();
    if app != Uuid::from_u128(302) {
        let display = Display::default();
        let hidden = project(&mut observer, &display);
        assert!(hidden.snapshot().decisions.is_empty());
        assert!(display.calls.borrow().iter().all(|call|!matches!(call,DisplayCall::Object(object,_) if matches!(object.zone(),ObjectZone::Library(_)))));
        for (handle, reference) in &view.references {
            if matches!(
                reference,
                NativeReference::LibraryCard(_)
                    | NativeReference::ZoneApplication(_)
                    | NativeReference::ZoneOptional(_)
            ) {
                assert!(observer.reference(handle).is_err());
            }
        }
    }
    prepared.candidate().state().clone()
}
fn partition(source: &GameState, all_other: bool) -> GameState {
    respond(source, |_, d| {
        assert!(matches!(d.kind, DecisionKind::Scry | DecisionKind::Surveil));
        let PlayerDomain::Ordering { objects, .. } = &d.domain else {
            panic!()
        };
        PlayerChoice::LibraryPartition {
            top: if all_other {
                vec![]
            } else {
                vec![objects[1].clone()]
            },
            other: if all_other {
                objects.iter().rev().cloned().collect()
            } else {
                vec![objects[0].clone()]
            },
        }
    })
}
fn order(source: &GameState) -> GameState {
    respond(source, |_, d| {
        assert!(d.kind == DecisionKind::OrderLibraryGraveyard);
        let PlayerDomain::Ordering { objects, .. } = &d.domain else {
            panic!()
        };
        PlayerChoice::Ordering {
            ordered: objects.iter().rev().cloned().collect(),
        }
    })
}
fn select(source: &GameState) -> GameState {
    if matches!(source.pending_decision().unwrap().frame(),RuleFrame::ZoneReplacement(f) if matches!(f.request(),ZoneReplacementRequest::Select {..}))
    {
        respond(source, |_, d| {
            let PlayerDomain::Options { options, .. } = &d.domain else {
                panic!()
            };
            PlayerChoice::Option {
                option: options[0].handle.clone(),
            }
        })
    } else {
        source.clone()
    }
}
fn answer(source: &GameState, answer: ZoneOptionalAnswer) -> GameState {
    respond(source, |view, d| {
        let PlayerDomain::Options { options, .. } = &d.domain else {
            panic!()
        };
        PlayerChoice::Option {
            option: options
                .iter()
                .find(|option| {
                    view.reference(&option.handle).unwrap() == NativeReference::ZoneOptional(answer)
                })
                .unwrap()
                .handle
                .clone(),
        }
    })
}
fn graveyard(source: &GameState) -> Vec<CardId> {
    let cards = source.execution().window().cards();
    cards
        .graveyard(A)
        .iter()
        .map(|id| cards.get(*id).unwrap().card_id().unwrap())
        .collect()
}
#[test]
#[ignore = "requires genuine vocabulary; finite library native originals and chooser privacy"]
fn library_native_partitions_replacements_departures_and_actual_final_movements_roundtrip() {
    let initial = prepare(LibraryActionKind::Scry, A, false, false, false);
    let mut old = NativeScope::new(&initial, bound(&initial, viewer_a())).unwrap();
    let p = project(&mut old, &Display::default());
    let old_handle = p.snapshot().decisions[0].decision.clone();
    let next = depart(&initial, Uuid::from_u128(301));
    let mut new = NativeScope::new(&next, bound(&next, viewer_a())).unwrap();
    project(&mut new, &Display::default());
    assert!(new.reference(&old_handle).is_err());
    let done = partition(&next, false);
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
    let cards = done.execution().window().cards();
    assert_eq!(
        cards
            .library(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(11), CardId(12), CardId(10)]
    );
    let alternate = respond(&next, |_, descriptor| {
        let PlayerDomain::Ordering { objects, .. } = &descriptor.domain else {
            panic!()
        };
        PlayerChoice::LibraryPartition {
            top: vec![objects[0].clone()],
            other: vec![objects[1].clone()],
        }
    });
    let cards = alternate.execution().window().cards();
    assert_eq!(
        cards
            .library(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(10), CardId(12), CardId(11)]
    );
    assert_ne!(done, alternate);
    for kind in [LibraryActionKind::Surveil, LibraryActionKind::Mill] {
        for applied in [false, true] {
            let mut source = prepare(kind, A, true, false, false);
            if kind == LibraryActionKind::Surveil {
                source = partition(&source, true);
            }
            source = select(&source);
            source = answer(
                &source,
                if applied {
                    ZoneOptionalAnswer::Apply
                } else {
                    ZoneOptionalAnswer::Decline
                },
            );
            assert!(graveyard(&source).is_empty());
            source = depart(&source, viewer_b());
            if source.pending_decision().is_some() {
                source = order(&source);
            }
            assert_eq!(source.execution().life_total(A), Some(&25.into()));
            assert_eq!(
                graveyard(&source),
                if applied {
                    vec![CardId(1), CardId(10)]
                } else {
                    vec![CardId(1), CardId(11), CardId(10)]
                }
            );
            let moves = source.execution().window().cards();
            assert_eq!(
                moves
                    .iter()
                    .filter(|object| object.zone() == ObjectZone::Exile)
                    .map(|object| object.card_id().unwrap())
                    .collect::<Vec<_>>(),
                if applied { vec![CardId(11)] } else { vec![] }
            );
            assert_eq!(
                moves
                    .iter()
                    .filter(|object| object.zone() == ObjectZone::Exile)
                    .count(),
                usize::from(applied)
            );
        }
    }
    for kind in [LibraryActionKind::Surveil, LibraryActionKind::Mill] {
        let mut source = prepare(kind, A, false, false, false);
        if kind == LibraryActionKind::Surveil {
            source = partition(&source, true);
        }
        source = order(&source);
        assert_eq!(graveyard(&source), vec![CardId(1), CardId(11), CardId(10)]);
    }
    let mut source = prepare(LibraryActionKind::Mill, A, true, false, false);
    source = select(&source);
    source = depart(&source, Uuid::from_u128(301));
    source = answer(&source, ZoneOptionalAnswer::Decline);
    source = select(&source);
    source = answer(&source, ZoneOptionalAnswer::Decline);
    source = order(&source);
    assert_eq!(graveyard(&source).len(), 3);
    let source = prepare(LibraryActionKind::Surveil, B, false, false, false);
    let done = depart(&source, viewer_b());
    assert!(done.pending_decision().is_none());
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
    let source = prepare(LibraryActionKind::Scry, A, false, false, true);
    let done = finish_original_stack(depart(&source, viewer_a()));
    assert_eq!(done.execution().life_total(D), Some(&25.into()));
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
}
fn normalized(view: &NativeScope<'_>, snapshot: &PlayerSnapshot) -> serde_json::Value {
    fn walk(value: &mut serde_json::Value, map: &BTreeMap<String, String>) {
        match value {
            serde_json::Value::String(s) => {
                if let Some(replacement) = map.get(s) {
                    *s = replacement.clone();
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    walk(value, map);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    walk(value, map);
                }
            }
            _ => {}
        }
    }
    let map = view
        .references
        .iter()
        .enumerate()
        .map(|(i, (handle, _))| {
            (
                serde_json::to_value(handle)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned(),
                format!("handle-{i:04}"),
            )
        })
        .collect();
    let mut value = serde_json::to_value(snapshot).unwrap();
    walk(&mut value, &map);
    value["visibleObjects"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|object| object["handle"].as_str().unwrap().to_owned());
    for zone in value["zones"].as_array_mut().unwrap() {
        if zone["visibility"] == "visible" && zone["orderKnown"] == false {
            zone["objects"]
                .as_array_mut()
                .unwrap()
                .sort_by_key(|object| object.as_str().unwrap().to_owned());
        }
    }
    value
}
#[test]
#[ignore = "requires genuine vocabulary; pending Mill has no look permission"]
fn mill_pending_choices_are_independent_of_hidden_identity_and_never_request_its_display() {
    fn observe(source: &GameState) -> serde_json::Value {
        let mut view = NativeScope::new(source, bound(source, viewer_a())).unwrap();
        let display = Display::default();
        let p = project(&mut view, &display);
        assert_private(&view, p.snapshot(), &display);
        normalized(&view, p.snapshot())
    }
    for replacements in [false, true] {
        let mut pairs = vec![];
        for flipped in [false, true] {
            let mut source = prepare(LibraryActionKind::Mill, A, replacements, flipped, false);
            let mut snapshots = vec![observe(&source)];
            if replacements {
                source = select(&source);
                snapshots.push(observe(&source));
                source = answer(&source, ZoneOptionalAnswer::Decline);
                source = select(&source);
                source = answer(&source, ZoneOptionalAnswer::Decline);
                snapshots.push(observe(&source));
            }
            pairs.push(snapshots);
        }
        assert_eq!(pairs[0], pairs[1]);
    }
}
