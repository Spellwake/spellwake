//! Counter-based upkeep and graveyard returns through the normal scheduler.
use super::{keyword_damage::cast_on, keyword_triggers::place, upkeep_keywords::upkeep, *};
use spellwake_engine::{
    characteristics::PowerToughnessCounters,
    mana::ManaPayment,
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    program::{CounterKeyword as K, TriggeredAbilityId},
};
fn authored(mut seed: SeedCard, keywords: &[K]) -> SeedCard {
    seed.program.triggered = keywords
        .iter()
        .copied()
        .enumerate()
        .map(|(n, k)| k.ability(TriggeredAbilityId(n as u64)))
        .collect();
    seed
}
fn sufficient_libraries(mut seeds: Vec<SeedCard>) -> Vec<SeedCard> {
    for (i, player) in [A, B].into_iter().enumerate() {
        for n in 0..4 {
            seeds.push(creature(
                50 + i as u64 * 10 + n,
                player,
                ObjectDestination::LibraryBottom,
            ));
        }
    }
    seeds
}
fn creature_spell(state: &GameState, n: u64) -> NativeCandidate {
    let cast = act(
        state,
        A,
        GameCommand::CastUntargeted {
            spell: object(state, n).id(),
            payment: Some(ManaPayment::default()),
        },
    );
    round(cast.state())
}
fn process(state: GameState) -> GameState {
    let mut state = place(state);
    while !state.execution().window().entries().is_empty() {
        state = place(round(&state).into_state());
    }
    state
}
fn pt(state: &GameState, id: u64) -> PowerToughnessCounters {
    state
        .execution()
        .power_toughness_counters(object(state, id).id())
        .unwrap()
}
#[test]
fn fading_and_vanishing_enter_with_counters_and_sacrifice_at_distinct_times() {
    let seed = authored(creature(1, A, ObjectDestination::Hand), &[K::Fading(1)]);
    let vanish = authored(
        creature(2, A, ObjectDestination::Hand),
        &[K::Vanishing(Some(1)), K::VanishingLastCounter],
    );
    let state = setup(
        vec![A, B],
        sufficient_libraries(vec![seed, vanish]),
        Step::PrecombatMain,
    )
    .into_state();
    let first = creature_spell(&state, 1);
    let id1 = object(first.state(), 1).id();
    assert!(first.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.after.object.id()==id1 && m.after.fade_counters==Some(1.into()))))));
    let second = creature_spell(first.state(), 2);
    let id2 = object(second.state(), 2).id();
    assert!(second.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.after.object.id()==id2 && m.after.time_counters==Some(1.into()))))));
    let ready = place(upkeep(second.into_state(), 3));
    assert_eq!(ready.execution().window().entries().len(), 2);
    let mut next = ready;
    let mut saw_last = false;
    while !next.execution().window().entries().is_empty() {
        let step = round(&next);
        if step.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::TimeCounterRemoved{object,after,..} if *object==id2 && !after.is_positive())))){
            saw_last=true;
            assert_eq!(object(step.state(),2).zone(),ObjectZone::Battlefield);
            assert!(!step.state().execution().window().entries().is_empty());
        }
        next = place(step.into_state());
    }
    assert!(saw_last);
    assert_eq!(object(&next, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(object(&next, 1).zone(), ObjectZone::Battlefield);
    assert!(next.execution().fade_counters().is_empty());
    let expired = process(upkeep(next, 5));
    assert_eq!(object(&expired, 1).zone(), ObjectZone::Graveyard(A));
}
#[test]
fn multiple_vanishing_instances_share_counters_and_trigger_separately_when_the_last_is_removed() {
    let kinds = [
        K::Vanishing(Some(1)),
        K::VanishingLastCounter,
        K::Vanishing(Some(1)),
        K::VanishingLastCounter,
    ];
    let seed = authored(creature(1, A, ObjectDestination::Hand), &kinds);
    let without = authored(
        creature(2, A, ObjectDestination::Hand),
        &[K::Vanishing(None), K::VanishingLastCounter],
    );
    let state = setup(
        vec![A, B],
        sufficient_libraries(vec![seed, without]),
        Step::PrecombatMain,
    )
    .into_state();
    let first = creature_spell(&state, 1).into_state();
    let second = creature_spell(&first, 2).into_state();
    let id = object(&second, 1).id();
    assert_eq!(second.execution().time_counters()[&id], 2.into());
    let ready = place(upkeep(second, 3));
    assert_eq!(ready.execution().window().entries().len(), 2);
    let one = round(&ready).into_state();
    assert_eq!(one.execution().time_counters()[&id], 1.into());
    let gone = place(round(&one).into_state());
    assert_eq!(gone.execution().window().entries().len(), 2);
    assert!(!gone.execution().time_counters().contains_key(&id));
    let first = round(&gone).into_state();
    assert_eq!(object(&first, 1).zone(), ObjectZone::Graveyard(A));
    let all = round(&first).into_state();
    assert_eq!(object(&all, 2).zone(), ObjectZone::Battlefield);
}
#[test]
fn death_keyword_uses_predeath_counters_and_returns_once_with_its_counter_under_its_owner() {
    for (keyword, plus, minus, reason) in [
        (K::Persist, 0i64, 1i64, ZoneReason::PersistReturn),
        (K::Undying, 1, 0, ZoneReason::UndyingReturn),
    ] {
        let mut controlled = authored(battlefield(1, A), &[keyword, keyword]);
        controlled.owner = B;
        let state = setup(
            vec![A, B],
            vec![
                controlled,
                instant(2, A, Instruction::DestroyTarget),
                instant(3, A, Instruction::DestroyTarget),
            ],
            Step::PrecombatMain,
        )
        .into_state();
        let original = object(&state, 1).id();
        let lost = place(cast_on(&state, 2, 1));
        assert_eq!(lost.execution().window().entries().len(), 2);
        assert_eq!(object(&lost, 1).zone(), ObjectZone::Graveyard(B));
        let restored = round(&lost);
        let id = object(restored.state(), 1).id();
        assert_ne!(id, original);
        assert_eq!(object(restored.state(), 1).controller(), Some(B));
        assert!(restored.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.reason==reason && m.after.battlefield_counters.as_ref().is_some_and(|c|c.plus_one==plus.into() && c.minus_one==minus.into()))))));
        let completed = round(restored.state()).into_state();
        assert_eq!(object(&completed, 1).id(), id);
        let gone = cast_on(&completed, 3, 1);
        assert!(gone.execution().window().entries().is_empty());
        assert_eq!(object(&gone, 1).zone(), ObjectZone::Graveyard(B));
    }
}
#[test]
fn persist_and_undying_compare_only_their_own_counter_type() {
    let source = authored(battlefield(1, A), &[K::Persist, K::Undying]);
    let seeds = vec![
        source,
        instant(
            2,
            A,
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 0.into(),
                },
            },
        ),
        instant(3, A, Instruction::DestroyTarget),
        instant(4, A, Instruction::DestroyTarget),
    ];
    let state = setup(vec![A, B], seeds, Step::PrecombatMain).into_state();
    let with_plus = cast_on(&state, 2, 1);
    let lost = cast_on(&with_plus, 3, 1);
    assert_eq!(lost.execution().window().entries().len(), 1);
    let persisted = round(&lost).into_state();
    assert_eq!(pt(&persisted, 1).minus_one, 1.into());
    let lost = cast_on(&persisted, 4, 1);
    assert_eq!(lost.execution().window().entries().len(), 1);
    let undying = round(&lost).into_state();
    assert_eq!(pt(&undying, 1).plus_one, 1.into());
}

