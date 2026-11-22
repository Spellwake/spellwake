//! Prepared public-root mechanisms, with real mana activation/payment and priority.
//! Literal partial programs and seeded zones do not qualify complete cards or formats.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod tr;

use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use spellwake_engine::{
    combat::CombatRules,
    effects::{EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::BasicLandType,
    life::LifeAdjustment,
    mana::{ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{CastOccurrence, RulesOccurrence, RulesRecord, TriggerRelated, WaitingInstance},
    priority::{PriorityPhase, SeatId},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, PlayerRelation, Program,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
    },
    setup::GameSeed,
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{collections::BTreeSet, sync::Arc};
use tr::{P0, P1};

fn gain(amount: i64) -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(amount.into()),
    }
}
fn ability(id: u64, condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(id),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![gain(1)]),
    }
}
fn watched(mut program: Program, condition: TriggerSpec) -> Program {
    program.triggered.push(ability(1, condition));
    program
}
fn instruction() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(BodyTarget::None, vec![gain(2)]),
    )
}
fn cast_permanent(creature: bool) -> Program {
    if creature {
        Program::creature(
            Some(ManaCost::one(ManaType::Blue)),
            1.into(),
            1.into(),
            BTreeSet::new(),
        )
    } else {
        Program::artifact(Some(ManaCost::one(ManaType::Blue)), vec![])
    }
}
fn named(mut program: Program) -> Program {
    program.names =
        ObjectNames::single(EnglishCardName::new("Cast identity fixture".into()).unwrap());
    program
}
fn initial(mut cards: Vec<SeedCard>, players: Vec<SeatId>, active: SeatId) -> GameState {
    let vocabulary = tr::vocabulary();
    for player in &players {
        for n in 0..3 {
            cards.push(SeedCard {
                card: CardId(1000 + u64::from(player.0) * 10 + n),
                owner: *player,
                destination: ObjectDestination::Battlefield {
                    controller: *player,
                },
                program: Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
            });
        }
        for n in 0..8 {
            cards.push(tr::library(
                2000 + u64::from(player.0) * 10 + n,
                *player,
                &vocabulary,
            ));
        }
    }
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        players,
        active,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(NameRules::literal_names()),
    );
    let context = GameContext::new(
        "cast-trigger-mechanisms".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal-partial-definitions".into(),
        "cast-trigger-v1".into(),
        "prepared-main-with-seeded-islands".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let state = GameState::initialize_scheduled(
        GameId(6012),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    assert!(state.pending_decision().is_none());
    for player in state.execution().window().participants() {
        assert_eq!(state.execution().mana(*player, ManaType::Blue), 0.into());
    }
    state
}
fn two(cards: Vec<SeedCard>) -> GameState {
    initial(cards, vec![P0, P1], P0)
}
fn fund(game: &GameState, actor: SeatId, index: u64) -> NativeCandidate {
    tr::submit(
        game,
        actor,
        GameCommand::ActivateMana {
            source: tr::object(game, 1000 + u64::from(actor.0) * 10 + index),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    )
}
fn payment() -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Blue,
            amount: 1.into(),
        }],
    }
}
fn paid(
    game: &GameState,
    actor: SeatId,
    card: u64,
    index: u64,
    target: Option<TargetRef>,
) -> NativeCandidate {
    let funded = fund(game, actor, index);
    let spell = tr::object(funded.state(), card);
    let command = if let Some(target) = target {
        GameCommand::CastPaid {
            spell,
            target,
            payment: payment(),
        }
    } else {
        GameCommand::CastUntargeted {
            spell,
            payment: Some(payment()),
        }
    };
    let cast = tr::submit(funded.state(), actor, command);
    assert!(cast.mana_payment().is_some());
    assert_eq!(
        cast.state().execution().mana(actor, ManaType::Blue),
        0.into()
    );
    cast
}
fn discoveries(candidate: &NativeCandidate) -> Vec<&WaitingInstance> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(value) => Some(value.as_ref()),
            _ => None,
        })
        .collect()
}
fn completed(candidate: &NativeCandidate) -> &CastOccurrence {
    let values: Vec<_> = tr::all_occurrences(candidate)
        .into_iter()
        .filter_map(|value| match value {
            RulesOccurrence::CastCompleted(value) => Some(value.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(values.len(), 1);
    values[0]
}
fn resolve(game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected priority")
    };
    let players = game.execution().window().participants();
    let first = players.iter().position(|player| *player == holder).unwrap();
    let mut state = game.clone();
    for index in 0..players.len() - 1 {
        state = tr::submit(
            &state,
            players[(first + index) % players.len()],
            GameCommand::Pass,
        )
        .into_state();
    }
    tr::submit(
        &state,
        players[(first + players.len() - 1) % players.len()],
        GameCommand::Pass,
    )
}
fn entry(game: &GameState, card: u64) -> StackEntryId {
    let object = tr::object(game, card);
    game.execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.kind() == &StackKind::CardSpell(object))
        .unwrap()
        .id()
}
fn assert_cause(waiting: &WaitingInstance, cast: &CastOccurrence) {
    assert_eq!(waiting.causes.len(), 1);
    let member = &waiting.causes.members()[0];
    assert_eq!(member.related_player, Some(cast.controller()));
    assert_eq!(
        member.related,
        TriggerRelated::SpellCast(Box::new(cast.clone()))
    );
}

#[test]
fn permanent_self_cast_triggers_on_stack_before_entry_and_uses_exact_identity() {
    let self_cast = named(watched(
        cast_permanent(false),
        spellwake_engine::program::TriggerSpec::self_cast(),
    ));
    let game = two(vec![
        tr::card(1, self_cast.clone(), false),
        tr::card(2, self_cast.clone(), true),
        tr::card(3, self_cast, false),
    ]);
    let hand = tr::object(&game, 1);
    let cast = paid(&game, P0, 1, 0, None);
    let cause = completed(&cast);
    assert_ne!(cause.spell().object.id(), hand);
    assert_eq!(cause.spell().object.zone(), ObjectZone::Stack);
    let found = discoveries(&cast);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source.object.id(), cause.spell().object.id());
    assert_cause(found[0], cause);
    assert_eq!(cast.state().execution().window().entries().len(), 2);
    let trigger = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let binding = cast
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.causes, found[0].causes);
    assert_eq!(
        cast.state()
            .execution()
            .stack_copiable_triggered_abilities(trigger),
        None
    );
    assert_eq!(
        cast.state()
            .execution()
            .stack_copiable_triggered_abilities(cause.entry()),
        Some(
            [ability(
                1,
                spellwake_engine::program::TriggerSpec::self_cast()
            )]
            .as_slice()
        )
    );
    let triggered = resolve(cast.state());
    assert_eq!(
        triggered.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert_eq!(
        triggered
            .state()
            .execution()
            .window()
            .cards()
            .get(tr::object(triggered.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Stack
    );
    let entered = resolve(triggered.state());
    assert_eq!(
        entered
            .state()
            .execution()
            .window()
            .cards()
            .get(tr::object(entered.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert!(discoveries(&entered).is_empty());
    assert!(entered.state().execution().window().entries().is_empty());
}

#[test]
fn instruction_self_cast_executes_independently_above_targeted_original() {
    let spell = watched(
        Program::instant(
            ManaCost::one(ManaType::Blue),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                }],
            ),
        ),
        spellwake_engine::program::TriggerSpec::self_cast(),
    );
    let game = two(vec![tr::card(1, spell, false)]);
    let cast = paid(&game, P0, 1, 0, Some(TargetRef::Player(P1)));
    assert_eq!(discoveries(&cast).len(), 1);
    assert_cause(discoveries(&cast)[0], completed(&cast));
    let trigger = resolve(cast.state());
    assert_eq!(trigger.state().execution().life_total(P0), Some(&21.into()));
    assert_eq!(trigger.state().execution().life_total(P1), Some(&20.into()));
    let original = resolve(trigger.state());
    assert_eq!(
        original.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert_eq!(
        original.state().execution().life_total(P1),
        Some(&22.into())
    );
    assert!(discoveries(&original).is_empty());
}

#[test]
fn watcher_relations_and_type_filters_use_the_actual_caster_and_current_types() {
    for controller in [P0, P1] {
        for relation in [
            PlayerRelation::Controller,
            PlayerRelation::Opponent,
            PlayerRelation::AnyParticipant,
        ] {
            for (filter, expected_types) in [
                (
                    spellwake_engine::program::SpellFilter::any(),
                    [true, true, true],
                ),
                (
                    spellwake_engine::program::SpellFilter::noncreature(),
                    [true, true, false],
                ),
                (
                    spellwake_engine::program::SpellFilter::instant_or_sorcery(),
                    [true, false, false],
                ),
                (
                    spellwake_engine::program::SpellFilter::of_type(CardType::Artifact),
                    [false, true, false],
                ),
            ] {
                for (index, spell) in [instruction(), cast_permanent(false), cast_permanent(true)]
                    .into_iter()
                    .enumerate()
                {
                    let observer = watched(
                        Program::artifact(None, vec![]),
                        spellwake_engine::program::TriggerSpec::spell_cast(relation, filter),
                    );
                    // Owner is deliberately different from watcher controller.
                    let owner = if controller == P0 { P1 } else { P0 };
                    let game = two(vec![
                        tr::card(1, spell, false),
                        tr::controlled(2, owner, controller, observer),
                    ]);
                    let cast = paid(&game, P0, 1, 0, None);
                    let relation_matches = match relation {
                        PlayerRelation::Controller => controller == P0,
                        PlayerRelation::Opponent => controller != P0,
                        PlayerRelation::AnyParticipant => true,
                    };
                    let matched = relation_matches && expected_types[index];
                    let found = discoveries(&cast);
                    assert_eq!(
                        found.len(),
                        usize::from(matched),
                        "{relation:?} {filter:?} {controller:?} case {index}"
                    );
                    if matched {
                        assert_eq!(found[0].controller, controller);
                        assert_eq!(found[0].source.object.id(), tr::object(cast.state(), 2));
                        assert_cause(found[0], completed(&cast));
                        let trigger = resolve(cast.state());
                        assert_eq!(
                            trigger.state().execution().life_total(controller),
                            Some(&21.into())
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn hidden_watchers_and_other_self_cast_sources_do_not_trigger() {
    let watcher = watched(
        Program::artifact(None, vec![]),
        spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::AnyParticipant,
            spellwake_engine::program::SpellFilter::any(),
        ),
    );
    let mut cards = vec![
        tr::card(1, instruction(), false),
        tr::card(2, watcher.clone(), false),
        tr::card(
            5,
            watched(
                cast_permanent(false),
                spellwake_engine::program::TriggerSpec::self_cast(),
            ),
            true,
        ),
    ];
    for (id, destination) in [
        (3, ObjectDestination::LibraryBottom),
        (4, ObjectDestination::Graveyard),
    ] {
        cards.push(SeedCard {
            card: CardId(id),
            owner: P0,
            destination,
            program: watcher.clone(),
        });
    }
    let game = two(cards);
    let cast = paid(&game, P0, 1, 0, None);
    assert!(discoveries(&cast).is_empty());
    assert_eq!(cast.state().execution().window().entries().len(), 1);
    assert_eq!(
        cast.state()
            .execution()
            .stack_copiable_triggered_abilities(completed(&cast).entry()),
        Some([].as_slice())
    );
    let original = completed(&cast).entry();
    let done = resolve(cast.state());
    assert_eq!(
        done.state()
            .execution()
            .stack_copiable_triggered_abilities(original),
        None
    );
}

#[test]
fn cast_cohort_uses_nonfirst_active_apnap_order_and_rejects_malformed_replies() {
    let p2 = SeatId(2);
    let condition = spellwake_engine::program::TriggerSpec::spell_cast(
        PlayerRelation::AnyParticipant,
        spellwake_engine::program::SpellFilter::any(),
    );
    let mut active_watcher = watched(Program::artifact(None, vec![]), condition.clone());
    active_watcher.triggered.push(ability(2, condition.clone()));
    let game = initial(
        vec![
            SeedCard {
                card: CardId(1),
                owner: P1,
                destination: ObjectDestination::Hand,
                program: instruction(),
            },
            tr::controlled(2, P0, P1, active_watcher),
            tr::controlled(
                3,
                p2,
                p2,
                watched(Program::artifact(None, vec![]), condition.clone()),
            ),
            tr::controlled(
                4,
                P0,
                P0,
                watched(Program::artifact(None, vec![]), condition),
            ),
        ],
        vec![P0, P1, p2],
        P1,
    );
    let cast = paid(&game, P1, 1, 0, None);
    let found: Vec<_> = discoveries(&cast).into_iter().cloned().collect();
    assert_eq!(found.len(), 4);
    for waiting in &found {
        assert_cause(waiting, completed(&cast));
    }
    let state = cast.state();
    let pending = state.pending_decision().unwrap();
    assert_eq!(pending.chooser(), P1);
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!("expected active cohort")
    };
    assert_eq!(frame.instances().len(), 2);
    assert_eq!(state.execution().waiting_triggers().count(), 4);
    let ids = frame.instances().to_vec();
    let foreign = found
        .iter()
        .find(|waiting| waiting.controller != P1)
        .unwrap()
        .id;
    let saved = state.clone();
    for selected in [
        vec![],
        vec![ids[0]],
        vec![ids[0], ids[0]],
        vec![ids[0], foreign],
    ] {
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis().clone(),
                actor: P1,
                command: GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::OrderTriggers(selected)
                }
            }),
            Err(GameError::Effect(EffectError::Trigger(
                spellwake_engine::occurrence::TriggerError::InvalidTriggerOrder
            )))
        );
        assert_eq!(state, &saved);
    }
    assert_eq!(
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor: P0,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderTriggers(ids.clone())
            }
        }),
        Err(GameError::WrongChooser)
    );
    assert_eq!(state, &saved);
    let accepted = tr::reply(
        state,
        P1,
        DecisionReply::OrderTriggers(vec![ids[1], ids[0]]),
    );
    let placed: Vec<_> = accepted
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(value) => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(
        placed
            .iter()
            .map(|value| value.controller)
            .collect::<Vec<_>>(),
        vec![P1, P1, p2, P0]
    );
    assert_eq!(placed[0].instance, ids[1]);
    assert_eq!(placed[1].instance, ids[0]);
    for placement in placed {
        let waiting = found
            .iter()
            .find(|waiting| waiting.id == placement.instance)
            .unwrap();
        let binding = accepted
            .state()
            .execution()
            .execution_bindings(placement.entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap();
        assert_eq!(binding.source, waiting.source);
        assert_eq!(binding.causes, waiting.causes);
    }
    assert_eq!(accepted.state().execution().waiting_triggers().count(), 0);
    let mut state = accepted.into_state();
    for _ in 0..4 {
        state = resolve(&state).into_state();
    }
    assert_eq!(state.execution().life_total(P0), Some(&21.into()));
    assert_eq!(state.execution().life_total(P1), Some(&22.into()));
    assert_eq!(state.execution().life_total(p2), Some(&21.into()));
    assert_eq!(state.execution().window().entries().len(), 1);
}

fn order_all(mut game: GameState) -> GameState {
    while let Some(pending) = game.pending_decision() {
        let RuleFrame::Triggers(frame) = pending.frame() else {
            panic!("expected trigger order")
        };
        game = tr::reply(
            &game,
            pending.chooser(),
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        )
        .into_state();
    }
    game
}

#[test]
fn copies_and_copies_of_copies_keep_self_cast_definitions_without_becoming_casts() {
    let condition = spellwake_engine::program::TriggerSpec::spell_cast(
        PlayerRelation::Controller,
        spellwake_engine::program::SpellFilter::any(),
    );
    let copier = Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![Instruction::CopySpell],
        ),
    );
    let counter = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![Instruction::CounterSpell],
        ),
    );
    let game = two(vec![
        tr::card(
            1,
            watched(
                instruction(),
                spellwake_engine::program::TriggerSpec::self_cast(),
            ),
            false,
        ),
        tr::card(2, copier.clone(), false),
        tr::card(3, copier, false),
        SeedCard {
            card: CardId(4),
            owner: P1,
            destination: ObjectDestination::Hand,
            program: counter,
        },
        tr::card(5, watched(Program::artifact(None, vec![]), condition), true),
    ]);
    let cast = paid(&game, P0, 1, 0, None);
    assert_eq!(discoveries(&cast).len(), 2);
    let original = entry(cast.state(), 1);
    let mut game = order_all(cast.into_state());
    game = resolve(&game).into_state();
    game = resolve(&game).into_state();
    let mut donor = original;
    let mut copies = Vec::new();
    for (card, index) in [(2, 1), (3, 2)] {
        let cast = paid(&game, P0, card, index, Some(TargetRef::Stack(donor)));
        assert_eq!(
            discoveries(&cast).len(),
            1,
            "the real copier cast triggers its watcher"
        );
        game = resolve(cast.state()).into_state();
        let copied = resolve(&game);
        // This donor has no targets, so the existing executor copies it directly.
        assert!(copied.state().pending_decision().is_none());
        assert_eq!(
            copied.state().next_decision_sequence(),
            game.next_decision_sequence()
        );
        assert!(discoveries(&copied).is_empty());
        assert!(
            !tr::all_occurrences(&copied)
                .iter()
                .any(|value| matches!(value, RulesOccurrence::CastCompleted(_)))
        );
        let facts: Vec<_> = tr::all_occurrences(&copied)
            .into_iter()
            .filter_map(|value| match value {
                RulesOccurrence::CopyCreated {
                    entry,
                    original,
                    copiable_triggered,
                    ..
                } => Some((entry, original, copiable_triggered)),
                _ => None,
            })
            .collect();
        assert_eq!(facts.len(), 1);
        assert_eq!(*facts[0].1, donor);
        assert_eq!(
            facts[0].2.as_slice(),
            [ability(
                1,
                spellwake_engine::program::TriggerSpec::self_cast()
            )]
            .as_slice()
        );
        donor = copied
            .state()
            .execution()
            .window()
            .entries()
            .last()
            .unwrap()
            .id();
        assert_eq!(*facts[0].0, donor);
        assert_eq!(
            copied
                .state()
                .execution()
                .stack_copiable_triggered_abilities(donor),
            Some(
                [ability(
                    1,
                    spellwake_engine::program::TriggerSpec::self_cast()
                )]
                .as_slice()
            )
        );
        copies.push(donor);
        game = copied.into_state();
    }
    game = tr::submit(&game, P0, GameCommand::Pass).into_state();
    let countered = tr::submit(
        &game,
        P1,
        GameCommand::Cast {
            spell: tr::object(&game, 4),
            target: TargetRef::Stack(original),
        },
    );
    assert!(discoveries(&countered).is_empty());
    game = resolve(countered.state()).into_state();
    assert_eq!(
        game.execution()
            .stack_copiable_triggered_abilities(original),
        None
    );
    for copy in copies {
        assert_eq!(
            game.execution().stack_copiable_triggered_abilities(copy),
            Some(
                [ability(
                    1,
                    spellwake_engine::program::TriggerSpec::self_cast()
                )]
                .as_slice()
            )
        );
    }
    assert_eq!(game.execution().life_total(P0), Some(&24.into()));
    game = resolve(&game).into_state();
    game = resolve(&game).into_state();
    assert_eq!(game.execution().life_total(P0), Some(&28.into()));
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn failed_casts_preserve_the_whole_root_including_event_and_trigger_allocators() {
    let game = two(vec![
        tr::card(
            1,
            watched(
                instruction(),
                spellwake_engine::program::TriggerSpec::self_cast(),
            ),
            false,
        ),
        tr::card(2, cast_permanent(false), true),
    ]);
    let saved = game.clone();
    let spell = tr::object(&game, 1);
    for command in [
        GameCommand::CastUntargeted {
            spell,
            payment: Some(payment()),
        },
        GameCommand::CastUntargeted {
            spell: tr::object(&game, 2),
            payment: None,
        },
        GameCommand::CastPaid {
            spell,
            target: TargetRef::Player(P1),
            payment: payment(),
        },
    ] {
        assert!(matches!(
            game.advance(GameInput {
                basis: game.basis().clone(),
                actor: P0,
                command
            }),
            Err(GameError::Effect(_))
        ));
        assert_eq!(game, saved);
    }
    let success = paid(&game, P0, 1, 0, None);
    assert_eq!(discoveries(&success).len(), 1);
    assert_eq!(success.state().execution().window().entries().len(), 2);
}

