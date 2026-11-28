//! Resolution-local choices and once-current controlled-permanent counts.
use super::*;

fn source_zone_matches(
    recorded: crate::program::SourceZoneFilter,
    required: crate::program::SourceZoneFilter,
) -> bool {
    use crate::program::SourceZoneFilter;
    if recorded == required {
        return true;
    }
    match required {
        SourceZoneFilter::HandOrGraveyard => matches!(
            recorded,
            SourceZoneFilter::Hand
                | SourceZoneFilter::Graveyard
                | SourceZoneFilter::OpponentGraveyard
                | SourceZoneFilter::AnyGraveyard
                | SourceZoneFilter::HandOrGraveyard
        ),
        SourceZoneFilter::AnyGraveyard => matches!(
            recorded,
            SourceZoneFilter::Graveyard
                | SourceZoneFilter::OpponentGraveyard
                | SourceZoneFilter::AnyGraveyard
        ),
        SourceZoneFilter::Exile => matches!(
            recorded,
            SourceZoneFilter::Exile
                | SourceZoneFilter::OpponentExile
                | SourceZoneFilter::LinkedExile
        ),
        _ => false,
    }
}

fn supertypes_from_mask(
    mask: crate::program::SupertypeMask,
) -> BTreeSet<crate::program::Supertype> {
    use crate::program::Supertype;
    let mut out = BTreeSet::new();
    if !mask.is_empty() {
        for kind in [
            Supertype::Basic,
            Supertype::Legendary,
            Supertype::Ongoing,
            Supertype::Snow,
            Supertype::World,
        ] {
            let single = BTreeSet::from([kind]);
            if !mask.matches_none(&single) {
                out.insert(kind);
            }
        }
    }
    out
}

fn colors_from_mask(mask: crate::program::ColorMask) -> BTreeSet<crate::characteristics::Color> {
    use crate::characteristics::Color;
    let mut out = BTreeSet::new();
    if !mask.is_empty() {
        for color in [
            Color::White,
            Color::Blue,
            Color::Black,
            Color::Red,
            Color::Green,
        ] {
            if mask.contains(color) {
                out.insert(color);
            }
        }
    }
    out
}

fn matches_compact_clause(
    clause: crate::program::ObjectClause,
    types: crate::program::CardTypeMask,
    colors: crate::program::ColorMask,
    supertypes: crate::program::SupertypeMask,
    subtypes: &BTreeSet<CreatureTypeId>,
    artifact_subtypes: Option<&BTreeSet<crate::program::ArtifactSubtype>>,
) -> bool {
    use crate::program::{ObjectFilterFlags, SingleSubtype, SubtypeFilter, Supertype};
    if clause.exact_name.is_some()
        || clause.required_counter_kind.is_some()
        || clause.forbidden_counter_kind.is_some()
    {
        return false;
    }
    if !clause.any_types.is_empty() && (types.bits() & clause.any_types.bits()) == 0 {
        return false;
    }
    if (types.bits() & clause.required_types.bits()) != clause.required_types.bits() {
        return false;
    }
    if (types.bits() & clause.forbidden_types.bits()) != 0 {
        return false;
    }
    let super_set = supertypes_from_mask(supertypes);
    if !clause.required_supertypes.matches_all(&super_set)
        || !clause.forbidden_supertypes.matches_none(&super_set)
    {
        return false;
    }
    let color_set = colors_from_mask(colors);
    if !clause.color.matches(&color_set) {
        return false;
    }
    let subtype_ok = match clause.subtypes {
        SubtypeFilter::Any => true,
        SubtypeFilter::Has(ct) => subtypes.contains(&ct),
        SubtypeFilter::Lacks(ct) => !subtypes.contains(&ct),
        SubtypeFilter::AnyOf(cts) => cts.iter().any(|ct| subtypes.contains(ct)),
        SubtypeFilter::NoneOf(cts) => cts.iter().all(|ct| !subtypes.contains(ct)),
        SubtypeFilter::Artifact(art) => artifact_subtypes.is_some_and(|set| set.contains(&art)),
        SubtypeFilter::AnyOfList(list) => list.iter().any(|item| match item {
            SingleSubtype::Creature(ct) => subtypes.contains(ct),
            SingleSubtype::Artifact(art) => artifact_subtypes.is_some_and(|set| set.contains(art)),
            _ => false,
        }),
        SubtypeFilter::AllOfList(list) => list.iter().all(|item| match item {
            SingleSubtype::Creature(ct) => subtypes.contains(ct),
            SingleSubtype::Artifact(art) => artifact_subtypes.is_some_and(|set| set.contains(art)),
            _ => false,
        }),
        SubtypeFilter::NoneOfList(list) => list.iter().all(|item| match item {
            SingleSubtype::Creature(ct) => !subtypes.contains(ct),
            SingleSubtype::Artifact(art) => artifact_subtypes.is_some_and(|set| !set.contains(art)),
            _ => false,
        }),
        _ => false,
    };
    if !subtype_ok {
        return false;
    }
    let is_historic =
        types.contains(CardType::Artifact) || super_set.contains(&Supertype::Legendary);
    if clause.flags.contains(ObjectFilterFlags::HISTORIC) && !is_historic {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::NON_HISTORIC) && is_historic {
        return false;
    }
    true
}

fn matches_cast_spell_record(
    filter: crate::program::ObjectFilter,
    rec: &crate::characteristics::CastSpellTurnRecord,
    from_zone: Option<crate::program::SourceZoneFilter>,
) -> bool {
    use crate::program::{ObjectFilter, SourceZoneFilter};
    let zone_ok = match from_zone {
        None => true,
        Some(SourceZoneFilter::Hand) => rec.from_hand,
        Some(
            SourceZoneFilter::Graveyard
            | SourceZoneFilter::AnyGraveyard
            | SourceZoneFilter::OpponentGraveyard,
        ) => rec.from_graveyard,
        Some(SourceZoneFilter::HandOrGraveyard) => rec.from_hand || rec.from_graveyard,
        Some(
            SourceZoneFilter::Exile
            | SourceZoneFilter::OpponentExile
            | SourceZoneFilter::LinkedExile,
        ) => rec.from_exile,
        Some(_) => false,
    };
    if !zone_ok {
        return false;
    }
    let check = |clause| {
        matches_compact_clause(
            clause,
            rec.types,
            rec.colors,
            rec.supertypes,
            &rec.subtypes,
            None,
        )
    };
    match filter {
        ObjectFilter::Clause(c) => check(c),
        ObjectFilter::Either(a, b) => check(a) || check(b),
    }
}

fn matches_zone_move_record(
    filter: crate::program::ObjectFilter,
    rec: &crate::characteristics::ZoneMoveTurnRecord,
    controller: SeatId,
    opponents: &[SeatId],
) -> bool {
    use crate::program::{ControllerFilter, ObjectFilter};
    let check = |clause: crate::program::ObjectClause| {
        let ctrl_ok = match clause.controller {
            ControllerFilter::Any => true,
            ControllerFilter::You => rec.previous_controller == Some(controller),
            ControllerFilter::Opponent => rec
                .previous_controller
                .is_some_and(|seat| opponents.contains(&seat)),
            ControllerFilter::NotYou => rec.previous_controller != Some(controller),
            ControllerFilter::OwnerYou => rec.owner == controller,
            ControllerFilter::OwnerOpponent => opponents.contains(&rec.owner),
            ControllerFilter::Player(seat) => rec.previous_controller == Some(seat),
            _ => true,
        };
        ctrl_ok
            && matches_compact_clause(
                clause,
                rec.types,
                rec.colors,
                rec.supertypes,
                &rec.subtypes,
                Some(&rec.artifact_subtypes),
            )
    };
    match filter {
        ObjectFilter::Clause(c) => check(c),
        ObjectFilter::Either(a, b) => check(a) || check(b),
    }
}

