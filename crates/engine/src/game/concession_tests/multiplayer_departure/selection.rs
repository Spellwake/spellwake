//! Actual selected actions retain their original legal choices across cuts.
use super::*;
use crate::characteristics::{
    PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
};
use crate::program::CardType;
const D: SeatId = SeatId(77);
fn controlled_fixture() -> GameState {
    let mut artifact = creature();
    artifact.card_types.insert(CardType::Artifact);
    let mut source = creature();
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ControlSourceController]),
    });
    let mut state = root(
        vec![
            control_spell(1, C),
            control_spell(2, A),
            card(
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
                artifact.clone(),
            ),
            card(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                source,
            ),
            card(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                artifact,
            ),
            card(
                6,
                C,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
            card(
                7,
                D,
                ObjectDestination::Battlefield { controller: D },
                creature(),
            ),
            card(8, D, ObjectDestination::Battlefield { controller: D }, {
                let mut source = Program::enchantment(None);
                source.zone_replacements = vec![ZoneReplacementAbility {
                    id: ZoneReplacementAbilityId(0),
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::EffectSacrifice,
                    ])),
                    objects: ZoneObjectKind::Card,
                    subject: ZoneReplacementSubject::AnyObject,
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Exile,
                    choice: ZoneReplacementChoice::OptionalAffectedPlayer,
                }];
                source
            }),
            card(
                9,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    crate::program::ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![
                            Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(2.into()),
                            },
                            Instruction::Sacrifice {
                                players: AffectedPlayers::EachPlayer,
                                amount: InstructionAmount::Literal(2.into()),
                                filter: crate::program::PermanentTypeFilter {
                                    required_types: BTreeSet::from([CardType::Creature]),
                                    forbidden_types: BTreeSet::new(),
                                },
                            },
                            Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(3.into()),
                            },
                        ],
                    ),
                ),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let x = object(&state, 3);
    state = cast_and_resolve(state, C, 1, x);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .controller(),
        Some(C)
    );
    let source = object(&state, 4);
    state = cast_and_resolve(state, A, 2, source);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(x)
            .unwrap()
            .controller(),
        Some(A)
    );
    state
}
#[test]
fn admitted_removed_static_source_exposes_prior_control_then_later_departure_restores_same_incarnation()
 {
    let state = controlled_fixture();
    let id = object(&state, 3);
    let original = state.execution().window().cards().get(id).unwrap().clone();
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(object(&state, 3), id);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(id)
            .unwrap()
            .controller(),
        Some(C)
    );
    let state = apply(&state, C, GameCommand::Concede).into_state();
    let restored = state.execution().window().cards().get(id).unwrap();
    assert_eq!(restored.id(), id);
    assert_eq!(restored.controller(), Some(A));
    assert_eq!(restored.owner(), A);
    assert_ne!(
        *restored, original,
        "actual control history changes without a new incarnation"
    );
    assert_eq!(state.execution().window().participants(), &[A, D]);
}

