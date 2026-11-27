use super::*;

fn damage_source_ref_to_query(
    src: DamageSourceRef,
    recipients: &DamageRecipientsRef,
) -> spellwake_engine::toolbox::DamageSourceQuery {
    use spellwake_engine::toolbox::{DamageSourceQuery, ObjectQuery};
    let needs_per_source_object = matches!(
        recipients,
        DamageRecipientsRef::OtherCreaturesAndEachPlayer
            | DamageRecipientsRef::EachOtherPlayer
            | DamageRecipientsRef::EachOtherOpponent
            | DamageRecipientsRef::EachSourceToItself
            | DamageRecipientsRef::EachSourceController
    );
    match src {
        DamageSourceRef::AbilitySource => {
            if needs_per_source_object {
                DamageSourceQuery::Objects(ObjectQuery::Source)
            } else {
                DamageSourceQuery::EntrySource
            }
        }
        DamageSourceRef::TargetObject | DamageSourceRef::AllPermanentTargets => {
            DamageSourceQuery::Objects(ObjectQuery::Targets)
        }
        DamageSourceRef::TriggeringObject => {
            DamageSourceQuery::Objects(ObjectQuery::TriggerSubject)
        }
        DamageSourceRef::AttachedHost => DamageSourceQuery::Objects(ObjectQuery::AttachedHost),
        DamageSourceRef::Matching(filter) => DamageSourceQuery::Objects(
            ObjectQuery::EffectSubject(Box::new(EffectSubject::Matching(filter))),
        ),
        DamageSourceRef::LastMoved => DamageSourceQuery::Objects(ObjectQuery::LastMoved),
        DamageSourceRef::CreatedTokens => DamageSourceQuery::Objects(ObjectQuery::CreatedTokens),
    }
}

fn damage_group_to_target_query(
    group: DamageGroup,
) -> spellwake_engine::toolbox::DamageTargetQuery {
    use spellwake_engine::toolbox::{DamageTargetQuery, ObjectQuery, PlayerSelector};
    match group {
        DamageGroup::Opponents => {
            DamageTargetQuery::Players(PlayerSelector::Group(AffectedPlayers::EachOpponent))
        }
        DamageGroup::Players => {
            DamageTargetQuery::Players(PlayerSelector::Group(AffectedPlayers::EachPlayer))
        }
        DamageGroup::Controller => {
            DamageTargetQuery::Players(PlayerSelector::Resolving(ResolvingPlayer::Controller))
        }
        DamageGroup::RelatedPlayer => DamageTargetQuery::Players(PlayerSelector::Resolving(
            ResolvingPlayer::UniqueRelatedPlayer,
        )),
        DamageGroup::AffectedPlayers(players) => {
            DamageTargetQuery::Players(PlayerSelector::Group(players))
        }
        DamageGroup::AbilitySource => DamageTargetQuery::Objects(ObjectQuery::Source),
        other => DamageTargetQuery::Relative(Box::new(DamageRecipientsRef::Group(other))),
    }
}

fn push_damage_group(instructions: &mut Vec<Instruction>, group: DamageGroup, amount: ValueExpr) {
    instructions.push(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
            damage_group_to_target_query(group),
            amount,
            false,
        ),
    );
}

fn rewrite_bt_related_to_first_target_controller(bt: &mut BodyTarget) {
    if let BodyTarget::Object {
        zone: TargetZone::Battlefield,
        filter: obj,
    } = bt
    {
        let rewrite_clause = |c: &mut ObjectClause| {
            if c.controller == ControllerFilter::RelatedPlayer {
                c.controller = ControllerFilter::FirstTargetController;
            }
            if c.owner == ControllerFilter::RelatedPlayer {
                c.owner = ControllerFilter::FirstTargetController;
            }
        };
        match obj {
            ObjectFilter::Clause(c) => rewrite_clause(c),
            ObjectFilter::Either(a, b) => {
                rewrite_clause(a);
                rewrite_clause(b);
            }
        }
    }
}

fn is_owner_anaphora_label(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "its owner"
            | "their owner"
            | "that creature's owner"
            | "that permanent's owner"
            | "that card's owner"
    )
}

fn lower_player_matching_damage_selector(
    player: PlayerRefAst,
    condition: &ConditionAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::toolbox::PlayerSelector> {
    let ConditionAst::ControlsCount {
        player: cond_player,
        filter,
        op,
        value,
    } = condition
    else {
        return None;
    };
    let at_least_one =
        (*op == CompareOp::Ge && *value == 1) || (*op == CompareOp::Gt && *value == 0);
    if !at_least_one || !matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) {
        return None;
    }
    let mut clean = filter.as_ref().clone();
    clean.zone = None;
    match player {
        PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
            if !matches!(
                cond_player,
                PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::You
                    | PlayerRefAst::ThatPlayer
            ) || clean.controller.is_some()
            {
                return None;
            }
        }
        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
            if !matches!(
                cond_player,
                PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::You
                    | PlayerRefAst::ThatPlayer
            ) {
                return None;
            }
            if clean.controller.is_some()
                && !matches!(
                    clean.controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                )
            {
                return None;
            }
            clean.controller = Some(ControllerRelation::Opponent);
        }
        _ => return None,
    }
    let mut trial = builder.clone();
    let obj = lower_object_filter_in_context(&clean, &trial, in_trigger, vocabulary).ok()?;
    bind_filter_player_target(&mut trial, &obj).ok()?;
    *builder = trial;
    Some(spellwake_engine::toolbox::PlayerSelector::ControllersOf(
        Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
            spellwake_engine::toolbox::QueryZone::Battlefield,
            obj,
        )),
    ))
}

