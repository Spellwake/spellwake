#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! Source prevention retains source identity and never becomes recipient protection.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    combat::AttackDeclaration,
    damage::TurnPreventionFilter,
    game::{DecisionReply, GameCommand, GameState, NativeCandidate},
    mana::{ManaCost, ManaPayment},
    object_identity::CardId,
    program::{BodyTarget, ExecutableBody, Instruction, Program},
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;
fn pinger(sacrifice: bool) -> Program {
    let mut p = Program::creature(Some(ManaCost::free()), 3.into(), 6.into(), BTreeSet::new());
    p.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: if sacrifice {
            SourceCost::Sacrifice
        } else {
            SourceCost::None
        },
        body: ExecutableBody::new(
            BodyTarget::SingleAnyTarget,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(2.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    });
    p
}
fn protect(game: &GameState, target: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, 10),
            target: TargetRef::Object(object(game, target)),
            payment: ManaPayment::default(),
        },
    );
    pass_pair(cast.state())
}
fn ping(game: &GameState, source: u64, target: TargetRef) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::ActivateAbility {
            source: object(game, source),
            ability: ActivatedAbilityId(1),
            target,
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}
#[test]
fn source_prevention_is_not_recipient_protection_and_preserves_damage_kind() {
    for (filter, expected) in [
        (TurnPreventionFilter::FromTarget, 20),
        (TurnPreventionFilter::CombatFromTarget, 18),
    ] {
        let game = world(
            vocabulary(),
            vec![
                card(1, pinger(false), true),
                card(2, pinger(false), true),
                spell(
                    10,
                    BodyTarget::SingleCreature,
                    vec![Instruction::PreventDamageThisTurn(filter)],
                ),
            ],
            vec![CardId(1), CardId(2)],
        );
        let protected = protect(&game, 1);
        let damaged = ping(protected.state(), 1, TargetRef::Player(P1));
        assert_eq!(
            damaged.state().execution().life_total(P1),
            Some(&expected.into())
        );
        let recipient = ping(
            damaged.state(),
            2,
            TargetRef::Object(object(damaged.state(), 1)),
        );
        // The protected source still takes damage; another source still damages the player.
        let other = ping(recipient.state(), 2, TargetRef::Player(P1));
        assert_eq!(
            other.state().execution().life_total(P1),
            Some(&(expected - 2).into())
        );
        let events = all_occurrences(&recipient);
        assert!(events.iter().any(|event| matches!(
            event,
            spellwake_engine::occurrence::RulesOccurrence::DamageDealt { .. }
        )));
    }
}
#[test]
fn departed_source_still_matches_its_captured_prevention_identity() {
    let game = world(
        vocabulary(),
        vec![
            card(1, pinger(true), true),
            spell(
                10,
                BodyTarget::SingleCreature,
                vec![Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::FromTarget,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let protected = protect(&game, 1);
    let damaged = ping(protected.state(), 1, TargetRef::Player(P1));
    assert_eq!(damaged.state().execution().life_total(P1), Some(&20.into()));
}
#[test]
fn combat_prevention_stops_only_the_selected_attacker() {
    let game = world(
        vocabulary(),
        vec![
            card(1, pinger(false), true),
            card(2, pinger(false), true),
            controlled(3, P1, P1, pinger(false)),
            spell(
                10,
                BodyTarget::SingleCreature,
                vec![Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::CombatFromTarget,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let protected = protect(&game, 1);
    let begin = pass_pair(protected.state());
    let attack = pass_pair(begin.state());
    let declared = reply(
        attack.state(),
        P0,
        DecisionReply::DeclareAttackers(
            vec![1, 2]
                .into_iter()
                .map(|id| AttackDeclaration {
                    attacker: object(attack.state(), id),
                    defender: P1,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    );
    let blockers = pass_pair(declared.state());
    let unblocked = reply(blockers.state(), P1, DecisionReply::DeclareBlockers(vec![]));
    let damage = pass_pair(unblocked.state());
    let ended = pass_pair(damage.state());
    assert_eq!(ended.state().execution().life_total(P1), Some(&17.into()));
}
#[test]
fn source_prevention_rejects_wrong_target_family_and_absent_ability_source() {
    let vocab = vocabulary();
    for filter in [
        TurnPreventionFilter::FromTarget,
        TurnPreventionFilter::CombatFromTarget,
        TurnPreventionFilter::CombatToOrFromTarget,
    ] {
        let bad = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::PreventDamageThisTurn(filter)],
            ),
        );
        assert!(bad.validate(&vocab).is_err());
    }
    for filter in [
        TurnPreventionFilter::FromAbilitySource,
        TurnPreventionFilter::CombatFromAbilitySource,
    ] {
        let bad = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PreventDamageThisTurn(filter)],
            ),
        );
        assert!(bad.validate(&vocab).is_err());
    }
}

#[test]
fn ability_source_prevention_survives_sacrifice_and_respects_combat_only() {
    for (filter, expected) in [
        (TurnPreventionFilter::FromAbilitySource, 20),
        (TurnPreventionFilter::CombatFromAbilitySource, 18),
    ] {
        let mut source = pinger(true);
        source.activated[0]
            .body
            .instructions
            .insert(0, Instruction::PreventDamageThisTurn(filter));
        let game = world(vocabulary(), vec![card(1, source, true)], vec![CardId(1)]);
        let damaged = ping(&game, 1, TargetRef::Player(P1));
        assert_eq!(
            damaged.state().execution().life_total(P1),
            Some(&expected.into())
        );
    }
}

#[test]
fn source_prevention_expires_at_cleanup() {
    let game = world(
        vocabulary(),
        vec![
            card(1, pinger(false), true),
            spell(
                10,
                BodyTarget::SingleCreature,
                vec![Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::FromTarget,
                )],
            ),
        ],
        vec![CardId(1)],
    );
    let protected = protect(&game, 1);
    let turn = protected.state().turn().unwrap().position().number;
    let begin = pass_pair(protected.state());
    let attack = pass_pair(begin.state());
    let mut next = reply(attack.state(), P0, DecisionReply::DeclareAttackers(vec![]));
    for _ in 0..10 {
        if next.state().turn().unwrap().position().number != turn {
            break;
        }
        next = pass_pair(next.state());
    }
    assert_ne!(next.state().turn().unwrap().position().number, turn);
    let priority = submit(next.state(), P1, GameCommand::Pass);
    let damaged = ping(priority.state(), 1, TargetRef::Player(P1));
    assert_eq!(damaged.state().execution().life_total(P1), Some(&18.into()));
}

#[test]
fn creature_source_groups_use_damage_kind_and_exclude_noncreature_sources() {
    for (filter, expected) in [
        (TurnPreventionFilter::Creatures, 20),
        (TurnPreventionFilter::CombatFromCreatures, 18),
    ] {
        let mut artifact = Program::artifact(None, vec![]);
        artifact.activated = pinger(false).activated;
        let game = world(
            vocabulary(),
            vec![
                card(1, pinger(true), true),
                card(2, artifact, true),
                spell(
                    10,
                    BodyTarget::None,
                    vec![Instruction::PreventDamageThisTurn(filter)],
                ),
            ],
            vec![CardId(1), CardId(2)],
        );
        let cast = submit(
            &game,
            P0,
            GameCommand::CastUntargeted {
                spell: object(&game, 10),
                payment: Some(ManaPayment::default()),
            },
        );
        let protected = pass_pair(cast.state());
        let creature = ping(protected.state(), 1, TargetRef::Player(P1));
        assert_eq!(
            creature.state().execution().life_total(P1),
            Some(&expected.into())
        );
        let artifact = ping(creature.state(), 2, TargetRef::Player(P1));
        assert_eq!(
            artifact.state().execution().life_total(P1),
            Some(&(expected - 2).into())
        );
    }
}
