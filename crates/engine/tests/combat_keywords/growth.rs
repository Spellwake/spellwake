//! CR 702.100, 702.134 and 702.165 with live targets and retained battlefield information.
use super::{
    keyword_damage::{cast_on, combat_creature},
    keyword_triggers::place,
    upkeep_keywords::upkeep,
    *,
};
use spellwake_engine::{
    characteristics::{
        AbilityChange, AbilityRemoval, KeywordAbility as K, PowerToughness, ResolvedDuration,
        ResolvedPartSpec, ResolvedPartsSpec,
    },
    mana::ManaPayment,
    occurrence::{EntryCompletion, RulesOccurrence, RulesRecord},
    program::{GrowthKeyword as G, TriggeredAbilityId},
};

fn grow(mut seed: SeedCard, keywords: &[G]) -> SeedCard {
    seed.program.triggered = keywords
        .iter()
        .cloned()
        .enumerate()
        .map(|(n, k)| k.ability(TriggeredAbilityId(n as u64)))
        .collect();
    seed
}
fn target(s: &GameState, n: u64) -> TargetRef {
    TargetRef::Object(object(s, n).id())
}
fn choose(s: &GameState, n: u64) -> GameState {
    reply(s, DecisionReply::TriggerTarget(target(s, n))).into_state()
}
fn count(s: &GameState, n: u64) -> spellwake_contracts::RulesInteger {
    s.execution()
        .power_toughness_counters(object(s, n).id())
        .unwrap()
        .plus_one
}
fn modify(id: u64, p: i64, t: i64) -> SeedCard {
    instant(
        id,
        A,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(PowerToughness {
                power: p.into(),
                toughness: t.into(),
            })]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
    )
}
fn cast_free(s: &GameState, n: u64) -> GameState {
    let cast = act(
        s,
        A,
        GameCommand::CastUntargeted {
            spell: object(s, n).id(),
            payment: Some(ManaPayment::default()),
        },
    );
    round(cast.state()).into_state()
}
fn hand(n: u64, p: i64, t: i64) -> SeedCard {
    let mut seed = creature(n, A, ObjectDestination::Hand);
    seed.program = Program::creature(
        Some(ManaCost::free()),
        p.into(),
        t.into(),
        Default::default(),
    );
    seed
}
fn occurred(step: &NativeCandidate, predicate: impl Fn(&RulesOccurrence) -> bool) -> bool {
    step.rules_journal()
        .records
        .iter()
        .any(|record| match record {
            RulesRecord::Event(event) => event.occurrences.iter().any(|o| predicate(&o.value)),
            _ => false,
        })
}
fn fizzled(step: &NativeCandidate) -> bool {
    occurred(step, |value| {
        matches!(
            value,
            RulesOccurrence::EntryCompleted {
                completion: EntryCompletion::AllTargetsIllegal,
                ..
            }
        )
    })
}
#[test]
fn mentor_has_only_smaller_attacking_targets_and_each_resolution_rechecks() {
    let mentor = grow(combat_creature(1, A, 3, 3, &[]), &[G::Mentor, G::Mentor]);
    let state = setup(
        vec![A, B],
        vec![
            mentor,
            combat_creature(2, A, 2, 3, &[]),
            combat_creature(3, A, 3, 3, &[]),
            combat_creature(4, A, 1, 1, &[]),
            combat_creature(5, B, 1, 1, &[]),
        ],
        Step::Untap,
    )
    .into_state();
    let mut game = place(declared(&attackers(&state), &[(1, B), (2, B), (3, B)]).into_state());
    for _ in 0..2 {
        let RuleFrame::TriggerTarget(f) = game.pending_decision().unwrap().frame() else {
            panic!("mentor target")
        };
        assert_eq!(f.domain(), &[target(&game, 2)]);
        game = choose(&game, 2);
    }
    let first = round(&game);
    let mentored = |value: &RulesOccurrence| matches!(value,RulesOccurrence::CreatureMentored{source,target} if *source==object(&game,1).id() && *target==object(&game,2).id());
    assert!(occurred(&first, mentored));
    assert_eq!(count(first.state(), 2), 1.into());
    let second = round(first.state());
    assert!(fizzled(&second));
    assert!(!occurred(&second, mentored));
    assert_eq!(count(second.state(), 2), 1.into());
}
#[test]
fn mentor_uses_departure_power_when_source_changes_and_leaves_in_response() {
    let mentor = grow(combat_creature(1, A, 3, 4, &[]), &[G::Mentor]);
    let state = setup(
        vec![A, B],
        vec![
            mentor,
            combat_creature(2, A, 2, 3, &[]),
            modify(4, -2, 0),
            instant(5, A, Instruction::ReturnToOwnersHand),
        ],
        Step::Untap,
    )
    .into_state();
    let selected = choose(declared(&attackers(&state), &[(1, B), (2, B)]).state(), 2);
    let source = object(&selected, 1).id();
    let smaller = cast_on(&selected, 4, 1);
    let left = cast_on(&smaller, 5, 1);
    assert_eq!(
        left.execution().growth_last_known()[&source]
            .current_power_toughness
            .as_ref()
            .unwrap()
            .power,
        1.into()
    );
    let result = round(&left);
    assert!(fizzled(&result));
    assert_eq!(count(result.state(), 2), 0.into());
    assert!(result.state().execution().growth_last_known().is_empty());
}
#[test]
fn evolve_uses_either_stat_at_each_check_and_multiple_instances_compare_again() {
    let watcher = grow(combat_creature(1, A, 1, 3, &[]), &[G::Evolve, G::Evolve]);
    let state = setup(
        vec![A, B],
        vec![watcher, hand(2, 2, 2), modify(3, -1, 2)],
        Step::PrecombatMain,
    )
    .into_state();
    let ready = place(cast_free(&state, 2));
    assert_eq!(ready.execution().window().entries().len(), 2);
    let first = round(&ready);
    let evolved = |value: &RulesOccurrence| matches!(value,RulesOccurrence::CreatureEvolved{object:source} if *source==object(&ready,1).id());
    assert!(occurred(&first, evolved));
    let second = round(first.state());
    assert!(!occurred(&second, evolved));
    assert_eq!(count(second.state(), 1), 1.into());
    let alternate = cast_on(&ready, 3, 2);
    let growth = round(&alternate);
    assert_eq!(count(growth.state(), 1), 1.into());
}
#[test]
fn evolve_uses_the_entering_creatures_last_battlefield_stats() {
    let watcher = grow(combat_creature(1, A, 1, 3, &[]), &[G::Evolve]);
    let state = setup(
        vec![A, B],
        vec![
            watcher,
            hand(2, 2, 2),
            modify(3, -1, 0),
            instant(4, A, Instruction::ReturnToOwnersHand),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let ready = cast_free(&state, 2);
    let left = cast_on(&ready, 4, 2);
    let passes = round(&left);
    assert_eq!(count(passes.state(), 1), 1.into());
    let smaller = cast_on(&ready, 3, 2);
    let left = cast_on(&smaller, 4, 2);
    let fails = round(&left);
    assert_eq!(count(fails.state(), 1), 0.into());
    assert!(fails.state().execution().growth_last_known().is_empty());
}
#[test]
fn backup_grants_the_printed_suffix_to_another_creature_until_end_of_turn() {
    let mut source = hand(1, 2, 2);
    source.program.printed_keywords = [K::Haste, K::Flying, K::Lifelink].into();
    let source = grow(
        source,
        &[G::Backup {
            amount: 2.into(),
            printed_below: [K::Flying, K::Lifelink].into(),
        }],
    );
    let state = setup(
        vec![A, B],
        vec![
            source,
            battlefield(2, B),
            instant(3, A, Instruction::ReturnToOwnersHand),
            instant(
                4,
                A,
                Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                        AbilityChange::remove(AbilityRemoval::All),
                    )]),
                    duration: ResolvedDuration::UntilEndOfTurn,
                },
            ),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let targets = cast_free(&state, 1);
    let self_target = choose(&targets, 1);
    let same = round(&self_target);
    assert_eq!(count(same.state(), 1), 2.into());
    assert_eq!(same.state().execution().continuous_effects().count(), 0);
    let other = choose(&targets, 2);
    let stripped = cast_on(&other, 4, 1);
    let after_strip = round(&stripped).into_state();
    let stats = after_strip
        .execution()
        .current_characteristics(object(&after_strip, 2).id())
        .unwrap()
        .unwrap();
    assert!(stats.has_keyword(K::Flying) && stats.has_keyword(K::Lifelink));
    let departed = cast_on(&other, 3, 1);
    let applied = round(&departed).into_state();
    assert_eq!(count(&applied, 2), 2.into());
    let stats = applied
        .execution()
        .current_characteristics(object(&applied, 2).id())
        .unwrap()
        .unwrap();
    assert!(stats.has_keyword(K::Flying) && stats.has_keyword(K::Lifelink));
    assert!(!stats.has_keyword(K::Haste));
    let after = upkeep(applied, 2);
    let stats = after
        .execution()
        .current_characteristics(object(&after, 2).id())
        .unwrap()
        .unwrap();
    assert!(!stats.has_keyword(K::Flying) && !stats.has_keyword(K::Lifelink));
    assert_eq!(count(&after, 2), 2.into());
}

