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
fn wither_and_infect_place_minus_counters_once_even_on_indestructible_blockers() {
    for keywords in [vec![Wither], vec![Infect], vec![Wither, Infect]] {
        let blocked = blocks(
            vec![
                combat_creature(1, A, 3, 4, &keywords),
                combat_creature(2, B, 1, 4, &[Indestructible]),
                instant(
                    3,
                    A,
                    Instruction::PutPowerToughnessCounters {
                        counters: PowerToughnessCounters {
                            plus_one: 1.into(),
                            minus_one: 0.into(),
                        },
                    },
                ),
            ],
            &[(2, 1)],
        );
        let strengthened = cast_on(&blocked, 3, 2);
        let result = round(&strengthened);
        let id = object(result.state(), 2).id();
        assert_eq!(
            result.state().execution().battlefield_marked_damage(id),
            Some(0.into())
        );
        assert_eq!(
            result.state().execution().power_toughness_counters(id),
            Some(PowerToughnessCounters {
                plus_one: 0.into(),
                minus_one: 2.into()
            })
        );
        assert!(occurrences(&result).any(|row|matches!(row,RulesOccurrence::CountersPlaced{object,added,..} if *object==id && added.minus_one==3.into())));
        let blocked = blocks(
            vec![
                combat_creature(1, A, 4, 4, &keywords),
                combat_creature(2, B, 1, 4, &[Indestructible]),
            ],
            &[(2, 1)],
        );
        assert_eq!(
            object(round(&blocked).state(), 2).zone(),
            ObjectZone::Graveyard(B)
        );
    }
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
fn poisonous_instances_use_the_stack_and_survive_the_sources_departure() {
    let mut seeds = vec![
        combat_creature(1, A, 2, 3, &[]),
        instant(3, A, Instruction::ReturnToOwnersHand),
    ];
    add(&mut seeds, 1, &[Poisonous(1.into()), Poisonous(2.into())]);
    let state = place(round(&blocks(seeds, &[])).into_state());
    assert_eq!(state.execution().window().entries().len(), 2);
    assert_eq!(state.execution().life_total(B), Some(&18.into()));
    assert_eq!(state.execution().poison_counters(B), 0.into());
    assert!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|entry| entry.targets().targets().is_empty())
    );
    let bounced = cast_on(&state, 3, 1);
    assert_eq!(object(&bounced, 1).zone(), ObjectZone::Hand(A));
    let first = round(&bounced);
    let partial = first.state().execution().poison_counters(B);
    assert!(partial == 1.into() || partial == 2.into());
    let second = round(first.state());
    assert_eq!(second.state().execution().poison_counters(B), 3.into());
    for resolution in [&first, &second] {
        assert!(occurrences(resolution).any(|row|matches!(row,RulesOccurrence::PlayerPoisoned{contributions,..} if matches!(contributions.as_slice(),[c] if matches!(c.origin,PoisonOrigin::Poisonous(_))))));
    }
}

#[test]
fn protected_player_receives_only_unprevented_infect_and_one_toxic_amount_on_a_hit() {
    for (shield, remaining, poison, entries) in [(2, 1, 3, 1), (3, 0, 0, 0)] {
        let mut source = combat_creature(1, A, 3, 3, &[Infect, Lifelink]);
        source.program.printed_toxic = Some(2.into());
        let mut protection = instant(
            3,
            A,
            Instruction::CreatePreventionShield {
                amount: shield.into(),
            },
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
fn departed_infect_source_uses_last_known_granted_keyword_for_noncombat_damage() {
    let mut source = battlefield(1, A);
    source.program = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Sacrifice,
            body: ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::Damage { amount: 3.into() }],
            ),
        }],
    );
    let mut grant = instant(
        2,
        A,
        change(AbilityChange {
            granted_keywords: [Infect].into(),
            granted_toxic: Some(5.into()),
            ..Default::default()
        }),
    );
    let spellwake_engine::program::ProgramKind::Spell(spell) = &mut grant.program.kind else {
        unreachable!()
    };
    spell.body.target = BodyTarget::SinglePermanent;
    let game = setup(vec![A, B], vec![source, grant], Step::PrecombatMain).into_state();
    let granted = cast_on(&game, 2, 1);
    let activation = act(
        &granted,
        A,
        GameCommand::ActivateAbility {
            source: object(&granted, 1).id(),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(B),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        object(activation.state(), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    let hit = round(activation.state());
    assert_eq!(hit.state().execution().poison_counters(B), 3.into());
    assert_eq!(hit.state().execution().life_total(B), Some(&20.into()));
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
