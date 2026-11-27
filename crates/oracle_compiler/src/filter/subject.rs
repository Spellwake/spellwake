//! Parsers for player references (`PlayerRefAst`), subjects/targets (`SubjectRefAst`),
//! and keyword ability lists.

use super::condition::parse_condition;
use super::object::parse_object_filter;
use super::zones::peel_zone_suffix;
use crate::ast::{
    ConditionAst, FilterPredicateAst, GameMarker, ObjectFilterAst, ObjectFilterFlags, PileRefAst,
    PlayerRefAst, SubjectRefAst, TargetCountAst,
};
use crate::combinators::{grp, strip_prefix_any, strip_suffix_any, trim_punct};
use crate::vocab::{Vocabulary, parse_number_word};

pub fn parse_keyword_list(text: &str, vocab: &Vocabulary) -> Option<Vec<String>> {
    let clean = trim_punct(text).to_ascii_lowercase();
    if clean.is_empty() || clean.contains('"') {
        return None;
    }
    // Handle "protection from ..." and "hexproof from ..." inside keyword lists
    if clean.starts_with("protection from ") || clean.starts_with("hexproof from ") {
        return Some(vec![clean]);
    }
    let normalized = clean
        .replace(" and from ", " /from ")
        .replace(", and ", ",")
        .replace(", or ", ",")
        .replace(" and ", ",")
        .replace(" or ", ",");
    let mut out = Vec::new();
    for part in normalized.split(',') {
        let kw = part.trim();
        if kw.is_empty() {
            continue;
        }
        if kw.starts_with("protection from ") || kw.starts_with("hexproof from ") {
            out.push(kw.to_string());
            continue;
        }
        if let Some(arg) = strip_prefix_any(
            &kw,
            &[
                "ward ",
                "toxic ",
                "afflict ",
                "afterlife ",
                "annihilator ",
                "bushido ",
                "firebending ",
                "frenzy ",
                "poisonous ",
                "rampage ",
                "renown ",
                "absorb ",
                "modular ",
                "crew ",
                "saddle ",
                "ripple ",
                "equip ",
                "dredge ",
                "prowl ",
                "miracle ",
                "affinity for ",
                "freerunning ",
                "sticker kicker ",
                "bloodthirst ",
                "encore ",
                "flashback ",
                "cycling ",
                "madness ",
                "kicker ",
                "buyback ",
                "overload ",
                "evoke ",
                "dash ",
                "unearth ",
                "escape ",
                "embalm ",
                "eternalize ",
                "plot ",
                "warp ",
                "foretell ",
                "suspend ",
                "ninjutsu ",
                "spectacle ",
                "blitz ",
                "disturb ",
                "scavenge ",
                "morph ",
                "megamorph ",
                "disguise ",
                "squad ",
                "offspring ",
                "web-slinging ",
                "outlast ",
                "sneak ",
                "fabricate ",
                "casualty ",
                "backup ",
                "awaken ",
                "devour ",
            ],
        ) && !arg.is_empty()
        {
            out.push(kw.to_string());
            continue;
        }
        if !vocab.is_evergreen_keyword(kw)
            && !matches!(
                kw,
                "foretell"
                    | "awaken"
                    | "flashback"
                    | "escape"
                    | "scavenge"
                    | "unearth"
                    | "madness"
                    | "cycling"
                    | "kicker"
                    | "evoke"
                    | "blitz"
                    | "dash"
                    | "disturb"
                    | "embalm"
                    | "eternalize"
                    | "encore"
                    | "plot"
                    | "warp"
                    | "suspend"
                    | "buyback"
                    | "overload"
                    | "spectacle"
                    | "surge"
                    | "prowl"
                    | "miracle"
                    | "cascade"
                    | "convoke"
                    | "delve"
                    | "improvise"
                    | "assist"
                    | "demonstrate"
                    | "storm"
                    | "retrace"
                    | "jump-start"
                    | "dredge"
                    | "transmute"
                    | "ninjutsu"
                    | "mutate"
                    | "morph"
                    | "disguise"
                    | "emerge"
                    | "toxic"
                    | "infect"
                    | "wither"
                    | "enlist"
                    | "backup"
                    | "saddle"
                    | "crew"
                    | "partner"
                    | "companion"
                    | "prototype"
                    | "freerunning"
                    | "offspring"
                    | "squad"
                    | "impending"
                    | "exhaust"
                    | "bargain"
                    | "cleave"
                    | "casualty"
                    | "ravenous"
                    | "craft"
                    | "spree"
                    | "gift"
                    | "harmonize"
                    | "mobilize"
                    | "mayhem"
                    | "vanishing"
                    | "fading"
                    | "echo"
                    | "phasing"
                    | "flanking"
                    | "bushido"
                    | "rampage"
                    | "banding"
                    | "horsemanship"
                    | "shadow"
                    | "fear"
                    | "intimidate"
                    | "skulk"
                    | "shroud"
                    | "split second"
                    | "riot"
                    | "unleash"
                    | "undying"
                    | "persist"
                    | "dethrone"
                    | "melee"
                    | "myriad"
                    | "exploit"
                    | "exalted"
                    | "extort"
                    | "evolve"
                    | "graft"
                    | "modular"
                    | "sunburst"
                    | "reconfigure"
                    | "bestow"
                    | "annihilator"
                    | "devoid"
                    | "ingest"
                    | "prowess"
                    | "training"
                    | "mentor"
                    | "renown"
                    | "fabricate"
                    | "afterlife"
                    | "ascend"
                    | "entwine"
                    | "replicate"
                    | "rebound"
                    | "cipher"
                    | "splice"
                    | "forecast"
                    | "haunt"
                    | "soulbond"
                    | "soulshift"
                    | "changeling"
                    | "champion"
                    | "hideaway"
                    | "affinity"
                    | "undaunted"
                    | "protection"
                    | "ward"
            )
        {
            return None;
        }
        out.push(kw.to_string());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Parse a player reference phrase.
#[must_use]
pub fn parse_player_ref(phrase: &str) -> Option<PlayerRefAst> {
    match trim_punct(phrase).to_ascii_lowercase().as_str() {
        "you" => Some(PlayerRefAst::You),
        "target player" | "up to one target player" => Some(PlayerRefAst::TargetPlayer),
        "target opponent" | "up to one target opponent" => Some(PlayerRefAst::TargetOpponent),
        "each player" | "all players" | "any number of target players" => {
            Some(PlayerRefAst::EachPlayer)
        }
        "each opponent" | "opponents" | "all opponents" | "all of your opponents" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "each other opponent"
        | "each of your other opponents"
        | "your other opponents"
        | "each of that player's opponents" => Some(PlayerRefAst::EachOtherOpponent),
        "each other player"
        | "each player other than you"
        | "all other players"
        | "other players" => Some(PlayerRefAst::EachOtherPlayer),
        "an opponent"
        | "an opponent chosen at random"
        | "one of your opponents"
        | "another opponent" => Some(PlayerRefAst::AnyOpponent),
        "a player"
        | "any player"
        | "any other player"
        | "a player of your choice"
        | "each friend"
        | "each foe" => Some(PlayerRefAst::AnyPlayer),
        "your team" | "each of your teammates" => Some(PlayerRefAst::You),
        "that player"
        | "they"
        | "that player or that permanent's controller"
        | "that permanent's controller or that player" => Some(PlayerRefAst::ThatPlayer),
        "defending player" | "the defending player" | "each defending player" => {
            Some(PlayerRefAst::DefendingPlayer)
        }
        "active player" | "the active player" | "the attacking player" | "attacking player" => {
            Some(PlayerRefAst::ActivePlayer)
        }
        "each player other than target player" => Some(PlayerRefAst::EachOtherPlayer),
        "you and the attacking player" => Some(PlayerRefAst::EachPlayer),
        "enchanted player" => Some(PlayerRefAst::EnchantedPlayer),
        "its controller" => Some(PlayerRefAst::ItsController),
        "its owner" => Some(PlayerRefAst::ItsOwner),
        "that creature's or spell's controller"
        | "that spell's or ability's controller"
        | "that permanent's or spell's controller"
        | "the controller of that spell or ability"
        | "that spell or ability's controller"
        | "their controller"
        | "their controllers"
        | "that player's controller"
        | "~'s controller"
        | "the controller of target instant or sorcery spell" => {
            Some(PlayerRefAst::ThatObjectController)
        }
        "each opponent with no cards in hand" => Some(PlayerRefAst::EachOpponent),
        "each player ~ attacked this turn" => Some(PlayerRefAst::EachPlayer),
        "the player to your right" | "the player to your left" => Some(PlayerRefAst::AnyOpponent),
        "that player or that planeswalker's controller" => {
            Some(PlayerRefAst::ThatPlayerOrPlaneswalkerController)
        }
        "each player on your team" => Some(PlayerRefAst::EachPlayerOnYourTeam),
        "the chosen player" => Some(PlayerRefAst::ChosenPlayer),
        "target opponent whose turn it is" => Some(PlayerRefAst::TargetOpponentWhoseTurnItIs),
        "target opponent chosen at random from among your opponents" => {
            Some(PlayerRefAst::TargetOpponentAtRandom)
        }
        "the player"
        | "those players"
        | "that opponent"
        | "that attacking player"
        | "the first player"
        | "the second player"
        | "the high bidder" => Some(PlayerRefAst::ThatPlayer),
        "their owner" | "~'s owner" | "the owner of target permanent" => {
            Some(PlayerRefAst::ItsOwner)
        }
        "players" => Some(PlayerRefAst::EachPlayer),
        "your opponents"
        | "each of your opponents"
        | "each of those opponents"
        | "any number of target opponents"
        | "each opponent who didn't"
        | "two target opponents" => Some(PlayerRefAst::EachOpponent),
        "each player who doesn't have max speed"
        | "each player who searched a library this way"
        | "each player who searched their library this way"
        | "each player who received no votes"
        | "each player who chose that number"
        | "each player who discarded a card with the greatest mana value among cards discarded this way"
        | "each player other than its controller"
        | "each of those players"
        | "each of them" => Some(PlayerRefAst::EachPlayer),
        "each opponent who didn't sacrifice a permanent or discard a card this way" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "the player whose turn it is" => Some(PlayerRefAst::ActivePlayer),
        "the player with the most life" => Some(PlayerRefAst::AnyPlayer),
        "another one of your opponents chosen at random" => Some(PlayerRefAst::AnyOpponent),
        "enchanted opponent" => Some(PlayerRefAst::EnchantedPlayer),
        "any opponent" | "target opponent chosen at random" => Some(PlayerRefAst::TargetOpponent),
        "target player other than its controller"
        | "target player other than ~'s owner"
        | "another target player"
        | "target player who was dealt combat damage by ~ this turn"
        | "target player dealt damage by ~ this turn"
        | "two target players"
        | "up to two target players" => Some(PlayerRefAst::TargetPlayer),
        "another target opponent" => Some(PlayerRefAst::TargetOpponent),
        "each of its controller's opponents" => Some(PlayerRefAst::EachOtherOpponent),
        "you and the controller of those creatures"
        | "each player starting with you"
        | "each player with the highest number"
        | "each player whose coin comes up tails"
        | "each player who sacrificed a plains this way"
        | "you and each opponent who voted for a choice you voted for" => {
            Some(PlayerRefAst::EachPlayer)
        }
        other if other.starts_with("each opponent who chose ") => Some(PlayerRefAst::EachOpponent),
        "target player who lost life this turn" => Some(PlayerRefAst::TargetPlayer),
        "the player who revealed the fewest items"
        | "the player who reveals the card with the greatest mana value" => {
            Some(PlayerRefAst::ThatPlayer)
        }
        "the voter" | "the other chosen player" => Some(PlayerRefAst::ThatPlayer),
        "any number of target players other than that player"
        | "each player with fewer cards in hand than the player with the most cards in hand" => {
            Some(PlayerRefAst::EachPlayer)
        }
        other => {
            // Compositional "<object>'s controller" / "the controller of <object>" and owner forms.
            const OBJ_REF_PREFIXES: &[&str] =
                &["that ", "the ", "enchanted ", "equipped ", "fortified "];
            if let Some(stem) = other
                .strip_suffix("'s controller")
                .or_else(|| other.strip_prefix("the controller of "))
                && OBJ_REF_PREFIXES.iter().any(|p| stem.starts_with(p))
            {
                return Some(PlayerRefAst::ThatObjectController);
            }
            if let Some(stem) = other
                .strip_suffix("'s owner")
                .or_else(|| other.strip_prefix("the owner of "))
                && OBJ_REF_PREFIXES.iter().any(|p| stem.starts_with(p))
            {
                return Some(PlayerRefAst::ItsOwner);
            }
            if other.starts_with("the player who controls the most ")
                || other.starts_with("the player who controls the fewest ")
                || other.starts_with("a player who controls more ")
                || other.starts_with("an opponent with the most ")
                || other.starts_with("an opponent with the fewest ")
                || other.starts_with("the opponent with the most ")
            {
                Some(PlayerRefAst::AnyOpponent)
            } else {
                None
            }
        }
    }
}

pub(super) fn is_characteristic_choice_prefix(prefix: &str) -> bool {
    let p = prefix.trim();
    p.is_empty()
        || p.ends_with("of the creature type")
        || p.ends_with("of the color")
        || p.ends_with("of the card type")
        || p.ends_with("in the sector")
}

fn attach_chosen_by_player(subj: &mut SubjectRefAst, chooser: PlayerRefAst) {
    let pred = FilterPredicateAst::ChosenByPlayer(chooser);
    let flt_opt = match subj {
        SubjectRefAst::Group(flt)
        | SubjectRefAst::All(flt)
        | SubjectRefAst::TargetObject { filter: flt, .. }
        | SubjectRefAst::DistinctTarget { filter: flt, .. } => Some(flt.as_mut()),
        _ => None,
    };
    if let Some(flt) = flt_opt
        && !flt.predicates.contains(&pred)
    {
        flt.predicates.push(pred);
    }
}

/// Parse a subject, target, or group phrase into a typed `SubjectRefAst`.
pub fn parse_subject_or_target(phrase: &str, vocab: &Vocabulary) -> Result<SubjectRefAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_subject".to_string());
    }
    let low = raw.to_ascii_lowercase();
    for (choice_suf, choice_player) in [
        (" of their choice", PlayerRefAst::ThatPlayer),
        (" of your choice", PlayerRefAst::You),
        (" of that player's choice", PlayerRefAst::ThatPlayer),
        (" of its controller's choice", PlayerRefAst::ItsController),
        (" of an opponent's choice", PlayerRefAst::AnyOpponent),
    ] {
        if let Some(prefix) = low.strip_suffix(choice_suf)
            && !is_characteristic_choice_prefix(prefix)
        {
            let mut parsed = parse_subject_or_target(&raw[..raw.len() - choice_suf.len()], vocab)?;
            attach_chosen_by_player(&mut parsed, choice_player);
            return Ok(parsed);
        }
    }

    // Wave E6.2: Mid-filter "of their choice" (Foundations sacrifice phrasing, e.g. "a creature of their choice that attacked or blocked this turn")
    if let Some((before_c, after_c)) = low.split_once(" of their choice that") {
        let rebuilt = format!("{} that{}", before_c.trim(), after_c);
        let mut parsed = parse_subject_or_target(&rebuilt, vocab)?;
        attach_chosen_by_player(&mut parsed, PlayerRefAst::ThatPlayer);
        return Ok(parsed);
    }

    // Wave E6.2 / E8: Compositional disjunctive shared target pools (`TargetOneOf`) preserving single shared target cardinality
    if let Some(subj) = parse_compositional_target_one_of(&low, vocab) {
        return Ok(subj);
    }
    if low == "any target that was dealt damage this turn" {
        let mut flt = ObjectFilterAst::default();
        flt.flags = flt.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if let Some(excl_raw) = low.strip_prefix("any target other than ")
        && let Ok(excl_subj) = parse_subject_or_target(excl_raw.trim(), vocab)
    {
        let mut flt = ObjectFilterAst::default();
        flt.predicates
            .push(FilterPredicateAst::Exclude(Box::new(excl_subj)));
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if let Some(after_act) = low.strip_prefix("target activated ability from ")
        && let Ok(src_flt) = parse_object_filter(after_act.trim(), vocab)
    {
        let mut ab_flt = ObjectFilterAst {
            is_ability: true,
            ..Default::default()
        };
        ab_flt
            .predicates
            .push(FilterPredicateAst::FromSource(Box::new(src_flt)));
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(ab_flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if low == "target player who attacked this turn" {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::TargetPlayer,
            condition: Box::new(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            })),
        });
    }
    if let Some(def_raw) = low.strip_prefix("each opponent attacking ")
        && let Some(defender) = parse_player_ref(def_raw.trim())
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachOpponent,
            condition: Box::new(ConditionAst::Marker(GameMarker::PlayerAttackingPlayer {
                attacker: PlayerRefAst::ThatPlayer,
                defender,
            })),
        });
    }

    // Wave E4: "a second target <flt>" / "a third target <flt>" & "target permanent or player/opponent"
    if let Some(after_ord) = strip_prefix_any(&low, &["a second target ", "a third target "])
        && let Ok(flt) = parse_object_filter(after_ord.trim(), vocab)
    {
        return Ok(SubjectRefAst::TargetObject {
            filter: Box::new(flt),
            count: TargetCountAst::Exact(1),
        });
    }
    if matches!(
        low.as_str(),
        "target permanent or player"
            | "up to one target permanent or opponent"
            | "another target permanent or opponent"
            | "the permanent or player"
            | "a player or planeswalker that opponent is attacking"
            | "target permanent or suspended card"
            | "the rest of those cards"
            | "the rest of those exiled cards"
            | "the rest of the cards revealed this way"
            | "the cards you revealed this way"
            | "the last cards you looked at this way"
            | "any of those cards you didn't play"
            | "the first card"
            | "the other one"
            | "one of the other creatures"
            | "that spell or ability"
            | "each permanent and each suspended card"
            | "all auras attached to them"
            | "any number of auras and equipment you control"
            | "two target permanents that share a permanent type"
            | "target permanent you neither own nor control"
            | "your commander from the command zone"
            | "all tokens created with it"
            | "it and all creatures it blocked this turn"
            | "ragavan"
            | "the creature tapped to pay ~'s additional cost"
            | "each of two creatures you control"
    ) {
        return Ok(SubjectRefAst::Anaphora(low));
    }

    // Cluster 2: Explicit zone-qualified self or anaphora subject (`~ from your graveyard`, `it from exile`)
    if let Some((p, zone)) = peel_zone_suffix(&low) {
        let inner = match p {
            "~" | "this card" => Some(SubjectRefAst::SelfObject),
            "it" | "itself" | "them" | "that card" | "those cards" | "that creature"
            | "that permanent" | "that artifact" | "that enchantment" | "that land"
            | "that planeswalker" | "the exiled card" | "the exiled cards" | "the chosen card"
            | "the revealed card" | "the discarded card" | "the milled card" => {
                Some(SubjectRefAst::Anaphora(p.to_string()))
            }
            _ => None,
        };
        if let Some(subj) = inner {
            return Ok(SubjectRefAst::InZone {
                subject: Box::new(subj),
                zone,
            });
        }
    }

    // Cluster 8 & 14: Craft LKI group and Pile references
    if low == "the exiled cards used to craft ~" || low == "the exiled cards used to craft it" {
        return Ok(SubjectRefAst::CraftExiledCards);
    }
    match low.as_str() {
        "the chosen pile" | "their chosen pile" => {
            return Ok(SubjectRefAst::PileRef(PileRefAst::ChosenPile));
        }
        "the other pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::OtherPile)),
        "that pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::ThatPile)),
        "each pile" => return Ok(SubjectRefAst::PileRef(PileRefAst::EachPile)),
        _ => {}
    }
    for (pile_suf, pile_kind) in [
        (" in their chosen pile", PileRefAst::ChosenPile),
        (" in the chosen pile", PileRefAst::ChosenPile),
        (" in that player's chosen pile", PileRefAst::ChosenPile),
        (" in the other pile", PileRefAst::OtherPile),
        (" in that pile", PileRefAst::ThatPile),
        (" in each pile", PileRefAst::EachPile),
    ] {
        if let Some(flt_raw) = low.strip_suffix(pile_suf) {
            let flt_clean = flt_raw
                .trim()
                .strip_prefix("the ")
                .unwrap_or(flt_raw.trim());
            if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
                return Ok(SubjectRefAst::PileRef(PileRefAst::FilteredInPile {
                    filter: Box::new(flt),
                    pile: Box::new(pile_kind),
                }));
            }
        }
    }

    if low == "~" {
        return Ok(SubjectRefAst::SelfObject);
    }
    // Compositional attached-host subjects ("enchanted/equipped/fortified <noun>").
    if let Some(noun) = strip_prefix_any(&low, &["enchanted ", "equipped ", "fortified "])
        && matches!(
            noun,
            "creature"
                | "permanent"
                | "land"
                | "artifact"
                | "enchantment"
                | "planeswalker"
                | "battle"
                | "vehicle"
                | "equipment"
                | "aura"
        )
    {
        return Ok(SubjectRefAst::AttachedHost(low));
    }
    // Compositional "that/the/those <single object noun>" and "<N> targets" anaphora/targets.
    if let Some(noun) = strip_prefix_any(&low, &["that ", "the "])
        && matches!(
            noun,
            "creature"
                | "permanent"
                | "card"
                | "spell"
                | "spell or ability"
                | "ability"
                | "artifact"
                | "enchantment"
                | "land"
                | "planeswalker"
                | "token"
                | "aura"
                | "equipment"
                | "wall"
                | "copy"
                | "copies"
                | "rest"
                | "other"
                | "aura card"
                | "targeted creature"
                | "amassed army"
                | "army you amassed"
                | "blighted creature"
                | "copied spell"
                | "milled card"
                | "exiled card"
                | "exiled cards"
                | "other exiled cards"
                | "sacrificed creature"
                | "sacrificed permanent"
                | "discarded card"
                | "revealed card"
                | "revealed cards"
                | "chosen creature"
                | "chosen creatures"
                | "chosen card"
                | "last chosen card"
                | "chosen cards"
                | "chosen permanents"
                | "chosen player"
                | "other chosen player"
                | "first creature"
                | "creature you control"
                | "creature you chose"
                | "creature your opponent chose"
                | "player you chose"
                | "permanent or player"
                | "player or planeswalker"
                | "creature or planeswalker"
        )
    {
        return Ok(SubjectRefAst::Anaphora(low));
    }
    if let Some(noun) = low.strip_prefix("those ")
        && matches!(
            noun,
            "creatures" | "permanents" | "cards" | "tokens" | "auras" | "lands" | "artifacts"
        )
    {
        return Ok(SubjectRefAst::Anaphora(low));
    }
    if let Some(cnt_raw) = strip_suffix_any(&low, &[" other targets", " targets"]) {
        let cnt_clean = cnt_raw.strip_prefix("up to ").unwrap_or(cnt_raw).trim();
        if matches!(cnt_clean, "x" | "one or two") || parse_number_word(cnt_clean).is_some() {
            return Ok(SubjectRefAst::AnyTarget);
        }
    }

    match low.as_str() {
        "it"
        | "itself"
        | "he"
        | "she"
        | "him"
        | "her"
        | "himself"
        | "herself"
        | "them"
        | "themselves"
        | "both creatures"
        | "one"
        | "another"
        | "the player who controls the most creatures"
        | "an opponent with the most life among your opponents"
        | "the creature it's attached to"
        | "the permanent it's attached to"
        | "the land it's attached to"
        | "the artifact it's attached to"
        | "the creature ~ is attached to"
        | "the permanent ~ is attached to"
        | "its controller"
        | "its owner"
        | "two of them at random"
        | "two of them"
        | "another permanent of that type"
        | "one of the top two cards of your graveyard"
        | "your ring-bearer"
        | "the cards they exiled this way"
        | "the cards you exiled this way"
        | "the player or planeswalker that creature is attacking"
        | "the player or planeswalker it's attacking"
        | "the bottom card of your library"
        | "target face-up exiled card"
        | "each of the chosen creatures"
        | "each of those creatures"
        | "each of them"
        | "your commander"
        | "a commander you control"
        | "one of them"
        | "one of those creatures"
        | "one of those cards"
        | "two of those cards"
        | "the top card of your graveyard"
        | "the top creature card of your graveyard"
        | "the top creature card of defending player's graveyard"
        | "the top card of their library"
        | "the top card of that player's library"
        | "the top card of your library"
        | "all permanents in the pile of their choice"
        | "one of them chosen at random"
        | "any of them that are walls"
        | "a random creature card from among them"
        | "each of those permanents and/or players"
        | "all cards they exiled this way"
        | "all cards you exiled this way"
        | "the cards exiled this way"
        | "one of the exiled cards"
        | "the rest of the exiled cards"
        | "the rest of the revealed cards"
        | "all other cards revealed this way"
        | "a card from the chosen pile"
        | "the nonland cards revealed this way"
        | "the top two cards of that player's library"
        | "either of them"
        | "target permanent or suspended card"
        | "the first spell you cast each turn" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        "any target"
        | "any other target"
        | "another target"
        | "any target chosen at random"
        | "any target of an opponent's choice"
        | "target creature or player"
        | "target opponent or creature"
        | "target player or battle"
        | "target opponent, creature an opponent controls, or planeswalker an opponent controls"
        | "target creature token, player, or planeswalker"
        | "target planeswalker that player controls or battle that player protects"
        | "up to one target creature, up to one target player, and/or up to one target planeswalker" =>
        {
            return Ok(SubjectRefAst::AnyTarget);
        }
        "target player or planeswalker"
        | "target planeswalker or player"
        | "up to one target player or planeswalker" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: false,
            });
        }
        "target opponent or planeswalker" | "target planeswalker or opponent" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: true,
            });
        }
        _ => {}
    }

    // Universal "any target that isn't a <flt>" (Wrathful Red Dragon, Wrathful Raptors)
    if let Some(excl) = low.strip_prefix("any target that isn't ")
        && parse_object_filter(excl, vocab).is_ok()
    {
        return Ok(SubjectRefAst::AnyTarget);
    }

    // Universal "that <subtype/type>", "one of those <subtype/type>", and "either of those <subtype/type>" anaphora
    if let Some(rest) = strip_prefix_any(&low, &["that ", "one of those ", "either of those "])
        && !rest.contains(" controls")
        && !rest.contains(" owns")
        && parse_object_filter(rest, vocab).is_ok()
    {
        return Ok(SubjectRefAst::Anaphora(low));
    }

    if let Some(p) = parse_player_ref(raw) {
        return Ok(SubjectRefAst::Player(p));
    }

    // Cluster 4: Generalized possessive `<subj>'s controller|owner` and `the controller|owner of <subj>`
    if let Some(subj_raw) = low
        .strip_suffix("'s controller")
        .or_else(|| low.strip_prefix("the controller of "))
        && !subj_raw.trim().is_empty()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(SubjectRefAst::ControllerOf(Box::new(subj)));
    }
    if let Some(subj_raw) = low
        .strip_suffix("'s owner")
        .or_else(|| low.strip_prefix("the owner of "))
        && !subj_raw.trim().is_empty()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(SubjectRefAst::OwnerOf(Box::new(subj)));
    }
    // Cluster 4: Filtered player references (`each player dealt [combat] damage this way`, etc.)
    for (suf, marker) in [
        (
            " dealt combat damage this way",
            GameMarker::DealtDamageThisWay { combat_only: true },
        ),
        (
            " dealt damage this way",
            GameMarker::DealtDamageThisWay { combat_only: false },
        ),
        (
            " dealt combat damage by ~ this turn",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: true },
        ),
        (
            " dealt damage by ~ this turn",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: false },
        ),
        (
            " previously dealt damage by it",
            GameMarker::DealtDamageBySelfThisTurn { combat_only: false },
        ),
    ] {
        if let Some(who_raw) = low.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw.trim())
        {
            return Ok(SubjectRefAst::PlayerMatching {
                player,
                condition: Box::new(ConditionAst::Marker(marker)),
            });
        }
    }

    // Composed multi-subject references with explicit second-branch quantifier/target/player
    // (must run BEFORE `strip_prefix("each ")` / `strip_prefix("target ")` / `strip_prefix("up to ")`):
    // e.g. "each creature and each player", "each opponent and each creature and planeswalker they control",
    // "up to one target land and up to one target creature", "target artifact and target creature".
    for (needle, keep_prefix) in [
        (" and/or ", ""),
        (" and each ", "each "),
        (", and each ", "each "),
        (", and/or up to ", "up to "),
        (", and up to ", "up to "),
        (" and up to ", "up to "),
        (", up to ", "up to "),
        (" and another target ", "another target "),
        (", and/or target ", "target "),
        (" and target ", "target "),
        (", and target ", "target "),
        (" or target ", "target "),
        (" and all ", "all "),
    ] {
        if let Some((left, right)) = low.split_once(needle) {
            let l = left.trim();
            let r_trim = right.trim();
            if needle == " and/or "
                && !(r_trim.starts_with("target ")
                    || r_trim.starts_with("up to ")
                    || r_trim.starts_with("another target ")
                    || r_trim.starts_with("each ")
                    || r_trim.starts_with("all ")
                    || r_trim.starts_with("any ")
                    || r_trim.starts_with("a ")
                    || r_trim.starts_with("an ")
                    || r_trim.starts_with("your ")
                    || r_trim.starts_with("their ")
                    || r_trim.starts_with("its ")
                    || parse_player_ref(r_trim).is_some())
            {
                continue;
            }
            let r_full = format!("{keep_prefix}{r_trim}");
            if !l.is_empty()
                && let (Ok(ls), Ok(rs)) = (
                    parse_subject_or_target(l, vocab),
                    parse_subject_or_target(&r_full, vocab),
                )
            {
                let mut parts = match ls {
                    SubjectRefAst::Compound(v) => v,
                    other => vec![other],
                };
                match rs {
                    SubjectRefAst::Compound(v) => parts.extend(v),
                    other => parts.push(other),
                }
                return Ok(SubjectRefAst::Compound(parts));
            }
        }
    }

    // "each of up to N/X [other] target <rest>" -> delegate to inner target
    if let Some(rest) = low.strip_prefix("each of ") {
        return parse_subject_or_target(rest, vocab);
    }

    // "another target <rest>"
    if let Some(rest) = low.strip_prefix("another target ") {
        let mut subj = finish_target(rest, TargetCountAst::Exact(1), vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }

    // "one or more target <rest>" / "one or two target <rest>" / "one, two, or three target <rest>"
    if let Some(rest) = low.strip_prefix("one or more target ") {
        return finish_target(rest, TargetCountAst::UpTo(99), vocab);
    }
    if let Some(rest) = low.strip_prefix("one or two target ") {
        return finish_target(rest, TargetCountAst::UpTo(2), vocab);
    }
    if let Some(rest) = low.strip_prefix("one, two, or three target ") {
        return finish_target(rest, TargetCountAst::UpTo(3), vocab);
    }
    if let Some(rest) = strip_prefix_any(&low, &["one or two ", "one, two, or three "])
        && !rest.starts_with("target ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(grp(flt));
    }

    // "up to N/X [other] target <rest>" OR non-targeting "up to N/X <flt>" (Gix's Command, Nihiloor, Krenko's Buzzcrusher, SOLDIER Military Program)
    if let Some(after) = low.strip_prefix("up to ") {
        let (cnt_tok, rest, excl_self) = if let Some((c, r)) = after.split_once(" other target ") {
            (c, r, true)
        } else if let Some((c, r)) = after.split_once(" target ") {
            (c, r, false)
        } else {
            ("", "", false)
        };
        if !cnt_tok.is_empty() {
            let count = if cnt_tok == "x" || cnt_tok == "that many" {
                TargetCountAst::UpToX
            } else if let Some(n) = parse_number_word(cnt_tok)
                && n >= 0
            {
                TargetCountAst::UpTo(u16::try_from(n).unwrap_or(1))
            } else {
                return Err(format!("bad_upto_target:{raw}"));
            };
            let mut subj = finish_target(rest, count, vocab)?;
            if excl_self && let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
        // Non-targeting "up to N/X/that many <flt>"
        if let Some(flt_raw) = after.strip_prefix("that many ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(grp(flt));
        }
        if let Some((c, flt_raw)) = after.split_once(' ')
            && (c == "x" || parse_number_word(c).is_some())
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(grp(flt));
        }
    }
    // Non-targeting "any number of <flt>" (Blessing of Frost, Nahiri's Resolve, Angelic Aberration)
    if let Some(flt_raw) = low.strip_prefix("any number of ")
        && !flt_raw.starts_with("target ")
        && !flt_raw.starts_with("other target ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(grp(flt));
    }
    // "the first <flt> you cast each turn" (Peri Brown, Maelstrom Nexus, etc.)
    if let Some(flt_raw) = low
        .strip_prefix("the first ")
        .and_then(|r| r.strip_suffix(" each turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(grp(flt));
    }

    // "any number of [other] target <rest>"
    if let Some(rest) = low.strip_prefix("any number of other target ") {
        let mut subj = finish_target(rest, TargetCountAst::AnyNumber, vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }
    if let Some(rest) = low.strip_prefix("any number of target ") {
        return finish_target(rest, TargetCountAst::AnyNumber, vocab);
    }

    // "<N|X> [other] target <rest>"
    if let Some((cnt_tok, rest)) = low.split_once(" other target ") {
        let count = if cnt_tok == "x" {
            Some(TargetCountAst::ChosenX)
        } else if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            Some(TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)))
        } else {
            None
        };
        if let Some(c) = count {
            let mut subj = finish_target(rest, c, vocab)?;
            if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
    }
    if let Some((cnt_tok, rest)) = low.split_once(" target ") {
        if cnt_tok == "x" {
            return finish_target(rest, TargetCountAst::ChosenX, vocab);
        }
        if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            return finish_target(
                rest,
                TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)),
                vocab,
            );
        }
    }

    if let Some(rest) = low.strip_prefix("target ") {
        return finish_target(rest, TargetCountAst::Exact(1), vocab);
    }

    if let Some(rest) = low.strip_prefix("each player who ")
        && let Ok(cond) = parse_condition(&format!("a player {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("you control {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachPlayer,
            condition: Box::new(cond),
        });
    }
    if let Some(rest) = low.strip_prefix("each opponent who ")
        && let Ok(cond) = parse_condition(&format!("an opponent {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("an opponent controls {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachOpponent,
            condition: Box::new(cond),
        });
    }

    if let Some(rest) = low.strip_prefix("each other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("each ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }

    if let Some(rest) = low.strip_prefix("all other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("all ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("those ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(grp(flt));
    }
    if let Some(rest) = low.strip_prefix("any ")
        && !rest.starts_with("target")
        && !rest.starts_with("number of ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(grp(flt));
    }

    let first_err = match parse_object_filter(raw, vocab) {
        Ok(flt) => return Ok(grp(flt)),
        Err(e) => e,
    };

    // Composed multi-subject references: "you and permanents you control",
    // "you or a permanent you control", "target artifact and target creature".
    // Bounded to avoid exponential backtracking on long non-subject clauses.
    if raw.len() <= 84 && (low.matches(" and ").count() + low.matches(" or ").count()) <= 2 {
        for sep in [", and ", " and ", " or "] {
            if let Some((left, right)) = low.split_once(sep) {
                let l = left.trim();
                let r = right.trim();
                if !l.is_empty()
                    && !r.is_empty()
                    && let (Ok(ls), Ok(rs)) = (
                        parse_subject_or_target(l, vocab),
                        parse_subject_or_target(r, vocab),
                    )
                {
                    let mut parts = match ls {
                        SubjectRefAst::Compound(v) => v,
                        other => vec![other],
                    };
                    match rs {
                        SubjectRefAst::Compound(v) => parts.extend(v),
                        other => parts.push(other),
                    }
                    return Ok(SubjectRefAst::Compound(parts));
                }
            }
        }
    }

    Err(first_err)
}

