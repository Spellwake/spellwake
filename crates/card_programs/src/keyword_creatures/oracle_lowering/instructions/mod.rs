use super::*;
pub(super) use control_misc::lower_loop_player_sub_body;
use spellwake_engine::characteristics::ActivationSuppression;
use spellwake_engine::day_night::DayNight;
use spellwake_engine::program::{DynamicStatAnchor, LinkedExileGroup, StackCopyTarget};
use spellwake_oracle_compiler::ast::ExchangeKind;

mod combat_damage;
mod control_misc;
mod counters_pt;
mod restrictions_statics;
mod search_library;
mod tokens_copies;
mod zone_moves;

use self::combat_damage::*;
use self::control_misc::*;
pub(super) use self::control_misc::{emit_choice_for_player_ref, parse_choice_kind};
use self::counters_pt::*;
use self::restrictions_statics::*;
use self::search_library::*;
use self::tokens_copies::*;
use self::zone_moves::*;

pub(super) fn lower_target_subject(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    match subject {
        SubjectRefAst::AnyTarget => Ok(BodyTarget::SingleAnyTarget),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: false,
        } => Ok(BodyTarget::SinglePlayerOrPlaneswalker),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: true,
        } => Ok(BodyTarget::SingleOpponentOrPlaneswalker),
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => Ok(if *opponent_only {
            BodyTarget::SingleOpponent
        } else {
            BodyTarget::SinglePlayer
        }),
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            if filter.is_ability {
                let mut inner = filter.as_ref().clone();
                inner.is_ability = false;
                inner.any_spell = false;
                if inner.zone == Some(ZoneFilter::Stack) {
                    inner.zone = None;
                }
                return if is_unrestricted_filter(&inner) {
                    Ok(BodyTarget::SingleStackEntry)
                } else {
                    Ok(BodyTarget::SingleStackObject(lower_spell_filter(
                        &inner, vocabulary,
                    )?))
                };
            }
            if filter.any_spell {
                let mut inner = filter.as_ref().clone();
                inner.any_spell = false;
                return Ok(BodyTarget::SingleSpell(lower_spell_filter(
                    &inner, vocabulary,
                )?));
            }
            if let Some(zone) = filter.zone {
                let mut inner = filter.as_ref().clone();
                inner.zone = None;
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::SingleOwnGraveyardCard)
                        } else {
                            Ok(BodyTarget::FilteredOwnGraveyardCard(lower_object_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::AnyGraveyard | ZoneFilter::SingleGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::AnyGraveyardCard)
                        } else if inner.any_types == AstCardTypeMask::CREATURE
                            && filter_only_has(
                                &inner,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            )
                        {
                            Ok(BodyTarget::AnyGraveyardCreature)
                        } else if inner.any_types
                            == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
                            && filter_only_has(
                                &inner,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            )
                        {
                            Ok(BodyTarget::AnyGraveyardCreatureOrPlaneswalker)
                        } else {
                            Ok(BodyTarget::FilteredAnyGraveyardCard {
                                filter: lower_object_filter(&inner, vocabulary)?,
                                opponents_only: false,
                            })
                        }
                    }
                    ZoneFilter::OpponentGraveyard => {
                        let filter = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        Ok(BodyTarget::FilteredAnyGraveyardCard {
                            filter,
                            opponents_only: true,
                        })
                    }
                    ZoneFilter::Stack => {
                        if inner.is_ability {
                            inner.is_ability = false;
                            inner.any_spell = false;
                            if is_unrestricted_filter(&inner) {
                                Ok(BodyTarget::SingleStackEntry)
                            } else {
                                Ok(BodyTarget::SingleStackObject(lower_spell_filter(
                                    &inner, vocabulary,
                                )?))
                            }
                        } else {
                            Ok(BodyTarget::SingleSpell(lower_spell_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::Battlefield => lower_battlefield_body_target(&inner, vocabulary),
                    _ => Err("oracle:unsupported-target-zone"),
                };
            }
            lower_battlefield_body_target(filter, vocabulary)
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => lower_target_one_of_body_target(options, vocabulary),
        _ => Err("oracle:unsupported-target-subject"),
    }
}

fn merge_object_clauses_by_types(clauses: Vec<ObjectClause>) -> Vec<ObjectClause> {
    let mut merged: Vec<ObjectClause> = Vec::with_capacity(clauses.len());
    for c in clauses {
        if merged.contains(&c) {
            continue;
        }
        let mut combined = false;
        if !c.any_types.is_empty() {
            for existing in &mut merged {
                if !existing.any_types.is_empty() {
                    let mut probe = *existing;
                    probe.any_types = c.any_types;
                    if probe == c {
                        existing.any_types = existing.any_types.union(c.any_types);
                        combined = true;
                        break;
                    }
                }
            }
        }
        if !combined {
            merged.push(c);
        }
    }
    merged
}

