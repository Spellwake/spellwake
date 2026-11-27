//! Stage 2 lowering for interactive choice, partition/pile, and generalized mana-spend
//! restriction primitives.
//!
//! Kept in a dedicated submodule with minimal single-line dispatch hooks in sibling files so
//! wire-backed lowering evolves independently of zero-wire convergence passes.

use super::instructions::{
    emit_choice_for_player_ref, lower_affected_players_in_context, parse_choice_kind,
    push_for_each_player,
};
use super::*;
use spellwake_engine::mana::{
    AbilityManaClause, AbilitySpendKind, ManaOutputRestriction, ManaSpecialAction,
    ManaSpellKeyword, ManaSpendPredicate, SpellCastMode, SpellColorRule, SpellManaClause,
};

/// Parse a dynamic [`ManaOutputRestriction`] for mana abilities whose spend rule references the
/// source permanent's entry-chosen creature type or color (for example, Kolvori or Throne of
/// Eldraine).
pub(super) fn parse_dynamic_mana_output_restriction(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaOutputRestriction> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "spend this mana only to cast monocolored spells of that color"
            | "spend this mana only to cast a monocolored spell of that color"
            | "spend this mana only to cast monocolored spells of the chosen color"
            | "spend this mana only to cast a monocolored spell of the chosen color"
    ) {
        return Some(ManaOutputRestriction::MonocoloredOfChosenColor);
    }
    if low.starts_with("spend this mana only to cast ")
        && (low.contains("of the chosen type") || low.contains("of that type"))
        && (low.contains("legendary creature") || low.contains("legendary spell"))
    {
        let mut fallback = ManaSpendPredicate::default();
        let clause = SpellManaClause {
            supertype: Some(Supertype::Legendary),
            card_type: low
                .contains("legendary creature")
                .then_some(CardType::Creature),
            ..Default::default()
        };
        fallback.spell_clauses.push(clause);
        fallback.canonicalize();
        if fallback.valid_authored_check(vocabulary) {
            return Some(ManaOutputRestriction::ChosenCreatureTypeOr(fallback));
        }
    }
    None
}

trait PredicateVocabCheck {
    fn valid_authored_check(&self, vocabulary: &CreatureTypeVocabulary) -> bool;
}

impl PredicateVocabCheck for ManaSpendPredicate {
    fn valid_authored_check(&self, vocabulary: &CreatureTypeVocabulary) -> bool {
        self.is_non_empty()
            && self.spell_clauses.iter().all(|c| {
                c.creature_type
                    .is_none_or(|k| vocabulary.label(k).is_some())
            })
            && self.ability_clauses.iter().all(|c| {
                c.source_creature_type
                    .is_none_or(|k| vocabulary.label(k).is_some())
            })
    }
}

