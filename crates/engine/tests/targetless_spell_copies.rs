//! Finite instruction-spell mechanisms through the native root, not full cards.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::{
    characteristics::KeywordAbility,
    draw::DrawEvent,
    effects::{CopyTargetChoice, EffectError, EffectWindow, RuleResult, SeedCard},
    game::{
        DecisionId, DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{
        EntryCompletion, EventCause, RulesJournal, RulesOccurrence, RulesRecord, ZoneReason,
    },
    priority::{PriorityPhase, SeatId},
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program, SpellFilter, Supertype},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn gain() -> Instruction {
    spellwake_engine::program::Instruction::AdjustLife(Box::new(
        spellwake_engine::toolbox::AdjustLifeSpec::gain(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::Literal(3.into()),
        ),
    ))
}
fn draw() -> Instruction {
    spellwake_engine::program::Instruction::DrawCards(Box::new(
        spellwake_engine::toolbox::DrawCardsSpec::simple(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::Literal(1.into()),
        ),
    ))
}
fn hand(id: u64, actor: SeatId, body: ExecutableBody, paid: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: actor,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            if paid {
                ManaCost::specific(ManaType::Blue, 1.into())
            } else {
                ManaCost::free()
            },
            body,
        ),
    }
}
fn setup(filter: SpellFilter, source: Vec<Instruction>, copier: Vec<Instruction>) -> GameState {
    let v = vocabulary();
    let mut original = hand(1, P0, ExecutableBody::new(BodyTarget::None, source), true);
    original.program.names =
        ObjectNames::single(EnglishCardName::new("Spellwake Targetless Fixture".into()).unwrap());
    original
        .program
        .printed_keywords
        .insert(KeywordAbility::Flash);
    original.program.supertypes.insert(Supertype::Snow);
    original.program.card_types.insert(CardType::Kindred);
    original.program.creature_subtypes = subtypes(&v, &["Elf"]);
    let mut cards = vec![
        original,
        hand(
            2,
            P1,
            ExecutableBody::new(BodyTarget::SingleSpell(filter), copier),
            true,
        ),
        hand(
            3,
            P0,
            ExecutableBody::new(
                BodyTarget::SingleSpell(filter),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
            false,
        ),
        hand(
            4,
            P0,
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            ),
            false,
        ),
    ];
    for (id, actor) in [(10, P0), (11, P1)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner: actor,
            destination: ObjectDestination::Battlefield { controller: actor },
            program: Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
        });
    }
    cards.extend([
        controlled(
            20,
            P0,
            P0,
            permanent(&v, &[CardType::Creature], Some((1, 1)), &["Elf"]),
        ),
        controlled(
            21,
            P1,
            P1,
            permanent(&v, &[CardType::Creature], Some((1, 1)), &["Goblin"]),
        ),
        controlled(
            22,
            P1,
            P1,
            permanent(&v, &[CardType::Creature], Some((1, 1)), &["Goblin"]),
        ),
    ]);
    for actor in [P0, P1] {
        for n in 0..8 {
            cards.push(library(100 + u64::from(actor.0) * 100 + n, actor, &v));
        }
    }
    world(
        v,
        cards,
        vec![CardId(10), CardId(11), CardId(20), CardId(21), CardId(22)],
    )
}
fn tap(game: &GameState, actor: SeatId, id: u64) -> NativeCandidate {
    submit(
        game,
        actor,
        GameCommand::ActivateMana {
            source: object(game, id),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    )
}
fn cast_source(game: &GameState) -> NativeCandidate {
    let tapped = tap(game, P0, 10);
    let result = submit(
        tapped.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: object(tapped.state(), 1),
            payment: None,
        },
    );
    assert!(result.mana_payment().is_some());
    assert_eq!(
        result.state().execution().mana(P0, ManaType::Blue),
        0.into()
    );
    result
}
fn source_and_copier(game: &GameState) -> (GameState, StackEntryId, StackEntryId) {
    let source = cast_source(game);
    let original = source.state().execution().window().entries()[0].id();
    let passed = submit(source.state(), P0, GameCommand::Pass);
    let tapped = tap(passed.state(), P1, 11);
    let result = submit(
        tapped.state(),
        P1,
        GameCommand::Cast {
            spell: object(tapped.state(), 2),
            target: TargetRef::Stack(original),
        },
    );
    assert!(result.mana_payment().is_some());
    assert_eq!(
        result.state().execution().mana(P1, ManaType::Blue),
        0.into()
    );
    let copier = result.state().execution().window().entries()[1].id();
    (result.into_state(), original, copier)
}
fn resolve(game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("actual priority required")
    };
    pass_in_order(game, [holder, if holder == P0 { P1 } else { P0 }])
}
fn copies(result: &NativeCandidate) -> Vec<StackEntryId> {
    all_occurrences(result)
        .iter()
        .filter_map(|value| match value {
            RulesOccurrence::CopyCreated { entry, .. } => Some(*entry),
            _ => None,
        })
        .collect()
}
fn assert_no_cast_or_payment(result: &NativeCandidate) {
    assert!(result.mana_payment().is_none());
    assert!(!all_occurrences(result).iter().any(|value| matches!(
        value,
        RulesOccurrence::CastCompleted(_) | RulesOccurrence::ManaDebited { .. }
    )));
}
fn assert_traits(game: &GameState, id: StackEntryId) {
    let execution = game.execution();
    assert_eq!(
        execution.stack_copiable_keywords(id),
        Some(&BTreeSet::from([KeywordAbility::Flash]))
    );
    assert_eq!(
        execution.stack_copiable_names(id),
        Some(&ObjectNames::single(
            EnglishCardName::new("Spellwake Targetless Fixture".into()).unwrap()
        ))
    );
    assert_eq!(
        execution.stack_copiable_supertypes(id),
        Some(&BTreeSet::from([Supertype::Snow]))
    );
    assert_eq!(
        execution.stack_copiable_card_types(id),
        Some(&BTreeSet::from([CardType::Instant, CardType::Kindred]))
    );
    assert_eq!(
        execution.stack_copiable_creature_subtypes(id),
        Some(&subtypes(execution.creature_vocabulary(), &["Elf"]))
    );
}
fn reject(game: &GameState, input: GameInput, expected: GameError) {
    let before = game.clone();
    assert_eq!(game.advance(input), Err(expected));
    assert_eq!(game, &before);
}
fn resolving_lower(game: &GameState) -> EffectWindow {
    let mut execution = game.execution().clone();
    for _ in 0..2 {
        if matches!(
            execution.window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            execution = execution.settle_checks(execution.revision()).unwrap();
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = execution.window().phase()
        else {
            panic!("actual lower priority")
        };
        execution = execution.pass(execution.revision(), holder).unwrap();
    }
    execution
}
fn occurrences(journal: &RulesJournal) -> Vec<&RulesOccurrence> {
    journal
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(e) => Some(&e.occurrences),
            _ => None,
        })
        .flatten()
        .map(|row| &row.value)
        .collect()
}

