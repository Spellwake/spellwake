//! Semantic creature-type substitutions in printed definitions, never raw text.
use super::{StaticContinuousAbility, StaticRestriction, types::*};
use crate::mana::ManaOutput;
use crate::program::{ExecutableBody, Instruction};
use spellwake_contracts::creature_types::CreatureTypeId;
use std::collections::BTreeSet;

fn word(change: CreatureTypeTextChange, value: &mut CreatureTypeId) {
    if *value == change.from {
        *value = change.to;
    }
}
fn words(change: CreatureTypeTextChange, values: &mut BTreeSet<CreatureTypeId>) {
    if values.remove(&change.from) {
        values.insert(change.to);
    }
}
fn operand(change: CreatureTypeTextChange, value: &mut CreatureTypeWord) {
    match value {
        CreatureTypeWord::Literal(value) => word(change, value),
        // A slot denotes a runtime value, not a printed type word.
        CreatureTypeWord::Chosen(_) => {}
    }
}
fn subtypes<T>(
    change: CreatureTypeTextChange,
    value: &mut SubtypeOperation<T>,
    rewrite: impl Fn(CreatureTypeTextChange, &mut T),
) {
    match value {
        SubtypeOperation::Keep => {}
        SubtypeOperation::Remove(values) => words(change, values),
        SubtypeOperation::Add(value) | SubtypeOperation::Set(value) => rewrite(change, value),
    }
}
fn instruction_subtype(change: CreatureTypeTextChange, value: &mut InstructionSubtypeOperand) {
    match value {
        InstructionSubtypeOperand::Literal(values) => words(change, values),
        InstructionSubtypeOperand::Chosen(_) => {}
    }
}
fn static_subtype(change: CreatureTypeTextChange, value: &mut StaticSubtypeOperand) {
    match value {
        StaticSubtypeOperand::Literal(values)
        | StaticSubtypeOperand::ImprintedCreatureTypes { keep: values } => words(change, values),
        StaticSubtypeOperand::SourceCurrentSubtypes
        | StaticSubtypeOperand::SourceEntryCreatureType
        | StaticSubtypeOperand::AllCreatureTypes => {}
    }
}
pub(crate) fn static_source_condition(
    change: CreatureTypeTextChange,
    cond: &mut StaticSourceCondition,
) {
    match cond {
        StaticSourceCondition::ControllerControlsSubtype(kind)
        | StaticSourceCondition::OpponentControlsSubtype(kind) => word(change, kind),
        StaticSourceCondition::ControlsMatching { selector: sel, .. }
        | StaticSourceCondition::OpponentControlsMatching { selector: sel, .. }
        | StaticSourceCondition::AttachedHostMatches(sel)
        | StaticSourceCondition::ControlsExactCount { selector: sel, .. } => {
            selector(change, sel);
        }
        StaticSourceCondition::Eval(rc) => rc.rewrite_creature_type(change.from, change.to),
        StaticSourceCondition::Not(inner) => static_source_condition(change, inner),
        StaticSourceCondition::And(a, b) | StaticSourceCondition::Or(a, b) => {
            static_source_condition(change, a);
            static_source_condition(change, b);
        }
        _ => {}
    }
}
fn selector(change: CreatureTypeTextChange, value: &mut PermanentSelector) {
    if let Some(kind) = &mut value.controlled_creatures_and_typed_tokens {
        word(change, kind);
    }
    for subtype in [
        &mut value.creature_subtype,
        &mut value.forbidden_creature_subtype,
    ]
    .into_iter()
    .flatten()
    {
        word(change, subtype);
    }
    words(change, &mut value.any_creature_subtypes);
    for set in [
        &mut value.required_subtypes,
        &mut value.any_subtypes,
        &mut value.forbidden_subtypes,
    ] {
        if set.remove(&SelectorSubtype::Creature(change.from)) {
            set.insert(SelectorSubtype::Creature(change.to));
        }
    }
    if let Some(cond) = &mut value.source_condition {
        static_source_condition(change, cond);
    }
}
fn numeric(change: CreatureTypeTextChange, value: &mut NumericExpression) {
    match value {
        NumericExpression::Literal(_)
        | NumericExpression::ZoneCount { .. }
        | NumericExpression::Counters { .. }
        | NumericExpression::ImprintedStat { .. }
        | NumericExpression::PlayerStat(_)
        | NumericExpression::AttachedAurasAndEquipment { .. }
        | NumericExpression::ConditionalBonus { .. } => {}
        NumericExpression::Count {
            selector: value, ..
        } => selector(change, value),
        NumericExpression::Eval(expr) => expr.rewrite_creature_type(change.from, change.to),
        NumericExpression::Add(lhs, rhs) => {
            numeric(change, lhs);
            numeric(change, rhs);
        }
        NumericExpression::Scale { expr, .. } | NumericExpression::Cap { expr, .. } => {
            numeric(change, expr);
        }
    }
}
fn rewrite_static_restriction(change: CreatureTypeTextChange, rule: &mut StaticRestriction) {
    match rule {
        StaticRestriction::PreventControlledTransform { subtype, except } => {
            word(change, subtype);
            word(change, except);
        }
        StaticRestriction::ProhibitCasting { filter, .. } => {
            filter.rewrite_creature_type(change.from, change.to);
        }
        StaticRestriction::OthersEnterTapped(sel)
        | StaticRestriction::PreventMatchingActivations(sel)
        | StaticRestriction::PreventMatchingNonManaActivations(sel)
        | StaticRestriction::IgnoreOpponentsHexproof(sel)
        | StaticRestriction::ActivateAbilitiesAsThoughHaste(sel)
        | StaticRestriction::CannotBeSacrificed(sel)
        | StaticRestriction::CannotBeUntapped(sel)
        | StaticRestriction::CantBeTargetedBy { selector: sel, .. }
        | StaticRestriction::EnterUntapped(sel)
        | StaticRestriction::ProhibitEntering(sel)
        | StaticRestriction::PreventCounters {
            selector: Some(sel),
            ..
        } => selector(change, sel),
        _ => {}
    }
}
fn rewrite_battlefield_static_parts(change: CreatureTypeTextChange, parts: &mut StaticParts) {
    for part in &mut parts.0 {
        match part {
            StaticPart::Text(value) => {
                word(change, &mut value.from);
                word(change, &mut value.to);
            }
            StaticPart::Type(value) | StaticPart::TypeWithSubtypes { base: value, .. } => {
                subtypes(change, &mut value.subtypes, static_subtype)
            }
            StaticPart::SetPT(value) | StaticPart::ModifyPT(value) => {
                numeric(change, &mut value.power);
                numeric(change, &mut value.toughness);
            }
            StaticPart::BlockingRestriction(
                BlockingRestriction::OnlyBlockedBySubtype(value)
                | BlockingRestriction::CannotBlockSubtype(value)
                | BlockingRestriction::CannotBeBlockedBySubtype(value),
            ) => word(change, value),
            StaticPart::BlockingRestriction(
                BlockingRestriction::CannotBlockMatching(filter)
                | BlockingRestriction::CannotBeBlockedByMatching(filter)
                | BlockingRestriction::OnlyBlockedByMatching(filter)
                | BlockingRestriction::CanBlockOnlyMatching(filter)
                | BlockingRestriction::CannotAttackUnlessDefenderControls(filter)
                | BlockingRestriction::MustBeBlockedByAllMatching(filter),
            ) => filter.rewrite_creature_type(change.from, change.to),
            StaticPart::BlockingRestriction(_) => {}
            StaticPart::GrantRestriction(restriction) => {
                rewrite_static_restriction(change, restriction);
            }
            StaticPart::GrantActivated(ability)
            | StaticPart::GrantActivatedRestricted { ability, .. }
            | StaticPart::GrantLoyalty { ability, .. } => body(change, &mut ability.body),
            StaticPart::GrantSubtypeProtection { types, .. } => static_subtype(change, types),
            StaticPart::GrantTrigger(ability) => trigger(change, ability),
            StaticPart::Abilities(ability_change) => {
                for ability in &mut ability_change.granted_triggers {
                    trigger(change, ability);
                }
            }
            StaticPart::Names(_)
            | StaticPart::ControlSourceController
            | StaticPart::Color(_)
            | StaticPart::InheritCounteredCreatureAbilities
            | StaticPart::InheritZoneAbilities { .. }
            | StaticPart::InheritExiledCreatureAbilities
            | StaticPart::GrantFixedColorMana(_)
            | StaticPart::GrantAnyColorMana
            | StaticPart::GrantMana(_)
            | StaticPart::GrantProtectionChosenColor
            | StaticPart::GrantProtectionChosenPlayer
            | StaticPart::SetPTToOwnManaValue
            | StaticPart::ModifyPTPerOwnColor { .. }
            | StaticPart::SwitchPT => {}
        }
    }
}
fn rewrite_static_ability(change: CreatureTypeTextChange, ability: &mut StaticContinuousAbility) {
    ability.rewrite_creature_type_in_rule(change);
    if let StaticContinuousAbility::Battlefield {
        selector: target,
        parts,
        ..
    } = ability
    {
        selector(change, target);
        rewrite_battlefield_static_parts(change, parts);
    }
}
fn parts(change: CreatureTypeTextChange, value: &mut ResolvedPartsSpec) {
    for part in &mut value.0 {
        match part {
            ResolvedPartSpec::Abilities(ability_change) => {
                for ability in &mut ability_change.granted_triggers {
                    trigger(change, ability);
                }
            }
            ResolvedPartSpec::Text(value) => {
                operand(change, &mut value.from);
                operand(change, &mut value.to);
            }
            ResolvedPartSpec::Type(value)
            | ResolvedPartSpec::TypeWithSubtypes { base: value, .. } => {
                subtypes(change, &mut value.subtypes, instruction_subtype);
            }
            ResolvedPartSpec::GrantTrigger(ability) => trigger(change, ability),
            ResolvedPartSpec::GrantActivated(ability)
            | ResolvedPartSpec::GrantActivatedRestricted { ability, .. }
            | ResolvedPartSpec::GrantLoyalty { ability, .. } => {
                body(change, &mut ability.body);
            }
            ResolvedPartSpec::GrantAnyColorMana
            | ResolvedPartSpec::GrantFixedColorMana(_)
            | ResolvedPartSpec::GrantMana(_) => {}
            ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::OnlyBlockedBySubtype(value)
                | BlockingRestriction::CannotBlockSubtype(value)
                | BlockingRestriction::CannotBeBlockedBySubtype(value),
            ) => word(change, value),
            ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBlockMatching(filter)
                | BlockingRestriction::CannotBeBlockedByMatching(filter)
                | BlockingRestriction::OnlyBlockedByMatching(filter)
                | BlockingRestriction::CanBlockOnlyMatching(filter)
                | BlockingRestriction::CannotAttackUnlessDefenderControls(filter)
                | BlockingRestriction::MustBeBlockedByAllMatching(filter),
            ) => filter.rewrite_creature_type(change.from, change.to),
            ResolvedPartSpec::BlockingRestriction(_) => {}
            ResolvedPartSpec::ModifyPTExpr { power, toughness }
            | ResolvedPartSpec::SetPTExpr { power, toughness } => {
                power.rewrite_creature_type(change.from, change.to);
                toughness.rewrite_creature_type(change.from, change.to);
            }
            ResolvedPartSpec::Names(_)
            | ResolvedPartSpec::Control(_)
            | ResolvedPartSpec::Color(_)
            | ResolvedPartSpec::ChosenColor { .. }
            | ResolvedPartSpec::ChosenBasicLandType { .. }
            | ResolvedPartSpec::ProtectionFromEachOpponent
            | ResolvedPartSpec::CannotBeTargetedByColors(_)
            | ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(_)
            | ResolvedPartSpec::MustBlockSubject(_)
            | ResolvedPartSpec::SetPT(_)
            | ResolvedPartSpec::ModifyPT(_)
            | ResolvedPartSpec::SwitchPT => {}
        }
    }
}
pub(super) fn rewrite_executable_body(value: &mut ExecutableBody, change: CreatureTypeTextChange) {
    body(change, value);
}

