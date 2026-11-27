//! Player-headed static ability parsers for `v2`.

use crate::v2::condition::parse_condition;
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, FilterStaticKind, ObjectFilter, PlayerRef,
    PlayerRestrictionKind, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef,
    SubjectStaticRule, VisibilityKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

use super::parse_static_core;

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
                    && (ts.eat_phrase(&["rather", "than", "pay"])
                        || ts.eat_phrase(&["rather", "than", "paying"]))
                    && (ts.eat_phrase(&["its", "mana", "cost"])
                        || ts.eat_phrase(&["that", "spell's", "mana", "cost"])
                        || ts.eat_phrase(&["this", "spell's", "mana", "cost"])
                        || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
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
                        ts.pos = ts.tokens.len();
                        return Ok(StaticAbility::RevealOpeningHand { effect: effs });
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
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::NoMaxHandSize,
                    condition,
                });
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
                        return Ok(StaticAbility::ZoneCastPermission {
                            player: PlayerRef::You,
                            filter: Some(filter),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AlternativeCost(costs),
                            condition: None,
                        });
                    }
                }
                ts.restore(alt_cp);
            }
            {
                let asgn_cp = ts.checkpoint();
                if ts.eat_phrase(&["may", "have"])
                    && let Ok(subj) = try_parse(ts, parse_subject_ref)
                    && (ts.eat_phrase(&[
                        "assign", "its", "combat", "damage", "as", "though", "it", "weren't",
                        "blocked",
                    ]) || ts.eat_phrase(&[
                        "assign", "his", "combat", "damage", "as", "though", "he", "weren't",
                        "blocked",
                    ]) || ts.eat_phrase(&[
                        "assign", "her", "combat", "damage", "as", "though", "she", "weren't",
                        "blocked",
                    ]))
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
                    condition: None,
                });
            }
            ts.restore(mcp);
            if ts.eat_phrase(&["may", "look", "at"])
                && {
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
            if ts.eat_phrase(&["play", "with", "their", "hands", "revealed"])
                || ts.eat_phrase(&["plays", "with", "their", "hand", "revealed"])
                || ts.eat_phrase(&["play", "with", "your", "hand", "revealed"])
            {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
                    condition,
                });
            }
            if ts.eat_phrase(&[
                "play",
                "with",
                "the",
                "top",
                "card",
                "of",
                "their",
                "libraries",
                "revealed",
            ]) || ts.eat_phrase(&[
                "plays", "with", "the", "top", "card", "of", "their", "library", "revealed",
            ]) || ts.eat_phrase(&[
                "play", "with", "the", "top", "card", "of", "your", "library", "revealed",
            ]) {
                return Ok(StaticAbility::VisibilityRule {
                    player,
                    kind: VisibilityKind::PlayWithTopRevealed,
                });
            }
            if ts.eat_phrase(&[
                "can't", "cast", "more", "than", "one", "spell", "each", "turn",
            ]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                    condition,
                });
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
                let _ = ts.eat_phrase(&["on", "each", "of", "your", "turns"])
                    || ts.eat_phrase(&["each", "turn"]);
                return Ok(StaticAbility::AdditionalLandPlayStatic { player, extra: 1 });
            }
            {
                let alcp = ts.checkpoint();
                if ts.eat_phrase(&["may", "play"])
                    && let Some(n) = ts.next_number(false).and_then(|v| u8::try_from(v).ok())
                    && ts.eat_phrase(&["additional", "lands"])
                {
                    let _ = ts.eat_phrase(&["on", "each", "of", "your", "turns"])
                        || ts.eat_phrase(&["each", "turn"]);
                    return Ok(StaticAbility::AdditionalLandPlayStatic { player, extra: n });
                }
                ts.restore(alcp);
            }
            {
                let plself = ts.checkpoint();
                if ts.eat_phrase(&["may", "play"]) && ts.eat_tilde() && ts.eat_word("from") {
                    let top_only = ts.eat_phrase(&["the", "top", "of"]);
                    if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: None,
                            zone: Some(zone),
                            mode: CastPermissionMode::PlayFromZone {
                                top_only,
                                lands: true,
                                spells: true,
                            },
                            condition: None,
                        });
                    }
                }
                ts.restore(plself);
            }
            {
                let plflt = ts.checkpoint();
                if ts.eat_phrase(&["may", "play", "the", "top", "card", "of"])
                    && let Ok(zone) = try_parse(ts, parse_zone_ref)
                {
                    return Ok(StaticAbility::ZoneCastPermission {
                        player,
                        filter: None,
                        subject: None,
                        zone: Some(zone),
                        mode: CastPermissionMode::PlayFromZone {
                            top_only: true,
                            lands: true,
                            spells: true,
                        },
                        condition,
                    });
                }
                ts.restore(plflt);
                if ts.eat_phrase(&["may", "play"])
                    && let Ok(mut flt) = try_parse(ts, parse_object_filter)
                {
                    if ts.eat_word("from") {
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            return Ok(StaticAbility::ZoneCastPermission {
                                player,
                                filter: Some(flt),
                                subject: None,
                                zone: Some(zone),
                                mode: CastPermissionMode::PlayFromZone {
                                    top_only,
                                    lands: true,
                                    spells: false,
                                },
                                condition,
                            });
                        }
                    } else if let Some(zone) = flt.zones.pop() {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: Some(flt),
                            subject: None,
                            zone: Some(zone),
                            mode: CastPermissionMode::PlayFromZone {
                                top_only: false,
                                lands: true,
                                spells: false,
                            },
                            condition,
                        });
                    }
                }
                ts.restore(plflt);
            }
            {
                let actcp = ts.checkpoint();
                if ts.eat_phrase(&["may", "activate"])
                    && ts.eat_tilde()
                    && ts.eat_phrase(&[
                        "'s",
                        "loyalty",
                        "abilities",
                        "any",
                        "time",
                        "you",
                        "could",
                        "cast",
                        "an",
                        "instant",
                    ])
                {
                    return Ok(StaticAbility::GlobalFlag(StaticFlagKind::LoyaltyAsInstant));
                }
                ts.restore(actcp);
                if ts.eat_phrase(&["may", "pay"])
                    && let Some(mc) = ts.eat_mana_cost()
                    && ts.eat_phrase(&["rather", "than", "pay", "the"])
                    && (ts.eat_phrase(&[
                        "cycling", "cost", "of", "the", "first", "card", "you", "cycle", "each",
                        "turn",
                    ]) || ts.eat_phrase(&[
                        "equip", "cost", "of", "the", "first", "equip", "ability", "you",
                        "activate", "each", "turn",
                    ]))
                {
                    return Ok(StaticAbility::ActivationCostModifier {
                        delta: mc,
                        less: true,
                        scale: None,
                        condition,
                    });
                }
                ts.restore(actcp);
                if ts.eat_phrase(&["may", "activate", "loyalty", "abilities", "of"])
                    && try_parse(ts, parse_object_filter).is_ok()
                    && {
                        let _ = ts.eat_phrase(&["on", "any", "player's", "turn"]);
                        ts.eat_phrase(&["any", "time", "you", "could", "cast", "an", "instant"])
                    }
                {
                    return Ok(StaticAbility::GlobalFlag(StaticFlagKind::LoyaltyAsInstant));
                }
                ts.restore(actcp);
                if ts.eat_phrase(&["may", "activate", "abilities", "of"])
                    && let Ok(filter) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["as", "though", "those", "creatures", "had", "haste"])
                {
                    return Ok(StaticAbility::FilterRule {
                        filter,
                        kind: FilterStaticKind::ActivateAbilitiesAsHaste,
                    });
                }
                ts.restore(actcp);
            }
            {
                let spcp = ts.checkpoint();
                if ts.eat_phrase(&[
                    "may", "spend", "mana", "as", "though", "it", "were", "mana", "of", "any",
                    "color",
                ]) || ts.eat_phrase(&[
                    "may", "spend", "mana", "as", "though", "it", "were", "mana", "of", "any",
                    "type",
                ]) {
                    let filter = if ts.eat_phrase(&["to", "cast"]) {
                        try_parse(ts, parse_object_filter).ok()
                    } else {
                        Some(ObjectFilter::default())
                    };
                    if let Some(filter) = filter {
                        return Ok(StaticAbility::FilterRule {
                            filter,
                            kind: FilterStaticKind::SpendManaAsAnyColor,
                        });
                    }
                }
                ts.restore(spcp);
            }
            if ts.eat_phrase(&["may", "play", "lands", "and", "cast", "spells", "from"]) {
                if (ts.eat_phrase(&["among", "cards", "exiled", "with"])
                    || ts.eat_phrase(&["among", "cards", "you", "own", "exiled", "with"]))
                    && ts.eat_tilde()
                {
                    return Ok(StaticAbility::ZoneCastPermission {
                        player,
                        filter: None,
                        subject: None,
                        zone: Some(crate::v2::ir::ZoneRef {
                            zone: crate::v2::ir::ZoneKind::Exile,
                            owner: crate::v2::ir::ZoneOwner::Default,
                        }),
                        mode: CastPermissionMode::PlayFromZone {
                            top_only: false,
                            lands: true,
                            spells: true,
                        },
                        condition,
                    });
                }
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(StaticAbility::ZoneCastPermission {
                    player,
                    filter: None,
                    subject: None,
                    zone: Some(zone),
                    mode: CastPermissionMode::PlayFromZone {
                        top_only,
                        lands: true,
                        spells: true,
                    },
                    condition: None,
                });
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
                    if let Some(zone) = zone {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: Some(cast_flt),
                            subject: None,
                            zone: Some(zone),
                            mode: CastPermissionMode::PlayFromZone {
                                top_only,
                                lands: true,
                                spells: true,
                            },
                            condition: None,
                        });
                    }
                }
                ts.restore(plcp);
            }
            if ts.eat_phrase(&["may", "play", "lands", "from"]) {
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(StaticAbility::ZoneCastPermission {
                    player,
                    filter: None,
                    subject: None,
                    zone: Some(zone),
                    mode: CastPermissionMode::PlayFromZone {
                        top_only,
                        lands: true,
                        spells: false,
                    },
                    condition: None,
                });
            }
            if ts.eat_phrase(&["may", "play", "cards", "from"]) {
                if (ts.eat_phrase(&["among", "cards", "exiled", "with"])
                    || ts.eat_phrase(&["among", "cards", "you", "own", "exiled", "with"]))
                    && ts.eat_tilde()
                {
                    return Ok(StaticAbility::ZoneCastPermission {
                        player,
                        filter: None,
                        subject: None,
                        zone: Some(crate::v2::ir::ZoneRef {
                            zone: crate::v2::ir::ZoneKind::Exile,
                            owner: crate::v2::ir::ZoneOwner::Default,
                        }),
                        mode: CastPermissionMode::PlayFromZone {
                            top_only: false,
                            lands: true,
                            spells: true,
                        },
                        condition,
                    });
                }
                if ts.eat_word("among")
                    && let Ok(mut among_flt) = try_parse(ts, parse_object_filter)
                {
                    let zone = among_flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Graveyard,
                        owner: crate::v2::ir::ZoneOwner::You,
                    });
                    return Ok(StaticAbility::ZoneCastPermission {
                        player,
                        filter: Some(among_flt),
                        subject: None,
                        zone: Some(zone),
                        mode: CastPermissionMode::PlayFromZone {
                            top_only: false,
                            lands: true,
                            spells: true,
                        },
                        condition,
                    });
                }
                let top_only = ts.eat_phrase(&["the", "top", "of"]);
                let zone = parse_zone_ref(ts)?;
                return Ok(StaticAbility::ZoneCastPermission {
                    player,
                    filter: None,
                    subject: None,
                    zone: Some(zone),
                    mode: CastPermissionMode::PlayFromZone {
                        top_only,
                        lands: true,
                        spells: true,
                    },
                    condition: None,
                });
            }
            if ts.eat_phrase(&["may", "play", "cards", "exiled", "with"]) && ts.eat_tilde() {
                return Ok(StaticAbility::ZoneCastPermission {
                    player,
                    filter: None,
                    subject: None,
                    zone: Some(crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Exile,
                        owner: crate::v2::ir::ZoneOwner::Default,
                    }),
                    mode: CastPermissionMode::PlayFromZone {
                        top_only: false,
                        lands: true,
                        spells: true,
                    },
                    condition: None,
                });
            }
            if ts.eat_phrase(&[
                "can", "cast", "spells", "only", "any", "time", "they", "could", "cast", "a",
                "sorcery",
            ]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CastOnlyAsSorcery,
                    condition,
                });
            }
            if ts.eat_phrase(&["may", "cast"]) {
                // `You may cast ~ from your graveyard` / `You may cast ~ as though it had flash [if ...]`
                let c_cp = ts.checkpoint();
                if ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) {
                    if ts.eat_phrase(&["without", "paying", "its", "mana", "cost"]) {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::SelfObject),
                            zone: None,
                            mode: CastPermissionMode::WithoutPaying,
                            condition,
                        });
                    }
                    if ts.eat_word("from") {
                        let zone = parse_zone_ref(ts)?;
                        let mut mode = CastPermissionMode::CastFromZone;
                        if ts.eat_phrase(&["using", "its"]) {
                            let _ = ts.next_word();
                            let _ = ts.eat_word("ability");
                        }
                        if ts.eat_word("by") {
                            let costs = parse_cost_list(ts).unwrap_or_default();
                            let _ = ts
                                .eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"])
                                || ts.eat_phrase(&[
                                    "in", "addition", "to", "paying", "its", "other", "costs",
                                ])
                                || ts
                                    .eat_phrase(&["in", "addition", "to", "its", "other", "costs"]);
                            if !costs.is_empty() {
                                mode = CastPermissionMode::AlternativeCost(costs);
                            }
                        }
                        let extra_cond = if ts.eat_word("if") {
                            opt(ts, parse_condition)
                        } else {
                            None
                        };
                        let merged_cond = match (condition, extra_cond) {
                            (Some(prev), Some(c)) => {
                                Some(ConditionExpr::And(Box::new(prev), Box::new(c)))
                            }
                            (None, Some(c)) => Some(c),
                            (prev, None) => prev,
                        };
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::SelfObject),
                            zone: Some(zone),
                            mode,
                            condition: merged_cond,
                        });
                    }
                    if ts.eat_phrase(&["as", "though", "it", "had", "flash"]) {
                        if ts.eat_phrase(&["if", "you", "pay"])
                            && let Some(mc) = ts.eat_mana_cost()
                            && ts.eat_phrase(&["more", "to", "cast", "it"])
                        {
                            return Ok(StaticAbility::ZoneCastPermission {
                                player,
                                filter: Some(ObjectFilter {
                                    any_spell: true,
                                    ..Default::default()
                                }),
                                subject: None,
                                zone: None,
                                mode: CastPermissionMode::AsThoughFlash {
                                    extra_cost: Some(Box::new(crate::v2::ir::Cost::Mana(mc))),
                                },
                                condition: None,
                            });
                        }
                        let cond = if ts.eat_phrase(&["as", "long", "as"]) || ts.eat_word("if") {
                            Some(parse_condition(ts)?)
                        } else {
                            None
                        };
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: Some(ObjectFilter {
                                any_spell: true,
                                ..Default::default()
                            }),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition: cond,
                        });
                    }
                }
                ts.restore(c_cp);
                // `You may cast the first <flt> you cast each turn as though it had flash` (`The Blue Spirit`)
                {
                    let f_cp = ts.checkpoint();
                    if ts.eat_phrase(&["the", "first"])
                        && let Ok(flt) = try_parse(ts, parse_object_filter)
                        && (ts.eat_phrase(&[
                            "you", "cast", "each", "turn", "as", "though", "it", "had", "flash",
                        ]) || ts
                            .eat_phrase(&["each", "turn", "as", "though", "it", "had", "flash"]))
                    {
                        return Ok(StaticAbility::NextSpellModifier {
                            filter: flt.clone(),
                            effect: Box::new(StaticAbility::ZoneCastPermission {
                                player,
                                filter: Some(flt),
                                subject: None,
                                zone: None,
                                mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                                condition,
                            }),
                        });
                    }
                    ts.restore(f_cp);
                }
                if let Ok(mut filter) = try_parse(ts, parse_object_filter) {
                    // Support `<flt1> and <flt2>` (`sorcery spells and Dragon spells`, `Angel spells and Human spells`)
                    {
                        let and_cp = ts.checkpoint();
                        if ts.eat_word("and")
                            && let Ok(other) = try_parse(ts, parse_object_filter)
                        {
                            filter = ObjectFilter {
                                predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![
                                    filter, other,
                                ])],
                                ..Default::default()
                            };
                        } else {
                            ts.restore(and_cp);
                        }
                    }
                    if ts.eat_phrase(&["without", "paying", "their", "mana", "costs"])
                        || ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                    {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: Some(SubjectRef::Group(Box::new(filter))),
                            zone: None,
                            mode: CastPermissionMode::WithoutPaying,
                            condition,
                        });
                    }
                    if filter
                        .flags
                        .contains(crate::v2::ir::FilterFlags::EXILED_WITH_SELF)
                        || ((ts.eat_phrase(&[
                            "from", "among", "cards", "you", "own", "exiled", "with",
                        ]) || ts.eat_phrase(&["from", "among", "cards", "exiled", "with"]))
                            && ts.eat_tilde())
                    {
                        let base = StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: None,
                            zone: Some(crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Exile,
                                owner: crate::v2::ir::ZoneOwner::Default,
                            }),
                            mode: CastPermissionMode::PlayFromZone {
                                top_only: false,
                                lands: false,
                                spells: true,
                            },
                            condition: None,
                        };
                        if ts.eat_phrase(&[
                            "and", "you", "may", "cast", "them", "as", "though", "they", "had",
                            "flash",
                        ]) {
                            return Ok(StaticAbility::Compound(vec![
                                base,
                                StaticAbility::ZoneCastPermission {
                                    player,
                                    filter: Some(filter),
                                    subject: None,
                                    zone: None,
                                    mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                                    condition: None,
                                },
                            ]));
                        }
                        return Ok(base);
                    }
                    if ts.eat_phrase(&["as", "though", "they", "had", "flash"]) {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: Some(filter),
                            subject: None,
                            zone: None,
                            mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                            condition: None,
                        });
                    }
                    if ts.eat_word("from") {
                        if ts.eat_word("among")
                            && let Ok(mut among_flt) = try_parse(ts, parse_object_filter)
                        {
                            let zone = among_flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Graveyard,
                                owner: crate::v2::ir::ZoneOwner::You,
                            });
                            return Ok(StaticAbility::ZoneCastPermission {
                                player,
                                filter: Some(filter),
                                subject: None,
                                zone: Some(zone),
                                mode: CastPermissionMode::PlayFromZone {
                                    top_only: false,
                                    lands: false,
                                    spells: true,
                                },
                                condition,
                            });
                        }
                        let top_only = ts.eat_phrase(&["the", "top", "of"]);
                        if let Ok(zone) = try_parse(ts, parse_zone_ref) {
                            return Ok(StaticAbility::ZoneCastPermission {
                                player,
                                filter: None,
                                subject: None,
                                zone: Some(zone),
                                mode: CastPermissionMode::PlayFromZone {
                                    top_only,
                                    lands: false,
                                    spells: true,
                                },
                                condition: None,
                            });
                        }
                    } else if let Some(zone) = filter.zones.pop() {
                        return Ok(StaticAbility::ZoneCastPermission {
                            player,
                            filter: None,
                            subject: None,
                            zone: Some(zone),
                            mode: CastPermissionMode::PlayFromZone {
                                top_only: false,
                                lands: false,
                                spells: true,
                            },
                            condition: None,
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("not a player-headed static ability"))
}
