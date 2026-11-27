use super::*;

pub(in super::super) fn trigger_subject_is_self(trig: &TriggerSpecAst) -> bool {
    !trig.events.is_empty()
        && trig.events.iter().all(|ev| match ev {
            TriggerEventAst::ZoneChange { subject, .. }
            | TriggerEventAst::AttacksWithOthers { subject, .. }
            | TriggerEventAst::AttacksUnblocked { subject }
            | TriggerEventAst::Blocks { subject, .. }
            | TriggerEventAst::BecomesBlocked { subject, .. }
            | TriggerEventAst::TapStateChanged { subject, .. }
            | TriggerEventAst::TurnedFaceUp { subject }
            | TriggerEventAst::Transforms { subject }
            | TriggerEventAst::PhaseChanged { subject, .. }
            | TriggerEventAst::Mutates { subject }
            | TriggerEventAst::BecomesTargeted { subject, .. }
            | TriggerEventAst::CountersChanged { subject, .. }
            | TriggerEventAst::Cycled { subject }
            | TriggerEventAst::Exploits { subject }
            | TriggerEventAst::BecomesAttached { subject, .. }
            | TriggerEventAst::Exerted { subject }
            | TriggerEventAst::Crewed { subject }
            | TriggerEventAst::Explores { subject }
            | TriggerEventAst::Connives { subject }
            | TriggerEventAst::BecomesMonstrous { subject }
            | TriggerEventAst::BecomesRenowned { subject }
            | TriggerEventAst::Exiled { subject } => {
                matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            }
            TriggerEventAst::Attacks { subject, .. } => {
                matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            TriggerEventAst::SpellCast { targets, .. } => {
                matches!(targets.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            TriggerEventAst::DamageDealt { source, .. } => {
                matches!(source.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            _ => false,
        })
}

pub(in super::super) fn is_self_pronoun(
    subject: &SubjectRefAst,
    builder: &BodyBuilder,
    self_pronoun_valid: bool,
) -> bool {
    match subject {
        SubjectRefAst::SelfObject => true,
        SubjectRefAst::Anaphora(p) if self_pronoun_valid && builder.target == BodyTarget::None => {
            matches!(
                p.to_ascii_lowercase().as_str(),
                "it" | "him"
                    | "her"
                    | "he"
                    | "she"
                    | "this creature"
                    | "this permanent"
                    | "this artifact"
                    | "this enchantment"
                    | "this land"
            )
        }
        _ => false,
    }
}

pub(in super::super) fn lower_comparison(op: CompareOp) -> Comparison {
    match op {
        CompareOp::Eq => Comparison::Equal,
        CompareOp::Lt => Comparison::Less,
        CompareOp::Le => Comparison::LessOrEqual,
        CompareOp::Gt => Comparison::Greater,
        CompareOp::Ge => Comparison::GreaterOrEqual,
    }
}

pub(in super::super) fn negate_comparison(c: Comparison) -> Comparison {
    match c {
        Comparison::Equal => Comparison::NotEqual,
        Comparison::NotEqual => Comparison::Equal,
        Comparison::Less => Comparison::GreaterOrEqual,
        Comparison::LessOrEqual => Comparison::Greater,
        Comparison::Greater => Comparison::LessOrEqual,
        Comparison::GreaterOrEqual => Comparison::Less,
    }
}

pub(super) fn resolution_cond_as_flag(cond: ResolutionCondition) -> ValueExpr {
    ValueExpr::Conditional {
        left: Box::new(cond.left),
        comparison: cond.comparison,
        right: Box::new(cond.right),
        then_value: Box::new(ValueExpr::literal(1)),
        else_value: Box::new(ValueExpr::literal(0)),
    }
}

pub(super) fn combine_resolution_conditions_and(
    conds: Vec<ResolutionCondition>,
) -> Option<ResolutionCondition> {
    let total = i64::try_from(conds.len()).ok()?;
    let mut iter = conds.into_iter();
    let first = iter.next()?;
    let Some(second) = iter.next() else {
        return Some(first);
    };
    let mut sum = ValueExpr::Add(
        Box::new(resolution_cond_as_flag(first)),
        Box::new(resolution_cond_as_flag(second)),
    );
    for cond in iter {
        sum = ValueExpr::Add(Box::new(sum), Box::new(resolution_cond_as_flag(cond)));
    }
    Some(ResolutionCondition {
        left: sum,
        comparison: Comparison::Equal,
        right: ValueExpr::literal(total),
    })
}

pub(super) fn combine_resolution_conditions_or(
    conds: Vec<ResolutionCondition>,
) -> Option<ResolutionCondition> {
    let mut iter = conds.into_iter();
    let first = iter.next()?;
    let Some(second) = iter.next() else {
        return Some(first);
    };
    let mut sum = ValueExpr::Add(
        Box::new(resolution_cond_as_flag(first)),
        Box::new(resolution_cond_as_flag(second)),
    );
    for cond in iter {
        sum = ValueExpr::Add(Box::new(sum), Box::new(resolution_cond_as_flag(cond)));
    }
    Some(ResolutionCondition {
        left: sum,
        comparison: Comparison::GreaterOrEqual,
        right: ValueExpr::literal(1),
    })
}

pub(super) fn is_self_anaphora(a: &str) -> bool {
    matches!(
        a.trim().to_ascii_lowercase().as_str(),
        "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
            | "this card"
            | "this battle"
            | "this vehicle"
    )
}

pub(super) fn is_object_body_target(target: BodyTarget, allow_any_target: bool) -> bool {
    if allow_any_target {
        !matches!(
            target,
            BodyTarget::None | BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
        )
    } else {
        !matches!(
            target,
            BodyTarget::None
                | BodyTarget::SinglePlayer
                | BodyTarget::SingleOpponent
                | BodyTarget::SingleAnyTarget
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::SingleOpponentOrPlaneswalker
                | BodyTarget::SingleCreatureOrPlayer
        )
    }
}

pub(super) fn resolve_prev_anaphora_ref(
    instructions: &[Instruction],
    is_obj_target: bool,
    in_trigger: bool,
) -> Option<MatchObjectRef> {
    let has_checked_top = instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::RevealUntilMatchExt { .. } | Instruction::LookTopChooseExt { .. }
        ) || matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                )
        ) || super::super::toolbox_emit::unwrap_zone_op(op).is_some_and(|spec| {
            matches!(
                &spec.source,
                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                    zone: spellwake_engine::toolbox::QueryZone::Library(_),
                    ..
                }
            ) && matches!(
                spec.destination,
                spellwake_engine::toolbox::ZoneDest::Exile { .. }
                    | spellwake_engine::toolbox::ZoneDest::RevealOnly
            )
        })
    });
    instructions.iter().rev().find_map(|op| match op {
        Instruction::RevealUntilMatchExt { .. } | Instruction::LookTopChooseExt { .. } => {
            Some(MatchObjectRef::RevealedTop)
        }
        Instruction::MutatePlayer(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
            ) =>
        {
            Some(MatchObjectRef::RevealedTop)
        }
        Instruction::MutatePlayer(spec)
            if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_)) =>
        {
            Some(MatchObjectRef::MovedLibraryCard)
        }
        Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
            if super::super::toolbox_emit::unwrap_zone_op(op).is_some() =>
        {
            let spec = super::super::toolbox_emit::unwrap_zone_op(op).unwrap();
            match &spec.destination {
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. },
                } if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets) => {
                    Some(MatchObjectRef::DestructionTarget)
                }
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Discard,
                } => Some(MatchObjectRef::DiscardedCard),
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                } => Some(MatchObjectRef::LastMovedToGraveyard),
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Mill,
                } => Some(MatchObjectRef::MovedLibraryCard),
                spellwake_engine::toolbox::ZoneDest::Exile { .. }
                    if matches!(
                        &spec.source,
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(_),
                            ..
                        }
                    ) =>
                {
                    Some(MatchObjectRef::LastMovedToExile)
                }
                spellwake_engine::toolbox::ZoneDest::None
                | spellwake_engine::toolbox::ZoneDest::RevealOnly
                | spellwake_engine::toolbox::ZoneDest::LookOnly { .. } => None,
                _ => match &spec.source {
                    spellwake_engine::toolbox::ObjectQuery::Source => {
                        Some(MatchObjectRef::AbilitySource)
                    }
                    spellwake_engine::toolbox::ObjectQuery::AttachedHost => {
                        Some(MatchObjectRef::AttachedHost)
                    }
                    spellwake_engine::toolbox::ObjectQuery::Targets if is_obj_target => {
                        Some(MatchObjectRef::TargetObject)
                    }
                    spellwake_engine::toolbox::ObjectQuery::TriggerSubject if in_trigger => {
                        Some(MatchObjectRef::TriggerSubject)
                    }
                    spellwake_engine::toolbox::ObjectQuery::Revealed if has_checked_top => {
                        Some(MatchObjectRef::RevealedTop)
                    }
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan { .. }
                    | spellwake_engine::toolbox::ObjectQuery::Union(_) => {
                        Some(MatchObjectRef::LastAffected)
                    }
                    _ => None,
                },
            }
        }
        Instruction::SearchLibrary { .. } => Some(MatchObjectRef::LastAffected),
        _ => None,
    })
}

pub(super) fn parse_library_top_anaphora(
    raw: &str,
    builder: &BodyBuilder,
    in_trigger: bool,
) -> Option<ResolvingPlayer> {
    let low = raw.trim().to_ascii_lowercase();
    match low.as_str() {
        "top_of_library" | "the top card of your library" | "top card of your library" => {
            Some(ResolvingPlayer::Controller)
        }
        "the top card of target player's library"
        | "top card of target player's library"
        | "the top card of target opponent's library"
        | "top card of target opponent's library"
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) =>
        {
            Some(ResolvingPlayer::TargetPlayer)
        }
        "the top card of their library"
        | "top card of their library"
        | "the top card of that player's library"
        | "top card of that player's library" => resolve_their_library_owner(builder, in_trigger),
        "the top card of the active player's library"
        | "the top card of active player's library"
        | "top card of active player's library" => Some(ResolvingPlayer::ActivePlayer),
        "the top card of the defending player's library"
        | "the top card of defending player's library"
        | "top card of defending player's library" => Some(ResolvingPlayer::DefendingPlayer),
        "the top card of enchanted player's library" | "top card of enchanted player's library" => {
            Some(ResolvingPlayer::EnchantedPlayer)
        }
        _ => None,
    }
}

pub(super) fn unwrap_battlefield_subject(mut subject: &SubjectRefAst) -> &SubjectRefAst {
    while let SubjectRefAst::InZone {
        subject: inner,
        zone: ZoneFilter::Battlefield,
    } = subject
    {
        subject = inner.as_ref();
    }
    subject
}

pub(super) fn bind_condition_target_object(
    filter: &ObjectFilterAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    allow_any_target: bool,
) -> Result<BodyTarget, &'static str> {
    let bt = if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) {
        let mut inner = filter.clone();
        inner.zone = None;
        lower_battlefield_body_target(&inner, vocabulary)?
    } else {
        lower_target_subject(
            &SubjectRefAst::TargetObject {
                filter: Box::new(filter.clone()),
                count: TargetCountAst::Exact(1),
            },
            vocabulary,
        )?
    };
    if !is_object_body_target(bt, allow_any_target) {
        return Err("oracle:unsupported-if-condition");
    }
    builder.bind_target(bt)?;
    Ok(bt)
}

pub(super) fn resolve_match_object_ref(
    subject: &SubjectRefAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    allow_any_target: bool,
) -> Option<MatchObjectRef> {
    let subject = unwrap_battlefield_subject(subject);
    let is_obj_target = is_object_body_target(builder.target, allow_any_target);
    let prev = resolve_prev_anaphora_ref(&builder.instructions, is_obj_target, in_trigger);
    match subject {
        SubjectRefAst::SelfObject => Some(MatchObjectRef::AbilitySource),
        SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => Some(MatchObjectRef::AbilitySource),
        SubjectRefAst::AttachedHost(_) => Some(MatchObjectRef::AttachedHost),
        SubjectRefAst::TriggerSubject if in_trigger => Some(MatchObjectRef::TriggerSubject),
        SubjectRefAst::Anaphora(a) => {
            if let Some(rp) = parse_library_top_anaphora(a, builder, in_trigger) {
                return Some(MatchObjectRef::LibraryTop(rp));
            }
            let al = a.trim().to_ascii_lowercase();
            if matches!(
                al.as_str(),
                "equipped creature"
                    | "enchanted creature"
                    | "enchanted permanent"
                    | "fortified land"
            ) {
                return Some(MatchObjectRef::AttachedHost);
            }
            if matches!(
                al.as_str(),
                "the revealed card"
                    | "revealed card"
                    | "that revealed card"
                    | "the card revealed this way"
            ) && prev == Some(MatchObjectRef::RevealedTop)
            {
                return Some(MatchObjectRef::RevealedTop);
            }
            if matches!(
                al.as_str(),
                "the exiled card" | "exiled card" | "that exiled card" | "the card exiled this way"
            ) {
                if prev == Some(MatchObjectRef::RevealedTop) {
                    return Some(MatchObjectRef::RevealedTop);
                }
                if last_moved_subject_safe(builder, MatchObjectRef::LastMovedToExile)
                    && builder.instructions.last().is_some_and(|op| {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                            .is_some_and(|spec| {
                                matches!(
                                    spec.destination,
                                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                                )
                            })
                    })
                {
                    return Some(MatchObjectRef::LastMovedToExile);
                }
            }
            if matches!(
                al.as_str(),
                "the discarded card"
                    | "discarded card"
                    | "that discarded card"
                    | "the card discarded this way"
            ) && prev == Some(MatchObjectRef::DiscardedCard)
            {
                return Some(MatchObjectRef::DiscardedCard);
            }
            if matches!(
                al.as_str(),
                "the sacrificed creature"
                    | "the sacrificed permanent"
                    | "the sacrificed artifact"
                    | "the sacrificed enchantment"
                    | "the sacrificed land"
                    | "that sacrificed creature"
                    | "that sacrificed permanent"
                    | "that sacrificed artifact"
                    | "that sacrificed enchantment"
                    | "that sacrificed land"
            ) {
                if builder
                    .instructions
                    .last()
                    .and_then(super::super::toolbox_emit::unwrap_zone_op)
                    .is_some_and(|spec| {
                        matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                            }
                        )
                    })
                {
                    return Some(MatchObjectRef::LastMovedToGraveyard);
                }
                return Some(MatchObjectRef::SacrificedCreature);
            }
            if matches!(
                al.as_str(),
                "the milled card" | "a card milled this way" | "that milled card"
            ) && prev == Some(MatchObjectRef::MovedLibraryCard)
            {
                return Some(MatchObjectRef::MovedLibraryCard);
            }
            if prev.is_some() && !is_obj_target {
                prev
            } else if is_obj_target {
                Some(MatchObjectRef::TargetObject)
            } else if in_trigger {
                Some(MatchObjectRef::TriggerSubject)
            } else if al == "it" && builder.has_source && builder.instructions.is_empty() {
                Some(MatchObjectRef::AbilitySource)
            } else {
                prev
            }
        }
        _ => None,
    }
}

pub(super) fn resolve_value_object_ref_for_stat(
    subject: &SubjectRefAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    requires_creature_target: bool,
) -> Option<ValueObjectRef> {
    let subject = unwrap_battlefield_subject(subject);
    let target_ok = if requires_creature_target {
        builder.target.guarantees_creature()
    } else {
        is_object_body_target(builder.target, false)
    };
    match subject {
        SubjectRefAst::SelfObject => Some(ValueObjectRef::AbilitySource),
        SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => Some(ValueObjectRef::AbilitySource),
        SubjectRefAst::AttachedHost(_) => Some(ValueObjectRef::AttachedHost),
        SubjectRefAst::Anaphora(a)
            if matches!(
                a.trim().to_ascii_lowercase().as_str(),
                "equipped creature"
                    | "enchanted creature"
                    | "enchanted permanent"
                    | "fortified land"
            ) =>
        {
            Some(ValueObjectRef::AttachedHost)
        }
        SubjectRefAst::TriggerSubject if in_trigger => Some(ValueObjectRef::TriggerSubject),
        SubjectRefAst::Anaphora(_) if target_ok => Some(ValueObjectRef::TargetObject),
        SubjectRefAst::Anaphora(_) if in_trigger => Some(ValueObjectRef::TriggerSubject),
        _ => None,
    }
}

pub(super) fn parse_small_positive_count(raw: &str) -> Option<i64> {
    match raw.trim() {
        "a" | "an" | "one" | "1" => Some(1),
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        "ten" | "10" => Some(10),
        "eleven" | "11" => Some(11),
        "twelve" | "12" => Some(12),
        other => other.parse::<i64>().ok().filter(|&v| v > 0),
    }
}

pub(super) fn parse_color_word_mana_type(raw: &str) -> Option<ManaType> {
    match raw.trim() {
        "white" | "w" | "{w}" | "white mana" => Some(ManaType::White),
        "blue" | "u" | "{u}" | "blue mana" => Some(ManaType::Blue),
        "black" | "b" | "{b}" | "black mana" => Some(ManaType::Black),
        "red" | "r" | "{r}" | "red mana" => Some(ManaType::Red),
        "green" | "g" | "{g}" | "green mana" => Some(ManaType::Green),
        "colorless" | "c" | "{c}" | "colorless mana" => Some(ManaType::Colourless),
        _ => None,
    }
}