/// Compositional parser mapping general `ManaSpendRestriction` prose into
/// [`ManaRestriction::Predicate`] (or an equivalent legacy variant when exact).
pub(super) fn parse_predicate_mana_restriction(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaRestriction> {
    let mut low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.is_empty() {
        return None;
    }

    // 1. Negative / prohibition forms
    if matches!(
        low.as_str(),
        "this mana can't be spent to cast spells" | "this mana can't be spent to cast a spell"
    ) {
        let mut pred = ManaSpendPredicate {
            allow_any_ability: true,
            special_actions: BTreeSet::from([
                ManaSpecialAction::Foretell,
                ManaSpecialAction::TurnFaceUp,
                ManaSpecialAction::TurnCreatureFaceUp,
                ManaSpecialAction::UnlockDoor,
                ManaSpecialAction::CumulativeUpkeep,
            ]),
            ..Default::default()
        };
        pred.canonicalize();
        return Some(ManaRestriction::Predicate(pred));
    }
    if matches!(
        low.as_str(),
        "this mana can't be spent to pay generic mana costs"
            | "this mana can't be spent to pay a generic mana cost"
    ) {
        let mut pred = ManaSpendPredicate {
            allow_any_spell: true,
            allow_any_ability: true,
            special_actions: BTreeSet::from([
                ManaSpecialAction::Foretell,
                ManaSpecialAction::TurnFaceUp,
                ManaSpecialAction::TurnCreatureFaceUp,
                ManaSpecialAction::UnlockDoor,
                ManaSpecialAction::CumulativeUpkeep,
            ]),
            disallow_generic: true,
            ..Default::default()
        };
        pred.canonicalize();
        return Some(ManaRestriction::Predicate(pred));
    }
    if low == "spend this mana only to cast a spell from anywhere other than your hand"
        || low == "spend this mana only to cast spells from anywhere other than your hand"
    {
        return Some(ManaRestriction::NotSpellFromHand);
    }

    // 2. Strip trailing uncounterable rider
    let mut pred = ManaSpendPredicate::default();
    for suffix in [
        ", and that spell can't be countered",
        ". that spell can't be countered",
    ] {
        if let Some(prefix) = low.strip_suffix(suffix) {
            pred.makes_spell_uncounterable = true;
            low = prefix.trim().to_owned();
            break;
        }
    }

    let body = low
        .strip_prefix("spend this mana only ")
        .or_else(|| low.strip_prefix("spend only "))?;

    // Handle "on ..." forms (e.g. "on costs that contain {c}" or "on costs that contain {x}")
    if let Some(rest) = body.strip_prefix("on ") {
        if rest.contains("{c}") && rest.contains("cost") {
            pred.allow_cost_containing_colorless = true;
            pred.canonicalize();
            return Some(ManaRestriction::Predicate(pred));
        }
        if rest.contains("{x}") && rest.contains("cost") {
            pred.allow_cost_containing_x = true;
            pred.canonicalize();
            return Some(ManaRestriction::Predicate(pred));
        }
        return None;
    }

    let body = body.strip_prefix("to ")?.trim();

    // Split top-level action clauses at explicit action verb boundaries.
    let segments = split_action_segments(body);
    if segments.is_empty() {
        return None;
    }

    // Track last creature subtypes from a preceding spell segment so "abilities of creatures of
    // those types" / "abilities of sources of those types" can inherit them.
    let mut inherited_creature_types: Vec<CreatureTypeId> = Vec::new();
    let mut deferred_ability_inherit = false;

    for seg in segments {
        let seg = seg.trim();
        if seg.is_empty() {
            return None;
        }
        if let Some(rest) = seg.strip_prefix("cast ") {
            let clauses = parse_spell_segment(rest, vocabulary)?;
            for c in &clauses {
                if let Some(ct) = c.creature_type {
                    inherited_creature_types.push(ct);
                }
            }
            pred.spell_clauses.extend(clauses);
        } else if let Some(rest) = seg.strip_prefix("activate ") {
            if !parse_ability_segment(
                rest,
                vocabulary,
                &inherited_creature_types,
                &mut pred,
                &mut deferred_ability_inherit,
            ) {
                return None;
            }
        } else if let Some(rest) = seg.strip_prefix("pay ") {
            if !parse_pay_segment(rest, &mut pred) {
                return None;
            }
        } else if seg.starts_with("foretell") {
            pred.special_actions.insert(ManaSpecialAction::Foretell);
        } else if seg.starts_with("turn ") && seg.contains("face up") {
            if seg.contains("creature") || seg.contains("manifested") {
                pred.special_actions
                    .insert(ManaSpecialAction::TurnCreatureFaceUp);
            } else {
                pred.special_actions.insert(ManaSpecialAction::TurnFaceUp);
            }
        } else if seg.starts_with("unlock ") && (seg.contains("door") || seg.contains("doors")) {
            pred.special_actions.insert(ManaSpecialAction::UnlockDoor);
        } else if seg == "gain a class level" || seg == "gain class levels" {
            pred.ability_clauses.push(AbilityManaClause {
                kind: Some(AbilitySpendKind::GainClassLevel),
                ..Default::default()
            });
        } else {
            return None;
        }
    }

    if deferred_ability_inherit {
        if inherited_creature_types.is_empty() {
            return None;
        }
        for ct in inherited_creature_types {
            pred.ability_clauses.push(AbilityManaClause {
                source_creature_type: Some(ct),
                ..Default::default()
            });
        }
    }

    pred.canonicalize();
    if pred.valid_authored_check(vocabulary) {
        Some(ManaRestriction::Predicate(pred))
    } else {
        None
    }
}

/// Split a `"to <seg1>, <seg2>, or <seg3>"` chain at top-level action verb boundaries.
fn split_action_segments(body: &str) -> Vec<String> {
    const DELIMS: &[&str] = &[
        ", or to ",
        ", and/or to ",
        ", and to ",
        " or to ",
        " and/or to ",
        " and to ",
        ", or ",
        ", and/or ",
        ", and ",
        " or ",
        " and/or ",
        " and ",
        ", ",
    ];
    const VERB_STARTS: &[&str] = &[
        "cast ",
        "activate ",
        "pay ",
        "foretell",
        "turn ",
        "unlock ",
        "gain a class level",
        "gain class levels",
    ];
    let mut out = Vec::new();
    let mut cur = body.trim();
    loop {
        let mut best: Option<(usize, usize)> = None;
        for &delim in DELIMS {
            let mut search_from = 0;
            while let Some(rel) = cur[search_from..].find(delim) {
                let idx = search_from + rel;
                let after = cur[idx + delim.len()..].trim_start();
                if VERB_STARTS.iter().any(|v| after.starts_with(v)) {
                    if best.is_none_or(|(b_idx, b_len)| {
                        idx < b_idx || (idx == b_idx && delim.len() > b_len)
                    }) {
                        best = Some((idx, delim.len()));
                    }
                    break;
                }
                search_from = idx + delim.len();
            }
        }
        match best {
            Some((idx, len)) => {
                out.push(cur[..idx].trim().to_owned());
                cur = cur[idx + len..].trim();
            }
            None => {
                if !cur.is_empty() {
                    out.push(cur.to_owned());
                }
                break;
            }
        }
    }
    out
}

fn parse_pay_segment(rest: &str, pred: &mut ManaSpendPredicate) -> bool {
    let r = rest.trim();
    if matches!(
        r,
        "cumulative upkeep costs" | "a cumulative upkeep cost" | "cumulative upkeep"
    ) {
        pred.special_actions
            .insert(ManaSpecialAction::CumulativeUpkeep);
        return true;
    }
    if matches!(r, "a disturb cost" | "disturb costs") {
        pred.spell_clauses.push(SpellManaClause {
            cast_mode: Some(SpellCastMode::Disturb),
            ..Default::default()
        });
        return true;
    }
    if matches!(r, "a morph cost" | "morph costs") {
        pred.special_actions
            .insert(ManaSpecialAction::TurnCreatureFaceUp);
        return true;
    }
    if r.contains("{c}") && r.contains("cost") {
        pred.allow_cost_containing_colorless = true;
        return true;
    }
    if r.contains("{x}") && r.contains("cost") {
        pred.allow_cost_containing_x = true;
        return true;
    }
    false
}

fn parse_ability_segment(
    rest: &str,
    vocabulary: &CreatureTypeVocabulary,
    inherited_creature_types: &[CreatureTypeId],
    pred: &mut ManaSpendPredicate,
    deferred_inherit: &mut bool,
) -> bool {
    let r = rest.trim();
    if matches!(
        r,
        "abilities" | "an ability" | "activated abilities" | "an activated ability"
    ) {
        pred.allow_any_ability = true;
        return true;
    }
    if matches!(r, "power-up abilities" | "a power-up ability") {
        pred.ability_clauses.push(AbilityManaClause {
            kind: Some(AbilitySpendKind::PowerUp),
            ..Default::default()
        });
        return true;
    }
    if matches!(r, "equip abilities" | "an equip ability") {
        pred.ability_clauses.push(AbilityManaClause {
            kind: Some(AbilitySpendKind::Equip),
            ..Default::default()
        });
        return true;
    }
    let core = r
        .strip_prefix("activated abilities of ")
        .or_else(|| r.strip_prefix("an activated ability of "))
        .or_else(|| r.strip_prefix("abilities of "))
        .or_else(|| r.strip_prefix("an ability of "))
        .map(str::trim);
    let Some(core) = core else {
        return false;
    };
    if matches!(
        core,
        "creatures of that type"
            | "a creature of that type"
            | "permanents of that type"
            | "a permanent of that type"
            | "sources of that type"
            | "a source of that type"
            | "creatures of those types"
            | "permanents of those types"
            | "sources of those types"
    ) {
        if inherited_creature_types.is_empty() {
            *deferred_inherit = true;
        } else {
            for &ct in inherited_creature_types {
                pred.ability_clauses.push(AbilityManaClause {
                    source_creature_type: Some(ct),
                    ..Default::default()
                });
            }
        }
        return true;
    }
    // Split disjunctive source list (e.g. "time lord or alien sources", "cleric, rogue, warrior, or wizard sources")
    let cleaned_core = strip_ability_source_suffix(core);
    let items = split_disjunctive_items(cleaned_core);
    if items.is_empty() {
        return false;
    }
    for raw_item in items {
        let item = strip_leading_article(strip_ability_source_suffix(raw_item.trim()));
        if item.is_empty() {
            return false;
        }
        if item == "outlaw" || item == "outlaws" {
            for name in ["Assassin", "Mercenary", "Pirate", "Rogue", "Warlock"] {
                let Some(ct) = vocabulary.resolve_label(name) else {
                    return false;
                };
                pred.ability_clauses.push(AbilityManaClause {
                    source_creature_type: Some(ct),
                    ..Default::default()
                });
            }
            continue;
        }
        if item == "colorless" {
            pred.ability_clauses.push(AbilityManaClause {
                source_colorless: true,
                ..Default::default()
            });
            continue;
        }
        if let Some(card_type) = parse_single_card_type_word(item) {
            pred.ability_clauses.push(AbilityManaClause {
                source_card_type: Some(card_type),
                ..Default::default()
            });
            continue;
        }
        if let Some(ct) = resolve_mana_creature_type(item, vocabulary) {
            pred.ability_clauses.push(AbilityManaClause {
                source_creature_type: Some(ct),
                ..Default::default()
            });
            continue;
        }
        return false;
    }
    true
}

fn strip_ability_source_suffix(s: &str) -> &str {
    let mut cur = s.trim();
    for suffix in [
        " permanents",
        " permanent",
        " sources",
        " source",
        " creatures",
        " creature",
        " cards",
        " card",
    ] {
        if let Some(stripped) = cur.strip_suffix(suffix) {
            cur = stripped.trim();
            break;
        }
    }
    cur
}

fn strip_leading_article(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix("a ")
        .or_else(|| s.strip_prefix("an "))
        .or_else(|| s.strip_prefix("the "))
        .unwrap_or(s)
        .trim()
}

fn parse_spell_segment(
    rest: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<SpellManaClause>> {
    let mut s = rest.trim();
    // Handle trailing shared modifiers that apply to the whole spell segment.
    let mut shared = SpellManaClause::default();
    if let Some(prefix) = s
        .strip_suffix(" from your graveyard")
        .or_else(|| s.strip_suffix(" from a graveyard"))
    {
        shared.cast_mode = Some(SpellCastMode::FromGraveyard);
        s = prefix.trim();
    } else if let Some(prefix) = s.strip_suffix(" from exile") {
        shared.cast_mode = Some(SpellCastMode::FromExile);
        s = prefix.trim();
    } else if let Some(prefix) = s
        .strip_suffix(" you don't own")
        .or_else(|| s.strip_suffix(" you do not own"))
    {
        shared.cast_mode = Some(SpellCastMode::NotOwned);
        s = prefix.trim();
    } else if let Some(prefix) = s.strip_suffix(" with no abilities") {
        shared.no_abilities = true;
        s = prefix.trim();
    } else if let Some(prefix) = s
        .strip_suffix(" with devoid")
        .or_else(|| s.strip_suffix(" that has devoid"))
        .or_else(|| s.strip_suffix(" that have devoid"))
    {
        shared.keyword = Some(ManaSpellKeyword::Devoid);
        s = prefix.trim();
    } else if let Some(prefix) = s
        .strip_suffix(" with exactly three colors")
        .or_else(|| s.strip_suffix(" that are exactly three colors"))
        .or_else(|| s.strip_suffix(" that's exactly three colors"))
    {
        shared.color_rule = Some(SpellColorRule::ExactColorCount(3));
        s = prefix.trim();
    }

    if matches!(
        s,
        "your second spell in a turn"
            | "your second spell each turn"
            | "your second spell this turn"
    ) {
        let mut c = shared;
        c.cast_mode = Some(SpellCastMode::SecondSpellThisTurn);
        return Some(vec![c]);
    }

    if matches!(s, "a spell" | "spells") {
        if shared != SpellManaClause::default() {
            return Some(vec![shared]);
        }
        return None;
    }

    // Split disjunctive items (preserving phrases like "a spell with freerunning" or "a chandra planeswalker spell").
    let items = split_disjunctive_items(s);
    if items.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    for raw_item in items {
        let parsed = parse_single_spell_item(raw_item.trim(), &shared, vocabulary)?;
        out.extend(parsed);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn parse_single_spell_item(
    raw: &str,
    shared: &SpellManaClause,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<SpellManaClause>> {
    let mut item = strip_leading_article(raw);
    let mut clause = shared.clone();

    // Peel per-item trailing clauses ("with freerunning", "that has foretell", "with devoid", etc.)
    if let Some(prefix) = item
        .strip_suffix(" with freerunning")
        .or_else(|| item.strip_suffix(" that has freerunning"))
        .or_else(|| item.strip_suffix(" that have freerunning"))
    {
        clause.keyword = Some(ManaSpellKeyword::Freerunning);
        item = prefix.trim();
    } else if let Some(prefix) = item
        .strip_suffix(" with foretell")
        .or_else(|| item.strip_suffix(" that has foretell"))
        .or_else(|| item.strip_suffix(" that have foretell"))
    {
        clause.keyword = Some(ManaSpellKeyword::Foretell);
        item = prefix.trim();
    } else if let Some(prefix) = item
        .strip_suffix(" with devoid")
        .or_else(|| item.strip_suffix(" that has devoid"))
        .or_else(|| item.strip_suffix(" that have devoid"))
    {
        clause.keyword = Some(ManaSpellKeyword::Devoid);
        item = prefix.trim();
    } else if let Some(prefix) = item.strip_suffix(" with no abilities") {
        clause.no_abilities = true;
        item = prefix.trim();
    }

    // Strip trailing "spells" / "spell" / "cards" / "card"
    item = item
        .strip_suffix(" spells")
        .or_else(|| item.strip_suffix(" spell"))
        .or_else(|| item.strip_suffix(" cards"))
        .or_else(|| item.strip_suffix(" card"))
        .unwrap_or(item)
        .trim();

    if item.is_empty() || item == "a" || item == "an" {
        return if clause != SpellManaClause::default() {
            Some(vec![clause])
        } else {
            None
        };
    }

    // Peel leading modifiers (legendary, colorless, colors, face-down, kicked)
    loop {
        if let Some(rest) = item.strip_prefix("legendary ") {
            clause.supertype = Some(Supertype::Legendary);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("snow ") {
            clause.supertype = Some(Supertype::Snow);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("colorless ") {
            clause.color_rule = Some(SpellColorRule::Colorless);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("multicolored ") {
            clause.color_rule = Some(SpellColorRule::Multicolored);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("face-down ") {
            clause.cast_mode = Some(SpellCastMode::FaceDown);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("kicked ") {
            clause.cast_mode = Some(SpellCastMode::Kicked);
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("white ") {
            clause.color_rule = Some(SpellColorRule::ExactColor(Color::White));
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("blue ") {
            clause.color_rule = Some(SpellColorRule::ExactColor(Color::Blue));
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("black ") {
            clause.color_rule = Some(SpellColorRule::ExactColor(Color::Black));
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("red ") {
            clause.color_rule = Some(SpellColorRule::ExactColor(Color::Red));
            item = rest.trim();
        } else if let Some(rest) = item.strip_prefix("green ") {
            clause.color_rule = Some(SpellColorRule::ExactColor(Color::Green));
            item = rest.trim();
        } else {
            break;
        }
    }

    // Standalone modifier-only tokens (e.g. "kicked", "face-down", "legendary", "colorless")
    match item {
        "" => return Some(vec![clause]),
        "legendary" => {
            clause.supertype = Some(Supertype::Legendary);
            return Some(vec![clause]);
        }
        "colorless" => {
            clause.color_rule = Some(SpellColorRule::Colorless);
            return Some(vec![clause]);
        }
        "multicolored" => {
            clause.color_rule = Some(SpellColorRule::Multicolored);
            return Some(vec![clause]);
        }
        "face-down" => {
            clause.cast_mode = Some(SpellCastMode::FaceDown);
            return Some(vec![clause]);
        }
        "kicked" => {
            clause.cast_mode = Some(SpellCastMode::Kicked);
            return Some(vec![clause]);
        }
        "outlaw" | "outlaws" => {
            let mut clauses = Vec::new();
            for name in ["Assassin", "Mercenary", "Pirate", "Rogue", "Warlock"] {
                let ct = vocabulary.resolve_label(name)?;
                let mut c = clause.clone();
                c.creature_type = Some(ct);
                clauses.push(c);
            }
            return Some(clauses);
        }
        "aura" | "auras" => {
            clause.is_aura = true;
            return Some(vec![clause]);
        }
        "equipment" => {
            clause.artifact_subtype = Some(ArtifactSubtype::Equipment);
            return Some(vec![clause]);
        }
        "vehicle" | "vehicles" => {
            clause.artifact_subtype = Some(ArtifactSubtype::Vehicle);
            return Some(vec![clause]);
        }
        "saga" | "sagas" => {
            clause.enchantment_subtype = Some(EnchantmentSubtype::Saga);
            return Some(vec![clause]);
        }
        "shrine" | "shrines" => {
            clause.enchantment_subtype = Some(EnchantmentSubtype::Shrine);
            return Some(vec![clause]);
        }
        "room" | "rooms" => {
            clause.enchantment_subtype = Some(EnchantmentSubtype::Room);
            return Some(vec![clause]);
        }
        "class" | "classes" => {
            clause.enchantment_subtype = Some(EnchantmentSubtype::Class);
            return Some(vec![clause]);
        }
        "lesson" | "lessons" => {
            clause.spell_subtype = Some(SpellSubtype::Lesson);
            return Some(vec![clause]);
        }
        "omen" | "omens" => {
            clause.spell_subtype = Some(SpellSubtype::Omen);
            return Some(vec![clause]);
        }
        "arcane" => {
            clause.spell_subtype = Some(SpellSubtype::Arcane);
            return Some(vec![clause]);
        }
        "adventure" | "adventures" => {
            clause.spell_subtype = Some(SpellSubtype::Adventure);
            return Some(vec![clause]);
        }
        "trap" | "traps" => {
            clause.spell_subtype = Some(SpellSubtype::Trap);
            return Some(vec![clause]);
        }
        _ => {}
    }

    // Card type word ("creature", "artifact", "enchantment", "instant", "sorcery", "planeswalker")
    if let Some(card_type) = parse_single_card_type_word(item) {
        clause.card_type = Some(card_type);
        return Some(vec![clause]);
    }

    // "<planeswalker subtype> planeswalker" (e.g. "chandra planeswalker")
    if let Some(pw_raw) = item.strip_suffix(" planeswalker") {
        let cap = capitalize_first(pw_raw.trim());
        if let Some(pw) = PlaneswalkerSubtype::from_label(&cap) {
            clause.card_type = Some(CardType::Planeswalker);
            clause.planeswalker_subtype = Some(pw);
            return Some(vec![clause]);
        }
    }

    // "<creature subtype> creature"
    if let Some(sub_raw) = item
        .strip_suffix(" creature")
        .or_else(|| item.strip_suffix(" creatures"))
        && let Some(ct) = resolve_mana_creature_type(sub_raw.trim(), vocabulary)
    {
        clause.card_type = Some(CardType::Creature);
        clause.creature_type = Some(ct);
        return Some(vec![clause]);
    }

    // Bare creature subtype (e.g. "assassin", "time lord", "alien", "mount", "pilot", "dragon", "dwarf")
    if let Some(ct) = resolve_mana_creature_type(item, vocabulary) {
        clause.creature_type = Some(ct);
        return Some(vec![clause]);
    }

    None
}

fn parse_single_card_type_word(word: &str) -> Option<CardType> {
    match word.trim() {
        "artifact" | "artifacts" => Some(CardType::Artifact),
        "battle" | "battles" => Some(CardType::Battle),
        "creature" | "creatures" => Some(CardType::Creature),
        "enchantment" | "enchantments" => Some(CardType::Enchantment),
        "instant" | "instants" => Some(CardType::Instant),
        "kindred" => Some(CardType::Kindred),
        "land" | "lands" => Some(CardType::Land),
        "planeswalker" | "planeswalkers" => Some(CardType::Planeswalker),
        "sorcery" | "sorceries" => Some(CardType::Sorcery),
        _ => None,
    }
}

fn resolve_mana_creature_type(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CreatureTypeId> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    resolve_protection_subtype(cleaned, vocabulary)
        .or_else(|| {
            cleaned
                .strip_suffix("ies")
                .and_then(|stem| resolve_protection_subtype(&format!("{stem}y"), vocabulary))
        })
        .or_else(|| {
            cleaned
                .strip_suffix("es")
                .and_then(|stem| resolve_protection_subtype(stem, vocabulary))
        })
        .or_else(|| {
            cleaned
                .strip_suffix('s')
                .and_then(|stem| resolve_protection_subtype(stem, vocabulary))
        })
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn split_disjunctive_items(s: &str) -> Vec<String> {
    let normalized = s
        .replace(", and/or ", "\x1f")
        .replace(", or ", "\x1f")
        .replace(", and ", "\x1f")
        .replace(" and/or ", "\x1f")
        .replace(" or ", "\x1f")
        .replace(" and ", "\x1f")
        .replace(", ", "\x1f");
    normalized
        .split('\x1f')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Apply `restriction` onto the most recent `AddMana` / `AddManaForPlayer` instruction in `op`
/// (mirroring `retain_mana_in_instruction`).
pub(super) fn restrict_mana_in_instruction(
    op: &mut Instruction,
    restriction: &ManaRestriction,
) -> bool {
    match op {
        Instruction::AddMana(yield_kind) | Instruction::AddManaForPlayer { yield_kind, .. } => {
            wrap_restricted_mana_yield(yield_kind, restriction)
        }
        _ => false,
    }
}

fn wrap_restricted_mana_yield(
    yield_kind: &mut EffectManaYield,
    restriction: &ManaRestriction,
) -> bool {
    match yield_kind {
        EffectManaYield::RetainedUntilCleanup(inner) => {
            wrap_restricted_mana_yield(inner.as_mut(), restriction)
        }
        EffectManaYield::Fixed(bundle) => {
            let moved = std::mem::take(bundle);
            *yield_kind = EffectManaYield::RestrictedFixed {
                bundle: moved,
                restriction: restriction.clone(),
            };
            true
        }
        EffectManaYield::Choice { colors, amount } => {
            let moved_colors = std::mem::take(colors);
            let moved_amount = std::mem::replace(amount, ValueExpr::literal(0));
            *yield_kind = EffectManaYield::RestrictedChoice {
                colors: moved_colors,
                amount: moved_amount,
                restriction: restriction.clone(),
            };
            true
        }
        EffectManaYield::Counted { kind, amount } => {
            let k = *kind;
            let moved_amount = std::mem::replace(amount, ValueExpr::literal(0));
            *yield_kind = EffectManaYield::RestrictedCounted {
                kind: k,
                amount: moved_amount,
                restriction: restriction.clone(),
            };
            true
        }
        _ => false,
    }
}

/// Patch `any_color = true` on a preceding exile/impulse play permission instruction when a
/// trailing `"Mana of any type/color can be spent to cast spells this way"` sentence appears.
pub(super) fn patch_any_color_cast_permission(op: &mut Instruction) -> bool {
    match op {
        Instruction::ApplyContinuous(spec) => {
            if let spellwake_engine::toolbox::ContinuousPayload::PlayPermission {
                any_color, ..
            } = &mut spec.payload
            {
                *any_color = true;
                true
            } else {
                false
            }
        }
        Instruction::ExileTopPlay { any_color, .. }
        | Instruction::LookTopChooseExt {
            keep_dest:
                spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. }
                | spellwake_engine::program::LookTopKeepDest::ExileLinked { any_color, .. },
            ..
        }
        | Instruction::RevealUntilMatchExt {
            match_dest:
                spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. }
                | spellwake_engine::program::LookTopKeepDest::ExileLinked { any_color, .. },
            ..
        } => {
            *any_color = true;
            true
        }
        _ => false,
    }
}

/// Lower CR 701.30a Populate into dedicated [`Instruction::Populate`] (or a loop for dynamic counts).
pub(super) fn lower_populate(
    count: &ValueExprAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let resolved = resolve_x(count, bound_x);
    if let ValueExprAst::Const(n) = resolved {
        if *n <= 0 {
            return Ok(());
        }
        if *n <= 8 {
            for _ in 0..*n {
                builder.instructions.push(Instruction::Populate);
            }
            return Ok(());
        }
    }
    let cnt = lower_value_expr_in_context(resolved, builder, in_trigger, vocabulary)?;
    builder.instructions.push(Instruction::RepeatLoop {
        count: cnt,
        body: Box::new(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::Populate],
        )),
    });
    Ok(())
}

/// Strip a revealed-hand reference (`from it`, `from them`, `from that hand`, `from their hand`,
/// or `of those cards`) whether trailing or embedded before a qualifier clause (for example
/// `"a nonland card from it with mana value 3 or less"`).
fn strip_revealed_hand_ref(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    for suffix in [
        " from it",
        " from them",
        " from that hand",
        " from their hand",
        " of those cards",
    ] {
        if let Some(rest) = trimmed.strip_suffix(suffix) {
            let cleaned = rest.trim();
            return (!cleaned.is_empty()).then(|| cleaned.to_owned());
        }
    }
    for mid in [
        " from it ",
        " from them ",
        " from that hand ",
        " from their hand ",
        " of those cards ",
    ] {
        if let Some((head, tail)) = trimmed.split_once(mid) {
            let joined = format!("{} {}", head.trim(), tail.trim());
            let cleaned = joined.trim();
            return (!cleaned.is_empty()).then(|| cleaned.to_owned());
        }
    }
    None
}

/// Extract a `"<filter> from it/them"` hand-choice description from either
/// `InstructionAst::ChooseOption` or `InstructionAst::InteractiveChoice(ChooseFromGroup)`.
pub(super) fn extract_revealed_hand_choice_desc(op: &InstructionAst) -> Option<String> {
    let raw = match op {
        InstructionAst::ChooseOption(s) => s.as_str(),
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::ChooseFromGroup {
            chooser: PlayerRefAst::You,
            description,
        }) => description.as_str(),
        _ => return None,
    };
    strip_revealed_hand_ref(raw)
}

/// Unwrap either a bare `LookTopLibrary` or a single-instruction `BindX([LookTopLibrary])`
/// so `LookTopLibrary + ChooseAndKeep` fusion works for `Domain`/`Devotion`/counted `X` cards.
pub(super) fn unwrap_look_top_with_bind_x<'a>(
    op: &'a InstructionAst,
    bound_x: Option<&'a ValueExprAst>,
) -> Option<(bool, &'a str, &'a ValueExprAst, Option<&'a ValueExprAst>)> {
    match op {
        InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } => Some((*reveal, library.as_str(), count.as_ref(), bound_x)),
        InstructionAst::BindX { x_expr, body } => match body.as_slice() {
            [
                InstructionAst::LookTopLibrary {
                    reveal,
                    library,
                    count,
                },
            ] => Some((
                *reveal,
                library.as_str(),
                count.as_ref(),
                Some(x_expr.as_ref()),
            )),
            _ => None,
        },
        _ => None,
    }
}

/// Fuse `LookTopLibrary` (or `BindX([LookTopLibrary])`) + `SeparateIntoPiles(2)`
/// (+ optional `ChoosePile`) + `ChooseAndKeep("one pile" | "that pile", keep_to, rest_to)`
/// into [`Instruction::RevealTopTwoPiles`].
pub(super) fn try_fuse_two_pile_split(
    ins: &[InstructionAst],
    i: usize,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Option<usize> {
    let (_, library, count, eff_bound_x) = unwrap_look_top_with_bind_x(&ins[i], bound_x)?;
    let lib_low = library.trim().to_ascii_lowercase();
    if !matches!(
        lib_low.as_str(),
        "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" | "your library" | "library"
    ) {
        return None;
    }
    let sep_idx = i + 1;
    let InstructionAst::SeparateIntoPiles {
        chooser: divider,
        pile_count: 2,
        ..
    } = ins.get(sep_idx)?
    else {
        return None;
    };
    let (controller_divides, target_opponent) = match divider {
        PlayerRefAst::You => (true, false),
        PlayerRefAst::AnyOpponent => (false, false),
        PlayerRefAst::TargetOpponent => (false, true),
        PlayerRefAst::ThatPlayer => (false, false),
        _ => return None,
    };
    let mut cursor = sep_idx + 1;
    if let Some(InstructionAst::ChoosePile { chooser: taker }) = ins.get(cursor) {
        let expected_ok = if controller_divides {
            matches!(
                taker,
                PlayerRefAst::AnyOpponent | PlayerRefAst::TargetOpponent | PlayerRefAst::ThatPlayer
            )
        } else {
            matches!(taker, PlayerRefAst::You)
        };
        if !expected_ok {
            return None;
        }
        cursor += 1;
    }
    let InstructionAst::ChooseAndKeep {
        pick,
        keep_to,
        rest_to,
    } = ins.get(cursor)?
    else {
        return None;
    };
    let pick_low = pick.trim().to_ascii_lowercase();
    if !matches!(
        pick_low.as_str(),
        "one pile" | "that pile" | "one of those piles" | "the chosen pile" | "a pile"
    ) {
        return None;
    }
    let chosen_dest = parse_two_pile_keep_dest(keep_to)?;
    let other_dest = parse_two_pile_rest_dest(rest_to)?;
    let cnt = lower_value_expr_in_context(
        resolve_x(count, eff_bound_x),
        builder,
        in_trigger,
        vocabulary,
    )
    .ok()?;
    if target_opponent {
        let mut probe = builder.clone();
        probe.bind_target(BodyTarget::SingleOpponent).ok()?;
        *builder = probe;
    }
    builder.instructions.push(Instruction::RevealTopTwoPiles {
        count: cnt,
        controller_divides,
        target_opponent,
        chosen_dest,
        other_dest,
    });
    Some(cursor - i + 1)
}

fn parse_two_pile_keep_dest(keep_to: &str) -> Option<spellwake_engine::program::LookTopKeepDest> {
    let low = keep_to.trim().to_ascii_lowercase();
    if low.contains("hand") {
        Some(spellwake_engine::program::LookTopKeepDest::Hand)
    } else if low.contains("battlefield") {
        Some(spellwake_engine::program::LookTopKeepDest::Battlefield {
            tapped: low.contains("tapped"),
            attacking: low.contains("attacking"),
        })
    } else if low.contains("graveyard") {
        Some(spellwake_engine::program::LookTopKeepDest::Graveyard)
    } else if low.contains("exile") {
        Some(spellwake_engine::program::LookTopKeepDest::Exile {
            playable: low.contains("play") || low.contains("cast"),
            free: low.contains("without paying"),
            cast_only: low.contains("cast") && !low.contains("play"),
            any_color: low.contains("mana of any"),
            duration: ResolvedDuration::NoStatedDuration,
        })
    } else if !low.contains("bottom") && low.contains("top") {
        Some(spellwake_engine::program::LookTopKeepDest::LibraryTop)
    } else {
        None
    }
}

fn parse_two_pile_rest_dest(rest_to: &str) -> Option<spellwake_engine::program::LookTopRestDest> {
    let low = rest_to.trim().to_ascii_lowercase();
    if low.contains("graveyard") {
        Some(spellwake_engine::program::LookTopRestDest::Graveyard)
    } else if low.contains("exile") {
        Some(spellwake_engine::program::LookTopRestDest::Exile)
    } else if low.contains("hand") {
        Some(spellwake_engine::program::LookTopRestDest::Hand)
    } else if low.contains("bottom") {
        if low.contains("random") {
            Some(spellwake_engine::program::LookTopRestDest::BottomRandom)
        } else {
            Some(spellwake_engine::program::LookTopRestDest::BottomOrder)
        }
    } else if low.contains("top") {
        Some(spellwake_engine::program::LookTopRestDest::TopOrder)
    } else {
        None
    }
}

/// Lower `InstructionAst::ChooseFromGroup` / `PlayerChooseFromGroup` for announcement target
/// bindings, player selection domains, and resolution-time non-target object selection.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_choose_from_group(
    chooser: PlayerRefAst,
    count: &ValueExprAst,
    random: bool,
    group: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    if random {
        if chooser == PlayerRefAst::You && matches!(count, ValueExprAst::Const(1)) {
            match group {
                SubjectRefAst::Player(PlayerRefAst::AnyOpponent) => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser: ResolvingPlayer::Controller,
                        domain: spellwake_engine::program::ResolutionChoiceDomain::RandomPlayer {
                            opponents_only: true,
                        },
                    });
                    return Ok(());
                }
                SubjectRefAst::Player(PlayerRefAst::AnyPlayer) => {
                    builder.instructions.push(Instruction::ChooseValue {
                        chooser: ResolvingPlayer::Controller,
                        domain: spellwake_engine::program::ResolutionChoiceDomain::RandomPlayer {
                            opponents_only: false,
                        },
                    });
                    return Ok(());
                }
                _ => {}
            }
        }
        return Err("oracle:unsupported-instruction");
    }
    if chooser == PlayerRefAst::You {
        match group {
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(1),
            } => {
                let single = SubjectRefAst::TargetObject {
                    filter: filter.clone(),
                    count: TargetCountAst::Exact(1),
                };
                let bt = lower_target_subject(&single, vocabulary)?;
                match bt {
                    BodyTarget::SingleOwnGraveyardCard
                    | BodyTarget::FilteredOwnGraveyardCard(_)
                    | BodyTarget::AnyGraveyardCard
                    | BodyTarget::AnyGraveyardCreature
                    | BodyTarget::FilteredAnyGraveyardCard { .. } => {
                        builder.bind_target(bt)?;
                        builder.graveyard_target_bounds = Some((0, 1));
                        return Ok(());
                    }
                    BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::FilteredPermanent(_) => {
                        builder.bind_target(bt)?;
                        builder.up_to_one_target = true;
                        return Ok(());
                    }
                    _ => return Err("oracle:unsupported-target-subject"),
                }
            }
            SubjectRefAst::TargetObject { filter, count }
                if filter.zone.is_some()
                    && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                        || matches!(count, TargetCountAst::UpTo(max) if *max >= 2)
                        || matches!(count, TargetCountAst::AnyNumber)) =>
            {
                let (min_t, max_t) = match *count {
                    TargetCountAst::Exact(n) => (usize::from(n), usize::from(n)),
                    TargetCountAst::UpTo(max) => (0, usize::from(max)),
                    TargetCountAst::AnyNumber => (0, usize::MAX),
                    _ => return Err("oracle:unsupported-target-subject"),
                };
                let single = SubjectRefAst::TargetObject {
                    filter: filter.clone(),
                    count: TargetCountAst::Exact(1),
                };
                let bt = lower_target_subject(&single, vocabulary)?;
                if matches!(
                    bt,
                    BodyTarget::SingleOwnGraveyardCard
                        | BodyTarget::FilteredOwnGraveyardCard(_)
                        | BodyTarget::AnyGraveyardCard
                        | BodyTarget::AnyGraveyardCreature
                        | BodyTarget::FilteredAnyGraveyardCard { .. }
                ) {
                    builder.bind_target(bt)?;
                    builder.graveyard_target_bounds = Some((min_t, max_t));
                    return Ok(());
                }
                return Err("oracle:unsupported-target-subject");
            }
            SubjectRefAst::TargetOneOf {
                options,
                count: TargetCountAst::UpTo(1),
            } => {
                let single = SubjectRefAst::TargetOneOf {
                    options: options.clone(),
                    count: TargetCountAst::Exact(1),
                };
                let bt = lower_target_subject(&single, vocabulary)?;
                if matches!(
                    bt,
                    BodyTarget::SingleCreature
                        | BodyTarget::SinglePermanent
                        | BodyTarget::SinglePlaneswalker
                        | BodyTarget::FilteredPermanent(_)
                ) {
                    builder.bind_target(bt)?;
                    builder.up_to_one_target = true;
                    return Ok(());
                }
                return Err("oracle:unsupported-target-subject");
            }
            SubjectRefAst::TargetObject { .. } | SubjectRefAst::TargetOneOf { .. } => {
                let bt = lower_target_subject(group, vocabulary)?;
                builder.bind_target(bt)?;
                return Ok(());
            }
            SubjectRefAst::Player(PlayerRefAst::AnyOpponent)
                if matches!(count, ValueExprAst::Const(1)) =>
            {
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                        opponents_only: true,
                    },
                });
                return Ok(());
            }
            SubjectRefAst::Player(PlayerRefAst::AnyPlayer)
                if matches!(count, ValueExprAst::Const(1)) =>
            {
                builder.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                        opponents_only: false,
                    },
                });
                return Ok(());
            }
            _ => {}
        }
    }
    if let SubjectRefAst::Group(filter_ast) = group {
        let (players, source_zone, filter, min, max) = lower_group_choose_spec(
            chooser, count, filter_ast, builder, vocabulary, in_trigger, bound_x,
        )?;
        builder.instructions.push(Instruction::ChooseObjects {
            players,
            source_zone,
            filter: Box::new(filter),
            min,
            max,
            distinct_powers: false,
            max_total_power: None,
            unchosen_action: None,
        });
        return Ok(());
    }
    Err("oracle:unsupported-instruction")
}

