#![allow(dead_code, unused_imports)]
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