pub(super) fn lower_combat_damage_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            let target_is_explicit_target = matches!(
                target.as_ref(),
                SubjectRefAst::AnyTarget
                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                    | SubjectRefAst::TargetPlayer { .. }
                    | SubjectRefAst::TargetObject { .. }
                    | SubjectRefAst::Player(
                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                    )
            );
            let target_matches_permanent_body_target = matches!(
                target.as_ref(),
                SubjectRefAst::TargetObject { .. } | SubjectRefAst::AnyTarget
            ) || matches!(
                target.as_ref(),
                SubjectRefAst::Anaphora(a)
                    if !a.eq_ignore_ascii_case("itself")
                        && !a.eq_ignore_ascii_case("himself")
                        && !a.eq_ignore_ascii_case("herself")
                        && !a.eq_ignore_ascii_case("themselves")
                        && !is_player_anaphora_label(a)
                        && !is_owner_anaphora_label(a)
            );
            let target_is_returned_gy_card = matches!(
                builder.target,
                BodyTarget::Object {
                    zone: TargetZone::OwnGraveyard | TargetZone::AnyGraveyard { .. },
                    ..
                }
            ) && builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ZoneOp(spec)
                        if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                            && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Battlefield { .. })
                )
            });
            let target_can_be_damage_source =
                is_permanent_body_target(builder.target) || target_is_returned_gy_card;
            let is_explicit_self_noun = |raw: &str| {
                let low = raw.trim().to_ascii_lowercase();
                low == "this spell"
                    || ((self_pronoun_valid || in_trigger)
                        && matches!(
                            low.as_str(),
                            "this creature"
                                | "this permanent"
                                | "this artifact"
                                | "this enchantment"
                                | "this land"
                                | "this planeswalker"
                                | "this vehicle"
                                | "this equipment"
                                | "this aura"
                                | "this saga"
                                | "this battle"
                                | "this room"
                                | "this artifact creature"
                                | "this enchantment creature"
                        ))
            };
            let amount_is_anaphora_pt = matches!(
                amount.as_ref(),
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power | StatKind::Toughness,
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            );
            let spell_creature_bite_anaphora = !self_pronoun_valid
                && !in_trigger
                && target_can_be_damage_source
                && builder.target.guarantees_creature()
                && amount_is_anaphora_pt;
            let is_self_damage_source = is_self_pronoun(source, builder, self_pronoun_valid)
                || match source.as_ref() {
                    SubjectRefAst::Anaphora(raw) if is_explicit_self_noun(raw) => true,
                    SubjectRefAst::Anaphora(raw)
                        if !spell_creature_bite_anaphora
                            && is_self_anaphora_label(raw)
                            && (self_pronoun_valid
                                || (!in_trigger
                                    && !last_instruction_created_tokens(
                                        &builder.instructions,
                                    )
                                    && !last_instruction_moved_object(&builder.instructions)
                                    && (builder.target != BodyTarget::None
                                        || target_is_explicit_target
                                        || !target_can_be_damage_source)))
                            && (!target_can_be_damage_source
                                || target_matches_permanent_body_target) =>
                    {
                        true
                    }
                    _ => false,
                };
            if !is_self_damage_source {
                // Non-self creature deals damage equal to its power:
                // 1) "Target creature deals damage equal to its power to ~" -> TargetCreatureToAbilitySource
                // 2) "Target creature deals damage equal to its power to itself" -> TargetCreature
                let is_its_power = matches!(
                    amount.as_ref(),
                    ValueExprAst::ObjectStat {
                        subject,
                        stat: StatKind::Power,
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                );
                let bt_is_creature = |bt: BodyTarget| {
                    matches!(
                        bt,
                        BodyTarget::Object {
                            zone: TargetZone::Battlefield,
                            filter: ObjectFilter::Clause(c),
                        } if c.any_types == CardTypeMask::CREATURE
                    )
                };
                let eff_source = match source.as_ref() {
                    SubjectRefAst::InZone {
                        subject: inner,
                        zone: ZoneFilter::Battlefield,
                    } => inner.as_ref(),
                    other => other,
                };
                if is_its_power {
                    match eff_source {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target(&f_clean, vocabulary)?;
                            if bt_is_creature(bt) {
                                if is_self_pronoun(target, builder, self_pronoun_valid) {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                            spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                            ),
                                            spellwake_engine::toolbox::DamageTargetQuery::Relative(
                                                Box::new(DamageRecipientsRef::AbilitySource),
                                            ),
                                            DamageAmountSpec::SourcePower,
                                            false,
                                        ),
                                    );
                                    return Ok(());
                                }
                                if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                                {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                            spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                            ),
                                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                            DamageAmountSpec::SourcePower,
                                            false,
                                        ),
                                    );
                                    return Ok(());
                                }
                            }
                        }
                        SubjectRefAst::Anaphora(_) if bt_is_creature(builder.target) => {
                            if is_self_pronoun(target, builder, self_pronoun_valid) {
                                builder.instructions.push(
                                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                        spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                            spellwake_engine::toolbox::ObjectQuery::Targets,
                                        ),
                                        spellwake_engine::toolbox::DamageTargetQuery::Relative(
                                            Box::new(DamageRecipientsRef::AbilitySource),
                                        ),
                                        DamageAmountSpec::SourcePower,
                                        false,
                                    ),
                                );
                                return Ok(());
                            }
                            if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                            {
                                builder.instructions.push(
                                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                        spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                            spellwake_engine::toolbox::ObjectQuery::Targets,
                                        ),
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                        DamageAmountSpec::SourcePower,
                                        false,
                                    ),
                                );
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                    let lower_bite_recipient_bt =
                        |t: &SubjectRefAst,
                         b: &BodyBuilder|
                         -> Result<Option<BodyTarget>, &'static str> {
                            let bt_opt = match t {
                                SubjectRefAst::AnyTarget => Some(BodyTarget::SINGLE_ANY_TARGET),
                                SubjectRefAst::TargetPlayerOrPlaneswalker {
                                    opponent_only: false,
                                } => Some(BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER),
                                SubjectRefAst::TargetPlayerOrPlaneswalker {
                                    opponent_only: true,
                                } => Some(BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER),
                                SubjectRefAst::TargetPlayer {
                                    opponent_only,
                                    count: TargetCountAst::Exact(1),
                                } => Some(if *opponent_only {
                                    BodyTarget::SINGLE_OPPONENT
                                } else {
                                    BodyTarget::SINGLE_PLAYER
                                }),
                                SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                                    Some(BodyTarget::SINGLE_PLAYER)
                                }
                                SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                                    Some(BodyTarget::SINGLE_OPPONENT)
                                }
                                SubjectRefAst::TargetObject {
                                    filter: dst_f,
                                    count: TargetCountAst::Exact(1),
                                } if dst_f.zone.is_none()
                                    && !dst_f.is_ability
                                    && !dst_f.any_spell =>
                                {
                                    let mut bt = lower_battlefield_body_target_in_context(
                                        dst_f, b, in_trigger, vocabulary,
                                    )?;
                                    if !in_trigger
                                        && dst_f.controller == Some(ControllerRelation::ThatPlayer)
                                    {
                                        rewrite_bt_related_to_first_target_controller(&mut bt);
                                    }
                                    Some(bt)
                                }
                                SubjectRefAst::TargetOneOf {
                                    options,
                                    count: TargetCountAst::Exact(1),
                                } => {
                                    let mut bt = lower_target_one_of_damage_body_target(
                                        options, b, in_trigger, vocabulary,
                                    )?;
                                    if !in_trigger
                                        && options.iter().any(|o| {
                                            matches!(
                                                o,
                                                SubjectRefAst::TargetObject { filter, .. }
                                                    if filter.controller
                                                        == Some(ControllerRelation::ThatPlayer)
                                            )
                                        })
                                    {
                                        rewrite_bt_related_to_first_target_controller(&mut bt);
                                    }
                                    Some(bt)
                                }
                                _ => None,
                            };
                            Ok(bt_opt.filter(|bt| {
                                matches!(
                                    bt,
                                    BodyTarget::Object {
                                        zone: TargetZone::Battlefield,
                                        ..
                                    } | BodyTarget::Player { .. }
                                        | BodyTarget::AnyTarget(
                                            AnyTargetKind::Any
                                                | AnyTargetKind::CreatureOrPlayer
                                                | AnyTargetKind::PlayerOrPlaneswalker
                                                | AnyTargetKind::OpponentOrPlaneswalker
                                        )
                                )
                            }))
                        };
                    if let Ok(Some(dst_bt)) = lower_bite_recipient_bt(target, builder) {
                        if let SubjectRefAst::TargetObject {
                            filter: src_f,
                            count: TargetCountAst::Exact(1),
                        } = source.as_ref()
                            && src_f.zone.is_none()
                            && builder.target == BodyTarget::None
                            && builder.instructions.is_empty()
                            && let Ok(src_bt) = lower_battlefield_body_target_in_context(
                                src_f, builder, in_trigger, vocabulary,
                            )
                            && src_bt.guarantees_creature()
                        {
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        src_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    ),
                                )));
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        dst_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    ),
                                )));
                            builder.instructions.push(Instruction::DealDamage(Box::new(
                                spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(
                                    false,
                                ),
                            )));
                            return Ok(());
                        }
                        if matches!(source.as_ref(), SubjectRefAst::Anaphora(_))
                            && builder.target.guarantees_creature()
                            && !builder.instructions.is_empty()
                            && builder.graveyard_target_bounds.is_none()
                            && builder.permanent_target_bounds.is_none()
                            && !builder.x_target_count
                            && !builder.up_to_one_target
                            && !builder.instructions.iter().any(|op| {
                                matches!(op, Instruction::FixedTargetClause(_))
                                    || matches!(
                                        op,
                                        Instruction::MutateObject(spec)
                                            if matches!(
                                                spec.op,
                                                spellwake_engine::toolbox::ObjectMutation::StateMarker(spellwake_engine::toolbox::ObjectStateMarkerOp::RememberPermanentTarget)
                                            )
                                    )
                            })
                        {
                            let mut first_ops = std::mem::take(&mut builder.instructions);
                            prepend_instruction_at(
                                &mut first_ops,
                                0,
                                Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target())),
                            );
                            let first_bt = std::mem::replace(&mut builder.target, BodyTarget::None);
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(first_bt, first_ops),
                                )));
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        dst_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    ),
                                )));
                            builder.instructions.push(Instruction::DealDamage(Box::new(
                                spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(
                                    false,
                                ),
                            )));
                            return Ok(());
                        }
                    }
                }
                // General two-permanent-target non-self damage (e.g. "Target creature deals
                // damage equal to its toughness / N / X to target creature/planeswalker").
                if builder.target == BodyTarget::None
                    && builder.instructions.is_empty()
                    && let SubjectRefAst::TargetObject {
                        filter: src_f,
                        count: TargetCountAst::Exact(1),
                    } = eff_source
                    && matches!(src_f.zone, None | Some(ZoneFilter::Battlefield))
                    && !src_f.is_ability
                    && !src_f.any_spell
                    && let SubjectRefAst::TargetObject {
                        filter: dst_f,
                        count: TargetCountAst::Exact(1),
                    } = target.as_ref()
                    && matches!(dst_f.zone, None | Some(ZoneFilter::Battlefield))
                    && !dst_f.is_ability
                    && !dst_f.any_spell
                {
                    let mut src_clean = src_f.as_ref().clone();
                    src_clean.zone = None;
                    let mut dst_clean = dst_f.as_ref().clone();
                    dst_clean.zone = None;
                    if let (Ok(src_bt), Ok(dst_bt)) = (
                        lower_battlefield_body_target_in_context(
                            &src_clean, builder, in_trigger, vocabulary,
                        ),
                        lower_battlefield_body_target_in_context(
                            &dst_clean, builder, in_trigger, vocabulary,
                        ),
                    ) && is_permanent_body_target(src_bt)
                        && is_permanent_body_target(dst_bt)
                    {
                        let is_its_toughness = matches!(
                            amount.as_ref(),
                            ValueExprAst::ObjectStat {
                                subject,
                                stat: StatKind::Toughness,
                            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                        );
                        let amt_res = if is_its_power {
                            Ok(DamageAmountSpec::SourcePower)
                        } else if is_its_toughness {
                            Ok(DamageAmountSpec::SourceToughness)
                        } else {
                            let resolved_amt = resolve_x(amount, bound_x);
                            let sac_rewritten = rewrite_sacrificed_permanent_anaphora(resolved_amt);
                            let eff_amt = sac_rewritten.as_ref().unwrap_or(resolved_amt);
                            lower_value_expr_in_context(eff_amt, builder, in_trigger, vocabulary)
                                .map(|e| {
                                    DamageAmountSpec::Expr(rewrite_milled_card_mana_value(
                                        e, builder,
                                    ))
                                })
                        };
                        if let Ok(amt_spec) = amt_res {
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        src_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    ),
                                )));
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        dst_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    ),
                                )));
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                    spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                        spellwake_engine::toolbox::ObjectQuery::LinkedTarget {
                                            index: 0,
                                        },
                                    ),
                                    spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                        spellwake_engine::toolbox::ObjectQuery::LinkedTarget {
                                            index: 1,
                                        },
                                    ),
                                    amt_spec,
                                    false,
                                ),
                            );
                            return Ok(());
                        }
                    }
                }
                let unwrapped_eff_source = unwrap_in_zone_subject(eff_source);
                let eff_source = unwrapped_eff_source.as_ref().unwrap_or(eff_source);
                let unwrapped_target = unwrap_in_zone_subject(target);
                let target_ref: &SubjectRefAst =
                    unwrapped_target.as_ref().unwrap_or(target.as_ref());
                let src_ref = match eff_source {
                    SubjectRefAst::SelfObject => Some(DamageSourceRef::AbilitySource),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            builder.up_to_one_target = true;
                        }
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if last_instruction_created_tokens(&builder.instructions)
                            || (has_prior_token_creation(&builder.instructions)
                                && raw.to_ascii_lowercase().contains("token")) =>
                    {
                        Some(DamageSourceRef::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(builder.target)
                            && builder.permanent_target_bounds.is_some_and(|b| b != (1, 1)) =>
                    {
                        Some(DamageSourceRef::AllPermanentTargets)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if target_can_be_damage_source
                            && (!is_self_anaphora_label(raw)
                                || !target_matches_permanent_body_target) =>
                    {
                        Some(DamageSourceRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        Some(DamageSourceRef::LastMoved)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        Some(DamageSourceRef::TriggeringObject)
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        Some(DamageSourceRef::AttachedHost)
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        Some(DamageSourceRef::Matching(group_obj))
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        Some(DamageSourceRef::Matching(obj))
                    }
                    _ => None,
                };
                if let Some(src) = src_ref {
                    let is_its_toughness = matches!(
                        amount.as_ref(),
                        ValueExprAst::ObjectStat {
                            subject,
                            stat: StatKind::Toughness,
                        } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    );
                    let rec_ref = match target_ref {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                            && !filter.is_ability
                            && !filter.any_spell =>
                        {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::UpTo(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            builder.up_to_one_target = true;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } => {
                            let bt = lower_target_one_of_damage_body_target(
                                options, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            if *count == TargetCountAst::UpTo(1) {
                                if is_permanent_body_target(bt) {
                                    builder.up_to_one_target = true;
                                } else if src == DamageSourceRef::AbilitySource
                                    && !is_its_power
                                    && !is_its_toughness
                                {
                                    let resolved_amt = resolve_x(amount, bound_x);
                                    let sac_rewritten =
                                        rewrite_sacrificed_permanent_anaphora(resolved_amt);
                                    let eff_amt = sac_rewritten.as_ref().unwrap_or(resolved_amt);
                                    let expr = rewrite_milled_card_mana_value(
                                        lower_value_expr_in_context(
                                            eff_amt, builder, in_trigger, vocabulary,
                                        )?,
                                        builder,
                                    );
                                    builder.instructions.push(Instruction::DealDamage(Box::new(
                                        spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                                            expr,
                                            0,
                                            1,
                                        ),
                                    )));
                                    return Ok(());
                                } else {
                                    return Err("oracle:unsupported-damage-target");
                                }
                            }
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::AnyTarget
                        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                        | SubjectRefAst::TargetPlayer { .. }
                        | SubjectRefAst::TargetObject { .. } => {
                            let bt = lower_target_subject(target, vocabulary)?;
                            builder.bind_target(bt)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                            builder.bind_target(BodyTarget::SINGLE_PLAYER)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                            builder.bind_target(BodyTarget::SINGLE_OPPONENT)?;
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Anaphora(a)
                            if a.eq_ignore_ascii_case("itself")
                                || a.eq_ignore_ascii_case("himself")
                                || a.eq_ignore_ascii_case("herself")
                                || a.eq_ignore_ascii_case("themselves") =>
                        {
                            match &src {
                                DamageSourceRef::TargetObject => Some(DamageRecipientsRef::Target),
                                DamageSourceRef::TriggeringObject => {
                                    Some(DamageRecipientsRef::TriggeringObject)
                                }
                                DamageSourceRef::AttachedHost => {
                                    Some(DamageRecipientsRef::AttachedHost)
                                }
                                DamageSourceRef::LastMoved => Some(DamageRecipientsRef::LastMoved),
                                DamageSourceRef::CreatedTokens => {
                                    Some(DamageRecipientsRef::CreatedTokens)
                                }
                                DamageSourceRef::Matching(_)
                                | DamageSourceRef::AllPermanentTargets => {
                                    Some(DamageRecipientsRef::EachSourceToItself)
                                }
                                _ => None,
                            }
                        }
                        SubjectRefAst::Anaphora(raw)
                            if is_player_anaphora_label(raw)
                                && matches!(
                                    builder.target,
                                    BodyTarget::Spell {
                                        include_abilities: false,
                                        ..
                                    }
                                ) =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                            )))
                        }
                        SubjectRefAst::Anaphora(raw)
                            if is_player_anaphora_label(raw)
                                && is_permanent_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Anaphora(_)
                            if last_instruction_created_tokens(&builder.instructions)
                                && !matches!(src, DamageSourceRef::CreatedTokens) =>
                        {
                            Some(DamageRecipientsRef::CreatedTokens)
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target != BodyTarget::None
                                && !is_stack_body_target(builder.target)
                                && !is_graveyard_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target)
                                && last_instruction_moved_object(&builder.instructions)
                                && !matches!(src, DamageSourceRef::LastMoved) =>
                        {
                            Some(DamageRecipientsRef::LastMoved)
                        }
                        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                            Some(DamageRecipientsRef::AbilitySource)
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some(DamageRecipientsRef::TriggeringObject)
                        }
                        SubjectRefAst::Anaphora(raw)
                            if in_trigger
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                        {
                            Some(DamageRecipientsRef::TriggeringObject)
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            Some(DamageRecipientsRef::AttachedHost)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                        ) if matches!(
                            src,
                            DamageSourceRef::Matching(_)
                                | DamageSourceRef::AllPermanentTargets
                                | DamageSourceRef::CreatedTokens
                                | DamageSourceRef::LastMoved
                        ) =>
                        {
                            Some(DamageRecipientsRef::EachSourceController)
                        }
                        SubjectRefAst::ControllerOf(inner)
                            if matches!(inner.as_ref(), SubjectRefAst::Anaphora(_))
                                && matches!(
                                    src,
                                    DamageSourceRef::Matching(_)
                                        | DamageSourceRef::AllPermanentTargets
                                        | DamageSourceRef::CreatedTokens
                                        | DamageSourceRef::LastMoved
                                        | DamageSourceRef::TargetObject
                                        | DamageSourceRef::TriggeringObject
                                        | DamageSourceRef::AttachedHost
                                ) =>
                        {
                            Some(DamageRecipientsRef::EachSourceController)
                        }
                        SubjectRefAst::Anaphora(raw)
                            if matches!(
                                raw.trim().to_ascii_lowercase().as_str(),
                                "its controller"
                                    | "their controller"
                                    | "that creature's controller"
                                    | "that permanent's controller"
                            ) && matches!(
                                src,
                                DamageSourceRef::Matching(_)
                                    | DamageSourceRef::AllPermanentTargets
                                    | DamageSourceRef::CreatedTokens
                                    | DamageSourceRef::LastMoved
                                    | DamageSourceRef::TargetObject
                                    | DamageSourceRef::TriggeringObject
                                    | DamageSourceRef::AttachedHost
                            ) =>
                        {
                            Some(DamageRecipientsRef::EachSourceController)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                        ) if builder.target != BodyTarget::None => {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if matches!(builder.target, BodyTarget::Player { .. }) =>
                        {
                            Some(DamageRecipientsRef::Target)
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if matches!(
                                builder.target,
                                BodyTarget::Spell {
                                    include_abilities: false,
                                    ..
                                }
                            ) =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::You) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Controller))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Opponents))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachPlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::Players))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachOtherPlayer) => {
                            Some(DamageRecipientsRef::EachOtherPlayer)
                        }
                        SubjectRefAst::Player(PlayerRefAst::EachOtherOpponent) => {
                            Some(DamageRecipientsRef::EachOtherOpponent)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponentAtRandom) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::RandomOpponent),
                            )))
                        }
                        SubjectRefAst::Player(
                            p @ (PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer),
                        ) => {
                            builder
                                .instructions
                                .push(Instruction::PromptChoice(Box::new(
                                    spellwake_engine::toolbox::PromptChoiceSpec::choose_value(
                                        ResolvingPlayer::Controller,
                                        spellwake_engine::toolbox::PromptDomain::player(matches!(
                                            p,
                                            PlayerRefAst::AnyOpponent
                                        )),
                                    ),
                                )));
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::ChosenPlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::ActivePlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::ActivePlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::DefendingPlayer) => {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::EnchantedPlayer)
                            if in_trigger || self_pronoun_valid || builder.has_source =>
                        {
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                AffectedPlayers::One(ResolvingPlayer::EnchantedPlayer),
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::ChosenPlayer) => {
                            let players = lower_affected_players_in_context(
                                PlayerRefAst::ChosenPlayer,
                                builder,
                                in_trigger,
                            )?;
                            Some(DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(
                                players,
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                            if !in_trigger && is_permanent_body_target(builder.target) =>
                        {
                            Some(DamageRecipientsRef::TargetController)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ThatObjectController,
                        ) if in_trigger => {
                            Some(DamageRecipientsRef::Group(DamageGroup::RelatedPlayer))
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                        {
                            let mut f_clean = f.as_ref().clone();
                            f_clean.zone = None;
                            let group = lower_damage_group(&f_clean, vocabulary)?;
                            if let DamageGroup::Filtered(obj) = group {
                                bind_filter_player_target(builder, &obj)?;
                            }
                            Some(DamageRecipientsRef::Group(group))
                        }
                        SubjectRefAst::Compound(items) if items.len() >= 2 => {
                            let is_other_creatures = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                                            && f.any_types == AstCardTypeMask::CREATURE
                                            && f.flags == AstFilterFlags::EXCLUDE_SELF
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    flags: true,
                                                    zone: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_all_creatures = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                                            && f.any_types == AstCardTypeMask::CREATURE
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    zone: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_all_pws = |s: &SubjectRefAst| {
                                matches!(
                                    s,
                                    SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                                            && f.any_types == AstCardTypeMask::PLANESWALKER
                                            && filter_only_has(
                                                f,
                                                FilterAllow {
                                                    types: true,
                                                    zone: true,
                                                    ..FilterAllow::NONE
                                                },
                                            )
                                )
                            };
                            let is_each_player = |s: &SubjectRefAst| {
                                matches!(s, SubjectRefAst::Player(PlayerRefAst::EachPlayer))
                            };
                            if items.len() == 2
                                && ((is_other_creatures(&items[0]) && is_each_player(&items[1]))
                                    || (is_each_player(&items[0]) && is_other_creatures(&items[1])))
                            {
                                Some(DamageRecipientsRef::OtherCreaturesAndEachPlayer)
                            } else if items.len() == 2
                                && ((is_all_creatures(&items[0]) && is_each_player(&items[1]))
                                    || (is_each_player(&items[0]) && is_all_creatures(&items[1])))
                            {
                                Some(DamageRecipientsRef::Group(DamageGroup::CreaturesAndPlayers))
                            } else if items.len() == 2
                                && ((is_all_creatures(&items[0]) && is_all_pws(&items[1]))
                                    || (is_all_pws(&items[0]) && is_all_creatures(&items[1])))
                            {
                                Some(DamageRecipientsRef::Group(
                                    DamageGroup::CreaturesAndPlaneswalkers,
                                ))
                            } else {
                                let mut player_group: Option<AffectedPlayers> = None;
                                let mut multi_players = false;
                                for item in items {
                                    if let SubjectRefAst::Player(p) = item {
                                        let ap = lower_affected_players_in_context(
                                            *p, builder, in_trigger,
                                        )?;
                                        if player_group.is_some() {
                                            multi_players = true;
                                            break;
                                        }
                                        player_group = Some(ap);
                                    }
                                }
                                if multi_players {
                                    None
                                } else {
                                    let mut perm_filters = Vec::new();
                                    let mut all_ok = true;
                                    for item in items {
                                        match item {
                                            SubjectRefAst::Each(f)
                                            | SubjectRefAst::All(f)
                                            | SubjectRefAst::Group(f)
                                                if matches!(
                                                    f.zone,
                                                    None | Some(ZoneFilter::Battlefield)
                                                ) =>
                                            {
                                                let mut adj = f.as_ref().clone();
                                                adj.zone = None;
                                                if adj.controller
                                                    == Some(ControllerRelation::ThatPlayer)
                                                {
                                                    match player_group {
                                                        Some(AffectedPlayers::EachOpponent) => {
                                                            adj.controller =
                                                                Some(ControllerRelation::Opponent);
                                                        }
                                                        Some(AffectedPlayers::EachPlayer) => {
                                                            adj.controller = None;
                                                        }
                                                        Some(AffectedPlayers::EachOtherPlayer) => {
                                                            adj.controller =
                                                                Some(ControllerRelation::NotYou);
                                                        }
                                                        Some(AffectedPlayers::One(
                                                            ResolvingPlayer::Controller,
                                                        )) => {
                                                            adj.controller =
                                                                Some(ControllerRelation::You);
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                let obj = lower_object_filter_in_context(
                                                    &adj, builder, in_trigger, vocabulary,
                                                )?;
                                                bind_filter_player_target(builder, &obj)?;
                                                perm_filters.push(obj);
                                            }
                                            SubjectRefAst::Player(_) => {}
                                            _ => {
                                                all_ok = false;
                                                break;
                                            }
                                        }
                                    }
                                    if !all_ok || perm_filters.is_empty() {
                                        None
                                    } else {
                                        let combined_filter = if perm_filters.len() == 1 {
                                            Some(perm_filters[0])
                                        } else {
                                            let mut clauses = Vec::new();
                                            for f in perm_filters {
                                                match f {
                                                    ObjectFilter::Clause(c) => clauses.push(c),
                                                    ObjectFilter::Either(a, b) => {
                                                        clauses.push(a);
                                                        clauses.push(b);
                                                    }
                                                }
                                            }
                                            let mut merged: Vec<ObjectClause> =
                                                Vec::with_capacity(clauses.len());
                                            for c in clauses {
                                                if merged.contains(&c) {
                                                    continue;
                                                }
                                                let mut folded = false;
                                                if !c.any_types.is_empty() {
                                                    for existing in &mut merged {
                                                        if !existing.any_types.is_empty() {
                                                            let mut probe = *existing;
                                                            probe.any_types = c.any_types;
                                                            if probe == c {
                                                                existing.any_types = existing
                                                                    .any_types
                                                                    .union(c.any_types);
                                                                folded = true;
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if !folded {
                                                    merged.push(c);
                                                }
                                            }
                                            match merged.as_slice() {
                                                [only] => Some(ObjectFilter::Clause(*only)),
                                                [first, second] => {
                                                    Some(ObjectFilter::Either(*first, *second))
                                                }
                                                _ => None,
                                            }
                                        };
                                        combined_filter.map(|filter| match player_group {
                                            Some(players) => DamageRecipientsRef::Group(
                                                DamageGroup::FilteredAndPlayers { filter, players },
                                            ),
                                            None => DamageRecipientsRef::Group(
                                                DamageGroup::Filtered(filter),
                                            ),
                                        })
                                    }
                                }
                            }
                        }
                        _ => None,
                    };
                    let amt_spec = if is_its_power {
                        DamageAmountSpec::SourcePower
                    } else if is_its_toughness {
                        DamageAmountSpec::SourceToughness
                    } else {
                        let resolved_amt = resolve_x(amount, bound_x);
                        let sac_rewritten = rewrite_sacrificed_permanent_anaphora(resolved_amt);
                        let eff_amt = sac_rewritten.as_ref().unwrap_or(resolved_amt);
                        DamageAmountSpec::Expr(rewrite_milled_card_mana_value(
                            lower_value_expr_in_context(eff_amt, builder, in_trigger, vocabulary)?,
                            builder,
                        ))
                    };
                    if let Some(rec) = rec_ref {
                        let source_q = damage_source_ref_to_query(src, &rec);
                        let target_q = match rec {
                            DamageRecipientsRef::Target => {
                                spellwake_engine::toolbox::DamageTargetQuery::Targets
                            }
                            DamageRecipientsRef::Group(group) => {
                                damage_group_to_target_query(group)
                            }
                            other => spellwake_engine::toolbox::DamageTargetQuery::Relative(
                                Box::new(other),
                            ),
                        };
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                source_q, target_q, amt_spec, false,
                            ),
                        );
                        return Ok(());
                    }
                    let src_obj_q_opt = match &src {
                        DamageSourceRef::TargetObject => {
                            Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                        }
                        DamageSourceRef::TriggeringObject => {
                            Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                        }
                        DamageSourceRef::AttachedHost => {
                            Some(spellwake_engine::toolbox::ObjectQuery::AttachedHost)
                        }
                        DamageSourceRef::AbilitySource => {
                            Some(spellwake_engine::toolbox::ObjectQuery::Source)
                        }
                        DamageSourceRef::CreatedTokens => {
                            Some(spellwake_engine::toolbox::ObjectQuery::CreatedTokens)
                        }
                        DamageSourceRef::LastMoved => {
                            Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                        }
                        DamageSourceRef::Matching(obj) => {
                            Some(spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                *obj,
                            ))
                        }
                        DamageSourceRef::AllPermanentTargets => {
                            Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                        }
                    };
                    if let Some(src_obj_q) = src_obj_q_opt {
                        let extra_target_q: Option<spellwake_engine::toolbox::DamageTargetQuery> =
                            match target_ref {
                                SubjectRefAst::Player(PlayerRefAst::ItsOwner) => Some(
                                    spellwake_engine::toolbox::DamageTargetQuery::Players(
                                        spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                            Box::new(src_obj_q.clone()),
                                        ),
                                    ),
                                ),
                                SubjectRefAst::Anaphora(raw) if is_owner_anaphora_label(raw) => {
                                    Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                        spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                            Box::new(src_obj_q.clone()),
                                        ),
                                    ))
                                }
                                SubjectRefAst::PlayerMatching { player, condition } => {
                                    lower_player_matching_damage_selector(
                                        *player, condition, builder, in_trigger, vocabulary,
                                    )
                                    .map(spellwake_engine::toolbox::DamageTargetQuery::Players)
                                }
                                SubjectRefAst::OwnerOf(inner) => match inner.as_ref() {
                                    SubjectRefAst::Anaphora(_) => Some(
                                        spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                                Box::new(src_obj_q.clone()),
                                            ),
                                        ),
                                    ),
                                    SubjectRefAst::TargetObject {
                                        count: TargetCountAst::Exact(1),
                                        ..
                                    } => {
                                        let bt = lower_target_subject(inner, vocabulary)?;
                                        builder.bind_target(bt)?;
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetOwner,
                                            ),
                                        ))
                                    }
                                    SubjectRefAst::TriggerSubject if in_trigger => Some(
                                        spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TriggerSubjectOwner,
                                            ),
                                        ),
                                    ),
                                    SubjectRefAst::SelfObject
                                        if in_trigger || self_pronoun_valid || builder.has_source =>
                                    {
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::SourceOwner,
                                            ),
                                        ))
                                    }
                                    SubjectRefAst::AttachedHost(_)
                                        if in_trigger || self_pronoun_valid || builder.has_source =>
                                    {
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                                Box::new(
                                                    spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                                                ),
                                            ),
                                        ))
                                    }
                                    _ => None,
                                },
                                SubjectRefAst::ControllerOf(inner) => match inner.as_ref() {
                                    SubjectRefAst::TargetObject {
                                        count: TargetCountAst::Exact(1),
                                        ..
                                    } => {
                                        let bt = lower_target_subject(inner, vocabulary)?;
                                        builder.bind_target(bt)?;
                                        let rp = if is_stack_body_target(bt) {
                                            ResolvingPlayer::TargetSpellController
                                        } else {
                                            ResolvingPlayer::TargetController
                                        };
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                rp,
                                            ),
                                        ))
                                    }
                                    SubjectRefAst::TriggerSubject if in_trigger => Some(
                                        spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TriggerSubjectController,
                                            ),
                                        ),
                                    ),
                                    SubjectRefAst::SelfObject => Some(
                                        spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    SubjectRefAst::AttachedHost(_)
                                        if in_trigger || self_pronoun_valid || builder.has_source =>
                                    {
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::AttachedHostController,
                                            ),
                                        ))
                                    }
                                    SubjectRefAst::All(f)
                                    | SubjectRefAst::Each(f)
                                    | SubjectRefAst::Group(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                                    {
                                        let mut clean = f.as_ref().clone();
                                        clean.zone = None;
                                        let obj = lower_object_filter_in_context(
                                            &clean, builder, in_trigger, vocabulary,
                                        )?;
                                        bind_filter_player_target(builder, &obj)?;
                                        Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                            spellwake_engine::toolbox::PlayerSelector::ControllersOf(
                                                Box::new(
                                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                                        spellwake_engine::toolbox::QueryZone::Battlefield,
                                                        obj,
                                                    ),
                                                ),
                                            ),
                                        ))
                                    }
                                    _ => None,
                                },
                                SubjectRefAst::Player(
                                    PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                                ) => {
                                    let rp = lower_resolving_player_in_context(
                                        PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                                        builder,
                                        in_trigger,
                                    )?;
                                    Some(spellwake_engine::toolbox::DamageTargetQuery::Players(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                    ))
                                }
                                SubjectRefAst::Compound(items) if items.len() >= 2 => {
                                    let mut obj_queries = Vec::new();
                                    let mut player_sel: Option<
                                        spellwake_engine::toolbox::PlayerSelector,
                                    > = None;
                                    let mut all_ok = true;
                                    for item in items {
                                        match item {
                                            SubjectRefAst::Each(f)
                                            | SubjectRefAst::All(f)
                                            | SubjectRefAst::Group(f)
                                                if matches!(
                                                    f.zone,
                                                    None | Some(ZoneFilter::Battlefield)
                                                ) =>
                                            {
                                                let mut clean = f.as_ref().clone();
                                                clean.zone = None;
                                                if let Ok(obj) = lower_object_filter_in_context(
                                                    &clean, builder, in_trigger, vocabulary,
                                                ) && bind_filter_player_target(builder, &obj)
                                                    .is_ok()
                                                {
                                                    obj_queries.push(
                                                        spellwake_engine::toolbox::ObjectQuery::zone(
                                                            spellwake_engine::toolbox::QueryZone::Battlefield,
                                                            obj,
                                                        ),
                                                    );
                                                } else {
                                                    all_ok = false;
                                                    break;
                                                }
                                            }
                                            SubjectRefAst::Player(p) if player_sel.is_none() => {
                                                if let Ok(ap) = lower_affected_players_in_context(
                                                    *p, builder, in_trigger,
                                                ) {
                                                    player_sel = Some(
                                                        spellwake_engine::toolbox::PlayerSelector::Group(
                                                            ap,
                                                        ),
                                                    );
                                                } else {
                                                    all_ok = false;
                                                    break;
                                                }
                                            }
                                            SubjectRefAst::PlayerMatching { player, condition }
                                                if player_sel.is_none() =>
                                            {
                                                if let Some(sel) =
                                                    lower_player_matching_damage_selector(
                                                        *player, condition, builder, in_trigger,
                                                        vocabulary,
                                                    )
                                                {
                                                    player_sel = Some(sel);
                                                } else {
                                                    all_ok = false;
                                                    break;
                                                }
                                            }
                                            _ => {
                                                all_ok = false;
                                                break;
                                            }
                                        }
                                    }
                                    if all_ok && !obj_queries.is_empty() {
                                        let oq = if obj_queries.len() == 1 {
                                            obj_queries.remove(0)
                                        } else {
                                            spellwake_engine::toolbox::ObjectQuery::Union(
                                                obj_queries,
                                            )
                                        };
                                        Some(match player_sel {
                                            Some(players) => {
                                                spellwake_engine::toolbox::DamageTargetQuery::Both {
                                                    objects: oq,
                                                    players,
                                                }
                                            }
                                            None => {
                                                spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                                    oq,
                                                )
                                            }
                                        })
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            };
                        if let Some(target_q) = extra_target_q {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                                    spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                        src_obj_q,
                                    ),
                                    target_q,
                                    amt_spec,
                                    false,
                                ),
                            );
                            return Ok(());
                        }
                    }
                }
                return Err("oracle:non-self-damage-source");
            }
            let had_prior_target = builder.target != BodyTarget::None;
            let raw_amount = resolve_x(amount, bound_x);
            let sac_rewritten_amount = rewrite_sacrificed_permanent_anaphora(raw_amount);
            let raw_amount_eff = sac_rewritten_amount.as_ref().unwrap_or(raw_amount);
            let rewrite_self_anaphora = !had_prior_target
                && (self_pronoun_valid || in_trigger)
                && (!(in_trigger && self_pronoun_valid)
                    || matches!(target.as_ref(), SubjectRefAst::TargetObject { .. }));
            let rewritten_amount;
            let effective_amount =
                if rewrite_self_anaphora && value_expr_ast_has_self_anaphora(raw_amount_eff) {
                    rewritten_amount = rewrite_value_expr_ast_self_anaphora(raw_amount_eff);
                    &rewritten_amount
                } else {
                    raw_amount_eff
                };
            let need_pre_bind_ctx = !had_prior_target
                && ((in_trigger
                    && !self_pronoun_valid
                    && value_expr_ast_has_bare_object_anaphora(effective_amount))
                    || (value_expr_ast_has_exiled_cost_ref(effective_amount)
                        && !has_prior_exile_instruction(&builder.instructions)));
            let pre_bind_builder = need_pre_bind_ctx.then(|| builder.clone());
            let lower_amt = |b: &BodyBuilder| -> Result<ValueExpr, &'static str> {
                let eval_ctx = pre_bind_builder.as_ref().unwrap_or(b);
                Ok(rewrite_milled_card_mana_value(
                    lower_value_expr_in_context(
                        effective_amount,
                        eval_ctx,
                        in_trigger,
                        vocabulary,
                    )?,
                    eval_ctx,
                ))
            };
            // Pre-bind `ControllerOf(TargetObject)` so value expressions referencing that
            // targeted spell or permanent (e.g. "equal to that spell's mana value") resolve.
            if let SubjectRefAst::ControllerOf(inner) = target.as_ref()
                && let SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } = inner.as_ref()
            {
                let bt = lower_target_subject(inner, vocabulary)?;
                builder.bind_target(bt)?;
                let amt_expr = lower_amt(builder)?;
                let rp = if is_stack_body_target(bt) {
                    ResolvingPlayer::TargetSpellController
                } else if is_permanent_body_target(bt) {
                    ResolvingPlayer::TargetController
                } else {
                    return Err("oracle:unsupported-damage-target");
                };
                push_damage_group(
                    &mut builder.instructions,
                    DamageGroup::AffectedPlayers(AffectedPlayers::One(rp)),
                    amt_expr,
                );
                return Ok(());
            }
            if let SubjectRefAst::OwnerOf(inner) = target.as_ref()
                && let SubjectRefAst::TargetObject {
                    count: TargetCountAst::Exact(1),
                    ..
                } = inner.as_ref()
            {
                let bt = lower_target_subject(inner, vocabulary)?;
                builder.bind_target(bt)?;
                let amt_expr = lower_amt(builder)?;
                push_damage_group(
                    &mut builder.instructions,
                    DamageGroup::AffectedPlayers(AffectedPlayers::One(
                        ResolvingPlayer::TargetOwner,
                    )),
                    amt_expr,
                );
                return Ok(());
            }
            if let SubjectRefAst::InZone {
                subject: inner_target,
                zone: ZoneFilter::Battlefield,
            } = target.as_ref()
            {
                return lower_combat_damage_instruction(
                    &InstructionAst::DealDamage {
                        source: source.clone(),
                        target: inner_target.clone(),
                        amount: amount.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            match target.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                    && !filter.is_ability
                    && !filter.any_spell =>
                {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !filter.is_ability
                        && !filter.any_spell
                        && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                {
                    let (min_t, max_t, is_x) = match *count {
                        TargetCountAst::Exact(n) => (n, n, false),
                        TargetCountAst::UpTo(max) => (0, max, false),
                        TargetCountAst::AnyNumber => (0, u16::MAX, false),
                        TargetCountAst::ChosenX => (0, u16::MAX, true),
                        _ => return Err("oracle:unsupported-damage-target"),
                    };
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if is_x {
                        builder.x_target_count = true;
                    }
                    let amt_expr = lower_amt(builder)?;
                    builder.push_bounded_target_op(
                        min_t,
                        max_t,
                        super::super::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetPlayer {
                    opponent_only,
                    count,
                } if matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                    || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                    || matches!(count, TargetCountAst::AnyNumber)
                    || (matches!(count, TargetCountAst::ChosenX) && !in_trigger) =>
                {
                    let (min_t, max_t, is_x) = match *count {
                        TargetCountAst::Exact(n) => (n, n, false),
                        TargetCountAst::UpTo(max) => (0, max, false),
                        TargetCountAst::AnyNumber => (0, u16::MAX, false),
                        TargetCountAst::ChosenX => (0, u16::MAX, true),
                        _ => return Err("oracle:unsupported-damage-target"),
                    };
                    builder.bind_target(if *opponent_only {
                        BodyTarget::SINGLE_OPPONENT
                    } else {
                        BodyTarget::SINGLE_PLAYER
                    })?;
                    if is_x {
                        builder.x_target_count = true;
                    }
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                            amt_expr, min_t, max_t,
                        ),
                    )));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf { options, count } => {
                    let bt = lower_target_one_of_damage_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let amt_expr = lower_amt(builder)?;
                    match *count {
                        TargetCountAst::Exact(1) => {
                            push_damage_target(&mut builder.instructions, amt_expr);
                            Ok(())
                        }
                        TargetCountAst::Exact(n) if n >= 2 => {
                            if is_permanent_body_target(bt) {
                                builder.push_bounded_target_op(
                                    n,
                                    n,
                                    super::super::toolbox_emit::deal_damage(
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                        amt_expr,
                                        false,
                                    ),
                                );
                            } else {
                                builder.instructions.push(Instruction::DealDamage(Box::new(
                                    spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                                        amt_expr, n, n,
                                    ),
                                )));
                            }
                            Ok(())
                        }
                        TargetCountAst::UpTo(max) if max > 0 => {
                            if is_permanent_body_target(bt) {
                                builder.push_bounded_target_op(
                                    0,
                                    max,
                                    super::super::toolbox_emit::deal_damage(
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                        amt_expr,
                                        false,
                                    ),
                                );
                            } else {
                                builder.instructions.push(Instruction::DealDamage(Box::new(
                                    spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                                        amt_expr, 0, max,
                                    ),
                                )));
                            }
                            Ok(())
                        }
                        TargetCountAst::AnyNumber => {
                            if is_permanent_body_target(bt) {
                                builder.push_bounded_target_op(
                                    0,
                                    u16::MAX,
                                    super::super::toolbox_emit::deal_damage(
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                        amt_expr,
                                        false,
                                    ),
                                );
                            } else {
                                builder.instructions.push(Instruction::DealDamage(Box::new(
                                    spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                                        amt_expr,
                                        0,
                                        u16::MAX,
                                    ),
                                )));
                            }
                            Ok(())
                        }
                        TargetCountAst::ChosenX if !in_trigger => {
                            builder.x_target_count = true;
                            if is_permanent_body_target(bt) {
                                builder.push_bounded_target_op(
                                    0,
                                    u16::MAX,
                                    super::super::toolbox_emit::deal_damage(
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                        amt_expr,
                                        false,
                                    ),
                                );
                            } else {
                                builder.instructions.push(Instruction::DealDamage(Box::new(
                                    spellwake_engine::toolbox::DealDamageSpec::announced_targets(
                                        amt_expr,
                                        0,
                                        u16::MAX,
                                    ),
                                )));
                            }
                            Ok(())
                        }
                        _ => Err("oracle:unsupported-damage-target"),
                    }
                }
                SubjectRefAst::AnyTarget
                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                | SubjectRefAst::TargetPlayer { .. }
                | SubjectRefAst::TargetObject { .. } => {
                    let bt = lower_target_subject(target, vocabulary)?;
                    builder.bind_target(bt)?;
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if is_player_anaphora_label(raw)
                        && matches!(
                            builder.target,
                            BodyTarget::Spell {
                                include_abilities: false,
                                ..
                            }
                        ) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AffectedPlayers(AffectedPlayers::One(
                            ResolvingPlayer::TargetSpellController,
                        )),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if is_player_anaphora_label(raw)
                        && is_permanent_body_target(builder.target) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AffectedPlayers(AffectedPlayers::One(
                            ResolvingPlayer::TargetController,
                        )),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if builder.target == BodyTarget::None
                        && is_player_anaphora_label(raw)
                        && !(raw.trim().eq_ignore_ascii_case("them")
                            && (last_instruction_created_tokens(&builder.instructions)
                                || last_instruction_moved_object(&builder.instructions)
                                || builder.last_group_filter.is_some())) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AffectedPlayers(AffectedPlayers::One(rp)),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::ControllerOf(inner) => {
                    let amt_expr = lower_amt(builder)?;
                    let selector = match inner.as_ref() {
                        SubjectRefAst::Anaphora(_) if is_stack_body_target(builder.target) => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetSpellController,
                            )
                        }
                        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetController,
                            )
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TriggerSubjectController,
                            )
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None && in_trigger =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TriggerSubjectController,
                            )
                        }
                        SubjectRefAst::AttachedHost(_)
                            if self_pronoun_valid || in_trigger || builder.has_source =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::AttachedHostController,
                            )
                        }
                        SubjectRefAst::SelfObject => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            )
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && last_instruction_created_tokens(&builder.instructions) =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::ControllersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                            ))
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && last_instruction_moved_object(&builder.instructions) =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::ControllersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            ))
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                        {
                            let mut clean = f.as_ref().clone();
                            clean.zone = None;
                            let obj = lower_object_filter_in_context(
                                &clean, builder, in_trigger, vocabulary,
                            )?;
                            bind_filter_player_target(builder, &obj)?;
                            spellwake_engine::toolbox::PlayerSelector::ControllersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Battlefield,
                                    obj,
                                ),
                            ))
                        }
                        _ => return Err("oracle:unsupported-damage-target"),
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Players(selector),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::OwnerOf(inner) => {
                    let amt_expr = lower_amt(builder)?;
                    let selector = match inner.as_ref() {
                        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetOwner,
                            )
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TriggerSubjectOwner,
                            )
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None && in_trigger =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TriggerSubjectOwner,
                            )
                        }
                        SubjectRefAst::SelfObject
                            if self_pronoun_valid || in_trigger || builder.has_source =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::SourceOwner,
                            )
                        }
                        SubjectRefAst::AttachedHost(_)
                            if self_pronoun_valid || in_trigger || builder.has_source =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                            ))
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && last_instruction_created_tokens(&builder.instructions) =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                            ))
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && last_instruction_moved_object(&builder.instructions) =>
                        {
                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            ))
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                        {
                            let mut clean = f.as_ref().clone();
                            clean.zone = None;
                            let obj = lower_object_filter_in_context(
                                &clean, builder, in_trigger, vocabulary,
                            )?;
                            bind_filter_player_target(builder, &obj)?;
                            spellwake_engine::toolbox::PlayerSelector::OwnersOf(Box::new(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Battlefield,
                                    obj,
                                ),
                            ))
                        }
                        _ => return Err("oracle:unsupported-damage-target"),
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Players(selector),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::PlayerMatching { player, condition } => {
                    let mut trial = builder.clone();
                    let selector = lower_player_matching_damage_selector(
                        *player, condition, &mut trial, in_trigger, vocabulary,
                    )
                    .ok_or("oracle:unsupported-damage-target")?;
                    let amt_expr = lower_amt(&trial)?;
                    trial.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Players(selector),
                            amt_expr,
                            false,
                        ),
                    );
                    *builder = trial;
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target != BodyTarget::None
                        && !is_stack_body_target(builder.target)
                        && !is_graveyard_body_target(builder.target) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                    builder.bind_target(BodyTarget::SINGLE_PLAYER)?;
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                    builder.bind_target(BodyTarget::SINGLE_OPPONENT)?;
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, DamageGroup::Opponents, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachPlayer) => {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, DamageGroup::Players, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::You) => {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, DamageGroup::Controller, amt_expr);
                    Ok(())
                }
                s if is_self_pronoun(s, builder, self_pronoun_valid)
                    || matches!(
                        s,
                        SubjectRefAst::Anaphora(a)
                            if (self_pronoun_valid || in_trigger || builder.has_source)
                                && matches!(
                                    a.trim().to_ascii_lowercase().as_str(),
                                    "itself" | "himself" | "herself"
                                )
                    ) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AbilitySource,
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    let amt_expr = lower_amt(builder)?;
                    let rp = lower_resolving_player_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    if rp == ResolvingPlayer::TargetPlayer
                        && matches!(builder.target, BodyTarget::Player { .. })
                    {
                        push_damage_target(&mut builder.instructions, amt_expr);
                    } else {
                        push_damage_group(
                            &mut builder.instructions,
                            DamageGroup::AffectedPlayers(AffectedPlayers::One(rp)),
                            amt_expr,
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    let amt_expr = lower_amt(builder)?;
                    let group = DamageGroup::Filtered(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ));
                    push_damage_group(&mut builder.instructions, group, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Each(f) | SubjectRefAst::All(f) | SubjectRefAst::Group(f) => {
                    if matches!(
                        f.controller,
                        Some(ControllerRelation::TargetOpponent | ControllerRelation::TargetPlayer)
                    ) && f.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                controller: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        let bt = if f.controller == Some(ControllerRelation::TargetOpponent) {
                            BodyTarget::SINGLE_OPPONENT
                        } else {
                            BodyTarget::SINGLE_PLAYER
                        };
                        builder.bind_target(bt)?;
                        let amt_expr = lower_amt(builder)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                                spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                            Box::new(
                                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                    ResolvingPlayer::TargetPlayer,
                                                ),
                                            ),
                                        ),
                                        ObjectFilter::creature(),
                                    ),
                                ),
                                amt_expr,
                                false,
                            ),
                        );
                        return Ok(());
                    }
                    let mut f_clean = f.as_ref().clone();
                    if f_clean.zone == Some(ZoneFilter::Battlefield) {
                        f_clean.zone = None;
                    }
                    let group = if f_clean.zone.is_none()
                        && (matches!(
                            f_clean.controller,
                            Some(
                                ControllerRelation::DefendingPlayer
                                    | ControllerRelation::ThatPlayer
                            )
                        ) || (!in_trigger && is_permanent_body_target(builder.target)))
                    {
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        DamageGroup::Filtered(obj)
                    } else {
                        lower_damage_group(&f_clean, vocabulary)?
                    };
                    if let DamageGroup::Filtered(obj) = group {
                        bind_filter_player_target(builder, &obj)?;
                    }
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, group, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Compound(items) if items.len() >= 2 => {
                    let is_all_creatures = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::CREATURE
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.subtypes.is_empty()
                                    && f.forbidden_subtypes.is_empty()
                                    && f.any_colors.is_empty()
                                    && f.forbidden_colors.is_empty()
                                    && f.required_keywords.is_empty()
                                    && f.forbidden_keywords.is_empty()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_all_pws = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::PLANESWALKER
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_each_player = |s: &SubjectRefAst| {
                        matches!(s, SubjectRefAst::Player(PlayerRefAst::EachPlayer))
                    };
                    let group = if items.len() == 2
                        && ((is_all_creatures(&items[0]) && is_each_player(&items[1]))
                            || (is_each_player(&items[0]) && is_all_creatures(&items[1])))
                    {
                        DamageGroup::CreaturesAndPlayers
                    } else if items.len() == 2
                        && ((is_all_creatures(&items[0]) && is_all_pws(&items[1]))
                            || (is_all_pws(&items[0]) && is_all_creatures(&items[1])))
                    {
                        DamageGroup::CreaturesAndPlaneswalkers
                    } else if items.iter().any(|it| {
                        matches!(
                            it,
                            SubjectRefAst::AnyTarget
                                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                | SubjectRefAst::TargetPlayer { .. }
                                | SubjectRefAst::TargetObject { .. }
                                | SubjectRefAst::TargetOneOf { .. }
                                | SubjectRefAst::Player(
                                    PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                                )
                        )
                    }) {
                        let is_dmg_target_item = |it: &SubjectRefAst| {
                            matches!(
                                it,
                                SubjectRefAst::AnyTarget
                                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                                    | SubjectRefAst::TargetPlayer { .. }
                                    | SubjectRefAst::TargetObject { .. }
                                    | SubjectRefAst::TargetOneOf { .. }
                                    | SubjectRefAst::Player(
                                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                                    )
                            )
                        };
                        let target_count = items.iter().filter(|it| is_dmg_target_item(it)).count();
                        if target_count > 1 {
                            if builder.target != BodyTarget::None {
                                return Err("oracle:unsupported-damage-target");
                            }
                            let mut trial = builder.clone();
                            for it in items {
                                let mut sub = BodyBuilder::with_trigger_hint(trial.trigger_hint);
                                sub.has_source =
                                    trial.has_source || in_trigger || self_pronoun_valid;
                                sub.last_group_filter = trial.last_group_filter;
                                lower_instruction(
                                    &InstructionAst::DealDamage {
                                        source: source.clone(),
                                        target: Box::new(it.clone()),
                                        amount: amount.clone(),
                                    },
                                    &mut sub,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    bound_x,
                                )?;
                                if sub.target != BodyTarget::None {
                                    let body = sub.finish()?;
                                    if body.permanent_target_bounds().is_some()
                                        || !body
                                            .instructions
                                            .iter()
                                            .any(Instruction::uses_mode_target)
                                    {
                                        return Err("oracle:unsupported-damage-target");
                                    }
                                    trial
                                        .instructions
                                        .push(Instruction::FixedTargetClause(Box::new(body)));
                                } else {
                                    splice_probed_body(&mut trial, sub)?;
                                }
                            }
                            *builder = trial;
                            return Ok(());
                        }
                        if target_count != 1 {
                            return Err("oracle:unsupported-damage-target");
                        }
                        let mut target_items = Vec::new();
                        let mut non_target_items = Vec::new();
                        for it in items {
                            if is_dmg_target_item(it) {
                                target_items.push(it);
                            } else {
                                non_target_items.push(it);
                            }
                        }
                        for it in target_items.into_iter().chain(non_target_items) {
                            let mut adj_it = it.clone();
                            if matches!(builder.target, BodyTarget::Player { .. }) {
                                let tgt_ctrl = if builder.target == BodyTarget::SINGLE_OPPONENT {
                                    ControllerRelation::TargetOpponent
                                } else {
                                    ControllerRelation::TargetPlayer
                                };
                                match &mut adj_it {
                                    SubjectRefAst::Each(f)
                                    | SubjectRefAst::All(f)
                                    | SubjectRefAst::Group(f)
                                        if f.controller == Some(ControllerRelation::ThatPlayer) =>
                                    {
                                        f.controller = Some(tgt_ctrl);
                                    }
                                    _ => {}
                                }
                            }
                            lower_instruction(
                                &InstructionAst::DealDamage {
                                    source: source.clone(),
                                    target: Box::new(adj_it),
                                    amount: amount.clone(),
                                },
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        return Ok(());
                    } else if items
                        .iter()
                        .any(|it| matches!(it, SubjectRefAst::PlayerMatching { .. }))
                    {
                        let mut trial = builder.clone();
                        let mut obj_queries = Vec::new();
                        let mut player_sel: Option<spellwake_engine::toolbox::PlayerSelector> =
                            None;
                        let mut all_ok = true;
                        for item in items {
                            match item {
                                SubjectRefAst::Each(f)
                                | SubjectRefAst::All(f)
                                | SubjectRefAst::Group(f)
                                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                                {
                                    let mut clean = f.as_ref().clone();
                                    clean.zone = None;
                                    if let Ok(obj) = lower_object_filter_in_context(
                                        &clean, &trial, in_trigger, vocabulary,
                                    ) && bind_filter_player_target(&mut trial, &obj).is_ok()
                                    {
                                        obj_queries.push(
                                            spellwake_engine::toolbox::ObjectQuery::zone(
                                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                                obj,
                                            ),
                                        );
                                    } else {
                                        all_ok = false;
                                        break;
                                    }
                                }
                                SubjectRefAst::Player(p) if player_sel.is_none() => {
                                    if let Ok(ap) = lower_affected_players_in_context(
                                        *p, &mut trial, in_trigger,
                                    ) {
                                        player_sel = Some(
                                            spellwake_engine::toolbox::PlayerSelector::Group(ap),
                                        );
                                    } else {
                                        all_ok = false;
                                        break;
                                    }
                                }
                                SubjectRefAst::PlayerMatching { player, condition }
                                    if player_sel.is_none() =>
                                {
                                    if let Some(sel) = lower_player_matching_damage_selector(
                                        *player, condition, &mut trial, in_trigger, vocabulary,
                                    ) {
                                        player_sel = Some(sel);
                                    } else {
                                        all_ok = false;
                                        break;
                                    }
                                }
                                _ => {
                                    all_ok = false;
                                    break;
                                }
                            }
                        }
                        if all_ok && let Some(players) = player_sel {
                            let target_q = if obj_queries.is_empty() {
                                spellwake_engine::toolbox::DamageTargetQuery::Players(players)
                            } else {
                                let objects = if obj_queries.len() == 1 {
                                    obj_queries.remove(0)
                                } else {
                                    spellwake_engine::toolbox::ObjectQuery::Union(obj_queries)
                                };
                                spellwake_engine::toolbox::DamageTargetQuery::Both {
                                    objects,
                                    players,
                                }
                            };
                            let amt_expr = lower_amt(&trial)?;
                            trial.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                                    target_q,
                                    amt_expr,
                                    false,
                                ),
                            );
                            *builder = trial;
                            return Ok(());
                        }
                        return Err("oracle:unsupported-damage-target");
                    } else {
                        let mut player_group: Option<AffectedPlayers> = None;
                        let mut simple_fallback_needed = false;
                        for item in items {
                            if let SubjectRefAst::Player(p) = item {
                                let ap =
                                    lower_affected_players_in_context(*p, builder, in_trigger)?;
                                if player_group.is_some() {
                                    simple_fallback_needed = true;
                                    break;
                                }
                                player_group = Some(ap);
                            }
                        }
                        if !simple_fallback_needed {
                            let mut perm_filters = Vec::new();
                            for item in items {
                                match item {
                                    SubjectRefAst::Each(f)
                                    | SubjectRefAst::All(f)
                                    | SubjectRefAst::Group(f)
                                        if matches!(
                                            f.zone,
                                            None | Some(ZoneFilter::Battlefield)
                                        ) =>
                                    {
                                        let mut adj = f.clone();
                                        adj.zone = None;
                                        if adj.controller == Some(ControllerRelation::ThatPlayer) {
                                            match player_group {
                                                Some(AffectedPlayers::EachOpponent) => {
                                                    adj.controller =
                                                        Some(ControllerRelation::Opponent);
                                                }
                                                Some(AffectedPlayers::EachPlayer) => {
                                                    adj.controller = None;
                                                }
                                                Some(AffectedPlayers::EachOtherPlayer) => {
                                                    adj.controller =
                                                        Some(ControllerRelation::NotYou);
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::Controller,
                                                )) => {
                                                    adj.controller = Some(ControllerRelation::You);
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::TargetPlayer,
                                                )) => {
                                                    adj.controller = Some(
                                                        if builder.target
                                                            == BodyTarget::SINGLE_OPPONENT
                                                        {
                                                            ControllerRelation::TargetOpponent
                                                        } else {
                                                            ControllerRelation::TargetPlayer
                                                        },
                                                    );
                                                }
                                                Some(AffectedPlayers::One(
                                                    ResolvingPlayer::DefendingPlayer,
                                                )) => {
                                                    adj.controller =
                                                        Some(ControllerRelation::DefendingPlayer);
                                                }
                                                _ => {}
                                            }
                                        }
                                        let obj = lower_object_filter_in_context(
                                            &adj, builder, in_trigger, vocabulary,
                                        )?;
                                        bind_filter_player_target(builder, &obj)?;
                                        perm_filters.push(obj);
                                    }
                                    SubjectRefAst::Player(_) => {}
                                    _ => {
                                        simple_fallback_needed = true;
                                        break;
                                    }
                                }
                            }
                            if !simple_fallback_needed {
                                if perm_filters.is_empty() {
                                    if let Some(players) = player_group {
                                        DamageGroup::AffectedPlayers(players)
                                    } else {
                                        return Err("oracle:unsupported-damage-target");
                                    }
                                } else {
                                    let combined_filter = if perm_filters.len() == 1 {
                                        perm_filters[0]
                                    } else {
                                        let mut clauses = Vec::new();
                                        for f in perm_filters {
                                            match f {
                                                ObjectFilter::Clause(c) => clauses.push(c),
                                                ObjectFilter::Either(a, b) => {
                                                    clauses.push(a);
                                                    clauses.push(b);
                                                }
                                            }
                                        }
                                        let mut merged: Vec<ObjectClause> =
                                            Vec::with_capacity(clauses.len());
                                        for c in clauses {
                                            if merged.contains(&c) {
                                                continue;
                                            }
                                            let mut folded = false;
                                            if !c.any_types.is_empty() {
                                                for existing in &mut merged {
                                                    if !existing.any_types.is_empty() {
                                                        let mut probe = *existing;
                                                        probe.any_types = c.any_types;
                                                        if probe == c {
                                                            existing.any_types = existing
                                                                .any_types
                                                                .union(c.any_types);
                                                            folded = true;
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            if !folded {
                                                merged.push(c);
                                            }
                                        }
                                        match merged.as_slice() {
                                            [only] => ObjectFilter::Clause(*only),
                                            [first, second] => {
                                                ObjectFilter::Either(*first, *second)
                                            }
                                            _ => {
                                                let amt_expr = lower_amt(builder)?;
                                                let queries: Vec<
                                                    spellwake_engine::toolbox::ObjectQuery,
                                                > = merged
                                                    .into_iter()
                                                    .map(|c| {
                                                        spellwake_engine::toolbox::ObjectQuery::zone(
                                                            spellwake_engine::toolbox::QueryZone::Battlefield,
                                                            ObjectFilter::Clause(c),
                                                        )
                                                    })
                                                    .collect();
                                                let obj_q =
                                                    spellwake_engine::toolbox::ObjectQuery::Union(
                                                        queries,
                                                    );
                                                let target_q = match player_group {
                                                    Some(players) => {
                                                        spellwake_engine::toolbox::DamageTargetQuery::Both {
                                                            objects: obj_q,
                                                            players: spellwake_engine::toolbox::PlayerSelector::Group(players),
                                                        }
                                                    }
                                                    None => {
                                                        spellwake_engine::toolbox::DamageTargetQuery::Objects(obj_q)
                                                    }
                                                };
                                                builder.instructions.push(
                                                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                                                        target_q,
                                                        amt_expr,
                                                        false,
                                                    ),
                                                );
                                                return Ok(());
                                            }
                                        }
                                    };
                                    match player_group {
                                        Some(players) => DamageGroup::FilteredAndPlayers {
                                            filter: combined_filter,
                                            players,
                                        },
                                        None => DamageGroup::Filtered(combined_filter),
                                    }
                                }
                            } else {
                                for it in items {
                                    lower_instruction(
                                        &InstructionAst::DealDamage {
                                            source: source.clone(),
                                            target: Box::new(it.clone()),
                                            amount: amount.clone(),
                                        },
                                        builder,
                                        vocabulary,
                                        ids,
                                        in_trigger,
                                        self_pronoun_valid,
                                        bound_x,
                                    )?;
                                }
                                return Ok(());
                            }
                        } else {
                            for it in items {
                                lower_instruction(
                                    &InstructionAst::DealDamage {
                                        source: source.clone(),
                                        target: Box::new(it.clone()),
                                        amount: amount.clone(),
                                    },
                                    builder,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    bound_x,
                                )?;
                            }
                            return Ok(());
                        }
                    };
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, group, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachOtherOpponent) => {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(&mut builder.instructions, DamageGroup::Opponents, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(
                    p @ (PlayerRefAst::AnyOpponent | PlayerRefAst::AnyPlayer),
                ) => {
                    let amt_expr = lower_amt(builder)?;
                    builder
                        .instructions
                        .push(Instruction::PromptChoice(Box::new(
                            spellwake_engine::toolbox::PromptChoiceSpec::choose_value(
                                ResolvingPlayer::Controller,
                                spellwake_engine::toolbox::PromptDomain::player(matches!(
                                    p,
                                    PlayerRefAst::AnyOpponent
                                )),
                            ),
                        )));
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AffectedPlayers(AffectedPlayers::One(
                            ResolvingPlayer::ChosenPlayer,
                        )),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::Player(p)
                    if matches!(
                        p,
                        PlayerRefAst::ActivePlayer
                            | PlayerRefAst::EachOtherPlayer
                            | PlayerRefAst::DefendingPlayer
                            | PlayerRefAst::EnchantedPlayer
                            | PlayerRefAst::ChosenPlayer
                            | PlayerRefAst::ItsController
                            | PlayerRefAst::ItsOwner
                            | PlayerRefAst::ThatObjectController
                            | PlayerRefAst::ThatPlayerOrPlaneswalkerController
                            | PlayerRefAst::TargetOpponentAtRandom
                    ) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::AffectedPlayers(players),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                            ),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if builder.target == BodyTarget::None
                        && in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                            ),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_created_tokens(&builder.instructions) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                            ),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                            ),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && builder.last_group_filter.is_some() =>
                {
                    let amt_expr = lower_amt(builder)?;
                    let obj = builder
                        .last_group_filter
                        .ok_or("oracle:unsupported-damage-target")?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::Filtered(obj),
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::Player(
                    PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController,
                ) if builder.target != BodyTarget::None => {
                    let amt_expr = lower_amt(builder)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                                DamageRecipientsRef::TargetController,
                            )),
                            amt_expr,
                            false,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Player(
                    PlayerRefAst::DefendingPlayer
                    | PlayerRefAst::EnchantedPlayer
                    | PlayerRefAst::ItsController
                    | PlayerRefAst::ThatObjectController,
                ) if builder.target == BodyTarget::None => {
                    let amt_expr = lower_amt(builder)?;
                    push_damage_group(
                        &mut builder.instructions,
                        DamageGroup::RelatedPlayer,
                        amt_expr,
                    );
                    Ok(())
                }
                SubjectRefAst::DistinctTarget { filter, .. }
                    if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !filter.is_ability
                        && !filter.any_spell =>
                {
                    let mut clean = filter.as_ref().clone();
                    clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let amt_expr = lower_amt(builder)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                _ => Err("oracle:unsupported-damage-target"),
            }
        }
        InstructionAst::Regenerate { subject } => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_source(
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_target(
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if filter.zone.is_none()
                        && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    let (minimum, maximum) = match *count {
                        TargetCountAst::Exact(n) => (n, n),
                        TargetCountAst::UpTo(1) => {
                            builder.up_to_one_target = true;
                            (0, 1)
                        }
                        TargetCountAst::UpTo(max) => (0, max),
                        TargetCountAst::AnyNumber => (0, u16::MAX),
                        TargetCountAst::ChosenX => {
                            builder.x_target_count = true;
                            (0, u16::MAX)
                        }
                        _ => return Err("oracle:unsupported-regenerate"),
                    };
                    builder.push_bounded_target_op(
                        minimum,
                        maximum,
                        super::super::toolbox_emit::regenerate_target(),
                    );
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if has_source => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(
                            EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                    let mut cleaned = f.as_ref().clone();
                    let mut other_than_target = false;
                    let mut kept_preds = Vec::with_capacity(cleaned.predicates.len());
                    for pred in cleaned.predicates {
                        match pred {
                            spellwake_oracle_compiler::ast::FilterPredicateAst::Exclude(
                                ref excl,
                            ) if is_permanent_body_target(builder.target)
                                && matches!(excl.as_ref(), SubjectRefAst::Anaphora(_)) =>
                            {
                                other_than_target = true;
                            }
                            spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(
                                ref s,
                            ) if is_permanent_body_target(builder.target)
                                && matches!(
                                    s.trim().to_ascii_lowercase().as_str(),
                                    "other than that creature"
                                        | "other than that permanent"
                                        | "other than that card"
                                ) =>
                            {
                                other_than_target = true;
                            }
                            other => kept_preds.push(other),
                        }
                    }
                    cleaned.predicates = kept_preds;
                    if !has_source
                        && is_permanent_body_target(builder.target)
                        && (cleaned.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0
                    {
                        cleaned.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
                        other_than_target = true;
                    }
                    let obj =
                        lower_object_filter_in_context(&cleaned, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    let subj = if other_than_target {
                        EffectSubject::MatchingOtherThanTarget(obj)
                    } else {
                        EffectSubject::Matching(obj)
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(
                            subj,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(
                            EffectSubject::TriggerSubject,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw) => {
                    if builder.permanent_target_bounds.is_some()
                        || is_permanent_body_target(builder.target)
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_target());
                        Ok(())
                    } else if last_instruction_created_tokens(&builder.instructions) {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(EffectSubject::CreatedTokens));
                        Ok(())
                    } else if last_instruction_moved_object(&builder.instructions) {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(EffectSubject::LastMoved));
                        Ok(())
                    } else if let Some(group) = builder.last_group_filter {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(EffectSubject::Matching(group)));
                        Ok(())
                    } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_subject(EffectSubject::TriggerSubject));
                        Ok(())
                    } else {
                        Err("oracle:unsupported-regenerate")
                    }
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    let mut trial = builder.clone();
                    for item in items {
                        lower_instruction(
                            &InstructionAst::Regenerate {
                                subject: Box::new(item.clone()),
                            },
                            &mut trial,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    *builder = trial;
                    Ok(())
                }
                _ => Err("oracle:unsupported-regenerate"),
            }
        }
        InstructionAst::Fight { a, b } => {
            let lower_exact_creature_bt =
                |subj: &SubjectRefAst, bld: &BodyBuilder| -> Option<(BodyTarget, bool)> {
                    let eff = match subj {
                        SubjectRefAst::InZone {
                            subject: inner,
                            zone: ZoneFilter::Battlefield,
                        } => inner.as_ref(),
                        other => other,
                    };
                    match eff {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, bld, in_trigger, vocabulary,
                            )
                            .ok()?;
                            bt.guarantees_creature().then_some((
                                bt,
                                f_clean.controller == Some(ControllerRelation::ThatPlayer),
                            ))
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: TargetCountAst::Exact(1),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, bld, in_trigger, vocabulary,
                            )
                            .ok()?;
                            let has_that_player = options.iter().any(|o| {
                                matches!(
                                    o,
                                    SubjectRefAst::TargetObject { filter, .. }
                                        if filter.controller == Some(ControllerRelation::ThatPlayer)
                                )
                            });
                            bt.guarantees_creature().then_some((bt, has_that_player))
                        }
                        _ => None,
                    }
                };
            if builder.target == BodyTarget::None
                && builder.instructions.is_empty()
                && let Some((bt_a, _)) = lower_exact_creature_bt(a, builder)
                && let Some((mut bt_b, b_has_that_player)) = lower_exact_creature_bt(b, builder)
            {
                if !in_trigger && b_has_that_player {
                    rewrite_bt_related_to_first_target_controller(&mut bt_b);
                }
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(
                            bt_a,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(
                            bt_b,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                builder.instructions.push(Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::fight(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::RememberedFirst,
                        ),
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::RememberedSecond,
                        ),
                    ),
                )));
                return Ok(());
            }
            let second_fight_target = match (a.as_ref(), b.as_ref()) {
                (SubjectRefAst::Anaphora(_), rhs) => lower_exact_creature_bt(rhs, builder),
                (lhs, SubjectRefAst::Anaphora(_)) => lower_exact_creature_bt(lhs, builder),
                _ => None,
            };
            if let Some((mut bt_b, b_has_that_player)) = second_fight_target
                && builder.target.guarantees_creature()
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && builder.permanent_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.up_to_x_target_count
                && !builder.up_to_one_target
                && !builder.instructions.iter().any(|op| {
                    matches!(op, Instruction::FixedTargetClause(_))
                        || matches!(
                            op,
                            Instruction::MutateObject(spec)
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::ObjectMutation::StateMarker(spellwake_engine::toolbox::ObjectStateMarkerOp::RememberPermanentTarget)
                                )
                        )
                })
            {
                if !in_trigger && b_has_that_player {
                    rewrite_bt_related_to_first_target_controller(&mut bt_b);
                }
                let mut first_ops = std::mem::take(&mut builder.instructions);
                prepend_instruction_at(
                    &mut first_ops,
                    0,
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                );
                let first_bt = std::mem::replace(&mut builder.target, BodyTarget::None);
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(first_bt, first_ops),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(
                            bt_b,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                builder.instructions.push(Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::fight(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::RememberedFirst,
                        ),
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            MoveSubjectRef::RememberedSecond,
                        ),
                    ),
                )));
                return Ok(());
            }
            let had_perm_target = matches!(
                builder.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) && builder.target != BodyTarget::SINGLE_PLANESWALKER;
            let first = lower_fight_or_exchange_subject(
                a,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                false,
                had_perm_target,
            )?;
            let second = lower_fight_or_exchange_subject(
                b,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                false,
                had_perm_target,
            )?;
            builder.instructions.push(Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec::fight(
                    spellwake_engine::toolbox::ObjectQuery::from_move_subject(first),
                    spellwake_engine::toolbox::ObjectQuery::from_move_subject(second),
                ),
            )));
            Ok(())
        }
        InstructionAst::PreventAllDamageFrom {
            combat_only,
            source,
            recipient,
            duration: DurationAst::UntilEndOfTurn,
        } => interactive::lower_prevent_all_damage_from(
            *combat_only,
            source,
            recipient.as_deref(),
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                    spellwake_engine::damage::TurnPreventionSpec {
                        combat_only: true,
                        source: spellwake_engine::damage::PreventionSourceSpec::Any,
                        recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                        both_directions: false,
                    },
                ),
            );
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                    spellwake_engine::damage::TurnPreventionSpec {
                        combat_only: false,
                        source: spellwake_engine::damage::PreventionSourceSpec::Any,
                        recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                        both_directions: false,
                    },
                ),
            );
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                    spellwake_engine::damage::TurnPreventionSpec {
                        combat_only: true,
                        source: spellwake_engine::damage::PreventionSourceSpec::AbilitySource,
                        recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                        both_directions: true,
                    },
                ),
            );
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                    spellwake_engine::damage::TurnPreventionSpec {
                        combat_only: false,
                        source: spellwake_engine::damage::PreventionSourceSpec::Any,
                        recipient: spellwake_engine::damage::PreventionRecipientSpec::AbilitySource,
                        both_directions: false,
                    },
                ),
            );
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } => interactive::lower_prevent_all_damage(
            *combat_only,
            subj,
            builder,
            vocabulary,
            self_pronoun_valid,
        ),
        InstructionAst::PreventDamage {
            amount,
            target,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let amt_expr = lower_value_expr_in_context(
                resolve_x(amount, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let is_target_subj = matches!(
                target.as_ref(),
                SubjectRefAst::AnyTarget
                    | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                    | SubjectRefAst::TargetPlayer { .. }
                    | SubjectRefAst::TargetObject { .. }
                    | SubjectRefAst::Player(
                        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
                    )
            );
            if is_target_subj {
                let bt = match target.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && !filter.is_ability && !filter.any_spell => {
                        lower_battlefield_body_target_in_context(
                            filter, builder, in_trigger, vocabulary,
                        )?
                    }
                    SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => BodyTarget::SINGLE_PLAYER,
                    SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                        BodyTarget::SINGLE_OPPONENT
                    }
                    _ => lower_target_subject(target, vocabulary)?,
                };
                builder.bind_target(bt)?;
                if dur == ResolvedDuration::UntilEndOfTurn
                    && let ValueExpr::Literal(n) = &amt_expr
                    && n.is_positive()
                {
                    builder
                        .instructions
                        .push(Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec {
                                selector: None,
                                payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield {
                                    amount: n.clone(),
                                },
                                duration: ResolvedDuration::UntilEndOfTurn,
                                condition: None,
                                one_shot: true,
                                as_emblem: None,
                                emblem_label: None,
                            },
                        )));
                    return Ok(());
                }
                let recipient = if is_permanent_body_target(bt) {
                    StaticDamageRecipient::TargetObject
                } else if matches!(
                    bt,
                    BodyTarget::Player { .. }
                        | BodyTarget::AnyTarget(
                            AnyTargetKind::Any
                                | AnyTargetKind::CreatureOrPlayer
                                | AnyTargetKind::PlayerOrPlaneswalker
                                | AnyTargetKind::OpponentOrPlaneswalker
                        )
                ) {
                    StaticDamageRecipient::TargetAny
                } else {
                    return Err("oracle:unsupported-prevent-damage");
                };
                let rule = TemporaryRuleKind::PreventDamageShield {
                    amount: Some(Box::new(amt_expr)),
                    source: None,
                    source_chosen_color: false,
                    source_chosen_type: false,
                    source_is_target: false,
                    half_round_down: false,
                    recipient,
                    combat_only: false,
                    one_shot: false,
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        rule, dur,
                    ),
                );
                return Ok(());
            }
            let has_source = in_trigger || self_pronoun_valid;
            let recipient = if has_source && is_self_pronoun(target, builder, self_pronoun_valid) {
                StaticDamageRecipient::SelfObject
            } else if matches!(target.as_ref(), SubjectRefAst::Anaphora(_)) {
                if is_permanent_body_target(builder.target) {
                    StaticDamageRecipient::TargetObject
                } else if matches!(
                    builder.target,
                    BodyTarget::Player { .. }
                        | BodyTarget::AnyTarget(
                            AnyTargetKind::Any
                                | AnyTargetKind::CreatureOrPlayer
                                | AnyTargetKind::PlayerOrPlaneswalker
                                | AnyTargetKind::OpponentOrPlaneswalker
                        )
                ) {
                    StaticDamageRecipient::TargetAny
                } else {
                    return Err("oracle:unsupported-prevent-damage");
                }
            } else {
                lower_redirect_from_subject(target, has_source, vocabulary)
                    .ok_or("oracle:unsupported-prevent-damage")?
            };
            let rule = TemporaryRuleKind::PreventDamageShield {
                amount: Some(Box::new(amt_expr)),
                source: None,
                source_chosen_color: false,
                source_chosen_type: false,
                source_is_target: false,
                half_round_down: false,
                recipient,
                combat_only: false,
                one_shot: false,
            };
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    rule, dur,
                ),
            );
            Ok(())
        }
        InstructionAst::PreventNextSourceDamage {
            source_desc,
            target,
        } => {
            let has_source = in_trigger || self_pronoun_valid;
            let src_low = source_desc.trim().to_ascii_lowercase();
            let mut trial = builder.clone();
            let (source, source_chosen_color, source_chosen_type, source_is_target) =
                parse_next_time_source_with_target(
                    &src_low, &mut trial, has_source, in_trigger, vocabulary,
                )
                .ok_or("oracle:unsupported-prevent-damage")??;
            let recipient = if has_source && is_self_pronoun(target, &trial, self_pronoun_valid) {
                StaticDamageRecipient::SelfObject
            } else {
                lower_prevent_recipient_subject(
                    target, &mut trial, has_source, in_trigger, vocabulary,
                )?
            };
            if source_is_target
                && matches!(
                    recipient,
                    StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
                )
            {
                return Err("oracle:unsupported-prevent-damage");
            }
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    TemporaryRuleKind::PreventDamageShield {
                        amount: None,
                        source,
                        source_chosen_color,
                        source_chosen_type,
                        source_is_target,
                        half_round_down: false,
                        recipient,
                        combat_only: false,
                        one_shot: true,
                    },
                    ResolvedDuration::UntilEndOfTurn,
                ),
            );
            *builder = trial;
            Ok(())
        }
        InstructionAst::AdditionalCombat(spec) => {
            let low = spec.to_ascii_lowercase();
            if builder.target != BodyTarget::None {
                return Err("oracle:unsupported-additional-combat");
            }
            if low.contains("additional combat phase")
                && low.contains("untap")
                && low.contains("creatures you control")
            {
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(
                            CombatUntapGroup::ControlledCreatures,
                        ),
                    )));
                Ok(())
            } else if in_trigger
                && low.contains("additional combat phase")
                && low.contains("untap")
                && (low.contains("attacking creature") || low.contains("creatures that attacked"))
                && !low.contains("at the beginning of")
                && !low.contains("beginning of that combat")
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                )),
                            ),
                            ObjectFilter::creature().attacking(),
                        ),
                        spellwake_engine::toolbox::ObjectMutation::TapState(
                            spellwake_engine::toolbox::TapStateOp::Set(false),
                        ),
                    ),
                );
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(
                            CombatUntapGroup::None,
                        ),
                    )));
                Ok(())
            } else if low.contains("additional combat phase")
                && !low.contains("untap")
                && !low.contains("at the beginning of")
                && !low.contains("beginning of that combat")
                && !low.contains("beginning of the next")
                && !low.contains("only once")
                && !low.contains("if it's the first")
                && !low.contains("unless")
                && !low.contains("sacrifice")
                && !low.contains("exile")
                && !low.contains("create")
                && !low.contains("until end of turn")
            {
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(
                            CombatUntapGroup::None,
                        ),
                    )));
                Ok(())
            } else {
                Err("oracle:unsupported-additional-combat")
            }
        }
        InstructionAst::CombatRuleMod {
            subject,
            rule,
            duration,
        } => {
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for it in items {
                    lower_instruction(
                        &InstructionAst::CombatRuleMod {
                            subject: Box::new(it.clone()),
                            rule: rule.clone(),
                            duration: duration.clone(),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                return Ok(());
            }
            let dur = lower_duration(duration)?;
            let low = rule.to_ascii_lowercase();
            let eff_subject = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            if low == "doublepower"
                || low == "double power"
                || low == "double_power"
                || (low.starts_with("doublepower ") && low.ends_with(" times"))
            {
                let repeat_expr = if let Some(mid) = low
                    .strip_prefix("doublepower ")
                    .and_then(|r| r.strip_suffix(" times"))
                {
                    let m = mid.trim();
                    if m == "x" {
                        Some(lower_value_expr_in_context(
                            resolve_x(&ValueExprAst::ChosenX, bound_x),
                            builder,
                            in_trigger,
                            vocabulary,
                        )?)
                    } else if let Some(n) = parse_small_number_word(m)
                        && n > 0
                    {
                        Some(ValueExpr::literal(i64::from(n)))
                    } else {
                        return Err("oracle:unsupported-combat-mod");
                    }
                } else {
                    None
                };
                if dur.requires_ability_source() && !(in_trigger || self_pronoun_valid) {
                    return Err("oracle:unsupported-combat-mod");
                }
                let dp_subj = match eff_subject {
                    s if (self_pronoun_valid || in_trigger)
                        && is_self_pronoun(s, builder, self_pronoun_valid) =>
                    {
                        EffectSubject::AbilitySource
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            builder.up_to_one_target = true;
                        }
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        EffectSubject::CreatedTokens
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions)
                            || matches!(
                                builder.instructions.last(),
                                Some(Instruction::MutateObject(_))
                            ) =>
                    {
                        EffectSubject::LastMoved
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && builder.target == BodyTarget::None
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        EffectSubject::TriggerSubject
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        EffectSubject::Matching(group_obj)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        EffectSubject::Matching(obj)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                let op = crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                    dp_subj, true, false, dur,
                );
                if let Some(count) = repeat_expr {
                    builder.instructions.push(Instruction::RepeatLoop {
                        count,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![op])),
                    });
                } else {
                    builder.instructions.push(op);
                }
                return Ok(());
            }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && matches!(
                    low.trim().trim_end_matches('.'),
                    "its activated abilities can't be activated"
                        | "its activated abilities can't be activated this turn"
                )
                && is_permanent_body_target(builder.target)
            {
                let lock_parts =
                    parse_temp_permanent_lock_parts("its activated abilities can't be activated")
                        .ok_or("oracle:unsupported-combat-mod")?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(lock_parts),
                        dur,
                    ),
                );
                return Ok(());
            }
            let is_turn_face_down = matches!(
                low.trim().trim_end_matches('.'),
                "turn_face_down" | "turn face down" | "turns face down"
            );
            if is_turn_face_down
                || matches!(
                    low.trim().trim_end_matches('.'),
                    "turn_face_up" | "turn face up" | "turns face up"
                )
            {
                let face_up_subj = match eff_subject {
                    s if (self_pronoun_valid || in_trigger)
                        && is_self_pronoun(s, builder, self_pronoun_valid) =>
                    {
                        EffectSubject::AbilitySource
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            builder.up_to_one_target = true;
                        }
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        EffectSubject::CreatedTokens
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions)
                            || matches!(
                                builder.instructions.last(),
                                Some(Instruction::MutateObject(_))
                            ) =>
                    {
                        EffectSubject::LastMoved
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && builder.target == BodyTarget::None
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        EffectSubject::TriggerSubject
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        EffectSubject::Matching(group_obj)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        EffectSubject::Matching(obj)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                if is_turn_face_down {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::turn_face_down_subject(face_up_subj));
                } else {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::turn_face_up_subject(face_up_subj));
                }
                return Ok(());
            }
            if let Some((lhs_raw, rhs_raw)) =
                low.split_once(" and creatures you control can't be the targets of ")
                && let Some(counter_raw) =
                    lhs_raw.strip_prefix("spells you control can't be countered by ")
            {
                let counter_clean = counter_raw
                    .trim()
                    .trim_end_matches(',')
                    .trim_end_matches(" until end of turn")
                    .trim_end_matches(" this turn")
                    .trim();
                let target_clean = rhs_raw
                    .trim()
                    .trim_end_matches('.')
                    .trim_end_matches(" until end of turn")
                    .trim_end_matches(" this turn")
                    .trim();
                if let Some((counter_colors, true)) = parse_cant_be_target_colors(counter_clean)
                    && let Some((target_colors, target_spells_only)) =
                        parse_cant_be_target_colors(target_clean)
                {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::CantBeCountered {
                            colors: counter_colors,
                            filter: None,
                            controller_only: true,
                            next_only: false,
                        }, dur));
                    let target_part = if target_spells_only {
                        ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(target_colors)
                    } else {
                        ResolvedPartSpec::CannotBeTargetedByColors(target_colors)
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::Matching(ObjectFilter::creature().controlled()),
                            )),
                            ResolvedPartsSpec(vec![target_part]),
                            dur,
                        ),
                    );
                    return Ok(());
                }
            }
            if let Some((is_your_spells, by_raw)) = low
                .strip_prefix("spells you control can't be countered by ")
                .map(|r| (true, r))
                .or_else(|| {
                    low.strip_prefix("can't be countered by ")
                        .map(|r| (false, r))
                })
            {
                let (by_body, eff_dur) = strip_trailing_temp_duration(by_raw, dur)
                    .unwrap_or((by_raw.trim().trim_end_matches('.').to_owned(), dur));
                if let Some((colors, true)) = parse_cant_be_target_colors(&by_body) {
                    let (filter, controller_only) = if is_your_spells {
                        (None, true)
                    } else {
                        match eff_subject {
                            SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::Group(f)
                                if f.zone.is_none() =>
                            {
                                let mut inner = f.as_ref().clone();
                                let ctrl_only = match inner.controller {
                                    Some(ControllerRelation::You) => {
                                        inner.controller = None;
                                        true
                                    }
                                    None => false,
                                    _ => return Err("oracle:unsupported-combat-mod"),
                                };
                                inner.any_spell = false;
                                let flt = if is_unrestricted_filter(&inner) {
                                    None
                                } else {
                                    Some(lower_object_filter(&inner, vocabulary)?)
                                };
                                (flt, ctrl_only)
                            }
                            _ => return Err("oracle:unsupported-combat-mod"),
                        }
                    };
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::CantBeCountered {
                            colors,
                            filter,
                            controller_only,
                            next_only: false,
                        }, eff_dur));
                    return Ok(());
                }
            }
            if low == "it becomes day" || low == "becomes_day" {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_day_night(
                        DayNight::Day,
                    ),
                );
                return Ok(());
            }
            if low == "it becomes night" || low == "becomes_night" {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_day_night(
                        DayNight::Night,
                    ),
                );
                return Ok(());
            }
            if let Some(same_tail) = low.strip_prefix("same_is_true_for:") {
                let parsed_kws = parse_same_is_true_for_keywords(same_tail)?;
                let len = builder.instructions.len();
                if len < 2 {
                    return Err("oracle:unsupported-combat-mod");
                }
                let prev_branch = builder.instructions[len - 2].clone();
                let prev_grant = builder.instructions[len - 1].clone();
                let Instruction::BranchCondition {
                    condition,
                    destination,
                } = prev_branch
                else {
                    return Err("oracle:unsupported-combat-mod");
                };
                let spellwake_engine::toolbox::ConditionExpr::Resolution(rc) = *condition else {
                    return Err("oracle:unsupported-combat-mod");
                };
                let ResolutionCondition {
                    left:
                        ValueExpr::Count {
                            zone: prev_zone,
                            filter: prev_filter,
                            controller_scope,
                        },
                    comparison: Comparison::GreaterOrEqual,
                    right,
                } = *rc
                else {
                    return Err("oracle:unsupported-combat-mod");
                };
                if destination != len || right != ValueExpr::literal(1) {
                    return Err("oracle:unsupported-combat-mod");
                }
                let ObjectFilter::Clause(prev_clause) = *prev_filter else {
                    return Err("oracle:unsupported-combat-mod");
                };
                let Some(prev_kw) = prev_clause.keyword else {
                    return Err("oracle:unsupported-combat-mod");
                };
                enum SameGrantShape {
                    DynamicBoost(spellwake_engine::toolbox::ObjectQuery, ResolvedDuration),
                    ResolvedParts(spellwake_engine::toolbox::ObjectQuery, ResolvedDuration),
                    AddCounters(spellwake_engine::toolbox::ObjectQuery, ValueExpr),
                }
                let kw_to_counter = |kw: KeywordAbility| -> Option<CounterKind> {
                    match kw {
                        KeywordAbility::Flying => Some(CounterKind::Flying),
                        KeywordAbility::FirstStrike => Some(CounterKind::FirstStrike),
                        KeywordAbility::DoubleStrike => Some(CounterKind::DoubleStrike),
                        KeywordAbility::Deathtouch => Some(CounterKind::Deathtouch),
                        KeywordAbility::Haste => Some(CounterKind::Haste),
                        KeywordAbility::Hexproof => Some(CounterKind::Hexproof),
                        KeywordAbility::Indestructible => Some(CounterKind::Indestructible),
                        KeywordAbility::Lifelink => Some(CounterKind::Lifelink),
                        KeywordAbility::Menace => Some(CounterKind::Menace),
                        KeywordAbility::Reach => Some(CounterKind::Reach),
                        KeywordAbility::Shadow => Some(CounterKind::Shadow),
                        KeywordAbility::Trample => Some(CounterKind::Trample),
                        KeywordAbility::Vigilance => Some(CounterKind::Vigilance),
                        _ => None,
                    }
                };
                let prev_kw_counter = kw_to_counter(prev_kw);
                let is_single_kw_ab = |parts: &ResolvedPartsSpec| -> bool {
                    matches!(
                        parts.0.as_slice(),
                        [ResolvedPartSpec::Abilities(ab)]
                            if ab.removal.is_none()
                                && ab.suppressed_keywords.is_empty()
                                && ab.granted_exalted == 0
                                && !ab.grants_unblockable
                                && ab.granted_triggers.is_empty()
                                && ab.granted_toxic.is_none()
                                && ab.granted_combat_keywords.is_empty()
                                && ab.granted_keywords.len() == 1
                                && ab.granted_keywords.contains(&prev_kw)
                    )
                };
                let shape = match prev_grant {
                    Instruction::ApplyContinuous(spec) => match (spec.selector, spec.payload) {
                        (
                            Some(sel),
                            spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                                power,
                                toughness,
                                keywords,
                                suppress_keywords,
                            },
                        ) if power == ValueExpr::literal(0)
                            && toughness == ValueExpr::literal(0)
                            && suppress_keywords.is_empty()
                            && keywords.len() == 1
                            && keywords.contains(&prev_kw) =>
                        {
                            SameGrantShape::DynamicBoost(sel, spec.duration)
                        }
                        (
                            Some(sel),
                            spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts),
                        ) if is_single_kw_ab(&parts) => {
                            SameGrantShape::ResolvedParts(sel, spec.duration)
                        }
                        _ => return Err("oracle:unsupported-combat-mod"),
                    },
                    Instruction::MutateObject(spec) => match spec.op {
                        spellwake_engine::toolbox::ObjectMutation::Counters(
                            spellwake_engine::toolbox::CounterOp::Add { kind, amount },
                        ) if prev_kw_counter == Some(kind) => {
                            SameGrantShape::AddCounters(spec.source, amount)
                        }
                        _ => return Err("oracle:unsupported-combat-mod"),
                    },
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                let is_counter_shape = matches!(shape, SameGrantShape::AddCounters(..));
                if is_counter_shape && parsed_kws.iter().any(|kw| kw_to_counter(*kw).is_none()) {
                    return Err("oracle:unsupported-combat-mod");
                }
                for kw in parsed_kws {
                    let mut clause = prev_clause;
                    clause.keyword = Some(kw);
                    let cond = ResolutionCondition {
                        left: ValueExpr::Count {
                            zone: prev_zone,
                            filter: Box::new(ObjectFilter::Clause(clause)),
                            controller_scope,
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    };
                    let next_dest = builder.instructions.len() + 2;
                    builder.instructions.push(Instruction::BranchCondition {
                        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                            Box::new(cond),
                        )),
                        destination: next_dest,
                    });
                    let grant_op = match &shape {
                        SameGrantShape::DynamicBoost(sel, d) => {
                            Instruction::ApplyContinuous(Box::new(
                                spellwake_engine::toolbox::ApplyContinuousSpec::dynamic_boost(
                                    sel.clone(),
                                    ValueExpr::literal(0),
                                    ValueExpr::literal(0),
                                    BTreeSet::from([kw]),
                                    BTreeSet::new(),
                                    *d,
                                ),
                            ))
                        }
                        SameGrantShape::ResolvedParts(sel, d) => {
                            let mut ab = AbilityChange::default();
                            ab.granted_keywords.insert(kw);
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                sel.clone(),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                                *d,
                            )
                        }
                        SameGrantShape::AddCounters(src, amt) => {
                            let ck = kw_to_counter(kw).ok_or("oracle:unsupported-combat-mod")?;
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                                src.clone(),
                                ck,
                                amt.clone(),
                            )
                        }
                    };
                    builder.instructions.push(grant_op);
                }
                return Ok(());
            }
            if low.contains("you choose which creatures block")
                || low == "you_choose_how_they_block"
                || low.contains("you choose how those creatures block")
            {
                builder
                    .instructions
                    .push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::block_declarations(
                            spellwake_engine::program::BlockDeclarationRule::ControllerChooses,
                        ),
                    )));
                return Ok(());
            }
            if low.contains("each opponent can't block with more than one creature") {
                builder
                    .instructions
                    .push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::block_declarations(
                            spellwake_engine::program::BlockDeclarationRule::EachOpponentOneBlocker,
                        ),
                    )));
                return Ok(());
            }
            if low
                == "creatures you control with toughness greater than their power assign combat damage equal to their toughness rather than their power"
            {
                let mut clause = ObjectClause::creature().controlled();
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::TOUGHNESS_GREATER_THAN_POWER);
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(clause)),
                        )),
                        ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                            BlockingRestriction::AssignDamageByToughness,
                        )]),
                        dur,
                    ),
                );
                return Ok(());
            }
            if low == "no_max_hand_size"
                && matches!(subject.as_ref(), SubjectRefAst::Player(PlayerRefAst::You))
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::DrawRule {
                            id: ids.static_id(),
                            rule: spellwake_engine::characteristics::StaticDrawRule::NoMaximumHandSize,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            if low == "next_spell_cant_be_countered"
                || matches!(low.as_str(), "can't be countered" | "cant_be_countered")
            {
                let flt = match subject.as_ref() {
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        Some(lower_object_filter(f, vocabulary)?)
                    }
                    _ => None,
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::CantBeCountered {
                            colors: ColorMask::EMPTY,
                            filter: flt,
                            controller_only: true,
                            next_only: low == "next_spell_cant_be_countered",
                        },
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "cant_play_lands" | "can't play lands" | "cantplaylands"
            ) {
                let players = match subject.as_ref() {
                    SubjectRefAst::Player(p) => {
                        lower_affected_players_in_context(*p, builder, in_trigger)?
                    }
                    SubjectRefAst::TargetPlayer {
                        opponent_only,
                        count: TargetCountAst::Exact(1),
                    } => {
                        builder.bind_target(if *opponent_only {
                            BodyTarget::SINGLE_OPPONENT
                        } else {
                            BodyTarget::SINGLE_PLAYER
                        })?;
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::CantPlayLands { players },
                        dur,
                    ),
                );
                return Ok(());
            }
            if low
                == "you can't lose life this turn, you can't lose the game this turn, and your opponents can't win the game this turn"
            {
                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::temporary_static(
                        StaticContinuousAbility::EventReplacement {
                            id: ids.static_id(),
                            event: spellwake_engine::characteristics::ReplaceableEvent::LifeLoss {
                                player: AffectedPlayers::One(ResolvingPlayer::Controller),
                            },
                            condition: None,
                            action:
                                spellwake_engine::characteristics::StaticReplacementAction::Skip,
                        },
                    ), dur));
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "you can't lose the game and your opponents can't win the game"
                    | "you can't lose the game this turn and your opponents can't win the game this turn"
            ) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            let low_no_dot = low.trim().trim_end_matches('.').trim();
            if (matches!(
                low_no_dot,
                "you can't lose the game"
                    | "you can't lose the game this turn"
                    | "you can't lose the game for the rest of the game"
            ) && matches!(
                subject.as_ref(),
                SubjectRefAst::Player(PlayerRefAst::You) | SubjectRefAst::SelfObject
            )) || (matches!(
                low_no_dot,
                "cant_lose_game"
                    | "can't lose the game"
                    | "can't lose the game this turn"
                    | "can't lose the game for the rest of the game"
            ) && matches!(subject.as_ref(), SubjectRefAst::Player(PlayerRefAst::You)))
            {
                let eff_dur = if dur == ResolvedDuration::NoStatedDuration {
                    if low_no_dot.ends_with(" this turn") {
                        ResolvedDuration::UntilEndOfTurn
                    } else if low_no_dot.ends_with(" for the rest of the game") {
                        ResolvedDuration::NoStatedDuration
                    } else {
                        dur
                    }
                } else {
                    dur
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantLoseGame,
                        }),
                        eff_dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "players can't lose life" | "players can't lose life this turn"
            ) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(
                            StaticContinuousAbility::EventReplacement {
                                id: ids.static_id(),
                                event: ReplaceableEvent::LifeLoss {
                                    player: AffectedPlayers::EachPlayer,
                                },
                                condition: None,
                                action: StaticReplacementAction::Skip,
                            },
                        ),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "players can't lose the game or win the game"
                    | "players can't win the game or lose the game"
                    | "players can't lose the game or win the game this turn"
                    | "players can't win the game or lose the game this turn"
            ) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                        }),
                        dur,
                    ),
                );
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            if low
                == "players can't lose life this turn and players can't lose the game or win the game this turn"
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(
                            StaticContinuousAbility::EventReplacement {
                                id: ids.static_id(),
                                event: ReplaceableEvent::LifeLoss {
                                    player: AffectedPlayers::EachPlayer,
                                },
                                condition: None,
                                action: StaticReplacementAction::Skip,
                            },
                        ),
                        dur,
                    ),
                );
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                        }),
                        dur,
                    ),
                );
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "your life total can't change"
                    | "life total can't change"
                    | "life_total_cant_change"
            ) && matches!(
                subject.as_ref(),
                SubjectRefAst::Player(PlayerRefAst::You) | SubjectRefAst::SelfObject
            ) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerLifeTotalCantChange,
                        }),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(
                low.as_str(),
                "players can't search libraries"
                    | "your opponents can't search libraries"
                    | "opponents can't search libraries"
                    | "can't search libraries"
                    | "cant_search_libraries"
            ) {
                let rel = if low.contains("opponent") {
                    Some(PlayerRelation::Opponent)
                } else if low.starts_with("players ") {
                    Some(PlayerRelation::AnyParticipant)
                } else {
                    match subject.as_ref() {
                        SubjectRefAst::Player(PlayerRefAst::You) => {
                            Some(PlayerRelation::Controller)
                        }
                        SubjectRefAst::Player(
                            PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                        ) => Some(PlayerRelation::Opponent),
                        SubjectRefAst::Player(
                            PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer,
                        ) => Some(PlayerRelation::AnyParticipant),
                        _ => None,
                    }
                };
                if let Some(rel) = rel {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::temporary_static(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::PlayersCantSearchLibraries(rel),
                            },
                        ), dur));
                    return Ok(());
                }
            }
            if let Some((rule_body, eff_dur)) = strip_trailing_temp_duration(&low, dur) {
                if let Some(nonmana_only) = parse_player_cant_activate_rule(&rule_body)
                    && matches!(
                        subject.as_ref(),
                        SubjectRefAst::Player(_)
                            | SubjectRefAst::TargetPlayer {
                                count: TargetCountAst::Exact(1),
                                ..
                            }
                    )
                {
                    let players = match subject.as_ref() {
                        SubjectRefAst::Player(p) => {
                            lower_affected_players_in_context(*p, builder, in_trigger)?
                        }
                        SubjectRefAst::TargetPlayer {
                            opponent_only,
                            count: TargetCountAst::Exact(1),
                        } => {
                            builder.bind_target(if *opponent_only {
                                BodyTarget::SINGLE_OPPONENT
                            } else {
                                BodyTarget::SINGLE_PLAYER
                            })?;
                            AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                        }
                        _ => return Err("oracle:unsupported-combat-mod"),
                    };
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::CantActivateAbilities {
                            players,
                            nonmana_only,
                        }, eff_dur));
                    return Ok(());
                }
                if let Some(lock_parts) = parse_temp_permanent_lock_parts(&rule_body) {
                    if eff_dur.requires_ability_source() && !(in_trigger || self_pronoun_valid) {
                        return Err("oracle:unsupported-combat-mod");
                    }
                    let use_skip_next_untap = lock_parts.len() == 1
                        && matches!(
                            lock_parts[0],
                            ResolvedPartSpec::Abilities(ref ab)
                                if ab.doesnt_untap && ab.suppressed_activations.is_none()
                        )
                        && matches!(
                            eff_dur,
                            ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
                        );
                    if !use_skip_next_untap {
                        return emit_resolved_parts_for_subject(
                            subject,
                            ResolvedPartsSpec(lock_parts),
                            eff_dur,
                            builder,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        );
                    }
                }
                if let Some((flt_raw, nonmana_only)) = rule_body
                    .strip_prefix("activated abilities of ")
                    .and_then(|r| {
                        r.strip_suffix(" can't be activated unless they're mana abilities")
                            .map(|f| (f, true))
                            .or_else(|| r.strip_suffix(" can't be activated").map(|f| (f, false)))
                    })
                    && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                        flt_raw.trim(),
                        &cached_compiler_vocab(vocabulary),
                    )
                    && let Ok(sel) =
                        lower_permanent_selector(&SubjectRefAst::Group(Box::new(ast_f)), vocabulary)
                {
                    let rule = if nonmana_only {
                        StaticRestriction::PreventMatchingNonManaActivations(Box::new(sel))
                    } else {
                        StaticRestriction::PreventMatchingActivations(Box::new(sel))
                    };
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::temporary_static(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule,
                            },
                        ), eff_dur));
                    return Ok(());
                }
                if let Some((subj_raw, nonmana_only)) = rule_body
                    .strip_prefix("activated abilities of ")
                    .and_then(|r| {
                        r.strip_suffix(" can't be activated unless they're mana abilities")
                            .map(|f| (f, true))
                            .or_else(|| r.strip_suffix(" can't be activated").map(|f| (f, false)))
                    })
                    && matches!(
                        subj_raw.trim(),
                        "it" | "them"
                            | "that permanent"
                            | "that creature"
                            | "that artifact"
                            | "that land"
                            | "that planeswalker"
                            | "those permanents"
                            | "those creatures"
                    )
                {
                    let supp = if nonmana_only {
                        ActivationSuppression::NonMana
                    } else {
                        ActivationSuppression::All
                    };
                    let ab = AbilityChange {
                        suppressed_activations: Some(supp),
                        ..AbilityChange::default()
                    };
                    let target_subj = if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                        && is_permanent_body_target(builder.target)
                    {
                        SubjectRefAst::Anaphora("it".to_string())
                    } else {
                        eff_subject.clone()
                    };
                    if !(matches!(target_subj, SubjectRefAst::SelfObject)
                        && !self_pronoun_valid
                        && !in_trigger)
                        && emit_resolved_parts_for_subject(
                            &target_subj,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                            eff_dur,
                            builder,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        )
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
            }
            if matches!(
                low.as_str(),
                "all creature types"
                    | "gains all creature types"
                    | "gain all creature types"
                    | "has all creature types"
                    | "have all creature types"
                    | "is every creature type"
                    | "are every creature type"
            ) {
                let mut all_subtypes = BTreeSet::new();
                for ord in 0..vocabulary.len() {
                    if let Ok(ordinal) = u16::try_from(ord)
                        && let Some(ct) = vocabulary.resolve_member(vocabulary.id(), ordinal)
                    {
                        all_subtypes.insert(ct);
                    }
                }
                let type_part = ResolvedPartSpec::Type(TypeChangeSpec {
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                        all_subtypes,
                    )),
                    land_subtypes: SubtypeOperation::Keep,
                });
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![type_part]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if let Some(type_part) = parse_type_removal_part(&low, vocabulary) {
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![type_part]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if low.contains(
                "reselect which player or permanent target attacking creature is attacking",
            ) || low == "reselect_attack_defender"
            {
                builder.bind_target(BodyTarget::filtered_permanent(
                    ObjectFilter::creature().attacking(),
                ))?;
                builder
                    .instructions
                    .push(Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::reselect_attack_defender(),
                    )));
                return Ok(());
            }
            if let Some(kws_raw) = low.strip_prefix("cant_have:") {
                let suppressed = parse_suppressed_keyword_list(kws_raw)?;
                let ab_change = AbilityChange {
                    suppressed_keywords: suppressed,
                    ..AbilityChange::default()
                };
                return emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            if low == "activate_loyalty_twice"
                || low
                    == "you may activate loyalty abilities of planeswalkers you control twice each turn rather than only once"
            {
                let is_controlled_pws = match subject.as_ref() {
                    SubjectRefAst::Player(PlayerRefAst::You) => true,
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                        f.zone.is_none()
                            && f.any_types == AstCardTypeMask::PLANESWALKER
                            && f.controller == Some(ControllerRelation::You)
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    controller: true,
                                    ..FilterAllow::NONE
                                },
                            )
                    }
                    _ => false,
                };
                if is_controlled_pws {
                    if dur == ResolvedDuration::UntilEndOfTurn {
                        builder.instructions.push(Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::additional_loyalty_activation(),
                        )));
                    } else {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::temporary_static(
                                StaticContinuousAbility::Modifier {
                                    id: ids.static_id(),
                                    rule: StaticModifierRule::TwiceLoyaltyPerTurn,
                                },
                            ), dur));
                    }
                    return Ok(());
                }
            }
            if let Some(flt_raw) = low.strip_prefix("cant_cast:") {
                let flt_clean = flt_raw.trim().trim_end_matches('.');
                if flt_clean.contains(" from ") {
                    let synth = format!("can't cast {flt_clean}");
                    if let Some(res) = try_lower_cant_cast_from_zone(
                        Some(subject.as_ref()),
                        &synth,
                        None,
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                    ) {
                        return res;
                    }
                    return Err("oracle:unsupported-combat-mod");
                }
                let flt = if flt_clean.is_empty()
                    || matches!(flt_clean, "spells" | "a spell" | "any spells")
                {
                    None
                } else {
                    let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                        flt_clean,
                        &cached_compiler_vocab(vocabulary),
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod")?;
                    if ast_f.zone.is_some() {
                        if let Some(res) = try_lower_cant_cast_from_zone(
                            Some(subject.as_ref()),
                            "cantcast:filter",
                            Some(&ast_f),
                            dur,
                            builder,
                            vocabulary,
                            in_trigger,
                        ) {
                            return res;
                        }
                        return Err("oracle:unsupported-combat-mod");
                    }
                    Some(lower_object_filter(&ast_f, vocabulary)?)
                };
                let players = match subject.as_ref() {
                    SubjectRefAst::Player(p) => {
                        lower_affected_players_in_context(*p, builder, in_trigger)?
                    }
                    SubjectRefAst::TargetPlayer {
                        opponent_only,
                        count: TargetCountAst::Exact(1),
                    } => {
                        builder.bind_target(if *opponent_only {
                            BodyTarget::SINGLE_OPPONENT
                        } else {
                            BodyTarget::SINGLE_PLAYER
                        })?;
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                    }
                    _ => return Err("oracle:unsupported-combat-mod"),
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                        TemporaryRuleKind::CantCastSpells {
                            players,
                            zone: None,
                            filter: flt,
                        },
                        dur,
                    ),
                );
                return Ok(());
            }
            if low.contains("can't cast ")
                && low.contains(" from ")
                && let Some(res) = try_lower_cant_cast_from_zone(
                    Some(subject.as_ref()),
                    &low,
                    None,
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                )
            {
                return res;
            }
            let is_doesnt_untap_rule = low.contains("doesn't untap during")
                || low.contains("don't untap during")
                || low == "doesnt_untap"
                || low == "doesntuntap";
            if is_doesnt_untap_rule
                && !matches!(
                    dur,
                    ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
                )
            {
                if dur.requires_ability_source() && !(in_trigger || self_pronoun_valid) {
                    return Err("oracle:unsupported-combat-mod");
                }
                return emit_resolved_parts_for_subject(
                    eff_subject,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange {
                        doesnt_untap: true,
                        ..AbilityChange::default()
                    })]),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-combat-mod");
            }
            if matches!(
                dur,
                ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
            ) && is_doesnt_untap_rule
            {
                if let SubjectRefAst::Compound(items) = eff_subject
                    && !items.is_empty()
                {
                    let mut trial = builder.clone();
                    for item in items {
                        lower_combat_damage_instruction(
                            &InstructionAst::CombatRuleMod {
                                subject: Box::new(item.clone()),
                                rule: rule.clone(),
                                duration: duration.clone(),
                            },
                            &mut trial,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    *builder = trial;
                    return Ok(());
                }
                match eff_subject {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::AbilitySource));
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n > 0 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            *n,
                            *n,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n > 0 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            *n,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            1,
                            1,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::UpTo(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            1,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.x_target_count = true;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::skip_next_untap_targets(),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::CreatedTokens));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::skip_next_untap_targets());
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions)
                            || matches!(
                                builder.instructions.last(),
                                Some(Instruction::MutateObject(_))
                            ) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::LastMoved));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::TriggerSubject));
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::TriggerSubject));
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::Matching(group_obj)));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(EffectSubject::Matching(obj)));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let combat_action = if low.contains("goad") {
                Some(CombatActionKind::Goad)
            } else if low.contains("untap") && low.contains("remove") && low.contains("combat") {
                Some(CombatActionKind::UntapAndRemoveFromCombat)
            } else if low.contains("remove") && low.contains("from combat") {
                Some(CombatActionKind::RemoveFromCombat)
            } else if low == "must_attack"
                || low == "attacks_if_able"
                || low == "attacks_this_turn_if_able"
                || low == "attacks this turn if able"
                || low == "attack this turn if able"
                || low == "attacks this combat if able"
                || low == "attack this combat if able"
                || low == "attacks that combat if able"
                || low == "attack that combat if able"
                || low == "attacks that turn if able"
                || low == "attack that turn if able"
                || low == "attacks each combat if able"
                || low == "attack each combat if able"
                || low == "attacks each turn if able"
                || low == "attack each turn if able"
                || low == "attacks if able"
                || low == "attack if able"
                || low == "must attack"
                || low == "must attack this turn"
                || low == "must attack this turn if able"
                || low == "must attack this combat"
                || low == "must attack this combat if able"
                || low == "must attack each combat if able"
                || low == "must attack if able"
                || low == "attacks you this turn if able"
                || low == "attacks you this combat if able"
                || low == "attack you this turn if able"
                || low == "attack you this combat if able"
                || low == "attacks that player this turn if able"
                || low == "attacks that player this combat if able"
                || low == "attack that player this turn if able"
                || low == "attack that player this combat if able"
                || low == "attacks that opponent this turn if able"
                || low == "attacks that opponent this combat if able"
                || (!low.contains(" unless ")
                    && !low.contains(" player")
                    && !low.contains(" opponent")
                    && !low.contains(" attacks you")
                    && !low.contains(" attack you")
                    && !low.contains(" you or ")
                    && !low.contains(" or blocks ")
                    && (low.ends_with(" attacks this turn if able")
                        || low.ends_with(" attacks this combat if able")
                        || low.ends_with(" attacks that combat if able")
                        || low.ends_with(" attacks that turn if able")
                        || low.ends_with(" attacks each combat if able")
                        || low.ends_with(" attack this turn if able")
                        || low.ends_with(" attack this combat if able")))
                || low.strip_prefix("must_attack:").is_some_and(|t| {
                    matches!(
                        t.trim(),
                        "" | "this turn"
                            | "this combat"
                            | "each combat"
                            | "each turn"
                            | "if able"
                            | "this turn if able"
                            | "this combat if able"
                            | "each combat if able"
                    )
                })
                || low.strip_prefix("attacks_if_able:").is_some_and(|t| {
                    matches!(
                        t.trim(),
                        "" | "this turn"
                            | "this combat"
                            | "each combat"
                            | "each turn"
                            | "if able"
                            | "this turn if able"
                            | "this combat if able"
                            | "each combat if able"
                    )
                })
            {
                Some(CombatActionKind::MustAttack)
            } else if low == "must block"
                || low == "must block this turn"
                || low == "must block this combat"
                || low == "must block if able"
                || low == "must block this turn if able"
                || low == "must block this combat if able"
                || low == "blocks this turn if able"
                || low == "block this turn if able"
                || low == "blocks this combat if able"
                || low == "block this combat if able"
                || low == "blocks it this combat if able"
                || low == "blocks it this turn if able"
                || (!low.contains(" unless ")
                    && !low.contains(" attacks ")
                    && !low.contains(" or ")
                    && (low.ends_with(" blocks this turn if able")
                        || low.ends_with(" blocks this combat if able")
                        || low.ends_with(" blocks that combat if able")
                        || low.ends_with(" block this turn if able")
                        || low.ends_with(" block this combat if able")
                        || low.ends_with(" must block this turn if able")
                        || low.ends_with(" must block this combat if able")))
            {
                Some(CombatActionKind::MustBlock)
            } else if (low.contains("all creatures able to block") && low.contains("do so"))
                || low == "must_be_blocked_if_able"
                || low == "allableblock"
                || (low.contains("must be blocked")
                    && !low.contains("by two")
                    && !low.contains("by three"))
            {
                Some(CombatActionKind::MustBeBlocked)
            } else if low == "becomes_blocked"
                || low == "becomes blocked"
                || low == "become blocked"
            {
                Some(CombatActionKind::BecomeBlocked)
            } else if low.contains("prevent all combat damage that would be dealt to and dealt by")
                || low.contains("prevent all combat damage that would be dealt by and to")
                || low.contains("prevent all combat damage that would be dealt to and by")
                || low.contains("prevent all combat damage that would be dealt by and dealt to")
                || low == "prevent_combat_damage_by_and_to"
            {
                Some(CombatActionKind::PreventCombatDamageByAndTo)
            } else {
                None
            };
            if let Some(action_kind) = combat_action {
                match eff_subject {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::AbilitySource, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TriggerSubject, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TriggerSubject, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )), action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::UpTo(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            1,
                            super::super::toolbox_emit::require_combat_action_targets(action_kind),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            *n,
                            *n,
                            super::super::toolbox_emit::require_combat_action_targets(action_kind),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0 =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            *max,
                            super::super::toolbox_emit::require_combat_action_targets(action_kind),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::require_combat_action_targets(action_kind),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.x_target_count = true;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::require_combat_action_targets(action_kind),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::CreatedTokens, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::LastMoved, action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && builder.last_group_filter.is_some() =>
                    {
                        let obj = builder
                            .last_group_filter
                            .ok_or("oracle:unsupported-combat-mod")?;
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::Matching(obj), action_kind));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.last_group_filter = Some(obj);
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::Matching(obj), action_kind));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if low.contains("goad") {
                if matches!(
                    subject.as_ref(),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(
                            f.controller,
                            Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                        ) && f.any_types == AstCardTypeMask::CREATURE
                            && f.forbidden_types.is_empty()
                            && f.subtypes.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                ) {
                    builder.last_group_filter = Some(ObjectFilter::creature().opposing());
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::goad_other_players_creatures());
                    return Ok(());
                }
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::goad_target());
                return Ok(());
            }
            if low.contains("attacks this turn if able") || low == "must_attack" {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(
                        EffectSubject::TargetObject,
                        CombatActionKind::MustAttack,
                    ),
                );
                return Ok(());
            }
            if (low.contains("must block this turn if able")
                || low.contains("blocks this turn if able"))
                && matches!(
                    subject.as_ref(),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(
                            f.controller,
                            Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                        ) && f.any_types == AstCardTypeMask::CREATURE
                )
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(
                        EffectSubject::Matching(ObjectFilter::creature().opposing()),
                        CombatActionKind::MustBlock,
                    ),
                );
                return Ok(());
            }
            if low.contains("all creatures able to block")
                && low.contains("do so")
                && let Ok(bt) = lower_target_subject(subject, vocabulary)
            {
                builder.bind_target(bt)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(
                        EffectSubject::TargetObject,
                        CombatActionKind::MustBeBlocked,
                    ),
                );
                return Ok(());
            }
            let mut rule_for_br = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            let mut br_dur = dur;
            for (suffix, implied_dur) in [
                (
                    " until your next turn",
                    ResolvedDuration::UntilControllerNextTurn,
                ),
                (" until the end of turn", ResolvedDuration::UntilEndOfTurn),
                (" until end of turn", ResolvedDuration::UntilEndOfTurn),
                (
                    " until the end of combat",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (" until end of combat", ResolvedDuration::UntilEndOfCombat),
                (" this combat", ResolvedDuration::UntilEndOfCombat),
                (" that combat", ResolvedDuration::UntilEndOfCombat),
                (" this turn", ResolvedDuration::UntilEndOfTurn),
                (" that turn", ResolvedDuration::UntilEndOfTurn),
            ] {
                if let Some(stripped) = rule_for_br.strip_suffix(suffix) {
                    rule_for_br = stripped.trim().to_owned();
                    if br_dur == ResolvedDuration::NoStatedDuration
                        || (br_dur == ResolvedDuration::UntilEndOfTurn
                            && implied_dur != ResolvedDuration::UntilEndOfTurn)
                    {
                        br_dur = implied_dur;
                    }
                    break;
                }
            }
            for (infix, implied_dur) in [
                (" this turn if able", ResolvedDuration::UntilEndOfTurn),
                (" that turn if able", ResolvedDuration::UntilEndOfTurn),
                (" this combat if able", ResolvedDuration::UntilEndOfCombat),
                (" that combat if able", ResolvedDuration::UntilEndOfCombat),
            ] {
                if let Some(prefix) = rule_for_br.strip_suffix(infix) {
                    rule_for_br = format!("{} if able", prefix.trim());
                    if br_dur == ResolvedDuration::NoStatedDuration
                        || (br_dur == ResolvedDuration::UntilEndOfTurn
                            && implied_dur != ResolvedDuration::UntilEndOfTurn)
                    {
                        br_dur = implied_dur;
                    }
                    break;
                }
            }
            for (needle, replacement, implied_dur) in [
                (
                    "can't be blocked this turn except by ",
                    "can't be blocked except by ",
                    ResolvedDuration::UntilEndOfTurn,
                ),
                (
                    "can't be blocked this combat except by ",
                    "can't be blocked except by ",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (
                    "can't be blocked this turn by ",
                    "can't be blocked by ",
                    ResolvedDuration::UntilEndOfTurn,
                ),
                (
                    "can't be blocked this combat by ",
                    "can't be blocked by ",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (
                    "can attack this turn as though ",
                    "can attack as though ",
                    ResolvedDuration::UntilEndOfTurn,
                ),
                (
                    "can attack this combat as though ",
                    "can attack as though ",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (
                    "can block an additional creature this combat",
                    "can block an additional creature",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (
                    "can block an additional creature this turn",
                    "can block an additional creature",
                    ResolvedDuration::UntilEndOfTurn,
                ),
                (
                    "can block any number of creatures this combat",
                    "can block any number of creatures",
                    ResolvedDuration::UntilEndOfCombat,
                ),
                (
                    "can block any number of creatures this turn",
                    "can block any number of creatures",
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ] {
                if rule_for_br.contains(needle) {
                    rule_for_br = rule_for_br.replace(needle, replacement);
                    if br_dur == ResolvedDuration::NoStatedDuration
                        || (br_dur == ResolvedDuration::UntilEndOfTurn
                            && implied_dur != ResolvedDuration::UntilEndOfTurn)
                    {
                        br_dur = implied_dur;
                    }
                    break;
                }
            }
            if matches!(
                rule_for_br.as_str(),
                "can't be sacrificed" | "cant_be_sacrificed" | "cannot be sacrificed"
            ) {
                let has_source = in_trigger || self_pronoun_valid || builder.has_source;
                let sel_opt = if is_self_pronoun(eff_subject, builder, self_pronoun_valid) {
                    has_source.then(|| PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    })
                } else if matches!(eff_subject, SubjectRefAst::AttachedHost(_)) {
                    has_source.then(|| PermanentSelector {
                        attached_to_source: true,
                        ..PermanentSelector::default()
                    })
                } else {
                    lower_permanent_selector(eff_subject, vocabulary).ok()
                };
                if let Some(sel) = sel_opt {
                    let sel_needs_source = sel.attached_to_source
                        || sel.exclude_source
                        || matches!(
                            sel.controller,
                            ControllerSelector::SourceItself
                                | ControllerSelector::SelfAtMaxSpeed
                                | ControllerSelector::SourcePair
                        );
                    if (!sel_needs_source && !br_dur.requires_ability_source()) || has_source {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                                TemporaryRuleKind::temporary_static(
                                    StaticContinuousAbility::Restriction {
                                        id: ids.static_id(),
                                        rule: StaticRestriction::CannotBeSacrificed(Box::new(sel)),
                                    },
                                ),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                }
            }
            if matches!(
                low.as_str(),
                "attacks during its controller's next turn if able"
                    | "attacks during their next turn if able"
                    | "attack during their controllers' next turn if able"
                    | "attacks during its controller's next combat if able"
                    | "attacks next turn if able"
            ) {
                let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                    BlockingRestriction::MustAttackEachCombatIfAble,
                )]);
                return emit_resolved_parts_for_subject(
                    eff_subject,
                    parts,
                    ResolvedDuration::UntilControllerNextTurn,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-combat-mod");
            }
            if rule_for_br == "attacks or blocks if able"
                || rule_for_br == "attack or block if able"
                || rule_for_br == "attacks_or_blocks_if_able"
                || rule_for_br == "attack_or_block_if_able"
                || rule_for_br.ends_with(" attacks or blocks if able")
                || rule_for_br.ends_with(" attack or block if able")
            {
                let parts = ResolvedPartsSpec(vec![
                    ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    ),
                    ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::MustBlockEachCombatIfAble,
                    ),
                ]);
                return emit_resolved_parts_for_subject(
                    eff_subject,
                    parts,
                    br_dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-combat-mod");
            }
            if let Some(brs) = lower_combat_mod_blocking_restrictions(&rule_for_br, vocabulary)
                .filter(|v| v.len() >= 2)
                && brs.iter().all(|br| {
                    !matches!(
                        br,
                        BlockingRestriction::CannotBlockSource
                            | BlockingRestriction::MustBlockSource
                            | BlockingRestriction::CannotBeBlockedBySource
                    ) || self_pronoun_valid
                })
            {
                let parts = ResolvedPartsSpec(
                    brs.into_iter()
                        .map(ResolvedPartSpec::BlockingRestriction)
                        .collect(),
                );
                return emit_resolved_parts_for_subject(
                    eff_subject,
                    parts,
                    br_dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-combat-mod");
            }
            if let Some(br) = lower_combat_mod_blocking_restriction(&rule_for_br, vocabulary)
                .or_else(|| lower_rule_blocking_restriction(&rule_for_br, None, vocabulary))
                .or_else(|| lower_extra_combat_mod_blocking_restriction(&rule_for_br))
                && (!matches!(
                    br,
                    BlockingRestriction::CannotBlockSource
                        | BlockingRestriction::MustBlockSource
                        | BlockingRestriction::CannotBeBlockedBySource
                ) || self_pronoun_valid)
            {
                match eff_subject {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Source,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]), br_dur));
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]), br_dur));
                        return Ok(());
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::UpTo(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            1,
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            *n,
                            *n,
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && *max > 0 =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            *max,
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::ChosenX,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                        && !in_trigger =>
                    {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(bt)?;
                        builder.x_target_count = true;
                        builder.push_bounded_target_op(
                            0,
                            u16::MAX,
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::AttachedHost(_) => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::Matching(ObjectFilter::Clause(
                                        ObjectClause::any().attached_to_self(),
                                    )),
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::Matching(obj),
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::TriggerSubject,
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if is_permanent_body_target(builder.target)
                            || matches!(
                                builder.target,
                                BodyTarget::AnyTarget(
                                    AnyTargetKind::Any
                                        | AnyTargetKind::CreatureOrPlayer
                                        | AnyTargetKind::PlayerOrPlaneswalker
                                        | AnyTargetKind::OpponentOrPlaneswalker
                                )
                            ) =>
                    {
                        let parts =
                            ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]);
                        builder.instructions.push(
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                parts,
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_created_tokens(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::CreatedTokens,
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if !is_permanent_body_target(builder.target)
                            && last_instruction_moved_object(&builder.instructions) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::LastMoved,
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        builder.bind_target(BodyTarget::filtered_permanent(obj))?;
                        builder
                            .instructions
                            .push(Instruction::ApplyContinuous(Box::new(
                                spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(
                                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                        br,
                                    )]),
                                    br_dur,
                                ),
                            )));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(raw)
                        if builder.target == BodyTarget::None
                            && in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::TriggerSubject,
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && builder.last_group_filter.is_some() =>
                    {
                        let obj = builder
                            .last_group_filter
                            .ok_or("oracle:unsupported-combat-mod")?;
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::Matching(obj),
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]),
                                br_dur,
                            ),
                        );
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if let Some(src_raw) = rule_for_br
                .strip_prefix("cantbetargetof:")
                .or_else(|| rule_for_br.strip_prefix("can't be the target of "))
                .or_else(|| rule_for_br.strip_prefix("can't be the targets of "))
            {
                if let Some((mask, spells_only)) = parse_cant_be_target_colors(src_raw) {
                    let part = if spells_only {
                        ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(mask)
                    } else {
                        ResolvedPartSpec::CannotBeTargetedByColors(mask)
                    };
                    return emit_resolved_parts_for_subject(
                        eff_subject,
                        ResolvedPartsSpec(vec![part]),
                        br_dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod");
                }
                if let Some(kws) = parse_cant_be_target_keywords(src_raw, vocabulary) {
                    return emit_resolved_parts_for_subject(
                        subject,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::grant_keywords(kws),
                        )]),
                        br_dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                    .map_err(|_| "oracle:unsupported-combat-mod");
                }
            }
            if let Some(attacker_raw) = rule_for_br
                .strip_prefix("must_block:")
                .or_else(|| rule_for_br.strip_prefix("must block "))
                .or_else(|| {
                    rule_for_br
                        .strip_prefix("blocks ")
                        .and_then(|r| r.strip_suffix(" if able"))
                })
                .or_else(|| {
                    rule_for_br
                        .strip_prefix("block ")
                        .and_then(|r| r.strip_suffix(" if able"))
                })
            {
                let mut trial = builder.clone();
                if let Some(attacker_ref) = parse_must_block_subject(
                    attacker_raw,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                ) && emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::MustBlockSubject(attacker_ref)]),
                    br_dur,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .is_ok()
                {
                    *builder = trial;
                    return Ok(());
                }
                // Two-target forced block via FixedTargetClause + RememberPermanentTarget:
                // Case 1: prior target already bound (e.g. Feral Contest: "Put a +1/+1 counter on
                // target creature you control. Another target creature blocks it this turn if able.")
                let mut attacker_clean = attacker_raw
                    .trim()
                    .trim_end_matches('.')
                    .trim()
                    .to_ascii_lowercase();
                for suffix in [
                    " this combat if able",
                    " this turn if able",
                    " if able",
                    " this combat",
                    " this turn",
                ] {
                    if let Some(s) = attacker_clean.strip_suffix(suffix) {
                        attacker_clean = s.trim().to_owned();
                    }
                }
                if let SubjectRefAst::TargetObject {
                    filter: blocker_f,
                    count: TargetCountAst::Exact(1),
                } = eff_subject
                    && matches!(blocker_f.zone, None | Some(ZoneFilter::Battlefield))
                {
                    let mut bf_clean = blocker_f.as_ref().clone();
                    bf_clean.zone = None;
                    if let Ok(blocker_bt) = lower_battlefield_body_target_in_context(
                        &bf_clean, builder, in_trigger, vocabulary,
                    ) && is_permanent_body_target(blocker_bt)
                    {
                        if matches!(
                            attacker_clean.as_str(),
                            "it" | "that creature" | "that permanent"
                        ) && is_permanent_body_target(builder.target)
                            && !builder.instructions.is_empty()
                            && builder.graveyard_target_bounds.is_none()
                            && builder.permanent_target_bounds.is_none()
                            && !builder.x_target_count
                            && !builder.up_to_one_target
                            && !builder.instructions.iter().any(|op| {
                                matches!(op, Instruction::FixedTargetClause(_))
                                    || matches!(
                                        op,
                                        Instruction::MutateObject(spec)
                                            if matches!(
                                                spec.op,
                                                spellwake_engine::toolbox::ObjectMutation::StateMarker(spellwake_engine::toolbox::ObjectStateMarkerOp::RememberPermanentTarget)
                                            )
                                    )
                            })
                        {
                            let mut first_ops = std::mem::take(&mut builder.instructions);
                            prepend_instruction_at(
                                &mut first_ops,
                                0,
                                Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target())),
                            );
                            let first_bt = std::mem::replace(&mut builder.target, BodyTarget::None);
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(first_bt, first_ops),
                                )));
                            builder
                                .instructions
                                .push(Instruction::FixedTargetClause(Box::new(
                                    ExecutableBody::new(
                                        blocker_bt,
                                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![
                                                ResolvedPartSpec::MustBlockSubject(
                                                    MoveSubjectRef::RememberedFirst,
                                                ),
                                            ]), br_dur)],
                                    ),
                                )));
                            return Ok(());
                        }
                        // Case 2: two independent targets (e.g. Hunt Down, Rimehorn Aurochs:
                        // "Target creature blocks target creature this turn if able.")
                        if builder.target == BodyTarget::None
                            && builder.instructions.is_empty()
                            && let Ok(SubjectRefAst::TargetObject {
                                filter: atk_f,
                                count: TargetCountAst::Exact(1),
                            }) = spellwake_oracle_compiler::filter::parse_subject_or_target(
                                &attacker_clean,
                                &cached_compiler_vocab(vocabulary),
                            )
                            && matches!(atk_f.zone, None | Some(ZoneFilter::Battlefield))
                        {
                            let mut af_clean = atk_f.as_ref().clone();
                            af_clean.zone = None;
                            if let Ok(attacker_bt) = lower_battlefield_body_target_in_context(
                                &af_clean, builder, in_trigger, vocabulary,
                            ) && is_permanent_body_target(attacker_bt)
                                && attacker_bt == blocker_bt
                            {
                                builder.instructions.push(Instruction::FixedTargetClause(
                                    Box::new(ExecutableBody::new(
                                        attacker_bt,
                                        vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))],
                                    )),
                                ));
                                builder.instructions.push(Instruction::FixedTargetClause(
                                    Box::new(ExecutableBody::new(
                                        blocker_bt,
                                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![
                                                ResolvedPartSpec::MustBlockSubject(
                                                    MoveSubjectRef::RememberedFirst,
                                                ),
                                            ]), br_dur)],
                                    )),
                                ));
                                return Ok(());
                            }
                        }
                    }
                }
            }
            Err("oracle:unsupported-combat-mod")
        }
        InstructionAst::RedirectDamage {
            amount,
            from,
            to,
            source_choice,
            duration,
        } => {
            let has_source = in_trigger || self_pronoun_valid;
            let dur = match duration {
                Some(d) => lower_duration(d)?,
                None => ResolvedDuration::UntilEndOfTurn,
            };
            let mut trial = builder.clone();
            let (source, source_is_target) = match source_choice.as_deref() {
                None => (None, false),
                Some(sc) => {
                    let sc_low = sc.trim().to_ascii_lowercase();
                    let (src, chosen_color, chosen_type, is_tgt) =
                        parse_next_time_source_with_target(
                            &sc_low, &mut trial, has_source, in_trigger, vocabulary,
                        )
                        .ok_or("oracle:unsupported-redirect-damage")??;
                    if chosen_color || chosen_type {
                        return Err("oracle:unsupported-redirect-damage");
                    }
                    (src, is_tgt)
                }
            };
            let from_rec = if has_source && is_self_pronoun(from, &trial, self_pronoun_valid) {
                StaticDamageRecipient::SelfObject
            } else {
                lower_redirect_from_subject_with_target(
                    from, &mut trial, has_source, in_trigger, vocabulary,
                )?
            };
            let to_dst = if has_source && is_self_pronoun(to, &trial, self_pronoun_valid) {
                DamageRedirectTarget::AbilitySource
            } else {
                lower_redirect_to_subject(to, &mut trial, has_source, vocabulary)?
            };
            let target_slots = usize::from(source_is_target)
                + usize::from(matches!(
                    from_rec,
                    StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
                ))
                + usize::from(matches!(
                    to_dst,
                    DamageRedirectTarget::TargetObject | DamageRedirectTarget::TargetAny
                ));
            if target_slots > 1 {
                return Err("oracle:unsupported-redirect-damage");
            }
            let resolved_amt = resolve_x(amount, bound_x);
            let rule = if matches!(resolved_amt, ValueExprAst::PriorResult) {
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from: from_rec,
                    to: to_dst,
                    combat_only: false,
                    amount: None,
                    one_shot: false,
                }
            } else {
                let expr =
                    lower_value_expr_in_context(resolved_amt, &trial, in_trigger, vocabulary)?;
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from: from_rec,
                    to: to_dst,
                    combat_only: false,
                    amount: Some(Box::new(expr)),
                    one_shot: false,
                }
            };
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    rule, dur,
                ),
            );
            *builder = trial;
            Ok(())
        }
        InstructionAst::RemoveFromCombat { subject } => match subject.as_ref() {
            SubjectRefAst::SelfObject => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Anaphora(raw)
                if matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "it" | "this creature" | "that creature"
                ) =>
            {
                if builder.target.guarantees_creature() {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::TargetObject,
                            )),
                        ),
                    );
                    Ok(())
                } else if self_pronoun_valid
                    && matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "it" | "this creature"
                    )
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                        ),
                    );
                    Ok(())
                } else if in_trigger && builder.target == BodyTarget::None {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                            spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                        ),
                    );
                    Ok(())
                } else {
                    Err("oracle:unsupported-target-subject")
                }
            }
            SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                        )),
                    ),
                );
                Ok(())
            }
            SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let obj = lower_object_filter(f, vocabulary)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::Matching(obj),
                        )),
                    ),
                );
                Ok(())
            }
            _ => {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::TargetObject,
                        )),
                    ),
                );
                Ok(())
            }
        },
        InstructionAst::DealDamageDivided {
            source,
            amount,
            targets_desc,
            divider: None,
        } => {
            let is_self_divided_source = is_self_pronoun(source, builder, self_pronoun_valid)
                || match source.as_ref() {
                    SubjectRefAst::Anaphora(raw) => {
                        let low = raw.trim().to_ascii_lowercase();
                        low == "this spell"
                            || ((self_pronoun_valid || in_trigger)
                                && matches!(
                                    low.as_str(),
                                    "this creature"
                                        | "this permanent"
                                        | "this artifact"
                                        | "this enchantment"
                                        | "this land"
                                        | "this planeswalker"
                                        | "this vehicle"
                                        | "this equipment"
                                        | "this aura"
                                        | "this saga"
                                        | "this battle"
                                        | "this room"
                                        | "this artifact creature"
                                        | "this enchantment creature"
                                ))
                            || (is_self_anaphora_label(raw)
                                && (self_pronoun_valid
                                    || (!in_trigger
                                        && !last_instruction_created_tokens(
                                            &builder.instructions,
                                        )
                                        && !last_instruction_moved_object(&builder.instructions)
                                        && builder.target == BodyTarget::None)))
                    }
                    _ => false,
                };
            if !is_self_divided_source {
                return Err("oracle:unsupported-divided-damage");
            }
            let raw_amount = resolve_x(amount, bound_x);
            let sac_rewritten_amount = rewrite_sacrificed_permanent_anaphora(raw_amount);
            let raw_amount_eff = sac_rewritten_amount.as_ref().unwrap_or(raw_amount);
            let rewrite_self_anaphora =
                builder.target == BodyTarget::None && (self_pronoun_valid || in_trigger);
            let rewritten_amount;
            let effective_amount =
                if rewrite_self_anaphora && value_expr_ast_has_self_anaphora(raw_amount_eff) {
                    rewritten_amount = rewrite_value_expr_ast_self_anaphora(raw_amount_eff);
                    &rewritten_amount
                } else {
                    raw_amount_eff
                };
            let amt = rewrite_milled_card_mana_value(
                lower_value_expr_in_context(effective_amount, builder, in_trigger, vocabulary)?,
                builder,
            );
            let lit_total = match &amt {
                ValueExpr::Literal(n) => n.to_usize().and_then(|v| u16::try_from(v).ok()),
                _ => None,
            };
            if matches!(&amt, ValueExpr::Literal(_)) && lit_total.is_none_or(|v| v == 0) {
                return Err("oracle:unsupported-divided-damage");
            }
            let desc_low = targets_desc
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            let (cap_opt, after_count, x_exact, x_up_to) = if let Some(rest) =
                desc_low.strip_prefix("any number of ")
            {
                (None, rest.trim(), false, false)
            } else if let Some(rest) = desc_low.strip_prefix("one or more ") {
                (None, rest.trim(), false, false)
            } else if let Some(rest) = desc_low.strip_prefix("up to x ") {
                if in_trigger {
                    return Err("oracle:unsupported-divided-damage");
                }
                (None, rest.trim(), false, true)
            } else if let Some(rest) = desc_low.strip_prefix("x ") {
                if in_trigger {
                    return Err("oracle:unsupported-divided-damage");
                }
                (None, rest.trim(), true, false)
            } else if let Some(rest) = desc_low.strip_prefix("one or two ") {
                (Some(2u16), rest.trim(), false, false)
            } else if let Some(rest) = desc_low.strip_prefix("one, two, or three ") {
                (Some(3u16), rest.trim(), false, false)
            } else if let Some(rest) = desc_low.strip_prefix("one, two, three, or four ") {
                (Some(4u16), rest.trim(), false, false)
            } else if let Some(rest) = desc_low.strip_prefix("one, two, three, four, or five ") {
                (Some(5u16), rest.trim(), false, false)
            } else if let Some(after_up) = desc_low.strip_prefix("up to ")
                && let Some((num_tok, rest)) = after_up.split_once(' ')
                && let Some(n) = parse_small_number_word(num_tok)
                && n > 0
            {
                (Some(n), rest.trim(), false, false)
            } else {
                (None, desc_low.as_str(), false, false)
            };
            let core_owned = after_count
                .strip_prefix("target ")
                .map(str::to_owned)
                .unwrap_or_else(|| after_count.replacen(" target ", " ", 1));
            let core = core_owned.trim();
            let bt = match core {
                "targets"
                | "any target"
                | "any targets"
                | "any target or targets"
                | "creatures, planeswalkers, and/or players"
                | "creatures, planeswalkers, or players"
                | "creatures, planeswalkers, and players" => Some(BodyTarget::SINGLE_ANY_TARGET),
                "creatures and/or players" | "creatures or players" | "creatures and players" => {
                    Some(BodyTarget::SINGLE_CREATURE_OR_PLAYER)
                }
                "players and/or planeswalkers" | "players or planeswalkers" => {
                    Some(BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER)
                }
                "opponents and/or planeswalkers" | "opponents or planeswalkers" => {
                    Some(BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER)
                }
                "creatures and/or planeswalkers"
                | "creatures or planeswalkers"
                | "creatures and planeswalkers" => Some(BodyTarget::filtered_permanent(
                    ObjectFilter::Clause(ObjectClause {
                        any_types: CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
                        ..ObjectClause::any()
                    }),
                )),
                "creatures and/or planeswalkers your opponents control"
                | "creatures or planeswalkers your opponents control"
                | "creatures and/or planeswalkers you don't control"
                | "creatures or planeswalkers you don't control"
                | "creatures and/or planeswalkers an opponent controls"
                | "creatures or planeswalkers an opponent controls" => Some(
                    BodyTarget::filtered_permanent(ObjectFilter::Clause(ObjectClause {
                        any_types: CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
                        controller: ControllerFilter::Opponent,
                        ..ObjectClause::any()
                    })),
                ),
                other => {
                    if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                        other,
                        &cached_compiler_vocab(vocabulary),
                    ) && ast_f.zone.is_none()
                        && !ast_f.is_ability
                        && !ast_f.any_spell
                    {
                        lower_battlefield_body_target_in_context(
                            &ast_f, builder, in_trigger, vocabulary,
                        )
                        .ok()
                    } else {
                        None
                    }
                }
            };
            if let Some(bt) = bt
                && matches!(
                    bt,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    } | BodyTarget::AnyTarget(
                        AnyTargetKind::Any
                            | AnyTargetKind::CreatureOrPlayer
                            | AnyTargetKind::PlayerOrPlaneswalker
                            | AnyTargetKind::OpponentOrPlaneswalker
                    )
                )
            {
                builder.bind_target(bt)?;
                if x_exact {
                    builder.x_target_count = true;
                }
                if x_up_to {
                    builder.up_to_x_target_count = true;
                }
                if let Some(total) = lit_total
                    && cap_opt.is_none_or(|cap| total <= cap)
                {
                    if in_trigger
                        && total == 3
                        && bt == BodyTarget::SINGLE_ANY_TARGET
                        && !x_exact
                        && !x_up_to
                    {
                        builder.instructions.push(Instruction::DealDamage(Box::new(
                            spellwake_engine::toolbox::DealDamageSpec::divide_three(),
                        )));
                    } else {
                        builder.instructions.push(Instruction::DealDamage(Box::new(
                            spellwake_engine::toolbox::DealDamageSpec::divided(
                                ValueExpr::Literal(i64::from(total).into()),
                                None,
                            ),
                        )));
                    }
                } else {
                    builder.instructions.push(Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::divided(amt, cap_opt),
                    )));
                }
                return Ok(());
            }
            Err("oracle:unsupported-divided-damage")
        }
        InstructionAst::Detain { subject } => {
            let lock_parts =
                parse_temp_permanent_lock_parts("detain").ok_or("oracle:unsupported-detain")?;
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(lock_parts),
                ResolvedDuration::UntilControllerNextTurn,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-detain")
        }
        InstructionAst::SetGoaded {
            subject,
            remove: false,
        } => lower_instruction(
            &InstructionAst::CombatRuleMod {
                subject: subject.clone(),
                rule: "goad".to_string(),
                duration: DurationAst::UntilYourNextTurn,
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::ForcedBlockByFilter {
            blockers,
            target,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let target_ref = lower_move_subject_ref(
                target,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            emit_resolved_parts_for_subject(
                blockers,
                ResolvedPartsSpec(vec![ResolvedPartSpec::MustBlockSubject(target_ref)]),
                dur,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-forced-block")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

pub(super) fn parse_next_time_damage_source(
    src_raw: &str,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(Option<ObjectFilter>, bool, bool)> {
    let mut s = src_raw.trim().to_ascii_lowercase();
    if matches!(s.as_str(), "~" | "this creature" | "this permanent") {
        if !has_source {
            return None;
        }
        return Some((
            Some(ObjectFilter::Clause(ObjectClause {
                source_relation: SourceRelation::SelfObject,
                ..ObjectClause::any()
            })),
            false,
            false,
        ));
    }
    let mut source_chosen_color = false;
    for pat in [
        "of the chosen color",
        "of the last chosen color",
        "that's the chosen color",
    ] {
        if s.contains(pat) {
            source_chosen_color = true;
            s = s.replace(pat, " ");
        }
    }
    let mut source_chosen_type = false;
    for pat in [
        "of the chosen creature type",
        "of the last chosen creature type",
        "that's the chosen creature type",
        "of a creature type of your choice",
        "of the chosen type",
        "of the last chosen type",
        "that's the chosen type",
    ] {
        if s.contains(pat) {
            source_chosen_type = true;
            s = s.replace(pat, " ");
        }
    }
    if (source_chosen_color || source_chosen_type) && !has_source {
        return None;
    }
    for pat in ["of your choice", "you choose"] {
        s = s.replace(pat, " ");
    }
    let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut required_keyword: Option<KeywordAbility> = None;
    let body = if let Some((before_with, kw_part)) = normalized.rsplit_once(" with ")
        && let Some(kw) = lower_keyword_ability_name(kw_part.trim())
    {
        required_keyword = Some(kw);
        before_with.trim().to_owned()
    } else {
        normalized
    };
    let tokens: Vec<&str> = body
        .split_whitespace()
        .filter(|t| !matches!(*t, "a" | "an" | "any" | "the" | "source" | "sources" | "or"))
        .collect();
    if tokens.is_empty() {
        return match required_keyword {
            None => Some((None, source_chosen_color, source_chosen_type)),
            Some(kw) => Some((
                Some(ObjectFilter::Clause(ObjectClause {
                    keyword: Some(kw),
                    ..ObjectClause::any()
                })),
                source_chosen_color,
                source_chosen_type,
            )),
        };
    }
    let mut any_colors = ColorMask::default();
    let mut forbidden_colors = ColorMask::default();
    let mut any_types = CardTypeMask::default();
    let mut subtype: Option<CreatureTypeId> = None;
    let mut saw_spell = false;
    let mut token_ok = true;
    for tok in tokens {
        match tok {
            "white" => any_colors = any_colors.union(ColorMask::WHITE),
            "blue" => any_colors = any_colors.union(ColorMask::BLUE),
            "black" => any_colors = any_colors.union(ColorMask::BLACK),
            "red" => any_colors = any_colors.union(ColorMask::RED),
            "green" => any_colors = any_colors.union(ColorMask::GREEN),
            "colorless" => forbidden_colors = ColorMask::ALL,
            "artifact" | "artifacts" => any_types = any_types.union(CardTypeMask::ARTIFACT),
            "creature" | "creatures" => any_types = any_types.union(CardTypeMask::CREATURE),
            "enchantment" | "enchantments" => {
                any_types = any_types.union(CardTypeMask::ENCHANTMENT);
            }
            "land" | "lands" => any_types = any_types.union(CardTypeMask::LAND),
            "instant" | "instants" => any_types = any_types.union(CardTypeMask::INSTANT),
            "sorcery" | "sorceries" => any_types = any_types.union(CardTypeMask::SORCERY),
            "planeswalker" | "planeswalkers" => {
                any_types = any_types.union(CardTypeMask::PLANESWALKER);
            }
            "spell" | "spells" => saw_spell = true,
            _ => {
                if subtype.is_none()
                    && let Some(ct) = resolve_protection_subtype(tok, vocabulary)
                {
                    subtype = Some(ct);
                } else {
                    token_ok = false;
                    break;
                }
            }
        }
    }
    if saw_spell
        && !(any_types == CardTypeMask::INSTANT
            || any_types == CardTypeMask::SORCERY
            || any_types == CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
    {
        token_ok = false;
    }
    if !token_ok {
        let candidate = body.trim();
        if !candidate.is_empty()
            && let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                candidate,
                &cached_compiler_vocab(vocabulary),
            )
        {
            ast_f.zone = None;
            if let Ok(mut obj) = lower_object_filter(&ast_f, vocabulary)
                && !obj.has_target_player()
            {
                if let Some(kw) = required_keyword {
                    match &mut obj {
                        ObjectFilter::Clause(c) if c.keyword.is_none() => c.keyword = Some(kw),
                        _ => return None,
                    }
                }
                return Some((Some(obj), source_chosen_color, source_chosen_type));
            }
        }
        return None;
    }
    let mut clause = ObjectClause::any();
    if !any_colors.is_empty() {
        clause.color = ObjectColorFilter::any_of(any_colors);
    } else if !forbidden_colors.is_empty() {
        clause.color = ObjectColorFilter::Colorless;
    }
    if !any_types.is_empty() {
        clause.any_types = any_types;
    }
    if let Some(ct) = subtype {
        clause.subtypes = SubtypeFilter::Has(ct);
    }
    if let Some(kw) = required_keyword {
        clause.keyword = Some(kw);
    }
    Some((
        Some(ObjectFilter::Clause(clause)),
        source_chosen_color,
        source_chosen_type,
    ))
}

pub(super) fn lower_fight_or_exchange_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
    allow_planeswalker: bool,
    had_perm_target: bool,
) -> Result<MoveSubjectRef, &'static str> {
    let has_source = in_trigger || self_pronoun_valid;
    if has_source && is_self_pronoun(subj, builder, self_pronoun_valid) {
        return Ok(MoveSubjectRef::AbilitySource);
    }
    match subj {
        SubjectRefAst::SelfObject if has_source => Ok(MoveSubjectRef::AbilitySource),
        SubjectRefAst::TargetObject {
            filter,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target(&f_clean, vocabulary)?;
            let valid_bt = matches!(
                bt,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) && (allow_planeswalker || bt != BodyTarget::SINGLE_PLANESWALKER);
            if !valid_bt {
                return Err("oracle:unsupported-fight");
            }
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) {
                builder.up_to_one_target = true;
            }
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
        } => {
            let bt = lower_target_one_of_permanent_body_target(
                options, builder, in_trigger, vocabulary,
            )?;
            let valid_bt = matches!(
                bt,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) && (allow_planeswalker || bt != BodyTarget::SINGLE_PLANESWALKER);
            if !valid_bt {
                return Err("oracle:unsupported-fight");
            }
            builder.bind_target(bt)?;
            if *count == TargetCountAst::UpTo(1) {
                builder.up_to_one_target = true;
            }
            Ok(MoveSubjectRef::TargetObject)
        }
        SubjectRefAst::TriggerSubject if in_trigger => Ok(MoveSubjectRef::TriggeringObject),
        SubjectRefAst::AttachedHost(_) if has_source => Ok(MoveSubjectRef::AttachedHost),
        SubjectRefAst::Anaphora(_) => {
            if had_perm_target {
                Ok(MoveSubjectRef::TargetObject)
            } else if last_instruction_created_tokens(&builder.instructions) {
                Ok(MoveSubjectRef::CreatedTokens)
            } else if in_trigger && !self_pronoun_valid {
                Ok(MoveSubjectRef::TriggeringObject)
            } else {
                Ok(MoveSubjectRef::LastMoved)
            }
        }
        _ => Err("oracle:unsupported-fight"),
    }
}

pub(super) fn extract_resolving_player_from_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<ResolvingPlayer, &'static str> {
    match subj {
        SubjectRefAst::Player(p) => lower_resolving_player_in_context(*p, builder, in_trigger),
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SINGLE_OPPONENT
            } else {
                BodyTarget::SINGLE_PLAYER
            })?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        _ => Err("oracle:unsupported-exchange"),
    }
}

