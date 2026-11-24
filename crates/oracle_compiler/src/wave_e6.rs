//! Wave E6 strictly lossless clause parsers covering orthogonal multi-zone, prior-action,
//! combat-target, multiplayer turn-order, pile-visibility, mana-spend rider, cumulative
//! exile-until, and reflexive-trigger Magic rules templates.
//!
//! Every rule here emits strongly-typed AST nodes in `crate::ast` with zero opaque
//! `StructuredRule` or untyped `GameStateFlag(String)` fallbacks.

use crate::ast::{
    AltCastCostAst, CardTypeMask, ColorMask, CombatState, CompareOp, ConditionAst,
    ControllerRelation, CostAtomAst, CountLimitAst, DelayedStepScope, DurationAst,
    FilterPredicateAst, GameMarker, InstructionAst, InteractiveChoiceKind, NameFilterAst,
    ObjectFilterAst, ObjectFilterFlags, PlayerRefAst, PlayerStatKind, PriorActionRefKind,
    ReplacementEffectAst, Rounding, StatBoundAst, StatKind, StaticAbilityAst, SubjectRefAst,
    SupertypeMask, TargetCountAst, TokenSpecAst, TriggerEventAst, TriggerSpecAst, TurnStepAst,
    ValueExprAst, ZoneFilter,
};
use crate::clauses::{StaticOrReplacement, parse_atomic_effect, parse_effect_body};
use crate::filter::{
    parse_condition, parse_object_filter, parse_player_ref, parse_subject_or_target,
    parse_value_expr,
};
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

/// Strictly lossless Wave E6 effect parser called after core compositional and Wave E3-E5 parsers.
#[inline(never)]
#[allow(clippy::too_many_lines)]
pub(crate) fn parse_wave_e6_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Face-down / face-up pile separation (Atris, Fortune's Favor, Sauron's Ransom, Phyrexian Portal)
    if let Some(body) = parse_pile_separation_and_visibility(low, vocab) {
        return Some(body);
    }

    // 2. Cumulative exile/reveal from top of library until total stat threshold (Dream Harvest, Tasha's Hideous Laughter)
    if let Some(body) = parse_cumulative_exile_until(low, vocab) {
        return Some(body);
    }

    // 3. Source-choice damage redirection (Harm's Way, Shining Shoal)
    if let Some(body) = parse_source_choice_redirect(low, vocab) {
        return Some(body);
    }

    // 4. Multiplayer turn-order, direction-ordered, and per-category selections
    if let Some(body) = parse_multiplayer_and_category_choices(low, vocab) {
        return Some(body);
    }

    // 5. Typed reflexive event triggers (Explosion of Riches, Saruman of Many Colors, Unleash the Inferno)
    if let Some(body) = parse_reflexive_event_triggers(low, vocab) {
        return Some(body);
    }

    // 6. Mana-spend riders, alternative cast costs, next-spell mana rules, and Traps
    if let Some(body) = parse_mana_riders_and_alt_costs(low, vocab) {
        return Some(body);
    }

    // 7. Delayed triggers, temporary triggers, and one-shot continuous effects
    if let Some(body) = parse_delayed_and_temporary_clauses(s, low, vocab) {
        return Some(body);
    }

    // 8. Remaining clean Bucket 1 + Bucket 2 compositional clauses
    parse_remaining_lossless_clauses(s, low, s_nodur, dur, vocab)
}

/// Strictly lossless Wave E6 static/replacement parser.
#[inline(never)]
pub(crate) fn parse_wave_e6_statics(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // Oviya, Automech Artisan: "Each creature that's attacking one of your opponents has trample."
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = crate::filter::parse_keyword_list(kw_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywords {
                subject: Box::new(subj),
                keywords: kws,
                condition: None,
                paired_rule: None,
            },
        ));
    }

    // Eriette of the Charmed Apple: "Each creature that's enchanted by an Aura you control can't attack you or planeswalkers you control."
    if let Some((subj_raw, rest)) = low.split_once(" can't attack ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("cant_attack:{}", rest.trim()),
                filter: None,
                condition: None,
            },
        ));
    }

    // Sailors' Bane: "~ costs {1} less to cast for each card you own in exile and in your graveyard that's an instant card, a sorcery card, or a card that has an Adventure."
    if let Some(after) = low.strip_prefix("~ costs ")
        && let Some((delta_raw, fe_raw)) = after.split_once(" less to cast for each ")
        && let Ok(per_flt) = parse_object_filter(fe_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CostModifier {
                subject: Box::new(SubjectRefAst::SelfObject),
                delta: delta_raw.trim().to_ascii_uppercase(),
                less: true,
                next_only: false,
                per: Some(Box::new(per_flt)),
                scale_expr: None,
                condition: None,
            },
        ));
    }

    None
}

fn parse_pile_separation_and_visibility(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Atris, Fortune's Favor, Sauron's Ransom:
    // "<player> look(s) at the top <N> cards of your library and separate(s) them into a face-down pile and a face-up pile"
    if let Some((who_raw, rest)) = low
        .split_once(" looks at the top ")
        .or_else(|| low.split_once(" look at the top "))
        && let Some(chooser) = parse_player_ref(who_raw.trim())
        && let Some((cnt_raw, tail)) = rest.split_once(" cards of your library and separate")
        && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        let face_down_piles = if tail.contains("two face-down piles") {
            2
        } else if tail.contains("a face-down pile and a face-up pile")
            || tail.contains("a face-up pile and a face-down pile")
        {
            1
        } else {
            return None;
        };
        return Some(vec![
            InstructionAst::LookTopLibrary {
                reveal: false,
                library: "your".to_string(),
                count: Box::new(cnt),
            },
            InstructionAst::SeparateIntoPiles {
                chooser,
                pile_count: 2,
                face_down_piles,
            },
        ]);
    }

    // Phyrexian Portal: "If your library has ten or more cards in it, target opponent looks at the top ten cards of your library and separates them into two face-down piles"
    if let Some(after) = low.strip_prefix("if your library has ten or more cards in it, ")
        && let Some(inner) = parse_pile_separation_and_visibility(after, vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::CardsInLibrary,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(10)),
            }),
            then_body: inner,
        }]);
    }

    // Ashiok, Nightmare Muse: "You may cast up to three spells from among face-up cards your opponents own from exile without paying their mana costs"
    if low
        == "you may cast up to three spells from among face-up cards your opponents own from exile without paying their mana costs"
    {
        let flt = ObjectFilterAst {
            any_card: true,
            owner: Some(ControllerRelation::Opponents),
            zone: Some(ZoneFilter::Exile),
            flags: ObjectFilterFlags::FACE_UP,
            ..Default::default()
        };
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: true,
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                free: true,
                duration: "now".to_string(),
                count_limit: Some(CountLimitAst::UpTo(Box::new(ValueExprAst::Const(3)))),
                alt_cost: None,
            },
        ])]);
    }

    // The Dragon-Kami Reborn: "Exile one of them face down with a hatching counter on it, then put the rest on the bottom of your library in any order"
    if low
        == "exile one of them face down with a hatching counter on it, then put the rest on the bottom of your library in any order"
    {
        return Some(vec![
            InstructionAst::ExileWithCounters {
                subject: Box::new(SubjectRefAst::Anaphora("one of them".to_string())),
                counter_kind: "hatching".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down: true,
            },
            InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::You),
                count: None,
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                to: "bottom of your library in any order".to_string(),
            },
        ]);
    }

    // Pyxis of Pandemonium: "Each player turns face up all cards they own exiled with ~, then puts all permanent cards among them onto the battlefield"
    if low
        == "each player turns face up all cards they own exiled with ~, then puts all permanent cards among them onto the battlefield"
    {
        let owned_exiled = ObjectFilterAst {
            any_card: true,
            owner: Some(ControllerRelation::ThatPlayer),
            flags: ObjectFilterFlags::EXILED_WITH_SELF,
            ..Default::default()
        };
        let perm_cards = ObjectFilterAst {
            any_types: CardTypeMask::PERMANENT,
            any_card: true,
            ..Default::default()
        };
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::TurnFaceUp {
                    player: PlayerRefAst::ThatPlayer,
                    subject: Box::new(SubjectRefAst::All(Box::new(owned_exiled))),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(perm_cards))),
                    to: "the battlefield".to_string(),
                },
            ],
        }]);
    }

    // Ghastly Conscription: "Exile all creature cards from target player's graveyard in a face-down pile, shuffle that pile, then manifest those cards"
    if low
        == "exile all creature cards from target player's graveyard in a face-down pile, shuffle that pile, then manifest those cards"
    {
        let mut cr_gy = card_type(CardTypeMask::CREATURE);
        cr_gy.zone = Some(ZoneFilter::TargetPlayerGraveyard);
        return Some(vec![
            InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::All(Box::new(cr_gy))),
                random: false,
                face_down: true,
                into_pile: true,
            },
            InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(SubjectRefAst::Anaphora("that pile".to_string())),
            },
            InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(0)),
                dread: false,
                subject: Some(Box::new(SubjectRefAst::Anaphora("those cards".to_string()))),
            },
        ]);
    }

    // Expose the Culprit: "Exile any number of face-up creatures you control with disguise in a face-down pile, shuffle that pile, then cloak them"
    if low
        == "exile any number of face-up creatures you control with disguise in a face-down pile, shuffle that pile, then cloak them"
    {
        let flt = ObjectFilterAst {
            any_types: CardTypeMask::CREATURE,
            controller: Some(ControllerRelation::You),
            required_keywords: vec!["disguise".to_string()],
            flags: ObjectFilterFlags::FACE_UP,
            ..Default::default()
        };
        return Some(vec![
            InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                random: false,
                face_down: true,
                into_pile: true,
            },
            InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(SubjectRefAst::Anaphora("that pile".to_string())),
            },
            InstructionAst::Cloak {
                count: Box::new(ValueExprAst::Const(0)),
                subject: Some(Box::new(SubjectRefAst::Anaphora("them".to_string()))),
            },
        ]);
    }

    None
}

fn parse_cumulative_exile_until(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // Dream Harvest / Tasha's Hideous Laughter:
    // "<player> exile(s) cards from the top of their library until <they have / that player has> exiled cards with total mana value <N> or greater [this way]"
    if let Some((who_raw, rest)) = low
        .split_once(" exiles cards from the top of their library until ")
        .or_else(|| low.split_once(" exile cards from the top of their library until "))
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((_, mv_tail)) = rest.split_once("exiled cards with total mana value ")
        && let Some(n_raw) = mv_tail
            .strip_suffix(" or greater this way")
            .or_else(|| mv_tail.strip_suffix(" or greater"))
        && let Ok(threshold) = parse_value_expr(n_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExileOrRevealTopUntilTotalStat {
            exile: true,
            player,
            stat: StatKind::ManaValue,
            threshold: Box::new(threshold),
        }]);
    }
    None
}