fn lower_target_one_of_body_target(
    options: &[SubjectRefAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    if options.is_empty() {
        return Err("oracle:unsupported-target-subject");
    }
    let mut has_any_player = false;
    let mut has_opponent = false;
    let mut has_creature = false;
    let mut has_planeswalker = false;
    let mut has_non_perm_special = false;
    let mut perm_clauses: Vec<ObjectClause> = Vec::new();
    for opt in options {
        match opt {
            SubjectRefAst::AnyTarget => {
                has_any_player = true;
                has_creature = true;
                has_planeswalker = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayer {
                opponent_only: false,
                count: TargetCountAst::Exact(1),
            }
            | SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                has_any_player = true;
                has_non_perm_special = true;
            }
            SubjectRefAst::TargetPlayer {
                opponent_only: true,
                count: TargetCountAst::Exact(1),
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
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && !filter.is_ability
                && !filter.any_spell =>
            {
                let mut inner = filter.as_ref().clone();
                inner.zone = None;
                let obj = lower_object_filter(&inner, vocabulary)?;
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
            _ => return Err("oracle:unsupported-target-subject"),
        }
    }
    let perm_clauses = merge_object_clauses_by_types(perm_clauses);
    if has_non_perm_special {
        let has_other_perm = perm_clauses.iter().any(|c| {
            let f = ObjectFilter::Clause(*c);
            f != ObjectFilter::creature() && f != ObjectFilter::planeswalker()
        });
        if has_other_perm {
            return Err("oracle:unsupported-target-subject");
        }
        return match (has_any_player, has_opponent, has_creature, has_planeswalker) {
            (true, false, true, true) => Ok(BodyTarget::SingleAnyTarget),
            (true, false, true, false) => Ok(BodyTarget::SingleCreatureOrPlayer),
            (true, false, false, true) => Ok(BodyTarget::SinglePlayerOrPlaneswalker),
            (false, true, false, true) => Ok(BodyTarget::SingleOpponentOrPlaneswalker),
            (true, false, false, false) => Ok(BodyTarget::SinglePlayer),
            (false, true, false, false) => Ok(BodyTarget::SingleOpponent),
            _ => Err("oracle:unsupported-target-subject"),
        };
    }
    match perm_clauses.len() {
        1 => {
            let f = ObjectFilter::Clause(perm_clauses[0]);
            if f == ObjectFilter::creature() {
                Ok(BodyTarget::SingleCreature)
            } else if f == ObjectFilter::planeswalker() {
                Ok(BodyTarget::SinglePlaneswalker)
            } else if f == ObjectFilter::any() {
                Ok(BodyTarget::SinglePermanent)
            } else {
                Ok(BodyTarget::FilteredPermanent(f))
            }
        }
        2 => Ok(BodyTarget::FilteredPermanent(ObjectFilter::Either(
            perm_clauses[0],
            perm_clauses[1],
        ))),
        _ => Err("oracle:unsupported-target-subject"),
    }
}

fn lower_target_one_of_permanent_body_target(
    options: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    if options.is_empty() {
        return Err("oracle:unsupported-target-subject");
    }
    let mut perm_clauses: Vec<ObjectClause> = Vec::new();
    for opt in options {
        let SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } = opt
        else {
            return Err("oracle:unsupported-target-subject");
        };
        if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            || filter.is_ability
            || filter.any_spell
        {
            return Err("oracle:unsupported-target-subject");
        }
        let mut inner = filter.as_ref().clone();
        inner.zone = None;
        let obj = lower_object_filter_in_context(&inner, builder, in_trigger, vocabulary)?;
        match obj {
            ObjectFilter::Clause(c) => perm_clauses.push(c),
            ObjectFilter::Either(a, b) => {
                perm_clauses.push(a);
                perm_clauses.push(b);
            }
        }
    }
    let perm_clauses = merge_object_clauses_by_types(perm_clauses);
    match perm_clauses.len() {
        1 => {
            let f = ObjectFilter::Clause(perm_clauses[0]);
            if f == ObjectFilter::creature() {
                Ok(BodyTarget::SingleCreature)
            } else if f == ObjectFilter::planeswalker() {
                Ok(BodyTarget::SinglePlaneswalker)
            } else if f == ObjectFilter::any() {
                Ok(BodyTarget::SinglePermanent)
            } else {
                Ok(BodyTarget::FilteredPermanent(f))
            }
        }
        2 => Ok(BodyTarget::FilteredPermanent(ObjectFilter::Either(
            perm_clauses[0],
            perm_clauses[1],
        ))),
        _ => Err("oracle:unsupported-target-subject"),
    }
}

pub(super) fn lower_battlefield_body_target(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    if f.any_permanent
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
    {
        return Ok(BodyTarget::SinglePermanent);
    }
    let mut obj = lower_object_filter(f, vocabulary)?;
    if f.controller == Some(ControllerRelation::DefendingPlayer) {
        rewrite_object_filter_controller(
            &mut obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::DefendingPlayer,
        );
    }
    if obj == ObjectFilter::creature() {
        Ok(BodyTarget::SingleCreature)
    } else if obj == ObjectFilter::planeswalker() {
        Ok(BodyTarget::SinglePlaneswalker)
    } else {
        Ok(BodyTarget::FilteredPermanent(obj))
    }
}

fn rewrite_object_filter_controller(
    obj: &mut ObjectFilter,
    from: ControllerFilter,
    to: ControllerFilter,
) {
    let patch = |c: &mut ObjectClause| {
        if c.controller == from {
            c.controller = to;
        }
    };
    match obj {
        ObjectFilter::Clause(c) => patch(c),
        ObjectFilter::Either(a, b) => {
            patch(a);
            patch(b);
        }
    }
}

fn rewrite_trigger_power_to_target_power(obj: &mut ObjectFilter) {
    let patch = |c: &mut ObjectClause| {
        if let PowerFilter::CompareAnchor { op, anchor } = c.power
            && anchor == DynamicStatAnchor::TriggerSubjectPower
        {
            c.power = PowerFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            };
        }
        if let ToughnessFilter::CompareAnchor { op, anchor } = c.toughness
            && anchor == DynamicStatAnchor::TriggerSubjectPower
        {
            c.toughness = ToughnessFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            };
        }
        if let ManaValueFilter::CompareAnchor { op, anchor } = c.mana_value
            && anchor == DynamicStatAnchor::TriggerSubjectPower
        {
            c.mana_value = ManaValueFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            };
        }
    };
    match obj {
        ObjectFilter::Clause(c) => patch(c),
        ObjectFilter::Either(a, b) => {
            patch(a);
            patch(b);
        }
    }
}

fn filter_uses_target_power(obj: &ObjectFilter) -> bool {
    let check = |c: &ObjectClause| {
        matches!(
            c.power,
            PowerFilter::CompareAnchor {
                anchor: DynamicStatAnchor::TargetPower,
                ..
            }
        ) || matches!(
            c.toughness,
            ToughnessFilter::CompareAnchor {
                anchor: DynamicStatAnchor::TargetPower,
                ..
            }
        ) || matches!(
            c.mana_value,
            ManaValueFilter::CompareAnchor {
                anchor: DynamicStatAnchor::TargetPower,
                ..
            }
        )
    };
    match obj {
        ObjectFilter::Clause(c) => check(c),
        ObjectFilter::Either(a, b) => check(a) || check(b),
    }
}

fn ensure_target_power_consumed(builder: &mut BodyBuilder, obj: &ObjectFilter) {
    if filter_uses_target_power(obj)
        && is_permanent_body_target(builder.target)
        && !builder
            .instructions
            .iter()
            .any(Instruction::uses_mode_target)
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
        prepend_instruction_at(
            &mut builder.instructions,
            0,
            Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
            )),
        );
    }
}

fn lower_battlefield_body_target_in_context(
    f: &ObjectFilterAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    let mut bt = lower_battlefield_body_target(f, vocabulary)?;
    if !in_trigger
        && builder.target == BodyTarget::None
        && builder.instructions.is_empty()
        && f.controller == Some(ControllerRelation::ThatPlayer)
        && let BodyTarget::FilteredPermanent(ref mut obj) = bt
    {
        rewrite_object_filter_controller(
            obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::ActivePlayer,
        );
    }
    Ok(bt)
}