#[test]
fn countering_the_original_before_its_trigger_resolves_keeps_the_owned_cast_cause() {
    let counter = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![Instruction::CounterSpell],
        ),
    );
    let game = two(vec![
        tr::card(
            1,
            watched(
                instruction(),
                spellwake_engine::program::TriggerSpec::self_cast(),
            ),
            false,
        ),
        SeedCard {
            card: CardId(2),
            owner: P1,
            destination: ObjectDestination::Hand,
            program: counter,
        },
    ]);
    let cast = paid(&game, P0, 1, 0, None);
    let captured = discoveries(&cast)[0].clone();
    let original = completed(&cast).entry();
    let game = tr::submit(cast.state(), P0, GameCommand::Pass).into_state();
    let counter = tr::submit(
        &game,
        P1,
        GameCommand::Cast {
            spell: tr::object(&game, 2),
            target: TargetRef::Stack(original),
        },
    );
    let countered = resolve(counter.state());
    assert_eq!(countered.state().execution().window().entries().len(), 1);
    assert_ne!(
        tr::object(countered.state(), 1),
        captured.source.object.id()
    );
    let trigger = countered.state().execution().window().entries()[0].id();
    let binding = countered
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.causes, captured.causes);
    assert_eq!(binding.source, captured.source);
    let resolved = resolve(countered.state());
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert!(resolved.state().execution().window().entries().is_empty());
}

