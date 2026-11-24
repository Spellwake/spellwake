//! Wave E7 strictly lossless clause parsers covering the remaining black-border
//! staples and orthogonal Magic rules templates while keeping Unfinity, Conspiracy
//! draft-matters, and Alpha tabletop anomalies outside general Stage 1 AST.
//!
//! Every rule here emits strongly-typed AST nodes in `crate::ast` with zero
//! `StructuredRule` or untyped `GameStateFlag` fallbacks.

use crate::ast::{
    CardTypeMask, CategoryIterationKind, ColorMask, CombatState, CompareOp, ConditionAst,
    ControllerRelation, CostAtomAst, DelayedStepScope, DurationAst, FilterPredicateAst, GameMarker,
    InstructionAst, NameFilterAst, ObjectFilterAst, ObjectFilterFlags, PlayerRefAst,
    PriorActionRefKind, ReplacementEffectAst, StatKind, StaticAbilityAst, SubjectRefAst,
    SupertypeMask, TargetCountAst, TokenSpecAst, TriggerEventAst, TriggerSpecAst, TurnStepAst,
    ValueExprAst, ZoneFilter,
};
use crate::clauses::{StaticOrReplacement, parse_effect_body};
use crate::vocab::Vocabulary;

fn obj_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        ..Default::default()
    }
}

fn card_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        any_card: true,
        ..Default::default()
    }
}

fn any_perm() -> ObjectFilterAst {
    ObjectFilterAst {
        any_permanent: true,
        ..Default::default()
    }
}

fn any_card_flt() -> ObjectFilterAst {
    ObjectFilterAst {
        any_card: true,
        ..Default::default()
    }
}

/// Strictly lossless Wave E7 effect parser called after core and Wave E6 parsers.
#[inline(never)]
pub(crate) fn parse_wave_e7_effects(
    s: &str,
    low: &str,
    _s_nodur: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Some(body) = parse_simultaneous_and_multiplayer_effects(low) {
        return Some(body);
    }
    if let Some(body) = parse_delayed_and_trigger_effects(s, low, vocab) {
        return Some(body);
    }
    parse_category_token_and_combat_effects(low)
}