fn lower_object_filter_in_context(
    f: &ObjectFilterAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    let mut obj = lower_object_filter(f, vocabulary)?;
    if f.controller == Some(ControllerRelation::DefendingPlayer) {
        rewrite_object_filter_controller(
            &mut obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::DefendingPlayer,
        );
    } else if !in_trigger
        && builder.target == BodyTarget::None
        && builder.instructions.is_empty()
        && f.controller == Some(ControllerRelation::ThatPlayer)
    {
        rewrite_object_filter_controller(
            &mut obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::ActivePlayer,
        );
    }
    if !in_trigger && is_permanent_body_target(builder.target) {
        rewrite_trigger_power_to_target_power(&mut obj);
    }
    Ok(obj)
}

fn unwrap_in_zone_subject(subj: &SubjectRefAst) -> Option<SubjectRefAst> {
    let SubjectRefAst::InZone { subject, zone } = subj else {
        return None;
    };
    let fold_filter = |f: &ObjectFilterAst| -> Option<ObjectFilterAst> {
        if f.zone.is_none() || f.zone == Some(*zone) {
            let mut cloned = f.clone();
            cloned.zone = Some(*zone);
            Some(cloned)
        } else {
            None
        }
    };
    match subject.as_ref() {
        SubjectRefAst::TargetObject { filter, count } => Some(SubjectRefAst::TargetObject {
            filter: Box::new(fold_filter(filter)?),
            count: *count,
        }),
        SubjectRefAst::Group(f) => Some(SubjectRefAst::Group(Box::new(fold_filter(f)?))),
        SubjectRefAst::All(f) => Some(SubjectRefAst::All(Box::new(fold_filter(f)?))),
        SubjectRefAst::Each(f) => Some(SubjectRefAst::Each(Box::new(fold_filter(f)?))),
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(
                    unwrap_in_zone_subject(&SubjectRefAst::InZone {
                        subject: Box::new(it.clone()),
                        zone: *zone,
                    })
                    .unwrap_or_else(|| it.clone()),
                );
            }
            Some(SubjectRefAst::Compound(out))
        }
        SubjectRefAst::SelfObject
        | SubjectRefAst::Anaphora(_)
        | SubjectRefAst::AttachedHost(_)
        | SubjectRefAst::TriggerSubject => Some(subject.as_ref().clone()),
        _ => None,
    }
}

fn normalize_resolving_spell_lib_placement(
    pl: LibraryPlacement,
    target: BodyTarget,
) -> LibraryPlacement {
    match pl {
        LibraryPlacement::ChoiceTopOrBottom {
            chooser: ResolvingPlayer::TargetOwner,
        } if target == BodyTarget::None => LibraryPlacement::ChoiceTopOrBottom {
            chooser: ResolvingPlayer::Controller,
        },
        other => other,
    }
}

pub(super) fn lower_resolving_player_in_context(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<ResolvingPlayer, &'static str> {
    let has_source = in_trigger || builder.has_source;
    match p {
        PlayerRefAst::You => Ok(ResolvingPlayer::Controller),
        PlayerRefAst::TargetPlayer => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::TargetOpponent => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::ThatPlayer => match builder.target {
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                Ok(ResolvingPlayer::TargetPlayer)
            }
            BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Ok(ResolvingPlayer::TargetSpellController)
            }
            BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_) => Ok(ResolvingPlayer::TargetController),
            BodyTarget::SingleOwnGraveyardCard | BodyTarget::FilteredOwnGraveyardCard(_) => {
                Ok(ResolvingPlayer::Controller)
            }
            BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => Ok(ResolvingPlayer::TargetOwner),
            _ if builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::SelectExtremeLife { .. }
                        )
                ) || matches!(
                    op,
                    Instruction::PromptChoice(spec)
                        if matches!(spec.domain, spellwake_engine::toolbox::PromptDomain::Players { .. })
                )
            }) =>
            {
                Ok(ResolvingPlayer::ChosenPlayer)
            }
            _ if in_trigger => Ok(ResolvingPlayer::UniqueRelatedPlayer),
            _ => Err("oracle:unbound-that-player"),
        },
        PlayerRefAst::ItsController => match builder.target {
            BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Ok(ResolvingPlayer::TargetSpellController)
            }
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            _ if in_trigger => Ok(ResolvingPlayer::TriggerSubjectController),
            BodyTarget::None
                if !builder
                    .instructions
                    .iter()
                    .any(has_prior_non_attached_object_op) =>
            {
                if has_source {
                    Ok(ResolvingPlayer::AttachedHostController)
                } else {
                    Err("oracle:unbound-that-player")
                }
            }
            _ => Err("oracle:unsupported-its-controller"),
        },
        PlayerRefAst::ItsOwner => match builder.target {
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SingleOwnGraveyardCard
            | BodyTarget::FilteredOwnGraveyardCard(_)
            | BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
            | BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetOwner),
            _ if in_trigger => Ok(ResolvingPlayer::TriggerSubjectOwner),
            BodyTarget::None if has_source => Ok(ResolvingPlayer::SourceOwner),
            _ => Err("oracle:unsupported-its-owner"),
        },
        PlayerRefAst::ActivePlayer => Ok(ResolvingPlayer::ActivePlayer),
        PlayerRefAst::EnchantedPlayer => {
            if has_source {
                Ok(ResolvingPlayer::EnchantedPlayer)
            } else {
                Err("oracle:unbound-that-player")
            }
        }
        PlayerRefAst::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
        PlayerRefAst::ThatObjectController => match builder.target {
            BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Ok(ResolvingPlayer::TargetSpellController)
            }
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            _ if in_trigger => Ok(ResolvingPlayer::TriggerSubjectController),
            BodyTarget::None
                if has_source
                    && !builder
                        .instructions
                        .iter()
                        .any(has_prior_non_attached_object_op) =>
            {
                Ok(ResolvingPlayer::AttachedHostController)
            }
            _ => Err("oracle:unbound-that-player"),
        },
        PlayerRefAst::ThatPlayerOrPlaneswalkerController => match builder.target {
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                Ok(ResolvingPlayer::TargetPlayer)
            }
            BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget
            | BodyTarget::SinglePermanent
            | BodyTarget::FilteredPermanent(_) => Ok(ResolvingPlayer::TargetController),
            _ if in_trigger => Ok(ResolvingPlayer::TriggerSubjectController),
            _ => Err("oracle:unsupported-resolving-player"),
        },
        PlayerRefAst::ChosenPlayer => {
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) {
                Ok(ResolvingPlayer::TargetPlayer)
            } else if builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::SelectExtremeLife { .. }
                        )
                ) || matches!(
                    op,
                    Instruction::PromptChoice(spec)
                        if matches!(spec.domain, spellwake_engine::toolbox::PromptDomain::Players { .. })
                )
            }) {
                Ok(ResolvingPlayer::ChosenPlayer)
            } else if has_source {
                Ok(ResolvingPlayer::SourceChosenPlayer)
            } else {
                Err("oracle:unbound-that-player")
            }
        }
        PlayerRefAst::TargetOpponentAtRandom => Ok(ResolvingPlayer::RandomOpponent),
        _ => Err("oracle:unsupported-resolving-player"),
    }
}