/// Fuse `ChooseFromGroup` / `PlayerChooseFromGroup` / `InteractiveChoice(ChooseFromGroup)` followed
/// by a keep-vs-rest destruction, exile, bounce, sacrifice, discard, or tap into
/// [`Instruction::ChooseObjects`] with `unchosen_action`.
pub(super) fn try_fuse_choose_objects_and_rest(
    ins: &[InstructionAst],
    i: usize,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Option<usize> {
    let next = ins.get(i + 1)?;
    let unchosen_action = extract_rest_action(next)?;
    let mut probe = builder.clone();
    let spec = match &ins[i] {
        InstructionAst::ChooseFromGroup {
            count,
            random: false,
            group,
        } => match group.as_ref() {
            SubjectRefAst::Group(filter_ast) => {
                let (players, source_zone, filter, min, max) = lower_group_choose_spec(
                    PlayerRefAst::You,
                    count,
                    filter_ast,
                    &mut probe,
                    vocabulary,
                    in_trigger,
                    bound_x,
                )
                .ok()?;
                ChooseObjectsSpec {
                    players,
                    source_zone,
                    filter,
                    min,
                    max,
                    distinct_powers: false,
                    max_total_power: None,
                }
            }
            _ => return None,
        },
        InstructionAst::PlayerChooseFromGroup {
            chooser,
            count,
            random: false,
            group,
        } => match group.as_ref() {
            SubjectRefAst::Group(filter_ast) => {
                let (players, source_zone, filter, min, max) = lower_group_choose_spec(
                    *chooser, count, filter_ast, &mut probe, vocabulary, in_trigger, bound_x,
                )
                .ok()?;
                ChooseObjectsSpec {
                    players,
                    source_zone,
                    filter,
                    min,
                    max,
                    distinct_powers: false,
                    max_total_power: None,
                }
            }
            _ => return None,
        },
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::ChooseFromGroup {
            chooser,
            description,
        }) => parse_interactive_choose_objects(
            *chooser,
            description,
            &mut probe,
            vocabulary,
            in_trigger,
        )?,
        _ => return None,
    };
    *builder = probe;
    builder.instructions.push(Instruction::ChooseObjects {
        players: spec.players,
        source_zone: spec.source_zone,
        filter: Box::new(spec.filter),
        min: spec.min,
        max: spec.max,
        distinct_powers: spec.distinct_powers,
        max_total_power: spec.max_total_power,
        unchosen_action: Some(unchosen_action),
    });
    Some(2)
}

