#![allow(dead_code, unused_imports)]
//! Focused public-root behavior. Selected reference evidence is frozen/run separately.
use super::*;
use spellwake_engine::occurrence::{TriggerRemovalReason, TriggerTargetChoice};

fn targeted(mut source: Program, target: BodyTarget, instructions: Vec<Instruction>) -> Program {
    source.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(target, instructions),
    });
    source
}
fn target_frame(game: &GameState) -> &TriggerTargetChoice {
    let RuleFrame::TriggerTarget(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected actual placement target frame")
    };
    frame
}
fn select(game: &GameState, target: TargetRef) -> NativeCandidate {
    tr::reply(
        game,
        target_frame(game).chooser(),
        DecisionReply::TriggerTarget(target),
    )
}
fn hand(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program,
    }
}
fn gain_target(amount: i64) -> Instruction {
    spellwake_engine::program::Instruction::AdjustLife(Box::new(
        spellwake_engine::toolbox::AdjustLifeSpec::gain(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::TargetPlayer,
            ),
            spellwake_engine::program::ValueExpr::Literal(amount.into()),
        ),
    ))
}
fn assert_empty(game: &GameState) {
    assert!(game.execution().window().entries().is_empty());
    assert_eq!(game.execution().waiting_triggers().count(), 0);
    assert!(game.pending_decision().is_none());
    assert_eq!(game.execution().pending_trigger_placement().unwrap(), None);
}

#[test]
fn target_player_is_distinct_from_caster_and_captured_watcher_controller() {
    let p2 = SeatId(2);
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::AnyParticipant,
            spellwake_engine::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::SinglePlayer, vec![gain_target(2), gain(1)]),
    });
    let game = initial(
        vec![
            hand(1, P1, instruction()),
            tr::controlled(2, P0, P0, watcher),
        ],
        vec![P0, P1, p2],
        P1,
    );
    let cast = paid(&game, P1, 1, 0, None);
    assert_eq!(target_frame(cast.state()).chooser(), P0);
    assert_eq!(
        target_frame(cast.state()).domain(),
        &[
            TargetRef::Player(P0),
            TargetRef::Player(P1),
            TargetRef::Player(p2)
        ]
    );
    assert_cause(discoveries(&cast)[0], completed(&cast));
    let chosen = select(cast.state(), TargetRef::Player(p2));
    let actual = chosen
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert_eq!(actual.controller(), P0);
    let binding = chosen
        .state()
        .execution()
        .execution_bindings(actual.id())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source.object.id(), tr::object(&game, 2));
    let trigger = resolve(chosen.state());
    let done = resolve(trigger.state());
    for (player, life) in [(P0, 21), (P1, 22), (p2, 22)] {
        assert_eq!(
            done.state().execution().life_total(player),
            Some(&life.into())
        );
    }
    assert_empty(done.state());
}

