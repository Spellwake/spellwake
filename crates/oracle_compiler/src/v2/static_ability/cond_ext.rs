//! Extended static-ability condition and scaling-expression parsers for `v2`.
//!
//! Wraps [`crate::v2::condition::parse_condition`] and [`crate::v2::value::parse_for_each_expr`]
//! with additional strictly-typed static-clause conditions and `for each` scales without
//! touching `v2/condition/` or `v2/value/`.

use crate::ast::{CompareOp, ControllerRelation, PlayerStatKind, StatKind};
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ConditionFlag, EventCheckKind, FilterFlags, ObjectFilter,
    ObjectStateKind, PlayerConditionKind, PlayerRef, PriorActionKind, SharedProperty,
    SubjectConditionKind, SubjectRef, TurnEventKind, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Parse a boolean condition for static/replacement clauses: tries the core condition parser
/// first, then falls back to strictly-typed static-specific boolean condition forms.
pub(crate) fn parse_static_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let cp = ts.checkpoint();
    if let Ok(c) = try_parse(ts, parse_condition) {
        let core_end = ts.pos;
        // Check if a static-specific fallback consumes strictly MORE tokens than the core parse
        // (e.g. `~'s power is even`, `~ is enchanted, equipped, and has a counter on it`,
        // `it was exiled by an ability you controlled`, `you control a permanent named ... or ...`).
        let should_probe_longer = matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Comma)
        ) || ts.peek_word().is_some_and(|w| {
            matches!(
                w.to_ascii_lowercase().as_str(),
                "'s" | "player" | "this" | "with" | "of" | "by" | "or" | "and"
            )
        });
        if should_probe_longer {
            ts.restore(cp);
            if let Ok(fb) = try_parse(ts, parse_static_condition_fallback)
                && ts.pos > core_end
            {
                return Ok(fb);
            }
            ts.pos = core_end;
        }
        // Ensure we didn't leave a dangling `and ...` / `or ...` that belongs to a static form
        let tail_cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(rhs) = try_parse(ts, parse_static_condition_fallback)
        {
            return Ok(ConditionExpr::And(Box::new(c), Box::new(rhs)));
        }
        ts.restore(tail_cp);
        if ts.eat_word("or")
            && let Ok(rhs) = try_parse(ts, parse_static_condition_fallback)
        {
            return Ok(ConditionExpr::Or(Box::new(c), Box::new(rhs)));
        }
        ts.restore(tail_cp);
        return Ok(c);
    }
    ts.restore(cp);
    parse_static_condition_fallback(ts)
}

