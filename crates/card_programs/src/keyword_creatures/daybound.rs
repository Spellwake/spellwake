//! Bind both faces and the dated back-face color indicator; retain each face's independent name and text.
use super::*;
use spellwake_engine::characteristics::Color;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
) -> Option<Program> {
    let [front, back] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "transform" || card.name != format!("{} // {}", front.name, back.name) {
        return None;
    }
    let make = |face: &CatalogFace, word: &str, key: KeywordAbility| -> Option<Program> {
        let text = without_reminder(face.oracle_text.as_deref()?)?;
        let mut found = false;
        let mut remaining = vec![];
        for row in text.lines().map(str::trim).filter(|row| !row.is_empty()) {
            if row == word {
                if found {
                    return None;
                }
                found = true;
            } else {
                remaining.push(row);
            }
        }
        if !found {
            return None;
        }
        let mut ordinary = card.clone();
        ordinary.layout = "normal".into();
        ordinary.name = face.name.clone();
        ordinary.faces = vec![face.clone()];
        let mut definition = creature_face(
            &ordinary,
            vocabulary,
            Profile::Current,
            Some(&remaining.join("\n")),
            key == KeywordAbility::Nightbound,
        )
        .ok()?;
        definition.names = ObjectNames::single(EnglishCardName::new(face.name.clone()).ok()?);
        definition.printed_keywords.insert(key);
        Some(definition)
    };
    let mut day = make(front, "Daybound", KeywordAbility::Daybound)?;
    let mut night = make(back, "Nightbound", KeywordAbility::Nightbound)?;
    night.color_indicator = Some(
        indicator?
            .iter()
            .map(|c| match c.as_str() {
                "W" => Some(Color::White),
                "U" => Some(Color::Blue),
                "B" => Some(Color::Black),
                "R" => Some(Color::Red),
                "G" => Some(Color::Green),
                _ => None,
            })
            .collect::<Option<BTreeSet<_>>>()?,
    );
    // A physical reverse face has no mana cost and cannot be cast by itself.
    if !back.mana_cost.as_deref().unwrap_or("").is_empty() {
        return None;
    }
    let spellwake_engine::program::ProgramKind::Permanent(ref mut p) = night.kind else {
        return None;
    };
    p.casting_cost = None;
    day.nightbound_back = Some(Box::new(night));
    Some(day)
}
