//! Fixed-price creature alternatives and their complete entered, left or death abilities.
use super::*;
use spellwake_engine::{
    characteristics::Color,
    program::{AffectedPlayers, EndStepCast, ResolvingPlayer, TriggerGrouping},
    token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype},
};
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let raw = face.oracle_text.as_deref()?;
    if !raw.lines().any(|line| price_line(line).is_some()) {
        return None;
    }
    let rules = without_reminder(raw)?;
    let mut cost = None;
    let mut base = Vec::new();
    let mut bodies = Vec::new();
    for line in rules.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some((mode, mana)) = price_line(line) {
            if cost.replace((mode, fixed_cost(mana)?)).is_some() {
                return None;
            }
        } else if let Some(effect) = line.strip_prefix("When you cast this spell, ") {
            bodies.push((
                spellwake_engine::program::TriggerSpec::self_cast(),
                effect_body(effect, vocabulary)?,
            ));
        } else if let Some(effect) = line.strip_prefix("When this creature enters, ") {
            bodies.push((
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect_body(effect, vocabulary)?,
            ));
        } else if let Some(effect) =
            line.strip_prefix("When this creature leaves the battlefield, ")
        {
            bodies.push((
                spellwake_engine::program::TriggerSpec::leaves_battlefield(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect_body(effect, vocabulary)?,
            ));
        } else if let Some(effect) = line.strip_prefix("When this creature dies, ") {
            bodies.push((
                spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                effect_body(effect, vocabulary)?,
            ));
        } else {
            base.push(line);
        }
    }
    let mut program =
        ordinary_creature(card, vocabulary, Profile::Current, Some(&base.join("\n"))).ok()?;
    if program.conditional_cost.is_some() {
        return None;
    }
    let mut next_id = program
        .triggered
        .iter()
        .map(|t| t.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)?;
    for (condition, body) in bodies {
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(next_id),
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body,
        });
        next_id = next_id.checked_add(1)?;
    }
    let (mode, price) = cost?;
    Some(match mode {
        CreaturePrice::Emerge(from) => {
            program.conditional_cost =
                Some(spellwake_engine::program::ConditionalCastCost::Emerge { mana: price, from });
            program
        }
        CreaturePrice::Evoke => program.with_evoke(price, TriggeredAbilityId(next_id)),
        CreaturePrice::EndStep(kind) => {
            program.with_end_step_cast(kind, price, TriggeredAbilityId(next_id))
        }
    })
}

enum CreaturePrice {
    Evoke,
    EndStep(EndStepCast),
    Emerge(CardType),
}
fn price_line(line: &str) -> Option<(CreaturePrice, &str)> {
    if let Some(mana) = line.strip_prefix("Emerge from artifact ") {
        return Some((CreaturePrice::Emerge(CardType::Artifact), mana));
    }
    let (keyword, cost) = line.split_once(' ')?;
    if !cost.starts_with('{') {
        return None;
    }
    let mode = match keyword {
        "Dash" => CreaturePrice::EndStep(EndStepCast::Dash),
        "Warp" => CreaturePrice::EndStep(EndStepCast::Warp),
        "Blitz" => CreaturePrice::EndStep(EndStepCast::Blitz),
        "Emerge" => CreaturePrice::Emerge(CardType::Creature),
        "Evoke" => CreaturePrice::Evoke,
        _ => return None,
    };
    Some((mode, cost))
}
fn effect_body(text: &str, vocab: &CreatureTypeVocabulary) -> Option<ExecutableBody> {
    let (target, op) = match text {
        "you gain 2 life." | "you gain 3 life." => (
            BodyTarget::None,
            Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(
                    if text == "you gain 2 life." {
                        2.into()
                    } else {
                        3.into()
                    },
                ),
            },
        ),
        "surveil 2." => (
            BodyTarget::None,
            Instruction::Surveil {
                player: ResolvingPlayer::Controller,
                amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
            },
        ),
        "put two +1/+1 counters on target creature." => (
            BodyTarget::SingleCreature,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::literal(2),
            ),
        ),
        "draw a card." => (
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        ),
        "draw two cards." => (
            BodyTarget::None,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::Literal(2.into()),
            ),
        ),
        "target player discards two cards." => (
            BodyTarget::SinglePlayer,
            Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
            },
        ),
        "it deals 6 damage to target creature." => (
            BodyTarget::SingleCreature,
            Instruction::Damage { amount: 6.into() },
        ),
        "create a 4/4 green Elemental creature token."
        | "create a 4/4 green Rhino Warrior creature token."
        | "create two 1/1 green Insect creature tokens."
        | "create a 1/1 white Human Soldier creature token." => {
            let (p, color, kinds): (i64, Color, &[&str]) = match text {
                "create a 4/4 green Elemental creature token." => (4, Color::Green, &["Elemental"]),
                "create a 4/4 green Rhino Warrior creature token." => {
                    (4, Color::Green, &["Rhino", "Warrior"])
                }
                "create two 1/1 green Insect creature tokens." => (1, Color::Green, &["Insect"]),
                _ => (1, Color::White, &["Human", "Soldier"]),
            };
            let types = kinds
                .iter()
                .map(|kind| vocab.resolve_label(kind))
                .collect::<Option<Vec<_>>>()?;
            let mut token =
                Program::creature(None, p.into(), p.into(), types.iter().copied().collect());
            token.color_indicator = Some([color].into());
            let def = TokenDefinition::new(
                token,
                TokenName::Default {
                    ordered_subtypes: types.into_iter().map(TokenSubtype::Creature).collect(),
                },
                vocab,
            )
            .ok()?;
            (
                BodyTarget::None,
                Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        amount: spellwake_engine::program::ValueExpr::Literal(
                            if text.starts_with("create two ") {
                                2.into()
                            } else {
                                1.into()
                            },
                        ),
                        ..TokenCreation::one(def)
                    }],
                },
            )
        }
        _ => return None,
    };
    Some(ExecutableBody::new(target, vec![op]))
}