fn selected(state: &GameState, ids: &[u64]) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    let objects = ids.iter().map(|id| object(state, *id)).collect();
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::SelectResolution(objects),
        },
    )
}
#[test]
fn each_player_discard_current_departure_preserves_real_prefix_and_skips_only_gone_hand() {
    let mut state = root(
        vec![
            card(
                1,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    crate::program::ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![
                            Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(2.into()),
                            },
                            Instruction::Discard {
                                players: AffectedPlayers::EachPlayer,
                                amount: InstructionAmount::Literal(1.into()),
                            },
                            Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(3.into()),
                            },
                        ],
                    ),
                ),
            ),
            card(
                10,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
            card(
                20,
                B,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
            card(
                30,
                C,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
            card(
                40,
                D,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    state = apply(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = selected(&state, &[10]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    let old = state.pending_decision().unwrap().clone();
    state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(20))
            .is_none()
    );
    assert!(
        state
            .advance(input(
                &state,
                A,
                GameCommand::Reply {
                    decision: old.id(),
                    choice: DecisionReply::SelectResolution(vec![])
                }
            ))
            .is_err()
    );
    state = selected(&state, &[30]).into_state();
    let candidate = selected(&state, &[40]);
    let actors = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|occurrence| {
            if let RulesOccurrence::ZoneChange(change) = &occurrence.value {
                if let ZoneReason::Discard { player } = change.reason {
                    Some(player)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(actors, BTreeSet::from([A, C, D]));
    let state = candidate.into_state();
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().window().participants(), &[A, C, D]);
}

fn zone_answer(state: &GameState, answer: ZoneOptionalAnswer) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(answer)),
        },
    )
}
fn select_zone_object(state: &GameState, id: u64) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    let RuleFrame::ZoneReplacement(frame) = pending.frame() else {
        panic!("replacement")
    };
    let ZoneReplacementRequest::Select { applications } = frame.request() else {
        panic!("select")
    };
    let application = *applications
        .iter()
        .find(|application| application.event.before() == object(state, id))
        .unwrap();
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(application)),
        },
    )
}
#[test]
fn original_actor_action_reactivates_with_its_own_unspent_replacement_after_other_actor_leaves() {
    let mut state = controlled_fixture();
    state = apply(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 9),
            payment: None,
        },
    )
    .into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = selected(&state, &[3, 5]).into_state();
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("C selection")
    };
    assert_eq!(frame.chooser(), C);
    assert_eq!(frame.required(), 1);
    state = apply(&state, B, GameCommand::Concede).into_state();
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("grown C selection")
    };
    assert_eq!(frame.chooser(), C);
    assert_eq!(frame.required(), 2);
    assert!(frame.domain().contains(&object(&state, 3)));
    state = selected(&state, &[3, 6]).into_state();
    state = selected(&state, &[7]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    state = select_zone_object(&state, 3).into_state();
    let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    let ZoneReplacementRequest::ConfirmOptional { application: old } = frame.request() else {
        panic!()
    };
    let old = *old;
    assert!(matches!(
        old.event,
        crate::effects::ZoneEventKey::Selection { player: C, .. }
    ));
    state = zone_answer(&state, ZoneOptionalAnswer::Apply).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let id = object(&state, 3);
    state = apply(&state, C, GameCommand::Concede).into_state();
    assert_eq!(object(&state, 3), id);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(id)
            .unwrap()
            .controller(),
        Some(A)
    );
    let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() else {
        panic!("A's original action still needs its own opportunity")
    };
    assert_eq!(frame.chooser(), A);
    let ZoneReplacementRequest::ConfirmOptional { application: new } = frame.request() else {
        panic!()
    };
    assert_eq!(new.event.before(), old.event.before());
    assert_ne!(*new, old);
    assert!(matches!(
        new.event,
        crate::effects::ZoneEventKey::Selection { player: A, .. }
    ));
    state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), D);
    state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    let pending = state.pending_decision().unwrap();
    let RuleFrame::ResolutionGraveyardOrder(frame) = pending.frame() else {
        panic!("actual A owner order")
    };
    let order = frame.domain().iter().rev().copied().collect();
    let candidate = apply(
        &state,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderResolutionGraveyard(order),
        },
    );
    let sacrifices = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|occurrence| {
            if let RulesOccurrence::ZoneChange(change) = &occurrence.value {
                if let ZoneReason::Sacrifice { player } = change.reason {
                    Some((change.before.object.card_id().unwrap(), player))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        sacrifices,
        BTreeSet::from([(CardId(3), A), (CardId(5), A), (CardId(7), D)])
    );
    let state = candidate.into_state();
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(
        state
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| object.zone() != crate::object_identity::ObjectZone::Exile)
    );
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(6))
            .is_none()
    );
    assert!(state.execution().window().entries().is_empty());
}