fn parse_compositional_target_one_of(low: &str, vocab: &Vocabulary) -> Option<SubjectRefAst> {
    // Extract leading count + optional "other" before "target "
    let (count, excl_self, rest) = if let Some(after) = low.strip_prefix("another target ") {
        (TargetCountAst::Exact(1), true, after)
    } else if let Some(after) = low.strip_prefix("up to ") {
        if let Some((cnt_s, r)) = after.split_once(" other target ") {
            let c = if cnt_s == "x" {
                TargetCountAst::UpToX
            } else {
                TargetCountAst::UpTo(u16::try_from(parse_number_word(cnt_s)?).ok()?)
            };
            (c, true, r)
        } else if let Some((cnt_s, r)) = after.split_once(" target ") {
            let c = if cnt_s == "x" {
                TargetCountAst::UpToX
            } else {
                TargetCountAst::UpTo(u16::try_from(parse_number_word(cnt_s)?).ok()?)
            };
            (c, false, r)
        } else {
            return None;
        }
    } else {
        let after = low.strip_prefix("target ")?;
        (TargetCountAst::Exact(1), false, after)
    };

    // 1. Cross-zone disjunctive targets: "<flt1> (from|on) the battlefield (and/or|or) <flt2> (from|in) (a graveyard|graveyards)"
    for sep in [" and/or ", " or "] {
        if let Some((left_raw, right_raw)) = rest.split_once(sep)
            && (left_raw.ends_with(" from the battlefield")
                || left_raw.ends_with(" on the battlefield"))
            && (right_raw.ends_with(" from a graveyard")
                || right_raw.ends_with(" from graveyards")
                || right_raw.ends_with(" in a graveyard")
                || right_raw.ends_with(" in graveyards"))
            && let (Ok(mut f1), Ok(f2)) = (
                parse_object_filter(left_raw.trim(), vocab),
                parse_object_filter(right_raw.trim(), vocab),
            )
        {
            if excl_self {
                f1.flags = f1.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Some(SubjectRefAst::TargetOneOf {
                options: vec![
                    SubjectRefAst::TargetObject {
                        filter: Box::new(f1),
                        count,
                    },
                    SubjectRefAst::TargetObject {
                        filter: Box::new(f2),
                        count,
                    },
                ],
                count,
            });
        }
    }

    // 2. Disjunctive permanent/object + opponent/player ("target opponent or battle", "another target battle or opponent", "target artifact, creature, planeswalker, or opponent")
    if let Some(obj_part) = strip_suffix_any(&rest, &[", or opponent", " or opponent"])
        && let Ok(mut flt) = parse_object_filter(obj_part.trim(), vocab)
    {
        if excl_self {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::TargetObject {
                    filter: Box::new(flt),
                    count,
                },
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent),
            ],
            count,
        });
    }
    if let Some(obj_part) = rest.strip_prefix("opponent or ")
        && let Ok(mut flt) = parse_object_filter(obj_part.trim(), vocab)
    {
        if excl_self {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent),
                SubjectRefAst::TargetObject {
                    filter: Box::new(flt),
                    count,
                },
            ],
            count,
        });
    }

    // 3. Disjunctive qualified object targets ("target planeswalker that was activated this turn or tapped creature")
    if rest.contains(" that ")
        && let Some((left_raw, right_raw)) = rest.rsplit_once(" or ")
        && left_raw.contains(" that ")
        && let (Ok(mut f1), Ok(f2)) = (
            parse_object_filter(left_raw.trim(), vocab),
            parse_object_filter(right_raw.trim(), vocab),
        )
    {
        if excl_self {
            f1.flags = f1.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Some(SubjectRefAst::TargetOneOf {
            options: vec![
                SubjectRefAst::TargetObject {
                    filter: Box::new(f1),
                    count,
                },
                SubjectRefAst::TargetObject {
                    filter: Box::new(f2),
                    count,
                },
            ],
            count,
        });
    }

    None
}

fn finish_target(
    rest: &str,
    count: TargetCountAst,
    vocab: &Vocabulary,
) -> Result<SubjectRefAst, String> {
    match rest.trim() {
        "player" | "players" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: false,
            count,
        }),
        "opponent" | "opponents" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: true,
            count,
        }),
        other => {
            let flt = parse_object_filter(other, vocab)?;
            Ok(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count,
            })
        }
    }
}