/// Strictly lossless Wave E7 static and replacement parser.
#[inline(never)]
pub(crate) fn parse_wave_e7_statics(
    _s: &str,
    low: &str,
    _vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    let low = low.trim_end_matches('.');
    // Chains of Mephistopheles: draw replacement with discard-or-mill branching
    if low
        == "if a player would draw a card except the first one they draw in each of their draw steps, that player discards a card instead. if the player discards a card this way, they draw a card. if the player doesn't discard a card this way, they mill a card"
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "a player".to_string(),
                would: "draw a card except the first one they draw in each of their draw steps"
                    .to_string(),
                instead: "discard_then_draw_else_mill".to_string(),
                instead_body: Some(vec![
                    InstructionAst::Discard {
                        player: PlayerRefAst::ThatPlayer,
                        count: Some(Box::new(ValueExprAst::Const(1))),
                        filter: None,
                        anaphora: None,
                        random: false,
                    },
                    InstructionAst::IfPreviousSucceeded(vec![InstructionAst::Draw {
                        player: PlayerRefAst::ThatPlayer,
                        count: Box::new(ValueExprAst::Const(1)),
                        up_to: false,
                    }]),
                    InstructionAst::IfPreviousFailed(vec![InstructionAst::Mill {
                        player: PlayerRefAst::ThatPlayer,
                        count: Box::new(ValueExprAst::Const(1)),
                    }]),
                ]),
            },
        ));
    }

    // The Mimeoplasm: as-enters exile two creature cards from graveyards, copy one with +1/+1 counters equal to the other's power
    if low
        == "as ~ enters, you may exile two creature cards from graveyards. if you do, it enters as a copy of one of those cards with a number of additional +1/+1 counters on it equal to the power of the other card"
    {
        let mut gy_creature = card_type(CardTypeMask::CREATURE);
        gy_creature.zone = Some(ZoneFilter::AnyGraveyard);
        let mut exiled_creature = card_type(CardTypeMask::CREATURE);
        exiled_creature
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::Compound(vec![
                ReplacementEffectAst::AsEntersAction(vec![InstructionAst::Optional(vec![
                    InstructionAst::MoveCounted {
                        actor: Some(PlayerRefAst::You),
                        count: Some(Box::new(ValueExprAst::Const(2))),
                        up_to: false,
                        any_number: false,
                        subject: Box::new(SubjectRefAst::Group(Box::new(gy_creature))),
                        to: "exile".to_string(),
                    },
                ])]),
                ReplacementEffectAst::EntersAsCopy {
                    optional: false,
                    filter: Box::new(exiled_creature),
                    exception: Some("if_exiled_two_creature_cards_as_enters".to_string()),
                },
                ReplacementEffectAst::EntersWithCounters {
                    counter_kind: "+1/+1".to_string(),
                    count: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora("the other card".to_string())),
                        stat: StatKind::Power,
                    }),
                    per: None,
                    x_expr: None,
                    condition: Some(Box::new(ConditionAst::Marker(
                        GameMarker::ZoneActionHappenedThisWay {
                            filter: Box::new(card_type(CardTypeMask::CREATURE)),
                            action: "exile".to_string(),
                        },
                    ))),
                },
            ]),
        ));
    }

    // Nameless Race: as-enters pay any amount of life up to white nontoken permanents opponents control plus white cards in their graveyards
    if low
        == "as ~ enters, pay any amount of life. the amount you pay can't be more than the total number of white nontoken permanents your opponents control plus the total number of white cards in their graveyards"
    {
        let mut white_nontoken = any_perm();
        white_nontoken.any_colors = ColorMask::WHITE;
        white_nontoken.token = Some(false);
        white_nontoken.controller = Some(ControllerRelation::Opponents);
        let mut white_gy = any_card_flt();
        white_gy.any_colors = ColorMask::WHITE;
        white_gy.zone = Some(ZoneFilter::OpponentsGraveyards);
        let max_life = ValueExprAst::Add(
            Box::new(ValueExprAst::Count(Box::new(white_nontoken))),
            Box::new(ValueExprAst::Count(Box::new(white_gy))),
        );
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::AsEntersAction(vec![
                InstructionAst::PayCost(CostAtomAst::PayLife(Box::new(ValueExprAst::ChosenX))),
                InstructionAst::AbilityRestriction {
                    restriction: "as_enters_life_cap".to_string(),
                    condition: Some(Box::new(ConditionAst::Compare {
                        lhs: Box::new(ValueExprAst::ChosenX),
                        op: CompareOp::Le,
                        rhs: Box::new(max_life),
                    })),
                },
            ]),
        ));
    }

    // Snowblind: conditional -X/-Y where X is snow lands defending player or controller controls and Y = min(X, toughness - 1)
    if low
        == "enchanted creature gets -x/-y. if that creature is attacking, x is the number of snow lands defending player controls. otherwise, x is the number of snow lands its controller controls. y is equal to x or to enchanted creature's toughness minus 1, whichever is smaller"
    {
        let enchanted = SubjectRefAst::AttachedHost("enchanted creature".to_string());
        let mut attacking_flt = obj_type(CardTypeMask::CREATURE);
        attacking_flt.combat = Some(CombatState::Attacking);
        let mut def_snow_lands = obj_type(CardTypeMask::LAND);
        def_snow_lands.required_supertypes = SupertypeMask::SNOW;
        def_snow_lands.controller = Some(ControllerRelation::DefendingPlayer);
        let mut ctrl_snow_lands = obj_type(CardTypeMask::LAND);
        ctrl_snow_lands.required_supertypes = SupertypeMask::SNOW;
        ctrl_snow_lands.controller = Some(ControllerRelation::ItsController);
        let x_expr = ValueExprAst::Conditional {
            condition: Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(enchanted.clone()),
                filter: Box::new(attacking_flt),
            }),
            then_expr: Box::new(ValueExprAst::Count(Box::new(def_snow_lands))),
            else_expr: Box::new(ValueExprAst::Count(Box::new(ctrl_snow_lands))),
        };
        let toughness_minus_one = ValueExprAst::Sub(
            Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(enchanted.clone()),
                stat: StatKind::Toughness,
            }),
            Box::new(ValueExprAst::Const(1)),
        );
        let y_expr = ValueExprAst::Min(Box::new(x_expr.clone()), Box::new(toughness_minus_one));
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::DynamicModifyPT {
                subject: Box::new(enchanted),
                power_delta: Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::Const(0)),
                    Box::new(x_expr),
                )),
                toughness_delta: Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::Const(0)),
                    Box::new(y_expr),
                )),
                condition: None,
            },
        ));
    }

    // Storage Matrix: untap step single-type choice restriction (called after "as long as ~ is untapped, " peel)
    if low
        == "each player chooses artifact, creature, or land during their untap step. that player can untap only permanents of the chosen type this step"
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "untap_step_choose_artifact_creature_or_land_only_untap_chosen_type"
                    .to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    None
}