// Retained for cross-module signature stability; instructions.rs calls lower_affected_players_in_context.
#[allow(dead_code)]
pub(super) fn lower_affected_players(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<AffectedPlayers, &'static str> {
    lower_affected_players_in_context(p, builder, true)
}

fn is_attached_to_self_filter(f: &ObjectFilter) -> bool {
    matches!(
        f,
        ObjectFilter::Clause(c) if c.source_relation == SourceRelation::AttachedToSelf
    )
}

fn has_prior_non_attached_object_op(op: &Instruction) -> bool {
    match op {
        Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
            if crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                .is_some() =>
        {
            let spec = crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                .unwrap();
            let attached_only = matches!(
                &spec.source,
                spellwake_engine::toolbox::ObjectQuery::AttachedHost
            ) || matches!(
                &spec.source,
                spellwake_engine::toolbox::ObjectQuery::ZoneScan { filter, .. }
                    if is_attached_to_self_filter(filter)
            );
            !attached_only
                && !matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::None
                        | spellwake_engine::toolbox::ZoneDest::RevealOnly
                        | spellwake_engine::toolbox::ZoneDest::LookOnly { .. }
                )
        }
        Instruction::CreateTokens { .. } | Instruction::SearchLibrary { .. } => true,
        Instruction::MutateObject(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::ObjectMutation::TokenAttach(
                    spellwake_engine::toolbox::TokenAttachOp::CreateCopy(_)
                )
            ) =>
        {
            true
        }
        Instruction::MutatePlayer(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::PlayerMutation::RevealTopTwoPiles { .. }
            ) =>
        {
            true
        }
        _ => false,
    }
}

fn try_group_affected_players(p: PlayerRefAst) -> Option<AffectedPlayers> {
    match p {
        PlayerRefAst::EachPlayer => Some(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Some(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Some(AffectedPlayers::EachOtherPlayer),
        _ => None,
    }
}

pub(super) fn push_for_each_player(
    players: AffectedPlayers,
    ops: Vec<Instruction>,
    builder: &mut BodyBuilder,
) {
    builder.instructions.push(Instruction::ForEachPlayerQuery {
        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
        bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
        body: Box::new(ExecutableBody::new(BodyTarget::None, ops)),
    });
}

pub(super) fn lower_affected_players_in_context(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<AffectedPlayers, &'static str> {
    match p {
        PlayerRefAst::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        other => Ok(AffectedPlayers::One(lower_resolving_player_in_context(
            other, builder, in_trigger,
        )?)),
    }
}

pub(super) fn lower_duration(d: &DurationAst) -> Result<ResolvedDuration, &'static str> {
    match d {
        DurationAst::UntilEndOfCombat => Ok(ResolvedDuration::UntilEndOfCombat),
        DurationAst::UntilEndOfTurn => Ok(ResolvedDuration::UntilEndOfTurn),
        DurationAst::UntilYourNextTurn => Ok(ResolvedDuration::UntilControllerNextTurn),
        DurationAst::UntilEndOfNextTurn => Ok(ResolvedDuration::ThroughControllerNextTurn),
        DurationAst::Permanent | DurationAst::Instant => Ok(ResolvedDuration::NoStatedDuration),
        DurationAst::UntilSourceLeaves => Ok(ResolvedDuration::WhileControllingAbilitySource),
        DurationAst::ForAsLongAs(cond) => match cond.as_ref() {
            ConditionAst::Marker(GameMarker::ControlsSelf) => {
                Ok(ResolvedDuration::WhileControllingAbilitySource)
            }
            ConditionAst::Marker(GameMarker::RemainsOnBattlefield) => {
                Ok(ResolvedDuration::WhileAbilitySourceOnBattlefield)
            }
            ConditionAst::Marker(GameMarker::RemainsTapped) => {
                Ok(ResolvedDuration::WhileAbilitySourceTapped)
            }
            ConditionAst::And(a, b)
                if matches!(
                    (a.as_ref(), b.as_ref()),
                    (
                        ConditionAst::Marker(GameMarker::ControlsSelf),
                        ConditionAst::Marker(GameMarker::RemainsTapped)
                    ) | (
                        ConditionAst::Marker(GameMarker::RemainsTapped),
                        ConditionAst::Marker(GameMarker::ControlsSelf)
                    )
                ) =>
            {
                Ok(ResolvedDuration::WhileControllingTappedAbilitySource)
            }
            _ => Err("oracle:unsupported-duration"),
        },
        DurationAst::UntilEndOfPlayersNextTurn(_) => Err("oracle:unsupported-duration"),
    }
}

fn bind_instruction_value_expr_targets(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let bind = |expr: &ValueExprAst, builder: &mut BodyBuilder| {
        bind_value_expr_targets(resolve_x(expr, bound_x), builder)
    };
    match ins {
        InstructionAst::Draw { count, .. }
        | InstructionAst::Mill { count, .. }
        | InstructionAst::Scry { count, .. }
        | InstructionAst::Surveil { count, .. }
        | InstructionAst::GainPlayerCounters { count, .. }
        | InstructionAst::LookTopLibrary { count, .. }
        | InstructionAst::PlayerLookTopLibrary { count, .. }
        | InstructionAst::ExileTopLibrary { count, .. }
        | InstructionAst::ExileWithCounters { count, .. }
        | InstructionAst::Earthbend { count }
        | InstructionAst::PutCounters { count, .. } => bind(count, builder),
        InstructionAst::DiscardUpTo {
            max_count,
            any_number: false,
            all: false,
            ..
        } => bind(max_count, builder),
        InstructionAst::AddQualifiedMana(qm) => {
            if let Some(amt) = &qm.amount {
                bind(amt, builder)
            } else {
                Ok(())
            }
        }
        InstructionAst::RemoveCounters { count: Some(c), .. }
        | InstructionAst::MoveCounters { count: Some(c), .. }
        | InstructionAst::Discard { count: Some(c), .. }
        | InstructionAst::Sacrifice { count: Some(c), .. }
        | InstructionAst::MoveCounted { count: Some(c), .. }
        | InstructionAst::UntapPermanent { count: Some(c), .. }
        | InstructionAst::ReturnZone { count: Some(c), .. }
        | InstructionAst::Copy { count: Some(c), .. } => bind(c, builder),
        InstructionAst::ChangeLife { delta, .. } => bind(delta, builder),
        InstructionAst::DealDamage { amount, .. } => bind(amount, builder),
        InstructionAst::ModifyPT { scale: Some(s), .. } => bind(s, builder),
        InstructionAst::SetDynamicPT {
            power, toughness, ..
        } => {
            if let Some(p) = power {
                bind(p, builder)?;
            }
            if let Some(t) = toughness {
                bind(t, builder)?;
            }
            Ok(())
        }
        InstructionAst::BindX { x_expr, .. } => bind(x_expr, builder),
        InstructionAst::AddMana {
            mana:
                ManaSpecAst::AnyColor { amount, .. }
                | ManaSpecAst::DifferentColors { amount }
                | ManaSpecAst::ScaledSymbol { amount, .. }
                | ManaSpecAst::CombinationOf { amount, .. },
            ..
        } => bind(amount, builder),
        InstructionAst::AddMana { .. } => Ok(()),
        InstructionAst::CreateToken { token, .. } => match token {
            TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. }
            | TokenSpecAst::Creature { count, .. } => bind(count, builder),
            TokenSpecAst::Role { .. } => Ok(()),
        },
        _ => Ok(()),
    }
}