pub(in super::super) fn lower_resolution_condition(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<ResolutionCondition, &'static str> {
    match cond {
        ConditionAst::Marker(..)
        | ConditionAst::PlayerActionThisTurn { .. }
        | ConditionAst::EventOccurredThisTurn { .. } => {
            let snap_target = builder.target;
            let snap_gy = builder.graveyard_target_bounds;
            let snap_perm = builder.permanent_target_bounds;
            let snap_x = builder.x_target_count;
            let snap_up_to_x = builder.up_to_x_target_count;
            let snap_group = builder.last_group_filter;
            match lower_marker_or_action_condition(cond, builder, vocabulary, in_trigger, bound_x) {
                Ok(rc) => Ok(rc),
                Err("oracle:unsupported-if-condition") => {
                    builder.target = snap_target;
                    builder.graveyard_target_bounds = snap_gy;
                    builder.permanent_target_bounds = snap_perm;
                    builder.x_target_count = snap_x;
                    builder.up_to_x_target_count = snap_up_to_x;
                    builder.last_group_filter = snap_group;
                    if let ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action,
                    } = cond
                    {
                        let as_event = ConditionAst::EventOccurredThisTurn {
                            event: action.clone(),
                        };
                        match lower_marker_or_action_condition(
                            &as_event, builder, vocabulary, in_trigger, bound_x,
                        ) {
                            Ok(rc) => return Ok(rc),
                            Err("oracle:unsupported-if-condition") => {
                                builder.target = snap_target;
                                builder.graveyard_target_bounds = snap_gy;
                                builder.permanent_target_bounds = snap_perm;
                                builder.x_target_count = snap_x;
                                builder.up_to_x_target_count = snap_up_to_x;
                                builder.last_group_filter = snap_group;
                            }
                            Err(err) => return Err(err),
                        }
                    }
                    lower_marker_or_action_fallback(cond, builder, vocabulary, in_trigger, bound_x)
                }
                Err(err) => Err(err),
            }
        }
        ConditionAst::Not(inner) => {
            let mut rc =
                lower_resolution_condition(inner, builder, vocabulary, in_trigger, bound_x)?;
            rc.comparison = negate_comparison(rc.comparison);
            Ok(rc)
        }
        ConditionAst::And(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            combine_resolution_conditions_and(vec![ca, cb]).ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Or(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            combine_resolution_conditions_or(vec![ca, cb]).ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::Compare { lhs, op, rhs } | ConditionAst::ExprCompare { lhs, op, rhs } => {
            let lhs_res = resolve_x(lhs, bound_x);
            let rhs_res = resolve_x(rhs, bound_x);
            // Handle controller life compared directly against AnyOpponent / EachOpponent life
            // (e.g. Glorious Enforcer: "if you have more life than an opponent").
            if let (
                ValueExprAst::PlayerStat {
                    player: p_lhs,
                    stat: PlayerStatKind::LifeTotal,
                },
                ValueExprAst::PlayerStat {
                    player: p_rhs,
                    stat: PlayerStatKind::LifeTotal,
                },
            ) = (lhs_res, rhs_res)
            {
                let opp_life = |opp: PlayerRefAst, cmp: CompareOp| -> Option<ValueExpr> {
                    match (opp, cmp) {
                        (PlayerRefAst::AnyOpponent, CompareOp::Gt | CompareOp::Ge)
                        | (PlayerRefAst::EachOpponent, CompareOp::Lt | CompareOp::Le) => {
                            Some(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife))
                        }
                        (PlayerRefAst::AnyOpponent, CompareOp::Lt | CompareOp::Le)
                        | (PlayerRefAst::EachOpponent, CompareOp::Gt | CompareOp::Ge) => {
                            Some(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife))
                        }
                        _ => None,
                    }
                };
                if *p_lhs == PlayerRefAst::You
                    && let Some(right_expr) = opp_life(*p_rhs, *op)
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        comparison: lower_comparison(*op),
                        right: right_expr,
                    });
                }
                if *p_rhs == PlayerRefAst::You {
                    let flipped = match *op {
                        CompareOp::Gt => CompareOp::Lt,
                        CompareOp::Ge => CompareOp::Le,
                        CompareOp::Lt => CompareOp::Gt,
                        CompareOp::Le => CompareOp::Ge,
                        other => other,
                    };
                    if let Some(left_expr) = opp_life(*p_lhs, flipped) {
                        return Ok(ResolutionCondition {
                            left: left_expr,
                            comparison: lower_comparison(*op),
                            right: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                        });
                    }
                }
            }
            bind_value_expr_targets(lhs_res, builder)?;
            bind_value_expr_targets(rhs_res, builder)?;
            let mut left = lower_condition_value_expr(lhs_res, builder, in_trigger, vocabulary)?;
            let mut right = lower_condition_value_expr(rhs_res, builder, in_trigger, vocabulary)?;
            if in_trigger && builder.target == BodyTarget::None {
                let lhs_is_self = matches!(
                    lhs_res,
                    ValueExprAst::ObjectStat { subject, .. }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
                );
                let rhs_is_self = matches!(
                    rhs_res,
                    ValueExprAst::ObjectStat { subject, .. }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
                );
                rewrite_value_expr_anaphora(
                    &mut left,
                    if lhs_is_self {
                        ValueObjectRef::AbilitySource
                    } else {
                        ValueObjectRef::TriggerSubject
                    },
                );
                rewrite_value_expr_anaphora(
                    &mut right,
                    if rhs_is_self {
                        ValueObjectRef::AbilitySource
                    } else {
                        ValueObjectRef::TriggerSubject
                    },
                );
            }
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: op @ (CompareOp::Ge | CompareOp::Gt),
            value,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && matches!(filter.controller, None | Some(ControllerRelation::You))
            && filter.owner.is_none()
            && filter.predicates.iter().any(|predicate| {
                matches!(
                    predicate,
                    spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(name)
                        if name == "different_powers"
                )
            }) =>
        {
            let mut selected = filter.as_ref().clone();
            selected.predicates.retain(|predicate| {
                !matches!(
                    predicate,
                    spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(name)
                        if name == "different_powers"
                )
            });
            selected.controller = Some(ControllerRelation::You);
            let obj = lower_object_filter(&selected, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::aggregate(
                    obj,
                    ObjectStatKind::Power,
                    StatAggregate::CountDistinct,
                ),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ControlsCount {
            player,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary)?;
            let left = match player {
                PlayerRefAst::You => {
                    ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj)
                }
                PlayerRefAst::EachPlayer => ValueExpr::count_battlefield(obj),
                PlayerRefAst::AnyPlayer
                    if *value >= 2 && matches!(op, CompareOp::Ge | CompareOp::Gt) =>
                {
                    let yours =
                        ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj);
                    let opp_max = ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(opp_max.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_max),
                    }
                }
                PlayerRefAst::AnyPlayer => ValueExpr::count_battlefield(obj),
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    }
                }
                PlayerRefAst::AnyOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    }
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    }
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    }
                }
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    ValueExpr::count_battlefield(obj.opposing())
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    ValueExpr::count_player_battlefield(rp, obj)
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::PlayerCounterCompare {
            player,
            counter_kind,
            op,
            value,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?;
            match player {
                PlayerRefAst::You => {
                    let subject = match ck {
                        CounterKind::Experience => {
                            spellwake_engine::program::CounterSubjectRef::ControllerExperience
                        }
                        CounterKind::Energy => {
                            spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                        }
                        CounterKind::Poison => {
                            spellwake_engine::program::CounterSubjectRef::ControllerPoison
                        }
                        other_ck => spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                            ResolvingPlayer::Controller,
                            other_ck,
                        ),
                    };
                    Ok(ResolutionCondition {
                        left: ValueExpr::counters(subject, Some(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::PlayerCounter(rp, ck),
                            Some(ck),
                        ),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
            }
        }
        ConditionAst::Metalcraft => Ok(ResolutionCondition {
            left: ValueExpr::controlled_type(CardType::Artifact),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        }),
        ConditionAst::Threshold => Ok(ResolutionCondition {
            left: ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::any()),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(7),
        }),
        ConditionAst::HandCount { player, op, value } => {
            let left = match player {
                PlayerRefAst::AnyPlayer if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    let yours = ValueExpr::count_hand(ResolvingPlayer::Controller);
                    let opp_max = ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(opp_max.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_max),
                    }
                }
                PlayerRefAst::AnyPlayer
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    let yours = ValueExpr::count_hand(ResolvingPlayer::Controller);
                    let opp_min = ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::LessOrEqual,
                        right: Box::new(opp_min.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_min),
                    }
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::AnyOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                _ => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    ValueExpr::count_hand(rp)
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::GraveyardCount { player, op, value } => {
            let left = match player {
                PlayerRefAst::AnyPlayer if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    let yours = ValueExpr::count_graveyard(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    );
                    let opp_max = ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(opp_max.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_max),
                    }
                }
                PlayerRefAst::AnyPlayer
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    let yours = ValueExpr::count_graveyard(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    );
                    let opp_min = ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::LessOrEqual,
                        right: Box::new(opp_min.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_min),
                    }
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::AnyOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                _ => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    ValueExpr::count_graveyard(rp, ObjectFilter::any())
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ZoneObjectCount { filter, op, value } => {
            let left = lower_value_expr_in_context(
                &ValueExprAst::Count(filter.clone()),
                builder,
                in_trigger,
                vocabulary,
            )
            .or_else(|_| {
                lower_filter_to_zone_query(filter, builder, in_trigger, vocabulary)
                    .map(|q| ValueExpr::QueryCount(Box::new(q)))
                    .ok_or("oracle:unsupported-if-condition")
            })?;
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::LifeCompare { player, op, value } => {
            let max_all_life = || {
                let ctrl = ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
                let opp_max = ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife);
                ValueExpr::Conditional {
                    left: Box::new(ctrl.clone()),
                    comparison: Comparison::GreaterOrEqual,
                    right: Box::new(opp_max.clone()),
                    then_value: Box::new(ctrl),
                    else_value: Box::new(opp_max),
                }
            };
            let min_all_life = || {
                let ctrl = ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
                let opp_min = ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife);
                ValueExpr::Conditional {
                    left: Box::new(ctrl.clone()),
                    comparison: Comparison::LessOrEqual,
                    right: Box::new(opp_min.clone()),
                    then_value: Box::new(ctrl),
                    else_value: Box::new(opp_min),
                }
            };
            let left = match player {
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)
                }
                PlayerRefAst::EachPlayer if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    max_all_life()
                }
                PlayerRefAst::EachPlayer if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    min_all_life()
                }
                PlayerRefAst::AnyPlayer if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    max_all_life()
                }
                PlayerRefAst::AnyPlayer if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    min_all_life()
                }
                _ => {
                    let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                    if rp == ResolvingPlayer::Controller {
                        ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                    } else {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp))
                    }
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ControlsCommander => Ok(ResolutionCondition {
            left: ValueExpr::count_player_battlefield(
                ResolvingPlayer::Controller,
                ObjectFilter::Clause(ObjectClause::any().commander()),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } => {
            let subject = unwrap_battlefield_subject(subject);
            let mut eff_op = *op;
            let mut eff_val = i64::from(*value);
            let ck_trim = counter_kind.trim();
            let ck_low = ck_trim.to_ascii_lowercase();
            let kind = if ck_low.is_empty()
                || matches!(ck_low.as_str(), "any" | "a" | "an" | "counter" | "counters")
            {
                None
            } else if *op == CompareOp::Ge
                && *value == 1
                && let Some(rest) = ck_low.strip_prefix("exactly ")
                && let Some((cnt_tok, kind_rest)) = rest.trim().split_once(' ')
                && let Some(n) = parse_small_positive_count(cnt_tok)
                && let Some(parsed_ck) = parse_counter_kind(kind_rest.trim())
            {
                eff_op = CompareOp::Eq;
                eff_val = n;
                Some(parsed_ck)
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?)
            };
            if let SubjectRefAst::Player(p) = subject {
                let ck = kind.ok_or("oracle:unsupported-if-condition")?;
                return match p {
                    PlayerRefAst::You => {
                        let subj = match ck {
                            CounterKind::Experience => {
                                spellwake_engine::program::CounterSubjectRef::ControllerExperience
                            }
                            CounterKind::Energy => {
                                spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                            }
                            CounterKind::Poison => {
                                spellwake_engine::program::CounterSubjectRef::ControllerPoison
                            }
                            other_ck => {
                                spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                                    ResolvingPlayer::Controller,
                                    other_ck,
                                )
                            }
                        };
                        Ok(ResolutionCondition {
                            left: ValueExpr::counters(subj, Some(ck)),
                            comparison: lower_comparison(eff_op),
                            right: ValueExpr::literal(eff_val),
                        })
                    }
                    PlayerRefAst::AnyOpponent
                        if matches!(eff_op, CompareOp::Ge | CompareOp::Gt) =>
                    {
                        Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                            comparison: lower_comparison(eff_op),
                            right: ValueExpr::literal(eff_val),
                        })
                    }
                    PlayerRefAst::EachOpponent
                        if matches!(eff_op, CompareOp::Le | CompareOp::Lt)
                            || (eff_op == CompareOp::Eq && eff_val == 0) =>
                    {
                        Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                            comparison: lower_comparison(eff_op),
                            right: ValueExpr::literal(eff_val),
                        })
                    }
                    other => {
                        let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                        Ok(ResolutionCondition {
                            left: ValueExpr::counters(
                                spellwake_engine::program::CounterSubjectRef::PlayerCounter(rp, ck),
                                Some(ck),
                            ),
                            comparison: lower_comparison(eff_op),
                            right: ValueExpr::literal(eff_val),
                        })
                    }
                };
            }
            let subj = match subject {
                SubjectRefAst::SelfObject => {
                    if in_trigger {
                        spellwake_engine::program::CounterSubjectRef::TriggerSource
                    } else {
                        spellwake_engine::program::CounterSubjectRef::AbilitySource
                    }
                }
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => {
                    if in_trigger {
                        spellwake_engine::program::CounterSubjectRef::TriggerSource
                    } else {
                        spellwake_engine::program::CounterSubjectRef::AbilitySource
                    }
                }
                SubjectRefAst::AttachedHost(_) => {
                    spellwake_engine::program::CounterSubjectRef::AttachedHost
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } => {
                    bind_condition_target_object(filter, builder, vocabulary, false)?;
                    spellwake_engine::program::CounterSubjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_)
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    let ck = kind.ok_or("oracle:unsupported-if-condition")?;
                    spellwake_engine::program::CounterSubjectRef::PlayerCounter(
                        ResolvingPlayer::TargetPlayer,
                        ck,
                    )
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    spellwake_engine::program::CounterSubjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_) if in_trigger => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                SubjectRefAst::Anaphora(a)
                    if a.trim().eq_ignore_ascii_case("it")
                        && builder.has_source
                        && resolve_prev_anaphora_ref(&builder.instructions, false, false)
                            .is_none() =>
                {
                    spellwake_engine::program::CounterSubjectRef::AbilitySource
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::counters(subj, kind),
                comparison: lower_comparison(eff_op),
                right: ValueExpr::literal(eff_val),
            })
        }
        ConditionAst::TotalStatCompare {
            filter,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::aggregate(obj, st, StatAggregate::Sum),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let is_object_target = !matches!(
                builder.target,
                BodyTarget::None
                    | BodyTarget::SinglePlayer
                    | BodyTarget::SingleOpponent
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker
                    | BodyTarget::SingleCreatureOrPlayer
            );
            let is_self_anaphora = |a: &str| {
                matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                        | "this planeswalker"
                )
            };
            let subject = unwrap_battlefield_subject(subject);
            let subj = match subject {
                SubjectRefAst::SelfObject => {
                    if in_trigger {
                        ValueObjectRef::TriggerSource
                    } else {
                        ValueObjectRef::AbilitySource
                    }
                }
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => {
                    if in_trigger {
                        ValueObjectRef::TriggerSource
                    } else {
                        ValueObjectRef::AbilitySource
                    }
                }
                SubjectRefAst::AttachedHost(_) => ValueObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject if in_trigger => ValueObjectRef::TriggerSubject,
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = bind_condition_target_object(filter, builder, vocabulary, false)?;
                    if matches!(stat, StatKind::Power | StatKind::Toughness)
                        && !bt.guarantees_creature()
                    {
                        return Err("oracle:unsupported-if-condition");
                    }
                    ValueObjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_)
                    if resolve_prev_anaphora_ref(
                        &builder.instructions,
                        is_object_target,
                        in_trigger,
                    ) == Some(MatchObjectRef::RevealedTop) =>
                {
                    ValueObjectRef::RevealedTop
                }
                SubjectRefAst::Anaphora(_) if is_object_target => ValueObjectRef::TargetObject,
                SubjectRefAst::Anaphora(_) if in_trigger => ValueObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(a)
                    if a.trim().eq_ignore_ascii_case("it")
                        && builder.has_source
                        && builder.target == BodyTarget::None
                        && resolve_prev_anaphora_ref(&builder.instructions, false, false)
                            .is_none() =>
                {
                    ValueObjectRef::AbilitySource
                }
                other => {
                    if let Some(q) =
                        lower_subject_to_object_query(other, builder, in_trigger, vocabulary)
                    {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::QueryAggregate {
                                query: Box::new(q),
                                stat: st,
                                op: StatAggregate::Max,
                            },
                            comparison: lower_comparison(*op),
                            right: ValueExpr::literal(i64::from(*value)),
                        });
                    }
                    return Err("oracle:unsupported-if-condition");
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_stat(subj, st),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::MatchesFilter { subject, filter } => {
            let subject = unwrap_battlefield_subject(subject);
            let mref = if let SubjectRefAst::TargetObject {
                filter: t_filter,
                count: TargetCountAst::Exact(1),
            } = subject
            {
                bind_condition_target_object(t_filter, builder, vocabulary, true)?;
                MatchObjectRef::TargetObject
            } else if let SubjectRefAst::Anaphora(a) = subject
                && builder.target == BodyTarget::None
            {
                let al = a.trim().to_ascii_lowercase();
                if matches!(
                    al.as_str(),
                    "the top card of target player's library"
                        | "top card of target player's library"
                ) {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    MatchObjectRef::LibraryTop(ResolvingPlayer::TargetPlayer)
                } else if matches!(
                    al.as_str(),
                    "the top card of target opponent's library"
                        | "top card of target opponent's library"
                ) {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    MatchObjectRef::LibraryTop(ResolvingPlayer::TargetPlayer)
                } else {
                    resolve_match_object_ref(subject, builder, in_trigger, true)
                        .ok_or("oracle:unsupported-if-condition")?
                }
            } else {
                resolve_match_object_ref(subject, builder, in_trigger, true)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasStatus { subject, status } => {
            let subject = unwrap_battlefield_subject(subject);
            let status_low = status.trim().to_ascii_lowercase();
            if matches!(subject, SubjectRefAst::SelfObject)
                || matches!(subject, SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                use spellwake_engine::program::SourceZoneKind;
                match status_low.as_str() {
                    "saddled" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Saddled),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "renowned" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Renowned),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "unrenowned" | "not renowned" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Renowned),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        });
                    }
                    "paired" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Paired),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "monstrous" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Monstrous),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "suspected" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Suspected),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "solved" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Solved),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "unsolved" | "not solved" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Solved),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        });
                    }
                    "prepared" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Prepared),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "goaded" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Goaded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "exerted" | "exerted_this_turn" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::ExertedThisTurn),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "in_command_zone" | "in command zone" | "in the command zone" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Command,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "in_graveyard" | "in graveyard" | "in a graveyard" | "in your graveyard" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Graveyard,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "on_battlefield" | "on battlefield" | "on the battlefield" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Battlefield,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "in_hand" | "in hand" | "in your hand" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Hand,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "in_library" | "in library" | "in your library" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Library,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "on_stack" | "on stack" | "on the stack" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Stack,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "exiled" | "in_exile" | "in exile" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                                SourceZoneKind::Exile,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "kicked" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::KickerCount(0)),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "ring_bearer" | "ring-bearer" | "your ring-bearer" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::IsRingBearer),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "attached" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::object_matches(
                                MatchObjectRef::AttachedHost,
                                ObjectFilter::permanent(),
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        });
                    }
                    other => {
                        if let Some(host_raw) = other.strip_prefix("attached:") {
                            let host_clean = host_raw.trim();
                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                            let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                                host_clean,
                                &compiler_vocab,
                            )
                            .map_err(|_| "oracle:unsupported-if-condition")?;
                            if !matches!(parsed.zone, None | Some(ZoneFilter::Battlefield))
                                || !parsed.extra_zones.is_empty()
                            {
                                return Err("oracle:unsupported-if-condition");
                            }
                            let mut cleaned = parsed;
                            cleaned.zone = None;
                            let obj = if is_unrestricted_filter(&cleaned) {
                                ObjectFilter::permanent()
                            } else {
                                lower_object_filter(&cleaned, vocabulary)?
                            };
                            return Ok(ResolutionCondition {
                                left: ValueExpr::object_matches(MatchObjectRef::AttachedHost, obj),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(1),
                            });
                        }
                    }
                }
            }
            if let SubjectRefAst::Group(f) = subject
                && matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && f.extra_zones.is_empty()
            {
                let mut clean = f.as_ref().clone();
                clean.zone = None;
                let base_obj = lower_object_filter(&clean, vocabulary)?;
                let apply_status = |c: &mut ObjectClause| -> bool {
                    match status_low.as_str() {
                        "tapped" if c.tapped.is_none() => {
                            c.tapped = Some(true);
                            true
                        }
                        "untapped" if c.tapped.is_none() => {
                            c.tapped = Some(false);
                            true
                        }
                        "attacking" if c.combat == CombatFilter::Any => {
                            c.combat = CombatFilter::Attacking;
                            true
                        }
                        "blocking" if c.combat == CombatFilter::Any => {
                            c.combat = CombatFilter::Blocking;
                            true
                        }
                        "blocked" if c.combat == CombatFilter::Any => {
                            c.combat = CombatFilter::Blocked;
                            true
                        }
                        "unblocked" if c.combat == CombatFilter::Any => {
                            c.combat = CombatFilter::Unblocked;
                            true
                        }
                        "attacking or blocking" if c.combat == CombatFilter::Any => {
                            c.combat = CombatFilter::AttackingOrBlocking;
                            true
                        }
                        "enchanted" => {
                            c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
                            true
                        }
                        "equipped" => {
                            c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
                            true
                        }
                        "modified" => {
                            c.flags = c.flags.union(ObjectFilterFlags::MODIFIED);
                            true
                        }
                        "suspected" => {
                            c.flags = c.flags.union(ObjectFilterFlags::SUSPECTED);
                            true
                        }
                        "renowned" => {
                            c.flags = c.flags.union(ObjectFilterFlags::RENOWNED);
                            true
                        }
                        "monstrous" => {
                            c.flags = c.flags.union(ObjectFilterFlags::MONSTROUS);
                            true
                        }
                        "saddled" => {
                            c.flags = c.flags.union(ObjectFilterFlags::SADDLED);
                            true
                        }
                        _ => false,
                    }
                };
                let applied = match base_obj {
                    ObjectFilter::Clause(mut c) => {
                        apply_status(&mut c).then_some(ObjectFilter::Clause(c))
                    }
                    ObjectFilter::Either(mut a, mut b) => (apply_status(&mut a)
                        && apply_status(&mut b))
                    .then_some(ObjectFilter::Either(a, b)),
                };
                if let Some(obj) = applied {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(obj),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            let mref = if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject
            {
                bind_condition_target_object(filter, builder, vocabulary, false)?;
                MatchObjectRef::TargetObject
            } else {
                resolve_match_object_ref(subject, builder, in_trigger, false)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            let obj = match status_low.as_str() {
                "tapped" => ObjectFilter::any().tapped(),
                "untapped" => ObjectFilter::Clause(ObjectClause::any().untapped()),
                "attacking" => ObjectFilter::any().attacking(),
                "blocking" => ObjectFilter::any().combat(CombatFilter::Blocking),
                "attacking or blocking" => ObjectFilter::any().attacking_or_blocking(),
                "blocked" => ObjectFilter::any().combat(CombatFilter::Blocked),
                "unblocked" => ObjectFilter::any().combat(CombatFilter::Unblocked),
                "enchanted" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
                    ObjectFilter::Clause(c)
                }
                "equipped" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
                    ObjectFilter::Clause(c)
                }
                "modified" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::MODIFIED);
                    ObjectFilter::Clause(c)
                }
                "face down" | "face-down" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::FACE_DOWN);
                    ObjectFilter::Clause(c)
                }
                "token" => {
                    let mut c = ObjectClause::any();
                    c.token = Some(true);
                    ObjectFilter::Clause(c)
                }
                "nontoken" => {
                    let mut c = ObjectClause::any();
                    c.token = Some(false);
                    ObjectFilter::Clause(c)
                }
                "legendary" => ObjectFilter::any().legendary(),
                "commander" => ObjectFilter::Clause(ObjectClause::any().commander()),
                "historic" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::HISTORIC);
                    ObjectFilter::Clause(c)
                }
                "nonhistoric" | "non-historic" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::NON_HISTORIC);
                    ObjectFilter::Clause(c)
                }
                "kicked" => ObjectFilter::any().with_flags(ObjectFilterFlags::KICKED),
                "renowned" => ObjectFilter::any().with_flags(ObjectFilterFlags::RENOWNED),
                "saddled" => ObjectFilter::any().with_flags(ObjectFilterFlags::SADDLED),
                "monstrous" => ObjectFilter::any().with_flags(ObjectFilterFlags::MONSTROUS),
                "suspected" => ObjectFilter::any().with_flags(ObjectFilterFlags::SUSPECTED),
                "solved" => ObjectFilter::any().with_flags(ObjectFilterFlags::SOLVED),
                "prepared" => ObjectFilter::any().with_flags(ObjectFilterFlags::PREPARED),
                "exerted" | "exerted_this_turn" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::EXERTED)
                }
                "with_counters" | "has_counter" | "has a counter on it" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::WITH_ANY_COUNTER)
                }
                "without_counters" | "has no counters on it" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::WITHOUT_COUNTERS)
                }
                "dealt_damage_this_turn" | "was dealt damage this turn" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN)
                }
                "attacked_this_turn" | "attacked this turn" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::ATTACKED_THIS_TURN)
                }
                "entered_this_turn" | "entered this turn" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::ENTERED_THIS_TURN)
                }
                "died_this_turn" | "died this turn" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::DIED_THIS_TURN)
                }
                "has_no_abilities" | "no abilities" => {
                    ObjectFilter::any().with_flags(ObjectFilterFlags::NO_ABILITIES)
                }
                "snow" => ObjectFilter::Clause(ObjectClause {
                    required_supertypes: SupertypeMask::SNOW,
                    ..ObjectClause::any()
                }),
                "basic" => ObjectFilter::Clause(ObjectClause {
                    required_supertypes: SupertypeMask::BASIC,
                    ..ObjectClause::any()
                }),
                "nonbasic" => ObjectFilter::Clause(ObjectClause {
                    forbidden_supertypes: SupertypeMask::BASIC,
                    ..ObjectClause::any()
                }),
                "colorless" => ObjectFilter::any().with_color(ObjectColorFilter::Colorless),
                "multicolored" => ObjectFilter::any().with_color(ObjectColorFilter::Multicolored),
                "monocolored" => ObjectFilter::any().with_color(ObjectColorFilter::Monocolored),
                "white" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::White)),
                "blue" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Blue)),
                "black" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Black)),
                "red" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red)),
                "green" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green)),
                "creature" => ObjectFilter::creature(),
                "noncreature" => ObjectFilter::Clause(ObjectClause::any().noncreature()),
                "artifact" => ObjectFilter::artifact(),
                "nonartifact" => {
                    ObjectFilter::Clause(ObjectClause::any().forbid_types(CardTypeMask::ARTIFACT))
                }
                "enchantment" => ObjectFilter::enchantment(),
                "nonenchantment" => ObjectFilter::Clause(
                    ObjectClause::any().forbid_types(CardTypeMask::ENCHANTMENT),
                ),
                "land" => ObjectFilter::land(),
                "planeswalker" => ObjectFilter::planeswalker(),
                "nonland" => ObjectFilter::nonland(),
                other => {
                    if let Some(sub_raw) = other.strip_prefix("is:") {
                        let sub = sub_raw.trim();
                        match sub {
                            "creature" => ObjectFilter::creature(),
                            "artifact" => ObjectFilter::artifact(),
                            "enchantment" => ObjectFilter::enchantment(),
                            "land" => ObjectFilter::land(),
                            "planeswalker" => ObjectFilter::planeswalker(),
                            "legendary" => ObjectFilter::any().legendary(),
                            _ => {
                                let title = sub
                                    .split('-')
                                    .map(|seg| {
                                        let mut ch = seg.chars();
                                        ch.next()
                                            .map(|c| {
                                                c.to_uppercase().collect::<String>() + ch.as_str()
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                                    .join("-");
                                if let Some(ct) = vocabulary.resolve_label(&title) {
                                    ObjectFilter::any().with_subtype(ct)
                                } else {
                                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                                    let parsed =
                                        spellwake_oracle_compiler::filter::parse_object_filter(
                                            sub,
                                            &compiler_vocab,
                                        )
                                        .map_err(|_| "oracle:unsupported-if-condition")?;
                                    lower_object_filter(&parsed, vocabulary)
                                        .map_err(|_| "oracle:unsupported-if-condition")?
                                }
                            }
                        }
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    }
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasKeyword { subject, keyword } => {
            let subject = unwrap_battlefield_subject(subject);
            let kw_toxic = keyword.trim().eq_ignore_ascii_case("toxic");
            let parsed_kw = if kw_toxic {
                None
            } else {
                Some(lower_keyword_ability_name(keyword).ok_or("oracle:unsupported-if-condition")?)
            };
            if let SubjectRefAst::Group(f) = subject
                && matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && f.extra_zones.is_empty()
            {
                let mut clean = f.as_ref().clone();
                clean.zone = None;
                let base_obj = lower_object_filter(&clean, vocabulary)?;
                let apply_kw = |c: &mut ObjectClause| -> bool {
                    if kw_toxic {
                        c.flags = c.flags.union(ObjectFilterFlags::HAS_TOXIC);
                        true
                    } else if let Some(kw) = parsed_kw {
                        if c.keyword.is_some() && c.keyword != Some(kw) {
                            return false;
                        }
                        *c = (*c).with_keyword(kw);
                        true
                    } else {
                        false
                    }
                };
                let applied = match base_obj {
                    ObjectFilter::Clause(mut c) => {
                        apply_kw(&mut c).then_some(ObjectFilter::Clause(c))
                    }
                    ObjectFilter::Either(mut a, mut b) => {
                        (apply_kw(&mut a) && apply_kw(&mut b)).then_some(ObjectFilter::Either(a, b))
                    }
                };
                if let Some(obj) = applied {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(obj),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            let mref = if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject
            {
                bind_condition_target_object(filter, builder, vocabulary, false)?;
                MatchObjectRef::TargetObject
            } else {
                resolve_match_object_ref(subject, builder, in_trigger, false)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            let filter = if kw_toxic {
                ObjectFilter::any().with_flags(ObjectFilterFlags::HAS_TOXIC)
            } else if let Some(kw) = parsed_kw {
                ObjectFilter::any().with_keyword(kw)
            } else {
                return Err("oracle:unsupported-if-condition");
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, filter),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Morbid {
            your_control_only: true,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControlledCreatureDiedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Revolt => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PermanentLeftBattlefieldThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::EnteredThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::IsNotToken => {
            let mut c = ObjectClause::any();
            c.token = Some(false);
            let is_obj_target = is_object_body_target(builder.target, false);
            let mref = if is_obj_target {
                MatchObjectRef::TargetObject
            } else if let Some(prev) =
                resolve_prev_anaphora_ref(&builder.instructions, false, in_trigger)
            {
                prev
            } else {
                MatchObjectRef::AbilitySource
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::Clause(c)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::OpponentCountCompare { op, value } => Ok(ResolutionCondition {
            left: ValueExpr::OpponentsCount,
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::DevotionCompare { colors, op, value } => {
            let mask =
                parse_devotion_color_mask(colors).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::Devotion(mask),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::OpponentMoreThanYou { metric, filter } => {
            if let Some(f) = filter {
                let obj = lower_object_filter(f, vocabulary)?;
                return Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                });
            }
            match metric.to_ascii_lowercase().as_str() {
                "life" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    comparison: Comparison::Greater,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                }),
                "cards" | "cards in hand" | "cards_in_hand" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_hand(ResolvingPlayer::Controller),
                }),
                "cards in graveyard" | "cards_in_graveyard" | "cards in their graveyard" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        },
                        comparison: Comparison::Greater,
                        right: ValueExpr::count_graveyard(
                            ResolvingPlayer::Controller,
                            ObjectFilter::any(),
                        ),
                    })
                }
                "lands" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::land()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::land(),
                    ),
                }),
                "creatures" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::creature()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::creature(),
                    ),
                }),
                "artifacts" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::artifact()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::artifact(),
                    ),
                }),
                "enchantments" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::enchantment()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::enchantment(),
                    ),
                }),
                "permanents" | "controls" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::any()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    ),
                }),
                "nonland permanents" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::nonland()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::nonland(),
                    ),
                }),
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::WasKicked => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::KickerCount(0)),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::TributeNotPaid => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(spellwake_engine::program::SourceStateExpr::TributePaid),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::ManaSpentToCast(raw) => {
            let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
            if low == "no mana" || low == "0" || low == "zero mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaSpentToCast),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if low == "no colored mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ColorsSpentToCast),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if low == "no snow mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::SnowManaSpentToCast),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if low == "no treasure mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::TreasureManaSpentToCast),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if let Some(rest) = low.strip_prefix("no ")
                && let Some(mt) = parse_color_word_mana_type(rest)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if let Some(rest) = low.strip_prefix("only ")
                && let Some(mt) = parse_color_word_mana_type(rest)
            {
                return combine_resolution_conditions_and(vec![
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaSpentToCast),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::event(EventValueKind::ManaSpentToCast),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition");
            }
            if let Some(mt) = parse_color_word_mana_type(&low) {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" snow mana"))
                .or_else(|| low.strip_suffix(" or more snow mana"))
                && let Some(n) = parse_small_positive_count(rest)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::SnowManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if !low.starts_with("no ")
                && (low == "{s}"
                    || low == "snow mana"
                    || low == "snow"
                    || low.contains("snow mana"))
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::SnowManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if !low.starts_with("no ") && low.contains("treasure") {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::TreasureManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| {
                    s.strip_suffix(" mana from creatures")
                        .or_else(|| s.strip_suffix(" mana produced by creatures"))
                        .or_else(|| s.strip_suffix(" mana produced by a creature"))
                })
                .or_else(|| {
                    low.strip_suffix(" or more mana from creatures")
                        .or_else(|| low.strip_suffix(" or more mana produced by creatures"))
                        .or_else(|| low.strip_suffix(" or more mana produced by a creature"))
                })
                && let Some(n) = parse_small_positive_count(rest)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CreatureManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if low.contains("produced by a creature")
                || low == "creature mana"
                || low == "mana from a creature"
                || low == "mana from creatures"
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CreatureManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" mana of the same color"))
                .or_else(|| low.strip_suffix(" or more mana of the same color"))
                && let Some(n) = parse_small_positive_count(rest)
            {
                let per_color: Vec<ResolutionCondition> = [
                    ManaType::White,
                    ManaType::Blue,
                    ManaType::Black,
                    ManaType::Red,
                    ManaType::Green,
                ]
                .into_iter()
                .map(|mt| ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                })
                .collect();
                return combine_resolution_conditions_or(per_color)
                    .ok_or("oracle:unsupported-if-condition");
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" colors of mana"))
                .or_else(|| low.strip_suffix(" or more colors of mana"))
                && let Some(n) = parse_small_positive_count(rest)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ColorsSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if let Some(body) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" mana"))
                && let Some((count_part, color_part)) = body.trim().rsplit_once(' ')
                && let (Some(n), Some(mt)) = (
                    parse_small_positive_count(count_part),
                    parse_color_word_mana_type(color_part),
                )
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if let Some(body) = low.strip_suffix(" mana")
                && let Some((count_part, color_part)) = body.split_once(" or more ")
                && let (Some(n), Some(mt)) = (
                    parse_small_positive_count(count_part),
                    parse_color_word_mana_type(color_part),
                )
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" mana"))
                .or_else(|| low.strip_suffix(" or more mana"))
                && let Some(n) = parse_small_positive_count(rest)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            let mana_sym_upper = low
                .strip_suffix(" mana")
                .unwrap_or(&low)
                .trim()
                .to_ascii_uppercase();
            if let Some((mc, 0, hybrids)) = parse_mana_cost_full(&mana_sym_upper)
                && hybrids.is_empty()
                && mc.generic == 0.into()
                && !mc.required.is_empty()
                && mc.required.values().all(RulesInteger::is_positive)
            {
                let conds: Vec<ResolutionCondition> = mc
                    .required
                    .iter()
                    .map(|(&mt, amt)| ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::Literal(amt.clone()),
                    })
                    .collect();
                return combine_resolution_conditions_and(conds)
                    .ok_or("oracle:unsupported-if-condition");
            }
            let no_both = low.strip_prefix("both ").unwrap_or(&low);
            let pair_body = no_both.strip_suffix(" mana").unwrap_or(no_both).trim();
            if let Some((c1, c2)) = pair_body.split_once(" and ")
                && let (Some(mt1), Some(mt2)) = (
                    parse_color_word_mana_type(c1),
                    parse_color_word_mana_type(c2),
                )
            {
                let conds = vec![
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt1)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt2)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                ];
                return combine_resolution_conditions_and(conds)
                    .ok_or("oracle:unsupported-if-condition");
            }
            if let Some((c1, c2)) = pair_body.split_once(" or ")
                && let (Some(mt1), Some(mt2)) = (
                    parse_color_word_mana_type(c1),
                    parse_color_word_mana_type(c2),
                )
            {
                let conds = vec![
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt1)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt2)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                ];
                return combine_resolution_conditions_or(conds)
                    .ok_or("oracle:unsupported-if-condition");
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::OpponentLostLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::GainedLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyPlayer,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::MaxPlayerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::AttackedThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count }
            if *min_count > 0
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.stat_bounds.is_empty()
                && filter.token.is_none()
                && filter.flags.is_empty() =>
        {
            if filter.any_types.is_empty() {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                let stat = if kind == CardType::Creature {
                    PlayerStatExpr::CreatureEnteredUnderControlThisTurn
                } else {
                    PlayerStatExpr::EnteredTypeThisTurn(kind)
                };
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(stat),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.flags == AstFilterFlags::EXCLUDE_SELF
            && filter.any_types.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.required_supertypes.is_empty()
            && filter.forbidden_supertypes.is_empty()
            && filter.subtypes.is_empty()
            && filter.forbidden_subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.controller.is_none()
            && filter.owner.is_none()
            && filter.zone.is_none()
            && filter.extra_zones.is_empty()
            && filter.combat.is_none()
            && filter.tapped.is_none()
            && filter.token.is_none()
            && filter.required_keywords.is_empty()
            && filter.forbidden_keywords.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.counter_kind.is_none()
            && filter.named.is_none()
            && filter.predicates.is_empty() =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OtherSpellsCastThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            if filter.any_types.is_empty() && filter.forbidden_types.is_empty() {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if filter.any_types.is_empty() && filter.forbidden_types == AstCardTypeMask::CREATURE {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastNoncreatureSpellsThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if filter.forbidden_types.is_empty() {
                if filter.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                let mut types = card_type_mask_to_set(filter.any_types)?;
                if types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellTypeThisTurn(kind)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    filter: Box::new(obj),
                    from_zone: None,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::CastSpellThisTurn { player, filter } => {
            let players = match player {
                PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
                PlayerRefAst::AnyOpponent => AffectedPlayers::EachOpponent,
                PlayerRefAst::AnyPlayer => AffectedPlayers::EachPlayer,
                PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer => {
                    return Err("oracle:unsupported-if-condition");
                }
                other => AffectedPlayers::One(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            let from_zone = match filter.zone {
                None => None,
                Some(ZoneFilter::YourHand) => Some(SourceZoneFilter::Hand),
                Some(ZoneFilter::YourGraveyard) => Some(SourceZoneFilter::Graveyard),
                Some(ZoneFilter::YourHandOrGraveyard) => Some(SourceZoneFilter::HandOrGraveyard),
                Some(ZoneFilter::Battlefield) => Some(SourceZoneFilter::Battlefield),
                Some(ZoneFilter::Exile) => Some(SourceZoneFilter::Exile),
                Some(ZoneFilter::OpponentGraveyard) => Some(SourceZoneFilter::OpponentGraveyard),
                Some(
                    ZoneFilter::AnyGraveyard
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::SingleGraveyard,
                ) => Some(SourceZoneFilter::AnyGraveyard),
                Some(ZoneFilter::YourLibrary) => Some(SourceZoneFilter::YourLibrary),
                Some(_) => return Err("oracle:unsupported-if-condition"),
            };
            let mut clean_filter = filter.as_ref().clone();
            clean_filter.zone = None;
            let obj = lower_object_filter(&clean_filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                    from_zone,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::IsYourTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Delirium => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(4),
        }),
        ConditionAst::WasKickedSlot(slot_str) => {
            let slot: usize = slot_str.trim().parse().unwrap_or(0);
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::KickerCount(slot)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::TargetsFilter(f) if builder.target != BodyTarget::None => {
            let obj = lower_object_filter(f, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::TargetObject, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::SharesTypeWithSelf => Ok(ResolutionCondition {
            left: ValueExpr::CountOtherSharingSourceSubtype,
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::WonContest => {
            let last_was_clash = builder
                .instructions
                .iter()
                .rev()
                .find_map(|op| match op {
                    Instruction::MutatePlayer(spec) => match &spec.op {
                        spellwake_engine::toolbox::PlayerMutation::Clash => Some(true),
                        spellwake_engine::toolbox::PlayerMutation::FlipCoins { .. } => Some(false),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap_or(false);
            let kind = if last_was_clash {
                EventValueKind::LastActionSucceeded
            } else {
                EventValueKind::LastCoinFlipWon
            };
            Ok(ResolutionCondition {
                left: ValueExpr::event(kind),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count } if *min_count > 0 => {
            let mut inner = filter.as_ref().clone();
            inner.controller = Some(ControllerRelation::You);
            inner.flags.0 |= AstFilterFlags::ENTERED_THIS_TURN.0;
            let obj = lower_object_filter(&inner, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_battlefield(obj),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::CoinFlipOutcome { outcome } => {
            let low = outcome.trim().to_ascii_lowercase();
            if matches!(
                low.as_str(),
                "win"
                    | "won"
                    | "you win"
                    | "you won"
                    | "you win the flip"
                    | "you won the flip"
                    | "you win a flip"
                    | "you won a flip"
            ) {
                Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })
            } else if matches!(
                low.as_str(),
                "lose"
                    | "lost"
                    | "you lose"
                    | "you lost"
                    | "you lose the flip"
                    | "you lost the flip"
                    | "you lose a flip"
                    | "you lost a flip"
            ) {
                Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                })
            } else if let Some(cnt_raw) = low
                .strip_prefix("you win ")
                .or_else(|| low.strip_prefix("you won "))
                .and_then(|r| r.strip_suffix(" or more flips"))
                && let Some(n) = parse_small_positive_count(cnt_raw)
            {
                Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CoinFlipsWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::WasCast { by_you, from_hand }
            if !is_object_body_target(builder.target, false) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::WasCast {
                    by_controller: *by_you,
                    from_hand: *from_hand,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::IsPlayersTurn(player) => match player {
            PlayerRefAst::You => Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }),
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            }),
            PlayerRefAst::ThatPlayer if in_trigger => Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(
                    ResolvingPlayer::UniqueRelatedPlayer,
                )),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }),
            PlayerRefAst::DefendingPlayer
            | PlayerRefAst::EnchantedPlayer
            | PlayerRefAst::TargetPlayer
            | PlayerRefAst::TargetOpponent
            | PlayerRefAst::ChosenPlayer => {
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(rp)),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                })
            }
            _ => Err("oracle:unsupported-if-condition"),
        },
        ConditionAst::ZoneChangeThisTurn {
            filter,
            from_zone,
            to_zone,
            controller,
            min_count,
        } => {
            use spellwake_engine::program::OpponentTurnStat;
            let is_plain_extras = filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.owner.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && (filter.flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0) == 0
                && filter.predicates.is_empty();
            if matches!(from_zone, None | Some(ZoneFilter::Battlefield))
                && matches!(
                    to_zone,
                    Some(
                        ZoneFilter::AnyGraveyard
                            | ZoneFilter::AllGraveyards
                            | ZoneFilter::SingleGraveyard
                            | ZoneFilter::YourGraveyard
                            | ZoneFilter::OpponentGraveyard
                    )
                )
            {
                let from_battlefield_only = *from_zone == Some(ZoneFilter::Battlefield);
                let (ctrl, forced_ctrl) = match controller {
                    None => (None, None),
                    Some(PlayerRefAst::You) => (Some(ResolvingPlayer::Controller), None),
                    Some(PlayerRefAst::AnyOpponent) if *min_count == 1 => {
                        (None, Some(ControllerRelation::Opponent))
                    }
                    Some(
                        PlayerRefAst::AnyOpponent
                        | PlayerRefAst::EachOpponent
                        | PlayerRefAst::AnyPlayer
                        | PlayerRefAst::EachPlayer,
                    ) => return Err("oracle:unsupported-if-condition"),
                    Some(other) => (
                        Some(lower_resolving_player_in_context(
                            *other, builder, in_trigger,
                        )?),
                        None,
                    ),
                };
                if ctrl.is_some()
                    && filter.controller.is_some()
                    && !matches!(
                        (controller, filter.controller),
                        (
                            Some(PlayerRefAst::You),
                            Some(ControllerRelation::You | ControllerRelation::YourTeam)
                        )
                    )
                {
                    return Err("oracle:unsupported-if-condition");
                }
                if forced_ctrl.is_some()
                    && filter.controller.is_some()
                    && filter.controller != forced_ctrl
                {
                    return Err("oracle:unsupported-if-condition");
                }
                let mut clean = filter.as_ref().clone();
                if matches!(clean.zone, Some(ZoneFilter::Battlefield)) {
                    clean.zone = None;
                }
                if clean.zone.is_some() || !clean.extra_zones.is_empty() {
                    return Err("oracle:unsupported-if-condition");
                }
                if let Some(fc) = forced_ctrl {
                    clean.controller = Some(fc);
                }
                match to_zone {
                    Some(ZoneFilter::YourGraveyard) => {
                        if clean.owner.is_some() && clean.owner != Some(ControllerRelation::You) {
                            return Err("oracle:unsupported-if-condition");
                        }
                        clean.owner = Some(ControllerRelation::You);
                    }
                    Some(ZoneFilter::OpponentGraveyard) => {
                        if clean.owner.is_some()
                            && !matches!(
                                clean.owner,
                                Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                            )
                        {
                            return Err("oracle:unsupported-if-condition");
                        }
                        clean.owner = Some(ControllerRelation::Opponent);
                    }
                    _ => {}
                }
                let obj = lower_object_filter(&clean, vocabulary)?;
                let clause_ok = |c: ObjectClause| {
                    matches!(
                        c.subtypes,
                        SubtypeFilter::Any
                            | SubtypeFilter::Has(_)
                            | SubtypeFilter::Lacks(_)
                            | SubtypeFilter::AnyOf(_)
                            | SubtypeFilter::NoneOf(_)
                            | SubtypeFilter::Artifact(_)
                    ) && c.flags == ObjectFilterFlags::EMPTY
                        && c.source_relation == SourceRelation::Any
                        && c.combat == CombatFilter::Any
                        && c.power == PowerFilter::Any
                        && c.toughness == ToughnessFilter::Any
                        && c.mana_value == ManaValueFilter::Any
                        && c.keyword.is_none()
                        && c.forbidden_keyword.is_none()
                        && c.tapped.is_none()
                        && c.token.is_none()
                        && c.exact_name.is_none()
                        && c.required_counter_kind.is_none()
                        && c.forbidden_counter_kind.is_none()
                };
                let (c1, c2) = obj.clauses();
                if !clause_ok(c1) || c2.is_some_and(|c| !clause_ok(c)) {
                    return Err("oracle:unsupported-if-condition");
                }
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(obj),
                            from_battlefield_only,
                            controller: ctrl,
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            if *from_zone == Some(ZoneFilter::YourGraveyard)
                && to_zone.is_none()
                && controller.is_none()
                && is_unrestricted_filter(filter)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            if from_zone.is_none()
                && *to_zone == Some(ZoneFilter::Exile)
                && controller.is_none()
                && is_unrestricted_filter(filter)
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsExiledThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            if *from_zone == Some(ZoneFilter::Battlefield)
                && to_zone.is_none()
                && is_plain_extras
                && filter.zone.is_none()
            {
                let your_ctrl = matches!(controller, Some(PlayerRefAst::You))
                    || (controller.is_none()
                        && matches!(
                            filter.controller,
                            Some(ControllerRelation::You | ControllerRelation::YourTeam)
                        ));
                let opp_ctrl = matches!(controller, Some(PlayerRefAst::AnyOpponent))
                    || (controller.is_none()
                        && filter.controller == Some(ControllerRelation::Opponent));
                let any_ctrl = controller.is_none() && filter.controller.is_none();
                if your_ctrl && filter.any_types.is_empty() && filter.forbidden_types.is_empty() {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    });
                }
                if your_ctrl
                    && filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    });
                }
                if any_ctrl
                    && filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::AnyNonlandPermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    });
                }
                if opp_ctrl
                    && *min_count == 1
                    && filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::PermanentLeft,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                if any_ctrl
                    && *min_count == 1
                    && filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                {
                    return combine_resolution_conditions_or(vec![
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                                OpponentTurnStat::PermanentLeft,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                    ])
                    .ok_or("oracle:unsupported-if-condition");
                }
            }
            if from_zone.is_none()
                && *to_zone == Some(ZoneFilter::Battlefield)
                && is_plain_extras
                && filter.zone.is_none()
                && filter.forbidden_types.is_empty()
            {
                if matches!(controller, Some(PlayerRefAst::AnyOpponent))
                    && *min_count == 1
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && matches!(filter.controller, None | Some(ControllerRelation::Opponent))
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::CreatureEntered,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                if matches!(controller, Some(PlayerRefAst::You))
                    && matches!(
                        filter.controller,
                        None | Some(ControllerRelation::You | ControllerRelation::YourTeam)
                    )
                {
                    if filter.any_types.is_empty() {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        });
                    }
                    if filter.any_types == AstCardTypeMask::CREATURE {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::CreatureEnteredUnderControlThisTurn,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        });
                    }
                    if let Ok(mut types) = card_type_mask_to_set(filter.any_types)
                        && types.len() == 1
                        && let Some(kind) = types.pop_first()
                    {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredTypeThisTurn(kind)),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        });
                    }
                }
                if filter.any_types == AstCardTypeMask::LAND
                    && filter.controller.is_none()
                    && let Some(p) = controller
                    && !matches!(
                        p,
                        PlayerRefAst::You
                            | PlayerRefAst::AnyOpponent
                            | PlayerRefAst::EachOpponent
                            | PlayerRefAst::AnyPlayer
                            | PlayerRefAst::EachPlayer
                            | PlayerRefAst::EachOtherPlayer
                    )
                {
                    let rp = lower_resolving_player_in_context(*p, builder, in_trigger)?;
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(rp)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    });
                }
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::AnyOfGroupMatches { group, predicate } => {
            let base_flt = match group.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                    f.as_ref()
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let zone = match base_flt.zone {
                None | Some(ZoneFilter::Battlefield) => ValueZone::Battlefield,
                Some(ZoneFilter::YourGraveyard) => {
                    ValueZone::Graveyard(ResolvingPlayer::Controller)
                }
                Some(ZoneFilter::YourHand) => ValueZone::Hand(ResolvingPlayer::Controller),
                Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) => {
                    ValueZone::AllGraveyards
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            if zone != ValueZone::Battlefield && base_flt.controller.is_some() {
                return Err("oracle:unsupported-if-condition");
            }
            let mut merged = base_flt.clone();
            merged.zone = None;
            let pred_low = predicate.trim().trim_end_matches('.').to_ascii_lowercase();
            let pred_no_art = pred_low
                .strip_prefix("one or more ")
                .or_else(|| pred_low.strip_prefix("an "))
                .or_else(|| pred_low.strip_prefix("a "))
                .unwrap_or(&pred_low);
            let pred_no_loc = pred_no_art
                .strip_suffix(" on them")
                .or_else(|| pred_no_art.strip_suffix(" on it"))
                .unwrap_or(pred_no_art)
                .trim();
            if lower_keyword_ability_name(&pred_low).is_some() {
                merged.required_keywords.push(pred_low);
            } else if let Some(ck_raw) = pred_no_loc
                .strip_suffix(" counters")
                .or_else(|| pred_no_loc.strip_suffix(" counter"))
                && merged.counter_kind.is_none()
                && parse_counter_kind(ck_raw.trim()).is_some()
            {
                merged.counter_kind = Some(ck_raw.trim().to_string());
            } else {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                    &format!("permanent with {pred_low}"),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-if-condition")?;
                let only_pred_fields = parsed.any_types.is_empty()
                    && parsed.forbidden_types.is_empty()
                    && parsed.required_supertypes.is_empty()
                    && parsed.forbidden_supertypes.is_empty()
                    && parsed.subtypes.is_empty()
                    && parsed.forbidden_subtypes.is_empty()
                    && parsed.any_colors.is_empty()
                    && parsed.forbidden_colors.is_empty()
                    && parsed.color_arity.is_none()
                    && parsed.controller.is_none()
                    && parsed.owner.is_none()
                    && parsed.zone.is_none()
                    && parsed.extra_zones.is_empty()
                    && parsed.combat.is_none()
                    && parsed.tapped.is_none()
                    && parsed.token.is_none()
                    && parsed.forbidden_keywords.is_empty()
                    && parsed.named.is_none()
                    && parsed.flags.is_empty()
                    && parsed.predicates.is_empty()
                    && (!parsed.required_keywords.is_empty()
                        || !parsed.stat_bounds.is_empty()
                        || parsed.counter_kind.is_some());
                if !only_pred_fields {
                    return Err("oracle:unsupported-if-condition");
                }
                merged.required_keywords.extend(parsed.required_keywords);
                merged.stat_bounds.extend(parsed.stat_bounds);
                if let Some(ck) = parsed.counter_kind {
                    if merged.counter_kind.is_some() {
                        return Err("oracle:unsupported-if-condition");
                    }
                    merged.counter_kind = Some(ck);
                }
            }
            let obj = lower_object_filter(&merged, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::Count {
                    zone,
                    filter: Box::new(obj),
                    controller_scope: None,
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::GameStateFlag(flag) => {
            let fl = flag.trim().trim_end_matches('.').to_ascii_lowercase();
            match fl.as_str() {
                "day" | "it's day" | "it is day" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                }),
                "night" | "it's night" | "it is night" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(2),
                }),
                "neither day nor night"
                | "it's neither day nor night"
                | "it is neither day nor night" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "city's blessing" | "you have the city's blessing" | "citys_blessing" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerHasCityBlessing),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "monarch" | "you're the monarch" | "you are the monarch" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "an opponent is the monarch" | "opponent is the monarch" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentIsMonarch),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "there is no monarch" | "no one is the monarch" | "no monarch" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
                "initiative" | "you have the initiative" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::HasInitiative),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "max speed" | "you have max speed" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(4),
                }),
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        // Pass E12 typed ConditionAst lowerings (23 variants).
        ConditionAst::ControlsEachColor(player) => {
            let controller = match player {
                PlayerRefAst::You => ControllerFilter::You,
                PlayerRefAst::AnyOpponent => ControllerFilter::Opponent,
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    ControllerFilter::TargetPlayer
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    ControllerFilter::TargetOpponent
                }
                PlayerRefAst::ThatPlayer if in_trigger => ControllerFilter::RelatedPlayer,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::DistinctColors {
                    filter: Box::new(ObjectFilter::Clause(ObjectClause {
                        any_types: CardTypeMask::PERMANENT,
                        controller,
                        ..ObjectClause::any()
                    })),
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(5),
            })
        }
        ConditionAst::GainedLifeAtLeastThisTurn {
            player: PlayerRefAst::You,
            amount,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*amount)),
        }),
        ConditionAst::GainedLifeAtLeastThisTurn { .. } => Err("oracle:unsupported-if-condition"),
        ConditionAst::ResolutionCountThisTurn { count } if *count >= 1 => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(i64::from(*count - 1)),
        }),
        ConditionAst::ColorsOfManaSpentCompare { op, value } => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::ColorsSpentToCast),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::SubjectControlledBy { subject, player } => {
            let controller = match player {
                PlayerRefAst::You => ControllerFilter::You,
                PlayerRefAst::AnyOpponent => ControllerFilter::Opponent,
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    ControllerFilter::TargetPlayer
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    ControllerFilter::TargetOpponent
                }
                PlayerRefAst::ThatPlayer if in_trigger => ControllerFilter::RelatedPlayer,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let mref = if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = unwrap_battlefield_subject(subject)
            {
                bind_condition_target_object(filter, builder, vocabulary, false)?;
                MatchObjectRef::TargetObject
            } else {
                resolve_match_object_ref(subject, builder, in_trigger, false)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::Clause(ObjectClause {
                        controller,
                        ..ObjectClause::any()
                    }),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::SubjectInZone { subject, zone } => {
            let is_self_subj = matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a)
                        if is_self_anaphora(a)
                            || (a.eq_ignore_ascii_case("it")
                                && !is_object_body_target(builder.target, false))
                );
            if !is_self_subj {
                return Err("oracle:unsupported-if-condition");
            }
            use spellwake_engine::program::SourceZoneKind;
            let sz = match zone {
                ZoneFilter::Battlefield => SourceZoneKind::Battlefield,
                ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard => SourceZoneKind::Graveyard,
                ZoneFilter::YourHand => SourceZoneKind::Hand,
                ZoneFilter::Exile => SourceZoneKind::Exile,
                ZoneFilter::Stack => SourceZoneKind::Stack,
                ZoneFilter::YourLibrary => SourceZoneKind::Library,
                ZoneFilter::CommandZone => SourceZoneKind::Command,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(sz)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::TopCardMatches { zone, filter } => {
            let rp = match zone {
                ZoneFilter::YourLibrary => ResolvingPlayer::Controller,
                ZoneFilter::TargetPlayerLibrary => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    ResolvingPlayer::TargetPlayer
                }
                ZoneFilter::TargetOpponentLibrary => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    ResolvingPlayer::TargetPlayer
                }
                ZoneFilter::ThatPlayerLibrary => lower_resolving_player_in_context(
                    PlayerRefAst::ThatPlayer,
                    builder,
                    in_trigger,
                )?,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            if filter.controller.is_some()
                || filter.owner.is_some()
                || !filter.extra_zones.is_empty()
                || filter.named.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = filter.as_ref().clone();
            if cleaned.zone == Some(*zone) {
                cleaned.zone = None;
            }
            if cleaned.zone.is_some() {
                return Err("oracle:unsupported-if-condition");
            }
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::LibraryTop(rp), obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::WasCastFromZone { subject, zone } => {
            let is_self_subj = matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a)
                        if is_self_anaphora(a)
                            || (a.eq_ignore_ascii_case("it")
                                && !is_object_body_target(builder.target, false))
                );
            if !is_self_subj {
                return Err("oracle:unsupported-if-condition");
            }
            let left = match zone {
                ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard => {
                    ValueExpr::event(EventValueKind::CastFromGraveyard)
                }
                ZoneFilter::Exile => ValueExpr::event(EventValueKind::CastFromExile),
                ZoneFilter::YourHand => ValueExpr::SourceState(SourceStateExpr::WasCast {
                    by_controller: false,
                    from_hand: true,
                }),
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::PaidAltCost { subject, cost_kind } => {
            let is_self_subj = matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a)
                        if is_self_anaphora(a)
                            || (a.eq_ignore_ascii_case("it")
                                && !is_object_body_target(builder.target, false))
                );
            if !is_self_subj {
                return Err("oracle:unsupported-if-condition");
            }
            let left = match cost_kind.trim().to_ascii_lowercase().as_str() {
                "kicker" | "kicked" | "multikicker" | "offspring" | "squad" | "entwine"
                | "buyback" => ValueExpr::event(EventValueKind::KickerCount(0)),
                "bargain" | "bargained" => ValueExpr::event(EventValueKind::Bargained),
                "cleave" | "cleaved" => ValueExpr::event(EventValueKind::Cleaved),
                "gift" | "gifted" => ValueExpr::event(EventValueKind::GiftPromised),
                "overload"
                | "surge"
                | "spectacle"
                | "prowl"
                | "dash"
                | "blitz"
                | "evoke"
                | "escape"
                | "escaped"
                | "madness"
                | "miracle"
                | "foretell"
                | "foretold"
                | "plot"
                | "plotted"
                | "emerge"
                | "freerunning"
                | "warp"
                | "warped"
                | "web-slinging"
                | "bestow"
                | "disturb"
                | "mutate"
                | "mutated"
                | "flashback"
                | "prototype"
                | "mayhem"
                | "sneak"
                | "harmonize"
                | "encore"
                | "unearth"
                | "ninjutsu"
                | "suspend"
                | "teamwork"
                | "sprint"
                | "spree"
                | "additional"
                | "optional additional"
                | "additional cast" => {
                    if in_trigger {
                        ValueExpr::SourceState(SourceStateExpr::PaidAlternative)
                    } else {
                        ValueExpr::event(EventValueKind::PaidAlternative)
                    }
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left,
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::LifeComparePlayers { player, op, other } => {
            let flip_op = |c: CompareOp| match c {
                CompareOp::Lt => CompareOp::Gt,
                CompareOp::Le => CompareOp::Ge,
                CompareOp::Gt => CompareOp::Lt,
                CompareOp::Ge => CompareOp::Le,
                CompareOp::Eq => CompareOp::Eq,
            };
            let is_multi = |p: PlayerRefAst| {
                matches!(
                    p,
                    PlayerRefAst::AnyPlayer
                        | PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOtherPlayer
                        | PlayerRefAst::AnyOpponent
                        | PlayerRefAst::EachOpponent
                )
            };
            if *player == PlayerRefAst::You
                && matches!(
                    other,
                    PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer
                )
            {
                let rhs = match op {
                    CompareOp::Gt | CompareOp::Ge => PlayerStatExpr::OpponentMaxLife,
                    CompareOp::Lt | CompareOp::Le => PlayerStatExpr::OpponentMinLife,
                    CompareOp::Eq => return Err("oracle:unsupported-if-condition"),
                };
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: lower_comparison(*op),
                    right: ValueExpr::PlayerStat(rhs),
                })
            } else if *player == PlayerRefAst::You && *other == PlayerRefAst::AnyOpponent {
                let rhs = match op {
                    CompareOp::Gt | CompareOp::Ge => PlayerStatExpr::OpponentMinLife,
                    CompareOp::Lt | CompareOp::Le => PlayerStatExpr::OpponentMaxLife,
                    CompareOp::Eq => return Err("oracle:unsupported-if-condition"),
                };
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: lower_comparison(*op),
                    right: ValueExpr::PlayerStat(rhs),
                })
            } else if matches!(
                player,
                PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer
            ) && *other == PlayerRefAst::You
            {
                let flipped = flip_op(*op);
                let rhs = match flipped {
                    CompareOp::Gt | CompareOp::Ge => PlayerStatExpr::OpponentMaxLife,
                    CompareOp::Lt | CompareOp::Le => PlayerStatExpr::OpponentMinLife,
                    CompareOp::Eq => return Err("oracle:unsupported-if-condition"),
                };
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: lower_comparison(flipped),
                    right: ValueExpr::PlayerStat(rhs),
                })
            } else if *player == PlayerRefAst::AnyOpponent && *other == PlayerRefAst::You {
                let flipped = flip_op(*op);
                let rhs = match flipped {
                    CompareOp::Gt | CompareOp::Ge => PlayerStatExpr::OpponentMinLife,
                    CompareOp::Lt | CompareOp::Le => PlayerStatExpr::OpponentMaxLife,
                    CompareOp::Eq => return Err("oracle:unsupported-if-condition"),
                };
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: lower_comparison(flipped),
                    right: ValueExpr::PlayerStat(rhs),
                })
            } else if !is_multi(*player) && *other == PlayerRefAst::EachOtherPlayer {
                let allow_tie = match op {
                    CompareOp::Gt => false,
                    CompareOp::Ge => true,
                    _ => return Err("oracle:unsupported-if-condition"),
                };
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHasMostLife {
                        player: rp,
                        allow_tie,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })
            } else if !is_multi(*player) && !is_multi(*other) {
                let rp1 = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                let rp2 = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp1)),
                    comparison: lower_comparison(*op),
                    right: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp2)),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::ControlsMoreThan {
            player,
            filter,
            than_player,
        } => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !filter.extra_zones.is_empty()
                || filter.controller.is_some()
                || filter.owner.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = filter.as_ref().clone();
            cleaned.zone = None;
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::permanent()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            let is_multi = |p: PlayerRefAst| {
                matches!(
                    p,
                    PlayerRefAst::AnyPlayer
                        | PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOtherPlayer
                        | PlayerRefAst::AnyOpponent
                        | PlayerRefAst::EachOpponent
                )
            };
            if *player == PlayerRefAst::You
                && matches!(
                    than_player,
                    PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer
                )
            {
                Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                    comparison: Comparison::Greater,
                    right: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    },
                })
            } else if *player == PlayerRefAst::You && *than_player == PlayerRefAst::AnyOpponent {
                Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                    comparison: Comparison::Greater,
                    right: ValueExpr::OpponentMinCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    },
                })
            } else if !is_multi(*player) && *than_player == PlayerRefAst::EachOtherPlayer {
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerControlsMost {
                        player: rp,
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                        allow_tie: false,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })
            } else if !is_multi(*player) && !is_multi(*than_player) {
                let rp1 = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                let rp2 = lower_resolving_player_in_context(*than_player, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(rp1, obj),
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(rp2, obj),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::PlayerControlsMost {
            player,
            filter,
            allow_tie,
        } => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !filter.extra_zones.is_empty()
                || filter.controller.is_some()
                || filter.owner.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = filter.as_ref().clone();
            cleaned.zone = None;
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::permanent()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            if *player == PlayerRefAst::You {
                Ok(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                    comparison: if *allow_tie {
                        Comparison::GreaterOrEqual
                    } else {
                        Comparison::Greater
                    },
                    right: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    },
                })
            } else if !matches!(
                player,
                PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
            ) {
                let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
                Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerControlsMost {
                        player: rp,
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                        allow_tie: *allow_tie,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })
            } else {
                Err("oracle:unsupported-if-condition")
            }
        }
        ConditionAst::PlayerCoinFlipOutcome { player, outcome } => {
            let prior_flip = builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::FlipCoins { .. }
                        )
                )
            });
            let player_ok =
                *player == PlayerRefAst::You || (*player == PlayerRefAst::ThatPlayer && prior_flip);
            if !player_ok {
                return Err("oracle:unsupported-if-condition");
            }
            match outcome.trim().to_ascii_lowercase().as_str() {
                "win" | "won" | "heads" | "head" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }),
                "lose" | "lost" | "tails" | "tail" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }),
                "both_heads" | "both heads" | "both_win" | "both win" => Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CoinFlipsWon),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                }),
                "both_tails" | "both tails" | "both_lose" | "both lose" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::CoinFlipsWon),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })
                }
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::DieRollCompare {
            player: PlayerRefAst::You,
            op,
            value,
            any_of_group: false,
            exact_times: None,
        } => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastDieRoll),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::PriorActionOccurred {
            action,
            subject_filter,
            min_count,
        } => {
            if builder.instructions.is_empty() || *min_count == 0 {
                return Err("oracle:unsupported-if-condition");
            }
            let act_low = action.trim().to_ascii_lowercase();
            let zone_mref: Option<MatchObjectRef> = match act_low.as_str() {
                "sacrificed"
                | "sacrificed_this_way"
                | "discarded"
                | "discarded_this_way"
                | "discard_this_way"
                | "destroyed"
                | "destroyed_this_way"
                | "milled"
                | "milled_this_way"
                | "died"
                | "died_this_way"
                | "put_into_graveyard"
                | "put_into_graveyard_this_way"
                | "put_into_a_graveyard_this_way" => Some(MatchObjectRef::LastMovedToGraveyard),
                "exiled" | "exiled_this_way" | "exile_this_way" => {
                    Some(MatchObjectRef::LastMovedToExile)
                }
                "returned_to_hand"
                | "returned_to_hand_this_way"
                | "put_into_hand"
                | "put_into_hand_this_way"
                | "bounced"
                | "bounced_this_way" => Some(MatchObjectRef::LastMovedToHand),
                "put_onto_battlefield"
                | "put_onto_battlefield_this_way"
                | "entered_this_way"
                | "entered_battlefield_this_way"
                | "returned_to_battlefield"
                | "returned_to_battlefield_this_way" => {
                    Some(MatchObjectRef::LastMovedToBattlefield)
                }
                "countered"
                | "countered_this_way"
                | "spell_countered_this_way"
                | "tapped"
                | "tapped_this_way"
                | "untapped"
                | "untapped_this_way"
                | "removed"
                | "removed_this_way"
                | "gained_control"
                | "gained_control_this_way"
                | "gain_control_this_way"
                | "cast"
                | "cast_this_way"
                | "drawn"
                | "drew"
                | "drew_this_way"
                | "draw_this_way"
                | "exchanged_this_way"
                | "put_on_top_of_library_this_way"
                | "copied"
                | "copied_this_way"
                | "revealed"
                | "revealed_this_way"
                | "moved"
                | "moved_this_way"
                | "did"
                | "paid"
                | "paid_this_way"
                | "succeeded"
                | "dealt_damage"
                | "dealt_damage_this_way"
                | "life_lost"
                | "lost_life_this_way"
                | "searched"
                | "searched_this_way"
                | "explored"
                | "explored_this_way"
                | "surveiled"
                | "surveiled_this_way"
                | "scried"
                | "scried_this_way"
                | "investigated"
                | "proliferated"
                | "connived"
                | "adapted"
                | "monstrous"
                | "manifested"
                | "cloaked"
                | "collected_evidence"
                | "foraged"
                | "blighted"
                | "earthbent"
                | "regenerated"
                | "transformed"
                | "turned_face_up"
                | "phased_out"
                | "goaded"
                | "attached"
                | "unattached"
                | "equipped"
                | "created"
                | "created_this_way"
                | "returned"
                | "returned_this_way"
                | "chosen"
                | "chosen_this_way" => None,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            match subject_filter.as_deref() {
                None => Ok(ResolutionCondition {
                    left: ValueExpr::event(if *min_count == 1 {
                        EventValueKind::LastActionSucceeded
                    } else {
                        EventValueKind::LastActionCount
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                }),
                Some(f)
                    if is_unrestricted_filter(f)
                        || (act_low == "exchanged_this_way"
                            && f.any_permanent
                            && f.any_types.is_empty()
                            && f.forbidden_types.is_empty()
                            && f.required_supertypes.is_empty()
                            && f.forbidden_supertypes.is_empty()
                            && f.subtypes.is_empty()
                            && f.forbidden_subtypes.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.color_arity.is_none()
                            && f.zone.is_none()
                            && f.extra_zones.is_empty()
                            && f.controller.is_none()
                            && f.owner.is_none()
                            && f.combat.is_none()
                            && f.tapped.is_none()
                            && f.token.is_none()
                            && f.required_keywords.is_empty()
                            && f.forbidden_keywords.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.counter_kind.is_none()
                            && f.named.is_none()
                            && f.flags.is_empty()
                            && f.predicates.is_empty()) =>
                {
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(if *min_count == 1 {
                            EventValueKind::LastActionSucceeded
                        } else {
                            EventValueKind::LastActionCount
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })
                }
                Some(f) => {
                    let has_lki = matches!(
                        builder.instructions.last(),
                        Some(
                            Instruction::ZoneOp(_)
                                | Instruction::ForEachPlayerQuery { .. }
                                | Instruction::MutateObject(_)
                        )
                    ) || matches!(
                        builder.instructions.last(),
                        Some(Instruction::PromptChoice(spec))
                            if matches!(spec.domain, spellwake_engine::toolbox::PromptDomain::Objects { .. })
                    );
                    let lki_action = matches!(
                        act_low.as_str(),
                        "discarded"
                            | "discarded_this_way"
                            | "discard_this_way"
                            | "sacrificed"
                            | "sacrificed_this_way"
                            | "exiled"
                            | "exiled_this_way"
                            | "tapped"
                            | "tapped_this_way"
                            | "untapped"
                            | "untapped_this_way"
                            | "chosen"
                            | "chosen_this_way"
                    );
                    if has_lki
                        && lki_action
                        && f.zone.is_none()
                        && f.extra_zones.is_empty()
                        && f.controller.is_none()
                        && f.owner.is_none()
                        && f.named.is_none()
                        && f.predicates.is_empty()
                        && f.flags.is_empty()
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        if !obj.has_target_player() {
                            return Ok(ResolutionCondition {
                                left: ValueExpr::LastActionMatchingCount(Box::new(obj)),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(i64::from(*min_count)),
                            });
                        }
                    }
                    if *min_count != 1 {
                        return Err("oracle:unsupported-if-condition");
                    }
                    let Some(mref) = zone_mref else {
                        return Err("oracle:unsupported-if-condition");
                    };
                    if !last_moved_subject_safe(builder, mref)
                        || f.zone.is_some()
                        || !f.extra_zones.is_empty()
                        || f.controller.is_some()
                        || f.owner.is_some()
                        || f.named.is_some()
                        || !f.predicates.is_empty()
                        || !f.flags.is_empty()
                    {
                        return Err("oracle:unsupported-if-condition");
                    }
                    let mut f_clean = f.clone();
                    if f_clean.any_card
                        && f_clean.token.is_none()
                        && mref == MatchObjectRef::LastMovedToGraveyard
                    {
                        f_clean.token = Some(false);
                    }
                    let obj = lower_object_filter(&f_clean, vocabulary)?;
                    Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(mref, obj),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })
                }
            }
        }
        ConditionAst::IsPhaseOrStep {
            player,
            phase,
            first_only,
        } => {
            use spellwake_engine::program::PhaseCheck;
            let ph = phase
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .replace('_', " ");
            if *first_only {
                if matches!(ph.as_str(), "combat" | "combat phase")
                    && matches!(player, None | Some(PlayerRefAst::You))
                {
                    let combat_first = ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CombatPhasesThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    };
                    if matches!(player, Some(PlayerRefAst::You)) {
                        return combine_resolution_conditions_and(vec![
                            combat_first,
                            ResolutionCondition {
                                left: ValueExpr::PlayerStat(
                                    PlayerStatExpr::ControllerIsActivePlayer,
                                ),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(1),
                            },
                        ])
                        .ok_or("oracle:unsupported-if-condition");
                    }
                    return Ok(combat_first);
                }
                return Err("oracle:unsupported-if-condition");
            }
            let (opt_rp, require_opponent_turn) = match player {
                None | Some(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer) => (None, false),
                Some(
                    PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherOpponent,
                ) => (None, true),
                Some(p) => (
                    Some(lower_resolving_player_in_context(*p, builder, in_trigger)?),
                    false,
                ),
            };
            let phase_cond = if let Some(step) = parse_step_name(&ph) {
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::IsInStep {
                        step,
                        player: opt_rp,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }
            } else {
                let pc = match ph.as_str() {
                    "beginning" | "beginning phase" => PhaseCheck::Beginning,
                    "main" | "main phase" => PhaseCheck::Main,
                    "combat" | "combat phase" => PhaseCheck::Combat,
                    "ending" | "ending phase" => PhaseCheck::Ending,
                    _ => return Err("oracle:unsupported-if-condition"),
                };
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::IsInPhase {
                        phase: pc,
                        player: opt_rp,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }
            };
            if require_opponent_turn {
                combine_resolution_conditions_and(vec![
                    phase_cond,
                    ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition")
            } else {
                Ok(phase_cond)
            }
        }
        ConditionAst::CardWithNameInZone {
            name_filter:
                name_filter @ (spellwake_oracle_compiler::ast::NameFilterAst::SelfName
                | spellwake_oracle_compiler::ast::NameFilterAst::Exact(_)),
            filter,
            zone,
            op,
            count,
        } => {
            let vz = match zone {
                ZoneFilter::Battlefield => ValueZone::Battlefield,
                ZoneFilter::YourGraveyard => ValueZone::Graveyard(ResolvingPlayer::Controller),
                ZoneFilter::AnyGraveyard => ValueZone::AllGraveyards,
                ZoneFilter::YourHand => ValueZone::Hand(ResolvingPlayer::Controller),
                ZoneFilter::Exile => ValueZone::Exile(None),
                ZoneFilter::YourLibrary => ValueZone::Library(ResolvingPlayer::Controller),
                ZoneFilter::TargetPlayerGraveyard => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    ValueZone::Graveyard(ResolvingPlayer::TargetPlayer)
                }
                ZoneFilter::TargetOpponentGraveyard => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    ValueZone::Graveyard(ResolvingPlayer::TargetPlayer)
                }
                ZoneFilter::ThatPlayerGraveyard => {
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    ValueZone::Graveyard(rp)
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            if !filter.extra_zones.is_empty() || filter.named.is_some() {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = filter.as_ref().clone();
            if cleaned.zone == Some(*zone) {
                cleaned.zone = None;
            }
            if cleaned.zone.is_some() {
                return Err("oracle:unsupported-if-condition");
            }
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            let named_obj = match name_filter {
                spellwake_oracle_compiler::ast::NameFilterAst::SelfName => {
                    obj.sharing_name_with_source()
                }
                spellwake_oracle_compiler::ast::NameFilterAst::Exact(name) => {
                    let key = spellwake_engine::program::ExactNameKey::from_name(name)
                        .ok_or("oracle:unsupported-if-condition")?;
                    let mut with_name = obj;
                    match &mut with_name {
                        ObjectFilter::Clause(c) => {
                            if c.exact_name.is_some() {
                                return Err("oracle:unsupported-if-condition");
                            }
                            c.exact_name = Some(key);
                        }
                        ObjectFilter::Either(a, b) => {
                            if a.exact_name.is_some() || b.exact_name.is_some() {
                                return Err("oracle:unsupported-if-condition");
                            }
                            a.exact_name = Some(key);
                            b.exact_name = Some(key);
                        }
                    }
                    with_name
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::Count {
                    zone: vz,
                    filter: Box::new(named_obj),
                    controller_scope: None,
                },
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*count)),
            })
        }
        ConditionAst::BlockedByFilter {
            subject,
            blocker_filter,
            min_count,
        } => {
            let is_self_subj = matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a));
            if !is_self_subj
                || *min_count == 0
                || blocker_filter.combat.is_some()
                || !matches!(blocker_filter.zone, None | Some(ZoneFilter::Battlefield))
                || !blocker_filter.extra_zones.is_empty()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = blocker_filter.as_ref().clone();
            cleaned.zone = None;
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::creature()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::count_battlefield(obj.combat(CombatFilter::BlockingSource)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::DamageDealtThisTurn {
            source,
            target_filter,
            combat_only,
            min_damage,
        } => {
            let vref = resolve_value_object_ref_for_stat(source, builder, in_trigger, false)
                .ok_or("oracle:unsupported-if-condition")?;
            let min_val = min_damage.unwrap_or(1);
            if min_val == 0 {
                return Err("oracle:unsupported-if-condition");
            }
            match target_filter.as_deref() {
                None => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                        subject: vref,
                        to_opponent_only: false,
                        combat_only: *combat_only,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(min_val)),
                }),
                Some(tf) => {
                    let mut without_ctrl = tf.clone();
                    without_ctrl.controller = None;
                    let is_opponent_only = tf.controller == Some(ControllerRelation::Opponent)
                        && without_ctrl.is_empty();
                    if !is_opponent_only {
                        return Err("oracle:unsupported-if-condition");
                    }
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                            subject: vref,
                            to_opponent_only: true,
                            combat_only: *combat_only,
                        }),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(min_val)),
                    })
                }
            }
        }
        ConditionAst::NumberParity { even } => {
            let chosen = ValueExpr::event(EventValueKind::ChosenNumber);
            let remainder = ValueExpr::Sub(
                Box::new(chosen.clone()),
                Box::new(ValueExpr::Scale {
                    expr: Box::new(chosen.half_down()),
                    factor: 2.into(),
                }),
            );
            Ok(ResolutionCondition {
                left: remainder,
                comparison: Comparison::Equal,
                right: ValueExpr::literal(if *even { 0 } else { 1 }),
            })
        }
        ConditionAst::AllColors(subject) => {
            let mref = if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = unwrap_battlefield_subject(subject)
            {
                bind_condition_target_object(filter, builder, vocabulary, false)?;
                MatchObjectRef::TargetObject
            } else {
                resolve_match_object_ref(subject, builder, in_trigger, false)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            let conds: Vec<ResolutionCondition> = [
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]
            .into_iter()
            .map(|c| ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::Clause(ObjectClause {
                        color: ObjectColorFilter::has(c),
                        ..ObjectClause::any()
                    }),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
            .collect();
            combine_resolution_conditions_and(conds).ok_or("oracle:unsupported-if-condition")
        }
        ConditionAst::AttachedTo { subject, host } => {
            let is_self_subj = matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a));
            if !is_self_subj {
                return Err("oracle:unsupported-if-condition");
            }
            let host_filter = match unwrap_battlefield_subject(host) {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => f,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            if !matches!(host_filter.zone, None | Some(ZoneFilter::Battlefield))
                || !host_filter.extra_zones.is_empty()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = host_filter.as_ref().clone();
            cleaned.zone = None;
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::permanent()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::AttachedHost, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::SameNameAs { subject, other } => {
            let is_self_subj = |s: &SubjectRefAst| {
                matches!(s, SubjectRefAst::SelfObject)
                    || matches!(s, SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            };
            let other_subj = if is_self_subj(subject) && !is_self_subj(other) {
                other.as_ref()
            } else if is_self_subj(other) && !is_self_subj(subject) {
                subject.as_ref()
            } else {
                return Err("oracle:unsupported-if-condition");
            };
            let other_mref = if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = unwrap_battlefield_subject(other_subj)
            {
                bind_condition_target_object(filter, builder, vocabulary, false)?;
                MatchObjectRef::TargetObject
            } else {
                resolve_match_object_ref(other_subj, builder, in_trigger, false)
                    .ok_or("oracle:unsupported-if-condition")?
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    other_mref,
                    ObjectFilter::any().sharing_name_with_source(),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::AttackedWithFilteredThisTurn {
            player,
            filter,
            min_count,
        } => {
            if *min_count == 0
                || !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !filter.extra_zones.is_empty()
                || filter.controller.is_some()
                || filter.owner.is_some()
                || filter.named.is_some()
            {
                return Err("oracle:unsupported-if-condition");
            }
            let mut cleaned = filter.as_ref().clone();
            cleaned.zone = None;
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::creature()
            } else {
                lower_object_filter(&cleaned, vocabulary)?
            };
            if *player == PlayerRefAst::You && obj == ObjectFilter::creature() {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::AttackingCreaturesThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            if !body_turn_record_filter_supported(obj, true) {
                return Err("oracle:unsupported-if-condition");
            }
            let players = match player {
                PlayerRefAst::You => AffectedPlayers::One(ResolvingPlayer::Controller),
                PlayerRefAst::AnyOpponent if *min_count == 1 => AffectedPlayers::EachOpponent,
                PlayerRefAst::AnyPlayer if *min_count == 1 => AffectedPlayers::EachPlayer,
                PlayerRefAst::AnyOpponent
                | PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOtherPlayer => {
                    return Err("oracle:unsupported-if-condition");
                }
                other => {
                    let rp = lower_resolving_player_in_context(*other, builder, in_trigger)?;
                    AffectedPlayers::One(rp)
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                    players,
                    filter: Box::new(obj),
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::SearchedLibraryThisWay if !builder.instructions.is_empty() => {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}

pub(super) fn lower_zone_filter_to_query_zone(
    zone: ZoneFilter,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Option<spellwake_engine::toolbox::QueryZone> {
    use spellwake_engine::toolbox::{PlayerSelector, QueryZone};
    match zone {
        ZoneFilter::Battlefield => Some(QueryZone::Battlefield),
        ZoneFilter::Stack => Some(QueryZone::Stack),
        ZoneFilter::Exile => Some(QueryZone::Exile),
        ZoneFilter::CommandZone => Some(QueryZone::Command),
        ZoneFilter::YourGraveyard => Some(QueryZone::Graveyard(Box::new(
            PlayerSelector::Resolving(ResolvingPlayer::Controller),
        ))),
        ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards => {
            Some(QueryZone::Graveyard(Box::new(PlayerSelector::Group(
                AffectedPlayers::EachOpponent,
            ))))
        }
        ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards => Some(QueryZone::Graveyard(
            Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer)),
        )),
        ZoneFilter::TargetPlayerGraveyard => {
            builder.bind_target(BodyTarget::SinglePlayer).ok()?;
            Some(QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::TargetOpponentGraveyard => {
            builder.bind_target(BodyTarget::SingleOpponent).ok()?;
            Some(QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::ThatPlayerGraveyard => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)
                    .ok()?;
            Some(QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                rp,
            ))))
        }
        ZoneFilter::DefendingPlayerGraveyard => Some(QueryZone::Graveyard(Box::new(
            PlayerSelector::Resolving(ResolvingPlayer::DefendingPlayer),
        ))),
        ZoneFilter::YourHand => Some(QueryZone::Hand(Box::new(PlayerSelector::Resolving(
            ResolvingPlayer::Controller,
        )))),
        ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands => Some(QueryZone::Hand(Box::new(
            PlayerSelector::Group(AffectedPlayers::EachOpponent),
        ))),
        ZoneFilter::EachPlayerHand | ZoneFilter::AnyHand => Some(QueryZone::Hand(Box::new(
            PlayerSelector::Group(AffectedPlayers::EachPlayer),
        ))),
        ZoneFilter::TargetPlayerHand => {
            builder.bind_target(BodyTarget::SinglePlayer).ok()?;
            Some(QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::TargetOpponentHand => {
            builder.bind_target(BodyTarget::SingleOpponent).ok()?;
            Some(QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::ThatPlayerHand => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)
                    .ok()?;
            Some(QueryZone::Hand(Box::new(PlayerSelector::Resolving(rp))))
        }
        ZoneFilter::YourLibrary => Some(QueryZone::Library(Box::new(PlayerSelector::Resolving(
            ResolvingPlayer::Controller,
        )))),
        ZoneFilter::OpponentLibrary => Some(QueryZone::Library(Box::new(PlayerSelector::Group(
            AffectedPlayers::EachOpponent,
        )))),
        ZoneFilter::EachPlayerLibrary | ZoneFilter::AnyLibrary => Some(QueryZone::Library(
            Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer)),
        )),
        ZoneFilter::TargetPlayerLibrary => {
            builder.bind_target(BodyTarget::SinglePlayer).ok()?;
            Some(QueryZone::Library(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::TargetOpponentLibrary => {
            builder.bind_target(BodyTarget::SingleOpponent).ok()?;
            Some(QueryZone::Library(Box::new(PlayerSelector::Resolving(
                ResolvingPlayer::TargetPlayer,
            ))))
        }
        ZoneFilter::ThatPlayerLibrary => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)
                    .ok()?;
            Some(QueryZone::Library(Box::new(PlayerSelector::Resolving(rp))))
        }
        _ => None,
    }
}

pub(super) fn lower_filter_to_zone_query(
    filter: &ObjectFilterAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::toolbox::ObjectQuery> {
    use spellwake_engine::toolbox::{ObjectQuery, PlayerSelector, QueryZone};
    if !filter.extra_zones.is_empty() {
        return None;
    }
    let mut clean = filter.clone();
    let z = clean.zone.take().unwrap_or(ZoneFilter::Battlefield);
    let obj = if is_unrestricted_filter(&clean) {
        if z == ZoneFilter::Battlefield {
            ObjectFilter::permanent()
        } else {
            ObjectFilter::any()
        }
    } else {
        lower_object_filter(&clean, vocabulary).ok()?
    };
    match z {
        ZoneFilter::YourGraveyardOrExile => Some(ObjectQuery::Union(vec![
            ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                obj,
            ),
            ObjectQuery::zone(
                QueryZone::ExileOwnedBy(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                obj,
            ),
        ])),
        ZoneFilter::AnyGraveyardOrExile => Some(ObjectQuery::Union(vec![
            ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer))),
                obj,
            ),
            ObjectQuery::zone(QueryZone::Exile, obj),
        ])),
        ZoneFilter::YourHandOrGraveyard => Some(ObjectQuery::Union(vec![
            ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                obj,
            ),
            ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::Controller,
                ))),
                obj,
            ),
        ])),
        other => {
            let qz = lower_zone_filter_to_query_zone(other, builder, in_trigger)?;
            Some(ObjectQuery::zone(qz, obj))
        }
    }
}

