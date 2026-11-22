use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId},
    land::LandType,
    program::{CardType, LibrarySearch, SearchCardMatcher},
};

/// Consume one complete ordinary cycling or typecycling clause with a fixed mana cost.
pub(super) fn hand_ability(
    label: &str,
    id: ActivatedAbilityId,
    vocabulary: &CreatureTypeVocabulary,
    source_cost: Option<&str>,
) -> Option<ActivatedAbility> {
    let (word, cost) = label.trim().split_once(" {")?;
    let cost = super::fixed_cost(&format!("{{{}", cost.to_ascii_uppercase()))?;
    if word.eq_ignore_ascii_case("transmute") {
        return Some(ActivatedAbility::transmute(
            id,
            cost,
            super::fixed_cost(source_cost?)?.total(),
        ));
    }
    if word.eq_ignore_ascii_case("cycling") {
        return Some(ActivatedAbility::cycling(id, cost));
    }
    let lower = word.to_ascii_lowercase();
    let qualifier = lower.strip_suffix("cycling")?;
    let filter = match qualifier {
        "basic land" => LibrarySearch::to_hand(SearchCardMatcher::BasicLand),
        "land" => LibrarySearch::card_type_to_hand(CardType::Land),
        other => {
            if let Some(kind) = LandType::ALL
                .into_iter()
                .find(|kind| kind.wire_label().eq_ignore_ascii_case(other))
            {
                LibrarySearch::land_subtype_to_hand(kind)
            } else {
                let label = vocabulary
                    .members()
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(other))?;
                LibrarySearch::to_hand(SearchCardMatcher::CreatureSubtype(
                    vocabulary.resolve_label(label)?,
                ))
            }
        }
    };
    Some(ActivatedAbility::typecycling(id, cost, filter))
}

/// Complete printed clauses admitted only by the third keyword profile.
pub(super) fn extended_hand_ability(
    label: &str,
    id: ActivatedAbilityId,
) -> Option<ActivatedAbility> {
    let lower = label.trim().to_ascii_lowercase();
    if let Some(cost) = lower.strip_prefix("unearth ") {
        return Some(ActivatedAbility::unearth(
            id,
            super::fixed_cost(&cost.trim().to_ascii_uppercase())?,
        ));
    }
    let (amount, cost) = lower.strip_prefix("reinforce ")?.split_once('—')?;
    let amount = spellwake_contracts::RulesInteger::try_from(amount.trim().to_owned()).ok()?;
    if !amount.is_positive() {
        return None;
    }
    Some(ActivatedAbility::reinforce(
        id,
        super::fixed_cost(&cost.trim().to_ascii_uppercase())?,
        amount,
    ))
}
