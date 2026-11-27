//! Strictly-typed CR 702 keyword ability parser for `v2`.
//!
//! Handles evergreen, deciduous, and named MTG keyword abilities plus their typed parameters
//! (costs, numbers, filters, color masks, and `everything`).

use crate::ast::{CardTypeMask, ColorMask, CompareOp, ControllerRelation, StatKind};
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    AnaphoraRef, FilterFlags, FilterPredicate, KeywordKind, KeywordParam, KeywordSpec,
    ObjectFilter, SharedProperty, StatBound, SubjectRef, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::vocab::{lookup_card_type, lookup_color};

/// Parse a typed [`KeywordKind`] (multi-word and single-word CR 702 keywords).
pub fn parse_keyword_kind(ts: &mut TokenStream<'_>) -> PResult<KeywordKind> {
    // Multi-word keywords first (longest match)
    if ts.eat_phrase(&["first", "strike"]) {
        return Ok(KeywordKind::FirstStrike);
    }
    if ts.eat_phrase(&["double", "strike"]) {
        return Ok(KeywordKind::DoubleStrike);
    }
    if ts.eat_phrase(&["split", "second"]) {
        return Ok(KeywordKind::SplitSecond);
    }
    if ts.eat_phrase(&["cumulative", "upkeep"]) {
        return Ok(KeywordKind::CumulativeUpkeep);
    }
    if ts.eat_phrase(&["basic", "landcycling"]) {
        return Ok(KeywordKind::BasicLandcycling);
    }
    if ts.eat_phrase(&["battle", "cry"]) {
        return Ok(KeywordKind::BattleCry);
    }
    if ts.eat_phrase(&["living", "weapon"]) {
        return Ok(KeywordKind::LivingWeapon);
    }
    if ts.eat_phrase(&["umbra", "armor"]) || ts.eat_phrase(&["totem", "armor"]) {
        return Ok(KeywordKind::UmbraArmor);
    }
    if ts.eat_phrase(&["level", "up"]) {
        return Ok(KeywordKind::LevelUp);
    }
    if ts.eat_phrase(&["aura", "swap"]) {
        return Ok(KeywordKind::AuraSwap);
    }
    if ts.eat_phrase(&["hidden", "agenda"]) {
        return Ok(KeywordKind::HiddenAgenda);
    }
    if ts.eat_phrase(&["read", "ahead"]) {
        return Ok(KeywordKind::ReadAhead);
    }
    if ts.eat_phrase(&["commander", "ninjutsu"]) {
        return Ok(KeywordKind::NinjutsuCommander);
    }
    if ts.eat_phrase(&["choose", "a", "background"]) {
        return Ok(KeywordKind::ChooseABackground);
    }
    if ts.eat_phrase(&["doctor's", "companion"]) {
        return Ok(KeywordKind::DoctorsCompanion);
    }
    if ts.eat_phrase(&["friends", "forever"]) {
        return Ok(KeywordKind::FriendsForever);
    }
    if ts.eat_phrase(&["job", "select"]) {
        return Ok(KeywordKind::JobSelect);
    }
    if ts.eat_phrase(&["more", "than", "meets", "the", "eye"]) {
        return Ok(KeywordKind::MoreThanMeetsTheEye);
    }
    if ts.eat_phrase(&["living", "metal"]) {
        return Ok(KeywordKind::LivingMetal);
    }
    if ts.eat_phrase(&["for", "mirrodin"]) {
        if matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Bang)) {
            ts.next_tok();
        }
        return Ok(KeywordKind::ForMirrodin);
    }
    if ts.eat_phrase(&["start", "your", "engines"]) {
        if matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Bang)) {
            ts.next_tok();
        }
        return Ok(KeywordKind::StartYourEngines);
    }
    // Multi-word landwalk variants (`snow forestwalk`, `snow swampwalk`, `snow landwalk`,
    // `nonbasic landwalk`, `legendary landwalk`, `artifact landwalk`, `basic landwalk`)
    {
        let lw_cp = ts.checkpoint();
        if (ts.eat_word("snow")
            || ts.eat_word("nonbasic")
            || ts.eat_word("legendary")
            || ts.eat_word("artifact")
            || ts.eat_word("basic"))
            && let Some(nw) = ts.peek_word()
            && nw.to_ascii_lowercase().ends_with("walk")
            && nw.len() > "walk".len()
        {
            ts.next_word();
            return Ok(KeywordKind::Landwalk);
        }
        ts.restore(lw_cp);
    }

    let cp = ts.checkpoint();
    let Some(w) = ts.next_word() else {
        return Err(ts.err("expected keyword"));
    };
    let low = w.to_ascii_lowercase();
    let kind = match low.as_str() {
        "flying" => KeywordKind::Flying,
        "deathtouch" => KeywordKind::Deathtouch,
        "haste" => KeywordKind::Haste,
        "hexproof" => KeywordKind::Hexproof,
        "indestructible" => KeywordKind::Indestructible,
        "lifelink" => KeywordKind::Lifelink,
        "menace" => KeywordKind::Menace,
        "reach" => KeywordKind::Reach,
        "trample" => KeywordKind::Trample,
        "vigilance" => KeywordKind::Vigilance,
        "defender" => KeywordKind::Defender,
        "flash" => KeywordKind::Flash,
        "shroud" => KeywordKind::Shroud,
        "ward" => KeywordKind::Ward,
        "equip" => KeywordKind::Equip,
        "kicker" => KeywordKind::Kicker,
        "multikicker" => KeywordKind::Multikicker,
        "flashback" => KeywordKind::Flashback,
        "cycling" => KeywordKind::Cycling,
        "suspend" => KeywordKind::Suspend,
        "spree" => KeywordKind::Spree,
        "protection" => KeywordKind::Protection,
        "enchant" => KeywordKind::Enchant,
        "toxic" => KeywordKind::Toxic,
        "afflict" => KeywordKind::Afflict,
        "annihilator" => KeywordKind::Annihilator,
        "cascade" => KeywordKind::Cascade,
        "convoke" => KeywordKind::Convoke,
        "delve" => KeywordKind::Delve,
        "prowess" => KeywordKind::Prowess,
        "wither" => KeywordKind::Wither,
        "infect" => KeywordKind::Infect,
        "undying" => KeywordKind::Undying,
        "persist" => KeywordKind::Persist,
        "exalted" => KeywordKind::Exalted,
        "extort" => KeywordKind::Extort,
        "fear" => KeywordKind::Fear,
        "intimidate" => KeywordKind::Intimidate,
        "skulk" => KeywordKind::Skulk,
        "shadow" => KeywordKind::Shadow,
        "horsemanship" => KeywordKind::Horsemanship,
        "flanking" => KeywordKind::Flanking,
        "banding" => KeywordKind::Banding,
        "changeling" => KeywordKind::Changeling,
        "devoid" => KeywordKind::Devoid,
        "storm" => KeywordKind::Storm,
        "rebound" => KeywordKind::Rebound,
        "retrace" => KeywordKind::Retrace,
        "buyback" => KeywordKind::Buyback,
        "entwine" => KeywordKind::Entwine,
        "overload" => KeywordKind::Overload,
        "miracle" => KeywordKind::Miracle,
        "foretell" => KeywordKind::Foretell,
        "plot" => KeywordKind::Plot,
        "warp" => KeywordKind::Warp,
        "escape" => KeywordKind::Escape,
        "unearth" => KeywordKind::Unearth,
        "blitz" => KeywordKind::Blitz,
        "dash" => KeywordKind::Dash,
        "evoke" => KeywordKind::Evoke,
        "bestow" => KeywordKind::Bestow,
        "ninjutsu" => KeywordKind::Ninjutsu,
        "morph" => KeywordKind::Morph,
        "megamorph" => KeywordKind::Megamorph,
        "disguise" => KeywordKind::Disguise,
        "crew" => KeywordKind::Crew,
        "saddle" => KeywordKind::Saddle,
        "station" => KeywordKind::Station,
        "partner" => KeywordKind::Partner,
        "companion" => KeywordKind::Companion,
        "fuse" => KeywordKind::Fuse,
        "plainswalk" => KeywordKind::Plainswalk,
        "islandwalk" => KeywordKind::Islandwalk,
        "swampwalk" => KeywordKind::Swampwalk,
        "mountainwalk" => KeywordKind::Mountainwalk,
        "forestwalk" => KeywordKind::Forestwalk,
        "landwalk" => KeywordKind::Landwalk,
        "affinity" => KeywordKind::Affinity,
        "madness" => KeywordKind::Madness,
        "echo" => KeywordKind::Echo,
        "vanishing" => KeywordKind::Vanishing,
        "fading" => KeywordKind::Fading,
        "soulbond" => KeywordKind::Soulbond,
        "evolve" => KeywordKind::Evolve,
        "outlast" => KeywordKind::Outlast,
        "exploit" => KeywordKind::Exploit,
        "renown" => KeywordKind::Renown,
        "awaken" => KeywordKind::Awaken,
        "surge" => KeywordKind::Surge,
        "emerge" => KeywordKind::Emerge,
        "escalate" => KeywordKind::Escalate,
        "fabricate" => KeywordKind::Fabricate,
        "improvise" => KeywordKind::Improvise,
        "aftermath" => KeywordKind::Aftermath,
        "embalm" => KeywordKind::Embalm,
        "eternalize" => KeywordKind::Eternalize,
        "ascend" => KeywordKind::Ascend,
        "assist" => KeywordKind::Assist,
        "jump-start" => KeywordKind::JumpStart,
        "mentor" => KeywordKind::Mentor,
        "riot" => KeywordKind::Riot,
        "spectacle" => KeywordKind::Spectacle,
        "afterlife" => KeywordKind::Afterlife,
        "mutate" => KeywordKind::Mutate,
        "encore" => KeywordKind::Encore,
        "boast" => KeywordKind::Boast,
        "disturb" => KeywordKind::Disturb,
        "decayed" => KeywordKind::Decayed,
        "daybound" => KeywordKind::Daybound,
        "nightbound" => KeywordKind::Nightbound,
        "cleave" => KeywordKind::Cleave,
        "training" => KeywordKind::Training,
        "reconfigure" => KeywordKind::Reconfigure,
        "compleated" => KeywordKind::Compleated,
        "casualty" => KeywordKind::Casualty,
        "myriad" => KeywordKind::Myriad,
        "enlist" => KeywordKind::Enlist,
        "squad" => KeywordKind::Squad,
        "prototype" => KeywordKind::Prototype,
        "backup" => KeywordKind::Backup,
        "bargain" => KeywordKind::Bargain,
        "craft" => KeywordKind::Craft,
        "offspring" => KeywordKind::Offspring,
        "impending" => KeywordKind::Impending,
        "harmonize" => KeywordKind::Harmonize,
        "mobilize" => KeywordKind::Mobilize,
        "phasing" => KeywordKind::Phasing,
        "rampage" => KeywordKind::Rampage,
        "bushido" => KeywordKind::Bushido,
        "modular" => KeywordKind::Modular,
        "sunburst" => KeywordKind::Sunburst,
        "dredge" => KeywordKind::Dredge,
        "bloodthirst" => KeywordKind::Bloodthirst,
        "graft" => KeywordKind::Graft,
        "ripple" => KeywordKind::Ripple,
        "recover" => KeywordKind::Recover,
        "frenzy" => KeywordKind::Frenzy,
        "absorb" => KeywordKind::Absorb,
        "fortify" => KeywordKind::Fortify,
        "gravestorm" => KeywordKind::Gravestorm,
        "poisonous" => KeywordKind::Poisonous,
        "transfigure" => KeywordKind::Transfigure,
        "champion" => KeywordKind::Champion,
        "hideaway" => KeywordKind::Hideaway,
        "prowl" => KeywordKind::Prowl,
        "reinforce" => KeywordKind::Reinforce,
        "conspire" => KeywordKind::Conspire,
        "devour" => KeywordKind::Devour,
        "scavenge" => KeywordKind::Scavenge,
        "unleash" => KeywordKind::Unleash,
        "cipher" => KeywordKind::Cipher,
        "tribute" => KeywordKind::Tribute,
        "dethrone" => KeywordKind::Dethrone,
        "melee" => KeywordKind::Melee,
        "undaunted" => KeywordKind::Undaunted,
        "ingest" => KeywordKind::Ingest,
        "meld" => KeywordKind::Meld,
        "ravenous" => KeywordKind::Ravenous,
        "demonstrate" => KeywordKind::Demonstrate,
        "freerunning" => KeywordKind::Freerunning,
        "splice" => KeywordKind::Splice,
        "soulshift" => KeywordKind::Soulshift,
        "firebending" => KeywordKind::Firebending,
        "epic" => KeywordKind::Epic,
        "replicate" => KeywordKind::Replicate,
        "sneak" => KeywordKind::Sneak,
        "teamwork" => KeywordKind::Teamwork,
        "mayhem" => KeywordKind::Mayhem,
        "amplify" => KeywordKind::Amplify,
        "gift" => KeywordKind::Gift,
        "provoke" => KeywordKind::Provoke,
        "transmute" => KeywordKind::Transmute,
        "increment" => KeywordKind::Increment,
        "paradigm" => KeywordKind::Paradigm,
        "haunt" => KeywordKind::Haunt,
        "storied" => KeywordKind::Storied,
        "tiered" => KeywordKind::Tiered,
        "web-slinging" => KeywordKind::WebSlinging,
        _ if (ts.vocab.lookup_subtype(&low).is_some() || lookup_card_type(&low).is_some())
            && ts.eat_word("offering") =>
        {
            KeywordKind::Offering
        }
        other if other.ends_with("cycling") && other.len() > "cycling".len() => {
            KeywordKind::Typecycling
        }
        other if other.ends_with("walk") && other.len() > "walk".len() => KeywordKind::Landwalk,
        _ => {
            ts.restore(cp);
            return Err(ts.err("expected known MTG keyword"));
        }
    };
    Ok(kind)
}