pub(super) fn lower_subject_to_object_query(
    subject: &SubjectRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::toolbox::ObjectQuery> {
    use spellwake_engine::toolbox::ObjectQuery;
    match subject {
        SubjectRefAst::SelfObject => Some(ObjectQuery::Source),
        SubjectRefAst::TriggerSubject if in_trigger => Some(ObjectQuery::TriggerSubject),
        SubjectRefAst::AttachedHost(_) => Some(ObjectQuery::AttachedHost),
        SubjectRefAst::InZone {
            subject: inner,
            zone,
        } => {
            let base = lower_subject_to_object_query(inner, builder, in_trigger, vocabulary)?;
            let qz = lower_zone_filter_to_query_zone(*zone, builder, in_trigger)?;
            Some(ObjectQuery::Intersect(
                Box::new(base),
                Box::new(ObjectQuery::zone(qz, ObjectFilter::any())),
            ))
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            bind_condition_target_object(filter, builder, vocabulary, true).ok()?;
            Some(ObjectQuery::Targets)
        }
        SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
            lower_filter_to_zone_query(f, builder, in_trigger, vocabulary)
        }
        SubjectRefAst::Anaphora(a) => {
            let al = a.trim().to_ascii_lowercase();
            if is_self_anaphora(&al) {
                return Some(ObjectQuery::Source);
            }
            if matches!(
                al.as_str(),
                "equipped creature"
                    | "enchanted creature"
                    | "enchanted permanent"
                    | "fortified land"
            ) {
                return Some(ObjectQuery::AttachedHost);
            }
            if matches!(
                al.as_str(),
                "the revealed card"
                    | "revealed card"
                    | "that revealed card"
                    | "the revealed cards"
                    | "those revealed cards"
                    | "the card revealed this way"
                    | "the cards revealed this way"
            ) {
                return Some(ObjectQuery::Revealed);
            }
            if matches!(
                al.as_str(),
                "that token"
                    | "those tokens"
                    | "the token"
                    | "the tokens"
                    | "the created token"
                    | "the created tokens"
            ) {
                return Some(ObjectQuery::CreatedTokens);
            }
            if matches!(
                al.as_str(),
                "them"
                    | "those cards"
                    | "those permanents"
                    | "those creatures"
                    | "the exiled card"
                    | "the exiled cards"
                    | "that exiled card"
                    | "the discarded card"
                    | "the discarded cards"
                    | "the milled cards"
                    | "the sacrificed creature"
                    | "the sacrificed permanent"
                    | "the returned card"
                    | "the returned creature"
            ) && !builder.instructions.is_empty()
            {
                let has_tokens = builder
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::CreateTokens { .. }));
                let has_zone_op = builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::ZoneOp(_)
                            | Instruction::ForEachPlayerQuery { .. }
                            | Instruction::LookTopChooseExt { .. }
                    ) || matches!(
                        op,
                        Instruction::MutatePlayer(spec)
                            if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_))
                    )
                });
                if has_zone_op {
                    return Some(ObjectQuery::LastMoved);
                }
                if has_tokens {
                    return Some(ObjectQuery::CreatedTokens);
                }
            }
            if is_object_body_target(builder.target, true) {
                return Some(ObjectQuery::Targets);
            }
            if in_trigger {
                return Some(ObjectQuery::TriggerSubject);
            }
            if al == "it" && builder.has_source && builder.instructions.is_empty() {
                return Some(ObjectQuery::Source);
            }
            None
        }
        _ => None,
    }
}