fn begin_selection(mut state: GameState, spell: u64) -> GameState {
    state = apply(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, spell),
            payment: None,
        },
    )
    .into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state
}
fn sacrifice_spell(amount: i64) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                },
                Instruction::Sacrifice {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::Literal(amount.into()),
                    filter: crate::program::PermanentTypeFilter {
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                    },
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(3.into()),
                },
            ],
        ),
    )
}
fn ordered(state: &GameState, ids: &[u64]) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    assert!(matches!(
        pending.frame(),
        RuleFrame::ResolutionGraveyardOrder(_)
    ));
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderResolutionGraveyard(
                ids.iter().map(|id| object(state, *id)).collect(),
            ),
        },
    )
}
#[test]
fn accepted_sacrifice_survives_loss_of_original_creature_filter_after_departure() {
    use crate::characteristics::{
        NumericExpression, NumericPair, StaticTypeChange, SubtypeOperation, TypeOperation,
    };
    let mut grant = Program::enchantment(None);
    grant.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![
            StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                subtypes: SubtypeOperation::Keep,
            }),
            StaticPart::SetPT(NumericPair {
                power: NumericExpression::Literal(2.into()),
                toughness: NumericExpression::Literal(2.into()),
            }),
        ]),
    });
    let state = root(
        vec![
            card(1, A, ObjectDestination::Hand, sacrifice_spell(1)),
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
            card(
                20,
                B,
                ObjectDestination::Battlefield { controller: B },
                grant,
            ),
            card(
                30,
                C,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
            card(
                40,
                D,
                ObjectDestination::Battlefield { controller: D },
                creature(),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let state = begin_selection(state, 1);
    let selected_id = object(&state, 10);
    assert!(
        state
            .execution()
            .current_characteristics(selected_id)
            .unwrap()
            .unwrap()
            .current_types
            .contains(&CardType::Creature)
    );
    let state = selected(&state, &[10]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(object(&state, 10), selected_id);
    assert!(
        !state
            .execution()
            .current_characteristics(selected_id)
            .unwrap()
            .unwrap()
            .current_types
            .contains(&CardType::Creature)
    );
    let state = selected(&state, &[30]).into_state();
    let candidate = selected(&state, &[40]);
    let selected_change = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .flat_map(|event| &event.occurrences)
        .find_map(|occurrence| match &occurrence.value {
            RulesOccurrence::ZoneChange(change) if change.before.object.id() == selected_id => {
                Some(change)
            }
            _ => None,
        })
        .expect("original selected artifact is actually sacrificed");
    assert_eq!(selected_change.reason, ZoneReason::Sacrifice { player: A });
    let state = candidate.into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .graveyard(A)
            .iter()
            .filter_map(|id| state
                .execution()
                .window()
                .cards()
                .get(*id)
                .unwrap()
                .card_id())
            .collect::<Vec<_>>(),
        vec![CardId(1), CardId(10)]
    );
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(state.execution().window().entries().is_empty());
}
#[test]
fn prior_owner_order_filters_removed_actor_members_without_reasking_survivor() {
    let mut cards = vec![card(1, A, ObjectDestination::Hand, sacrifice_spell(2))];
    for (id, owner, controller) in [
        (10, A, A),
        (11, A, A),
        (20, A, C),
        (21, A, C),
        (40, D, D),
        (41, D, D),
    ] {
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller },
            creature(),
        ));
    }
    let state = begin_selection(
        root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]),
        1,
    );
    let state = selected(&state, &[10, 11]).into_state();
    let state = selected(&state, &[20, 21]).into_state();
    let state = selected(&state, &[40, 41]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    let state = ordered(&state, &[21, 10, 20, 11]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), D);
    let state = apply(&state, C, GameCommand::Concede).into_state();
    assert_eq!(
        state.pending_decision().unwrap().chooser(),
        D,
        "A's surviving accepted order is held"
    );
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    let state = ordered(&state, &[41, 40]).into_state();
    let cards = state.execution().window().cards();
    assert_eq!(
        cards
            .graveyard(A)
            .iter()
            .filter_map(|id| state
                .execution()
                .window()
                .cards()
                .get(*id)
                .unwrap()
                .card_id())
            .collect::<Vec<_>>(),
        vec![CardId(1), CardId(10), CardId(11)]
    );
    assert_eq!(
        cards
            .graveyard(D)
            .iter()
            .filter_map(|id| state
                .execution()
                .window()
                .cards()
                .get(*id)
                .unwrap()
                .card_id())
            .collect::<Vec<_>>(),
        vec![CardId(41), CardId(40)]
    );
    for id in [20, 21] {
        assert_eq!(
            cards
                .get(cards.current_identity(CardId(id)).unwrap())
                .unwrap()
                .zone(),
            crate::object_identity::ObjectZone::Exile
        );
    }
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(state.execution().window().entries().is_empty());
}

