//! Player-headed static ability parsers for `v2`.

use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, FilterStaticKind, ObjectFilter, PlayerRef,
    PlayerRestrictionKind, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef,
    SubjectStaticRule, VisibilityKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

use super::cond_ext::parse_static_condition;
use super::{and_cond, merge_cond, parse_static_core, wrap_compound};

fn exile_zone() -> crate::v2::ir::ZoneRef {
    crate::v2::ir::ZoneRef {
        zone: crate::v2::ir::ZoneKind::Exile,
        owner: crate::v2::ir::ZoneOwner::Default,
    }
}

fn zone_cast_perm(
    player: PlayerRef,
    filter: Option<ObjectFilter>,
    zone: Option<crate::v2::ir::ZoneRef>,
    mode: CastPermissionMode,
    condition: Option<ConditionExpr>,
) -> StaticAbility {
    StaticAbility::ZoneCastPermission {
        player,
        filter,
        subject: None,
        zone,
        mode,
        condition,
    }
}

fn play_from_zone(
    player: PlayerRef,
    filter: Option<ObjectFilter>,
    zone: crate::v2::ir::ZoneRef,
    top_only: bool,
    lands: bool,
    spells: bool,
    condition: Option<ConditionExpr>,
) -> StaticAbility {
    zone_cast_perm(
        player,
        filter,
        Some(zone),
        CastPermissionMode::PlayFromZone {
            top_only,
            lands,
            spells,
        },
        condition,
    )
}

fn eat_each_turn_suffix(ts: &mut TokenStream<'_>) {
    let _ = ts.eat_any_phrase(&[
        &["on", "each", "of", "your", "turns"],
        &["on", "each", "of", "their", "turns"],
        &["during", "each", "of", "your", "turns"],
        &["during", "each", "of", "their", "turns"],
        &["each", "turn"],
    ]);
}

fn parse_optional_by_alt_cost(
    ts: &mut TokenStream<'_>,
    mode: &mut CastPermissionMode,
    allow_using_ability: bool,
) {
    if allow_using_ability && ts.eat_any_phrase(&[&["using", "their"], &["using", "its"]]) {
        let _ = ts.next_word();
        let _ = ts.eat_any_word(&["abilities", "ability"]);
    }
    if ts.eat_word("by") {
        let costs = parse_cost_list(ts).unwrap_or_default();
        let _ = ts.eat_in_addition_to_other_costs();
        if !costs.is_empty() {
            *mode = CastPermissionMode::AlternativeCost(costs);
        }
    }
}

