#![allow(dead_code, unused_imports)]
//! CR704.3/704.5k actual owner/root trajectories. Prepared initial cohorts are
//! explicit premises; these do not claim opening-to-outcome format qualification.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    characteristics::{
        ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
        WorldTenureChange,
    },
    combat::CombatRules,
    effects::{EffectDecision, EffectWindow, RuleResult, SeedCard},
    game::{DecisionReply, GameCommand, GameContext, GameId, GameState},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{
        RulesJournal, RulesOccurrence, RulesRecord, StateBasedMoveReason, StateBasedMoveReasons,
        ZoneReason,
    },
    priority::SeatId,
    program::{BodyTarget, Instruction, Program, Supertype},
    setup::GameSeed,
    turn::{Step, TurnStart},
};
use std::{collections::BTreeSet, sync::Arc};
use trajectories::{P0, P1};

fn artifact(world: bool) -> Program {
    let mut p = Program::artifact(Some(ManaCost::free()), vec![]);
    if world {
        p.supertypes.insert(Supertype::World);
    }
    p
}
fn card(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program,
    }
}
fn seed(cards: Vec<SeedCard>, cohorts: Vec<Vec<CardId>>) -> GameSeed {
    // Deliberately independent ordering-clock premise, not World age inference.
    let order = cards
        .iter()
        .rev()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let mut seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        trajectories::vocabulary(),
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    seed.initial_world_cohorts = cohorts;
    seed
}
fn root(cards: Vec<SeedCard>, cohorts: Vec<Vec<CardId>>) -> GameState {
    let seed = seed(cards, cohorts);
    let context = GameContext::new(
        "world-trajectories".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal".into(),
        "world-history".into(),
        "prepared-main".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(7045),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn id(window: &EffectWindow, card: u64) -> ObjectId {
    window
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn age(window: &EffectWindow, object: ObjectId) -> Option<u64> {
    window
        .world_tenures()
        .find_map(|(id, t)| (id == object).then_some(t.sequence()))
}
fn events(journal: &RulesJournal) -> Vec<&RulesOccurrence> {
    journal
        .records
        .iter()
        .filter_map(|r| match r {
            RulesRecord::Event(e) => Some(&e.occurrences),
            _ => None,
        })
        .flatten()
        .map(|r| &r.value)
        .collect()
}
fn change(op: TypeOperation<Supertype>) -> Instruction {
    trajectories::effect(
        vec![ResolvedPartSpec::Type(TypeChangeSpec {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: op,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })],
        ResolvedDuration::NoStatedDuration,
    )
}

#[test]
fn world_constructor_requires_a_complete_explicit_tenure_premise() {
    let cards = vec![card(1, P0, artifact(true))];
    assert!(EffectWindow::new(seed(cards.clone(), vec![])).is_err());
    let state = EffectWindow::new(seed(cards, vec![vec![CardId(1)]])).unwrap();
    assert_eq!(age(&state, id(&state, 1)), Some(1));
    assert_eq!(state.operational_counters().next_world_tenure, 2);
}

#[test]
fn world_rule_sole_latest_survives_across_controllers_and_emits_one_actual_move() {
    let state = EffectWindow::new(seed(
        vec![card(1, P0, artifact(true)), card(2, P1, artifact(true))],
        vec![vec![CardId(1)], vec![CardId(2)]],
    ))
    .unwrap();
    let old = id(&state, 1);
    let new = id(&state, 2);
    let source = state.clone();
    let done = state.begin_checks(state.revision()).unwrap();
    assert_eq!(state, source);
    assert!(matches!(done.result(), RuleResult::Complete { .. }));
    assert_eq!(
        done.state()
            .window()
            .cards()
            .get(id(done.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        done.state().window().cards().get(new).unwrap().zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(age(done.state(), new), Some(2));
    assert_eq!(age(done.state(), old), None);
    let moves: Vec<_> = events(done.append())
        .into_iter()
        .filter_map(|e| match e {
            RulesOccurrence::ZoneChange(z) => Some(z),
            _ => None,
        })
        .collect();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].before.object.id(), old);
    assert_eq!(
        moves[0].reason,
        ZoneReason::StateBased(StateBasedMoveReasons::new(StateBasedMoveReason::World))
    );
}

#[test]
fn tied_latest_worlds_wait_for_actual_owner_order_then_all_move_in_one_event() {
    let state = EffectWindow::new(seed(
        vec![
            card(1, P0, artifact(true)),
            card(2, P0, artifact(true)),
            card(3, P1, artifact(true)),
        ],
        vec![vec![CardId(1), CardId(2), CardId(3)]],
    ))
    .unwrap();
    let originals = [id(&state, 1), id(&state, 2), id(&state, 3)];
    let pending = state.begin_checks(state.revision()).unwrap();
    let RuleResult::Pending {
        frame: EffectDecision::Checks(frame),
        ..
    } = pending.result()
    else {
        panic!("owner must arrange its two entrants")
    };
    assert_eq!(frame.chooser(), P0);
    let spellwake_engine::checks::CheckDecision::GraveyardOrder(order) = frame else {
        panic!("expected graveyard arrangement")
    };
    assert_eq!(
        order.domain().iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([originals[0], originals[1]])
    );
    assert!(
        pending
            .state()
            .window()
            .cards()
            .iter()
            .all(|c| c.zone() == ObjectZone::Battlefield)
    );
    assert_eq!(pending.state().world_tenures().count(), 3);
    let before = pending.state().clone();
    assert!(
        before
            .resume_rule(
                before.revision(),
                &EffectDecision::Checks(frame.clone()),
                P1,
                DecisionReply::OrderGraveyard(vec![originals[1], originals[0]]),
                None
            )
            .is_err()
    );
    assert_eq!(pending.state(), &before);
    let done = before
        .resume_rule(
            before.revision(),
            &EffectDecision::Checks(frame.clone()),
            P0,
            DecisionReply::OrderGraveyard(vec![originals[1], originals[0]]),
            None,
        )
        .unwrap();
    assert!(matches!(done.result(), RuleResult::Complete { .. }));
    assert_eq!(done.state().world_tenures().count(), 0);
    let owner_order: Vec<_> = done
        .state()
        .window()
        .cards()
        .graveyard(P0)
        .iter()
        .map(|id| {
            done.state()
                .window()
                .cards()
                .get(*id)
                .unwrap()
                .card_id()
                .expect("physical fixture card")
        })
        .collect();
    assert_eq!(owner_order, vec![CardId(2), CardId(1)]);
    let moves: Vec<_> = events(done.append())
        .into_iter()
        .filter(|e| matches!(e, RulesOccurrence::ZoneChange(_)))
        .collect();
    assert_eq!(moves.len(), 3);
    assert_eq!(done.state().operational_counters().next_world_tenure, 2);
}

#[test]
fn zero_toughness_and_world_causes_share_one_physical_movement() {
    let mut dying = Program::creature(None, 0.into(), 0.into(), BTreeSet::new());
    dying.supertypes.insert(Supertype::World);
    let state = EffectWindow::new(seed(
        vec![card(1, P0, dying), card(2, P1, artifact(true))],
        vec![vec![CardId(1)], vec![CardId(2)]],
    ))
    .unwrap();
    let old = id(&state, 1);
    let done = state.begin_checks(state.revision()).unwrap();
    let moves: Vec<_> = events(done.append())
        .into_iter()
        .filter_map(|e| match e {
            RulesOccurrence::ZoneChange(z) => Some(z),
            _ => None,
        })
        .collect();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].before.object.id(), old);
    let ZoneReason::StateBased(causes) = moves[0].reason else {
        panic!("state action")
    };
    assert_eq!(
        causes.iter().collect::<Vec<_>>(),
        vec![
            StateBasedMoveReason::ZeroToughness,
            StateBasedMoveReason::World
        ]
    );
    assert!(!causes.contains(StateBasedMoveReason::LethalDestruction));
}

#[test]
fn actual_world_gain_keeps_the_new_world_even_when_its_ordering_timestamp_is_older() {
    let game = root(
        vec![
            card(1, P0, artifact(true)),
            card(2, P0, artifact(false)),
            trajectories::spell(
                3,
                BodyTarget::SinglePermanent,
                vec![change(TypeOperation::Add(BTreeSet::from([
                    Supertype::World,
                ])))],
            ),
        ],
        vec![vec![CardId(1)]],
    );
    let old = id(game.execution(), 1);
    let target = id(game.execution(), 2);
    let (done, _) = trajectories::resolve(&game, 3, target);
    assert!(done.state().execution().window().cards().get(old).is_none());
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(id(done.state().execution(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(age(done.state().execution(), target), Some(2));
    let changes: Vec<_> = events(done.rules_journal())
        .into_iter()
        .filter_map(|e| match e {
            RulesOccurrence::WorldTenureChanged(change) => Some(*change),
            _ => None,
        })
        .collect();
    assert!(matches!(changes[0],WorldTenureChange::Gained{object,..} if object==target));
    assert!(matches!(changes[1],WorldTenureChange::Lost{object,..} if object==old));
    assert_eq!(changes.len(), 2);
}

#[test]
fn normal_entries_and_resolved_gain_regain_or_copy_use_actual_world_history() {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        program::ExecutableBody,
    };
    for route in ["gain", "regain", "copy"] {
        let mut receiver = artifact(route == "regain");
        if route == "copy" {
            receiver.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![Instruction::CopyTargetToSource {
                        duration: ResolvedDuration::NoStatedDuration,
                        power_toughness: None,
                    }],
                ),
            });
        }
        let mut first = card(1, P0, receiver);
        first.destination = ObjectDestination::Hand;
        let mut world = card(2, P0, artifact(true));
        world.destination = ObjectDestination::Hand;
        let game = root(
            vec![
                first,
                world,
                trajectories::spell(
                    3,
                    BodyTarget::SinglePermanent,
                    vec![change(TypeOperation::Remove(BTreeSet::from([
                        Supertype::World,
                    ])))],
                ),
                trajectories::spell(
                    4,
                    BodyTarget::SinglePermanent,
                    vec![change(TypeOperation::Add(BTreeSet::from([
                        Supertype::World,
                    ])))],
                ),
            ],
            vec![],
        );
        // Every battlefield entry comes from a real permanent spell, with no
        // initial battlefield or invented tenure premise for either object.
        let first = if route == "copy" {
            let donor = trajectories::cast_permanent(&game, 2);
            trajectories::cast_permanent(donor.state(), 1)
        } else {
            trajectories::cast_permanent(&game, 1)
        };
        let target = id(first.state().execution(), 1);
        let before_world = if route == "regain" {
            assert_eq!(age(first.state().execution(), target), Some(1));
            let (removed, _) = trajectories::resolve(first.state(), 3, target);
            assert_eq!(age(removed.state().execution(), target), None);
            removed.state().clone()
        } else {
            assert_eq!(age(first.state().execution(), target), None);
            first.state().clone()
        };
        let other = if route == "copy" {
            first
        } else {
            trajectories::cast_permanent(&before_world, 2)
        };
        let donor = id(other.state().execution(), 2);
        let old_age = age(other.state().execution(), donor).unwrap();
        assert_eq!(age(other.state().execution(), target), None);
        let done = if route == "copy" {
            let activated = trajectories::activate(other.state(), target, donor, false);
            trajectories::pass_pair(activated.state())
        } else {
            trajectories::resolve(other.state(), 4, target).0
        };
        assert_eq!(age(done.state().execution(), target), Some(old_age + 1));
        assert_eq!(age(done.state().execution(), donor), None);
        assert_eq!(id(done.state().execution(), 1), target);
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(target)
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
        assert!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(donor)
                .is_none()
        );
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(id(done.state().execution(), 2))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(P0)
        );
        let changes: Vec<_> = events(done.rules_journal())
            .into_iter()
            .filter_map(|event| match event {
                RulesOccurrence::WorldTenureChanged(change) => Some(*change),
                _ => None,
            })
            .collect();
        assert_eq!(changes.len(), 2);
        assert!(
            matches!(changes[0], WorldTenureChange::Gained{object,tenure} if object==target && tenure.sequence()==old_age+1)
        );
        assert!(matches!(changes[1], WorldTenureChange::Lost{object,..} if object==donor));
    }
}