#[test]
fn simultaneous_lethal_toughness_and_counter_cancellation_use_precheck_lki() {
    let permanent = authored(battlefield(1, A), &[K::Persist, K::Undying]);
    let spell = instant(
        2,
        A,
        Instruction::PutPowerToughnessCounters {
            counters: PowerToughnessCounters {
                plus_one: 1.into(),
                minus_one: 5.into(),
            },
        },
    );
    let state = setup(vec![A, B], vec![permanent, spell], Step::PrecombatMain).into_state();
    let grave = cast_on(&state, 2, 1);
    assert_eq!(object(&grave, 1).zone(), ObjectZone::Graveyard(A));
    assert!(grave.execution().window().entries().is_empty());
}

#[test]
fn a_token_has_a_death_trigger_but_cannot_return_from_its_graveyard() {
    use spellwake_engine::token::{TokenCreation, TokenDefinition, TokenName};
    let mut token = Program::creature(None, 2.into(), 2.into(), Default::default());
    token
        .triggered
        .push(K::Persist.ability(TriggeredAbilityId(1)));
    let token = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &test_vocabulary(),
    )
    .unwrap();
    let maker = SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(token)],
                }],
            ),
        ),
    };
    let state = setup(
        vec![A, B],
        vec![maker, instant(2, A, Instruction::DestroyTarget)],
        Step::PrecombatMain,
    )
    .into_state();
    let born = creature_spell(&state, 1).into_state();
    let id = born
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token())
        .unwrap()
        .id();
    let kill = act(
        &born,
        A,
        GameCommand::Cast {
            spell: object(&born, 2).id(),
            target: TargetRef::Object(id),
        },
    );
    let grave = round(kill.state()).into_state();
    assert_eq!(grave.execution().window().entries().len(), 1);
    assert!(
        !grave
            .execution()
            .window()
            .cards()
            .iter()
            .any(|o| o.is_token())
    );
    let done = round(&grave).into_state();
    assert!(done.execution().window().entries().is_empty());
    assert!(
        !done
            .execution()
            .window()
            .cards()
            .iter()
            .any(|o| o.is_token())
    );
}
