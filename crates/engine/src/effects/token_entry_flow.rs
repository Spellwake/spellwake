//! One simultaneous creation cut, with relative order chosen before actual birth.
use super::*;
use crate::characteristics::{AbilityChange, ResolvedDuration, ResolvedPart, ResolvedParts};
use crate::token::{TokenCreation, TokenEntryPreview, TokenRecipients};

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimestampGroup {
    chooser: SeatId,
    members: Vec<TokenBatchMember>,
}
/// The existing cursor moves here; previews are not another current-object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TokenEntryWork {
    devour: Option<zone_replacement::ZoneReplacementBatch>,
    pub(super) cursor: ResolutionCursor,
    members: Vec<TokenEntryPreview>,
    groups: Vec<TimestampGroup>,
    next_group: usize,
    attach_after_creation: Option<ObjectId>,
    timestamp_order: Vec<TokenBatchMember>,
    mobilize: bool,
    encore_defenders: Vec<SeatId>,
    defenders: BTreeMap<TargetRef, SeatId>,
    chosen_defenders: Vec<TargetRef>,
    entry_choices: BTreeMap<TokenBatchMember, crate::land::EntryChoice>,
    entry_controllers: BTreeMap<TokenBatchMember, SeatId>,
    aura_hosts: BTreeMap<TokenBatchMember, TargetRef>,
    fight_pairs: Vec<(TokenBatchMember, ObjectId)>,
    copy_pool: entry_copy::EntryCopyPool,
    used_intrinsic_copies: BTreeMap<TokenBatchMember, Vec<crate::program::EntryCopyRule>>,
    used_battlefield_copies: BTreeMap<TokenBatchMember, BTreeSet<ObjectId>>,
    copied: BTreeMap<TokenBatchMember, entry_copy::EntryCopyResult>,
    original_definitions: BTreeMap<TokenBatchMember, Program>,
}