#[test]
fn departure_exposes_replacement_and_new_arrival_requires_fresh_complete_owner_order() {
    use crate::characteristics::{AbilityChange, AbilityRemoval};
    let definition = |destination, replace_with, subject, choice| ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectSacrifice])),
        objects: ZoneObjectKind::Card,
        subject,
        destination,
        replace_with,
        choice,
    };
    let mut redirected = creature();
    redirected.zone_replacements.push(definition(
        PublicZoneDestination::Graveyard,
        PublicZoneDestination::Exile,
        ZoneReplacementSubject::SourceSelf,
        ZoneReplacementChoice::Mandatory,
    ));
    let mut reverse = Program::enchantment(None);
    reverse.zone_replacements.push(definition(
        PublicZoneDestination::Exile,
        PublicZoneDestination::Graveyard,
        ZoneReplacementSubject::AnyObject,
        ZoneReplacementChoice::OptionalAffectedPlayer,
    ));
    let mut suppressor = Program::artifact(None, vec![]);
    suppressor.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Enchantment]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
            AbilityRemoval::All,
        ))]),
    });
    let state = begin_selection(
        root(
            vec![
                card(1, A, ObjectDestination::Hand, sacrifice_spell(3)),
                card(
                    10,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    redirected,
                ),
                card(
                    11,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    creature(),
                ),
                card(
                    12,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    creature(),
                ),
                card(
                    20,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    reverse,
                ),
                card(
                    30,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                    creature(),
                ),
                card(
                    31,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                    creature(),
                ),
                card(
                    40,
                    D,
                    ObjectDestination::Battlefield { controller: D },
                    suppressor,
                ),
            ],
            &[20; 4],
            Some(Step::PrecombatMain),
            vec![A, B, C, D],
        ),
        1,
    );
    let state = selected(&state, &[10, 11, 12]).into_state();
    let state = selected(&state, &[30, 31]).into_state();
    let RuleFrame::ResolutionGraveyardOrder(frame) = state.pending_decision().unwrap().frame()
    else {
        panic!("settled original destinations")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([object(&state, 11), object(&state, 12)])
    );
    let state = ordered(&state, &[12, 11]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let state = apply(&state, D, GameCommand::Concede).into_state();
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ZoneReplacement(_)
    ));
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    let state = zone_answer(&state, ZoneOptionalAnswer::Apply).into_state();
    let RuleFrame::ResolutionGraveyardOrder(frame) = state.pending_decision().unwrap().frame()
    else {
        panic!("new entrant requires complete new owner order")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.domain().len(), 3);
    assert!(frame.domain().contains(&object(&state, 10)));
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    let state = ordered(&state, &[11, 10, 12]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let state = ordered(&state, &[31, 30]).into_state();
    let cards = state.execution().window().cards();
    assert_eq!(
        cards
            .graveyard(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(1), CardId(11), CardId(10), CardId(12)]
    );
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(state.execution().window().entries().is_empty());
    assert!(
        cards
            .iter()
            .all(|object| object.zone() != crate::object_identity::ObjectZone::Exile)
    );
}

