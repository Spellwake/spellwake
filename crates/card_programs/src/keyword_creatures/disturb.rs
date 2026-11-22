//! Transforming cards cast for a disturb cost (CR 702.146).
use super::*;
use spellwake_engine::characteristics::Color;
use spellwake_engine::program::{
    Disturb, ProgramKind, PublicZoneDestination, ZoneActionPredicate, ZoneObjectKind,
    ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
    ZoneReplacementSubject,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
) -> Option<Program> {
    let [front, back] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "transform"
        || card.name != format!("{} // {}", front.name, back.name)
        || !back.mana_cost.as_deref().unwrap_or("").is_empty()
    {
        return None;
    }
    let front_text = without_reminder(front.oracle_text.as_deref()?)?;
    let mut cost = None;
    let mut rules = vec![];
    for line in front_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(price) = line.strip_prefix("Disturb ") {
            if cost.is_some() {
                return None;
            }
            cost = Some(fixed_cost(price)?);
        } else {
            rules.push(line);
        }
    }
    let cost = cost?;
    let mut standalone = card.clone();
    standalone.layout = "normal".into();
    standalone.name = front.name.clone();
    standalone.faces = vec![front.clone()];
    standalone.faces[0].oracle_text = Some(rules.join("\n"));
    let mut program = super::compile(&standalone, vocabulary, Profile::Current).ok()?;
    let reverse_text = without_reminder(back.oracle_text.as_deref()?)?;
    let mut found = false;
    let mut rules = vec![];
    for line in reverse_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(subject) = line.strip_prefix("If ").and_then(|s| {
            s.strip_suffix(" would be put into a graveyard from anywhere, exile it instead.")
        }) {
            if found
                || !(subject == back.name
                    || matches!(subject, "this Aura" | "this creature" | "this enchantment"))
            {
                return None;
            }
            found = true;
        } else {
            rules.push(line);
        }
    }
    if !found {
        return None;
    }
    standalone.name = back.name.clone();
    standalone.faces = vec![back.clone()];
    standalone.faces[0].mana_cost = Some("{0}".into());
    standalone.faces[0].oracle_text = Some(rules.join("\n"));
    let mut reverse = super::compile(&standalone, vocabulary, Profile::Current).ok()?;
    let ProgramKind::Permanent(permanent) = &mut reverse.kind else {
        return None;
    };
    permanent.casting_cost = None;
    reverse.names = ObjectNames::single(EnglishCardName::new(back.name.clone()).ok()?);
    reverse.color_indicator = Some(
        indicator?
            .iter()
            .map(|color| match color.as_str() {
                "W" => Some(Color::White),
                "U" => Some(Color::Blue),
                "B" => Some(Color::Black),
                "R" => Some(Color::Red),
                "G" => Some(Color::Green),
                _ => None,
            })
            .collect::<Option<_>>()?,
    );
    if !reverse.zone_replacements.is_empty() {
        return None;
    }
    reverse.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(0),
        actions: ZoneActionPredicate::Any,
        objects: ZoneObjectKind::Either,
        subject: ZoneReplacementSubject::SourceSelf,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::Mandatory,
    });
    program.disturb = Some(Box::new(Disturb {
        cost,
        back: reverse,
    }));
    Some(program)
}