impl EffectWindow {
    pub(super) fn entering_copy_candidate(&self) -> Result<Option<&Program>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        if self
            .triggers
            .optional_trigger_effect_used(trigger, entry.controller())
        {
            return Ok(None);
        }
        self.trigger_entrant_definition(trigger)
    }

    fn token_player_relation_matches(
        &self,
        rel: crate::program::PlayerRelation,
        rule_ctrl: SeatId,
        candidate: SeatId,
    ) -> Result<bool, EffectError> {
        Ok(match rel {
            crate::program::PlayerRelation::Controller => candidate == rule_ctrl,
            crate::program::PlayerRelation::Opponent => self
                .combat_rules
                .opponents(self.window.participants(), rule_ctrl)?
                .contains(&candidate),
            crate::program::PlayerRelation::AnyParticipant => true,
        })
    }

    fn token_program_matches_filter(
        program: &Program,
        filter: crate::program::ObjectFilter,
    ) -> bool {
        fn clause_ok(c: crate::program::ObjectClause, program: &Program) -> bool {
            if let Some(expected_token) = c.token
                && !expected_token
            {
                return false;
            }
            if !c.any_types.matches_any(&program.card_types)
                || !c.required_types.matches_all(&program.card_types)
                || !c.forbidden_types.matches_none(&program.card_types)
            {
                return false;
            }
            if !c.required_supertypes.matches_all(&program.supertypes)
                || !c.forbidden_supertypes.matches_none(&program.supertypes)
            {
                return false;
            }
            let attachment = program.permanent().and_then(|p| p.attachment);
            let subtype_ok = match c.subtypes {
                crate::program::SubtypeFilter::Any => true,
                crate::program::SubtypeFilter::Has(k) => program.creature_subtypes.contains(&k),
                crate::program::SubtypeFilter::Lacks(k) => !program.creature_subtypes.contains(&k),
                crate::program::SubtypeFilter::AnyOf(ks) => {
                    ks.iter().any(|k| program.creature_subtypes.contains(k))
                }
                crate::program::SubtypeFilter::NoneOf(ks) => {
                    ks.iter().all(|k| !program.creature_subtypes.contains(k))
                }
                crate::program::SubtypeFilter::Land(k) => program.land_subtypes.contains(&k),
                crate::program::SubtypeFilter::NotLand(k) => !program.land_subtypes.contains(&k),
                crate::program::SubtypeFilter::Artifact(k) => {
                    program.artifact_subtypes.contains(&k)
                }
                crate::program::SubtypeFilter::Enchantment(k) => {
                    program.enchantment_subtypes.contains(&k)
                }
                crate::program::SubtypeFilter::Planeswalker(k) => {
                    program.planeswalker_subtypes.contains(&k)
                }
                crate::program::SubtypeFilter::Spell(k) => program.spell_subtypes.contains(&k),
                crate::program::SubtypeFilter::Aura => {
                    matches!(attachment, Some(crate::attachment::AttachmentKind::Aura(_)))
                }
                crate::program::SubtypeFilter::ExceptChosenSlot(_)
                | crate::program::SubtypeFilter::MatchesChosenSlot(_)
                | crate::program::SubtypeFilter::OutsideNamedSlots => false,
            };
            if !subtype_ok {
                return false;
            }
            let colors = program.defined_colors();
            if !c.color.matches(&colors) {
                return false;
            }
            if let Some(keyword) = c.keyword
                && !program.printed_keywords.contains(&keyword)
            {
                return false;
            }
            if let Some(keyword) = c.forbidden_keyword
                && program.printed_keywords.contains(&keyword)
            {
                return false;
            }
            if !c.matches_enchantment_exclusions(attachment, &program.enchantment_subtypes) {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::HISTORIC)
                && !program.card_types.contains(&CardType::Artifact)
                && !program
                    .supertypes
                    .contains(&crate::program::Supertype::Legendary)
                && !program
                    .enchantment_subtypes
                    .contains(&crate::program::EnchantmentSubtype::Saga)
            {
                return false;
            }
            let pt = program
                .permanent()
                .and_then(|p| p.printed_pt.as_ref())
                .map(crate::characteristics::PrintedPowerToughness::baseline);
            let power_ok = match c.power {
                crate::program::PowerFilter::Any => true,
                crate::program::PowerFilter::AtLeast(min) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.power >= i64::from(min).into()),
                crate::program::PowerFilter::AtMost(max) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.power <= i64::from(max).into()),
                crate::program::PowerFilter::Equal(exact) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.power == i64::from(exact).into()),
                crate::program::PowerFilter::PowerOrToughnessAtMost(max) => {
                    pt.as_ref().is_some_and(|stats| {
                        stats.power <= i64::from(max).into()
                            || stats.toughness <= i64::from(max).into()
                    })
                }
                crate::program::PowerFilter::AtMostX
                | crate::program::PowerFilter::EqualX
                | crate::program::PowerFilter::AtLeastX
                | crate::program::PowerFilter::CompareAnchor { .. }
                | crate::program::PowerFilter::AtMostControlledCreatureCount => false,
            };
            if !power_ok {
                return false;
            }
            let toughness_ok = match c.toughness {
                crate::program::ToughnessFilter::Any => true,
                crate::program::ToughnessFilter::AtLeast(min) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.toughness >= i64::from(min).into()),
                crate::program::ToughnessFilter::AtMost(max) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.toughness <= i64::from(max).into()),
                crate::program::ToughnessFilter::Equal(exact) => pt
                    .as_ref()
                    .is_some_and(|stats| stats.toughness == i64::from(exact).into()),
                crate::program::ToughnessFilter::AtMostX
                | crate::program::ToughnessFilter::EqualX
                | crate::program::ToughnessFilter::AtLeastX
                | crate::program::ToughnessFilter::CompareAnchor { .. } => false,
            };
            if !toughness_ok {
                return false;
            }
            let mv = program.mana_value();
            match c.mana_value {
                crate::program::ManaValueFilter::Any => true,
                crate::program::ManaValueFilter::AtMost(max) => mv <= i64::from(max).into(),
                crate::program::ManaValueFilter::AtLeast(min) => mv >= i64::from(min).into(),
                crate::program::ManaValueFilter::Equal(exact) => mv == i64::from(exact).into(),
                crate::program::ManaValueFilter::Even => {
                    mv.nonnegative_half_rounded_up() == mv.nonnegative_half_rounded_down()
                }
                crate::program::ManaValueFilter::Odd => {
                    mv.nonnegative_half_rounded_up() != mv.nonnegative_half_rounded_down()
                }
                crate::program::ManaValueFilter::AtMostX
                | crate::program::ManaValueFilter::EqualX
                | crate::program::ManaValueFilter::AtLeastX
                | crate::program::ManaValueFilter::CompareAnchor { .. } => false,
            }
        }
        match filter {
            crate::program::ObjectFilter::Clause(a) => clause_ok(a, program),
            crate::program::ObjectFilter::Either(a, b) => {
                clause_ok(a, program) || clause_ok(b, program)
            }
        }
    }

    /// Apply active `StaticModifierRule::StealCreatedTokens` replacements (CR 614.1, e.g. Crafty Cutpurse)
    /// from both `temporary_rules` and battlefield permanents. Each rule applies at most once (CR 614.5).
    fn resolve_token_creator(
        &self,
        current: &EvaluatedCharacteristics,
        mut creator: SeatId,
        token_program: Option<&Program>,
    ) -> Result<SeatId, EffectError> {
        let mut applied_temp = BTreeSet::new();
        let mut applied_perm = BTreeSet::new();
        loop {
            let mut changed = false;
            for (idx, active) in self.temporary_rules.iter().enumerate() {
                if active.consumed || applied_temp.contains(&idx) {
                    continue;
                }
                if let crate::program::TemporaryRuleKind::TemporaryStatic(ab) = &active.rule
                    && let crate::characteristics::StaticContinuousAbility::Modifier {
                        rule:
                            crate::characteristics::StaticModifierRule::StealCreatedTokens {
                                affected,
                                token_filter,
                            },
                        ..
                    } = ab.as_ref()
                    && self.token_player_relation_matches(*affected, active.controller, creator)?
                    && token_filter.as_deref().is_none_or(|filter| {
                        token_program
                            .is_some_and(|prog| Self::token_program_matches_filter(prog, *filter))
                    })
                {
                    applied_temp.insert(idx);
                    if active.controller != creator
                        && self.window.participants().contains(&active.controller)
                    {
                        creator = active.controller;
                        changed = true;
                        break;
                    }
                }
            }
            if changed {
                continue;
            }
            for source in current
                .objects
                .values()
                .filter(|perm| perm.zone == ObjectZone::Battlefield)
            {
                let Some(source_ctrl) = source.controller else {
                    continue;
                };
                for ab in source.active_battlefield_rules() {
                    let key = (source.object, ab.id());
                    if applied_perm.contains(&key) {
                        continue;
                    }
                    if let crate::characteristics::StaticContinuousAbility::Modifier {
                        rule:
                            crate::characteristics::StaticModifierRule::StealCreatedTokens {
                                affected,
                                token_filter,
                            },
                        ..
                    } = ab
                        && self.token_player_relation_matches(*affected, source_ctrl, creator)?
                        && token_filter.as_deref().is_none_or(|filter| {
                            token_program.is_some_and(|prog| {
                                Self::token_program_matches_filter(prog, *filter)
                            })
                        })
                    {
                        applied_perm.insert(key);
                        if source_ctrl != creator
                            && self.window.participants().contains(&source_ctrl)
                        {
                            creator = source_ctrl;
                            changed = true;
                            break;
                        }
                    }
                }
                if changed {
                    break;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(creator)
    }

    pub(super) fn adjusted_token_count(
        &self,
        current: &EvaluatedCharacteristics,
        creator: SeatId,
        base: usize,
        token_program: Option<&Program>,
    ) -> Result<usize, EffectError> {
        if base == 0 {
            return Ok(0);
        }
        let mut doublers = current.controlled_rules(creator, |ability| {
            matches!(
                ability,
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::DoubleCreatedTokens,
                    ..
                }
            )
        });
        let mut total_add = 0usize;
        let mut factors: Vec<usize> = Vec::new();
        let mut apply_rule = |ability: &crate::characteristics::StaticContinuousAbility,
                              rule_ctrl: SeatId,
                              count_doubler: bool|
         -> Result<(), EffectError> {
            match ability {
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::DoubleCreatedTokens,
                    ..
                } if count_doubler && rule_ctrl == creator => {
                    doublers = doublers.saturating_add(1);
                }
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyTokens {
                            controller,
                            add,
                            multiply,
                        },
                    ..
                } => {
                    if self.token_player_relation_matches(*controller, rule_ctrl, creator)? {
                        total_add = total_add
                            .checked_add(usize::from(*add))
                            .ok_or(EffectError::TokenCapacity)?;
                        if *multiply > 1 {
                            factors.push(usize::from(*multiply));
                        }
                    }
                }
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyTokenSubtype {
                            subtype,
                            controller,
                            add,
                            multiply,
                        },
                    ..
                } => {
                    let subtype_ok =
                        token_program.is_some_and(|p| p.artifact_subtypes.contains(subtype));
                    if subtype_ok
                        && self.token_player_relation_matches(*controller, rule_ctrl, creator)?
                    {
                        total_add = total_add
                            .checked_add(usize::from(*add))
                            .ok_or(EffectError::TokenCapacity)?;
                        if *multiply > 1 {
                            factors.push(usize::from(*multiply));
                        }
                    }
                }
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyTokensFiltered {
                            filter,
                            controller,
                            add,
                            multiply,
                        },
                    ..
                } => {
                    let filter_ok = token_program
                        .is_some_and(|p| Self::token_program_matches_filter(p, **filter));
                    if filter_ok
                        && self.token_player_relation_matches(*controller, rule_ctrl, creator)?
                    {
                        total_add = total_add
                            .checked_add(usize::from(*add))
                            .ok_or(EffectError::TokenCapacity)?;
                        if *multiply > 1 {
                            factors.push(usize::from(*multiply));
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        };
        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            let crate::program::TemporaryRuleKind::TemporaryStatic(ability) = &active.rule else {
                continue;
            };
            apply_rule(ability.as_ref(), active.controller, true)?;
        }
        for source in current
            .objects
            .values()
            .filter(|perm| perm.zone == ObjectZone::Battlefield)
        {
            let Some(source_ctrl) = source.controller else {
                continue;
            };
            for ability in source.active_battlefield_rules() {
                apply_rule(ability, source_ctrl, false)?;
            }
        }
        let mut count = base
            .checked_add(total_add)
            .ok_or(EffectError::TokenCapacity)?;
        let multiplier = 1usize
            .checked_shl(u32::try_from(doublers).map_err(|_| EffectError::TokenCapacity)?)
            .ok_or(EffectError::TokenCapacity)?;
        count = count
            .checked_mul(multiplier)
            .ok_or(EffectError::TokenCapacity)?;
        for factor in factors {
            count = count
                .checked_mul(factor)
                .ok_or(EffectError::TokenCapacity)?;
        }
        Ok(count)
    }

    /// Collect bonus token batches generated by active `StaticModifierRule::BonusTokenOnCreate`
    /// and `StaticModifierRule::BonusTokenFiltered` rules (CR 614.1, e.g. Peregrin Took, Chatterfang)
    /// when `creator` creates `created_count > 0` tokens.
    pub(super) fn bonus_tokens_for_creation(
        &self,
        current: &EvaluatedCharacteristics,
        creator: SeatId,
        created_count: usize,
    ) -> Result<Vec<(Program, usize)>, EffectError> {
        self.bonus_tokens_for_creation_with_program(current, creator, created_count, None)
    }

    fn bonus_tokens_for_creation_with_program(
        &self,
        current: &EvaluatedCharacteristics,
        creator: SeatId,
        created_count: usize,
        token_program: Option<&Program>,
    ) -> Result<Vec<(Program, usize)>, EffectError> {
        if created_count == 0 {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut collect_bonus = |ab: &crate::characteristics::StaticContinuousAbility,
                                 rule_ctrl: SeatId|
         -> Result<(), EffectError> {
            match ab {
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::BonusTokenOnCreate {
                            controller,
                            token,
                            count,
                            per_created,
                        },
                    ..
                } if self.token_player_relation_matches(*controller, rule_ctrl, creator)? => {
                    let bonus = if *per_created {
                        created_count
                            .checked_mul(usize::from(*count))
                            .ok_or(EffectError::TokenCapacity)?
                    } else {
                        usize::from(*count)
                    };
                    if bonus > 0 {
                        let materialized = token
                            .materialize(&self.creature_vocabulary)
                            .unwrap_or_else(|_| token.program().clone());
                        out.push((materialized, bonus));
                    }
                }
                crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::BonusTokenFiltered {
                            filter,
                            controller,
                            token,
                            count,
                            per_created,
                        },
                    ..
                } if token_program
                    .is_some_and(|p| Self::token_program_matches_filter(p, **filter))
                    && self.token_player_relation_matches(*controller, rule_ctrl, creator)? =>
                {
                    let bonus = if *per_created {
                        created_count
                            .checked_mul(usize::from(*count))
                            .ok_or(EffectError::TokenCapacity)?
                    } else {
                        usize::from(*count)
                    };
                    if bonus > 0 {
                        let materialized = token
                            .materialize(&self.creature_vocabulary)
                            .unwrap_or_else(|_| token.program().clone());
                        out.push((materialized, bonus));
                    }
                }
                _ => {}
            }
            Ok(())
        };
        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            if let crate::program::TemporaryRuleKind::TemporaryStatic(ab) = &active.rule {
                collect_bonus(ab.as_ref(), active.controller)?;
            }
        }
        for source in current
            .objects
            .values()
            .filter(|perm| perm.zone == ObjectZone::Battlefield)
        {
            let Some(source_ctrl) = source.controller else {
                continue;
            };
            for ab in source.active_battlefield_rules() {
                collect_bonus(ab, source_ctrl)?;
            }
        }
        Ok(out)
    }

    pub(super) fn trigger_entrant_definition(
        &self,
        trigger: &crate::occurrence::TriggerBinding,
    ) -> Result<Option<&Program>, EffectError> {
        use crate::occurrence::TriggerRelated;
        let [member] = trigger.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let after = match &member.related {
            TriggerRelated::ZoneChange { after, .. }
            | TriggerRelated::TokenCreated { after, .. } => after,
            _ => return Err(EffectError::InconsistentProgram),
        };
        let donor = after.object.id();
        let copied = self
            .window
            .cards()
            .get(donor)
            .filter(|card| card.zone() == ObjectZone::Battlefield)
            .and_then(|_| self.copiable_program(donor))
            .or_else(|| self.departed_copy_sources.get(&donor));
        Ok(copied.filter(|program| program.permanent().is_some()))
    }

    pub(super) fn copy_entering_permanent_once(&mut self) -> Result<(), EffectError> {
        let Some(program) = self.entering_copy_candidate()?.cloned() else {
            return self.advance_instruction();
        };
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let controller = entry.controller();
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let (source, grant, definition) = (
            trigger.source.object.id(),
            trigger.granting_effect,
            trigger.definition,
        );
        self.triggers
            .use_optional_trigger_effect(source, grant, definition, controller);
        self.begin_materialized_token_entries(vec![(controller, 1, program, false)], None, false)
    }

    pub(super) fn begin_token_entries(
        &mut self,
        creations: &[TokenCreation],
        attach_after_creation: Option<ObjectId>,
        mobilize: bool,
    ) -> Result<(), EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let controller = entry.controller();
        let mut groups = Vec::new();

        // All counts read the same actual pre-batch state and resolution locals.
        for creation in creations {
            let amount = self.resolve_value_expr(controller, &creation.amount)?;
            let count = String::from(amount)
                .parse::<usize>()
                .map_err(|_| EffectError::TokenCapacity)?;
            let mut definition = creation
                .definition
                .materialize(&self.creature_vocabulary)
                .map_err(|_| EffectError::InconsistentProgram)?;
            match &creation.base_size {
                None => {}
                Some(crate::token::TokenSize::AtCreation(expression)) => {
                    let size = self.resolve_value_expr(controller, expression)?;
                    let crate::program::ProgramKind::Permanent(permanent) = &mut definition.kind
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    permanent.printed_pt = Some(
                        crate::characteristics::PowerToughness {
                            power: size.clone(),
                            toughness: size,
                        }
                        .into(),
                    );
                }
                Some(crate::token::TokenSize::AbilitySourceCounters(kind)) => {
                    use crate::characteristics::{
                        DefiningPart, DefiningParts, NumericExpression, StaticAbilityId,
                        StaticContinuousAbility,
                    };
                    let StackKind::TriggeredAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let count = NumericExpression::Counters {
                        source: Some(source.id()),
                        kind: *kind,
                    };
                    definition.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
                        id: StaticAbilityId(0),
                        parts: DefiningParts(vec![DefiningPart::PowerToughness {
                            power: Some(Box::new(count.clone())),
                            toughness: Some(Box::new(count)),
                        }]),
                    });
                }
            }
            if creation.recipients == TokenRecipients::DestroyedPermanentControllers {
                let mut counts: BTreeMap<SeatId, usize> = BTreeMap::new();
                if let Some(cursor) = self.resolution_cursor() {
                    for &seat in &cursor.results.last_moved_controllers {
                        *counts.entry(seat).or_insert(0) += 1;
                    }
                }
                for (creator, mult) in counts {
                    let total = count.saturating_mul(mult);
                    if total > 0 {
                        groups.push((creator, total, definition.clone(), creation.enters_tapped));
                    }
                }
                continue;
            }
            let prior_controller =
                if creation.recipients == TokenRecipients::CombatDamageSourceController {
                    self.combat_damage_source_controller()?
                } else if creation.recipients == TokenRecipients::RememberedTargetController {
                    Some(
                        self.resolution_cursor()
                            .and_then(|cursor| cursor.remembered_permanent.as_ref())
                            .ok_or(EffectError::InconsistentProgram)?
                            .0,
                    )
                } else if creation.recipients == TokenRecipients::ExiledTargetController {
                    Some(
                        self.resolution_cursor()
                            .and_then(|cursor| cursor.exiled_target.as_ref())
                            .ok_or(EffectError::InconsistentProgram)?
                            .controller,
                    )
                } else if let TokenRecipients::Player(role) = creation.recipients {
                    Some(self.resolving_player(role)?)
                } else {
                    None
                };
            for creator in
                self.window.participants().iter().copied().filter(|seat| {
                    match creation.recipients {
                        TokenRecipients::CombatDamageSourceController
                        | TokenRecipients::RememberedTargetController
                        | TokenRecipients::ExiledTargetController
                        | TokenRecipients::Player(_) => Some(*seat) == prior_controller,
                        TokenRecipients::Controller => *seat == controller,
                        TokenRecipients::EachPlayer => true,
                        TokenRecipients::EachOpponent => *seat != controller,
                        TokenRecipients::DestroyedPermanentControllers => false,
                    }
                })
            {
                groups.push((creator, count, definition.clone(), creation.enters_tapped));
            }
        }
        self.begin_materialized_token_entries(groups, attach_after_creation, mobilize)
    }

    /// Materialized copiable programs receive the same creation, ordering and entry rules as literal templates.
    pub(super) fn begin_materialized_token_entries(
        &mut self,
        groups: Vec<(SeatId, usize, Program, bool)>,
        attach_after_creation: Option<ObjectId>,
        mobilize: bool,
    ) -> Result<(), EffectError> {
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongPhase)?
            .controller();
        let current = self.evaluated_characteristics()?;
        let enters_attacking = mobilize
            || matches!(
                self.current_instruction()?,
                Instruction::CreateAttackingTokens { .. }
                    | Instruction::CreateTokenCopy {
                        attacking: true,
                        ..
                    }
            );
        let defenders = if enters_attacking
            && self.window.active_participant() == Some(controller)
            && self.combat.is_some()
        {
            self.attack_destinations(controller, &current)?
        } else {
            BTreeMap::new()
        };
        let mut groups = groups;
        for (creator, count, definition, _) in &mut groups {
            if *count > 0 && self.window.participants().contains(creator) {
                *creator = self.resolve_token_creator(&current, *creator, Some(definition))?;
            }
        }
        groups.retain(|(creator, _, definition, _)| {
            self.window.participants().contains(creator)
                && (definition.entry_copy.is_some()
                    || definition.permanent().is_some_and(|p| {
                        p.entry_tap == Some(crate::land::EntryTap::ChosenOpponentControls)
                    })
                    || self
                        .token_aura_domain(definition, *creator, &current)
                        .is_none_or(|hosts| !hosts.is_empty()))
        });
        let mut bonus_groups: Vec<(SeatId, usize, Program, bool)> = Vec::new();
        for (creator, count, definition, _) in &mut groups {
            if *count == 0 {
                continue;
            }
            *count = self.adjusted_token_count(&current, *creator, *count, Some(definition))?;
            if *count > 0 {
                for (bonus_prog, bonus_count) in self.bonus_tokens_for_creation_with_program(
                    &current,
                    *creator,
                    *count,
                    Some(definition),
                )? {
                    if bonus_count > 0 {
                        bonus_groups.push((*creator, bonus_count, bonus_prog, false));
                    }
                }
            }
        }
        groups.extend(bonus_groups);
        let total = groups.iter().try_fold(0usize, |sum, (_, count, _, _)| {
            sum.checked_add(*count).ok_or(EffectError::TokenCapacity)
        })?;
        let count = u64::try_from(total).map_err(|_| EffectError::TokenCapacity)?;
        self.window
            .cards()
            .revision()
            .checked_add(count)
            .ok_or(StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::RevisionExhausted,
            ))?;
        self.window
            .cards()
            .next_identity_sequence()
            .checked_add(count)
            .ok_or(StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::IdentityExhausted,
            ))?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(total)
            .map_err(|_| EffectError::TokenCapacity)?;
        let mut encore_defenders = Vec::new();
        if !groups.is_empty() && self.current_instruction()? == &Instruction::Encore {
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), controller)?;
            if opponents.len() != groups.len() {
                return Err(EffectError::InconsistentProgram);
            }
            encore_defenders
                .try_reserve_exact(total)
                .map_err(|_| EffectError::TokenCapacity)?;
            for (opponent, (_, count, _, _)) in opponents.into_iter().zip(&groups) {
                encore_defenders.extend(std::iter::repeat_n(opponent, *count));
            }
        }
        let fight_opponents = if !groups.is_empty()
            && matches!(
                self.current_instruction()?,
                Instruction::CreateTokensToFight(_)
            ) {
            let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
            let captured = self
                .effect_group(
                    entry,
                    crate::program::EffectSubject::Matching(
                        crate::program::ObjectFilter::creature().opposing(),
                    ),
                )?
                .into_iter()
                .collect::<Vec<_>>();
            if captured.len() != groups.len() {
                return Err(EffectError::InconsistentProgram);
            }
            captured
        } else {
            Vec::new()
        };
        let mut fight_pairs = Vec::new();
        for (index, (creator, count, definition, enters_tapped)) in groups.into_iter().enumerate() {
            if count > 0
                && let Some(&opponent) = fight_opponents.get(index)
            {
                fight_pairs.push((TokenBatchMember::new(members.len()), opponent));
            }
            for _ in 0..count {
                members.push(TokenEntryPreview {
                    enters_tapped,
                    member: TokenBatchMember::new(members.len()),
                    creator,
                    definition: definition.clone(),
                });
            }
        }
        if members.is_empty() {
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .last_created_tokens
                .clear();
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let (entry, instruction) = (cursor.entry, cursor.instruction);
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry, instruction, None);
            #[cfg(not(test))]
            let _ = (entry, instruction);
            return Ok(());
        }
        let mut groups = Vec::new();
        for chooser in self.window.apnap_players()? {
            let offered = members
                .iter()
                .filter(|m| m.creator == chooser)
                .map(|m| m.member)
                .collect::<Vec<_>>();
            if !offered.is_empty() {
                groups.push(TimestampGroup {
                    chooser,
                    members: offered,
                });
            }
        }
        let copy_pool = if members
            .iter()
            .any(|member| member.definition.entry_copy.is_some())
            || (members
                .iter()
                .any(|member| member.definition.has_type(CardType::Creature))
                && entry_copy::EntryCopyPool::has_battlefield_copy_rules(&current, true))
        {
            entry_copy::EntryCopyPool::capture(self, &current)?
        } else {
            entry_copy::EntryCopyPool::default()
        };
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::TokenEntry(Box::new(TokenEntryWork {
            devour: None,
            cursor,
            members,
            groups,
            next_group: 0,
            attach_after_creation,
            timestamp_order: Vec::new(),
            mobilize,
            encore_defenders,
            defenders,
            chosen_defenders: Vec::new(),
            entry_choices: BTreeMap::new(),
            entry_controllers: BTreeMap::new(),
            aura_hosts: BTreeMap::new(),
            fight_pairs,
            copy_pool,
            used_intrinsic_copies: BTreeMap::new(),
            used_battlefield_copies: BTreeMap::new(),
            copied: BTreeMap::new(),
            original_definitions: BTreeMap::new(),
        })));
        Ok(())
    }

    fn token_timestamp_frame(&self) -> Result<TokenTimestampOrder, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongTimestampOrderFrame);
        };
        let group = work
            .groups
            .get(work.next_group)
            .ok_or(EffectError::WrongTimestampOrderFrame)?;
        if group.members.len() < 2
            || self.window.resolving().map(|e| e.id()) != Some(work.cursor.entry)
        {
            return Err(EffectError::WrongTimestampOrderFrame);
        }
        let members = group
            .members
            .iter()
            .map(|member| {
                work.members
                    .get(member.ordinal())
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)
            })
            .collect::<Result<_, _>>()?;
        Ok(TokenTimestampOrder {
            revision: self.revision,
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
            chooser: group.chooser,
            members,
        })
    }

    pub(super) fn accept_token_timestamp_order(
        &self,
        expected: u64,
        frame: &TokenTimestampOrder,
        actor: SeatId,
        order: Vec<TokenBatchMember>,
    ) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if &self.token_timestamp_frame()? != frame {
            return Err(EffectError::WrongTimestampOrderFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let supplied = order.iter().copied().collect::<BTreeSet<_>>();
        let offered = frame
            .members()
            .iter()
            .map(|m| m.member)
            .collect::<BTreeSet<_>>();
        if order.len() != offered.len() || supplied.len() != order.len() || supplied != offered {
            return Err(EffectError::IllegalTimestampOrder);
        }
        let mut next = self.clone();
        let Some(PendingWork::TokenEntry(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongTimestampOrderFrame);
        };
        work.timestamp_order.extend(order);
        work.next_group = work
            .next_group
            .checked_add(1)
            .ok_or(EffectError::TokenCapacity)?;
        next.revision = revision;
        Ok(next)
    }

    fn token_defender_frame(&self) -> Result<crate::token::TokenDefenderFrame, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let member = work
            .members
            .get(work.chosen_defenders.len())
            .ok_or(EffectError::WrongResolutionFrame)?;
        if work.defenders.len() < 2 || work.next_group != work.groups.len() {
            return Err(EffectError::WrongResolutionFrame);
        }
        Ok(crate::token::TokenDefenderFrame {
            revision: self.revision,
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
            member: member.member,
            chooser: member.creator,
            defenders: work.defenders.clone(),
        })
    }
    pub(super) fn accept_token_defender(
        &self,
        expected: u64,
        frame: &crate::token::TokenDefenderFrame,
        actor: SeatId,
        defender: TargetRef,
    ) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if &self.token_defender_frame()? != frame {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if !frame.defenders.contains_key(&defender) {
            return Err(CombatError::IllegalDefender.into());
        }
        let mut next = self.clone();
        let Some(PendingWork::TokenEntry(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        work.chosen_defenders.push(defender);
        next.revision = revision;
        Ok(next)
    }

    fn token_copy_frame(&self) -> Result<Option<ZoneReplacementFrame>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        for member in work
            .groups
            .iter()
            .flat_map(|group| group.members.iter())
            .map(|member| &work.members[member.ordinal()])
        {
            let chooser = work
                .entry_controllers
                .get(&member.member)
                .copied()
                .unwrap_or(member.creator);
            let donors = member
                .definition
                .entry_copy
                .as_ref()
                .filter(|rule| {
                    work.used_intrinsic_copies
                        .get(&member.member)
                        .is_none_or(|used| !used.contains(rule))
                })
                .map(|rule| work.copy_pool.domain(rule, chooser))
                .unwrap_or_default();
            let battlefield_donors = if member.definition.has_type(CardType::Creature) {
                work.copy_pool
                    .token_donors(chooser)
                    .iter()
                    .copied()
                    .filter(|(source, _)| {
                        work.used_battlefield_copies
                            .get(&member.member)
                            .is_none_or(|used| !used.contains(source))
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if !donors.is_empty() || !battlefield_donors.is_empty() {
                return Ok(Some(ZoneReplacementFrame::token_copy(
                    self.revision,
                    work.cursor.entry,
                    work.cursor.instruction,
                    chooser,
                    member.member,
                    donors,
                    battlefield_donors,
                )));
            }
        }
        Ok(None)
    }

    fn apply_token_entry_copy(
        &mut self,
        frame: &ZoneReplacementFrame,
        reply: ZoneReplacementReply,
    ) -> Result<(), EffectError> {
        let ZoneReplacementRequest::EntryCopy {
            object: zone_replacement::EntryCopySubject::Token(key),
            donors,
            battlefield_donors,
        } = frame.request()
        else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let Some(PendingWork::TokenEntry(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let member = &mut work.members[key.ordinal()];
        let previous = work.copied.get(key);
        let copy = match reply {
            ZoneReplacementReply::EntryCopy(donor) if !donors.is_empty() => {
                let rule = member
                    .definition
                    .entry_copy
                    .as_ref()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                work.used_intrinsic_copies
                    .entry(*key)
                    .or_default()
                    .push(rule.clone());
                donor
                    .map(|id| work.copy_pool.select(rule, frame.chooser(), id, previous))
                    .transpose()?
            }
            ZoneReplacementReply::EntryCopyFromPermanent(source) => {
                let donor = battlefield_donors
                    .iter()
                    .find_map(|&(candidate, donor)| (candidate == source).then_some(donor))
                    .ok_or(EffectError::IllegalTarget)?;
                work.used_battlefield_copies
                    .entry(*key)
                    .or_default()
                    .insert(source);
                Some(work.copy_pool.battlefield_copy(donor, previous)?)
            }
            _ => return Err(EffectError::WrongDecisionKind),
        };
        if let Some(copy) = copy {
            work.original_definitions
                .entry(*key)
                .or_insert_with(|| member.definition.clone());
            member.definition = (*copy.definition).clone();
            work.copied.insert(*key, copy);
        }
        Ok(())
    }

    pub(super) fn resume_token_copy(
        &self,
        expected: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.token_controller_frame()?.is_some()
            || self.token_copy_frame()?.as_ref() != Some(frame)
        {
            return Err(EffectError::WrongResolutionFrame);
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ZoneReplacement(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        next.apply_token_entry_copy(frame, reply)?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    fn token_controller_frame(
        &self,
    ) -> Result<Option<crate::token::TokenEntryChoiceFrame>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        for member in work
            .groups
            .iter()
            .flat_map(|group| &group.members)
            .map(|key| &work.members[key.ordinal()])
        {
            let rule = crate::land::EntryTap::ChosenOpponentControls;
            if member
                .definition
                .permanent()
                .is_some_and(|p| p.entry_tap == Some(rule))
                && !work.entry_controllers.contains_key(&member.member)
                && !self.opponents_of(member.creator)?.is_empty()
            {
                return Ok(Some(crate::token::TokenEntryChoiceFrame {
                    revision: self.revision,
                    chooser: member.creator,
                    member: member.member,
                    preview: member.clone(),
                    rule,
                    eligible_reveals: Vec::new(),
                }));
            }
        }
        Ok(None)
    }

    fn token_entry_choice_frame(
        &self,
    ) -> Result<Option<crate::token::TokenEntryChoiceFrame>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        for member in work
            .groups
            .iter()
            .flat_map(|group| group.members.iter())
            .filter_map(|key| work.members.get(key.ordinal()))
        {
            if work.entry_choices.contains_key(&member.member) {
                continue;
            }
            let chooser = work
                .entry_controllers
                .get(&member.member)
                .copied()
                .unwrap_or(member.creator);
            let Some(rule) = member.definition.permanent().and_then(|p| p.entry_tap) else {
                continue;
            };
            let eligible_reveals = match rule {
                crate::land::EntryTap::ReadAhead { .. }
                | crate::land::EntryTap::AlwaysChooseColorExcept(_)
                | crate::land::EntryTap::ChooseColor
                | crate::land::EntryTap::ChooseColorExcept(_)
                | crate::land::EntryTap::ChooseCreatureType
                | crate::land::EntryTap::ChooseNonlandCardName
                | crate::land::EntryTap::ChooseOpponent
                | crate::land::EntryTap::ChoosePlayer
                | crate::land::EntryTap::ChooseTwoPlayers
                | crate::land::EntryTap::ChooseCardType
                | crate::land::EntryTap::ChooseBasicLandType
                | crate::land::EntryTap::ChooseOddOrEven
                | crate::land::EntryTap::ChooseKhansOrDragons
                | crate::land::EntryTap::ChooseOptions(_)
                | crate::land::EntryTap::Unleash
                | crate::land::EntryTap::Riot
                | crate::land::EntryTap::Tribute(_)
                | crate::land::EntryTap::SiegeProtector
                | crate::land::EntryTap::UnlessPayLife(_) => Vec::new(),
                crate::land::EntryTap::UnlessRevealHandLandType(a, b) => {
                    let evaluated = self.evaluated_characteristics()?;
                    self.window
                        .cards()
                        .iter()
                        .filter(|card| {
                            card.card_id().is_some() && card.zone() == ObjectZone::Hand(chooser)
                        })
                        .filter(|card| {
                            evaluated.get(card.id()).is_some_and(|c| {
                                c.has_type(CardType::Land)
                                    && (c.current_land_subtypes.contains(&a.land_type())
                                        || c.current_land_subtypes.contains(&b.land_type()))
                            })
                        })
                        .map(GameObject::id)
                        .collect()
                }
                _ => continue,
            };
            let chooser = if matches!(rule, crate::land::EntryTap::Tribute(_)) {
                self.opponents_of(chooser)
                    .ok()
                    .and_then(|ops| ops.first().copied())
                    .unwrap_or(chooser)
            } else {
                chooser
            };
            return Ok(Some(crate::token::TokenEntryChoiceFrame {
                revision: self.revision,
                chooser,
                member: member.member,
                preview: member.clone(),
                rule,
                eligible_reveals,
            }));
        }
        Ok(None)
    }
    pub(super) fn accept_token_entry_choice(
        &self,
        expected: u64,
        frame: &crate::token::TokenEntryChoiceFrame,
        actor: SeatId,
        choice: crate::land::EntryChoice,
    ) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        let issued = if let Some(controller) = self.token_controller_frame()? {
            Some(controller)
        } else if self.token_copy_frame()?.is_none() {
            self.token_entry_choice_frame()?
        } else {
            None
        };
        if issued.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        match (frame.rule, choice) {
            (
                crate::land::EntryTap::ChosenOpponentControls,
                crate::land::EntryChoice::Opponent(seat),
            ) if self.opponents_of(actor)?.contains(&seat) => {}
            (
                crate::land::EntryTap::AlwaysChooseColorExcept(excluded)
                | crate::land::EntryTap::ChooseColorExcept(excluded),
                crate::land::EntryChoice::Color(color),
            ) if color != excluded && color != ManaType::Colourless => {}
            (crate::land::EntryTap::ChooseColor, crate::land::EntryChoice::Color(color))
                if color != ManaType::Colourless => {}
            (
                crate::land::EntryTap::ReadAhead { final_chapter },
                crate::land::EntryChoice::Chapter(n),
            ) if (1..=final_chapter).contains(&n) => {}
            (
                crate::land::EntryTap::ChooseCreatureType,
                crate::land::EntryChoice::CreatureType(kind),
            ) if self.creature_vocabulary.label(kind).is_some() => {}
            (
                crate::land::EntryTap::ChooseNonlandCardName,
                crate::land::EntryChoice::CardName(index),
            ) if frame.card_names().get(index.0).is_some() => {}
            (
                crate::land::EntryTap::SiegeProtector | crate::land::EntryTap::ChooseOpponent,
                crate::land::EntryChoice::Opponent(player),
            ) if self.opponents_of(actor)?.contains(&player) => {}
            (crate::land::EntryTap::ChoosePlayer, crate::land::EntryChoice::Opponent(player))
                if self.window.participants().contains(&player) => {}
            (rule, crate::land::EntryChoice::OptionIndex(idx))
                if rule
                    .option_labels()
                    .is_some_and(|labels| usize::from(idx) < labels.len()) => {}
            (
                crate::land::EntryTap::ChooseTwoPlayers,
                crate::land::EntryChoice::Players(players),
            ) if self.valid_distinct_players(players) => {}
            (
                crate::land::EntryTap::UnlessRevealHandLandType(_, _),
                crate::land::EntryChoice::Reveal(None),
            ) => {}
            (
                crate::land::EntryTap::UnlessRevealHandLandType(_, _),
                crate::land::EntryChoice::Reveal(Some(id)),
            ) if frame.eligible_reveals.contains(&id) => {}
            (crate::land::EntryTap::UnlessPayLife(_), crate::land::EntryChoice::PayLife(false)) => {
            }
            (
                crate::land::EntryTap::UnlessPayLife(amount),
                crate::land::EntryChoice::PayLife(true),
            ) if amount > 0
                && self
                    .life
                    .get(actor)
                    .is_some_and(|life| *life >= i64::from(amount).into()) => {}
            _ => return Err(EffectError::IllegalTarget),
        }
        let mut next = self.clone();
        let Some(PendingWork::TokenEntry(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let crate::land::EntryChoice::Opponent(seat) = choice {
            work.entry_controllers.insert(frame.member, seat);
        } else {
            work.entry_choices.insert(frame.member, choice);
        }
        next.revision = revision;
        Ok(next)
    }

    pub(super) fn token_aura_domain(
        &self,
        definition: &Program,
        creator: SeatId,
        current: &EvaluatedCharacteristics,
    ) -> Option<Vec<TargetRef>> {
        let crate::attachment::AttachmentKind::Aura(enchant) = definition.permanent()?.attachment?
        else {
            return None;
        };
        Some(if definition.has_type(CardType::Creature) {
            Vec::new()
        } else {
            self.aura_entry_domain(
                enchant,
                creator,
                &super::protection::ProtectionSource::from(definition),
                current,
            )
        })
    }

    fn token_aura_frame(&self) -> Result<Option<crate::token::TokenAuraChoiceFrame>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let current = self.evaluated_characteristics()?;
        for member in work.groups.iter().flat_map(|group| group.members.iter()) {
            if work.aura_hosts.contains_key(member) {
                continue;
            }
            let preview = &work.members[member.ordinal()];
            let chooser = work
                .entry_controllers
                .get(member)
                .copied()
                .unwrap_or(preview.creator);
            if let Some(domain) = self.token_aura_domain(&preview.definition, chooser, &current)
                && !domain.is_empty()
            {
                return Ok(Some(crate::token::TokenAuraChoiceFrame {
                    revision: self.revision,
                    chooser,
                    preview: preview.clone(),
                    domain,
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn accept_token_aura(
        &self,
        expected: u64,
        frame: &crate::token::TokenAuraChoiceFrame,
        actor: SeatId,
        target: TargetRef,
    ) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.token_aura_frame()?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if !frame.domain().contains(&target) {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let Some(PendingWork::TokenEntry(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        work.aura_hosts.insert(frame.preview.member, target);
        next.revision = revision;
        Ok(next)
    }

    /// Advance forced groups or publish one complete batch; no partial entrant is
    /// observable during ordering. The shared driver handles the returned frame.
    pub(super) fn progress_token_entries(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongPhase);
        };
        while let Some(group) = work.groups.get(work.next_group) {
            if group.members.len() > 1 {
                return self
                    .token_timestamp_frame()
                    .map(EffectDecision::TimestampOrder)
                    .map(Some);
            }
            work.timestamp_order.extend(group.members.iter().copied());
            work.next_group = work
                .next_group
                .checked_add(1)
                .ok_or(EffectError::TokenCapacity)?;
        }
        if work.chosen_defenders.len() < work.members.len() {
            match work.defenders.len() {
                0 => {}
                1 => {
                    let (&only, _) = work
                        .defenders
                        .first_key_value()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    work.chosen_defenders.resize(work.members.len(), only);
                }
                _ => {
                    return self
                        .token_defender_frame()
                        .map(EffectDecision::TokenDefender)
                        .map(Some);
                }
            }
        }
        loop {
            if let Some(frame) = self.token_controller_frame()? {
                return Ok(Some(EffectDecision::TokenEntryChoice(frame)));
            }
            let Some(frame) = self.token_copy_frame()? else {
                break;
            };
            if let ZoneReplacementRequest::EntryCopy {
                donors,
                battlefield_donors,
                ..
            } = frame.request()
                && donors.is_empty()
                && let [(source, _)] = battlefield_donors.as_slice()
            {
                self.apply_token_entry_copy(
                    &frame,
                    ZoneReplacementReply::EntryCopyFromPermanent(*source),
                )?;
                continue;
            }
            return Ok(Some(EffectDecision::ZoneReplacement(frame)));
        }
        if let Some(frame) = self.token_entry_choice_frame()? {
            return Ok(Some(EffectDecision::TokenEntryChoice(frame)));
        }
        if let Some(frame) = self.token_aura_frame()? {
            return Ok(Some(EffectDecision::TokenAuraChoice(frame)));
        }
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongPhase);
        };
        let rules = work
            .members
            .iter()
            .filter_map(|member| {
                let chooser = match work.entry_choices.get(&member.member) {
                    Some(crate::land::EntryChoice::Opponent(seat)) => *seat,
                    _ => member.creator,
                };
                Some((
                    zone_replacement::EntryCopySubject::Token(member.member),
                    (
                        chooser,
                        zone_replacement::devour::multiplier(&member.definition)?,
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        if !rules.is_empty() {
            let mut fresh = self.begin_zone_replacements_with_entry(
                zone_replacement::AuthorizedZoneBatch {
                    cause: crate::occurrence::EventCause::Instruction {
                        entry: work.cursor.entry,
                        instruction: work.cursor.instruction,
                    },
                    action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                    intents: Vec::new(),
                },
                Some(&BTreeMap::new()),
            )?;
            fresh.enable_devour(self, rules);
            let Some(PendingWork::TokenEntry(work)) = &mut self.pending_work else {
                return Err(EffectError::WrongPhase);
            };
            let batch = work.devour.get_or_insert_with(|| fresh.clone());
            if let Some(frame) = batch.progress(self.revision)? {
                return Ok(Some(frame));
            }
            batch.verify_initial(fresh)?;
        }
        let Some(PendingWork::TokenEntry(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        let TokenEntryWork {
            devour,
            cursor,
            members,
            timestamp_order,
            attach_after_creation,
            mobilize,
            encore_defenders,
            defenders,
            chosen_defenders,
            entry_choices,
            entry_controllers,
            aura_hosts,
            fight_pairs,
            copied,
            original_definitions,
            ..
        } = *work;
        if timestamp_order.len() != members.len()
            || timestamp_order
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != members.len()
        {
            return Err(EffectError::InconsistentProgram);
        }
        if let Some(batch) = &devour {
            batch.commit_devour(self, append)?;
        }
        let before = self.rules_snapshot(None)?;
        // An entry-copy replacement can turn only some members of a single batch into unattachable Auras.
        let current = before.view().evaluated;
        let admitted = members
            .iter()
            .filter(|member| {
                self.token_aura_domain(
                    &member.definition,
                    entry_controllers
                        .get(&member.member)
                        .copied()
                        .unwrap_or(member.creator),
                    &current,
                )
                .is_none_or(|hosts| !hosts.is_empty())
            })
            .collect::<Vec<_>>();
        if admitted.is_empty() {
            self.take_myriad_destinations(cursor.entry);
            self.pending_work = Some(PendingWork::Resolution(cursor));
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .last_created_tokens
                .clear();
            self.advance_instruction()?;
            return Ok(None);
        }
        let inputs = admitted
            .iter()
            .map(|m| {
                (
                    m.creator,
                    original_definitions
                        .get(&m.member)
                        .unwrap_or(&m.definition)
                        .clone(),
                )
            })
            .collect();
        let (window, objects) = self.window.record_token_creations(
            self.window.revision(),
            inputs,
            &self.creature_vocabulary,
        )?;
        let by_member = admitted
            .iter()
            .zip(&objects)
            .map(|(member, object)| (member.member, object.id()))
            .collect::<BTreeMap<_, _>>();
        let order = timestamp_order
            .iter()
            .filter_map(|member| by_member.get(member).copied())
            .collect::<Vec<_>>();
        self.window = window;
        if let Some(batch) = devour {
            for (&member, &id) in &by_member {
                let amount =
                    batch.devour_counters(zone_replacement::EntryCopySubject::Token(member));
                if amount > 0 {
                    self.prospective_devour_counters.insert(id, amount);
                }
            }
        }
        for (member, copy) in copied {
            let Some(&id) = by_member.get(&member) else {
                continue;
            };
            self.characteristics
                .entry_copies
                .insert(id, copy.definition);
            if copy.extra_counters > 0 {
                self.copy_entry_counters.insert(id, copy.extra_counters);
            }
        }
        for member in admitted.iter().filter(|member| member.enters_tapped) {
            let id = by_member[&member.member];
            self.window.enter_tapped(id)?;
        }
        for (member, controller) in entry_controllers {
            let Some(&id) = by_member.get(&member) else {
                continue;
            };
            if members[member.ordinal()]
                .definition
                .permanent()
                .is_some_and(|p| p.entry_tap == Some(crate::land::EntryTap::ChosenOpponentControls))
            {
                self.entry_choices
                    .insert(id, crate::land::EntryChoice::Opponent(controller));
                self.characteristics
                    .entry_player_choices
                    .insert(id, controller);
            } else {
                self.window.set_entering_controller(id, controller)?;
            }
        }
        for (member, choice) in entry_choices {
            let Some(&id) = by_member.get(&member) else {
                continue;
            };
            match choice {
                crate::land::EntryChoice::Color(color) => {
                    self.entry_color_choices.insert(id, color);
                    if let Some(c) = color.color() {
                        self.characteristics.entry_color_choices.insert(id, c);
                    }
                }
                reveal @ (crate::land::EntryChoice::Players(_)
                | crate::land::EntryChoice::Opponent(_)
                | crate::land::EntryChoice::Reveal(_)
                | crate::land::EntryChoice::Chapter(_)
                | crate::land::EntryChoice::CardName(_)
                | crate::land::EntryChoice::PayLife(_)
                | crate::land::EntryChoice::OptionIndex(_)) => {
                    if let crate::land::EntryChoice::OptionIndex(idx) = reveal {
                        self.characteristics.entry_option_choices.insert(id, idx);
                    } else if let crate::land::EntryChoice::Opponent(seat) = reveal {
                        self.characteristics.entry_player_choices.insert(id, seat);
                    }
                    self.entry_choices.insert(id, reveal);
                }
                crate::land::EntryChoice::CreatureType(kind) => {
                    self.entry_creature_type_choices.insert(id, kind);
                }
            }
        }
        self.characteristics.register_battlefield_entries(&order)?;
        for (member, host) in aura_hosts {
            let Some(&aura) = by_member.get(&member) else {
                continue;
            };
            self.characteristics.attach(aura, host, true)?;
        }
        for member in &order {
            self.apply_entry_counters(&before, *member, PowerToughnessCounters::default(), 0)?;
        }
        if let Some(defenders) = self.take_myriad_destinations(cursor.entry) {
            if defenders.len() != members.len() {
                return Err(EffectError::InconsistentProgram);
            }
            for (member, object) in admitted.iter().zip(&objects) {
                let (seat, target) = defenders[member.member.ordinal()];
                self.put_attacking_at(object.id(), seat, target)?;
            }
        }
        for (member, object) in admitted.iter().zip(&objects) {
            if let Some(&target) = chosen_defenders.get(member.member.ordinal())
                && self
                    .window
                    .cards()
                    .get(object.id())
                    .and_then(GameObject::controller)
                    == Some(self.window.active_player())
            {
                self.put_attacking_at(object.id(), defenders[&target], target)?;
            }
        }
        if mobilize {
            let binding = self
                .execution_bindings(cursor.entry)
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let controller = self
                .window
                .resolving()
                .ok_or(EffectError::WrongPhase)?
                .controller();
            self.triggers.delay_sacrifice(
                binding.source.clone(),
                controller,
                crate::turn::Step::End,
                objects.iter().map(GameObject::id).collect(),
            );
        }
        let entry = cursor.entry;
        let instruction = cursor.instruction;
        self.pending_work = Some(PendingWork::Resolution(cursor));
        self.emit_from(
            crate::occurrence::EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |_, current, _| {
                objects
                    .iter()
                    .map(|object| {
                        Ok(crate::occurrence::RulesOccurrence::TokenCreated {
                            creator: object.owner(),
                            after: Box::new(
                                current
                                    .appearance(object.id())
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                        })
                    })
                    .collect()
            },
        )?;
        if !fight_pairs.is_empty() {
            let pairs = fight_pairs
                .into_iter()
                .filter_map(|(member, opponent)| {
                    by_member.get(&member).map(|token| (*token, opponent))
                })
                .collect();
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[instruction] = Instruction::FightPairedCreatures(pairs);
            return Ok(None);
        }
        if !encore_defenders.is_empty() {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongPhase)?
                .clone();
            let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
                return Err(EffectError::InconsistentProgram);
            };
            let source = payload
                .encore_source
                .as_deref()
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            let ids = objects.iter().map(GameObject::id).collect::<Vec<_>>();
            self.encore_attack_targets.extend(
                admitted
                    .iter()
                    .zip(&ids)
                    .map(|(member, id)| (*id, encore_defenders[member.member.ordinal()])),
            );
            self.triggers.delay_sacrifice(
                source,
                entry.controller(),
                crate::turn::Step::End,
                ids.clone(),
            );
            self.modify_objects_for_duration(
                &entry,
                instruction,
                ids.into_iter().collect(),
                ResolvedParts(vec![ResolvedPart::Abilities(
                    AbilityChange::grant_keywords([KeywordAbility::Haste]),
                )]),
                ResolvedDuration::NoStatedDuration,
                append,
            )?;
        }
        if let Some(source) = attach_after_creation {
            match objects.as_slice() {
                [] => {}
                [token] => {
                    let target = TargetRef::Object(token.id());
                    let cut = self.rules_snapshot(None)?;
                    if let Some(change) = self.attach_if_legal(entry, source, target)? {
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction { entry, instruction },
                            &cut,
                            None,
                            append,
                            |_, _, _| Ok(vec![change]),
                        )?;
                    }
                }
                tokens => {
                    let cursor = self
                        .resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    cursor.chosen_token_equipment = Some(source);
                    cursor.last_created_tokens = tokens.iter().map(GameObject::id).collect();
                    return Ok(None);
                }
            }
        }
        if let Some(Instruction::CreateAuraTokenAttached { host, .. }) = self
            .stack_body(entry)
            .and_then(|body| body.instructions.get(instruction))
            .cloned()
        {
            let entry_obj = self.window.resolving().filter(|e| e.id() == entry).cloned();
            let host_target = entry_obj.as_ref().and_then(|resolving| match host {
                crate::program::MoveSubjectRef::TargetObject => self
                    .instruction_target(resolving)
                    .ok()
                    .map(|t| t.choice().target),
                crate::program::MoveSubjectRef::AbilitySource => match resolving.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => {
                        Some(TargetRef::Object(source.id()))
                    }
                    _ => None,
                },
                crate::program::MoveSubjectRef::TriggeringObject => self
                    .execution_bindings(entry)
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|m| m.subject_object_id())
                    .map(TargetRef::Object),
                crate::program::MoveSubjectRef::AttachedHost => match resolving.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => self.attached_to(source.id()),
                    _ => None,
                },
                crate::program::MoveSubjectRef::RevealedCard => None,
                crate::program::MoveSubjectRef::CreatedTokens => self
                    .resolution_cursor()
                    .and_then(|c| c.last_created_tokens.first().copied())
                    .map(TargetRef::Object),
                crate::program::MoveSubjectRef::LastMoved => self
                    .resolution_cursor()
                    .and_then(|c| {
                        c.results
                            .last_moved_objects
                            .first()
                            .copied()
                            .or(c.results.returned_target_permanent)
                    })
                    .map(TargetRef::Object),
                crate::program::MoveSubjectRef::RememberedFirst => self
                    .legal_linked_targets()
                    .ok()
                    .flatten()
                    .map(|(a, _)| TargetRef::Object(a)),
                crate::program::MoveSubjectRef::RememberedSecond => self
                    .legal_linked_targets()
                    .ok()
                    .flatten()
                    .map(|(_, b)| TargetRef::Object(b)),
            });
            if let Some(target) = host_target {
                for token in &objects {
                    let cut = self.rules_snapshot(None)?;
                    if let Some(change) = self.attach_if_legal(entry, token.id(), target)? {
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction { entry, instruction },
                            &cut,
                            None,
                            append,
                            |_, _, _| Ok(vec![change]),
                        )?;
                    }
                }
            }
        }
        if let Some(Instruction::CreateTokenCopy {
            sacrifice_at_end_step,
            exile_at_end_step,
            ..
        }) = self
            .stack_body(entry)
            .and_then(|body| body.instructions.get(instruction))
            && (*sacrifice_at_end_step || *exile_at_end_step)
            && !objects.is_empty()
        {
            let sacrifice_at_end_step = *sacrifice_at_end_step;
            let exile_at_end_step = *exile_at_end_step;
            let ids: Vec<ObjectId> = objects.iter().map(GameObject::id).collect();
            let resolving = self.window.resolving().cloned();
            if let Some(resolving) = resolving {
                let snapshot = self.rules_snapshot(None)?;
                let source_id = match resolving.kind() {
                    StackKind::CardSpell(id) => Some(*id),
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                let appearance = source_id
                    .and_then(|sid| {
                        snapshot
                            .view()
                            .appearance(sid)
                            .or_else(|| self.departed_reflexive_sources.get(&sid).cloned())
                    })
                    .or_else(|| {
                        self.execution_bindings(entry)
                            .and_then(|b| b.trigger.as_ref())
                            .map(|t| t.source.clone())
                    });
                if let Some(app) = appearance {
                    if sacrifice_at_end_step {
                        self.triggers.delay_sacrifice(
                            app.clone(),
                            resolving.controller(),
                            crate::turn::Step::End,
                            ids.clone(),
                        );
                    }
                    if exile_at_end_step {
                        self.triggers.delay_exile(
                            app,
                            resolving.controller(),
                            crate::turn::Step::End,
                            ids,
                        );
                    }
                }
            }
        }
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .last_created_tokens = objects.iter().map(GameObject::id).collect();
        self.advance_instruction()?;
        #[cfg(test)]
        characteristic_observations::instruction(self, "Completed", entry, instruction, None);
        Ok(None)
    }
}