struct ChooseObjectsSpec {
    players: spellwake_engine::program::AffectedPlayers,
    source_zone: spellwake_engine::program::SourceZoneFilter,
    filter: ObjectFilter,
    min: ValueExpr,
    max: ValueExpr,
    distinct_powers: bool,
    max_total_power: Option<ValueExpr>,
}

fn extract_rest_action(
    op: &InstructionAst,
) -> Option<spellwake_engine::program::UnchosenObjectsAction> {
    use spellwake_engine::program::UnchosenObjectsAction;
    let is_rest_subject = |subj: &SubjectRefAst| -> bool {
        match subj {
            SubjectRefAst::Anaphora(raw) => {
                let low = raw.trim().to_ascii_lowercase();
                low.contains("the rest")
                    || low.contains("rest of them")
                    || low.contains("other")
                    || low.contains("not chosen")
                    || low.contains("unchosen")
            }
            SubjectRefAst::Group(flt) => {
                (flt.flags.0 & spellwake_oracle_compiler::ast::ObjectFilterFlags::EXCLUDE_SELF.0)
                    != 0
            }
            _ => false,
        }
    };
    match op {
        InstructionAst::Destroy { subject, .. } if is_rest_subject(subject) => {
            Some(UnchosenObjectsAction::Destroy)
        }
        InstructionAst::Exile { subject, .. } if is_rest_subject(subject) => {
            Some(UnchosenObjectsAction::Exile)
        }
        InstructionAst::ReturnZone {
            subject,
            to,
            except,
            ..
        } if (is_rest_subject(subject) || except.is_some())
            && to.to_ascii_lowercase().contains("hand") =>
        {
            Some(UnchosenObjectsAction::ReturnToHand)
        }
        InstructionAst::Sacrifice {
            subject, except, ..
        } if is_rest_subject(subject) || except.is_some() => Some(UnchosenObjectsAction::Sacrifice),
        InstructionAst::Discard { anaphora, .. }
            if anaphora.as_deref().is_some_and(|a| {
                let low = a.to_ascii_lowercase();
                low.contains("rest") || low.contains("other")
            }) =>
        {
            Some(UnchosenObjectsAction::Discard)
        }
        InstructionAst::TapPermanent { subject, .. } if is_rest_subject(subject) => {
            Some(UnchosenObjectsAction::Tap)
        }
        _ => None,
    }
}

fn lower_group_choose_spec(
    chooser: PlayerRefAst,
    count: &ValueExprAst,
    filter_ast: &spellwake_oracle_compiler::ast::ObjectFilterAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<
    (
        spellwake_engine::program::AffectedPlayers,
        spellwake_engine::program::SourceZoneFilter,
        ObjectFilter,
        ValueExpr,
        ValueExpr,
    ),
    &'static str,
> {
    use spellwake_engine::program::SourceZoneFilter;
    use spellwake_oracle_compiler::ast::{ControllerRelation, ZoneFilter};
    let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
    let mut normalized = filter_ast.clone();
    let (source_zone, stripped_ctrl) = if (normalized.flags.0
        & spellwake_oracle_compiler::ast::ObjectFilterFlags::EXILED_WITH_SELF.0)
        != 0
        && matches!(normalized.zone, None | Some(ZoneFilter::Exile))
        && (in_trigger || builder.has_source)
    {
        normalized.flags.0 &=
            !spellwake_oracle_compiler::ast::ObjectFilterFlags::EXILED_WITH_SELF.0;
        (SourceZoneFilter::LinkedExile, false)
    } else if strip_interactive_last_moved_predicates(&mut normalized, &builder.instructions) {
        (SourceZoneFilter::LastMoved, false)
    } else {
        match normalized.zone {
            Some(ZoneFilter::YourHand) if chooser == PlayerRefAst::You => {
                (SourceZoneFilter::Hand, false)
            }
            Some(ZoneFilter::ThatPlayerHand)
                if chooser == PlayerRefAst::You
                    && matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
            {
                (SourceZoneFilter::TargetPlayerHand, false)
            }
            Some(ZoneFilter::ThatPlayerHand) if chooser != PlayerRefAst::You => {
                (SourceZoneFilter::Hand, false)
            }
            Some(ZoneFilter::TargetPlayerHand) if chooser == PlayerRefAst::You => {
                builder.bind_target(BodyTarget::SinglePlayer)?;
                (SourceZoneFilter::TargetPlayerHand, false)
            }
            Some(ZoneFilter::TargetOpponentHand) if chooser == PlayerRefAst::You => {
                builder.bind_target(BodyTarget::SingleOpponent)?;
                (SourceZoneFilter::TargetPlayerHand, false)
            }
            Some(ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands) => {
                (SourceZoneFilter::OpponentHand, false)
            }
            Some(ZoneFilter::YourGraveyard) if chooser == PlayerRefAst::You => {
                (SourceZoneFilter::Graveyard, false)
            }
            Some(ZoneFilter::ThatPlayerGraveyard)
                if chooser == PlayerRefAst::You
                    && matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
            {
                (SourceZoneFilter::TargetPlayerGraveyard, false)
            }
            Some(ZoneFilter::ThatPlayerGraveyard) if chooser != PlayerRefAst::You => {
                (SourceZoneFilter::Graveyard, false)
            }
            Some(ZoneFilter::TargetPlayerGraveyard) if chooser == PlayerRefAst::You => {
                builder.bind_target(BodyTarget::SinglePlayer)?;
                (SourceZoneFilter::TargetPlayerGraveyard, false)
            }
            Some(ZoneFilter::TargetOpponentGraveyard) if chooser == PlayerRefAst::You => {
                builder.bind_target(BodyTarget::SingleOpponent)?;
                (SourceZoneFilter::TargetPlayerGraveyard, false)
            }
            Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                (SourceZoneFilter::OpponentGraveyard, false)
            }
            Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                (SourceZoneFilter::AnyGraveyard, false)
            }
            Some(ZoneFilter::CommandZone) if chooser == PlayerRefAst::You => {
                (SourceZoneFilter::CommandZone, false)
            }
            Some(ZoneFilter::Exile) => (SourceZoneFilter::Exile, false),
            None | Some(ZoneFilter::Battlefield) => {
                if (chooser == PlayerRefAst::You
                    && normalized.controller == Some(ControllerRelation::You))
                    || (chooser != PlayerRefAst::You
                        && normalized.controller == Some(ControllerRelation::ThatPlayer))
                {
                    (SourceZoneFilter::Battlefield, true)
                } else if chooser != PlayerRefAst::You
                    && normalized.controller == Some(ControllerRelation::You)
                {
                    return Err("oracle:unsupported-instruction");
                } else {
                    (SourceZoneFilter::AnyBattlefield, false)
                }
            }
            _ => return Err("oracle:unsupported-instruction"),
        }
    };
    normalized.zone = None;
    if stripped_ctrl {
        normalized.controller = None;
    }
    let filter = super::filters::lower_object_filter(&normalized, vocabulary)?;
    let amt =
        lower_value_expr_in_context(resolve_x(count, bound_x), builder, in_trigger, vocabulary)?;
    Ok((players, source_zone, filter, amt.clone(), amt))
}

fn creature_object_filter() -> ObjectFilter {
    let mut c = spellwake_engine::program::ObjectClause::any();
    c.any_types = spellwake_engine::program::CardTypeMask::CREATURE;
    ObjectFilter::Clause(c)
}