fn parse_source_choice_redirect(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // Harm's Way / Shining Shoal:
    // "The next <N/X> damage that a source of your choice would deal to <from> this turn is dealt to <to> instead"
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((amt_raw, rest)) =
            after.split_once(" damage that a source of your choice would deal to ")
        && let Some((from_raw, to_raw)) = rest.split_once(" this turn is dealt to ")
        && let Some(to_clean) = to_raw.strip_suffix(" instead")
        && let (Ok(amt), Ok(to)) = (
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(to_clean.trim(), vocab),
        )
    {
        let from = match from_raw.trim() {
            "you and/or permanents you control" => {
                let mut p = any_perm();
                p.controller = Some(ControllerRelation::You);
                SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::You),
                    SubjectRefAst::All(Box::new(p)),
                ])
            }
            "you and/or creatures you control" => {
                let mut c = obj_type(CardTypeMask::CREATURE);
                c.controller = Some(ControllerRelation::You);
                SubjectRefAst::Compound(vec![
                    SubjectRefAst::Player(PlayerRefAst::You),
                    SubjectRefAst::All(Box::new(c)),
                ])
            }
            other => parse_subject_or_target(other, vocab).ok()?,
        };
        return Some(vec![InstructionAst::RedirectDamage {
            amount: Box::new(amt),
            from: Box::new(from),
            to: Box::new(to),
            source_choice: Some("source_of_your_choice".to_string()),
            duration: Some(DurationAst::UntilEndOfTurn),
        }]);
    }
    None
}

fn parse_multiplayer_and_category_choices(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Aminatou, the Fateshifter
    if low
        == "each player gains control of all nonland permanents other than ~ controlled by the next player in the chosen direction"
    {
        let mut flt = any_perm();
        flt.forbidden_types = CardTypeMask::LAND;
        flt.flags = ObjectFilterFlags::EXCLUDE_SELF;
        flt.controller = Some(ControllerRelation::NextPlayerInDirection);
        return Some(vec![InstructionAst::GainControl {
            controller: PlayerRefAst::EachPlayer,
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            duration: DurationAst::Permanent,
        }]);
    }

    // Order of Succession
    if low
        == "starting with you and proceeding in the chosen direction, each player chooses a creature controlled by the next player in that direction"
    {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::NextPlayerInDirection);
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::You,
            direction: Some("chosen".to_string()),
            each_player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            distinct: false,
            count: TargetCountAst::Exact(1),
            body: Vec::new(),
        }]);
    }

    // The Legend of Yangchen // Avatar Yangchen
    if low
        == "starting with you, each player chooses up to one permanent with mana value 3 or greater from among permanents your opponents control"
    {
        let mut flt = any_perm();
        flt.controller = Some(ControllerRelation::Opponents);
        flt.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Ge,
            value: Box::new(ValueExprAst::Const(3)),
        });
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::You,
            direction: None,
            each_player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            distinct: false,
            count: TargetCountAst::UpTo(1),
            body: Vec::new(),
        }]);
    }

    // Manifold Insights
    if low
        == "starting with the next opponent in turn order, each opponent chooses a different nonland card from among them"
    {
        let mut flt = any_card_flt();
        flt.forbidden_types = CardTypeMask::LAND;
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::AnyOpponent,
            direction: Some("turn_order".to_string()),
            each_player: PlayerRefAst::EachOpponent,
            filter: Box::new(flt),
            distinct: true,
            count: TargetCountAst::Exact(1),
            body: Vec::new(),
        }]);
    }

    // Rejoin the Fight
    if low
        == "then starting with the next opponent in turn order, each opponent chooses a creature card in your graveyard that hasn't been chosen"
    {
        let mut flt = card_type(CardTypeMask::CREATURE);
        flt.zone = Some(ZoneFilter::YourGraveyard);
        flt.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ChosenThisWay { negated: true },
            ));
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::AnyOpponent,
            direction: Some("turn_order".to_string()),
            each_player: PlayerRefAst::EachOpponent,
            filter: Box::new(flt),
            distinct: true,
            count: TargetCountAst::Exact(1),
            body: Vec::new(),
        }]);
    }

    // Fortunate Few
    if low
        == "choose a nonland permanent you don't control, then each other player chooses a nonland permanent they don't control that hasn't been chosen this way"
    {
        let mut flt = any_perm();
        flt.forbidden_types = CardTypeMask::LAND;
        flt.controller = Some(ControllerRelation::NotYou);
        return Some(vec![InstructionAst::SequentialChoose {
            start_player: PlayerRefAst::You,
            direction: Some("turn_order".to_string()),
            each_player: PlayerRefAst::EachPlayer,
            filter: Box::new(flt),
            distinct: true,
            count: TargetCountAst::Exact(1),
            body: Vec::new(),
        }]);
    }

    // Grenzo's Rebuttal
    if low
        == "starting with you, each player chooses an artifact, a creature, and a land from among the permanents controlled by the player to their left"
    {
        let mk = |mask| ObjectFilterAst {
            any_types: mask,
            controller: Some(ControllerRelation::PlayerToLeft),
            ..Default::default()
        };
        return Some(vec![InstructionAst::ChoosePerFilter {
            player: PlayerRefAst::EachPlayer,
            categories: vec![
                mk(CardTypeMask::ARTIFACT),
                mk(CardTypeMask::CREATURE),
                mk(CardTypeMask::LAND),
            ],
            sacrifice_rest: false,
        }]);
    }

    // Cataclysm
    if low
        == "each player chooses from among the permanents they control an artifact, a creature, an enchantment, and a land, then sacrifices the rest"
    {
        let mk = |mask| ObjectFilterAst {
            any_types: mask,
            controller: Some(ControllerRelation::ThatPlayer),
            ..Default::default()
        };
        return Some(vec![InstructionAst::ChoosePerFilter {
            player: PlayerRefAst::EachPlayer,
            categories: vec![
                mk(CardTypeMask::ARTIFACT),
                mk(CardTypeMask::CREATURE),
                mk(CardTypeMask::ENCHANTMENT),
                mk(CardTypeMask::LAND),
            ],
            sacrifice_rest: true,
        }]);
    }

    // Selective Adaptation (both sentences)
    if low
        == "choose from among them a card with flying, a card with first strike, and so on for double strike, deathtouch, haste, hexproof, indestructible, lifelink, menace, reach, trample, and vigilance"
    {
        let kws = [
            "flying",
            "first strike",
            "double strike",
            "deathtouch",
            "haste",
            "hexproof",
            "indestructible",
            "lifelink",
            "menace",
            "reach",
            "trample",
            "vigilance",
        ];
        let categories = kws
            .into_iter()
            .map(|kw| ObjectFilterAst {
                any_card: true,
                required_keywords: vec![kw.to_string()],
                ..Default::default()
            })
            .collect();
        return Some(vec![InstructionAst::ChoosePerFilter {
            player: PlayerRefAst::You,
            categories,
            sacrifice_rest: false,
        }]);
    }
    if low
        == "put one of the chosen cards onto the battlefield, the other chosen cards into your hand, and the rest into your graveyard"
    {
        return Some(vec![
            InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::You),
                count: Some(Box::new(ValueExprAst::Const(1))),
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Anaphora("the chosen cards".to_string())),
                to: "the battlefield".to_string(),
            },
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Anaphora(
                    "the other chosen cards".to_string(),
                )),
                to: "your hand".to_string(),
            },
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                to: "your graveyard".to_string(),
            },
        ]);
    }

    // Fatal Grudge: "Each opponent chooses a permanent they control that shares a card type with the sacrificed permanent and sacrifices it"
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some(flt_raw) = rest.strip_suffix(" and sacrifices it")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Sacrifice {
            player,
            count: Some(Box::new(ValueExprAst::Const(1))),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: Some(player),
        }]);
    }

    None
}

fn parse_reflexive_event_triggers(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // Explosion of Riches: "Whenever a card is drawn this way, ~ deals 5 damage to target opponent chosen at random from among your opponents"
    if let Some(body_raw) = low.strip_prefix("whenever a card is drawn this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::CardDrawn {
                    player: PlayerRefAst::AnyPlayer,
                    qualifier: "this_way".to_string(),
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    // Saruman of Many Colors: "When one or more cards are milled this way, exile target enchantment, instant, or sorcery card with equal or lesser mana value than that spell from an opponent's graveyard"
    if let Some(body_raw) = low.strip_prefix("when one or more cards are milled this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::CardMilled {
                    player: PlayerRefAst::AnyPlayer,
                    filter: Box::new(any_card_flt()),
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    // Unleash the Inferno: "When it deals excess damage this way, destroy target artifact or enchantment an opponent controls with mana value less than or equal to that amount of excess damage"
    if let Some(body_raw) = low.strip_prefix("when it deals excess damage this way, ")
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReflexiveEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::DamageDealt {
                    combat_only: false,
                    noncombat_only: false,
                    source: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                    recipient: None,
                    grouped: false,
                    excess_only: true,
                }],
                intervening_if: None,
            },
            body,
        }]);
    }

    None
}

