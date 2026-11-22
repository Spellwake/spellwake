//! Complete printed instruction lines exercised by cards with rebound.
use super::*;
use spellwake_engine::{
    characteristics::Color,
    program::{AffectedPlayers, ResolvingPlayer},
    token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype},
};

pub(super) fn body(text: &str, vocabulary: &CreatureTypeVocabulary) -> Option<ExecutableBody> {
    let (target, instructions) = match text {
        "Target creature gets +2/+2 until end of turn." => {
            (BodyTarget::SingleCreature, vec![pump(2, 2, None)])
        }
        "Target creature gets +2/+0 and gains deathtouch until end of turn." => (
            BodyTarget::SingleCreature,
            vec![pump(2, 0, Some(KeywordAbility::Deathtouch))],
        ),
        "Discard a card, then draw a card." => (
            BodyTarget::None,
            vec![
                Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
                draw(1),
            ],
        ),
        "Create a 3/3 green Elephant creature token." => (
            BodyTarget::None,
            vec![creature_token(
                vocabulary,
                &["Elephant"],
                3,
                Color::Green,
                None,
            )?],
        ),
        "Create a 2/2 blue Djinn Monk creature token with flying." => (
            BodyTarget::None,
            vec![creature_token(
                vocabulary,
                &["Djinn", "Monk"],
                2,
                Color::Blue,
                Some(KeywordAbility::Flying),
            )?],
        ),
        _ => return None,
    };
    Some(ExecutableBody::new(target, instructions))
}
pub(super) fn creature_token(
    vocabulary: &CreatureTypeVocabulary,
    labels: &[&str],
    size: i64,
    color: Color,
    ability: Option<KeywordAbility>,
) -> Option<Instruction> {
    let subtypes: Vec<_> = labels
        .iter()
        .map(|label| vocabulary.resolve_label(label))
        .collect::<Option<_>>()?;
    let mut token = Program::creature(
        None,
        size.into(),
        size.into(),
        subtypes.iter().copied().collect(),
    )
    .with_printed_keywords(ability);
    token.effect_defined_colors = Some(BTreeSet::from([color]));
    let name = TokenName::Default {
        ordered_subtypes: subtypes.into_iter().map(TokenSubtype::Creature).collect(),
    };
    let definition = TokenDefinition::new(token, name, vocabulary).ok()?;
    Some(Instruction::CreateTokens {
        creations: vec![TokenCreation::one(definition)],
    })
}
