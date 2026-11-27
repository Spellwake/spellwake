//! Global, combat-rule, trigger-doubler, and extended keyword static abilities for `v2`.

use crate::ast::{CardTypeMask, CompareOp, StatKind};
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{parse_keyword_list, parse_object_filter};
use crate::v2::ir::{
    AnaphoraRef, CastPermissionMode, ConditionExpr, Cost, FilterPredicate, KeywordKind,
    KeywordParam, KeywordSpec, ObjectFilter, PlayerRef, RuleRestrictionKind, StatBound,
    StaticAbility, SubjectRef, TriggerModifierKind, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, try_parse};

fn grant_self_keywords(
    keywords: Vec<KeywordSpec>,
    condition: Option<ConditionExpr>,
) -> StaticAbility {
    StaticAbility::GrantKeywords {
        subject: SubjectRef::SelfObject,
        keywords,
        condition,
    }
}

/// Parse global static abilities and extended standalone keyword lines that do not follow
/// the standard `<subject> <predicate>` shape. Fails closed whenever exact IR semantics
/// cannot be preserved.
pub(super) fn parse_global_static_ability(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();

    // 1. `Rather than pay the mana cost for a spell, its controller may <cost>`
    if ts.eat_phrase(&[
        "rather", "than", "pay", "the", "mana", "cost", "for", "a", "spell",
    ]) && ts.eat_comma()
        && ts.eat_phrase(&["its", "controller", "may"])
        && let Ok(costs) = try_parse(ts, parse_cost_list)
        && !costs.is_empty()
    {
        return Ok(StaticAbility::ZoneCastPermission {
            player: PlayerRef::EachPlayer,
            filter: Some(ObjectFilter {
                any_spell: true,
                ..Default::default()
            }),
            subject: None,
            zone: None,
            mode: CastPermissionMode::AlternativeCost(costs),
            condition,
        });
    }
    ts.restore(cp);

    // 2. `Spells with the chosen names can't be cast`
    if ts.eat_phrase(&[
        "spells", "with", "the", "chosen", "names", "can't", "be", "cast",
    ]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(SubjectRef::All(Box::new(ObjectFilter {
                any_spell: true,
                predicates: vec![FilterPredicate::OfChosenName],
                ..Default::default()
            }))),
            rule: RuleRestrictionKind::CantBeCast,
            condition,
        });
    }
    ts.restore(cp);

    // 3. `Activated abilities of the chosen permanent can't be activated`
    if ts.eat_phrase(&[
        "activated",
        "abilities",
        "of",
        "the",
        "chosen",
        "permanent",
        "can't",
        "be",
        "activated",
    ]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(SubjectRef::Anaphora(AnaphoraRef::ThatPermanent)),
            rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
            condition,
        });
    }
    ts.restore(cp);

    // 4. `Fortified land has <kws>`
    if ts.eat_phrase(&["fortified", "land", "has"])
        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
    {
        return Ok(StaticAbility::GrantKeywords {
            subject: SubjectRef::Anaphora(AnaphoraRef::EnchantedPermanent),
            keywords,
            condition,
        });
    }
    ts.restore(cp);

    // 5. `If <flt> being dealt damage causes a triggered ability of <perm_flt> to trigger, that ability triggers an additional time`
    if ts.eat_word("if")
        && let Ok(event_filter) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&[
            "being",
            "dealt",
            "damage",
            "causes",
            "a",
            "triggered",
            "ability",
            "of",
        ])
        && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["to", "trigger"])
        && ts.eat_comma()
        && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
    {
        return Ok(StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::AdditionalTriggers,
            filter: event_filter,
            permanent_filter: Some(permanent_filter),
            extra_count: 1,
        });
    }
    ts.restore(cp);

    // 6. Extended standalone keyword lines where exact parameters are preserved in `KeywordParam`
    if let Some(st) = try_parse_extended_keyword_line(ts, condition) {
        return Ok(st);
    }
    ts.restore(cp);

    Err(ts.err("not a global static ability"))
}