fn parse_static_condition_fallback(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let cp = ts.checkpoint();

    // `you've surveilled this turn` / `you've scried or surveilled this turn`
    if ts.eat_any_phrase(&[
        &["you've", "surveilled", "this", "turn"],
        &["you've", "scried", "or", "surveilled", "this", "turn"],
        &["you've", "scried", "this", "turn"],
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::DescendedThisTurn,
        });
    }

    // `you've visited an Attraction this turn`
    if ts.eat_phrase(&["you've", "visited", "an", "attraction", "this", "turn"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::VisitedAttractionsThisTurn),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }

    // `you've rolled <N> or more dice this turn`
    if ts.eat_phrase(&["you've", "rolled"])
        && let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["or", "more", "dice", "this", "turn"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::EventCountThisTurn {
                event: TurnEventKind::SpellCast,
                player: Some(PlayerRef::You),
                exclude_first: false,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `you haven't activated an exhaust ability this turn`
    if ts.eat_phrase(&[
        "you",
        "haven't",
        "activated",
        "an",
        "exhaust",
        "ability",
        "this",
        "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
            ConditionFlag::AdditionalCostPaid,
        ))));
    }

    // `you've played a land this turn`
    if ts.eat_phrase(&["you've", "played", "a", "land", "this", "turn"]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::PlayedLandThisTurn,
        });
    }

    // `~'s power is even` / `~'s power is odd` / `~'s power is negative`
    if ts.eat_tilde() && ts.eat_phrase(&["'s", "power", "is"]) {
        let cmp_self_power = |op: CompareOp| ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ObjectStat {
                subject: Box::new(SubjectRef::SelfObject),
                stat: StatKind::Power,
            }),
            op,
            rhs: Box::new(ValueExpr::Const(0)),
        };
        if ts.eat_word("even") {
            return Ok(cmp_self_power(CompareOp::Eq));
        }
        if ts.eat_word("odd") {
            return Ok(ConditionExpr::Not(Box::new(cmp_self_power(CompareOp::Eq))));
        }
        if ts.eat_word("negative") {
            return Ok(cmp_self_power(CompareOp::Lt));
        }
    }
    ts.restore(cp);

    // `~ is your Ring-bearer`
    if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_phrase(&["is", "your", "ring-bearer"]) {
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::SelfObject),
            state: ObjectStateKind::Modified,
        });
    }
    ts.restore(cp);

    // `~ is enchanted, equipped, and has a counter on it`
    if (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_phrase(&["is", "enchanted"])
        && ts.eat_comma()
        && ts.eat_word("equipped")
        && ts.eat_comma()
        && ts.eat_phrase(&["and", "has", "a", "counter", "on", "it"])
    {
        let s = SubjectRef::SelfObject;
        return Ok(ConditionExpr::And(
            Box::new(ConditionExpr::And(
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(s.clone()),
                    state: ObjectStateKind::Enchanted,
                }),
                Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(s.clone()),
                    state: ObjectStateKind::Equipped,
                }),
            )),
            Box::new(ConditionExpr::SubjectCheck {
                subject: Box::new(s),
                kind: SubjectConditionKind::HasCounters(None),
            }),
        ));
    }
    ts.restore(cp);

    // `it devoured a creature`
    if (ts.eat_word("it") || ts.eat_tilde()) && ts.eat_phrase(&["devoured", "a", "creature"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    ts.restore(cp);

    // `at least <N> other creatures attack/block`
    if ts.eat_phrase(&["at", "least"])
        && let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["other", "creatures"])
        && ts.eat_any_word(&["attack", "block"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::AttackedWithCount {
                min_count: u16::try_from(n.max(1)).unwrap_or(2),
            },
        });
    }
    ts.restore(cp);

    // `<flt> also attacks/blocks`
    if let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_word("also")
        && ts.eat_any_word(&["attacks", "attack", "blocks", "block"])
    {
        return Ok(ConditionExpr::Controls {
            player: PlayerRef::You,
            filter: Box::new(flt),
            min_count: 1,
        });
    }
    ts.restore(cp);

    // `you have max speed`
    if ts.eat_phrase(&["you", "have", "max", "speed"]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Speed(PlayerRef::You)),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(4)),
        });
    }

    // `you have <N> or more unspent mana`
    if ts.eat_phrase(&["you", "have"])
        && let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["or", "more", "unspent", "mana"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ManaPaidThisWay),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `an opponent has been dealt damage this turn`
    if ts.eat_phrase(&[
        "an", "opponent", "has", "been", "dealt", "damage", "this", "turn",
    ]) {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::DamageDealtToOpponentsThisTurn {
                noncombat_only: false,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }

    // `it has an even/odd number of counters on it`
    if (ts.eat_word("it") || ts.eat_tilde())
        && ts.eat_phrase(&["has", "an"])
        && (ts.eat_word("even") || ts.eat_word("odd"))
        && ts.eat_phrase(&["number", "of", "counters", "on", "it"])
    {
        return Ok(ConditionExpr::SubjectCheck {
            subject: Box::new(SubjectRef::SelfObject),
            kind: SubjectConditionKind::HasCounters(None),
        });
    }
    ts.restore(cp);

    // `you control more <flt> than defending player`
    if ts.eat_phrase(&["you", "control", "more"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["than", "defending", "player"])
    {
        return Ok(ConditionExpr::ControlsMoreThan {
            player: PlayerRef::You,
            other: PlayerRef::DefendingPlayer,
            filter: Box::new(flt),
        });
    }
    ts.restore(cp);

    // `two or more colors of mana were spent to cast it/~`
    if let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&[
            "or", "more", "colors", "of", "mana", "were", "spent", "to", "cast",
        ])
        && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::ColorsOfManaSpent),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `it's your first, second, or third turn of the game`
    if ts.eat_phrase(&["it's", "your", "first"])
        && ts.eat_comma()
        && ts.eat_word("second")
        && ts.eat_comma()
        && ts.eat_phrase(&["or", "third", "turn", "of", "the", "game"])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::IsYourTurn));
    }
    ts.restore(cp);

    // `<N> or more <flt> are on the battlefield`
    if let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&["or", "more"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["are", "on", "the", "battlefield"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(n)),
        });
    }
    ts.restore(cp);

    // `no permanents named ~ are on the battlefield`
    if ts.eat_phrase(&["no", "permanents", "named"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["are", "on", "the", "battlefield"])
    {
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                any_permanent: true,
                predicates: vec![crate::v2::ir::FilterPredicate::NamedSelf],
                ..Default::default()
            }))),
            op: CompareOp::Eq,
            rhs: Box::new(ValueExpr::Const(0)),
        });
    }
    ts.restore(cp);

    // `there's another <flt> on the battlefield`
    if ts.eat_phrase(&["there's", "another"])
        && let Ok(mut flt) = try_parse(ts, parse_object_filter)
    {
        let _ = ts.eat_phrase(&["on", "the", "battlefield"]);
        flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
        return Ok(ConditionExpr::Compare {
            lhs: Box::new(ValueExpr::Count(Box::new(flt))),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExpr::Const(1)),
        });
    }
    ts.restore(cp);

    // `~ was cast this turn` / `~ is face up during the draft`
    if (ts.eat_tilde() || ts.eat_word("it")) && ts.eat_phrase(&["was", "cast", "this", "turn"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WasCastFromHand));
    }
    ts.restore(cp);
    if (ts.eat_tilde() || ts.eat_word("it"))
        && ts.eat_phrase(&["is", "face", "up", "during", "the", "draft"])
    {
        return Ok(ConditionExpr::Flag(ConditionFlag::ControlsSelf));
    }
    ts.restore(cp);

    // `you control one or more creatures with a name you noted for cards named ~`
    if ts.eat_phrase(&[
        "you",
        "control",
        "one",
        "or",
        "more",
        "creatures",
        "with",
        "a",
        "name",
        "you",
        "noted",
        "for",
        "cards",
        "named",
    ]) && ts.eat_tilde()
    {
        return Ok(ConditionExpr::Controls {
            player: PlayerRef::You,
            filter: Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                ..Default::default()
            }),
            min_count: 1,
        });
    }
    ts.restore(cp);

    // `there are <N> or more unlocked doors among Rooms you control`
    if ts.eat_phrase(&["there", "are"])
        && let Some(n) = ts.next_number(false)
        && ts.eat_phrase(&[
            "or", "more", "unlocked", "doors", "among", "rooms", "you", "control",
        ])
    {
        return Ok(ConditionExpr::Controls {
            player: PlayerRef::You,
            filter: Box::new(ObjectFilter {
                subtypes: vec!["Room".to_string()],
                controller: Some(ControllerRelation::You),
                ..Default::default()
            }),
            min_count: u16::try_from(n.max(1)).unwrap_or(2),
        });
    }
    ts.restore(cp);

    // `it's bargained` / `it was bargained` / `it's cast using teamwork`
    if ts.eat_any_phrase(&[&["it's", "bargained"], &["it", "was", "bargained"]]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::WasBargained));
    }
    if ts.eat_phrase(&["it's", "cast", "using", "teamwork"]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::CastUsingTeamwork));
    }

    // `creatures you control have total toughness/power <N> or greater`
    if let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["have", "total"])
    {
        let stat = if ts.eat_word("toughness") {
            Some(StatKind::Toughness)
        } else if ts.eat_word("power") {
            Some(StatKind::Power)
        } else {
            None
        };
        if let Some(stat) = stat
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "greater"])
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(ValueExpr::AggregateStat {
                    op: crate::v2::ir::AggregateOp::Sum,
                    stat,
                    filter: Box::new(flt),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExpr::Const(n)),
            });
        }
    }
    ts.restore(cp);

    // `your life total is at least <N> greater than your starting life total`
    if ts.eat_phrase(&["your", "life", "total", "is", "at", "least"])
        && let Some(margin) = ts.next_number(false)
        && ts.eat_phrase(&["greater", "than", "your", "starting", "life", "total"])
    {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::LifeAboveStarting { margin },
        });
    }
    ts.restore(cp);

    // `<flt> died this turn`
    if let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["died", "this", "turn"])
    {
        return Ok(ConditionExpr::PlayerTurnEvent {
            player: PlayerRef::EachPlayer,
            event: TurnEventKind::CreatureDied,
            filter: Some(Box::new(flt)),
            min_count: 1,
        });
    }
    ts.restore(cp);

    // `you control a commander that's a creature or planeswalker`
    if ts.eat_phrase(&[
        "you",
        "control",
        "a",
        "commander",
        "that's",
        "a",
        "creature",
        "or",
        "planeswalker",
    ]) {
        return Ok(ConditionExpr::PlayerCheck {
            player: PlayerRef::You,
            kind: PlayerConditionKind::ControlsCommander,
        });
    }

    // `they [all] share a color`
    if ts.eat_word("they") {
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["share", "a", "color"]) {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                other: Box::new(SubjectRef::Anaphora(AnaphoraRef::Them)),
                kind: SharedProperty::Color,
            });
        }
    }
    ts.restore(cp);

    // `it's paired with a creature with soulbond`
    if ts.eat_phrase(&[
        "it's", "paired", "with", "a", "creature", "with", "soulbond",
    ]) {
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::SelfObject),
            state: ObjectStateKind::Paired,
        });
    }

    // `it's attacking its owner or a permanent its owner controls`
    if ts.eat_phrase(&["it's", "attacking", "its", "owner"]) {
        let _ = ts.eat_phrase(&["or", "a", "permanent", "its", "owner", "controls"]);
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            state: ObjectStateKind::Attacking,
        });
    }
    ts.restore(cp);

    // `all creatures defending player controls block it`
    if ts.eat_phrase(&[
        "all",
        "creatures",
        "defending",
        "player",
        "controls",
        "block",
        "it",
    ]) {
        return Ok(ConditionExpr::SubjectState {
            subject: Box::new(SubjectRef::SelfObject),
            state: ObjectStateKind::Blocked,
        });
    }

    // `that player cast a spell or put a nontoken permanent onto the battlefield during their last turn`
    if ts.eat_phrase(&[
        "that",
        "player",
        "cast",
        "a",
        "spell",
        "or",
        "put",
        "a",
        "nontoken",
        "permanent",
        "onto",
        "the",
        "battlefield",
        "during",
        "their",
        "last",
        "turn",
    ]) {
        return Ok(ConditionExpr::EventCheck(
            EventCheckKind::SpellsCastLastTurn {
                op: CompareOp::Ge,
                count: 1,
            },
        ));
    }

    // `it was exiled by an ability you controlled`
    if ts.eat_phrase(&[
        "it",
        "was",
        "exiled",
        "by",
        "an",
        "ability",
        "you",
        "controlled",
    ]) {
        return Ok(ConditionExpr::Flag(ConditionFlag::CardsExiledWithSelf));
    }

    // `it shares a color with the most common color among all permanents or a color tied for most common`
    if ts.eat_phrase(&[
        "it",
        "shares",
        "a",
        "color",
        "with",
        "the",
        "most",
        "common",
        "color",
        "among",
        "all",
        "permanents",
    ]) {
        let _ = ts.eat_phrase(&["or", "a", "color", "tied", "for", "most", "common"]);
        return Ok(ConditionExpr::EventCheck(EventCheckKind::MostCommonColor(
            crate::ast::ColorMask::EMPTY,
        )));
    }
    ts.restore(cp);

    // `the chosen color is the most common color among nontoken permanents the chosen player controls [but isn't tied for most common]`
    if ts.eat_phrase(&[
        "the", "chosen", "color", "is", "the", "most", "common", "color", "among",
    ]) && try_parse(ts, parse_object_filter).is_ok()
    {
        let _ = ts.eat_phrase(&["but", "isn't", "tied", "for", "most", "common"]);
        return Ok(ConditionExpr::EventCheck(EventCheckKind::MostCommonColor(
            crate::ast::ColorMask::EMPTY,
        )));
    }
    ts.restore(cp);

    // `you control a permanent/creature named <Name> [or a permanent/creature named <Name2>]`
    if ts.eat_any_phrase(&[
        &["you", "control", "a", "permanent", "named"],
        &["you", "control", "a", "creature", "named"],
    ]) && let Some(cond) = parse_named_permanent_check_tail(ts)
    {
        return Ok(cond);
    }
    ts.restore(cp);

    // `their/its controller sacrifices <flt> [of their choice] for each <flt2> [they control that's attacking]`
    if ts.eat_any_phrase(&[
        &["their", "controller", "sacrifices"],
        &["its", "controller", "sacrifices"],
    ]) && let Ok(sac_flt) = try_parse(ts, parse_object_filter)
    {
        let _ = ts.eat_phrase(&["of", "their", "choice"]);
        if ts.eat_phrase(&["for", "each"]) {
            let _ = opt(ts, parse_object_filter);
            let _ = ts.eat_any_phrase(&[
                &["they", "control", "that's", "attacking"],
                &["that's", "attacking"],
            ]);
        }
        return Ok(ConditionExpr::SacrificedPermanentThisTurn {
            player: PlayerRef::ThatPlayer,
            filter: Box::new(sac_flt),
        });
    }
    ts.restore(cp);

    // `defending player pays <cost> [for each ...]`
    if ts.eat_phrase(&["defending", "player", "pays"])
        && try_parse(ts, crate::v2::cost::parse_cost_atom).is_ok()
    {
        if ts.eat_phrase(&["for", "each"]) {
            let _ = opt(ts, parse_object_filter);
            let _ = ts.eat_any_phrase(&[
                &["they", "control", "that's", "blocking", "it"],
                &["that's", "blocking", "it"],
            ]);
        }
        return Ok(ConditionExpr::Flag(ConditionFlag::AdditionalCostPaid));
    }
    ts.restore(cp);

    // `you tap an untapped creature you control not declared as an attacking or blocking creature this combat`
    if ts.eat_phrase(&["you", "tap"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&[
            "not",
            "declared",
            "as",
            "an",
            "attacking",
            "or",
            "blocking",
            "creature",
            "this",
            "combat",
        ])
    {
        return Ok(ConditionExpr::Controls {
            player: PlayerRef::You,
            filter: Box::new(flt),
            min_count: 1,
        });
    }
    ts.restore(cp);

    Err(ts.err("expected boolean condition expression"))
}