#[test]
fn watcher_departure_does_not_destroy_its_already_placed_body() {
    let condition = spellwake_engine::program::TriggerSpec::spell_cast(
        PlayerRelation::Controller,
        spellwake_engine::program::SpellFilter::of_type(CardType::Sorcery),
    );
    let watcher = watched(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        condition,
    );
    let sorcery = Program::sorcery(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(BodyTarget::None, vec![gain(2)]),
    );
    let bounce = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::ReturnToOwnersHand],
        ),
    );
    let game = two(vec![
        tr::card(1, sorcery, false),
        tr::card(2, watcher, true),
        tr::card(3, bounce, false),
    ]);
    let cast = paid(&game, P0, 1, 0, None);
    let captured = discoveries(&cast)[0].clone();
    let bounce = tr::submit(
        cast.state(),
        P0,
        GameCommand::Cast {
            spell: tr::object(cast.state(), 3),
            target: TargetRef::Object(tr::object(cast.state(), 2)),
        },
    );
    assert!(discoveries(&bounce).is_empty());
    let bounced = resolve(bounce.state());
    assert_ne!(tr::object(bounced.state(), 2), captured.source.object.id());
    assert_eq!(
        bounced
            .state()
            .execution()
            .window()
            .cards()
            .get(tr::object(bounced.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    let trigger = bounced
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let binding = bounced
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source, captured.source);
    assert_eq!(binding.causes, captured.causes);
    let triggered = resolve(bounced.state());
    let resolved = resolve(triggered.state());
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&23.into())
    );
    assert!(resolved.state().execution().window().entries().is_empty());
}

