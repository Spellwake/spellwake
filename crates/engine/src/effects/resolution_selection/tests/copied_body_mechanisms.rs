//! Synthetic selected-body copy mechanisms through real GameState inputs.
//! This is not a source-card fixture, seat-safe view or LKI qualification.
use super::*;
use crate::{
    effects::{CopyResolution, CopyTargetChoice},
    occurrence::ZoneReason,
    stack_window::{StackEntryId, StackKind},
};

fn slot() -> CreatureTypeSlot {
    CreatureTypeSlot::new(0)
}

fn type_frame(state: &GameState) -> &crate::toolbox::PromptChoiceFrame {
    let RuleFrame::PromptChoice(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual creature-type prompt decision expected")
    };
    frame
}

fn copy_frame(state: &GameState) -> &CopyResolution {
    let RuleFrame::CopyTargets(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual CopyTargets suspension expected")
    };
    frame
}

fn occurrences(candidate: &NativeCandidate) -> Vec<&RulesOccurrence> {
    events(candidate)
        .into_iter()
        .flat_map(|event| event.occurrences.iter().map(|row| &row.value))
        .collect()
}

fn answer(state: &GameState, choice: DecisionReply) -> GameInput {
    let decision = state.pending_decision().unwrap();
    GameInput {
        basis: state.basis().clone(),
        actor: decision.chooser(),
        command: GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    }
}

fn reject_input(state: &GameState, input: GameInput, expected: GameError) {
    let before = state.clone();
    assert_eq!(state.advance(input), Err(expected));
    assert_eq!(state, &before, "rejected input is atomic");
}

fn selected_body() -> ExecutableBody {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![
            Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                chooser: crate::toolbox::PlayerSelector::Resolving(
                    crate::program::ResolvingPlayer::Controller,
                ),
                domain: crate::toolbox::PromptDomain::Subtype {
                    kind: crate::toolbox::SubtypeChoiceKind::Creature,
                    excluding: BTreeSet::from([vocabulary.resolve_label("Dragon").unwrap()]),
                    legacy_slot: Some(slot()),
                },
                category: crate::toolbox::ChoiceCategory::Generic,
                bind_to: crate::toolbox::BindingId(0),
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
            })),
            Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                amount: crate::program::ValueExpr::count_battlefield(
                    crate::program::ObjectFilter::any()
                        .controlled()
                        .of_chosen_slot(slot()),
                ),
            },
            gain(4),
        ],
    )
}

fn setup() -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut elf = creature();
    elf.creature_subtypes
        .insert(vocabulary.resolve_label("Elf").unwrap());
    let mut goblin = creature();
    goblin
        .creature_subtypes
        .insert(vocabulary.resolve_label("Goblin").unwrap());
    let mut cards = vec![
        seed(
            2,
            B,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
                    vec![
                        // A real live parent local has the same slot ordinal as
                        // the donor, but must not become a copied-body local.
                        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
                            chooser: crate::toolbox::PlayerSelector::Resolving(
                                crate::program::ResolvingPlayer::Controller,
                            ),
                            domain: crate::toolbox::PromptDomain::Subtype {
                                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                                excluding: BTreeSet::new(),
                                legacy_slot: Some(slot()),
                            },
                            category: crate::toolbox::ChoiceCategory::Generic,
                            bind_to: crate::toolbox::BindingId(0),
                            bind_unchosen: None,
                            tag_partition: None,
                            auto_resolve_trivial: false,
                        })),
                        Instruction::StackOp(Box::new(
                            crate::toolbox::StackOpSpec::copy_targeted_spell(),
                        )),
                    ],
                ),
            ),
        ),
        seed(
            3,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
                    vec![Instruction::StackOp(Box::new(
                        crate::toolbox::StackOpSpec::counter_targeted(
                            crate::toolbox::CounterDestination::Graveyard,
                        ),
                    ))],
                ),
            ),
        ),
        seed(30, A, ObjectDestination::Battlefield { controller: A }, elf),
        seed(
            31,
            B,
            ObjectDestination::Battlefield { controller: B },
            goblin.clone(),
        ),
        seed(
            32,
            B,
            ObjectDestination::Battlefield { controller: B },
            goblin,
        ),
    ];
    cards.extend(
        [(10, A), (11, A), (20, C), (21, C), (22, C)]
            .into_iter()
            .map(|(card, owner)| seed(card, owner, ObjectDestination::Hand, creature())),
    );
    let body = selected_body();
    fixture(body.target, body.instructions, vec![A, B, C], A, cards)
}