fn parse_simultaneous_and_multiplayer_effects(low: &str) -> Option<Vec<InstructionAst>> {
    // Goblin Welder: if both targets are still legal, simultaneously sacrifice the artifact and return the artifact card
    if low
        == "if both targets are still legal as this ability resolves, that player simultaneously sacrifices the artifact and returns the artifact card to the battlefield"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::AllTargetsLegal)),
            then_body: vec![InstructionAst::Simultaneous(vec![
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(ValueExprAst::Const(1))),
                    subject: Box::new(SubjectRefAst::Anaphora("the artifact".to_string())),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                },
                InstructionAst::ReturnZone {
                    subject: Box::new(SubjectRefAst::Anaphora("the artifact card".to_string())),
                    from_zone: Some("graveyard".to_string()),
                    to: "battlefield".to_string(),
                    tapped: false,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    random: false,
                    except: None,
                    count: None,
                    up_to: false,
                    any_number: false,
                    actor: Some(PlayerRefAst::ThatPlayer),
                },
            ])],
        }]);
    }

    // Sands of Time: simultaneously untap tapped artifacts/creatures/lands and tap untapped artifacts/creatures/lands
    if low
        == "that player simultaneously untaps each tapped artifact, creature, and land they control and taps each untapped artifact, creature, and land they control"
    {
        let acl_mask = CardTypeMask::ARTIFACT
            .union(CardTypeMask::CREATURE)
            .union(CardTypeMask::LAND);
        let mut tapped_acl = obj_type(acl_mask);
        tapped_acl.tapped = Some(true);
        tapped_acl.controller = Some(ControllerRelation::ThatPlayer);
        let mut untapped_acl = obj_type(acl_mask);
        untapped_acl.tapped = Some(false);
        untapped_acl.controller = Some(ControllerRelation::ThatPlayer);
        return Some(vec![InstructionAst::Simultaneous(vec![
            InstructionAst::UntapPermanent {
                subject: Box::new(SubjectRefAst::Each(Box::new(tapped_acl))),
                count: None,
                up_to: false,
            },
            InstructionAst::TapPermanent {
                subject: Box::new(SubjectRefAst::Each(Box::new(untapped_acl))),
                any_number: false,
            },
        ])]);
    }

    // Cruel Entertainment: first player controls second player's next turn and vice versa
    if low
        == "the first player controls the second player during the second player's next turn, and the second player controls the first player during the first player's next turn"
    {
        let first = SubjectRefAst::Anaphora("the first player".to_string());
        let second = SubjectRefAst::Anaphora("the second player".to_string());
        return Some(vec![
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(first.clone()),
                controlled: Box::new(second.clone()),
            },
            InstructionAst::PlayerControlsPlayerTurn {
                controller: Box::new(second),
                controlled: Box::new(first),
            },
        ]);
    }

    // Oath of Ghouls: choose target opponent whose graveyard has fewer creature cards than that player's graveyard
    if low
        == "that player chooses target player whose graveyard has fewer creature cards in it than their graveyard does and is their opponent"
    {
        let mut opp_gy_cr = card_type(CardTypeMask::CREATURE);
        opp_gy_cr.zone = Some(ZoneFilter::TargetOpponentGraveyard);
        let mut own_gy_cr = card_type(CardTypeMask::CREATURE);
        own_gy_cr.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::TargetOpponent,
                condition: Box::new(ConditionAst::Compare {
                    lhs: Box::new(ValueExprAst::Count(Box::new(opp_gy_cr))),
                    op: CompareOp::Lt,
                    rhs: Box::new(ValueExprAst::Count(Box::new(own_gy_cr))),
                }),
            }),
        }]);
    }

    // Rite of Ruin: ordered 1/2/3 sacrifices by chosen permanent types
    if low
        == "each player sacrifices one permanent of their choice of the first type, sacrifices two of their choice of the second type, then sacrifices three of their choice of the third type"
    {
        let nth_perm = |nth: u8| {
            let mut flt = any_perm();
            flt.predicates.push(FilterPredicateAst::NthChosenType(nth));
            Box::new(SubjectRefAst::Group(Box::new(flt)))
        };
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(ValueExprAst::Const(1))),
                    subject: nth_perm(1),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                },
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(ValueExprAst::Const(2))),
                    subject: nth_perm(2),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                },
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(ValueExprAst::Const(3))),
                    subject: nth_perm(3),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: None,
                },
            ],
        }]);
    }

    // Timesifter: highest mana value exiled takes an extra turn; ties repeat until broken
    if low
        == "the player who exiled the card with the greatest mana value takes an extra turn after this one"
    {
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::AnyPlayer,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                    player: PlayerRefAst::ThatPlayer,
                    action: PriorActionRefKind::ExiledThisWay,
                    stat: StatKind::ManaValue,
                    greatest: true,
                    allow_tie: false,
                })),
            }),
            body: vec![InstructionAst::ExtraTurn {
                player: PlayerRefAst::ThatPlayer,
            }],
        }]);
    }
    if low
        == "if two or more players' cards are tied for greatest, the tied players repeat this process until the tie is broken"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::PlayersTiedFor {
                metric: "greatest_mana_value_exiled".to_string(),
                min_tied: 2,
            }),
            then_body: vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: Vec::new(),
                until: Some("tie_is_broken".to_string()),
            }],
        }]);
    }

    // Psychic Battle: highest mana value revealed may change target(s); ties remain unchanged; no self-trigger
    if low
        == "the player who reveals the card with the greatest mana value may change the target or targets"
    {
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::AnyPlayer,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                    player: PlayerRefAst::ThatPlayer,
                    action: PriorActionRefKind::RevealedThisWay,
                    stat: StatKind::ManaValue,
                    greatest: true,
                    allow_tie: false,
                })),
            }),
            body: vec![InstructionAst::OptionalPlayer {
                player: PlayerRefAst::ThatPlayer,
                body: vec![InstructionAst::ChangeTargets {
                    target: Box::new(SubjectRefAst::Anaphora("the target or targets".to_string())),
                    new_target: None,
                    must_target: None,
                    single_target_only: false,
                }],
            }],
        }]);
    }
    if low == "if two or more cards are tied for greatest, the target or targets remain unchanged" {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::PlayersTiedFor {
                metric: "greatest_mana_value_revealed".to_string(),
                min_tied: 2,
            }),
            then_body: vec![InstructionAst::AbilityRestriction {
                restriction: "targets_remain_unchanged".to_string(),
                condition: None,
            }],
        }]);
    }
    if low == "changing targets this way doesn't trigger abilities of permanents named ~" {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "doesnt_trigger_abilities_of_permanents_named_self".to_string(),
            condition: None,
        }]);
    }

    None
}