fn parse_mana_riders_and_alt_costs(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // Opal Palace: "If you spend this mana to cast your commander, it enters with a number of additional +1/+1 counters on it equal to the number of times it's been cast from the command zone this game"
    if low
        == "if you spend this mana to cast your commander, it enters with a number of additional +1/+1 counters on it equal to the number of times it's been cast from the command zone this game"
    {
        let mut cmdr = any_card_flt();
        cmdr.flags = ObjectFilterFlags::IS_COMMANDER;
        cmdr.owner = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::ManaSpendRider {
            cast_filter: Box::new(cmdr),
            rider: vec![InstructionAst::TemporaryReplacement {
                replacement: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    counter_kind: "+1/+1".to_string(),
                    count: Box::new(ValueExprAst::TimesCastFromCommandZone(Box::new(
                        SubjectRefAst::Anaphora("it".to_string()),
                    ))),
                    additional: true,
                    per: None,
                }),
                duration: DurationAst::Instant,
            }],
        }]);
    }

    // Marshland Bloodcaster: "Rather than pay the mana cost of the next spell you cast this turn, you may pay life equal to that spell's mana value"
    if low
        == "rather than pay the mana cost of the next spell you cast this turn, you may pay life equal to that spell's mana value"
    {
        let spell_mv = ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            stat: StatKind::ManaValue,
        };
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayLife(Box::new(spell_mv))],
                condition: None,
                for_spells: Some(Box::new(ObjectFilterAst {
                    any_spell: true,
                    controller: Some(ControllerRelation::You),
                    ..Default::default()
                })),
                next_only: true,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // North Star: "For one spell this turn, you may spend mana as though it were mana of any type to pay that spell's mana cost"
    if low
        == "for one spell this turn, you may spend mana as though it were mana of any type to pay that spell's mana cost"
    {
        return Some(vec![InstructionAst::TemporaryManaSpendRule {
            mana_kind: None,
            as_though: "mana_of_any_type".to_string(),
            purpose: Some("pay_spell_mana_cost".to_string()),
            duration: DurationAst::UntilEndOfTurn,
            next_only: true,
        }]);
    }

    // Inferno Trap / Ravenous Trap (alternative cast cost lines routed through spell_effects when condition parses)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", you may pay ")
        && let Some(cost_raw) = rest.strip_suffix(" rather than pay ~'s mana cost")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(cost_atom) = crate::clauses::parse_cost_atom(cost_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeCastCost {
                cost: vec![cost_atom],
                condition: Some(Box::new(cond)),
                for_spells: None,
                next_only: false,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    // Mr. House, President and CEO: "Roll a six-sided die plus an additional six-sided die for each mana from Treasures spent to activate this ability"
    if low
        == "roll a six-sided die plus an additional six-sided die for each mana from treasures spent to activate this ability"
    {
        return Some(vec![
            InstructionAst::RollDie { sides: 6 },
            InstructionAst::ForEachCount {
                count: Box::new(ValueExprAst::ManaSpentToActivate {
                    mana_filter: Some("from_treasure".to_string()),
                }),
                body: vec![InstructionAst::RollDie { sides: 6 }],
            },
        ]);
    }

    // Caravan Vigil / Celestial Reunion / Dragon's Fire conditional "instead"
    if let Some((before_instead, cond_raw)) =
        low.split_once(" instead of putting it into your hand if ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(inner) = parse_atomic_effect(before_instead.trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: vec![InstructionAst::ReplaceEffect(inner)],
        }]);
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", ")
        && let Some(action_raw) = rest.strip_suffix(" instead of putting it into your hand")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(inner) = parse_atomic_effect(action_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: vec![InstructionAst::ReplaceEffect(inner)],
        }]);
    }
    if low
        == "if you revealed a dragon card or chose a dragon as you cast ~, ~ deals damage equal to the power of that card or creature instead"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: Some("revealed_or_chose_dragon".to_string()),
            })),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::DealDamage {
                    source: Box::new(SubjectRefAst::SelfObject),
                    target: Box::new(SubjectRefAst::Anaphora("previous_target".to_string())),
                    amount: Box::new(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora(
                            "that card or creature".to_string(),
                        )),
                        stat: StatKind::Power,
                    }),
                },
            ])],
        }]);
    }

    // Yidaro, Wandering Monster
    if low
        == "if you've cycled a card named ~ four or more times this game, put it onto the battlefield from your graveyard instead"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(
                GameMarker::CycledCardNamedTimesThisGame {
                    name: NameFilterAst::SelfName,
                    min_times: 4,
                },
            )),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::ReturnZone {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    from_zone: Some("your graveyard".to_string()),
                    to: "the battlefield".to_string(),
                    tapped: false,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    random: false,
                    except: None,
                    count: None,
                    up_to: false,
                    any_number: false,
                    actor: Some(PlayerRefAst::You),
                },
            ])],
        }]);
    }

    None
}

fn parse_delayed_and_temporary_clauses(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Overblaze: "Each time target permanent would deal damage to a permanent or player this turn, it deals double that damage to that permanent or player instead"
    if low
        == "each time target permanent would deal damage to a permanent or player this turn, it deals double that damage to that permanent or player instead"
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::DoubleAllDamage {
                source: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(any_perm()),
                    count: TargetCountAst::Exact(1),
                }),
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Nuka-Nuke Launcher: "until the end of defending player's next turn, that player gets two rad counters whenever they cast a spell"
    if low
        == "until the end of defending player's next turn, that player gets two rad counters whenever they cast a spell"
    {
        return Some(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::SpellCast {
                    caster: PlayerRefAst::DefendingPlayer,
                    spell: None,
                    self_spell: false,
                    targets: None,
                    nth_each_turn: None,
                    include_copy: false,
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::GainPlayerCounters {
                player: PlayerRefAst::DefendingPlayer,
                counter_kind: "rad".to_string(),
                count: Box::new(ValueExprAst::Const(2)),
            }],
            duration: DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::DefendingPlayer),
        }]);
    }

    // Gaze of Pain: "Until end of turn, whenever a creature you control attacks and isn't blocked, you may choose to have it deal damage equal to its power to a target creature"
    if low
        == "until end of turn, whenever a creature you control attacks and isn't blocked, you may choose to have it deal damage equal to its power to a target creature"
    {
        let mut cr_you = obj_type(CardTypeMask::CREATURE);
        cr_you.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::AttacksUnblocked {
                    subject: Box::new(SubjectRefAst::Group(Box::new(cr_you))),
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::Optional(vec![InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                target: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(obj_type(CardTypeMask::CREATURE)),
                    count: TargetCountAst::Exact(1),
                }),
                amount: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::Power,
                }),
            }])],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Howl of the Horde
    if low
        == "if you attacked this turn, when you next cast an instant or sorcery spell this turn, copy that spell an additional time"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::AttackedThisTurn),
            then_body: vec![InstructionAst::WhenYouNext {
                event: "cast an instant or sorcery spell".to_string(),
                body: vec![InstructionAst::Copy {
                    subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                    exception: None,
                    count: Some(Box::new(ValueExprAst::Const(1))),
                }],
                duration: DurationAst::UntilEndOfTurn,
            }],
        }]);
    }

    // Repeated Reverberation
    if low
        == "when you next cast an instant spell, cast a sorcery spell, or activate a loyalty ability this turn, copy that spell or ability twice"
    {
        return Some(vec![InstructionAst::WhenYouNext {
            event: "cast an instant spell, cast a sorcery spell, or activate a loyalty ability"
                .to_string(),
            body: vec![InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora("that spell or ability".to_string())),
                exception: None,
                count: Some(Box::new(ValueExprAst::Const(2))),
            }],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // Mechtitan Core: "When that token leaves the battlefield, return all cards exiled with ~ except ~ to the battlefield tapped under their owners' control"
    if low
        == "when that token leaves the battlefield, return all cards exiled with ~ except ~ to the battlefield tapped under their owners' control"
    {
        let mut exiled = any_card_flt();
        exiled.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        return Some(vec![InstructionAst::DelayedEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: None,
                    subject: Box::new(SubjectRefAst::Anaphora("that token".to_string())),
                    grouped: false,
                    entering_controller: None,
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::All(Box::new(exiled))),
                from_zone: Some("exile".to_string()),
                to: "the battlefield tapped under their owners' control".to_string(),
                tapped: true,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: Some(Box::new(SubjectRefAst::SelfObject)),
                count: None,
                up_to: false,
                any_number: false,
                actor: None,
            }],
        }]);
    }

    // Reincarnation: "When that creature dies this turn, return a creature card from its owner's graveyard to the battlefield under the control of that creature's owner"
    if low
        == "when that creature dies this turn, return a creature card from its owner's graveyard to the battlefield under the control of that creature's owner"
    {
        let mut cr_gy = card_type(CardTypeMask::CREATURE);
        cr_gy.zone = Some(ZoneFilter::ItsOwnerGraveyard);
        return Some(vec![InstructionAst::DelayedEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: Some("graveyard".to_string()),
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    grouped: false,
                    entering_controller: None,
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(cr_gy))),
                from_zone: Some("its owner's graveyard".to_string()),
                to: "the battlefield under the control of that creature's owner".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: Some(Box::new(ValueExprAst::Const(1))),
                up_to: false,
                any_number: false,
                actor: None,
            }],
        }]);
    }

    // Hazezon Tamar: "create X 1/1 Sand Warrior creature tokens that are red, green, and white at the beginning of your next upkeep, where X is the number of lands you control at that time"
    if low
        == "create x 1/1 sand warrior creature tokens that are red, green, and white at the beginning of your next upkeep, where x is the number of lands you control at that time"
    {
        let mut lands = obj_type(CardTypeMask::LAND);
        lands.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::Upkeep,
            scope: DelayedStepScope::YourNext,
            body: vec![InstructionAst::BindX {
                x_expr: Box::new(ValueExprAst::Count(Box::new(lands))),
                body: vec![InstructionAst::CreateToken {
                    controller: PlayerRefAst::You,
                    token: TokenSpecAst::Creature {
                        count: Box::new(ValueExprAst::ChosenX),
                        power: "1".to_string(),
                        toughness: "1".to_string(),
                        colors: ColorMask::RED
                            .union(ColorMask::GREEN)
                            .union(ColorMask::WHITE),
                        supertypes: SupertypeMask::EMPTY,
                        card_types: CardTypeMask::CREATURE,
                        subtypes: vec!["Sand".to_string(), "Warrior".to_string()],
                        keywords: Vec::new(),
                        tapped: false,
                        attacking: false,
                        named: None,
                    },
                }],
            }],
        }]);
    }

    // Magus of the Jar / Memory Jar: "At the beginning of the next end step, each player discards their hand and returns to their hand each card they exiled this way"
    if low
        == "at the beginning of the next end step, each player discards their hand and returns to their hand each card they exiled this way"
    {
        let mut exiled = any_card_flt();
        exiled
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndStep,
            scope: DelayedStepScope::Next,
            body: vec![InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachPlayer,
                body: vec![
                    InstructionAst::DiscardHand {
                        player: PlayerRefAst::ThatPlayer,
                    },
                    InstructionAst::ReturnZone {
                        subject: Box::new(SubjectRefAst::Each(Box::new(exiled))),
                        from_zone: Some("exile".to_string()),
                        to: "their hand".to_string(),
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
                ],
            }],
        }]);
    }

    // Elkin Lair / Infinite Authority: "At the beginning of the next end step, if <cond>, <body>"
    if let Some(after) = low.strip_prefix("at the beginning of the next end step, if ")
        && let Some((cond_raw, body_raw)) = after.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndStep,
            scope: DelayedStepScope::Next,
            body: vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }],
        }]);
    }

    // Arcum's Whistle: "If they don't pay, the creature attacks this turn if able, and at the beginning of the next end step, destroy it if it didn't attack this turn"
    if low
        == "if they don't pay, the creature attacks this turn if able, and at the beginning of the next end step, destroy it if it didn't attack this turn"
    {
        let cr = SubjectRefAst::Anaphora("the creature".to_string());
        return Some(vec![InstructionAst::IfPreviousFailed(vec![
            InstructionAst::CombatRuleMod {
                subject: Box::new(cr.clone()),
                rule: "attacks_this_turn_if_able".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                scope: DelayedStepScope::Next,
                body: vec![InstructionAst::IfCondition {
                    condition: Box::new(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                        subject: Box::new(cr.clone()),
                        or_entered_this_turn: false,
                    })),
                    then_body: vec![InstructionAst::Destroy {
                        subject: Box::new(cr),
                        random: false,
                        except: None,
                    }],
                }],
            },
        ])]);
    }

    // Peace Talks: "This turn and next turn, creatures can't attack, and players and permanents can't be the targets of spells or activated abilities"
    if low
        == "this turn and next turn, creatures can't attack, and players and permanents can't be the targets of spells or activated abilities"
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::Compound(vec![
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::All(Box::new(obj_type(
                        CardTypeMask::CREATURE,
                    ))))),
                    rule: "cant_attack".to_string(),
                    filter: None,
                    condition: None,
                },
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Compound(vec![
                        SubjectRefAst::Player(PlayerRefAst::EachPlayer),
                        SubjectRefAst::All(Box::new(any_perm())),
                    ]))),
                    rule: "cant_be_targeted_by_spells_or_activated_abilities".to_string(),
                    filter: None,
                    condition: None,
                },
            ])),
            duration: DurationAst::UntilEndOfNextTurn,
        }]);
    }

    // The First Tyrannic War: "If its mana cost contains {X}, it enters with a number of +1/+1 counters on it equal to the number of lands you control"
    // Must compile as TemporaryReplacement(OthersEnterWithCounters), never PutCounters (0/0 Hydra SBA trap).
    if low
        == "if its mana cost contains {x}, it enters with a number of +1/+1 counters on it equal to the number of lands you control"
    {
        let mut x_flt = any_card_flt();
        x_flt
            .predicates
            .push(FilterPredicateAst::HasManaSymbolInCost("{X}".to_string()));
        let mut lands = obj_type(CardTypeMask::LAND);
        lands.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(x_flt),
            }),
            then_body: vec![InstructionAst::TemporaryReplacement {
                replacement: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    counter_kind: "+1/+1".to_string(),
                    count: Box::new(ValueExprAst::Count(Box::new(lands))),
                    additional: false,
                    per: None,
                }),
                duration: DurationAst::Instant,
            }],
        }]);
    }

    None
}