#[test]
fn both_filters_copy_paid_targetless_spells_without_choices_or_recasting_for_another_controller() {
    for filter in [
        spellwake_engine::program::SpellFilter::any(),
        spellwake_engine::program::SpellFilter::copyable_instruction(),
    ] {
        let game = setup(
            filter,
            vec![draw(), gain()],
            vec![Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            ))],
        );
        let (game, original, copier) = source_and_copier(&game);
        let physical_count = game.execution().window().cards().iter().count();
        let next_decision = game.next_decision_sequence();
        let result = resolve(&game);
        let [copy] = copies(&result)[..] else {
            panic!("one actual copy")
        };
        let after = result.state();
        assert!(after.pending_decision().is_none());
        assert_eq!(after.next_decision_sequence(), next_decision);
        assert_no_cast_or_payment(&result);
        assert!(result.draw_journal().events.is_empty());
        assert_eq!(
            after.execution().window().cards().iter().count(),
            physical_count
        );
        assert_eq!(
            after
                .execution()
                .window()
                .entries()
                .iter()
                .map(|e| e.id())
                .collect::<Vec<_>>(),
            [original, copy]
        );
        let entry = after.execution().window().entries().last().unwrap();
        assert!(entry.targets().targets().is_empty());
        assert_eq!(
            entry.kind(),
            &StackKind::SpellCopy {
                copied_from: original,
                owner: P1
            }
        );
        let metadata = &after.execution().window().spell_copies()[&copy];
        assert_eq!(
            (metadata.owner, metadata.controller, metadata.zone),
            (P1, Some(P1), ObjectZone::Stack)
        );
        assert_eq!(
            after.execution().stack_body(copy),
            game.execution().stack_body(original)
        );
        assert_traits(after, copy);
        let created = all_occurrences(&result)
            .into_iter()
            .find(|v| matches!(v, RulesOccurrence::CopyCreated { .. }))
            .unwrap();
        assert_eq!(
            created,
            &RulesOccurrence::CopyCreated {
                entry: copy,
                original,
                controller: P1,
                targets: vec![],
                copiable_names: game
                    .execution()
                    .stack_copiable_names(original)
                    .unwrap()
                    .clone(),
                copiable_supertypes: BTreeSet::from([Supertype::Snow]),
                copiable_card_types: BTreeSet::from([CardType::Instant, CardType::Kindred]),
                copiable_spell_subtypes: BTreeSet::new(),
                copiable_artifact_subtypes: BTreeSet::new(),
                copiable_enchantment_subtypes: BTreeSet::new(),
                copiable_keywords: BTreeSet::from([KeywordAbility::Flash]),
                copiable_triggered: vec![],
                copiable_creature_subtypes: subtypes(
                    after.execution().creature_vocabulary(),
                    &["Elf"]
                )
            }
        );
        assert!(result.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.cause==EventCause::Instruction {entry:copier,instruction:0} && e.occurrences.iter().any(|o|&o.value==created))));
        let expected_draw = after.execution().window().cards().library(P1)[0];
        let copied = resolve(after);
        assert_eq!(copied.draw_journal().events.len(), 1);
        assert!(
            matches!(&copied.draw_journal().events[0],DrawEvent::Drawn {player,before,after} if *player==P1&&before.id()==expected_draw&&after.zone()==ObjectZone::Hand(P1))
        );
        assert_eq!(copied.state().execution().life_total(P1), Some(&23.into()));
        assert_eq!(copied.state().execution().life_total(P0), Some(&20.into()));
        assert!(copied.state().execution().stack_body(copy).is_none());
        assert!(
            !copied
                .state()
                .execution()
                .window()
                .spell_copies()
                .contains_key(&copy)
        );
        let original_done = resolve(copied.state());
        assert_eq!(original_done.draw_journal().events.len(), 1);
        assert_eq!(
            original_done.state().execution().life_total(P0),
            Some(&23.into())
        );
        assert_eq!(
            original_done
                .state()
                .execution()
                .window()
                .cards()
                .get(object(original_done.state(), 1))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(P0)
        );
        let input = GameInput {
            basis: result.state().basis().clone(),
            actor: P1,
            command: GameCommand::Reply {
                decision: DecisionId(next_decision),
                choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            },
        };
        reject(result.state(), input, GameError::NoDecision);
    }
}

