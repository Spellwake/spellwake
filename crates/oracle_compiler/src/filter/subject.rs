//! Parsers for player references (`PlayerRefAst`), subjects/targets (`SubjectRefAst`),
//! and keyword ability lists.

use super::condition::parse_condition;
use super::object::parse_object_filter;
use super::zones::peel_zone_suffix;
use crate::ast::{
    ConditionAst, FilterPredicateAst, GameMarker, ObjectFilterAst, ObjectFilterFlags, PileRefAst,
    PlayerRefAst, SubjectRefAst, TargetCountAst,
};
use crate::combinators::trim_punct;
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
        if let Some(arg) = kw
            .strip_prefix("ward ")
            .or_else(|| kw.strip_prefix("toxic "))
            .or_else(|| kw.strip_prefix("afflict "))
            .or_else(|| kw.strip_prefix("afterlife "))
            .or_else(|| kw.strip_prefix("annihilator "))
            .or_else(|| kw.strip_prefix("bushido "))
            .or_else(|| kw.strip_prefix("firebending "))
            .or_else(|| kw.strip_prefix("frenzy "))
            .or_else(|| kw.strip_prefix("poisonous "))
            .or_else(|| kw.strip_prefix("rampage "))
            .or_else(|| kw.strip_prefix("renown "))
            .or_else(|| kw.strip_prefix("absorb "))
            .or_else(|| kw.strip_prefix("modular "))
            .or_else(|| kw.strip_prefix("crew "))
            .or_else(|| kw.strip_prefix("saddle "))
            .or_else(|| kw.strip_prefix("ripple "))
            .or_else(|| kw.strip_prefix("equip "))
            .or_else(|| kw.strip_prefix("dredge "))
            .or_else(|| kw.strip_prefix("prowl "))
            .or_else(|| kw.strip_prefix("miracle "))
            .or_else(|| kw.strip_prefix("affinity for "))
            .or_else(|| kw.strip_prefix("freerunning "))
            .or_else(|| kw.strip_prefix("sticker kicker "))
            .or_else(|| kw.strip_prefix("bloodthirst "))
            .or_else(|| kw.strip_prefix("encore "))
            .or_else(|| kw.strip_prefix("flashback "))
            .or_else(|| kw.strip_prefix("cycling "))
            .or_else(|| kw.strip_prefix("madness "))
            .or_else(|| kw.strip_prefix("kicker "))
            .or_else(|| kw.strip_prefix("buyback "))
            .or_else(|| kw.strip_prefix("overload "))
            .or_else(|| kw.strip_prefix("evoke "))
            .or_else(|| kw.strip_prefix("dash "))
            .or_else(|| kw.strip_prefix("unearth "))
            .or_else(|| kw.strip_prefix("escape "))
            .or_else(|| kw.strip_prefix("embalm "))
            .or_else(|| kw.strip_prefix("eternalize "))
            .or_else(|| kw.strip_prefix("plot "))
            .or_else(|| kw.strip_prefix("warp "))
            .or_else(|| kw.strip_prefix("foretell "))
            .or_else(|| kw.strip_prefix("suspend "))
            .or_else(|| kw.strip_prefix("ninjutsu "))
            .or_else(|| kw.strip_prefix("spectacle "))
            .or_else(|| kw.strip_prefix("blitz "))
            .or_else(|| kw.strip_prefix("disturb "))
            .or_else(|| kw.strip_prefix("scavenge "))
            .or_else(|| kw.strip_prefix("morph "))
            .or_else(|| kw.strip_prefix("megamorph "))
            .or_else(|| kw.strip_prefix("disguise "))
            .or_else(|| kw.strip_prefix("squad "))
            .or_else(|| kw.strip_prefix("offspring "))
            .or_else(|| kw.strip_prefix("web-slinging "))
            .or_else(|| kw.strip_prefix("outlast "))
            .or_else(|| kw.strip_prefix("sneak "))
            .or_else(|| kw.strip_prefix("fabricate "))
            .or_else(|| kw.strip_prefix("casualty "))
            .or_else(|| kw.strip_prefix("backup "))
            .or_else(|| kw.strip_prefix("awaken "))
            .or_else(|| kw.strip_prefix("devour "))
            && !arg.is_empty()
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
        "that creature's controller"
        | "the creature's controller"
        | "that creature's or spell's controller"
        | "that permanent's controller"
        | "the permanent's controller"
        | "that land's controller"
        | "the land's controller"
        | "that artifact's controller"
        | "the artifact's controller"
        | "that enchantment's controller"
        | "the enchantment's controller"
        | "that planeswalker's controller"
        | "the planeswalker's controller"
        | "that battle's controller"
        | "that token's controller"
        | "that aura's controller"
        | "that equipment's controller"
        | "that card's controller"
        | "that object's controller"
        | "that spell's controller"
        | "the spell's controller"
        | "the ability's controller"
        | "that spell's or ability's controller"
        | "that permanent's or spell's controller"
        | "the sacrificed creature's controller"
        | "the sacrificed permanent's controller"
        | "the destroyed creature's controller"
        | "the destroyed permanent's controller"
        | "the exiled permanent's controller"
        | "the exiled creature's controller"
        | "the targeted creature's controller"
        | "the blocked creature's controller"
        | "the attacking creature's controller"
        | "the blocking creature's controller"
        | "enchanted land's controller"
        | "enchanted enchantment's controller"
        | "enchanted planeswalker's controller"
        | "fortified land's controller"
        | "the controller of that spell"
        | "the controller of that permanent"
        | "the controller of that creature"
        | "the controller of that artifact"
        | "the controller of that enchantment"
        | "the controller of that land"
        | "the controller of that planeswalker"
        | "the controller of that ability"
        | "the controller of that spell or ability"
        | "the controller of the countered spell"
        | "the controller of that countered spell" => Some(PlayerRefAst::ThatObjectController),
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
        "their controller"
        | "that player's controller"
        | "~'s controller"
        | "enchanted permanent's controller"
        | "enchanted creature's controller"
        | "enchanted artifact's controller"
        | "equipped creature's controller"
        | "the source's controller"
        | "that source's controller"
        | "that spell or ability's controller" => Some(PlayerRefAst::ThatObjectController),
        "the exiled card's owner"
        | "their owner"
        | "~'s owner"
        | "that creature's owner"
        | "that permanent's owner"
        | "that card's owner"
        | "that spell's owner"
        | "that artifact's owner"
        | "that enchantment's owner"
        | "that land's owner"
        | "that planeswalker's owner"
        | "that battle's owner"
        | "that aura's owner"
        | "that equipment's owner"
        | "that object's owner"
        | "the creature's owner"
        | "the permanent's owner"
        | "the card's owner"
        | "the spell's owner"
        | "the land's owner"
        | "the artifact's owner"
        | "the enchantment's owner"
        | "the planeswalker's owner"
        | "the exiled permanent's owner"
        | "the exiled creature's owner"
        | "the discarded card's owner"
        | "the revealed card's owner"
        | "the milled card's owner"
        | "the sacrificed creature's owner"
        | "the sacrificed permanent's owner"
        | "enchanted creature's owner"
        | "enchanted permanent's owner"
        | "enchanted land's owner"
        | "enchanted artifact's owner"
        | "equipped creature's owner"
        | "the owner of that card"
        | "the owner of that permanent"
        | "the owner of that creature"
        | "the owner of that spell"
        | "the owner of that artifact"
        | "the owner of that enchantment"
        | "the owner of that land"
        | "the owner of that planeswalker"
        | "the owner of the exiled card" => Some(PlayerRefAst::ItsOwner),
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
        "the owner of target permanent" => Some(PlayerRefAst::ItsOwner),
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
        "the controller of target instant or sorcery spell" | "that ability's controller" => {
            Some(PlayerRefAst::ThatObjectController)
        }
        "their controllers" => Some(PlayerRefAst::ThatObjectController),
        "each of its controller's opponents" => Some(PlayerRefAst::EachOtherOpponent),
        "you and the controller of those creatures"
        | "each player starting with you"
        | "each player with the highest number"
        | "each player whose coin comes up tails"
        | "each player who sacrificed a plains this way"
        | "you and each opponent who voted for a choice you voted for" => {
            Some(PlayerRefAst::EachPlayer)
        }
        "each opponent who chose silence" | "each opponent who chose snitch" => {
            Some(PlayerRefAst::EachOpponent)
        }
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
    if let Some(after_ord) = low
        .strip_prefix("a second target ")
        .or_else(|| low.strip_prefix("a third target "))
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

    match low.as_str() {
        "~" => return Ok(SubjectRefAst::SelfObject),
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
        | "that creature"
        | "that permanent"
        | "that card"
        | "that spell"
        | "that spell or ability"
        | "that ability"
        | "that artifact"
        | "that enchantment"
        | "that land"
        | "that planeswalker"
        | "that token"
        | "that aura"
        | "that equipment"
        | "the creature"
        | "the permanent"
        | "the card"
        | "the spell"
        | "the ability"
        | "the artifact"
        | "the enchantment"
        | "the land"
        | "the planeswalker"
        | "the aura"
        | "the equipment"
        | "those creatures"
        | "those permanents"
        | "those cards"
        | "those tokens"
        | "both creatures"
        | "the exiled card"
        | "the exiled cards"
        | "the other exiled cards"
        | "the sacrificed creature"
        | "the sacrificed permanent"
        | "the discarded card"
        | "the revealed card"
        | "the revealed cards"
        | "the chosen creature"
        | "the chosen card"
        | "the last chosen card"
        | "the chosen cards"
        | "the chosen player"
        | "the other chosen player"
        | "the token"
        | "the copy"
        | "the blighted creature"
        | "the chosen creatures"
        | "the army you amassed"
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
        | "the creature you control"
        | "the rest"
        | "the milled card"
        | "the copied spell"
        | "the copies"
        | "that permanent or player"
        | "that player or planeswalker"
        | "that creature or planeswalker"
        | "the aura card"
        | "the targeted creature"
        | "another permanent of that type"
        | "one of the top two cards of your graveyard"
        | "your ring-bearer"
        | "the cards they exiled this way"
        | "the cards you exiled this way"
        | "the player or planeswalker that creature is attacking"
        | "the player or planeswalker it's attacking"
        | "the amassed army"
        | "the bottom card of your library"
        | "target face-up exiled card"
        | "each of the chosen creatures"
        | "each of those creatures"
        | "each of them"
        | "your commander"
        | "a commander you control"
        | "the player you chose"
        | "one of them"
        | "one of those creatures"
        | "one of those cards"
        | "two of those cards"
        | "the other"
        | "those auras"
        | "those lands"
        | "those artifacts"
        | "the chosen permanents"
        | "that wall"
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
        | "the first creature"
        | "the creature you chose"
        | "the creature your opponent chose"
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
        | "target permanent or suspended card" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        "enchanted creature"
        | "enchanted permanent"
        | "enchanted land"
        | "enchanted artifact"
        | "enchanted enchantment"
        | "enchanted planeswalker"
        | "enchanted battle"
        | "enchanted vehicle"
        | "enchanted equipment"
        | "enchanted aura"
        | "fortified land"
        | "equipped creature"
        | "equipped permanent" => return Ok(SubjectRefAst::AttachedHost(low)),
        "any target"
        | "any other target"
        | "another target"
        | "any target chosen at random"
        | "any target of an opponent's choice"
        | "target creature or player"
        | "target opponent or creature"
        | "target player or battle"
        | "one or two targets"
        | "two targets"
        | "three targets"
        | "x targets"
        | "up to x targets"
        | "up to two targets"
        | "up to three targets"
        | "up to four targets"
        | "up to five targets"
        | "up to six targets"
        | "up to two other targets"
        | "up to three other targets"
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
        "the first spell you cast each turn" => {
            return Ok(SubjectRefAst::Anaphora(low));
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
    if let Some(rest) = low
        .strip_prefix("that ")
        .or_else(|| low.strip_prefix("one of those "))
        .or_else(|| low.strip_prefix("either of those "))
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
    if let Some(rest) = low
        .strip_prefix("one or two ")
        .or_else(|| low.strip_prefix("one, two, or three "))
        && !rest.starts_with("target ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
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
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
        if let Some((c, flt_raw)) = after.split_once(' ')
            && (c == "x" || parse_number_word(c).is_some())
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
    }
    // Non-targeting "any number of <flt>" (Blessing of Frost, Nahiri's Resolve, Angelic Aberration)
    if let Some(flt_raw) = low.strip_prefix("any number of ")
        && !flt_raw.starts_with("target ")
        && !flt_raw.starts_with("other target ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }
    // "the first <flt> you cast each turn" (Peri Brown, Maelstrom Nexus, etc.)
    if let Some(flt_raw) = low
        .strip_prefix("the first ")
        .and_then(|r| r.strip_suffix(" each turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
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
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("any ")
        && !rest.starts_with("target")
        && !rest.starts_with("number of ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    let first_err = match parse_object_filter(raw, vocab) {
        Ok(flt) => return Ok(SubjectRefAst::Group(Box::new(flt))),
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
    if let Some(obj_part) = rest
        .strip_suffix(", or opponent")
        .or_else(|| rest.strip_suffix(" or opponent"))
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