#[test]
fn consumed_replacement_source_witnesses_keep_actual_controller_at_each_application() {
    let mut artifact = creature();
    artifact.card_types.insert(CardType::Artifact);
    artifact.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    let mut control_source = creature();
    control_source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ControlSourceController]),
    });
    let mut cards = vec![
        control_spell(1, C),
        control_spell(2, A),
        card(
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
            artifact,
        ),
        card(
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
            control_source,
        ),
        card(
            9,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::EachPlayer,
                        amount: InstructionAmount::Literal(1.into()),
                    }],
                ),
            ),
        ),
    ];
    for (id, owner) in [(10, A), (30, C), (40, D)] {
        cards.push(card(
            id,
            owner,
            ObjectDestination::Hand,
            Program::enchantment(None),
        ));
    }
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    let source_id = object(&state, 3);
    state = cast_and_resolve(state, C, 1, source_id);
    let control_id = object(&state, 4);
    state = cast_and_resolve(state, A, 2, control_id);
    state = begin_selection(state, 9);
    state = selected(&state, &[10]).into_state();
    state = selected(&state, &[30]).into_state();
    state = selected(&state, &[40]).into_state();
    state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(source_id)
            .unwrap()
            .controller(),
        Some(C)
    );
    state = zone_answer(&state, ZoneOptionalAnswer::Apply).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), D);
    state = apply(&state, C, GameCommand::Concede).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(source_id)
            .unwrap()
            .controller(),
        Some(A)
    );
    let evidence = state.execution().resolution_selection_state().unwrap();
    let crate::effects::ResolutionSelectionStage::Replacing { batch, .. } = evidence.stage else {
        panic!("D replacement")
    };
    let witnesses = batch
        .consumed_sources()
        .iter()
        .map(|(application, source)| {
            assert_eq!(source.before.id(), source_id);
            let crate::effects::ZoneEventKey::Selection { player, .. } = application.event else {
                panic!("actor-scoped original action")
            };
            (player, source.before.controller())
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(witnesses, BTreeMap::from([(A, Some(A)), (C, Some(C))]));
    state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    let cards = state.execution().window().cards();
    assert_eq!(
        cards
            .graveyard(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(9), CardId(10), CardId(2)]
    );
    assert_eq!(
        cards
            .graveyard(D)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(40)]
    );
    assert!(cards.current_identity(CardId(30)).is_none());
    assert!(state.execution().window().entries().is_empty());
}

#[test]
fn selected_departure_requires_exact_issued_frame_and_rejects_exhausted_publication_atomically() {
    use crate::effects::private_qualification::QualificationFault;
    let initial = begin_selection(controlled_fixture(), 9);
    let current = selected(&initial, &[3, 5]).into_state();
    assert_eq!(
        current
            .execution()
            .depart_players(&BTreeSet::from([B]), current.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    for field in 0..3 {
        let mut bad = current.clone();
        match field {
            0 => bad.pending = None,
            1 => {
                bad.pending.as_mut().unwrap().frame =
                    initial.pending_decision().unwrap().frame().clone()
            }
            2 => bad.pending.as_mut().unwrap().basis.revision += 1,
            _ => unreachable!(),
        }
        let before = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(GameError::UnsupportedConcessionScope));
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, before);
    }
    for fault in [
        QualificationFault::Revision(u64::MAX),
        QualificationFault::Event(u64::MAX),
        QualificationFault::ObjectRevision(u64::MAX),
    ] {
        let mut bad = current.clone();
        bad.set_test_check_effect_fault(fault);
        let before = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        let expected = match fault {
            QualificationFault::Revision(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::RevisionExhausted,
            )),
            QualificationFault::Event(_) => GameError::Effect(EffectError::Trigger(
                crate::occurrence::TriggerError::EventIdentityExhausted,
            )),
            QualificationFault::ObjectRevision(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::RevisionExhausted,
                ),
            )),
            _ => unreachable!(),
        };
        assert_eq!(result, Err(expected), "{fault:?}");
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, before);
    }
    for field in 0..2 {
        let mut bad = current.clone();
        match field {
            0 => bad.set_test_input_revision(u64::MAX),
            1 => bad.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(
            result,
            Err(if field == 0 {
                GameError::RevisionExhausted
            } else {
                GameError::DecisionIdentityExhausted
            })
        );
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, before);
    }
}