#[test]
fn self_cast_trigger_can_counter_causing_spell_and_keep_its_owned_body() {
    let source = targeted(
        instruction(),
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        vec![Instruction::CounterSpell],
    );
    let game = two(vec![hand(1, P0, source)]);
    let cast = paid(&game, P0, 1, 0, None);
    let original = entry(cast.state(), 1);
    assert_eq!(
        target_frame(cast.state()).domain(),
        &[TargetRef::Stack(original)]
    );
    assert_ne!(target_frame(cast.state()).entry(), original);
    let chosen = select(cast.state(), TargetRef::Stack(original));
    let done = resolve(chosen.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(tr::object(done.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_empty(done.state());
}

#[test]
fn spell_copy_target_gate_uses_existing_copy_owner_without_an_extra_cast() {
    let source = targeted(
        instruction(),
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        vec![Instruction::CopySpell],
    );
    let game = two(vec![hand(1, P0, source)]);
    let cast = paid(&game, P0, 1, 0, None);
    let original = entry(cast.state(), 1);
    let chosen = select(cast.state(), TargetRef::Stack(original));
    let copied = resolve(chosen.state());
    assert_eq!(copied.state().execution().window().entries().len(), 2);
    assert!(
        !tr::all_occurrences(&copied)
            .iter()
            .any(|value| matches!(value, RulesOccurrence::CastCompleted(_)))
    );
    assert!(copied.state().pending_decision().is_none());
    let copy = resolve(copied.state());
    let done = resolve(copy.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&24.into()));
    assert_empty(done.state());
}

#[test]
fn off_domain_wrong_kind_wrong_actor_and_normal_action_preserve_pending_root() {
    let source = targeted(
        instruction(),
        BodyTarget::SinglePlayer,
        vec![gain_target(1)],
    );
    let game = two(vec![hand(1, P0, source)]);
    let cast = paid(&game, P0, 1, 0, None);
    let pending = cast.state();
    let saved = pending.clone();
    let decision = pending.pending_decision().unwrap().id();
    for (actor, command) in [
        (
            P1,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::TriggerTarget(TargetRef::Player(P0)),
            },
        ),
        (
            P0,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::TriggerTarget(TargetRef::Player(SeatId(99))),
            },
        ),
        (
            P0,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::OrderTriggers(vec![]),
            },
        ),
        (P0, GameCommand::Pass),
    ] {
        assert!(
            pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor,
                    command
                })
                .is_err()
        );
        assert_eq!(pending, &saved);
    }
    for target in [
        TargetRef::Object(tr::object(pending, 1)),
        TargetRef::Stack(entry(pending, 1)),
    ] {
        assert!(
            pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: P0,
                    command: GameCommand::Reply {
                        decision,
                        choice: DecisionReply::TriggerTarget(target)
                    }
                })
                .is_err()
        );
        assert_eq!(pending, &saved);
    }
    let wrong_id = spellwake_engine::game::DecisionId(decision.0 + 1);
    assert!(
        pending
            .advance(GameInput {
                basis: pending.basis().clone(),
                actor: P0,
                command: GameCommand::Reply {
                    decision: wrong_id,
                    choice: DecisionReply::TriggerTarget(TargetRef::Player(P0))
                }
            })
            .is_err()
    );
    let mut foreign = pending.basis().clone();
    foreign.game = GameId(999999);
    assert!(
        pending
            .advance(GameInput {
                basis: foreign,
                actor: P0,
                command: GameCommand::Reply {
                    decision,
                    choice: DecisionReply::TriggerTarget(TargetRef::Player(P0))
                }
            })
            .is_err()
    );
    assert_eq!(pending, &saved);
    let input = GameInput {
        basis: pending.basis().clone(),
        actor: P0,
        command: GameCommand::Reply {
            decision,
            choice: DecisionReply::TriggerTarget(TargetRef::Player(P0)),
        },
    };
    let accepted = pending.advance(input.clone()).unwrap();
    assert!(accepted.state().advance(input).is_err());
    assert_eq!(pending, &saved);
}

#[test]
fn nonfirst_active_mixed_cohorts_choose_targets_in_accepted_order_before_priority() {
    let p2 = SeatId(2);
    let mut cards = vec![hand(1, P1, instruction())];
    for (id, player) in [(10, P0), (11, P1), (12, p2)] {
        let mut watcher = Program::artifact(None, vec![]);
        for (definition, target, body) in [
            (1, BodyTarget::None, vec![gain(1)]),
            (2, BodyTarget::SinglePlayer, vec![gain_target(2)]),
        ] {
            watcher.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(definition),
                condition: spellwake_engine::program::TriggerSpec::spell_cast(
                    PlayerRelation::AnyParticipant,
                    spellwake_engine::program::SpellFilter::any(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(target, body),
            });
        }
        cards.push(tr::controlled(id, player, player, watcher));
    }
    let game = initial(cards, vec![P0, P1, p2], P1);
    let cast = paid(&game, P1, 1, 0, None);
    let captured = discoveries(&cast).into_iter().cloned().collect::<Vec<_>>();
    let mut state = cast.into_state();
    for (player, definitions, target_player) in
        [(P1, [2, 1], p2), (p2, [1, 2], P0), (P0, [2, 1], P1)]
    {
        let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
            panic!("expected controller order")
        };
        assert_eq!(frame.chooser(), player);
        let order = definitions
            .into_iter()
            .map(|definition| {
                captured
                    .iter()
                    .find(|waiting| {
                        waiting.controller == player
                            && waiting.definition == TriggeredAbilityId(definition)
                    })
                    .unwrap()
                    .id
            })
            .collect();
        state = tr::reply(&state, player, DecisionReply::OrderTriggers(order)).into_state();
        assert_eq!(target_frame(&state).chooser(), player);
        assert!(matches!(
            state.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ));
        state = select(&state, TargetRef::Player(target_player)).into_state();
    }
    assert!(state.pending_decision().is_none());
    let entries = &state.execution().window().entries()[1..];
    assert_eq!(entries.len(), 6);
    for (actual, (player, definition, target_player)) in entries.iter().zip([
        (P1, 2, Some(p2)),
        (P1, 1, None),
        (p2, 1, None),
        (p2, 2, Some(P0)),
        (P0, 2, Some(P1)),
        (P0, 1, None),
    ]) {
        assert_eq!(actual.controller(), player);
        let binding = state
            .execution()
            .execution_bindings(actual.id())
            .unwrap()
            .trigger
            .as_ref()
            .unwrap();
        assert_eq!(binding.definition, TriggeredAbilityId(definition));
        assert_eq!(
            actual
                .targets()
                .targets()
                .first()
                .map(|bound| bound.choice().target),
            target_player.map(TargetRef::Player)
        );
        let saved = captured
            .iter()
            .find(|waiting| waiting.id == binding.instance)
            .unwrap();
        assert_eq!(binding.source, saved.source);
        assert_eq!(binding.causes, saved.causes);
    }
    for _ in 0..7 {
        state = resolve(&state).into_state();
    }
    for (player, life) in [(P0, 23), (P1, 25), (p2, 23)] {
        assert_eq!(state.execution().life_total(player), Some(&life.into()));
    }
    assert_empty(&state);
}