fn pending_copy() -> (GameState, StackEntryId, StackEntryId) {
    let state = setup();
    let state = cast(&state, A, 1, Some(TargetRef::Player(C)));
    let original = state.execution().window().entries()[0].id();
    let state = advance(&state, A, GameCommand::Pass).into_state();
    let state = cast(&state, B, 2, Some(TargetRef::Stack(original)));
    let copier = state.execution().window().entries().last().unwrap().id();
    let state = pass_to_boundary(state);
    assert_eq!(type_frame(&state).entry, copier);
    assert_eq!(type_frame(&state).chooser, B);
    assert!(
        state
            .execution()
            .resolution_state()
            .unwrap()
            .locals()
            .is_empty()
    );
    let elf = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    let state = reply(
        &state,
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(elf)),
    )
    .into_state();
    assert_eq!(copy_frame(&state).entry(), copier);
    assert_eq!(copy_frame(&state).original(), original);
    assert_eq!(copy_frame(&state).chooser(), B);
    assert_eq!(copy_frame(&state).instruction_index(), 1);
    assert_eq!(
        state.execution().resolution_state().unwrap().locals(),
        &BTreeMap::from([(slot(), elf)])
    );
    assert!(state.execution().resolution_selection_state().is_none());
    (state, original, copier)
}

fn create_copy(
    state: &GameState,
    original: StackEntryId,
    choice: CopyTargetChoice,
) -> (NativeCandidate, StackEntryId, GameInput) {
    let input = answer(state, DecisionReply::CopyTargets(choice));
    assert_rejected(state, A, input.command.clone(), GameError::WrongChooser);
    assert_rejected(
        state,
        B,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(id(
                state, 30,
            )))),
        },
        GameError::Effect(EffectError::IllegalTarget),
    );
    let result = advance(state, input.actor, input.command.clone());
    let created: Vec<_> = occurrences(&result)
        .into_iter()
        .filter_map(|value| match value {
            RulesOccurrence::CopyCreated { entry, .. } => Some(*entry),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 1, "resume creates one copy exactly once");
    let copy = created[0];
    let after = result.state();
    assert_eq!(after.execution().stack_body(copy), Some(&selected_body()));
    assert_eq!(
        after.execution().stack_body(original),
        Some(&selected_body())
    );
    let original_entry = after
        .execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.id() == original)
        .unwrap();
    let copied = after.execution().window().entries().last().unwrap();
    assert_eq!(copied.id(), copy);
    assert_eq!(
        copied.kind(),
        &StackKind::SpellCopy {
            copied_from: original,
            owner: B
        }
    );
    assert_eq!(copied.controller(), B);
    let mut expected_binding = original_entry.targets().targets()[0].choice();
    if let CopyTargetChoice::Change(target) = choice {
        expected_binding.target = target;
    }
    assert_eq!(copied.targets().targets().len(), 1);
    assert_eq!(copied.targets().targets()[0].choice(), expected_binding);
    assert_eq!(
        original_entry.targets().targets()[0].choice().target,
        TargetRef::Player(C)
    );
    assert_eq!(after.execution().window().spell_copies()[&copy].owner, B);
    assert!(result.mana_payment().is_none());
    assert!(!occurrences(&result).iter().any(|value| matches!(
        value,
        RulesOccurrence::CastCompleted(_) | RulesOccurrence::ManaDebited { .. }
    )));
    assert!(after.pending_decision().is_none());
    assert!(after.execution().resolution_state().is_none());
    assert!(after.execution().resolution_selection_state().is_none());
    reject_input(after, input.clone(), GameError::StaleRevision);
    assert_rejected(after, B, input.command.clone(), GameError::NoDecision);
    (result, copy, input)
}

fn pass_for_fresh_type(mut state: GameState, entry: StackEntryId, chooser: SeatId) -> GameState {
    let participants = state.execution().window().participants().len();
    for index in 0..participants {
        assert!(state.pending_decision().is_none());
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("real later priority pass required")
        };
        state = advance(&state, holder, GameCommand::Pass).into_state();
        if index + 1 < participants {
            assert!(state.pending_decision().is_none());
            assert!(state.execution().resolution_state().is_none());
            assert_eq!(
                state.execution().window().entries().last().unwrap().id(),
                entry
            );
        }
    }
    let frame = type_frame(&state);
    assert_eq!(frame.entry, entry);
    assert_eq!(frame.instruction, 0);
    assert_eq!(frame.chooser, chooser);
    let dragon = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Dragon")
        .unwrap();
    let crate::toolbox::ResolvedPromptDomain::CreatureType { allowed } = &frame.domain else {
        panic!("creature type domain expected")
    };
    assert!(!allowed.contains(&dragon));
    let cursor = state.execution().resolution_state().unwrap();
    assert_eq!(cursor.entry(), entry);
    assert_eq!(cursor.instruction(), 0);
    assert!(
        cursor.locals().is_empty(),
        "parent/earlier-copy locals must not leak"
    );
    assert!(state.execution().resolution_selection_state().is_none());
    state
}