pub(super) fn parse_exchange_zone_from_subject(
    subj: &SubjectRefAst,
    player: ResolvingPlayer,
) -> Option<spellwake_engine::toolbox::QueryZone> {
    use spellwake_engine::toolbox::{PlayerSelector, QueryZone};
    let ps = || Box::new(PlayerSelector::Resolving(player));
    match subj {
        SubjectRefAst::Anaphora(raw) => {
            let low = raw.trim().to_ascii_lowercase();
            if low.contains("hand") {
                Some(QueryZone::Hand(ps()))
            } else if low.contains("graveyard") {
                Some(QueryZone::Graveyard(ps()))
            } else if low.contains("library") {
                Some(QueryZone::Library(ps()))
            } else if low.contains("exile") {
                Some(QueryZone::Exile)
            } else {
                None
            }
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => match f.zone {
            Some(ZoneFilter::YourHand) => Some(QueryZone::Hand(ps())),
            Some(ZoneFilter::YourGraveyard) => Some(QueryZone::Graveyard(ps())),
            Some(ZoneFilter::YourLibrary) => Some(QueryZone::Library(ps())),
            Some(ZoneFilter::Exile) => Some(QueryZone::Exile),
            _ => None,
        },
        _ => None,
    }
}

pub(super) type ParsedNextTimeSource = (Option<ObjectFilter>, bool, bool, bool);

pub(super) fn parse_next_time_source_with_target(
    src_raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<ParsedNextTimeSource, &'static str>> {
    let trimmed = src_raw.trim().to_ascii_lowercase();
    if let Some(filter_raw) = trimmed.strip_prefix("target ") {
        let filter_clean = filter_raw.trim().trim_end_matches('.');
        if matches!(filter_clean, "source" | "permanent") {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_PERMANENT)
                    .map(|()| (None, false, false, true)),
            );
        }
        let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            filter_clean,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        ast_f.zone = None;
        let bt = lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
            .ok()?;
        if !is_permanent_body_target(bt) {
            return None;
        }
        return Some(builder.bind_target(bt).map(|()| (None, false, false, true)));
    }
    if matches!(
        trimmed.as_str(),
        "that creature" | "that permanent" | "that source" | "it"
    ) && is_permanent_body_target(builder.target)
    {
        return Some(Ok((None, false, false, true)));
    }
    parse_next_time_damage_source(&trimmed, has_source, vocabulary)
        .map(|(src, chosen_color, chosen_type)| Ok((src, chosen_color, chosen_type, false)))
}

