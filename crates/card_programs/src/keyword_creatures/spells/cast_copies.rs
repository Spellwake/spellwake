//! Exact, complete effect lines on single-face spells with cast-copy triggers.
use super::*;
use spellwake_engine::{characteristics::Color, life::LifeAdjustment, program::ResolvingPlayer};
pub(super) fn body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    let (target, instructions) = match text {
        "Search target player's library for a card and exile it. Then that player shuffles." => (
            BodyTarget::SinglePlayer,
            vec![Instruction::ExileFromTargetLibrary],
        ),
        "Target player mills three cards." => (
            BodyTarget::SinglePlayer,
            vec![Instruction::Mill {
                player: ResolvingPlayer::TargetPlayer,
                amount: spellwake_engine::program::ValueExpr::Literal(3.into()),
            }],
        ),
        "You gain 3 life." => (
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            }],
        ),
        "Target player loses 2 life and you gain 2 life." => (
            BodyTarget::SinglePlayer,
            vec![
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Lose(2.into()),
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                },
            ],
        ),
        "Target creature gets +1/+2 until end of turn." => {
            (BodyTarget::SingleCreature, vec![pump(1, 2, None)])
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
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
        _ => return None,
    };
    Some(ExecutableBody::new(target, instructions))
}
