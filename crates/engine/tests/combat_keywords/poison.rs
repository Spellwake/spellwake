#![allow(dead_code, unused_imports)]
//! CR 702.70, 702.80, 702.90, 702.164 and 704.5c, through scheduled game inputs.
use super::keyword_damage::{blocks, cast_on, change, combat_creature};
use super::keyword_triggers::{add, place};
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{AbilityChange, AbilityRemoval, KeywordAbility::*, PowerToughnessCounters},
    draw::GameOutcome,
    mana::ManaPayment,
    occurrence::{PlayerLossReason, PoisonOrigin, RulesOccurrence, RulesRecord},
    program::CombatTriggerKeyword::Poisonous,
};

fn occurrences(candidate: &NativeCandidate) -> impl Iterator<Item = &RulesOccurrence> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| {
            if let RulesRecord::Event(event) = row {
                Some(&event.occurrences)
            } else {
                None
            }
        })
        .flatten()
        .map(|row| &row.value)
}

#[test]
fn infect_lifelink_and_toxic_apply_together_with_ordinary_simultaneous_damage() {
    let mut venom = combat_creature(1, A, 3, 4, &[Infect, Lifelink]);
    venom.program.printed_toxic = Some(2.into());
    let ordinary = combat_creature(2, A, 4, 4, &[Wither]);
    let game = attackers(setup(vec![A, B], vec![venom, ordinary], Step::Untap).state());
    let attack = declared(&game, &[(1, B), (2, B)]);
    let declare_blockers = round(attack.state());
    let damage = round(declare_blockers.state());
    assert_eq!(damage.state().execution().life_total(B), Some(&16.into()));
    assert_eq!(damage.state().execution().life_total(A), Some(&23.into()));
    assert_eq!(damage.state().execution().poison_counters(B), 5.into());
    let receipts: Vec<_> = occurrences(&damage)
        .filter_map(|row| {
            if let RulesOccurrence::PlayerPoisoned {
                player,
                before,
                after,
                contributions,
            } = row
            {
                assert_eq!((*player, before, after), (B, &0.into(), &5.into()));
                Some(contributions)
            } else {
                None
            }
        })
        .collect();
    let [contributions] = receipts.as_slice() else {
        panic!("one atomic poison change for the recipient")
    };
    let [infect, toxic] = contributions.as_slice() else {
        panic!("both poisonous damage results retain their origin")
    };
    assert!(
        matches!((infect.origin,toxic.origin),(PoisonOrigin::Infect(a),PoisonOrigin::Toxic(b)) if a==b)
    );
    assert_eq!((&infect.amount, &toxic.amount), (&3.into(), &2.into()));
}

#[test]
fn toxic_adds_each_ability_once_per_hit_and_layer_six_removal_applies_between_strikes() {
    let mut attacker = combat_creature(1, A, 4, 4, &[DoubleStrike]);
    attacker.program.printed_toxic = Some(2.into());
    let blocked = blocks(
        vec![
            attacker,
            instant(
                3,
                A,
                change(AbilityChange {
                    granted_toxic: Some(1.into()),
                    ..Default::default()
                }),
            ),
            instant(4, A, change(AbilityChange::remove(AbilityRemoval::All))),
        ],
        &[],
    );
    let granted = cast_on(&blocked, 3, 1);
    let early = round(&granted);
    assert_eq!(early.state().execution().life_total(B), Some(&16.into()));
    assert_eq!(early.state().execution().poison_counters(B), 3.into());
    let normal = round(early.state());
    assert_eq!(normal.state().execution().poison_counters(B), 6.into());
    let removed = cast_on(early.state(), 4, 1);
    assert_eq!(
        removed
            .execution()
            .evaluated_characteristics()
            .unwrap()
            .get(object(&removed, 1).id())
            .unwrap()
            .toxic_value,
        None
    );
    assert_eq!(
        round(&removed).state().execution().poison_counters(B),
        3.into()
    );
}

#[test]
fn protected_player_receives_only_unprevented_infect_and_one_toxic_amount_on_a_hit() {
    for (shield, remaining, poison, entries) in [(2, 1, 3, 1), (3, 0, 0, 0)] {
        let mut source = combat_creature(1, A, 3, 3, &[Infect, Lifelink]);
        source.program.printed_toxic = Some(2.into());
        let mut protection = instant(
            3,
            A,
            Instruction::ApplyContinuous(Box::new(
                spellwake_engine::toolbox::ApplyContinuousSpec {
                    selector: None,
                    payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
                        amount: shield.into(),
                    },
                    duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                    condition: None,
                    one_shot: true,
                    as_emblem: None,
                    emblem_label: None,
                },
            )),
        );
        let spellwake_engine::program::ProgramKind::Spell(spell) = &mut protection.program.kind
        else {
            unreachable!()
        };
        spell.body.target = BodyTarget::SinglePlayer;
        let mut seeds = vec![source, protection];
        add(&mut seeds, 1, &[Poisonous(1.into())]);
        let game = blocks(seeds, &[]);
        let cast = act(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, 3).id(),
                target: TargetRef::Player(B),
            },
        );
        let protected = round(cast.state());
        let hit = round(protected.state());
        assert_eq!(hit.state().execution().window().entries().len(), entries);
        assert_eq!(hit.state().execution().poison_counters(B), poison.into());
        assert_eq!(
            hit.state().execution().life_total(A),
            Some(&(20 + remaining).into())
        );
        assert_eq!(hit.state().execution().life_total(B), Some(&20.into()));
    }
}

#[test]
fn tenth_poison_counter_checks_loss_with_other_reasons_and_multiple_recipients() {
    for (seats, amount) in [(vec![A, B], 9), (vec![A, B], 10), (vec![A, B, C], 10)] {
        let mut first = combat_creature(1, A, 20, 4, &[]);
        first.program.printed_toxic = Some(amount.into());
        if amount == 9 {
            first = combat_creature(1, A, 9, 4, &[Infect]);
        }
        let mut seeds = vec![first];
        let mut attacks = vec![(1, B)];
        if seats.len() == 3 {
            seeds.push(combat_creature(2, A, 10, 4, &[Infect]));
            attacks.push((2, C));
        }
        let ready = attackers(setup(seats.clone(), seeds, Step::Untap).state());
        let attack = declared(&ready, &attacks);
        let blocking = round(attack.state());
        let damage = round(blocking.state());
        let state = damage.state();
        assert_eq!(state.execution().poison_counters(B), amount.into());
        if amount == 9 {
            assert!(state.execution().state_based_losses().is_empty());
            assert_eq!(state.outcome(), None);
        } else {
            assert!(occurrences(&damage).any(|row|matches!(row,RulesOccurrence::PlayerLost{player,reasons} if *player==B && reasons.contains(&PlayerLossReason::TenPoisonCounters) && reasons.contains(&PlayerLossReason::NonpositiveLife))));
            assert!(state.execution().state_based_losses().contains(&B));
            if seats.len() == 2 {
                assert_eq!(state.outcome(), Some(GameOutcome::Winner(A)));
            } else {
                assert!(state.execution().state_based_losses().contains(&C));
                assert!(occurrences(&damage).any(|row|matches!(row,RulesOccurrence::PlayerLost{player,reasons} if *player==C && reasons==&[PlayerLossReason::TenPoisonCounters].into())));
            }
        }
    }
}
