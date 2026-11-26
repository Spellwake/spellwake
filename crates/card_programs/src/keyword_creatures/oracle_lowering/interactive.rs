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
        Instruction::ExileTopPlay { any_color, .. }
        | Instruction::GrantCardPlayPermission { any_color, .. } => {
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

/// Extract a `"<filter> from it/them"` hand-choice description from either
/// `InstructionAst::ChooseOption` or `InstructionAst::InteractiveChoice(ChooseFromGroup)`.
pub(super) fn extract_revealed_hand_choice_desc(op: &InstructionAst) -> Option<&str> {
    let raw = match op {
        InstructionAst::ChooseOption(s) => s.as_str(),
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::ChooseFromGroup {
            chooser: PlayerRefAst::You,
            description,
        }) => description.as_str(),
        _ => return None,
    };
    let trimmed = raw.trim().trim_end_matches('.');
    trimmed
        .strip_suffix(" from it")
        .or_else(|| trimmed.strip_suffix(" from them"))
        .or_else(|| trimmed.strip_suffix(" from that hand"))
        .or_else(|| trimmed.strip_suffix(" of those cards"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
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
            SubjectRefAst::TargetObject { .. } => {
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
    let (source_zone, stripped_ctrl) = match filter_ast.zone {
        Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand | ZoneFilter::AnyHand) => {
            (SourceZoneFilter::Hand, false)
        }
        Some(ZoneFilter::YourGraveyard | ZoneFilter::ThatPlayerGraveyard) => {
            (SourceZoneFilter::Graveyard, false)
        }
        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::TargetOpponentGraveyard) => {
            (SourceZoneFilter::OpponentGraveyard, false)
        }
        Some(ZoneFilter::AnyGraveyard) => (SourceZoneFilter::AnyGraveyard, false),
        Some(ZoneFilter::Exile) => (SourceZoneFilter::Exile, false),
        None | Some(ZoneFilter::Battlefield) => {
            if matches!(
                filter_ast.controller,
                Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
            ) {
                (SourceZoneFilter::Battlefield, true)
            } else {
                (SourceZoneFilter::AnyBattlefield, false)
            }
        }
        _ => return Err("oracle:unsupported-instruction"),
    };
    let mut normalized = filter_ast.clone();
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
    if raw.contains("target") || raw.contains("from it") || raw.contains("from them") {
        return None;
    }
    let players = lower_affected_players_in_context(chooser, builder, in_trigger).ok()?;

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

    // 4. Generic battlefield non-target object filter choices
    let (min_val, max_val, body_phrase) = if let Some(rest) = raw.strip_prefix("any number of ") {
        (0i64, 99i64, rest)
    } else if let Some(rest) = raw.strip_prefix("up to one ") {
        (0, 1, rest)
    } else if let Some(rest) = raw.strip_prefix("up to two ") {
        (0, 2, rest)
    } else if let Some(rest) = raw.strip_prefix("two ") {
        (2, 2, rest)
    } else if let Some(rest) = raw.strip_prefix("one ") {
        (1, 1, rest)
    } else if let Some(rest) = raw.strip_prefix("an ") {
        (1, 1, rest)
    } else {
        let rest = raw.strip_prefix("a ")?;
        (1, 1, rest)
    };

    let (source_zone, filter_phrase) = if let Some(p) = body_phrase
        .strip_suffix(" they control")
        .or_else(|| body_phrase.strip_suffix(" you control"))
    {
        (SourceZoneFilter::Battlefield, p.trim())
    } else if body_phrase.ends_with(" that player controls") {
        // When controller chooses a creature "that player controls" inside ForEachPlayer,
        // `ThatPlayer` maps to LoopPlayer/ChosenPlayer in `lower_object_filter`.
        (SourceZoneFilter::AnyBattlefield, body_phrase.trim())
    } else if body_phrase.contains(" in ") {
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
    if normalized.zone.is_some() {
        return None;
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
    if let Ok(kind) = parse_choice_kind(cleaned, true, vocabulary) {
        return emit_choice_for_player_ref(chooser, kind, builder, ids, in_trigger);
    }
    let domain = match cleaned {
        "an opponent at random" | "opponent at random" => ResolutionChoiceDomain::RandomPlayer {
            opponents_only: true,
        },
        "a player at random" | "player at random" => ResolutionChoiceDomain::RandomPlayer {
            opponents_only: false,
        },
        "an opponent" | "opponent" => ResolutionChoiceDomain::Player {
            opponents_only: true,
        },
        "a player" | "player" => ResolutionChoiceDomain::Player {
            opponents_only: false,
        },
        "a number" | "number" => ResolutionChoiceDomain::Number { min: 0, max: 20 },
        "secretly choose 1, 2, or 3" | "1, 2, or 3" => {
            ResolutionChoiceDomain::Number { min: 1, max: 3 }
        }
        "a creature card name"
        | "a card name other than a basic land card name"
        | "a nonbasic land card name" => ResolutionChoiceDomain::CardName { nonland_only: true },
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
        "a creature type"
            if matches!(
                chooser,
                PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherPlayer
            ) =>
        {
            // Each player choosing a creature type uses a deterministic labeled choice if needed;
            // only controller currently has a dedicated CreatureTypeSlot.
            return Err("oracle:unsupported-choose-option");
        }
        other => {
            if chooser == PlayerRefAst::You
                && let Some(target_raw) = other
                    .strip_prefix("another ")
                    .filter(|rest| rest.starts_with("target "))
                && let Ok(mut subj) = spellwake_oracle_compiler::filter::parse_subject_or_target(
                    target_raw,
                    &cached_compiler_vocab(vocabulary),
                )
            {
                if let SubjectRefAst::TargetObject { filter, .. } = &mut subj {
                    filter.flags = filter
                        .flags
                        .union(spellwake_oracle_compiler::ast::ObjectFilterFlags::EXCLUDE_SELF);
                }
                let bt = lower_target_subject(&subj, vocabulary)?;
                builder.bind_target(bt)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test".to_owned(),
            [0u8; 32],
            [
                "Assassin",
                "Dragon",
                "Elf",
                "Mercenary",
                "Pirate",
                "Rogue",
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
}