#[test]
fn frozen_derived_selection_count_survives_contributor_and_ability_source_departure() {
    use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    use crate::program::CreatureTypeSlot;
    let human = crate::test_support::creature_types::selected_vocabulary()
        .unwrap()
        .resolve_label("Human")
        .unwrap();
    for ability in [false, true] {
        let body = ExecutableBody::new(
            if ability {
                BodyTarget::SinglePlayer
            } else {
                BodyTarget::None
            },
            vec![
                Instruction::ChooseCreatureType {
                    into: CreatureTypeSlot::new(0),
                    excluding: BTreeSet::new(),
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                },
                Instruction::Sacrifice {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::ControlledPermanentsOfChosenType {
                        chosen: CreatureTypeSlot::new(0),
                    },
                    filter: crate::program::PermanentTypeFilter {
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                    },
                },
                if ability {
                    Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    }
                } else {
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    }
                },
            ],
        );
        let source = if ability {
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(
                    None,
                    vec![ActivatedAbility {
                        id: ActivatedAbilityId(1),
                        mana_cost: ManaCost::free(),
                        source_cost: SourceCost::None,
                        body,
                    }],
                ),
            )
        } else {
            card(
                1,
                A,
                ObjectDestination::Hand,
                Program::instant(ManaCost::free(), body),
            )
        };
        let mut cards = vec![source];
        for (id, owner, controller) in [(10, A, A), (20, B, A), (30, C, C), (31, C, C)] {
            cards.push(card(
                id,
                owner,
                ObjectDestination::Battlefield { controller },
                Program::creature(None, 2.into(), 3.into(), BTreeSet::from([human])),
            ));
        }
        let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
        state = apply(
            &state,
            A,
            if ability {
                GameCommand::ActivateAbility {
                    source: object(&state, 1),
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Player(A),
                    payment: None,
                }
            } else {
                GameCommand::CastUntargeted {
                    spell: object(&state, 1),
                    payment: None,
                }
            },
        )
        .into_state();
        for actor in [A, B, C, D] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        let pending = state.pending_decision().unwrap();
        state = apply(
            &state,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::CreatureType(human),
            },
        )
        .into_state();
        state = selected(&state, &[10, 20]).into_state();
        assert_eq!(state.pending_decision().unwrap().chooser(), C);
        state = apply(&state, B, GameCommand::Concede).into_state();
        let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
            panic!("surviving C choice")
        };
        assert_eq!(
            frame.required(),
            2,
            "count was materialized before A lost a contributor"
        );
        if ability {
            assert!(
                state
                    .execution()
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .is_none()
            );
        }
        state = selected(&state, &[30, 31]).into_state();
        assert_eq!(state.pending_decision().unwrap().chooser(), C);
        state = ordered(&state, &[31, 30]).into_state();
        let objects = state.execution().window().cards();
        assert_eq!(
            objects
                .graveyard(C)
                .iter()
                .map(|id| objects.get(*id).unwrap().card_id().unwrap())
                .collect::<Vec<_>>(),
            vec![CardId(31), CardId(30)]
        );
        assert!(objects.current_identity(CardId(20)).is_none());
        assert_eq!(state.execution().life_total(A), Some(&25.into()));
        assert!(state.execution().window().entries().is_empty());
    }
}