#[test]
fn targeted_entry_trigger_uses_the_real_new_battlefield_source_identity() {
    let mut source = targeted(
        cast_permanent(true),
        BodyTarget::SinglePlayer,
        vec![gain_target(2)],
    );
    source.triggered[0].condition = spellwake_engine::program::TriggerSpec::enters(
        spellwake_engine::program::ObjectFilter::self_object(),
    );
    let game = two(vec![hand(1, P0, source)]);
    let cast = paid(&game, P0, 1, 0, None);
    assert!(cast.state().pending_decision().is_none());
    let stack_source = tr::object(cast.state(), 1);
    let entered = resolve(cast.state());
    let battlefield = tr::object(entered.state(), 1);
    assert_ne!(battlefield, stack_source);
    let waiting = entered
        .state()
        .execution()
        .waiting_triggers()
        .next()
        .unwrap();
    assert_eq!(waiting.source.object.id(), battlefield);
    let chosen = select(entered.state(), TargetRef::Player(P1));
    let done = resolve(chosen.state());
    assert_eq!(done.state().execution().life_total(P1), Some(&22.into()));
    assert_empty(done.state());
}

#[test]
fn a_step_trigger_chooses_targets_for_its_controller_during_another_players_upkeep() {
    let mut watcher = targeted(
        Program::artifact(None, vec![]),
        BodyTarget::SinglePlayer,
        vec![gain_target(2)],
    );
    watcher.triggered[0].condition = spellwake_engine::program::TriggerSpec::step_began(
        Step::Upkeep,
        PlayerRelation::AnyParticipant,
    );
    let mut state = two(vec![tr::controlled(2, P0, P0, watcher)]);
    for _ in 0..60 {
        if let Some(pending) = state.pending_decision() {
            match pending.frame() {
                RuleFrame::TriggerTarget(_) => break,
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(_)) => {
                    state = tr::reply(
                        &state,
                        pending.chooser(),
                        DecisionReply::DeclareAttackers(vec![]),
                    )
                    .into_state();
                }
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Blockers(_)) => {
                    state = tr::reply(
                        &state,
                        pending.chooser(),
                        DecisionReply::DeclareBlockers(vec![]),
                    )
                    .into_state();
                }
                _ => panic!("unexpected step trajectory decision"),
            }
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("normal priority")
            };
            state = tr::submit(&state, holder, GameCommand::Pass).into_state();
        }
    }
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.execution().window().active_player(), P1);
    assert_eq!(target_frame(&state).chooser(), P0);
    let captured = state.execution().waiting_triggers().next().unwrap();
    assert!(matches!(
        captured.causes.members()[0].related,
        TriggerRelated::Step { active: P1, .. }
    ));
    let chosen = select(&state, TargetRef::Player(P1));
    let done = resolve(chosen.state());
    assert_eq!(done.state().execution().life_total(P1), Some(&22.into()));
    assert_empty(done.state());
}

