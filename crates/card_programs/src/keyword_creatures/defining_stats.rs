//! Printed star power and toughness defined by counting permanents.
use super::{InventoryCard, Profile, without_reminder};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    characteristics::{
        ControllerSelector, DefiningPart, DefiningParts, NumericExpression, PermanentSelector,
        PrintedStat, StaticAbilityId, StaticContinuousAbility,
    },
    program::{CardType, Program, ProgramKind},
};
use std::collections::BTreeSet;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let power = face.power.as_deref()? == "*";
    let toughness = face.toughness.as_deref()? == "*";
    if !power && !toughness {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut rules = Vec::new();
    let mut formula = None;
    for line in text.lines() {
        let predicate = line
            .strip_prefix("This creature's ")
            .or_else(|| line.strip_prefix(&format!("{}'s ", face.name)))
            .or_else(|| {
                face.name
                    .split_once(',')
                    .and_then(|(short, _)| line.strip_prefix(&format!("{short}'s ")))
            });
        if let Some(label) = predicate
            .and_then(|s| s.strip_prefix("power is equal to your devotion to "))
            .and_then(|s| s.trim_end().strip_suffix('.'))
        {
            use spellwake_engine::characteristics::{Color, StaticPlayerStat};
            let color = match label {
                "white" => Color::White,
                "blue" => Color::Blue,
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                _ => return None,
            };
            if !power
                || toughness
                || formula
                    .replace(NumericExpression::PlayerStat(StaticPlayerStat::Devotion(
                        color,
                    )))
                    .is_some()
            {
                return None;
            }
            continue;
        }
        if predicate
            == Some(
                "power and toughness are each equal to half the highest life total among your opponents, rounded up.",
            )
        {
            if !power || !toughness || formula.replace(NumericExpression::PlayerStat(spellwake_engine::characteristics::StaticPlayerStat::HalfHighestOpponentLife)).is_some() {
                return None;
            }
            continue;
        }
        let counted_zone = match predicate {
            Some(
                "power and toughness are each equal to the number of creature cards in your graveyard.",
            ) => Some(NumericExpression::ZoneCount {
                zone: spellwake_engine::characteristics::StaticZone::OwnGraveyard,
                types: spellwake_engine::program::CardTypeMask::CREATURE,
            }),
            Some("power and toughness are each equal to the number of cards in your hand.") => {
                Some(NumericExpression::ZoneCount {
                    zone: spellwake_engine::characteristics::StaticZone::OwnHand,
                    types: spellwake_engine::program::CardTypeMask::EMPTY,
                })
            }
            _ => None,
        };
        if let Some(expression) = counted_zone {
            if !power || !toughness || formula.replace(expression).is_some() {
                return None;
            }
            continue;
        }
        let operand = match (power, toughness) {
            (true, true) => "power and toughness are each equal to the number of ",
            (true, false) => "power is equal to the number of ",
            _ => "toughness is equal to the number of ",
        };
        let Some(subject) = predicate
            .and_then(|s| s.strip_prefix(operand))
            .and_then(|s| s.strip_suffix(" you control."))
        else {
            rules.push(line);
            continue;
        };
        let kind = match subject {
            "lands" => CardType::Land,
            "artifacts" => CardType::Artifact,
            "creatures" => CardType::Creature,
            "enchantments" => CardType::Enchantment,
            "planeswalkers" => CardType::Planeswalker,
            _ => return None,
        };
        if formula
            .replace(NumericExpression::count(PermanentSelector {
                required_types: BTreeSet::from([kind]),
                controller: ControllerSelector::SourceController,
                ..Default::default()
            }))
            .is_some()
        {
            return None;
        }
    }
    let formula = formula?;
    let mut reduced = card.clone();
    let reduced_face = &mut reduced.faces[0];
    if power {
        reduced_face.power = Some("0".into());
    }
    if toughness {
        reduced_face.toughness = Some("0".into());
    }
    reduced_face.oracle_text = Some(rules.join("\n"));
    let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
    let ProgramKind::Permanent(permanent) = &mut program.kind else {
        return None;
    };
    let pt = permanent.printed_pt.as_mut()?;
    let star = PrintedStat::Star {
        multiplier: 1.into(),
        offset: 0.into(),
    };
    if power {
        pt.power = star.clone();
    }
    if toughness {
        pt.toughness = star;
    }
    let next_id = program
        .continuous
        .iter()
        .chain(&program.additional_battlefield_abilities)
        .map(|ability| ability.id().0)
        .max()
        .unwrap_or(0)
        .checked_add(1)?;
    if let Some(existing) = program.continuous.take() {
        if !matches!(existing, StaticContinuousAbility::Battlefield { .. }) {
            return None;
        }
        program.additional_battlefield_abilities.push(existing);
    }
    program.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(next_id),
        parts: DefiningParts(vec![DefiningPart::PowerToughness {
            power: power.then(|| Box::new(formula.clone())),
            toughness: toughness.then(|| Box::new(formula)),
        }]),
    });
    Some(program)
}

