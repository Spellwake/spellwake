//! Strictly-typed CR 702 keyword ability parser for `v2`.
//!
//! Handles evergreen, deciduous, and named MTG keyword abilities plus their typed parameters
//! (costs, numbers, filters, color masks, and `everything`).

use crate::ast::ColorMask;
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::parse_object_filter;
use crate::v2::ir::{KeywordKind, KeywordParam, KeywordSpec};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::vocab::lookup_color;

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
            // `Equip {2}` OR `Equip Soldier {1}` / `Equip legendary creature {2}`
            let _ = ts.eat_em_dash();
            if let Ok(costs) = try_parse(ts, parse_cost_list) {
                Some(KeywordParam::Cost(costs))
            } else if let Ok(flt) = try_parse(ts, parse_object_filter)
                && let Ok(costs) = try_parse(ts, parse_cost_list)
            {
                Some(KeywordParam::FilterAndCost(Box::new(flt), costs))
            } else {
                None
            }
        }
        KeywordKind::Suspend | KeywordKind::Awaken | KeywordKind::Impending => {
            // `Suspend 3 — {1}{R}` / `Awaken 4—{4}{W}{W}`
            if let Some(n) = ts.next_number(false).and_then(|v| u16::try_from(v).ok()) {
                let _ = ts.eat_em_dash();
                if let Ok(costs) = try_parse(ts, parse_cost_list) {
                    Some(KeywordParam::NumberAndCost(n, costs))
                } else {
                    Some(KeywordParam::Number(n))
                }
            } else {
                None
            }
        }
        KeywordKind::Reinforce => {
            if let Some(n) = ts.next_number(false).and_then(|v| u16::try_from(v).ok()) {
                let _ = ts.eat_em_dash();
                if let Ok(costs) = try_parse(ts, parse_cost_list) {
                    Some(KeywordParam::NumberAndCost(n, costs))
                } else {
                    Some(KeywordParam::Number(n))
                }
            } else {
                None
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
        | KeywordKind::CumulativeUpkeep
        | KeywordKind::Madness
        | KeywordKind::Echo
        | KeywordKind::Outlast
        | KeywordKind::Surge
        | KeywordKind::Emerge
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
        | KeywordKind::Freerunning => {
            let _ = ts.eat_em_dash();
            opt(ts, parse_cost_list).map(KeywordParam::Cost)
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
        | KeywordKind::Modular
        | KeywordKind::Dredge
        | KeywordKind::Bloodthirst
        | KeywordKind::Graft
        | KeywordKind::Ripple
        | KeywordKind::Frenzy
        | KeywordKind::Absorb
        | KeywordKind::Poisonous
        | KeywordKind::Hideaway
        | KeywordKind::Devour
        | KeywordKind::Tribute => ts
            .next_number(false)
            .and_then(|n| u16::try_from(n).ok())
            .map(KeywordParam::Number),
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
    let mut colors = ColorMask::EMPTY;
    let cp = ts.checkpoint();
    while let Some(w) = ts.peek_word() {
        if let Some(c) = lookup_color(&w.to_ascii_lowercase()) {
            ts.next_word();
            colors = colors.union(c);
            let _ = ts.eat_comma();
            if ts.eat_phrase(&["and", "from"]) || ts.eat_word("and") {
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
    opt(ts, parse_object_filter).map(|f| KeywordParam::Filter(Box::new(f)))
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