#[test]
fn both_spell_domains_include_real_noncard_instruction_copies_as_targets() {
    for filter in [
        spellwake_engine::program::SpellFilter::any(),
        spellwake_engine::program::SpellFilter::copyable_instruction(),
    ] {
        let source = targeted(
            instruction(),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
            vec![Instruction::CopySpell],
        );
        let counter = targeted(
            instruction(),
            BodyTarget::SingleSpell(filter),
            vec![Instruction::CounterSpell],
        );
        let game = two(vec![hand(1, P0, source), hand(3, P0, counter)]);
        let cast = paid(&game, P0, 1, 0, None);
        let original = entry(cast.state(), 1);
        let selected = select(cast.state(), TargetRef::Stack(original));
        let copied = resolve(selected.state());
        let copy = copied
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .find(|entry| matches!(entry.kind(), StackKind::SpellCopy { .. }))
            .unwrap()
            .id();
        let response = paid(copied.state(), P0, 3, 1, None);
        let causing = entry(response.state(), 3);
        assert_eq!(
            target_frame(response.state()).domain(),
            &[
                TargetRef::Stack(original),
                TargetRef::Stack(copy),
                TargetRef::Stack(causing)
            ]
        );
        let selected = select(response.state(), TargetRef::Stack(copy));
        let countered = resolve(selected.state());
        assert!(
            !countered
                .state()
                .execution()
                .window()
                .entries()
                .iter()
                .any(|entry| entry.id() == copy)
        );
        assert!(
            countered
                .state()
                .execution()
                .window()
                .spell_copies()
                .is_empty()
        );
        let response = resolve(countered.state());
        let done = resolve(response.state());
        assert_eq!(done.state().execution().life_total(P0), Some(&24.into()));
        assert_empty(done.state());
    }
}

#[test]
fn any_spell_includes_permanent_spells_while_copyable_filter_preserves_interpreter_limit() {
    for filter in [
        spellwake_engine::program::SpellFilter::any(),
        spellwake_engine::program::SpellFilter::copyable_instruction(),
    ] {
        let counter = targeted(
            instruction(),
            BodyTarget::SingleSpell(filter),
            vec![Instruction::CounterSpell],
        );
        let game = two(vec![
            hand(1, P0, cast_permanent(false)),
            hand(3, P0, counter),
        ]);
        let permanent = paid(&game, P0, 1, 0, None);
        let permanent_entry = entry(permanent.state(), 1);
        let response = paid(permanent.state(), P0, 3, 1, None);
        let causing = entry(response.state(), 3);
        let chosen = if filter == spellwake_engine::program::SpellFilter::any() {
            assert_eq!(
                target_frame(response.state()).domain(),
                &[TargetRef::Stack(permanent_entry), TargetRef::Stack(causing)]
            );
            permanent_entry
        } else {
            assert_eq!(
                target_frame(response.state()).domain(),
                &[TargetRef::Stack(causing)]
            );
            causing
        };
        let selected = select(response.state(), TargetRef::Stack(chosen));
        let triggered = resolve(selected.state());
        let done = resolve(triggered.state());
        let expected_zone = if filter == spellwake_engine::program::SpellFilter::any() {
            ObjectZone::Graveyard(P0)
        } else {
            ObjectZone::Battlefield
        };
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(tr::object(done.state(), 1))
                .unwrap()
                .zone(),
            expected_zone
        );
        assert_empty(done.state());
    }
}

#[test]
fn source_control_change_preserves_the_targeted_trigger_controller_and_untargeted_tail() {
    use spellwake_engine::characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec};
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::SinglePlayer, vec![gain_target(2), gain(1)]),
    });
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
        hand(1, P0, instruction()),
        tr::controlled(2, P0, P0, watcher),
        hand(3, P1, control),
    ]);
    let watcher = tr::object(&game, 2);
    let cast = paid(&game, P0, 1, 0, None);
    let captured = discoveries(&cast)[0].clone();
    let chosen = select(cast.state(), TargetRef::Player(P1));
    let trigger = chosen
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let passed = tr::submit(chosen.state(), P0, GameCommand::Pass);
    let response = paid(passed.state(), P1, 3, 0, Some(TargetRef::Object(watcher)));
    let changed = resolve(response.state());
    assert_eq!(tr::current(changed.state(), watcher).controller, Some(P1));
    let original = changed
        .state()
        .execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.id() == trigger)
        .unwrap();
    assert_eq!(original.controller(), P0);
    let binding = changed
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source, captured.source);
    assert_eq!(binding.causes, captured.causes);
    let ability = resolve(changed.state());
    let done = resolve(ability.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&23.into()));
    assert_eq!(done.state().execution().life_total(P1), Some(&22.into()));
    assert_empty(done.state());
}

