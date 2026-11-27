//! Ordinary CR 119.3/119.5 adjustments in the existing ordered resolution loop.
//! Replacement/prohibition, team-life and life-payment mechanics remain excluded.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    combat::AttackDeclaration,
    draw::GameOutcome,
    game::{DecisionReply, GameCommand, GameState, NativeCandidate},
    life::{LifeAdjustment, LifeChange, LifeChangeKind},
    mana::ManaPayment,
    object_identity::CardId,
    occurrence::{RulesOccurrence, RulesRecord, TriggerRelated},
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, Program, TriggeredAbility,
        TriggeredAbilityId,
    },
};

fn adjust(adjustment: LifeAdjustment) -> Instruction {
    match adjustment {
        spellwake_engine::life::LifeAdjustment::Gain(n) => {
            spellwake_engine::program::Instruction::AdjustLife(Box::new(
                spellwake_engine::toolbox::AdjustLifeSpec::gain(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(n),
                ),
            ))
        }
        spellwake_engine::life::LifeAdjustment::Lose(n) => {
            spellwake_engine::program::Instruction::AdjustLife(Box::new(
                spellwake_engine::toolbox::AdjustLifeSpec::lose(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(n),
                ),
            ))
        }
        spellwake_engine::life::LifeAdjustment::Set(n) => {
            spellwake_engine::program::Instruction::AdjustLife(Box::new(
                spellwake_engine::toolbox::AdjustLifeSpec::set(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(n),
                ),
            ))
        }
    }
}

#[test]
fn simultaneous_unblocked_damage_creates_one_loss_event_per_player() {
    let mut witness = observer(LifeChangeKind::Loss);
    witness.triggered[0].condition = spellwake_engine::program::TriggerSpec::life_changed(
        LifeChangeKind::Loss,
        PlayerRelation::Opponent,
    );
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                Program::creature(None, 2.into(), 4.into(), std::collections::BTreeSet::new()),
                true,
            ),
            card(
                2,
                Program::creature(None, 3.into(), 4.into(), std::collections::BTreeSet::new()),
                true,
            ),
            card(3, witness, true),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let beginning = pass_pair(&game);
    let attackers = pass_pair(beginning.state());
    let declared = reply(
        attackers.state(),
        P0,
        DecisionReply::DeclareAttackers(vec![
            AttackDeclaration {
                attacker: object(attackers.state(), 1),
                defender: P1,
                defending_planeswalker: None,
            },
            AttackDeclaration {
                attacker: object(attackers.state(), 2),
                defender: P1,
                defending_planeswalker: None,
            },
        ]),
    );
    let blockers = pass_pair(declared.state());
    // This defender has no creatures, so the canonical blocker work completes
    // without issuing a choice. Advance its real priority boundary.
    assert!(blockers.state().pending_decision().is_none());
    let damage = pass_pair(blockers.state());
    assert_eq!(damage.state().execution().life_total(P1), Some(&15.into()));
    assert_eq!(
        changes(&damage),
        vec![LifeChange {
            lifelink: None,
            player: P1,
            kind: LifeChangeKind::Loss,
            amount: 5.into(),
            before: 20.into(),
            after: 15.into()
        }]
    );
    assert_eq!(
        damage
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
            .count(),
        1
    );
    assert_eq!(damage.state().execution().window().entries().len(), 1);
    let settled = pass_pair(damage.state());
    assert!(settled.state().execution().window().entries().is_empty());
    assert_eq!(settled.state().execution().life_total(P1), Some(&15.into()));
}

#[test]
fn direct_life_loss_is_not_damage_and_does_not_consume_a_prevention_shield() {
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                Program::creature(None, 3.into(), 3.into(), std::collections::BTreeSet::new()),
                true,
            ),
            spell(
                2,
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec {
                        selector: None,
                        payload:
                            spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
                                amount: 7.into(),
                            },
                        duration:
                            spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                        condition: None,
                        one_shot: true,
                        as_emblem: None,
                        emblem_label: None,
                    },
                ))],
            ),
            spell(
                3,
                BodyTarget::None,
                vec![adjust(LifeAdjustment::Lose(5.into()))],
            ),
        ],
        vec![CardId(1)],
    );
    let (shielded, _) = resolve(&game, 2, object(&game, 1));
    let completed = resolve_life(shielded.state(), 3);
    assert_eq!(
        completed.state().execution().life_total(P0),
        Some(&15.into())
    );
    assert_eq!(
        completed
            .state()
            .execution()
            .prevention_shields()
            .next()
            .unwrap()
            .remaining,
        7.into()
    );
    assert!(completed.damage_journal().attempts.is_empty());
    assert!(completed.damage_journal().applied.is_empty());
}
fn resolve_life(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}
fn changes(candidate: &NativeCandidate) -> Vec<LifeChange> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|occurrence| match &occurrence.value {
            RulesOccurrence::LifeChanged(change) => Some(change.clone()),
            _ => None,
        })
        .collect()
}
fn observer(kind: LifeChangeKind) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::life_changed(
            kind,
            PlayerRelation::Controller,
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::DrawCards(Box::new(
                spellwake_engine::toolbox::DrawCardsSpec::simple(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(0.into()),
                ),
            ))],
        ),
    });
    program
}

