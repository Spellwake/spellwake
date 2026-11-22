//! Complete single-striation Spacecraft with literal thresholds and printed keyword grants.
use super::*;
use spellwake_engine::{
    characteristics::PowerToughness,
    program::{ResolvingPlayer, TriggerGrouping},
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact — Spacecraft")
    {
        return None;
    }
    let raw = face.oracle_text.as_deref()?;
    if !raw
        .lines()
        .any(|line| line == "Station" || line.starts_with("Station ("))
    {
        return None;
    }
    let text = without_reminder(raw)?;
    let rows = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let (last, prefix) = rows.split_last()?;
    let (threshold, grants) = last.split_once("+ | ")?;
    let threshold = threshold.parse().ok()?;
    let keywords = grants
        .split(", ")
        .map(|s| match s.to_ascii_lowercase().as_str() {
            "flying" => Some(KeywordAbility::Flying),
            "haste" => Some(KeywordAbility::Haste),
            "trample" => Some(KeywordAbility::Trample),
            "lifelink" => Some(KeywordAbility::Lifelink),
            "first strike" => Some(KeywordAbility::FirstStrike),
            "vigilance" => Some(KeywordAbility::Vigilance),
            _ => None,
        })
        .collect::<Option<BTreeSet<_>>>()?;
    let mut program = Program::station_spacecraft(
        fixed_cost(face.mana_cost.as_deref()?)?,
        threshold,
        PowerToughness {
            power: number(face.power.as_deref())?.into(),
            toughness: number(face.toughness.as_deref())?.into(),
        },
        keywords,
    );
    match prefix {
        ["Station"] => {}
        ["When this Spacecraft enters, surveil 2.", "Station"] => {
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Surveil {
                        player: ResolvingPlayer::Controller,
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                    }],
                ),
            })
        }
        _ => return None,
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}
