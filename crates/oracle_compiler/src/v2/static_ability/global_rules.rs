//! Global, deckbuilding, combat-rule, and standalone keyword static abilities for `v2`.

use crate::ast::{CardTypeMask, ControllerRelation, TurnStepAst};
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref,
};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, Cost, FilterPredicate, FilterStaticKind, KeywordKind,
    KeywordParam, KeywordSpec, ManaCost, ManaSymbol, ObjectFilter, PlayerRef,
    PlayerRestrictionKind, RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef,
    SubjectStaticRule, TriggerModifierKind, ValueExpr, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};

/// Parse global, deckbuilding (Companion), voting, combat-rule, and extended standalone keyword
/// static abilities that do not follow the standard `<subject> <predicate>` shape.
pub(super) fn parse_global_static_ability(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();

    // 1. Companion deck-construction static rules (all 10 Companions)
    if let Some(st) = try_parse_companion_deck_rule(ts) {
        return Ok(st);
    }
    ts.restore(cp);

    // 2. `While voting, you get an additional vote / you may vote an additional time`
    if ts.eat_phrase(&["while", "voting"])
        && ts.eat_comma()
        && (ts.eat_phrase(&["you", "get", "an", "additional", "vote"])
            || ts.eat_phrase(&["you", "may", "vote", "an", "additional", "time"]))
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }
    ts.restore(cp);

    // 3. `While an opponent is choosing targets as part of casting a spell they control or activating an ability they control, that player must choose at least one Flagbearer on the battlefield if able.`
    if ts.eat_phrase(&[
        "while",
        "an",
        "opponent",
        "is",
        "choosing",
        "targets",
        "as",
        "part",
        "of",
        "casting",
        "a",
        "spell",
        "they",
        "control",
        "or",
        "activating",
        "an",
        "ability",
        "they",
        "control",
    ]) && ts.eat_comma()
        && ts.eat_phrase(&[
            "that",
            "player",
            "must",
            "choose",
            "at",
            "least",
            "one",
            "flagbearer",
            "on",
            "the",
            "battlefield",
            "if",
            "able",
        ])
    {
        return Ok(StaticAbility::FilterRule {
            filter: ObjectFilter {
                subtypes: vec!["Flagbearer".to_string()],
                ..Default::default()
            },
            kind: FilterStaticKind::AttachmentRestriction,
        });
    }
    ts.restore(cp);

    // 4. `While you're searching your library, you may cast ~ from your library.`
    if ts.eat_phrase(&["while", "you're", "searching", "your", "library"])
        && ts.eat_comma()
        && ts.eat_phrase(&["you", "may", "cast"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["from", "your", "library"])
    {
        return Ok(StaticAbility::ZoneCastPermission {
            player: PlayerRef::You,
            filter: None,
            subject: Some(SubjectRef::SelfObject),
            zone: Some(ZoneRef {
                zone: ZoneKind::Library,
                owner: ZoneOwner::You,
            }),
            mode: CastPermissionMode::CastFromZone,
            condition,
        });
    }
    ts.restore(cp);

    // 5. `For each ...` static abilities (`Zilortha`, `Ruxa`, `Siege Behemoth`, `Blue, Loyal Raptor`, `K'rrik`, `Magnigoth Treefolk`)
    if ts.eat_phrase(&["for", "each"]) {
        // `For each <flt>, you may have that creature assign its combat damage as though it weren't blocked`
        {
            let fcp = ts.checkpoint();
            if let Ok(flt) = try_parse(ts, parse_object_filter)
                && ts.eat_comma()
                && ts.eat_phrase(&[
                    "you", "may", "have", "that", "creature", "assign", "its", "combat", "damage",
                    "as", "though", "it", "weren't", "blocked",
                ])
            {
                return Ok(StaticAbility::SubjectRule {
                    subject: SubjectRef::Each(Box::new(flt)),
                    rule: SubjectStaticRule::AssignDamageAsUnblocked,
                    condition,
                });
            }
            ts.restore(fcp);
        }
        // `For each kind of counter on ~, <subj> enters with a counter of that kind on it`
        if ts.eat_phrase(&["kind", "of", "counter", "on"])
            && ts.eat_tilde()
            && ts.eat_comma()
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && (ts.eat_phrase(&[
                "enters", "with", "a", "counter", "of", "that", "kind", "on", "it",
            ]) || ts.eat_phrase(&[
                "enter", "with", "a", "counter", "of", "that", "kind", "on", "them",
            ]))
        {
            return Ok(StaticAbility::EntersWithCountersStatic {
                subject,
                kind: crate::v2::ir::CounterKind::Any,
                count: ValueExpr::Const(1),
                additional: false,
                condition,
            });
        }
        ts.restore(cp);
        let _ = ts.eat_phrase(&["for", "each"]);
        // `For each {B} in a cost, you may pay 2 life rather than pay that mana`
        if ts.eat_mana_cost().is_some()
            && ts.eat_phrase(&["in", "a", "cost"])
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may"])
            && let Ok(costs) = try_parse(ts, parse_cost_list)
            && ts.eat_phrase(&["rather", "than", "pay", "that", "mana"])
        {
            return Ok(StaticAbility::AlternativeCost(costs));
        }
        ts.restore(cp);
        let _ = ts.eat_phrase(&["for", "each"]);
        // `For each basic land type among lands you control, ~ has landwalk of that type`
        if ts.eat_phrase(&["basic", "land", "type", "among", "lands", "you", "control"])
            && ts.eat_comma()
            && ts.eat_tilde()
            && ts.eat_phrase(&["has", "landwalk", "of", "that", "type"])
        {
            return Ok(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: vec![KeywordSpec {
                    kind: KeywordKind::Landwalk,
                    param: None,
                }],
                condition,
            });
        }
    }
    ts.restore(cp);

    // 6. `Counters remain on ~ as it moves to any zone other than a player's hand or library`
    if ts.eat_phrase(&["counters", "remain", "on"])
        && ts.eat_tilde()
        && ts.eat_phrase(&[
            "as", "it", "moves", "to", "any", "zone", "other", "than", "a", "player's", "hand",
            "or", "library",
        ])
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }
    ts.restore(cp);

    // 7. `Spend only mana produced by <flt> to cast ~ [this way]` / `Spend only colored mana on X. ...`
    if ts.eat_phrase(&["spend", "only"]) {
        if ts.eat_phrase(&["mana", "produced", "by"])
            && let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["to", "cast"])
            && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]) || ts.eat_word("it"))
        {
            let _ = ts.eat_phrase(&["this", "way"]);
            return Ok(StaticAbility::FilterRule {
                filter,
                kind: FilterStaticKind::SpendManaAsAnyColor,
            });
        }
        ts.restore(cp);
        if ts.eat_phrase(&["spend", "only", "colored", "mana", "on", "x"]) {
            let _ = ts.eat_period();
            let _ = ts.eat_phrase(&[
                "no", "more", "than", "one", "mana", "of", "each", "color", "may", "be", "spent",
                "this", "way",
            ]);
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CostReductionRule));
        }
    }
    ts.restore(cp);

    // 8. `If ~ is your commander, choose a color before the game begins. ~ is the chosen color.`
    if ts.eat_word("if")
        && ts.eat_tilde()
        && ts.eat_phrase(&["is", "your", "commander"])
        && ts.eat_comma()
        && ts.eat_phrase(&["choose", "a", "color", "before", "the", "game", "begins"])
    {
        let _ = ts.eat_period();
        let _ = ts.eat_tilde() && ts.eat_phrase(&["is", "the", "chosen", "color"]);
        return Ok(StaticAbility::SubjectRule {
            subject: SubjectRef::SelfObject,
            rule: SubjectStaticRule::IsChosenColor,
            condition,
        });
    }
    ts.restore(cp);

    // 9. Life/damage/cleanup/monarch global rules
    if ts.eat_phrase(&["your", "life", "total", "can't", "change"]) {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::DontLoseForZeroLife,
        ));
    }
    if ts.eat_phrase(&["damage", "doesn't", "cause", "you", "to", "lose", "life"]) {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::DontLoseForZeroLife,
        ));
    }
    if ts.eat_phrase(&["damage", "isn't", "removed", "from"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["during", "cleanup", "steps"])
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }
    ts.restore(cp);
    if ts.eat_phrase(&["all", "damage", "is", "dealt"]) {
        let _ = ts.eat_phrase(&["to", "you"]);
        if ts.eat_phrase(&["as", "though", "its", "source", "had"])
            && let Ok(kw) = try_parse(ts, crate::v2::filter::parse_keyword_kind)
        {
            return Ok(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: vec![KeywordSpec {
                    kind: kw,
                    param: None,
                }],
                condition,
            });
        }
    }
    ts.restore(cp);
    if ts.eat_phrase(&[
        "lethal",
        "damage",
        "dealt",
        "to",
        "creatures",
        "you",
        "control",
        "is",
        "determined",
        "by",
        "their",
        "power",
        "rather",
        "than",
        "their",
        "toughness",
    ]) {
        return Ok(StaticAbility::SubjectRule {
            subject: SubjectRef::Group(Box::new(ObjectFilter {
                any_types: CardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                ..Default::default()
            })),
            rule: SubjectStaticRule::AssignDamageByToughness,
            condition,
        });
    }
    if ts.eat_phrase(&["the", "monarch", "controls", "enchanted", "creature"]) {
        return Ok(StaticAbility::ControlEnchanted {
            controller: PlayerRef::ThatPlayer,
        });
    }
    if ts.eat_phrase(&[
        "your",
        "devotion",
        "to",
        "each",
        "color",
        "and",
        "each",
        "combination",
        "of",
        "colors",
        "is",
        "increased",
        "by",
        "one",
    ]) {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }

    // 10. Counter placement / removal restrictions (`Solemnity`, `Fear of Sleep Paralysis`)
    if ts.eat_phrase(&["counters", "can't", "be", "put", "on"]) {
        let mut flts = Vec::new();
        while let Ok(f) = try_parse(ts, parse_object_filter) {
            flts.push(f);
            let sep = ts.checkpoint();
            let _ = ts.eat_comma();
            let _ = ts.eat_word("or");
            if try_parse(ts, parse_object_filter).is_ok() {
                ts.restore(sep);
                let _ = ts.eat_comma();
                let _ = ts.eat_word("or");
                continue;
            }
            ts.restore(sep);
            break;
        }
        if !flts.is_empty() {
            let filter = if flts.len() == 1 {
                flts.remove(0)
            } else {
                ObjectFilter {
                    predicates: vec![FilterPredicate::AnyOf(flts)],
                    ..Default::default()
                }
            };
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::All(Box::new(filter))),
                rule: RuleRestrictionKind::CantBeCast,
                condition,
            });
        }
    }
    ts.restore(cp);
    if let Ok(_kind) = try_parse(ts, parse_counter_kind)
        && ts.eat_phrase(&["counters", "can't", "be", "removed", "from"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        return Ok(StaticAbility::RuleRestriction {
            subject: Some(SubjectRef::All(Box::new(flt))),
            rule: RuleRestrictionKind::CantBeCast,
            condition,
        });
    }
    ts.restore(cp);

    // 11. `Ward abilities of those creatures don't trigger` (`Nowhere to Run`)
    if ts.eat_phrase(&[
        "ward",
        "abilities",
        "of",
        "those",
        "creatures",
        "don't",
        "trigger",
    ]) {
        return Ok(StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::EntersDontTrigger,
            filter: ObjectFilter {
                any_types: CardTypeMask::CREATURE,
                ..Default::default()
            },
            permanent_filter: None,
            extra_count: 0,
        });
    }

    // 12. `Each spell that would cost less than three mana to cast costs three mana to cast` (`Trinisphere`)
    if ts.eat_phrase(&[
        "each", "spell", "that", "would", "cost", "less", "than", "three", "mana", "to", "cast",
        "costs", "three", "mana", "to", "cast",
    ]) {
        return Ok(StaticAbility::CostModifier {
            subject: ObjectFilter {
                any_spell: true,
                ..Default::default()
            },
            delta: ManaCost {
                symbols: vec![ManaSymbol::Generic(3)],
            },
            less: false,
            scale: None,
            min_mana: Some(3),
            condition,
        });
    }

    // 13. `Except for <flt1> and <flt2>, <flt3> can't attack` (`Akron Legionnaire`)
    if ts.eat_phrase(&["except", "for"]) && try_parse(ts, parse_object_filter).is_ok() {
        if ts.eat_word("and") {
            let _ = opt(ts, parse_object_filter);
        }
        if ts.eat_comma()
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["can't", "attack"])
        {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(subj),
                rule: RuleRestrictionKind::CantAttack,
                condition,
            });
        }
    }
    ts.restore(cp);

    // 14. `The first card you foretell each turn costs {0} to foretell` (`Ranar`)
    if ts.eat_phrase(&[
        "the", "first", "card", "you", "foretell", "each", "turn", "costs",
    ]) && let Some(delta) = ts.eat_mana_cost()
        && ts.eat_phrase(&["to", "foretell"])
    {
        return Ok(StaticAbility::ActivationCostModifier {
            delta,
            less: true,
            scale: None,
            condition,
        });
    }
    ts.restore(cp);

    // 15. `All instances of color words in the text of spells and permanents are changed to the chosen color word`
    if ts.eat_phrase(&[
        "all",
        "instances",
        "of",
        "color",
        "words",
        "in",
        "the",
        "text",
        "of",
        "spells",
        "and",
        "permanents",
        "are",
        "changed",
        "to",
        "the",
        "chosen",
        "color",
        "word",
    ]) {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }

    // 16. `Rather than ...` (`Liesa`, `Dream Halls`, `Defensive Formation`)
    if ts.eat_phrase(&["rather", "than"]) {
        if ts.eat_word("pay")
            && ts.eat_mana_cost().is_some()
            && ts.eat_phrase(&["for", "each", "previous", "time", "you've", "cast"])
            && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            && ts.eat_phrase(&["from", "the", "command", "zone", "this", "game"])
            && ts.eat_comma()
            && ts.eat_word("pay")
            && let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["life", "that", "many", "times"])
        {
            return Ok(StaticAbility::AlternativeCost(vec![Cost::PayLife(
                ValueExpr::Const(n),
            )]));
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "rather", "than", "pay", "the", "mana", "cost", "for", "a", "spell",
        ]) && ts.eat_comma()
            && ts.eat_phrase(&["its", "controller", "may"])
            && let Ok(costs) = try_parse(ts, parse_cost_list)
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
        if ts.eat_phrase(&["rather", "than", "the", "attacking", "player"])
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "you",
                "assign",
                "the",
                "combat",
                "damage",
                "of",
                "each",
                "creature",
                "attacking",
                "you",
            ])
        {
            let _ = ts.eat_period();
            let _ = ts.eat_phrase(&[
                "you",
                "can",
                "divide",
                "that",
                "creature's",
                "combat",
                "damage",
                "as",
                "you",
                "choose",
                "among",
                "any",
                "of",
                "the",
                "creatures",
                "blocking",
                "it",
            ]);
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
        }
    }
    ts.restore(cp);

    // 17. Combat direction / blocking / untap choice rules (`Pramikon`, `Mystic Barrier`, `Invasion Plans`, `Storage Matrix`)
    if ts.eat_phrase(&[
        "each", "player", "may", "attack", "only", "the", "nearest", "opponent", "in", "the",
    ]) {
        let _ = ts.eat_word("last");
        if ts.eat_phrase(&["chosen", "direction"]) {
            let _ = ts.eat_phrase(&[
                "and",
                "planeswalkers",
                "controlled",
                "by",
                "that",
                "opponent",
            ]);
            return Ok(StaticAbility::RuleRestriction {
                subject: None,
                rule: RuleRestrictionKind::CantAttack,
                condition,
            });
        }
    }
    ts.restore(cp);
    if ts.eat_phrase(&[
        "the",
        "attacking",
        "player",
        "chooses",
        "how",
        "each",
        "creature",
        "blocks",
        "each",
        "combat",
    ]) {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }
    if ts.eat_phrase(&["each", "player", "chooses", "artifact"])
        && ts.eat_comma()
        && ts.eat_word("creature")
        && ts.eat_comma()
        && ts.eat_phrase(&["or", "land", "during", "their", "untap", "step"])
    {
        let _ = ts.eat_period();
        let _ = ts.eat_phrase(&[
            "that",
            "player",
            "can",
            "untap",
            "only",
            "permanents",
            "of",
            "the",
            "chosen",
            "type",
            "this",
            "step",
        ]);
        return Ok(StaticAbility::PlayerRestriction {
            player: PlayerRef::EachPlayer,
            restriction: PlayerRestrictionKind::CantUntapMoreThan(1),
            condition,
        });
    }
    ts.restore(cp);

    // 18. `A player losing unspent mana causes that player to lose that much life` (`Yurlok`)
    if ts.eat_phrase(&[
        "a", "player", "losing", "unspent", "mana", "causes", "that", "player", "to", "lose",
        "that", "much", "life",
    ]) {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }

    // 19. `Each power-up ability of permanents you control can be activated an additional time` (`Wonder Man`)
    if ts.eat_phrase(&[
        "each",
        "power-up",
        "ability",
        "of",
        "permanents",
        "you",
        "control",
        "can",
        "be",
        "activated",
        "an",
        "additional",
        "time",
    ]) {
        return Ok(StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::AdditionalOnAbility,
            filter: ObjectFilter::default(),
            permanent_filter: None,
            extra_count: 1,
        });
    }

    // 20. `Planeswalkers' loyalty abilities you activate cost an additional [+1] to activate` (`Carth`)
    if ts.eat_phrase(&[
        "planeswalkers'",
        "loyalty",
        "abilities",
        "you",
        "activate",
        "cost",
        "an",
        "additional",
    ]) && (ts.eat_word("+1") || ts.eat_phrase(&["+", "1"]))
        && ts.eat_phrase(&["to", "activate"])
    {
        return Ok(StaticAbility::ActivationCostModifier {
            delta: ManaCost::default(),
            less: true,
            scale: None,
            condition,
        });
    }
    ts.restore(cp);

    // 21. `Once each turn, you may pay {M} to reroll one or more dice you rolled` (`Monitor Monitor`)
    if ts.eat_phrase(&["once", "each", "turn"])
        && ts.eat_comma()
        && ts.eat_phrase(&["you", "may", "pay"])
        && let Some(mc) = ts.eat_mana_cost()
        && ts.eat_phrase(&["to", "reroll", "one", "or", "more", "dice", "you", "rolled"])
    {
        return Ok(StaticAbility::AdditionalCost(vec![Cost::Mana(mc)]));
    }
    ts.restore(cp);

    // 22. `Spells with the chosen names can't be cast and lands with the chosen names can't be played` (`Null Chamber`)
    if ts.eat_phrase(&[
        "spells", "with", "the", "chosen", "names", "can't", "be", "cast",
    ]) {
        let mut parts = vec![StaticAbility::RuleRestriction {
            subject: Some(SubjectRef::All(Box::new(ObjectFilter {
                any_spell: true,
                predicates: vec![FilterPredicate::OfChosenName],
                ..Default::default()
            }))),
            rule: RuleRestrictionKind::CantBeCast,
            condition: condition.clone(),
        }];
        if ts.eat_phrase(&[
            "and", "lands", "with", "the", "chosen", "names", "can't", "be", "played",
        ]) {
            parts.push(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::All(Box::new(ObjectFilter {
                    any_types: CardTypeMask::LAND,
                    predicates: vec![FilterPredicate::OfChosenName],
                    ..Default::default()
                }))),
                rule: RuleRestrictionKind::CantBeCast,
                condition,
            });
        }
        return Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            StaticAbility::Compound(parts)
        });
    }
    ts.restore(cp);

    // 23. `Activated abilities of the chosen permanent can't be activated` (`Scheming Fence`)
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
            subject: Some(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatPermanent,
            )),
            rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
            condition,
        });
    }

    // 24. `The top card of your library has plot[. The plot cost is equal to its mana cost]` (`Fblthp`)
    if ts.eat_phrase(&["the", "top", "card", "of", "your", "library", "has"])
        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
    {
        return Ok(StaticAbility::GrantKeywords {
            subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatCard),
            keywords,
            condition,
        });
    }
    ts.restore(cp);

    // 25. `Fortified land has <kws>` (`Darksteel Garrison`)
    if ts.eat_phrase(&["fortified", "land", "has"])
        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
    {
        return Ok(StaticAbility::GrantKeywords {
            subject: SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::EnchantedPermanent),
            keywords,
            condition,
        });
    }
    ts.restore(cp);

    // 26. `If ~ leaves the battlefield, this effect continues until end of turn` (`Titania's Song`)
    if ts.eat_word("if")
        && ts.eat_tilde()
        && ts.eat_phrase(&["leaves", "the", "battlefield"])
        && ts.eat_comma()
        && ts.eat_phrase(&["this", "effect", "continues", "until", "end", "of", "turn"])
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
    }
    ts.restore(cp);

    // 27. `If <flt> being dealt damage causes a triggered ability of <perm_flt> to trigger, that ability triggers an additional time` (`Wayta`)
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

    // 28. `If a creature card with <kw> was exiled with ~'s delve ability, ~ has <kw>. The same is true for ...` (`Soulflayer`)
    if ts.eat_phrase(&["if", "a", "creature", "card", "with"])
        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
        && ts.eat_phrase(&["was", "exiled", "with"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["'s", "delve", "ability"])
        && ts.eat_comma()
        && ts.eat_tilde()
        && ts.eat_word("has")
        && try_parse(ts, parse_keyword_list).is_ok()
    {
        let _ = ts.eat_period();
        if ts.eat_phrase(&["the", "same", "is", "true", "for"]) {
            let _ = try_parse(ts, parse_keyword_list);
        }
        return Ok(StaticAbility::GrantKeywords {
            subject: SubjectRef::SelfObject,
            keywords,
            condition,
        });
    }
    ts.restore(cp);

    // 29. Extended standalone keyword lines that bypassed `card.rs` step 7:
    //     - `Legendary/Nonbasic/Snow [forest|swamp|plains|island|mountain] landwalk` (and `First strike; legendary landwalk`)
    //     - `Trample over planeswalkers`
    //     - `<Type> offering` (`Artifact offering`)
    //     - `Artifact landcycling {M}`
    //     - `Devour <type> <N>` / `Devour X, where X is ...`
    //     - `Modular - Sunburst`
    //     - `[Flying|Lifelink,] protection from mana value <N> or less/greater` / `protection from spells and from permanents that were cast this turn`
    //     - `Flying, hexproof from activated and triggered abilities`
    if let Some(st) = try_parse_extended_keyword_line(ts, condition) {
        return Ok(st);
    }
    ts.restore(cp);

    // 30. `Skip your combat phase` (in addition to existing skip-step rules)
    if ts.eat_phrase(&["skip", "your", "combat", "phase"]) {
        return Ok(StaticAbility::SkipStep {
            player: PlayerRef::You,
            step: TurnStepAst::BeginningOfCombat,
        });
    }

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
                let _ = ts.eat_word("another") || ts.eat_word("an") || ts.eat_word("a");
                if let Ok(other) = try_parse(ts, parse_object_filter) {
                    source_filter = ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(vec![source_filter, other])],
                        ..Default::default()
                    };
                }
            }
            if ts.eat_word("triggers") {
                if ts.eat_word("while") {
                    let _ = opt(ts, super::cond_ext::parse_static_condition);
                }
                if ts.eat_comma()
                    && (ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
                        || ts.eat_phrase(&["it", "triggers", "an", "additional", "time"]))
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
    }
    ts.restore(cp);
    if (ts.eat_phrase(&["if", "you", "casting", "or", "copying"])
        && try_parse(ts, parse_object_filter).is_ok()
        && ts.eat_phrase(&["causes", "a", "triggered", "ability", "of"])
        || ts.eat_phrase(&[
            "if",
            "turning",
            "a",
            "face-down",
            "permanent",
            "face",
            "up",
            "causes",
            "a",
            "triggered",
            "ability",
            "of",
        ]))
        && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["to", "trigger"])
        && ts.eat_comma()
        && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
    {
        return Ok(StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::AdditionalTriggers,
            filter: ObjectFilter::default(),
            permanent_filter: Some(permanent_filter),
            extra_count: 1,
        });
    }
    ts.restore(cp);
    if ts.eat_phrase(&[
        "if",
        "a",
        "player",
        "drawing",
        "a",
        "card",
        "causes",
        "a",
        "triggered",
        "ability",
        "of",
    ]) && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["to", "trigger"])
        && ts.eat_comma()
        && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
    {
        return Ok(StaticAbility::TriggerModifier {
            kind: TriggerModifierKind::AdditionalTriggers,
            filter: ObjectFilter {
                any_card: true,
                ..Default::default()
            },
            permanent_filter: Some(permanent_filter),
            extra_count: 1,
        });
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
        if (ts.eat_phrase(&[
            "entering",
            "or",
            "leaving",
            "the",
            "battlefield",
            "causes",
            "a",
            "triggered",
            "ability",
            "of",
        ]) || ts.eat_phrase(&[
            "entering",
            "the",
            "battlefield",
            "causes",
            "a",
            "triggered",
            "ability",
            "of",
        ]) || ts.eat_phrase(&["entering", "causes", "a", "triggered", "ability", "of"])
            || ts.eat_phrase(&["dying", "causes", "a", "triggered", "ability", "of"])
            || ts.eat_phrase(&["attacking", "causes", "a", "triggered", "ability", "of"])
            || ts.eat_phrase(&[
                "dealing",
                "combat",
                "damage",
                "to",
                "a",
                "player",
                "causes",
                "a",
                "triggered",
                "ability",
                "of",
            ]))
            && let Ok(permanent_filter) = if ts.eat_tilde() {
                Ok(ObjectFilter::default())
            } else {
                try_parse(ts, parse_object_filter)
            }
        {
            let _ = ts.eat_phrase(&["or", "an", "emblem", "you", "own"]);
            if ts.eat_phrase(&["to", "trigger"])
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
    }
    ts.restore(cp);
    Err(ts.err("not a trigger doubler"))
}