/// Parse a keyword specification including optional typed parameter.
pub fn parse_keyword_spec(ts: &mut TokenStream<'_>) -> PResult<KeywordSpec> {
    let kind = parse_keyword_kind(ts)?;
    let param = match kind {
        KeywordKind::Equip => {
            // `Equip {2}`, `Equip Soldier {1}`, `Equip legendary creature {2}`, `Equip worthy {1}`
            let _ = ts.eat_em_dash();
            if let Ok(costs) = try_parse(ts, parse_cost_list) {
                Some(KeywordParam::Cost(costs))
            } else if ts.eat_word("worthy")
                && let Ok(costs) = try_parse(ts, parse_cost_list)
            {
                Some(KeywordParam::FilterAndCost(Box::default(), costs))
            } else if let Ok(flt) = try_parse(ts, parse_object_filter)
                && let Ok(costs) = try_parse(ts, parse_cost_list)
            {
                Some(KeywordParam::FilterAndCost(Box::new(flt), costs))
            } else {
                None
            }
        }
        KeywordKind::Suspend
        | KeywordKind::Awaken
        | KeywordKind::Impending
        | KeywordKind::Reinforce => {
            // `Suspend 3 - {1}{R}`, `Suspend X - {X}{W}{W}`, `Awaken 4 - {4}{W}{W}`, `Reinforce X - {X}{G}{G}`
            if let Some(n) = ts.next_number(false).and_then(|v| u16::try_from(v).ok()) {
                let _ = ts.eat_em_dash();
                if let Ok(costs) = try_parse(ts, parse_cost_list) {
                    Some(KeywordParam::NumberAndCost(n, costs))
                } else {
                    Some(KeywordParam::Number(n))
                }
            } else if ts.eat_word("x") {
                let _ = ts.eat_em_dash();
                if let Ok(costs) = try_parse(ts, parse_cost_list) {
                    Some(KeywordParam::Cost(costs))
                } else {
                    Some(KeywordParam::VariableX)
                }
            } else {
                None
            }
        }
        KeywordKind::Trample => {
            // `Trample over planeswalkers`
            if ts.eat_phrase(&["over", "planeswalkers"]) {
                Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    any_types: CardTypeMask::PLANESWALKER,
                    ..Default::default()
                })))
            } else {
                None
            }
        }
        KeywordKind::Landwalk => {
            // `landwalk of the chosen type`, `landwalk of that type`,
            // `landwalk of each of the land types of the sacrificed land`
            if ts.eat_phrase(&["of", "the", "chosen", "type"])
                || ts.eat_phrase(&["of", "that", "type"])
            {
                Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    predicates: vec![FilterPredicate::OfChosenCreatureType],
                    ..Default::default()
                })))
            } else if ts.eat_phrase(&["of", "each", "of", "the", "land", "types", "of"])
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    predicates: vec![FilterPredicate::SharesPropertyWith {
                        property: SharedProperty::LandType,
                        reference: Box::new(subj),
                        negated: false,
                    }],
                    ..Default::default()
                })))
            } else {
                None
            }
        }
        KeywordKind::Emerge => {
            // `Emerge {5}{U}{U}` OR `Emerge from artifact {5}{B}{B}`
            let ecp = ts.checkpoint();
            if ts.eat_word("from")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && let Ok(costs) = try_parse(ts, parse_cost_list)
            {
                Some(KeywordParam::FilterAndCost(Box::new(flt), costs))
            } else {
                ts.restore(ecp);
                let _ = ts.eat_em_dash();
                opt(ts, parse_cost_list).map(KeywordParam::Cost)
            }
        }
        KeywordKind::Modular => {
            // `Modular 2`, `Modular X`, `Modular - Sunburst`
            let mcp = ts.checkpoint();
            if ts.eat_em_dash() && ts.eat_word("sunburst") {
                Some(KeywordParam::VariableX)
            } else {
                ts.restore(mcp);
                if ts.eat_word("x") {
                    Some(KeywordParam::VariableX)
                } else {
                    ts.next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .map(KeywordParam::Number)
                }
            }
        }
        KeywordKind::Devour => {
            // `Devour 2`, `Devour artifact 1`, `Devour Food 3`, `Devour land 3`, `Devour X, where X is ...`
            if ts.eat_word("x") {
                let wcp = ts.checkpoint();
                if ts.eat_comma()
                    && ts.eat_phrase(&["where", "x", "is"])
                    && let Ok(expr) = try_parse(ts, crate::v2::value::parse_value_expr)
                {
                    Some(KeywordParam::VariableXBound(Box::new(expr)))
                } else {
                    ts.restore(wcp);
                    Some(KeywordParam::VariableX)
                }
            } else if let Some(n) = ts.next_number(false).and_then(|v| u16::try_from(v).ok()) {
                Some(KeywordParam::Number(n))
            } else {
                let dcp = ts.checkpoint();
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let _ = ts.next_number(false);
                    Some(KeywordParam::Filter(Box::new(flt)))
                } else {
                    ts.restore(dcp);
                    None
                }
            }
        }
        KeywordKind::CumulativeUpkeep => {
            let _ = ts.eat_em_dash();
            if let Some(costs) = opt(ts, parse_cost_list) {
                Some(KeywordParam::Cost(costs))
            } else {
                // Non-standard effect cumulative upkeeps (e.g. `Draw a card.`, `Add {R}.`, `Flip a coin.`)
                let ecp = ts.checkpoint();
                let start = ts.pos;
                let mut end = start;
                while end < ts.tokens.len()
                    && !matches!(
                        ts.tokens[end].kind,
                        TokenKind::Period | TokenKind::Semicolon
                    )
                {
                    end += 1;
                }
                if end > start {
                    let mut sub =
                        ts.sub_stream(&ts.tokens[start..end], crate::v2::ir::ParseStage::Effect);
                    if crate::v2::clause::parse_effect_sentence(&mut sub).is_ok() {
                        ts.pos = end;
                        Some(KeywordParam::Cost(Vec::new()))
                    } else {
                        ts.restore(ecp);
                        None
                    }
                } else {
                    None
                }
            }
        }
        KeywordKind::Ward
        | KeywordKind::Kicker
        | KeywordKind::Multikicker
        | KeywordKind::Flashback
        | KeywordKind::Cycling
        | KeywordKind::Typecycling
        | KeywordKind::BasicLandcycling
        | KeywordKind::Foretell
        | KeywordKind::Plot
        | KeywordKind::Warp
        | KeywordKind::Escape
        | KeywordKind::Unearth
        | KeywordKind::Blitz
        | KeywordKind::Dash
        | KeywordKind::Evoke
        | KeywordKind::Bestow
        | KeywordKind::Ninjutsu
        | KeywordKind::NinjutsuCommander
        | KeywordKind::Morph
        | KeywordKind::Megamorph
        | KeywordKind::Disguise
        | KeywordKind::Buyback
        | KeywordKind::Entwine
        | KeywordKind::Overload
        | KeywordKind::Miracle
        | KeywordKind::Madness
        | KeywordKind::Echo
        | KeywordKind::Outlast
        | KeywordKind::Surge
        | KeywordKind::Escalate
        | KeywordKind::Embalm
        | KeywordKind::Eternalize
        | KeywordKind::Spectacle
        | KeywordKind::Mutate
        | KeywordKind::Encore
        | KeywordKind::Disturb
        | KeywordKind::Cleave
        | KeywordKind::Reconfigure
        | KeywordKind::Squad
        | KeywordKind::Offspring
        | KeywordKind::Harmonize
        | KeywordKind::Recover
        | KeywordKind::AuraSwap
        | KeywordKind::Fortify
        | KeywordKind::Transfigure
        | KeywordKind::Prowl
        | KeywordKind::LevelUp
        | KeywordKind::Scavenge
        | KeywordKind::Freerunning
        | KeywordKind::MoreThanMeetsTheEye
        | KeywordKind::Replicate
        | KeywordKind::Sneak
        | KeywordKind::Mayhem
        | KeywordKind::Transmute
        | KeywordKind::WebSlinging => {
            let _ = ts.eat_em_dash();
            opt(ts, parse_cost_list).map(KeywordParam::Cost)
        }
        KeywordKind::Gift => {
            use crate::v2::ir::GiftKind;
            if ts.eat_phrase(&["a", "card"]) {
                Some(KeywordParam::Gift(GiftKind::Card))
            } else if ts.eat_phrase(&["a", "treasure"]) {
                Some(KeywordParam::Gift(GiftKind::Treasure))
            } else if ts.eat_phrase(&["a", "food"]) {
                Some(KeywordParam::Gift(GiftKind::Food))
            } else if ts.eat_phrase(&["a", "tapped", "fish"]) {
                Some(KeywordParam::Gift(GiftKind::TappedFish))
            } else if ts.eat_phrase(&["an", "octopus"]) {
                Some(KeywordParam::Gift(GiftKind::Octopus))
            } else if ts.eat_phrase(&["an", "extra", "turn"]) {
                Some(KeywordParam::Gift(GiftKind::ExtraTurn))
            } else {
                None
            }
        }
        KeywordKind::Prototype => {
            // `Prototype {1}{B} — 1/1`
            let costs = opt(ts, parse_cost_list).unwrap_or_default();
            let _ = ts.eat_em_dash();
            if let Some((p, t)) = ts.next_base_pt() {
                Some(KeywordParam::CostAndBasePt(costs, p, t))
            } else if !costs.is_empty() {
                Some(KeywordParam::Cost(costs))
            } else {
                None
            }
        }
        KeywordKind::Craft => {
            // `Craft with <artifact/four or more nonlands/.../a Dinosaur, a Merfolk, a Pirate, and a Vampire> {cost}`
            if ts.eat_word("with") {
                if ts.next_number(true).is_some() {
                    let _ = ts.eat_phrase(&["or", "more"]);
                }
                let first_flt = opt(ts, parse_object_filter).unwrap_or_default();
                let mut flts = vec![first_flt];
                loop {
                    let ccp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_and = ts.eat_word("and");
                    if (had_comma || had_and)
                        && let Ok(next_flt) = try_parse(ts, parse_object_filter)
                    {
                        flts.push(next_flt);
                        continue;
                    }
                    ts.restore(ccp);
                    break;
                }
                let _ = ts.eat_phrase(&["that", "share", "a", "card", "type"])
                    || ts.eat_phrase(&["with", "different", "names"]);
                let costs = opt(ts, parse_cost_list).unwrap_or_default();
                let flt = if flts.len() == 1 {
                    flts.remove(0)
                } else {
                    ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(flts)],
                        ..Default::default()
                    }
                };
                Some(KeywordParam::FilterAndCost(Box::new(flt), costs))
            } else {
                None
            }
        }
        KeywordKind::Crew
        | KeywordKind::Saddle
        | KeywordKind::Toxic
        | KeywordKind::Afflict
        | KeywordKind::Annihilator
        | KeywordKind::Station
        | KeywordKind::Vanishing
        | KeywordKind::Fading
        | KeywordKind::Renown
        | KeywordKind::Fabricate
        | KeywordKind::Afterlife
        | KeywordKind::Casualty
        | KeywordKind::Backup
        | KeywordKind::Mobilize
        | KeywordKind::Rampage
        | KeywordKind::Bushido
        | KeywordKind::Dredge
        | KeywordKind::Bloodthirst
        | KeywordKind::Graft
        | KeywordKind::Ripple
        | KeywordKind::Frenzy
        | KeywordKind::Absorb
        | KeywordKind::Poisonous
        | KeywordKind::Hideaway
        | KeywordKind::Tribute
        | KeywordKind::Soulshift
        | KeywordKind::Firebending
        | KeywordKind::Teamwork
        | KeywordKind::Amplify => {
            if ts.eat_word("x") {
                let wcp = ts.checkpoint();
                if ts.eat_comma()
                    && ts.eat_phrase(&["where", "x", "is"])
                    && let Ok(expr) = try_parse(ts, crate::v2::value::parse_value_expr)
                {
                    Some(KeywordParam::VariableXBound(Box::new(expr)))
                } else {
                    ts.restore(wcp);
                    Some(KeywordParam::VariableX)
                }
            } else {
                ts.next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .map(KeywordParam::Number)
            }
        }
        KeywordKind::Splice => {
            if ts.eat_word("onto")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && let Ok(costs) = try_parse(ts, parse_cost_list)
            {
                Some(KeywordParam::FilterAndCost(Box::new(flt), costs))
            } else {
                None
            }
        }
        KeywordKind::Affinity => {
            if ts.eat_word("for") {
                opt(ts, parse_object_filter).map(|f| KeywordParam::Filter(Box::new(f)))
            } else {
                None
            }
        }
        KeywordKind::Protection | KeywordKind::Hexproof => parse_from_clause(ts),
        KeywordKind::Enchant => {
            if ts.eat_word("player") || ts.eat_word("opponent") {
                None
            } else {
                opt(ts, parse_object_filter).map(|f| KeywordParam::Filter(Box::new(f)))
            }
        }
        KeywordKind::Champion => {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            opt(ts, parse_object_filter).map(|f| KeywordParam::Filter(Box::new(f)))
        }
        KeywordKind::Partner => {
            if ts.eat_em_dash() {
                let mut parts = Vec::new();
                while let Some(w) = ts.next_word() {
                    parts.push(w);
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(KeywordParam::PartnerName(parts.join(" ")))
                }
            } else if ts.eat_word("with") {
                let mut parts = Vec::new();
                while let Some(w) = ts.next_word() {
                    parts.push(w);
                    if ts.eat_comma() {
                        parts.push(",");
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(KeywordParam::PartnerName(parts.join(" ")))
                }
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(KeywordSpec { kind, param })
}

fn parse_from_clause(ts: &mut TokenStream<'_>) -> Option<KeywordParam> {
    if !ts.eat_word("from") {
        return None;
    }
    if ts.eat_word("everything") {
        return Some(KeywordParam::Everything);
    }
    // `from each color among <flt>` / `from each color that's not in your commander's color identity`
    if ts.eat_phrase(&["each", "color"]) {
        if ts.eat_word("among")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(KeywordParam::Filter(Box::new(flt)));
        }
        if ts.eat_phrase(&[
            "that's",
            "not",
            "in",
            "your",
            "commander's",
            "color",
            "identity",
        ]) {
            return Some(KeywordParam::ChosenColor);
        }
        let all_colors = ColorMask::WHITE
            .union(ColorMask::BLUE)
            .union(ColorMask::BLACK)
            .union(ColorMask::RED)
            .union(ColorMask::GREEN);
        return Some(KeywordParam::Color(all_colors));
    }
    if ts.eat_phrase(&["all", "colors"]) || ts.eat_phrase(&["any", "color"]) {
        let all_colors = ColorMask::WHITE
            .union(ColorMask::BLUE)
            .union(ColorMask::BLACK)
            .union(ColorMask::RED)
            .union(ColorMask::GREEN);
        return Some(KeywordParam::Color(all_colors));
    }
    if ts.eat_phrase(&["the", "color", "of", "your", "choice"])
        || ts.eat_phrase(&["a", "color", "of", "your", "choice"])
        || ts.eat_phrase(&["the", "color", "of", "its", "controller's", "choice"])
        || ts.eat_phrase(&["a", "color", "of", "its", "controller's", "choice"])
        || ts.eat_phrase(&["the", "last", "chosen", "color"])
        || ts.eat_phrase(&["the", "chosen", "color"])
        || ts.eat_phrase(&["that", "color"])
        || ts.eat_phrase(&[
            "colorless",
            "or",
            "from",
            "the",
            "color",
            "of",
            "your",
            "choice",
        ])
        || ts.eat_phrase(&[
            "colorless",
            "or",
            "from",
            "a",
            "color",
            "of",
            "your",
            "choice",
        ])
    {
        return Some(KeywordParam::ChosenColor);
    }
    if ts.eat_phrase(&["the", "chosen", "player"])
        || ts.eat_phrase(&["that", "player"])
        || ts.eat_phrase(&["that", "opponent"])
        || ts.eat_phrase(&["target", "player"])
        || ts.eat_phrase(&["target", "opponent"])
    {
        return Some(KeywordParam::ChosenPlayer);
    }
    if ts.eat_phrase(&["the", "chosen", "card", "name"])
        || ts.eat_phrase(&["the", "chosen", "name"])
    {
        return Some(KeywordParam::ChosenCardName);
    }
    if ts.eat_phrase(&["the", "chosen", "card", "type"])
        || ts.eat_phrase(&["the", "chosen", "creature", "type"])
        || ts.eat_phrase(&["the", "chosen", "type"])
    {
        return Some(KeywordParam::Filter(Box::new(ObjectFilter {
            predicates: vec![FilterPredicate::OfChosenCreatureType],
            ..Default::default()
        })));
    }
    // `from each of your opponents`
    if ts.eat_phrase(&["each", "of", "your", "opponents"]) || ts.eat_phrase(&["each", "opponent"]) {
        return Some(KeywordParam::Filter(Box::new(ObjectFilter {
            controller: Some(ControllerRelation::Opponents),
            ..Default::default()
        })));
    }
    // `from each of its colors` / `from each of that creature's colors` / `from each of the exiled card's card types`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["each", "of"]) {
            let subj_opt = if ts.eat_word("its") || ts.eat_word("their") {
                Some(SubjectRef::SelfObject)
            } else if ts.eat_phrase(&["that", "creature's"]) {
                Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
            } else if ts.eat_phrase(&["that", "permanent's"]) {
                Some(SubjectRef::Anaphora(AnaphoraRef::ThatPermanent))
            } else if ts.eat_phrase(&["that", "spell's"]) {
                Some(SubjectRef::Anaphora(AnaphoraRef::ThatSpell))
            } else if ts.eat_phrase(&["the", "exiled", "card's"])
                || ts.eat_phrase(&["that", "card's"])
            {
                Some(SubjectRef::Anaphora(AnaphoraRef::TheExiledCard))
            } else {
                None
            };
            if let Some(reference) = subj_opt {
                if ts.eat_word("colors") || ts.eat_word("color") {
                    return Some(KeywordParam::Filter(Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::SharesPropertyWith {
                            property: SharedProperty::Color,
                            reference: Box::new(reference),
                            negated: false,
                        }],
                        ..Default::default()
                    })));
                }
                if ts.eat_phrase(&["card", "types"]) || ts.eat_phrase(&["card", "type"]) {
                    return Some(KeywordParam::Filter(Box::new(ObjectFilter {
                        predicates: vec![FilterPredicate::SharesPropertyWith {
                            property: SharedProperty::CardType,
                            reference: Box::new(reference),
                            negated: false,
                        }],
                        ..Default::default()
                    })));
                }
            }
        }
        ts.restore(ecp);
    }
    // `from each mana value (among <flt> | of the chosen quality | other than the chosen number)`
    {
        let mv_cp = ts.checkpoint();
        if ts.eat_phrase(&["each", "mana", "value"]) {
            if ts.eat_word("among")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Some(KeywordParam::Filter(Box::new(flt)));
            }
            if ts.eat_phrase(&["of", "the", "chosen", "quality"])
                || ts.eat_phrase(&["other", "than", "the", "chosen", "number"])
            {
                return Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::ManaValue,
                        op: CompareOp::Eq,
                        value: Box::new(ValueExpr::Const(0)),
                    }],
                    ..Default::default()
                })));
            }
        }
        ts.restore(mv_cp);
    }
    // `from even mana values` / `from odd mana values` / `from mana value <N> or less|greater`
    if ts.eat_phrase(&["even", "mana", "values"]) || ts.eat_phrase(&["even", "mana", "value"]) {
        return Some(KeywordParam::Filter(Box::new(ObjectFilter {
            stat_bounds: vec![StatBound {
                stat: StatKind::ManaValue,
                op: CompareOp::Eq,
                value: Box::new(ValueExpr::Const(0)),
            }],
            ..Default::default()
        })));
    }
    if ts.eat_phrase(&["odd", "mana", "values"]) || ts.eat_phrase(&["odd", "mana", "value"]) {
        return Some(KeywordParam::Filter(Box::new(ObjectFilter {
            stat_bounds: vec![StatBound {
                stat: StatKind::ManaValue,
                op: CompareOp::Eq,
                value: Box::new(ValueExpr::Const(1)),
            }],
            ..Default::default()
        })));
    }
    {
        let mv_cp = ts.checkpoint();
        if ts.eat_phrase(&["mana", "value"])
            && let Ok(val) = try_parse(ts, crate::v2::value::parse_value_expr)
        {
            let op = if ts.eat_phrase(&["or", "less"]) {
                CompareOp::Le
            } else if ts.eat_phrase(&["or", "greater"]) {
                CompareOp::Ge
            } else {
                CompareOp::Eq
            };
            return Some(KeywordParam::Filter(Box::new(ObjectFilter {
                stat_bounds: vec![StatBound {
                    stat: StatKind::ManaValue,
                    op,
                    value: Box::new(val),
                }],
                ..Default::default()
            })));
        }
        ts.restore(mv_cp);
    }
    if ts.eat_word("ring-bearers") || ts.eat_word("ring-bearer") {
        return Some(KeywordParam::Filter(Box::new(ObjectFilter {
            any_types: CardTypeMask::CREATURE,
            flags: FilterFlags::MODIFIED,
            ..Default::default()
        })));
    }
    let mut colors = ColorMask::EMPTY;
    let cp = ts.checkpoint();
    while let Some(w) = ts.peek_word() {
        if let Some(c) = lookup_color(&w.to_ascii_lowercase()) {
            ts.next_word();
            colors = colors.union(c);
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "from"]) || ts.eat_word("from") || ts.eat_word("and") {
                continue;
            }
            break;
        }
        break;
    }
    if !colors.is_empty() {
        return Some(KeywordParam::Color(colors));
    }
    ts.restore(cp);
    let first = opt(ts, parse_object_filter)?;
    let mut filters = vec![first];
    loop {
        let fcp = ts.checkpoint();
        let _ = ts.eat_comma();
        if ts.eat_phrase(&["and", "from"]) || ts.eat_word("from") {
            if ts.eat_phrase(&["each", "color"]) || ts.eat_phrase(&["all", "colors"]) {
                let all_colors = ColorMask::WHITE
                    .union(ColorMask::BLUE)
                    .union(ColorMask::BLACK)
                    .union(ColorMask::RED)
                    .union(ColorMask::GREEN);
                filters.push(ObjectFilter {
                    any_colors: all_colors,
                    ..Default::default()
                });
                continue;
            }
            if let Ok(next_f) = try_parse(ts, parse_object_filter) {
                filters.push(next_f);
                continue;
            }
        }
        ts.restore(fcp);
        if ts.eat_phrase(&["or", "from", "the", "color", "of", "your", "choice"])
            || ts.eat_phrase(&["or", "from", "a", "color", "of", "your", "choice"])
        {
            return Some(KeywordParam::ChosenColor);
        }
        ts.restore(fcp);
        break;
    }
    if filters.len() == 1 {
        Some(KeywordParam::Filter(Box::new(filters.remove(0))))
    } else {
        Some(KeywordParam::Filter(Box::new(ObjectFilter {
            predicates: vec![FilterPredicate::AnyOf(filters)],
            ..Default::default()
        })))
    }
}

/// Parse a comma/semicolon/`and`/`or`-separated list of keywords (`flying, deathtouch, or hexproof`).
pub fn parse_keyword_list(ts: &mut TokenStream<'_>) -> PResult<(Vec<KeywordSpec>, bool)> {
    let first = parse_keyword_spec(ts)?;
    let mut kws = vec![first];
    let mut is_or = false;
    loop {
        let cp = ts.checkpoint();
        let had_sep = ts.eat_comma() || ts.eat_semicolon();
        let had_or = ts.eat_word("or");
        let had_and = !had_or && ts.eat_word("and");
        if (had_sep || had_or || had_and)
            && let Ok(next_kw) = try_parse(ts, parse_keyword_spec)
        {
            if had_or {
                is_or = true;
            }
            kws.push(next_kw);
            continue;
        }
        ts.restore(cp);
        break;
    }
    Ok((kws, is_or))
}