fn parse_status_or_predicate_filter(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let stripped = low
        .strip_prefix("is ")
        .or_else(|| low.strip_prefix("are "))
        .or_else(|| low.strip_prefix("was "))
        .or_else(|| low.strip_prefix("were "))
        .or_else(|| low.strip_prefix("has "))
        .or_else(|| low.strip_prefix("have "))
        .unwrap_or(low.as_str())
        .trim();
    match stripped {
        "tapped" => return Some(ObjectFilter::any().tapped()),
        "untapped" => return Some(ObjectFilter::Clause(ObjectClause::any().untapped())),
        "attacking" => return Some(ObjectFilter::any().attacking()),
        "blocking" => return Some(ObjectFilter::any().combat(CombatFilter::Blocking)),
        "attacking or blocking" => return Some(ObjectFilter::any().attacking_or_blocking()),
        "blocked" => return Some(ObjectFilter::any().combat(CombatFilter::Blocked)),
        "unblocked" => return Some(ObjectFilter::any().combat(CombatFilter::Unblocked)),
        "enchanted" => {
            let mut c = ObjectClause::any();
            c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
            return Some(ObjectFilter::Clause(c));
        }
        "equipped" => {
            let mut c = ObjectClause::any();
            c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
            return Some(ObjectFilter::Clause(c));
        }
        "token" | "a token" | "tokens" => {
            let mut c = ObjectClause::any();
            c.token = Some(true);
            return Some(ObjectFilter::Clause(c));
        }
        "nontoken" | "not a token" => {
            let mut c = ObjectClause::any();
            c.token = Some(false);
            return Some(ObjectFilter::Clause(c));
        }
        _ => {}
    }
    if let Some(kw) = lower_keyword_ability_name(stripped) {
        return Some(ObjectFilter::any().with_keyword(kw));
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    if let Ok(ast_f) =
        spellwake_oracle_compiler::filter::parse_object_filter(stripped, &compiler_vocab)
        && ast_f.zone.is_none()
        && ast_f.extra_zones.is_empty()
        && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
    {
        return Some(obj);
    }
    None
}

pub(super) fn lower_condition_expr(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<spellwake_engine::toolbox::ConditionExpr, &'static str> {
    use spellwake_engine::toolbox::{
        ConditionExpr, ObjectQuery, PlayerSelector, QuerySlice, QueryZone,
    };
    match cond {
        ConditionAst::And(a, b) => {
            let ea = lower_condition_expr(a, builder, vocabulary, in_trigger, bound_x)?;
            let eb = lower_condition_expr(b, builder, vocabulary, in_trigger, bound_x)?;
            return Ok(ConditionExpr::And(vec![ea, eb]));
        }
        ConditionAst::Or(a, b) => {
            let ea = lower_condition_expr(a, builder, vocabulary, in_trigger, bound_x)?;
            let eb = lower_condition_expr(b, builder, vocabulary, in_trigger, bound_x)?;
            return Ok(ConditionExpr::Or(vec![ea, eb]));
        }
        ConditionAst::Not(inner) => {
            let ei = lower_condition_expr(inner, builder, vocabulary, in_trigger, bound_x)?;
            return Ok(ConditionExpr::Not(Box::new(ei)));
        }
        _ => {}
    }
    let snap_target = builder.target;
    let snap_gy = builder.graveyard_target_bounds;
    let snap_perm = builder.permanent_target_bounds;
    let snap_x = builder.x_target_count;
    let snap_up_to_x = builder.up_to_x_target_count;
    let snap_group = builder.last_group_filter;
    if let Ok(rc) = lower_resolution_condition(cond, builder, vocabulary, in_trigger, bound_x) {
        return Ok(ConditionExpr::Compare {
            left: Box::new(rc.left),
            op: rc.comparison,
            right: Box::new(rc.right),
        });
    }
    builder.target = snap_target;
    builder.graveyard_target_bounds = snap_gy;
    builder.permanent_target_bounds = snap_perm;
    builder.x_target_count = snap_x;
    builder.up_to_x_target_count = snap_up_to_x;
    builder.last_group_filter = snap_group;

    match cond {
        ConditionAst::MatchesFilter { subject, filter } => {
            let q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let obj = lower_object_filter(&clean, vocabulary)?;
            if matches!(
                subject.as_ref(),
                SubjectRefAst::All(_) | SubjectRefAst::Each(_)
            ) {
                Ok(ConditionExpr::AllMatch {
                    query: Box::new(q),
                    filter: Box::new(obj),
                    require_non_empty: true,
                })
            } else {
                Ok(ConditionExpr::AnyMatch {
                    query: Box::new(q),
                    filter: Box::new(obj),
                })
            }
        }
        ConditionAst::AnyOfGroupMatches { group, predicate } => {
            let q = lower_subject_to_object_query(group, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let obj = parse_status_or_predicate_filter(predicate, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::AnyMatch {
                query: Box::new(q),
                filter: Box::new(obj),
            })
        }
        ConditionAst::SubjectInZone { subject, zone }
        | ConditionAst::Marker(GameMarker::InZone { subject, zone }) => {
            let q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let qz = lower_zone_filter_to_query_zone(*zone, builder, in_trigger)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::NonEmpty(Box::new(ObjectQuery::Intersect(
                Box::new(q),
                Box::new(ObjectQuery::zone(qz, ObjectFilter::any())),
            ))))
        }
        ConditionAst::TopCardMatches { zone, filter } => {
            let qz = lower_zone_filter_to_query_zone(*zone, builder, in_trigger)
                .ok_or("oracle:unsupported-if-condition")?;
            if !filter.extra_zones.is_empty() || filter.named.is_some() {
                return Err("oracle:unsupported-if-condition");
            }
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&clean, vocabulary)?
            };
            Ok(ConditionExpr::AnyMatch {
                query: Box::new(ObjectQuery::ZoneScan {
                    zone: qz,
                    filter: Box::new(ObjectFilter::any()),
                    slice: QuerySlice::Top(Box::new(ValueExpr::literal(1))),
                }),
                filter: Box::new(obj),
            })
        }
        ConditionAst::HasStatus { subject, status } => {
            let q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let obj = parse_status_or_predicate_filter(status, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::AnyMatch {
                query: Box::new(q),
                filter: Box::new(obj),
            })
        }
        ConditionAst::HasKeyword { subject, keyword } => {
            let q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let kw =
                lower_keyword_ability_name(keyword).ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::AnyMatch {
                query: Box::new(q),
                filter: Box::new(ObjectFilter::any().with_keyword(kw)),
            })
        }
        ConditionAst::ControlsCount {
            player,
            filter,
            op,
            value,
        } => {
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let obj = lower_object_filter(&clean, vocabulary)?;
            let ps = match player {
                PlayerRefAst::You => PlayerSelector::Resolving(ResolvingPlayer::Controller),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    PlayerSelector::Group(AffectedPlayers::EachOpponent)
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                    PlayerSelector::Group(AffectedPlayers::EachPlayer)
                }
                other => PlayerSelector::Resolving(lower_resolving_player_in_context(
                    *other, builder, in_trigger,
                )?),
            };
            Ok(ConditionExpr::Compare {
                left: Box::new(ValueExpr::QueryCount(Box::new(ObjectQuery::zone(
                    QueryZone::BattlefieldControlledBy(Box::new(ps)),
                    obj,
                )))),
                op: lower_comparison(*op),
                right: Box::new(ValueExpr::literal(i64::from(*value))),
            })
        }
        ConditionAst::ZoneObjectCount { filter, op, value } => {
            let q = lower_filter_to_zone_query(filter, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::Compare {
                left: Box::new(ValueExpr::QueryCount(Box::new(q))),
                op: lower_comparison(*op),
                right: Box::new(ValueExpr::literal(i64::from(*value))),
            })
        }
        ConditionAst::LifeComparePlayers {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            other: PlayerRefAst::EachOpponent,
        } => {
            let ctrl = ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
            Ok(ConditionExpr::And(vec![
                ConditionExpr::Compare {
                    left: Box::new(ctrl.clone()),
                    op: Comparison::Equal,
                    right: Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)),
                },
                ConditionExpr::Compare {
                    left: Box::new(ctrl),
                    op: Comparison::Equal,
                    right: Box::new(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)),
                },
            ]))
        }
        ConditionAst::Marker(GameMarker::AllMatchingAre { filter, predicate }) => {
            let base_q = lower_filter_to_zone_query(filter, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            if !predicate.extra_zones.is_empty() {
                return Err("oracle:unsupported-if-condition");
            }
            let mut pred_clean = predicate.as_ref().clone();
            pred_clean.zone = None;
            let pred_obj = lower_object_filter(&pred_clean, vocabulary)?;
            Ok(ConditionExpr::AllMatch {
                query: Box::new(base_q),
                filter: Box::new(pred_obj),
                require_non_empty: false,
            })
        }
        ConditionAst::Marker(GameMarker::SubjectRemainsExiled { subject, .. }) => {
            let q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            Ok(ConditionExpr::NonEmpty(Box::new(ObjectQuery::Intersect(
                Box::new(q),
                Box::new(ObjectQuery::zone(QueryZone::Exile, ObjectFilter::any())),
            ))))
        }
        ConditionAst::Marker(GameMarker::OnlyObjectInZone {
            subject,
            filter,
            zone,
        }) => {
            let subj_q = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-if-condition")?;
            let qz = lower_zone_filter_to_query_zone(*zone, builder, in_trigger)
                .ok_or("oracle:unsupported-if-condition")?;
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            let obj = if is_unrestricted_filter(&clean) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&clean, vocabulary)?
            };
            let zone_q = ObjectQuery::zone(qz, obj);
            Ok(ConditionExpr::And(vec![
                ConditionExpr::NonEmpty(Box::new(ObjectQuery::Intersect(
                    Box::new(subj_q),
                    Box::new(zone_q.clone()),
                ))),
                ConditionExpr::Compare {
                    left: Box::new(ValueExpr::QueryCount(Box::new(zone_q))),
                    op: Comparison::LessOrEqual,
                    right: Box::new(ValueExpr::literal(1)),
                },
            ]))
        }
        ConditionAst::Marker(GameMarker::SameManaValueAsTriggerSpell)
            if in_trigger
                && !is_object_body_target(builder.target, false)
                && resolve_prev_anaphora_ref(&builder.instructions, false, true)
                    == Some(MatchObjectRef::RevealedTop) =>
        {
            Ok(ConditionExpr::And(vec![
                ConditionExpr::AnyMatch {
                    query: Box::new(ObjectQuery::Revealed),
                    filter: Box::new(ObjectFilter::nonland()),
                },
                ConditionExpr::Compare {
                    left: Box::new(ValueExpr::object_stat(
                        ValueObjectRef::RevealedTop,
                        ObjectStatKind::ManaValue,
                    )),
                    op: Comparison::Equal,
                    right: Box::new(ValueExpr::object_stat(
                        ValueObjectRef::TriggeringSpell,
                        ObjectStatKind::ManaValue,
                    )),
                },
            ]))
        }
        ConditionAst::Marker(GameMarker::ControlsNeither)
            if is_object_body_target(builder.target, false) =>
        {
            Ok(ConditionExpr::And(vec![
                ConditionExpr::NonEmpty(Box::new(ObjectQuery::Targets)),
                ConditionExpr::Not(Box::new(ConditionExpr::AnyMatch {
                    query: Box::new(ObjectQuery::Targets),
                    filter: Box::new(ObjectFilter::any().controlled()),
                })),
            ]))
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: raw,
        }
        | ConditionAst::EventOccurredThisTurn { event: raw } => {
            let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
            if low == "it's a spell with lesser mana value"
                && in_trigger
                && !is_object_body_target(builder.target, false)
                && resolve_prev_anaphora_ref(&builder.instructions, false, true)
                    == Some(MatchObjectRef::RevealedTop)
            {
                return Ok(ConditionExpr::And(vec![
                    ConditionExpr::AnyMatch {
                        query: Box::new(ObjectQuery::Revealed),
                        filter: Box::new(ObjectFilter::nonland()),
                    },
                    ConditionExpr::Compare {
                        left: Box::new(ValueExpr::object_stat(
                            ValueObjectRef::RevealedTop,
                            ObjectStatKind::ManaValue,
                        )),
                        op: Comparison::Less,
                        right: Box::new(ValueExpr::object_stat(
                            ValueObjectRef::TriggeringSpell,
                            ObjectStatKind::ManaValue,
                        )),
                    },
                ]));
            }
            if let Some(after_rev) = low
                .strip_prefix("you reveal ")
                .or_else(|| low.strip_prefix("you revealed "))
                && let Some(phrase) = after_rev.strip_suffix(" this way")
                && resolve_prev_anaphora_ref(&builder.instructions, false, in_trigger)
                    == Some(MatchObjectRef::RevealedTop)
            {
                let flt_raw = phrase
                    .trim()
                    .strip_prefix("one or more ")
                    .or_else(|| phrase.trim().strip_prefix("at least one "))
                    .or_else(|| phrase.trim().strip_prefix("an "))
                    .or_else(|| phrase.trim().strip_prefix("a "))
                    .unwrap_or("")
                    .trim();
                if !flt_raw.is_empty()
                    && !flt_raw.contains(" named ")
                    && !flt_raw.contains(" chosen ")
                {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    if let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                        flt_raw,
                        &compiler_vocab,
                    ) && !is_unrestricted_filter(&parsed)
                        && parsed.zone.is_none()
                        && parsed.extra_zones.is_empty()
                        && parsed.controller.is_none()
                        && parsed.owner.is_none()
                        && parsed.named.is_none()
                        && parsed.predicates.is_empty()
                        && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                    {
                        return Ok(ConditionExpr::AnyMatch {
                            query: Box::new(ObjectQuery::Revealed),
                            filter: Box::new(obj),
                        });
                    }
                }
            }
            Err("oracle:unsupported-if-condition")
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}