pub(super) fn parse_damage_recipient_phrase_with_target(
    raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<StaticDamageRecipient, &'static str>> {
    let cleaned = raw
        .trim()
        .trim_end_matches('.')
        .strip_prefix("to ")
        .unwrap_or_else(|| raw.trim().trim_end_matches('.'))
        .trim();
    match cleaned {
        "any target" | "target creature, planeswalker, or player" | "target creature or player" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_ANY_TARGET)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target player or planeswalker" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target opponent or planeswalker" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target player" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_PLAYER)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "target opponent" => {
            return Some(
                builder
                    .bind_target(BodyTarget::SINGLE_OPPONENT)
                    .map(|()| StaticDamageRecipient::TargetAny),
            );
        }
        "that creature" | "that permanent" | "it" if is_permanent_body_target(builder.target) => {
            return Some(Ok(StaticDamageRecipient::TargetObject));
        }
        "that player"
            if matches!(
                builder.target,
                BodyTarget::Player { .. }
                    | BodyTarget::AnyTarget(
                        AnyTargetKind::Any
                            | AnyTargetKind::PlayerOrPlaneswalker
                            | AnyTargetKind::OpponentOrPlaneswalker
                    )
            ) =>
        {
            return Some(Ok(StaticDamageRecipient::TargetAny));
        }
        other if other.starts_with("target ") => {
            let filter_raw = other.strip_prefix("target ")?.trim();
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                filter_raw,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let bt =
                lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
                    .ok()?;
            if !is_permanent_body_target(bt) {
                return None;
            }
            return Some(
                builder
                    .bind_target(bt)
                    .map(|()| StaticDamageRecipient::TargetObject),
            );
        }
        _ => {}
    }
    parse_damage_recipient_phrase(cleaned, has_source, vocabulary).map(Ok)
}

