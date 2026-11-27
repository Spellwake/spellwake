//! Real lower-owner casts and explicit malformed-view probes, not full card tests.
use super::*;
use crate::occurrence::{RulesRecord, TriggerError};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn ready() -> EffectWindow {
    let state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![crate::program::Instruction::AdjustLife(Box::new(
                        crate::toolbox::AdjustLifeSpec::gain(
                            crate::toolbox::PlayerSelector::Resolving(
                                crate::program::ResolvingPlayer::Controller,
                            ),
                            crate::program::ValueExpr::Literal(1.into()),
                        ),
                    ))],
                ),
            ),
        }],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::selected_vocabulary().unwrap(),
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    state.settle_checks(state.revision()).unwrap()
}

#[test]
fn completed_capture_owns_the_exact_new_stack_incarnation() {
    let before = ready();
    let hand = before.window.cards().current_identity(CardId(1)).unwrap();
    let saved = before.clone();
    let cast = before
        .cast_on_turn(before.revision(), A, hand, None, None, None)
        .unwrap();
    assert_eq!(before, saved);
    let entry = cast.state().window.entries().last().unwrap().id();
    let snapshot = cast.state().rules_snapshot(None).unwrap();
    let capture = snapshot
        .view()
        .cast_occurrence(entry, A, &[], 0.into())
        .unwrap();
    let emitted = cast
        .append()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .find_map(|row| match &row.value {
            RulesOccurrence::CastCompleted(value) => Some(value.as_ref()),
            _ => None,
        })
        .unwrap();
    assert_eq!(&capture, emitted);
    assert_eq!(capture.entry(), entry);
    assert_eq!(capture.controller(), A);
    assert_ne!(capture.spell().object.id(), hand);
    assert_eq!(capture.spell().object.zone(), ObjectZone::Stack);
    assert_eq!(capture.spell().object.card_id(), Some(CardId(1)));
    drop(snapshot);
    drop(cast);
    assert_eq!(capture.spell().types, BTreeSet::from([CardType::Instant]));
}

#[test]
fn capture_rejects_missing_entry_wrong_controller_and_hand_era_overlay() {
    let before = ready();
    let hand = before.window.cards().current_identity(CardId(1)).unwrap();
    let cast = before
        .cast_on_turn(before.revision(), A, hand, None, None, None)
        .unwrap();
    let entry = cast.state().window.entries().last().unwrap().id();
    let snapshot = cast.state().rules_snapshot(None).unwrap();
    assert_eq!(
        before
            .rules_snapshot(None)
            .unwrap()
            .view()
            .cast_occurrence(entry, A, &[], 0.into()),
        Err(TriggerError::InvalidWorld)
    );
    assert_eq!(
        snapshot.view().cast_occurrence(entry, B, &[], 0.into()),
        Err(TriggerError::InvalidWorld)
    );
    assert_eq!(
        snapshot
            .view_with_cards(before.window.cards())
            .unwrap()
            .cast_occurrence(entry, A, &[], 0.into()),
        Err(TriggerError::InvalidWorld)
    );
}

#[test]
fn capture_rejects_incoherent_evaluated_identity_controller_and_roster() {
    let before = ready();
    let hand = before.window.cards().current_identity(CardId(1)).unwrap();
    let cast = before
        .cast_on_turn(before.revision(), A, hand, None, None, None)
        .unwrap();
    let entry = cast.state().window.entries().last().unwrap().id();
    let snapshot = cast.state().rules_snapshot(None).unwrap();
    let object = snapshot
        .view()
        .cast_occurrence(entry, A, &[], 0.into())
        .unwrap()
        .spell()
        .object
        .id();
    for defect in 0..5 {
        let mut view = snapshot.view();
        let evaluated = Arc::make_mut(&mut view.evaluated);
        if defect == 0 {
            evaluated.objects.remove(&object);
        } else {
            let current = evaluated.objects.get_mut(&object).unwrap();
            match defect {
                1 => current.object = hand,
                2 => current.controller = Some(B),
                3 => current.abilities.source = hand,
                4 => current.abilities.card = Some(CardId(99)),
                _ => unreachable!(),
            }
        }
        assert_eq!(
            view.cast_occurrence(entry, A, &[], 0.into()),
            Err(TriggerError::InvalidWorld)
        );
    }
}

#[test]
fn canonical_noncard_copy_is_not_a_physical_cast_capture() {
    let before = ready();
    let hand = before.window.cards().current_identity(CardId(1)).unwrap();
    let mut cast = before
        .cast_on_turn(before.revision(), A, hand, None, None, None)
        .unwrap()
        .into_parts()
        .0;
    let original = cast.window.entries().last().unwrap().id();
    cast.window = cast.window.finish_checks(cast.window.revision()).unwrap();
    cast.window = cast.window.pass(cast.window.revision(), A).unwrap();
    cast.window = cast.window.finish_checks(cast.window.revision()).unwrap();
    cast.window = cast.window.pass(cast.window.revision(), B).unwrap();
    // Explicit primitive allocator probe: this does not execute or publish CopyCreated.
    cast.window = cast
        .window
        .record_spell_copy(cast.window.revision(), original, A, vec![])
        .unwrap();
    let copy = cast.window.entries().last().unwrap().id();
    assert!(matches!(
        cast.window.entries().last().unwrap().kind(),
        StackKind::SpellCopy { .. }
    ));
    assert_eq!(
        cast.rules_snapshot(None)
            .unwrap()
            .view()
            .cast_occurrence(copy, A, &[], 0.into()),
        Err(TriggerError::InvalidWorld)
    );
}