#[test]
fn selected_token_owned_by_departing_player_is_removed_without_a_later_sacrifice() {
    use crate::token::{TokenCreation, TokenDefinition, TokenName};
    let vocabulary = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let token = TokenDefinition::new(
        creature(),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap();
    let mut state = root(
        vec![
            card(
                1,
                B,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::CreateTokens {
                            creations: vec![TokenCreation::one(token)],
                        }],
                    ),
                ),
            ),
            control_spell(2, A),
            card(3, A, ObjectDestination::Hand, sacrifice_spell(1)),
            card(
                30,
                C,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
            card(
                40,
                D,
                ObjectDestination::Battlefield { controller: D },
                creature(),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    state = apply(&state, A, GameCommand::Pass).into_state();
    state = apply(
        &state,
        B,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    let token = state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|object| object.is_token())
        .unwrap()
        .id();
    state = cast_and_resolve(state, A, 2, token);
    state = begin_selection(state, 3);
    let pending = state.pending_decision().unwrap();
    state = apply(
        &state,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::SelectResolution(vec![token]),
        },
    )
    .into_state();
    let departed = apply(&state, B, GameCommand::Concede);
    assert!(
        departed
            .state()
            .execution()
            .window()
            .cards()
            .get(token)
            .is_none()
    );
    state = selected(departed.state(), &[30]).into_state();
    let finished = selected(&state, &[40]);
    for candidate in [&departed, &finished] {
        assert!(!candidate.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence|matches!(&occurrence.value,RulesOccurrence::ZoneChange(change) if change.before.object.id()==token && matches!(change.reason,ZoneReason::Sacrifice {..}))))));
    }
    assert!(finished.state().execution().window().entries().is_empty());
    assert_eq!(finished.state().execution().life_total(A), Some(&25.into()));
}
#[test]
fn selection_controller_departure_drops_remaining_action_and_real_lower_stack_continues() {
    let mut state = root(
        vec![
            card(1, A, ObjectDestination::Hand, sacrifice_spell(1)),
            card(
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
            ),
            card(
                10,
                A,
                ObjectDestination::Battlefield { controller: A },
                creature(),
            ),
            card(
                30,
                C,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
            card(
                40,
                D,
                ObjectDestination::Battlefield { controller: D },
                creature(),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    for actor in [A, B, C] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        D,
        GameCommand::CastUntargeted {
            spell: object(&state, 2),
            payment: None,
        },
    )
    .into_state();
    state = apply(&state, D, GameCommand::Pass).into_state();
    state = begin_selection(state, 1);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    let canceled = apply(&state, A, GameCommand::Concede);
    assert!(canceled.state().pending_decision().is_none());
    assert!(
        canceled
            .state()
            .execution()
            .resolution_selection_state()
            .is_none()
    );
    assert_eq!(canceled.state().execution().window().entries().len(), 1);
    assert!(!canceled.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence|matches!(&occurrence.value,RulesOccurrence::ZoneChange(change) if matches!(change.reason,ZoneReason::Sacrifice {..}))))));
    state = canceled.into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert_eq!(state.execution().life_total(D), Some(&25.into()));
    for card in [30, 40] {
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(object(&state, card))
                .unwrap()
                .zone(),
            crate::object_identity::ObjectZone::Battlefield
        );
    }
}

#[test]
fn prior_no_source_owner_order_reopens_for_newly_exposed_replacement_after_departure() {
    use crate::characteristics::{AbilityChange, AbilityRemoval};
    let mut replacement = Program::enchantment(None);
    replacement.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectSacrifice])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    let mut suppressor = Program::artifact(None, vec![]);
    suppressor.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Enchantment]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
            AbilityRemoval::All,
        ))]),
    });
    let mut cards = vec![
        card(1, A, ObjectDestination::Hand, sacrifice_spell(2)),
        card(
            20,
            B,
            ObjectDestination::Battlefield { controller: B },
            replacement,
        ),
        card(
            40,
            D,
            ObjectDestination::Battlefield { controller: D },
            suppressor,
        ),
    ];
    for (id, owner) in [(10, A), (11, A), (30, C), (31, C)] {
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            creature(),
        ));
    }
    let state = begin_selection(
        root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]),
        1,
    );
    let state = selected(&state, &[10, 11]).into_state();
    let state = selected(&state, &[30, 31]).into_state();
    let state = ordered(&state, &[11, 10]).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let state = apply(&state, D, GameCommand::Concede).into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    let state = select_zone_object(&state, 10).into_state();
    let state = zone_answer(&state, ZoneOptionalAnswer::Apply).into_state();
    let state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    let state = select_zone_object(&state, 30).into_state();
    let state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    let state = zone_answer(&state, ZoneOptionalAnswer::Decline).into_state();
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionGraveyardOrder(_)
    ));
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    let state = ordered(&state, &[31, 30]).into_state();
    let cards = state.execution().window().cards();
    assert_eq!(
        cards
            .get(cards.current_identity(CardId(10)).unwrap())
            .unwrap()
            .zone(),
        crate::object_identity::ObjectZone::Exile
    );
    assert_eq!(
        cards
            .graveyard(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(1), CardId(11)]
    );
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert!(state.execution().window().entries().is_empty());
}
