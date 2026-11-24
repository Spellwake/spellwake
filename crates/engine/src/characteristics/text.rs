//! Semantic creature-type substitutions in printed definitions, never raw text.
use super::{StaticContinuousAbility, types::*};
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
fn static_source_condition(change: CreatureTypeTextChange, cond: &mut StaticSourceCondition) {
    match cond {
        StaticSourceCondition::ControllerControlsSubtype(kind)
        | StaticSourceCondition::OpponentControlsSubtype(kind) => word(change, kind),
        StaticSourceCondition::ControlsMatching { selector: sel, .. }
        | StaticSourceCondition::OpponentControlsMatching { selector: sel, .. } => {
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
    }
}
fn parts(change: CreatureTypeTextChange, value: &mut ResolvedPartsSpec) {
    for part in &mut value.0 {
        match part {
            ResolvedPartSpec::Abilities(crate::characteristics::AbilityChange {
                granted_trigger: Some(ability),
                ..
            }) => trigger(change, ability),
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
            | ResolvedPartSpec::GrantActivatedRestricted { ability, .. } => {
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
            | ResolvedPartSpec::Abilities(crate::characteristics::AbilityChange {
                granted_trigger: None,
                ..
            })
            | ResolvedPartSpec::SetPT(_)
            | ResolvedPartSpec::ModifyPT(_)
            | ResolvedPartSpec::SwitchPT => {}
        }
    }
}
pub(super) fn rewrite_executable_body(value: &mut ExecutableBody, change: CreatureTypeTextChange) {
    body(change, value);
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
            Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { subtype, .. }
            | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { subtype, .. } => {
                word(change, subtype)
            }
            Instruction::ReturnGraveyardCohort(returning) => {
                if let crate::program::GraveyardReturnFilter::CreatureSubtype(kind) =
                    &mut returning.filter
                {
                    word(change, kind);
                }
                if let Some(kind) = &mut returning.additions.creature_subtype {
                    word(change, kind);
                }
            }
            Instruction::AddMana(yield_kind) => {
                let inner = match yield_kind {
                    crate::program::EffectManaYield::RetainedUntilCleanup(inner) => inner.as_mut(),
                    other => other,
                };
                if let crate::program::EffectManaYield::Counted { amount, .. }
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
            | Instruction::DrawTargetPlayer { amount }
            | Instruction::DrawController { amount }
            | Instruction::DrawAndDamageOpponents { amount }
            | Instruction::DrawControllerAndTarget { amount }
            | Instruction::DrawEachPlayer { amount }
            | Instruction::OfferDynamicManaPayment {
                extra_generic: amount,
                ..
            }
            | Instruction::PayLife { amount, .. }
            | Instruction::PayEnergy { amount, .. }
            | Instruction::ExileFromZone { amount, .. }
            | Instruction::TapOrUntapControlled { amount, .. }
            | Instruction::CollectEvidence {
                minimum: amount, ..
            }
            | Instruction::Blight { amount, .. }
            | Instruction::RemoveCountersFromPermanent { amount, .. }
            | Instruction::DamageOpponentsAmount(amount)
            | Instruction::DamageRelatedPlayerAmount(amount)
            | Instruction::DamageGroupAmount { amount, .. }
            | Instruction::OpponentsLoseAndControllerGains(amount)
            | Instruction::ExileFromHandOrBattlefield(amount)
            | Instruction::ReturnControlledPermanents { amount, .. }
            | Instruction::ExileChosenPermanents { amount, .. }
            | Instruction::Discard { amount, .. }
            | Instruction::DiscardFiltered { amount, .. }
            | Instruction::AddPlayerCounter { amount, .. }
            | Instruction::PutNamedCounterOnTarget { amount, .. }
            | Instruction::PutCountersOnSubject { amount, .. }
            | Instruction::AdjustPlayersLife { amount, .. }
            | Instruction::ExileTopPlay { count: amount, .. }
            | Instruction::LookTopChoose { count: amount, .. }
            | Instruction::RepeatLoop { count: amount, .. } => rewrite_amount(amount),
            Instruction::SelectZoneCardsToZone { min, max, .. } => {
                rewrite_amount(min);
                rewrite_amount(max);
            }
            Instruction::Sacrifice { amount, .. }
            | Instruction::ModifyGroupByAmount { amount, .. }
            | Instruction::AnimateTargetAsCountedCreature { amount, .. }
            | Instruction::DrainOpponents(amount)
            | Instruction::DamageAmount(amount)
            | Instruction::DamageAmountThenExileIfDies(amount)
            | Instruction::DamageCreatureAtSpellMastery { amount, .. } => rewrite_amount(amount),
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations } => {
                for spec in creations {
                    rewrite_amount(&mut spec.amount);
                }
            }
            Instruction::TemptWithTokens { creation, .. } => rewrite_amount(&mut creation.amount),
            _ => {}
        }
        match instruction {
            Instruction::DiscardFiltered { filter, .. }
            | Instruction::ExileFromZone { filter, .. }
            | Instruction::TapOrUntapControlled { filter, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            Instruction::Sacrifice { filter, .. }
            | Instruction::ReturnControlledPermanents { filter, .. }
            | Instruction::ExileChosenPermanents { filter, .. }
            | Instruction::SacrificeToContinue { filter } => {
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
            Instruction::SelectZoneCardsToZone { filter, .. }
            | Instruction::RevealUntilMatch { filter, .. }
            | Instruction::MoveGraveyardGroup { filter, .. }
            | Instruction::PutHandPermanentFiltered { filter, .. } => {
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
            Instruction::DealDamageFrom {
                source,
                recipients,
                amount,
            } => {
                if let crate::program::DamageSourceRef::Matching(f) = source.as_mut() {
                    f.rewrite_creature_type(change.from, change.to);
                }
                if let crate::program::DamageRecipientsRef::Group(
                    crate::program::DamageGroup::Filtered(f)
                    | crate::program::DamageGroup::FilteredAndPlayers { filter: f, .. },
                ) = recipients.as_mut()
                {
                    f.rewrite_creature_type(change.from, change.to);
                }
                if let crate::program::DamageAmountSpec::Expr(expr) = amount.as_mut() {
                    expr.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::SkipNextUntap { subject }
            | Instruction::RequireCombatAction { subject, .. }
            | Instruction::DestroySubject { subject, .. } => {
                subject.rewrite_creature_type(change.from, change.to);
            }
            Instruction::CreateTemporaryRule { rule, .. } => match rule.as_mut() {
                crate::program::TemporaryRuleKind::CantBeCountered {
                    filter: Some(f), ..
                }
                | crate::program::TemporaryRuleKind::CantCastSpells {
                    filter: Some(f), ..
                }
                | crate::program::TemporaryRuleKind::SpellCostReduction { filter: f, .. }
                | crate::program::TemporaryRuleKind::SpellCostIncrease { filter: f, .. }
                | crate::program::TemporaryRuleKind::FilteredFlash { filter: f, .. }
                | crate::program::TemporaryRuleKind::ZoneCastPermission {
                    filter: Some(f), ..
                } => {
                    f.rewrite_creature_type(change.from, change.to);
                }
                crate::program::TemporaryRuleKind::SpellCostReductionExpr {
                    filter,
                    amount,
                    ..
                } => {
                    filter.rewrite_creature_type(change.from, change.to);
                    amount.rewrite_creature_type(change.from, change.to);
                }
                crate::program::TemporaryRuleKind::ExileIfDies { subject } => {
                    subject.rewrite_creature_type(change.from, change.to);
                }
                _ => {}
            },
            Instruction::GrantCardPlayPermission { .. } => {}
            Instruction::CreateTokenCopy {
                count, add_subtype, ..
            } => {
                count.rewrite_creature_type(change.from, change.to);
                if let Some(sub) = add_subtype
                    && *sub == change.from
                {
                    *sub = change.to;
                }
            }
            Instruction::CreateAuraTokenAttached { definition, .. } => {
                definition.rewrite_creature_types(change);
            }
            Instruction::ScheduleDelayedEventTrigger { body: inner, .. } => {
                body(change, inner);
            }
            Instruction::DivideDamageAmongTargets { .. }
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::GiveFixedTargetControl { .. }
            | Instruction::WinGame(_)
            | Instruction::LoseGame(_)
            | Instruction::AttachPermanent { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::MoveSubject { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::ChooseValue { .. }
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
            | Instruction::CopyStackObject { count: amount, .. } => {
                amount.rewrite_creature_type(change.from, change.to);
            }
            Instruction::ExileGroupFromZone { filter, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            Instruction::DestroyChosen { filter, count, .. } => {
                filter.rewrite_creature_type(change.from, change.to);
                count.rewrite_creature_type(change.from, change.to);
            }
            Instruction::SacrificeSubject { subject } => {
                subject.rewrite_creature_type(change.from, change.to);
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
            Instruction::GainControlOfPermanent { subject, .. }
            | Instruction::DoublePowerToughnessSubject { subject, .. }
            | Instruction::DoubleCountersOnSubject { subject, .. }
            | Instruction::ShuffleSubjectIntoLibrary { subject, .. } => {
                if let crate::program::EffectSubject::Matching(group) = subject.as_mut() {
                    group.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::BecomeCopy {
                receiver,
                exceptions,
                ..
            } => {
                if let crate::program::EffectSubject::Matching(group) = receiver.as_mut() {
                    group.rewrite_creature_type(change.from, change.to);
                }
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
            Instruction::MoveGroup { group, .. }
            | Instruction::DestroyGroup(group)
            | Instruction::DestroyGroupWithoutRegeneration(group)
            | Instruction::PutGroupOnLibraryBottom(group)
            | Instruction::SetGroupTapped { filter: group, .. } => {
                group.rewrite_creature_type(change.from, change.to);
            }
            Instruction::PutCountersOnSubject { subject, .. } => {
                if let crate::program::EffectSubject::Matching(group) = subject.as_mut() {
                    group.rewrite_creature_type(change.from, change.to);
                }
            }
            Instruction::BranchIfFalse { condition, .. } => {
                condition.left.rewrite_creature_type(change.from, change.to);
                condition
                    .right
                    .rewrite_creature_type(change.from, change.to);
            }
            Instruction::FixedTargetClause(clause) => body(change, clause),
            Instruction::ReturnGraveyardCohort(_)
            | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
            | Instruction::DamageTargetControllersCreatures { .. }
            | Instruction::DamageCapturedBlocker(_)
            | Instruction::GiveControlOfAbilitySource
            | Instruction::OfferOpponentsDamage
            | Instruction::WeakenTargetByChosenX => {}
            Instruction::BoostTargetPower { amount, .. } => rewrite_amount(amount),
            Instruction::AnimateTargetAsCountedCreature { subtype, .. }
            | Instruction::LoseLifeAndScryForSubtype(subtype)
            | Instruction::ExileForBlackToken { subtype, .. }
            | Instruction::ExileForSubtypeToken(subtype)
            | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(subtype) => word(change, subtype),
            Instruction::CreateTokenAndAttach { definition } => {
                definition.rewrite_creature_types(change)
            }
            Instruction::CreateTokensToFight(definition) => {
                definition.rewrite_creature_types(change)
            }
            Instruction::TemptWithTokens { creation, .. } => {
                creation.definition.rewrite_creature_types(change)
            }
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations } => {
                for creation in creations {
                    if let Some(crate::token::TokenSize::AtCreation(size)) = &mut creation.base_size
                    {
                        rewrite_amount(size);
                    }
                    creation.definition.rewrite_creature_types(change);
                }
            }
            Instruction::ChooseCreatureType { excluding, .. } => words(change, excluding),
            Instruction::OrderArtifactCreatureLandSacrifices
            | Instruction::ChooseTypeThenDestroyUnchosen
            | Instruction::ChooseCreatureTypeForPlayer { .. } => {}
            Instruction::SearchLibrary { filter } => match &mut filter.matcher {
                crate::program::SearchCardMatcher::CreatureSubtype(kind) => {
                    word(change, kind);
                }
                crate::program::SearchCardMatcher::Filtered(obj_filter) => {
                    obj_filter.rewrite_creature_type(change.from, change.to);
                }
                _ => {}
            },
            Instruction::DamageGroup {
                group:
                    crate::program::DamageGroup::Filtered(filter)
                    | crate::program::DamageGroup::FilteredAndPlayers { filter, .. },
                ..
            }
            | Instruction::DamageGroupAmount {
                group:
                    crate::program::DamageGroup::Filtered(filter)
                    | crate::program::DamageGroup::FilteredAndPlayers { filter, .. },
                ..
            } => {
                filter.rewrite_creature_type(change.from, change.to);
            }
            Instruction::ModifyGroupByAmount { group, .. }
            | Instruction::ModifyGroupUntilEndOfTurn { group, .. }
            | Instruction::SuppressGroupKeywords { group, .. } => {
                group.rewrite_creature_type(change.from, change.to);
            }
            Instruction::PreventDamageThisTurn(
                crate::damage::TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(
                    first,
                    second,
                ),
            ) => {
                word(change, first);
                word(change, second);
            }
            Instruction::TransformSubtype(kind)
            | Instruction::RegenerateControlledSubtype(kind)
            | Instruction::PreventDamageThisTurn(
                crate::damage::TurnPreventionFilter::CombatFromCreaturesWithoutSubtype(kind),
            )
            | Instruction::ControlledSubtypeDamagesAttacker(kind)
            | Instruction::CreateArmyIfAbsent { subtype: kind, .. }
            | Instruction::Amass { subtype: kind, .. } => word(change, kind),
            Instruction::ChangeGroup {
                group,
                parts: value,
                ..
            } => {
                if let crate::program::EffectSubject::Matching(filter) = group.as_mut() {
                    filter.rewrite_creature_type(change.from, change.to);
                }
                parts(change, value);
            }
            Instruction::ChangeControlledPlaneswalkers { parts: value, .. }
            | Instruction::CreateContinuousEffect { parts: value, .. }
            | Instruction::ChangeAbilitySource { parts: value, .. } => parts(change, value),
            // These admitted instructions contain no literal creature-type words.
            // Draw's chosen-type reference remains a runtime local value.
            Instruction::AttachOnEntry { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::SetControlledCreatureSizeToHand
            | Instruction::FightPairedCreatures(_)
            | Instruction::OfferManaPayment { .. }
            | Instruction::ModifyAnyNumberOfPermanents { .. }
            | Instruction::AffectPermanentTargets { .. }
            | Instruction::MillEachPlayer(_)
            | Instruction::PutLibraryCohortIntoGraveyard(_)
            | Instruction::ExileOnePerPlayerUntilSourceLeaves
            | Instruction::AttachControlledToSource { .. }
            | Instruction::AttachBattlefieldAurasToSource
            | Instruction::EnterHandOrGraveyardAurasAttachedToSource
            | Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::GiveEquippedCreatureToUpkeepPlayer
            | Instruction::CopyCreatureExiledForAbility
            | Instruction::DamageFromGrantingPermanent { .. }
            | Instruction::CopyEntrantAsArtifact
            | Instruction::ReplaceSourceCreatedTokens
            | Instruction::CopyEntrantOnceEachTurn
            | Instruction::CopyCreatureAsToken
            | Instruction::CopyDifferentlyNamedTokens
            | Instruction::CopyOtherControlledNontokensAsEntryHost
            | Instruction::Bolster(_)
            | Instruction::UntapUpTo { .. }
            | Instruction::ChooseVowedCreatures
            | Instruction::ChooseCreaturesToKeep
            | Instruction::DestroyOtherCreatures
            | Instruction::SacrificeOtherCreatures
            | Instruction::SacrificeColoredPermanents
            | Instruction::ExileFromHandOrBattlefield(_)
            | Instruction::ExileRelatedHandCardForPlay
            | Instruction::PayLifeOrSacrificeCreatures
            | Instruction::ChooseOtherControlledCreature { .. }
            | Instruction::DiscardOneAtRandom(_)
            | Instruction::SacrificeRandomUntilLand
            | Instruction::DiscardUpTo { .. }
            | Instruction::SacrificeMatching(_)
            | Instruction::Discard { .. }
            | Instruction::Jump { .. }
            | Instruction::GainControllerLife { .. }
            | Instruction::DrainTargetAndGain(_)
            | Instruction::AdjustControllerLife { .. }
            | Instruction::ExchangeLifeWithSourceToughness
            | Instruction::AdjustTargetLife { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::DamageAmount(_)
            | Instruction::DamageAmountThenExileIfDies(_)
            | Instruction::DamageCreatureAtSpellMastery { .. }
            | Instruction::Damage { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::ControllerLosesRememberedManaValue
            | Instruction::ExiledTargetControllerGainsPower
            | Instruction::GoadOtherPlayersCreatures
            | Instruction::AdditionalCombat { .. }
            | Instruction::AdditionalCombatAfterUntappingAttackers
            | Instruction::TakeExtraTurn
            | Instruction::DisallowDamagePreventionThisTurn
            | Instruction::GrantNoMaxHandSizeRestOfGame
            | Instruction::PreventDamageThisTurn(
                crate::damage::TurnPreventionFilter::ToAbilitySource
                | crate::damage::TurnPreventionFilter::ToTarget
                | crate::damage::TurnPreventionFilter::CombatToOrFromAbilitySource
                | crate::damage::TurnPreventionFilter::CombatFromCreaturesOtherThanTarget
                | crate::damage::TurnPreventionFilter::CombatToControlledCreatures
                | crate::damage::TurnPreventionFilter::Combat
                | crate::damage::TurnPreventionFilter::All
                | crate::damage::TurnPreventionFilter::CreaturesControlledByOpponents,
            )
            | Instruction::ExileDefeatedSiege
            | Instruction::ExileSourceAndGainLife(_)
            | Instruction::ExileAbilitySource
            | Instruction::ReturnSourceToHand
            | Instruction::PutCounterOnRelatedPermanent
            | Instruction::PutCountersOnControlledCreatures
            | Instruction::PutCountersOnControlledSubtype(_)
            | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
            | Instruction::PutCountersOnOtherCreatures
            | Instruction::PutCreatureAndOtherPlaneswalkerCounters
            | Instruction::PutChargeCounterOnSource
            | Instruction::PutBlessingCounterOnSource
            | Instruction::PutNamedCounterOnSource { .. }
            | Instruction::PutQuestCounterOnSource
            | Instruction::PutStorageCounterOnSource
            | Instruction::AbilitySourceControllerLosesLife(_)
            | Instruction::RelatedPlayerLosesLife(_)
            | Instruction::LinkedPlayerLosesDamageAmount
            | Instruction::RelatedPlayerLosesHalfLife
            | Instruction::FightSourceWithTarget
            | Instruction::ClearSourceCounters(_)
            | Instruction::TransformSource
            | Instruction::TransformSourceAndAttachToTarget
            | Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource
            | Instruction::UntapAbilitySource
            | Instruction::TapAbilitySource
            | Instruction::RegenerateSource
            | Instruction::RegenerateTarget
            | Instruction::DoubleCountersOnTargets
            | Instruction::ExileCombatDamagedCreature
            | Instruction::DestroyCapturedDamageSource
            | Instruction::RemoveCreatureCounters { .. }
            | Instruction::UntapControlledLands
            | Instruction::UntapControlledCreatures
            | Instruction::TapAttackersAndSkipNextUntap
            | Instruction::UntapAndControlCreatures
            | Instruction::Proliferate
            | Instruction::DamagePlayersHalfLife
            | Instruction::DamageGroup { .. }
            | Instruction::DrawAndDamageOpponents { .. }
            | Instruction::RepeatOpponentLifeLoss
            | Instruction::OpponentsLoseAndControllerGains(_)
            | Instruction::AdjustEachOpponentLife { .. }
            | Instruction::AdjustPlayersLife { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateReflectingShield
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::OfferSourceCounters { .. }
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::ReturnGraveyardTargetsTapped(_)
            | Instruction::ReturnCapturedDyingCreature
            | Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DistributePlusOneCounters { .. }
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::ExileUntilOtherTargetLeaves
            | Instruction::CopyOtherTargetAsArtifact
            | Instruction::ExileResolvingSpell
            | Instruction::ReturnResolvingSpellToHand
            | Instruction::Ascend
            | Instruction::DestroyFixedTargets
            | Instruction::DestroyTargetAndAttachedEquipment
            | Instruction::ReturnFixedTargets
            | Instruction::FightFixedTargets
            | Instruction::BindOrderedPermanents { .. }
            | Instruction::ExileOrderedPermanent(_)
            | Instruction::ExileLibraryTop { .. }
            | Instruction::PutTargetOwnedExiledLandsOntoBattlefield
            | Instruction::LookOpponentExileForCast { .. }
            | Instruction::ExileFromTargetLibrary
            | Instruction::OfferVariablePayment { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayEnergy { .. }
            | Instruction::ExertAbilitySource
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
            | Instruction::DrawEachPlayerForPayments
            | Instruction::CreateReflexiveGraveyardReturn
            | Instruction::ReturnGraveyardCreatureWithCorpse(_)
            | Instruction::ReturnRandomLinkedExile
            | Instruction::ReturnLinkedSearchCards
            | Instruction::PutHandPermanentsOntoBattlefield { .. }
            | Instruction::RevealSharedTypeCreatureCastOrBottom
            | Instruction::RevealOddCastOrDraw
            | Instruction::MayTurnControlledCreatureFaceUp
            | Instruction::ExploreSource
            | Instruction::MayRevealLibraryTop
            | Instruction::ManifestTop { .. }
            | Instruction::ExileNonlandsToHandAndDamage { .. }
            | Instruction::RevealTopHandLoseManaValue { .. }
            | Instruction::RevealUntilLandKind
            | Instruction::RevealOpponentChoosesCreature
            | Instruction::ShuffleAbilitySourceIntoLibrary
            | Instruction::RevealToNameAfterSourceShuffle(_)
            | Instruction::WatchSmallCreatureCombatDraw(_)
            | Instruction::WatchTargetCombatDamageDraw
            | Instruction::ChooseOwnCommanderToCast
            | Instruction::ChooseMatchingHandSpellToCast
            | Instruction::SearchEachLibraryForFreeCast
            | Instruction::ChooseFoundSpell
            | Instruction::CastFoundSpell(_)
            | Instruction::LookTopOneRestGraveyard { .. }
            | Instruction::RevealTopHandRestGraveyard { .. }
            | Instruction::LookTopOneRestBottom { .. }
            | Instruction::LookTopCreatureSharingAttachedSubtype { .. }
            | Instruction::RevealTopCreaturesAndLandsRestBottom { .. }
            | Instruction::RevealHandAndBottomPiles
            | Instruction::RevealDividePiles
            | Instruction::DivideTargetPermanentsAndSacrificePile
            | Instruction::DrawToChosenOpponentHandSize
            | Instruction::LookTopPlaneswalkers
            | Instruction::RevealTopPermanentOrHand
            | Instruction::LookTopLandBattlefieldTapped
            | Instruction::LookTopReveal { .. }
            | Instruction::LookTopTakeOne { .. }
            | Instruction::IncreaseSpeed
            | Instruction::BecomeMonarch(_)
            | Instruction::TapEnchantedCreature
            | Instruction::RequireCondition(_)
            | Instruction::PayControllerLife(_)
            | Instruction::PutEquipmentFromHandOrGraveyard
            | Instruction::EquipNewToken
            | Instruction::Encore
            | Instruction::GrantNewTokensHaste
            | Instruction::RequireAttackSourceNextTurn
            | Instruction::ChooseAttackDirection
            | Instruction::ReselectAttackDefender
            | Instruction::SetBlockDeclarations(_)
            | Instruction::AdditionalLoyaltyActivation
            | Instruction::MoveLoyaltyCounterToEnteringPermanent
            | Instruction::CreateEmblem(_)
            | Instruction::CreateOpponentEmblems(_)
            | Instruction::ImpulseCastOrDamage(_)
            | Instruction::RevealCreatureOfSacrificedType
            | Instruction::SacrificeCombatContributor
            | Instruction::ExileNonlandForFormerControllers
            | Instruction::ExileMilledCreatures { .. }
            | Instruction::VoteForGraveyardCards { .. }
            | Instruction::ShuffleOnePermanentPerPlayer
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::ReturnCardsDestroyedThisWay
            | Instruction::RevealCreatureForEachTargetController
            | Instruction::ReturnOddGraveyardPermanentIfPaid { .. }
            | Instruction::PutGraveyardCreatureAtMostX
            | Instruction::PutLandFromExiledCohort
            | Instruction::ExileTopForEachSpellType(_)
            | Instruction::ExileTopFreeNoncreatureSpells(_)
            | Instruction::ExileRelatedTopForCastThisTurn
            | Instruction::ExileRelatedTopForPlay
            | Instruction::ExileTopForPlay { .. }
            | Instruction::ExileTopFaceDown
            | Instruction::ScheduleCreatedTokenExile(_)
            | Instruction::ScheduleCreatedTokenSacrifice
            | Instruction::ExileDelayedObjects(_)
            | Instruction::ExileRelatedPermanents(_)
            | Instruction::ExileTargetWithLink(_)
            | Instruction::ReturnCardsExiledWithLink(_)
            | Instruction::ChooseAndReturnOwnGraveyardCardsByType
            | Instruction::ReturnOwnGraveyardCardsOfTypeToHand(_)
            | Instruction::TargetDamagesItsControllersOtherCreatures
            | Instruction::OtherCreaturesReturnPowerDamage
            | Instruction::DestroyCapturedCombatCreature { .. }
            | Instruction::ReflectAttachedCreatureDamage
            | Instruction::DestroyAttachedEnchantmentSubtype(_)
            | Instruction::GrantFlashback(_)
            | Instruction::TapTargetWhileControllingSource
            | Instruction::RegisterTokenLinkedExile
            | Instruction::ReturnTokenLinkedExile
            | Instruction::AddMana(_)
            | Instruction::AbandonSourceScheme
            | Instruction::TabletopHonorAction { .. }
            | Instruction::CreateNonlegendaryLoyaltyCopy
            | Instruction::WeakenOpposingAttackersUntilNextTurn
            | Instruction::PhaseOtherPlaneswalkersNextEndStep
            | Instruction::ChooseMeldPartner(_)
            | Instruction::ExileMeldPair
            | Instruction::MeldExiledPair(_)
            | Instruction::PairWithCreature { .. }
            | Instruction::PhaseOut(_)
            | Instruction::PhaseOutObjects(_)
            | Instruction::ReturnOneNonlandPerOpponent
            | Instruction::WeakenDelayedAttacker(_)
            | Instruction::DrawRevealRemoveLoyalty
            | Instruction::CreateReflexiveDiscardDamage
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::RemoveLoyaltyExileTopForPlay
            | Instruction::DamageDamagedPlayersCreatures
            | Instruction::DamageOpponentsAmount(_)
            | Instruction::DamageRelatedPlayerAmount(_)
            | Instruction::DamageGroupAmount { .. }
            | Instruction::DoublePowerToughness { .. }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::FlipCoin { .. }
            | Instruction::Cascade
            | Instruction::CascadeBottom
            | Instruction::Myriad
            | Instruction::CopyForOtherCreatures
            | Instruction::CopyResolvingSpell
            | Instruction::WatchNextSpellOrLoyaltyTwice
            | Instruction::WatchNextLoyaltyCopy
            | Instruction::ChoosePlaneswalkerTypeForCopies
            | Instruction::CopyCapturedAbility
            | Instruction::CopyOnCast(_)
            | Instruction::MadnessCast
            | Instruction::MiracleCast(_)
            | Instruction::CastDefeatedSiege
            | Instruction::CastTargetGraveyardInstant
            | Instruction::ReboundCast(_)
            | Instruction::EndStepCast { .. }
            | Instruction::SacrificeDelayedObjects(_)
            | Instruction::ScheduleArtifactReturn(_)
            | Instruction::ScheduleGraveyardReturnAtEndStep(_)
            | Instruction::ReturnSacrificedCardToBattlefield
            | Instruction::ReturnDyingCreatureAsForest
            | Instruction::ReturnDyingSourceTapped
            | Instruction::ReturnDelayedGraveyardCard(_)
            | Instruction::ReturnDelayedGraveyardCardTransformed(_)
            | Instruction::ReturnDelayedPermanentToHand(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::ResolveTransfer(_)
            | Instruction::DrainOpponents(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::SacrificeUpkeepSource(_)
            | Instruction::CounterWardSource
            | Instruction::BoostSourceByCastManaValue
            | Instruction::BoostLoneAttacker
            | Instruction::BoostSaddledAttacker { .. }
            | Instruction::PutTargetInLibrary(_)
            | Instruction::PutOwnHandCardOnLibraryBottom
            | Instruction::ExileGraveyardTarget
            | Instruction::ExileCapturedGraveyardCard
            | Instruction::GrantGraveyardCastAndWatchAbilities
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ExileCardsFromGraveyard { .. }
            | Instruction::ReturnUpToOneRecentGraveyardCard
            | Instruction::ExileUpToTwoGraveyardTargetsGainLife
            | Instruction::ReturnTargetGraveyardCreatureToHand
            | Instruction::ReturnRandomGraveyardCards { .. }
            | Instruction::ChooseRevealedHandCard { .. }
            | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
            | Instruction::ReturnSourceGraveyardToBattlefield { .. }
            | Instruction::ReturnSourceGraveyardToHand
            | Instruction::ReturnOwnedCommandersToHand
            | Instruction::ExileTopForFreePlay
            | Instruction::ExileTargetUntilNonlandForFreeCast
            | Instruction::ExileTopRepeatOnPermanentEntry
            | Instruction::ShuffleEachHandAndGraveyard
            | Instruction::EndControllerTurn
            | Instruction::ShuffleOwnLibrary
            | Instruction::ShufflePlayerLibrary(_)
            | Instruction::ShuffleTargetIntoLibrary
            | Instruction::RevealPlayerHand(_)
            | Instruction::ShuffleOwnGraveyardIntoLibrary
            | Instruction::ShuffleTargetGraveyardCards { .. }
            | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
            | Instruction::CastLinkedSpellMatchingX
            | Instruction::DestroyDamagedPlayersNonlandsAtX
            | Instruction::PutChosenXChargeCounters
            | Instruction::DestroyNonlandsAtSourceChargeCount
            | Instruction::AddCounterAndDischargeOnResolution(_)
            | Instruction::ExileAllGraveyards
            | Instruction::ExilePlayerGraveyards(_)
            | Instruction::PayControllerEnergy(_)
            | Instruction::RemoveCountersFromPermanent { .. }
            | Instruction::ExileCreatureAtMostRevealedPower
            | Instruction::ExileTarget
            | Instruction::ImprintTargetLand
            | Instruction::ImprintTargetCreature
            | Instruction::ExileTargetAsAbilityDonor
            | Instruction::ExileTargetUntilSourceLeaves
            | Instruction::ExileUntilNextEndStep
            | Instruction::ExileTargetUntilOpponentMonarch
            | Instruction::BoostSource { .. }
            | Instruction::ResolvePowerAbility(_)
            | Instruction::Adapt(_)
            | Instruction::PutCountersOnSource { .. }
            | Instruction::UnattachEquipmentAndProtectHost
            | Instruction::Unattach
            | Instruction::Attach(_)
            | Instruction::GoadTarget
            | Instruction::RequireTargetAttack
            | Instruction::RemoveTargetFromCombat
            | Instruction::DestroyTarget
            | Instruction::DestroyTargetWithoutRegeneration
            | Instruction::ReturnToOwnersHand
            | Instruction::ReturnOwnGraveyardCreatureToBattlefield
            | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
            | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. }
            | Instruction::CounterSpell
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::LoseLifeEqualToTargetSpellManaValue
            | Instruction::ChooseNewTargets
            | Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::GrantProtectionChosenColor(_)
            | Instruction::PutIndestructibleCounter
            | Instruction::PutLoyaltyCounter
            | Instruction::PutNamedCounterOnTarget { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawTargetPlayer { .. }
            | Instruction::DrawHighestEntrantController
            | Instruction::DrawRelatedPlayer
            | Instruction::DrawCombatDamageSourceController
            | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
            | Instruction::DrawControllerAndTarget { .. }
            | Instruction::DrawEachPlayer { .. }
            | Instruction::DrawPlayersControllingName(_)
            | Instruction::DrawPlayer { .. }
            | Instruction::AddExperienceCounter
            | Instruction::AddPlayerCounter { .. }
            | Instruction::ControlManaSourceThroughNextTurn
            | Instruction::GrantControllerFlashUntilCleanup
            | Instruction::RequireOpponentsCreaturesBlockThisTurn
            | Instruction::RequireBlocksAgainstTargetThisTurn
            | Instruction::RevealTopAndRemoveAttackerIfForest
            | Instruction::DrawController { .. } => {}
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
    program.cast_reductions = text.cast_reductions;
    program.zone_replacements = text.zone_replacements;
    program.mana = text.mana;
    program.activated = text.activated;
    program.triggered = text.triggered;
    program.continuous = text.continuous;
    program.additional_battlefield_abilities = text.additional_battlefield_abilities;
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
    fn rewrite_mana_output(change: CreatureTypeTextChange, output: &mut ManaOutput) {
        match output {
            ManaOutput::Counted { permanents, .. } => selector(change, permanents),
            ManaOutput::Modified { modifier, output } => {
                match modifier {
                    crate::mana::ManaOutputModifier::Sacrifice(
                        crate::mana::ManaSacrificeCost::ControlledSubtype(subtype),
                    ) => word(change, subtype),
                    crate::mana::ManaOutputModifier::RequirePermanents {
                        selector: quality,
                        ..
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
    for cost in &mut text.extra_cast_costs {
        cost.rewrite_creature_type(change.from, change.to);
    }
    for ability in &mut text.triggered {
        trigger(change, ability);
    }
    for replacement in &mut text.zone_replacements {
        if let crate::program::ZoneReplacementSubject::Matching { filter, .. } =
            &mut replacement.subject
        {
            filter.rewrite_creature_type(change.from, change.to);
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
                StaticPart::GrantActivated(ability)
                | StaticPart::GrantActivatedRestricted { ability, .. } => {
                    body(change, &mut ability.body)
                }
                StaticPart::GrantSubtypeProtection { types, .. } => static_subtype(change, types),
                StaticPart::GrantTrigger(ability)
                | StaticPart::Abilities(crate::characteristics::AbilityChange {
                    granted_trigger: Some(ability),
                    ..
                }) => trigger(change, ability),
                StaticPart::Names(_)
                | StaticPart::ControlSourceController
                | StaticPart::Color(_)
                | StaticPart::Abilities(crate::characteristics::AbilityChange {
                    granted_trigger: None,
                    ..
                })
                | StaticPart::InheritCounteredCreatureAbilities
                | StaticPart::InheritExiledCreatureAbilities
                | StaticPart::GrantFixedColorMana(_)
                | StaticPart::GrantAnyColorMana
                | StaticPart::GrantMana(_)
                | StaticPart::GrantProtectionChosenColor
                | StaticPart::GrantProtectionChosenPlayer
                | StaticPart::SetPTToOwnManaValue
                | StaticPart::SwitchPT => {}
            }
        }
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