fn eval_const_value_expr(expr: &ValueExprAst) -> Option<i32> {
    match expr {
        ValueExprAst::Const(n) => Some(*n),
        ValueExprAst::Add(a, b) => eval_const_value_expr(a)?.checked_add(eval_const_value_expr(b)?),
        ValueExprAst::Sub(a, b) => eval_const_value_expr(a)?.checked_sub(eval_const_value_expr(b)?),
        ValueExprAst::Mul(a, b) => eval_const_value_expr(a)?.checked_mul(eval_const_value_expr(b)?),
        _ => None,
    }
}

fn value_expr_max(a: ValueExpr, b: ValueExpr) -> ValueExpr {
    ValueExpr::Conditional {
        left: Box::new(a.clone()),
        comparison: Comparison::GreaterOrEqual,
        right: Box::new(b.clone()),
        then_value: Box::new(a),
        else_value: Box::new(b),
    }
}

fn value_expr_min(a: ValueExpr, b: ValueExpr) -> ValueExpr {
    ValueExpr::Conditional {
        left: Box::new(a.clone()),
        comparison: Comparison::LessOrEqual,
        right: Box::new(b.clone()),
        then_value: Box::new(a),
        else_value: Box::new(b),
    }
}

fn value_expr_abs_diff(a: ValueExpr, b: ValueExpr) -> ValueExpr {
    ValueExpr::Conditional {
        left: Box::new(a.clone()),
        comparison: Comparison::GreaterOrEqual,
        right: Box::new(b.clone()),
        then_value: Box::new(ValueExpr::Sub(Box::new(a.clone()), Box::new(b.clone()))),
        else_value: Box::new(ValueExpr::Sub(Box::new(b), Box::new(a))),
    }
}