#[test]
fn watcher_control_change_after_discovery_keeps_the_original_trigger_controller() {
    use spellwake_engine::characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec};
    let condition = spellwake_engine::program::TriggerSpec::spell_cast(
        PlayerRelation::Controller,
        spellwake_engine::program::SpellFilter::any(),
    );
    let control = Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![tr::effect(
                vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )],
                ResolvedDuration::NoStatedDuration,
            )],
        ),
    );
    let game = two(vec![
        tr::card(1, instruction(), false),
        tr::card(2, watched(Program::artifact(None, vec![]), condition), true),
        SeedCard {
            card: CardId(3),
            owner: P1,
            destination: ObjectDestination::Hand,
            program: control,
        },
    ]);
    let watcher = tr::object(&game, 2);
    let cast = paid(&game, P0, 1, 0, None);
    let discovered = discoveries(&cast)[0].clone();
    let captured_cast = completed(&cast).clone();
    let trigger = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let old_binding = cast
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .clone()
        .unwrap();
    assert_eq!(discovered.controller, P0);
    assert_cause(&discovered, &captured_cast);
    let response = tr::submit(cast.state(), P0, GameCommand::Pass).into_state();
    let response = paid(&response, P1, 3, 0, Some(TargetRef::Object(watcher)));
    assert!(discoveries(&response).is_empty());
    let changed = resolve(response.state());
    assert_eq!(tr::object(changed.state(), 2), watcher);
    assert_eq!(
        changed
            .state()
            .execution()
            .window()
            .cards()
            .get(watcher)
            .unwrap()
            .controller(),
        Some(P1)
    );
    assert!(tr::all_occurrences(&changed).iter().any(|value| matches!(value, RulesOccurrence::ControllerChanged(change) if change.before.id() == watcher && change.before.controller() == Some(P0) && change.after.controller() == Some(P1))));
    let actual_trigger = changed
        .state()
        .execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.id() == trigger)
        .unwrap();
    assert_eq!(actual_trigger.controller(), P0);
    let binding = changed
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding, &old_binding);
    assert_eq!(binding.source, discovered.source);
    assert_eq!(binding.source.object.controller(), Some(P0));
    assert_eq!(binding.causes, discovered.causes);
    let resolved = resolve(changed.state());
    assert_eq!(
        resolved.state().execution().life_total(P0),
        Some(&21.into())
    );
    assert_eq!(
        resolved.state().execution().life_total(P1),
        Some(&20.into())
    );
    let original = resolve(resolved.state());
    assert_eq!(
        original.state().execution().life_total(P0),
        Some(&23.into())
    );
    assert_eq!(
        original.state().execution().life_total(P1),
        Some(&20.into())
    );
    assert!(original.state().execution().window().entries().is_empty());
}

