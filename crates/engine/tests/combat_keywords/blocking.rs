use super::*;
use spellwake_engine::characteristics::KeywordAbility::{Flying, Menace, Reach};

#[test]
fn flying_menace_and_reach_survive_the_scheduled_attack_and_block_decisions() {
    let mut seeds = cards();
    for card in &mut seeds {
        match card.card {
            CardId(1) => card.program.printed_keywords.extend([Flying, Menace]),
            CardId(3) => {
                card.program.printed_keywords.insert(Reach);
            }
            CardId(4) => {
                card.program.printed_keywords.insert(Flying);
            }
            _ => {}
        }
    }
    seeds.push(battlefield(5, B));
    let game = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attacking = declared(&game, &[(1, B), (2, B)]);
    let blocking = round(attacking.state()).into_state();
    let pending = blocking.pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) = pending.frame() else {
        panic!("defender must receive the blocking choice");
    };
    let id = |n| object(&blocking, n).id();
    assert_eq!(frame.legal_attackers(id(5)), &[id(2)]);
    assert_eq!(frame.legal_attackers(id(3)), &[id(1), id(2)]);
    assert_eq!(frame.group_minimums().collect::<Vec<_>>(), vec![(id(1), 2)]);
    for rows in [vec![(3, 1)], vec![(5, 1), (4, 1)]] {
        let choice = DecisionReply::DeclareBlockers(
            rows.into_iter()
                .map(|(blocker, attacker)| BlockDeclaration {
                    blocker: id(blocker),
                    attacker: id(attacker),
                })
                .collect(),
        );
        reject(
            &blocking,
            input(
                &blocking,
                B,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice,
                },
            ),
            GameError::Effect(EffectError::Combat(CombatError::IllegalBlockTarget)),
        );
    }
    let accepted = blocked(&blocking, &[(3, 1), (4, 1), (5, 2)]);
    let combat = accepted.state().execution().combat().unwrap();
    assert_eq!(combat.blockers().count(), 3);
    assert_eq!(
        combat.attacker(id(1)).unwrap().block_status(),
        BlockStatus::Blocked
    );
    assert_eq!(
        combat.attacker(id(2)).unwrap().block_status(),
        BlockStatus::Blocked
    );
}

fn exalted_sources() -> Vec<SeedCard> {
    use spellwake_engine::{
        characteristics::{
            AbilityChange, ControllerSelector, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts,
        },
        program::{CardType, TriggeredAbility, TriggeredAbilityId},
    };
    let mut seeds = cards();
    let lone = seeds
        .iter_mut()
        .find(|card| card.card == CardId(1))
        .unwrap();
    lone.program
        .printed_keywords
        .insert(spellwake_engine::characteristics::KeywordAbility::Shroud);
    lone.program.triggered.extend([
        TriggeredAbility::exalted(TriggeredAbilityId(0)),
        TriggeredAbility::exalted(TriggeredAbilityId(1)),
    ]);
    let opposing = seeds
        .iter_mut()
        .find(|card| card.card == CardId(4))
        .unwrap();
    opposing
        .program
        .triggered
        .push(TriggeredAbility::exalted(TriggeredAbilityId(0)));
    for id in [20, 21] {
        if id == 21 {
            seeds.push(SeedCard {
                card: CardId(id),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::artifact(None, vec![]),
            });
        }
        let lord = seeds
            .iter_mut()
            .find(|card| card.card == CardId(id))
            .unwrap();
        lord.program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(0),
            selector: PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: std::collections::BTreeSet::from([CardType::Creature]),
                ..Default::default()
            },
            parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
                granted_exalted: 1,
                granted_combat_keywords: vec![],
                ..Default::default()
            })]),
        });
    }
    seeds
}

#[test]
fn exalted_stacks_independent_printed_and_granted_instances_without_targeting() {
    use spellwake_engine::occurrence::RulesRecord;
    let game = attackers(setup(vec![A, B], exalted_sources(), Step::Untap).state());
    let attacking = declared(&game, &[(1, B)]);
    // The attacker has two printed and two granted instances; the second
    // friendly creature has another two granted instances. The defender has none that trigger.
    let attacker = object(attacking.state(), 1).id();
    let actual = attacking
        .state()
        .execution()
        .current_characteristics(attacker)
        .unwrap()
        .unwrap();
    assert_eq!(actual.exalted_instances(), 4);
    let discovered: Vec<_> = attacking
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(ability) => Some(ability),
            _ => None,
        })
        .collect();
    assert_eq!(discovered.len(), 6);
    let identities: std::collections::BTreeSet<_> = discovered
        .iter()
        .map(|ability| {
            (
                ability.source.object.id(),
                ability.granting_effect,
                ability.definition,
            )
        })
        .collect();
    assert_eq!(identities.len(), 6);
    let RuleFrame::Triggers(order) = attacking.state().pending_decision().unwrap().frame() else {
        panic!("six simultaneous triggers require placement order")
    };
    let mut state = reply(
        attacking.state(),
        DecisionReply::OrderTriggers(order.instances().to_vec()),
    )
    .into_state();
    assert!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|entry| entry.targets().targets().is_empty())
    );
    for resolved in 1..=6 {
        state = round(&state).into_state();
        let creature = state.execution().creature(attacker).unwrap().unwrap();
        assert_eq!(creature.power, (2 + resolved).into());
        assert_eq!(creature.toughness, (4 + resolved).into());
    }
    assert_eq!(state.execution().continuous_effects().count(), 6);
    for _ in 0..12 {
        if state.execution().continuous_effects().next().is_none() {
            break;
        }
        state = if state.pending_decision().is_some() {
            blocked(&state, &[]).into_state()
        } else {
            round(&state).into_state()
        };
    }
    assert_eq!(state.execution().continuous_effects().count(), 0);
    assert_eq!(
        state.execution().creature(attacker).unwrap().unwrap().power,
        2.into()
    );
}