fn lower_condition_value_expr(
    expr: &ValueExprAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    if let Ok(ve) = lower_value_expr_in_context(expr, builder, in_trigger, vocabulary) {
        return Ok(ve);
    }
    match expr {
        ValueExprAst::CardTypesAmong(filter) => {
            if filter.any_permanent
                || filter.any_spell
                || filter.is_ability
                || !filter.extra_zones.is_empty()
            {
                return Err("oracle:unsupported-value-expr");
            }
            let mut clean = filter.as_ref().clone();
            let zone = clean.zone.take();
            if !is_unrestricted_filter(&clean) {
                return Err("oracle:unsupported-value-expr");
            }
            match zone {
                Some(ZoneFilter::YourGraveyard) => Ok(ValueExpr::PlayerStat(
                    PlayerStatExpr::OwnGraveyardDistinctCardTypes,
                )),
                Some(ZoneFilter::AnyGraveyard) => Ok(ValueExpr::PlayerStat(
                    PlayerStatExpr::AllGraveyardsDistinctCardTypes,
                )),
                _ => Err("oracle:unsupported-value-expr"),
            }
        }
        ValueExprAst::TotalCardsInAllHands => Ok(ValueExpr::Count {
            zone: ValueZone::AllHands,
            filter: Box::new(ObjectFilter::any()),
            controller_scope: None,
        }),
        ValueExprAst::PlayerStat { player, stat } => {
            if matches!(
                player,
                PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherOpponent
            ) {
                return Err("oracle:unsupported-value-expr");
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)
                .map_err(|_| "oracle:unsupported-value-expr")?;
            match stat {
                PlayerStatKind::LifeTotal => Ok(if rp == ResolvingPlayer::Controller {
                    ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp))
                }),
                PlayerStatKind::CardsInHand => {
                    Ok(ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)))
                }
                PlayerStatKind::CardsInGraveyard => Ok(ValueExpr::PlayerStat(
                    PlayerStatExpr::PlayerGraveyardSize(rp),
                )),
                PlayerStatKind::CardsInLibrary => {
                    Ok(ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(rp)))
                }
            }
        }
        ValueExprAst::AggregateOverPlayers { op, metric } => match (op, metric.as_ref()) {
            (
                AggregateOp::Max,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
                    stat: PlayerStatKind::LifeTotal,
                },
            ) => Ok(value_expr_max(
                ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
            )),
            (
                AggregateOp::Min,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
                    stat: PlayerStatKind::LifeTotal,
                },
            ) => Ok(value_expr_min(
                ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
            )),
            (
                AggregateOp::Max,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    stat: PlayerStatKind::LifeTotal,
                },
            ) => Ok(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)),
            (
                AggregateOp::Min,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    stat: PlayerStatKind::LifeTotal,
                },
            ) => Ok(ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)),
            (
                AggregateOp::Max,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
                    stat: PlayerStatKind::CardsInHand,
                },
            ) => Ok(ValueExpr::PlayerStat(PlayerStatExpr::LargestHandSize)),
            (
                AggregateOp::Min,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
                    stat: PlayerStatKind::CardsInHand,
                },
            ) => Ok(value_expr_min(
                ValueExpr::count_hand(ResolvingPlayer::Controller),
                ValueExpr::OpponentMinCount {
                    zone: ValueZone::Hand(ResolvingPlayer::Controller),
                    filter: Box::new(ObjectFilter::any()),
                },
            )),
            (
                AggregateOp::Max,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    stat: PlayerStatKind::CardsInHand,
                },
            ) => Ok(ValueExpr::OpponentMaxCount {
                zone: ValueZone::Hand(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::any()),
            }),
            (
                AggregateOp::Min,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    stat: PlayerStatKind::CardsInHand,
                },
            ) => Ok(ValueExpr::OpponentMinCount {
                zone: ValueZone::Hand(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::any()),
            }),
            (
                AggregateOp::Min,
                ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
                    stat: PlayerStatKind::CardsInLibrary,
                },
            ) => Ok(ValueExpr::PlayerStat(PlayerStatExpr::MinLibrarySize)),
            (AggregateOp::Max | AggregateOp::Min, ValueExprAst::Count(filter))
                if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                    && filter.extra_zones.is_empty()
                    && filter.owner.is_none()
                    && filter.named.is_none() =>
            {
                let mut clean = filter.as_ref().clone();
                clean.zone = None;
                let ctrl = clean.controller.take();
                let obj = if is_unrestricted_filter(&clean) {
                    ObjectFilter::permanent()
                } else {
                    lower_object_filter(&clean, vocabulary)
                        .map_err(|_| "oracle:unsupported-value-expr")?
                };
                match (op, ctrl) {
                    (AggregateOp::Max, Some(ControllerRelation::Opponent)) => {
                        Ok(ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Battlefield,
                            filter: Box::new(obj),
                        })
                    }
                    (AggregateOp::Min, Some(ControllerRelation::Opponent)) => {
                        Ok(ValueExpr::OpponentMinCount {
                            zone: ValueZone::Battlefield,
                            filter: Box::new(obj),
                        })
                    }
                    (AggregateOp::Max, None) => Ok(value_expr_max(
                        ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                        ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Battlefield,
                            filter: Box::new(obj),
                        },
                    )),
                    (AggregateOp::Min, None) => Ok(value_expr_min(
                        ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                        ValueExpr::OpponentMinCount {
                            zone: ValueZone::Battlefield,
                            filter: Box::new(obj),
                        },
                    )),
                    _ => Err("oracle:unsupported-value-expr"),
                }
            }
            _ => Err("oracle:unsupported-value-expr"),
        },
        ValueExprAst::ObjectStat { subject, stat } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let req_creature = matches!(stat, StatKind::Power | StatKind::Toughness);
            let subj =
                resolve_value_object_ref_for_stat(subject, builder, in_trigger, req_creature)
                    .ok_or("oracle:unsupported-value-expr")?;
            Ok(ValueExpr::object_stat(subj, st))
        }
        ValueExprAst::Add(a, b) => Ok(ValueExpr::Add(
            Box::new(lower_condition_value_expr(
                a, builder, in_trigger, vocabulary,
            )?),
            Box::new(lower_condition_value_expr(
                b, builder, in_trigger, vocabulary,
            )?),
        )),
        ValueExprAst::Sub(a, b) => Ok(ValueExpr::Sub(
            Box::new(lower_condition_value_expr(
                a, builder, in_trigger, vocabulary,
            )?),
            Box::new(lower_condition_value_expr(
                b, builder, in_trigger, vocabulary,
            )?),
        )),
        ValueExprAst::Mul(a, b) => {
            if let Some(k) = eval_const_value_expr(a) {
                let inner = lower_condition_value_expr(b, builder, in_trigger, vocabulary)?;
                Ok(ValueExpr::Scale {
                    expr: Box::new(inner),
                    factor: i64::from(k).into(),
                })
            } else if let Some(k) = eval_const_value_expr(b) {
                let inner = lower_condition_value_expr(a, builder, in_trigger, vocabulary)?;
                Ok(ValueExpr::Scale {
                    expr: Box::new(inner),
                    factor: i64::from(k).into(),
                })
            } else {
                Err("oracle:unsupported-value-expr")
            }
        }
        ValueExprAst::DivConst {
            expr: inner,
            denom,
            rounding,
        } if *denom > 0 => {
            let d = std::num::NonZeroU16::new(
                u16::try_from(*denom).map_err(|_| "oracle:unsupported-value-expr")?,
            )
            .ok_or("oracle:unsupported-value-expr")?;
            Ok(ValueExpr::DivConst {
                expr: Box::new(lower_condition_value_expr(
                    inner, builder, in_trigger, vocabulary,
                )?),
                divisor: d,
                round_up: *rounding == spellwake_oracle_compiler::ast::Rounding::Up,
            })
        }
        ValueExprAst::Max(a, b) => Ok(value_expr_max(
            lower_condition_value_expr(a, builder, in_trigger, vocabulary)?,
            lower_condition_value_expr(b, builder, in_trigger, vocabulary)?,
        )),
        ValueExprAst::Min(a, b) => Ok(value_expr_min(
            lower_condition_value_expr(a, builder, in_trigger, vocabulary)?,
            lower_condition_value_expr(b, builder, in_trigger, vocabulary)?,
        )),
        ValueExprAst::AbsDiff(a, b) => Ok(value_expr_abs_diff(
            lower_condition_value_expr(a, builder, in_trigger, vocabulary)?,
            lower_condition_value_expr(b, builder, in_trigger, vocabulary)?,
        )),
        _ => Err("oracle:unsupported-value-expr"),
    }
}