#[test]
fn removed_watcher_ability_is_unavailable_at_the_completed_cast_cut() {
    use spellwake_engine::characteristics::{
        AbilityChange, AbilityRemoval, ResolvedDuration, ResolvedPartSpec,
    };
    let condition = spellwake_engine::program::TriggerSpec::spell_cast(
        PlayerRelation::Controller,
        spellwake_engine::program::SpellFilter::of_type(CardType::Artifact),
    );
    let removal = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![tr::effect(
                vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                    AbilityRemoval::All,
                ))],
                ResolvedDuration::NoStatedDuration,
            )],
        ),
    );
    let game = two(vec![
        tr::card(1, cast_permanent(false), false),
        tr::card(2, watched(Program::artifact(None, vec![]), condition), true),
        tr::card(3, removal, false),
    ]);
    let removed = tr::submit(
        &game,
        P0,
        GameCommand::Cast {
            spell: tr::object(&game, 3),
            target: TargetRef::Object(tr::object(&game, 2)),
        },
    );
    assert!(discoveries(&removed).is_empty());
    let removed = resolve(removed.state());
    let current = removed
        .state()
        .execution()
        .current_characteristics(tr::object(removed.state(), 2))
        .unwrap()
        .unwrap();
    assert!(current.abilities.triggered.is_empty());
    assert_eq!(current.printed_text.triggered.len(), 1);
    let cast = paid(removed.state(), P0, 1, 0, None);
    assert!(discoveries(&cast).is_empty());
    assert_eq!(cast.state().execution().window().entries().len(), 1);
}