impl EffectWindow {
    pub(super) fn resolve_value_expr(
        &self,
        controller: SeatId,
        expr: &crate::program::ValueExpr,
    ) -> Result<RulesInteger, EffectError> {
        self.resolve_value_expr_at(controller, expr, self.resolution_cursor())
    }
    pub(super) fn resolve_value_expr_at(
        &self,
        controller: SeatId,
        expr: &crate::program::ValueExpr,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<RulesInteger, EffectError> {
        self.resolve_value_expr_with_source(controller, expr, cursor, None)
    }
    pub(super) fn resolve_value_expr_with_source(
        &self,
        controller: SeatId,
        expr: &crate::program::ValueExpr,
        cursor: Option<&ResolutionCursor>,
        explicit_source: Option<ObjectId>,
    ) -> Result<RulesInteger, EffectError> {
        use crate::program::{
            AttachedCountKind, CounterSubjectRef, EventValueKind, MatchObjectRef, ObjectStatKind,
            PlayerStatExpr, SourceStateExpr, SubtypeFilter, ValueExpr, ValueObjectRef, ValueZone,
        };
        let resolve_ability_source = || -> Option<ObjectId> {
            if let Some(id) = explicit_source {
                return Some(id);
            }
            if let Some(binding) = cursor
                .and_then(|c| self.execution_bindings(c.entry))
                .and_then(|b| b.trigger.as_ref())
            {
                return Some(binding.source.object.id());
            }
            match self
                .window
                .resolving()
                .map(crate::stack_window::StackEntry::kind)
            {
                Some(
                    StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source },
                ) => Some(source.id()),
                Some(StackKind::CardSpell(id)) => Some(*id),
                _ => None,
            }
        };
        if let Some(value) =
            self.resolve_toolbox_value_expr(controller, expr, cursor, explicit_source)?
        {
            return Ok(value);
        }
        match expr {
            ValueExpr::Literal(value) => Ok(value.clone()),
            ValueExpr::Scale { expr, factor } => Ok(self
                .resolve_value_expr_with_source(controller, expr, cursor, explicit_source)?
                .times(factor)),
            ValueExpr::DivConst {
                expr,
                divisor,
                round_up,
            } => {
                let value =
                    self.resolve_value_expr_with_source(controller, expr, cursor, explicit_source)?;
                let d = usize::from(divisor.get());
                if d == 2 {
                    Ok(if *round_up {
                        value.nonnegative_half_rounded_up()
                    } else {
                        value.nonnegative_half_rounded_down()
                    })
                } else if let Some(n) = value.to_usize() {
                    let result = if *round_up { n.div_ceil(d) } else { n / d };
                    Ok(RulesInteger::from_usize(result))
                } else {
                    Err(EffectError::InconsistentProgram)
                }
            }
            ValueExpr::Add(a, b) => {
                let lhs =
                    self.resolve_value_expr_with_source(controller, a, cursor, explicit_source)?;
                let rhs =
                    self.resolve_value_expr_with_source(controller, b, cursor, explicit_source)?;
                Ok(lhs.plus(&rhs))
            }
            ValueExpr::Sub(a, b) => {
                let lhs =
                    self.resolve_value_expr_with_source(controller, a, cursor, explicit_source)?;
                let rhs =
                    self.resolve_value_expr_with_source(controller, b, cursor, explicit_source)?;
                Ok(lhs.minus(&rhs))
            }
            ValueExpr::DomainCount => {
                let evaluated = self.evaluated_characteristics()?;
                let mut basic_types = BTreeSet::new();
                for permanent in evaluated.objects.values() {
                    if permanent.zone == ObjectZone::Battlefield
                        && permanent.controller == Some(controller)
                        && permanent.has_type(CardType::Land)
                    {
                        for subtype in &permanent.current_land_subtypes {
                            if let Some(basic) = subtype.basic() {
                                basic_types.insert(basic);
                            }
                        }
                    }
                }
                Ok(RulesInteger::from_usize(basic_types.len()))
            }
            ValueExpr::OpponentsCount => {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                Ok(RulesInteger::from_usize(opponents.len()))
            }
            ValueExpr::Devotion(colors) => {
                let evaluated = self.evaluated_characteristics()?;
                let mut total: usize = 0;
                let matches_mana = |mt: crate::mana::ManaType| match mt {
                    crate::mana::ManaType::White => {
                        colors.contains(crate::characteristics::Color::White)
                    }
                    crate::mana::ManaType::Blue => {
                        colors.contains(crate::characteristics::Color::Blue)
                    }
                    crate::mana::ManaType::Black => {
                        colors.contains(crate::characteristics::Color::Black)
                    }
                    crate::mana::ManaType::Red => {
                        colors.contains(crate::characteristics::Color::Red)
                    }
                    crate::mana::ManaType::Green => {
                        colors.contains(crate::characteristics::Color::Green)
                    }
                    crate::mana::ManaType::Colourless => false,
                };
                for permanent in evaluated.objects.values() {
                    if permanent.zone != ObjectZone::Battlefield
                        || permanent.controller != Some(controller)
                    {
                        continue;
                    }
                    let Some(program) = self.copiable_program(permanent.object) else {
                        continue;
                    };
                    let Some(cost) = program.permanent().and_then(|p| p.casting_cost.as_ref())
                    else {
                        continue;
                    };
                    for (mana_type, count) in &cost.required {
                        if matches_mana(*mana_type) {
                            total = total.saturating_add(count.to_usize().unwrap_or(0));
                        }
                    }
                    for hybrid in &program.hybrid_symbols {
                        let first_match = matches_mana(hybrid.first);
                        let second_match = matches!(
                            hybrid.alternative,
                            crate::mana::HybridAlternative::Mana(mt) if matches_mana(mt)
                        );
                        if first_match || second_match {
                            total = total.saturating_add(1);
                        }
                    }
                }
                Ok(RulesInteger::from_usize(total))
            }
            ValueExpr::BoundInt(_)
            | ValueExpr::BoundCount { .. }
            | ValueExpr::BoundStat { .. }
            | ValueExpr::QueryCount(_)
            | ValueExpr::QueryAggregate { .. }
            | ValueExpr::StateTrackInt(_)
            | ValueExpr::ChoiceHistoryCount { .. } => {
                unreachable!("handled by resolve_toolbox_value_expr")
            }
            ValueExpr::Conditional {
                left,
                comparison,
                right,
                then_value,
                else_value,
            } => {
                let lhs =
                    self.resolve_value_expr_with_source(controller, left, cursor, explicit_source)?;
                let rhs = self.resolve_value_expr_with_source(
                    controller,
                    right,
                    cursor,
                    explicit_source,
                )?;
                if comparison.evaluate(&lhs, &rhs) {
                    self.resolve_value_expr_with_source(
                        controller,
                        then_value,
                        cursor,
                        explicit_source,
                    )
                } else {
                    self.resolve_value_expr_with_source(
                        controller,
                        else_value,
                        cursor,
                        explicit_source,
                    )
                }
            }
            ValueExpr::OpponentMaxCount { zone, filter }
            | ValueExpr::OpponentMinCount { zone, filter } => {
                let is_min = matches!(expr, ValueExpr::OpponentMinCount { .. });
                let mut bound_filter = **filter;
                if let Some(slot) = bound_filter.chosen_type_slot() {
                    let selected = cursor
                        .and_then(|c| c.values.get(&slot))
                        .copied()
                        .ok_or(EffectError::InconsistentProgram)?;
                    if self.creature_vocabulary.label(selected).is_none() {
                        return Err(EffectError::InconsistentProgram);
                    }
                    bound_filter = bound_filter.bind_matching_chosen_slot(slot, selected);
                }
                let ability_source = if bound_filter.excludes_ability_source() {
                    Some(resolve_ability_source().ok_or(EffectError::InconsistentProgram)?)
                } else {
                    resolve_ability_source()
                };
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let mut best: Option<usize> = None;
                for opp in opponents {
                    let count = match *zone {
                        ValueZone::Battlefield => {
                            let scoped = bound_filter.of_player(opp);
                            evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Battlefield
                                        && self.matches_object_filter(
                                            scoped,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count()
                        }
                        ValueZone::Graveyard(_) => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.zone == ObjectZone::Graveyard(opp)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::Hand(_) => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.zone == ObjectZone::Hand(opp)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::Library(_) => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.zone == ObjectZone::Library(opp)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::Exile(_) => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.zone == ObjectZone::Exile
                                    && o.owner == opp
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::AllGraveyards => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                matches!(o.zone, ObjectZone::Graveyard(_))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::AllHands => evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                matches!(o.zone, ObjectZone::Hand(_))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        o,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count(),
                        ValueZone::OpponentsHands => {
                            let opponents = self
                                .combat_rules
                                .opponents(self.window.participants(), controller)?;
                            evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    matches!(o.zone, ObjectZone::Hand(s) if opponents.contains(&s))
                                        && self.matches_object_filter(
                                            bound_filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count()
                        }
                        ValueZone::LinkedExile => {
                            let Some(src) = ability_source.or_else(resolve_ability_source) else {
                                return Ok(0.into());
                            };
                            evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Exile
                                        && (self
                                            .characteristics
                                            .imprinted_cards
                                            .get(&src)
                                            .is_some_and(|list| list.contains(&o.object))
                                            || self
                                                .characteristics
                                                .ability_donors_in_exile
                                                .get(&src)
                                                .is_some_and(|list| list.contains(&o.object))
                                            || self
                                                .characteristics
                                                .linked_exiles
                                                .get(&src)
                                                .is_some_and(|list| list.contains(&o.object)))
                                        && self.matches_object_filter(
                                            bound_filter,
                                            o,
                                            controller,
                                            Some(src),
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count()
                        }
                    };
                    best = Some(match best {
                        Some(prev) => {
                            if is_min {
                                prev.min(count)
                            } else {
                                prev.max(count)
                            }
                        }
                        None => count,
                    });
                }
                Ok(RulesInteger::from_usize(best.unwrap_or(0)))
            }
            ValueExpr::PartyCount(roles) => {
                let evaluated = self.evaluated_characteristics()?;
                let creatures = evaluated.objects.values().filter_map(|o| {
                    (o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && o.is_creature())
                    .then_some(&o.current_creature_subtypes)
                });
                Ok(RulesInteger::from_usize(
                    crate::characteristics::compute_party_size(creatures, **roles),
                ))
            }
            ValueExpr::LastActionMatchingCount(filter) => {
                let ability_source = if filter.excludes_ability_source() {
                    Some(resolve_ability_source().ok_or(EffectError::InconsistentProgram)?)
                } else {
                    resolve_ability_source()
                };
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let Some(cursor) = self.resolution_cursor() else {
                    return Ok(0.into());
                };
                let count = cursor
                    .results
                    .last_action_lki
                    .iter()
                    .filter(|o| {
                        self.matches_object_filter(
                            **filter,
                            o,
                            controller,
                            ability_source,
                            &excluded,
                            &evaluated,
                        )
                    })
                    .count();
                Ok(RulesInteger::from_usize(count))
            }
            ValueExpr::DistinctColors { filter } => {
                let ability_source = if filter.excludes_ability_source() {
                    Some(resolve_ability_source().ok_or(EffectError::InconsistentProgram)?)
                } else {
                    resolve_ability_source()
                };
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let mut colors = BTreeSet::new();
                for o in evaluated.objects.values() {
                    if o.zone == ObjectZone::Battlefield
                        && self.matches_object_filter(
                            **filter,
                            o,
                            controller,
                            ability_source,
                            &excluded,
                            &evaluated,
                        )
                    {
                        colors.extend(o.current_colors.iter().copied());
                    }
                }
                Ok(RulesInteger::from_usize(colors.len()))
            }
            ValueExpr::DistinctManaValuesInGraveyard(role) => {
                let seat = if *role == crate::program::ResolvingPlayer::Controller {
                    controller
                } else {
                    self.resolving_player_at(*role, cursor)?
                };
                let evaluated = self.evaluated_characteristics()?;
                let mut mvs = BTreeSet::new();
                for o in evaluated.objects.values() {
                    if o.zone == ObjectZone::Graveyard(seat)
                        && let Some(mv) = self.copiable_program(o.object).map(Program::mana_value)
                    {
                        mvs.insert(mv);
                    }
                }
                Ok(RulesInteger::from_usize(mvs.len()))
            }
            ValueExpr::SourceState(kind) => {
                let source_id = resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                let val: RulesInteger = match kind {
                    SourceStateExpr::EnteredThisTurn => i64::from(
                        self.characteristics
                            .entered_objects_this_turn
                            .contains(&source_id),
                    )
                    .into(),
                    SourceStateExpr::AttackedThisTurn => i64::from(
                        self.characteristics
                            .attacked_objects_this_turn
                            .contains(&source_id)
                            || self.triggers.attacked_this_turn(source_id),
                    )
                    .into(),
                    SourceStateExpr::AttackedOrBlockedThisTurn => i64::from(
                        self.characteristics
                            .attacked_objects_this_turn
                            .contains(&source_id)
                            || self
                                .characteristics
                                .blocked_objects_this_turn
                                .contains(&source_id)
                            || self.triggers.attacked_this_turn(source_id),
                    )
                    .into(),
                    SourceStateExpr::AttackedOrBlockedThisCombat => {
                        i64::from(self.combat.as_ref().is_some_and(|c| {
                            c.attacker(source_id).is_some() || c.blocker(source_id).is_some()
                        }))
                        .into()
                    }
                    SourceStateExpr::AttackingAlone => {
                        i64::from(self.combat.as_ref().is_some_and(|c| {
                            c.attacker(source_id).is_some() && c.attackers().count() == 1
                        }))
                        .into()
                    }
                    SourceStateExpr::Saddled => {
                        i64::from(self.characteristics.saddled.contains(&source_id)).into()
                    }
                    SourceStateExpr::Renowned => {
                        i64::from(self.characteristics.renowned.contains(&source_id)).into()
                    }
                    SourceStateExpr::Paired => i64::from(
                        self.characteristics
                            .paired_creatures
                            .contains_key(&source_id),
                    )
                    .into(),
                    SourceStateExpr::Monstrous => {
                        i64::from(self.characteristics.monstrous.contains(&source_id)).into()
                    }
                    SourceStateExpr::TributePaid => {
                        i64::from(self.characteristics.tribute_paid.contains(&source_id)).into()
                    }
                    SourceStateExpr::Suspected => i64::from(
                        self.characteristics
                            .suspect_counters
                            .get(&source_id)
                            .is_some_and(RulesInteger::is_positive),
                    )
                    .into(),
                    SourceStateExpr::Solved => {
                        i64::from(self.characteristics.solved_cases.contains(&source_id)).into()
                    }
                    SourceStateExpr::Prepared => {
                        i64::from(self.characteristics.prepared.contains(&source_id)).into()
                    }
                    SourceStateExpr::EntryOptionChosen(expected) => i64::from(
                        self.characteristics.entry_option_choices.get(&source_id) == Some(expected),
                    )
                    .into(),
                    SourceStateExpr::Goaded => i64::from(
                        self.goaded
                            .get(&source_id)
                            .is_some_and(|seats| !seats.is_empty()),
                    )
                    .into(),
                    SourceStateExpr::ExertedThisTurn => i64::from(
                        self.characteristics
                            .exerted_objects_this_turn
                            .contains(&source_id),
                    )
                    .into(),
                    SourceStateExpr::InZone(expected) => {
                        use crate::program::SourceZoneKind;
                        let in_z = self.window.cards().get(source_id).is_some_and(|o| {
                            matches!(
                                (o.zone(), expected),
                                (ObjectZone::Battlefield, SourceZoneKind::Battlefield)
                                    | (ObjectZone::Graveyard(_), SourceZoneKind::Graveyard)
                                    | (ObjectZone::Hand(_), SourceZoneKind::Hand)
                                    | (ObjectZone::Exile, SourceZoneKind::Exile)
                                    | (ObjectZone::Command, SourceZoneKind::Command)
                                    | (ObjectZone::Stack, SourceZoneKind::Stack)
                                    | (ObjectZone::Library(_), SourceZoneKind::Library)
                            )
                        });
                        i64::from(in_z).into()
                    }
                    SourceStateExpr::DamageDealtThisTurn => self
                        .characteristics
                        .object_damage_taken_this_turn
                        .get(&source_id)
                        .cloned()
                        .unwrap_or_else(|| 0.into()),
                    SourceStateExpr::AbilityActivatedThisTurn => i64::from(
                        self.characteristics
                            .activated_sources_this_turn
                            .get(&source_id)
                            .copied()
                            .unwrap_or(0),
                    )
                    .into(),
                    SourceStateExpr::AbilityResolvedCountThisTurn => i64::from(
                        self.characteristics
                            .ability_resolutions_by_source_this_turn
                            .get(&source_id)
                            .copied()
                            .unwrap_or(0),
                    )
                    .into(),
                    SourceStateExpr::ManaSpentToCast
                    | SourceStateExpr::ColorsSpentToCast
                    | SourceStateExpr::ManaTypeSpentToCast(_)
                    | SourceStateExpr::SnowManaSpentToCast
                    | SourceStateExpr::TreasureManaSpentToCast
                    | SourceStateExpr::CreatureManaSpentToCast
                    | SourceStateExpr::CastFromOutsideHand
                    | SourceStateExpr::CastFromExile
                    | SourceStateExpr::CastFromGraveyard
                    | SourceStateExpr::CastDuringMainPhase
                    | SourceStateExpr::Bargained
                    | SourceStateExpr::GiftPromised
                    | SourceStateExpr::Cleaved
                    | SourceStateExpr::EvidenceCollected => {
                        let summary =
                            self.characteristics
                                .cast_summaries
                                .get(&source_id)
                                .or_else(|| {
                                    self.window.resolving().and_then(|e| match e.kind() {
                                        StackKind::CardSpell(id) if *id == source_id => {
                                            Some(e.cast_summary())
                                        }
                                        _ => None,
                                    })
                                });
                        match kind {
                            SourceStateExpr::ManaSpentToCast => {
                                summary.map_or_else(|| 0.into(), |s| s.total_mana.clone())
                            }
                            SourceStateExpr::ColorsSpentToCast => summary.map_or_else(
                                || 0.into(),
                                |s| RulesInteger::from_usize(s.colors_spent.len()),
                            ),
                            SourceStateExpr::ManaTypeSpentToCast(mt) => summary
                                .and_then(|s| s.mana_by_type.get(mt))
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                            SourceStateExpr::SnowManaSpentToCast => {
                                summary.map_or_else(|| 0.into(), |s| s.snow_mana.clone())
                            }
                            SourceStateExpr::TreasureManaSpentToCast => {
                                summary.map_or_else(|| 0.into(), |s| s.treasure_mana.clone())
                            }
                            SourceStateExpr::CreatureManaSpentToCast => {
                                summary.map_or_else(|| 0.into(), |s| s.creature_mana.clone())
                            }
                            SourceStateExpr::CastFromOutsideHand => {
                                i64::from(summary.is_some_and(|s| s.cast_from_outside_hand)).into()
                            }
                            SourceStateExpr::CastFromExile => {
                                i64::from(summary.is_some_and(|s| s.cast_from_exile)).into()
                            }
                            SourceStateExpr::CastFromGraveyard => {
                                i64::from(summary.is_some_and(|s| s.cast_from_graveyard)).into()
                            }
                            SourceStateExpr::CastDuringMainPhase => {
                                i64::from(summary.is_some_and(|s| s.cast_during_own_main)).into()
                            }
                            SourceStateExpr::Bargained => {
                                i64::from(summary.is_some_and(|s| s.bargained)).into()
                            }
                            SourceStateExpr::GiftPromised => {
                                i64::from(summary.is_some_and(|s| s.gift_promised)).into()
                            }
                            SourceStateExpr::Cleaved => {
                                i64::from(summary.is_some_and(|s| s.cleaved)).into()
                            }
                            SourceStateExpr::EvidenceCollected => {
                                i64::from(summary.is_some_and(|s| s.evidence_collected)).into()
                            }
                            _ => 0.into(),
                        }
                    }
                    SourceStateExpr::IsRingBearer => i64::from(
                        self.characteristics.ring_bearer.get(&controller) == Some(&source_id)
                            && self.window.cards().get(source_id).is_some_and(|o| {
                                o.zone() == ObjectZone::Battlefield
                                    && o.controller() == Some(controller)
                            }),
                    )
                    .into(),
                    SourceStateExpr::EnteredFromGraveyard => i64::from(
                        self.characteristics
                            .entered_from_graveyard
                            .contains(&source_id),
                    )
                    .into(),
                    SourceStateExpr::PaidAlternative => i64::from(
                        self.characteristics
                            .paid_alternative_objects
                            .contains(&source_id)
                            || self
                                .characteristics
                                .cast_summaries
                                .get(&source_id)
                                .is_some_and(|s| s.paid_alternative)
                            || self.window.resolving().is_some_and(|e| match e.kind() {
                                StackKind::CardSpell(id) if *id == source_id => {
                                    e.paid_conditional_cost() || e.cast_summary().paid_alternative
                                }
                                _ => false,
                            }),
                    )
                    .into(),
                    SourceStateExpr::WasCast {
                        by_controller,
                        from_hand,
                    } => {
                        let on_stack_cast = self.window.resolving().is_some_and(
                            |e| matches!(e.kind(), StackKind::CardSpell(id) if *id == source_id),
                        );
                        let was_cast = on_stack_cast
                            || self.characteristics.cast_summaries.contains_key(&source_id)
                            || self
                                .characteristics
                                .cast_by_controller_objects
                                .contains(&source_id)
                            || self
                                .characteristics
                                .cast_from_hand_objects
                                .contains(&source_id);
                        let ctrl_ok = !*by_controller
                            || self
                                .characteristics
                                .cast_by_controller_objects
                                .contains(&source_id)
                            || self.window.resolving().is_some_and(|e| {
                                matches!(e.kind(), StackKind::CardSpell(id) if *id == source_id && e.controller() == controller)
                            });
                        let hand_ok = !*from_hand
                            || self
                                .characteristics
                                .cast_from_hand_objects
                                .contains(&source_id)
                            || self
                                .characteristics
                                .cast_summaries
                                .get(&source_id)
                                .is_some_and(|s| !s.cast_from_outside_hand)
                            || self.window.resolving().is_some_and(|e| {
                                matches!(e.kind(), StackKind::CardSpell(id) if *id == source_id && !e.cast_summary().cast_from_outside_hand)
                            });
                        i64::from(was_cast && ctrl_ok && hand_ok).into()
                    }
                    SourceStateExpr::EnteredFromZone(zone) => {
                        let matched = self
                            .characteristics
                            .entered_from_zone
                            .get(&source_id)
                            .is_some_and(|recorded| source_zone_matches(*recorded, *zone))
                            || (self
                                .characteristics
                                .entered_from_graveyard
                                .contains(&source_id)
                                && source_zone_matches(
                                    crate::program::SourceZoneFilter::Graveyard,
                                    *zone,
                                ));
                        i64::from(matched).into()
                    }
                    SourceStateExpr::DonorControlledByController => {
                        let related = self
                            .resolving_player_at(
                                crate::program::ResolvingPlayer::UniqueRelatedPlayer,
                                cursor,
                            )
                            .ok();
                        i64::from(related == Some(controller)).into()
                    }
                };
                Ok(val)
            }
            ValueExpr::Count {
                zone,
                filter,
                controller_scope,
            } => {
                let mut bound_filter = **filter;
                if let Some(slot) = bound_filter.chosen_type_slot() {
                    let selected = cursor
                        .and_then(|c| c.values.get(&slot))
                        .copied()
                        .ok_or(EffectError::InconsistentProgram)?;
                    if self.creature_vocabulary.label(selected).is_none() {
                        return Err(EffectError::InconsistentProgram);
                    }
                    bound_filter = bound_filter.bind_matching_chosen_slot(slot, selected);
                }
                let ability_source = if bound_filter.excludes_ability_source() {
                    let source =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    Some(source)
                } else {
                    resolve_ability_source()
                };
                let excluded = BTreeSet::new();
                match *zone {
                    ValueZone::Battlefield => {
                        if let Some(role) = controller_scope {
                            let seat = if *role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(*role, cursor)?
                            };
                            bound_filter = bound_filter.of_player(seat);
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|object| {
                                object.zone == ObjectZone::Battlefield
                                    && self.matches_object_filter(
                                        bound_filter,
                                        object,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Graveyard(player_role) => {
                        let seat = if player_role == crate::program::ResolvingPlayer::Controller {
                            controller
                        } else {
                            self.resolving_player_at(player_role, cursor)?
                        };
                        if bound_filter == crate::program::ObjectFilter::any() {
                            return Ok(RulesInteger::from_usize(
                                self.window.cards().graveyard(seat).len(),
                            ));
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Graveyard(seat)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Hand(player_role) => {
                        let seat = if player_role == crate::program::ResolvingPlayer::Controller {
                            controller
                        } else {
                            self.resolving_player_at(player_role, cursor)?
                        };
                        if bound_filter == crate::program::ObjectFilter::any() {
                            return Ok(RulesInteger::from_usize(
                                self.window
                                    .cards()
                                    .iter()
                                    .filter(|object| object.zone() == ObjectZone::Hand(seat))
                                    .count(),
                            ));
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Hand(seat)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::AllGraveyards => {
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                matches!(card.zone, ObjectZone::Graveyard(_))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::AllHands => {
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                matches!(card.zone, ObjectZone::Hand(_))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::OpponentsHands => {
                        let evaluated = self.evaluated_characteristics()?;
                        let opponents = self
                            .combat_rules
                            .opponents(self.window.participants(), controller)?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                matches!(card.zone, ObjectZone::Hand(s) if opponents.contains(&s))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Exile(owner_role) => {
                        let req_owner = match owner_role {
                            None => None,
                            Some(crate::program::ResolvingPlayer::Controller) => Some(controller),
                            Some(role) => Some(self.resolving_player_at(role, cursor)?),
                        };
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Exile
                                    && req_owner.is_none_or(|seat| {
                                        self.window
                                            .cards()
                                            .get(card.object)
                                            .is_some_and(|obj| obj.owner() == seat)
                                    })
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::Library(player_role) => {
                        let seat = if player_role == crate::program::ResolvingPlayer::Controller {
                            controller
                        } else {
                            self.resolving_player_at(player_role, cursor)?
                        };
                        if bound_filter == crate::program::ObjectFilter::any() {
                            return Ok(RulesInteger::from_usize(
                                self.window.cards().library(seat).len(),
                            ));
                        }
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Library(seat)
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        ability_source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                    ValueZone::LinkedExile => {
                        let Some(src) = ability_source.or_else(resolve_ability_source) else {
                            return Ok(0.into());
                        };
                        let evaluated = self.evaluated_characteristics()?;
                        let count = evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Exile
                                    && (self
                                        .characteristics
                                        .imprinted_cards
                                        .get(&src)
                                        .is_some_and(|list| list.contains(&card.object))
                                        || self
                                            .characteristics
                                            .ability_donors_in_exile
                                            .get(&src)
                                            .is_some_and(|list| list.contains(&card.object))
                                        || self
                                            .characteristics
                                            .linked_exiles
                                            .get(&src)
                                            .is_some_and(|list| list.contains(&card.object)))
                                    && self.matches_object_filter(
                                        bound_filter,
                                        card,
                                        controller,
                                        Some(src),
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .count();
                        Ok(RulesInteger::from_usize(count))
                    }
                }
            }
            ValueExpr::CountOtherSharingSourceSubtype => {
                let trigger = cursor
                    .and_then(|c| self.execution_bindings(c.entry))
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                let source = trigger.source.object.id();
                let evaluated = self.evaluated_characteristics()?;
                let subtypes = evaluated
                    .get(source)
                    .filter(|object| object.zone == ObjectZone::Battlefield)
                    .map(|object| &object.current_creature_subtypes)
                    .or_else(|| {
                        self.departed_damage_sources
                            .get(&source)
                            .map(|source| &source.creature_subtypes)
                    });
                let count = evaluated
                    .objects
                    .values()
                    .filter(|object| {
                        object.object != source
                            && object.zone == ObjectZone::Battlefield
                            && object.controller == Some(controller)
                            && object.is_creature()
                            && subtypes.is_some_and(|types| {
                                !object.current_creature_subtypes.is_disjoint(types)
                            })
                    })
                    .count();
                Ok(RulesInteger::from_usize(count))
            }
            ValueExpr::CountAttackingController => Ok(RulesInteger::from_usize(
                self.combat.as_ref().map_or(0, |combat| {
                    combat
                        .attackers()
                        .filter(|attacker| {
                            attacker.defender() == controller
                                && attacker.defending_planeswalker().is_none()
                        })
                        .count()
                }),
            )),
            ValueExpr::Aggregate { filter, stat, op } => {
                let ability_source = if filter.excludes_ability_source() {
                    let source =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    Some(source)
                } else {
                    resolve_ability_source()
                };
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let values = evaluated.objects.values().filter_map(|object| {
                    if object.zone != ObjectZone::Battlefield
                        || !self.matches_object_filter(
                            **filter,
                            object,
                            controller,
                            ability_source,
                            &excluded,
                            &evaluated,
                        )
                    {
                        return None;
                    }
                    match stat {
                        ObjectStatKind::Power => object
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone()),
                        ObjectStatKind::Toughness => object
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.toughness.clone()),
                        ObjectStatKind::ManaValue => self.permanent_mana_value(object.object),
                        ObjectStatKind::Loyalty => {
                            Some(self.counters_of_kind(object.object, CounterKind::Loyalty))
                        }
                    }
                });
                Ok(op.evaluate(values))
            }
            ValueExpr::ObjectStat { subject, stat } => match (subject, stat) {
                (ValueObjectRef::AbilitySource, stat) => {
                    let source_id =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    match stat {
                        ObjectStatKind::Power => Ok(self
                            .current_characteristics(source_id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.power)),
                        ObjectStatKind::Toughness => Ok(self
                            .current_characteristics(source_id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.toughness)),
                        ObjectStatKind::ManaValue => Ok(self
                            .permanent_mana_value(source_id)
                            .or_else(|| self.copiable_program(source_id).map(Program::mana_value))
                            .unwrap_or_else(|| 0.into())),
                        ObjectStatKind::Loyalty => {
                            Ok(self.counters_of_kind(source_id, CounterKind::Loyalty))
                        }
                    }
                }
                (ValueObjectRef::AttachedHost, stat) => {
                    let source_id =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    let Some(TargetRef::Object(host_id)) = self.attached_to(source_id) else {
                        return Ok(0.into());
                    };
                    match stat {
                        ObjectStatKind::Power => Ok(self
                            .current_characteristics(host_id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.power)),
                        ObjectStatKind::Toughness => Ok(self
                            .current_characteristics(host_id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.toughness)),
                        ObjectStatKind::ManaValue => Ok(self
                            .permanent_mana_value(host_id)
                            .or_else(|| self.copiable_program(host_id).map(Program::mana_value))
                            .unwrap_or_else(|| 0.into())),
                        ObjectStatKind::Loyalty => {
                            Ok(self.counters_of_kind(host_id, CounterKind::Loyalty))
                        }
                    }
                }
                (ValueObjectRef::RevealedTop, stat) => {
                    let Some(id) = cursor
                        .or_else(|| self.resolution_cursor())
                        .and_then(|c| c.results.revealed_top_card)
                    else {
                        return Ok(0.into());
                    };
                    match stat {
                        ObjectStatKind::Power => Ok(self
                            .current_characteristics(id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.power)),
                        ObjectStatKind::Toughness => Ok(self
                            .current_characteristics(id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.toughness)),
                        ObjectStatKind::ManaValue => Ok(self
                            .copiable_program(id)
                            .map(Program::mana_value)
                            .unwrap_or_else(|| 0.into())),
                        ObjectStatKind::Loyalty => {
                            Ok(self.counters_of_kind(id, CounterKind::Loyalty))
                        }
                    }
                }
                (ValueObjectRef::LastAmassedArmy, stat) => {
                    let Some(id) = cursor
                        .or_else(|| self.resolution_cursor())
                        .and_then(|c| c.results.last_amassed_army)
                    else {
                        return Ok(0.into());
                    };
                    match stat {
                        ObjectStatKind::Power => Ok(self
                            .current_characteristics(id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.power)),
                        ObjectStatKind::Toughness => Ok(self
                            .current_characteristics(id)?
                            .and_then(|value| value.power_toughness)
                            .map_or_else(|| 0.into(), |pt| pt.derived.toughness)),
                        ObjectStatKind::ManaValue => Ok(self
                            .copiable_program(id)
                            .map(Program::mana_value)
                            .unwrap_or_else(|| 0.into())),
                        ObjectStatKind::Loyalty => {
                            Ok(self.counters_of_kind(id, CounterKind::Loyalty))
                        }
                    }
                }
                (ValueObjectRef::TriggerSubject, stat) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let Some(member) = binding.causes.members().first() else {
                        return Ok(0.into());
                    };
                    let Some(id) = member.subject_object_id() else {
                        return Ok(0.into());
                    };
                    match stat {
                        ObjectStatKind::Power => {
                            if let Some(pt) = self
                                .current_characteristics(id)?
                                .and_then(|v| v.power_toughness)
                            {
                                Ok(pt.derived.power)
                            } else if let crate::occurrence::TriggerRelated::ZoneChange {
                                before,
                                ..
                            } = &member.related
                            {
                                Ok(before
                                    .current_power_toughness
                                    .as_ref()
                                    .map_or_else(|| 0.into(), |pt| pt.power.clone()))
                            } else {
                                Ok(0.into())
                            }
                        }
                        ObjectStatKind::Toughness => {
                            if let Some(pt) = self
                                .current_characteristics(id)?
                                .and_then(|v| v.power_toughness)
                            {
                                Ok(pt.derived.toughness)
                            } else if let crate::occurrence::TriggerRelated::ZoneChange {
                                before,
                                ..
                            } = &member.related
                            {
                                Ok(before
                                    .current_power_toughness
                                    .as_ref()
                                    .map_or_else(|| 0.into(), |pt| pt.toughness.clone()))
                            } else {
                                Ok(0.into())
                            }
                        }
                        ObjectStatKind::ManaValue => Ok(self
                            .permanent_mana_value(id)
                            .or_else(|| self.copiable_program(id).map(Program::mana_value))
                            .unwrap_or_else(|| 0.into())),
                        ObjectStatKind::Loyalty => {
                            Ok(self.counters_of_kind(id, CounterKind::Loyalty))
                        }
                    }
                }
                (ValueObjectRef::TriggerSource, stat) => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let id = trigger.source.object.id();
                    if *stat == ObjectStatKind::ManaValue {
                        return Ok(self
                            .permanent_mana_value(id)
                            .or_else(|| self.copiable_program(id).map(Program::mana_value))
                            .unwrap_or_else(|| 0.into()));
                    }
                    if *stat == ObjectStatKind::Loyalty {
                        return Ok(self.counters_of_kind(id, CounterKind::Loyalty));
                    }
                    let current = self.evaluated_characteristics()?;
                    let value = if let Some(live) = current
                        .get(id)
                        .filter(|o| o.zone == ObjectZone::Battlefield)
                    {
                        live.power_toughness.as_ref().map(|pt| {
                            if *stat == ObjectStatKind::Power {
                                &pt.derived.power
                            } else {
                                &pt.derived.toughness
                            }
                        })
                    } else {
                        self.departed_reflexive_sources
                            .get(&id)
                            .unwrap_or(&trigger.source)
                            .current_power_toughness
                            .as_ref()
                            .map(|pt| {
                                if *stat == ObjectStatKind::Power {
                                    &pt.power
                                } else {
                                    &pt.toughness
                                }
                            })
                    };
                    Ok(value.cloned().unwrap_or_else(|| 0.into()).max(0.into()))
                }
                (
                    ValueObjectRef::TargetObject,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let bound = match self.instruction_target(entry) {
                        Ok(bound) => bound,
                        Err(_) if entry.targets().targets().is_empty() => return Ok(0.into()),
                        Err(err) => return Err(err),
                    };
                    let TargetRef::Object(id) = bound.choice().target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    if let Some(stats) = self
                        .current_characteristics(id)?
                        .and_then(|value| value.power_toughness)
                    {
                        Ok(if *stat == ObjectStatKind::Power {
                            stats.derived.power
                        } else {
                            stats.derived.toughness
                        })
                    } else if let Some(lki) = cursor
                        .or_else(|| self.resolution_cursor())
                        .and_then(|c| c.results.target_lki.get(&id))
                    {
                        let raw = if *stat == ObjectStatKind::Power {
                            lki.power.as_ref()
                        } else {
                            lki.toughness.as_ref()
                        };
                        raw.cloned().ok_or(EffectError::InconsistentProgram)
                    } else {
                        Err(EffectError::InconsistentProgram)
                    }
                }
                (ValueObjectRef::TargetObject, ObjectStatKind::ManaValue) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let bound = match self.instruction_target(entry) {
                        Ok(bound) => bound,
                        Err(_) if entry.targets().targets().is_empty() => return Ok(0.into()),
                        Err(err) => return Err(err),
                    };
                    match bound.choice().target {
                        TargetRef::Object(id) => self
                            .copiable_program(id)
                            .map(Program::mana_value)
                            .or_else(|| {
                                cursor
                                    .or_else(|| self.resolution_cursor())
                                    .and_then(|c| c.results.target_lki.get(&id))
                                    .map(|lki| lki.mana_value.clone())
                            })
                            .ok_or(EffectError::InconsistentProgram),
                        TargetRef::Stack(sid) => self
                            .window
                            .entries()
                            .iter()
                            .find(|e| e.id() == sid)
                            .and_then(|e| match e.kind() {
                                StackKind::CardSpell(id) => {
                                    self.copiable_program(*id).map(Program::mana_value)
                                }
                                _ => None,
                            })
                            .ok_or(EffectError::InconsistentProgram),
                        TargetRef::Player(_) => Err(EffectError::InconsistentProgram),
                    }
                }
                (ValueObjectRef::TargetObject, ObjectStatKind::Loyalty) => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let bound = match self.instruction_target(entry) {
                        Ok(bound) => bound,
                        Err(_) if entry.targets().targets().is_empty() => return Ok(0.into()),
                        Err(err) => return Err(err),
                    };
                    let TargetRef::Object(id) = bound.choice().target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(self.counters_of_kind(id, CounterKind::Loyalty))
                }
                (
                    ValueObjectRef::DyingSource | ValueObjectRef::DyingCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &cause.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let raw = before.current_power_toughness.as_ref().map_or_else(
                        || 0.into(),
                        |pt| {
                            if *stat == ObjectStatKind::Power {
                                pt.power.clone()
                            } else {
                                pt.toughness.clone()
                            }
                        },
                    );
                    Ok(raw.max(0.into()))
                }
                (
                    ValueObjectRef::EnteringPermanent,
                    ObjectStatKind::Power | ObjectStatKind::Toughness | ObjectStatKind::ManaValue,
                ) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let after = match &cause.related {
                        crate::occurrence::TriggerRelated::ZoneChange { after, .. }
                        | crate::occurrence::TriggerRelated::TokenCreated { after, .. } => after,
                        _ => return Err(EffectError::InconsistentProgram),
                    };
                    let id = after.object.id();
                    if *stat == ObjectStatKind::ManaValue {
                        return Ok(self
                            .copiable_program(id)
                            .map(Program::mana_value)
                            .unwrap_or_else(|| 0.into()));
                    }
                    let evaluated = self.evaluated_characteristics()?;
                    let raw = evaluated
                        .get(id)
                        .and_then(|o| o.power_toughness.as_ref())
                        .map(|pt| {
                            if *stat == ObjectStatKind::Power {
                                pt.derived.power.clone()
                            } else {
                                pt.derived.toughness.clone()
                            }
                        })
                        .or_else(|| {
                            after.current_power_toughness.as_ref().map(|pt| {
                                if *stat == ObjectStatKind::Power {
                                    pt.power.clone()
                                } else {
                                    pt.toughness.clone()
                                }
                            })
                        })
                        .unwrap_or_else(|| 0.into());
                    Ok(raw.max(0.into()))
                }
                (ValueObjectRef::DestructionTarget, ObjectStatKind::Toughness) => Ok(cursor
                    .and_then(|c| c.results.destruction_target_toughness.clone())
                    .unwrap_or_else(|| 0.into())
                    .max(0.into())),
                (
                    ValueObjectRef::LastSacrificedCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => Ok(cursor
                    .and_then(|c| c.results.last_sacrificed_creature.as_ref())
                    .map_or_else(
                        || 0.into(),
                        |stats| {
                            let value = if *stat == ObjectStatKind::Power {
                                &stats.power
                            } else {
                                &stats.toughness
                            };
                            value.clone().max(0.into())
                        },
                    )),
                (
                    ValueObjectRef::SacrificedCreature,
                    ObjectStatKind::Power | ObjectStatKind::Toughness,
                ) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let value = if *stat == ObjectStatKind::Power {
                        &payload.sacrificed_creature_power
                    } else {
                        &payload.sacrificed_creature_toughness
                    };
                    Ok(value
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone()
                        .max(0.into()))
                }
                (ValueObjectRef::SacrificedPermanent, ObjectStatKind::ManaValue) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(payload
                        .sacrificed_permanent_mana_value
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone()
                        .max(0.into()))
                }
                (ValueObjectRef::ExiledCostCreature, ObjectStatKind::Power) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let Some((object, last_power)) = &payload.exiled_cost_creature else {
                        return Ok(0.into());
                    };
                    let power = self
                        .current_characteristics(*object)?
                        .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                        .unwrap_or_else(|| last_power.clone());
                    Ok(power.max(0.into()))
                }
                (ValueObjectRef::ExiledCostCreature, ObjectStatKind::ManaValue) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let Some((object, _)) = &payload.exiled_cost_creature else {
                        return Ok(0.into());
                    };
                    let mv = self
                        .copiable_program(*object)
                        .map(Program::mana_value)
                        .unwrap_or_else(|| 0.into());
                    Ok(mv.max(0.into()))
                }
                (ValueObjectRef::RevealedCostCard, ObjectStatKind::Power) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let Some(cost) = &payload.revealed_cost_card else {
                        return Ok(0.into());
                    };
                    let power = self
                        .current_characteristics(cost.object)?
                        .and_then(|card| card.power_toughness.map(|stats| stats.derived.power))
                        .unwrap_or_else(|| cost.last_power.clone());
                    Ok(power.max(0.into()))
                }
                (ValueObjectRef::RevealedCostCard, ObjectStatKind::ManaValue) => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let Some(cost) = &payload.revealed_cost_card else {
                        return Ok(0.into());
                    };
                    Ok(cost.mana_value.clone().max(0.into()))
                }
                (ValueObjectRef::TriggeringSpell, ObjectStatKind::ManaValue) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::SpellCast(spell) = &cause.related else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(spell.mana_value().clone())
                }
                (ValueObjectRef::FinalSaga, ObjectStatKind::ManaValue) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [cause] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::FinalSagaChapter { mana_value, .. } =
                        &cause.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(mana_value.clone())
                }
                _ => Err(EffectError::InconsistentProgram),
            },
            ValueExpr::CounterCount { subject, kind } => match subject {
                CounterSubjectRef::AbilitySource => {
                    if let Some(source_id) = explicit_source {
                        return Ok(match kind {
                            Some(k) => self.counters_of_kind(source_id, *k),
                            None => self.characteristics.total_permanent_counters(source_id),
                        });
                    }
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    match kind {
                        Some(k) => self.source_counter_value_at_resolution(entry, *k),
                        None => {
                            let source_id = match entry.kind() {
                                StackKind::ActivatedAbility { source }
                                | StackKind::TriggeredAbility { source } => source.id(),
                                _ => return Err(EffectError::InconsistentProgram),
                            };
                            Ok(self.characteristics.total_permanent_counters(source_id))
                        }
                    }
                }
                CounterSubjectRef::TargetObject => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(match kind {
                        Some(k) => self.counters_of_kind(id, *k),
                        None => self.characteristics.total_permanent_counters(id),
                    })
                }
                CounterSubjectRef::TriggerSource => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let id = trigger.source.object.id();
                    if self
                        .evaluated_characteristics()?
                        .get(id)
                        .is_some_and(|object| object.zone == ObjectZone::Battlefield)
                    {
                        return Ok(match kind {
                            Some(k) => self.counters_of_kind(id, *k),
                            None => self.characteristics.total_permanent_counters(id),
                        });
                    }
                    let appearance = self
                        .departed_reflexive_sources
                        .get(&id)
                        .unwrap_or(&trigger.source);
                    Ok(match kind {
                        Some(CounterKind::PlusOne) => appearance
                            .battlefield_counters
                            .as_ref()
                            .map_or_else(|| 0.into(), |c| c.plus_one.clone()),
                        None => appearance
                            .battlefield_total_counters
                            .clone()
                            .unwrap_or_else(|| 0.into()),
                        _ => 0.into(),
                    })
                }
                CounterSubjectRef::DyingSource => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    match binding.causes.members() {
                        [member] => match &member.related {
                            crate::occurrence::TriggerRelated::ZoneChange { before, .. }
                                if before.object.id() == binding.source.object.id() =>
                            {
                                Ok(match kind {
                                    None => before
                                        .battlefield_total_counters
                                        .clone()
                                        .unwrap_or_else(|| 0.into()),
                                    Some(CounterKind::PlusOne) => before
                                        .battlefield_counters
                                        .as_ref()
                                        .map_or_else(|| 0.into(), |c| c.plus_one.clone()),
                                    _ => 0.into(),
                                })
                            }
                            _ => Err(EffectError::InconsistentProgram),
                        },
                        _ => Err(EffectError::InconsistentProgram),
                    }
                }
                CounterSubjectRef::AttachedHost => {
                    let source_id =
                        resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                    let Some(TargetRef::Object(host_id)) = self.attached_to(source_id) else {
                        return Ok(0.into());
                    };
                    Ok(match kind {
                        Some(k) => self.counters_of_kind(host_id, *k),
                        None => self.characteristics.total_permanent_counters(host_id),
                    })
                }
                CounterSubjectRef::TriggerSubject => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let Some(id) = binding
                        .causes
                        .members()
                        .first()
                        .and_then(|m| m.subject_object_id())
                    else {
                        return Ok(0.into());
                    };
                    Ok(match kind {
                        Some(k) => self.counters_of_kind(id, *k),
                        None => self.characteristics.total_permanent_counters(id),
                    })
                }
                CounterSubjectRef::ControllerExperience => Ok(self.experience_counters(controller)),
                CounterSubjectRef::ControllerEnergy => {
                    Ok(self.player_counters_of_kind(controller, CounterKind::Energy))
                }
                CounterSubjectRef::ControllerPoison => Ok(self.poison_counters(controller)),
                CounterSubjectRef::PlayerCounter(role, counter_kind) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(self.player_counters_of_kind(seat, *counter_kind))
                }
            },
            ValueExpr::AttachedCount(kind) => match kind {
                AttachedCountKind::EquipmentOnSource
                | AttachedCountKind::AurasAndEquipmentOnSource => {
                    let both = matches!(kind, AttachedCountKind::AurasAndEquipmentOnSource);
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let source = binding.source.object.id();
                    let current = self.evaluated_characteristics()?;
                    if current
                        .get(source)
                        .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        Ok(if both {
                            current.auras_and_equipment_attached_to(source)
                        } else {
                            current.equipment_attached_to(source)
                        })
                    } else {
                        Ok(self.departed_damage_sources.get(&source).map_or_else(
                            || 0.into(),
                            |p| {
                                if both {
                                    p.attached_auras_and_equipment.clone()
                                } else {
                                    p.attached_equipment.clone()
                                }
                            },
                        ))
                    }
                }
                AttachedCountKind::CursesOnRelatedPlayer => {
                    let player = self.resolving_player_at(
                        crate::program::ResolvingPlayer::UniqueRelatedPlayer,
                        cursor,
                    )?;
                    let world = self.evaluated_characteristics()?;
                    let count = world
                        .objects
                        .values()
                        .filter(|object| {
                            object.zone == ObjectZone::Battlefield
                                && object.attached_to == Some(TargetRef::Player(player))
                                && object
                                    .current_enchantment_subtypes
                                    .contains(&crate::program::EnchantmentSubtype::Curse)
                        })
                        .count();
                    Ok(RulesInteger::from_usize(count))
                }
            },
            ValueExpr::PlayerStat(stat) => match stat {
                PlayerStatExpr::ControllerLife => self
                    .life
                    .get(controller)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram),
                PlayerStatExpr::PlayerLife(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    self.life
                        .get(seat)
                        .cloned()
                        .ok_or(EffectError::InconsistentProgram)
                }
                PlayerStatExpr::PlayerHandSize(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(RulesInteger::from_usize(
                        self.window
                            .cards()
                            .iter()
                            .filter(|o| o.zone() == ObjectZone::Hand(seat))
                            .count(),
                    ))
                }
                PlayerStatExpr::PlayerLibrarySize(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(RulesInteger::from_usize(
                        self.window.cards().library(seat).len(),
                    ))
                }
                PlayerStatExpr::PlayerGraveyardSize(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(RulesInteger::from_usize(
                        self.window
                            .cards()
                            .iter()
                            .filter(|o| o.zone() == ObjectZone::Graveyard(seat))
                            .count(),
                    ))
                }
                PlayerStatExpr::ControllerLifeAboveStarting => {
                    let starting = self
                        .life
                        .starting_total(controller)
                        .ok_or(EffectError::InvalidResolvingPlayer)?;
                    let current = self
                        .life
                        .get(controller)
                        .ok_or(EffectError::InvalidResolvingPlayer)?;
                    Ok(current.minus(starting))
                }
                PlayerStatExpr::OpponentsLifeLostThisTurn => Ok(self
                    .characteristics
                    .life_lost_this_turn
                    .iter()
                    .filter(|(player, _)| **player != controller)
                    .fold(0.into(), |total: RulesInteger, (_, loss)| total.plus(loss))),
                PlayerStatExpr::CardsDrawnThisTurn => Ok(self
                    .cards_drawn_this_turn
                    .get(&controller)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::CardsShortOfHandSize(required) => {
                    let count = self
                        .window
                        .cards()
                        .iter()
                        .filter(|o| o.zone() == ObjectZone::Hand(controller))
                        .count();
                    Ok(RulesInteger::from_usize(
                        usize::from(*required).saturating_sub(count),
                    ))
                }
                PlayerStatExpr::LargestHandSize => {
                    let mut sizes = BTreeMap::new();
                    for object in self.window.cards().iter() {
                        if let ObjectZone::Hand(player) = object.zone() {
                            *sizes.entry(player).or_insert(0usize) += 1;
                        }
                    }
                    Ok(RulesInteger::from_usize(
                        sizes.into_values().max().unwrap_or(0),
                    ))
                }
                PlayerStatExpr::ControllerIsMonarch => {
                    Ok(i64::from(self.characteristics.monarch == Some(controller)).into())
                }
                PlayerStatExpr::ControlsGreatestCreature => {
                    let evaluated = self.evaluated_characteristics()?;
                    let creatures = evaluated
                        .objects
                        .values()
                        .filter(|p| p.zone == ObjectZone::Battlefield && p.is_creature())
                        .filter_map(|p| {
                            p.power_toughness
                                .as_ref()
                                .map(|pt| (&pt.derived.power, p.controller == Some(controller)))
                        });
                    Ok(i64::from(creatures.max().is_some_and(|(_, c)| c)).into())
                }
                PlayerStatExpr::PlayerIsActivePlayer(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(i64::from(self.window.active_player() == seat).into())
                }
                PlayerStatExpr::ControllerIsActivePlayer => {
                    Ok(i64::from(self.window.active_player() == controller).into())
                }
                PlayerStatExpr::OwnGraveyardDistinctCardTypes => {
                    let evaluated = self.evaluated_characteristics()?;
                    let mut types = BTreeSet::new();
                    for card in evaluated
                        .objects
                        .values()
                        .filter(|c| c.zone == ObjectZone::Graveyard(controller))
                    {
                        types.extend(card.current_types.iter().copied());
                    }
                    Ok(RulesInteger::from_usize(types.len()))
                }
                PlayerStatExpr::ControllerLifeLostThisTurn => Ok(self
                    .characteristics
                    .life_lost_this_turn
                    .get(&controller)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::ControllerLifeGainedThisTurn => Ok(self
                    .characteristics
                    .life_gained_this_turn
                    .get(&controller)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::ControllerSpellsCastThisTurn => Ok(i64::from(
                    self.characteristics
                        .spells_cast_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::ControllerAttackedThisTurn => Ok(i64::from(
                    self.characteristics
                        .attacked_this_turn_players
                        .contains(&controller),
                )
                .into()),
                PlayerStatExpr::ControllerDiscardedThisTurn => Ok(i64::from(
                    self.characteristics
                        .cards_discarded_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::ControllerSacrificedThisTurn => Ok(i64::from(
                    self.characteristics
                        .permanents_sacrificed_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::PermanentLeftBattlefieldThisTurn => Ok(i64::from(
                    self.characteristics
                        .permanents_left_battlefield_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CreatureEnteredUnderControlThisTurn => Ok(i64::from(
                    self.characteristics
                        .creatures_entered_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::DescendedThisTurn => Ok(i64::from(
                    self.characteristics
                        .descended_this_turn
                        .contains(&controller),
                )
                .into()),
                PlayerStatExpr::CommittedCrimeThisTurn => Ok(i64::from(
                    self.characteristics
                        .committed_crime_this_turn
                        .contains(&controller),
                )
                .into()),
                PlayerStatExpr::ControllerHasCityBlessing => {
                    Ok(i64::from(self.characteristics.city_blessing.contains(&controller)).into())
                }
                PlayerStatExpr::ControllerStartingLife => self
                    .life
                    .starting_total(controller)
                    .cloned()
                    .ok_or(EffectError::InvalidResolvingPlayer),
                PlayerStatExpr::OpponentMaxLife => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    Ok(opponents
                        .into_iter()
                        .filter_map(|p| self.life.get(p).cloned())
                        .max()
                        .unwrap_or_else(|| {
                            self.life
                                .get(controller)
                                .cloned()
                                .unwrap_or_else(|| 0.into())
                        }))
                }
                PlayerStatExpr::OpponentIsMonarch => Ok(i64::from(
                    self.characteristics
                        .monarch
                        .is_some_and(|m| m != controller),
                )
                .into()),
                PlayerStatExpr::NoMonarch => {
                    Ok(i64::from(self.characteristics.monarch.is_none()).into())
                }
                PlayerStatExpr::ControllerIsAttacking => Ok(i64::from(
                    self.window.active_player() == controller
                        && self
                            .combat
                            .as_ref()
                            .is_some_and(|c| c.attackers().next().is_some()),
                )
                .into()),
                PlayerStatExpr::ControllerIsDefending => Ok(i64::from(
                    self.combat
                        .as_ref()
                        .is_some_and(|c| c.attackers().any(|a| a.defender() == controller)),
                )
                .into()),
                PlayerStatExpr::ControllerSpeed => Ok(i64::from(
                    self.characteristics
                        .speed
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::AllGraveyardsDistinctCardTypes => {
                    let evaluated = self.evaluated_characteristics()?;
                    let mut types = BTreeSet::new();
                    for card in evaluated
                        .objects
                        .values()
                        .filter(|c| matches!(c.zone, ObjectZone::Graveyard(_)))
                    {
                        types.extend(card.current_types.iter().copied());
                    }
                    Ok(RulesInteger::from_usize(types.len()))
                }
                PlayerStatExpr::CastSpellTypeThisTurn(kind) => Ok(i64::from(
                    self.characteristics
                        .cast_spell_types_this_turn
                        .get(&controller)
                        .and_then(|m| m.get(kind))
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CastNoncreatureSpellsThisTurn => Ok(i64::from(
                    self.characteristics
                        .noncreature_spells_cast_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CastSpellsFromHandThisTurn => Ok(i64::from(
                    self.characteristics
                        .spells_cast_from_hand_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::AttackingCreaturesThisTurn => Ok(RulesInteger::from_usize(
                    self.characteristics
                        .attacking_creatures_this_turn
                        .get(&controller)
                        .map_or(0, BTreeSet::len),
                )),
                PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn => Ok(i64::from(
                    self.characteristics
                        .nonland_permanents_left_battlefield_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::AnyNonlandPermanentLeftBattlefieldThisTurn => Ok(self
                    .characteristics
                    .nonland_permanents_left_battlefield_this_turn
                    .values()
                    .copied()
                    .map(i64::from)
                    .sum::<i64>()
                    .into()),
                PlayerStatExpr::AnySpellWarpedThisTurn => {
                    Ok(i64::from(self.characteristics.any_spell_warped_this_turn).into())
                }
                PlayerStatExpr::EnteredPermanentThisTurn => Ok(i64::from(
                    self.characteristics
                        .permanents_entered_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::EnteredTypeThisTurn(kind) => Ok(i64::from(
                    self.characteristics
                        .entered_types_this_turn
                        .get(&controller)
                        .and_then(|m| m.get(kind))
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::PlayedLandThisTurn => Ok(i64::from(
                    self.characteristics
                        .lands_played_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CardsLeftGraveyardThisTurn => Ok(i64::from(
                    self.characteristics
                        .cards_left_graveyard_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CreaturesDiedThisTurn => Ok(self.creatures_died_this_turn.clone()),
                PlayerStatExpr::ControlledCreatureDiedThisTurn => Ok(i64::from(
                    self.characteristics
                        .controlled_creatures_died_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CreatedTokensThisTurn => Ok(i64::from(
                    self.characteristics
                        .tokens_created_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::OpponentDealtDamageThisTurn { noncombat_only } => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let set = if *noncombat_only {
                        &self
                            .characteristics
                            .players_dealt_noncombat_damage_this_turn
                    } else {
                        &self.characteristics.players_dealt_damage_this_turn
                    };
                    Ok(i64::from(opponents.iter().any(|opp| set.contains(opp))).into())
                }
                PlayerStatExpr::AnyOpponentPoisoned => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    Ok(i64::from(
                        opponents
                            .iter()
                            .any(|opp| self.poison_counters(*opp).is_positive()),
                    )
                    .into())
                }
                PlayerStatExpr::LoyaltyActivatedThisTurn => Ok(i64::from(
                    self.characteristics
                        .loyalty_activators
                        .contains(&controller),
                )
                .into()),
                PlayerStatExpr::IsOwnMainPhase => Ok(i64::from(
                    self.window.active_player() == controller
                        && matches!(
                            self.current_step,
                            Some(
                                crate::turn::Step::PrecombatMain
                                    | crate::turn::Step::PostcombatMain
                            )
                        ),
                )
                .into()),
                PlayerStatExpr::ExhaustActivatedThisTurn => {
                    let src = resolve_ability_source();
                    Ok(i64::from(src.is_some_and(|id| {
                        self.characteristics
                            .exhausted
                            .iter()
                            .any(|(obj, _, _, _)| *obj == id)
                    }))
                    .into())
                }
                PlayerStatExpr::CompletedDungeons => Ok(i64::from(
                    self.characteristics
                        .dungeons
                        .get(&controller)
                        .map_or(0, |d| d.completed),
                )
                .into()),
                PlayerStatExpr::RingLevel => Ok(i64::from(
                    self.characteristics
                        .ring_level
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::HasInitiative => {
                    Ok(i64::from(self.characteristics.initiative == Some(controller)).into())
                }
                PlayerStatExpr::OpponentMinLife => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    Ok(opponents
                        .iter()
                        .filter_map(|&opp| self.life.get(opp))
                        .min()
                        .cloned()
                        .or_else(|| self.life.get(controller).cloned())
                        .unwrap_or_else(|| 0.into()))
                }
                PlayerStatExpr::OpponentMaxCounter(kind) => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    Ok(opponents
                        .iter()
                        .map(|&opp| {
                            if *kind == CounterKind::Experience {
                                self.characteristics
                                    .experience
                                    .get(&opp)
                                    .cloned()
                                    .unwrap_or_else(|| 0.into())
                            } else if *kind == CounterKind::Poison {
                                self.poison_counters(opp)
                            } else {
                                self.characteristics
                                    .player_counters
                                    .get(kind)
                                    .and_then(|m| m.get(&opp))
                                    .cloned()
                                    .unwrap_or_else(|| 0.into())
                            }
                        })
                        .max()
                        .unwrap_or_else(|| 0.into()))
                }
                PlayerStatExpr::OpponentStatThisTurn(stat) => {
                    use crate::program::OpponentTurnStat;
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let any_opp = opponents.iter().any(|&opp| match stat {
                        OpponentTurnStat::Discarded => {
                            self.characteristics
                                .cards_discarded_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::Sacrificed => {
                            self.characteristics
                                .permanents_sacrificed_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::CastSpells => {
                            self.characteristics
                                .spells_cast_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::DrewCards => {
                            self.characteristics
                                .cards_drawn_this_turn_counts
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::Attacked => self
                            .characteristics
                            .attacked_this_turn_players
                            .contains(&opp),
                        OpponentTurnStat::GainedLife => self
                            .characteristics
                            .life_gained_this_turn
                            .get(&opp)
                            .is_some_and(RulesInteger::is_positive),
                        OpponentTurnStat::LostLife => self
                            .characteristics
                            .life_lost_this_turn
                            .get(&opp)
                            .is_some_and(RulesInteger::is_positive),
                        OpponentTurnStat::PlayedLand => {
                            self.characteristics
                                .lands_played_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::CreatureEntered => {
                            self.characteristics
                                .creatures_entered_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                        OpponentTurnStat::PermanentLeft => {
                            self.characteristics
                                .permanents_left_battlefield_this_turn
                                .get(&opp)
                                .copied()
                                .unwrap_or(0)
                                > 0
                        }
                    });
                    Ok(i64::from(any_opp).into())
                }
                PlayerStatExpr::DayNightState => Ok(match self.day_night() {
                    crate::day_night::DayNight::Neither => 0.into(),
                    crate::day_night::DayNight::Day => 1.into(),
                    crate::day_night::DayNight::Night => 2.into(),
                }),
                PlayerStatExpr::MinLibrarySize => Ok(RulesInteger::from_usize(
                    self.window
                        .participants()
                        .iter()
                        .map(|&p| self.window.cards().library(p).len())
                        .min()
                        .unwrap_or(0),
                )),
                PlayerStatExpr::CombatPhasesThisTurn => {
                    Ok(i64::from(self.characteristics.combat_phases_this_turn).into())
                }
                PlayerStatExpr::CardsEnteredGraveyardThisTurn => Ok(i64::from(
                    self.characteristics
                        .cards_entered_graveyard_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::CountersPlacedOnCreaturesThisTurn => Ok(i64::from(
                    self.characteristics
                        .counters_placed_on_creatures_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::LandsEnteredThisTurn(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(i64::from(
                        self.characteristics
                            .lands_entered_this_turn
                            .get(&seat)
                            .copied()
                            .unwrap_or(0),
                    )
                    .into())
                }
                PlayerStatExpr::DiceRolledThisTurn => Ok(i64::from(
                    self.characteristics
                        .dice_rolled_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0),
                )
                .into()),
                PlayerStatExpr::OtherSpellsCastManaValueThisTurn => {
                    let total = self
                        .characteristics
                        .other_spells_mv_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0);
                    let own_mv = self
                        .window
                        .resolving()
                        .filter(|entry| {
                            entry.controller() == controller
                                && matches!(entry.kind(), StackKind::CardSpell(_))
                        })
                        .and_then(|entry| self.entry_spell_mana_value(entry.id()))
                        .and_then(|mv| mv.to_usize())
                        .map_or(0u32, |n| u32::try_from(n).unwrap_or(u32::MAX));
                    Ok(i64::from(total.saturating_sub(own_mv)).into())
                }
                PlayerStatExpr::OtherSpellsCastThisTurn => {
                    let total = self
                        .characteristics
                        .spells_cast_this_turn
                        .get(&controller)
                        .copied()
                        .unwrap_or(0);
                    let self_on_stack = u16::from(self.window.resolving().is_some_and(|entry| {
                        entry.controller() == controller
                            && matches!(entry.kind(), StackKind::CardSpell(_))
                    }));
                    Ok(i64::from(total.saturating_sub(self_on_stack)).into())
                }
                PlayerStatExpr::MaxPlayerLifeLostThisTurn => Ok(self
                    .characteristics
                    .life_lost_this_turn
                    .values()
                    .max()
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players,
                    filter,
                    from_zone,
                } => {
                    let seats = match *players {
                        crate::program::AffectedPlayers::One(role) => {
                            vec![if role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(role, cursor)?
                            }]
                        }
                        crate::program::AffectedPlayers::EachPlayer => {
                            self.window.participants().to_vec()
                        }
                        crate::program::AffectedPlayers::EachOtherPlayer => self
                            .window
                            .participants()
                            .iter()
                            .copied()
                            .filter(|&p| p != controller)
                            .collect(),
                        crate::program::AffectedPlayers::EachOpponent => self
                            .combat_rules
                            .opponents(self.window.participants(), controller)?,
                    };
                    let mut count = 0usize;
                    for seat in seats {
                        if let Some(records) = self.characteristics.cast_spells_this_turn.get(&seat)
                        {
                            count += records
                                .iter()
                                .filter(|rec| matches_cast_spell_record(**filter, rec, *from_zone))
                                .count();
                        }
                    }
                    Ok(RulesInteger::from_usize(count))
                }
                PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(i64::from(
                        self.characteristics
                            .activated_graveyard_ability_this_turn
                            .contains(&seat),
                    )
                    .into())
                }
                PlayerStatExpr::PlayerLifeLostLastTurn(role) => {
                    let seat = if *role == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*role, cursor)?
                    };
                    Ok(self
                        .characteristics
                        .life_lost_last_turn
                        .get(&seat)
                        .cloned()
                        .unwrap_or_else(|| 0.into()))
                }
                PlayerStatExpr::CardsExiledThisTurn => {
                    Ok(i64::from(self.characteristics.cards_exiled_this_turn).into())
                }
                PlayerStatExpr::SpellsCastLastTurn => Ok(self
                    .characteristics
                    .spells_cast_last_turn
                    .values()
                    .fold(RulesInteger::from(0), |acc, n| acc.plus(n))),
                PlayerStatExpr::MaxPlayerSpellsCastLastTurn => Ok(self
                    .characteristics
                    .spells_cast_last_turn
                    .values()
                    .max()
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                    filter,
                    from_battlefield_only,
                    controller: ctrl_role,
                } => {
                    let req_ctrl = match ctrl_role {
                        None => None,
                        Some(role) => {
                            Some(if *role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(*role, cursor)?
                            })
                        }
                    };
                    let opp_vec = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let count = self
                        .characteristics
                        .cards_put_into_graveyard_this_turn
                        .iter()
                        .filter(|rec| {
                            (!*from_battlefield_only || rec.from_battlefield)
                                && req_ctrl.is_none_or(|seat| {
                                    rec.previous_controller == Some(seat)
                                        || (rec.previous_controller.is_none() && rec.owner == seat)
                                })
                                && matches_zone_move_record(**filter, rec, controller, &opp_vec)
                        })
                        .count();
                    Ok(RulesInteger::from_usize(count))
                }
                PlayerStatExpr::SacrificedMatchingThisTurn { players, filter } => {
                    let seats = match *players {
                        crate::program::AffectedPlayers::One(role) => {
                            vec![if role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(role, cursor)?
                            }]
                        }
                        crate::program::AffectedPlayers::EachPlayer => {
                            self.window.participants().to_vec()
                        }
                        crate::program::AffectedPlayers::EachOtherPlayer => self
                            .window
                            .participants()
                            .iter()
                            .copied()
                            .filter(|&p| p != controller)
                            .collect(),
                        crate::program::AffectedPlayers::EachOpponent => self
                            .combat_rules
                            .opponents(self.window.participants(), controller)?,
                    };
                    let opp_vec = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let mut count = 0usize;
                    for seat in seats {
                        if let Some(records) = self.characteristics.sacrificed_this_turn.get(&seat)
                        {
                            count += records
                                .iter()
                                .filter(|rec| {
                                    matches_zone_move_record(**filter, rec, controller, &opp_vec)
                                })
                                .count();
                        }
                    }
                    Ok(RulesInteger::from_usize(count))
                }
                PlayerStatExpr::DamageDealtThisTurn {
                    subject,
                    to_opponent_only,
                    combat_only,
                } => {
                    let target_id = match subject {
                        ValueObjectRef::AbilitySource => resolve_ability_source(),
                        ValueObjectRef::TriggerSource => cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .map(|t| t.source.object.id())
                            .or_else(resolve_ability_source),
                        ValueObjectRef::AttachedHost => {
                            resolve_ability_source().and_then(|src| match self.attached_to(src) {
                                Some(TargetRef::Object(host)) => Some(host),
                                _ => None,
                            })
                        }
                        ValueObjectRef::TargetObject => self
                            .window
                            .resolving()
                            .and_then(|entry| self.instruction_target(entry).ok())
                            .and_then(|bound| match bound.choice().target {
                                TargetRef::Object(id) => Some(id),
                                _ => None,
                            }),
                        ValueObjectRef::TriggerSubject
                        | ValueObjectRef::DyingSource
                        | ValueObjectRef::DyingCreature
                        | ValueObjectRef::EnteringPermanent
                        | ValueObjectRef::TriggeringSpell => cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .and_then(|t| t.causes.members().first())
                            .and_then(|m| m.subject_object_id()),
                        ValueObjectRef::RevealedTop => {
                            cursor.and_then(|c| c.results.revealed_top_card)
                        }
                        ValueObjectRef::LastAmassedArmy => {
                            cursor.and_then(|c| c.results.last_amassed_army)
                        }
                        _ => None,
                    };
                    let Some(id) = target_id else {
                        return Ok(0.into());
                    };
                    if *combat_only {
                        if !self
                            .characteristics
                            .combat_damage_dealt_to_player_this_turn
                            .contains(&id)
                        {
                            return Ok(0.into());
                        }
                        if *to_opponent_only
                            && !self
                                .characteristics
                                .damage_dealt_to_opponents_this_turn
                                .get(&id)
                                .is_some_and(|s| !s.is_empty())
                        {
                            return Ok(0.into());
                        }
                        Ok(self
                            .characteristics
                            .damage_dealt_this_turn_by
                            .get(&id)
                            .cloned()
                            .filter(RulesInteger::is_positive)
                            .unwrap_or_else(|| 1.into()))
                    } else if *to_opponent_only {
                        Ok(
                            match self
                                .characteristics
                                .damage_dealt_to_opponents_this_turn
                                .get(&id)
                            {
                                Some(opps) if !opps.is_empty() => self
                                    .characteristics
                                    .damage_dealt_this_turn_by
                                    .get(&id)
                                    .cloned()
                                    .filter(RulesInteger::is_positive)
                                    .unwrap_or_else(|| RulesInteger::from_usize(opps.len())),
                                _ => 0.into(),
                            },
                        )
                    } else {
                        Ok(self
                            .characteristics
                            .damage_dealt_this_turn_by
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| 0.into()))
                    }
                }
                PlayerStatExpr::DamageReceivedThisTurn {
                    subject,
                    excess_only,
                } => {
                    let target_id = match subject {
                        ValueObjectRef::AbilitySource => resolve_ability_source(),
                        ValueObjectRef::TriggerSource => cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .map(|t| t.source.object.id())
                            .or_else(resolve_ability_source),
                        ValueObjectRef::AttachedHost => {
                            resolve_ability_source().and_then(|src| match self.attached_to(src) {
                                Some(TargetRef::Object(host)) => Some(host),
                                _ => None,
                            })
                        }
                        ValueObjectRef::TargetObject => self
                            .window
                            .resolving()
                            .and_then(|entry| self.instruction_target(entry).ok())
                            .and_then(|bound| match bound.choice().target {
                                TargetRef::Object(id) => Some(id),
                                _ => None,
                            }),
                        ValueObjectRef::TriggerSubject
                        | ValueObjectRef::DyingSource
                        | ValueObjectRef::DyingCreature
                        | ValueObjectRef::EnteringPermanent
                        | ValueObjectRef::TriggeringSpell => cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .and_then(|t| t.causes.members().first())
                            .and_then(|m| m.subject_object_id()),
                        ValueObjectRef::RevealedTop => {
                            cursor.and_then(|c| c.results.revealed_top_card)
                        }
                        ValueObjectRef::LastAmassedArmy => {
                            cursor.and_then(|c| c.results.last_amassed_army)
                        }
                        _ => None,
                    };
                    let Some(id) = target_id else {
                        return Ok(0.into());
                    };
                    if *excess_only {
                        Ok(i64::from(
                            self.characteristics
                                .excess_damage_received_this_turn
                                .contains(&id),
                        )
                        .into())
                    } else {
                        Ok(self
                            .characteristics
                            .object_damage_taken_this_turn
                            .get(&id)
                            .cloned()
                            .or_else(|| self.battlefield_marked_damage(id))
                            .unwrap_or_else(|| 0.into()))
                    }
                }
                PlayerStatExpr::ControlsGreatestStat {
                    player,
                    filter,
                    stat,
                } => {
                    let seat = if *player == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*player, cursor)?
                    };
                    let ability_source = resolve_ability_source();
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let mut best: Option<(RulesInteger, bool)> = None;
                    for object in evaluated.objects.values() {
                        if object.zone != ObjectZone::Battlefield
                            || !self.matches_object_filter(
                                **filter,
                                object,
                                controller,
                                ability_source,
                                &excluded,
                                &evaluated,
                            )
                        {
                            continue;
                        }
                        let val = match stat {
                            ObjectStatKind::Power => object
                                .power_toughness
                                .as_ref()
                                .map(|pt| pt.derived.power.clone()),
                            ObjectStatKind::Toughness => object
                                .power_toughness
                                .as_ref()
                                .map(|pt| pt.derived.toughness.clone()),
                            ObjectStatKind::ManaValue => self.permanent_mana_value(object.object),
                            ObjectStatKind::Loyalty => {
                                Some(self.counters_of_kind(object.object, CounterKind::Loyalty))
                            }
                        };
                        let Some(val) = val else {
                            continue;
                        };
                        let ours = object.controller == Some(seat);
                        best = Some(match best {
                            None => (val, ours),
                            Some((cur_max, cur_ours)) => match val.cmp(&cur_max) {
                                std::cmp::Ordering::Greater => (val, ours),
                                std::cmp::Ordering::Equal => (cur_max, cur_ours || ours),
                                std::cmp::Ordering::Less => (cur_max, cur_ours),
                            },
                        });
                    }
                    Ok(i64::from(best.is_some_and(|(_, ours)| ours)).into())
                }
                PlayerStatExpr::PlayerHasMostLife { player, allow_tie } => {
                    let seat = if *player == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*player, cursor)?
                    };
                    if !self.window.participants().contains(&seat) {
                        return Ok(0.into());
                    }
                    let Some(own_life) = self.life.get(seat) else {
                        return Ok(0.into());
                    };
                    let is_most = self
                        .window
                        .participants()
                        .iter()
                        .copied()
                        .filter(|&other| other != seat)
                        .filter_map(|other| self.life.get(other))
                        .all(|other_life| {
                            if *allow_tie {
                                own_life >= other_life
                            } else {
                                own_life > other_life
                            }
                        });
                    Ok(i64::from(is_most).into())
                }
                PlayerStatExpr::PlayerControlsMost {
                    player,
                    zone,
                    filter,
                    allow_tie,
                } => {
                    let seat = if *player == crate::program::ResolvingPlayer::Controller {
                        controller
                    } else {
                        self.resolving_player_at(*player, cursor)?
                    };
                    if !self.window.participants().contains(&seat) {
                        return Ok(0.into());
                    }
                    let ability_source = resolve_ability_source();
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let count_for = |target_seat: SeatId| -> usize {
                        match *zone {
                            ValueZone::Battlefield => {
                                let scoped = filter.of_player(target_seat);
                                evaluated
                                    .objects
                                    .values()
                                    .filter(|o| {
                                        o.zone == ObjectZone::Battlefield
                                            && self.matches_object_filter(
                                                scoped,
                                                o,
                                                controller,
                                                ability_source,
                                                &excluded,
                                                &evaluated,
                                            )
                                    })
                                    .count()
                            }
                            ValueZone::Graveyard(_) => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Graveyard(target_seat)
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::Hand(_) => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Hand(target_seat)
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::Library(_) => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Library(target_seat)
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::Exile(_) => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    o.zone == ObjectZone::Exile
                                        && o.owner == target_seat
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::AllGraveyards => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    matches!(o.zone, ObjectZone::Graveyard(_))
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::AllHands => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    matches!(o.zone, ObjectZone::Hand(_))
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::OpponentsHands => evaluated
                                .objects
                                .values()
                                .filter(|o| {
                                    matches!(o.zone, ObjectZone::Hand(s) if s != seat)
                                        && self.matches_object_filter(
                                            **filter,
                                            o,
                                            controller,
                                            ability_source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .count(),
                            ValueZone::LinkedExile => ability_source
                                .or_else(resolve_ability_source)
                                .map_or(0, |src| {
                                    evaluated
                                        .objects
                                        .values()
                                        .filter(|o| {
                                            o.zone == ObjectZone::Exile
                                                && (self
                                                    .characteristics
                                                    .imprinted_cards
                                                    .get(&src)
                                                    .is_some_and(|list| list.contains(&o.object))
                                                    || self
                                                        .characteristics
                                                        .ability_donors_in_exile
                                                        .get(&src)
                                                        .is_some_and(|list| {
                                                            list.contains(&o.object)
                                                        })
                                                    || self
                                                        .characteristics
                                                        .linked_exiles
                                                        .get(&src)
                                                        .is_some_and(|list| {
                                                            list.contains(&o.object)
                                                        }))
                                                && self.matches_object_filter(
                                                    **filter,
                                                    o,
                                                    controller,
                                                    Some(src),
                                                    &excluded,
                                                    &evaluated,
                                                )
                                        })
                                        .count()
                                }),
                        }
                    };
                    let own = count_for(seat);
                    let is_most = self
                        .window
                        .participants()
                        .iter()
                        .copied()
                        .filter(|&other| other != seat)
                        .all(|other| {
                            let other_count = count_for(other);
                            if *allow_tie {
                                own >= other_count
                            } else {
                                own > other_count
                            }
                        });
                    Ok(i64::from(is_most).into())
                }
                PlayerStatExpr::IsInStep { step, player } => {
                    let step_ok = self.current_step == Some(*step);
                    let player_ok = match *player {
                        None => true,
                        Some(role) => {
                            let seat = if role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(role, cursor)?
                            };
                            seat == self.window.active_player()
                        }
                    };
                    Ok(i64::from(step_ok && player_ok).into())
                }
                PlayerStatExpr::IsInPhase { phase, player } => {
                    use crate::program::PhaseCheck;
                    use crate::turn::Step;
                    let phase_ok = self.current_step.is_some_and(|step| match phase {
                        PhaseCheck::Beginning => {
                            matches!(step, Step::Untap | Step::Upkeep | Step::Draw)
                        }
                        PhaseCheck::Main => {
                            matches!(step, Step::PrecombatMain | Step::PostcombatMain)
                        }
                        PhaseCheck::Combat => matches!(
                            step,
                            Step::BeginningCombat
                                | Step::DeclareAttackers
                                | Step::DeclareBlockers
                                | Step::CombatDamage
                                | Step::EndCombat
                        ),
                        PhaseCheck::Ending => matches!(step, Step::End | Step::Cleanup),
                    });
                    let player_ok = match *player {
                        None => true,
                        Some(role) => {
                            let seat = if role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(role, cursor)?
                            };
                            seat == self.window.active_player()
                        }
                    };
                    Ok(i64::from(phase_ok && player_ok).into())
                }
                PlayerStatExpr::AttackedMatchingThisTurn { players, filter } => {
                    let seats = match *players {
                        crate::program::AffectedPlayers::One(role) => {
                            vec![if role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(role, cursor)?
                            }]
                        }
                        crate::program::AffectedPlayers::EachPlayer => {
                            self.window.participants().to_vec()
                        }
                        crate::program::AffectedPlayers::EachOtherPlayer => self
                            .window
                            .participants()
                            .iter()
                            .copied()
                            .filter(|&p| p != controller)
                            .collect(),
                        crate::program::AffectedPlayers::EachOpponent => self
                            .combat_rules
                            .opponents(self.window.participants(), controller)?,
                    };
                    let opp_vec = self
                        .combat_rules
                        .opponents(self.window.participants(), controller)?;
                    let mut count = 0usize;
                    for seat in seats {
                        if let Some(records) =
                            self.characteristics.attacked_records_this_turn.get(&seat)
                        {
                            count += records
                                .iter()
                                .filter(|rec| {
                                    matches_zone_move_record(**filter, rec, controller, &opp_vec)
                                })
                                .count();
                        }
                    }
                    Ok(RulesInteger::from_usize(count))
                }
            },
            ValueExpr::ObjectMatches { subject, filter } => {
                let (clause_a, _) = filter.clauses();
                let matched = match subject {
                    MatchObjectRef::RevealedTop => cursor.is_some_and(|c| {
                        clause_a
                            .any_types
                            .matches_any(&c.results.revealed_top_types)
                            && !c.results.revealed_top_types.is_empty()
                    }),
                    MatchObjectRef::DiscardedCard => cursor.is_some_and(|c| {
                        clause_a.any_types.matches_any(&c.results.discarded_types)
                            && !c.results.discarded_types.is_empty()
                    }),
                    MatchObjectRef::MovedLibraryCard => {
                        let c = cursor.ok_or(EffectError::WrongResolutionFrame)?;
                        clause_a
                            .any_types
                            .matches_any(&c.results.moved_library_types)
                            && !c.results.moved_library_types.is_empty()
                    }
                    MatchObjectRef::DestructionTarget => {
                        let SubtypeFilter::Has(kind) = clause_a.subtypes else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        cursor
                            .is_some_and(|c| c.results.destruction_target_subtypes.contains(&kind))
                    }
                    MatchObjectRef::DyingCreature => {
                        let SubtypeFilter::Has(kind) = clause_a.subtypes else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let binding = self
                            .window
                            .resolving()
                            .and_then(|entry| self.execution_bindings(entry.id()))
                            .and_then(|bindings| bindings.trigger.as_ref())
                            .ok_or(EffectError::InconsistentProgram)?;
                        let [member] = binding.causes.members() else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                            &member.related
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        before.is_creature() && before.creature_subtypes.contains(&kind)
                    }
                    MatchObjectRef::SacrificedCreature => {
                        let entry = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get(&entry.id())
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        if let Some(lki) = payload.sacrificed_lki.as_deref() {
                            let evaluated = self.evaluated_characteristics()?;
                            let excluded = BTreeSet::new();
                            self.matches_object_filter(
                                **filter,
                                lki,
                                controller,
                                resolve_ability_source(),
                                &excluded,
                                &evaluated,
                            )
                        } else if let SubtypeFilter::Has(kind) = clause_a.subtypes {
                            payload.sacrificed_creature_subtypes.contains(&kind)
                        } else {
                            false
                        }
                    }
                    MatchObjectRef::AbilitySource => {
                        let source_id =
                            resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        evaluated.get(source_id).is_some_and(|permanent| {
                            self.matches_object_filter(
                                **filter,
                                permanent,
                                controller,
                                Some(source_id),
                                &excluded,
                                &evaluated,
                            )
                        })
                    }
                    MatchObjectRef::TargetObject => {
                        let entry = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let TargetRef::Object(object) =
                            self.instruction_target(entry)?.choice().target
                        else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        evaluated.get(object).is_some_and(|permanent| {
                            self.matches_object_filter(
                                **filter, permanent, controller, None, &excluded, &evaluated,
                            )
                        })
                    }
                    MatchObjectRef::TriggerSubject => {
                        let binding = cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .ok_or(EffectError::InconsistentProgram)?;
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        let source_id = resolve_ability_source();
                        binding
                            .causes
                            .members()
                            .first()
                            .and_then(|m| m.subject_object_id())
                            .and_then(|id| evaluated.get(id))
                            .is_some_and(|permanent| {
                                self.matches_object_filter(
                                    **filter, permanent, controller, source_id, &excluded,
                                    &evaluated,
                                )
                            })
                    }
                    MatchObjectRef::AttachedHost => {
                        let source_id =
                            resolve_ability_source().ok_or(EffectError::InconsistentProgram)?;
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        match self.attached_to(source_id) {
                            Some(TargetRef::Object(host)) => {
                                evaluated.get(host).is_some_and(|permanent| {
                                    self.matches_object_filter(
                                        **filter,
                                        permanent,
                                        controller,
                                        Some(source_id),
                                        &excluded,
                                        &evaluated,
                                    )
                                })
                            }
                            _ => false,
                        }
                    }
                    MatchObjectRef::LastAffected
                    | MatchObjectRef::LastMovedToGraveyard
                    | MatchObjectRef::LastMovedToExile
                    | MatchObjectRef::LastMovedToHand
                    | MatchObjectRef::LastMovedToBattlefield => {
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        let source_id = resolve_ability_source();
                        let check_zone = |zone: ObjectZone| match subject {
                            MatchObjectRef::LastAffected => true,
                            MatchObjectRef::LastMovedToGraveyard => {
                                matches!(zone, ObjectZone::Graveyard(_))
                            }
                            MatchObjectRef::LastMovedToExile => zone == ObjectZone::Exile,
                            MatchObjectRef::LastMovedToHand => matches!(zone, ObjectZone::Hand(_)),
                            MatchObjectRef::LastMovedToBattlefield => {
                                zone == ObjectZone::Battlefield
                            }
                            _ => true,
                        };
                        cursor.is_some_and(|c| {
                            let candidates = if !c.results.last_moved_objects.is_empty() {
                                c.results.last_moved_objects.as_slice()
                            } else if !c.results.destroyed_cards_to_graveyard.is_empty() {
                                c.results.destroyed_cards_to_graveyard.as_slice()
                            } else {
                                &[]
                            };
                            candidates.iter().any(|&id| {
                                evaluated.get(id).is_some_and(|permanent| {
                                    check_zone(permanent.zone)
                                        && self.matches_object_filter(
                                            **filter, permanent, controller, source_id, &excluded,
                                            &evaluated,
                                        )
                                })
                            })
                        })
                    }
                    MatchObjectRef::LibraryTop(role) => {
                        let seat = if *role == crate::program::ResolvingPlayer::Controller {
                            controller
                        } else {
                            self.resolving_player_at(*role, cursor)?
                        };
                        let Some(&top_id) = self.window.cards().library(seat).first() else {
                            return Ok(0.into());
                        };
                        let evaluated = self.evaluated_characteristics()?;
                        let excluded = BTreeSet::new();
                        let source_id = resolve_ability_source();
                        evaluated.get(top_id).is_some_and(|card| {
                            self.matches_object_filter(
                                **filter, card, controller, source_id, &excluded, &evaluated,
                            )
                        })
                    }
                };
                Ok(i64::from(matched).into())
            }
            ValueExpr::EventValue(kind) => match kind {
                EventValueKind::ChosenX => {
                    let entry = self
                        .window
                        .resolving()
                        .filter(|entry| cursor.is_none_or(|c| c.entry == entry.id()))
                        .ok_or(EffectError::InconsistentProgram)?;
                    if let Some(x) = entry.chosen_x() {
                        return Ok(x.clone());
                    }
                    if let Some(binding) = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        && let [member] = binding.causes.members()
                        && let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                            &member.related
                    {
                        return Ok(before.spell_x.clone().unwrap_or_else(|| 0.into()));
                    }
                    Ok(0.into())
                }
                EventValueKind::EnteredKickerX => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(before.spell_x.clone().unwrap_or_else(|| 0.into()))
                }
                EventValueKind::EnteredKickerCount(index) => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(before
                        .kicker_choices
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| 0.into()))
                }
                EventValueKind::KickerCount(index) => Ok(self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?
                    .kicker_choices()
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::PaidAlternative => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .paid_conditional_cost(),
                )
                .into()),
                EventValueKind::Bargained => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .bargained(),
                )
                .into()),
                EventValueKind::CastDuringOwnMain => Ok(i64::from(
                    self.window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .controller_cast_during_own_main(),
                )
                .into()),
                EventValueKind::CastFromGraveyard => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let v = match entry.kind() {
                        StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => {
                            entry.cast_from_graveyard || entry.cast_summary().cast_from_graveyard
                        }
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => self
                            .characteristics
                            .cast_summaries
                            .get(&source.id())
                            .is_some_and(|s| s.cast_from_graveyard),
                        _ => false,
                    };
                    Ok(i64::from(v).into())
                }
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
                | EventValueKind::AdditionalCostPaid => {
                    let summary = self
                        .window
                        .resolving()
                        .and_then(|entry| match entry.kind() {
                            StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => {
                                Some(entry.cast_summary())
                            }
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => {
                                self.characteristics.cast_summaries.get(&source.id())
                            }
                            _ => None,
                        });
                    let val: RulesInteger = match kind {
                        EventValueKind::ManaSpentToCast => {
                            summary.map_or_else(|| 0.into(), |s| s.total_mana.clone())
                        }
                        EventValueKind::ColorsSpentToCast => summary.map_or_else(
                            || 0.into(),
                            |s| RulesInteger::from_usize(s.colors_spent.len()),
                        ),
                        EventValueKind::ManaTypeSpentToCast(mt) => summary
                            .and_then(|s| s.mana_by_type.get(mt))
                            .cloned()
                            .unwrap_or_else(|| 0.into()),
                        EventValueKind::SnowManaSpentToCast => {
                            summary.map_or_else(|| 0.into(), |s| s.snow_mana.clone())
                        }
                        EventValueKind::TreasureManaSpentToCast => {
                            summary.map_or_else(|| 0.into(), |s| s.treasure_mana.clone())
                        }
                        EventValueKind::CreatureManaSpentToCast => {
                            summary.map_or_else(|| 0.into(), |s| s.creature_mana.clone())
                        }
                        EventValueKind::CastFromOutsideHand => {
                            i64::from(summary.is_some_and(|s| s.cast_from_outside_hand)).into()
                        }
                        EventValueKind::CastFromExile => {
                            i64::from(summary.is_some_and(|s| s.cast_from_exile)).into()
                        }
                        EventValueKind::GiftPromised => {
                            i64::from(summary.is_some_and(|s| s.gift_promised)).into()
                        }
                        EventValueKind::Cleaved => {
                            i64::from(summary.is_some_and(|s| s.cleaved)).into()
                        }
                        EventValueKind::EvidenceCollected => {
                            i64::from(summary.is_some_and(|s| s.evidence_collected)).into()
                        }
                        EventValueKind::AdditionalCostPaid => {
                            i64::from(summary.is_some_and(|s| s.additional_cost_paid)).into()
                        }
                        _ => 0.into(),
                    };
                    Ok(val)
                }
                EventValueKind::TriggeringSpellManaSpent => {
                    let val = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .and_then(|t| t.causes.members().first())
                        .and_then(|cause| match &cause.related {
                            crate::occurrence::TriggerRelated::SpellCast(cast) => {
                                Some(cast.total_mana_spent().clone())
                            }
                            crate::occurrence::TriggerRelated::ZoneChange { before, after } => self
                                .characteristics
                                .cast_summaries
                                .get(&after.object.id())
                                .or_else(|| {
                                    self.characteristics.cast_summaries.get(&before.object.id())
                                })
                                .map(|s| s.total_mana.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| 0.into());
                    Ok(val)
                }
                EventValueKind::MatchedOccurrences => {
                    self.resolution_trigger_count_at(controller, cursor)
                }
                EventValueKind::PaidCreatureTaps => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(RulesInteger::from_usize(payload.paid_creature_taps))
                }
                EventValueKind::PaidNamedCounters => {
                    let entry = cursor.ok_or(EffectError::WrongResolutionFrame)?.entry;
                    let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    payload
                        .paid_named_counters
                        .clone()
                        .ok_or(EffectError::InconsistentProgram)
                }
                EventValueKind::LargestDiscard => cursor
                    .and_then(|c| c.largest_discard)
                    .map(RulesInteger::from_usize)
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::DestroyedPermanents => cursor
                    .and_then(|c| c.destroyed_permanents)
                    .map(RulesInteger::from_usize)
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::ExiledMilledCreaturePower => cursor
                    .and_then(|c| c.impulse.exiled_milled_power.clone())
                    .ok_or(EffectError::InconsistentProgram),
                EventValueKind::MilledCardsManaValueSum => {
                    let values = &cursor
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .results
                        .milled_mana_values;
                    Ok(values
                        .iter()
                        .fold(0.into(), |total: RulesInteger, value| total.plus(value)))
                }
                EventValueKind::MilledCardsManaValueMax => {
                    let values = &cursor
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .results
                        .milled_mana_values;
                    Ok(values.iter().max().cloned().unwrap_or_else(|| 0.into()))
                }
                EventValueKind::DeclaredSubtypeAttackers => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::DeclaredSubtypeAttackers(attackers) =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(RulesInteger::from_usize(attackers.len()))
                }
                EventValueKind::DeclaredAttackersAtController => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::InconsistentProgram)?
                        .id();
                    let binding = self
                        .execution_bindings(entry)
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = binding.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::AttackGroup { creatures, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let count = creatures
                        .iter()
                        .filter(|object| {
                            let current = self
                                .combat
                                .as_ref()
                                .and_then(|combat| combat.attacker(**object))
                                .map(|attacker| attacker.defender());
                            current.or_else(|| {
                                self.departed_attack_defenders
                                    .as_ref()
                                    .and_then(|values| values.get(*object).map(|(seat, _)| *seat))
                            }) == Some(controller)
                        })
                        .count();
                    Ok(RulesInteger::from_usize(count))
                }
                EventValueKind::TriggeringLifeChange => {
                    let trigger = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    let [member] = trigger.causes.members() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let crate::occurrence::TriggerRelated::LifeChanged(change) = &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(change.amount.clone())
                }
                EventValueKind::CombatDamageToPlayer => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    match binding.causes.members() {
                        [member] => match &member.related {
                            crate::occurrence::TriggerRelated::CombatDamageToPlayer {
                                amount,
                                ..
                            }
                            | crate::occurrence::TriggerRelated::DamageToOpponent {
                                amount, ..
                            }
                            | crate::occurrence::TriggerRelated::CombatDamageToPlaneswalker {
                                amount,
                                ..
                            }
                            | crate::occurrence::TriggerRelated::SubtypeCombatDamage {
                                amount,
                                ..
                            }
                            | crate::occurrence::TriggerRelated::DamageDealtToPermanent {
                                amount,
                                ..
                            } => Ok(amount.clone()),
                            _ => Err(EffectError::InconsistentProgram),
                        },
                        _ => Err(EffectError::InconsistentProgram),
                    }
                }
                EventValueKind::DamageReceived => {
                    let binding = cursor
                        .and_then(|c| self.execution_bindings(c.entry))
                        .and_then(|b| b.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    binding.causes.members().iter().try_fold(
                        RulesInteger::from(0),
                        |total, member| match &member.related {
                            crate::occurrence::TriggerRelated::DamageReceived {
                                amount, ..
                            } => Ok(total.plus(amount)),
                            _ => Err(EffectError::InconsistentProgram),
                        },
                    )
                }
                EventValueKind::CreaturesDiedThisTurn => Ok(self.creatures_died_this_turn.clone()),
                EventValueKind::LastSacrificeSucceeded => {
                    Ok(i64::from(cursor.is_some_and(|c| c.results.last_sacrifice)).into())
                }
                EventValueKind::LastActionSucceeded => {
                    Ok(i64::from(cursor.is_some_and(|c| c.results.last_action_succeeded)).into())
                }
                EventValueKind::LastActionCount => Ok(cursor
                    .and_then(|c| c.results.last_action_count.clone())
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::SelectedGraveyardExileCreatures => {
                    Ok(RulesInteger::from_usize(cursor.map_or(0, |c| {
                        c.results.selected_graveyard_exile_creatures
                    })))
                }
                EventValueKind::SourceAttackedThisTurn => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let binding = self
                        .execution_bindings(entry.id())
                        .and_then(|binding| binding.trigger.as_ref())
                        .ok_or(EffectError::InconsistentProgram)?;
                    Ok(
                        i64::from(self.triggers.attacked_this_turn(binding.source.object.id()))
                            .into(),
                    )
                }
                EventValueKind::TriggerSourceOnBattlefield => {
                    let Some(StackKind::TriggeredAbility { source }) = self
                        .window
                        .resolving()
                        .map(crate::stack_window::StackEntry::kind)
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(i64::from(
                        self.evaluated_characteristics()?
                            .get(source.id())
                            .is_some_and(|object| object.zone == ObjectZone::Battlefield),
                    )
                    .into())
                }
                EventValueKind::LastDieRoll => Ok(cursor
                    .and_then(|c| c.results.last_die_roll.clone())
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::CoinFlipsWon => Ok(cursor
                    .and_then(|c| c.results.last_coin_flips_won.clone())
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::LastCoinFlipWon => Ok(i64::from(
                    cursor.is_some_and(|c| c.results.last_coin_flip_won == Some(true)),
                )
                .into()),
                EventValueKind::ChosenNumber => Ok(cursor
                    .and_then(|c| c.results.chosen_number.clone())
                    .unwrap_or_else(|| 0.into())),
                EventValueKind::ChosenOptionIndex => Ok(cursor
                    .and_then(|c| c.results.chosen_option_index)
                    .map_or_else(|| 0.into(), |idx| i64::from(idx).into())),
                EventValueKind::VoteCount(idx) => Ok(cursor
                    .and_then(|c| c.results.vote_counts.get(usize::from(*idx)).copied())
                    .map_or_else(|| 0.into(), |count| i64::from(count).into())),
                EventValueKind::WinningVoteOption => Ok(cursor
                    .and_then(|c| c.results.winning_vote_option)
                    .map_or_else(|| (-1).into(), |idx| i64::from(idx).into())),
                EventValueKind::LastNonlandDiscarded => {
                    Ok(i64::from(cursor.map_or(0, |c| c.results.last_nonland_discarded)).into())
                }
                EventValueKind::DamagePreventedThisWay => Ok(cursor.map_or_else(
                    || 0.into(),
                    |c| c.results.damage_prevented_this_resolution.clone(),
                )),
                EventValueKind::DealtDamageThisWay { combat_only } => {
                    let dealt = if *combat_only {
                        cursor
                            .and_then(|c| self.execution_bindings(c.entry))
                            .and_then(|b| b.trigger.as_ref())
                            .is_some_and(|t| {
                                t.causes.members().iter().any(|m| {
                                    matches!(
                                        &m.related,
                                        crate::occurrence::TriggerRelated::CombatDamageToPlayer { .. }
                                            | crate::occurrence::TriggerRelated::CombatDamageToPlaneswalker { .. }
                                            | crate::occurrence::TriggerRelated::SubtypeCombatDamage { .. }
                                    )
                                })
                            })
                    } else {
                        cursor.is_some_and(|c| c.results.dealt_damage_this_resolution)
                    };
                    Ok(i64::from(dealt).into())
                }
                EventValueKind::SharesCardTypeWithLinkedExile(subject) => {
                    let Some(source_id) = resolve_ability_source() else {
                        return Ok(0.into());
                    };
                    let evaluated = self.evaluated_characteristics()?;
                    let mut exiled_types = BTreeSet::new();
                    for list in [
                        self.characteristics.linked_exiles.get(&source_id),
                        self.characteristics.imprinted_cards.get(&source_id),
                        self.characteristics.ability_donors_in_exile.get(&source_id),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        for id in list {
                            if let Some(card) = evaluated.get(*id)
                                && card.zone == ObjectZone::Exile
                            {
                                exiled_types.extend(card.current_types.iter().copied());
                            }
                        }
                    }
                    for ((src, _), ids) in &self.independent_exile_links {
                        if *src == source_id {
                            for id in ids {
                                if let Some(card) = evaluated.get(*id)
                                    && card.zone == ObjectZone::Exile
                                {
                                    exiled_types.extend(card.current_types.iter().copied());
                                }
                            }
                        }
                    }
                    if exiled_types.is_empty() {
                        return Ok(0.into());
                    }
                    let mut subject_types = BTreeSet::new();
                    match subject {
                        MatchObjectRef::AbilitySource => {
                            if let Some(card) = evaluated.get(source_id) {
                                subject_types.extend(card.current_types.iter().copied());
                            }
                        }
                        MatchObjectRef::TargetObject => {
                            if let Some(entry) = self.window.resolving()
                                && let Ok(bound) = self.instruction_target(entry)
                            {
                                match bound.choice().target {
                                    TargetRef::Object(id) => {
                                        if let Some(card) = evaluated.get(id) {
                                            subject_types
                                                .extend(card.current_types.iter().copied());
                                        }
                                    }
                                    TargetRef::Stack(sid) => {
                                        if let Some(traits) = self.stack_copiable_traits(sid) {
                                            subject_types.extend(traits.card_types.iter().copied());
                                        }
                                    }
                                    TargetRef::Player(_) => {}
                                }
                            }
                        }
                        MatchObjectRef::TriggerSubject => {
                            if let Some(id) = cursor
                                .and_then(|c| self.execution_bindings(c.entry))
                                .and_then(|b| b.trigger.as_ref())
                                .and_then(|t| t.causes.members().first())
                                .and_then(|m| m.subject_object_id())
                                && let Some(card) = evaluated.get(id)
                            {
                                subject_types.extend(card.current_types.iter().copied());
                            }
                        }
                        MatchObjectRef::AttachedHost => {
                            if let Some(TargetRef::Object(host)) = self.attached_to(source_id)
                                && let Some(card) = evaluated.get(host)
                            {
                                subject_types.extend(card.current_types.iter().copied());
                            }
                        }
                        MatchObjectRef::RevealedTop => {
                            if let Some(c) = cursor {
                                subject_types.extend(c.results.revealed_top_types.iter().copied());
                            }
                        }
                        MatchObjectRef::DiscardedCard => {
                            if let Some(c) = cursor {
                                subject_types.extend(c.results.discarded_types.iter().copied());
                            }
                        }
                        MatchObjectRef::MovedLibraryCard => {
                            if let Some(c) = cursor {
                                subject_types.extend(c.results.moved_library_types.iter().copied());
                            }
                        }
                        MatchObjectRef::LibraryTop(role) => {
                            let seat = if *role == crate::program::ResolvingPlayer::Controller {
                                controller
                            } else {
                                self.resolving_player_at(*role, cursor)?
                            };
                            if let Some(&top_id) = self.window.cards().library(seat).first()
                                && let Some(card) = evaluated.get(top_id)
                            {
                                subject_types.extend(card.current_types.iter().copied());
                            }
                        }
                        MatchObjectRef::LastAffected
                        | MatchObjectRef::LastMovedToGraveyard
                        | MatchObjectRef::LastMovedToExile
                        | MatchObjectRef::LastMovedToHand
                        | MatchObjectRef::LastMovedToBattlefield => {
                            if let Some(c) = cursor {
                                for &id in &c.results.last_moved_objects {
                                    if let Some(card) = evaluated.get(id) {
                                        subject_types.extend(card.current_types.iter().copied());
                                    }
                                }
                            }
                        }
                        MatchObjectRef::DyingCreature => {
                            if let Some(binding) = cursor
                                .and_then(|c| self.execution_bindings(c.entry))
                                .and_then(|b| b.trigger.as_ref())
                                && let Some(member) = binding.causes.members().first()
                                && let crate::occurrence::TriggerRelated::ZoneChange {
                                    before, ..
                                } = &member.related
                            {
                                subject_types.extend(before.types.iter().copied());
                            }
                        }
                        MatchObjectRef::DestructionTarget | MatchObjectRef::SacrificedCreature => {}
                    }
                    Ok(i64::from(!subject_types.is_disjoint(&exiled_types)).into())
                }
            },
        }
    }
}
