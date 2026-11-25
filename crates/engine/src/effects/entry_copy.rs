//! An entry-copy choice precedes general entry replacements and uses the unchanged donor world.
use super::*;
use crate::program::{EntryCopyRule, Supertype};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EntryCopyResult {
    pub(super) definition: Arc<Program>,
    pub(super) extra_counters: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyWorldSnapshot {
    active_player: SeatId,
    participants: Vec<SeatId>,
    life: crate::life::LifeState,
    combat: Option<crate::combat::CombatState>,
    cards: GameObjects,
    programs: Arc<BTreeMap<CardId, Program>>,
    state: CharacteristicState,
    objects: BTreeMap<ObjectId, ObjectCharacteristics>,
    name_rules: Arc<spellwake_contracts::names::NameRules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct EntryCopyPool {
    programs: BTreeMap<ObjectId, Arc<Program>>,
    snapshots: BTreeMap<ObjectId, ObjectCharacteristics>,
    entrant_names: BTreeMap<ObjectId, spellwake_contracts::names::ObjectNames>,
    world: Option<Arc<CopyWorldSnapshot>>,
    all_battlefield: Vec<ObjectId>,
    all_graveyard: Vec<ObjectId>,
    graves: Vec<ObjectId>,
    creatures: Vec<ObjectId>,
    controlled_creatures: BTreeMap<SeatId, Vec<ObjectId>>,
    artifacts: Vec<ObjectId>,
    enchantments: Vec<ObjectId>,
    controlled: BTreeMap<SeatId, Vec<ObjectId>>,
    battlefield_replacements: BTreeMap<SeatId, Vec<(ObjectId, ObjectId)>>,
    token_replacements: BTreeMap<SeatId, Vec<(ObjectId, ObjectId)>>,
}

impl EntryCopyPool {
    pub(super) fn has_battlefield_copy_rules(
        current: &EvaluatedCharacteristics,
        for_tokens: bool,
    ) -> bool {
        use crate::characteristics::{StaticContinuousAbility as Ability, StaticModifierRule};
        current.objects.values().any(|source| {
            source.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    Ability::Modifier {
                        rule: StaticModifierRule::EnterControlledCreaturesAsSourceCopy,
                        ..
                    }
                )
            }) || (!for_tokens
                && source.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        Ability::Modifier {
                            rule: StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy,
                            ..
                        }
                    )
                }))
        })
    }

    pub(super) fn capture(
        window: &EffectWindow,
        current: &EvaluatedCharacteristics,
    ) -> Result<Self, EffectError> {
        let mut pool = Self::default();
        for object in window.window.cards().iter() {
            if let Some(face) = window.prospective_entry_face(object.id()) {
                pool.entrant_names.insert(object.id(), face.names.clone());
            }
            let Some(card) = current.get(object.id()) else {
                continue;
            };
            match card.zone {
                ObjectZone::Graveyard(_) if object.card_id().is_some() => {
                    pool.all_graveyard.push(object.id());
                    if card.is_creature() {
                        pool.graves.push(object.id());
                    }
                }
                ObjectZone::Battlefield => {
                    pool.all_battlefield.push(object.id());
                    if card.is_creature()
                        || card.has_type(CardType::Planeswalker)
                        || card.has_type(CardType::Artifact)
                        || card.has_type(CardType::Enchantment)
                    {
                        let controller = card.controller.ok_or(EffectError::InconsistentProgram)?;
                        if card.is_creature() {
                            pool.creatures.push(object.id());
                            pool.controlled_creatures
                                .entry(controller)
                                .or_default()
                                .push(object.id());
                        }
                        if card.has_type(CardType::Artifact) {
                            pool.artifacts.push(object.id());
                        }
                        if card.has_type(CardType::Enchantment) {
                            pool.enchantments.push(object.id());
                        }
                        if card.is_creature() || card.has_type(CardType::Planeswalker) {
                            pool.controlled
                                .entry(controller)
                                .or_default()
                                .push(object.id());
                        }
                    }
                }
                _ => continue,
            }
            let donor = window
                .copiable_program(object.id())
                .ok_or(EffectError::InconsistentProgram)?;
            pool.programs.insert(object.id(), Arc::new(donor.clone()));
            pool.snapshots.insert(object.id(), card.clone());
        }
        for aura in current.objects.values().filter(|view| view.has_battlefield_rule(|rule| matches!(rule,
            crate::characteristics::StaticContinuousAbility::Modifier { rule: crate::characteristics::StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy, .. }
        ))) {
            let (Some(TargetRef::Object(host)), Some(player)) = (aura.attached_to, aura.controller) else { continue; };
            if current.get(host).is_none_or(|view| view.zone != ObjectZone::Battlefield || !view.is_creature()) {
                continue;
            }
            pool.battlefield_replacements.entry(player).or_default().push((aura.object, host));
        }
        for source in current.objects.values().filter(|view| view.has_battlefield_rule(|rule| matches!(rule,
            crate::characteristics::StaticContinuousAbility::Modifier { rule: crate::characteristics::StaticModifierRule::EnterControlledCreaturesAsSourceCopy, .. }
        ))) {
            let player = source.controller.ok_or(EffectError::InconsistentProgram)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = pool.programs.entry(source.object) {
                let program = window.copiable_program(source.object).ok_or(EffectError::InconsistentProgram)?;
                entry.insert(Arc::new(program.clone()));
            }
            pool.battlefield_replacements.entry(player).or_default().push((source.object, source.object));
            pool.token_replacements.entry(player).or_default().push((source.object, source.object));
        }
        pool.world = Some(Arc::new(CopyWorldSnapshot {
            active_player: window.window.active_player(),
            participants: window.window.participants().to_vec(),
            life: window.life.clone(),
            combat: window.combat.clone(),
            cards: window.window.cards().clone(),
            programs: Arc::clone(&window.programs),
            state: (*window.characteristics).clone(),
            objects: current.objects.clone(),
            name_rules: Arc::clone(&window.name_rules),
        }));
        Ok(pool)
    }
    pub(super) fn battlefield_donors(&self, controller: SeatId) -> &[(ObjectId, ObjectId)] {
        self.battlefield_replacements
            .get(&controller)
            .map_or(&[], Vec::as_slice)
    }
    pub(super) fn token_donors(&self, controller: SeatId) -> &[(ObjectId, ObjectId)] {
        self.token_replacements
            .get(&controller)
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn battlefield_copy(
        &self,
        donor: ObjectId,
        previous: Option<&EntryCopyResult>,
    ) -> Result<EntryCopyResult, EffectError> {
        let definition = self
            .programs
            .get(&donor)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        Ok(EntryCopyResult {
            definition,
            extra_counters: previous.map_or(0, |copy| copy.extra_counters),
        })
    }

    fn matching_domain(
        &self,
        zone: crate::program::EntryCopyZone,
        filter: crate::program::ObjectFilter,
        controller: SeatId,
    ) -> Vec<ObjectId> {
        let candidates = match zone {
            crate::program::EntryCopyZone::Battlefield => &self.all_battlefield,
            crate::program::EntryCopyZone::OwnGraveyard
            | crate::program::EntryCopyZone::AnyGraveyard => &self.all_graveyard,
        };
        let excluded = BTreeSet::new();
        let matches_clause = |clause: crate::program::ObjectClause,
                              snap: &ObjectCharacteristics| {
            let ctrl_ok = match clause.controller {
                crate::program::ControllerFilter::Any => true,
                crate::program::ControllerFilter::You => snap.controller == Some(controller),
                crate::program::ControllerFilter::Opponent
                | crate::program::ControllerFilter::NotYou => {
                    snap.controller.is_some_and(|s| s != controller)
                }
                crate::program::ControllerFilter::OwnerYou => snap.owner == controller,
                crate::program::ControllerFilter::OwnerOpponent => snap.owner != controller,
                crate::program::ControllerFilter::Player(s) => snap.controller == Some(s),
                crate::program::ControllerFilter::RelatedPlayer
                | crate::program::ControllerFilter::TargetPlayer
                | crate::program::ControllerFilter::TargetOpponent
                | crate::program::ControllerFilter::ActivePlayer
                | crate::program::ControllerFilter::DefendingPlayer => false,
            };
            if !ctrl_ok {
                return false;
            }
            if let Some(tok) = clause.token
                && snap.is_token != tok
            {
                return false;
            }
            clause.matches_static_characteristics(snap, &excluded, || {
                Some(snap.defined.mana_value.clone())
            })
        };
        candidates
            .iter()
            .copied()
            .filter(|id| {
                let Some(snap) = self.snapshots.get(id) else {
                    return false;
                };
                if zone == crate::program::EntryCopyZone::OwnGraveyard
                    && snap.zone != ObjectZone::Graveyard(controller)
                {
                    return false;
                }
                match filter {
                    crate::program::ObjectFilter::Clause(a) => matches_clause(a, snap),
                    crate::program::ObjectFilter::Either(a, b) => {
                        matches_clause(a, snap) || matches_clause(b, snap)
                    }
                }
            })
            .collect()
    }

    pub(super) fn domain(&self, rule: &EntryCopyRule, controller: SeatId) -> Vec<ObjectId> {
        match rule {
            EntryCopyRule::GraveyardCreature => self.graves.clone(),
            EntryCopyRule::BattlefieldCreature
            | EntryCopyRule::BattlefieldCreatureWithAbility(_) => self.creatures.clone(),
            EntryCopyRule::ControlledCreature => self
                .controlled_creatures
                .get(&controller)
                .cloned()
                .unwrap_or_default(),
            EntryCopyRule::BattlefieldArtifact => self.artifacts.clone(),
            EntryCopyRule::BattlefieldEnchantment => self.enchantments.clone(),
            EntryCopyRule::ControlledCreatureOrPlaneswalker => self
                .controlled
                .get(&controller)
                .cloned()
                .unwrap_or_default(),
            EntryCopyRule::Matching(spec) => {
                self.matching_domain(spec.zone, spec.filter, controller)
            }
            EntryCopyRule::MatchingExt(spec) => {
                self.matching_domain(spec.zone, spec.filter, controller)
            }
        }
    }

    pub(super) fn select(
        &self,
        rule: &EntryCopyRule,
        controller: SeatId,
        donor: ObjectId,
        previous: Option<&EntryCopyResult>,
    ) -> Result<EntryCopyResult, EffectError> {
        self.select_for_entrant(rule, controller, donor, previous, None)
    }

    pub(super) fn select_for_entrant(
        &self,
        rule: &EntryCopyRule,
        controller: SeatId,
        donor: ObjectId,
        previous: Option<&EntryCopyResult>,
        entrant: Option<ObjectId>,
    ) -> Result<EntryCopyResult, EffectError> {
        if !self.domain(rule, controller).contains(&donor) {
            return Err(EffectError::IllegalTarget);
        }
        let mut definition = self
            .programs
            .get(&donor)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        let mut extra_counters = previous.map_or(0, |copy| copy.extra_counters);
        if *rule == EntryCopyRule::ControlledCreatureOrPlaneswalker {
            Arc::make_mut(&mut definition)
                .supertypes
                .remove(&Supertype::Legendary);
            extra_counters = extra_counters
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
        }
        if let EntryCopyRule::BattlefieldCreatureWithAbility(ability) = rule {
            let definition = Arc::make_mut(&mut definition);
            let mut ability = (**ability).clone();
            let next = definition
                .activated
                .iter()
                .try_fold(0_u64, |candidate, ability| {
                    ability.id.0.checked_add(1).map(|next| next.max(candidate))
                })
                .ok_or(EffectError::InconsistentProgram)?;
            ability.id = crate::activated::ActivatedAbilityId(next);
            definition.activated.push(ability);
        }
        if let EntryCopyRule::Matching(spec) = rule {
            let def = Arc::make_mut(&mut definition);
            if spec.nonlegendary {
                def.supertypes.remove(&Supertype::Legendary);
            }
            def.card_types.extend(spec.extra_types.iter().copied());
            def.creature_subtypes
                .extend(spec.extra_creature_subtypes.iter().copied());
            def.printed_keywords
                .extend(spec.extra_keywords.iter().copied());
            if let Some((kind, count)) = spec.extra_counters
                && count > 0
            {
                def.entry_counter_abilities
                    .push(fixed_entry_counter(kind, count));
            }
            if let Some(extra_act) = &spec.extra_activated {
                let mut ability = (**extra_act).clone();
                let next = def
                    .activated
                    .iter()
                    .try_fold(0_u64, |candidate, a| {
                        a.id.0.checked_add(1).map(|n| n.max(candidate))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                ability.id = crate::activated::ActivatedAbilityId(next);
                def.activated.push(ability);
            }
            if let Some(extra_trig) = &spec.extra_triggered {
                let mut trigger = (**extra_trig).clone();
                let next = def
                    .triggered
                    .iter()
                    .try_fold(0_u64, |candidate, t| {
                        t.id.0.checked_add(1).map(|n| n.max(candidate))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                trigger.id = crate::program::TriggeredAbilityId(next);
                def.triggered.push(trigger);
            }
        }
        if let EntryCopyRule::MatchingExt(spec) = rule {
            let def = Arc::make_mut(&mut definition);
            if spec.nonlegendary {
                def.supertypes.remove(&Supertype::Legendary);
            }
            def.card_types.extend(spec.extra_types.iter().copied());
            def.creature_subtypes
                .extend(spec.extra_creature_subtypes.iter().copied());
            def.printed_keywords
                .extend(spec.extra_keywords.iter().copied());
            if !spec.extra_colors.is_empty() {
                let mut colors = def.defined_colors();
                colors.extend(spec.extra_colors.iter().copied());
                def.effect_defined_colors = Some(colors);
            }
            if let Some((power, toughness)) = &spec.override_pt
                && let crate::program::ProgramKind::Permanent(permanent) = &mut def.kind
            {
                permanent.printed_pt = Some(
                    crate::characteristics::PowerToughness {
                        power: power.clone(),
                        toughness: toughness.clone(),
                    }
                    .into(),
                );
                let is_pt_cda = |ab: &crate::characteristics::StaticContinuousAbility| {
                    matches!(
                        ab,
                        crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
                            parts,
                            ..
                        } if parts.0.iter().any(|part| {
                            matches!(
                                part,
                                crate::characteristics::DefiningPart::PowerToughness { .. }
                            )
                        })
                    )
                };
                if def.continuous.as_ref().is_some_and(is_pt_cda) {
                    def.continuous = None;
                }
                def.additional_battlefield_abilities
                    .retain(|ab| !is_pt_cda(ab));
            }
            if spec.keep_name
                && let Some(original_names) = previous
                    .map(|copy| &copy.definition.names)
                    .or_else(|| entrant.and_then(|id| self.entrant_names.get(&id)))
            {
                def.names = original_names.clone();
            }
            for extra_act in &spec.extra_activated {
                let mut ability = extra_act.clone();
                let next = def
                    .activated
                    .iter()
                    .try_fold(0_u64, |candidate, a| {
                        a.id.0.checked_add(1).map(|n| n.max(candidate))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                ability.id = crate::activated::ActivatedAbilityId(next);
                def.activated.push(ability);
            }
            for extra_trig in &spec.extra_triggered {
                let mut trigger = extra_trig.clone();
                let next = def
                    .triggered
                    .iter()
                    .try_fold(0_u64, |candidate, t| {
                        t.id.0.checked_add(1).map(|n| n.max(candidate))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                trigger.id = crate::program::TriggeredAbilityId(next);
                def.triggered.push(trigger);
            }
            for extra_stat in &spec.extra_static {
                let next = def
                    .continuous
                    .iter()
                    .chain(&def.additional_battlefield_abilities)
                    .try_fold(0_u64, |candidate, ab| {
                        ab.id().0.checked_add(1).map(|n| n.max(candidate))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                def.additional_battlefield_abilities.push(
                    extra_stat
                        .clone()
                        .with_component_id(crate::characteristics::StaticAbilityId(next)),
                );
            }
            for &(kind, count) in &spec.extra_counters {
                if count > 0 {
                    def.entry_counter_abilities
                        .push(fixed_entry_counter(kind, count));
                }
            }
            if let Some(world) = &self.world {
                let donor_controller = self.snapshots.get(&donor).and_then(|snap| snap.controller);
                for (cond, kind, count) in &spec.conditional_counters {
                    if *count == 0 {
                        continue;
                    }
                    let eval_source = if condition_uses_entrant_source(cond) {
                        entrant.unwrap_or(donor)
                    } else {
                        donor
                    };
                    let ctx = crate::characteristics::WorldEvalCtx {
                        active_player: world.active_player,
                        participants: &world.participants,
                        life: Some(&world.life),
                        combat: world.combat.as_ref(),
                        cards: &world.cards,
                        programs: &world.programs,
                        state: &world.state,
                        objects: &world.objects,
                        source: eval_source,
                        controller,
                        name_rules: &world.name_rules,
                        trigger_subject: Some(donor),
                        related_player: donor_controller,
                        defending_player: None,
                    };
                    if crate::characteristics::eval_world_condition(&ctx, cond) {
                        def.entry_counter_abilities
                            .push(fixed_entry_counter(*kind, *count));
                    }
                }
            }
        }
        Ok(EntryCopyResult {
            definition,
            extra_counters,
        })
    }
}

fn fixed_entry_counter(kind: CounterKind, count: u8) -> crate::program::EntryCounterAbility {
    match kind {
        CounterKind::PlusOne => {
            crate::program::EntryCounterAbility::FixedPlusOne(i64::from(count).into())
        }
        CounterKind::MinusOne => {
            crate::program::EntryCounterAbility::FixedMinusOne(i64::from(count).into())
        }
        CounterKind::Charge => {
            crate::program::EntryCounterAbility::FixedCharge(i64::from(count).into())
        }
        CounterKind::Loyalty => {
            crate::program::EntryCounterAbility::FixedLoyalty(i64::from(count).into())
        }
        other => crate::program::EntryCounterAbility::FixedNamed {
            kind: other,
            amount: i64::from(count).into(),
        },
    }
}

fn expr_uses_entrant_source(expr: &crate::program::ValueExpr) -> bool {
    use crate::program::{EventValueKind, SourceStateExpr, ValueExpr};
    match expr {
        ValueExpr::SourceState(kind) => matches!(
            kind,
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
                | SourceStateExpr::EvidenceCollected
                | SourceStateExpr::EnteredFromGraveyard
                | SourceStateExpr::PaidAlternative
                | SourceStateExpr::WasCast { .. }
                | SourceStateExpr::EnteredFromZone(_)
        ),
        ValueExpr::EventValue(kind) => matches!(
            kind,
            EventValueKind::KickerCount(_)
                | EventValueKind::PaidAlternative
                | EventValueKind::Bargained
                | EventValueKind::CastDuringOwnMain
                | EventValueKind::CastFromGraveyard
                | EventValueKind::ManaSpentToCast
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
        ),
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            expr_uses_entrant_source(expr)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            expr_uses_entrant_source(a) || expr_uses_entrant_source(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            expr_uses_entrant_source(left)
                || expr_uses_entrant_source(right)
                || expr_uses_entrant_source(then_value)
                || expr_uses_entrant_source(else_value)
        }
        _ => false,
    }
}

fn condition_uses_entrant_source(cond: &crate::program::ResolutionCondition) -> bool {
    expr_uses_entrant_source(&cond.left) || expr_uses_entrant_source(&cond.right)
}

impl EffectWindow {
    pub(super) fn prospective_entry_face(&self, object: ObjectId) -> Option<&Program> {
        let program = self.copiable_program(object)?;
        if self.linked_return_work.as_ref().is_some_and(|work| {
            matches!(work.origin, LinkedReturnOrigin::GraveyardTransformed { .. })
                && work.objects.contains(&object)
        }) {
            return program.transform_back.as_deref();
        }
        Some(program)
    }

    pub(super) fn prospective_definition(&self, object: ObjectId) -> Option<&Program> {
        self.prospective_entry_copies
            .get(&object)
            .map(|copy| copy.definition.as_ref())
            .or_else(|| self.prospective_entry_face(object))
    }
}