#[test]
fn life_can_fall_below_zero_and_recover_before_resolution_finishes() {
    let game = world(
        vocabulary(),
        vec![spell(
            1,
            BodyTarget::None,
            vec![
                adjust(LifeAdjustment::Lose(30.into())),
                adjust(LifeAdjustment::Gain(25.into())),
            ],
        )],
        vec![],
    );
    let saved = game.clone();
    let completed = resolve_life(&game, 1);
    assert_eq!(game, saved);
    assert_eq!(
        completed.state().execution().life_total(P0),
        Some(&15.into())
    );
    assert_eq!(completed.state().outcome(), None);
    assert_eq!(
        changes(&completed),
        vec![
            LifeChange {
                lifelink: None,
                player: P0,
                kind: LifeChangeKind::Loss,
                amount: 30.into(),
                before: 20.into(),
                after: (-10).into()
            },
            LifeChange {
                lifelink: None,
                player: P0,
                kind: LifeChangeKind::Gain,
                amount: 25.into(),
                before: (-10).into(),
                after: 15.into()
            },
        ]
    );
    assert!(
        completed
            .state()
            .execution()
            .state_based_losses()
            .is_empty()
    );
}

#[test]
fn setting_life_records_a_gain_or_loss_and_zero_changes_create_no_event() {
    let game = world(
        vocabulary(),
        vec![spell(
            1,
            BodyTarget::None,
            vec![
                adjust(LifeAdjustment::Gain(0.into())),
                adjust(LifeAdjustment::Lose(0.into())),
                adjust(LifeAdjustment::Set(20.into())),
                adjust(LifeAdjustment::Set(28.into())),
                adjust(LifeAdjustment::Set(17.into())),
                adjust(LifeAdjustment::Set(17.into())),
            ],
        )],
        vec![],
    );
    let completed = resolve_life(&game, 1);
    let changes = changes(&completed);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].kind, LifeChangeKind::Gain);
    assert_eq!(changes[0].amount, 8.into());
    assert_eq!(changes[1].kind, LifeChangeKind::Loss);
    assert_eq!(changes[1].amount, 11.into());
    assert_eq!(
        completed.state().execution().life_total(P0),
        Some(&17.into())
    );
}

#[test]
fn nonpositive_life_at_the_end_of_resolution_uses_shared_state_based_loss() {
    for adjustment in [
        LifeAdjustment::Lose(20.into()),
        LifeAdjustment::Set((-7).into()),
    ] {
        let game = world(
            vocabulary(),
            vec![spell(1, BodyTarget::None, vec![adjust(adjustment)])],
            vec![],
        );
        let completed = resolve_life(&game, 1);
        assert_eq!(completed.state().outcome(), Some(GameOutcome::Winner(P1)));
        assert_eq!(
            completed.state().execution().state_based_losses(),
            &std::collections::BTreeSet::from([P0])
        );
    }
}

#[test]
fn actual_gain_and_loss_events_capture_one_matching_trigger_each() {
    for (kind, adjustment) in [
        (LifeChangeKind::Gain, LifeAdjustment::Gain(5.into())),
        (LifeChangeKind::Gain, LifeAdjustment::Set(25.into())),
        (LifeChangeKind::Loss, LifeAdjustment::Lose(5.into())),
        (LifeChangeKind::Loss, LifeAdjustment::Set(15.into())),
    ] {
        let game = world(
            vocabulary(),
            vec![
                card(1, observer(kind), true),
                spell(2, BodyTarget::None, vec![adjust(adjustment)]),
            ],
            vec![CardId(1)],
        );
        let completed = resolve_life(&game, 2);
        let events = changes(&completed);
        let discovered: Vec<_> = completed
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::TriggerDiscovered(waiting) => Some(waiting),
                _ => None,
            })
            .collect();
        assert_eq!(discovered.len(), 1);
        assert_eq!(
            discovered[0].causes.members()[0].related,
            TriggerRelated::LifeChanged(events[0].clone())
        );
        assert_eq!(completed.state().execution().window().entries().len(), 1);
        let settled = pass_pair(completed.state());
        assert!(settled.state().execution().window().entries().is_empty());
        assert!(changes(&settled).is_empty());
    }
}

#[test]
fn zero_gain_and_unchanged_set_do_not_discover_life_triggers() {
    let game = world(
        vocabulary(),
        vec![
            card(1, observer(LifeChangeKind::Gain), true),
            spell(
                2,
                BodyTarget::None,
                vec![
                    adjust(LifeAdjustment::Gain(0.into())),
                    adjust(LifeAdjustment::Set(20.into())),
                ],
            ),
        ],
        vec![CardId(1)],
    );
    let completed = resolve_life(&game, 2);
    assert!(changes(&completed).is_empty());
    assert!(completed.state().execution().window().entries().is_empty());
    assert!(
        !completed
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::TriggerDiscovered(_)))
    );
}