#[test]
fn placement_target_survives_a_later_resolution_type_choice_and_materialized_layer_effect() {
    use spellwake_engine::{
        characteristics::{
            InstructionSubtypeOperand, ResolvedDuration, SubtypeOperation, TypeOperation,
        },
        program::CreatureTypeSlot,
    };
    let slot = CreatureTypeSlot::new(0);
    let source = targeted(
        instruction(),
        BodyTarget::SingleCreature,
        vec![
            Instruction::ChooseCreatureType {
                into: slot,
                excluding: BTreeSet::new(),
            },
            tr::effect(
                vec![tr::type_part(
                    TypeOperation::Keep,
                    SubtypeOperation::Add(InstructionSubtypeOperand::Chosen(slot)),
                )],
                ResolvedDuration::NoStatedDuration,
            ),
        ],
    );
    let game = two(vec![
        hand(1, P0, source),
        tr::controlled(
            2,
            P1,
            P1,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ]);
    let creature = tr::object(&game, 2);
    let cast = paid(&game, P0, 1, 0, None);
    let chosen = select(cast.state(), TargetRef::Object(creature));
    let entry = chosen
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let binding = chosen
        .state()
        .execution()
        .execution_bindings(entry)
        .unwrap()
        .trigger
        .clone();
    let pending = resolve(chosen.state());
    let RuleFrame::CreatureType(frame) = pending.state().pending_decision().unwrap().frame() else {
        panic!("nested type choice")
    };
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.slot(), slot);
    assert_eq!(
        pending
            .state()
            .execution()
            .execution_bindings(entry)
            .unwrap()
            .trigger,
        binding
    );
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .resolving()
            .unwrap()
            .targets()
            .targets()[0]
            .choice()
            .target,
        TargetRef::Object(creature)
    );
    let elf = pending
        .state()
        .execution()
        .creature_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    let continued = tr::reply(pending.state(), P0, DecisionReply::CreatureType(elf));
    assert!(
        tr::current(continued.state(), creature)
            .current_creature_subtypes
            .contains(&elf)
    );
    let done = resolve(continued.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&22.into()));
    assert_empty(done.state());
}

#[test]
fn nested_copy_choice_changes_only_the_copy_plan_and_keeps_the_trigger_binding() {
    let source = targeted(
        Program::instant(
            ManaCost::one(ManaType::Blue),
            ExecutableBody::new(BodyTarget::SinglePlayer, vec![gain_target(2)]),
        ),
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        vec![Instruction::CopySpell],
    );
    let game = two(vec![hand(1, P0, source)]);
    let cast = paid(&game, P0, 1, 0, Some(TargetRef::Player(P1)));
    let original = entry(cast.state(), 1);
    let chosen = select(cast.state(), TargetRef::Stack(original));
    let trigger = chosen
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let binding = chosen
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .clone();
    let pending = resolve(chosen.state());
    let RuleFrame::CopyTargets(frame) = pending.state().pending_decision().unwrap().frame() else {
        panic!("later copy target continuation")
    };
    assert_eq!(frame.entry(), trigger);
    assert_eq!(
        pending
            .state()
            .execution()
            .execution_bindings(trigger)
            .unwrap()
            .trigger,
        binding
    );
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .resolving()
            .unwrap()
            .targets()
            .targets()[0]
            .choice()
            .target,
        TargetRef::Stack(original)
    );
    let copied = tr::reply(
        pending.state(),
        P0,
        DecisionReply::CopyTargets(spellwake_engine::effects::CopyTargetChoice::Change(
            TargetRef::Player(P0),
        )),
    );
    let entries = copied.state().execution().window().entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].targets().targets()[0].choice().target,
        TargetRef::Player(P1)
    );
    assert_eq!(
        entries[1].targets().targets()[0].choice().target,
        TargetRef::Player(P0)
    );
    assert!(
        copied
            .state()
            .execution()
            .execution_bindings(trigger)
            .is_none()
    );
    for entry in entries {
        assert!(
            copied
                .state()
                .execution()
                .execution_bindings(entry.id())
                .unwrap()
                .trigger
                .is_none()
        );
    }
    let copy = resolve(copied.state());
    let done = resolve(copy.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&22.into()));
    assert_eq!(done.state().execution().life_total(P1), Some(&22.into()));
    assert_empty(done.state());
}