pub(super) fn parse_trigger_doubler(ts: &mut TokenStream<'_>) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["if", "a", "triggered", "ability", "of"]) {
        let source_filter_opt = if ts.eat_tilde() {
            Some(ObjectFilter::default())
        } else {
            try_parse(ts, parse_object_filter).ok()
        };
        if let Some(mut source_filter) = source_filter_opt {
            if ts.eat_word("or") {
                let _ = ts.eat_any_word(&["another", "an", "a"]);
                if let Ok(other) = try_parse(ts, parse_object_filter) {
                    source_filter = ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(vec![source_filter, other])],
                        ..Default::default()
                    };
                }
            }
            if ts.eat_word("triggers")
                && ts.eat_comma()
                && ts.eat_any_phrase(&[
                    &["that", "ability", "triggers", "an", "additional", "time"],
                    &["it", "triggers", "an", "additional", "time"],
                ])
            {
                return Ok(StaticAbility::TriggerModifier {
                    kind: TriggerModifierKind::AdditionalOnAbility,
                    filter: source_filter,
                    permanent_filter: None,
                    extra_count: 1,
                });
            }
        }
    }
    ts.restore(cp);
    if ts.eat_word("if")
        && let Ok(mut event_filter) = try_parse(ts, parse_object_filter)
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("or")
            && let Ok(f2) = try_parse(ts, parse_object_filter)
        {
            event_filter = ObjectFilter {
                predicates: vec![FilterPredicate::AnyOf(vec![event_filter, f2])],
                ..Default::default()
            };
        } else {
            ts.restore(ocp);
        }
        let matched_event = (ts.eat_any_phrase(&[
            &["entering", "or", "leaving", "the", "battlefield"],
            &["entering", "the", "battlefield"],
            &["dealing", "combat", "damage", "to", "a", "player"],
        ]) || ts.eat_any_word(&["entering", "dying", "attacking"]))
            && ts.eat_phrase(&["causes", "a", "triggered", "ability", "of"]);
        if matched_event
            && let Ok(permanent_filter) = if ts.eat_tilde() {
                Ok(ObjectFilter::default())
            } else {
                try_parse(ts, parse_object_filter)
            }
            && ts.eat_phrase(&["to", "trigger"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
        {
            return Ok(StaticAbility::TriggerModifier {
                kind: TriggerModifierKind::AdditionalTriggers,
                filter: event_filter,
                permanent_filter: Some(permanent_filter),
                extra_count: 1,
            });
        }
    }
    ts.restore(cp);
    Err(ts.err("not a trigger doubler"))
}

fn try_parse_extended_keyword_line(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> Option<StaticAbility> {
    let cp = ts.checkpoint();
    let mut kws: Vec<KeywordSpec> = Vec::new();
    // Optional leading normal keywords separated by `,` or `;`
    if let Ok((lead, _)) = try_parse(ts, parse_keyword_list) {
        if ts.eat_comma() {
            kws.extend(lead);
        } else if matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Semicolon)
        ) {
            ts.next_tok();
            kws.extend(lead);
        } else {
            ts.restore(cp);
        }
    }
    let body_cp = ts.checkpoint();

    // `Artifact landcycling {M}`
    if ts.eat_phrase(&["artifact", "landcycling"])
        && let Some(mc) = ts.eat_mana_cost()
    {
        kws.push(KeywordSpec {
            kind: KeywordKind::Typecycling,
            param: Some(KeywordParam::FilterAndCost(
                Box::new(ObjectFilter {
                    all_types: CardTypeMask::ARTIFACT.union(CardTypeMask::LAND),
                    ..Default::default()
                }),
                vec![Cost::Mana(mc)],
            )),
        });
        return Some(grant_self_keywords(kws, condition));
    }
    ts.restore(body_cp);

    // `<Type> offering`
    if let Some(w) = ts.peek_word()
        && let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase())
    {
        ts.next_word();
        if ts.eat_word("offering") {
            kws.push(KeywordSpec {
                kind: KeywordKind::Offering,
                param: Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    all_types: ct,
                    ..Default::default()
                }))),
            });
            return Some(grant_self_keywords(kws, condition));
        }
    }
    ts.restore(body_cp);

    // `Modular - Sunburst`
    if ts.eat_word("modular") && ts.eat_em_dash() && ts.eat_word("sunburst") {
        kws.push(KeywordSpec {
            kind: KeywordKind::Modular,
            param: None,
        });
        kws.push(KeywordSpec {
            kind: KeywordKind::Sunburst,
            param: None,
        });
        return Some(grant_self_keywords(kws, condition));
    }
    ts.restore(body_cp);

    // `protection from mana value <N> or less/greater`
    if ts.eat_phrase(&["protection", "from", "mana", "value"])
        && let Some(n) = ts.next_number(false)
    {
        let op = if ts.eat_phrase(&["or", "less"]) {
            Some(CompareOp::Le)
        } else if ts.eat_phrase(&["or", "greater"]) {
            Some(CompareOp::Ge)
        } else {
            None
        };
        if let Some(op) = op {
            kws.push(KeywordSpec {
                kind: KeywordKind::Protection,
                param: Some(KeywordParam::Filter(Box::new(ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::ManaValue,
                        op,
                        value: Box::new(ValueExpr::Const(n)),
                    }],
                    ..Default::default()
                }))),
            });
            return Some(grant_self_keywords(kws, condition));
        }
    }

    ts.restore(cp);
    None
}
