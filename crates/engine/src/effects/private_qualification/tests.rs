use super::*;
use crate::{
    occurrence::{RulesOccurrence, RulesRecord, TriggerRelated},
    program::{ObjectRelation, OrdinaryCondition, TriggeredAbility, TriggeredAbilityId},
};

fn watched(mut program: Program, condition: OrdinaryCondition) -> Program {
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::InstructionAmount::Literal(1.into()),
            }],
        ),
    });
    program
}

#[test]
fn unordered_entry_batch_rejects_without_mutating_the_accepted_world() {
    let a = SeatId(0);
    let b = SeatId(1);
    let state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![
            SeedCard {
                card: CardId(200001),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: watched(
                    Program::artifact(None, vec![]),
                    OrdinaryCondition::Enters(ObjectRelation::AnotherControlledCreature),
                ),
            },
            SeedCard {
                card: CardId(200002),
                owner: a,
                destination: ObjectDestination::Hand,
                program: watched(
                    Program::creature(None, 1.into(), 1.into(), std::collections::BTreeSet::new()),
                    OrdinaryCondition::Enters(ObjectRelation::ControlledCreature),
                ),
            },
            SeedCard {
                card: CardId(200003),
                owner: a,
                destination: ObjectDestination::Hand,
                program: watched(
                    Program::creature(None, 1.into(), 1.into(), std::collections::BTreeSet::new()),
                    OrdinaryCondition::Enters(ObjectRelation::ControlledCreature),
                ),
            },
        ],
        vec![CardId(200001)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::synthetic_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    let moves: Vec<_> = [200002, 200003]
        .map(|card| {
            (
                state.window.cards().current_identity(CardId(card)).unwrap(),
                ObjectDestination::Battlefield { controller: a },
            )
        })
        .into();
    let saved = state.clone();
    let error = state
        .qualification_zone_moves(state.revision(), &moves)
        .unwrap_err();
    assert_eq!(
        error,
        EffectError::Characteristics(CharacteristicsError::UnsupportedTimestampProducer)
    );
    assert_eq!(state, saved);
    for id in [200002, 200003] {
        let identity = state.window.cards().current_identity(CardId(id)).unwrap();
        assert_eq!(
            state.window.cards().get(identity).unwrap().zone(),
            ObjectZone::Hand(a)
        );
    }
    assert_eq!(state.waiting_triggers().count(), 0);
    assert!(state.window.entries().is_empty());
}

#[test]
fn first_checks_capture_opposing_counters_before_death_and_place_the_singleton() {
    let a = SeatId(0);
    let b = SeatId(1);
    let mut state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![SeedCard {
            card: CardId(210001),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: watched(
                Program::creature(None, 0.into(), 0.into(), std::collections::BTreeSet::new()),
                OrdinaryCondition::Dies(ObjectRelation::SelfObject),
            ),
        }],
        vec![CardId(210001)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::synthetic_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    let object = state
        .window
        .cards()
        .current_identity(CardId(210001))
        .unwrap();
    let counters = PowerToughnessCounters {
        plus_one: 1.into(),
        minus_one: 1.into(),
    };
    state
        .qualification_prepare_counters(object, &counters)
        .unwrap();
    let saved = state.clone();
    let (next, result, append) = state.begin_checks(state.revision()).unwrap().into_parts();
    assert_eq!(state, saved);
    assert!(matches!(result, RuleResult::Complete { .. }));
    let event = append
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .unwrap();
    assert_eq!(event.occurrences.len(), 2);
    assert!(
        event.occurrences.iter().any(|occurrence| matches!(
            occurrence.value,
            RulesOccurrence::CountersCancelled { .. }
        ))
    );
    let discovery = append
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some(waiting),
            _ => None,
        })
        .unwrap();
    assert_eq!(discovery.source.object.id(), object);
    assert_eq!(
        discovery.source.battlefield_counters.as_ref().unwrap(),
        &counters
    );
    let TriggerRelated::ZoneChange { before, after } = &discovery.causes.members()[0].related
    else {
        panic!("death relation missing")
    };
    assert_eq!(before.battlefield_counters.as_ref().unwrap(), &counters);
    assert_eq!(after.object.zone(), ObjectZone::Graveyard(a));
    assert_eq!(after.current_power_toughness, Some(PowerToughness::zero()));
    assert!(next.waiting_triggers().next().is_none());
    assert_eq!(next.window.entries().len(), 1);
    let binding = next
        .execution_bindings(next.window.entries()[0].id())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source, discovery.source);
    assert_eq!(binding.causes, discovery.causes);
    assert_eq!(binding.instance, discovery.id);
}
