//! Strictly-typed recursive [`ConditionExpr`] parser for `v2`.

use crate::ast::CompareOp;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_kind, parse_object_filter, parse_player_ref,
    parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, ConditionExpr, ObjectStateKind, PlayerRef, PriorActionKind, SubjectRef,
    TurnEventKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed boolean [`ConditionExpr`].
pub fn parse_condition(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    let mut cond = parse_condition_atom(ts)?;
    loop {
        let cp = ts.checkpoint();
        if ts.eat_word("and") {
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::And(Box::new(cond), Box::new(rhs));
                continue;
            }
            ts.restore(cp);
            break;
        }
        if ts.eat_word("or") {
            if let Ok(rhs) = try_parse(ts, parse_condition_atom) {
                cond = ConditionExpr::Or(Box::new(cond), Box::new(rhs));
                continue;
            }
            ts.restore(cp);
            break;
        }
        break;
    }
    Ok(cond)
}

fn parse_condition_atom(ts: &mut TokenStream<'_>) -> PResult<ConditionExpr> {
    // Turn / cast / kicker checks
    if ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "the", "turn",
    ]) || ts.eat_phrase(&[
        "it's", "the", "first", "combat", "phase", "of", "your", "turn",
    ]) {
        return Ok(ConditionExpr::IsFirstCombatPhase);
    }
    if ts.eat_phrase(&["it's", "your", "turn"]) {
        return Ok(ConditionExpr::IsYourTurn);
    }
    if ts.eat_phrase(&["it's", "night"]) {
        return Ok(ConditionExpr::IsNight);
    }
    if ts.eat_phrase(&["it's", "day"]) {
        return Ok(ConditionExpr::IsDay);
    }
    if ts.eat_phrase(&["it's", "not", "your", "turn"])
        || ts.eat_phrase(&["it's", "not", "their", "turn"])
    {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::IsYourTurn)));
    }
    if ts.eat_phrase(&["you", "have", "the", "city's", "blessing"]) {
        return Ok(ConditionExpr::HasCitysBlessing);
    }
    if ts.eat_phrase(&["you've", "been", "attacked", "this", "step"])
        || ts.eat_phrase(&["you", "were", "attacked", "this", "step"])
    {
        return Ok(ConditionExpr::AttackedThisStep(PlayerRef::You));
    }
    if ts.eat_phrase(&["you", "attacked", "this", "turn"]) {
        return Ok(ConditionExpr::AttackedThisTurn(PlayerRef::You));
    }
    if ts.eat_phrase(&["you're", "the", "monarch"])
        || ts.eat_phrase(&["you", "are", "the", "monarch"])
    {
        return Ok(ConditionExpr::IsMonarch(PlayerRef::You));
    }
    if ts.eat_phrase(&["you", "have", "a", "full", "party"]) {
        return Ok(ConditionExpr::HasFullParty(PlayerRef::You));
    }
    if ts.eat_phrase(&["you've", "completed", "a", "dungeon"])
        || ts.eat_phrase(&["you", "have", "completed", "a", "dungeon"])
    {
        return Ok(ConditionExpr::CompletedDungeon(PlayerRef::You));
    }
    if ts.eat_phrase(&["you", "descended", "this", "turn"]) {
        return Ok(ConditionExpr::DescendedThisTurn(PlayerRef::You));
    }
    if ts.eat_phrase(&[
        "you", "haven't", "cast", "a", "spell", "from", "your", "hand", "this", "turn",
    ]) {
        return Ok(ConditionExpr::Not(Box::new(
            ConditionExpr::CastSpellFromHandThisTurn(PlayerRef::You),
        )));
    }
    if ts.eat_phrase(&["you", "control", "your", "commander"])
        || ts.eat_phrase(&["you", "control", "a", "commander"])
    {
        return Ok(ConditionExpr::ControlsCommander(PlayerRef::You));
    }
    if ts.eat_phrase(&["tribute", "wasn't", "paid"]) {
        return Ok(ConditionExpr::TributeNotPaid);
    }
    if ts.eat_phrase(&[
        "two", "or", "more", "players", "have", "lost", "the", "game",
    ]) {
        return Ok(ConditionExpr::PlayersLostGame { min_count: 2 });
    }
    if ts.eat_phrase(&[
        "this", "is", "the", "first", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "first", "time"])
    {
        return Ok(ConditionExpr::ResolutionCountThisTurn(1));
    }
    if ts.eat_phrase(&[
        "this", "is", "the", "second", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "second", "time"])
    {
        return Ok(ConditionExpr::ResolutionCountThisTurn(2));
    }
    if ts.eat_phrase(&[
        "this", "is", "the", "third", "time", "this", "ability", "has", "resolved", "this", "turn",
    ]) || ts.eat_phrase(&["it's", "the", "third", "time"])
    {
        return Ok(ConditionExpr::ResolutionCountThisTurn(3));
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["'s", "additional", "cost", "was", "paid"]) {
            return Ok(ConditionExpr::AdditionalCostPaid);
        }
        ts.restore(acp);
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "cast", "using", "teamwork"]) {
            return Ok(ConditionExpr::CastUsingTeamwork);
        }
        ts.restore(tcp);
    }
    {
        let gcp = ts.checkpoint();
        if ts.eat_phrase(&["it", "shares", "a", "creature", "type", "with"])
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::SharesTypeWith {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                other: Box::new(other),
                kind: crate::v2::ir::SharedProperty::CreatureType,
            });
        }
        ts.restore(gcp);
        if ts.eat_phrase(&["it", "has", "greater", "power", "or", "toughness", "than"])
            && let Ok(other) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::HasGreaterStatThan {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                other: Box::new(other),
                stat: None,
            });
        }
        ts.restore(gcp);
    }
    // `there are no <kind> counters on <subj>` / `there are <N> or more mana values among cards in <zone>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counters") || ts.eat_word("counter"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                    subject: Box::new(subject),
                    kind,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
            });
        }
        ts.restore(ncp);
    }
    if ts.eat_phrase(&["you", "win", "the", "flip"]) || ts.eat_phrase(&["you", "win"]) {
        return Ok(ConditionExpr::WonCoinFlip);
    }
    if ts.eat_phrase(&["you", "lose", "the", "flip"]) || ts.eat_phrase(&["you", "lose"]) {
        return Ok(ConditionExpr::Not(Box::new(ConditionExpr::WonCoinFlip)));
    }
    if ts.eat_phrase(&["you", "search", "your", "library", "this", "way"])
        || ts.eat_phrase(&[
            "that", "player", "searches", "their", "library", "this", "way",
        ])
        || ts.eat_phrase(&["they", "search", "their", "library", "this", "way"])
    {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::SearchedThisWay,
        ));
    }
    if ts.eat_phrase(&["no", "spells", "were", "cast", "last", "turn"]) {
        return Ok(ConditionExpr::SpellsCastLastTurn {
            op: CompareOp::Eq,
            count: 0,
        });
    }
    if ts.eat_phrase(&[
        "two", "or", "more", "spells", "were", "cast", "last", "turn",
    ]) || ts.eat_phrase(&[
        "a", "player", "cast", "two", "or", "more", "spells", "last", "turn",
    ]) {
        return Ok(ConditionExpr::SpellsCastLastTurn {
            op: CompareOp::Ge,
            count: 2,
        });
    }
    {
        let mcp = ts.checkpoint();
        if ts.eat_phrase(&["at", "least"])
            && let Some(n) = ts.next_number(false)
        {
            if let Some(w) = ts.peek_word()
                && let Some(color) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "was", "spent", "to", "cast"])
                    && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
                {
                    return Ok(ConditionExpr::ColoredManaSpent {
                        color,
                        min_count: u8::try_from(n.max(1)).unwrap_or(1),
                    });
                }
            }
            ts.restore(mcp);
            if ts.eat_phrase(&["at", "least"])
                && let Ok(val) = try_parse(ts, parse_value_expr)
                && ts.eat_phrase(&["mana", "was", "spent", "to", "cast"])
                && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            {
                return Ok(ConditionExpr::ManaSpentCompare {
                    op: CompareOp::Ge,
                    value: Box::new(val),
                });
            }
        }
        ts.restore(mcp);
    }
    if ts.eat_phrase(&[
        "it", "wasn't", "cast", "or", "no", "mana", "was", "spent", "to", "cast", "it",
    ]) || ts.eat_phrase(&["no", "mana", "was", "spent", "to", "cast", "it"])
    {
        return Ok(ConditionExpr::NotCastOrNoManaSpent(Box::new(
            SubjectRef::Anaphora(AnaphoraRef::It),
        )));
    }
    if ts.eat_phrase(&["an", "opponent", "is", "poisoned"]) {
        return Ok(ConditionExpr::PlayerPoisoned(PlayerRef::AnyOpponent));
    }
    if ts.eat_phrase(&["a", "player", "is", "poisoned"]) {
        return Ok(ConditionExpr::PlayerPoisoned(PlayerRef::AnyPlayer));
    }
    if ts.eat_phrase(&["you", "are", "poisoned"]) || ts.eat_phrase(&["you're", "poisoned"]) {
        return Ok(ConditionExpr::PlayerPoisoned(PlayerRef::You));
    }
    // `<N> or more <flt> entered [the battlefield] [under your control] this turn`
    {
        let ecp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["entered", "the", "battlefield"]) || ts.eat_word("entered"))
        {
            let controller = if ts.eat_phrase(&["under", "your", "control"]) {
                PlayerRef::You
            } else {
                PlayerRef::EachPlayer
            };
            if ts.eat_phrase(&["this", "turn"]) {
                return Ok(ConditionExpr::PermanentEnteredThisTurn {
                    filter: Box::new(flt),
                    controller,
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                });
            }
        }
        ts.restore(ecp);
    }
    // `you don't/didn't put <subj> onto the battlefield / into your hand [this way]`
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["you", "don't", "put"]) || ts.eat_phrase(&["you", "didn't", "put"]))
            && try_parse(ts, parse_subject_ref).is_ok()
            && let Ok((dest, _)) = try_parse(ts, crate::v2::filter::parse_zone_dest)
        {
            let _ = ts.eat_phrase(&["this", "way"]);
            return Ok(ConditionExpr::DidntPutInZone { zone: dest.zone });
        }
        ts.restore(cp);
    }
    // `<zone> has <N> or more cards in it` (`a graveyard has twenty or more cards in it`)
    {
        let cp = ts.checkpoint();
        if let Ok(zone) = try_parse(ts, parse_zone_ref)
            && ts.eat_word("has")
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(crate::v2::ir::ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }),
                zone,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["the", "gift", "was", "promised"]) {
        return Ok(ConditionExpr::GiftWasPromised);
    }
    if ts.eat_phrase(&["it", "was", "bargained"])
        || ts.eat_phrase(&["this", "spell", "was", "bargained"])
    {
        return Ok(ConditionExpr::WasBargained);
    }
    if ts.eat_phrase(&["no", "colored", "mana", "was", "spent", "to", "cast", "it"])
        || ts.eat_phrase(&[
            "no", "colored", "mana", "was", "spent", "to", "cast", "this", "spell",
        ])
    {
        return Ok(ConditionExpr::ManaSpentNoColor);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "bargained"]) {
            return Ok(ConditionExpr::WasBargained);
        }
        ts.restore(cp);
    }
    // `a <flt> was/is milled/exiled/discarded/destroyed/revealed this way`
    {
        let cp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("was")
                || ts.eat_word("were")
                || ts.eat_word("is")
                || ts.eat_word("are"))
        {
            let act = if ts.eat_phrase(&["milled", "this", "way"]) {
                Some(PriorActionKind::MilledThisWay)
            } else if ts.eat_phrase(&["exiled", "this", "way"]) {
                Some(PriorActionKind::ExiledThisWay)
            } else if ts.eat_phrase(&["discarded", "this", "way"]) {
                Some(PriorActionKind::DiscardedThisWay)
            } else if ts.eat_phrase(&["destroyed", "this", "way"]) {
                Some(PriorActionKind::DestroyedThisWay)
            } else if ts.eat_phrase(&["revealed", "this", "way"]) {
                Some(PriorActionKind::RevealedThisWay)
            } else if ts.eat_phrase(&["sacrificed", "this", "way"]) {
                Some(PriorActionKind::SacrificedThisWay)
            } else if ts.eat_phrase(&[
                "put",
                "into",
                "a",
                "graveyard",
                "from",
                "the",
                "battlefield",
                "this",
                "turn",
            ]) || ts.eat_phrase(&[
                "put",
                "into",
                "your",
                "graveyard",
                "from",
                "anywhere",
                "this",
                "turn",
            ]) || ts.eat_phrase(&[
                "put",
                "into",
                "a",
                "graveyard",
                "from",
                "anywhere",
                "this",
                "turn",
            ]) {
                Some(PriorActionKind::DestroyedThisWay)
            } else {
                None
            };
            if let Some(action) = act {
                return Ok(ConditionExpr::PriorActionMatched {
                    action,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["it", "was", "kicked"]) || ts.eat_phrase(&["that", "spell", "was", "kicked"])
    {
        if ts.eat_phrase(&["with", "its"])
            && let Some(mc) = ts.eat_mana_cost()
            && ts.eat_word("kicker")
        {
            return Ok(ConditionExpr::KickedWith(mc));
        }
        let _ = ts.eat_word("twice");
        return Ok(ConditionExpr::WasKicked);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "cast", "it"])
            || (ts.eat_phrase(&["you", "cast"]) && ts.eat_tilde())
        {
            let _ = ts.eat_phrase(&["from", "your", "hand"])
                || ts.eat_phrase(&["during", "your", "main", "phase"]);
            return Ok(ConditionExpr::YouCastIt);
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&[
        "a",
        "permanent",
        "left",
        "the",
        "battlefield",
        "under",
        "your",
        "control",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "a",
        "permanent",
        "you",
        "controlled",
        "left",
        "the",
        "battlefield",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "a",
        "permanent",
        "left",
        "the",
        "battlefield",
        "this",
        "turn",
    ]) {
        return Ok(ConditionExpr::PermanentLeftThisTurn);
    }
    if ts.eat_phrase(&["that", "creature", "dies", "this", "way"])
        || ts.eat_phrase(&["that", "creature", "dies", "this", "turn"])
    {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::DestroyedThisWay,
        ));
    }
    if ts.eat_phrase(&["that", "spell", "is", "countered", "this", "way"]) {
        return Ok(ConditionExpr::PriorObjectEvent(
            PriorActionKind::ExiledThisWay,
        ));
    }
    if ts.eat_phrase(&["a", "creature", "died", "this", "turn"])
        || ts.eat_phrase(&[
            "a", "creature", "died", "under", "your", "control", "this", "turn",
        ])
        || ts.eat_phrase(&["a", "creature", "you", "controlled", "died", "this", "turn"])
    {
        return Ok(ConditionExpr::PermanentDiedThisTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_phrase(&["was", "kicked"]) {
            if ts.eat_phrase(&["with", "its"])
                && let Some(mc) = ts.eat_mana_cost()
                && ts.eat_word("kicker")
            {
                return Ok(ConditionExpr::KickedWith(mc));
            }
            let _ = ts.eat_word("twice");
            return Ok(ConditionExpr::WasKicked);
        }
        ts.restore(cp);
    }
    // `{M} was spent to cast it/~` (`{U} was spent to cast it`, `{W}{U} was spent to cast ~`)
    {
        let mscp = ts.checkpoint();
        if let Some(mc) = ts.eat_mana_cost()
            && ts.eat_phrase(&["was", "spent", "to", "cast"])
            && (ts.eat_word("it") || ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
        {
            return Ok(ConditionExpr::ManaCostSpent(mc));
        }
        ts.restore(mscp);
    }
    // `creatures you control have total power <N> or greater` (Formidable / Ferocious)
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["creatures", "you", "control", "have", "total", "power"])
            && let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
        {
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::AggregateStat {
                    op: crate::v2::ir::AggregateOp::Sum,
                    stat: crate::ast::StatKind::Power,
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::CREATURE,
                        controller: Some(crate::ast::ControllerRelation::You),
                        ..Default::default()
                    }),
                }),
                op: CompareOp::Ge,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(fcp);
    }
    // `<N> or more <flt> are in <zone>` (`eight or more cards are in your graveyard`)
    {
        let zcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "in"])
            && let Ok(zone) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(zcp);
    }
    // `<N> or more creatures are attacking` (`four or more creatures are attacking`)
    {
        let acp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["are", "attacking"])
        {
            flt.combat = Some(crate::ast::CombatState::Attacking);
            return Ok(ConditionExpr::Compare {
                lhs: Box::new(crate::v2::ir::ValueExpr::Count(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(acp);
    }
    // `<N> or more creatures died this turn` (`five or more creatures died this turn`)
    {
        let dcp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more"])
            && (ts.eat_word("creatures") || ts.eat_word("permanents"))
            && ts.eat_phrase(&["died", "this", "turn"])
        {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::EachPlayer,
                event: TurnEventKind::CreatureDied,
                filter: None,
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(dcp);
    }

    if ts.eat_phrase(&[
        "one",
        "or",
        "more",
        "cards",
        "left",
        "your",
        "graveyard",
        "this",
        "turn",
    ]) || ts.eat_phrase(&["a", "card", "left", "your", "graveyard", "this", "turn"])
    {
        return Ok(ConditionExpr::CardsLeftGraveyardThisTurn(PlayerRef::You));
    }
    if ts.eat_phrase(&[
        "a",
        "land",
        "entered",
        "the",
        "battlefield",
        "under",
        "your",
        "control",
        "this",
        "turn",
    ]) || ts.eat_phrase(&["a", "land", "you", "control", "entered", "this", "turn"])
    {
        return Ok(ConditionExpr::LandEnteredThisTurn(PlayerRef::You));
    }
    // `<N> or more mana was spent to cast <subj>` (`Tellah`)
    {
        let m2cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "mana", "was", "spent", "to", "cast"])
            && try_parse(ts, parse_subject_ref).is_ok()
        {
            return Ok(ConditionExpr::ManaSpentCompare {
                op: CompareOp::Ge,
                value: Box::new(crate::v2::ir::ValueExpr::Const(n)),
            });
        }
        ts.restore(m2cp);
    }
    // `<N> or more damage was dealt to <subj> this turn` (`Burning-Eye Zubera`)
    {
        let dmg_cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "more", "damage", "was", "dealt", "to"])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::DamageDealtToSubjectThisTurn {
                subject: Box::new(subject),
                min_damage: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        ts.restore(dmg_cp);
    }
    // `there is a/an <flt> in <zone>` (`Wretched Camel`)
    {
        let ticp = ts.checkpoint();
        if ts.eat_phrase(&["there", "is"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            let zone = if let Some(z) = flt.zones.pop() {
                Some(z)
            } else if ts.eat_word("in") {
                try_parse(ts, parse_zone_ref).ok()
            } else {
                None
            };
            if let Some(zone) = zone {
                return Ok(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count: 1,
                });
            }
        }
        ts.restore(ticp);
    }
    // `there are [at least] <N> [or more] card types among cards in <zone>` (Delirium)
    // OR `there are [at least] <N> [or more] <flt> in <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"]) {
            let _ = ts.eat_phrase(&["at", "least"]);
            let min_count = ts
                .next_number(false)
                .and_then(|n| u16::try_from(n).ok())
                .unwrap_or(1);
            let _ = ts.eat_phrase(&["or", "more"]);
            if ts.eat_phrase(&["card", "types", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::DistinctCardTypesInZone {
                    zone,
                    min_count: u8::try_from(min_count).unwrap_or(4),
                });
            }
            if ts.eat_phrase(&["mana", "values", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(crate::v2::ir::ValueExpr::DistinctManaValues(zone)),
                    op: CompareOp::Ge,
                    rhs: Box::new(crate::v2::ir::ValueExpr::Const(i32::from(min_count))),
                });
            }
            // `there are <N> or more <kind> counters on <subj>`
            {
                let ccp = ts.checkpoint();
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                            subject: Box::new(subject),
                            kind,
                        }),
                        op: CompareOp::Ge,
                        rhs: Box::new(crate::v2::ir::ValueExpr::Const(i32::from(min_count))),
                    });
                }
                ts.restore(ccp);
            }
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                let zone = if let Some(z) = flt.zones.pop() {
                    Some(z)
                } else if ts.eat_word("in") {
                    try_parse(ts, parse_zone_ref).ok()
                } else {
                    None
                };
                if let Some(zone) = zone {
                    return Ok(ConditionExpr::ZoneHasCount {
                        filter: Box::new(flt),
                        zone,
                        min_count,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `your life total is <N> or less/more`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["your", "life", "total", "is"])
            && let Ok(val) = try_parse(ts, parse_value_expr)
        {
            let op = if ts.eat_phrase(&["or", "less"]) {
                CompareOp::Le
            } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                CompareOp::Ge
            } else {
                CompareOp::Eq
            };
            return Ok(ConditionExpr::LifeCompare {
                player: PlayerRef::You,
                op,
                value: Box::new(val),
            });
        }
        ts.restore(cp);
    }

    // `its mana cost contains {R}`
    if ts.eat_phrase(&["its", "mana", "cost", "contains"]) {
        let sym = ts
            .next_mana()
            .ok_or_else(|| ts.err("expected mana symbol after `its mana cost contains`"))?;
        return Ok(ConditionExpr::ManaCostContains {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            symbol: sym,
        });
    }

    // Player-headed conditions (`you control ...`, `you've gained life this turn`, `an opponent has cast a ... spell this turn`)
    {
        let cp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, parse_player_ref) {
            if ts.eat_word("control") || ts.eat_word("controls") {
                if player == PlayerRef::You && ts.eat_tilde() {
                    return Ok(ConditionExpr::ControlsSelf);
                }
                if ts.eat_word("more")
                    && let Ok(filter) = try_parse(ts, parse_object_filter)
                    && ts.eat_word("than")
                    && let Ok(other) = try_parse(ts, parse_player_ref)
                {
                    return Ok(ConditionExpr::ControlsMoreThan {
                        player,
                        other,
                        filter: Box::new(filter),
                    });
                }
                if ts.eat_word("no") {
                    let filter = parse_object_filter(ts)?;
                    return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: 1,
                    })));
                }
                let _ = ts.eat_phrase(&["at", "least"]) || ts.eat_word("exactly");
                let (min_count, is_fewer) = if let Some(n) = ts.next_number(false) {
                    let fewer = ts.eat_phrase(&["or", "fewer"]);
                    if !fewer {
                        let _ = ts.eat_phrase(&["or", "more"]);
                    }
                    (u16::try_from(n.max(1)).unwrap_or(1), fewer)
                } else {
                    let _ = ts.eat_word("a") || ts.eat_word("an");
                    (1, false)
                };
                let filter = match try_parse(ts, parse_object_filter) {
                    Ok(f) => f,
                    Err(e) => {
                        if !is_fewer
                            && min_count == 1
                            && let Ok(subj) = try_parse(ts, parse_subject_ref)
                        {
                            return Ok(ConditionExpr::ControlsSubject {
                                player,
                                subject: Box::new(subj),
                            });
                        }
                        return Err(e);
                    }
                };
                let first_cond = if is_fewer {
                    ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count: min_count.saturating_add(1),
                    }))
                } else {
                    ConditionExpr::Controls {
                        player,
                        filter: Box::new(filter),
                        min_count,
                    }
                };
                // Support `you control <flt1> and <flt2>` (`Okiba Salvage`, `Of One Mind`, `Raka Sanctuary`, `When We Were Young`)
                let acp = ts.checkpoint();
                if ts.eat_word("and") {
                    let min2 = if let Some(n2) = ts.next_number(false) {
                        let _ = ts.eat_phrase(&["or", "more"]);
                        u16::try_from(n2.max(1)).unwrap_or(1)
                    } else {
                        1
                    };
                    if let Ok(flt2) = try_parse(ts, parse_object_filter) {
                        return Ok(ConditionExpr::And(
                            Box::new(first_cond),
                            Box::new(ConditionExpr::Controls {
                                player,
                                filter: Box::new(flt2),
                                min_count: min2,
                            }),
                        ));
                    }
                }
                ts.restore(acp);
                return Ok(first_cond);
            }
            if ts.eat_word("has") || ts.eat_word("have") {
                if ts.eat_phrase(&["more", "life", "than"])
                    && let Ok(other) = try_parse(ts, parse_player_ref)
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                        op: CompareOp::Gt,
                        rhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player: other,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                    });
                }
                if ts.eat_phrase(&["no", "cards", "in", "hand"])
                    || ts.eat_phrase(&["no", "cards", "in", "their", "hand"])
                    || ts.eat_phrase(&["no", "cards", "in", "your", "hand"])
                {
                    return Ok(ConditionExpr::Compare {
                        lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                            player,
                            stat: crate::ast::PlayerStatKind::CardsInHand,
                        }),
                        op: CompareOp::Eq,
                        rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
                    });
                }
                let _ = ts.eat_word("exactly");
                if let Some(n) = ts.next_number(false) {
                    let op = if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else if ts.eat_phrase(&["or", "more"]) || ts.eat_phrase(&["or", "greater"]) {
                        CompareOp::Ge
                    } else {
                        CompareOp::Eq
                    };
                    if ts.eat_phrase(&["cards", "in", "their", "graveyard"])
                        || ts.eat_phrase(&["cards", "in", "your", "graveyard"])
                        || ts.eat_phrase(&["card", "in", "their", "graveyard"])
                    {
                        return Ok(ConditionExpr::ZoneHasCount {
                            filter: Box::new(crate::v2::ir::ObjectFilter {
                                any_card: true,
                                ..Default::default()
                            }),
                            zone: crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Graveyard,
                                owner: if player == PlayerRef::You {
                                    crate::v2::ir::ZoneOwner::You
                                } else {
                                    crate::v2::ir::ZoneOwner::ThatPlayer
                                },
                            },
                            min_count: u16::try_from(n.max(1)).unwrap_or(1),
                        });
                    }
                    if ts.eat_phrase(&["cards", "in", "hand"])
                        || ts.eat_phrase(&["card", "in", "hand"])
                        || ts.eat_phrase(&["cards", "in", "their", "hand"])
                        || ts.eat_phrase(&["cards", "in", "your", "hand"])
                    {
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::PlayerStat {
                                player,
                                stat: crate::ast::PlayerStatKind::CardsInHand,
                            }),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    if ts.eat_word("opponents") || ts.eat_word("opponent") {
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::OpponentsCount),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    if ts.eat_word("life") {
                        return Ok(ConditionExpr::LifeCompare {
                            player,
                            op,
                            value: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                    {
                        return Ok(ConditionExpr::PlayerHasCounters {
                            player,
                            kind,
                            min_count: u16::try_from(n.max(1)).unwrap_or(1),
                        });
                    }
                }
            }
            if (ts.eat_word("own") || ts.eat_word("owns"))
                && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            {
                let zone = flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                    zone: crate::v2::ir::ZoneKind::Exile,
                    owner: crate::v2::ir::ZoneOwner::Default,
                });
                return Ok(ConditionExpr::ZoneHasCount {
                    filter: Box::new(flt),
                    zone,
                    min_count: 1,
                });
            }
            if ts.eat_phrase(&["don't", "control"]) || ts.eat_phrase(&["doesn't", "control"]) {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                let filter = parse_object_filter(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                    player,
                    filter: Box::new(filter),
                    min_count: 1,
                })));
            }
            if ts.eat_phrase(&["has", "cast"]) || ts.eat_word("cast") {
                let min_count = if let Some(n) = ts.next_number(false) {
                    let _ = ts.eat_phrase(&["or", "more"]);
                    u16::try_from(n.max(1)).unwrap_or(1)
                } else {
                    1
                };
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count,
                    });
                }
            }
            if ts.eat_phrase(&["attacked", "with", "creatures", "with", "total", "power"])
                && let Some(n) = ts.next_number(false)
                && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]))
            {
                let _ = ts.eat_phrase(&["this", "combat"]) || ts.eat_phrase(&["this", "turn"]);
                return Ok(ConditionExpr::AttackedWithTotalPower {
                    player,
                    min_power: n,
                });
            }
            if ts.eat_phrase(&["gained", "or", "lost", "life", "this", "turn"]) {
                return Ok(ConditionExpr::Or(
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeGained,
                        filter: None,
                        min_count: 1,
                    }),
                    Box::new(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeLost,
                        filter: None,
                        min_count: 1,
                    }),
                ));
            }
            if ts.eat_phrase(&["lost", "life", "last", "turn"]) {
                return Ok(ConditionExpr::LostLifeLastTurn(player));
            }
            if ts.eat_phrase(&["didn't", "cast", "a", "spell", "this", "turn"]) {
                return Ok(ConditionExpr::Not(Box::new(
                    ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::SpellCast,
                        filter: None,
                        min_count: 1,
                    },
                )));
            }
            if ts.eat_phrase(&["didn't", "cast", "it", "from", "your", "hand"])
                || ts.eat_phrase(&["didn't", "cast", "it", "from", "their", "hand"])
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::WasCastFromHand)));
            }
            if ts.eat_word("controlled")
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ConditionExpr::SubjectMatches {
                    subject: Box::new(subj),
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        controller: Some(crate::ast::ControllerRelation::You),
                        ..Default::default()
                    }),
                });
            }
            if ts.eat_phrase(&["attacked", "with"])
                && let Some(n) = ts.next_number(false)
            {
                let _ = ts.eat_phrase(&["or", "more"]);
                if (ts.eat_word("creatures") || ts.eat_word("creature"))
                    && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(ConditionExpr::AttackedWithCount {
                        player,
                        min_count: u16::try_from(n.max(1)).unwrap_or(1),
                    });
                }
            }
            if ts.eat_phrase(&["was", "dealt"]) || ts.eat_phrase(&["were", "dealt"]) {
                let combat_only = ts.eat_word("combat");
                let _ = ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    if ts.eat_word("by") {
                        let _ = opt(ts, parse_subject_ref);
                    }
                    if ts.eat_phrase(&["this", "turn"]) {
                        return Ok(ConditionExpr::PlayerDealtDamageThisTurn {
                            player,
                            combat_only,
                        });
                    }
                }
            }
            if ts.eat_word("revealed")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["this", "way"])
            {
                return Ok(ConditionExpr::RevealedThisWay(Box::new(flt)));
            }
            if ts.eat_word("had")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
                && (ts.eat_phrase(&[
                    "enter",
                    "the",
                    "battlefield",
                    "under",
                    "your",
                    "control",
                    "this",
                    "turn",
                ]) || ts.eat_phrase(&["enter", "under", "your", "control", "this", "turn"])
                    || ts.eat_phrase(&["enter", "the", "battlefield", "this", "turn"]))
            {
                return Ok(ConditionExpr::PermanentEnteredThisTurn {
                    filter: Box::new(flt),
                    controller: player,
                    min_count: 1,
                });
            }
            if ts.eat_word("gained") || ts.eat_phrase(&["has", "gained"]) {
                let min_count = ts
                    .next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_phrase(&["life", "this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeGained,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_word("discarded") || ts.eat_phrase(&["has", "discarded"]) {
                let min_count = if ts.eat_word("a") {
                    1
                } else {
                    ts.next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1)
                };
                let _ = ts.eat_phrase(&["or", "more"]);
                if (ts.eat_word("card") || ts.eat_word("cards")) && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::CardDiscarded,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_word("lost") || ts.eat_phrase(&["has", "lost"]) {
                let min_count = ts
                    .next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(1);
                let _ = ts.eat_phrase(&["or", "more"]);
                if ts.eat_phrase(&["life", "this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player,
                        event: TurnEventKind::LifeLost,
                        filter: None,
                        min_count,
                    });
                }
            }
            if ts.eat_phrase(&["you've", "cast"]) {
                let filter = opt(ts, parse_object_filter).map(Box::new);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::PlayerTurnEvent {
                        player: PlayerRef::You,
                        event: TurnEventKind::SpellCast,
                        filter,
                        min_count: 1,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `you've gained [N or more] life this turn`
    if ts.eat_phrase(&["you've", "gained"]) {
        let min_count = ts
            .next_number(false)
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(1);
        let _ = ts.eat_phrase(&["or", "more"]);
        if ts.eat_phrase(&["life", "this", "turn"]) {
            return Ok(ConditionExpr::PlayerTurnEvent {
                player: PlayerRef::You,
                event: TurnEventKind::LifeGained,
                filter: None,
                min_count,
            });
        }
    }

    // `you've drawn <N> or more cards this turn`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "drawn"])
            && let Some(n) = ts.next_number(false)
        {
            let _ = ts.eat_phrase(&["or", "more"]);
            if (ts.eat_word("cards") || ts.eat_word("card")) && ts.eat_phrase(&["this", "turn"]) {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::CardDrawn,
                    filter: None,
                    min_count: u16::try_from(n.max(1)).unwrap_or(1),
                });
            }
        }
        ts.restore(dcp);
    }

    // `<subj> is in the command zone or on the battlefield` (`Inalla`, `Edgar Markov`, `The Ur-Dragon`, Eminence cycle)
    {
        let ecp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&[
                "is",
                "in",
                "the",
                "command",
                "zone",
                "or",
                "on",
                "the",
                "battlefield",
            ])
        {
            return Ok(ConditionExpr::InCommandZoneOrBattlefield(Box::new(subj)));
        }
        ts.restore(ecp);
    }
    // `it isn't a mana ability` (`Harsh Mentor` + 7+ cards)
    if ts.eat_phrase(&["it", "isn't", "a", "mana", "ability"]) {
        return Ok(ConditionExpr::IsNotManaAbility);
    }
    // `there are no <flt> [on the battlefield | in <zone>]` (`Spirit Mirror` + 10+ cards)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are", "no"])
            && let Ok(mut flt) = try_parse(ts, parse_object_filter)
        {
            let zone = flt.zones.pop().unwrap_or(crate::v2::ir::ZoneRef {
                zone: crate::v2::ir::ZoneKind::Battlefield,
                owner: crate::v2::ir::ZoneOwner::Default,
            });
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::ZoneHasCount {
                filter: Box::new(flt),
                zone,
                min_count: 1,
            })));
        }
        ts.restore(ncp);
    }
    // `two or more <flt> are tied for greatest/least <stat>` (`Desecrator Hag`, `Drop of Honey`, `Purging Scythe`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["two", "or", "more"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["are", "tied", "for"])
        {
            let _ = ts.eat_word("the");
            let greatest = if ts.eat_word("greatest") || ts.eat_word("highest") {
                Some(true)
            } else if ts.eat_word("least") || ts.eat_word("lowest") {
                Some(false)
            } else {
                None
            };
            let stat = if ts.eat_word("power") {
                Some(crate::ast::StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(crate::ast::StatKind::Toughness)
            } else if ts.eat_phrase(&["mana", "value"]) {
                Some(crate::ast::StatKind::ManaValue)
            } else {
                None
            };
            if let (Some(greatest), Some(stat)) = (greatest, stat) {
                return Ok(ConditionExpr::TiedForExtremeStat { stat, greatest });
            }
        }
        ts.restore(tcp);
    }
    // `<flt> is/are put into a graveyard this way` (`Come Back Wrong` + 8+ cards)
    {
        let gcp = ts.checkpoint();
        if let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&["is", "put", "into", "a", "graveyard", "this", "way"])
                || ts.eat_phrase(&["are", "put", "into", "a", "graveyard", "this", "way"])
                || ts.eat_phrase(&["is", "put", "into", "your", "graveyard", "this", "way"]))
        {
            return Ok(ConditionExpr::PriorActionMatched {
                action: PriorActionKind::MilledThisWay,
                filter: Box::new(flt),
            });
        }
        ts.restore(gcp);
    }
    // `no opponent controls <flt>` (`Kavu Runner`, `Vexing Beetle`)
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["no", "opponent", "controls"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ConditionExpr::Not(Box::new(ConditionExpr::Controls {
                player: PlayerRef::AnyOpponent,
                filter: Box::new(flt),
                min_count: 1,
            })));
        }
        ts.restore(ncp);
    }
    // `a library has <N> or fewer cards in it` (`Isleback Spawn`)
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["a", "library", "has"])
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["or", "fewer", "cards", "in", "it"])
        {
            return Ok(ConditionExpr::LibraryCardsCompare {
                op: CompareOp::Le,
                count: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
        ts.restore(lcp);
    }
    // `<subj> had counters on it` / `<subj> has [a|an|<kind>] counters on it` / `<subj> doesn't have a <kind> counter on it`
    // / `<subj> has the chosen name` / `<subj> dealt [combat] damage [to <tgt>] this turn` / `<subj> hasn't/haven't dealt [combat] damage yet`
    // / `<subj> attacked [or blocked] this combat` / `<subj> didn't enter the battlefield this turn` / `<subj> isn't prepared` / `<subj> was[n't] cast`
    {
        let scp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_phrase(&["had", "counters", "on", "it"])
                || ts.eat_phrase(&["had", "counters", "on", "them"])
            {
                return Ok(ConditionExpr::HadCountersOnIt(Box::new(subj)));
            }
            if ts.eat_phrase(&["has", "counters", "on", "it"])
                || ts.eat_phrase(&["have", "counters", "on", "them"])
            {
                return Ok(ConditionExpr::SubjectHasCounters {
                    subject: Box::new(subj),
                    kind: None,
                });
            }
            {
                let hcp = ts.checkpoint();
                if (ts.eat_word("has") || ts.eat_word("have"))
                    && {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        true
                    }
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
                {
                    return Ok(ConditionExpr::SubjectHasCounters {
                        subject: Box::new(subj),
                        kind: Some(ck),
                    });
                }
                ts.restore(hcp);
            }
            {
                let ncp = ts.checkpoint();
                if (ts.eat_phrase(&["doesn't", "have"]) || ts.eat_phrase(&["don't", "have"]))
                    && {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        true
                    }
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
                {
                    return Ok(ConditionExpr::Not(Box::new(
                        ConditionExpr::SubjectHasCounters {
                            subject: Box::new(subj),
                            kind: Some(ck),
                        },
                    )));
                }
                ts.restore(ncp);
            }
            if ts.eat_phrase(&["has", "the", "chosen", "name"]) {
                return Ok(ConditionExpr::HasChosenName(Box::new(subj)));
            }
            if ts.eat_phrase(&["was", "cast"])
                && !ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("from")
                        || w.eq_ignore_ascii_case("for")
                        || w.eq_ignore_ascii_case("using")
                })
            {
                return Ok(ConditionExpr::SubjectWasCast(Box::new(subj)));
            }
            if ts.eat_phrase(&["wasn't", "cast"]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectWasCast(
                    Box::new(subj),
                ))));
            }
            if ts.eat_phrase(&["isn't", "prepared"]) {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Modified,
                })));
            }
            if ts.eat_phrase(&["didn't", "enter", "the", "battlefield", "this", "turn"])
                || ts.eat_phrase(&["didn't", "enter", "this", "turn"])
            {
                return Ok(ConditionExpr::Not(Box::new(
                    ConditionExpr::EnteredThisTurn(Box::new(subj)),
                )));
            }
            if ts.eat_phrase(&["attacked", "or", "blocked", "this", "combat"]) {
                return Ok(ConditionExpr::SubjectAttackedThisCombat {
                    subject: Box::new(subj),
                    or_blocked: true,
                });
            }
            if ts.eat_phrase(&["attacked", "this", "combat"]) {
                return Ok(ConditionExpr::SubjectAttackedThisCombat {
                    subject: Box::new(subj),
                    or_blocked: false,
                });
            }
            if ts.eat_phrase(&["hasn't", "dealt", "combat", "damage", "yet"])
                || ts.eat_phrase(&["haven't", "dealt", "combat", "damage", "yet"])
            {
                return Ok(ConditionExpr::SubjectHasntDealtDamageYet {
                    subject: Box::new(subj),
                    combat_only: true,
                });
            }
            if ts.eat_phrase(&["hasn't", "dealt", "damage", "yet"])
                || ts.eat_phrase(&["haven't", "dealt", "damage", "yet"])
            {
                return Ok(ConditionExpr::SubjectHasntDealtDamageYet {
                    subject: Box::new(subj),
                    combat_only: false,
                });
            }
            if ts.eat_word("dealt") {
                let combat_only = ts.eat_word("combat");
                if ts.eat_word("damage") {
                    if ts.eat_word("to") {
                        let _ = opt(ts, parse_subject_ref);
                    }
                    if ts.eat_phrase(&["this", "turn"]) {
                        return Ok(ConditionExpr::SubjectDealtDamageThisTurn {
                            subject: Box::new(subj),
                            combat_only,
                        });
                    }
                }
            }
        }
        ts.restore(scp);
    }

    // `you've committed a crime this turn` / `you've discarded a card this turn` / `there are N colors among <flt>` / `there are N or more permanent types among cards in <zone>` / `~ or another <flt> entered ...`
    if ts.eat_phrase(&["you've", "committed", "a", "crime", "this", "turn"]) {
        return Ok(ConditionExpr::CommittedCrimeThisTurn(PlayerRef::You));
    }
    if ts.eat_phrase(&["you've", "discarded", "a", "card", "this", "turn"])
        || ts.eat_phrase(&[
            "you've",
            "discarded",
            "one",
            "or",
            "more",
            "cards",
            "this",
            "turn",
        ])
    {
        return Ok(ConditionExpr::PlayerTurnEvent {
            player: PlayerRef::You,
            event: TurnEventKind::CardDiscarded,
            filter: None,
            min_count: 1,
        });
    }
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["there", "are"])
            && let Some(n) = ts.next_number(false)
        {
            let _ = ts.eat_phrase(&["or", "more"]);
            if ts.eat_phrase(&["colors", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::ColorsAmongCount {
                    filter: Box::new(flt),
                    min_count: u8::try_from(n.max(1)).unwrap_or(5),
                });
            }
            if ts.eat_phrase(&["permanent", "types", "among", "cards", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::DistinctPermanentTypesInZone {
                    zone,
                    min_count: u8::try_from(n.max(1)).unwrap_or(4),
                });
            }
        }
        ts.restore(tcp);
    }
    {
        let ecp = ts.checkpoint();
        if ts.eat_tilde()
            && ts.eat_word("or")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&[
                "entered",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ]) || ts.eat_phrase(&["entered", "under", "your", "control", "this", "turn"])
                || ts.eat_phrase(&["entered", "the", "battlefield", "this", "turn"]))
        {
            return Ok(ConditionExpr::PermanentEnteredThisTurn {
                filter: Box::new(flt),
                controller: PlayerRef::You,
                min_count: 1,
            });
        }
        ts.restore(ecp);
    }
    if ts.eat_phrase(&["a", "creature", "is", "attacking", "you"]) {
        return Ok(ConditionExpr::CreatureAttackingYou);
    }
    {
        let lcp = ts.checkpoint();
        if (ts.eat_word("a") || ts.eat_word("an"))
            && try_parse(ts, parse_object_filter).is_ok()
            && (ts.eat_phrase(&[
                "left",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ]) || ts.eat_phrase(&["left", "the", "battlefield", "this", "turn"]))
        {
            return Ok(ConditionExpr::PermanentLeftThisTurn);
        }
        ts.restore(lcp);
    }
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "sacrificed"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["this", "turn"])
        {
            return Ok(ConditionExpr::SacrificedPermanentThisTurn {
                player: PlayerRef::You,
                filter: Box::new(flt),
            });
        }
        ts.restore(scp);
    }

    // `you've cast [<N> or more] <flt> this turn`
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["you've", "cast"]) {
            let min_count = if let Some(n) = ts.next_number(false) {
                let _ = ts.eat_phrase(&["or", "more"]);
                u16::try_from(n.max(1)).unwrap_or(1)
            } else {
                1
            };
            let filter = opt(ts, parse_object_filter).map(Box::new);
            if ts.eat_phrase(&["this", "turn"]) {
                return Ok(ConditionExpr::PlayerTurnEvent {
                    player: PlayerRef::You,
                    event: TurnEventKind::SpellCast,
                    filter,
                    min_count,
                });
            }
        }
        ts.restore(ccp);
    }

    // General numeric value comparison (`<val1> is greater than / less than / less than or equal to / equal to <val2>`)
    {
        let cp = ts.checkpoint();
        if let Ok(lhs) = try_parse(ts, parse_value_expr)
            && ts.eat_word("is")
        {
            let op = if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
                Some(CompareOp::Le)
            } else if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
                Some(CompareOp::Ge)
            } else if ts.eat_phrase(&["less", "than"]) {
                Some(CompareOp::Lt)
            } else if ts.eat_phrase(&["greater", "than"]) {
                Some(CompareOp::Gt)
            } else if ts.eat_phrase(&["equal", "to"]) {
                Some(CompareOp::Eq)
            } else {
                None
            };
            if let Some(op) = op
                && let Ok(rhs) = try_parse(ts, parse_value_expr)
            {
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(lhs),
                    op,
                    rhs: Box::new(rhs),
                });
            }
            // Suffix form: `<val1> is <val2> or greater / or less` (e.g. `that spell's mana value is 5 or greater`)
            if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                let suffix_op =
                    if ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else if ts.eat_phrase(&["or", "less"]) {
                        CompareOp::Le
                    } else {
                        CompareOp::Eq
                    };
                return Ok(ConditionExpr::Compare {
                    lhs: Box::new(lhs),
                    op: suffix_op,
                    rhs: Box::new(rhs),
                });
            }
        }
        ts.restore(cp);
    }

    // Subject-headed conditions (`~ is in your graveyard`, `~ is untapped`, `<subj> has/doesn't have <kw>`, `it's a <filter>`)
    {
        let cp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            if ts.eat_word("targets")
                && let Ok(filter) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(filter),
                });
            }
            if ts.eat_phrase(&["doesn't", "target"])
                && let Ok(filter) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::TargetsFilter {
                    subject: Box::new(subj),
                    filter: Box::new(filter),
                })));
            }
            if ts.eat_phrase(&["attacked", "or", "blocked", "this", "turn"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::AttackedOrBlockedThisTurn,
                });
            }
            if ts.eat_phrase(&["is", "paired", "with", "another", "creature"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Paired,
                });
            }
            if ts.eat_phrase(&["entered", "the", "battlefield"]) || ts.eat_word("entered") {
                let _ = ts.eat_phrase(&["under", "your", "control"])
                    || ts.eat_phrase(&["under", "their", "control"])
                    || ts.eat_phrase(&["under", "an", "opponent's", "control"]);
                if ts.eat_phrase(&["this", "turn"]) {
                    return Ok(ConditionExpr::EnteredThisTurn(Box::new(subj)));
                }
            }
            if ts.eat_phrase(&["is", "on", "the", "battlefield"])
                || ts.eat_phrase(&["are", "on", "the", "battlefield"])
                || ts.eat_phrase(&["remains", "on", "the", "battlefield"])
            {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone: crate::v2::ir::ZoneRef {
                        zone: crate::v2::ir::ZoneKind::Battlefield,
                        owner: crate::v2::ir::ZoneOwner::Default,
                    },
                });
            }
            if ts.eat_phrase(&["remains", "attached", "to"])
                || ts.eat_phrase(&["is", "attached", "to"])
            {
                if let Ok(target) = try_parse(ts, parse_object_filter) {
                    return Ok(ConditionExpr::SubjectAttachedTo {
                        subject: Box::new(subj),
                        target: Box::new(target),
                    });
                }
                if ts.eat_word("it") {
                    return Ok(ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state: ObjectStateKind::Enchanted,
                    });
                }
            }
            if ts.eat_phrase(&["remains", "exiled"]) || ts.eat_phrase(&["remain", "exiled"]) {
                return Ok(ConditionExpr::RemainsExiled(Box::new(subj)));
            }
            if ts.eat_phrase(&["remains", "tapped"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Tapped,
                });
            }
            if ts.eat_phrase(&["remains", "untapped"]) {
                return Ok(ConditionExpr::SubjectState {
                    subject: Box::new(subj),
                    state: ObjectStateKind::Untapped,
                });
            }
            if ts.eat_phrase(&["is", "in"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::InZone {
                    subject: Box::new(subj),
                    zone,
                });
            }
            if ts.eat_phrase(&["doesn't", "have"]) || ts.eat_phrase(&["don't", "have"]) {
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::Not(Box::new(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                })));
            }
            if ts.eat_word("has") || ts.eat_word("have") || ts.eat_word("had") {
                // `<subj> has the same mana value / power / toughness / color / name / card type as <other>` (`Counterbalance`)
                {
                    let smcp = ts.checkpoint();
                    if ts.eat_phrase(&["the", "same"]) {
                        let prop = if ts.eat_phrase(&["mana", "value"]) {
                            Some(crate::v2::ir::SharedProperty::ManaValue)
                        } else if ts.eat_word("power") {
                            Some(crate::v2::ir::SharedProperty::Power)
                        } else if ts.eat_word("toughness") {
                            Some(crate::v2::ir::SharedProperty::Toughness)
                        } else if ts.eat_word("color") {
                            Some(crate::v2::ir::SharedProperty::Color)
                        } else if ts.eat_word("name") {
                            Some(crate::v2::ir::SharedProperty::Name)
                        } else if ts.eat_phrase(&["card", "type"]) {
                            Some(crate::v2::ir::SharedProperty::CardType)
                        } else {
                            None
                        };
                        if let Some(kind) = prop
                            && ts.eat_word("as")
                            && let Ok(other) = try_parse(ts, parse_subject_ref)
                        {
                            return Ok(ConditionExpr::SharesTypeWith {
                                subject: Box::new(subj),
                                other: Box::new(other),
                                kind,
                            });
                        }
                    }
                    ts.restore(smcp);
                }
                // `<subj> had/has no [<kind>] counters on it`
                {
                    let ncp = ts.checkpoint();
                    if ts.eat_word("no") {
                        let kind = if ts.eat_word("counters") || ts.eat_word("counter") {
                            Some(crate::v2::ir::CounterKind::Any)
                        } else if let Ok(ck) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counters") || ts.eat_word("counter"))
                        {
                            Some(ck)
                        } else {
                            None
                        };
                        if let Some(kind) = kind {
                            let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                                    subject: Box::new(subj),
                                    kind,
                                }),
                                op: CompareOp::Eq,
                                rhs: Box::new(crate::v2::ir::ValueExpr::Const(0)),
                            });
                        }
                    }
                    ts.restore(ncp);
                }
                // `<subj> has mana value/power/toughness <cmp> <val>` (`that spell has mana value 5 or greater`)
                {
                    let scp = ts.checkpoint();
                    let stat = if ts.eat_phrase(&["mana", "value"]) {
                        Some(crate::ast::StatKind::ManaValue)
                    } else if ts.eat_word("power") {
                        Some(crate::ast::StatKind::Power)
                    } else if ts.eat_word("toughness") {
                        Some(crate::ast::StatKind::Toughness)
                    } else {
                        None
                    };
                    if let Some(stat) = stat {
                        let op_opt = if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
                            Some(CompareOp::Le)
                        } else if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
                            Some(CompareOp::Ge)
                        } else if ts.eat_phrase(&["less", "than"]) {
                            Some(CompareOp::Lt)
                        } else if ts.eat_phrase(&["greater", "than"]) {
                            Some(CompareOp::Gt)
                        } else {
                            None
                        };
                        if let Some(op) = op_opt
                            && let Ok(rhs) = try_parse(ts, parse_value_expr)
                        {
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::ObjectStat {
                                    subject: Box::new(subj),
                                    stat,
                                }),
                                op,
                                rhs: Box::new(rhs),
                            });
                        }
                        if let Ok(rhs) = try_parse(ts, parse_value_expr) {
                            let op = if ts.eat_phrase(&["or", "greater"])
                                || ts.eat_phrase(&["or", "more"])
                            {
                                CompareOp::Ge
                            } else if ts.eat_phrase(&["or", "less"]) {
                                CompareOp::Le
                            } else {
                                CompareOp::Eq
                            };
                            return Ok(ConditionExpr::Compare {
                                lhs: Box::new(crate::v2::ir::ValueExpr::ObjectStat {
                                    subject: Box::new(subj),
                                    stat,
                                }),
                                op,
                                rhs: Box::new(rhs),
                            });
                        }
                    }
                    ts.restore(scp);
                }
                // `<subj> has <a/an/N> [or more] <kind> counter(s) on it`
                let hcp = ts.checkpoint();
                if let Some(n) = ts.next_number(true) {
                    let op = if ts.eat_phrase(&["or", "more"]) {
                        CompareOp::Ge
                    } else if ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "fewer"]) {
                        CompareOp::Le
                    } else {
                        CompareOp::Eq
                    };
                    if let Ok(kind) = try_parse(ts, parse_counter_kind)
                        && (ts.eat_word("counters") || ts.eat_word("counter"))
                    {
                        let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                        return Ok(ConditionExpr::Compare {
                            lhs: Box::new(crate::v2::ir::ValueExpr::CounterCount {
                                subject: Box::new(subj),
                                kind,
                            }),
                            op,
                            rhs: Box::new(crate::v2::ir::ValueExpr::Const(n)),
                        });
                    }
                }
                ts.restore(hcp);
                let kw = parse_keyword_kind(ts)?;
                return Ok(ConditionExpr::HasKeyword {
                    subject: Box::new(subj),
                    keyword: kw,
                });
            }
            if ts.eat_phrase(&["was", "cast", "from"])
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(ConditionExpr::CastFromZone {
                    subject: Box::new(subj),
                    zone,
                });
            }
            let negated = ts.eat_word("isn't")
                || ts.eat_phrase(&["is", "not"])
                || ts.eat_word("aren't")
                || ts.eat_phrase(&["are", "not"])
                || ts.eat_word("wasn't")
                || ts.eat_phrase(&["was", "not"])
                || ts.eat_word("weren't")
                || ts.eat_phrase(&["were", "not"]);
            let positive = !negated
                && (ts.eat_word("is")
                    || ts.eat_word("are")
                    || ts.eat_word("was")
                    || ts.eat_word("were"));
            if negated || positive {
                if let Some(state) = parse_object_state(ts) {
                    let base = ConditionExpr::SubjectState {
                        subject: Box::new(subj),
                        state,
                    };
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    let base = ConditionExpr::SubjectMatches {
                        subject: Box::new(subj),
                        filter: Box::new(flt),
                    };
                    return Ok(if negated {
                        ConditionExpr::Not(Box::new(base))
                    } else {
                        base
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `it's blocking or blocked by <flt>` / `it's a <filter>` / `it's tapped` / `it's untapped`
    if ts.eat_word("it's") {
        let negated = ts.eat_word("not");
        {
            let bcp = ts.checkpoint();
            if !negated
                && (ts.eat_phrase(&["blocking", "or", "blocked", "by"])
                    || ts.eat_phrase(&["blocked", "by"])
                    || ts.eat_word("blocking"))
                && let Ok(other) = try_parse(ts, parse_object_filter)
            {
                return Ok(ConditionExpr::SubjectCombatWith {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    other: Box::new(other),
                });
            }
            ts.restore(bcp);
        }
        if let Some(state) = parse_object_state(ts) {
            let base = ConditionExpr::SubjectState {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                state,
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
        if let Ok(flt) = try_parse(ts, parse_object_filter) {
            let base = ConditionExpr::SubjectMatches {
                subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                filter: Box::new(flt),
            };
            return Ok(if negated {
                ConditionExpr::Not(Box::new(base))
            } else {
                base
            });
        }
    }

    Err(ts.err("expected boolean condition expression"))
}

fn parse_object_state(ts: &mut TokenStream<'_>) -> Option<ObjectStateKind> {
    if ts.eat_word("tapped") {
        Some(ObjectStateKind::Tapped)
    } else if ts.eat_word("untapped") {
        Some(ObjectStateKind::Untapped)
    } else if ts.eat_word("attacking") {
        Some(ObjectStateKind::Attacking)
    } else if ts.eat_word("blocking") {
        Some(ObjectStateKind::Blocking)
    } else if ts.eat_word("blocked") {
        Some(ObjectStateKind::Blocked)
    } else if ts.eat_word("unblocked") {
        Some(ObjectStateKind::Unblocked)
    } else if ts.eat_word("equipped") {
        Some(ObjectStateKind::Equipped)
    } else if ts.eat_word("enchanted") {
        Some(ObjectStateKind::Enchanted)
    } else if ts.eat_word("modified") {
        Some(ObjectStateKind::Modified)
    } else if ts.eat_word("saddled") {
        Some(ObjectStateKind::Saddled)
    } else if ts.eat_word("monstrous") {
        Some(ObjectStateKind::Monstrous)
    } else if ts.eat_phrase(&["attacking", "alone"]) {
        Some(ObjectStateKind::AttackingAlone)
    } else if ts.eat_phrase(&["paired", "with", "a", "creature"])
        || ts.eat_phrase(&["paired", "with", "another", "creature"])
    {
        Some(ObjectStateKind::Paired)
    } else if ts.eat_phrase(&["face", "down"]) || ts.eat_word("face-down") {
        Some(ObjectStateKind::FaceDown)
    } else {
        None
    }
}
