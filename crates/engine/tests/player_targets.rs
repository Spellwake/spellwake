//! Authored player targets under the existing announcement/resolution/copy gates.
//! Player protection/hexproof, planeswalker/battle targets and multiplayer
//! elimination remain unqualified; this is not complete Lightning Bolt support.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use trajectories::*;

use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{CopyTargetChoice, EffectError},
    game::{DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate},
    mana::{ManaCost, ManaPayment},
    object_identity::CardId,
    priority::SeatId,
    program::{BodyTarget, ExecutableBody, Instruction, Program},
    targets::TargetRef,
};
use std::collections::BTreeSet;

fn damage() -> Instruction {
    spellwake_engine::program::Instruction::DealDamage(Box::new(
        spellwake_engine::toolbox::DealDamageSpec {
            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                spellwake_engine::program::ValueExpr::Literal(3.into()),
            ),
            exile_if_dies: false,
        },
    ))
}
fn cast_at(game: &GameState, id: u64, target: TargetRef) -> NativeCandidate {
    submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, id),
            target,
            payment: ManaPayment::default(),
        },
    )
}
fn damage_world() -> GameState {
    world(
        vocabulary(),
        vec![
            card(
                1,
                Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                true,
            ),
            spell(2, BodyTarget::SingleCreatureOrPlayer, vec![damage()]),
            spell(3, BodyTarget::SingleCreatureOrPlayer, vec![damage()]),
            card(4, Program::artifact(None, vec![]), true),
        ],
        vec![CardId(1), CardId(4)],
    )
}

#[test]
fn creature_or_player_damage_uses_the_same_processor_for_either_recipient() {
    let game = damage_world();
    let creature = object(&game, 1);
    let cast = cast_at(&game, 2, TargetRef::Player(P1));
    let player_damage = pass_pair(cast.state());
    assert_eq!(
        player_damage.state().execution().life_total(P1),
        Some(&17.into())
    );
    assert_eq!(player_damage.damage_journal().applied.len(), 1);
    let cast = cast_at(player_damage.state(), 3, TargetRef::Object(creature));
    let creature_damage = pass_pair(cast.state());
    assert_eq!(
        creature_damage
            .state()
            .execution()
            .battlefield_marked_damage(creature),
        Some(3.into())
    );
    assert_eq!(
        creature_damage.state().execution().life_total(P1),
        Some(&17.into())
    );
    assert_eq!(creature_damage.damage_journal().applied.len(), 1);
}

#[test]
fn invalid_player_or_noncreature_targets_reject_without_announcing_or_paying() {
    let game = damage_world();
    let saved = game.clone();
    for target in [
        TargetRef::Player(SeatId(99)),
        TargetRef::Object(object(&game, 4)),
        TargetRef::Object(object(&game, 3)),
    ] {
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis().clone(),
                actor: P0,
                command: GameCommand::CastPaid {
                    spell: object(&game, 2),
                    target,
                    payment: ManaPayment::default()
                }
            }),
            Err(GameError::Effect(EffectError::IllegalTarget))
        );
        assert_eq!(game, saved);
    }
}

#[test]
fn copied_player_target_spell_can_retarget_a_creature_independently() {
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                true,
            ),
            spell(2, BodyTarget::SingleCreatureOrPlayer, vec![damage()]),
            spell(
                3,
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
            ),
        ],
        vec![CardId(1)],
    );
    let creature = object(&game, 1);
    let original = cast_at(&game, 2, TargetRef::Player(P1));
    let entry = original.state().execution().window().entries()[0].id();
    let copy = cast_at(original.state(), 3, TargetRef::Stack(entry));
    let choosing = pass_pair(copy.state());
    let copied = reply(
        choosing.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(creature))),
    );
    let copy_damage = pass_pair(copied.state());
    assert_eq!(
        copy_damage
            .state()
            .execution()
            .battlefield_marked_damage(creature),
        Some(3.into())
    );
    assert_eq!(
        copy_damage.state().execution().life_total(P1),
        Some(&20.into())
    );
    assert_eq!(
        copy_damage.state().execution().window().entries()[0]
            .targets()
            .targets()[0]
            .choice()
            .target,
        TargetRef::Player(P1)
    );
    let original_damage = pass_pair(copy_damage.state());
    assert_eq!(
        original_damage.state().execution().life_total(P1),
        Some(&17.into())
    );
}

#[test]
fn targeted_activated_ability_can_deal_damage_to_a_player() {
    let source = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Tap,
            body: ExecutableBody::new(BodyTarget::SinglePlayer, vec![damage()]),
        }],
    );
    let game = world(vocabulary(), vec![card(1, source, true)], vec![CardId(1)]);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateAbility {
            source: object(&game, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(P1),
            payment: Some(ManaPayment::default()),
        },
    );
    let completed = pass_pair(activated.state());
    assert_eq!(
        completed.state().execution().life_total(P1),
        Some(&17.into())
    );
    assert_eq!(completed.damage_journal().applied.len(), 1);
}