fn parse_this_way_count_and_filter(
    phrase: &str,
    vocabulary: &CreatureTypeVocabulary,
    allow_controller: bool,
) -> Option<(i64, ObjectFilter)> {
    let trimmed = phrase.trim().trim_end_matches('.');
    let (min_n, flt_raw) = if let Some((cnt_raw, rest)) = trimmed.split_once(" or more ") {
        let n = parse_small_positive_count(cnt_raw)?;
        (n, rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix("at least ")
        && let Some((cnt_raw, tail)) = rest.split_once(' ')
        && let Some(n) = parse_small_positive_count(cnt_raw)
    {
        (n, tail.trim())
    } else {
        let rest = trimmed
            .strip_prefix("one or more ")
            .or_else(|| trimmed.strip_prefix("an "))
            .or_else(|| trimmed.strip_prefix("a "))?;
        (1, rest.trim())
    };
    if flt_raw.is_empty() || flt_raw.contains(" named ") || flt_raw.contains(" chosen ") {
        return None;
    }
    let obj = if matches!(flt_raw, "permanent" | "permanents" | "card" | "cards") {
        ObjectFilter::any()
    } else {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let parsed =
            spellwake_oracle_compiler::filter::parse_object_filter(flt_raw, &compiler_vocab)
                .ok()?;
        if parsed.zone.is_some()
            || !parsed.extra_zones.is_empty()
            || parsed.owner.is_some()
            || parsed.named.is_some()
            || !parsed.predicates.is_empty()
            || (!allow_controller && parsed.controller.is_some())
            || is_unrestricted_filter(&parsed)
        {
            return None;
        }
        lower_object_filter(&parsed, vocabulary).ok()?
    };
    Some((min_n, obj))
}

fn lower_marker_or_action_fallback(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    _bound_x: Option<&ValueExprAst>,
) -> Result<ResolutionCondition, &'static str> {
    match cond {
        ConditionAst::Marker(GameMarker::HasNotPlayedOrCast)
            if !in_trigger
                && builder.instructions.is_empty()
                && builder.target == BodyTarget::None =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::WasCast {
                    by_controller: false,
                    from_hand: false,
                }),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        ConditionAst::Marker(GameMarker::Monarch(
            PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
        )) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan { player, than }) => {
            match (player, than) {
                (PlayerRefAst::You, PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer) => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                        comparison: Comparison::Greater,
                        right: ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        },
                    })
                }
                (PlayerRefAst::You, PlayerRefAst::AnyOpponent) => Ok(ResolutionCondition {
                    left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                    comparison: Comparison::Greater,
                    right: ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    },
                }),
                (PlayerRefAst::AnyOpponent, PlayerRefAst::You) => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_hand(ResolvingPlayer::Controller),
                }),
                (PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer, PlayerRefAst::You) => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::OpponentMinCount {
                            zone: ValueZone::Hand(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        },
                        comparison: Comparison::Greater,
                        right: ValueExpr::count_hand(ResolvingPlayer::Controller),
                    })
                }
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        }
        | ConditionAst::EventOccurredThisTurn { event: action } => {
            lower_event_or_action_text_fallback(action, builder, vocabulary, in_trigger)
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}

