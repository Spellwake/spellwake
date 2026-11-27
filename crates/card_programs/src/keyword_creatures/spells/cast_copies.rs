//! Exact, complete effect lines on single-face spells with cast-copy triggers.
use super::*;
use spellwake_engine::{characteristics::Color, program::ResolvingPlayer};
pub(super) fn body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    let (target, instructions) = match text {
        "Target player mills three cards." => (
            BodyTarget::SINGLE_PLAYER,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::mill(
                    ResolvingPlayer::TargetPlayer,
                    spellwake_engine::program::ValueExpr::Literal(3.into()),
                ),
            ))],
        ),
        "You gain 3 life." => (
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(3),
                ),
            ],
        ),
        "Target player loses 2 life and you gain 2 life." => (
            BodyTarget::SINGLE_PLAYER,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    ),
                    spellwake_engine::program::ValueExpr::literal(2),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(2),
                ),
            ],
        ),
        "Target creature gets +1/+2 until end of turn." => {
            (BodyTarget::SINGLE_CREATURE, vec![pump(1, 2, None)])
        }
        "Create a 4/4 green Beast creature token." => (
            BodyTarget::None,
            vec![rebound::creature_token(
                vocabulary,
                &["Beast"],
                4,
                Color::Green,
                None,
            )?],
        ),
        "Create a 1/1 green Squirrel creature token." => (
            BodyTarget::None,
            vec![rebound::creature_token(
                vocabulary,
                &["Squirrel"],
                1,
                Color::Green,
                None,
            )?],
        ),
        "Create two 1/1 red Goblin creature tokens." => {
            let mut create = rebound::creature_token(vocabulary, &["Goblin"], 1, Color::Red, None)?;
            let Instruction::CreateTokens { creations } = &mut create else {
                return None;
            };
            creations.first_mut()?.amount = spellwake_engine::program::ValueExpr::Literal(2.into());
            (BodyTarget::None, vec![create])
        }
        source if source.strip_prefix(name) == Some(" deals 1 damage to target creature.") => (
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
            ],
        ),
        _ => return None,
    };
    Some(ExecutableBody::new(target, instructions))
}