pub(super) fn parse_damage_recipient_phrase(
    raw: &str,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    let cleaned = raw
        .trim()
        .trim_end_matches('.')
        .strip_prefix("to ")
        .unwrap_or_else(|| raw.trim().trim_end_matches('.'))
        .trim();
    match cleaned {
        "a permanent or player"
        | "a creature, planeswalker, or player"
        | "a creature or player"
        | "one or more players" => Some(StaticDamageRecipient::Any),
        "an opponent" => Some(StaticDamageRecipient::Opponents),
        "an opponent or a permanent an opponent controls"
        | "an opponent or a permanent they control" => {
            Some(StaticDamageRecipient::OpponentOrOpponentPermanent)
        }
        "you" => Some(StaticDamageRecipient::Controller),
        "you or a permanent you control"
        | "you and/or permanents you control"
        | "you and permanents you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledPermanent)
        }
        "you or a creature you control"
        | "you or creatures you control"
        | "you and/or creatures you control"
        | "you and creatures you control" => {
            Some(StaticDamageRecipient::ControllerOrControlledCreatures)
        }
        "a creature you control" | "creatures you control" | "any creature you control" => {
            Some(StaticDamageRecipient::ControlledCreatures)
        }
        "a creature an opponent controls" | "creatures an opponent controls" => {
            Some(StaticDamageRecipient::OpponentCreatures)
        }
        "a creature" | "any creature" | "creatures" => Some(StaticDamageRecipient::Creatures),
        "a player" | "any player" => Some(StaticDamageRecipient::Players),
        "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment"
        | "this planeswalker"
            if has_source =>
        {
            Some(StaticDamageRecipient::SelfObject)
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent" if has_source => {
            Some(StaticDamageRecipient::AttachedHost)
        }
        other => {
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                other,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
            if obj.has_target_player() {
                return None;
            }
            Some(StaticDamageRecipient::MatchingPermanent(obj))
        }
    }
}