#[test]
fn world_loss_and_regain_inside_one_resolving_body_renew_age_without_midbody_checks() {
    let game = root(
        vec![
            card(1, P0, artifact(true)),
            trajectories::spell(
                2,
                BodyTarget::SinglePermanent,
                vec![
                    change(TypeOperation::Remove(BTreeSet::from([Supertype::World]))),
                    change(TypeOperation::Add(BTreeSet::from([Supertype::World]))),
                ],
            ),
        ],
        vec![vec![CardId(1)]],
    );
    let target = id(game.execution(), 1);
    let (done, _) = trajectories::resolve(&game, 2, target);
    assert_eq!(age(done.state().execution(), target), Some(2));
    let changes: Vec<_> = events(done.rules_journal())
        .into_iter()
        .filter_map(|e| match e {
            RulesOccurrence::WorldTenureChanged(change) => Some(*change),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 2);
    assert!(matches!(changes[0], WorldTenureChange::Lost { .. }));
    assert!(matches!(changes[1], WorldTenureChange::Gained { .. }));
    assert!(
        !events(done.rules_journal())
            .iter()
            .any(|e| matches!(e,RulesOccurrence::ZoneChange(z) if z.before.object.id()==target))
    );
}

#[test]
fn one_actual_static_entry_gives_two_worlds_equal_latest_tenure_so_even_the_older_world_leaves() {
    use spellwake_engine::characteristics::{
        PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        StaticTypeChange,
    };
    use spellwake_engine::program::CardType;
    let mut enabler = artifact(false);
    enabler.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Add(BTreeSet::from([Supertype::World])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })]),
    });
    let mut entering = card(4, P0, enabler);
    entering.destination = ObjectDestination::Hand;
    let game = root(
        vec![
            card(1, P1, artifact(true)),
            card(
                2,
                P0,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
            ),
            card(
                3,
                P0,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
            ),
            entering,
        ],
        vec![vec![CardId(1)]],
    );
    let cast = trajectories::submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: id(game.execution(), 4),
            payment: None,
        },
    );
    let pending = trajectories::pass_pair(cast.state());
    let decision = pending.state().pending_decision().unwrap();
    let newer = [
        id(pending.state().execution(), 2),
        id(pending.state().execution(), 3),
    ];
    assert_eq!(
        age(
            pending.state().execution(),
            id(pending.state().execution(), 1)
        ),
        Some(1)
    );
    for object in newer {
        assert_eq!(age(pending.state().execution(), object), Some(2));
    }
    assert!(
        pending
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| [CardId(1), CardId(2), CardId(3)]
                .contains(&c.card_id().expect("physical fixture card")))
            .all(|c| c.zone() == ObjectZone::Battlefield)
    );
    let done = trajectories::submit(
        pending.state(),
        P0,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::OrderGraveyard(vec![newer[1], newer[0]]),
        },
    );
    assert_eq!(done.state().execution().world_tenures().count(), 0);
    for card in [1, 2, 3] {
        assert!(matches!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(id(done.state().execution(), card))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(_)
        ));
    }
    assert_eq!(
        done.state()
            .execution()
            .operational_counters()
            .next_world_tenure,
        3
    );
}