fn parse_delayed_and_trigger_effects(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Chancellor of the Annex: opening-hand delayed trigger on each opponent's first spell of the game
    if let Some(after) =
        low.strip_prefix("when each opponent casts their first spell of the game, ")
    {
        let body_slice = &s[s.len() - after.len()..];
        let (body, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !body.is_empty() {
            let mut first_spell = ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            };
            first_spell
                .predicates
                .push(FilterPredicateAst::NthSpellCastThisGame(1));
            return Some(vec![InstructionAst::DelayedEventTrigger {
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::SpellCast {
                        caster: PlayerRefAst::EachOpponent,
                        spell: Some(Box::new(first_spell)),
                        self_spell: false,
                        targets: None,
                        nth_each_turn: None,
                        include_copy: false,
                    }],
                    intervening_if: None,
                },
                body,
            }]);
        }
    }

    // Zevlor, Elturel Exile: next single-target instant/sorcery spell targeting an opponent or their permanent copies for each other opponent
    if low
        == "when you next cast an instant or sorcery spell that targets only a single opponent or a single permanent an opponent controls this turn, for each other opponent, choose that player or a permanent they control, copy that spell, and the copy targets the chosen player or permanent"
    {
        let mut opp_perm = any_perm();
        opp_perm.controller = Some(ControllerRelation::ThatPlayer);
        return Some(vec![InstructionAst::WhenYouNext {
            event: "cast an instant or sorcery spell that targets only a single opponent or a single permanent an opponent controls this turn".to_string(),
            body: vec![InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachOtherOpponent,
                body: vec![
                    InstructionAst::ChooseFromGroup {
                        count: Box::new(ValueExprAst::Const(1)),
                        random: false,
                        group: Box::new(SubjectRefAst::Compound(vec![
                            SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
                            SubjectRefAst::Group(Box::new(opp_perm)),
                        ])),
                    },
                    InstructionAst::Copy {
                        subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                        exception: None,
                        count: None,
                    },
                    InstructionAst::ChangeTargets {
                        target: Box::new(SubjectRefAst::Anaphora("the copy".to_string())),
                        new_target: Some(Box::new(SubjectRefAst::Anaphora(
                            "the chosen player or permanent".to_string(),
                        ))),
                        must_target: None,
                        single_target_only: false,
                    },
                ],
            }],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Exterminator Magmarch: if another opponent controls a nonland permanent that spell could target, choose one
    if low
        == "if another opponent controls one or more nonland permanents that spell could target, choose one of those permanents"
    {
        let mut nonland = any_perm();
        nonland.forbidden_types = CardTypeMask::LAND;
        nonland
            .predicates
            .push(FilterPredicateAst::CouldBeTargetedBy(Box::new(
                SubjectRefAst::Anaphora("that spell".to_string()),
            )));
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::ControlsCount {
                player: PlayerRefAst::EachOtherOpponent,
                filter: Box::new(nonland),
                op: CompareOp::Ge,
                value: 1,
            }),
            then_body: vec![InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Anaphora("those permanents".to_string())),
            }],
        }]);
    }

    // Helm of Obedience: mill 1 then repeat until creature card or X cards put into graveyard this way
    if low
        == "target opponent mills a card, then repeats this process until a creature card or x cards have been put into their graveyard this way, whichever comes first"
    {
        return Some(vec![
            InstructionAst::Mill {
                player: PlayerRefAst::TargetOpponent,
                count: Box::new(ValueExprAst::Const(1)),
            },
            InstructionAst::RepeatProcessExt {
                times: Some(Box::new(ValueExprAst::ChosenX)),
                for_list: Vec::new(),
                until: Some(
                    "a creature card has been put into their graveyard this way".to_string(),
                ),
            },
        ]);
    }

    // Quenchable Fire: delayed upkeep damage unless {U} paid before that step
    if low
        == "it deals an additional 3 damage to that player or planeswalker at the beginning of your next upkeep step unless that player or that planeswalker's controller pays {u} before that step"
    {
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::Upkeep,
            scope: DelayedStepScope::YourNext,
            body: vec![InstructionAst::Unless {
                body: vec![InstructionAst::DealDamage {
                    source: Box::new(SubjectRefAst::SelfObject),
                    target: Box::new(SubjectRefAst::Anaphora(
                        "that player or planeswalker".to_string(),
                    )),
                    amount: Box::new(ValueExprAst::Const(3)),
                }],
                unless_player: Some(PlayerRefAst::ThatPlayerOrPlaneswalkerController),
                unless_cost: Some(CostAtomAst::PayMana("{U}".to_string())),
                unless_action: None,
                unless_condition: None,
            }],
        }]);
    }

    // Runesword: turn-scoped trigger denying regeneration on damage dealt to a creature
    if low
        == "if the creature deals damage to a creature this turn, the creature dealt damage can't be regenerated this turn"
    {
        return Some(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::DamageDealt {
                    combat_only: false,
                    noncombat_only: false,
                    source: Some(Box::new(SubjectRefAst::Anaphora(
                        "the creature".to_string(),
                    ))),
                    recipient: Some(Box::new(SubjectRefAst::Group(Box::new(obj_type(
                        CardTypeMask::CREATURE,
                    ))))),
                    grouped: false,
                    excess_only: false,
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::CombatRuleMod {
                subject: Box::new(SubjectRefAst::Anaphora(
                    "the creature dealt damage".to_string(),
                )),
                rule: "cant_be_regenerated".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            }],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Silhouette: turn-scoped replacement preventing damage caused by a spell or ability targeting that creature
    if low
        == "if a spell or ability that targets that creature would cause a source to deal damage to that creature this turn, prevent that damage"
    {
        return Some(vec![InstructionAst::TemporaryReplacement {
            replacement: Box::new(ReplacementEffectAst::EventInstead {
                subject_or_event: "a spell or ability that targets that creature".to_string(),
                would: "cause a source to deal damage to that creature".to_string(),
                instead: "prevent that damage".to_string(),
                instead_body: Some(vec![InstructionAst::PreventAllDamage {
                    combat_only: false,
                    target: Some(Box::new(SubjectRefAst::Anaphora(
                        "that creature".to_string(),
                    ))),
                    duration: DurationAst::Instant,
                }]),
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Cyclopean Tomb inner body (after DelayedTrigger peels "at the beginning of each of your upkeeps for the rest of the game, ")
    if low
        == "remove all mire counters from a land that a mire counter was put onto with ~ but that a mire counter has not been removed from with ~"
    {
        let mut land = obj_type(CardTypeMask::LAND);
        land.predicates
            .push(FilterPredicateAst::CounterPlacedNotRemovedBySelf {
                counter_kind: "mire".to_string(),
            });
        return Some(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Group(Box::new(land))),
            counter_kind: "mire".to_string(),
            all: true,
            count: None,
        }]);
    }

    None
}

#[allow(clippy::too_many_lines)]
fn parse_category_token_and_combat_effects(low: &str) -> Option<Vec<InstructionAst>> {
    // Niv-Mizzet Reborn: for each color pair, choose a card that's exactly those colors from among them
    if low == "for each color pair, choose a card that's exactly those colors from among them" {
        let mut pair_card = any_card_flt();
        pair_card
            .predicates
            .push(FilterPredicateAst::MatchesChosen { negated: false });
        return Some(vec![InstructionAst::ForEachCategory {
            category: CategoryIterationKind::ColorPair,
            body: vec![InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(pair_card))),
            }],
        }]);
    }

    // Exotic Pets: for each kind of counter among creatures you control, put a counter of that kind on either token
    if low
        == "for each kind of counter among creatures you control, put a counter of that kind on either of those tokens"
    {
        let mut your_creatures = obj_type(CardTypeMask::CREATURE);
        your_creatures.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::ForEachCategory {
            category: CategoryIterationKind::CounterKindAmong(Box::new(your_creatures)),
            body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora(
                    "either of those tokens".to_string(),
                )),
                counter_kind: "that kind".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            }],
        }]);
    }

    // The Book of Vile Darkness: create Vecna token with indestructible and all triggered abilities of the exiled cards
    if low
        == "create vecna, a legendary 8/8 black zombie god creature token with indestructible and all triggered abilities of the exiled cards"
    {
        let mut exiled_src = any_card_flt();
        exiled_src.zone = Some(ZoneFilter::Exile);
        exiled_src.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        exiled_src
            .predicates
            .push(FilterPredicateAst::HasAbilityKind {
                kind: "triggered".to_string(),
                negated: false,
            });
        return Some(vec![
            InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token: TokenSpecAst::Creature {
                    count: Box::new(ValueExprAst::Const(1)),
                    power: "8".to_string(),
                    toughness: "8".to_string(),
                    colors: ColorMask::BLACK,
                    supertypes: SupertypeMask::LEGENDARY,
                    card_types: CardTypeMask::CREATURE,
                    subtypes: vec!["Zombie".to_string(), "God".to_string()],
                    keywords: vec!["indestructible".to_string()],
                    tapped: false,
                    attacking: false,
                    named: Some("Vecna".to_string()),
                },
            },
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::InheritAbilities {
                    subject: Box::new(SubjectRefAst::Anaphora("the token".to_string())),
                    source_filter: Box::new(exiled_src),
                    activated_only: false,
                    condition: None,
                }),
                duration: DurationAst::Permanent,
            },
        ]);
    }

    // The Chain Veil: each planeswalker you control gets an extra loyalty activation this turn
    if low
        == "for each planeswalker you control, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn"
    {
        let mut your_pw = obj_type(CardTypeMask::PLANESWALKER);
        your_pw.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::ForEachObject {
            filter: Box::new(your_pw),
            body: vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                    rule: "extra_loyalty_activation_as_though_none_activated".to_string(),
                    filter: None,
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            }],
        }]);
    }

    // Heirloom Epic: convoke-style alternative payment per mana in this ability's activation cost
    if low
        == "for each mana in this ability's activation cost, you may tap an untapped creature you control rather than pay that mana"
    {
        let mut untapped_cr = obj_type(CardTypeMask::CREATURE);
        untapped_cr.tapped = Some(false);
        untapped_cr.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeAbilityCost {
                cost: vec![CostAtomAst::TapPermanent {
                    count: Box::new(ValueExprAst::Const(1)),
                    filter: Box::new(untapped_cr),
                    any_number: false,
                }],
                ability_kind: "this_ability_convoke".to_string(),
                scope: "per_mana_in_activation_cost".to_string(),
                condition: None,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    // Game of Chaos: coin flip branches with winner-decides repeat
    if low
        == "if you win the flip, you gain 1 life and target opponent loses 1 life, and you decide whether to flip again"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::CoinFlipOutcome {
                outcome: "win".to_string(),
            }),
            then_body: vec![
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::You,
                    delta: Box::new(ValueExprAst::Const(1)),
                    gain: true,
                },
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::TargetOpponent,
                    delta: Box::new(ValueExprAst::Const(1)),
                    gain: false,
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::You,
                    body: vec![InstructionAst::RepeatProcess {
                        condition: None,
                        max_times: None,
                        any_number: false,
                    }],
                },
            ],
        }]);
    }
    if low
        == "if you lose the flip, you lose 1 life and that opponent gains 1 life, and that player decides whether to flip again"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::CoinFlipOutcome {
                outcome: "lose".to_string(),
            }),
            then_body: vec![
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::You,
                    delta: Box::new(ValueExprAst::Const(1)),
                    gain: false,
                },
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    delta: Box::new(ValueExprAst::Const(1)),
                    gain: true,
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::RepeatProcess {
                        condition: None,
                        max_times: None,
                        any_number: false,
                    }],
                },
            ],
        }]);
    }

    // Goblin Artisans: lose flip counters target artifact spell you control not targeted by another Goblin Artisans ability
    if low
        == "if you lose the flip, counter target artifact spell you control that isn't the target of an ability from another creature named ~"
    {
        let mut other_self = obj_type(CardTypeMask::CREATURE);
        other_self.flags = ObjectFilterFlags::EXCLUDE_SELF;
        other_self.named = Some(NameFilterAst::SelfName);
        let mut ab_flt = ObjectFilterAst {
            is_ability: true,
            ..Default::default()
        };
        ab_flt
            .predicates
            .push(FilterPredicateAst::FromSource(Box::new(other_self)));
        let mut spell = obj_type(CardTypeMask::ARTIFACT);
        spell.any_spell = true;
        spell.controller = Some(ControllerRelation::You);
        spell
            .predicates
            .push(FilterPredicateAst::NotTargetedBy(Box::new(ab_flt)));
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::CoinFlipOutcome {
                outcome: "lose".to_string(),
            }),
            then_body: vec![InstructionAst::Counter {
                target: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(spell),
                    count: TargetCountAst::Exact(1),
                }),
                unless_pay: None,
                unless_action: None,
            }],
        }]);
    }

    // Drain Life and Soul Burn: gain life equal to damage dealt capped by pre-damage target stat (and {B} spent on X)
    if low
        == "you gain life equal to the damage dealt, but not more life than the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness"
    {
        return Some(vec![InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta: Box::new(ValueExprAst::Min(
                Box::new(ValueExprAst::PriorResult),
                Box::new(ValueExprAst::TargetPreDamageCap),
            )),
            gain: true,
        }]);
    }
    if low
        == "you gain life equal to the damage dealt, but not more than the amount of {b} spent on x, the player's life total before the damage was dealt, the planeswalker's loyalty before the damage was dealt, or the creature's toughness"
    {
        let black_spent_on_x = ValueExprAst::ManaSpentToCast {
            mana_filter: Some("{B}_on_X".to_string()),
        };
        return Some(vec![InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta: Box::new(ValueExprAst::Min(
                Box::new(ValueExprAst::Min(
                    Box::new(ValueExprAst::PriorResult),
                    Box::new(black_spent_on_x),
                )),
                Box::new(ValueExprAst::TargetPreDamageCap),
            )),
            gain: true,
        }]);
    }

    // Evolving Door: search for a creature card whose color count equals sacrificed creature's colors plus one
    if low
        == "count the colors of the sacrificed creature, then search your library for a creature card that's exactly that many colors plus one"
    {
        let mut sac_cr = obj_type(CardTypeMask::CREATURE);
        sac_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::SacrificedThisWay,
            ));
        let target_colors = ValueExprAst::Add(
            Box::new(ValueExprAst::ColorsAmong(Box::new(sac_cr))),
            Box::new(ValueExprAst::Const(1)),
        );
        let mut search_flt = card_type(CardTypeMask::CREATURE);
        search_flt
            .predicates
            .push(FilterPredicateAst::ColorCountCompare {
                op: CompareOp::Eq,
                expr: Box::new(target_colors),
            });
        return Some(vec![InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
            up_to: false,
            any_number: false,
            exact: false,
            filter: Box::new(search_flt),
            reveal: false,
            to_zone: "found_set".to_string(),
            shuffle: false,
            extra_zones: Vec::new(),
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: None,
            modifier: None,
            follow_up: Vec::new(),
        }]);
    }

    // Total War: destroy untapped non-Wall non-attacking creatures that player controls except those not controlled continuously since turn start
    if low
        == "destroy all untapped non-wall creatures that player controls that didn't attack, except for creatures the player hasn't controlled continuously since the beginning of the turn"
    {
        let mut victims = obj_type(CardTypeMask::CREATURE);
        victims.tapped = Some(false);
        victims.forbidden_subtypes.push("Wall".to_string());
        victims.controller = Some(ControllerRelation::ThatPlayer);
        victims.combat = Some(CombatState::NotAttacking);
        let mut except_flt = obj_type(CardTypeMask::CREATURE);
        except_flt.controller = Some(ControllerRelation::ThatPlayer);
        except_flt
            .predicates
            .push(FilterPredicateAst::ControlledSinceTurnStart { negated: true });
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(SubjectRefAst::All(Box::new(victims))),
            random: false,
            except: Some(Box::new(SubjectRefAst::Group(Box::new(except_flt)))),
        }]);
    }

    // Pygmy Hippo: defending player activates mana ability of each land they control and loses all unspent mana
    if low
        == "you may have defending player activate a mana ability of each land they control and lose all unspent mana"
    {
        let mut def_land = obj_type(CardTypeMask::LAND);
        def_land.controller = Some(ControllerRelation::DefendingPlayer);
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::ForEachObject {
                filter: Box::new(def_land),
                body: vec![InstructionAst::AbilityRestriction {
                    restriction: "activate_mana_ability".to_string(),
                    condition: None,
                }],
            },
            InstructionAst::LoseUnspentMana {
                player: PlayerRefAst::DefendingPlayer,
            },
        ])]);
    }

    // Glyph of Delusion: BindX(power of blocked creature) + put X glyph counters on target creature target Wall blocked this turn
    if low
        == "put x glyph counters on target creature that target wall blocked this turn, where x is the power of that blocked creature"
    {
        let mut wall = obj_type(CardTypeMask::CREATURE);
        wall.subtypes.push("Wall".to_string());
        let mut blocked_cr = obj_type(CardTypeMask::CREATURE);
        blocked_cr.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(wall),
                count: TargetCountAst::Exact(1),
            }),
            this_turn: true,
        });
        return Some(vec![InstructionAst::BindX {
            x_expr: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("that blocked creature".to_string())),
                stat: StatKind::Power,
            }),
            body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(blocked_cr),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: "glyph".to_string(),
                count: Box::new(ValueExprAst::ChosenX),
            }],
        }]);
    }

    // Glyph of Reincarnation: for each creature that died this way, put a creature card from LKI controller's graveyard onto the battlefield under owner's control
    if low
        == "for each creature that died this way, put a creature card from the graveyard of the player who controlled that creature the last time it became blocked by that wall onto the battlefield under its owner's control"
    {
        let mut died = obj_type(CardTypeMask::CREATURE);
        died.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DestroyedThisWay,
            ));
        let mut gy_cr = card_type(CardTypeMask::CREATURE);
        gy_cr.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        gy_cr.flags = ObjectFilterFlags::LKI_PAST_TENSE;
        gy_cr.predicates.push(FilterPredicateAst::BlockedBy {
            blocker: Box::new(SubjectRefAst::Anaphora("that wall".to_string())),
            this_turn: true,
        });
        return Some(vec![InstructionAst::ForEachObject {
            filter: Box::new(died),
            body: vec![InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(gy_cr))),
                to: "the battlefield under its owner's control".to_string(),
            }],
        }]);
    }

    // Barrin's Unmaking: return target permanent if it shares a color with the most common color among all permanents (or tied)
    if low
        == "return target permanent to its owner's hand if that permanent shares a color with the most common color among all permanents or a color tied for most common"
    {
        let tgt_perm = SubjectRefAst::TargetObject {
            filter: Box::new(any_perm()),
            count: TargetCountAst::Exact(1),
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(
                GameMarker::SharesMostCommonColorAmong {
                    subject: Box::new(tgt_perm.clone()),
                    scope: Box::new(any_perm()),
                    allow_tie: true,
                },
            )),
            then_body: vec![InstructionAst::ReturnZone {
                subject: Box::new(tgt_perm),
                from_zone: Some("battlefield".to_string()),
                to: "its owner's hand".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }],
        }]);
    }

    // Autumn Willow: until end of turn, ~ can be targeted by target player's spells and abilities as though it didn't have shroud
    if low
        == "until end of turn, ~ can be the target of spells and abilities controlled by target player as though it didn't have shroud"
    {
        let mut sp_or_ab = ObjectFilterAst {
            any_spell: true,
            is_ability: true,
            controller: Some(ControllerRelation::TargetPlayer),
            ..Default::default()
        };
        sp_or_ab.any_types = CardTypeMask::EMPTY;
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "ignore_shroud_for_controller".to_string(),
                filter: Some(Box::new(sp_or_ab)),
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Whippoorwill: damage to that creature this turn can't be prevented or redirected
    if low
        == "damage that would be dealt to that creature this turn can't be prevented or dealt instead to another permanent or player"
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Anaphora(
                    "that creature".to_string(),
                ))),
                rule: "damage_cant_be_prevented_or_redirected".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Minamo's Meddling: target spell's controller reveals hand and discards each card with same name as a spliced card
    if low
        == "that spell's controller reveals their hand, then discards each card with the same name as a card spliced onto that spell"
    {
        let mut spliced_match = any_card_flt();
        spliced_match
            .predicates
            .push(FilterPredicateAst::SameNameAsSplicedOnto(Box::new(
                SubjectRefAst::Anaphora("that spell".to_string()),
            )));
        return Some(vec![
            InstructionAst::RevealHand {
                player: PlayerRefAst::ThatObjectController,
            },
            InstructionAst::Discard {
                player: PlayerRefAst::ThatObjectController,
                count: None,
                filter: Some(Box::new(spliced_match)),
                anaphora: None,
                random: false,
            },
        ]);
    }

    // Suffocation: ~ deals 4 damage to the controller of the last red instant or sorcery spell that dealt damage to you this turn
    if low
        == "~ deals 4 damage to the controller of the last red instant or sorcery spell that dealt damage to you this turn"
    {
        let mut red_is = obj_type(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY));
        red_is.any_colors = ColorMask::RED;
        red_is.any_spell = true;
        red_is.flags = ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN;
        red_is
            .predicates
            .push(FilterPredicateAst::LastMatchingThisTurn);
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(SubjectRefAst::SelfObject),
            target: Box::new(SubjectRefAst::ControllerOf(Box::new(SubjectRefAst::Group(
                Box::new(red_is),
            )))),
            amount: Box::new(ValueExprAst::Const(4)),
        }]);
    }

    None
}