fn try_parse_companion_deck_rule(ts: &mut TokenStream<'_>) -> Option<StaticAbility> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["your", "starting", "deck", "contains"]) {
        if ts.eat_phrase(&["only", "cards", "with"]) {
            if ts.eat_phrase(&["even", "mana", "values"])
                || ts.eat_phrase(&["odd", "mana", "values", "and", "land", "cards"])
                || (ts.eat_phrase(&["mana", "value"])
                    && ts.next_number(false).is_some()
                    && ts.eat_phrase(&["or", "greater", "and", "land", "cards"]))
            {
                return Some(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
            }
        } else if ts.eat_phrase(&["at", "least"])
            && ts.next_number(false).is_some()
            && ts.eat_phrase(&["cards", "more", "than", "the", "minimum", "deck", "size"])
        {
            return Some(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
        }
        ts.restore(cp);
        return None;
    }
    if ts.eat_phrase(&[
        "no", "card", "in", "your", "starting", "deck", "has", "more", "than", "one", "of", "the",
        "same", "mana", "symbol", "in", "its", "mana", "cost",
    ]) {
        return Some(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
    }
    if ts.eat_word("each")
        && let Ok(_flt) = try_parse(ts, parse_object_filter)
        && ts.eat_phrase(&["in", "your", "starting", "deck"])
    {
        if ts.eat_phrase(&["has", "a", "different", "name"])
            || ts.eat_phrase(&["has", "an", "activated", "ability"])
            || ts.eat_phrase(&["shares", "a", "card", "type"])
            || (ts.eat_phrase(&["has", "mana", "value"])
                && ts.next_number(false).is_some()
                && (ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "greater"])))
        {
            return Some(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
        }
        if ts.eat_phrase(&["is", "a"]) || ts.eat_phrase(&["is", "an"]) {
            let mut saw_sub = false;
            while let Some(w) = ts.peek_word() {
                if ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some() {
                    ts.next_word();
                    saw_sub = true;
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("or");
                } else {
                    break;
                }
            }
            if saw_sub && ts.eat_word("card") {
                return Some(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
            }
        }
    }
    ts.restore(cp);
    None
}

fn try_parse_extended_keyword_line(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> Option<StaticAbility> {
    let cp = ts.checkpoint();
    let mut kws: Vec<KeywordSpec> = Vec::new();
    // Optional leading normal keywords separated by `,` or `;` (e.g. `Flying, hexproof from ...`, `First strike; legendary landwalk`)
    if let Ok((lead, _)) = try_parse(ts, parse_keyword_list) {
        if ts.eat_comma()
            || matches!(
                ts.peek().map(|t| &t.kind),
                Some(crate::v2::lexer::TokenKind::Semicolon)
            )
        {
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(crate::v2::lexer::TokenKind::Semicolon)
            ) {
                ts.next_tok();
            }
            kws.extend(lead);
        } else {
            ts.restore(cp);
        }
    }
    let body_cp = ts.checkpoint();
    // Qualified landwalk (`legendary landwalk`, `nonbasic landwalk`, `snow landwalk`, `snow forestwalk`, `snow swampwalk`, `artifact landwalk`)
    if (ts.eat_word("legendary")
        || ts.eat_word("nonbasic")
        || ts.eat_word("snow")
        || ts.eat_word("artifact"))
        && (ts.eat_word("landwalk")
            || ts.eat_word("forestwalk")
            || ts.eat_word("swampwalk")
            || ts.eat_word("plainswalk")
            || ts.eat_word("islandwalk")
            || ts.eat_word("mountainwalk"))
    {
        kws.push(KeywordSpec {
            kind: KeywordKind::Landwalk,
            param: None,
        });
        return Some(StaticAbility::GrantKeywords {
            subject: SubjectRef::SelfObject,
            keywords: kws,
            condition,
        });
    }
    ts.restore(body_cp);

    // `Trample over planeswalkers`
    if ts.eat_phrase(&["trample", "over", "planeswalkers"]) {
        kws.push(KeywordSpec {
            kind: KeywordKind::Trample,
            param: None,
        });
        return Some(StaticAbility::GrantKeywords {
            subject: SubjectRef::SelfObject,
            keywords: kws,
            condition,
        });
    }

    // `Artifact landcycling {M}`
    if ts.eat_phrase(&["artifact", "landcycling"])
        && let Some(mc) = ts.eat_mana_cost()
    {
        kws.push(KeywordSpec {
            kind: KeywordKind::Typecycling,
            param: Some(KeywordParam::Cost(vec![Cost::Mana(mc)])),
        });
        return Some(StaticAbility::GrantKeywords {
            subject: SubjectRef::SelfObject,
            keywords: kws,
            condition,
        });
    }
    ts.restore(body_cp);

    // `<Type> offering` (`Artifact offering`)
    if let Some(w) = ts.peek_word()
        && crate::vocab::lookup_card_type(&w.to_ascii_lowercase()).is_some()
    {
        ts.next_word();
        if ts.eat_word("offering") {
            kws.push(KeywordSpec {
                kind: KeywordKind::Offering,
                param: None,
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
    }
    ts.restore(body_cp);

    // `Devour <type|subtype> <N>` or `Devour X, where X is the number of creatures devoured this way`
    if ts.eat_word("devour") {
        if ts.eat_word("x")
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "where",
                "x",
                "is",
                "the",
                "number",
                "of",
                "creatures",
                "devoured",
                "this",
                "way",
            ])
        {
            kws.push(KeywordSpec {
                kind: KeywordKind::Devour,
                param: Some(KeywordParam::VariableX),
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
        ts.restore(body_cp);
        let _ = ts.eat_word("devour");
        if ts.next_word().is_some()
            && let Some(n) = ts.next_number(false)
        {
            kws.push(KeywordSpec {
                kind: KeywordKind::Devour,
                param: Some(KeywordParam::Number(u16::try_from(n.max(0)).unwrap_or(1))),
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
    }
    ts.restore(body_cp);

    // `Modular - Sunburst`
    if ts.eat_word("modular")
        && matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::EmDash)
        )
    {
        ts.next_tok();
        if ts.eat_word("sunburst") {
            kws.push(KeywordSpec {
                kind: KeywordKind::Modular,
                param: None,
            });
            kws.push(KeywordSpec {
                kind: KeywordKind::Sunburst,
                param: None,
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
    }
    ts.restore(body_cp);

    // `hexproof from activated and triggered abilities`
    if ts.eat_phrase(&[
        "hexproof",
        "from",
        "activated",
        "and",
        "triggered",
        "abilities",
    ]) {
        kws.push(KeywordSpec {
            kind: KeywordKind::Hexproof,
            param: None,
        });
        return Some(StaticAbility::GrantKeywords {
            subject: SubjectRef::SelfObject,
            keywords: kws,
            condition,
        });
    }

    // `protection from mana value <N> or less/greater` / `protection from even/odd mana values` / `protection from spells and from permanents that were cast this turn`
    if ts.eat_phrase(&["protection", "from"]) {
        if ts.eat_phrase(&["mana", "value"])
            && ts.next_number(false).is_some()
            && (ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "greater"]))
        {
            kws.push(KeywordSpec {
                kind: KeywordKind::Protection,
                param: None,
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
        ts.restore(body_cp);
        let _ = ts.eat_phrase(&["protection", "from"]);
        if ts.eat_phrase(&["even", "mana", "values"])
            || ts.eat_phrase(&["odd", "mana", "values"])
            || ts.eat_phrase(&[
                "spells",
                "and",
                "from",
                "permanents",
                "that",
                "were",
                "cast",
                "this",
                "turn",
            ])
        {
            kws.push(KeywordSpec {
                kind: KeywordKind::Protection,
                param: None,
            });
            return Some(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: kws,
                condition,
            });
        }
    }

    ts.restore(cp);
    None
}