fn choose_count(
    state: &GameState,
    entry: StackEntryId,
    label: &str,
    affected: SeatId,
    required: usize,
) -> GameState {
    let value = state
        .execution()
        .creature_vocabulary()
        .resolve_label(label)
        .unwrap();
    let input = answer(
        state,
        DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(value)),
    );
    let wrong = if input.actor == A { B } else { A };
    assert_rejected(state, wrong, input.command.clone(), GameError::WrongChooser);
    let result = advance(state, input.actor, input.command);
    assert_eq!(occurrences(&result).iter().filter(|occurrence| matches!(occurrence,
        RulesOccurrence::CreatureTypeChosen { entry: actual, instruction: 0, chooser, value: chosen, .. }
            if *actual == entry && *chooser == input.actor && *chosen == value
    )).count(), 1);
    let after = result.state();
    let frame = select_frame(after);
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.instruction(), 1);
    assert_eq!(frame.operation(), ResolutionSelectionKind::Discard);
    assert_eq!(frame.chooser(), affected);
    assert_eq!(frame.required(), required);
    let work = after.execution().resolution_selection_state().unwrap();
    assert_eq!(work.cursor.entry(), entry);
    assert_eq!(work.cursor.instruction(), 1);
    assert_eq!(work.cursor.locals(), &BTreeMap::from([(slot(), value)]));
    let ResolutionSelectionStage::Choosing {
        requirements,
        next_player,
        accepted,
    } = work.stage
    else {
        panic!("new selection stage expected")
    };
    assert_eq!(*next_player, 0);
    assert!(
        accepted.is_empty(),
        "an earlier entry's accepted selection must not leak"
    );
    assert_eq!(requirements.len(), 1);
    assert_eq!(requirements[0].player, affected);
    assert_eq!(requirements[0].required, required);
    result.into_state()
}

fn finish_selection(state: &GameState, selected_cards: &[u64]) -> (NativeCandidate, GameInput) {
    let selected: Vec<_> = selected_cards.iter().map(|card| id(state, *card)).collect();
    let selection = answer(state, DecisionReply::SelectResolution(selected.clone()));
    let result = advance(state, selection.actor, selection.command.clone());
    if selected.len() == 1 {
        return (result, selection);
    }
    let after = result.state();
    assert_eq!(order_frame(after).chooser(), selection.actor);
    assert_eq!(
        order_frame(after)
            .domain()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        selected.iter().copied().collect()
    );
    for object in &selected {
        assert_eq!(
            after
                .execution()
                .window()
                .cards()
                .get(*object)
                .unwrap()
                .zone(),
            ObjectZone::Hand(selection.actor)
        );
    }
    reject_input(after, selection.clone(), GameError::StaleRevision);
    assert_rejected(
        after,
        selection.actor,
        selection.command.clone(),
        GameError::WrongDecision,
    );
    let completed = reply(
        after,
        DecisionReply::OrderResolutionGraveyard(selected.into_iter().rev().collect()),
    );
    (completed, selection)
}

fn assert_copy_finished(result: &NativeCandidate, copy: StackEntryId, discarded: SeatId) {
    let values = occurrences(result);
    let moved = values
        .iter()
        .position(|value| {
            matches!(value,
                RulesOccurrence::CopyMoved { before, after }
                    if before.id == copy && before.owner == B && before.controller == Some(B)
                        && before.zone == ObjectZone::Stack && after.id == copy
                        && after.owner == B && after.controller.is_none()
                        && after.zone == ObjectZone::Graveyard(B)
            )
        })
        .unwrap();
    let ceased = values
        .iter()
        .position(|value| {
            matches!(value,
                RulesOccurrence::CopyCeased { copy: object }
                    if object.id == copy && object.owner == B && object.controller.is_none()
                        && object.zone == ObjectZone::Graveyard(B)
            )
        })
        .unwrap();
    assert!(
        moved < ceased,
        "actual root journal separates movement from later checks"
    );
    assert_eq!(values.iter().filter(|value| matches!(value,
        RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::Discard { player: discarded }
    )).count(), 2);
    let state = result.state();
    assert!(state.execution().stack_body(copy).is_none());
    assert!(
        !state
            .execution()
            .window()
            .spell_copies()
            .contains_key(&copy)
    );
    assert!(state.execution().resolution_selection_state().is_none());
    assert!(state.execution().resolution_state().is_none());
    assert!(state.pending_decision().is_none());
    assert_eq!(
        physical_pile(state, B),
        vec![CardId(2)],
        "the nonphysical copy is never a graveyard card"
    );
    assert_eq!(state.execution().life_total(B), Some(&24.into()));
}