pub(super) fn parse_damage_redirect_target(
    raw: &str,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<DamageRedirectTarget, &'static str>> {
    let cleaned = raw.trim().trim_end_matches('.').trim();
    match cleaned {
        "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment"
        | "this planeswalker"
            if has_source =>
        {
            Some(Ok(DamageRedirectTarget::AbilitySource))
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent" if has_source => {
            Some(Ok(DamageRedirectTarget::AttachedHost))
        }
        "you" => Some(Ok(DamageRedirectTarget::Controller)),
        "its controller"
        | "that creature's controller"
        | "that source's controller"
        | "that spell's controller"
        | "that permanent's controller" => Some(Ok(DamageRedirectTarget::DamageSourceController)),
        "any target" | "target creature, planeswalker, or player" | "target creature or player" => {
            Some(
                builder
                    .bind_target(BodyTarget::SINGLE_ANY_TARGET)
                    .map(|()| DamageRedirectTarget::TargetAny),
            )
        }
        "target player or planeswalker" => Some(
            builder
                .bind_target(BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target opponent or planeswalker" => Some(
            builder
                .bind_target(BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target player" => Some(
            builder
                .bind_target(BodyTarget::SINGLE_PLAYER)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "target opponent" => Some(
            builder
                .bind_target(BodyTarget::SINGLE_OPPONENT)
                .map(|()| DamageRedirectTarget::TargetAny),
        ),
        "that creature" | "that permanent" | "it" if is_permanent_body_target(builder.target) => {
            Some(Ok(DamageRedirectTarget::TargetObject))
        }
        other => {
            let filter_raw = other.strip_prefix("target ")?;
            let mut ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
                filter_raw,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            ast_f.zone = None;
            let bt =
                lower_battlefield_body_target_in_context(&ast_f, builder, in_trigger, vocabulary)
                    .ok()?;
            if !is_permanent_body_target(bt) {
                return None;
            }
            Some(
                builder
                    .bind_target(bt)
                    .map(|()| DamageRedirectTarget::TargetObject),
            )
        }
    }
}

pub(super) fn lower_redirect_from_subject_with_target(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticDamageRecipient, &'static str> {
    match subj {
        SubjectRefAst::AnyTarget => {
            builder.bind_target(BodyTarget::SINGLE_ANY_TARGET)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER
            } else {
                BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER
            })?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SINGLE_OPPONENT
            } else {
                BodyTarget::SINGLE_PLAYER
            })?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
            builder.bind_target(BodyTarget::SINGLE_PLAYER)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
            builder.bind_target(BodyTarget::SINGLE_OPPONENT)?;
            Ok(StaticDamageRecipient::TargetAny)
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt =
                lower_battlefield_body_target_in_context(filter, builder, in_trigger, vocabulary)?;
            if !is_permanent_body_target(bt) {
                return Err("oracle:unsupported-redirect-damage");
            }
            builder.bind_target(bt)?;
            Ok(StaticDamageRecipient::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            Ok(StaticDamageRecipient::TargetObject)
        }
        other => lower_redirect_from_subject(other, has_source, vocabulary)
            .ok_or("oracle:unsupported-redirect-damage"),
    }
}