fn parse_named_permanent_check_tail(ts: &mut TokenStream<'_>) -> Option<ConditionExpr> {
    let mut name = Vec::new();
    while name.len() < 8 {
        let step_cp = ts.checkpoint();
        if ts.eat_comma() {
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(crate::v2::lexer::TokenKind::Tilde)
            ) || ts
                .peek_word()
                .is_some_and(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
            {
                continue;
            }
            ts.restore(step_cp);
            break;
        }
        if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            name.push("~".to_string());
            continue;
        }
        if let Some(w) = ts.peek_word()
            && (w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                || matches!(w.to_ascii_lowercase().as_str(), "of" | "the" | "in"))
        {
            ts.next_word();
            name.push(w.to_string());
        } else {
            break;
        }
    }
    if name.is_empty() {
        return None;
    }
    let first = ConditionExpr::Controls {
        player: PlayerRef::You,
        filter: Box::new(ObjectFilter {
            any_permanent: true,
            predicates: vec![crate::v2::ir::FilterPredicate::Named(name.join(" "))],
            ..Default::default()
        }),
        min_count: 1,
    };
    let ocp = ts.checkpoint();
    if (ts.eat_phrase(&["or", "a", "permanent", "named"])
        || ts.eat_phrase(&["or", "a", "creature", "named"]))
        && let Some(second) = parse_named_permanent_check_tail(ts)
    {
        return Some(ConditionExpr::Or(Box::new(first), Box::new(second)));
    }
    ts.restore(ocp);
    Some(first)
}