#[test]
fn each_grouping_uses_the_real_single_cast_cause_for_bound_draw_count() {
    for grouping in [
        TriggerGrouping::EachOccurrence,
        TriggerGrouping::OneOrMoreTogether,
        TriggerGrouping::OneOrMoreByPlayer,
    ] {
        let mut spell = watched(
            instruction(),
            spellwake_engine::program::TriggerSpec::self_cast(),
        );
        spell.triggered[0].grouping = grouping;
        spell.triggered[0].body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::event(
                    spellwake_engine::program::EventValueKind::MatchedOccurrences,
                ),
            }],
        );
        let game = two(vec![tr::card(1, spell, false)]);
        let cast = paid(&game, P0, 1, 0, None);
        let found = discoveries(&cast);
        assert_eq!(found.len(), 1);
        assert_cause(found[0], completed(&cast));
        let drawn = resolve(cast.state());
        assert_eq!(drawn.draw_journal().events.len(), 1);
        assert_eq!(
            drawn.state().execution().window().cards().library(P0).len(),
            7
        );
        assert_eq!(drawn.state().execution().window().entries().len(), 1);
    }
}

#[test]
fn illegal_sorcery_timing_and_missing_target_reject_before_new_cast_facts() {
    let sorcery = watched(
        Program::sorcery(
            ManaCost::one(ManaType::Blue),
            ExecutableBody::new(BodyTarget::None, vec![gain(2)]),
        ),
        spellwake_engine::program::TriggerSpec::self_cast(),
    );
    let targeted = watched(
        Program::instant(
            ManaCost::one(ManaType::Blue),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                }],
            ),
        ),
        spellwake_engine::program::TriggerSpec::self_cast(),
    );
    let game = two(vec![
        tr::card(1, instruction(), false),
        tr::card(2, sorcery, false),
        tr::card(3, targeted, false),
    ]);
    let original = paid(&game, P0, 1, 0, None);
    let funded = fund(original.state(), P0, 1);
    let state = funded.state();
    let saved = state.clone();
    for card in [2, 3] {
        let result = state.advance(GameInput {
            basis: state.basis().clone(),
            actor: P0,
            command: GameCommand::CastUntargeted {
                spell: tr::object(state, card),
                payment: Some(payment()),
            },
        });
        if card == 2 {
            assert!(matches!(
                result,
                Err(GameError::Effect(EffectError::Timing(_)))
            ));
        } else {
            assert_eq!(result, Err(GameError::Effect(EffectError::TargetRequired)));
        }
        assert_eq!(state, &saved);
    }
    assert_eq!(state.execution().mana(P0, ManaType::Blue), 1.into());
}

#[path = "cast_triggers/targeted.rs"]
mod targeted;

#[path = "cast_triggers/conditional.rs"]
mod conditional;