pub(super) fn lower_prevent_recipient_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticDamageRecipient, &'static str> {
    lower_redirect_from_subject_with_target(subj, builder, has_source, in_trigger, vocabulary)
        .map_err(|_| "oracle:unsupported-prevent-damage")
}

pub(super) fn lower_redirect_from_subject(
    subj: &SubjectRefAst,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticDamageRecipient> {
    match subj {
        SubjectRefAst::Player(PlayerRefAst::You) => Some(StaticDamageRecipient::Controller),
        SubjectRefAst::Player(PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent) => {
            Some(StaticDamageRecipient::Opponents)
        }
        SubjectRefAst::Player(PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer) => {
            Some(StaticDamageRecipient::Players)
        }
        SubjectRefAst::SelfObject if has_source => Some(StaticDamageRecipient::SelfObject),
        SubjectRefAst::AttachedHost(_) if has_source => Some(StaticDamageRecipient::AttachedHost),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            if f.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                match f.controller {
                    None => return Some(StaticDamageRecipient::Creatures),
                    Some(ControllerRelation::You) => {
                        return Some(StaticDamageRecipient::ControlledCreatures);
                    }
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        return Some(StaticDamageRecipient::OpponentCreatures);
                    }
                    _ => {}
                }
            }
            let obj = lower_object_filter(f, vocabulary).ok()?;
            if obj.has_target_player() {
                return None;
            }
            Some(StaticDamageRecipient::MatchingPermanent(obj))
        }
        SubjectRefAst::Compound(items) if items.len() == 2 => {
            if matches!(items[0], SubjectRefAst::Player(PlayerRefAst::You))
                && let SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) =
                    &items[1]
                && f.zone.is_none()
                && f.controller == Some(ControllerRelation::You)
            {
                if f.any_types == AstCardTypeMask::CREATURE {
                    return Some(StaticDamageRecipient::ControllerOrControlledCreatures);
                }
                if f.any_permanent || f.any_types.is_empty() {
                    return Some(StaticDamageRecipient::ControllerOrControlledPermanent);
                }
            }
            None
        }
        _ => None,
    }
}

pub(super) fn lower_redirect_to_subject(
    subj: &SubjectRefAst,
    builder: &mut BodyBuilder,
    has_source: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<DamageRedirectTarget, &'static str> {
    match subj {
        SubjectRefAst::SelfObject if has_source => Ok(DamageRedirectTarget::AbilitySource),
        SubjectRefAst::AttachedHost(_) if has_source => Ok(DamageRedirectTarget::AttachedHost),
        SubjectRefAst::Player(PlayerRefAst::You) => Ok(DamageRedirectTarget::Controller),
        SubjectRefAst::Player(PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController) => {
            Ok(DamageRedirectTarget::DamageSourceController)
        }
        SubjectRefAst::AnyTarget => {
            builder.bind_target(BodyTarget::SINGLE_ANY_TARGET)?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER
            } else {
                BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER
            })?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => {
            builder.bind_target(if *opponent_only {
                BodyTarget::SINGLE_OPPONENT
            } else {
                BodyTarget::SINGLE_PLAYER
            })?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
            builder.bind_target(BodyTarget::SINGLE_PLAYER)?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
            builder.bind_target(BodyTarget::SINGLE_OPPONENT)?;
            Ok(DamageRedirectTarget::TargetAny)
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            if !is_permanent_body_target(bt) {
                return Err("oracle:unsupported-redirect-damage");
            }
            builder.bind_target(bt)?;
            Ok(DamageRedirectTarget::TargetObject)
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            Ok(DamageRedirectTarget::TargetObject)
        }
        _ => Err("oracle:unsupported-redirect-damage"),
    }
}

pub(super) fn extract_redirect_destination_phrase(rep_norm: &str) -> Option<&str> {
    let cleaned = rep_norm
        .strip_prefix("instead, ")
        .or_else(|| rep_norm.strip_prefix("instead "))
        .unwrap_or(rep_norm)
        .trim();
    for prefix in [
        "that creature deals that combat damage to ",
        "that source deals that combat damage to ",
        "that permanent deals that combat damage to ",
        "~ deals that combat damage to ",
        "it deals that combat damage to ",
        "that creature deals that damage to ",
        "that source deals that damage to ",
        "that spell deals that damage to ",
        "that permanent deals that damage to ",
        "~ deals that damage to ",
        "it deals that damage to ",
        "that combat damage is dealt to ",
        "all that combat damage is dealt to ",
        "all of that combat damage is dealt to ",
        "all combat damage is dealt to ",
        "all that damage is dealt to ",
        "all of that damage is dealt to ",
        "all damage is dealt to ",
        "that damage is dealt to ",
        "damage is dealt to ",
        "it's dealt to ",
        "it is dealt to ",
        "you may have that combat damage dealt to ",
        "you may have that combat damage be dealt to ",
        "you may have that damage dealt to ",
        "you may have that damage be dealt to ",
        "you may have it dealt to ",
        "deal that combat damage to ",
        "deal that damage to ",
        "is dealt to ",
        "are dealt to ",
        "be dealt to ",
        "dealt to ",
    ] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }
    None
}

pub(super) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn split_dealt_from_and_source(phrase: &str) -> Option<(&str, Option<&str>)> {
    let p = phrase.trim();
    if let Some(rest) = p
        .strip_prefix("be dealt by ")
        .or_else(|| p.strip_prefix("dealt by "))
        .or_else(|| p.strip_prefix("be dealt from "))
        .or_else(|| p.strip_prefix("dealt from "))
        && let Some((s_part, f_part)) = rest.split_once(" to ")
    {
        return Some((f_part.trim(), Some(s_part.trim())));
    }
    let after_dealt = p
        .strip_prefix("be dealt to ")
        .or_else(|| p.strip_prefix("dealt to "))?;
    if let Some((f_part, s_part)) = after_dealt
        .split_once(" by ")
        .or_else(|| after_dealt.split_once(" from "))
    {
        Some((f_part.trim(), Some(s_part.trim())))
    } else {
        Some((after_dealt.trim(), None))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_lower_redirect_next_damage(
    subj_low: &str,
    ev_low: &str,
    rep_low: &str,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    has_source: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<(), &'static str>> {
    let rep_no_dot = rep_low.trim_end_matches('.').trim();
    let rep_no_instead = rep_no_dot
        .strip_suffix(" instead")
        .unwrap_or(rep_no_dot)
        .trim();
    let eff_ev_owned: String;
    let eff_rep_owned: String;
    let (ev_low, rep_norm) = if ev_low == "would_event" {
        let (ev_split, dest_split) = rep_no_instead
            .split_once(" is dealt to ")
            .or_else(|| rep_no_instead.split_once(" are dealt to "))?;
        if ev_split.contains("of your choice") || ev_split.contains("you choose") {
            return None;
        }
        eff_ev_owned = ev_split.trim().to_owned();
        eff_rep_owned =
            collapse_ws(&format!("is dealt to {}", dest_split.trim()).replace(" this turn", ""));
        (eff_ev_owned.as_str(), eff_rep_owned.trim())
    } else {
        eff_ev_owned = String::new();
        eff_rep_owned = collapse_ws(&rep_no_instead.replace(" this turn", ""));
        let _ = &eff_ev_owned;
        (ev_low, eff_rep_owned.trim())
    };
    let to_raw = extract_redirect_destination_phrase(rep_norm)?;

    let check_single_target_slot = |source_is_target: bool,
                                    from: StaticDamageRecipient,
                                    to: DamageRedirectTarget,
                                    allow_same_source_and_to: bool|
     -> bool {
        let from_uses = matches!(
            from,
            StaticDamageRecipient::TargetObject | StaticDamageRecipient::TargetAny
        );
        let to_uses = matches!(
            to,
            DamageRedirectTarget::TargetObject | DamageRedirectTarget::TargetAny
        );
        if allow_same_source_and_to
            && source_is_target
            && !from_uses
            && to == DamageRedirectTarget::TargetObject
        {
            return true;
        }
        usize::from(source_is_target) + usize::from(from_uses) + usize::from(to_uses) <= 1
    };

    if ev_low == "next_time_this_turn" {
        if let Some((combat_only, after_would)) = subj_low
            .strip_prefix("combat damage would ")
            .map(|r| (true, r))
            .or_else(|| subj_low.strip_prefix("damage would ").map(|r| (false, r)))
        {
            let after_norm = collapse_ws(
                &after_would
                    .replace(" this turn", "")
                    .replace(" this combat", ""),
            );
            let (from_part, s_part_opt) = split_dealt_from_and_source(&after_norm)?;
            let mut trial = builder.clone();
            let (source, source_is_target) = if let Some(s_part) = s_part_opt {
                if s_part.contains("of your choice") || s_part.contains("you choose") {
                    return None;
                }
                let (src, chosen_color, chosen_type, is_tgt) =
                    match parse_next_time_source_with_target(
                        s_part, &mut trial, has_source, in_trigger, vocabulary,
                    )? {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    };
                if chosen_color || chosen_type {
                    return None;
                }
                (src, is_tgt)
            } else {
                (None, false)
            };
            let from = match parse_damage_recipient_phrase_with_target(
                from_part, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let to = match parse_damage_redirect_target(
                to_raw, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if !check_single_target_slot(source_is_target, from, to, false) {
                return None;
            }
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    TemporaryRuleKind::RedirectNextDamage {
                        source,
                        source_is_target,
                        from,
                        to,
                        combat_only,
                        amount: None,
                        one_shot: true,
                    },
                    ResolvedDuration::UntilEndOfTurn,
                ),
            );
            *builder = trial;
            return Some(Ok(()));
        }
        let (src_raw, dst_raw) = subj_low.split_once(" would deal ")?;
        if !dst_raw.contains("damage") {
            return None;
        }
        let combat_only = dst_raw.contains("combat damage");
        let after_dmg = dst_raw
            .split_once("combat damage")
            .or_else(|| dst_raw.split_once("damage"))
            .map_or("", |(_, r)| r.trim());
        let mut trial = builder.clone();
        if to_raw == "itself" {
            let (source, chosen_color, chosen_type, source_is_target) =
                match parse_next_time_source_with_target(
                    src_raw, &mut trial, has_source, in_trigger, vocabulary,
                )? {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            if chosen_color || chosen_type || !source_is_target {
                return None;
            }
            let from = if after_dmg.is_empty() {
                StaticDamageRecipient::Any
            } else {
                match parse_damage_recipient_phrase_with_target(
                    after_dmg, &mut trial, has_source, in_trigger, vocabulary,
                )? {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                }
            };
            if !check_single_target_slot(true, from, DamageRedirectTarget::TargetObject, true) {
                return None;
            }
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    TemporaryRuleKind::RedirectNextDamage {
                        source,
                        source_is_target: true,
                        from,
                        to: DamageRedirectTarget::TargetObject,
                        combat_only,
                        amount: None,
                        one_shot: true,
                    },
                    ResolvedDuration::UntilEndOfTurn,
                ),
            );
            *builder = trial;
            return Some(Ok(()));
        }
        let (source, chosen_color, chosen_type, source_is_target) = if src_raw.trim() == "it"
            && self_pronoun_valid
            && trial.target == BodyTarget::None
            && !last_instruction_created_tokens(&trial.instructions)
            && !last_instruction_moved_object(&trial.instructions)
        {
            (
                Some(ObjectFilter::Clause(ObjectClause {
                    source_relation: SourceRelation::SelfObject,
                    ..ObjectClause::any()
                })),
                false,
                false,
                false,
            )
        } else {
            match parse_next_time_source_with_target(
                src_raw, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            }
        };
        if chosen_color || chosen_type {
            return None;
        }
        let from = if after_dmg.is_empty() {
            StaticDamageRecipient::Any
        } else {
            match parse_damage_recipient_phrase_with_target(
                after_dmg, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            }
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from,
                    to,
                    combat_only,
                    amount: None,
                    one_shot: true,
                },
                ResolvedDuration::UntilEndOfTurn,
            ),
        );
        *builder = trial;
        return Some(Ok(()));
    }

    let subj_no_dur = collapse_ws(
        &subj_low
            .trim()
            .replace(" this turn", "")
            .replace(" this combat", ""),
    );
    let subj_clean = subj_no_dur
        .trim()
        .strip_suffix(" that")
        .or_else(|| subj_no_dur.trim().strip_suffix(" which"))
        .unwrap_or(subj_no_dur.trim())
        .trim();
    let ev_no_would = ev_low
        .trim()
        .trim_end_matches('.')
        .strip_prefix("would ")
        .unwrap_or(ev_low.trim().trim_end_matches('.'))
        .trim();
    let ev_norm_owned = collapse_ws(
        &ev_no_would
            .replace(" this turn", "")
            .replace(" this combat", ""),
    );
    let ev_norm = ev_norm_owned.trim();

    if let Some((after_next, fallback_ev)) = subj_clean
        .strip_prefix("the next ")
        .map(|rest| (rest, ev_norm))
        .or_else(|| ev_norm.strip_prefix("the next ").map(|rest| (rest, "")))
        && let Some((amt_tok, kind_rest)) = after_next.split_once(' ')
        && kind_rest.contains("damage")
    {
        let is_x = amt_tok == "x";
        let lit_n = if is_x {
            None
        } else {
            parse_small_number_word(amt_tok).filter(|&n| n > 0)
        };
        if !is_x && lit_n.is_none() {
            return None;
        }
        let combat_only = kind_rest.contains("combat damage");
        let kind_norm = collapse_ws(&kind_rest.replace(" this turn", ""));
        let fb_norm = collapse_ws(&fallback_ev.replace(" this turn", ""));
        let mut trial = builder.clone();
        let (from_part, source, source_is_target) =
            if let Some((before_would, dst_part)) = kind_norm.split_once(" would deal to ") {
                let src_raw = before_would
                    .split_once(" that ")
                    .map_or("", |(_, s)| s.trim());
                let (src, chosen_color, chosen_type, is_tgt) = if src_raw.is_empty() {
                    (None, false, false, false)
                } else {
                    match parse_next_time_source_with_target(
                        src_raw, &mut trial, has_source, in_trigger, vocabulary,
                    )? {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                };
                if chosen_color || chosen_type {
                    return None;
                }
                (dst_part.trim().to_owned(), src, is_tgt)
            } else if let Some((before_would, dst_part)) = fb_norm.split_once(" would deal to ") {
                let src_raw = before_would
                    .split_once(" that ")
                    .map_or(before_would.trim(), |(_, s)| s.trim());
                let (src, chosen_color, chosen_type, is_tgt) = if src_raw.is_empty() {
                    (None, false, false, false)
                } else {
                    match parse_next_time_source_with_target(
                        src_raw, &mut trial, has_source, in_trigger, vocabulary,
                    )? {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                };
                if chosen_color || chosen_type {
                    return None;
                }
                (dst_part.trim().to_owned(), src, is_tgt)
            } else if let Some((f_part, s_part_opt)) = split_dealt_from_and_source(&fb_norm)
                .or_else(|| {
                    kind_norm
                        .find("be dealt ")
                        .or_else(|| kind_norm.find("dealt "))
                        .and_then(|idx| split_dealt_from_and_source(&kind_norm[idx..]))
                })
            {
                if let Some(s_part) = s_part_opt {
                    let (src, chosen_color, chosen_type, is_tgt) =
                        match parse_next_time_source_with_target(
                            s_part, &mut trial, has_source, in_trigger, vocabulary,
                        )? {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                    if chosen_color || chosen_type {
                        return None;
                    }
                    (f_part.to_owned(), src, is_tgt)
                } else {
                    (f_part.to_owned(), None, false)
                }
            } else {
                return None;
            };
        let from = match parse_damage_recipient_phrase_with_target(
            &from_part, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        let amount_expr = if let Some(n) = lit_n {
            ValueExpr::Literal(i64::from(n).into())
        } else {
            match bound_x {
                Some(expr) => {
                    match lower_value_expr_in_context(expr, &trial, in_trigger, vocabulary) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    }
                }
                None => ValueExpr::chosen_x(),
            }
        };
        let rule = TemporaryRuleKind::RedirectNextDamage {
            source,
            source_is_target,
            from,
            to,
            combat_only,
            amount: Some(Box::new(amount_expr)),
            one_shot: false,
        };
        trial.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                rule,
                ResolvedDuration::UntilEndOfTurn,
            ),
        );
        *builder = trial;
        return Some(Ok(()));
    }

    if matches!(
        subj_clean,
        "damage"
            | "all damage"
            | "any damage"
            | "the damage"
            | "combat damage"
            | "all combat damage"
            | "any combat damage"
            | "the combat damage"
    ) && let Some((from_part, s_part_opt)) = split_dealt_from_and_source(ev_norm)
    {
        let combat_only = subj_clean.contains("combat");
        let mut trial = builder.clone();
        let (source, source_is_target) = if let Some(s_part) = s_part_opt {
            let (src, chosen_color, chosen_type, is_tgt) = match parse_next_time_source_with_target(
                s_part, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if chosen_color || chosen_type {
                return None;
            }
            (src, is_tgt)
        } else {
            (None, false)
        };
        let from = match parse_damage_recipient_phrase_with_target(
            from_part, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from,
                    to,
                    combat_only,
                    amount: None,
                    one_shot: false,
                },
                ResolvedDuration::UntilEndOfTurn,
            ),
        );
        *builder = trial;
        return Some(Ok(()));
    }

    if let Some((combat_only, from_raw)) = ev_norm
        .strip_prefix("deal combat damage to ")
        .map(|r| (true, r))
        .or_else(|| ev_norm.strip_prefix("deal damage to ").map(|r| (false, r)))
    {
        let mut trial = builder.clone();
        let (source, chosen_color, chosen_type, source_is_target) =
            match parse_next_time_source_with_target(
                subj_clean, &mut trial, has_source, in_trigger, vocabulary,
            )? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
        if chosen_color || chosen_type {
            return None;
        }
        let from = match parse_damage_recipient_phrase_with_target(
            from_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let to = match parse_damage_redirect_target(
            to_raw, &mut trial, has_source, in_trigger, vocabulary,
        )? {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !check_single_target_slot(source_is_target, from, to, false) {
            return None;
        }
        trial.instructions.push(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from,
                    to,
                    combat_only,
                    amount: None,
                    one_shot: false,
                },
                ResolvedDuration::UntilEndOfTurn,
            ),
        );
        *builder = trial;
        return Some(Ok(()));
    }

    None
}