fn rewrite_object_query(change: CreatureTypeTextChange, query: &mut crate::toolbox::ObjectQuery) {
    match query {
        crate::toolbox::ObjectQuery::ZoneScan { filter, slice, .. } => {
            filter.rewrite_creature_type(change.from, change.to);
            match slice {
                crate::toolbox::QuerySlice::Top(expr)
                | crate::toolbox::QuerySlice::Bottom(expr) => {
                    expr.rewrite_creature_type(change.from, change.to);
                }
                crate::toolbox::QuerySlice::UntilMatch { filter, count, .. } => {
                    filter.rewrite_creature_type(change.from, change.to);
                    count.rewrite_creature_type(change.from, change.to);
                }
                crate::toolbox::QuerySlice::All => {}
            }
        }
        crate::toolbox::ObjectQuery::BoundFiltered { filter, .. } => {
            filter.rewrite_creature_type(change.from, change.to);
        }
        crate::toolbox::ObjectQuery::Filtered { source, filter } => {
            rewrite_object_query(change, source);
            filter.rewrite_creature_type(change.from, change.to);
        }
        crate::toolbox::ObjectQuery::Take { source, count } => {
            rewrite_object_query(change, source);
            count.rewrite_creature_type(change.from, change.to);
        }
        crate::toolbox::ObjectQuery::EffectSubject(subject) => {
            subject.rewrite_creature_type(change.from, change.to);
        }
        crate::toolbox::ObjectQuery::Tagged {
            filter: Some(filter),
            ..
        } => {
            filter.rewrite_creature_type(change.from, change.to);
        }
        crate::toolbox::ObjectQuery::Union(parts) => {
            for part in parts {
                rewrite_object_query(change, part);
            }
        }
        crate::toolbox::ObjectQuery::Difference(left, right)
        | crate::toolbox::ObjectQuery::Intersect(left, right) => {
            rewrite_object_query(change, left);
            rewrite_object_query(change, right);
        }
        _ => {}
    }
}

