//! Complete single-face creature rules with Start your engines! and self-only max-speed grants.
use super::*;
use spellwake_engine::characteristics::{
    AbilityChange, ControllerSelector, NumericExpression, NumericPair, PermanentSelector,
    StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
};
pub(super) fn compile(card: &InventoryCard, vocab: &CreatureTypeVocabulary) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let raw = face.oracle_text.as_deref()?;
    if !raw
        .lines()
        .any(|l| l.starts_with("Start your engines!") || l.starts_with("Max speed — "))
    {
        return None;
    }
    let text = without_reminder(raw)?;
    let mut starts = false;
    let mut max = None;
    let mut normal = vec![];
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if line == "Start your engines!" {
            starts = true;
        } else if let Some(grant) = line.strip_prefix("Max speed — This creature ") {
            let mut parts = vec![];
            let (pt, abilities) = match grant {
                "has lifelink." => (None, vec![KeywordAbility::Lifelink]),
                "gets +1/+2." => (Some((1_i64, 2_i64)), vec![]),
                "gets +1/+0." => (Some((1, 0)), vec![]),
                "gets +0/+1 and has vigilance." => (Some((0, 1)), vec![KeywordAbility::Vigilance]),
                "has deathtouch and haste." => (
                    None,
                    vec![KeywordAbility::Deathtouch, KeywordAbility::Haste],
                ),
                "has double strike." => (None, vec![KeywordAbility::DoubleStrike]),
                _ => return None,
            };
            if !abilities.is_empty() {
                parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(
                    abilities,
                )));
            }
            if let Some((power, toughness)) = pt {
                parts.push(StaticPart::ModifyPT(NumericPair {
                    power: NumericExpression::Literal(power.into()),
                    toughness: NumericExpression::Literal(toughness.into()),
                }));
            }
            if max.replace(StaticParts(parts)).is_some() {
                return None;
            }
        } else {
            normal.push(line);
        }
    }
    let mut program =
        ordinary_creature(card, vocab, Profile::Current, Some(&normal.join("\n"))).ok()?;
    if starts {
        program
            .printed_keywords
            .insert(KeywordAbility::StartYourEngines);
    }
    if let Some(parts) = max
        && program
            .continuous
            .replace(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: PermanentSelector {
                    controller: ControllerSelector::SelfAtMaxSpeed,
                    ..Default::default()
                },
                parts,
            })
            .is_some()
    {
        return None;
    }
    program.validate(vocab).ok()?;
    Some(program)
}