fn lower_event_or_action_text_fallback(
    raw: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Result<ResolutionCondition, &'static str> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    match low.as_str() {
        "the player is your opponent" if in_trigger && builder.target == BodyTarget::None => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        "you've rolled three or more dice this turn" => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DiceRolledThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        }),
        "a player other than you lost life this turn" => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        "~ is enchanted, equipped, and has a counter on it" => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                MatchObjectRef::AbilitySource,
                ObjectFilter::any().with_flags(
                    ObjectFilterFlags::ENCHANTED
                        .union(ObjectFilterFlags::EQUIPPED)
                        .union(ObjectFilterFlags::WITH_ANY_COUNTER),
                ),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        "three or more creatures attacked this turn" => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                players: AffectedPlayers::EachPlayer,
                filter: Box::new(ObjectFilter::creature()),
            }),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        }),
        "~ dealt damage to it this turn" => {
            let mref = resolve_match_object_ref(
                &SubjectRefAst::Anaphora("it".to_string()),
                builder,
                in_trigger,
                false,
            )
            .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any()
                        .with_flags(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        "that card has the chosen name" => {
            let mref = resolve_match_object_ref(
                &SubjectRefAst::Anaphora("it".to_string()),
                builder,
                in_trigger,
                false,
            )
            .ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    mref,
                    ObjectFilter::any().with_flags(ObjectFilterFlags::MATCHES_CHOSEN_NAME),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        "they control fewer lands than you" | "that player controls fewer lands than you" => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_player_battlefield(rp, ObjectFilter::land()),
                comparison: Comparison::Less,
                right: ValueExpr::count_player_battlefield(
                    ResolvingPlayer::Controller,
                    ObjectFilter::land(),
                ),
            })
        }
        "you control more creatures than attacking player" => Ok(ResolutionCondition {
            left: ValueExpr::count_player_battlefield(
                ResolvingPlayer::Controller,
                ObjectFilter::creature(),
            ),
            comparison: Comparison::Greater,
            right: ValueExpr::count_player_battlefield(
                ResolvingPlayer::ActivePlayer,
                ObjectFilter::creature(),
            ),
        }),
        "you control more lands than attacking player" => Ok(ResolutionCondition {
            left: ValueExpr::count_player_battlefield(
                ResolvingPlayer::Controller,
                ObjectFilter::land(),
            ),
            comparison: Comparison::Greater,
            right: ValueExpr::count_player_battlefield(
                ResolvingPlayer::ActivePlayer,
                ObjectFilter::land(),
            ),
        }),
        "that player controls more lands than each other player" => {
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerControlsMost {
                    player: rp,
                    zone: ValueZone::Battlefield,
                    filter: Box::new(ObjectFilter::land()),
                    allow_tie: false,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        "your library has no cards in it" => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                ResolvingPlayer::Controller,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        "exactly one creature is attacking" => Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature().attacking()),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        "you've cast both a creature spell and a noncreature spell this turn" => {
            combine_resolution_conditions_and(vec![
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellTypeThisTurn(
                        CardType::Creature,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastNoncreatureSpellsThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        "it's the second creature spell you cast this turn" => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::creature()),
                from_zone: None,
            }),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(2),
        }),
        "a card is exiled with ~" | "a card is exiled with it"
            if builder.has_source || in_trigger =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::QueryCount(Box::new(
                    spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None },
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        "you don't cast a spell this way"
        | "you don't copy a spell this way"
        | "you don't draw a card this way"
            if !builder.instructions.is_empty() =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            })
        }
        "there are twenty or more counters on it or you have twenty or more cards in hand"
            if builder.has_source && !is_object_body_target(builder.target, false) =>
        {
            combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::CounterCount {
                        subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        kind: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(20),
                },
                ResolutionCondition {
                    left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(20),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        "three or more cards left your graveyard this turn or if you've sacrificed a food this turn" =>
        {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let food_ast =
                spellwake_oracle_compiler::filter::parse_object_filter("food", &compiler_vocab)
                    .map_err(|_| "oracle:unsupported-if-condition")?;
            let food_filter = lower_object_filter(&food_ast, vocabulary)?;
            if !body_turn_record_filter_supported(food_filter, true) {
                return Err("oracle:unsupported-if-condition");
            }
            combine_resolution_conditions_or(vec![
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(3),
                },
                ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::SacrificedMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(food_filter),
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        "you own a card named ~ in exile, in your hand, in your graveyard, and on the battlefield" =>
        {
            let same_name = ObjectFilter::any().sharing_name_with_source();
            let owned_bf_same_name = ObjectFilter::Clause(ObjectClause {
                owner: ControllerFilter::You,
                source_relation: SourceRelation::SharingNameWithAbilitySource,
                ..ObjectClause::any()
            });
            combine_resolution_conditions_and(vec![
                ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Exile(Some(ResolvingPlayer::Controller)),
                        filter: Box::new(same_name),
                        controller_scope: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(same_name),
                        controller_scope: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::Count {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(same_name),
                        controller_scope: None,
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
                ResolutionCondition {
                    left: ValueExpr::count_battlefield(owned_bf_same_name),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                },
            ])
            .ok_or("oracle:unsupported-if-condition")
        }
        "a permanent you controlled or a token was destroyed this way" => {
            let preceded_by_destroy = builder
                .instructions
                .last()
                .and_then(super::super::toolbox_emit::unwrap_zone_op)
                .is_some_and(|spec| {
                    matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                        }
                    )
                });
            if !preceded_by_destroy {
                return Err("oracle:unsupported-if-condition");
            }
            let filter = ObjectFilter::Either(
                ObjectClause {
                    controller: ControllerFilter::You,
                    ..ObjectClause::any()
                },
                ObjectClause {
                    token: Some(true),
                    ..ObjectClause::any()
                },
            );
            Ok(ResolutionCondition {
                left: ValueExpr::LastActionMatchingCount(Box::new(filter)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        "excess damage was dealt to a permanent this way"
        | "excess damage was dealt to a creature this way" => {
            let preceded_by_target_damage = matches!(
                builder.instructions.last(),
                Some(Instruction::DealDamage(spec))
                    if matches!(
                        &spec.recipients,
                        spellwake_engine::toolbox::DamageTargetQuery::Targets
                            | spellwake_engine::toolbox::DamageTargetQuery::CreaturePowerToOther { .. }
                            | spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::Targets
                            )
                    ) || matches!(
                        &spec.recipients,
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(r)
                            if matches!(r.as_ref(), DamageRecipientsRef::Target)
                    )
            );
            if !preceded_by_target_damage || !is_object_body_target(builder.target, true) {
                return Err("oracle:unsupported-if-condition");
            }
            let excess_cond = ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                    subject: ValueObjectRef::TargetObject,
                    excess_only: true,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            };
            if low == "excess damage was dealt to a creature this way" {
                combine_resolution_conditions_and(vec![
                    excess_cond,
                    ResolutionCondition {
                        left: ValueExpr::object_matches(
                            MatchObjectRef::TargetObject,
                            ObjectFilter::creature(),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    },
                ])
                .ok_or("oracle:unsupported-if-condition")
            } else {
                Ok(excess_cond)
            }
        }
        other => {
            if let Some(flt_raw) = other.strip_prefix("that target is ")
                && is_object_body_target(builder.target, true)
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                    flt_raw.trim(),
                    &compiler_vocab,
                ) && !is_unrestricted_filter(&parsed)
                    && parsed.zone.is_none()
                    && parsed.extra_zones.is_empty()
                    && parsed.named.is_none()
                    && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::object_matches(MatchObjectRef::TargetObject, obj),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            if let Some(flt_raw) = other.strip_prefix("it shares a color with ")
                && matches!(
                    builder.target,
                    BodyTarget::SingleCreature
                        | BodyTarget::SinglePermanent
                        | BodyTarget::SinglePlaneswalker
                        | BodyTarget::FilteredPermanent(_)
                )
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                    flt_raw.trim(),
                    &compiler_vocab,
                ) && !is_unrestricted_filter(&parsed)
                    && matches!(parsed.zone, None | Some(ZoneFilter::Battlefield))
                    && parsed.extra_zones.is_empty()
                    && parsed.named.is_none()
                    && parsed.predicates.is_empty()
                    && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_battlefield(
                            obj.with_flags(ObjectFilterFlags::SHARES_COLOR_WITH_TARGET),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            if let Some(core) = other.strip_suffix(" this way") {
                let last_zone_op = builder
                    .instructions
                    .last()
                    .and_then(super::super::toolbox_emit::unwrap_zone_op);
                if let Some(spec) = last_zone_op {
                    if matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Hand) {
                        let phrase_opt = core
                            .strip_prefix("you return ")
                            .or_else(|| core.strip_prefix("you returned "))
                            .or_else(|| core.strip_suffix(" was returned"))
                            .or_else(|| core.strip_suffix(" is returned"))
                            .or_else(|| core.strip_suffix(" were returned"))
                            .or_else(|| core.strip_suffix(" are returned"));
                        if let Some(phrase) = phrase_opt
                            && let Some((min_n, obj)) =
                                parse_this_way_count_and_filter(phrase, vocabulary, true)
                        {
                            return Ok(ResolutionCondition {
                                left: ValueExpr::LastActionMatchingCount(Box::new(obj)),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(min_n),
                            });
                        }
                    }
                    if matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { .. }
                        }
                    ) {
                        let phrase_opt = core
                            .strip_suffix(" was destroyed")
                            .or_else(|| core.strip_suffix(" is destroyed"))
                            .or_else(|| core.strip_suffix(" were destroyed"))
                            .or_else(|| core.strip_suffix(" are destroyed"));
                        if let Some(phrase) = phrase_opt
                            && let Some((min_n, obj)) =
                                parse_this_way_count_and_filter(phrase, vocabulary, true)
                        {
                            return Ok(ResolutionCondition {
                                left: ValueExpr::LastActionMatchingCount(Box::new(obj)),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(min_n),
                            });
                        }
                    }
                }
            }
            Err("oracle:unsupported-if-condition")
        }
    }
}

#[cfg(test)]
mod wave11_distinct_power_tests {
    use super::*;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test-vocab".into(),
            [0; 32],
            vec![
                "Human".into(),
                "Knight".into(),
                "Soldier".into(),
                "Warlock".into(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn coven_uses_distinct_powers_and_preserves_other_constraints() {
        let vocab = vocabulary();
        let condition = spellwake_oracle_compiler::filter::parse_condition(
            "you control three or more creatures with different powers",
            &cached_compiler_vocab(&vocab),
        )
        .unwrap();
        let expected = ResolutionCondition {
            left: ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::CountDistinct,
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        };
        assert_eq!(
            lower_resolution_condition(&condition, &mut BodyBuilder::new(), &vocab, false, None),
            Ok(expected.clone())
        );
        assert_eq!(
            super::super::super::abilities::lower_trigger_condition(&condition, &vocab),
            Ok(TriggerCondition::Eval(Box::new(expected)))
        );
        let ConditionAst::ControlsCount { filter, .. } = &condition else {
            panic!("controls-count AST")
        };
        for bad_filter in [
            {
                let mut f = filter.clone();
                f.zone = Some(ZoneFilter::YourGraveyard);
                f
            },
            {
                let mut f = filter.clone();
                f.controller = Some(ControllerRelation::Opponent);
                f
            },
            {
                let mut f = filter.clone();
                f.predicates.push(
                    spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(
                        "unimplemented_condition".into(),
                    ),
                );
                f
            },
        ] {
            let bad = ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: bad_filter,
                op: CompareOp::Ge,
                value: 3,
            };
            assert!(
                lower_resolution_condition(&bad, &mut BodyBuilder::new(), &vocab, false, None)
                    .is_err()
            );
        }
        let mut narrowed = filter.clone();
        narrowed.tapped = Some(true);
        let condition = ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: narrowed,
            op: CompareOp::Ge,
            value: 3,
        };
        let lowered =
            lower_resolution_condition(&condition, &mut BodyBuilder::new(), &vocab, false, None)
                .unwrap();
        assert_ne!(
            lowered.left,
            ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::CountDistinct
            )
        );
    }

    #[test]
    fn distinct_power_value_rejects_graveyard_counts() {
        let vocab = vocabulary();
        let filter = spellwake_oracle_compiler::filter::parse_object_filter(
            "creatures you control",
            &cached_compiler_vocab(&vocab),
        )
        .unwrap();
        let expr = ValueExprAst::DistinctCountAmong {
            category: "different_powers".into(),
            filter: Box::new(filter.clone()),
        };
        assert_eq!(
            lower_value_expr(&expr, &vocab),
            Ok(ValueExpr::aggregate(
                ObjectFilter::creature().controlled(),
                ObjectStatKind::Power,
                StatAggregate::CountDistinct
            ))
        );
        let mut graveyard = filter;
        graveyard.zone = Some(ZoneFilter::YourGraveyard);
        assert!(
            lower_value_expr(
                &ValueExprAst::DistinctCountAmong {
                    category: "different_powers".into(),
                    filter: Box::new(graveyard)
                },
                &vocab
            )
            .is_err()
        );
    }

    #[test]
    fn bound_search_bridge_rejects_unrelated_continuations_and_rolls_back() {
        let vocab = vocabulary();
        let ast = compile_card(
            &CardInput {
                oracle_id: "test",
                name: "Celebrate the Harvest",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Celebrate the Harvest",
                    type_line: "Sorcery",
                    oracle_text: "Search your library for up to X basic land cards, where X is the number of different powers among creatures you control. Put those cards onto the battlefield tapped, then shuffle.",
                }],
            },
            &cached_compiler_vocab(&vocab),
        );
        let original = ast.faces[0].spell_effects.clone();
        assert_eq!(original.len(), 3);
        for variant in 0..5 {
            let mut instructions = original.clone();
            match variant {
                0 => {
                    instructions[2] = InstructionAst::ShuffleLibrary {
                        player: PlayerRefAst::EachOpponent,
                    }
                }
                1 => {
                    let InstructionAst::MoveZone { subject, .. } = &mut instructions[1] else {
                        panic!("move")
                    };
                    **subject = SubjectRefAst::Anaphora("unrelated cards".into());
                }
                2 => {
                    let InstructionAst::BindX { body, .. } = &mut instructions[0] else {
                        panic!("bind")
                    };
                    body.push(InstructionAst::ShuffleLibrary {
                        player: PlayerRefAst::You,
                    });
                }
                3 => {
                    let InstructionAst::BindX { x_expr, .. } = &mut instructions[0] else {
                        panic!("bind")
                    };
                    let ValueExprAst::DistinctCountAmong { category, .. } = x_expr.as_mut() else {
                        panic!("distinct")
                    };
                    *category = "unsupported-category".into();
                }
                _ => instructions.push(InstructionAst::Draw {
                    player: PlayerRefAst::You,
                    count: Box::new(ValueExprAst::ChosenX),
                    up_to: false,
                }),
            }
            let mut builder = BodyBuilder::new();
            let mut ids = LoweringIds::default();
            let res = super::lower_instruction_seq(
                &instructions,
                &mut builder,
                &vocab,
                &mut ids,
                false,
                false,
                None,
            );
            if variant == 0 {
                // With general found_set ZoneOp lowering, SearchLibrary + MoveZone +
                // ShuffleLibrary(EachOpponent) lowers faithfully without invoking the
                // controller-only BindX bridge; verify EachOpponent shuffle is preserved.
                assert!(
                    res.is_ok(),
                    "variant 0 should lower via general found_set pipeline"
                );
                assert!(
                    builder.instructions.iter().any(|ins| matches!(
                        ins,
                        Instruction::ForEachPlayerQuery { selector, .. }
                            if matches!(
                                selector.as_ref(),
                                spellwake_engine::toolbox::PlayerSelector::Group(
                                    spellwake_engine::program::AffectedPlayers::EachOpponent
                                )
                            )
                    )),
                    "variant 0 must preserve EachOpponent shuffle instead of collapsing to controller"
                );
            } else if variant == 2 {
                // With general found_set ZoneOp lowering, an extra inner ShuffleLibrary lowers
                // faithfully as two distinct shuffles rather than collapsing into the 1-shuffle bridge.
                assert!(
                    res.is_ok(),
                    "variant 2 should lower via general found_set pipeline"
                );
                let expected_shuffle =
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library();
                let shuffle_count = builder
                    .instructions
                    .iter()
                    .filter(|ins| **ins == expected_shuffle)
                    .count();
                assert_eq!(
                    shuffle_count, 2,
                    "variant 2 must preserve both shuffles instead of collapsing to the 1-shuffle bridge"
                );
            } else if variant == 4 {
                // With general found_set ZoneOp + sentence-scoped BindResolutionX, a trailing
                // Draw(ChosenX) composes faithfully via BindResolutionX + DrawCards.
                assert!(
                    res.is_ok(),
                    "variant 4 should compose cleanly via BindResolutionX + found_set"
                );
                assert!(
                    builder
                        .instructions
                        .iter()
                        .any(|ins| matches!(ins, Instruction::DrawCards(_))),
                    "variant 4 must preserve trailing DrawCards(ChosenX)"
                );
            } else {
                assert!(res.is_err(), "variant {variant}");
            }
            if variant == 3 {
                assert!(builder.instructions.is_empty());
                assert_eq!(
                    (
                        ids.next_activated,
                        ids.next_triggered,
                        ids.next_mana,
                        ids.next_static,
                        ids.next_ct_slot
                    ),
                    (0, 0, 0, 0, 0)
                );
            }
        }
    }

    #[test]
    fn pinned_coven_cards_and_celebrate_the_harvest_lower_completely() {
        let vocab = vocabulary();
        for (name, type_line, cost, stats, text) in [
            (
                "Dawnhart Wardens",
                "Creature \u{2014} Human Warlock",
                "{1}{G}{W}",
                Some(("3", "3")),
                "Vigilance\nCoven \u{2014} At the beginning of combat on your turn, if you control three or more creatures with different powers, creatures you control get +1/+0 until end of turn.",
            ),
            (
                "Candlegrove Witch",
                "Creature \u{2014} Human Warlock",
                "{1}{W}",
                Some(("2", "2")),
                "Coven \u{2014} At the beginning of combat on your turn, if you control three or more creatures with different powers, this creature gains flying until end of turn.",
            ),
            (
                "Ritual Guardian",
                "Creature \u{2014} Human Soldier",
                "{2}{W}",
                Some(("3", "2")),
                "Coven \u{2014} At the beginning of combat on your turn, if you control three or more creatures with different powers, this creature gains lifelink until end of turn.",
            ),
            (
                "Candlelit Cavalry",
                "Creature \u{2014} Human Knight",
                "{4}{G}",
                Some(("5", "5")),
                "Coven \u{2014} At the beginning of combat on your turn, if you control three or more creatures with different powers, this creature gains trample until end of turn.",
            ),
            (
                "Celebrate the Harvest",
                "Sorcery",
                "{3}{G}",
                None,
                "Search your library for up to X basic land cards, where X is the number of different powers among creatures you control. Put those cards onto the battlefield tapped, then shuffle.",
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(type_line.into()),
                    oracle_text: Some(text.into()),
                    mana_cost: Some(cost.into()),
                    power: stats.map(|(power, _)| power.into()),
                    toughness: stats.map(|(_, toughness)| toughness.into()),
                }],
                legalities: BTreeMap::new(),
                eligible_formats: vec![],
                required_by_base_formats: true,
                program_evidence: "test",
            };
            compile_with_reason(&card, &vocab).unwrap_or_else(|reason| {
                let input = CardInput {
                    oracle_id: "test",
                    name,
                    layout: "normal",
                    faces: vec![FaceInput {
                        name,
                        type_line,
                        oracle_text: text,
                    }],
                };
                panic!(
                    "{name}: {reason}; AST: {:#?}",
                    compile_card(&input, &cached_compiler_vocab(&vocab))
                );
            });
        }
    }

    #[test]
    fn phase12_wave2_condition_and_value_expr_unlocks() {
        let vocab = vocabulary();

        // PlayerActionThisTurn(You) -> EventOccurredThisTurn bridge (Winnow, Crimson Honor Guard, Furnace Punisher)
        let mut b_winnow = BodyBuilder::new();
        b_winnow.target = BodyTarget::SinglePermanent;
        let rc_winnow = lower_resolution_condition(
            &ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: "another permanent with the same name is on the battlefield".into(),
            },
            &mut b_winnow,
            &vocab,
            false,
            None,
        )
        .expect("Winnow condition should lower");
        assert_eq!(rc_winnow.comparison, Comparison::GreaterOrEqual);
        assert_eq!(rc_winnow.right, ValueExpr::literal(2));

        // Fevered Visions: "the player is your opponent" in trigger
        let mut b_fv = BodyBuilder::new();
        let rc_fv = lower_resolution_condition(
            &ConditionAst::EventOccurredThisTurn {
                event: "the player is your opponent".into(),
            },
            &mut b_fv,
            &vocab,
            true,
            None,
        )
        .expect("Fevered Visions opponent check should lower");
        assert_eq!(
            rc_fv.left,
            ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer)
        );
        assert_eq!(rc_fv.comparison, Comparison::Equal);
        assert_eq!(rc_fv.right, ValueExpr::literal(0));

        // Everflame Eidolon: self "it" on source-bound empty activated ability
        let mut b_everflame = BodyBuilder::new();
        b_everflame.has_source = true;
        let aura_flt = ObjectFilterAst {
            subtypes: vec!["Aura".into()],
            ..ObjectFilterAst::default()
        };
        let rc_aura = lower_resolution_condition(
            &ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".into())),
                filter: Box::new(aura_flt),
            },
            &mut b_everflame,
            &vocab,
            false,
            None,
        )
        .expect("Everflame Eidolon Aura check should lower");
        assert!(matches!(
            rc_aura.left,
            ValueExpr::ObjectMatches {
                subject: MatchObjectRef::AbilitySource,
                ..
            }
        ));

        // Freestrider Commando: HasNotPlayedOrCast on replacement entry context
        let mut b_entry = BodyBuilder::new();
        let rc_not_cast = lower_resolution_condition(
            &ConditionAst::Marker(GameMarker::HasNotPlayedOrCast),
            &mut b_entry,
            &vocab,
            false,
            None,
        )
        .expect("HasNotPlayedOrCast entry check should lower");
        assert_eq!(
            rc_not_cast.left,
            ValueExpr::SourceState(SourceStateExpr::WasCast {
                by_controller: false,
                from_hand: false,
            })
        );
        assert_eq!(rc_not_cast.comparison, Comparison::Equal);
        assert_eq!(rc_not_cast.right, ValueExpr::literal(0));

        // Delayed trigger HasNotPlayedOrCast must remain fail-closed
        let mut b_delayed = BodyBuilder::new();
        assert!(
            lower_resolution_condition(
                &ConditionAst::Marker(GameMarker::HasNotPlayedOrCast),
                &mut b_delayed,
                &vocab,
                true,
                None,
            )
            .is_err()
        );

        // Omnivorous Flytrap: CardTypesAmong(YourGraveyard) in Compare
        let gy_flt = ObjectFilterAst {
            zone: Some(ZoneFilter::YourGraveyard),
            ..ObjectFilterAst::default()
        };
        let mut b_flytrap = BodyBuilder::new();
        let rc_gy_types = lower_resolution_condition(
            &ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CardTypesAmong(Box::new(gy_flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(6)),
            },
            &mut b_flytrap,
            &vocab,
            true,
            None,
        )
        .expect("CardTypesAmong(YourGraveyard) should lower");
        assert_eq!(
            rc_gy_types.left,
            ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes)
        );
        assert_eq!(rc_gy_types.right, ValueExpr::literal(6));

        // Hedron Alignment: all four owned zones
        let mut b_hedron = BodyBuilder::new();
        b_hedron.has_source = true;
        assert!(
            lower_resolution_condition(
                &ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: "you own a card named ~ in exile, in your hand, in your graveyard, and on the battlefield".into(),
                },
                &mut b_hedron,
                &vocab,
                true,
                None,
            )
            .is_ok()
        );

        // Strict fail-closed boundaries: Vizier of Deferment (non-self attacked or blocked),
        // Green Slime (permanent_only countered ability)
        let mut b_vizier = BodyBuilder::new();
        b_vizier.target = BodyTarget::SingleCreature;
        assert!(
            lower_resolution_condition(
                &ConditionAst::Marker(GameMarker::AttackedOrBlockedThisTurn {
                    subject: Box::new(SubjectRefAst::Anaphora("it".into())),
                }),
                &mut b_vizier,
                &vocab,
                true,
                None,
            )
            .is_err()
        );

        let mut b_slime = BodyBuilder::new();
        assert!(
            lower_resolution_condition(
                &ConditionAst::Marker(GameMarker::AbilityCounteredThisWay {
                    permanent_only: true,
                }),
                &mut b_slime,
                &vocab,
                true,
                None,
            )
            .is_err()
        );
    }
}