#[test]
fn repeated_instructions_create_ordered_copies_and_complete_the_exact_copier() {
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![gain()],
        vec![
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
        ],
    );
    let (game, original, copier) = source_and_copier(&game);
    let copier_object = object(&game, 2);
    let result = resolve(&game);
    let ids = copies(&result);
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        result.state().next_decision_sequence(),
        game.next_decision_sequence()
    );
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|e| e.id())
            .collect::<Vec<_>>(),
        [original, ids[0], ids[1]]
    );
    let relevant = result
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| match r {
            RulesRecord::Event(e)
                if e.occurrences
                    .iter()
                    .any(|o| matches!(o.value, RulesOccurrence::CopyCreated { .. })) =>
            {
                Some(e.cause.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        relevant,
        [
            EventCause::Instruction {
                entry: copier,
                instruction: 0
            },
            EventCause::Instruction {
                entry: copier,
                instruction: 1
            }
        ]
    );
    let completion = result
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event)
                if event.cause
                    == (EventCause::ResolutionCompleted {
                        entry: copier,
                        completion: EntryCompletion::Resolved,
                    }) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completion.len(), 1);
    assert_eq!(completion[0].occurrences.len(), 1);
    assert!(matches!(
        &completion[0].occurrences[0].value,
        RulesOccurrence::ZoneChange(zone)
            if zone.before.object.id() == copier_object
                && zone.after.object.card_id().expect("physical fixture card") == CardId(2)
                && zone.after.object.zone() == ObjectZone::Graveyard(P1)
                && zone.reason == ZoneReason::ResolutionExit
    ));
    assert_no_cast_or_payment(&result);
}