#[test]
fn exalted_checks_the_whole_attack_declaration() {
    let state = attackers(setup(vec![A, B], exalted_sources(), Step::Untap).state());
    for rows in [vec![], vec![(1, B), (2, B)]] {
        let attacking = declared(&state, &rows);
        assert!(
            !attacking
                .rules_journal()
                .records
                .iter()
                .any(|record| matches!(
                    record,
                    spellwake_engine::occurrence::RulesRecord::TriggerDiscovered(_)
                ))
        );
        assert!(attacking.state().execution().window().entries().is_empty());
    }
}

#[test]
fn exalted_uses_the_exact_attacker_and_survives_departure_of_the_source() {
    use spellwake_engine::program::{TriggeredAbility, TriggeredAbilityId};
    for remove in [1, 2] {
        let mut seeds = cards();
        seeds
            .iter_mut()
            .find(|card| card.card == CardId(2))
            .unwrap()
            .program
            .triggered
            .push(TriggeredAbility::exalted(TriggeredAbilityId(1)));
        let game = attackers(setup(vec![A, B], seeds, Step::Untap).state());
        let attacking = declared(&game, &[(1, B)]).into_state();
        let id = object(&attacking, 1).id();
        assert_eq!(attacking.execution().window().entries().len(), 1);
        let responding = act(
            &attacking,
            A,
            GameCommand::Cast {
                spell: object(&attacking, 30).id(),
                target: TargetRef::Object(object(&attacking, remove).id()),
            },
        );
        let bounced = round(responding.state());
        let done = round(bounced.state()).into_state();
        if remove == 1 {
            assert_ne!(object(&done, 1).id(), id);
            assert_eq!(done.execution().continuous_effects().count(), 0);
        } else {
            assert_eq!(
                done.execution().creature(id).unwrap().unwrap().power,
                3.into()
            );
            assert_eq!(done.execution().continuous_effects().count(), 1);
        }
    }
}

#[test]
fn a_later_ability_removal_suppresses_printed_and_granted_exalted() {
    use spellwake_engine::characteristics::{
        AbilityChange, AbilityRemoval, ControllerSelector, PermanentSelector, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    };
    use spellwake_engine::program::CardType;
    let mut seeds = exalted_sources();
    let mut suppressor = Program::artifact(None, vec![]);
    suppressor.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: std::collections::BTreeSet::from([CardType::Creature]),
            ..Default::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
            AbilityRemoval::All,
        ))]),
    });
    seeds.push(SeedCard {
        card: CardId(22),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: suppressor,
    });
    let game = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attacker = object(&game, 1).id();
    assert_eq!(
        game.execution()
            .current_characteristics(attacker)
            .unwrap()
            .unwrap()
            .exalted_instances(),
        0
    );
    let attacking = declared(&game, &[(1, B)]);
    assert!(attacking.state().execution().window().entries().is_empty());
}

#[test]
fn protection_checks_all_current_colors_of_each_would_be_blocker() {
    use spellwake_engine::characteristics::{Color, KeywordAbility};
    let mut seeds = cards();
    for card in &mut seeds {
        match card.card {
            CardId(1) => {
                card.program
                    .printed_keywords
                    .insert(KeywordAbility::ProtectionFromRed);
            }
            CardId(3) => {
                card.program.color_indicator = Some([Color::White, Color::Red].into());
            }
            CardId(4) => {
                card.program.color_indicator = Some([Color::Blue].into());
            }
            _ => {}
        }
    }
    let game = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let attacking = declared(&game, &[(1, B), (2, B)]);
    let blocking = round(attacking.state()).into_state();
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
        blocking.pending_decision().unwrap().frame()
    else {
        panic!("expected blockers")
    };
    let id = |n| object(&blocking, n).id();
    assert_eq!(frame.legal_attackers(id(3)), &[id(2)]);
    assert_eq!(frame.legal_attackers(id(4)), &[id(1), id(2)]);
    let accepted = blocked(&blocking, &[(3, 2), (4, 1)]);
    assert_eq!(
        accepted
            .state()
            .execution()
            .combat()
            .unwrap()
            .blockers()
            .count(),
        2
    );
}
