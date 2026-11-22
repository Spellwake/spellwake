//! Compile complete Oracle faces with soulbond and one continuous reward.
use super::*;
use spellwake_engine::{
    characteristics::{
        AbilityChange, ControllerSelector, NumericExpression, NumericPair, PermanentSelector,
        StaticPart, StaticParts,
    },
    program::{BodyTarget, ExecutableBody, Instruction, TriggerGrouping},
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let clean = without_reminder(face.oracle_text.as_deref()?)?;
    let mut found = false;
    let mut reward = None;
    let mut remaining = Vec::new();
    for line in clean.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some((source_name, rest)) = line
            .strip_prefix("As long as ")
            .and_then(|text| text.split_once(" is paired with another creature, "))
            && (source_name == "this creature" || source_name == face.name)
        {
            if reward.is_some() {
                return None;
            }
            reward = Some(paired_reward(rest, vocabulary)?);
            continue;
        }
        let mut other_keywords = Vec::new();
        for term in line.split(',').map(str::trim) {
            if term.eq_ignore_ascii_case("soulbond") {
                if found {
                    return None;
                }
                found = true;
            } else {
                other_keywords.push(term);
            }
        }
        if !other_keywords.is_empty() {
            remaining.push(other_keywords.join(", "));
        }
    }
    if !found {
        return None;
    }
    let mut program = ordinary_creature(
        card,
        vocabulary,
        Profile::Current,
        Some(&remaining.join("\n")),
    )
    .ok()?;
    let first = program
        .triggered
        .iter()
        .map(|trigger| trigger.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)?;
    for (entrant_only, id) in [(false, first), (true, first.checked_add(1)?)] {
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: spellwake_engine::program::TriggerSpec::keyword(
                spellwake_engine::program::TriggerKeywordEvent::Soulbond { entrant_only },
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PairWithCreature { entrant_only }],
            ),
        });
    }
    if let Some(part) = reward {
        super::army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                controller: ControllerSelector::SourcePair,
                ..Default::default()
            },
            StaticParts(vec![part]),
        )?;
    }
    Some(program)
}

fn paired_reward(text: &str, vocabulary: &CreatureTypeVocabulary) -> Option<StaticPart> {
    if let Some(quoted) = text
        .strip_prefix("each of those creatures has \"")
        .and_then(|tail| tail.strip_suffix('"'))
    {
        return super::army_grants::granted_creature_ability(quoted, vocabulary);
    }
    if let Some(keyword) = text
        .strip_prefix("both creatures have ")
        .and_then(|tail| tail.strip_suffix('.'))
    {
        if keyword == "protection from Zombies" {
            return Some(StaticPart::GrantSubtypeProtection {
                types: spellwake_engine::characteristics::StaticSubtypeOperand::Literal(
                    [vocabulary.resolve_label("Zombie")?].into(),
                ),
                creatures_only: false,
            });
        }
        return Some(StaticPart::Abilities(AbilityChange {
            granted_keywords: BTreeSet::from([static_keyword(keyword)?]),
            ..Default::default()
        }));
    }
    let (power, toughness) = text
        .strip_prefix("each of those creatures gets ")?
        .strip_suffix('.')?
        .split_once('/')?;
    Some(StaticPart::ModifyPT(NumericPair {
        power: NumericExpression::Literal(power.parse::<i64>().ok()?.into()),
        toughness: NumericExpression::Literal(toughness.parse::<i64>().ok()?.into()),
    }))
}