pub(super) fn parse_player_static_ability(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 0c. Player-headed static abilities (`You may look at the top card of ...`,
    //     `You may play lands and cast spells from ...`,
    //     `Each opponent can cast spells only any time they could cast a sorcery`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            {
                let alt_cp = ts.checkpoint();
                let _ = ts.eat_word("may");
                if let Ok(costs) = try_parse(ts, parse_cost_list)
                    && ts.eat_any_phrase(&[
                        &["rather", "than", "pay"],
                        &["rather", "than", "paying"],
                    ])
                    && (ts.eat_any_phrase(&[
                        &["its", "mana", "cost"],
                        &["that", "spell's", "mana", "cost"],
                        &["this", "spell's", "mana", "cost"],
                    ]) || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
                {
                    return Ok(StaticAbility::AlternativeCost(costs));
                }
                ts.restore(alt_cp);
            }
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "reveal"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["from", "your", "opening", "hand"])
            {
                let _ = ts.eat_period();
                if ts.eat_phrase(&["if", "you", "do"]) && ts.eat_comma() {
                    let rem = &ts.tokens[ts.pos..];
                    let mut sub = ts.sub_stream(rem, crate::v2::ir::ParseStage::Effect);
                    if let Ok(effs) = crate::v2::clause::parse_effect_sentence(&mut sub) {
                        let _ = sub.eat_period();
                        if sub.is_eof() {
                            ts.commit_substream_eof(ts.tokens.len(), &sub);
                            return Ok(StaticAbility::RevealOpeningHand { effect: effs });
                        }
                    }
                }
            }
            if player == PlayerRef::You
                && ts.eat_phrase(&["don't", "lose", "the", "game", "for", "having"])
                && ts.next_number(false) == Some(0)
                && ts.eat_phrase(&["or", "less", "life"])
            {
                return Ok(StaticAbility::GlobalFlag(
                    StaticFlagKind::DontLoseForZeroLife,
                ));
            }
            if player == PlayerRef::You && ts.eat_phrase(&["can't", "lose", "the", "game"]) {
                let first = StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantLoseGame,
                    condition: condition.clone(),
                };
                if ts.eat_phrase(&["and", "your", "opponents", "can't", "win", "the", "game"]) {
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::PlayerRestriction {
                            player: PlayerRef::EachOpponent,
                            restriction: PlayerRestrictionKind::CantWinGame,
                            condition,
                        },
                    ]));
                }
                return Ok(first);
            }
            if player == PlayerRef::You && ts.eat_phrase(&["have", "no", "maximum", "hand", "size"])
            {
                let first = StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::NoMaxHandSize,
                    condition,
                };
                if ts.eat_phrase(&["and", "don't", "lose", "the", "game", "for", "having"])
                    && ts.next_number(false) == Some(0)
                    && ts.eat_phrase(&["or", "less", "life"])
                {
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::GlobalFlag(StaticFlagKind::DontLoseForZeroLife),
                    ]));
                }
                return Ok(first);
            }
            // `You may <cost> rather than pay this spell's/~'s mana cost` OR `... rather than pay the mana cost for <flt>`
            {
                let alt_cp = ts.checkpoint();
                if player == PlayerRef::You
                    && ts.eat_word("may")
                    && let Ok(costs) = try_parse(ts, parse_cost_list)
                    && ts.eat_phrase(&["rather", "than", "pay"])
                {
                    if ts.eat_phrase(&["this", "spell's", "mana", "cost"])
                        || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"]))
                    {
                        return Ok(StaticAbility::AlternativeCost(costs));
                    }
                    if ts.eat_phrase(&["the", "mana", "cost", "for"])
                        && let Ok(filter) = try_parse(ts, parse_object_filter)
                    {
                        return Ok(zone_cast_perm(
                            PlayerRef::You,
                            Some(filter),
                            None,
                            CastPermissionMode::AlternativeCost(costs),
                            condition,
                        ));
                    }
                }
                ts.restore(alt_cp);
            }
            {
                let asgn_cp = ts.checkpoint();
                if ts.eat_phrase(&["may", "have"])
                    && let Ok(subj) = try_parse(ts, parse_subject_ref)
                    && ts.eat_any_phrase(&[
                        &[
                            "assign", "its", "combat", "damage", "as", "though", "it", "weren't",
                            "blocked",
                        ],
                        &[
                            "assign", "his", "combat", "damage", "as", "though", "he", "weren't",
                            "blocked",
                        ],
                        &[
                            "assign", "her", "combat", "damage", "as", "though", "she", "weren't",
                            "blocked",
                        ],
                    ])
                {
                    return Ok(StaticAbility::SubjectRule {
                        subject: subj,
                        rule: SubjectStaticRule::AssignDamageAsUnblocked,
                        condition,
                    });
                }
                ts.restore(asgn_cp);
            }
            let mcp = ts.checkpoint();
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "exert"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["as", "it", "attacks"])
            {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::MayExertOnAttack));
            }
            ts.restore(mcp);
            if player == PlayerRef::You
                && ts.eat_phrase(&["may", "choose", "not", "to", "untap"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["during", "your", "untap", "step"])
            {
                return Ok(StaticAbility::SubjectRule {
                    subject: SubjectRef::SelfObject,
                    rule: SubjectStaticRule::MayNotUntap,
                    condition,
                });
            }
            ts.restore(mcp);
            if ts.eat_phrase(&["may", "look", "at"])
                && {
                    let _ = ts.eat_word("the");
                    let _ = ts.eat_word("face-down");
                    ts.eat_phrase(&["cards", "exiled", "with"])
                }
                && ts.eat_tilde()
            {
                return Ok(StaticAbility::VisibilityRule {
                    player,
                    kind: VisibilityKind::LookAtExiledWithSelf,
                });
            }
            ts.restore(mcp);
            // `Each player may look at cards they exiled with ~[, and ...]`
            if ts.eat_phrase(&["may", "look", "at", "cards", "they", "exiled", "with"])
                && ts.eat_tilde()
            {
                let first = StaticAbility::VisibilityRule {
                    player,
                    kind: VisibilityKind::LookAtExiledWithSelf,
                };
                let ccp = ts.checkpoint();
                if ts.eat_comma()
                    && ts.eat_word("and")
                    && let Ok(second) = try_parse(ts, |t| parse_static_core(t, condition.clone()))
                {
                    return Ok(StaticAbility::Compound(vec![first, second]));
                }
                ts.restore(ccp);
                return Ok(first);
            }
            ts.restore(mcp);
            if ts.eat_any_phrase(&[
                &["play", "with", "their", "hands", "revealed"],
                &["plays", "with", "their", "hand", "revealed"],
                &["play", "with", "your", "hand", "revealed"],
            ]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
                    condition,
                });
            }
            if ts.eat_any_phrase(&[
                &[
                    "play",
                    "with",
                    "the",
                    "top",
                    "card",
                    "of",
                    "their",
                    "libraries",
                    "revealed",
                ],
                &[
                    "plays", "with", "the", "top", "card", "of", "their", "library", "revealed",
                ],
                &[
                    "play", "with", "the", "top", "card", "of", "your", "library", "revealed",
                ],
            ]) {
                return Ok(StaticAbility::VisibilityRule {
                    player,
                    kind: VisibilityKind::PlayWithTopRevealed,
                });
            }
            {
                let mcp = ts.checkpoint();
                if ts.eat_phrase(&["can't", "cast", "more", "than", "one", "spell"]) {
                    let _ = ts.eat_phrase(&["this", "way"]);
                    if ts.eat_phrase(&["each", "turn"]) {
                        return Ok(StaticAbility::PlayerRestriction {
                            player,
                            restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                            condition,
                        });
                    }
                }
                ts.restore(mcp);
            }
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["can't", "cast"])
                    && let Ok(flt) = try_parse(ts, parse_object_filter)
                {
                    return Ok(StaticAbility::RuleRestriction {
                        subject: Some(SubjectRef::Group(Box::new(flt))),
                        rule: RuleRestrictionKind::CantBeCast,
                        condition,
                    });
                }
                ts.restore(ccp);
            }
            if ts.eat_phrase(&["may", "look", "at", "the", "top", "card", "of"]) {
                let library = parse_zone_ref(ts)?;
                let _ = ts.eat_phrase(&["any", "time"]);
                let first = StaticAbility::VisibilityRule {
                    player,
                    kind: VisibilityKind::LookAtTop(library),
                };
                let ccp = ts.checkpoint();
                if ts.eat_comma() && ts.eat_word("and") {
                    if let Ok(second) = try_parse(ts, |t| parse_static_core(t, condition.clone())) {
                        return Ok(StaticAbility::Compound(vec![first, second]));
                    }
                    ts.restore(ccp);
                }
                return Ok(first);
            }
            if ts.eat_phrase(&["may", "play", "an", "additional", "land"]) {
                eat_each_turn_suffix(ts);
                return Ok(StaticAbility::AdditionalLandPlayStatic { player, extra: 1 });
            }
            {
                let alcp = ts.checkpoint();
                if ts.eat_phrase(&["may", "play"])
                    && let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
                    && ts.eat_phrase(&["additional", "lands"])
                {
                    eat_each_turn_suffix(ts);
                    return Ok(StaticAbility::AdditionalLandPlayStatic { player, extra: n });
                }
                ts.restore(alcp);
            }
            {
                let plself = ts.checkpoint();
                if ts.eat_phrase(&["may", "play"]) && ts.eat_tilde() && ts.eat_word("from") {
                    let top_only = ts.eat_phrase(&["the", "top", "of"]);
                    if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                        return Ok(play_from_zone(
                            player, None, zone, top_only, true, true, condition,
                        ));
                    }
                }
                ts.restore(plself);
            }
            {
                let plflt = ts.checkpoint();
                if ts.eat_phrase(&["may", "play", "the", "top", "card", "of"])
                    && let Ok(zone) = try_parse(ts, parse_zone_ref)
                {
                    return Ok(play_from_zone(
                        player, None, zone, true, true, true, condition,
                    ));
                }
                ts.restore(plflt);
                if ts.eat_phrase(&["may", "play"])
                    && let Ok(mut flt) = try_parse(ts, parse_object_filter)
                {
                    if ts.eat_word("from") {
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            return Ok(play_from_zone(
                                player,
                                Some(flt),
                                zone,
                                top_only,
                                true,
                                false,
                                condition,
                            ));
                        }
                    } else if let Some(zone) = flt.zones.pop() {
                        return Ok(play_from_zone(
                            player,
                            Some(flt),
                            zone,
                            false,
                            true,
                            false,
                            condition,
                        ));
                    }
                }
                ts.restore(plflt);
            }
            {
                let actcp = ts.checkpoint();
                if player == PlayerRef::You
                    && condition.is_none()
                    && ts.eat_phrase(&["may", "activate"])
                {
                    if ts.eat_phrase(&["loyalty", "abilities", "of"]) && ts.eat_tilde() {
                        let _ = ts.eat_phrase(&["on", "any", "player's", "turn"]);
                        if ts.eat_phrase(&["any", "time", "you", "could", "cast", "an", "instant"])
                        {
                            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::LoyaltyAsInstant));
                        }
                    }
                    ts.restore(actcp);
                    let _ = ts.eat_phrase(&["may", "activate"]);
                    if ts.eat_tilde()
                        && ts.eat_phrase(&["'s", "loyalty", "abilities", "any", "time"])
                        && let Ok(PlayerRef::You) = try_parse(ts, parse_player_ref)
                        && ts.eat_phrase(&["could", "cast", "an", "instant"])
                    {
                        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::LoyaltyAsInstant));
                    }
                }
                ts.restore(actcp);
                if ts.eat_phrase(&["may", "pay"])
                    && let Some(mc) = ts.eat_mana_cost()
                    && ts.eat_phrase(&["to", "cast"])
                    && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
                {
                    return Ok(StaticAbility::AlternativeCost(vec![
                        crate::v2::ir::Cost::Mana(mc),
                    ]));
                }
                ts.restore(actcp);
                if condition.is_none()
                    && ts.eat_phrase(&["may", "activate", "abilities", "of"])
                    && let Ok(mut filter) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["as", "though", "those", "creatures", "had", "haste"])
                {
                    let player_ok = match player {
                        PlayerRef::EachPlayer => true,
                        PlayerRef::You
                            if filter.controller.is_none()
                                || filter.controller
                                    == Some(crate::ast::ControllerRelation::You) =>
                        {
                            filter.controller = Some(crate::ast::ControllerRelation::You);
                            true
                        }
                        _ => false,
                    };
                    if player_ok {
                        return Ok(StaticAbility::FilterRule {
                            filter,
                            kind: FilterStaticKind::ActivateAbilitiesAsHaste,
                        });
                    }
                }
                ts.restore(actcp);
            }
            {
                let spcp = ts.checkpoint();
                let spend_matched = (ts.eat_phrase(&["may", "spend", "mana", "as", "though"])
                    && ts.eat_any_phrase(&[
                        &["it", "were", "mana", "of", "any", "color"],
                        &["it", "were", "mana", "of", "any", "type"],
                    ]))
                    || ts.eat_any_phrase(&[
                        &["can", "spend", "mana", "of", "any", "type"],
                        &["can", "spend", "mana", "of", "any", "color"],
                    ]);
                if condition.is_none()
                    && spend_matched
                    && ts.eat_phrase(&["to", "cast"])
                    && let Ok(mut filter) = try_parse(ts, parse_object_filter)
                {
                    let player_ok = match player {
                        PlayerRef::EachPlayer => true,
                        PlayerRef::You
                            if filter.controller.is_none()
                                || filter.controller
                                    == Some(crate::ast::ControllerRelation::You) =>
                        {
                            filter.controller = Some(crate::ast::ControllerRelation::You);
                            true
                        }
                        _ => false,
                    };
                    if player_ok {
                        return Ok(StaticAbility::FilterRule {
                            filter,
                            kind: FilterStaticKind::SpendManaAsAnyColor,
                        });
                    }
                }
                ts.restore(spcp);
            }
            {
                let pland_cp = ts.checkpoint();
                if ts.eat_any_phrase(&[
                    &["may", "play", "lands", "and", "cast"],
                    &["may", "play", "a", "land", "and", "cast"],
                    &["may", "play", "a", "land", "or", "cast"],
                ]) {
                    let spell_flt =
                        if ts.eat_any_phrase(&[&["spells", "from"], &["a", "spell", "from"]]) {
                            Some(None)
                        } else {
                            let s_cp = ts.checkpoint();
                            if let Ok(sf) = try_parse(ts, parse_object_filter)
                                && ts.eat_word("from")
                            {
                                Some(Some(sf))
                            } else {
                                ts.restore(s_cp);
                                None
                            }
                        };
                    if let Some(spell_flt) = spell_flt {
                        if ts.eat_any_phrase(&[
                            &["among", "cards", "exiled", "with"],
                            &["among", "cards", "you", "own", "exiled", "with"],
                            &["among", "cards", "they", "exiled", "with"],
                        ]) && ts.eat_tilde()
                        {
                            let _ = ts.eat_phrase(&["this", "turn"]);
                            return Ok(play_from_zone(
                                player,
                                spell_flt,
                                exile_zone(),
                                false,
                                true,
                                true,
                                condition,
                            ));
                        }
                        if ts.eat_phrase(&["among", "those", "cards"]) {
                            return Ok(play_from_zone(
                                player,
                                spell_flt,
                                exile_zone(),
                                false,
                                true,
                                true,
                                condition,
                            ));
                        }
                        if ts.eat_phrase(&["among", "cards", "you", "exiled"]) {
                            return Ok(play_from_zone(
                                player,
                                spell_flt,
                                exile_zone(),
                                false,
                                true,
                                true,
                                condition,
                            ));
                        }
                        if ts.eat_word("among")
                            && let Ok(mut among_flt) = try_parse(ts, parse_object_filter)
                        {
                            let zone = among_flt.zones.pop().unwrap_or_else(exile_zone);
                            return Ok(play_from_zone(
                                player,
                                spell_flt.or(Some(among_flt)),
                                zone,
                                false,
                                true,
                                true,
                                condition,
                            ));
                        }
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            return Ok(play_from_zone(
                                player, spell_flt, zone, top_only, true, true, condition,
                            ));
                        }
                    }
                }
                ts.restore(pland_cp);
            }
            {
                let plcp = ts.checkpoint();
                if ts.eat_phrase(&["may", "play"])
                    && let Ok(mut play_flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["and", "cast"])
                    && let Ok(mut cast_flt) = try_parse(ts, parse_object_filter)
                {
                    let top_only = ts.eat_phrase(&["from", "the", "top", "of"]);
                    let zone = if top_only {
                        opt(ts, parse_zone_ref)
                    } else if ts.eat_word("from") {
                        let _ = ts.eat_phrase(&["the", "top", "of"]);
                        opt(ts, parse_zone_ref)
                    } else {
                        cast_flt.zones.pop().or_else(|| play_flt.zones.pop())
                    };
                    let is_plain_land = play_flt
                        == ObjectFilter {
                            any_types: crate::ast::CardTypeMask::LAND,
                            ..Default::default()
                        }
                        || play_flt
                            == ObjectFilter {
                                all_types: crate::ast::CardTypeMask::LAND,
                                ..Default::default()
                            };
                    if is_plain_land && let Some(zone) = zone {
                        return Ok(play_from_zone(
                            player,
                            Some(cast_flt),
                            zone,
                            top_only,
                            true,
                            true,
                            condition,
                        ));
                    }
                }
                ts.restore(plcp);
            }
            if ts.eat_phrase(&["may", "play", "lands", "from"]) {
                if ts.eat_phrase(&["among", "cards", "exiled", "with"]) && ts.eat_tilde() {
                    return Ok(play_from_zone(
                        player,
                        None,
                        exile_zone(),
                        false,
                        true,
                        false,
                        condition,
                    ));
                }
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(play_from_zone(
                    player, None, zone, top_only, true, false, condition,
                ));
            }
            if ts.eat_phrase(&["may", "play", "cards", "from"]) {
                if ts.eat_any_phrase(&[
                    &["among", "cards", "exiled", "with"],
                    &["among", "cards", "you", "own", "exiled", "with"],
                ]) && ts.eat_tilde()
                {
                    return Ok(play_from_zone(
                        player,
                        None,
                        exile_zone(),
                        false,
                        true,
                        true,
                        condition,
                    ));
                }
                if ts.eat_word("among")
                    && let Ok(mut among_flt) = try_parse(ts, parse_object_filter)
                {
                    let zone = among_flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Graveyard,
                        owner: crate::v2::ir::ZoneOwner::You,
                    });
                    return Ok(play_from_zone(
                        player,
                        Some(among_flt),
                        zone,
                        false,
                        true,
                        true,
                        condition,
                    ));
                }
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(play_from_zone(
                    player, None, zone, top_only, true, true, condition,
                ));
            }
            if ts.eat_phrase(&["may", "play", "cards", "exiled", "with"]) && ts.eat_tilde() {
                return Ok(play_from_zone(
                    player,
                    None,
                    exile_zone(),
                    false,
                    true,
                    true,
                    condition,
                ));
            }
            {
                let sorc_cp = ts.checkpoint();
                if ts.eat_phrase(&["can", "cast", "spells", "only", "any", "time"])
                    && try_parse(ts, parse_player_ref).is_ok()
                    && ts.eat_phrase(&["could", "cast", "a", "sorcery"])
                {
                    return Ok(StaticAbility::PlayerRestriction {
                        player,
                        restriction: PlayerRestrictionKind::CastOnlyAsSorcery,
                        condition,
                    });
                }
                ts.restore(sorc_cp);
            }
            // `<player> can cast spells [and activate abilities] only during their own turns / only during your turn`
            {
                let csc_cp = ts.checkpoint();
                if ts.eat_phrase(&["can", "cast", "spells"]) {
                    let also_act = ts.eat_phrase(&["and", "activate", "abilities"]);
                    if ts.eat_any_phrase(&[
                        &["only", "during", "their", "own", "turns"],
                        &["only", "during", "your", "turn"],
                    ]) {
                        let not_own = ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::IsYourTurn,
                        )));
                        let merged = Some(and_cond(condition.clone(), not_own));
                        let mut parts = vec![StaticAbility::PlayerRestriction {
                            player,
                            restriction: PlayerRestrictionKind::CantCastSpells,
                            condition: merged.clone(),
                        }];
                        if also_act {
                            parts.push(StaticAbility::PlayerRestriction {
                                player,
                                restriction: PlayerRestrictionKind::CantActivateAbilities,
                                condition: merged,
                            });
                        }
                        return Ok(wrap_compound(parts));
                    }
                }
                ts.restore(csc_cp);
            }
            if ts.eat_phrase(&["may", "cast"]) {
                // `You may cast the exiled card`
                if ts.eat_phrase(&["the", "exiled", "card"]) {
                    return Ok(StaticAbility::ZoneCastPermission {
                        player,
                        filter: None,
                        subject: Some(SubjectRef::Anaphora(
                            crate::v2::ir::AnaphoraRef::TheExiledCard,
                        )),
                        zone: Some(exile_zone()),
                        mode: CastPermissionMode::CastFromZone,
                        condition,
                    });
                }
                // `You may cast ~ from your graveyard [or [from] exile]` / `You may cast ~|it as though it had flash [if ...]`
                let c_cp = ts.checkpoint();
                if ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) || ts.eat_word("it") {
                    if ts.eat_phrase(&["without", "paying", "its", "mana", "cost"]) {
                        let base = StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::SelfObject),
                            zone: None,
                            mode: CastPermissionMode::WithoutPaying,
                            condition: condition.clone(),
                        };
                        if ts.eat_phrase(&["and", "as", "though", "it", "had", "flash"]) {
                            return Ok(StaticAbility::Compound(vec![
                                base,
                                StaticAbility::ZoneCastPermission {
                                    player,
                                    filter: None,
                                    subject: Some(SubjectRef::SelfObject),
                                    zone: None,
                                    mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                                    condition,
                                },
                            ]));
                        }
                        return Ok(base);
                    }
                    if ts.eat_word("from") {
                        let zone = parse_zone_ref(ts)?;
                        let mut mode = CastPermissionMode::CastFromZone;
                        if ts.eat_word("by") {
                            let costs = parse_cost_list(ts).unwrap_or_default();
                            let _ = ts
                                .eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"])
                                || ts.eat_in_addition_to_other_costs();
                            if !costs.is_empty() {
                                mode = CastPermissionMode::AlternativeCost(costs);
                            }
                        }
                        let extra_cond = if ts.eat_word("if") {
                            opt(ts, parse_static_condition)
                        } else {
                            None
                        };
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::SelfObject),
                            zone: Some(zone),
                            mode,
                            condition: merge_cond(condition, extra_cond),
                        });
                    }
                    if ts.eat_phrase(&["as", "though", "it", "had", "flash"]) {
                        let any_spell_flt = Some(ObjectFilter {
                            any_spell: true,
                            ..Default::default()
                        });
                        if ts.eat_phrase(&["if", "you", "pay"])
                            && let Some(mc) = ts.eat_mana_cost()
                            && ts.eat_phrase(&["more", "to", "cast", "it"])
                        {
                            return Ok(zone_cast_perm(
                                player,
                                any_spell_flt,
                                None,
                                CastPermissionMode::AsThoughFlash {
                                    extra_cost: Some(Box::new(crate::v2::ir::Cost::Mana(mc))),
                                },
                                condition,
                            ));
                        }
                        let cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
                            Some(parse_static_condition(ts)?)
                        } else {
                            None
                        };
                        return Ok(zone_cast_perm(
                            player,
                            any_spell_flt,
                            None,
                            CastPermissionMode::AsThoughFlash { extra_cost: None },
                            merge_cond(condition, cond),
                        ));
                    }
                }
                ts.restore(c_cp);
                // `You may cast the first <flt> you cast each turn as though it had flash`
                {
                    let f_cp = ts.checkpoint();
                    if ts.eat_phrase(&["the", "first"])
                        && let Ok(flt) = try_parse(ts, parse_object_filter)
                        && ts.eat_any_phrase(&[
                            &[
                                "you", "cast", "each", "turn", "as", "though", "it", "had", "flash",
                            ],
                            &["each", "turn", "as", "though", "it", "had", "flash"],
                        ])
                    {
                        return Ok(StaticAbility::NextSpellModifier {
                            filter: flt.clone(),
                            effect: Box::new(zone_cast_perm(
                                player,
                                Some(flt),
                                None,
                                CastPermissionMode::AsThoughFlash { extra_cost: None },
                                condition,
                            )),
                        });
                    }
                    ts.restore(f_cp);
                }
                if let Ok(mut filter) = try_parse(ts, parse_object_filter) {
                    // Support `<flt1> and <flt2>`
                    {
                        let and_cp = ts.checkpoint();
                        if ts.eat_word("and")
                            && let Ok(mut other) = try_parse(ts, parse_object_filter)
                        {
                            let shared_zones = if !other.zones.is_empty() {
                                std::mem::take(&mut other.zones)
                            } else {
                                std::mem::take(&mut filter.zones)
                            };
                            filter = ObjectFilter {
                                zones: shared_zones,
                                predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![
                                    filter, other,
                                ])],
                                ..Default::default()
                            };
                        } else {
                            ts.restore(and_cp);
                        }
                    }
                    if ts.eat_any_phrase(&[
                        &["without", "paying", "their", "mana", "costs"],
                        &["without", "paying", "its", "mana", "cost"],
                    ]) {
                        let base = StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::Group(Box::new(filter.clone()))),
                            zone: None,
                            mode: CastPermissionMode::WithoutPaying,
                            condition: condition.clone(),
                        };
                        if ts.eat_any_phrase(&[
                            &["and", "as", "though", "they", "had", "flash"],
                            &["and", "as", "though", "it", "had", "flash"],
                        ]) {
                            return Ok(StaticAbility::Compound(vec![
                                base,
                                zone_cast_perm(
                                    player,
                                    Some(filter),
                                    None,
                                    CastPermissionMode::AsThoughFlash { extra_cost: None },
                                    condition,
                                ),
                            ]));
                        }
                        return Ok(base);
                    }
                    {
                        let by_cp = ts.checkpoint();
                        if ts.eat_word("by") {
                            let costs = parse_cost_list(ts).unwrap_or_default();
                            if !costs.is_empty()
                                && ts.eat_any_phrase(&[
                                    &["rather", "than", "paying", "their", "mana", "costs"],
                                    &["rather", "than", "paying", "its", "mana", "cost"],
                                ])
                            {
                                return Ok(zone_cast_perm(
                                    player,
                                    Some(filter),
                                    None,
                                    CastPermissionMode::AlternativeCost(costs),
                                    condition,
                                ));
                            }
                        }
                        ts.restore(by_cp);
                    }
                    if filter
                        .flags
                        .contains(crate::v2::ir::FilterFlags::EXILED_WITH_SELF)
                        || (ts.eat_any_phrase(&[
                            &["from", "among", "cards", "you", "own", "exiled", "with"],
                            &[
                                "from", "among", "the", "cards", "they", "don't", "own", "exiled",
                                "with",
                            ],
                            &["from", "among", "cards", "exiled", "with"],
                        ]) && ts.eat_tilde())
                    {
                        let mut mode = CastPermissionMode::PlayFromZone {
                            top_only: false,
                            lands: false,
                            spells: true,
                        };
                        parse_optional_by_alt_cost(ts, &mut mode, false);
                        let base = zone_cast_perm(
                            player,
                            Some(filter.clone()),
                            Some(exile_zone()),
                            mode,
                            condition.clone(),
                        );
                        let flash_cp = ts.checkpoint();
                        if ts.eat_word("and")
                            && let Ok(PlayerRef::You) = try_parse(ts, parse_player_ref)
                            && ts.eat_phrase(&[
                                "may", "cast", "them", "as", "though", "they", "had", "flash",
                            ])
                        {
                            return Ok(StaticAbility::Compound(vec![
                                base,
                                zone_cast_perm(
                                    player,
                                    Some(filter),
                                    None,
                                    CastPermissionMode::AsThoughFlash { extra_cost: None },
                                    condition,
                                ),
                            ]));
                        }
                        ts.restore(flash_cp);
                        return Ok(base);
                    }
                    if ts.eat_phrase(&["as", "though", "they", "had", "flash"]) {
                        return Ok(zone_cast_perm(
                            player,
                            Some(filter),
                            None,
                            CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition,
                        ));
                    }
                    if ts.eat_word("from") {
                        if ts.eat_word("among")
                            && let Ok(mut among_flt) = try_parse(ts, parse_object_filter)
                        {
                            let zone = among_flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Graveyard,
                                owner: crate::v2::ir::ZoneOwner::You,
                            });
                            let mut mode = CastPermissionMode::PlayFromZone {
                                top_only: false,
                                lands: false,
                                spells: true,
                            };
                            parse_optional_by_alt_cost(ts, &mut mode, false);
                            return Ok(zone_cast_perm(
                                player,
                                Some(filter),
                                Some(zone),
                                mode,
                                condition,
                            ));
                        }
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            let mut mode = CastPermissionMode::PlayFromZone {
                                top_only,
                                lands: false,
                                spells: true,
                            };
                            parse_optional_by_alt_cost(ts, &mut mode, true);
                            return Ok(zone_cast_perm(
                                player,
                                Some(filter),
                                Some(zone),
                                mode,
                                condition,
                            ));
                        }
                    } else if let Some(zone) = filter.zones.pop() {
                        let mut mode = CastPermissionMode::PlayFromZone {
                            top_only: false,
                            lands: false,
                            spells: true,
                        };
                        parse_optional_by_alt_cost(ts, &mut mode, true);
                        return Ok(zone_cast_perm(
                            player,
                            Some(filter),
                            Some(zone),
                            mode,
                            condition,
                        ));
                    }
                }
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("not a player-headed static ability"))
}