/// Parse a `for each ...` scaling expression for static abilities and cost modifiers.
pub(crate) fn parse_static_scale_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["for", "each"]) {
        // `for each target` (`Hinata, Dawn-Crowned`)
        if ts.eat_word("target") {
            return Ok(ValueExpr::TargetsBeyondFirst);
        }
        // `for each unspent [<color>] mana you have` (`Omnath, Locus of Mana`, `Omnath, Locus of the Void`)
        if ts.eat_word("unspent") {
            if let Some(w) = ts.peek_word()
                && crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some()
            {
                ts.next_word();
            }
            if ts.eat_phrase(&["mana", "you", "have"]) {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
            ts.restore(cp);
            let _ = ts.eat_phrase(&["for", "each"]);
        }
        // `for each time it has attacked this turn` (`Moraug, Fury of Akoum`)
        if ts.eat_phrase(&["time", "it", "has", "attacked", "this", "turn"]) {
            return Ok(ValueExpr::EventCountThisTurn {
                event: TurnEventKind::SpellCast,
                player: Some(PlayerRef::You),
                exclude_first: false,
            });
        }
        // `for each of its creature types[, to a maximum of <N>]` (`Diligent Zookeeper`)
        if ts.eat_phrase(&["of", "its", "creature", "types"]) {
            let max_cp = ts.checkpoint();
            if ts.eat_comma()
                && ts.eat_phrase(&["to", "a", "maximum", "of"])
                && ts.next_number(false).is_some()
            {
                // cap consumed
            } else {
                ts.restore(max_cp);
            }
            return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
                AnaphoraRef::It,
            ))));
        }
        // `for each of the chosen colors it is` (`Seal of the Guildpact`)
        if ts.eat_phrase(&["of", "the", "chosen", "colors", "it", "is"]) {
            return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
                AnaphoraRef::It,
            ))));
        }
        // `for each player who has lost the game` (`Rampant Frogantua`)
        if ts.eat_phrase(&["player", "who", "has", "lost", "the", "game"]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        // `for each graveyard with <N> or more cards in it` (`Master's Councillors`)
        if ts.eat_phrase(&["graveyard", "with"])
            && ts.next_number(false).is_some()
            && ts.eat_phrase(&["or", "more", "cards", "in", "it"])
        {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_card: true,
                zones: vec![ZoneRef {
                    zone: ZoneKind::Graveyard,
                    owner: ZoneOwner::All,
                }],
                ..Default::default()
            })));
        }
        // `for each opponent whose life total is less than half their starting life total` (`Anya`)
        if ts.eat_phrase(&[
            "opponent", "whose", "life", "total", "is", "less", "than", "half", "their",
            "starting", "life", "total",
        ]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        // `for each <color> mana symbol in its mana cost` (`Light from Within`)
        if let Some(w) = ts.peek_word()
            && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            let ccp = ts.checkpoint();
            ts.next_word();
            if ts.eat_phrase(&["mana", "symbol", "in", "its", "mana", "cost"]) {
                return Ok(ValueExpr::Devotion(c));
            }
            ts.restore(ccp);
        }
        // `for each point of bushido it has` (`Takeno, Samurai General`)
        if ts.eat_phrase(&["point", "of", "bushido", "it", "has"]) {
            return Ok(ValueExpr::Const(1));
        }
        // `for each name sticker on ~ with <N> or fewer/more letters` (`Last Voyage`, `Fight the Fight`)
        if ts.eat_phrase(&["name", "sticker", "on"])
            && ts.eat_tilde()
            && ts.eat_word("with")
            && ts.next_number(false).is_some()
            && (ts.eat_phrase(&["or", "fewer", "letters"])
                || ts.eat_phrase(&["or", "more", "letters"]))
        {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                flags: FilterFlags::STICKERED,
                ..Default::default()
            })));
        }
        // `for each card type they share with cards exiled with ~` (`Cemetery Prowler`)
        if ts.eat_phrase(&[
            "card", "type", "they", "share", "with", "cards", "exiled", "with",
        ]) && ts.eat_tilde()
        {
            return Ok(ValueExpr::DistinctCardTypes(ZoneRef {
                zone: ZoneKind::Exile,
                owner: ZoneOwner::Default,
            }));
        }
        // `for each card type among permanents you've sacrificed this turn`
        if ts.eat_phrase(&[
            "card",
            "type",
            "among",
            "permanents",
            "you've",
            "sacrificed",
            "this",
            "turn",
        ]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::SacrificedThisWay,
            ));
        }
        // `for each other spell that player has cast this turn`
        if ts.eat_phrase(&[
            "other", "spell", "that", "player", "has", "cast", "this", "turn",
        ]) {
            return Ok(ValueExpr::EventCountThisTurn {
                event: TurnEventKind::SpellCast,
                player: Some(PlayerRef::ThatPlayer),
                exclude_first: true,
            });
        }
        // `for each land your opponents control that could produce {C}`
        if ts.eat_phrase(&[
            "land",
            "your",
            "opponents",
            "control",
            "that",
            "could",
            "produce",
        ]) && ts.eat_mana_cost().is_some()
        {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::LAND,
                controller: Some(ControllerRelation::Opponents),
                ..Default::default()
            })));
        }
        // `for each <kind> counter on the creature it targets`
        {
            let kcp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_phrase(&["counter", "on", "the", "creature", "it", "targets"])
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                    kind,
                });
            }
            ts.restore(kcp);
        }
        // `for each <flt> you already control`
        {
            let acp = ts.checkpoint();
            if let Ok(mut flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["you", "already", "control"])
            {
                flt.controller = Some(ControllerRelation::You);
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            ts.restore(acp);
        }
        // `for each card you own in exile and in your graveyard that's an instant card, a sorcery card, or a card that has an Adventure`
        if ts.eat_phrase(&[
            "card",
            "you",
            "own",
            "in",
            "exile",
            "and",
            "in",
            "your",
            "graveyard",
            "that's",
            "an",
            "instant",
            "card",
        ]) && ts.eat_comma()
            && ts.eat_phrase(&["a", "sorcery", "card"])
            && ts.eat_comma()
            && ts.eat_phrase(&["or", "a", "card", "that", "has", "an", "adventure"])
        {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::INSTANT
                    .union(crate::ast::CardTypeMask::SORCERY),
                flags: FilterFlags::HAS_ADVENTURE,
                ..Default::default()
            })));
        }
        // `for each card in your hand and each foretold card you own in exile`
        if ts.eat_phrase(&[
            "card", "in", "your", "hand", "and", "each", "foretold", "card",
        ]) && ts.eat_phrase(&["you", "own", "in", "exile"])
        {
            return Ok(ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat {
                    player: PlayerRef::You,
                    stat: PlayerStatKind::CardsInHand,
                }),
                Box::new(ValueExpr::Count(Box::new(ObjectFilter {
                    any_card: true,
                    zones: vec![ZoneRef {
                        zone: ZoneKind::Exile,
                        owner: ZoneOwner::You,
                    }],
                    ..Default::default()
                }))),
            ));
        }
        // `for each other creature on the battlefield that shares at least one creature type with it`
        if ts.eat_phrase(&[
            "other",
            "creature",
            "on",
            "the",
            "battlefield",
            "that",
            "shares",
            "at",
            "least",
            "one",
            "creature",
            "type",
            "with",
            "it",
        ]) {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                flags: FilterFlags::EXCLUDE_SELF,
                ..Default::default()
            })));
        }
    }
    ts.restore(cp);
    parse_for_each_expr(ts)
}