/// Continuous source power and toughness modifiers based on a controlled permanent count.
pub(super) fn count_based_modifier(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{NumericPair, StaticPart, StaticParts};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut remainder = Vec::new();
    let mut modifier = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((subject, clause)) = line.split_once(" gets ") else {
            remainder.push(line);
            continue;
        };
        if subject != "This creature" && subject != face.name {
            remainder.push(line);
            continue;
        }
        let Some((stats, kind)) = clause
            .split_once(" for each ")
            .and_then(|(stats, rest)| rest.strip_suffix(" you control.").map(|kind| (stats, kind)))
        else {
            remainder.push(line);
            continue;
        };
        let kind = match kind {
            "artifact" => CardType::Artifact,
            "creature" => CardType::Creature,
            "enchantment" => CardType::Enchantment,
            "land" => CardType::Land,
            "planeswalker" => CardType::Planeswalker,
            _ => return None,
        };
        let (power, toughness) = stats.split_once('/')?;
        let expression = |coefficient: &str| -> Option<NumericExpression> {
            let coefficient = coefficient.parse::<i64>().ok()?;
            Some(if coefficient == 0 {
                NumericExpression::Literal(0.into())
            } else {
                NumericExpression::Count {
                    selector: PermanentSelector {
                        required_types: [kind].into(),
                        controller: ControllerSelector::SourceController,
                        ..PermanentSelector::default()
                    },
                    multiplier: coefficient.into(),
                    offset: 0.into(),
                }
            })
        };
        if modifier
            .replace(NumericPair {
                power: expression(power)?,
                toughness: expression(toughness)?,
            })
            .is_some()
        {
            return None;
        }
    }
    let modifier = modifier?;
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(remainder.join("\n"));
    let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
    let id = program
        .continuous
        .iter()
        .chain(&program.additional_battlefield_abilities)
        .map(|ability| ability.id().0)
        .max()
        .map_or(Some(0), |id| id.checked_add(1))?;
    let ability = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(id),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ModifyPT(modifier)]),
    };
    if program.continuous.is_none() {
        program.continuous = Some(ability);
    } else {
        program.additional_battlefield_abilities.push(ability);
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}

/// A source's conditional keywords gated by its actual +1/+1 counters.
pub(super) fn counter_threshold_keywords(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{AbilityChange, StaticPart, StaticParts};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let oracle = without_reminder(face.oracle_text.as_deref()?)?;
    let short_name = face.name.split_once(',').map(|(short, _)| short);
    let mut remainder = Vec::new();
    let mut ability = None;
    for line in oracle
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((_, modifier)) = line.split_once(" has ").filter(|(subject, _)| {
            *subject == "This creature" || *subject == face.name || Some(*subject) == short_name
        }) else {
            remainder.push(line);
            continue;
        };
        let Some((keyword, threshold)) =
            modifier
                .split_once(" as long as it has ")
                .and_then(|(keyword, rest)| {
                    rest.strip_suffix(" or more +1/+1 counters on it.")
                        .map(|threshold| (keyword, threshold))
                })
        else {
            remainder.push(line);
            continue;
        };
        let minimum = u16::try_from(super::ordinary_text::amount(threshold)?).ok()?;
        let keyword = super::static_keyword(keyword)?;
        if ability.replace((minimum, keyword)).is_some() {
            return None;
        }
    }
    let (minimum, keyword) = ability?;
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(remainder.join("\n"));
    let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
    let id = program
        .continuous
        .iter()
        .chain(&program.additional_battlefield_abilities)
        .map(|ability| ability.id().0)
        .max()
        .map_or(Some(0), |id| id.checked_add(1))?;
    let rule = StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(id),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceItself,
            minimum_plus_one_counters: minimum,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [keyword],
        ))]),
    };
    if program.continuous.is_none() {
        program.continuous = Some(rule);
    } else {
        program.additional_battlefield_abilities.push(rule);
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}
