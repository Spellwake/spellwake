//! Real lower-owner casts and explicit malformed-view probes, not full card tests.
use super::*;
use crate::{
    life::LifeAdjustment,
    occurrence::{RulesRecord, TriggerError},
};

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
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    }],
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
    let capture = snapshot.view().cast_occurrence(entry, A).unwrap();
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
            .cast_occurrence(entry, A),
        Err(TriggerError::InvalidWorld)
    );
    assert_eq!(
        snapshot.view().cast_occurrence(entry, B),
        Err(TriggerError::InvalidWorld)
    );
    assert_eq!(
        snapshot
            .view_with_cards(before.window.cards())
            .unwrap()
            .cast_occurrence(entry, A),
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
        .cast_occurrence(entry, A)
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
            view.cast_occurrence(entry, A),
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
            .cast_occurrence(copy, A),
        Err(TriggerError::InvalidWorld)
    );
}

#[test]
fn privileged_projection_carries_the_same_cast_through_events_waiting_and_bindings() {
    use crate::{
        game::{GameCommand, RuleFrame},
        program::{
            OrdinaryCondition, PlayerRelation, SpellCastFilter, TriggerGrouping, TriggeredAbility,
            TriggeredAbilityId,
        },
        test_support::{
            characteristic_trajectories as tr, packet::AliasContext, program as codec, projection,
        },
    };
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    for self_cast in [false, true] {
        let mut spell = Program::instant(ManaCost::free(), body.clone());
        if self_cast {
            spell.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition: OrdinaryCondition::SelfCast,
                grouping: TriggerGrouping::EachOccurrence,
                body: body.clone(),
            });
        }
        let definitions = spell.triggered.clone();
        let mut watcher = Program::artifact(None, vec![]);
        watcher.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::SpellCast {
                player: PlayerRelation::Controller,
                filter: SpellCastFilter::AnySpell,
            },
            grouping: TriggerGrouping::EachOccurrence,
            body: body.clone(),
        });
        let game = tr::world(
            tr::vocabulary(),
            vec![tr::card(1, spell, false), tr::card(2, watcher, true)],
            vec![CardId(2)],
        );
        let cast = tr::submit(
            &game,
            A,
            GameCommand::CastUntargeted {
                spell: tr::object(&game, 1),
                payment: None,
            },
        );
        let journal = projection::journal(cast.rules_journal());
        let fact = journal
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["tag"] == "Event")
            .flat_map(|row| row["occurrences"].as_array().unwrap())
            .find(|row| row["value"]["tag"] == "CastCompleted")
            .unwrap();
        let encoded_cast = &fact["value"]["cast"];
        assert!(encoded_cast.is_object());
        assert_eq!(encoded_cast["controller"], A.0);
        assert!(fact["value"].get("entry").is_none());
        for row in journal
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["tag"] == "TriggerDiscovered")
        {
            let related = &row["value"]["causes"]["members"][0]["related"];
            assert_eq!(related["tag"], "SpellCast");
            assert_eq!(&related["cast"], encoded_cast);
        }
        let mut state = cast.into_state();
        if self_cast {
            let pending = state.pending_decision().unwrap();
            let RuleFrame::Triggers(frame) = pending.frame() else {
                panic!("expected trigger order")
            };
            state = tr::reply(
                &state,
                A,
                DecisionReply::OrderTriggers(frame.instances().to_vec()),
            )
            .into_state();
        }
        let view = projection::effect(state.execution(), &AliasContext::default()).unwrap();
        let rows = view["stack"].as_array().unwrap();
        let original = rows
            .iter()
            .find(|row| row["kind"]["tag"] == "CardSpell")
            .unwrap();
        assert_eq!(
            original["copiable_triggered"],
            codec::triggered_abilities(&definitions)
        );
        assert_eq!(
            original["copiable_triggered"].as_array().unwrap().len(),
            usize::from(self_cast)
        );
        let triggers: Vec<_> = rows
            .iter()
            .filter(|row| row["kind"]["tag"] == "TriggeredAbility")
            .collect();
        assert_eq!(triggers.len(), 1 + usize::from(self_cast));
        for row in triggers {
            assert!(row.as_object().unwrap().contains_key("copiable_triggered"));
            assert!(row["copiable_triggered"].is_null());
            assert_eq!(
                &row["bindings"]["trigger"]["causes"]["members"][0]["related"]["cast"],
                encoded_cast
            );
        }
    }
}