#[test]
fn copy_of_copy_owns_body_and_traits_after_the_physical_original_is_countered() {
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![draw(), gain()],
        vec![Instruction::StackOp(Box::new(
            spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
        ))],
    );
    let (game, original, _) = source_and_copier(&game);
    let first = resolve(&game);
    let first_id = copies(&first)[0];
    let next = submit(
        first.state(),
        P0,
        GameCommand::Cast {
            spell: object(first.state(), 3),
            target: TargetRef::Stack(first_id),
        },
    );
    let second = resolve(next.state());
    let second_id = copies(&second)[0];
    assert_ne!(first_id, second_id);
    let counter = submit(
        second.state(),
        P0,
        GameCommand::Cast {
            spell: object(second.state(), 4),
            target: TargetRef::Stack(original),
        },
    );
    let countered = resolve(counter.state());
    let state = countered.state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    assert_eq!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .map(|e| e.id())
            .collect::<Vec<_>>(),
        [first_id, second_id]
    );
    for id in [first_id, second_id] {
        assert_traits(state, id);
        assert_eq!(
            state.execution().stack_body(id).unwrap().target,
            BodyTarget::None
        );
    }
    assert_eq!(
        state.execution().window().spell_copies()[&second_id].owner,
        P0
    );
    let second_done = resolve(state);
    assert_eq!(second_done.draw_journal().events.len(), 1);
    assert_eq!(
        second_done.state().execution().life_total(P0),
        Some(&23.into())
    );
    let first_done = resolve(second_done.state());
    assert_eq!(first_done.draw_journal().events.len(), 1);
    assert_eq!(
        first_done.state().execution().life_total(P1),
        Some(&23.into())
    );
    assert!(first_done.state().execution().window().entries().is_empty());
}

#[test]
fn each_targetless_copy_and_original_makes_fresh_resolution_choices_and_controller_counts() {
    let mut choose = type_choice_body();
    let Instruction::ChooseCreatureType { excluding, .. } = &mut choose else {
        panic!("type-choice fixture")
    };
    let excluded = vocabulary().resolve_label("Dragon").unwrap();
    excluding.insert(excluded);
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![choose, chosen_draw()],
        vec![
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
        ],
    );
    let (game, original, _) = source_and_copier(&game);
    let copies_result = resolve(&game);
    let ids = copies(&copies_result);
    assert_eq!(ids.len(), 2);
    assert!(copies_result.state().pending_decision().is_none());
    let mut state = copies_result.into_state();
    for (id, actor, label, draws) in [
        (ids[1], P1, "Goblin", 2),
        (ids[0], P1, "Elf", 0),
        (original, P0, "Elf", 1),
    ] {
        let pending = resolve(&state);
        let source = pending.state();
        let decision = source.pending_decision().unwrap();
        let RuleFrame::CreatureType(frame) = decision.frame() else {
            panic!("actual resolution type choice")
        };
        assert_eq!(frame.entry(), id);
        assert_eq!(frame.chooser(), actor);
        assert_eq!(
            frame.vocabulary(),
            source.execution().creature_vocabulary().id()
        );
        assert!(
            source
                .execution()
                .resolution_state()
                .unwrap()
                .locals()
                .is_empty()
        );
        assert_eq!(
            source.next_decision_sequence(),
            state.next_decision_sequence() + 1
        );
        let selected = source
            .execution()
            .creature_vocabulary()
            .resolve_label(label)
            .unwrap();
        let reply = GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CreatureType(selected),
        };
        reject(
            source,
            GameInput {
                basis: state.basis().clone(),
                actor,
                command: reply.clone(),
            },
            GameError::StaleRevision,
        );
        reject(
            source,
            GameInput {
                basis: source.basis().clone(),
                actor: if actor == P0 { P1 } else { P0 },
                command: reply.clone(),
            },
            GameError::WrongChooser,
        );
        reject(
            source,
            GameInput {
                basis: source.basis().clone(),
                actor,
                command: GameCommand::Reply {
                    decision: DecisionId(decision.id().0 + 1),
                    choice: DecisionReply::CreatureType(selected),
                },
            },
            GameError::WrongDecision,
        );
        reject(
            source,
            GameInput {
                basis: source.basis().clone(),
                actor,
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::CreatureType(excluded),
                },
            },
            GameError::Effect(EffectError::IllegalCreatureType),
        );
        reject(
            source,
            GameInput {
                basis: source.basis().clone(),
                actor,
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                },
            },
            GameError::Effect(EffectError::WrongDecisionKind),
        );
        let done = submit(source, actor, reply);
        assert_eq!(done.draw_journal().events.len(), draws);
        assert!(all_occurrences(&done).iter().any(|o|matches!(o,RulesOccurrence::CreatureTypeChosen {entry,instruction,chooser,value,..} if *entry==id&&*instruction==0&&*chooser==actor&&*value==selected)));
        state = done.into_state();
    }
    assert!(state.execution().window().entries().is_empty());
}