#[allow(clippy::too_many_lines)]
fn parse_remaining_lossless_clauses(
    _s: &str,
    low: &str,
    _s_nodur: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Serpentine Spike: 3 pairwise-distinct targets via DistinctTarget { ordinal: 1/2/3 }
    if low
        == "~ deals 2 damage to target creature, 3 damage to another target creature, and 4 damage to a third target creature"
    {
        let cr = obj_type(CardTypeMask::CREATURE);
        return Some(
            [(1u8, 2i32), (2, 3), (3, 4)]
                .into_iter()
                .map(|(ordinal, amt)| InstructionAst::DealDamage {
                    source: Box::new(SubjectRefAst::SelfObject),
                    target: Box::new(SubjectRefAst::DistinctTarget {
                        ordinal,
                        filter: Box::new(cr.clone()),
                    }),
                    amount: Box::new(ValueExprAst::Const(amt)),
                })
                .collect(),
        );
    }

    // 2. Ballroom Brawlers: single target choice without duplicating TargetObject across branches
    if low
        == "~ and up to one other target creature you control both gain your choice of first strike or lifelink until end of turn"
    {
        let mut other_cr = obj_type(CardTypeMask::CREATURE);
        other_cr.controller = Some(ControllerRelation::You);
        other_cr.flags = ObjectFilterFlags::EXCLUDE_SELF;
        return Some(vec![InstructionAst::GrantKeywordChoice {
            subject: Box::new(SubjectRefAst::Compound(vec![
                SubjectRefAst::SelfObject,
                SubjectRefAst::TargetObject {
                    filter: Box::new(other_cr),
                    count: TargetCountAst::UpTo(1),
                },
            ])),
            choices: vec!["first strike".to_string(), "lifelink".to_string()],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 3. Master of the Wild Hunt: divider is ItsController, NOT You
    if low
        == "that creature deals damage equal to its power divided as its controller chooses among any number of those wolves"
    {
        return Some(vec![InstructionAst::DealDamageDivided {
            source: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            amount: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                stat: StatKind::Power,
            }),
            targets_desc: "any number of those wolves".to_string(),
            divider: Some(PlayerRefAst::ItsController),
        }]);
    }

    // 4. Turtles Forever: exact: true overrides CR 701.19b partial fail-to-find
    if low
        == "search your library and/or outside the game for exactly four legendary creature cards you own with different names, then reveal those cards"
    {
        let mut flt = card_type(CardTypeMask::CREATURE);
        flt.required_supertypes = SupertypeMask::LEGENDARY;
        flt.owner = Some(ControllerRelation::You);
        flt.flags = ObjectFilterFlags::DIFFERENT_NAMES;
        return Some(vec![InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: Box::new(ValueExprAst::Const(4)),
            up_to: false,
            any_number: false,
            exact: true,
            filter: Box::new(flt),
            reveal: true,
            to_zone: "found_set".to_string(),
            shuffle: false,
            extra_zones: vec![ZoneFilter::OutsideTheGame],
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: None,
            modifier: None,
            follow_up: Vec::new(),
        }]);
    }

    // 5. Wandering Archaic // Explore the Vastlands: two independent optional picks (land AND/OR instant-or-sorcery)
    if low
        == "each player looks at the top five cards of their library and may reveal a land card and/or an instant or sorcery card from among them"
    {
        let land = card_type(CardTypeMask::LAND);
        let inst_sorc = card_type(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::LookTopLibrary {
                    reveal: false,
                    library: "their".to_string(),
                    count: Box::new(ValueExprAst::Const(5)),
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::RevealFromLooked {
                        filter: Box::new(land),
                        to: "revealed".to_string(),
                    }],
                },
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::RevealFromLooked {
                        filter: Box::new(inst_sorc),
                        to: "revealed".to_string(),
                    }],
                },
            ],
        }]);
    }

    // 6. Evil's Thrall: UntilEndOfPlayersNextTurn(You)
    if low
        == "if you control a villain with greater mana value than that creature, gain control of that creature until the end of your next turn instead"
    {
        let mut villain = ObjectFilterAst {
            subtypes: vec!["Villain".to_string()],
            controller: Some(ControllerRelation::You),
            ..Default::default()
        };
        villain.predicates.push(FilterPredicateAst::StatRelationTo {
            stat: StatKind::ManaValue,
            cmp: CompareOp::Gt,
            expr: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                stat: StatKind::ManaValue,
            }),
        });
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(villain),
                op: CompareOp::Ge,
                value: 1,
            }),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::GainControl {
                    controller: PlayerRefAst::You,
                    subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                    duration: DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::You),
                },
            ])],
        }]);
    }

    // 7. Knowledge Pool / Mysterious Limousine / Sasaya / Winnowing (FilterPredicateAst::Exclude, NOT EXCLUDE_SELF!)
    if low
        == "if the player does, they may cast a spell from among other cards exiled with ~ without paying its mana cost"
    {
        let mut flt = any_card_flt();
        flt.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
            SubjectRefAst::Anaphora("that exiled card".to_string()),
        )));
        return Some(vec![InstructionAst::IfPreviousSucceeded(vec![
            InstructionAst::OptionalPlayer {
                player: PlayerRefAst::ThatPlayer,
                body: vec![InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::ThatPlayer,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                    free: true,
                    duration: "now".to_string(),
                    count_limit: Some(CountLimitAst::UpTo(Box::new(ValueExprAst::Const(1)))),
                    alt_cost: None,
                }],
            },
        ])]);
    }
    if low
        == "if a creature is put into exile this way, return each other card exiled with ~ to the battlefield under its owner's control"
    {
        let mut flt = any_card_flt();
        flt.flags = ObjectFilterFlags::EXILED_WITH_SELF;
        flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
            SubjectRefAst::Anaphora("that exiled creature".to_string()),
        )));
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(obj_type(CardTypeMask::CREATURE)),
                    action: "exiled".to_string(),
                },
            )),
            then_body: vec![InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::Each(Box::new(flt))),
                from_zone: Some("exile".to_string()),
                to: "the battlefield under its owner's control".to_string(),
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
    if low
        == "add an additional one mana of any type that land produced for each other land you control with the same name as it"
    {
        let mut other_land = obj_type(CardTypeMask::LAND);
        other_land.controller = Some(ControllerRelation::You);
        other_land.named = Some(NameFilterAst::SameAs(Box::new(SubjectRefAst::Anaphora(
            "that land".to_string(),
        ))));
        other_land
            .predicates
            .push(FilterPredicateAst::Exclude(Box::new(
                SubjectRefAst::Anaphora("that land".to_string()),
            )));
        return Some(vec![InstructionAst::AddManaLandProduced {
            player: PlayerRefAst::You,
            count: Some(Box::new(ValueExprAst::Count(Box::new(other_land)))),
        }]);
    }
    if low
        == "then each player sacrifices all other creatures they control that don't share a creature type with the chosen creature they control"
    {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::ThatPlayer);
        flt.predicates.push(FilterPredicateAst::Exclude(Box::new(
            SubjectRefAst::Anaphora("the chosen creature".to_string()),
        )));
        flt.predicates.push(FilterPredicateAst::SharesPropertyWith {
            property: "creature_type".to_string(),
            reference: Box::new(SubjectRefAst::Anaphora("the chosen creature".to_string())),
            negated: true,
        });
        return Some(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::EachPlayer,
            count: None,
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }

    // 8. Akroma, Vision of Ixidor: CountKeywordsIn across all 13 keywords
    if low
        == "until end of turn, each other creature you control gets +1/+1 if it has flying, +1/+1 if it has first strike, and so on for double strike, deathtouch, haste, hexproof, indestructible, lifelink, menace, protection, reach, trample, vigilance, and partner"
    {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::You);
        flt.flags = ObjectFilterFlags::EXCLUDE_SELF;
        let kws = [
            "flying",
            "first strike",
            "double strike",
            "deathtouch",
            "haste",
            "hexproof",
            "indestructible",
            "lifelink",
            "menace",
            "protection",
            "reach",
            "trample",
            "vigilance",
            "partner",
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect();
        return Some(vec![InstructionAst::ModifyPT {
            subject: Box::new(SubjectRefAst::Each(Box::new(flt))),
            dx: "+1".to_string(),
            dy: "+1".to_string(),
            duration: DurationAst::UntilEndOfTurn,
            scale: Some(Box::new(ValueExprAst::CountKeywordsIn {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                keywords: kws,
            })),
        }]);
    }

    // 9. Super-Adaptoid
    if let Some(list_raw) = low.strip_prefix("do the same for ") {
        let norm = list_raw.replace(", and ", ", ");
        let items: Vec<String> = norm
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !items.is_empty() {
            return Some(vec![InstructionAst::RepeatProcessExt {
                times: None,
                for_list: items,
                until: None,
            }]);
        }
    }

    // 10. Wheel of Potential (both sentences, honoring EnergyPaidThisWay disambiguation + UntilEndOfPlayersNextTurn(You))
    if low
        == "each player may exile their hand and draw a number of cards equal to the amount of {e} paid this way"
    {
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::ThatPlayerHand);
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::OptionalPlayer {
                player: PlayerRefAst::ThatPlayer,
                body: vec![
                    InstructionAst::Exile {
                        actor: Some(PlayerRefAst::ThatPlayer),
                        subject: Box::new(SubjectRefAst::All(Box::new(hand))),
                        random: false,
                        face_down: false,
                        into_pile: false,
                    },
                    InstructionAst::Draw {
                        player: PlayerRefAst::ThatPlayer,
                        count: Box::new(ValueExprAst::EnergyPaidThisWay),
                        up_to: false,
                    },
                ],
            }],
        }]);
    }
    if low
        == "if seven or more {e} was paid this way, you may play cards you own exiled this way until the end of your next turn"
    {
        let mut exiled = any_card_flt();
        exiled.owner = Some(ControllerRelation::You);
        exiled
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::EnergyPaidThisWayAtLeast(
                7,
            ))),
            then_body: vec![InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: false,
                subject: Box::new(SubjectRefAst::Group(Box::new(exiled))),
                free: false,
                duration: "until the end of your next turn".to_string(),
                count_limit: None,
                alt_cost: None,
            }],
        }]);
    }

    // 11. Ensnared by the Mara (VillainousChoice with typed branches)
    if low
        == "each opponent faces a villainous choice \u{2014} they exile cards from the top of their library until they exile a nonland card, then you may cast that card without paying its mana cost, or that player exiles the top four cards of their library and ~ deals damage equal to the total mana value of those exiled cards to that player"
    {
        let mut nonland = any_card_flt();
        nonland.forbidden_types = CardTypeMask::LAND;
        let mut exiled_way = any_card_flt();
        exiled_way
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        let branch1 = vec![
            InstructionAst::ExileOrRevealTopUntil {
                exile: true,
                library: "their".to_string(),
                filter: Box::new(nonland),
            },
            InstructionAst::Optional(vec![InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: true,
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                free: true,
                duration: "now".to_string(),
                count_limit: None,
                alt_cost: None,
            }]),
        ];
        let branch2 = vec![
            InstructionAst::ExileTopLibrary {
                actor: Some(PlayerRefAst::ThatPlayer),
                whose: "their".to_string(),
                count: Box::new(ValueExprAst::Const(4)),
                face_down: false,
            },
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                amount: Box::new(ValueExprAst::AggregateStat {
                    op: crate::ast::AggregateOp::Sum,
                    stat: StatKind::ManaValue,
                    filter: Box::new(exiled_way),
                }),
            },
        ];
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::VillainousChoice {
                chooser: PlayerRefAst::EachOpponent,
                options: vec![branch1, branch2],
            },
        )]);
    }

    // 12. Nacatl War-Pride: "create X tokens that are copies of it and that are tapped and attacking, where X is the number of creatures defending player controls"
    if low
        == "create x tokens that are copies of it and that are tapped and attacking, where x is the number of creatures defending player controls"
    {
        let mut def_cr = obj_type(CardTypeMask::CREATURE);
        def_cr.controller = Some(ControllerRelation::DefendingPlayer);
        return Some(vec![InstructionAst::BindX {
            x_expr: Box::new(ValueExprAst::Count(Box::new(def_cr))),
            body: vec![InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token: TokenSpecAst::Copy {
                    count: Box::new(ValueExprAst::ChosenX),
                    source: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    tapped: true,
                    attacking: true,
                    exception: None,
                },
            }],
        }]);
    }

    // 13. Outlaw Stitcher (Max(0, spells - 1) clamp!)
    if low
        == "create a 2/2 blue and black zombie rogue creature token, then put two +1/+1 counters on that token for each spell you've cast this turn other than the first"
    {
        let scale = ValueExprAst::Max(
            Box::new(ValueExprAst::Const(0)),
            Box::new(ValueExprAst::Sub(
                Box::new(ValueExprAst::EventCountThisTurn {
                    event: "spells_cast_this_turn".to_string(),
                    player: Some(PlayerRefAst::You),
                }),
                Box::new(ValueExprAst::Const(1)),
            )),
        );
        return Some(vec![
            InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token: TokenSpecAst::Creature {
                    count: Box::new(ValueExprAst::Const(1)),
                    power: "2".to_string(),
                    toughness: "2".to_string(),
                    colors: ColorMask::BLUE.union(ColorMask::BLACK),
                    supertypes: SupertypeMask::EMPTY,
                    card_types: CardTypeMask::CREATURE,
                    subtypes: vec!["Zombie".to_string(), "Rogue".to_string()],
                    keywords: Vec::new(),
                    tapped: false,
                    attacking: false,
                    named: None,
                },
            },
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora("that token".to_string())),
                counter_kind: "+1/+1".to_string(),
                count: Box::new(ValueExprAst::Mul(
                    Box::new(ValueExprAst::Const(2)),
                    Box::new(scale),
                )),
            },
        ]);
    }

    // 14. Oversimplify
    if low
        == "each player creates a 0/0 green and blue fractal creature token and puts a number of +1/+1 counters on it equal to the total power of creatures they controlled that were exiled this way"
    {
        let mut exiled_cr = obj_type(CardTypeMask::CREATURE);
        exiled_cr.controller = Some(ControllerRelation::ThatPlayer);
        exiled_cr.flags = ObjectFilterFlags::LKI_PAST_TENSE;
        exiled_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::CreateToken {
                    controller: PlayerRefAst::ThatPlayer,
                    token: TokenSpecAst::Creature {
                        count: Box::new(ValueExprAst::Const(1)),
                        power: "0".to_string(),
                        toughness: "0".to_string(),
                        colors: ColorMask::GREEN.union(ColorMask::BLUE),
                        supertypes: SupertypeMask::EMPTY,
                        card_types: CardTypeMask::CREATURE,
                        subtypes: vec!["Fractal".to_string()],
                        keywords: Vec::new(),
                        tapped: false,
                        attacking: false,
                        named: None,
                    },
                },
                InstructionAst::PutCounters {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    counter_kind: "+1/+1".to_string(),
                    count: Box::new(ValueExprAst::AggregateStat {
                        op: crate::ast::AggregateOp::Sum,
                        stat: StatKind::Power,
                        filter: Box::new(exiled_cr),
                    }),
                },
            ],
        }]);
    }

    // 15. Fangs of Kalonia / Spectacular Showdown
    if low
        == "put a +1/+1 counter on target creature you control, then double the number of +1/+1 counters on each creature that had a +1/+1 counter put on it this way"
    {
        let mut tgt_cr = obj_type(CardTypeMask::CREATURE);
        tgt_cr.controller = Some(ControllerRelation::You);
        let mut each_cr = obj_type(CardTypeMask::CREATURE);
        each_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ReceivedCounterThisWay("+1/+1".to_string()),
            ));
        return Some(vec![
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(tgt_cr),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: "+1/+1".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
            InstructionAst::DoubleStatOrCounters {
                subject: Box::new(SubjectRefAst::Each(Box::new(each_cr))),
                kind: "+1/+1 counters".to_string(),
                duration: DurationAst::Instant,
            },
        ]);
    }
    if low
        == "put a double strike counter on target creature, then goad each creature that had a double strike counter put on it this way"
    {
        let mut each_cr = obj_type(CardTypeMask::CREATURE);
        each_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ReceivedCounterThisWay("double strike".to_string()),
            ));
        return Some(vec![
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(obj_type(CardTypeMask::CREATURE)),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: "double strike".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
            InstructionAst::CombatRuleMod {
                subject: Box::new(SubjectRefAst::Each(Box::new(each_cr))),
                rule: "goad".to_string(),
                duration: DurationAst::UntilYourNextTurn,
            },
        ]);
    }

    // 16. Kaya's Wrath / Over the Top / Warp World / Cryptic Gateway / Queen Kayla
    if low
        == "you gain life equal to the number of creatures you controlled that were destroyed this way"
    {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::You);
        flt.flags = ObjectFilterFlags::LKI_PAST_TENSE;
        flt.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DestroyedThisWay,
            ));
        return Some(vec![InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta: Box::new(ValueExprAst::Count(Box::new(flt))),
            gain: true,
        }]);
    }
    if low
        == "each player reveals a number of cards from the top of their library equal to the number of nonland permanents they control, puts all permanent cards they revealed this way onto the battlefield, and puts the rest into their graveyard"
    {
        let mut nonland = any_perm();
        nonland.forbidden_types = CardTypeMask::LAND;
        nonland.controller = Some(ControllerRelation::ThatPlayer);
        let mut rev_perm = card_type(CardTypeMask::PERMANENT);
        rev_perm
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::LookTopLibrary {
                    reveal: true,
                    library: "their".to_string(),
                    count: Box::new(ValueExprAst::Count(Box::new(nonland))),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(rev_perm))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                    to: "their graveyard".to_string(),
                },
            ],
        }]);
    }
    if low
        == "each player puts all artifact, creature, and land cards revealed this way onto the battlefield, then does the same for enchantment cards, then puts all cards revealed this way that weren't put onto the battlefield on the bottom of their library"
    {
        let mut acl = card_type(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        );
        acl.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        let mut ench = card_type(CardTypeMask::ENCHANTMENT);
        ench.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::RevealedThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(acl))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::All(Box::new(ench))),
                    to: "the battlefield".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
                    to: "bottom of their library".to_string(),
                },
            ],
        }]);
    }
    if low
        == "you may put a creature card from your hand that shares a creature type with each creature tapped this way onto the battlefield"
    {
        let mut tapped_cr = obj_type(CardTypeMask::CREATURE);
        tapped_cr
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::TappedThisWay,
            ));
        let mut hand_cr = card_type(CardTypeMask::CREATURE);
        hand_cr.zone = Some(ZoneFilter::YourHand);
        hand_cr
            .predicates
            .push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Each(Box::new(tapped_cr))),
                negated: false,
            });
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(hand_cr))),
                to: "the battlefield".to_string(),
            },
        ])]);
    }
    if low
        == "you may choose an artifact or creature card with mana value 1 you discarded this way, then do the same for artifact or creature cards with mana values 2 and 3"
    {
        let mk = |mv| {
            let mut f = card_type(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE));
            f.stat_bounds.push(StatBoundAst {
                stat: StatKind::ManaValue,
                op: CompareOp::Eq,
                value: Box::new(ValueExprAst::Const(mv)),
            });
            f.predicates.push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::DiscardedThisWay,
            ));
            f
        };
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::ChoosePerFilter {
                player: PlayerRefAst::You,
                categories: vec![mk(1), mk(2), mk(3)],
                sacrifice_rest: false,
            },
        ])]);
    }

    // 17. Spy Network / Dream Cache / Decree of Annihilation / Sway of the Stars / Remove Enchantments
    if low
        == "look at target player's hand, the top card of that player's library, and any face-down creatures they control"
    {
        let mut fd = obj_type(CardTypeMask::CREATURE);
        fd.controller = Some(ControllerRelation::ThatPlayer);
        fd.flags = ObjectFilterFlags::FACE_DOWN;
        return Some(vec![InstructionAst::LookAt {
            player: PlayerRefAst::You,
            targets: vec![
                SubjectRefAst::Anaphora("target player's hand".to_string()),
                SubjectRefAst::Anaphora("the top card of that player's library".to_string()),
                SubjectRefAst::All(Box::new(fd)),
            ],
        }]);
    }
    if low
        == "draw three cards, then put two cards from your hand both on top of your library or both on the bottom of your library"
    {
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::YourHand);
        let mk_move = |dest: &str| {
            vec![InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::You),
                count: Some(Box::new(ValueExprAst::Const(2))),
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Group(Box::new(hand.clone()))),
                to: dest.to_string(),
            }]
        };
        return Some(vec![
            InstructionAst::Draw {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(3)),
                up_to: false,
            },
            InstructionAst::Choice(vec![
                mk_move("top of your library"),
                mk_move("bottom of your library"),
            ]),
        ]);
    }
    if low
        == "exile all artifacts, creatures, and lands from the battlefield, all cards from all graveyards, and all cards from all hands"
    {
        let mut bf = obj_type(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::CREATURE)
                .union(CardTypeMask::LAND),
        );
        bf.zone = Some(ZoneFilter::Battlefield);
        let mut gys = any_card_flt();
        gys.zone = Some(ZoneFilter::AllGraveyards);
        let mut hands = any_card_flt();
        hands.zone = Some(ZoneFilter::AnyHand);
        return Some(vec![InstructionAst::Exile {
            actor: Some(PlayerRefAst::You),
            subject: Box::new(SubjectRefAst::Compound(vec![
                SubjectRefAst::All(Box::new(bf)),
                SubjectRefAst::All(Box::new(gys)),
                SubjectRefAst::All(Box::new(hands)),
            ])),
            random: false,
            face_down: false,
            into_pile: false,
        }]);
    }
    if low
        == "each player shuffles their hand, graveyard, and all permanents they own into their library, then draws seven cards"
    {
        let mut owned = any_perm();
        owned.owner = Some(ControllerRelation::ThatPlayer);
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::ThatPlayerHand);
        let mut gy = any_card_flt();
        gy.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::ShuffleIntoLibrary {
                    subject: Box::new(SubjectRefAst::Compound(vec![
                        SubjectRefAst::All(Box::new(hand)),
                        SubjectRefAst::All(Box::new(gy)),
                        SubjectRefAst::All(Box::new(owned)),
                    ])),
                },
                InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: Box::new(ValueExprAst::Const(7)),
                    up_to: false,
                },
            ],
        }]);
    }
    if low
        == "then destroy all other enchantments you control, all other auras attached to permanents you control, and all other auras attached to attacking creatures your opponents control"
    {
        let mut ench = obj_type(CardTypeMask::ENCHANTMENT);
        ench.controller = Some(ControllerRelation::You);
        ench.flags = ObjectFilterFlags::EXCLUDE_SELF;
        let mut aura1 = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            flags: ObjectFilterFlags::EXCLUDE_SELF,
            ..Default::default()
        };
        let mut p_you = any_perm();
        p_you.controller = Some(ControllerRelation::You);
        aura1
            .predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::All(Box::new(p_you)),
            )));
        let mut aura2 = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            flags: ObjectFilterFlags::EXCLUDE_SELF,
            ..Default::default()
        };
        let mut atk_opp = obj_type(CardTypeMask::CREATURE);
        atk_opp.combat = Some(CombatState::Attacking);
        atk_opp.controller = Some(ControllerRelation::Opponents);
        aura2
            .predicates
            .push(FilterPredicateAst::AttachedTo(Box::new(
                SubjectRefAst::All(Box::new(atk_opp)),
            )));
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(SubjectRefAst::Compound(vec![
                SubjectRefAst::All(Box::new(ench)),
                SubjectRefAst::All(Box::new(aura1)),
                SubjectRefAst::All(Box::new(aura2)),
            ])),
            random: false,
            except: None,
        }]);
    }

    // 18. Mercurial Transformation / Navigator's Compass / Phantasmal Form / Zur, Eternal Schemer
    if low
        == "until end of turn, target nonland permanent loses all abilities and becomes your choice of a blue frog creature with base power and toughness 1/1 or a blue octopus creature with base power and toughness 4/4"
    {
        let mut nl = any_perm();
        nl.forbidden_types = CardTypeMask::LAND;
        let tgt = SubjectRefAst::TargetObject {
            filter: Box::new(nl),
            count: TargetCountAst::Exact(1),
        };
        return Some(vec![
            InstructionAst::RemoveAllAbilities {
                subject: Box::new(tgt),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::Choice(vec![
                vec![InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    power: "1".to_string(),
                    toughness: "1".to_string(),
                    description: Some("blue Frog creature".to_string()),
                    keywords: Vec::new(),
                    duration: DurationAst::UntilEndOfTurn,
                }],
                vec![InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    power: "4".to_string(),
                    toughness: "4".to_string(),
                    description: Some("blue Octopus creature".to_string()),
                    keywords: Vec::new(),
                    duration: DurationAst::UntilEndOfTurn,
                }],
            ]),
        ]);
    }
    if low
        == "until end of turn, target land you control becomes the basic land type of your choice in addition to its other types"
    {
        let mut land = obj_type(CardTypeMask::LAND);
        land.controller = Some(ControllerRelation::You);
        return Some(vec![
            InstructionAst::BecomeChosenCharacteristic {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(land),
                    count: TargetCountAst::Exact(1),
                }),
                kind: "basic_land_type".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::KeepExistingTypes("in addition to its other types".to_string()),
        ]);
    }
    if low
        == "until end of turn, up to two target creatures each have base power and toughness 3/3, gain flying, and become blue illusions in addition to their other colors and types"
    {
        let tgt = SubjectRefAst::TargetObject {
            filter: Box::new(obj_type(CardTypeMask::CREATURE)),
            count: TargetCountAst::UpTo(2),
        };
        return Some(vec![
            InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(tgt),
                power: "3".to_string(),
                toughness: "3".to_string(),
                description: Some("blue Illusion".to_string()),
                keywords: vec!["flying".to_string()],
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::KeepExistingTypes(
                "in addition to their other colors and types".to_string(),
            ),
        ]);
    }
    if low
        == "target non-aura enchantment you control becomes a creature in addition to its other types and has base power and base toughness each equal to its mana value"
    {
        let mut ench = obj_type(CardTypeMask::ENCHANTMENT);
        ench.forbidden_subtypes.push("Aura".to_string());
        ench.controller = Some(ControllerRelation::You);
        let mv = ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: StatKind::ManaValue,
        };
        return Some(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(SubjectRefAst::TargetObject {
                        filter: Box::new(ench),
                        count: TargetCountAst::Exact(1),
                    }),
                    type_or_subtype: "creature".to_string(),
                    additive: true,
                    condition: None,
                }),
                duration: DurationAst::Permanent,
            },
            InstructionAst::SetDynamicPT {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                power: Some(Box::new(mv.clone())),
                toughness: Some(Box::new(mv)),
                duration: DurationAst::Permanent,
            },
        ]);
    }

    // 19. Guff Rewrites History / Mysterious Stranger / Vastwood Hydra / Furygale Flocking / Linessa
    if low == "those permanents' owners shuffle them into their libraries" {
        return Some(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(SubjectRefAst::Anaphora("those permanents".to_string())),
        }]);
    }
    if low == "each player may cast the nonland card they exiled without paying its mana cost" {
        let mut nl = any_card_flt();
        nl.forbidden_types = CardTypeMask::LAND;
        nl.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::OptionalPlayer {
                player: PlayerRefAst::ThatPlayer,
                body: vec![InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::ThatPlayer,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Group(Box::new(nl))),
                    free: true,
                    duration: "now".to_string(),
                    count_limit: None,
                    alt_cost: None,
                }],
            }],
        }]);
    }
    if low
        == "for each graveyard with an instant or sorcery card in it, exile target instant or sorcery card from that graveyard"
    {
        let mut is_gy = card_type(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY));
        is_gy.zone = Some(ZoneFilter::ThatPlayerGraveyard);
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachPlayer,
                condition: Box::new(ConditionAst::ZoneObjectCount {
                    filter: Box::new(is_gy.clone()),
                    op: CompareOp::Ge,
                    value: 1,
                }),
            }),
            body: vec![InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(is_gy),
                    count: TargetCountAst::Exact(1),
                }),
                random: false,
                face_down: false,
                into_pile: false,
            }],
        }]);
    }
    if low
        == "you may distribute a number of +1/+1 counters equal to the number of +1/+1 counters on ~ among any number of creatures you control"
    {
        let mut cr_you = obj_type(CardTypeMask::CREATURE);
        cr_you.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::DistributeCounters {
                counter_kind: "+1/+1".to_string(),
                count: Box::new(ValueExprAst::CounterCount {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "+1/+1".to_string(),
                }),
                targets: Box::new(SubjectRefAst::Group(Box::new(cr_you))),
            },
        ])]);
    }
    if low
        == "for each opponent, create two 3/3 blue and red elemental creature tokens with flying that attack that opponent this turn if able"
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body: vec![
                InstructionAst::CreateToken {
                    controller: PlayerRefAst::You,
                    token: TokenSpecAst::Creature {
                        count: Box::new(ValueExprAst::Const(2)),
                        power: "3".to_string(),
                        toughness: "3".to_string(),
                        colors: ColorMask::BLUE.union(ColorMask::RED),
                        supertypes: SupertypeMask::EMPTY,
                        card_types: CardTypeMask::CREATURE,
                        subtypes: vec!["Elemental".to_string()],
                        keywords: vec!["flying".to_string()],
                        tapped: false,
                        attacking: false,
                        named: None,
                    },
                },
                InstructionAst::CombatRuleMod {
                    subject: Box::new(SubjectRefAst::Anaphora("those tokens".to_string())),
                    rule: "attack_that_opponent_this_turn_if_able".to_string(),
                    duration: DurationAst::UntilEndOfTurn,
                },
            ],
        }]);
    }
    if low
        == "target player returns a creature they control to its owner's hand, then repeats this process for an artifact, an enchantment, and a land"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.controller = Some(ControllerRelation::ThatPlayer);
        return Some(vec![
            InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(cr))),
                from_zone: None,
                to: "its owner's hand".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: Some(Box::new(ValueExprAst::Const(1))),
                up_to: false,
                any_number: false,
                actor: Some(PlayerRefAst::TargetPlayer),
            },
            InstructionAst::RepeatProcessExt {
                times: None,
                for_list: vec![
                    "an artifact".to_string(),
                    "an enchantment".to_string(),
                    "a land".to_string(),
                ],
                until: None,
            },
        ]);
    }

    // 20. Admiral Beckett Brass / Gollum, Obsessed Stalker / Scythe Specter / Azra Bladeseeker / Malignant Growth
    if low
        == "gain control of target nonland permanent controlled by a player who was dealt combat damage by three or more pirates this turn"
    {
        let mut nl = any_perm();
        nl.forbidden_types = CardTypeMask::LAND;
        nl.controller = Some(ControllerRelation::ThatPlayer);
        let pirate = ObjectFilterAst {
            subtypes: vec!["Pirate".to_string()],
            ..Default::default()
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                player: PlayerRefAst::ThatPlayer,
                source_filter: Box::new(pirate),
                min_sources: 3,
                combat_only: true,
                this_game: false,
            })),
            then_body: vec![InstructionAst::GainControl {
                controller: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(nl),
                    count: TargetCountAst::Exact(1),
                }),
                duration: DurationAst::Permanent,
            }],
        }]);
    }
    if low
        == "each opponent dealt combat damage this game by a creature named ~ loses life equal to the amount of life you gained this turn"
    {
        let mut self_cr = obj_type(CardTypeMask::CREATURE);
        self_cr.named = Some(NameFilterAst::SelfName);
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachOpponent,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerDealtDamageBy {
                    player: PlayerRefAst::ThatPlayer,
                    source_filter: Box::new(self_cr),
                    min_sources: 1,
                    combat_only: true,
                    this_game: true,
                })),
            }),
            body: vec![InstructionAst::ChangeLife {
                player: PlayerRefAst::ThatPlayer,
                delta: Box::new(ValueExprAst::LifeChangedThisTurn {
                    player: PlayerRefAst::You,
                    gained: true,
                }),
                gain: false,
            }],
        }]);
    }
    if low
        == "each player who discarded a card with the greatest mana value among cards discarded this way loses life equal to that mana value"
    {
        return Some(vec![InstructionAst::ForEachSubjectPlayer {
            players: Box::new(SubjectRefAst::PlayerMatching {
                player: PlayerRefAst::EachPlayer,
                condition: Box::new(ConditionAst::Marker(GameMarker::PlayerActionExtremeStat {
                    player: PlayerRefAst::ThatPlayer,
                    action: PriorActionRefKind::DiscardedThisWay,
                    stat: StatKind::ManaValue,
                    greatest: true,
                    allow_tie: true,
                })),
            }),
            body: vec![InstructionAst::ChangeLife {
                player: PlayerRefAst::ThatPlayer,
                delta: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                    stat: StatKind::ManaValue,
                }),
                gain: false,
            }],
        }]);
    }
    if low
        == "each player on your team may discard a card, then each player who discarded a card this way draws a card"
    {
        return Some(vec![
            InstructionAst::ForEachPlayer {
                player: PlayerRefAst::EachPlayerOnYourTeam,
                body: vec![InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::ThatPlayer,
                    body: vec![InstructionAst::Discard {
                        player: PlayerRefAst::ThatPlayer,
                        count: Some(Box::new(ValueExprAst::Const(1))),
                        filter: None,
                        anaphora: None,
                        random: false,
                    }],
                }],
            },
            InstructionAst::ForEachSubjectPlayer {
                players: Box::new(SubjectRefAst::PlayerMatching {
                    player: PlayerRefAst::EachPlayerOnYourTeam,
                    condition: Box::new(ConditionAst::Marker(
                        GameMarker::ZoneActionHappenedThisWay {
                            filter: Box::new(any_card_flt()),
                            action: "discarded".to_string(),
                        },
                    )),
                }),
                body: vec![InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: Box::new(ValueExprAst::Const(1)),
                    up_to: false,
                }],
            },
        ]);
    }
    if low
        == "that player draws an additional card for each growth counter on ~, then ~ deals damage to the player equal to the number of cards they drew this way"
    {
        return Some(vec![
            InstructionAst::Draw {
                player: PlayerRefAst::ThatPlayer,
                count: Box::new(ValueExprAst::CounterCount {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "growth".to_string(),
                }),
                up_to: false,
            },
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                amount: Box::new(ValueExprAst::CountDrawnThisWay(PlayerRefAst::ThatPlayer)),
            },
        ]);
    }

    // 21. Flaming Gambit / Boreas Charger / Bruna / Hama / The Beamtown Bullies / Jurin / Recursive Recruitment / Quakebringer / The Wedding of River Song / Old Man of the Sea / Blim / Tunnel Vision / Alania
    if low
        == "that player or that planeswalker's controller may choose a creature they control and have ~ deal that damage to it instead"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.controller = Some(ControllerRelation::ThatPlayerOrPlaneswalkerController);
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::ThatPlayerOrPlaneswalkerController,
            body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::DealDamage {
                    source: Box::new(SubjectRefAst::SelfObject),
                    target: Box::new(SubjectRefAst::Group(Box::new(cr))),
                    amount: Box::new(ValueExprAst::PriorResult),
                },
            ])],
        }]);
    }
    if low
        == "search your library for a number of plains cards equal to the difference, reveal those cards, put one of them onto the battlefield tapped and the rest into your hand, then shuffle"
    {
        let plains = ObjectFilterAst {
            subtypes: vec!["Plains".to_string()],
            any_card: true,
            ..Default::default()
        };
        return Some(vec![InstructionAst::SearchLibrary {
            library: "your".to_string(),
            count: Box::new(ValueExprAst::PriorResult),
            up_to: false,
            any_number: false,
            exact: false,
            filter: Box::new(plains),
            reveal: true,
            to_zone: "follow_up".to_string(),
            shuffle: true,
            extra_zones: Vec::new(),
            secondary_filter: None,
            secondary_is_or: false,
            where_x: None,
            rest_to_zone: Some("your hand".to_string()),
            modifier: None,
            follow_up: vec![InstructionAst::ChooseAndKeep {
                pick: "one of them".to_string(),
                keep_to: "the battlefield tapped".to_string(),
                rest_to: "your hand".to_string(),
            }],
        }]);
    }
    if low
        == "you may attach to it any number of auras on the battlefield and you may put onto the battlefield attached to it any number of aura cards that could enchant it from your graveyard and/or hand"
    {
        let mut bf_aura = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            zone: Some(ZoneFilter::Battlefield),
            ..Default::default()
        };
        bf_aura
            .predicates
            .push(FilterPredicateAst::CouldAttachTo(Box::new(
                SubjectRefAst::Anaphora("it".to_string()),
            )));
        let mut gy_hand_aura = ObjectFilterAst {
            subtypes: vec!["Aura".to_string()],
            any_card: true,
            zone: Some(ZoneFilter::YourHandOrGraveyard),
            ..Default::default()
        };
        gy_hand_aura
            .predicates
            .push(FilterPredicateAst::CouldAttachTo(Box::new(
                SubjectRefAst::Anaphora("it".to_string()),
            )));
        return Some(vec![
            InstructionAst::Optional(vec![InstructionAst::Attach {
                subject: Box::new(SubjectRefAst::Group(Box::new(bf_aura))),
                target: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                any_number: true,
                all: false,
            }]),
            InstructionAst::Optional(vec![InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(gy_hand_aura))),
                from_zone: Some("your graveyard and/or hand".to_string()),
                to: "the battlefield".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                random: false,
                except: None,
                count: None,
                up_to: false,
                any_number: true,
                actor: Some(PlayerRefAst::You),
            }]),
        ]);
    }
    if low
        == "for as long as you control ~, you may cast the exiled card during your turn by waterbending {x} rather than paying its mana cost, where x is its mana value"
    {
        return Some(vec![InstructionAst::ForAsLongAs {
            condition: Box::new(ConditionAst::Marker(GameMarker::ControlsSelf)),
            body: vec![InstructionAst::BindX {
                x_expr: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("the exiled card".to_string())),
                    stat: StatKind::ManaValue,
                }),
                body: vec![InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Anaphora("the exiled card".to_string())),
                    free: false,
                    duration: "during your turn".to_string(),
                    count_limit: None,
                    alt_cost: Some(AltCastCostAst::PayCost(vec![CostAtomAst::Waterbend(
                        "{X}".to_string(),
                    )])),
                }],
            }],
        }]);
    }
    if low
        == "target opponent whose turn it is puts target nonlegendary creature card from your graveyard onto the battlefield under their control"
    {
        let mut cr = card_type(CardTypeMask::CREATURE);
        cr.forbidden_supertypes = SupertypeMask::LEGENDARY;
        cr.zone = Some(ZoneFilter::YourGraveyard);
        return Some(vec![InstructionAst::ReturnZone {
            subject: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(cr),
                count: TargetCountAst::Exact(1),
            }),
            from_zone: Some("your graveyard".to_string()),
            to: "the battlefield under their control".to_string(),
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: Some(PlayerRefAst::TargetOpponentWhoseTurnItIs),
        }]);
    }
    if low
        == "each creature you control attacking a player gets +1/+0 until end of turn for each creature that player controls"
    {
        let mut atk = obj_type(CardTypeMask::CREATURE);
        atk.controller = Some(ControllerRelation::You);
        atk.predicates.push(FilterPredicateAst::AttackingTarget {
            target: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
            this_turn: false,
        });
        let mut def_cr = obj_type(CardTypeMask::CREATURE);
        def_cr.controller = Some(ControllerRelation::ThatPlayer);
        return Some(vec![InstructionAst::ModifyPT {
            subject: Box::new(SubjectRefAst::Each(Box::new(atk))),
            dx: "+1".to_string(),
            dy: "+0".to_string(),
            duration: DurationAst::UntilEndOfTurn,
            scale: Some(Box::new(ValueExprAst::Count(Box::new(def_cr)))),
        }]);
    }
    if low
        == "if ~ was cast from a graveyard, put a +1/+1 counter on each of them for every three cards in your graveyard"
    {
        let mut gy = any_card_flt();
        gy.zone = Some(ZoneFilter::YourGraveyard);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::CastFromGraveyard)),
            then_body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora("each of them".to_string())),
                counter_kind: "+1/+1".to_string(),
                count: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::Count(Box::new(gy))),
                    denom: 3,
                    rounding: Rounding::Down,
                }),
            }],
        }]);
    }
    if low
        == "draw two cards, then you may exile a nonland card from your hand with a number of time counters on it equal to its mana value"
    {
        let mut nl = any_card_flt();
        nl.forbidden_types = CardTypeMask::LAND;
        nl.zone = Some(ZoneFilter::YourHand);
        return Some(vec![
            InstructionAst::Draw {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(2)),
                up_to: false,
            },
            InstructionAst::Optional(vec![InstructionAst::ExileWithCounters {
                subject: Box::new(SubjectRefAst::Group(Box::new(nl))),
                counter_kind: "time".to_string(),
                count: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::ManaValue,
                }),
                face_down: false,
            }]),
        ]);
    }
    if low
        == "gain control of target creature with power less than or equal to ~'s power for as long as ~ remains tapped and that creature's power remains less than or equal to ~'s power"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.stat_bounds.push(StatBoundAst {
            stat: StatKind::Power,
            op: CompareOp::Le,
            value: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::SelfObject),
                stat: StatKind::Power,
            }),
        });
        let cond = parse_condition(
            "~ remains tapped and that creature's power remains less than or equal to ~'s power",
            vocab,
        )
        .ok()?;
        return Some(vec![InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(cr),
                count: TargetCountAst::Exact(1),
            }),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        }]);
    }
    if low
        == "then each player loses life and discards cards equal to the number of permanents they control but don't own"
    {
        let mut flt = any_perm();
        flt.controller = Some(ControllerRelation::ThatPlayer);
        flt.predicates
            .push(FilterPredicateAst::NotOwnedByController);
        let cnt = ValueExprAst::Count(Box::new(flt));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    delta: Box::new(cnt.clone()),
                    gain: false,
                },
                InstructionAst::Discard {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(cnt)),
                    filter: None,
                    anaphora: None,
                    random: false,
                },
            ],
        }]);
    }
    if low
        == "if it is, that player puts the rest of the revealed cards into their graveyard and puts the card with the chosen name on top of their library"
    {
        let mut named = any_card_flt();
        named.named = Some(NameFilterAst::SameAsReferenced);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::SameNameAsTarget)),
            then_body: vec![
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora(
                        "the rest of the revealed cards".to_string(),
                    )),
                    to: "their graveyard".to_string(),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Group(Box::new(named))),
                    to: "top of their library".to_string(),
                },
            ],
        }]);
    }
    if low
        == "if it's the first instant spell, the first sorcery spell, or the first otter spell other than ~ you've cast this turn, you may have target opponent draw a card"
    {
        let cond = parse_condition(
            "it's the first instant spell, the first sorcery spell, or the first otter spell other than ~ you've cast this turn",
            vocab,
        )
        .ok()?;
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: vec![InstructionAst::Optional(vec![InstructionAst::Draw {
                player: PlayerRefAst::TargetOpponent,
                count: Box::new(ValueExprAst::Const(1)),
                up_to: false,
            }])],
        }]);
    }
    if low
        == "if you do, you may cast that card without paying its mana cost if the two spells have the same mana value"
    {
        return Some(vec![InstructionAst::IfPreviousSucceeded(vec![
            InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::Marker(
                    GameMarker::SameManaValueAsTriggerSpell,
                )),
                then_body: vec![InstructionAst::Optional(vec![
                    InstructionAst::GrantCastPermission {
                        player: PlayerRefAst::You,
                        cast_only: true,
                        subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                        free: true,
                        duration: "now".to_string(),
                        count_limit: None,
                        alt_cost: None,
                    },
                ])],
            },
        ])]);
    }
    if low
        == "if you control a gideon planeswalker, that player sacrifices two of those creatures of their choice instead"
    {
        let gideon = ObjectFilterAst {
            any_types: CardTypeMask::PLANESWALKER,
            subtypes: vec!["Gideon".to_string()],
            controller: Some(ControllerRelation::You),
            ..Default::default()
        };
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(gideon),
                op: CompareOp::Ge,
                value: 1,
            }),
            then_body: vec![InstructionAst::ReplaceEffect(vec![
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: Some(Box::new(ValueExprAst::Const(2))),
                    subject: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
                    except: None,
                    half: false,
                    rounding: None,
                    chooser: Some(PlayerRefAst::ThatPlayer),
                },
            ])],
        }]);
    }
    if low
        == "if its controller has more than four cards in hand, they exile cards from their hand equal to the difference"
    {
        let mut hand = any_card_flt();
        hand.zone = Some(ZoneFilter::ThatPlayerHand);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::HandCount {
                player: PlayerRefAst::ItsController,
                op: CompareOp::Gt,
                value: 4,
            }),
            then_body: vec![InstructionAst::MoveCounted {
                actor: Some(PlayerRefAst::ItsController),
                count: Some(Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::PlayerStat {
                        player: PlayerRefAst::ItsController,
                        stat: PlayerStatKind::CardsInHand,
                    }),
                    Box::new(ValueExprAst::Const(4)),
                ))),
                up_to: false,
                any_number: false,
                subject: Box::new(SubjectRefAst::Group(Box::new(hand))),
                to: "exile".to_string(),
            }],
        }]);
    }
    if low
        == "put a counter of that kind on target permanent you control if it doesn't have a counter of that kind on it"
    {
        let mut perm = any_perm();
        perm.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Not(Box::new(ConditionAst::CounterCompare {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "same".to_string(),
                op: CompareOp::Ge,
                value: 1,
            }))),
            then_body: vec![InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(perm),
                    count: TargetCountAst::Exact(1),
                }),
                counter_kind: "same".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            }],
        }]);
    }
    if low
        == "if ~'s additional cost was paid, this effect doesn't affect combat damage that would be dealt by red creatures"
    {
        return Some(vec![InstructionAst::IfCondition {
            condition: Box::new(ConditionAst::Marker(GameMarker::AdditionalCostPaid {
                description: None,
            })),
            then_body: vec![InstructionAst::CombatRuleMod {
                subject: Box::new(SubjectRefAst::All(Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    any_colors: ColorMask::RED,
                    ..Default::default()
                }))),
                rule: "exclude_combat_damage_from_prevention".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            }],
        }]);
    }
    if let Some(after) = low.strip_prefix("this ability triggers only if ")
        && let Ok(cond) = parse_condition(after.trim(), vocab)
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "triggers_only_if".to_string(),
            condition: Some(Box::new(cond)),
        }]);
    }
    if low
        == "you may put a creature card you own with lesser mana value from your hand or from the command zone onto the battlefield"
    {
        let mut flt = card_type(CardTypeMask::CREATURE);
        flt.owner = Some(ControllerRelation::You);
        flt.zone = Some(ZoneFilter::YourHand);
        flt.extra_zones.push(ZoneFilter::CommandZone);
        flt.predicates.push(FilterPredicateAst::StatRelationTo {
            stat: StatKind::ManaValue,
            cmp: CompareOp::Lt,
            expr: Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                stat: StatKind::ManaValue,
            }),
        });
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                to: "the battlefield".to_string(),
            },
        ])]);
    }
    if low
        == "put all commanders you own from the command zone and from your graveyard into your hand"
    {
        let mut flt = any_card_flt();
        flt.flags = ObjectFilterFlags::IS_COMMANDER;
        flt.owner = Some(ControllerRelation::You);
        flt.zone = Some(ZoneFilter::CommandZone);
        flt.extra_zones.push(ZoneFilter::YourGraveyard);
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            to: "your hand".to_string(),
        }]);
    }
    if low == "creatures the active player controls attack this turn if able" {
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.controller = Some(ControllerRelation::ActivePlayer);
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            rule: "attacks_this_turn_if_able".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    // Magus of the Jar / Memory Jar: inner body (when DelayedTrigger already peeled the prefix)
    if low
        == "each player discards their hand and returns to their hand each card they exiled this way"
    {
        let mut exiled = any_card_flt();
        exiled
            .predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::ExiledThisWay,
            ));
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![
                InstructionAst::DiscardHand {
                    player: PlayerRefAst::ThatPlayer,
                },
                InstructionAst::ReturnZone {
                    subject: Box::new(SubjectRefAst::Each(Box::new(exiled))),
                    from_zone: Some("exile".to_string()),
                    to: "their hand".to_string(),
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
            ],
        }]);
    }

    // Jabari's Influence
    if low
        == "gain control of target nonartifact, nonblack creature that attacked you this turn and put a -1/-0 counter on it"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.forbidden_types = CardTypeMask::ARTIFACT;
        cr.forbidden_colors = ColorMask::BLACK;
        cr.predicates.push(FilterPredicateAst::AttackingTarget {
            target: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            this_turn: true,
        });
        return Some(vec![
            InstructionAst::GainControl {
                controller: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::TargetObject {
                    filter: Box::new(cr),
                    count: TargetCountAst::Exact(1),
                }),
                duration: DurationAst::Permanent,
            },
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "-1/-0".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
        ]);
    }

    // Spoils of the Hunt
    if low
        == "target creature you control gets +1/+0 until end of turn for each mana from a treasure that was spent to cast ~"
    {
        let mut cr = obj_type(CardTypeMask::CREATURE);
        cr.controller = Some(ControllerRelation::You);
        return Some(vec![InstructionAst::ModifyPT {
            subject: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(cr),
                count: TargetCountAst::Exact(1),
            }),
            dx: "+1".to_string(),
            dy: "+0".to_string(),
            duration: DurationAst::UntilEndOfTurn,
            scale: Some(Box::new(ValueExprAst::ManaSpentToCast {
                mana_filter: Some("from_treasure".to_string()),
            })),
        }]);
    }

    // Radagast the Brown
    if low
        == "you may reveal a creature card that doesn't share a creature type with a creature you control from among those cards and put it into your hand"
    {
        let mut your_cr = obj_type(CardTypeMask::CREATURE);
        your_cr.controller = Some(ControllerRelation::You);
        let mut cr_card = card_type(CardTypeMask::CREATURE);
        cr_card
            .predicates
            .push(FilterPredicateAst::SharesPropertyWith {
                property: "creature_type".to_string(),
                reference: Box::new(SubjectRefAst::Group(Box::new(your_cr))),
                negated: true,
            });
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::RevealFromLooked {
                filter: Box::new(cr_card),
                to: "your hand".to_string(),
            },
        ])]);
    }

    // Put a card you put into your graveyard this way into your hand
    if low == "put a card you put into your graveyard this way into your hand" {
        let mut flt = any_card_flt();
        flt.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::PutIntoGraveyardThisWay,
            ));
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            to: "your hand".to_string(),
        }]);
    }

    // Generic multi-sentence fallback inside Wave E6 if a compound sentence wasn't caught earlier
    if low.starts_with("if ")
        && let Some((cond_raw, rest)) = low[3..].split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let (body, fails) = parse_effect_body(rest.trim(), vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    None
}