pub(super) fn bind_filter_player_target(
    builder: &mut BodyBuilder,
    filter: &ObjectFilter,
) -> Result<(), &'static str> {
    let mut needs_player = false;
    let mut needs_opponent = false;
    let mut inspect = |clause: ObjectClause| {
        match clause.controller {
            ControllerFilter::TargetPlayer => needs_player = true,
            ControllerFilter::TargetOpponent => needs_opponent = true,
            _ => {}
        }
        match clause.owner {
            ControllerFilter::TargetPlayer => needs_player = true,
            ControllerFilter::TargetOpponent => needs_opponent = true,
            _ => {}
        }
    };
    match *filter {
        ObjectFilter::Clause(c) => inspect(c),
        ObjectFilter::Either(a, b) => {
            inspect(a);
            inspect(b);
        }
    }
    if needs_opponent {
        if builder.target != BodyTarget::SingleOpponent {
            builder.bind_target(BodyTarget::SingleOpponent)?;
        }
    } else if needs_player
        && !matches!(
            builder.target,
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
        )
    {
        builder.bind_target(BodyTarget::SinglePlayer)?;
    }
    Ok(())
}

fn is_permanent_body_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
    )
}

fn is_stack_body_target(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent
    )
}

fn has_prior_token_creation(ops: &[Instruction]) -> bool {
    ops.iter().any(|op| {
        matches!(op, Instruction::CreateTokens { .. })
            || matches!(
                op,
                Instruction::MutateObject(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateCopy(_))
                            | spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateAndAttach { .. })
                            | spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateAuraAttached { .. })
                    )
            )
    })
}

fn value_expr_uses_chosen_x(expr: &ValueExpr) -> bool {
    match expr {
        ValueExpr::EventValue(EventValueKind::ChosenX) => true,
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_uses_chosen_x(a) || value_expr_uses_chosen_x(b)
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            value_expr_uses_chosen_x(inner)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_uses_chosen_x(left)
                || value_expr_uses_chosen_x(right)
                || value_expr_uses_chosen_x(then_value)
                || value_expr_uses_chosen_x(else_value)
        }
        _ => false,
    }
}

fn effect_mana_yield_uses_chosen_x(yield_spec: &EffectManaYield) -> bool {
    match yield_spec {
        EffectManaYield::RetainedUntilCleanup(inner) => effect_mana_yield_uses_chosen_x(inner),
        EffectManaYield::RetainedUntilEndOfCombat { amount, .. }
        | EffectManaYield::Counted { amount, .. }
        | EffectManaYield::RestrictedCounted { amount, .. }
        | EffectManaYield::Split { amount, .. }
        | EffectManaYield::RestrictedChoice { amount, .. }
        | EffectManaYield::Choice { amount, .. }
        | EffectManaYield::LandProducible { amount, .. }
        | EffectManaYield::CommanderIdentity { amount }
        | EffectManaYield::SourceChosenColor { amount }
        | EffectManaYield::DistinctColors { amount } => value_expr_uses_chosen_x(amount),
        EffectManaYield::Fixed(_)
        | EffectManaYield::CombatDamageUntilCleanup(_)
        | EffectManaYield::RestrictedFixed { .. }
        | EffectManaYield::ChoiceBundle { .. } => false,
    }
}

