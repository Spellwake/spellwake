//! Match complete CR 702.138 clauses before the comma-separated creature keyword parser.
use super::{Profile, fixed_cost};
use spellwake_engine::program::ConditionalCastCost;

fn count(word: &str) -> Option<u16> {
    Some(match word {
        "a" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        _ => return None,
    })
}

pub(super) fn extract(
    rules: String,
    profile: Profile,
) -> Option<(String, Option<ConditionalCastCost>)> {
    if profile < Profile::GameplayV3
        || !rules.lines().any(|line| line.trim().starts_with("Escape—"))
    {
        return Some((rules, None));
    }
    let mut price = None;
    let mut counters = None;
    let mut other = Vec::new();
    for line in rules.lines().map(str::trim) {
        if let Some(cost) = line.strip_prefix("Escape—") {
            let (mana, cards) = cost.split_once(", Exile ")?;
            let (word, grammar) = cards.split_once(' ')?;
            let exile = count(word)?;
            let expected = if exile == 1 {
                "other card from your graveyard."
            } else {
                "other cards from your graveyard."
            };
            if grammar != expected || price.replace((fixed_cost(mana)?, exile)).is_some() {
                return None;
            }
        } else if let Some(remainder) = line.strip_prefix("This creature escapes with ") {
            let (word, rest) = remainder.split_once(' ')?;
            let amount = count(word)?;
            let expected = if amount == 1 {
                "+1/+1 counter on it."
            } else {
                "+1/+1 counters on it."
            };
            if rest != expected || counters.replace(amount).is_some() {
                return None;
            }
        } else {
            other.push(line);
        }
    }
    let (mana, exile) = price?;
    Some((
        other.join("\n"),
        Some(ConditionalCastCost::Escape {
            mana,
            exile,
            entry_counters: counters.unwrap_or(0),
        }),
    ))
}

pub(super) fn attachment(
    card: &super::InventoryCard,
    vocabulary: &super::CreatureTypeVocabulary,
) -> Option<super::Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if face.type_line.as_deref() != Some("Enchantment — Aura") {
        return None;
    }
    let (rest, cost) = extract(
        super::without_reminder(face.oracle_text.as_deref()?)?,
        Profile::GameplayV3,
    )?;
    let cost = cost?;
    let mut program =
        super::attachments::compile_rules(card, vocabulary, rest, Profile::GameplayV3)?;
    program.conditional_cost = Some(cost);
    Some(program)
}