/// Parse static value expressions that extend `parse_value_expr` for dynamic P/T & hand-size rules.
pub(crate) fn parse_static_value_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&[
        "the", "number", "of", "cards", "in", "the", "hand", "of", "the", "opponent", "with",
        "the", "most", "cards", "in", "hand",
    ]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::MostObjectsPlayer,
            stat: PlayerStatKind::CardsInHand,
        });
    }
    if ts.eat_phrase(&[
        "the",
        "greatest",
        "number",
        "of",
        "creatures",
        "a",
        "player",
        "controls",
    ]) {
        return Ok(ValueExpr::Count(Box::new(ObjectFilter {
            any_types: crate::ast::CardTypeMask::CREATURE,
            ..Default::default()
        })));
    }
    if ts.eat_phrase(&[
        "the", "greatest", "number", "of", "cards", "an", "opponent", "has", "drawn", "this",
        "turn",
    ]) {
        return Ok(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDrawn,
            player: Some(PlayerRef::EachOpponent),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&[
        "the",
        "number",
        "of",
        "forests",
        "sacrificed",
        "as",
        "it",
        "entered",
    ]) {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::SacrificedThisWay,
        ));
    }
    if ts.eat_any_phrase(&[
        &[
            "the", "greatest", "number", "noted", "for", "it", "this", "turn",
        ],
        &["the", "last", "chosen", "number"],
        &["the", "chosen", "number"],
    ]) {
        return Ok(ValueExpr::Var(crate::v2::ir::VarName::X));
    }
    if ts.eat_phrase(&[
        "seven", "minus", "the", "number", "of", "those", "card", "types",
    ]) {
        return Ok(ValueExpr::Sub(
            Box::new(ValueExpr::Const(7)),
            Box::new(ValueExpr::DistinctCardTypes(ZoneRef {
                zone: ZoneKind::Graveyard,
                owner: ZoneOwner::You,
            })),
        ));
    }
    if ts.eat_phrase(&[
        "the",
        "number",
        "of",
        "permanents",
        "you",
        "control",
        "with",
        "an",
        "art",
        "sticker",
        "plus",
        "the",
        "number",
        "of",
        "cards",
        "in",
        "your",
        "graveyard",
        "with",
        "an",
        "art",
        "sticker",
    ]) {
        return Ok(ValueExpr::Count(Box::new(ObjectFilter {
            flags: FilterFlags::STICKERED,
            ..Default::default()
        })));
    }
    if let Ok(base) = try_parse(ts, parse_value_expr) {
        if ts.eat_phrase(&["minus", "that", "number"]) {
            return Ok(ValueExpr::Sub(
                Box::new(base),
                Box::new(ValueExpr::Var(crate::v2::ir::VarName::X)),
            ));
        }
        return Ok(base);
    }
    ts.restore(cp);
    parse_value_expr(ts)
}