fn body_uses_chosen_x(body: &ExecutableBody) -> bool {
    body.instructions.iter().any(|op| match op {
        Instruction::ZoneOp(spec) => {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op_any_value_expr(
                spec,
                &value_expr_uses_chosen_x,
            )
        }
        Instruction::ForEachPlayerQuery { body, .. } | Instruction::ForEach { body, .. } => {
            body_uses_chosen_x(body)
        }
        Instruction::DrawCards(spec) => value_expr_uses_chosen_x(&spec.amount),
        Instruction::MutateObject(spec) => {
            let sel_x = match &spec.selection {
                spellwake_engine::toolbox::ZoneSelection::Random { count, .. } => {
                    value_expr_uses_chosen_x(count)
                }
                spellwake_engine::toolbox::ZoneSelection::Prompt { min, max, .. } => {
                    value_expr_uses_chosen_x(min) || value_expr_uses_chosen_x(max)
                }
                spellwake_engine::toolbox::ZoneSelection::All => false,
            };
            sel_x
                || match &spec.op {
                    spellwake_engine::toolbox::ObjectMutation::Counters(
                        spellwake_engine::toolbox::CounterOp::Add { amount, .. },
                    )
                    | spellwake_engine::toolbox::ObjectMutation::Counters(
                        spellwake_engine::toolbox::CounterOp::Remove { amount, .. },
                    )
                    | spellwake_engine::toolbox::ObjectMutation::StateMarker(
                        spellwake_engine::toolbox::ObjectStateMarkerOp::BecomeMonstrous { amount },
                    )
                    | spellwake_engine::toolbox::ObjectMutation::Earthbend { amount }
                    | spellwake_engine::toolbox::ObjectMutation::Connive {
                        count: Some(amount),
                    } => value_expr_uses_chosen_x(amount),
                    spellwake_engine::toolbox::ObjectMutation::TokenAttach(
                        spellwake_engine::toolbox::TokenAttachOp::CreateCopy(copy),
                    ) => value_expr_uses_chosen_x(&copy.count),
                    spellwake_engine::toolbox::ObjectMutation::Counters(
                        spellwake_engine::toolbox::CounterOp::Move { amount, .. },
                    ) => amount.as_ref().is_some_and(value_expr_uses_chosen_x),
                    _ => false,
                }
        }
        Instruction::PromptChoice(spec) => {
            let domain_x = match &spec.domain {
                spellwake_engine::toolbox::PromptDomain::Objects {
                    min,
                    max,
                    max_total_power,
                    ..
                } => {
                    value_expr_uses_chosen_x(min)
                        || value_expr_uses_chosen_x(max)
                        || max_total_power
                            .as_deref()
                            .is_some_and(value_expr_uses_chosen_x)
                }
                spellwake_engine::toolbox::PromptDomain::Number { min, max } => {
                    value_expr_uses_chosen_x(min) || value_expr_uses_chosen_x(max)
                }
                spellwake_engine::toolbox::PromptDomain::ManaPayment { extra_generic, .. } => {
                    value_expr_uses_chosen_x(extra_generic)
                }
                _ => false,
            };
            domain_x || spec.option_branches.iter().any(body_uses_chosen_x)
        }
        Instruction::ApplyContinuous(spec) => match &spec.payload {
            spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                power, toughness, ..
            } => value_expr_uses_chosen_x(power) || value_expr_uses_chosen_x(toughness),
            spellwake_engine::toolbox::ContinuousPayload::SkipTurns { count, .. } => {
                value_expr_uses_chosen_x(count)
            }
            _ => false,
        },
        Instruction::AdjustLife(spec) => match &spec.op {
            spellwake_engine::toolbox::LifeOp::Gain(amount)
            | spellwake_engine::toolbox::LifeOp::Lose(amount)
            | spellwake_engine::toolbox::LifeOp::Set(amount)
            | spellwake_engine::toolbox::LifeOp::Pay(amount)
            | spellwake_engine::toolbox::LifeOp::Drain { amount, .. } => {
                value_expr_uses_chosen_x(amount)
            }
            _ => false,
        },
        Instruction::DealDamage(spec) => {
            matches!(
                &spec.amount,
                spellwake_engine::program::DamageAmountSpec::Expr(amount)
                    if value_expr_uses_chosen_x(amount)
            ) || matches!(
                &spec.recipients,
                spellwake_engine::toolbox::DamageTargetQuery::Divided { total, .. }
                    if value_expr_uses_chosen_x(total)
            )
        }
        Instruction::MutatePlayer(spec) => match &spec.op {
            spellwake_engine::toolbox::PlayerMutation::Counter { amount, .. }
            | spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                spellwake_engine::toolbox::LibraryTopAction::Scry(amount),
            )
            | spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                spellwake_engine::toolbox::LibraryTopAction::Surveil(amount),
            )
            | spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                spellwake_engine::toolbox::LibraryTopAction::Mill(amount),
            )
            | spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                spellwake_engine::toolbox::LibraryTopAction::ManifestTop(amount),
            )
            | spellwake_engine::toolbox::PlayerMutation::KeywordAction(
                spellwake_engine::toolbox::PlayerKeywordAction::Incubate { count: amount },
            )
            | spellwake_engine::toolbox::PlayerMutation::KeywordAction(
                spellwake_engine::toolbox::PlayerKeywordAction::Blight { amount },
            )
            | spellwake_engine::toolbox::PlayerMutation::KeywordAction(
                spellwake_engine::toolbox::PlayerKeywordAction::CollectEvidence { minimum: amount },
            )
            | spellwake_engine::toolbox::PlayerMutation::BindResolutionX(amount)
            | spellwake_engine::toolbox::PlayerMutation::RandomEvent(
                spellwake_engine::toolbox::RandomEventOp::RollDie { count: amount, .. },
            )
            | spellwake_engine::toolbox::PlayerMutation::RevealTopTwoPiles {
                count: amount, ..
            } => value_expr_uses_chosen_x(amount),
            spellwake_engine::toolbox::PlayerMutation::AddMana(yield_spec) => {
                effect_mana_yield_uses_chosen_x(yield_spec)
            }
            _ => false,
        },
        Instruction::StackOp(spec) => matches!(
            &spec.op,
            spellwake_engine::toolbox::StackOpKind::Discover { value: amount }
                if value_expr_uses_chosen_x(amount)
        ),
        Instruction::CreateTokens { creations } => creations
            .iter()
            .any(|c| value_expr_uses_chosen_x(&c.amount)),
        Instruction::BranchCondition { condition, .. } => matches!(
            condition.as_ref(),
            spellwake_engine::toolbox::ConditionExpr::Resolution(rc)
                if value_expr_uses_chosen_x(&rc.left) || value_expr_uses_chosen_x(&rc.right)
        ),
        Instruction::RepeatLoop { count, body } => {
            value_expr_uses_chosen_x(count) || body_uses_chosen_x(body.as_ref())
        }
        Instruction::FixedTargetClause(body) => body_uses_chosen_x(body.as_ref()),
        _ => false,
    })
}

fn resolution_condition_is_spell_cast_invariant(cond: &ResolutionCondition) -> bool {
    fn expr_ok(e: &ValueExpr) -> bool {
        match e {
            ValueExpr::Literal(_)
            | ValueExpr::EventValue(
                EventValueKind::ManaSpentToCast
                | EventValueKind::ColorsSpentToCast
                | EventValueKind::ManaTypeSpentToCast(_)
                | EventValueKind::SnowManaSpentToCast
                | EventValueKind::TreasureManaSpentToCast
                | EventValueKind::CreatureManaSpentToCast
                | EventValueKind::CastFromOutsideHand
                | EventValueKind::CastFromExile
                | EventValueKind::GiftPromised
                | EventValueKind::Cleaved
                | EventValueKind::EvidenceCollected
                | EventValueKind::AdditionalCostPaid
                | EventValueKind::ChosenX,
            ) => true,
            ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => expr_ok(a) && expr_ok(b),
            ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
                expr_ok(inner)
            }
            _ => false,
        }
    }
    expr_ok(&cond.left) && expr_ok(&cond.right)
}