#[test]
fn increment_compares_mana_actually_paid_and_rechecks_creature_stats_each_resolution() {
    use spellwake_engine::mana::{CastingPayment, ManaAbilityId, ManaAbilityRef, ManaSpend};
    let mut spell = hand(2, 2, 2);
    spell.program = Program::creature(
        Some(ManaCost::generic(3.into())),
        2.into(),
        2.into(),
        Default::default(),
    )
    .with_evoke(ManaCost::free(), TriggeredAbilityId(0));
    let game = setup(
        vec![A, B],
        vec![
            grow(
                combat_creature(1, A, 2, 2, &[]),
                &[G::Increment, G::Increment],
            ),
            spell,
            upkeep_keywords::land(3, ManaType::Blue, 3),
            grow(combat_creature(4, B, 0, 2, &[]), &[G::Increment]),
        ],
        Step::PrecombatMain,
    )
    .into_state();
    let free = act(
        &game,
        A,
        GameCommand::CastWithResources {
            spell: object(&game, 2).id(),
            target: None,
            payment: CastingPayment {
                use_alternative: true,
                ..Default::default()
            },
        },
    );
    assert_eq!(free.state().execution().window().entries().len(), 1);
    let pool = act(
        &game,
        A,
        GameCommand::ActivateMana {
            source: object(&game, 3).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    )
    .into_state();
    let cast = act(
        &pool,
        A,
        GameCommand::CastUntargeted {
            spell: object(&pool, 2).id(),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 3.into(),
                }],
            }),
        },
    )
    .into_state();
    let stack = place(cast);
    assert_eq!(stack.execution().window().entries().len(), 3);
    let first = round(&stack);
    let second = round(first.state());
    assert_eq!(count(second.state(), 1), 1.into());
    assert_eq!(count(second.state(), 4), 0.into());
}