pub(super) fn lower_target_one_of_damage_body_target(
    options: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    let mut has_any_player = false;
    let mut has_opponent = false;
    let mut has_creature = false;
    let mut has_planeswalker = false;
    let mut perm_clauses: Vec<ObjectClause> = Vec::new();
    let mut has_non_perm_special = false;
    for opt in options {
        match opt {
            SubjectRefAst::TargetPlayer {
                opponent_only: false,
                ..
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                has_any_player = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayer {
                opponent_only: true,
                ..
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                has_opponent = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayerOrPlaneswalker { opponent_only } => {
                if *opponent_only {
                    has_opponent = true;
                } else {
                    has_any_player = true;
                }
                has_planeswalker = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::AnyTarget => {
                return Ok(BodyTarget::SINGLE_ANY_TARGET);
            }
            SubjectRefAst::TargetObject { filter, .. }
                if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                    && !filter.is_ability
                    && !filter.any_spell =>
            {
                let mut clean = filter.as_ref().clone();
                clean.zone = None;
                let obj = lower_object_filter_in_context(&clean, builder, in_trigger, vocabulary)?;
                if obj == ObjectFilter::creature() {
                    has_creature = true;
                } else if obj == ObjectFilter::planeswalker() {
                    has_planeswalker = true;
                }
                match obj {
                    ObjectFilter::Clause(c) => perm_clauses.push(c),
                    ObjectFilter::Either(a, b) => {
                        perm_clauses.push(a);
                        perm_clauses.push(b);
                    }
                }
            }
            _ => return Err("oracle:unsupported-damage-target"),
        }
    }
    let perm_clauses = merge_object_clauses_by_types(perm_clauses);
    if has_non_perm_special {
        let non_special_perm_count = perm_clauses
            .iter()
            .filter(|c| {
                ObjectFilter::Clause(**c) != ObjectFilter::creature()
                    && ObjectFilter::Clause(**c) != ObjectFilter::planeswalker()
            })
            .count();
        if non_special_perm_count > 0 {
            return Err("oracle:unsupported-damage-target");
        }
        return match (has_any_player, has_opponent, has_creature, has_planeswalker) {
            (true, false, true, true) => Ok(BodyTarget::SINGLE_ANY_TARGET),
            (true, false, true, false) => Ok(BodyTarget::SINGLE_CREATURE_OR_PLAYER),
            (true, false, false, true) => Ok(BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER),
            (false, true, false, true) => Ok(BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER),
            (true, false, false, false) => Ok(BodyTarget::SINGLE_PLAYER),
            (false, true, false, false) => Ok(BodyTarget::SINGLE_OPPONENT),
            _ => Err("oracle:unsupported-damage-target"),
        };
    }
    match perm_clauses.len() {
        1 => {
            let f = ObjectFilter::Clause(perm_clauses[0]);
            if f == ObjectFilter::creature() {
                Ok(BodyTarget::SINGLE_CREATURE)
            } else if f == ObjectFilter::planeswalker() {
                Ok(BodyTarget::SINGLE_PLANESWALKER)
            } else {
                Ok(BodyTarget::filtered_permanent(f))
            }
        }
        2 => Ok(BodyTarget::filtered_permanent(ObjectFilter::Either(
            perm_clauses[0],
            perm_clauses[1],
        ))),
        _ => Err("oracle:unsupported-damage-target"),
    }
}

fn has_prior_exile_instruction(ops: &[Instruction]) -> bool {
    ops.iter().any(|op| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op).is_some_and(
            |spec| {
                matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                )
            },
        )
    })
}

fn value_expr_ast_has_bare_object_anaphora(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "it" | "its" | "that creature" | "the creature" | "that permanent" | "the permanent"
                    )
            )
        }
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            value_expr_ast_has_bare_object_anaphora(a) || value_expr_ast_has_bare_object_anaphora(b)
        }
        ValueExprAst::DivConst { expr: inner, .. } => {
            value_expr_ast_has_bare_object_anaphora(inner)
        }
        _ => false,
    }
}

fn value_expr_ast_has_exiled_cost_ref(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } => match subject.as_ref() {
            SubjectRefAst::Anaphora(raw) => {
                let low = raw.trim().to_ascii_lowercase();
                low.contains("exiled card") || low.contains("exiled creature")
            }
            SubjectRefAst::Group(f) => matches!(
                f.predicates.as_slice(),
                [
                    spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
                        spellwake_oracle_compiler::ast::PriorActionRefKind::ExiledThisWay
                    )
                ]
            ),
            _ => false,
        },
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            value_expr_ast_has_exiled_cost_ref(a) || value_expr_ast_has_exiled_cost_ref(b)
        }
        ValueExprAst::DivConst { expr: inner, .. } => value_expr_ast_has_exiled_cost_ref(inner),
        _ => false,
    }
}

fn rewrite_sacrificed_permanent_anaphora(expr: &ValueExprAst) -> Option<ValueExprAst> {
    use spellwake_oracle_compiler::ast::{FilterPredicateAst, PriorActionRefKind};
    match expr {
        ValueExprAst::ObjectStat { subject, stat } => match subject.as_ref() {
            SubjectRefAst::Anaphora(raw) => {
                let low = raw.trim().to_ascii_lowercase();
                if low.contains("sacrificed artifact")
                    || low.contains("sacrificed enchantment")
                    || low.contains("sacrificed land")
                    || low.contains("sacrificed token")
                    || low.contains("sacrificed card")
                    || low.contains("artifact sacrificed")
                    || low.contains("permanent sacrificed")
                    || low.contains("enchantment sacrificed")
                    || low.contains("land sacrificed")
                {
                    return Some(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora(
                            "the sacrificed permanent".to_owned(),
                        )),
                        stat: *stat,
                    });
                }
                if low.contains("creature sacrificed") {
                    return Some(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora(
                            "the sacrificed creature".to_owned(),
                        )),
                        stat: *stat,
                    });
                }
                None
            }
            SubjectRefAst::Group(f) => {
                let simple_prior_action = f.zone.is_none()
                    && f.extra_zones.is_empty()
                    && f.forbidden_types.is_empty()
                    && f.forbidden_subtypes.is_empty()
                    && f.forbidden_colors.is_empty()
                    && f.forbidden_keywords.is_empty()
                    && f.stat_bounds.is_empty()
                    && f.named.is_none()
                    && f.flags.is_empty();
                if !simple_prior_action {
                    return None;
                }
                match f.predicates.as_slice() {
                    [
                        FilterPredicateAst::AffectedByPriorAction(
                            PriorActionRefKind::SacrificedThisWay,
                        ),
                    ] => {
                        let label = if f.any_types == AstCardTypeMask::CREATURE {
                            "the sacrificed creature"
                        } else {
                            "the sacrificed permanent"
                        };
                        Some(ValueExprAst::ObjectStat {
                            subject: Box::new(SubjectRefAst::Anaphora(label.to_owned())),
                            stat: *stat,
                        })
                    }
                    [
                        FilterPredicateAst::AffectedByPriorAction(
                            PriorActionRefKind::ExiledThisWay,
                        ),
                    ] => Some(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora("the exiled card".to_owned())),
                        stat: *stat,
                    }),
                    [
                        FilterPredicateAst::AffectedByPriorAction(
                            PriorActionRefKind::RevealedThisWay,
                        ),
                    ] => Some(ValueExprAst::ObjectStat {
                        subject: Box::new(SubjectRefAst::Anaphora("the revealed card".to_owned())),
                        stat: *stat,
                    }),
                    _ => None,
                }
            }
            _ => None,
        },
        ValueExprAst::Add(a, b) => {
            let ra = rewrite_sacrificed_permanent_anaphora(a);
            let rb = rewrite_sacrificed_permanent_anaphora(b);
            if ra.is_some() || rb.is_some() {
                Some(ValueExprAst::Add(
                    Box::new(ra.unwrap_or_else(|| a.as_ref().clone())),
                    Box::new(rb.unwrap_or_else(|| b.as_ref().clone())),
                ))
            } else {
                None
            }
        }
        ValueExprAst::Sub(a, b) => {
            let ra = rewrite_sacrificed_permanent_anaphora(a);
            let rb = rewrite_sacrificed_permanent_anaphora(b);
            if ra.is_some() || rb.is_some() {
                Some(ValueExprAst::Sub(
                    Box::new(ra.unwrap_or_else(|| a.as_ref().clone())),
                    Box::new(rb.unwrap_or_else(|| b.as_ref().clone())),
                ))
            } else {
                None
            }
        }
        ValueExprAst::Mul(a, b) => {
            let ra = rewrite_sacrificed_permanent_anaphora(a);
            let rb = rewrite_sacrificed_permanent_anaphora(b);
            if ra.is_some() || rb.is_some() {
                Some(ValueExprAst::Mul(
                    Box::new(ra.unwrap_or_else(|| a.as_ref().clone())),
                    Box::new(rb.unwrap_or_else(|| b.as_ref().clone())),
                ))
            } else {
                None
            }
        }
        ValueExprAst::DivConst {
            expr: inner,
            denom,
            rounding,
        } => rewrite_sacrificed_permanent_anaphora(inner).map(|ri| ValueExprAst::DivConst {
            expr: Box::new(ri),
            denom: *denom,
            rounding: *rounding,
        }),
        _ => None,
    }
}

pub(super) fn lower_extra_combat_mod_blocking_restriction(
    rule_body: &str,
) -> Option<BlockingRestriction> {
    let low = rule_body.trim().trim_end_matches('.').trim();
    match low {
        "cant_be_regenerated"
        | "cantberegenerated"
        | "can't be regenerated"
        | "cannot be regenerated" => Some(BlockingRestriction::CannotBeRegenerated),
        "cant_attack_alone" | "cantattackalone" | "can't attack alone" | "cannot attack alone" => {
            Some(BlockingRestriction::CannotAttackAlone)
        }
        "cant_block_alone" | "cantblockalone" | "can't block alone" | "cannot block alone" => {
            Some(BlockingRestriction::CannotBlockAlone)
        }
        "assign_combat_damage_as_though_unblocked"
        | "assigns combat damage as though it weren't blocked"
        | "assign combat damage as though they weren't blocked"
        | "assign their combat damage as though they weren't blocked"
        | "assigns its combat damage as though it weren't blocked"
        | "assign their combat damage this turn as though they weren't blocked"
        | "assigns its combat damage this turn as though it weren't blocked"
        | "may assign its combat damage as though it weren't blocked"
        | "may assign their combat damage as though they weren't blocked"
        | "may assign its combat damage this turn as though it weren't blocked"
        | "may assign their combat damage this turn as though they weren't blocked"
        | "its combat damage as though it weren't blocked"
        | "their combat damage as though they weren't blocked"
        | "combat damage as though it weren't blocked"
        | "combat damage as though they weren't blocked" => {
            Some(BlockingRestriction::AssignDamageAsThoughUnblocked)
        }
        "assigns_damage_by_toughness"
        | "assigns combat damage equal to its toughness rather than its power"
        | "assign combat damage equal to their toughness rather than their power" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        "assigns_no_combat_damage"
        | "assignsnocombatdamage"
        | "assigns no combat damage"
        | "assign no combat damage"
        | "it assigns no combat damage"
        | "they assign no combat damage"
        | "assigns no combat damage this turn"
        | "assign no combat damage this turn"
        | "assigns no combat damage this combat"
        | "assign no combat damage this combat"
        | "no combat damage" => Some(BlockingRestriction::AssignNoCombatDamage),
        "cant_attack_you_or_your_planeswalkers"
        | "cant_attack:you or planeswalkers you control"
        | "cant_attack:you or a planeswalker you control"
        | "cant_attack:you or permanents you control"
        | "can't attack you or planeswalkers you control"
        | "can't attack you or a planeswalker you control"
        | "can't attack you or permanents you control" => {
            Some(BlockingRestriction::CannotAttackSourceController)
        }
        "must_block_all_attackers"
        | "must block all attackers"
        | "blocks each attacking creature if able"
        | "blocks all creatures able to block" => Some(BlockingRestriction::MustBlockAllAttackers),
        "can't be blocked by creatures with greater power"
        | "cannot be blocked by creatures with greater power" => {
            Some(BlockingRestriction::CannotBeBlockedByGreaterPower)
        }
        "can't be blocked by tokens"
        | "cannot be blocked by tokens"
        | "can't be blocked by creature tokens" => {
            Some(BlockingRestriction::CannotBeBlockedByTokens)
        }
        "can't be blocked by creatures with flying"
        | "cannot be blocked by creatures with flying"
        | "can't be blocked by flying" => Some(BlockingRestriction::CannotBeBlockedByFlying),
        "can't be blocked except by creatures with flying or reach"
        | "cannot be blocked except by creatures with flying or reach" => {
            Some(BlockingRestriction::OnlyBlockedByFlyingOrReach)
        }
        "can't be blocked except by artifact creatures"
        | "cannot be blocked except by artifact creatures" => {
            Some(BlockingRestriction::OnlyBlockedByArtifact)
        }
        "can't be blocked by nonartifact creatures"
        | "cannot be blocked by nonartifact creatures" => {
            Some(BlockingRestriction::CannotBeBlockedByNonArtifact)
        }
        "can attack as though it didn't have defender"
        | "can attack as though they didn't have defender" => {
            Some(BlockingRestriction::CanAttackAsThoughNoDefender)
        }
        "can block an additional creature" | "can block an additional creature each combat" => {
            Some(BlockingRestriction::AdditionalBlock)
        }
        "can block any number of creatures" | "can block any number of creatures each combat" => {
            Some(BlockingRestriction::CanBlockAnyNumber)
        }
        "assigns combat damage equal to its toughness"
        | "assign combat damage equal to their toughness" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        _ => {
            if let Some(n_raw) = low
                .strip_prefix("cant_be_blocked_by_more_than:")
                .or_else(|| low.strip_prefix("can't be blocked by more than "))
                .or_else(|| low.strip_prefix("cannot be blocked by more than "))
            {
                let num_tok = n_raw.split_whitespace().next().unwrap_or("");
                let n = parse_small_number_word(num_tok).and_then(|v| u8::try_from(v).ok())?;
                if n > 0 {
                    return Some(BlockingRestriction::CannotBeBlockedByMoreThan(n));
                }
            }
            if let Some(n_raw) = low
                .strip_prefix("cant_block_more_than:")
                .or_else(|| low.strip_prefix("can't block more than "))
                .or_else(|| low.strip_prefix("cannot block more than "))
            {
                let num_tok = n_raw.split_whitespace().next().unwrap_or("");
                let n = parse_small_number_word(num_tok).and_then(|v| u8::try_from(v).ok())?;
                if n > 0 {
                    return Some(BlockingRestriction::CannotBlockMoreThan(n));
                }
            }
            if let Some(rest) = low
                .strip_prefix("can't be blocked by creatures with power ")
                .or_else(|| low.strip_prefix("cannot be blocked by creatures with power "))
            {
                if let Some(num_s) = rest.strip_suffix(" or less")
                    && let Ok(n) = num_s.trim().parse::<u16>()
                {
                    return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(n));
                }
                if let Some(num_s) = rest.strip_suffix(" or greater")
                    && let Ok(n) = num_s.trim().parse::<u16>()
                {
                    return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(n));
                }
            }
            None
        }
    }
}

pub(super) fn parse_must_block_subject(
    raw: &str,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Option<MoveSubjectRef> {
    let mut clean = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    for suffix in [
        " this combat if able",
        " this turn if able",
        " if able",
        " this combat",
        " this turn",
    ] {
        if let Some(s) = clean.strip_suffix(suffix) {
            clean = s.trim().to_owned();
        }
    }
    match clean.as_str() {
        "~" | "this creature" | "this permanent" if self_pronoun_valid || in_trigger => {
            Some(MoveSubjectRef::AbilitySource)
        }
        "enchanted creature" | "equipped creature" | "enchanted permanent"
            if self_pronoun_valid || in_trigger =>
        {
            Some(MoveSubjectRef::AttachedHost)
        }
        "it" | "them" | "that creature" | "those creatures" | "that permanent" | "that token"
        | "those tokens" => {
            if last_instruction_created_tokens(&builder.instructions) {
                Some(MoveSubjectRef::CreatedTokens)
            } else if is_permanent_body_target(builder.target) {
                Some(MoveSubjectRef::TargetObject)
            } else if last_instruction_moved_object(&builder.instructions) {
                Some(MoveSubjectRef::LastMoved)
            } else if in_trigger && (!self_pronoun_valid || !is_self_anaphora_label(&clean)) {
                Some(MoveSubjectRef::TriggeringObject)
            } else if self_pronoun_valid {
                Some(MoveSubjectRef::AbilitySource)
            } else {
                None
            }
        }
        _ => {
            let parsed = spellwake_oracle_compiler::filter::parse_subject_or_target(
                &clean,
                &cached_compiler_vocab(vocabulary),
            )
            .ok()?;
            lower_move_subject_ref(&parsed, builder, vocabulary, in_trigger, self_pronoun_valid)
                .ok()
        }
    }
}

pub(super) fn rewrite_loop_player_filter(
    f: &ObjectFilterAst,
    loop_player: PlayerRefAst,
) -> ObjectFilterAst {
    let mut out = f.clone();
    if loop_player == PlayerRefAst::You {
        if out.controller == Some(ControllerRelation::You) {
            out.controller = Some(ControllerRelation::ThatPlayer);
        }
        if out.owner == Some(ControllerRelation::You) {
            out.owner = Some(ControllerRelation::ThatPlayer);
        }
        out.zone = match out.zone {
            Some(ZoneFilter::YourGraveyard) => Some(ZoneFilter::ThatPlayerGraveyard),
            Some(ZoneFilter::YourHand) => Some(ZoneFilter::ThatPlayerHand),
            Some(ZoneFilter::YourLibrary) => Some(ZoneFilter::ThatPlayerLibrary),
            other => other,
        };
    } else if matches!(
        loop_player,
        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent
    ) && matches!(
        out.controller,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
    ) {
        out.controller = Some(ControllerRelation::ThatPlayer);
    }
    for sb in &mut out.stat_bounds {
        *sb.value = rewrite_loop_player_value_expr(&sb.value, loop_player);
    }
    out
}

pub(super) fn rewrite_loop_player_subject(
    s: &SubjectRefAst,
    loop_player: PlayerRefAst,
) -> SubjectRefAst {
    match s {
        SubjectRefAst::Player(p) if *p == loop_player => {
            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
        }
        SubjectRefAst::All(f) => {
            SubjectRefAst::All(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::Each(f) => {
            SubjectRefAst::Each(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::Group(f) => {
            SubjectRefAst::Group(Box::new(rewrite_loop_player_filter(f, loop_player)))
        }
        SubjectRefAst::TargetObject { filter, count } => SubjectRefAst::TargetObject {
            filter: Box::new(rewrite_loop_player_filter(filter, loop_player)),
            count: *count,
        },
        SubjectRefAst::InZone { subject, zone } => {
            let mapped_zone = if loop_player == PlayerRefAst::You {
                match zone {
                    ZoneFilter::YourGraveyard => ZoneFilter::ThatPlayerGraveyard,
                    ZoneFilter::YourHand => ZoneFilter::ThatPlayerHand,
                    ZoneFilter::YourLibrary => ZoneFilter::ThatPlayerLibrary,
                    other => *other,
                }
            } else {
                *zone
            };
            SubjectRefAst::InZone {
                subject: Box::new(rewrite_loop_player_subject(subject, loop_player)),
                zone: mapped_zone,
            }
        }
        SubjectRefAst::Compound(items) => SubjectRefAst::Compound(
            items
                .iter()
                .map(|it| rewrite_loop_player_subject(it, loop_player))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod wave12_source_prevention_tests {
    use super::*;
    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "test-vocab".into(),
            [0; 32],
            vec![
                "Cleric".into(),
                "Human".into(),
                "Knight".into(),
                "Phantom".into(),
                "Soldier".into(),
                "Spirit".into(),
                "Wizard".into(),
            ],
        )
        .unwrap()
    }
    #[test]
    fn pinned_source_prevention_cards_and_spell_source_residual() {
        let vocabulary = super::super::super::engine_trajectories::vocabulary();
        for (name, type_line, cost, power, toughness, text) in [
            (
                "Deftblade Elite",
                "Creature \u{2014} Human Soldier",
                "{W}",
                Some("1"),
                Some("1"),
                "Provoke (Whenever this creature attacks, you may have target creature defending player controls untap and block it if able.)\n{1}{W}: Prevent all combat damage that would be dealt to and dealt by this creature this turn.",
            ),
            (
                "Ethereal Haze",
                "Instant \u{2014} Arcane",
                "{W}",
                None,
                None,
                "Prevent all damage that would be dealt by creatures this turn.",
            ),
            (
                "Falling Timber",
                "Instant",
                "{2}{G}",
                None,
                None,
                "Kicker\u{2014}Sacrifice a land. (You may sacrifice a land in addition to any other costs as you cast this spell.)\nPrevent all combat damage target creature would deal this turn. If this spell was kicked, prevent all combat damage another target creature would deal this turn.",
            ),
            (
                "Fend Off",
                "Instant",
                "{1}{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target creature this turn.\nCycling {2} ({2}, Discard this card: Draw a card.)",
            ),
            (
                "Gossamer Chains",
                "Enchantment",
                "{W}{W}",
                None,
                None,
                "Return this enchantment to its owner's hand: Prevent all combat damage that would be dealt by target unblocked creature this turn.",
            ),
            (
                "Horn of Deafening",
                "Artifact",
                "{4}",
                None,
                None,
                "{2}, {T}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Ignoble Soldier",
                "Creature \u{2014} Human Soldier",
                "{2}{W}",
                Some("3"),
                Some("1"),
                "Whenever this creature becomes blocked, prevent all combat damage that would be dealt by it this turn.",
            ),
            (
                "Kor Haven",
                "Legendary Land",
                "",
                None,
                None,
                "{T}: Add {C}.\n{1}{W}, {T}: Prevent all combat damage that would be dealt by target attacking creature this turn.",
            ),
            (
                "Lady Evangela",
                "Legendary Creature \u{2014} Human Cleric",
                "{W}{U}{B}",
                Some("1"),
                Some("2"),
                "{W}{B}, {T}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Maze of Ith",
                "Land",
                "",
                None,
                None,
                "{T}: Untap target attacking creature. Prevent all combat damage that would be dealt to and dealt by that creature this turn.",
            ),
            (
                "Resistance Fighter",
                "Creature \u{2014} Human Soldier",
                "{W}",
                Some("1"),
                Some("1"),
                "Sacrifice this creature: Prevent all combat damage target creature would deal this turn.",
            ),
            (
                "Restrain",
                "Instant",
                "{2}{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target attacking creature this turn.\nDraw a card.",
            ),
            (
                "Safeguard",
                "Enchantment",
                "{3}{W}{W}",
                None,
                None,
                "{2}{W}: Prevent all combat damage that would be dealt by target creature this turn.",
            ),
            (
                "Shieldmage Elder",
                "Creature \u{2014} Human Cleric Wizard",
                "{5}{W}",
                Some("2"),
                Some("3"),
                "Tap two untapped Clerics you control: Prevent all damage target creature would deal this turn.\nTap two untapped Wizards you control: Prevent all damage target spell would deal this turn.",
            ),
            (
                "Urborg Phantom",
                "Creature \u{2014} Spirit Minion",
                "{2}{B}",
                Some("3"),
                Some("1"),
                "This creature can't block.\n{U}: Prevent all combat damage that would be dealt to and dealt by this creature this turn.",
            ),
            (
                "Warning",
                "Instant",
                "{W}",
                None,
                None,
                "Prevent all combat damage that would be dealt by target attacking creature this turn.",
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: None,
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(type_line.into()),
                    oracle_text: Some(text.into()),
                    mana_cost: Some(cost.into()),
                    power: power.map(str::to_owned),
                    toughness: toughness.map(str::to_owned),
                }],
                legalities: BTreeMap::new(),
                eligible_formats: vec![],
                required_by_base_formats: true,
                program_evidence: "test",
            };
            // Shieldmage Elder is now qualified: CR 400.7c spell-to-permanent and spell-copy
            // identity propagation is implemented in DamageState::propagate_permanent_spell_identity.
            compile_with_reason(&card, &vocabulary)
                .unwrap_or_else(|reason| panic!("{name}: {reason}"));
        }
    }
    #[test]
    fn source_prevention_rejects_duration_and_anaphora_ambiguity_and_lowers_qualified_specs() {
        let vocab = vocabulary();
        let filter = spellwake_oracle_compiler::filter::parse_object_filter(
            "creatures",
            &cached_compiler_vocab(&vocab),
        )
        .unwrap();
        let target = SubjectRefAst::TargetObject {
            filter: Box::new(filter.clone()),
            count: TargetCountAst::Exact(1),
        };
        // Variant 1 (ongoing duration) and variant 3 (raw untyped anaphora instead of
        // ChosenByPlayer filter predicate) must remain fail-closed.
        for variant in [1usize, 3] {
            let source = match variant {
                1 => SubjectRefAst::Anaphora(
                    "that creature for as long as this remains on the battlefield".into(),
                ),
                3 => SubjectRefAst::Anaphora("a source of your choice".into()),
                _ => unreachable!(),
            };
            let ins = InstructionAst::PreventAllDamageFrom {
                combat_only: true,
                source: Box::new(source),
                recipient: None,
                duration: DurationAst::UntilEndOfTurn,
            };
            assert!(
                lower_combat_damage_instruction(
                    &ins,
                    &mut BodyBuilder::new(),
                    &vocab,
                    &mut LoweringIds::default(),
                    false,
                    false,
                    None
                )
                .is_err(),
                "variant {variant} must stay fail-closed"
            );
        }
        // Variant 0 (explicit Controller recipient) and variant 2 (targeted spell source)
        // now lower to exact qualified `ApplyContinuous` PreventDamage specs.
        for variant in [0usize, 2] {
            let source = if variant == 2 {
                let mut f = filter.clone();
                f.any_spell = true;
                SubjectRefAst::TargetObject {
                    filter: Box::new(f),
                    count: TargetCountAst::Exact(1),
                }
            } else {
                target.clone()
            };
            let ins = InstructionAst::PreventAllDamageFrom {
                combat_only: true,
                source: Box::new(source),
                recipient: if variant == 0 {
                    Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You)))
                } else {
                    None
                },
                duration: DurationAst::UntilEndOfTurn,
            };
            let mut builder = BodyBuilder::new();
            lower_combat_damage_instruction(
                &ins,
                &mut builder,
                &vocab,
                &mut LoweringIds::default(),
                false,
                false,
                None,
            )
            .unwrap_or_else(|e| panic!("variant {variant} should lower: {e}"));
            assert!(
                matches!(
                    builder.instructions.as_slice(),
                    [Instruction::ApplyContinuous(spec)]
                        if matches!(
                            spec.payload,
                            spellwake_engine::toolbox::ContinuousPayload::PreventDamage(_)
                        )
                ),
                "variant {variant} should lower to ApplyContinuous PreventDamage"
            );
        }
    }
}