fn try_append_fixed_first_target_parts(
    builder: &mut BodyBuilder,
    parts: ResolvedPartsSpec,
    dur: ResolvedDuration,
) -> bool {
    if builder.target != BodyTarget::None || builder.instructions.is_empty() {
        return false;
    }
    let first_is_remembered_perm = matches!(
        builder.instructions.first(),
        Some(Instruction::FixedTargetClause(first_body))
            if is_permanent_body_target(first_body.target)
                && first_body
                    .instructions
                    .iter()
                    .any(|op| {
                        matches!(
                            op,
                            Instruction::MutateObject(spec)
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::ObjectMutation::StateMarker(spellwake_engine::toolbox::ObjectStateMarkerOp::RememberPermanentTarget)
                                )
                        )
                    })
    );
    if !first_is_remembered_perm {
        return false;
    }
    let is_control_flow = |op: &Instruction| {
        matches!(
            op,
            Instruction::BranchCondition { .. }
                | Instruction::Jump { .. }
                | Instruction::OptionalSequence { .. }
        ) || matches!(
            op,
            Instruction::PromptChoice(spec)
                if matches!(
                    spec.domain,
                    spellwake_engine::toolbox::PromptDomain::ManaPayment { .. }
                )
        )
    };
    let rest = &builder.instructions[1..];
    if !rest.iter().any(is_control_flow) {
        if let Some(Instruction::FixedTargetClause(first_body)) = builder.instructions.first_mut() {
            first_body.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            return true;
        }
        return false;
    }
    if !rest.is_empty()
        && !rest[..rest.len() - 1].iter().any(is_control_flow)
        && let Some(Instruction::BranchCondition {
            condition,
            destination: 0,
        }) = rest.last()
        && matches!(
            condition.as_ref(),
            spellwake_engine::toolbox::ConditionExpr::Resolution(rc)
                if resolution_condition_is_spell_cast_invariant(rc)
        )
    {
        let cond_clone = condition.clone();
        if let Some(Instruction::FixedTargetClause(first_body)) = builder.instructions.first_mut() {
            let next_dest = first_body.instructions.len() + 2;
            first_body.instructions.push(Instruction::BranchCondition {
                condition: cond_clone,
                destination: next_dest,
            });
            first_body.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            return true;
        }
    }
    false
}

fn last_instruction_created_tokens(ops: &[Instruction]) -> bool {
    matches!(ops.last(), Some(Instruction::CreateTokens { .. }))
        || matches!(
            ops.last(),
            Some(Instruction::MutateObject(spec))
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateCopy(_))
                        | spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateAndAttach { .. })
                        | spellwake_engine::toolbox::ObjectMutation::TokenAttach(spellwake_engine::toolbox::TokenAttachOp::CreateAuraAttached { .. })
                ) || matches!(
                    spec.source,
                    spellwake_engine::toolbox::ObjectQuery::CreatedTokens
                ) || matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(s)
                        if matches!(s.as_ref(), EffectSubject::CreatedTokens)
                )
        )
        || matches!(
            ops.last(),
            Some(Instruction::ApplyContinuous(spec))
                if matches!(
                    spec.selector.as_ref(),
                    Some(spellwake_engine::toolbox::ObjectQuery::CreatedTokens)
                ) || matches!(
                    spec.selector.as_ref(),
                    Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(s))
                        if matches!(s.as_ref(), EffectSubject::CreatedTokens)
                )
        )
}