fn parse_interactive_choose_objects(
    chooser: PlayerRefAst,
    description: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Option<ChooseObjectsSpec> {
    use spellwake_engine::program::SourceZoneFilter;
    let raw = description
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if raw.contains("target")
        && !raw.contains("in target player's ")
        && !raw.contains("from target player's ")
        && !raw.contains("in target opponent's ")
        && !raw.contains("from target opponent's ")
    {
        return None;
    }
    let players = lower_affected_players_in_context(chooser, builder, in_trigger).ok()?;

    // Revealed target player's hand choice (for example Brain Maggot, Kitesail Freebooter):
    // immediately after `RevealPlayerHand(TargetPlayer)`, "a nonland card from it [with ...]"
    // lowers to `ChooseObjects` over `SourceZoneFilter::TargetPlayerHand`.
    if chooser == PlayerRefAst::You
        && matches!(
            builder.instructions.last(),
            Some(Instruction::ZoneOp(spec))
                                if matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::RevealOnly)
                                    && matches!(
                                        &spec.source,
                                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                            zone: spellwake_engine::toolbox::QueryZone::Hand(ps),
                                            ..
                                        } if matches!(
                                            ps.as_ref(),
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetPlayer
                                            )
                                        )
                                    )
        )
        && let Some(core) = strip_revealed_hand_ref(&raw)
    {
        let (min_val, max_val, flt_phrase) = parse_choose_objects_count_prefix(&core)?;
        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
            flt_phrase,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        if parsed.zone.is_some()
            || parsed.controller.is_some()
            || parsed.owner.is_some()
            || parsed.named.is_some()
        {
            return None;
        }
        let filter = super::filters::lower_object_filter(&parsed, vocabulary).ok()?;
        return Some(ChooseObjectsSpec {
            players,
            source_zone: SourceZoneFilter::TargetPlayerHand,
            filter,
            min: ValueExpr::literal(min_val),
            max: ValueExpr::literal(max_val),
            distinct_powers: false,
            max_total_power: None,
        });
    }
    if raw.contains("from it") || raw.contains("from them") {
        return None;
    }

    // Special constrained forms with exact rules semantics:
    // 1. "any number of creatures with different powers" (Sigarda's Vanguard, Sigardian Zealot)
    if raw == "any number of creatures with different powers" {
        return Some(ChooseObjectsSpec {
            players,
            source_zone: SourceZoneFilter::AnyBattlefield,
            filter: creature_object_filter(),
            min: ValueExpr::literal(0),
            max: ValueExpr::literal(99),
            distinct_powers: true,
            max_total_power: None,
        });
    }
    // 2. "any number of creatures they control with total power N or less" (Slaughter the Strong, Destined Confrontation)
    if let Some(rest) = raw
        .strip_prefix("any number of creatures they control with total power ")
        .or_else(|| raw.strip_prefix("any number of creatures you control with total power "))
        && let Some(num_str) = rest.strip_suffix(" or less")
        && let Ok(limit) = num_str.trim().parse::<i64>()
    {
        return Some(ChooseObjectsSpec {
            players,
            source_zone: SourceZoneFilter::Battlefield,
            filter: creature_object_filter(),
            min: ValueExpr::literal(0),
            max: ValueExpr::literal(99),
            distinct_powers: false,
            max_total_power: Some(ValueExpr::literal(limit)),
        });
    }

    // 3. Hand / Graveyard single-card choices
    if matches!(raw.as_str(), "a card in your hand" | "a card in their hand") {
        return Some(ChooseObjectsSpec {
            players,
            source_zone: SourceZoneFilter::Hand,
            filter: ObjectFilter::any(),
            min: ValueExpr::literal(1),
            max: ValueExpr::literal(1),
            distinct_powers: false,
            max_total_power: None,
        });
    }
    if matches!(
        raw.as_str(),
        "a card in your graveyard" | "a card in their graveyard"
    ) {
        return Some(ChooseObjectsSpec {
            players,
            source_zone: SourceZoneFilter::Graveyard,
            filter: ObjectFilter::any(),
            min: ValueExpr::literal(1),
            max: ValueExpr::literal(1),
            distinct_powers: false,
            max_total_power: None,
        });
    }

    // 4. Generic non-target object filter choices across battlefield, hand, graveyard, exile,
    //    command zone, linked exile, and recently moved objects.
    let (min_val, max_val, body_phrase) = parse_choose_objects_count_prefix(&raw)?;

    let mut req_target: Option<BodyTarget> = None;
    let (source_zone, filter_phrase) = if let Some(p) =
        body_phrase.strip_suffix(" exiled this way").filter(|_| {
            has_live_interactive_last_moved(&builder.instructions, InteractiveLastMovedKind::Exile)
        }) {
        (SourceZoneFilter::LastMoved, p.trim())
    } else if let Some(p) = body_phrase.strip_suffix(" milled this way").filter(|_| {
        has_live_interactive_last_moved(&builder.instructions, InteractiveLastMovedKind::Mill)
    }) {
        (SourceZoneFilter::LastMoved, p.trim())
    } else if let Some(p) = body_phrase.strip_suffix(" discarded this way").filter(|_| {
        has_live_interactive_last_moved(&builder.instructions, InteractiveLastMovedKind::Discard)
    }) {
        (SourceZoneFilter::LastMoved, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" from among them")
        .or_else(|| body_phrase.strip_suffix(" from among those cards"))
        .filter(|_| {
            has_live_interactive_last_moved(&builder.instructions, InteractiveLastMovedKind::Any)
        })
    {
        (SourceZoneFilter::LastMoved, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" exiled with ~")
        .or_else(|| body_phrase.strip_suffix(" exiled with this creature"))
        .or_else(|| body_phrase.strip_suffix(" exiled with this artifact"))
        .or_else(|| body_phrase.strip_suffix(" exiled with this enchantment"))
        .or_else(|| body_phrase.strip_suffix(" exiled with this permanent"))
        .filter(|_| in_trigger || builder.has_source)
    {
        (SourceZoneFilter::LinkedExile, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in your graveyard")
        .or_else(|| body_phrase.strip_suffix(" from your graveyard"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        (SourceZoneFilter::Graveyard, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in their graveyard")
        .or_else(|| body_phrase.strip_suffix(" from their graveyard"))
        .or_else(|| body_phrase.strip_suffix(" in that player's graveyard"))
        .or_else(|| body_phrase.strip_suffix(" from that player's graveyard"))
    {
        if chooser == PlayerRefAst::You {
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) {
                (SourceZoneFilter::TargetPlayerGraveyard, p.trim())
            } else {
                return None;
            }
        } else {
            (SourceZoneFilter::Graveyard, p.trim())
        }
    } else if let Some(p) = body_phrase
        .strip_suffix(" in target player's graveyard")
        .or_else(|| body_phrase.strip_suffix(" from target player's graveyard"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        req_target = Some(BodyTarget::SinglePlayer);
        (SourceZoneFilter::TargetPlayerGraveyard, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in target opponent's graveyard")
        .or_else(|| body_phrase.strip_suffix(" from target opponent's graveyard"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        req_target = Some(BodyTarget::SingleOpponent);
        (SourceZoneFilter::TargetPlayerGraveyard, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in an opponent's graveyard")
        .or_else(|| body_phrase.strip_suffix(" from an opponent's graveyard"))
        .or_else(|| body_phrase.strip_suffix(" in opponents' graveyards"))
        .or_else(|| body_phrase.strip_suffix(" from opponents' graveyards"))
    {
        (SourceZoneFilter::OpponentGraveyard, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in a graveyard")
        .or_else(|| body_phrase.strip_suffix(" in any graveyard"))
        .or_else(|| body_phrase.strip_suffix(" from a graveyard"))
        .or_else(|| body_phrase.strip_suffix(" from any graveyard"))
    {
        (SourceZoneFilter::AnyGraveyard, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in your hand")
        .or_else(|| body_phrase.strip_suffix(" from your hand"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        (SourceZoneFilter::Hand, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in their hand")
        .or_else(|| body_phrase.strip_suffix(" from their hand"))
        .or_else(|| body_phrase.strip_suffix(" in that player's hand"))
        .or_else(|| body_phrase.strip_suffix(" from that player's hand"))
    {
        if chooser == PlayerRefAst::You {
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) {
                (SourceZoneFilter::TargetPlayerHand, p.trim())
            } else {
                return None;
            }
        } else {
            (SourceZoneFilter::Hand, p.trim())
        }
    } else if let Some(p) = body_phrase
        .strip_suffix(" in target player's hand")
        .or_else(|| body_phrase.strip_suffix(" from target player's hand"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        req_target = Some(BodyTarget::SinglePlayer);
        (SourceZoneFilter::TargetPlayerHand, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in target opponent's hand")
        .or_else(|| body_phrase.strip_suffix(" from target opponent's hand"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        req_target = Some(BodyTarget::SingleOpponent);
        (SourceZoneFilter::TargetPlayerHand, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in an opponent's hand")
        .or_else(|| body_phrase.strip_suffix(" from an opponent's hand"))
    {
        (SourceZoneFilter::OpponentHand, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in the command zone")
        .or_else(|| body_phrase.strip_suffix(" from the command zone"))
        .or_else(|| body_phrase.strip_suffix(" in your command zone"))
        .or_else(|| body_phrase.strip_suffix(" from your command zone"))
    {
        if chooser != PlayerRefAst::You {
            return None;
        }
        (SourceZoneFilter::CommandZone, p.trim())
    } else if let Some(p) = body_phrase
        .strip_suffix(" in exile")
        .or_else(|| body_phrase.strip_suffix(" from exile"))
    {
        (SourceZoneFilter::Exile, p.trim())
    } else if let Some(p) = body_phrase.strip_suffix(" on the battlefield") {
        (SourceZoneFilter::AnyBattlefield, p.trim())
    } else if let Some(p) = body_phrase.strip_suffix(" they control") {
        if chooser == PlayerRefAst::You {
            return None;
        }
        (SourceZoneFilter::Battlefield, p.trim())
    } else if let Some(p) = body_phrase.strip_suffix(" you control") {
        if chooser != PlayerRefAst::You {
            return None;
        }
        (SourceZoneFilter::Battlefield, p.trim())
    } else if body_phrase.ends_with(" that player controls") {
        // When controller chooses a creature "that player controls" inside ForEachPlayer,
        // `ThatPlayer` maps to LoopPlayer/ChosenPlayer in `lower_object_filter`.
        (SourceZoneFilter::AnyBattlefield, body_phrase.trim())
    } else if body_phrase.contains(" in ") || body_phrase.contains(" from ") {
        return None;
    } else {
        (SourceZoneFilter::AnyBattlefield, body_phrase.trim())
    };
    if filter_phrase.is_empty() {
        return None;
    }
    let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
        filter_phrase,
        &cached_compiler_vocab(vocabulary),
    )
    .ok()?;
    let mut normalized = parsed;
    if source_zone == SourceZoneFilter::Battlefield {
        normalized.controller = None;
    }
    if normalized.zone == Some(spellwake_oracle_compiler::ast::ZoneFilter::Battlefield)
        && matches!(
            source_zone,
            SourceZoneFilter::Battlefield | SourceZoneFilter::AnyBattlefield
        )
    {
        normalized.zone = None;
    }
    if normalized.zone.is_some() {
        return None;
    }
    if chooser != PlayerRefAst::You
        && normalized.controller == Some(spellwake_oracle_compiler::ast::ControllerRelation::You)
    {
        return None;
    }
    if let Some(bt) = req_target {
        builder.bind_target(bt).ok()?;
    }
    let filter = super::filters::lower_object_filter(&normalized, vocabulary).ok()?;
    Some(ChooseObjectsSpec {
        players,
        source_zone,
        filter,
        min: ValueExpr::literal(min_val),
        max: ValueExpr::literal(max_val),
        distinct_powers: false,
        max_total_power: None,
    })
}

fn parse_choose_objects_count_prefix(raw: &str) -> Option<(i64, i64, &str)> {
    if let Some(rest) = raw.strip_prefix("any number of ") {
        Some((0, 99, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to one ")
        .or_else(|| raw.strip_prefix("up to 1 "))
    {
        Some((0, 1, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to two ")
        .or_else(|| raw.strip_prefix("up to 2 "))
    {
        Some((0, 2, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to three ")
        .or_else(|| raw.strip_prefix("up to 3 "))
    {
        Some((0, 3, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to four ")
        .or_else(|| raw.strip_prefix("up to 4 "))
    {
        Some((0, 4, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to five ")
        .or_else(|| raw.strip_prefix("up to 5 "))
    {
        Some((0, 5, rest))
    } else if let Some(rest) = raw
        .strip_prefix("up to six ")
        .or_else(|| raw.strip_prefix("up to 6 "))
    {
        Some((0, 6, rest))
    } else if let Some(rest) = raw.strip_prefix("six ").or_else(|| raw.strip_prefix("6 ")) {
        Some((6, 6, rest))
    } else if let Some(rest) = raw.strip_prefix("five ").or_else(|| raw.strip_prefix("5 ")) {
        Some((5, 5, rest))
    } else if let Some(rest) = raw.strip_prefix("four ").or_else(|| raw.strip_prefix("4 ")) {
        Some((4, 4, rest))
    } else if let Some(rest) = raw
        .strip_prefix("three ")
        .or_else(|| raw.strip_prefix("3 "))
    {
        Some((3, 3, rest))
    } else if let Some(rest) = raw.strip_prefix("two ").or_else(|| raw.strip_prefix("2 ")) {
        Some((2, 2, rest))
    } else if let Some(rest) = raw.strip_prefix("one ").or_else(|| raw.strip_prefix("1 ")) {
        Some((1, 1, rest))
    } else if let Some(rest) = raw.strip_prefix("an ") {
        Some((1, 1, rest))
    } else {
        raw.strip_prefix("a ").map(|rest| (1, 1, rest))
    }
}

#[derive(Clone, Copy)]
enum InteractiveLastMovedKind {
    Any,
    Exile,
    Mill,
    Discard,
}

fn has_live_interactive_last_moved(
    ops: &[Instruction],
    expected: InteractiveLastMovedKind,
) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::Mill { .. } => {
                return matches!(
                    expected,
                    InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Mill
                );
            }
            Instruction::ExileTopPlay { .. } | Instruction::ExileTopForPlay { .. } => {
                return matches!(
                    expected,
                    InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Exile
                );
            }
            Instruction::Discard { .. } => {
                return matches!(
                    expected,
                    InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Discard
                );
            }
            Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
                if crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                    .is_some() =>
            {
                let spec =
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                        .unwrap();
                return match &spec.destination {
                    spellwake_engine::toolbox::ZoneDest::Exile { .. } => matches!(
                        expected,
                        InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Exile
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                    } => matches!(
                        expected,
                        InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Discard
                    ),
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Mill,
                    } => matches!(
                        expected,
                        InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Mill
                    ),
                    spellwake_engine::toolbox::ZoneDest::RevealOnly
                    | spellwake_engine::toolbox::ZoneDest::None => continue,
                    _ => matches!(expected, InteractiveLastMovedKind::Any),
                };
            }
            Instruction::LookTopChooseExt { keep_dest, .. }
            | Instruction::RevealUntilMatchExt {
                match_dest: keep_dest,
                ..
            } => {
                return match keep_dest {
                    spellwake_engine::program::LookTopKeepDest::Exile { .. }
                    | spellwake_engine::program::LookTopKeepDest::ExileLinked { .. } => matches!(
                        expected,
                        InteractiveLastMovedKind::Any | InteractiveLastMovedKind::Exile
                    ),
                    _ => matches!(expected, InteractiveLastMovedKind::Any),
                };
            }

            Instruction::ChooseObjects { .. }
            | Instruction::SearchLibrary { .. }
            | Instruction::ManifestTop { .. }
            | Instruction::ManifestDread => {
                return matches!(expected, InteractiveLastMovedKind::Any);
            }
            Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::ApplyContinuous(_)
            | Instruction::AdjustLife(_)
            | Instruction::AddMana(_)
            | Instruction::AddManaForPlayer { .. } => continue,
            _ => return false,
        }
    }
    false
}

fn strip_interactive_last_moved_predicates(
    filter: &mut spellwake_oracle_compiler::ast::ObjectFilterAst,
    ops: &[Instruction],
) -> bool {
    use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind, ZoneFilter};
    if matches!(
        filter.zone,
        Some(
            ZoneFilter::OpponentHand
                | ZoneFilter::OpponentsHands
                | ZoneFilter::TargetOpponentHand
                | ZoneFilter::TargetPlayerHand
        )
    ) {
        return false;
    }
    let classify = |pred: &FilterPredicateAst| -> Option<InteractiveLastMovedKind> {
        match pred {
            FilterPredicateAst::PriorActionZoneRef { kind } => {
                match kind.trim().to_ascii_lowercase().as_str() {
                    "from_among_those"
                    | "from_among_those_cards"
                    | "among_them"
                    | "among them"
                    | "from_among_them"
                    | "from among them"
                    | "among_those"
                    | "among those"
                    | "among_those_cards"
                    | "among those cards" => Some(InteractiveLastMovedKind::Any),
                    "milled_this_way"
                    | "cards_milled_this_way"
                    | "among_the_cards_milled_this_way"
                    | "among_cards_milled_this_way"
                    | "from_among_the_cards_milled_this_way"
                    | "from_among_cards_milled_this_way" => Some(InteractiveLastMovedKind::Mill),
                    "exiled_this_way"
                    | "cards_exiled_this_way"
                    | "among_the_cards_exiled_this_way"
                    | "among_cards_exiled_this_way"
                    | "from_among_the_cards_exiled_this_way"
                    | "from_among_cards_exiled_this_way"
                    | "from_among_those_cards_exiled_this_way" => {
                        Some(InteractiveLastMovedKind::Exile)
                    }
                    "discarded_this_way"
                    | "among_the_cards_discarded_this_way"
                    | "among_cards_discarded_this_way"
                    | "from_among_the_cards_discarded_this_way"
                    | "from_among_cards_discarded_this_way" => {
                        Some(InteractiveLastMovedKind::Discard)
                    }
                    _ => None,
                }
            }
            FilterPredicateAst::AffectedByPriorAction(PriorActionRefKind::ExiledThisWay) => {
                Some(InteractiveLastMovedKind::Exile)
            }
            FilterPredicateAst::AffectedByPriorAction(PriorActionRefKind::DiscardedThisWay) => {
                Some(InteractiveLastMovedKind::Discard)
            }
            FilterPredicateAst::StateQualification(raw) => {
                match raw.trim().to_ascii_lowercase().as_str() {
                    "exiled this way" => Some(InteractiveLastMovedKind::Exile),
                    "milled this way" => Some(InteractiveLastMovedKind::Mill),
                    "discarded this way" => Some(InteractiveLastMovedKind::Discard),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    let mut matched = false;
    for pred in &filter.predicates {
        if let Some(kind) = classify(pred) {
            if !has_live_interactive_last_moved(ops, kind) {
                return false;
            }
            matched = true;
        }
    }
    if !matched {
        return false;
    }
    filter.predicates.retain(|p| classify(p).is_none());
    filter.zone = None;
    true
}

/// Lower `InstructionAst::PlayerChooseOption { chooser, options }` into `ChooseValue` /
/// `ChooseOptionForPlayers` / `ChooseCreatureType`.
pub(super) fn lower_player_choose_option(
    chooser: PlayerRefAst,
    options: &[String],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<(), &'static str> {
    if options.is_empty() {
        return Err("oracle:unsupported-choose-option");
    }
    if options.len() == 1 {
        let cleaned = options[0].trim().trim_end_matches('.').to_ascii_lowercase();
        let cleaned = cleaned.strip_prefix("choose ").unwrap_or(&cleaned).trim();
        return try_lower_choose_option_ext(chooser, cleaned, builder, vocabulary, ids, in_trigger);
    }
    if options.len() <= 20 {
        let domain =
            spellwake_engine::program::ResolutionChoiceDomain::LabeledOption(options.to_vec());
        return emit_domain_for_player_ref(chooser, domain, builder, in_trigger);
    }
    Err("oracle:unsupported-choose-option")
}

/// Extended fallback for `InstructionAst::ChooseOption` and `PlayerChooseOption`.
pub(super) fn try_lower_choose_option_ext(
    chooser: PlayerRefAst,
    cleaned: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::program::ResolutionChoiceDomain;
    use spellwake_engine::toolbox::{
        BindingId, ChoiceCategory, PlayerSelector, PromptChoiceSpec, PromptDomain,
        SubtypeChoiceKind, ZoneDest, ZoneOpSpec, ZoneSelection,
    };

    let eff_cleaned = cleaned
        .trim()
        .strip_prefix("secretly choose ")
        .or_else(|| cleaned.trim().strip_prefix("secretly "))
        .unwrap_or_else(|| cleaned.trim())
        .strip_prefix("either ")
        .unwrap_or_else(|| {
            cleaned
                .trim()
                .strip_prefix("secretly choose ")
                .or_else(|| cleaned.trim().strip_prefix("secretly "))
                .unwrap_or_else(|| cleaned.trim())
        })
        .trim();

    if let Ok(kind) = parse_choice_kind(eff_cleaned, true, vocabulary) {
        return emit_choice_for_player_ref(chooser, kind, builder, ids, in_trigger);
    }
    if matches!(
        eff_cleaned,
        "one of them" | "one of those cards" | "a card from among them"
    ) && matches!(
        builder.instructions.last(),
        Some(
            Instruction::ZoneOp(_)
                | Instruction::ForEachPlayerQuery { .. }
                | Instruction::ExileTopPlay { .. }
                | Instruction::LookTopChooseExt { .. }
                | Instruction::RevealUntilMatchExt { .. }
                | Instruction::ChooseObjects { .. }
                | Instruction::Mill { .. }
        )
    ) && let Ok(chooser_rp) = lower_resolving_player_in_context(chooser, builder, in_trigger)
    {
        builder.instructions.push(Instruction::ChooseOneLastMoved {
            chooser: chooser_rp,
        });
        return Ok(());
    }

    // Universal PromptChoice paths for subtype/multi-color/per-player creature type choices.
    if eff_cleaned == "a creature type"
        && matches!(
            chooser,
            PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer
        )
    {
        const SEAT_BIND: BindingId = BindingId(920);
        const CHOICE_BIND: BindingId = BindingId(921);
        let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
        let prompt = Instruction::PromptChoice(Box::new(PromptChoiceSpec {
            chooser: PlayerSelector::Bound(SEAT_BIND),
            domain: PromptDomain::Subtype {
                kind: SubtypeChoiceKind::Creature,
            },
            category: ChoiceCategory::Generic,
            bind_to: CHOICE_BIND,
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
        }));
        builder.instructions.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(PlayerSelector::Group(players)),
            bind_player: SEAT_BIND,
            body: Box::new(ExecutableBody::new(BodyTarget::None, vec![prompt])),
        });
        return Ok(());
    }
    if matches!(
        eff_cleaned,
        "a planeswalker type" | "a planeswalker type of your choice"
    ) {
        const CHOICE_BIND: BindingId = BindingId(922);
        let rp = lower_resolving_player_in_context(chooser, builder, in_trigger)?;
        builder
            .instructions
            .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: PlayerSelector::Resolving(rp),
                domain: PromptDomain::Subtype {
                    kind: SubtypeChoiceKind::Planeswalker,
                },
                category: ChoiceCategory::Generic,
                bind_to: CHOICE_BIND,
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
            })));
        return Ok(());
    }
    if matches!(
        eff_cleaned,
        "two colors" | "two different colors" | "two colors of your choice"
    ) {
        const CHOICE_BIND: BindingId = BindingId(923);
        let rp = lower_resolving_player_in_context(chooser, builder, in_trigger)?;
        builder
            .instructions
            .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: PlayerSelector::Resolving(rp),
                domain: PromptDomain::Options {
                    labels: vec![
                        "White".to_owned(),
                        "Blue".to_owned(),
                        "Black".to_owned(),
                        "Red".to_owned(),
                        "Green".to_owned(),
                    ],
                    min: 2,
                    max: 2,
                    allow_repeat: false,
                },
                category: ChoiceCategory::Generic,
                bind_to: CHOICE_BIND,
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
            })));
        return Ok(());
    }

    let domain = match eff_cleaned {
        "an opponent at random" | "opponent at random" => ResolutionChoiceDomain::RandomPlayer {
            opponents_only: true,
        },
        "a player at random" | "player at random" => ResolutionChoiceDomain::RandomPlayer {
            opponents_only: false,
        },
        "an opponent" | "opponent" | "another opponent" => ResolutionChoiceDomain::Player {
            opponents_only: true,
        },
        "a player" | "player" | "another player" => ResolutionChoiceDomain::Player {
            opponents_only: false,
        },
        "a number" | "number" | "a number 0 or greater" | "any number" | "0 or more" => {
            ResolutionChoiceDomain::Number { min: 0, max: 20 }
        }
        "a number greater than 0" | "a positive number" | "a number 1 or greater" => {
            ResolutionChoiceDomain::Number { min: 1, max: 20 }
        }
        "1, 2, or 3" => ResolutionChoiceDomain::Number { min: 1, max: 3 },
        "a color of mana" | "any one color" | "one color" => ResolutionChoiceDomain::Color,
        "a permanent type" => ResolutionChoiceDomain::CardType(vec![
            CardType::Artifact,
            CardType::Battle,
            CardType::Creature,
            CardType::Enchantment,
            CardType::Land,
            CardType::Planeswalker,
        ]),
        "a nonland card type" => ResolutionChoiceDomain::CardType(vec![
            CardType::Artifact,
            CardType::Battle,
            CardType::Creature,
            CardType::Enchantment,
            CardType::Instant,
            CardType::Kindred,
            CardType::Planeswalker,
            CardType::Sorcery,
        ]),
        "a noncreature card type" => ResolutionChoiceDomain::CardType(vec![
            CardType::Artifact,
            CardType::Battle,
            CardType::Enchantment,
            CardType::Instant,
            CardType::Kindred,
            CardType::Land,
            CardType::Planeswalker,
            CardType::Sorcery,
        ]),
        "the name of a card" => ResolutionChoiceDomain::CardName {
            nonland_only: false,
        },
        "a creature card name"
        | "a card name other than a basic land card name"
        | "a nonbasic land card name"
        | "an instant or sorcery card name"
        | "a noncreature card name" => ResolutionChoiceDomain::CardName { nonland_only: true },
        "a land type" => ResolutionChoiceDomain::BasicLandType,
        "land or nonland" => {
            ResolutionChoiceDomain::LabeledOption(vec!["land".to_owned(), "nonland".to_owned()])
        }
        "friend or foe" => {
            ResolutionChoiceDomain::LabeledOption(vec!["friend".to_owned(), "foe".to_owned()])
        }
        "choose left or right" | "left or right" => {
            ResolutionChoiceDomain::LabeledOption(vec!["left".to_owned(), "right".to_owned()])
        }
        other => {
            if chooser == PlayerRefAst::You
                && (other.starts_with("target ")
                    || other
                        .strip_prefix("another ")
                        .is_some_and(|rest| rest.starts_with("target ")))
            {
                let (is_another, target_raw) = match other.strip_prefix("another ") {
                    Some(rest) => (true, rest),
                    None => (false, other),
                };
                if let Ok(mut subj) = spellwake_oracle_compiler::filter::parse_subject_or_target(
                    target_raw,
                    &cached_compiler_vocab(vocabulary),
                ) {
                    if is_another && let SubjectRefAst::TargetObject { filter, .. } = &mut subj {
                        filter.flags = filter
                            .flags
                            .union(spellwake_oracle_compiler::ast::ObjectFilterFlags::EXCLUDE_SELF);
                    }
                    if let Ok(bt) = lower_target_subject(&subj, vocabulary) {
                        builder.bind_target(bt)?;
                        return Ok(());
                    }
                }
            }
            if let Some(spec) =
                parse_interactive_choose_objects(chooser, other, builder, vocabulary, in_trigger)
            {
                if spec.distinct_powers || spec.max_total_power.is_some() {
                    builder.instructions.push(Instruction::ChooseObjects {
                        players: spec.players,
                        source_zone: spec.source_zone,
                        filter: Box::new(spec.filter),
                        min: spec.min,
                        max: spec.max,
                        distinct_powers: spec.distinct_powers,
                        max_total_power: spec.max_total_power,
                        unchosen_action: None,
                    });
                } else if let AffectedPlayers::One(rp) = spec.players {
                    let query = super::toolbox_emit::source_zone_query(
                        spec.players,
                        spec.source_zone,
                        spec.filter,
                    );
                    let hidden = matches!(
                        spec.source_zone,
                        SourceZoneFilter::Hand
                            | SourceZoneFilter::TargetPlayerHand
                            | SourceZoneFilter::OpponentHand
                            | SourceZoneFilter::YourLibrary
                            | SourceZoneFilter::HandOrGraveyard
                    );
                    builder
                        .instructions
                        .push(super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                            query,
                            ZoneSelection::Prompt {
                                chooser: PlayerSelector::Resolving(rp),
                                min: Box::new(spec.min),
                                max: Box::new(spec.max),
                                constraint: None,
                                category: ChoiceCategory::Generic,
                                hidden,
                            },
                            ZoneDest::None,
                        )));
                } else {
                    const SEAT_BIND: BindingId = BindingId(924);
                    let seat_p = || Box::new(PlayerSelector::Bound(SEAT_BIND));
                    let query = match spec.source_zone {
                        SourceZoneFilter::Hand => spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Hand(seat_p()),
                            spec.filter,
                        ),
                        SourceZoneFilter::Graveyard => {
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Graveyard(seat_p()),
                                spec.filter,
                            )
                        }
                        SourceZoneFilter::HandOrGraveyard => {
                            spellwake_engine::toolbox::ObjectQuery::Union(vec![
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Hand(seat_p()),
                                    spec.filter,
                                ),
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(seat_p()),
                                    spec.filter,
                                ),
                            ])
                        }
                        SourceZoneFilter::Battlefield => {
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                    seat_p(),
                                ),
                                spec.filter,
                            )
                        }
                        other => {
                            super::toolbox_emit::source_zone_query(spec.players, other, spec.filter)
                        }
                    };
                    let hidden = matches!(
                        spec.source_zone,
                        SourceZoneFilter::Hand
                            | SourceZoneFilter::TargetPlayerHand
                            | SourceZoneFilter::OpponentHand
                            | SourceZoneFilter::YourLibrary
                            | SourceZoneFilter::HandOrGraveyard
                    );
                    let inner_op = super::toolbox_emit::zone_op(ZoneOpSpec::simple(
                        query,
                        ZoneSelection::Prompt {
                            chooser: PlayerSelector::Bound(SEAT_BIND),
                            min: Box::new(spec.min),
                            max: Box::new(spec.max),
                            constraint: None,
                            category: ChoiceCategory::Generic,
                            hidden,
                        },
                        ZoneDest::None,
                    ));
                    builder.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(PlayerSelector::Group(spec.players)),
                        bind_player: SEAT_BIND,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![inner_op])),
                    });
                }
                return Ok(());
            }
            return Err("oracle:unsupported-choose-option");
        }
    };
    emit_domain_for_player_ref(chooser, domain, builder, in_trigger)
}

/// Extended fallback for `InstructionAst::InteractiveChoice(ChooseFromGroup { chooser, description })`.
pub(super) fn try_lower_interactive_choose_from_group(
    chooser: PlayerRefAst,
    cleaned: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<(), &'static str> {
    if chooser == PlayerRefAst::You
        && (cleaned.starts_with("target ")
            || cleaned.starts_with("another target ")
            || cleaned.starts_with("any number of target ")
            || cleaned.starts_with("two target ")
            || cleaned.starts_with("up to two target ")
            || cleaned.starts_with("up to three target "))
    {
        let (is_another, target_raw) = match cleaned.strip_prefix("another ") {
            Some(rest) => (true, rest),
            None => (false, cleaned),
        };
        if let Ok(mut subj) = spellwake_oracle_compiler::filter::parse_subject_or_target(
            target_raw,
            &cached_compiler_vocab(vocabulary),
        ) {
            if is_another && let SubjectRefAst::TargetObject { filter, .. } = &mut subj {
                filter.flags = filter
                    .flags
                    .union(spellwake_oracle_compiler::ast::ObjectFilterFlags::EXCLUDE_SELF);
            }
            if let Ok(bt) = lower_target_subject(&subj, vocabulary) {
                builder.bind_target(bt)?;
                return Ok(());
            }
        }
    }
    if let Some(spec) =
        parse_interactive_choose_objects(chooser, cleaned, builder, vocabulary, in_trigger)
    {
        builder.instructions.push(Instruction::ChooseObjects {
            players: spec.players,
            source_zone: spec.source_zone,
            filter: Box::new(spec.filter),
            min: spec.min,
            max: spec.max,
            distinct_powers: spec.distinct_powers,
            max_total_power: spec.max_total_power,
            unchosen_action: None,
        });
        return Ok(());
    }
    try_lower_choose_option_ext(chooser, cleaned, builder, vocabulary, ids, in_trigger)
        .map_err(|_| "oracle:unsupported-instruction")
}

fn emit_domain_for_player_ref(
    chooser: PlayerRefAst,
    domain: spellwake_engine::program::ResolutionChoiceDomain,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(), &'static str> {
    match chooser {
        PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer => {
            let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
            push_for_each_player(
                players,
                vec![Instruction::ChooseValue {
                    chooser: ResolvingPlayer::LoopPlayer,
                    domain,
                }],
                builder,
            );
            Ok(())
        }
        PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer => {
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                    opponents_only: chooser == PlayerRefAst::AnyOpponent,
                },
            });
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::ChosenPlayer,
                domain,
            });
            Ok(())
        }
        other => {
            let rp = lower_resolving_player_in_context(other, builder, in_trigger)?;
            builder.instructions.push(Instruction::ChooseValue {
                chooser: rp,
                domain,
            });
            Ok(())
        }
    }
}

/// Check whether an `ObjectFilterAst` has only the explicitly permitted fields set for a
/// damage-source prevention filter (fail-closed on any predicate, subtype, stat bound, etc.).
fn prevention_filter_base_clean(f: &ObjectFilterAst) -> bool {
    f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.owner.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.stat_bounds.is_empty()
        && f.counter_kind.is_none()
        && f.named.is_none()
        && f.extra_zones.is_empty()
        && !f.is_ability
        && !f.any_spell
        && matches!(f.zone, None | Some(ZoneFilter::Battlefield))
}

fn parse_color_sources_anaphora(raw: &str) -> Option<spellwake_engine::program::ColorMask> {
    use spellwake_engine::program::ColorMask;
    let body = raw
        .trim()
        .strip_prefix("that ")
        .unwrap_or(raw.trim())
        .trim();
    let mut mask = ColorMask::EMPTY;
    let mut count = 0usize;
    let parts: Vec<&str> = if body.contains(" and/or ") {
        body.split(" and/or ").collect()
    } else if body.contains(" and ") {
        body.split(" and ").collect()
    } else {
        return None;
    };
    if parts.len() < 2 {
        return None;
    }
    for (idx, part) in parts.iter().enumerate() {
        let p = part.trim();
        let color_word = if let Some(c) = p.strip_suffix(" sources") {
            c.trim()
        } else if idx + 1 < parts.len() {
            // Form "black and/or red sources": earlier terms omit "sources", final term must have it.
            p
        } else {
            return None;
        };
        let bit = match color_word {
            "white" => ColorMask::WHITE,
            "blue" => ColorMask::BLUE,
            "black" => ColorMask::BLACK,
            "red" => ColorMask::RED,
            "green" => ColorMask::GREEN,
            _ => return None,
        };
        mask = mask.union(bit);
        count += 1;
    }
    (count >= 2 && !mask.is_empty()).then_some(mask)
}

fn lower_prevention_source_filter_single(
    f: &ObjectFilterAst,
) -> Option<spellwake_engine::damage::PreventionSourceFilter> {
    use spellwake_engine::damage::PreventionSourceFilter;
    if !prevention_filter_base_clean(f)
        || !f.predicates.is_empty()
        || !f.flags.is_empty()
        || f.required_keywords.len() > 1
        || f.forbidden_keywords.len() > 1
    {
        return None;
    }
    let creatures_only = if f.any_types.is_empty() {
        false
    } else if f.any_types == AstCardTypeMask::CREATURE {
        true
    } else {
        return None;
    };
    let colorless_only = match f.color_arity {
        None => false,
        Some(ColorArity::Colorless) => true,
        Some(_) => return None,
    };
    let (attacking_only, blocking_only, unblocked_only) = match f.combat {
        None => (false, false, false),
        Some(CombatState::Attacking) => (true, false, false),
        Some(CombatState::Blocking) => (false, true, false),
        Some(CombatState::Unblocked) => (false, false, true),
        Some(_) => return None,
    };
    let required_keyword = match f.required_keywords.first() {
        None => None,
        Some(name) => Some(lower_keyword_ability_name(name)?),
    };
    let forbidden_keyword = match f.forbidden_keywords.first() {
        None => None,
        Some(name) => Some(lower_keyword_ability_name(name)?),
    };
    let controller = match f.controller {
        None => ControllerFilter::Any,
        Some(ControllerRelation::You) => ControllerFilter::You,
        Some(ControllerRelation::Opponent) => ControllerFilter::Opponent,
        Some(ControllerRelation::NotYou) => ControllerFilter::NotYou,
        Some(_) => return None,
    };
    Some(PreventionSourceFilter {
        creatures_only,
        attacking_only,
        blocking_only,
        unblocked_only,
        any_colors: lower_color_mask(f.any_colors),
        forbidden_colors: lower_color_mask(f.forbidden_colors),
        colorless_only,
        required_keyword,
        forbidden_keyword,
        controller,
    })
}

fn is_plain_controlled_creatures_filter(f: &ObjectFilterAst) -> bool {
    prevention_filter_base_clean(f)
        && f.predicates.is_empty()
        && f.flags.is_empty()
        && f.any_types == AstCardTypeMask::CREATURE
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.combat.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.controller == Some(ControllerRelation::You)
}

fn is_plain_all_creatures_filter(f: &ObjectFilterAst) -> bool {
    prevention_filter_base_clean(f)
        && f.predicates.is_empty()
        && f.flags.is_empty()
        && f.any_types == AstCardTypeMask::CREATURE
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.combat.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.controller.is_none()
}

fn is_plain_controlled_permanents_filter(f: &ObjectFilterAst) -> bool {
    prevention_filter_base_clean(f)
        && f.predicates.is_empty()
        && f.flags.is_empty()
        && f.any_types.is_empty()
        && f.any_permanent
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.combat.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.controller == Some(ControllerRelation::You)
}

fn lower_prevention_recipient(
    recipient: Option<&SubjectRefAst>,
    builder: &BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    self_pronoun_valid: bool,
) -> Result<
    (
        spellwake_engine::damage::PreventionRecipientSpec,
        Option<BodyTarget>,
    ),
    &'static str,
> {
    use spellwake_engine::damage::PreventionRecipientSpec;
    match recipient {
        None => Ok((PreventionRecipientSpec::Any, None)),
        Some(SubjectRefAst::Player(PlayerRefAst::You)) => {
            Ok((PreventionRecipientSpec::Controller, None))
        }
        Some(SubjectRefAst::Player(PlayerRefAst::EachPlayer)) => {
            Ok((PreventionRecipientSpec::AllPlayers, None))
        }
        Some(rec) if is_self_pronoun(rec, builder, self_pronoun_valid) => {
            if !builder.has_source {
                return Err("oracle:unsupported-prevention-recipient");
            }
            Ok((PreventionRecipientSpec::AbilitySource, None))
        }
        Some(SubjectRefAst::Group(g)) if is_plain_controlled_creatures_filter(g) => {
            Ok((PreventionRecipientSpec::ControlledCreatures, None))
        }
        Some(SubjectRefAst::Group(g)) if is_plain_all_creatures_filter(g) => {
            Ok((PreventionRecipientSpec::AllCreatures, None))
        }
        Some(SubjectRefAst::Compound(parts)) if parts.len() == 2 => {
            let has_you = parts
                .iter()
                .any(|p| matches!(p, SubjectRefAst::Player(PlayerRefAst::You)));
            let has_ctrl_creatures = parts.iter().any(
                |p| matches!(p, SubjectRefAst::Group(g) if is_plain_controlled_creatures_filter(g)),
            );
            let has_ctrl_perms = parts.iter().any(
                |p| matches!(p, SubjectRefAst::Group(g) if is_plain_controlled_permanents_filter(g)),
            );
            if has_you && has_ctrl_creatures {
                Ok((
                    PreventionRecipientSpec::ControllerAndControlledCreatures,
                    None,
                ))
            } else if has_you && has_ctrl_perms {
                Ok((
                    PreventionRecipientSpec::ControllerAndControlledPermanents,
                    None,
                ))
            } else {
                Err("oracle:unsupported-prevention-recipient")
            }
        }
        Some(
            rec @ (SubjectRefAst::AnyTarget
            | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
            | SubjectRefAst::TargetObject {
                count: TargetCountAst::Exact(1),
                ..
            }
            | SubjectRefAst::TargetPlayer {
                count: TargetCountAst::Exact(1),
                ..
            }
            | SubjectRefAst::TargetOneOf {
                count: TargetCountAst::Exact(1),
                ..
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent)),
        ) => {
            let bt = lower_target_subject(rec, vocabulary)?;
            if !is_valid_prevention_recipient_target(bt) {
                return Err("oracle:unsupported-prevention-recipient");
            }
            Ok((PreventionRecipientSpec::Target, Some(bt)))
        }
        Some(SubjectRefAst::Anaphora(raw)) if raw.ends_with(" this turn") => {
            let trimmed = raw.strip_suffix(" this turn").unwrap_or(raw).trim();
            if trimmed.is_empty() {
                return Err("oracle:unsupported-prevention-recipient");
            }
            let reparsed = spellwake_oracle_compiler::filter::parse_subject_or_target(
                trimmed,
                &cached_compiler_vocab(vocabulary),
            )
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(trimmed.to_owned()));
            if matches!(&reparsed, SubjectRefAst::Anaphora(next) if next.ends_with(" this turn")) {
                return Err("oracle:unsupported-prevention-recipient");
            }
            lower_prevention_recipient(Some(&reparsed), builder, vocabulary, self_pronoun_valid)
        }
        Some(SubjectRefAst::Anaphora(text))
            if matches!(
                text.as_str(),
                "it" | "that creature" | "that permanent" | "that player"
            ) =>
        {
            if !is_valid_prevention_recipient_target(builder.target)
                || builder.x_target_count
                || builder.up_to_x_target_count
                || builder.instructions.iter().any(|op| {
                    matches!(op, Instruction::AffectPermanentTargets { maximum, .. } if *maximum != 1)
                })
            {
                return Err("oracle:unsupported-prevention-recipient");
            }
            Ok((PreventionRecipientSpec::Target, None))
        }
        Some(_) => Err("oracle:unsupported-prevention-recipient"),
    }
}

pub(super) fn lower_prevent_all_damage(
    combat_only: bool,
    target: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::damage::{PreventionSourceSpec, TurnPreventionSpec};

    let (resolved_recipient, recipient_target_to_bind) =
        lower_prevention_recipient(Some(target), builder, vocabulary, self_pronoun_valid)?;
    if let Some(bt) = recipient_target_to_bind {
        builder.bind_target(bt)?;
    }
    builder.instructions.push(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
            TurnPreventionSpec {
                combat_only,
                source: PreventionSourceSpec::Any,
                recipient: resolved_recipient,
                both_directions: false,
            },
        ),
    );
    Ok(())
}

fn is_valid_prevention_recipient_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleAnyTarget
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::SinglePlayer
            | BodyTarget::SingleOpponent
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::FilteredPermanent(_)
    )
}

/// Lower `InstructionAst::PreventAllDamageFrom` including interactive source choice
/// (CR 609.7a/b), chosen-color prevention, spell-source prevention, multi-target prevention,
/// and generalized source/recipient scopes while keeping dependent prevention-event followups
/// and unsupported predicates strictly fail-closed.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_prevent_all_damage_from(
    combat_only: bool,
    source: &SubjectRefAst,
    recipient: Option<&SubjectRefAst>,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::damage::{
        DamageSourceChoiceFilter, PreventionRecipientSpec, PreventionSourceFilter,
        PreventionSourceSpec, TurnPreventionSpec,
    };
    use spellwake_oracle_compiler::ast::FilterPredicateAst;

    let both_directions =
        matches!(recipient, Some(SubjectRefAst::Anaphora(text)) if text == "and dealt");
    if both_directions && !combat_only {
        return Err("oracle:unsupported-prevention-recipient");
    }

    // 1. Lower the recipient scope (binding a target if the recipient is targeted).
    let (resolved_recipient, recipient_target_to_bind) = if both_directions {
        (PreventionRecipientSpec::Any, None)
    } else {
        lower_prevention_recipient(recipient, builder, vocabulary, self_pronoun_valid)?
    };

    // 2. Fast-path CombatFromCreaturesWithoutSubtype (not yet representable in PreventionSourceFilter).
    if resolved_recipient == PreventionRecipientSpec::Any
        && combat_only
        && !both_directions
        && let SubjectRefAst::Group(group) = source
        && matches!(group.zone, None | Some(ZoneFilter::Battlefield))
        && group.predicates.is_empty()
        && group.flags.is_empty()
        && group.forbidden_subtypes.len() == 1
        && let Ok(lowered) = lower_object_filter(group, vocabulary)
    {
        let subtype = vocabulary
            .resolve_label(&group.forbidden_subtypes[0])
            .ok_or("oracle:unsupported-prevention-source")?;
        if lowered == ObjectFilter::creature().without_subtype(subtype) {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::CombatFromCreaturesWithoutSubtype(subtype),
                ));
            return Ok(());
        }
    }

    // 3. Multi-target permanent sources (`Soul Parry`, `Serene Sunset`, `Energy Arc`).
    if resolved_recipient == PreventionRecipientSpec::Any {
        if let SubjectRefAst::TargetObject { filter, count } = source
            && !filter.any_spell
            && !filter.is_ability
            && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                || matches!(count, TargetCountAst::AnyNumber)
                || (matches!(count, TargetCountAst::ChosenX | TargetCountAst::UpToX)
                    && !in_trigger))
        {
            let (min_t, max_t, is_x) = match *count {
                TargetCountAst::Exact(n) => (n, n, false),
                TargetCountAst::UpTo(max) => (0, max, false),
                TargetCountAst::AnyNumber => (0, u16::MAX, false),
                TargetCountAst::ChosenX | TargetCountAst::UpToX => (0, u16::MAX, true),
            };
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target(&f_clean, vocabulary)?;
            builder.bind_target(bt)?;
            if is_x {
                builder.x_target_count = true;
            }
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: min_t,
                    maximum: max_t,
                    action: PermanentTargetAction::PreventDamageThisTurn {
                        combat_only,
                        both_directions,
                    },
                });
            return Ok(());
        }
        if let SubjectRefAst::Anaphora(text) = source
            && matches!(text.as_str(), "those creatures" | "those permanents")
            && let Some((min_t, max_t)) = builder.instructions.iter().find_map(|op| match op {
                Instruction::AffectPermanentTargets {
                    minimum, maximum, ..
                } => Some((*minimum, *maximum)),
                _ => None,
            })
        {
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: min_t,
                    maximum: max_t,
                    action: PermanentTargetAction::PreventDamageThisTurn {
                        combat_only,
                        both_directions,
                    },
                });
            return Ok(());
        }
    }

    // 4. Lower generalized `PreventionSourceSpec`.
    let resolved_source = if is_self_pronoun(source, builder, self_pronoun_valid) {
        if !builder.has_source {
            return Err("oracle:unsupported-prevention-source");
        }
        PreventionSourceSpec::AbilitySource
    } else if let SubjectRefAst::Group(group) = source {
        if both_directions {
            return Err("oracle:unsupported-prevention-source");
        }
        // 4a. Interactive damage source choice (CR 609.7a): "a [red] source / creature of your choice".
        if group.predicates == [FilterPredicateAst::ChosenByPlayer(PlayerRefAst::You)] {
            if !prevention_filter_base_clean(group)
                || !group.flags.is_empty()
                || !group.forbidden_colors.is_empty()
                || group.color_arity.is_some()
                || group.controller.is_some()
                || group.combat.is_some()
                || !group.required_keywords.is_empty()
                || !group.forbidden_keywords.is_empty()
            {
                return Err("oracle:unsupported-prevention-source");
            }
            let choice_filter = if group.any_types.is_empty() && group.any_colors.is_empty() {
                DamageSourceChoiceFilter::Any
            } else if group.any_types.is_empty() && !group.any_colors.is_empty() {
                DamageSourceChoiceFilter::Colors(lower_color_mask(group.any_colors))
            } else if group.any_types == AstCardTypeMask::CREATURE && group.any_colors.is_empty() {
                DamageSourceChoiceFilter::Creature
            } else {
                return Err("oracle:unsupported-prevention-source");
            };
            PreventionSourceSpec::ChosenSource(choice_filter)
        } else if group.flags == AstFilterFlags::MATCHES_CHOSEN_COLOR
            && group.predicates.is_empty()
            && prevention_filter_base_clean(group)
            && group.any_types.is_empty()
            && group.any_colors.is_empty()
            && group.forbidden_colors.is_empty()
            && group.color_arity.is_none()
            && group.controller.is_none()
            && group.combat.is_none()
            && group.required_keywords.is_empty()
            && group.forbidden_keywords.is_empty()
        {
            // 4b. Interactive color choice: "sources of the color of your choice" (`Avacyn`, `Prismatic Strands`).
            PreventionSourceSpec::ChosenColor
        } else if group.flags == AstFilterFlags::EXCLUDE_SELF
            && group.predicates.is_empty()
            && prevention_filter_base_clean(group)
            && group.any_types == AstCardTypeMask::CREATURE
            && group.any_colors.is_empty()
            && group.forbidden_colors.is_empty()
            && group.color_arity.is_none()
            && group.controller.is_none()
            && group.combat.is_none()
            && group.required_keywords.is_empty()
            && group.forbidden_keywords.is_empty()
        {
            // 4c. "other creatures" (`Haze Frog`).
            if !builder.has_source {
                return Err("oracle:unsupported-prevention-source");
            }
            PreventionSourceSpec::CreaturesOtherThanAbilitySource
        } else if let Some(filter) = lower_prevention_source_filter_single(group) {
            PreventionSourceSpec::Filter(filter)
        } else {
            return Err("oracle:unsupported-prevention-source");
        }
    } else if let SubjectRefAst::Anaphora(raw) = source
        && !both_directions
        && raw.trim() == "that creatures"
    {
        PreventionSourceSpec::Filter(PreventionSourceFilter {
            creatures_only: true,
            ..Default::default()
        })
    } else if let SubjectRefAst::Anaphora(raw) = source
        && !both_directions
        && raw.trim() == "that other creatures"
    {
        if !builder.has_source {
            return Err("oracle:unsupported-prevention-source");
        }
        PreventionSourceSpec::CreaturesOtherThanAbilitySource
    } else if let SubjectRefAst::Anaphora(raw) = source
        && !both_directions
        && let Some(any_colors) = parse_color_sources_anaphora(raw)
    {
        PreventionSourceSpec::Filter(PreventionSourceFilter {
            any_colors,
            ..Default::default()
        })
    } else if let SubjectRefAst::Compound(groups) = source {
        // E.g. "black sources and red sources" (`Luminesce`), "blue creatures and black creatures" (`Radiant Kavu`).
        if both_directions || groups.is_empty() {
            return Err("oracle:unsupported-prevention-source");
        }
        let mut combined: Option<PreventionSourceFilter> = None;
        for part in groups {
            let SubjectRefAst::Group(g) = part else {
                return Err("oracle:unsupported-prevention-source");
            };
            let f = lower_prevention_source_filter_single(g)
                .ok_or("oracle:unsupported-prevention-source")?;
            if f.any_colors.is_empty() || !f.forbidden_colors.is_empty() || f.colorless_only {
                return Err("oracle:unsupported-prevention-source");
            }
            match &mut combined {
                None => combined = Some(f),
                Some(acc) => {
                    if acc.creatures_only != f.creatures_only
                        || acc.attacking_only != f.attacking_only
                        || acc.blocking_only != f.blocking_only
                        || acc.unblocked_only != f.unblocked_only
                        || acc.required_keyword != f.required_keyword
                        || acc.forbidden_keyword != f.forbidden_keyword
                        || acc.controller != f.controller
                    {
                        return Err("oracle:unsupported-prevention-source");
                    }
                    acc.any_colors = acc.any_colors.union(f.any_colors);
                }
            }
        }
        PreventionSourceSpec::Filter(combined.ok_or("oracle:unsupported-prevention-source")?)
    } else if let SubjectRefAst::TargetObject {
        filter,
        count: TargetCountAst::Exact(1),
    } = source
        && (filter.any_spell || filter.zone == Some(ZoneFilter::Stack))
    {
        if both_directions || resolved_recipient == PreventionRecipientSpec::Target {
            return Err("oracle:unsupported-prevention-source");
        }
        let bt = lower_target_subject(source, vocabulary)?;
        if !matches!(bt, BodyTarget::SingleSpell(_)) {
            return Err("oracle:unsupported-prevention-source");
        }
        builder.bind_target(bt)?;
        PreventionSourceSpec::TargetSpell
    } else {
        // Single targeted permanent or anaphora to previously targeted permanent.
        let target = match source {
            SubjectRefAst::TargetObject {
                count: TargetCountAst::Exact(1),
                ..
            } => lower_target_subject(source, vocabulary)?,
            SubjectRefAst::Anaphora(text)
                if matches!(text.as_str(), "it" | "that creature" | "that permanent") =>
            {
                builder.target
            }
            _ => return Err("oracle:unsupported-prevention-source"),
        };
        if !matches!(
            target,
            BodyTarget::SingleCreature
                | BodyTarget::SinglePermanent
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::FilteredPermanent(_)
        ) {
            return Err("oracle:unsupported-prevention-source");
        }
        if matches!(source, SubjectRefAst::TargetObject { .. }) {
            if recipient_target_to_bind.is_some()
                || resolved_recipient == PreventionRecipientSpec::Target
            {
                return Err("oracle:unsupported-prevention-source");
            }
            builder.bind_target(target)?;
        } else if builder.x_target_count || builder.up_to_x_target_count
            || builder.instructions.iter().any(|op| {
                matches!(op, Instruction::AffectPermanentTargets { maximum, .. } if *maximum != 1)
            })
        {
            return Err("oracle:unsupported-prevention-source");
        }
        PreventionSourceSpec::TargetPermanent
    };

    if let Some(bt) = recipient_target_to_bind {
        builder.bind_target(bt)?;
    }

    let spec = TurnPreventionSpec {
        combat_only,
        source: resolved_source,
        recipient: resolved_recipient,
        both_directions,
    };
    if matches!(spec.source, PreventionSourceSpec::ChosenColor) {
        builder
            .instructions
            .push(Instruction::PreventDamageSpec(Box::new(spec)));
    } else {
        builder.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(spec),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test".to_owned(),
            [0u8; 32],
            [
                "Angel",
                "Assassin",
                "Cleric",
                "Dragon",
                "Elf",
                "Frog",
                "Inkling",
                "Kithkin",
                "Mercenary",
                "Pirate",
                "Rogue",
                "Saproling",
                "Scarecrow",
                "Warlock",
                "Wizard",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .expect("test vocabulary")
    }

    #[test]
    fn parses_jegantha_disallow_generic_restriction() {
        let vocab = test_vocab();
        let r = parse_predicate_mana_restriction(
            "This mana can't be spent to pay generic mana costs.",
            &vocab,
        )
        .expect("jegantha restriction");
        assert!(r.disallows_generic());
    }

    #[test]
    fn parses_dynamic_throne_and_kolvori_mana_output_restrictions() {
        let vocab = test_vocab();
        assert_eq!(
            parse_dynamic_mana_output_restriction(
                "Spend this mana only to cast monocolored spells of that color.",
                &vocab
            ),
            Some(ManaOutputRestriction::MonocoloredOfChosenColor)
        );
        let kolvori = parse_dynamic_mana_output_restriction(
            "Spend this mana only to cast a creature spell of the chosen type or a legendary creature spell.",
            &vocab,
        )
        .expect("kolvori restriction");
        assert!(matches!(
            kolvori,
            ManaOutputRestriction::ChosenCreatureTypeOr(_)
        ));
    }

    #[test]
    fn parses_uncounterable_outlaw_and_ability_inheritance_predicates() {
        let vocab = test_vocab();
        let r = parse_predicate_mana_restriction(
            "Spend this mana only to cast Dragon spells or activate abilities of creatures of that type, and that spell can't be countered.",
            &vocab,
        )
        .expect("dragon predicate");
        assert!(r.makes_spell_uncounterable());
        let ManaRestriction::Predicate(pred) = r else {
            panic!("expected predicate");
        };
        assert_eq!(pred.spell_clauses.len(), 1);
        assert_eq!(pred.ability_clauses.len(), 1);

        let outlaw =
            parse_predicate_mana_restriction("Spend this mana only to cast outlaw spells.", &vocab)
                .expect("outlaw predicate");
        let ManaRestriction::Predicate(outlaw_pred) = outlaw else {
            panic!("expected predicate");
        };
        assert_eq!(outlaw_pred.spell_clauses.len(), 5);
    }

    fn card_fixture(
        name: &str,
        type_line: &str,
        mana_cost: &str,
        oracle_text: &str,
    ) -> InventoryCard {
        InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: name.into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: name.into(),
                type_line: Some(type_line.into()),
                oracle_text: Some(oracle_text.into()),
                mana_cost: Some(mana_cost.into()),
                power: None,
                toughness: None,
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        }
    }

    #[test]
    fn wave14_prevention_lowers_source_choice_and_scopes_while_keeping_guarded_cards_fail_closed() {
        let vocab = test_vocab();

        // Positive: source choice, color choice, spell target, multi-target, recipient/source scopes.
        let positive_cases = [
            (
                "Pay No Heed",
                "Instant",
                "{W}",
                "Prevent all damage a source of your choice would deal this turn.",
            ),
            (
                "Auriok Replica",
                "Artifact Creature \u{2014} Cleric",
                "{3}",
                "{W}, Sacrifice this creature: Prevent all damage a source of your choice would deal to you this turn.",
            ),
            (
                "Burrenton Forge-Tender",
                "Creature \u{2014} Kithkin Wizard",
                "{W}",
                "Protection from red\nSacrifice this creature: Prevent all damage a red source of your choice would deal this turn.",
            ),
            (
                "Al-abara's Carpet",
                "Artifact",
                "{5}",
                "{5}, {T}: Prevent all damage that would be dealt to you this turn by attacking creatures without flying.",
            ),
            (
                "Eerie Interference",
                "Instant",
                "{2}{W}",
                "Prevent all damage that would be dealt to you and creatures you control this turn by creatures.",
            ),
            (
                "Surge of Salvation",
                "Instant",
                "{W}",
                "You and permanents you control gain hexproof until end of turn. Prevent all damage that black and/or red sources would deal to creatures you control this turn.",
            ),
            (
                "Luminesce",
                "Instant",
                "{W}",
                "Prevent all damage that black sources and red sources would deal this turn.",
            ),
            (
                "Hunter's Ambush",
                "Instant",
                "{2}{G}",
                "Prevent all combat damage that would be dealt by nongreen creatures this turn.",
            ),
            (
                "Snag",
                "Instant",
                "{3}{G}",
                "You may discard a Forest card rather than pay this spell's mana cost.\nPrevent all combat damage that would be dealt by unblocked creatures this turn.",
            ),
            (
                "Lithomancer's Focus",
                "Instant",
                "{W}",
                "Target creature gets +2/+2 until end of turn. Prevent all damage that would be dealt to that creature this turn by colorless sources.",
            ),
            (
                "Soul Parry",
                "Instant",
                "{1}{W}",
                "Prevent all damage one or two target creatures would deal this turn.",
            ),
            (
                "Energy Arc",
                "Instant",
                "{W}{U}",
                "Untap any number of target creatures. Prevent all combat damage that would be dealt to and dealt by those creatures this turn.",
            ),
            (
                "Serene Sunset",
                "Instant",
                "{X}{G}",
                "Prevent all combat damage X target creatures would deal this turn.",
            ),
            (
                "Blinding Fog",
                "Instant",
                "{2}{G}",
                "Prevent all damage that would be dealt to creatures this turn. Creatures you control gain hexproof until end of turn.",
            ),
            (
                "Commencement of Festivities",
                "Instant",
                "{1}{G}",
                "Prevent all combat damage that would be dealt to players this turn.",
            ),
            (
                "Riot Control",
                "Instant",
                "{2}{W}",
                "You gain 1 life for each creature your opponents control. Prevent all damage that would be dealt to you this turn.",
            ),
            (
                "Safe Passage",
                "Instant",
                "{2}{W}",
                "Prevent all damage that would be dealt to you and creatures you control this turn.",
            ),
            (
                "Endure",
                "Instant",
                "{3}{W}{W}",
                "Prevent all damage that would be dealt to you and permanents you control this turn.",
            ),
            (
                "Djeru's Resolve",
                "Instant",
                "{W}",
                "Untap target creature. Prevent all damage that would be dealt to it this turn.",
            ),
            (
                "Chameleon Blur",
                "Instant",
                "{3}{G}",
                "Prevent all damage that creatures would deal to players this turn.",
            ),
            (
                "Haze Frog",
                "Creature \u{2014} Frog",
                "{3}{G}{G}",
                "Flash\nWhen this creature enters, prevent all combat damage that other creatures would deal this turn.",
            ),
        ];
        for (name, ty, cost, text) in positive_cases {
            let mut c = card_fixture(name, ty, cost, text);
            if ty.contains("Creature") {
                c.faces[0].power = Some("1".into());
                c.faces[0].toughness = Some("1".into());
            }
            assert!(
                compile_with_reason(&c, &vocab).is_ok(),
                "{name} should lower cleanly, got {:?}",
                compile_with_reason(&c, &vocab)
            );
        }

        // Fail-closed guards: dependent prevention-event followups, activation mana-color provenance, ongoing duration.
        let guarded_cases = [
            (
                "Hallow",
                "Instant",
                "{W}",
                "Prevent all damage target spell would deal this turn. You gain life equal to the damage prevented this way.",
            ),
            (
                "Channel Harm",
                "Instant",
                "{5}{W}",
                "Prevent all damage that would be dealt to you and permanents you control this turn by sources you don't control. If damage is prevented this way, you may have Channel Harm deal that much damage to target creature.",
            ),
            (
                "Protective Sphere",
                "Enchantment",
                "{2}{W}",
                "{1}, Pay 1 life: Prevent all damage that would be dealt to you this turn by a source of your choice that shares a color with the mana spent on this activation cost.",
            ),
            (
                "Inkshield",
                "Instant",
                "{3}{W}{B}",
                "Prevent all combat damage that would be dealt to you this turn. For each 1 damage prevented this way, create a 2/1 white and black Inkling creature token with flying.",
            ),
        ];
        for (name, ty, cost, text) in guarded_cases {
            let c = card_fixture(name, ty, cost, text);
            if compile_with_reason(&c, &vocab).is_ok() {
                let parsed = spellwake_oracle_compiler::compile_face(
                    spellwake_oracle_compiler::FaceInput {
                        name,
                        type_line: ty,
                        oracle_text: text,
                    },
                    &cached_compiler_vocab(&vocab),
                );
                panic!("{name} MUST remain fail-closed! AST={parsed:?}");
            }
        }
    }
}
