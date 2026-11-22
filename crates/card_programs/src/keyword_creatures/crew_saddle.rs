//! Complete, normal-face Vehicle and Mount rules boxes in the later keyword profile.
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, PowerAbility},
    characteristics::PowerToughness,
    program::{ProgramKind, TriggerGrouping},
};
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
    let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
    let pt = PowerToughness {
        power: number(face.power.as_deref())?.into(),
        toughness: number(face.toughness.as_deref())?.into(),
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let rows = text
        .lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    let (last, other) = rows.split_last()?;
    if let Some(n) = last.strip_prefix("Crew ") {
        let mut program = Program::artifact(
            cost,
            vec![ActivatedAbility::tap_for_power(
                ActivatedAbilityId(1),
                PowerAbility::Crew,
                n.parse().ok()?,
            )],
        );
        match face.type_line.as_deref()? {
            "Artifact — Vehicle" => {}
            "Legendary Artifact — Vehicle" => {
                program.supertypes.insert(Supertype::Legendary);
            }
            _ => return None,
        }
        for row in other {
            for word in row.split(", ") {
                let key = match word.to_ascii_lowercase().as_str() {
                    "flying" => KeywordAbility::Flying,
                    "menace" => KeywordAbility::Menace,
                    "trample" => KeywordAbility::Trample,
                    "vigilance" => KeywordAbility::Vigilance,
                    "haste" => KeywordAbility::Haste,
                    "first strike" => KeywordAbility::FirstStrike,
                    "double strike" => KeywordAbility::DoubleStrike,
                    _ => return None,
                };
                program.printed_keywords.insert(key);
            }
        }
        let ProgramKind::Permanent(def) = &mut program.kind else {
            return None;
        };
        def.printed_pt = Some(pt.into());
        return Some(program);
    }
    let n = last.strip_prefix("Saddle ")?.parse().ok()?;
    if other
        != [
            "Whenever this creature attacks while saddled, it gets +1/+2 and gains menace until end of turn.",
        ]
    {
        return None;
    }
    let words = face.type_line.as_deref()?.strip_prefix("Creature — ")?;
    let subtypes = words
        .split_whitespace()
        .map(|word| vocabulary.resolve_label(word))
        .collect::<Option<BTreeSet<_>>>()?;
    let mut program = Program::creature(cost, pt.power, pt.toughness, subtypes);
    program.activated.push(ActivatedAbility::tap_for_power(
        ActivatedAbilityId(1),
        PowerAbility::Saddle,
        n,
    ));
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::self_attacks()
            .only_if(spellwake_engine::program::TriggerCondition::SourceSaddled),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::BoostSaddledAttacker {
                adjustment: PowerToughness {
                    power: 1.into(),
                    toughness: 2.into(),
                },
                keyword: KeywordAbility::Menace,
            }],
        ),
    });
    Some(program)
}