pub(super) fn lower_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    bind_instruction_value_expr_targets(ins, builder, bound_x)?;
    match ins {
        InstructionAst::AdditionalCombat(..)
        | InstructionAst::CombatRuleMod { .. }
        | InstructionAst::DealDamage { .. }
        | InstructionAst::DealDamageDivided { .. }
        | InstructionAst::Detain { .. }
        | InstructionAst::Fight { .. }
        | InstructionAst::ForcedBlockByFilter { .. }
        | InstructionAst::PreventAllDamage { .. }
        | InstructionAst::PreventAllDamageFrom { .. }
        | InstructionAst::PreventDamage { .. }
        | InstructionAst::PreventNextSourceDamage { .. }
        | InstructionAst::RedirectDamage { .. }
        | InstructionAst::Regenerate { .. }
        | InstructionAst::RemoveFromCombat { .. }
        | InstructionAst::SetGoaded { .. } => lower_combat_damage_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::ShuffleLibrary { .. } => {
            match lower_zone_moves_instruction(
                ins,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            ) {
                Ok(()) => Ok(()),
                Err("oracle:unbound-that-player") => {
                    lower_shuffle_that_player_fallback(builder, in_trigger)
                }
                Err(err) => Err(err),
            }
        }
        InstructionAst::Destroy { .. }
        | InstructionAst::Discard { .. }
        | InstructionAst::DiscardHand { .. }
        | InstructionAst::DiscardUnless { .. }
        | InstructionAst::DiscardUpTo { .. }
        | InstructionAst::Exile { .. }
        | InstructionAst::ExileGraveyard { .. }
        | InstructionAst::ExileOrRevealTopUntil { .. }
        | InstructionAst::ExileTopLibrary { .. }
        | InstructionAst::ExileUntilLeaves { .. }
        | InstructionAst::Mill { .. }
        | InstructionAst::ModifyPreviousDestroyNoRegen
        | InstructionAst::MoveCounted { .. }
        | InstructionAst::MoveZone { .. }
        | InstructionAst::PhaseOut { .. }
        | InstructionAst::PutInLibraryAtPosition { .. }
        | InstructionAst::ReturnZone { .. }
        | InstructionAst::RevealByPlayer { .. }
        | InstructionAst::RevealCards { .. }
        | InstructionAst::RevealHand { .. }
        | InstructionAst::Sacrifice { .. }
        | InstructionAst::SacrificeAnyNumber { .. }
        | InstructionAst::SacrificeUnlessPay { .. }
        | InstructionAst::ShuffleGraveyardIntoLibrary { .. }
        | InstructionAst::ShuffleIntoLibrary { .. }
        | InstructionAst::WheelShuffleHandAndGraveyard { .. } => lower_zone_moves_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::Amass { .. }
        | InstructionAst::BecomeCopy { .. }
        | InstructionAst::Copy { .. }
        | InstructionAst::CreateEmblem(..)
        | InstructionAst::CreateEmblemFor { .. }
        | InstructionAst::CreateQualifiedToken(..)
        | InstructionAst::CreateToken { .. }
        | InstructionAst::CreateTokenWithAbilities { .. }
        | InstructionAst::Earthbend { .. }
        | InstructionAst::Incubate { .. }
        | InstructionAst::Investigate { .. }
        | InstructionAst::Cloak { .. }
        | InstructionAst::Manifest { .. }
        | InstructionAst::MayChooseNewTargetsForCopy => lower_tokens_copies_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::Adapt { .. }
        | InstructionAst::AnimateOrSetBasePT { .. }
        | InstructionAst::BecomeChosenCharacteristic { .. }
        | InstructionAst::BecomePrepared { .. }
        | InstructionAst::Blight { .. }
        | InstructionAst::Bolster { .. }
        | InstructionAst::Connive { .. }
        | InstructionAst::DistributeCounters { .. }
        | InstructionAst::DoubleStatOrCounters { .. }
        | InstructionAst::Endure { .. }
        | InstructionAst::Explore { .. }
        | InstructionAst::GainPlayerCounters { .. }
        | InstructionAst::GrantKeywordChoice { .. }
        | InstructionAst::GrantKeywords { .. }
        | InstructionAst::KeepExistingTypes(..)
        | InstructionAst::ModifyPT { .. }
        | InstructionAst::ModifyPTChoice { .. }
        | InstructionAst::Monstrosity { .. }
        | InstructionAst::MoveCounter { .. }
        | InstructionAst::MoveCounters { .. }
        | InstructionAst::Proliferate
        | InstructionAst::PutCounters { .. }
        | InstructionAst::RemoveAllAbilities { .. }
        | InstructionAst::RemoveCounters { .. }
        | InstructionAst::RemoveKeywords { .. }
        | InstructionAst::SetDynamicPT { .. }
        | InstructionAst::SetSuspected { .. }
        | InstructionAst::Support { .. }
        | InstructionAst::Suspect { .. }
        | InstructionAst::SwitchPowerToughness { .. }
        | InstructionAst::Transform { .. }
        | InstructionAst::TurnFaceUp { .. }
        | InstructionAst::TurnFaceDown { .. } => lower_counters_pt_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::AddMana { .. }
        | InstructionAst::AddQualifiedMana(..)
        | InstructionAst::Attach { .. }
        | InstructionAst::ChangeTargets { .. }
        | InstructionAst::Counter { .. }
        | InstructionAst::Discover { .. }
        | InstructionAst::Draw { .. }
        | InstructionAst::Exchange { .. }
        | InstructionAst::GainControl { .. }
        | InstructionAst::GrantCastPermission { .. }
        | InstructionAst::GrantModifiedCastPermission(..)
        | InstructionAst::LookTopLibrary { .. }
        | InstructionAst::PayAnyAmountOfMana { .. }
        | InstructionAst::PlayerLookTopLibrary { .. }
        | InstructionAst::RetainManaUntilEndOfTurn
        | InstructionAst::Scry { .. }
        | InstructionAst::SearchLibrary { .. }
        | InstructionAst::SkipNextUntap { .. }
        | InstructionAst::Surveil { .. }
        | InstructionAst::TapOrUntapPermanent { .. }
        | InstructionAst::TapPermanent { .. }
        | InstructionAst::UntapPermanent { .. } => lower_search_library_group(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::AbilityRestriction { .. }
        | InstructionAst::AdditionalLandPlay { .. }
        | InstructionAst::DelayedEventTrigger { .. }
        | InstructionAst::DelayedTrigger { .. }
        | InstructionAst::EndTurn
        | InstructionAst::EntersTappedRule { .. }
        | InstructionAst::ExtraTurn { .. }
        | InstructionAst::ForAsLongAs { .. }
        | InstructionAst::GrantAbilityText { .. }
        | InstructionAst::LoseGame { .. }
        | InstructionAst::OneShotReplacement { .. }
        | InstructionAst::ReflexiveTrigger(..)
        | InstructionAst::ReplaceEffectWith(..)
        | InstructionAst::ScopedDuration { .. }
        | InstructionAst::ScopedDurationText { .. }
        | InstructionAst::SkipNextTurn { .. }
        | InstructionAst::SkipNextTurns { .. }
        | InstructionAst::SkipNextStep { .. }
        | InstructionAst::TemporaryPlayerRestriction { .. }
        | InstructionAst::TemporaryReplacement { .. }
        | InstructionAst::TemporaryStatic { .. }
        | InstructionAst::TemporaryTrigger { .. }
        | InstructionAst::WhenYouNext { .. }
        | InstructionAst::WinGame { .. } => lower_restrictions_statics_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::AbandonSourceScheme
        | InstructionAst::BecomeMonarch { .. }
        | InstructionAst::BindX { .. }
        | InstructionAst::BindXY { .. }
        | InstructionAst::ChangeLife { .. }
        | InstructionAst::Choice(..)
        | InstructionAst::ChooseAndKeep { .. }
        | InstructionAst::ChooseOption(..)
        | InstructionAst::ChooseFromGroup { .. }
        | InstructionAst::PlayerChooseFromGroup { .. }
        | InstructionAst::PlayerChooseOption { .. }
        | InstructionAst::Populate { .. }
        | InstructionAst::ChoosePlayerTarget { .. }
        | InstructionAst::Clash { .. }
        | InstructionAst::CollectEvidence { .. }
        | InstructionAst::Empower { .. }
        | InstructionAst::Exert { .. }
        | InstructionAst::ExileWithCounters { .. }
        | InstructionAst::FlipCoin
        | InstructionAst::ForEachCount { .. }
        | InstructionAst::ForEachMatchingPlayer { .. }
        | InstructionAst::ForEachObject { .. }
        | InstructionAst::ForEachPlayer { .. }
        | InstructionAst::ForEachSubjectPlayer { .. }
        | InstructionAst::Forage
        | InstructionAst::IfPreviousFailed(..)
        | InstructionAst::InteractiveChoice(..)
        | InstructionAst::Learn
        | InstructionAst::LookAtHand { .. }
        | InstructionAst::CloakByPlayer { .. }
        | InstructionAst::ManifestByPlayer { .. }
        | InstructionAst::ManualTabletopAction { .. }
        | InstructionAst::Modal { .. }
        | InstructionAst::OpenAttraction
        | InstructionAst::Optional(..)
        | InstructionAst::OptionalPlayer { .. }
        | InstructionAst::PayCost(..)
        | InstructionAst::PlayerPayCost { .. }
        | InstructionAst::PutSticker { .. }
        | InstructionAst::PutStickerWithCost { .. }
        | InstructionAst::RevealRandomFromHand { .. }
        | InstructionAst::RevealUntil { .. }
        | InstructionAst::RingTemptsYou
        | InstructionAst::RollDiceChooseOne { .. }
        | InstructionAst::RollDie { .. }
        | InstructionAst::SacrificeAllExcept { .. }
        | InstructionAst::SetLifeTotal { .. }
        | InstructionAst::TakeInitiative { .. }
        | InstructionAst::Unless { .. }
        | InstructionAst::UnlockDoor { .. }
        | InstructionAst::VentureIntoDungeon { .. }
        | InstructionAst::Vote { .. } => lower_control_misc_instruction(
            ins,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        _ => Err("oracle:unsupported-instruction"),
    }
}