#[test]
fn retained_and_retargeted_selection_copies_have_fresh_controller_choices_and_selected_work() {
    for choice in [
        CopyTargetChoice::Retain,
        CopyTargetChoice::Change(TargetRef::Player(A)),
    ] {
        let (pending, original, _) = pending_copy();
        let physical_count = pending.execution().window().cards().iter().count();
        let (created, copy, copy_input) = create_copy(&pending, original, choice);
        assert_eq!(
            created.state().execution().window().cards().iter().count(),
            physical_count
        );
        let state = pass_for_fresh_type(created.into_state(), copy, B);
        assert_rejected(&state, B, copy_input.command, GameError::WrongDecision);
        let affected = if matches!(choice, CopyTargetChoice::Retain) {
            C
        } else {
            A
        };
        // Only B has two Goblins. The original's A controller has none.
        let state = choose_count(&state, copy, "Goblin", affected, 2);
        let selected = if affected == C { [20, 21] } else { [10, 11] };
        let (completed_copy, old_selection) = finish_selection(&state, &selected);
        assert_copy_finished(&completed_copy, copy, affected);
        let after = completed_copy.state();
        assert_eq!(after.execution().window().entries().len(), 1);
        assert_eq!(after.execution().window().entries()[0].id(), original);
        assert_eq!(
            after
                .execution()
                .window()
                .cards()
                .get(id(after, 1))
                .unwrap()
                .zone(),
            ObjectZone::Stack
        );
        assert_eq!(after.execution().life_total(A), Some(&20.into()));
        assert_eq!(
            after.execution().window().cards().iter().count(),
            physical_count
        );
        let state = pass_for_fresh_type(completed_copy.into_state(), original, A);
        // Only A has an Elf. Original count/chooser may not inherit B's two,
        // the copied Goblin local, or its completed selection/order prefix.
        let state = choose_count(&state, original, "Elf", C, 1);
        assert_rejected(&state, C, old_selection.command, GameError::WrongDecision);
        let original_card = if affected == C { 22 } else { 20 };
        assert!(
            select_frame(&state)
                .domain()
                .contains(&id(&state, original_card))
        );
        let (done, _) = finish_selection(&state, &[original_card]);
        let state = done.state();
        assert!(state.execution().window().entries().is_empty());
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(id(state, 1))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(A)
        );
        assert_eq!(state.execution().life_total(A), Some(&24.into()));
        assert_eq!(state.execution().life_total(B), Some(&24.into()));
        assert_eq!(physical_pile(state, B), vec![CardId(2)]);
        assert!(state.execution().window().spell_copies().is_empty());
    }
}

#[test]
fn copied_selected_payload_survives_countering_the_physical_donor_before_later_priority_passes() {
    let (pending, original, _) = pending_copy();
    let (created, copy, _) = create_copy(&pending, original, CopyTargetChoice::Retain);
    let state = cast(created.state(), A, 3, Some(TargetRef::Stack(original)));
    let state = pass_to_boundary(state);
    assert!(state.pending_decision().is_none());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(id(&state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert!(state.execution().stack_body(original).is_none());
    assert_eq!(state.execution().stack_body(copy), Some(&selected_body()));
    assert_eq!(state.execution().window().entries().len(), 1);
    assert_eq!(state.execution().window().entries()[0].id(), copy);
    let state = pass_for_fresh_type(state, copy, B);
    let state = choose_count(&state, copy, "Goblin", C, 2);
    let (done, _) = finish_selection(&state, &[20, 21]);
    assert_copy_finished(&done, copy, C);
    let state = done.state();
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().life_total(A), Some(&20.into()));
    assert_eq!(state.execution().life_total(B), Some(&24.into()));
    assert_eq!(physical_pile(state, A), vec![CardId(3), CardId(1)]);
    assert_eq!(physical_pile(state, C), vec![CardId(21), CardId(20)]);
}
