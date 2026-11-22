//! Complete creature faces whose activated abilities put counters on their source.
use super::*;
use spellwake_engine::activated::{ActivationRestriction, SourceCost};

pub(crate) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let type_line = face.type_line.as_deref()?.strip_prefix("Creature — ")?;
    let subtypes = type_line
        .split_whitespace()
        .map(|label| vocabulary.resolve_label(label))
        .collect::<Option<BTreeSet<_>>>()?;
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let rows = text
        .lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    let (ability, others) = rows.split_last()?;
    let (cost, physical, restriction, amount) = if let Some(cost) = ability.strip_prefix("Outlast ")
    {
        (
            fixed_cost(cost)?,
            SourceCost::Tap,
            ActivationRestriction::Sorcery,
            1,
        )
    } else {
        let (cost, effect) = ability.strip_prefix("Exhaust — ")?.split_once(": ")?;
        let number = match effect {
            "Put a +1/+1 counter on this creature." => 1,
            "Put two +1/+1 counters on this creature." => 2,
            "Put three +1/+1 counters on this creature." => 3,
            _ => return None,
        };
        let (cost, physical) = cost
            .strip_suffix(", {T}")
            .map_or((cost, SourceCost::None), |cost| (cost, SourceCost::Tap));
        (
            fixed_cost(cost)?,
            physical,
            ActivationRestriction::Exhaust,
            number,
        )
    };
    let mut program = Program::creature(
        Some(fixed_cost(face.mana_cost.as_deref()?)?),
        number(face.power.as_deref())?.into(),
        number(face.toughness.as_deref())?.into(),
        subtypes,
    );
    for row in others {
        for word in row.split(", ") {
            let key = match word.to_ascii_lowercase().as_str() {
                "reach" => KeywordAbility::Reach,
                "trample" => KeywordAbility::Trample,
                "flying" => KeywordAbility::Flying,
                "haste" => KeywordAbility::Haste,
                _ => return None,
            };
            program.printed_keywords.insert(key);
        }
    }
    let id = ActivatedAbilityId(1);
    program.activated.push(ActivatedAbility {
        id,
        mana_cost: cost,
        source_cost: physical,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::PutCountersOnSource {
                amount: amount.into(),
            }],
        ),
    });
    program.activation_restrictions.insert(id, restriction);
    Some(program)
}
