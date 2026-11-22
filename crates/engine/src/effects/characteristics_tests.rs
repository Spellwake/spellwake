//! Exact trusted owner continuations from the reviewed characteristic recipes.
use super::*;
use crate::{
    characteristics::{ResolvedDuration, ResolvedPart, ResolvedParts},
    occurrence::TriggerError,
    program::{BodyTarget, CardType, ExecutableBody, ProgramKind},
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn seed(programs: Vec<Program>) -> EffectWindow {
    let order: Vec<_> = (1..=programs.len()).map(|id| CardId(id as u64)).collect();
    let cards = programs
        .into_iter()
        .enumerate()
        .map(|(index, program)| SeedCard {
            card: CardId(index as u64 + 1),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program,
        })
        .collect();
    let state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::selected_vocabulary()
            .expect("explicit qualification vocabulary"),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    // These U recipes declare continuous control explicitly. Supply that
    // trusted physical premise through the existing owner hook.
    let state = state
        .enter_turn_step(state.revision(), A, false, false)
        .unwrap()
        .into_state();
    let state = state.record_turn_start(state.revision()).unwrap();
    assert!(
        state
            .window
            .cards()
            .iter()
            .all(|card| card.has_continuous_control_since_turn_start() == Some(true))
    );
    state
}
fn object(state: &EffectWindow, card: u64) -> ObjectId {
    state.window.cards().current_identity(CardId(card)).unwrap()
}
/// A retired entry identity is a trusted premise of these U fixtures. No card
/// from this separate allocator fixture is inserted into their physical world.
fn retired_entry() -> StackEntryId {
    let cards = GameObjects::new(0)
        .seed_card(0, CardId(99), A, ObjectDestination::Hand)
        .unwrap()
        .next;
    let source = cards.current_identity(CardId(99)).unwrap();
    let window = StackWindow::new(vec![A, B], A, cards).unwrap();
    let window = window.finish_checks(window.revision()).unwrap();
    let window = window
        .record_card_spell(window.revision(), A, source)
        .unwrap();
    window.entries()[0].id()
}
#[test]
fn cl_fault_query_invariant_propagates_to_target_gate_without_mutation() {
    let mut state = seed(vec![Program::creature(
        Some(ManaCost::free()),
        2.into(),
        4.into(),
        BTreeSet::new(),
    )]);
    let target = object(&state, 1);
    state
        .characteristics
        .qualification_remove_object_timestamp(target);
    let saved = state.clone();
    let expected = EffectError::CharacteristicInvariant(
        CharacteristicInvariantError::MissingBattlefieldTimestamp(target),
    );
    assert_eq!(state.evaluated_characteristics(), Err(expected.clone()));
    let body = ExecutableBody::new(
        BodyTarget::SingleCreature,
        vec![Instruction::Damage { amount: 1.into() }],
    );
    assert_eq!(
        state.legal_body_target(&body, TargetRef::Object(target), A),
        Err(expected)
    );
    assert_eq!(state, saved);
    assert!(state.state_based_losses.is_empty());
}
#[test]
fn cl_fault_cleanup_publication_rolls_back_expiry_and_all_battlefield_marks() {
    let mut artifact = Program::artifact(Some(ManaCost::free()), vec![]);
    let ProgramKind::Permanent(definition) = &mut artifact.kind else {
        unreachable!()
    };
    assert_eq!(artifact.card_types, BTreeSet::from([CardType::Artifact]));
    definition.printed_pt = Some(
        PowerToughness {
            power: 1.into(),
            toughness: 1.into(),
        }
        .into(),
    );
    let mut state = seed(vec![
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new()),
        artifact,
    ]);
    let x = object(&state, 1);
    let y = object(&state, 2);
    let boost = state
        .characteristics
        .add_effect(
            retired_entry(),
            x,
            ResolvedParts(vec![ResolvedPart::ModifyPT(PowerToughness {
                power: 0.into(),
                toughness: 2.into(),
            })]),
            ResolvedDuration::UntilEndOfTurn,
        )
        .unwrap();
    state.damage.insert(x, 2.into());
    state.damage.insert(y, 3.into());
    state.triggers.set_test_next_event(u64::MAX);
    let saved = state.clone();
    let turn = TurnScheduler::new(
        TurnStart {
            number: 1,
            step: Step::Cleanup,
        },
        false,
    )
    .unwrap();
    assert_eq!(
        state.begin_cleanup(state.revision(), &turn),
        Err(EffectError::Trigger(TriggerError::EventIdentityExhausted))
    );
    assert_eq!(state, saved);
    let creature = state.creature(x).unwrap().unwrap();
    assert_eq!((creature.power, creature.toughness), (2.into(), 4.into()));
    assert_eq!(state.creature(y).unwrap(), None);
    assert_eq!(state.battlefield_marked_damage(x), Some(2.into()));
    assert_eq!(state.battlefield_marked_damage(y), Some(3.into()));
    assert_eq!(state.continuous_effect(boost.id), Some(&boost));
    assert!(state.state_based_losses.is_empty());
}

#[test]
fn face_down_layer_one_b_masks_the_latest_copy_and_preview_restores_that_copy() {
    use crate::face_down::{FaceDownKind, FaceUpAbility};
    let mut printed = Program::creature(None, 1.into(), 5.into(), BTreeSet::new());
    printed
        .face_up_abilities
        .push(FaceUpAbility::Morph(ManaCost::one(ManaType::Green)));
    let mut source = seed(vec![printed]);
    let card = object(&source, 1);
    let mut borrowed = Program::creature(None, 4.into(), 7.into(), BTreeSet::new());
    borrowed
        .face_up_abilities
        .push(FaceUpAbility::Disguise(ManaCost::one(ManaType::Red)));
    source
        .characteristics
        .add_effect(
            retired_entry(),
            card,
            ResolvedParts(vec![ResolvedPart::Copy(Box::new(borrowed.clone()))]),
            ResolvedDuration::NoStatedDuration,
        )
        .unwrap();
    source
        .window
        .set_orientation(card, Some(FaceDownKind::Morph))
        .unwrap();
    assert_eq!(
        source.copiable_program(card),
        Some(FaceDownKind::Morph.definition())
    );
    let shown = source.face_up_preview(card).unwrap().unwrap();
    assert_eq!(
        shown.printed_text.face_up_abilities,
        borrowed.face_up_abilities
    );
    assert_eq!(shown.power_toughness.unwrap().derived.toughness, 7.into());
    assert_eq!(
        source.window.cards().get(card).unwrap().face_down(),
        Some(FaceDownKind::Morph)
    );
    source.window.set_orientation(card, None).unwrap();
    assert_eq!(source.copiable_program(card), Some(&borrowed));
}