fn body(change: CreatureTypeTextChange, value: &mut ExecutableBody) {
    if let Some(modes) = &mut value.triggered_modes {
        for option in &mut modes.options {
            body(change, option);
        }
    }
    if let crate::program::BodyTarget::FilteredPermanent(filter)
    | crate::program::BodyTarget::FilteredOwnGraveyardCard(filter)
    | crate::program::BodyTarget::FilteredAnyGraveyardCard { filter, .. } = &mut value.target
    {
        filter.rewrite_creature_type(change.from, change.to);
    } else if let crate::program::BodyTarget::SingleSpell(filter)
    | crate::program::BodyTarget::SingleStackObject(filter) = &mut value.target
    {
        filter.rewrite_creature_type(change.from, change.to);
    }
    for instruction in &mut value.instructions {
        let rewrite_amount = |amount: &mut crate::program::ValueExpr| {
            amount.rewrite_creature_type(change.from, change.to);
        };
        match instruction {
            Instruction::AddMana(yield_kind) | Instruction::AddManaForPlayer { yield_kind, .. } => {
                let inner = match yield_kind {
                    crate::program::EffectManaYield::RetainedUntilCleanup(inner) => inner.as_mut(),
                    other => other,
                };
                if let crate::program::EffectManaYield::Counted { amount, .. }
                | crate::program::EffectManaYield::RestrictedCounted { amount, .. }
                | crate::program::EffectManaYield::RestrictedChoice { amount, .. }
                | crate::program::EffectManaYield::RetainedUntilEndOfCombat {
                    amount, ..
                }
                | crate::program::EffectManaYield::Split { amount, .. }
                | crate::program::EffectManaYield::Choice { amount, .. }
                | crate::program::EffectManaYield::LandProducible { amount, .. }
                | crate::program::EffectManaYield::CommanderIdentity { amount }
                | crate::program::EffectManaYield::SourceChosenColor { amount }
                | crate::program::EffectManaYield::DistinctColors { amount } = inner
                {
                    rewrite_amount(amount);
                }
            }
            Instruction::DamageTargetControllersCreatures { amount, .. }
            | Instruction::Scry { amount, .. }
            | Instruction::Surveil { amount, .. }
            | Instruction::Mill { amount, .. }
            | Instruction::OfferDynamicManaPayment {
                extra_generic: amount,
                ..
            }
            | Instruction::CollectEvidence {
                minimum: amount, ..
            }
            | Instruction::Blight { amount, .. }
            | Instruction::Discard { amount, .. }
            | Instruction::ExileTopPlay { count: amount, .. }
            | Instruction::LookTopChoose { count: amount, .. }
            | Instruction::RepeatLoop { count: amount, .. } => rewrite_amount(amount),
            Instruction::ChooseObjects {
                min,
                max,
                max_total_power,
                ..
            } => {
                rewrite_amount(min);
                rewrite_amount(max);
                if let Some(limit) = max_total_power {
                    rewrite_amount(limit);
                }
            }
            Instruction::Sacrifice { amount, .. } => rewrite_amount(amount),
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations, .. } => {
                for spec in creations {
                    rewrite_amount(&mut spec.amount);
                }
            }
            _ => {}
        }
        match instruction {
            Instruction::Sacrifice { filter, .. } => {
                if let Some(obj) = &mut filter.object_filter {
                    obj.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::LookTopChoose {
                filter, secondary, ..
            } => {
                if let Some(filter) = filter {
                    filter.rewrite_creature_type(change.from, change.to);
                }
                if let Some(sec) = secondary {
                    sec.filter.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::ChooseObjects { filter, .. }
            | Instruction::RevealUntilMatch { filter, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            Instruction::RepeatLoop { body: inner, .. } => {
                body(change, inner);
            }
            Instruction::ChooseOption { options, .. } => {
                for branch in options {
                    body(change, &mut branch.body);
                }
            }
            Instruction::DrawCards(spec) => {
                rewrite_amount(&mut spec.amount);
            }
            Instruction::AdjustLife(spec) => match &mut spec.op {
                crate::toolbox::LifeOp::Gain(v)
                | crate::toolbox::LifeOp::Lose(v)
                | crate::toolbox::LifeOp::Set(v)
                | crate::toolbox::LifeOp::Pay(v)
                | crate::toolbox::LifeOp::Drain { amount: v, .. } => rewrite_amount(v),
                crate::toolbox::LifeOp::LoseHalf { .. } => {}
            },
            Instruction::DealDamage(spec) => {
                if let crate::toolbox::DamageSourceQuery::Objects(q) = &mut spec.source {
                    rewrite_object_query(change, q);
                }
                match &mut spec.recipients {
                    crate::toolbox::DamageTargetQuery::Objects(q)
                    | crate::toolbox::DamageTargetQuery::Both { objects: q, .. } => {
                        rewrite_object_query(change, q);
                    }
                    crate::toolbox::DamageTargetQuery::Relative(rel) => {
                        if let crate::program::DamageRecipientsRef::Group(group) = rel.as_mut() {
                            match group {
                                crate::program::DamageGroup::Filtered(f)
                                | crate::program::DamageGroup::FilteredAndPlayers {
                                    filter: f,
                                    ..
                                } => f.rewrite_creature_type(change.from, change.to),
                                crate::program::DamageGroup::CreaturesExcept(kind)
                                | crate::program::DamageGroup::CreaturesOfSubtype(kind) => {
                                    word(change, kind);
                                }
                                _ => {}
                            }
                        }
                    }
                    crate::toolbox::DamageTargetQuery::Targets
                    | crate::toolbox::DamageTargetQuery::Players(_) => {}
                }
                if let crate::program::DamageAmountSpec::Expr(e) = &mut spec.amount {
                    rewrite_amount(e);
                }
            }
            Instruction::MutatePlayer(spec) => match &mut spec.op {
                crate::toolbox::PlayerMutation::AddCounter { amount, .. }
                | crate::toolbox::PlayerMutation::PayCounter { amount, .. } => {
                    rewrite_amount(amount);
                }
                _ => {}
            },
            Instruction::MutateObject(spec) => {
                rewrite_object_query(change, &mut spec.source);
                match &mut spec.op {
                    crate::toolbox::ObjectMutation::AddCounters { amount, .. }
                    | crate::toolbox::ObjectMutation::RemoveCounters { amount, .. }
                    | crate::toolbox::ObjectMutation::MoveCounters {
                        amount: Some(amount),
                        ..
                    } => rewrite_amount(amount),
                    _ => {}
                }
            }
            Instruction::ApplyContinuous(spec) => {
                if let Some(query) = &mut spec.selector {
                    rewrite_object_query(change, query);
                }
                match &mut spec.payload {
                    crate::toolbox::ContinuousPayload::ResolvedParts(value) => parts(change, value),
                    crate::toolbox::ContinuousPayload::DynamicBoost {
                        power, toughness, ..
                    } => {
                        rewrite_amount(power);
                        rewrite_amount(toughness);
                    }
                    crate::toolbox::ContinuousPayload::PreventDamage(prev) => {
                        if let crate::damage::PreventionSourceSpec::Filter(filter) =
                            &mut prev.source
                        {
                            for subtype in filter.forbidden_subtypes.iter_mut().flatten() {
                                word(change, subtype);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Instruction::CreateTokenCopy {
                count,
                add_subtypes,
                ..
            } => {
                count.rewrite_creature_type(change.from, change.to);
                if add_subtypes.remove(&change.from) {
                    add_subtypes.insert(change.to);
                }
            }
            Instruction::BindResolutionX(expr) => {
                expr.rewrite_creature_type(change.from, change.to);
            }
            Instruction::CreateAuraTokenAttached { definition, .. } => {
                definition.rewrite_creature_types(change);
            }
            Instruction::ScheduleDelayedEventTrigger { body: inner, .. } => {
                body(change, inner);
            }
            Instruction::DivideDamageExprAmongTargets { total, .. } => {
                total.rewrite_creature_type(change.from, change.to);
            }
            Instruction::DivideDamageAmongTargets { .. }
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::ChooseOneLastMoved { .. }
            | Instruction::CounterMatchingStackObjects { .. }
            | Instruction::TransferAllCountersFromTriggeringLki { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ExileTopPlay { .. }
            | Instruction::ChooseValue { .. }
            | Instruction::ZoneOp(_)
            | Instruction::PromptChoice(_)
            | Instruction::BranchCondition { .. }
            | Instruction::ForEach { .. }
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::BindEnv { .. }
            | Instruction::SetObjectTag { .. }
            | Instruction::UpdateStateTrack { .. }
            | Instruction::SideDeckOp(_)
            | Instruction::Vote { .. }
            | Instruction::Incubate { .. }
            | Instruction::Connive { .. }
            | Instruction::ConniveFinalize { .. }
            | Instruction::Discover { .. }
            | Instruction::Clash { .. }
            | Instruction::RingTemptsYou
            | Instruction::VentureIntoDungeon { .. }
            | Instruction::TakeInitiative { .. }
            | Instruction::UnlockDoor { .. }
            | Instruction::ManifestDread
            | Instruction::Fight { .. }
            | Instruction::Exchange(_)
            | Instruction::CastLinkedFaceDownFree
            | Instruction::Ripple(_)
            | Instruction::Demonstrate
            | Instruction::Enlist => {}
            Instruction::Monstrosity { amount }
            | Instruction::CopyStackObject { count: amount, .. }
            | Instruction::Earthbend { amount } => {
                amount.rewrite_creature_type(change.from, change.to);
            }
            Instruction::RollDie { bands, .. } => {
                for band in bands {
                    body(change, &mut band.body);
                }
            }
            Instruction::FlipCoins {
                win_body,
                lose_body,
                ..
            } => {
                if let Some(inner) = win_body {
                    body(change, inner);
                }
                if let Some(inner) = lose_body {
                    body(change, inner);
                }
            }
            Instruction::ChooseOptionForPlayers { options, .. } => {
                for branch in options {
                    body(change, &mut branch.body);
                }
            }
            Instruction::Behold { filter, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            Instruction::BecomeCopy {
                receiver,
                exceptions,
                ..
            } => {
                receiver.rewrite_creature_type(change.from, change.to);
                if exceptions.extra_creature_subtypes.remove(&change.from) {
                    exceptions.extra_creature_subtypes.insert(change.to);
                }
                for tr in &mut exceptions.extra_triggered {
                    trigger(change, tr);
                }
                for act in &mut exceptions.extra_activated {
                    body(change, &mut act.body);
                }
            }
            Instruction::BecomeCopyFixedTargets { exceptions, .. } => {
                if exceptions.extra_creature_subtypes.remove(&change.from) {
                    exceptions.extra_creature_subtypes.insert(change.to);
                }
                for tr in &mut exceptions.extra_triggered {
                    trigger(change, tr);
                }
                for act in &mut exceptions.extra_activated {
                    body(change, &mut act.body);
                }
            }
            Instruction::BranchIfFalse { condition, .. } => {
                condition.left.rewrite_creature_type(change.from, change.to);
                condition
                    .right
                    .rewrite_creature_type(change.from, change.to);
            }
            Instruction::FixedTargetClause(clause) => body(change, clause),
            Instruction::DamageTargetControllersCreatures { .. } => {}
            Instruction::CreateTokenAndAttach { definition } => {
                definition.rewrite_creature_types(change)
            }
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations, .. } => {
                for creation in creations {
                    match &mut creation.base_size {
                        Some(crate::token::TokenSize::AtCreation(size)) => {
                            rewrite_amount(size);
                        }
                        Some(crate::token::TokenSize::AtCreationPair { power, toughness }) => {
                            rewrite_amount(power);
                            rewrite_amount(toughness);
                        }
                        Some(crate::token::TokenSize::AbilitySourceCounters(_)) | None => {}
                    }
                    creation.definition.rewrite_creature_types(change);
                }
            }
            Instruction::ChooseCreatureType { excluding, .. } => words(change, excluding),
            Instruction::SearchLibrary { filter } => {
                if let crate::program::SearchCount::UpToExpr(expr) = &mut filter.count {
                    rewrite_amount(expr);
                }
                match &mut filter.matcher {
                    crate::program::SearchCardMatcher::CreatureSubtype(kind) => {
                        word(change, kind);
                    }
                    crate::program::SearchCardMatcher::Filtered(obj_filter) => {
                        obj_filter.rewrite_creature_type(change.from, change.to);
                    }
                    crate::program::SearchCardMatcher::SameNameAs {
                        extra: Some(obj_filter),
                        ..
                    } => {
                        obj_filter.rewrite_creature_type(change.from, change.to);
                    }
                    _ => {}
                }
            }
            Instruction::Amass { subtype: kind, .. } => word(change, kind),
            Instruction::AffectPermanentTargets { action, .. } => match action {
                crate::program::PermanentTargetAction::PutCounters { amount, .. }
                | crate::program::PermanentTargetAction::Damage(amount) => {
                    rewrite_amount(amount);
                }
                crate::program::PermanentTargetAction::RemoveCounters {
                    amount: Some(amount),
                    ..
                } => {
                    rewrite_amount(amount);
                }
                crate::program::PermanentTargetAction::Modify { parts: value, .. }
                | crate::program::PermanentTargetAction::ModifyWithPartner {
                    parts: value, ..
                } => {
                    parts(change, value);
                }
                _ => {}
            },
            Instruction::ForEachPlayer { body: inner, .. } => body(change, inner),
            Instruction::OfferAnyPlayer {
                cost_body,
                paid_body,
                declined_body,
                ..
            } => {
                body(change, cost_body);
                body(change, paid_body);
                if let Some(declined) = declined_body {
                    body(change, declined);
                }
            }
            Instruction::DealDamageToAnnouncedTargets { amount, .. } => rewrite_amount(amount),
            Instruction::GrantChosenProtection { recipients, .. } => {
                if let crate::program::ProtectionRecipients::Matching(filter) = recipients {
                    filter.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::MoveCounters { amount, .. } => {
                if let Some(expr) = amount {
                    rewrite_amount(expr);
                }
            }
            Instruction::RevealUntilMatchExt { filter, count, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
                rewrite_amount(count);
            }
            Instruction::LookTopChooseExt { count, filter, .. } => {
                rewrite_amount(count);
                if let Some(filter) = filter {
                    filter.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::RevealTopTwoPiles { count, .. } => {
                rewrite_amount(count);
            }
            Instruction::CreateTokenCopyExt {
                count,
                extra_subtypes,
                ..
            } => {
                rewrite_amount(count);
                words(change, extra_subtypes);
            }
            // These admitted instructions contain no literal creature-type words.
            // Draw's chosen-type reference remains a runtime local value.
            Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::ModifyAnyNumberOfPermanents { .. }
            | Instruction::ExileOnePerPlayerUntilSourceLeaves
            | Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::DamageFromGrantingPermanent { .. }
            | Instruction::Populate
            | Instruction::Bolster(_)
            | Instruction::SacrificeRandomUntilLand
            | Instruction::Discard { .. }
            | Instruction::Jump { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::AdditionalCombat { .. }
            | Instruction::ExileDefeatedSiege
            | Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource
            | Instruction::Proliferate
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::OfferSourceCounters { .. }
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::FightFixedTargets
            | Instruction::BindOrderedPermanents { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
            | Instruction::ReturnGraveyardCreatureWithCorpse(_)
            | Instruction::PutHandPermanentsOntoBattlefield { .. }
            | Instruction::MayRevealLibraryTop
            | Instruction::ManifestTop { .. }
            | Instruction::ChooseFoundSpell
            | Instruction::CastFoundSpell(_)
            | Instruction::RequireCondition(_)
            | Instruction::Encore
            | Instruction::ReselectAttackDefender
            | Instruction::SetBlockDeclarations(_)
            | Instruction::AdditionalLoyaltyActivation
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::ReturnCardsDestroyedThisWay
            | Instruction::ExileTopForPlay { .. }
            | Instruction::ScheduleCreatedTokenExile(_)
            | Instruction::ScheduleCreatedTokenSacrifice
            | Instruction::ExileDelayedObjects(_)
            | Instruction::GrantFlashback(_)
            | Instruction::AddMana(_)
            | Instruction::AddManaForPlayer { .. }
            | Instruction::ReturnEarthbentCard
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::PairWithCreature { .. }
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::FlipCoin { .. }
            | Instruction::Cascade
            | Instruction::CascadeBottom
            | Instruction::Myriad
            | Instruction::WatchNextSpellOrLoyaltyTwice
            | Instruction::WatchNextLoyaltyCopy
            | Instruction::CopyCapturedAbility
            | Instruction::CopyOnCast(_)
            | Instruction::MadnessCast
            | Instruction::MiracleCast(_)
            | Instruction::CastDefeatedSiege
            | Instruction::ReboundCast(_)
            | Instruction::EndStepCast { .. }
            | Instruction::SacrificeDelayedObjects(_)
            | Instruction::ScheduleArtifactReturn(_)
            | Instruction::ReturnDelayedGraveyardCard(_)
            | Instruction::ReturnDelayedGraveyardCardTransformed(_)
            | Instruction::ReturnDelayedPermanentToHand(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::ResolveTransfer(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::CounterWardSource
            | Instruction::CounterTriggeringStackObject
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ChooseRevealedHandCard { .. }
            | Instruction::ResolvePowerAbility(_)
            | Instruction::Attach(_)
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. }
            | Instruction::CounterSpell
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::ChooseNewTargets
            | Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::GrantProtectionChosenColor(_)
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. } => {}
        }
    }
}

pub(super) fn apply(change: CreatureTypeTextChange, object: &mut ObjectCharacteristics) {
    words(change, &mut object.current_creature_subtypes);
    printed(change, &mut object.printed_text);
}
/// Apply the same semantic text transformation to a literal creation template.
pub(crate) fn rewrite_program(
    change: CreatureTypeTextChange,
    program: &mut crate::program::Program,
) {
    words(change, &mut program.creature_subtypes);
    let mut text = PrintedText::from_program(program);
    printed(change, &mut text);
    program.entry_counter_abilities = text.entry_counter_abilities;
    program.conditional_cost = text.conditional_cost;
    program.additional_cast_costs = text.additional_cast_costs;
    program.graveyard_self_cast = text.graveyard_self_cast;
    program.cast_reductions = text.cast_reductions;
    program.extra_activation_costs = text.extra_activation_costs;
    program.activation_cost_reductions = text.activation_cost_reductions;
    program.activation_target_cost_reductions = text.activation_target_cost_reductions;
    program.extra_cast_costs = text.extra_cast_costs;
    program.zone_replacements = text.zone_replacements;
    program.zone_replacement_extras = text.zone_replacement_extras;
    program.mana = text.mana;
    program.activated = text.activated;
    program.triggered = text.triggered;
    program.continuous = text.continuous;
    program.additional_battlefield_abilities = text.additional_battlefield_abilities;
    if let Some(crate::program::EntryCopyRule::MatchingExt(spec)) = &mut program.entry_copy {
        spec.filter.rewrite_creature_type(change.from, change.to);
        words(change, &mut spec.extra_creature_subtypes);
        for (cond, _, _) in &mut spec.conditional_counters {
            cond.rewrite_creature_type(change.from, change.to);
        }
        for ability in &mut spec.extra_activated {
            body(change, &mut ability.body);
        }
        for ability in &mut spec.extra_triggered {
            trigger(change, ability);
        }
        for ability in &mut spec.extra_static {
            rewrite_static_ability(change, ability);
        }
        for mana in &mut spec.extra_mana {
            rewrite_mana_output(change, &mut mana.output);
        }
    }
    if let crate::program::ProgramKind::Spell(value) = &mut program.kind {
        body(change, &mut value.body);
    }
}
fn trigger(change: CreatureTypeTextChange, ability: &mut crate::program::TriggeredAbility) {
    ability
        .condition
        .rewrite_creature_type(change.from, change.to);
    body(change, &mut ability.body);
}
fn rewrite_mana_output(change: CreatureTypeTextChange, output: &mut ManaOutput) {
    match output {
        ManaOutput::Counted { permanents, .. } => selector(change, permanents),
        ManaOutput::Modified { modifier, output } => {
            match modifier {
                crate::mana::ManaOutputModifier::Sacrifice(
                    crate::mana::ManaSacrificeCost::ControlledSubtype(subtype),
                ) => word(change, subtype),
                crate::mana::ManaOutputModifier::RequirePermanents {
                    selector: quality, ..
                } => {
                    selector(change, quality);
                }
                _ => {}
            }
            rewrite_mana_output(change, output);
        }
        ManaOutput::Fixed(_)
        | ManaOutput::ChooseColor { .. }
        | ManaOutput::Triggered(_)
        | ManaOutput::TwoColors { .. }
        | ManaOutput::CounterBased(_)
        | ManaOutput::SpecialColor(_)
        | ManaOutput::Dynamic(_) => {}
    }
}
fn printed(change: CreatureTypeTextChange, text: &mut PrintedText) {
    if let Some(
        crate::program::ConditionalCastCost::Overload {
            each: unbracketed, ..
        }
        | crate::program::ConditionalCastCost::Cleave { unbracketed, .. },
    ) = &mut text.conditional_cost
    {
        body(change, unbracketed);
    }
    if let Some(crate::program::ConditionalCastCost::Condition { condition, .. }) =
        &mut text.conditional_cost
    {
        condition.left.rewrite_creature_type(change.from, change.to);
        condition
            .right
            .rewrite_creature_type(change.from, change.to);
    }
    if let Some(crate::program::ConditionalCastCost::Alternative {
        condition, extra, ..
    }) = &mut text.conditional_cost
    {
        if let Some(cond) = condition {
            cond.rewrite_creature_type(change.from, change.to);
        }
        for cost in extra {
            cost.rewrite_creature_type(change.from, change.to);
        }
    }
    for cost in &mut text.additional_cast_costs {
        if let crate::program::AdditionalCastCost::KickerExtra { extra, .. } = cost {
            for item in extra {
                item.rewrite_creature_type(change.from, change.to);
            }
        }
    }
    match &mut text.graveyard_self_cast {
        Some(crate::program::GraveyardSelfCast::WhileControllingSubtype(kind)) => {
            word(change, kind);
        }
        Some(
            crate::program::GraveyardSelfCast::WithExtraCost(costs)
            | crate::program::GraveyardSelfCast::WithAlternativeCost { extra: costs, .. },
        ) => {
            for cost in costs {
                cost.rewrite_creature_type(change.from, change.to);
            }
        }
        _ => {}
    }
    for reduction in &mut text.cast_reductions {
        match reduction {
            crate::program::CastReduction::Affinity(quality) => {
                selector(change, quality);
            }
            crate::program::CastReduction::CountZone { filter, .. }
            | crate::program::CastReduction::IfTargets { filter, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            crate::program::CastReduction::Conditional { condition, .. } => {
                condition.left.rewrite_creature_type(change.from, change.to);
                condition
                    .right
                    .rewrite_creature_type(change.from, change.to);
            }
            crate::program::CastReduction::Dynamic(expr) => {
                expr.rewrite_creature_type(change.from, change.to);
            }
            _ => {}
        }
    }
    for ability in text
        .continuous
        .iter_mut()
        .chain(&mut text.additional_battlefield_abilities)
    {
        ability.rewrite_creature_type_in_rule(change);
    }
    for ability in &mut text.mana {
        rewrite_mana_output(change, &mut ability.output);
    }
    for ability in &mut text.activated {
        match &mut ability.source_cost {
            crate::activated::SourceCost::TapPermanents {
                filter: crate::activated::TapCostFilter::Subtype(kind),
                ..
            }
            | crate::activated::SourceCost::SacrificeSelected {
                filter: crate::activated::SelectedSacrificeFilter::Subtype(kind),
                ..
            }
            | crate::activated::SourceCost::PayLife {
                sacrifice: Some(crate::activated::SelectedSacrificeFilter::Subtype(kind)),
                ..
            } => {
                word(change, kind);
            }
            crate::activated::SourceCost::TapPermanents {
                filter: crate::activated::TapCostFilter::Matching(filter),
                ..
            }
            | crate::activated::SourceCost::SacrificeSelected {
                filter: crate::activated::SelectedSacrificeFilter::Matching(filter),
                ..
            }
            | crate::activated::SourceCost::PayLife {
                sacrifice: Some(crate::activated::SelectedSacrificeFilter::Matching(filter)),
                ..
            }
            | crate::activated::SourceCost::Discard {
                mode: crate::activated::DiscardCostMode::Filtered { filter, .. },
                ..
            }
            | crate::activated::SourceCost::GraveyardDiscard {
                mode: crate::activated::DiscardCostMode::Filtered { filter, .. },
            }
            | crate::activated::SourceCost::ExileGraveyard {
                filter: crate::activated::GraveyardExileCost::Filtered { filter, .. },
                ..
            }
            | crate::activated::SourceCost::GraveyardExileOther {
                filter: crate::activated::GraveyardExileCost::Filtered { filter, .. },
            } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            _ => {}
        }
        body(change, &mut ability.body);
    }
    for costs in text.extra_activation_costs.values_mut() {
        for cost in costs {
            cost.rewrite_creature_type(change.from, change.to);
        }
    }
    for expr in text.activation_cost_reductions.values_mut() {
        expr.rewrite_creature_type(change.from, change.to);
    }
    for (filter, _) in text.activation_target_cost_reductions.values_mut() {
        filter.rewrite_creature_type(change.from, change.to);
    }
    for cost in &mut text.extra_cast_costs {
        cost.rewrite_creature_type(change.from, change.to);
    }
    for ability in &mut text.triggered {
        trigger(change, ability);
    }
    for replacement in &mut text.zone_replacements {
        if let crate::program::ZoneReplacementSubject::Matching { filter, .. }
        | crate::program::ZoneReplacementSubject::MatchingNotCast { filter, .. } =
            &mut replacement.subject
        {
            filter.rewrite_creature_type(change.from, change.to);
        }
    }
    for extra in text.zone_replacement_extras.values_mut() {
        if let Some(followup) = &mut extra.followup_body {
            body(change, followup);
        }
    }
    for ability in text
        .continuous
        .iter_mut()
        .chain(&mut text.additional_battlefield_abilities)
    {
        let StaticContinuousAbility::Battlefield {
            selector: target,
            parts,
            ..
        } = ability
        else {
            continue;
        };
        selector(change, target);
        rewrite_battlefield_static_parts(change, parts);
    }
    for cond in text.static_conditions.values_mut() {
        static_source_condition(change, cond);
    }
    if let Some(StaticContinuousAbility::CharacteristicDefining { parts, .. }) =
        &mut text.continuous
    {
        for part in &mut parts.0 {
            match part {
                DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(values)) => {
                    words(change, values)
                }
                DefiningPart::PowerToughness { power, toughness } => {
                    for expression in power.iter_mut().chain(toughness) {
                        numeric(change, expression);
                    }
                }
                DefiningPart::CreatureTypes(DefiningCreatureTypes::All)
                | DefiningPart::CommanderChosenColor(_)
                | DefiningPart::Colors(_) => {}
            }
        }
    }
}