#[test]
fn copy_and_source_controller_changes_track_continuity_instead_of_restarting_every_reevaluation() {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::{
            ControlRecipient, ControllerSelector, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts, StaticTypeChange,
        },
        program::{CardType, ExecutableBody},
    };
    let mut receiver = artifact(false);
    receiver.activated = vec![ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CopyTargetToSource {
                duration: ResolvedDuration::NoStatedDuration,
                power_toughness: None,
            }],
        ),
    }];
    let mut enabler = artifact(false);
    enabler.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            controller: ControllerSelector::SourceController,
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Add(BTreeSet::from([Supertype::World])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })]),
    });
    let mut source = card(2, P1, enabler);
    source.destination = ObjectDestination::Battlefield { controller: P0 };
    let game = root(
        vec![
            card(1, P0, receiver),
            source,
            card(3, P1, artifact(false)),
            trajectories::spell(
                4,
                BodyTarget::SinglePermanent,
                vec![trajectories::effect(
                    vec![ResolvedPartSpec::Control(ControlRecipient::TargetOwner)],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![vec![CardId(1)]],
    );
    let receiver = id(game.execution(), 1);
    let donor = id(game.execution(), 3);
    let activated = trajectories::activate(&game, receiver, donor, false);
    let copied = trajectories::pass_pair(activated.state());
    assert_eq!(age(copied.state().execution(), receiver), Some(1));
    assert_eq!(
        copied
            .state()
            .execution()
            .operational_counters()
            .next_world_tenure,
        2
    );
    assert!(
        copied
            .state()
            .execution()
            .copiable_program(receiver)
            .unwrap()
            .supertypes
            .is_empty()
    );
    assert!(
        !events(copied.rules_journal())
            .iter()
            .any(|e| matches!(e, RulesOccurrence::WorldTenureChanged(_)))
    );
    let (controlled, _) =
        trajectories::resolve(copied.state(), 4, id(copied.state().execution(), 2));
    assert_eq!(age(controlled.state().execution(), receiver), None);
    assert_eq!(age(controlled.state().execution(), donor), Some(2));
    assert_eq!(
        controlled
            .state()
            .execution()
            .operational_counters()
            .next_world_tenure,
        3
    );
}

#[test]
fn changing_a_worlds_controller_without_losing_world_preserves_its_tenure() {
    use spellwake_engine::characteristics::ControlRecipient;
    let mut original = card(1, P1, artifact(true));
    original.destination = ObjectDestination::Battlefield { controller: P0 };
    let game = root(
        vec![
            original,
            trajectories::spell(
                2,
                BodyTarget::SinglePermanent,
                vec![trajectories::effect(
                    vec![ResolvedPartSpec::Control(ControlRecipient::TargetOwner)],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![vec![CardId(1)]],
    );
    let target = id(game.execution(), 1);
    let (done, _) = trajectories::resolve(&game, 2, target);
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(target)
            .unwrap()
            .controller(),
        Some(P1)
    );
    assert_eq!(age(done.state().execution(), target), Some(1));
    assert_eq!(
        done.state()
            .execution()
            .operational_counters()
            .next_world_tenure,
        2
    );
    assert!(
        !events(done.rules_journal())
            .iter()
            .any(|e| matches!(e, RulesOccurrence::WorldTenureChanged(_)))
    );
}

#[test]
fn cleanup_expiry_reacquires_world_with_new_tenure_and_removes_the_other_current_world() {
    let remove = trajectories::effect(
        vec![ResolvedPartSpec::Type(TypeChangeSpec {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Remove(BTreeSet::from([Supertype::World])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })],
        ResolvedDuration::UntilEndOfTurn,
    );
    let game = root(
        vec![
            card(1, P0, artifact(true)),
            card(2, P1, artifact(false)),
            trajectories::spell(3, BodyTarget::SinglePermanent, vec![remove]),
            trajectories::spell(
                4,
                BodyTarget::SinglePermanent,
                vec![change(TypeOperation::Add(BTreeSet::from([
                    Supertype::World,
                ])))],
            ),
        ],
        vec![vec![CardId(1)]],
    );
    let first = id(game.execution(), 1);
    let second = id(game.execution(), 2);
    let (removed, _) = trajectories::resolve(&game, 3, first);
    let (gained, _) = trajectories::resolve(removed.state(), 4, second);
    assert_eq!(age(gained.state().execution(), second), Some(2));
    assert_eq!(age(gained.state().execution(), first), None);
    // This turn-3 fixture performs an SBA during cleanup, so it has the required
    // same-cleanup priority window. The generic first-turn/no-SBA helper does not apply.
    let end = trajectories::reach_end(gained.state(), [P0, P1]);
    let cleaned = trajectories::pass_pair(&end);
    assert_eq!(age(cleaned.state().execution(), first), Some(3));
    assert_eq!(age(cleaned.state().execution(), second), None);
    assert_eq!(
        cleaned
            .state()
            .execution()
            .window()
            .cards()
            .get(id(cleaned.state().execution(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P1)
    );
    assert_eq!(
        cleaned.state().turn().unwrap().position().step,
        Step::Cleanup
    );
    assert_eq!(cleaned.state().turn().unwrap().position().number, 3);
    let repeated = trajectories::pass_pair(cleaned.state());
    assert_eq!(age(repeated.state().execution(), first), Some(3));
    assert_eq!(
        repeated
            .state()
            .execution()
            .operational_counters()
            .next_world_tenure,
        4
    );
    assert_eq!(
        repeated.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    assert_eq!(repeated.state().turn().unwrap().position().number, 4);
}