#[test]
fn automatic_copy_keeps_its_journal_when_a_later_real_choice_suspends() {
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![gain()],
        vec![
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            type_choice_body(),
            chosen_draw(),
        ],
    );
    let (game, _, copier) = source_and_copier(&game);
    let lower = resolving_lower(&game);
    let saved = lower.clone();
    assert_eq!(
        lower.resolve_top(lower.revision()),
        Err(EffectError::CreatureTypeChoiceRequired)
    );
    assert_eq!(
        lower.resolve_top_with_copy_choice(lower.revision(), P1, CopyTargetChoice::Retain),
        Err(EffectError::CreatureTypeChoiceRequired)
    );
    assert_eq!(lower, saved);
    let result = resolve(&game);
    assert_eq!(copies(&result).len(), 1);
    let state = result.state();
    let decision = state.pending_decision().unwrap();
    assert!(matches!(decision.frame(),RuleFrame::CreatureType(f) if f.entry()==copier));
    assert_eq!(
        state.next_decision_sequence(),
        game.next_decision_sequence() + 1
    );
    let done = submit(
        state,
        P1,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CreatureType(
                state
                    .execution()
                    .creature_vocabulary()
                    .resolve_label("Goblin")
                    .unwrap(),
            ),
        },
    );
    assert_eq!(done.draw_journal().events.len(), 2);
    assert!(copies(&done).is_empty());
    assert_eq!(done.state().execution().window().entries().len(), 2);
}

#[test]
fn unused_explicit_copy_choice_on_automatic_completion_is_rejected_atomically() {
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![gain()],
        vec![Instruction::StackOp(Box::new(
            spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
        ))],
    );
    let (game, _, _) = source_and_copier(&game);
    let lower = resolving_lower(&game);
    let before = lower.clone();
    for choice in [
        CopyTargetChoice::Retain,
        CopyTargetChoice::Change(TargetRef::Player(P0)),
    ] {
        assert_eq!(
            lower.resolve_top_with_copy_choice(lower.revision(), P1, choice),
            Err(EffectError::UnexpectedCopyChoice)
        );
        assert_eq!(lower, before);
    }
    let done = lower.resolve_top(lower.revision()).unwrap();
    assert_eq!(done.window().spell_copies().len(), 1);
}

#[test]
fn targetless_copy_exit_cessation_and_payload_pruning_are_separate_actual_cuts() {
    let game = setup(
        spellwake_engine::program::SpellFilter::copyable_instruction(),
        vec![gain()],
        vec![Instruction::StackOp(Box::new(
            spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
        ))],
    );
    let (game, _, _) = source_and_copier(&game);
    let created = resolve(&game);
    let id = copies(&created)[0];
    let lower = resolving_lower(created.state());
    let progress = lower.begin_resolution(lower.revision()).unwrap();
    assert!(matches!(progress.result(), RuleResult::Complete { .. }));
    assert_eq!(
        progress.state().window().spell_copies()[&id].controller,
        None
    );
    assert_eq!(
        progress.state().window().spell_copies()[&id].zone,
        ObjectZone::Graveyard(P1)
    );
    assert_eq!(progress.state().window().spell_copies()[&id].owner, P1);
    assert!(progress.state().stack_body(id).is_some());
    assert!(occurrences(progress.append()).iter().any(|o|matches!(o,RulesOccurrence::CopyMoved {before,after} if before.id==id&&before.controller==Some(P1)&&after.controller.is_none()&&after.owner==P1)));
    assert!(
        !occurrences(progress.append())
            .iter()
            .any(|o| matches!(o, RulesOccurrence::CopyCeased { .. }))
    );
    let checks = progress
        .state()
        .begin_checks(progress.state().revision())
        .unwrap();
    assert!(occurrences(checks.append()).iter().any(|o|matches!(o,RulesOccurrence::CopyCeased {copy} if copy.id==id&&copy.owner==P1&&copy.controller.is_none())));
    assert!(!checks.state().window().spell_copies().contains_key(&id));
    assert!(checks.state().stack_body(id).is_none());
}

#[test]
fn copy_then_counter_then_copy_skips_the_departed_source_without_a_false_decision() {
    let game = setup(
        spellwake_engine::program::SpellFilter::any(),
        vec![gain()],
        vec![
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
        ],
    );
    let (game, _, _) = source_and_copier(&game);
    let result = resolve(&game);
    assert_eq!(copies(&result).len(), 1);
    assert!(result.state().pending_decision().is_none());
    assert_eq!(
        result.state().next_decision_sequence(),
        game.next_decision_sequence()
    );
    assert_eq!(result.state().execution().window().entries().len(), 1);
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(object(result.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
}
