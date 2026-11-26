use super::*;
use spellwake_oracle_compiler::ast::{
    FilterPredicateAst, NameFilterAst, PartitionMembershipAst, PileRefAst,
};

pub(super) fn subject_is_anaphora(subj: &SubjectRefAst) -> bool {
    matches!(subj, SubjectRefAst::Anaphora(_))
}

pub(super) fn player_is_target_anaphora(p: PlayerRefAst) -> bool {
    matches!(
        p,
        PlayerRefAst::ThatPlayer
            | PlayerRefAst::ItsController
            | PlayerRefAst::ThatObjectController
            | PlayerRefAst::ItsOwner
    )
}

fn zone_is_target_anaphora(zone: ZoneFilter) -> bool {
    matches!(
        zone,
        ZoneFilter::ThatPlayerGraveyard
            | ZoneFilter::ThatPlayerHand
            | ZoneFilter::ThatPlayerLibrary
            | ZoneFilter::ItsOwnerGraveyard
    )
}

fn controller_relation_is_target_anaphora(rel: ControllerRelation) -> bool {
    matches!(
        rel,
        ControllerRelation::ThatPlayer
            | ControllerRelation::ItsController
            | ControllerRelation::OwnerThatPlayer
    )
}

fn name_filter_references_target_anaphora(name: &NameFilterAst) -> bool {
    match name {
        NameFilterAst::SameAsReferenced => true,
        NameFilterAst::SameAs(subj) => subject_references_target_anaphora(subj),
        NameFilterAst::SelfName | NameFilterAst::Exact(_) => false,
    }
}

fn pile_references_target_anaphora(pile: &PileRefAst) -> bool {
    match pile {
        PileRefAst::FilteredInPile { filter, pile } => {
            filter_references_target_anaphora(filter) || pile_references_target_anaphora(pile)
        }
        PileRefAst::ChosenPile
        | PileRefAst::OtherPile
        | PileRefAst::ThatPile
        | PileRefAst::EachPile => false,
    }
}

fn filter_references_target_anaphora(f: &ObjectFilterAst) -> bool {
    if f.zone.is_some_and(zone_is_target_anaphora)
        || f.extra_zones.iter().copied().any(zone_is_target_anaphora)
        || f.controller
            .is_some_and(controller_relation_is_target_anaphora)
        || f.owner.is_some_and(controller_relation_is_target_anaphora)
        || f.named
            .as_ref()
            .is_some_and(name_filter_references_target_anaphora)
        || f.stat_bounds
            .iter()
            .any(|sb| value_expr_references_target_anaphora(&sb.value))
    {
        return true;
    }
    f.predicates.iter().any(|pred| match pred {
        FilterPredicateAst::SuperlativeAmong { scope, .. }
        | FilterPredicateAst::AttachedBy(scope)
        | FilterPredicateAst::FromSource(scope)
        | FilterPredicateAst::NotTargetedBy(scope)
        | FilterPredicateAst::BlockedOrBlockedBy { other: scope, .. } => {
            filter_references_target_anaphora(scope)
        }
        FilterPredicateAst::AnyOf(filters) => filters.iter().any(filter_references_target_anaphora),
        FilterPredicateAst::CouldAttachTo(subj)
        | FilterPredicateAst::SharesPropertyWith {
            reference: subj, ..
        }
        | FilterPredicateAst::DifferentNameFrom(subj)
        | FilterPredicateAst::AttachedTo(subj)
        | FilterPredicateAst::AttackingTarget { target: subj, .. }
        | FilterPredicateAst::Blocking(subj)
        | FilterPredicateAst::Exclude(subj)
        | FilterPredicateAst::TargetsSubject(subj)
        | FilterPredicateAst::CouldBeTargetedBy(subj)
        | FilterPredicateAst::BlockedBy { blocker: subj, .. }
        | FilterPredicateAst::SameNameAsSplicedOnto(subj)
        | FilterPredicateAst::WithSameControllerAs(subj)
        | FilterPredicateAst::MatchesDraftLedger {
            source_card_name: subj,
            ..
        } => subject_references_target_anaphora(subj),
        FilterPredicateAst::InObjectPartition {
            membership: PartitionMembershipAst::MatchingSubjectTag(subj),
            ..
        } => subject_references_target_anaphora(subj),
        FilterPredicateAst::StatRelationTo { expr, .. }
        | FilterPredicateAst::ColorCountCompare { expr, .. } => {
            value_expr_references_target_anaphora(expr)
        }
        FilterPredicateAst::ChosenByPlayer(p) => player_is_target_anaphora(*p),
        FilterPredicateAst::EnteredZoneFromThisTurn { from_zone } => {
            zone_is_target_anaphora(*from_zone)
        }
        _ => false,
    })
}

fn subject_references_target_anaphora(subj: &SubjectRefAst) -> bool {
    match subj {
        SubjectRefAst::Anaphora(_) => true,
        SubjectRefAst::InZone { subject, zone } => {
            subject_references_target_anaphora(subject) || zone_is_target_anaphora(*zone)
        }
        SubjectRefAst::ControllerOf(inner) | SubjectRefAst::OwnerOf(inner) => {
            subject_references_target_anaphora(inner)
        }
        SubjectRefAst::Player(p) => player_is_target_anaphora(*p),
        SubjectRefAst::PlayerMatching { player, condition } => {
            player_is_target_anaphora(*player) || condition_references_target_anaphora(condition)
        }
        SubjectRefAst::Each(f)
        | SubjectRefAst::All(f)
        | SubjectRefAst::Group(f)
        | SubjectRefAst::TargetObject { filter: f, .. }
        | SubjectRefAst::DistinctTarget { filter: f, .. } => filter_references_target_anaphora(f),
        SubjectRefAst::TargetOneOf { options, .. } | SubjectRefAst::Compound(options) => {
            options.iter().any(subject_references_target_anaphora)
        }
        SubjectRefAst::PileRef(pile) => pile_references_target_anaphora(pile),
        SubjectRefAst::ManualTabletopSelection { filter, .. } => filter
            .as_deref()
            .is_some_and(filter_references_target_anaphora),
        SubjectRefAst::SelfObject
        | SubjectRefAst::AttachedHost(_)
        | SubjectRefAst::CraftExiledCards
        | SubjectRefAst::AnyTarget
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::TriggerSubject
        | SubjectRefAst::TriggerCause => false,
    }
}

pub(super) fn value_expr_references_target_anaphora(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            subject_is_anaphora(subject)
        }
        ValueExprAst::Count(f) => filter_references_target_anaphora(f),
        ValueExprAst::PlayerStat { player, .. }
        | ValueExprAst::PlayerCounterCount { player, .. }
        | ValueExprAst::LifeChangedThisTurn { player, .. } => player_is_target_anaphora(*player),
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Max(a, b)
        | ValueExprAst::Min(a, b) => {
            value_expr_references_target_anaphora(a) || value_expr_references_target_anaphora(b)
        }
        ValueExprAst::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            condition_references_target_anaphora(condition)
                || value_expr_references_target_anaphora(then_expr)
                || value_expr_references_target_anaphora(else_expr)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_target_anaphora(expr),
        _ => false,
    }
}

pub(super) fn condition_references_target_anaphora(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_references_target_anaphora(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_references_target_anaphora(a) || condition_references_target_anaphora(b)
        }
        ConditionAst::Compare { lhs, rhs, .. } | ConditionAst::ExprCompare { lhs, rhs, .. } => {
            value_expr_references_target_anaphora(lhs) || value_expr_references_target_anaphora(rhs)
        }
        ConditionAst::MatchesFilter { subject, filter } => {
            subject_references_target_anaphora(subject) || filter_references_target_anaphora(filter)
        }
        ConditionAst::HasStatus { subject, .. }
        | ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::StatCompare { subject, .. }
        | ConditionAst::CounterCompare { subject, .. }
        | ConditionAst::PaidAltCost { subject, .. }
        | ConditionAst::AllColors(subject) => subject_references_target_anaphora(subject),
        ConditionAst::TargetsSubject { subject, target } => {
            subject_references_target_anaphora(subject)
                || subject_references_target_anaphora(target)
        }
        ConditionAst::AttachedTo { subject, host } => {
            subject_references_target_anaphora(subject) || subject_references_target_anaphora(host)
        }
        ConditionAst::SameNameAs { subject, other }
        | ConditionAst::SharesPropertyWith { subject, other, .. } => {
            subject_references_target_anaphora(subject) || subject_references_target_anaphora(other)
        }
        ConditionAst::BlockedByFilter {
            subject,
            blocker_filter,
            ..
        } => {
            subject_references_target_anaphora(subject)
                || filter_references_target_anaphora(blocker_filter)
        }
        ConditionAst::PlayerControlsMost { player, filter, .. }
        | ConditionAst::AttackedWithFilteredThisTurn { player, filter, .. }
        | ConditionAst::ControlsCount { player, filter, .. }
        | ConditionAst::CastSpellThisTurn { player, filter } => {
            player_is_target_anaphora(*player) || filter_references_target_anaphora(filter)
        }
        ConditionAst::ControlsMoreThan {
            player,
            filter,
            than_player,
        } => {
            player_is_target_anaphora(*player)
                || player_is_target_anaphora(*than_player)
                || filter_references_target_anaphora(filter)
        }
        ConditionAst::SubjectControlledBy { subject, player } => {
            subject_references_target_anaphora(subject) || player_is_target_anaphora(*player)
        }
        ConditionAst::SubjectInZone { subject, zone }
        | ConditionAst::WasCastFromZone { subject, zone } => {
            subject_references_target_anaphora(subject) || zone_is_target_anaphora(*zone)
        }
        ConditionAst::TopCardMatches { zone, filter } => {
            zone_is_target_anaphora(*zone) || filter_references_target_anaphora(filter)
        }
        ConditionAst::CardWithNameInZone {
            name_filter,
            filter,
            zone,
            ..
        } => {
            name_filter_references_target_anaphora(name_filter)
                || zone_is_target_anaphora(*zone)
                || filter_references_target_anaphora(filter)
        }
        ConditionAst::IsPhaseOrStep { player, .. } => player.is_some_and(player_is_target_anaphora),
        ConditionAst::GainedLifeAtLeastThisTurn { player, .. } => {
            player_is_target_anaphora(*player)
        }
        ConditionAst::LifeComparePlayers { player, other, .. } => {
            player_is_target_anaphora(*player) || player_is_target_anaphora(*other)
        }
        ConditionAst::DamageDealtThisTurn {
            source,
            target_filter,
            ..
        } => {
            subject_references_target_anaphora(source)
                || target_filter
                    .as_deref()
                    .is_some_and(filter_references_target_anaphora)
        }
        ConditionAst::PriorActionOccurred { subject_filter, .. } => subject_filter
            .as_deref()
            .is_some_and(filter_references_target_anaphora),
        ConditionAst::OpponentMoreThanYou { filter, .. } => filter
            .as_deref()
            .is_some_and(filter_references_target_anaphora),
        ConditionAst::PrimeNumberCheck { expr } => value_expr_references_target_anaphora(expr),
        ConditionAst::AnyOfGroupMatches { group, .. } => subject_references_target_anaphora(group),
        ConditionAst::CanLegallySwapBlockAssignments { subjects, .. } => {
            subject_references_target_anaphora(subjects)
        }
        ConditionAst::ZoneObjectCount { filter, .. }
        | ConditionAst::TotalStatCompare { filter, .. }
        | ConditionAst::TargetsFilter(filter)
        | ConditionAst::EnteredUnderYourControlThisTurn { filter, .. } => {
            filter_references_target_anaphora(filter)
        }
        ConditionAst::ZoneChangeThisTurn {
            filter,
            from_zone,
            to_zone,
            controller,
            ..
        } => {
            filter_references_target_anaphora(filter)
                || from_zone.is_some_and(zone_is_target_anaphora)
                || to_zone.is_some_and(zone_is_target_anaphora)
                || controller.is_some_and(player_is_target_anaphora)
        }
        ConditionAst::HandCount { player, .. }
        | ConditionAst::GraveyardCount { player, .. }
        | ConditionAst::LifeCompare { player, .. }
        | ConditionAst::PlayerActionThisTurn { player, .. }
        | ConditionAst::PlayerCounterCompare { player, .. }
        | ConditionAst::LostLifeThisTurn { player, .. }
        | ConditionAst::UnspentManaAtLeast { player, .. }
        | ConditionAst::IsPlayersTurn(player)
        | ConditionAst::PlayerCoinFlipOutcome { player, .. }
        | ConditionAst::DieRollCompare { player, .. }
        | ConditionAst::IsStartingPlayer(player)
        | ConditionAst::ControlsEachColor(player) => player_is_target_anaphora(*player),
        ConditionAst::Marker(_)
        | ConditionAst::WasKicked
        | ConditionAst::WasKickedSlot(_)
        | ConditionAst::WasCast { .. }
        | ConditionAst::Morbid { .. }
        | ConditionAst::Revolt
        | ConditionAst::Threshold
        | ConditionAst::Delirium
        | ConditionAst::Metalcraft
        | ConditionAst::IsYourTurn
        | ConditionAst::GainedLifeThisTurn
        | ConditionAst::OpponentLostLifeThisTurn
        | ConditionAst::AttackedThisTurn
        | ConditionAst::EnteredThisTurn
        | ConditionAst::ControlsCommander
        | ConditionAst::TributeNotPaid
        | ConditionAst::IsNotToken
        | ConditionAst::SharesTypeWithSelf
        | ConditionAst::OpponentCountCompare { .. }
        | ConditionAst::ManaSpentToCast(_)
        | ConditionAst::WonContest
        | ConditionAst::GameStateFlag(_)
        | ConditionAst::SearchedLibraryThisWay
        | ConditionAst::SpellsCastLastTurn { .. }
        | ConditionAst::DevotionCompare { .. }
        | ConditionAst::CoinFlipOutcome { .. }
        | ConditionAst::PlayersTiedFor { .. }
        | ConditionAst::EventOccurredThisTurn { .. }
        | ConditionAst::ManualTabletopCheck { .. }
        | ConditionAst::NumberParity { .. }
        | ConditionAst::ColorsOfManaSpentCompare { .. }
        | ConditionAst::ResolutionCountThisTurn { .. } => false,
    }
}

pub(super) fn instruction_references_target_anaphora(instr: &InstructionAst) -> bool {
    if instruction_references_anaphora(instr) {
        return true;
    }
    match instr {
        InstructionAst::ChangeLife { player, delta, .. } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(delta)
        }
        InstructionAst::SetLifeTotal { player, value } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(value)
        }
        InstructionAst::Draw { player, count, .. }
        | InstructionAst::Mill { player, count }
        | InstructionAst::Scry { player, count }
        | InstructionAst::Surveil { player, count }
        | InstructionAst::Investigate { player, count }
        | InstructionAst::Blight { player, count } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(count)
        }
        InstructionAst::Discard { player, count, .. } => {
            player_is_target_anaphora(*player)
                || count
                    .as_deref()
                    .is_some_and(value_expr_references_target_anaphora)
        }
        InstructionAst::DiscardHand { player }
        | InstructionAst::RevealHand { player }
        | InstructionAst::ShuffleLibrary { player }
        | InstructionAst::ExtraTurn { player }
        | InstructionAst::BecomeMonarch { player }
        | InstructionAst::TakeInitiative { player } => player_is_target_anaphora(*player),
        InstructionAst::Sacrifice { player, count, .. } => {
            player_is_target_anaphora(*player)
                || count
                    .as_deref()
                    .is_some_and(value_expr_references_target_anaphora)
        }
        InstructionAst::DealDamage { amount, .. } => value_expr_references_target_anaphora(amount),
        InstructionAst::PutCounters { count, .. } => value_expr_references_target_anaphora(count),
        InstructionAst::RemoveCounters { count, .. } => count
            .as_deref()
            .is_some_and(value_expr_references_target_anaphora),
        InstructionAst::GainPlayerCounters { player, count, .. } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(count)
        }
        InstructionAst::CreateToken { controller, .. } => player_is_target_anaphora(*controller),
        InstructionAst::ModifyPT { scale, .. } => scale
            .as_deref()
            .is_some_and(value_expr_references_target_anaphora),
        InstructionAst::PlayerPayCost { player, .. } => player_is_target_anaphora(*player),
        InstructionAst::SearchLibrary { library, .. } => matches!(
            library.trim().to_ascii_lowercase().as_str(),
            "that player" | "that player's" | "its controller" | "its controller's"
        ),
        InstructionAst::ExileTopLibrary { actor, whose, .. } => {
            actor.is_some_and(player_is_target_anaphora)
                || matches!(
                    whose.trim().to_ascii_lowercase().as_str(),
                    "that player" | "that player's" | "its controller" | "its controller's"
                )
        }
        InstructionAst::IfCondition {
            condition,
            then_body,
        }
        | InstructionAst::ForAsLongAs {
            condition,
            body: then_body,
        } => {
            condition_references_target_anaphora(condition)
                || then_body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::Optional(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. } => {
            inner.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } => {
            value_expr_references_target_anaphora(x_expr)
                || value_expr_references_target_anaphora(y_expr)
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            matches!(players.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::CreateEmblemFor { recipient, .. } => {
            matches!(recipient.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
        }
        InstructionAst::RevealCards { subject } => {
            matches!(subject.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
        }
        InstructionAst::OptionalPlayer { player, body } => {
            player_is_target_anaphora(*player)
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_action,
            unless_condition,
            ..
        } => {
            unless_player.is_some_and(player_is_target_anaphora)
                || unless_condition
                    .as_deref()
                    .is_some_and(condition_references_target_anaphora)
                || body.iter().any(instruction_references_target_anaphora)
                || unless_action
                    .as_deref()
                    .is_some_and(|a| a.iter().any(instruction_references_target_anaphora))
        }
        _ => false,
    }
}

pub(super) fn instruction_references_anaphora(instr: &InstructionAst) -> bool {
    match instr {
        InstructionAst::DealDamage { source, target, .. } => {
            subject_is_anaphora(source) || subject_is_anaphora(target)
        }
        InstructionAst::Fight { a, b } => subject_is_anaphora(a) || subject_is_anaphora(b),
        InstructionAst::MoveCounters { from, to, .. }
        | InstructionAst::MoveCounter { from, to, .. } => {
            subject_is_anaphora(from) || subject_is_anaphora(to)
        }
        InstructionAst::Attach {
            subject, target, ..
        } => subject_is_anaphora(subject) || subject_is_anaphora(target),
        InstructionAst::RedirectDamage { from, to, .. } => {
            subject_is_anaphora(from) || subject_is_anaphora(to)
        }
        InstructionAst::BecomeCopy {
            subject, source, ..
        } => subject_is_anaphora(subject) || subject_is_anaphora(source),
        InstructionAst::Exchange { left, right, .. } => {
            subject_is_anaphora(left) || right.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::Sacrifice { subject, .. }
        | InstructionAst::SacrificeUnlessPay { subject, .. }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::PhaseOutUntil { subject, .. }
        | InstructionAst::Transform { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Copy { subject, .. }
        | InstructionAst::GrantCastPermission { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::SetSuspected { subject, .. }
        | InstructionAst::TurnFaceDown { subject }
        | InstructionAst::RevealCards { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::Exert { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::DoubleStatOrCounters { subject, .. } => subject_is_anaphora(subject),
        InstructionAst::CreateEmblemFor { recipient, .. } => {
            subject_is_anaphora(recipient)
                || matches!(
                    recipient.as_ref(),
                    SubjectRefAst::ControllerOf(inner) | SubjectRefAst::OwnerOf(inner)
                        if subject_is_anaphora(inner)
                )
        }
        InstructionAst::Counter { target, .. }
        | InstructionAst::PreventDamage { target, .. }
        | InstructionAst::ChangeTargets { target, .. }
        | InstructionAst::ChangeText { target, .. } => subject_is_anaphora(target),
        InstructionAst::PreventAllDamage { target, .. } => {
            target.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::Cloak { subject, .. } | InstructionAst::Airbend { subject } => {
            subject.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::Discard { anaphora, .. } => anaphora.is_some(),
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::ForAsLongAs { body: inner, .. }
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachObject { body: inner, .. }
        | InstructionAst::ForEachCount { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. } => {
            inner.iter().any(instruction_references_anaphora)
        }
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            subject_is_anaphora(players) || body.iter().any(instruction_references_anaphora)
        }
        InstructionAst::Unless {
            body,
            unless_action,
            ..
        } => {
            body.iter().any(instruction_references_anaphora)
                || unless_action
                    .as_ref()
                    .is_some_and(|a| a.iter().any(instruction_references_anaphora))
        }
        InstructionAst::Choice(branches) => branches
            .iter()
            .any(|b| b.iter().any(instruction_references_anaphora)),
        _ => false,
    }
}

fn condition_references_chosen_x(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_references_chosen_x(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_references_chosen_x(a) || condition_references_chosen_x(b)
        }
        ConditionAst::Compare { lhs, rhs, .. } | ConditionAst::ExprCompare { lhs, rhs, .. } => {
            value_expr_references_chosen_x(lhs) || value_expr_references_chosen_x(rhs)
        }
        _ => false,
    }
}

fn value_expr_references_chosen_x(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ChosenX => true,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Max(a, b)
        | ValueExprAst::Min(a, b)
        | ValueExprAst::AbsDiff(a, b) => {
            value_expr_references_chosen_x(a) || value_expr_references_chosen_x(b)
        }
        ValueExprAst::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            condition_references_chosen_x(condition)
                || value_expr_references_chosen_x(then_expr)
                || value_expr_references_chosen_x(else_expr)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_chosen_x(expr),
        ValueExprAst::Pow { base, exp } => {
            value_expr_references_chosen_x(base) || value_expr_references_chosen_x(exp)
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            value_expr_references_chosen_x(threshold) || value_expr_references_chosen_x(actual)
        }
        _ => false,
    }
}

pub(super) fn scale_ast_amount(base: &ValueExprAst, mult: ValueExprAst) -> ValueExprAst {
    if matches!(base, ValueExprAst::Const(1)) {
        mult
    } else {
        ValueExprAst::Mul(Box::new(mult), Box::new(base.clone()))
    }
}

pub(super) fn try_scale_token_spec(
    spec: &TokenSpecAst,
    mult: ValueExprAst,
) -> Option<TokenSpecAst> {
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => Some(TokenSpecAst::Predefined {
            name: name.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
            tapped: *tapped,
        }),
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => Some(TokenSpecAst::Creature {
            count: Box::new(scale_ast_amount(count, mult)),
            power: power.clone(),
            toughness: toughness.clone(),
            colors: *colors,
            supertypes: *supertypes,
            card_types: *card_types,
            subtypes: subtypes.clone(),
            keywords: keywords.clone(),
            tapped: *tapped,
            attacking: *attacking,
            named: named.clone(),
        }),
        _ => None,
    }
}

pub(super) fn try_scale_instruction(
    instr: &InstructionAst,
    mult: ValueExprAst,
) -> Option<InstructionAst> {
    match instr {
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => Some(InstructionAst::ChangeLife {
            player: *player,
            delta: Box::new(scale_ast_amount(delta, mult)),
            gain: *gain,
        }),
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } => Some(InstructionAst::Draw {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
            up_to: *up_to,
        }),
        InstructionAst::Mill { player, count } => Some(InstructionAst::Mill {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora: None,
            random: false,
        } => Some(InstructionAst::Discard {
            player: *player,
            count: Some(Box::new(scale_ast_amount(
                count.as_deref().unwrap_or(&ValueExprAst::Const(1)),
                mult,
            ))),
            filter: filter.clone(),
            anaphora: None,
            random: false,
        }),
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => Some(InstructionAst::DealDamage {
            source: source.clone(),
            target: target.clone(),
            amount: Box::new(scale_ast_amount(amount, mult)),
        }),
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => Some(InstructionAst::PutCounters {
            subject: subject.clone(),
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => Some(InstructionAst::GainPlayerCounters {
            player: *player,
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::CreateToken { controller, token } => Some(InstructionAst::CreateToken {
            controller: *controller,
            token: try_scale_token_spec(token, mult)?,
        }),
        InstructionAst::Investigate { player, count } => Some(InstructionAst::Investigate {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } if !body.is_empty() && !value_expr_references_chosen_x(&mult) => {
            let scaled_body = body
                .iter()
                .map(|s| try_scale_instruction(s, mult.clone()))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::BindXY {
                x_expr: x_expr.clone(),
                y_expr: y_expr.clone(),
                body: scaled_body,
            })
        }
        InstructionAst::ForEachSubjectPlayer { players, body } if !body.is_empty() => {
            let scaled_body = body
                .iter()
                .map(|s| try_scale_instruction(s, mult.clone()))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForEachSubjectPlayer {
                players: players.clone(),
                body: scaled_body,
            })
        }
        _ => None,
    }
}

pub(super) fn instruction_acts_only_on_controller(instr: &InstructionAst) -> bool {
    matches!(
        instr,
        InstructionAst::Scry {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Surveil {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Proliferate
            | InstructionAst::Explore { .. }
            | InstructionAst::FlipCoin
            | InstructionAst::RollDie { .. }
            | InstructionAst::Investigate {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                ..
            }
            | InstructionAst::Draw {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::Mill {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::GainPlayerCounters {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::PutCounters { .. }
    )
}

pub(super) fn try_scale_controller_instruction(
    instr: &InstructionAst,
    mult: ValueExprAst,
) -> Option<InstructionAst> {
    let acts_on_you = matches!(
        instr,
        InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Draw {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Mill {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            ..
        } | InstructionAst::Investigate {
            player: PlayerRefAst::You,
            ..
        }
    );
    if acts_on_you {
        try_scale_instruction(instr, mult)
    } else {
        None
    }
}

pub(super) fn try_rewrite_for_each_player(
    instr: &InstructionAst,
    target_player: PlayerRefAst,
) -> Option<InstructionAst> {
    let matches_loop_player = |p: PlayerRefAst| p == PlayerRefAst::ThatPlayer || p == target_player;
    let rewrite_that_player_controller = |f: &ObjectFilterAst| -> Option<ObjectFilterAst> {
        if f.controller != Some(ControllerRelation::ThatPlayer) {
            return None;
        }
        let mut out = f.clone();
        out.controller = match target_player {
            PlayerRefAst::EachPlayer => None,
            PlayerRefAst::EachOpponent => Some(ControllerRelation::Opponent),
            PlayerRefAst::You => Some(ControllerRelation::You),
            _ => return None,
        };
        Some(out)
    };
    let rewrite_that_player_zone_or_ctrl = |f: &ObjectFilterAst| -> Option<ObjectFilterAst> {
        let mut out = f.clone();
        let mut changed = false;
        if out.controller == Some(ControllerRelation::ThatPlayer) {
            out.controller = match target_player {
                PlayerRefAst::EachPlayer => None,
                PlayerRefAst::EachOpponent => Some(ControllerRelation::Opponent),
                PlayerRefAst::You => Some(ControllerRelation::You),
                _ => return None,
            };
            changed = true;
        }
        if out.zone == Some(ZoneFilter::ThatPlayerHand) {
            out.zone = Some(match target_player {
                PlayerRefAst::EachPlayer => ZoneFilter::EachPlayerHand,
                PlayerRefAst::EachOpponent => ZoneFilter::OpponentsHands,
                PlayerRefAst::You => ZoneFilter::YourHand,
                _ => return None,
            });
            changed = true;
        }
        changed.then_some(out)
    };
    match instr {
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } if matches_loop_player(*player) => Some(InstructionAst::Draw {
            player: target_player,
            count: count.clone(),
            up_to: *up_to,
        }),
        InstructionAst::Mill { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Mill {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } if matches_loop_player(*player) => Some(InstructionAst::Discard {
            player: target_player,
            count: count.clone(),
            filter: filter.clone(),
            anaphora: anaphora.clone(),
            random: *random,
        }),
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half,
            rounding,
            chooser,
        } if matches_loop_player(*player) => Some(InstructionAst::Sacrifice {
            player: target_player,
            count: count.clone(),
            subject: subject.clone(),
            except: except.clone(),
            half: *half,
            rounding: *rounding,
            chooser: *chooser,
        }),
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } if matches_loop_player(*player) => Some(InstructionAst::ChangeLife {
            player: target_player,
            delta: delta.clone(),
            gain: *gain,
        }),
        InstructionAst::CreateToken { controller, token } if matches_loop_player(*controller) => {
            Some(InstructionAst::CreateToken {
                controller: target_player,
                token: token.clone(),
            })
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } if matches_loop_player(*player) => Some(InstructionAst::GainPlayerCounters {
            player: target_player,
            counter_kind: counter_kind.clone(),
            count: count.clone(),
        }),
        InstructionAst::Investigate { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Investigate {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Scry { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Scry {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Surveil { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Surveil {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } if matches!(target.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) => {
            Some(InstructionAst::DealDamage {
                source: source.clone(),
                target: Box::new(SubjectRefAst::Player(target_player)),
                amount: amount.clone(),
            })
        }
        InstructionAst::CreateEmblemFor {
            recipient,
            text,
            compiled,
        } if matches!(recipient.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) => {
            Some(InstructionAst::CreateEmblemFor {
                recipient: Box::new(SubjectRefAst::Player(target_player)),
                text: text.clone(),
                compiled: compiled.clone(),
            })
        }
        InstructionAst::RevealCards { subject } => match subject.as_ref() {
            SubjectRefAst::Player(p) if matches_loop_player(*p) => {
                Some(InstructionAst::RevealCards {
                    subject: Box::new(SubjectRefAst::Player(target_player)),
                })
            }
            SubjectRefAst::All(f) => Some(InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_zone_or_ctrl(f)?,
                ))),
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_zone_or_ctrl(f)?,
                ))),
            }),
            _ => None,
        },
        InstructionAst::SetSuspected { subject, remove } => match subject.as_ref() {
            SubjectRefAst::All(f) => Some(InstructionAst::SetSuspected {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
                remove: *remove,
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::SetSuspected {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
                remove: *remove,
            }),
            _ => None,
        },
        InstructionAst::TurnFaceDown { subject } => match subject.as_ref() {
            SubjectRefAst::All(f) => Some(InstructionAst::TurnFaceDown {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::TurnFaceDown {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
            }),
            _ => None,
        },
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } if !body.is_empty()
            && !value_expr_references_target_anaphora(x_expr)
            && !value_expr_references_target_anaphora(y_expr) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::BindXY {
                x_expr: x_expr.clone(),
                y_expr: y_expr.clone(),
                body: rewritten,
            })
        }
        InstructionAst::ForAsLongAs { condition, body }
            if !body.is_empty() && !condition_references_target_anaphora(condition) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForAsLongAs {
                condition: condition.clone(),
                body: rewritten,
            })
        }
        InstructionAst::ForEachSubjectPlayer { players, body }
            if !body.is_empty()
                && matches!(players.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForEachSubjectPlayer {
                players: Box::new(SubjectRefAst::Player(target_player)),
                body: rewritten,
            })
        }
        InstructionAst::ExileGraveyard { whose }
            if matches!(
                whose.trim().to_ascii_lowercase().as_str(),
                "their" | "that player's" | "his or her"
            ) =>
        {
            let mapped = match target_player {
                PlayerRefAst::EachPlayer => "each player's",
                PlayerRefAst::EachOpponent => "each opponent's",
                PlayerRefAst::You => "your",
                _ => return None,
            };
            Some(InstructionAst::ExileGraveyard {
                whose: mapped.to_owned(),
            })
        }
        _ => None,
    }
}

pub(super) fn push_damage_target(instructions: &mut Vec<Instruction>, amt: ValueExpr) {
    if let ValueExpr::Literal(n) = amt {
        instructions.push(Instruction::Damage { amount: n });
    } else {
        instructions.push(Instruction::DamageAmount(amt));
    }
}

pub(super) fn lower_damage_group(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<DamageGroup, &'static str> {
    if f.any_types == AstCardTypeMask::CREATURE
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
    {
        if f.forbidden_keywords == ["flying"] {
            return Ok(DamageGroup::CreaturesWithoutFlying);
        }
        if f.subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
        {
            return Ok(DamageGroup::CreaturesOfSubtype(ct));
        }
        if f.forbidden_subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.forbidden_subtypes[0])
        {
            return Ok(DamageGroup::CreaturesExcept(ct));
        }
        if f.subtypes.is_empty()
            && f.forbidden_subtypes.is_empty()
            && f.required_keywords.is_empty()
            && f.forbidden_keywords.is_empty()
            && f.flags.is_empty()
        {
            return Ok(DamageGroup::Creatures);
        }
    }
    if f.any_types == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.flags.is_empty()
    {
        return Ok(DamageGroup::CreaturesAndPlaneswalkers);
    }
    if f.zone.is_none() {
        let obj = lower_object_filter(f, vocabulary)?;
        return Ok(DamageGroup::Filtered(obj));
    }
    Err("oracle:unsupported-damage-group")
}

pub(super) fn scale_value_expr(expr: ValueExpr, factor: i64) -> ValueExpr {
    match factor {
        0 => ValueExpr::literal(0),
        1 => expr,
        -1 => ValueExpr::Sub(Box::new(ValueExpr::literal(0)), Box::new(expr)),
        f => ValueExpr::Scale {
            expr: Box::new(expr),
            factor: f.into(),
        },
    }
}

pub(super) fn parse_dynamic_pt_expr(
    raw: &str,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    let trimmed = raw.trim();
    if let Some(n) = parse_signed_pt_delta(trimmed) {
        return Some(ValueExpr::literal(n));
    }
    let low = trimmed.to_ascii_lowercase();
    let x_val = || -> Option<ValueExpr> {
        match bound_x {
            Some(bx) => lower_value_expr(bx, vocabulary).ok(),
            None => Some(ValueExpr::chosen_x()),
        }
    };
    if low == "x" || low == "+x" {
        return x_val();
    }
    if low == "-x" {
        return Some(scale_value_expr(x_val()?, -1));
    }
    if let Some(coeff) = low.strip_suffix('x')
        && let Ok(k) = coeff.parse::<i64>()
    {
        return Some(scale_value_expr(x_val()?, k));
    }
    None
}

fn bind_filter_player_target_local(
    builder: &mut BodyBuilder,
    filter: &ObjectFilter,
) -> Result<(), &'static str> {
    let mut needs_player = false;
    let mut needs_opponent = false;
    let mut inspect = |clause: ObjectClause| {
        for relation in [clause.controller, clause.owner] {
            match relation {
                ControllerFilter::TargetPlayer => needs_player = true,
                ControllerFilter::TargetOpponent => needs_opponent = true,
                _ => {}
            }
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

fn is_self_anaphora_label_local(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "it" | "its"
            | "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
    )
}

fn last_instruction_created_tokens_local(ops: &[Instruction]) -> bool {
    matches!(
        ops.last(),
        Some(
            Instruction::CreateTokens { .. }
                | Instruction::CreateTokenCopy { .. }
                | Instruction::CreateTokenCopyExt { .. }
                | Instruction::CreateTokenAndAttach { .. }
                | Instruction::CreateAuraTokenAttached { .. }
                | Instruction::CreateAttackingTokens { .. }
        )
    ) || matches!(
        ops.last(),
        Some(
            Instruction::ModifyGroupUntilEndOfTurn { group, .. }
                | Instruction::ChangeGroup { group, .. }
                | Instruction::ModifyGroupByAmount { group, .. }
                | Instruction::SuppressGroupKeywords { group, .. }
        ) if matches!(group.as_ref(), EffectSubject::CreatedTokens)
    ) || matches!(
        ops.last(),
        Some(
            Instruction::PutCountersOnSubject { subject, .. }
                | Instruction::RemoveCountersFromSubject { subject, .. }
                | Instruction::SetSubjectTapped { subject, .. }
                | Instruction::SkipNextUntap { subject }
                | Instruction::RequireCombatAction { subject, .. }
        ) if matches!(subject.as_ref(), EffectSubject::CreatedTokens)
    )
}

fn existing_affect_permanent_targets_bounds(ops: &[Instruction]) -> Option<(u16, u16)> {
    ops.iter().rev().find_map(|op| {
        if let Instruction::AffectPermanentTargets {
            minimum, maximum, ..
        } = op
        {
            Some((*minimum, *maximum))
        } else {
            None
        }
    })
}

fn rewrite_object_filter_controller_local(
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

fn rewrite_trigger_power_to_target_power_local(obj: &mut ObjectFilter) {
    use spellwake_engine::program::DynamicStatAnchor;
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

fn lower_object_filter_in_context_local(
    f: &ObjectFilterAst,
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    let mut obj = lower_object_filter(f, vocabulary)?;
    if f.controller == Some(ControllerRelation::DefendingPlayer) {
        rewrite_object_filter_controller_local(
            &mut obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::DefendingPlayer,
        );
    } else if !in_trigger
        && builder.target == BodyTarget::None
        && builder.instructions.is_empty()
        && f.controller == Some(ControllerRelation::ThatPlayer)
    {
        rewrite_object_filter_controller_local(
            &mut obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::ActivePlayer,
        );
    }
    if !in_trigger && is_permanent_body_target(builder.target) {
        rewrite_trigger_power_to_target_power_local(&mut obj);
    }
    Ok(obj)
}

fn lower_battlefield_body_target_in_context_local(
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
        rewrite_object_filter_controller_local(
            obj,
            ControllerFilter::RelatedPlayer,
            ControllerFilter::ActivePlayer,
        );
    }
    Ok(bt)
}

fn merge_object_clauses_by_types_local(clauses: Vec<ObjectClause>) -> Vec<ObjectClause> {
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

fn lower_target_one_of_permanent_filter_local(
    options: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    if options.is_empty() {
        return Err("oracle:unsupported-modify-pt-subject");
    }
    let mut perm_clauses: Vec<ObjectClause> = Vec::new();
    for opt in options {
        let SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } = opt
        else {
            return Err("oracle:unsupported-modify-pt-subject");
        };
        if filter.zone.is_some() || filter.is_ability || filter.any_spell {
            return Err("oracle:unsupported-modify-pt-subject");
        }
        let obj = lower_object_filter_in_context_local(filter, builder, in_trigger, vocabulary)?;
        if obj.has_target_player() {
            return Err("oracle:unsupported-modify-pt-subject");
        }
        match obj {
            ObjectFilter::Clause(c) => perm_clauses.push(c),
            ObjectFilter::Either(a, b) => {
                perm_clauses.push(a);
                perm_clauses.push(b);
            }
        }
    }
    let perm_clauses = merge_object_clauses_by_types_local(perm_clauses);
    match perm_clauses.len() {
        1 => Ok(ObjectFilter::Clause(perm_clauses[0])),
        2 => Ok(ObjectFilter::Either(perm_clauses[0], perm_clauses[1])),
        _ => Err("oracle:unsupported-modify-pt-subject"),
    }
}

fn lower_target_one_of_permanent_body_target_local(
    options: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    let filter =
        lower_target_one_of_permanent_filter_local(options, builder, in_trigger, vocabulary)?;
    if filter == ObjectFilter::creature() {
        Ok(BodyTarget::SingleCreature)
    } else if filter == ObjectFilter::planeswalker() {
        Ok(BodyTarget::SinglePlaneswalker)
    } else if filter == ObjectFilter::any() {
        Ok(BodyTarget::SinglePermanent)
    } else {
        Ok(BodyTarget::FilteredPermanent(filter))
    }
}

fn subject_contains_player_ref_local(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::Player(_)
        | SubjectRefAst::PlayerMatching { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
        | SubjectRefAst::AnyTarget => true,
        SubjectRefAst::Compound(items) => items.iter().any(subject_contains_player_ref_local),
        SubjectRefAst::InZone { subject, .. } => subject_contains_player_ref_local(subject),
        _ => false,
    }
}

fn is_other_than_target_state_qual(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "other than that creature" | "other than that permanent"
    )
}

fn try_extract_compound_target_relation(
    items: &[SubjectRefAst],
    builder: &BodyBuilder,
    in_trigger: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Option<(BodyTarget, EffectSubject)>, &'static str> {
    let [
        SubjectRefAst::TargetObject {
            filter: f1,
            count: TargetCountAst::Exact(1),
        },
        SubjectRefAst::All(f2) | SubjectRefAst::Each(f2),
    ] = items
    else {
        return Ok(None);
    };
    if f1.zone.is_some() || f2.zone.is_some() {
        return Ok(None);
    }

    let is_same_name_target = matches!(&f2.named, Some(NameFilterAst::SameAsReferenced))
        || matches!(
            &f2.named,
            Some(NameFilterAst::SameAs(ref_subj))
                if matches!(
                    ref_subj.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::TargetObject { .. }
                )
        );
    if is_same_name_target {
        let bt =
            lower_battlefield_body_target_in_context_local(f1, builder, in_trigger, vocabulary)?;
        let mut f2_clean = f2.as_ref().clone();
        f2_clean.named = None;
        f2_clean.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
        f2_clean.predicates.retain(|pred| match pred {
            FilterPredicateAst::Exclude(subj) => {
                !matches!(subj.as_ref(), SubjectRefAst::Anaphora(_))
            }
            FilterPredicateAst::StateQualification(raw) => {
                let norm = raw.trim().to_ascii_lowercase();
                !matches!(
                    norm.as_str(),
                    "with the same name as that creature"
                        | "with the same name as that permanent"
                        | "with the same name as that land"
                        | "with the same name as that card"
                        | "other than that creature"
                        | "other than that permanent"
                )
            }
            _ => true,
        });
        let obj = lower_object_filter_in_context_local(&f2_clean, builder, in_trigger, vocabulary)?;
        if obj.has_target_player() {
            return Err("oracle:unsupported-modify-pt-subject");
        }
        return Ok(Some((bt, EffectSubject::TargetAndSameNameMatching(obj))));
    }

    let share_color_idx = f2.predicates.iter().position(|pred| {
        matches!(
            pred,
            FilterPredicateAst::SharesPropertyWith {
                property,
                reference,
                negated: false,
            } if property.eq_ignore_ascii_case("color")
                && match reference.as_ref() {
                    SubjectRefAst::Anaphora(raw) => !raw.trim().eq_ignore_ascii_case("group"),
                    SubjectRefAst::TargetObject { .. } => true,
                    _ => false,
                }
        )
    });
    if let Some(idx) = share_color_idx {
        let bt =
            lower_battlefield_body_target_in_context_local(f1, builder, in_trigger, vocabulary)?;
        let mut f2_clean = f2.as_ref().clone();
        f2_clean.predicates.remove(idx);
        f2_clean.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
        f2_clean.predicates.retain(|pred| match pred {
            FilterPredicateAst::Exclude(subj) => {
                !matches!(subj.as_ref(), SubjectRefAst::Anaphora(_))
            }
            FilterPredicateAst::StateQualification(raw) => !is_other_than_target_state_qual(raw),
            _ => true,
        });
        let obj = lower_object_filter_in_context_local(&f2_clean, builder, in_trigger, vocabulary)?;
        if obj.has_target_player() {
            return Err("oracle:unsupported-modify-pt-subject");
        }
        return Ok(Some((
            bt,
            EffectSubject::TargetAndSharingColorMatching(obj),
        )));
    }

    Ok(None)
}

fn lower_modify_pt_group_subject(
    f: &ObjectFilterAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<EffectSubject, &'static str> {
    if let Some(excl_target_idx) = f.predicates.iter().position(|pred| {
        matches!(
            pred,
            FilterPredicateAst::Exclude(subj)
                if matches!(
                    subj.as_ref(),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none()
                )
        )
    }) {
        let FilterPredicateAst::Exclude(subj) = &f.predicates[excl_target_idx] else {
            return Err("oracle:unsupported-modify-pt-subject");
        };
        let SubjectRefAst::TargetObject { filter: tf, .. } = subj.as_ref() else {
            return Err("oracle:unsupported-modify-pt-subject");
        };
        let bt =
            lower_battlefield_body_target_in_context_local(tf, builder, in_trigger, vocabulary)?;
        let mut f_clean = f.clone();
        f_clean.predicates.remove(excl_target_idx);
        let obj = lower_object_filter_in_context_local(&f_clean, builder, in_trigger, vocabulary)?;
        if obj.has_target_player() {
            return Err("oracle:unsupported-modify-pt-subject");
        }
        builder.bind_target(bt)?;
        return Ok(EffectSubject::MatchingOtherThanTarget(obj));
    }

    if is_permanent_body_target(builder.target) {
        let excl_anaphora_idx = f.predicates.iter().position(|pred| match pred {
            FilterPredicateAst::Exclude(subj) => {
                matches!(subj.as_ref(), SubjectRefAst::Anaphora(_))
            }
            FilterPredicateAst::StateQualification(raw) => is_other_than_target_state_qual(raw),
            _ => false,
        });
        if let Some(idx) = excl_anaphora_idx {
            let mut f_clean = f.clone();
            f_clean.predicates.remove(idx);
            let obj =
                lower_object_filter_in_context_local(&f_clean, builder, in_trigger, vocabulary)?;
            if obj.has_target_player() {
                return Err("oracle:unsupported-modify-pt-subject");
            }
            return Ok(EffectSubject::MatchingOtherThanTarget(obj));
        }
        if !self_pronoun_valid && !in_trigger && (f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0 {
            let mut f_clean = f.clone();
            f_clean.flags.0 &= !AstFilterFlags::EXCLUDE_SELF.0;
            let obj =
                lower_object_filter_in_context_local(&f_clean, builder, in_trigger, vocabulary)?;
            if obj.has_target_player() {
                return Err("oracle:unsupported-modify-pt-subject");
            }
            return Ok(EffectSubject::MatchingOtherThanTarget(obj));
        }
    }

    let obj = lower_object_filter_in_context_local(f, builder, in_trigger, vocabulary)?;
    bind_filter_player_target_local(builder, &obj)?;
    builder.last_group_filter = Some(obj);
    Ok(EffectSubject::Matching(obj))
}

pub(super) fn lower_modify_pt_part(
    subject: &SubjectRefAst,
    part: ResolvedPartSpec,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            if !self_pronoun_valid && !in_trigger {
                return Err("oracle:spell-cannot-modify-self");
            }
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Compound(items) => {
            if items.is_empty() || items.iter().any(subject_contains_player_ref_local) {
                return Err("oracle:unsupported-modify-pt-subject");
            }
            if let Some((bt, group)) =
                try_extract_compound_target_relation(items, builder, in_trigger, vocabulary)?
            {
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::ChangeGroup {
                    group: Box::new(group),
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
                return Ok(());
            }
            let mut trial = builder.clone();
            let mut target_binders = 0usize;
            for item in items {
                let prev_target = trial.target;
                lower_modify_pt_part(
                    item,
                    part.clone(),
                    dur,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )?;
                if trial.target != prev_target {
                    target_binders += 1;
                    if target_binders > 1 {
                        return Err("oracle:unsupported-modify-pt-subject");
                    }
                }
            }
            if items.len() > 1 {
                trial.last_group_filter = None;
            }
            *builder = trial;
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target_in_context_local(
                filter, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if filter.zone.is_none() && *n >= 2 => {
            let bt = lower_battlefield_body_target_in_context_local(
                filter, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if filter.zone.is_none() && *max > 0 => {
            let bt = lower_battlefield_body_target_in_context_local(
                filter, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::ChosenX,
        } if filter.zone.is_none() && !in_trigger => {
            let bt = lower_battlefield_body_target_in_context_local(
                filter, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.x_target_count = true;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if filter.zone.is_none() => {
            let obj =
                lower_object_filter_in_context_local(filter, builder, in_trigger, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => {
            let bt = lower_target_one_of_permanent_body_target_local(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(n),
        } if *n >= 2 => {
            let bt = lower_target_one_of_permanent_body_target_local(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::UpTo(max),
        } if *max > 0 => {
            let bt = lower_target_one_of_permanent_body_target_local(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::ChosenX,
        } if !in_trigger => {
            let bt = lower_target_one_of_permanent_body_target_local(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.x_target_count = true;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::AnyNumber,
        } => {
            let obj = lower_target_one_of_permanent_filter_local(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if last_instruction_created_tokens_local(&builder.instructions)
                && (!is_permanent_body_target(builder.target)
                    || raw.to_ascii_lowercase().contains("token")) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::CreatedTokens),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            if let Some((minimum, maximum)) =
                existing_affect_permanent_targets_bounds(&builder.instructions)
            {
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum,
                        maximum,
                        action: PermanentTargetAction::Modify {
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        },
                    });
            } else if builder
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::ModifyAnyNumberOfPermanents { .. }))
            {
                builder
                    .instructions
                    .push(Instruction::ModifyAnyNumberOfPermanents {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    });
            } else {
                builder
                    .instructions
                    .push(Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    });
            }
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if !is_permanent_body_target(builder.target)
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(group_obj)),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if in_trigger
                && (!self_pronoun_valid || !is_self_anaphora_label_local(raw))
                && builder.target == BodyTarget::None =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            let group = lower_modify_pt_group_subject(
                f,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(group),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-modify-pt-subject"),
    }
}

pub(super) fn lower_modify_pt_target(
    subject: &SubjectRefAst,
    pt: PowerToughness,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid)
            && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            if in_trigger {
                builder
                    .instructions
                    .push(Instruction::ModifyGroupUntilEndOfTurn {
                        group: Box::new(EffectSubject::AbilitySource),
                        power_toughness: Some(pt),
                        keywords: BTreeSet::new(),
                    });
            } else if !self_pronoun_valid {
                return Err("oracle:spell-cannot-modify-self");
            } else {
                builder.instructions.push(Instruction::BoostSource {
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            }
            Ok(())
        }
        SubjectRefAst::Compound(items) if dur == ResolvedDuration::UntilEndOfTurn => {
            if items.is_empty() || items.iter().any(subject_contains_player_ref_local) {
                return Err("oracle:unsupported-modify-pt-subject");
            }
            if let Some((bt, group)) =
                try_extract_compound_target_relation(items, builder, in_trigger, vocabulary)?
            {
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::ModifyGroupUntilEndOfTurn {
                        group: Box::new(group),
                        power_toughness: Some(pt),
                        keywords: BTreeSet::new(),
                    });
                return Ok(());
            }
            let mut trial = builder.clone();
            let mut target_binders = 0usize;
            for item in items {
                let prev_target = trial.target;
                lower_modify_pt_target(
                    item,
                    pt.clone(),
                    dur,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )?;
                if trial.target != prev_target {
                    target_binders += 1;
                    if target_binders > 1 {
                        return Err("oracle:unsupported-modify-pt-subject");
                    }
                }
            }
            if items.len() > 1 {
                trial.last_group_filter = None;
            }
            *builder = trial;
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger && dur == ResolvedDuration::UntilEndOfTurn => {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::TriggerSubject),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if dur == ResolvedDuration::UntilEndOfTurn
                && last_instruction_created_tokens_local(&builder.instructions)
                && (!is_permanent_body_target(builder.target)
                    || raw.to_ascii_lowercase().contains("token")) =>
        {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::CreatedTokens),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if !is_permanent_body_target(builder.target)
                && dur == ResolvedDuration::UntilEndOfTurn
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(group_obj)),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if in_trigger
                && (!self_pronoun_valid || !is_self_anaphora_label_local(raw))
                && builder.target == BodyTarget::None
                && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::TriggerSubject),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if dur == ResolvedDuration::UntilEndOfTurn => {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ))),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            let group = lower_modify_pt_group_subject(
                f,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(group),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        _ => lower_modify_pt_part(
            subject,
            ResolvedPartSpec::ModifyPT(pt),
            dur,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
    }
}

pub(super) fn lower_sacrifice_selection(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::SacrificeSelection> {
    use spellwake_engine::program::SacrificeSelection;
    if !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !matches!(f.controller, None | Some(ControllerRelation::You))
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.any_card
        || f.any_spell
        || f.is_ability
    {
        return None;
    }
    let exclude_self = f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
    if f.flags.0 & !AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        return None;
    }
    if f.subtypes.len() == 1 {
        if exclude_self
            || f.token.is_some()
            || (!f.any_types.is_empty() && f.any_types != AstCardTypeMask::CREATURE)
        {
            return None;
        }
        let ct = vocabulary.resolve_label(&f.subtypes[0])?;
        return Some(SacrificeSelection::Subtype(ct));
    }
    if !f.subtypes.is_empty() {
        return None;
    }
    let nontoken = match f.token {
        None => false,
        Some(false) => true,
        Some(true) => return None,
    };
    let art_or_cre = AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE);
    if nontoken {
        if !exclude_self && f.any_types == AstCardTypeMask::CREATURE {
            return Some(SacrificeSelection::NontokenCreature);
        }
        return None;
    }
    if f.any_types == AstCardTypeMask::CREATURE {
        Some(if exclude_self {
            SacrificeSelection::OtherCreature
        } else {
            SacrificeSelection::Creature
        })
    } else if f.any_types == AstCardTypeMask::ARTIFACT {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifact
        } else {
            SacrificeSelection::Artifact
        })
    } else if f.any_types == AstCardTypeMask::ENCHANTMENT && !exclude_self {
        Some(SacrificeSelection::Enchantment)
    } else if f.any_types == AstCardTypeMask::LAND && !exclude_self {
        Some(SacrificeSelection::Land)
    } else if f.any_types == art_or_cre {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifactOrCreature
        } else {
            SacrificeSelection::ArtifactOrCreature
        })
    } else if f.any_types.is_empty() && f.any_permanent {
        Some(if exclude_self {
            SacrificeSelection::OtherPermanent
        } else {
            SacrificeSelection::Permanent
        })
    } else {
        None
    }
}

pub(super) fn try_build_offer_mana(
    payer: ResolvingPlayer,
    raw: &str,
    paid_destination: usize,
    declined_destination: usize,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Instruction> {
    if let Some(cost) = parse_fixed_mana_cost(raw) {
        return Some(Instruction::OfferManaPayment {
            payer,
            cost,
            paid_destination,
            declined_destination,
        });
    }
    if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
        && x_cnt > 0
    {
        let x_val = match bound_x {
            Some(expr) => lower_value_expr(expr, vocabulary).ok()?,
            None => ValueExpr::chosen_x(),
        };
        let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
        return Some(Instruction::OfferDynamicManaPayment {
            payer,
            fixed,
            extra_generic,
            paid_destination,
            declined_destination,
        });
    }
    None
}

pub(super) fn with_offer_dests(mut op: Instruction, paid: usize, declined: usize) -> Instruction {
    match &mut op {
        Instruction::OfferManaPayment {
            paid_destination,
            declined_destination,
            ..
        }
        | Instruction::OfferDynamicManaPayment {
            paid_destination,
            declined_destination,
            ..
        } => {
            *paid_destination = paid;
            *declined_destination = declined;
        }
        _ => {}
    }
    op
}

pub(super) fn set_offer_paid(op: Option<&mut Instruction>, paid: usize) {
    if let Some(
        Instruction::OfferManaPayment {
            paid_destination, ..
        }
        | Instruction::OfferDynamicManaPayment {
            paid_destination, ..
        },
    ) = op
    {
        *paid_destination = paid;
    }
}

pub(super) fn set_offer_declined(op: Option<&mut Instruction>, declined: usize) {
    if let Some(
        Instruction::OfferManaPayment {
            declined_destination,
            ..
        }
        | Instruction::OfferDynamicManaPayment {
            declined_destination,
            ..
        },
    ) = op
    {
        *declined_destination = declined;
    }
}

pub(super) fn lower_resolution_pay_cost_ops(
    player: ResolvingPlayer,
    cost: &CostAtomAst,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<Instruction>, &'static str> {
    let lower_filter_any = |f: &ObjectFilterAst| -> Result<ObjectFilter, &'static str> {
        let mut inner = f.clone();
        inner.zone = None;
        if inner.is_empty() || inner.any_card {
            Ok(ObjectFilter::any())
        } else {
            lower_object_filter(&inner, vocabulary)
        }
    };
    match cost {
        CostAtomAst::PayLife(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            if player == ResolvingPlayer::Controller
                && let ValueExpr::Literal(n) = &amt
                && let Some(u) = n.to_usize().and_then(|v| u16::try_from(v).ok())
                && u > 0
            {
                return Ok(vec![Instruction::PayControllerLife(u)]);
            }
            Ok(vec![Instruction::PayLife {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::PayEnergy(n) if *n > 0 => {
            if player == ResolvingPlayer::Controller {
                Ok(vec![Instruction::PayControllerEnergy(*n)])
            } else {
                Ok(vec![Instruction::PayEnergy {
                    player,
                    amount: ValueExpr::literal(i64::from(*n)),
                }])
            }
        }
        CostAtomAst::PayEnergyExpr(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            Ok(vec![Instruction::PayEnergy {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::SacrificeSelf => Ok(vec![Instruction::SacrificeStateTriggerSource]),
        CostAtomAst::ExileSelf { .. } => Ok(vec![Instruction::ExileAbilitySource]),
        CostAtomAst::ExertSelf => Ok(vec![Instruction::ExertAbilitySource]),
        CostAtomAst::Behold(filter) => {
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::Behold {
                filter: Box::new(obj),
                optional: false,
            }])
        }
        CostAtomAst::RollDie(sides) => {
            let nz = std::num::NonZeroU16::new(u16::from(*sides)).ok_or("oracle:zero-die-sides")?;
            Ok(vec![Instruction::RollDie {
                player,
                sides: nz,
                count: ValueExpr::literal(1),
                mode: spellwake_engine::program::DieRollMode::Sum,
                bands: Vec::new(),
            }])
        }
        CostAtomAst::DiscardHand => Ok(vec![Instruction::Discard {
            players: AffectedPlayers::One(player),
            amount: ValueExpr::count_hand(player),
        }]),
        CostAtomAst::Discard {
            count,
            filter,
            random,
            up_to: false,
            any_number: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let plain_any = filter.is_empty()
                || (filter.any_card && filter.predicates.is_empty() && filter.flags.0 == 0);
            if *random {
                if plain_any && amt == ValueExpr::literal(1) {
                    return Ok(vec![Instruction::DiscardOneAtRandom(player)]);
                }
                return Err("oracle:unsupported-pay-cost");
            }
            if plain_any {
                Ok(vec![Instruction::Discard {
                    players: AffectedPlayers::One(player),
                    amount: amt,
                }])
            } else {
                let obj = lower_object_filter(filter, vocabulary)?;
                Ok(vec![Instruction::DiscardFiltered {
                    players: AffectedPlayers::One(player),
                    amount: amt,
                    filter: Box::new(obj),
                }])
            }
        }
        CostAtomAst::Sacrifice {
            count,
            filter,
            up_to: false,
            min_one: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ptf = lower_permanent_type_filter(filter, vocabulary)?;
            Ok(vec![Instruction::Sacrifice {
                players: AffectedPlayers::One(player),
                amount: amt,
                filter: ptf,
            }])
        }
        CostAtomAst::SacrificeAll(filter) => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !matches!(
                    filter.controller,
                    None | Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
                )
            {
                return Err("oracle:unsupported-pay-cost");
            }
            let obj = lower_filter_any(filter)?;
            let scoped = match player {
                ResolvingPlayer::Controller => obj.controlled(),
                ResolvingPlayer::UniqueRelatedPlayer => obj.of_related_player(),
                ResolvingPlayer::TargetPlayer => obj.of_target_player(),
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            Ok(vec![Instruction::SacrificeSubject {
                subject: Box::new(EffectSubject::Matching(scoped)),
            }])
        }
        CostAtomAst::Exile {
            count,
            filter,
            face_down: false,
            face_up: _,
            single_graveyard: false,
            any_number: false,
            min_one: false,
            total_mv: None,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let zone = match filter.zone {
                Some(ZoneFilter::YourGraveyard) => CostExileZone::Graveyard,
                Some(ZoneFilter::YourHand) => CostExileZone::Hand,
                None | Some(ZoneFilter::Battlefield) => CostExileZone::Battlefield,
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            if zone == CostExileZone::Graveyard
                && (filter.any_card || filter.any_types.is_empty())
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.flags.0 == 0
                && filter.predicates.is_empty()
                && let ValueExpr::Literal(n) = &amt
                && let Some(nz) = n
                    .to_usize()
                    .and_then(|v| u8::try_from(v).ok())
                    .and_then(NonZeroU8::new)
            {
                return Ok(vec![Instruction::ExileCardsFromGraveyard {
                    player,
                    count: nz,
                }]);
            }
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::ExileFromZone {
                players: AffectedPlayers::One(player),
                zone,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::Exile {
            filter,
            face_down: false,
            face_up: _,
            single_graveyard: false,
            any_number: true,
            min_one,
            total_mv: None,
            ..
        } => {
            let source_zone = match filter.zone {
                Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => SourceZoneFilter::Hand,
                Some(ZoneFilter::YourGraveyard | ZoneFilter::ThatPlayerGraveyard) => {
                    SourceZoneFilter::Graveyard
                }
                Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                    SourceZoneFilter::OpponentGraveyard
                }
                Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                    SourceZoneFilter::AnyGraveyard
                }
                Some(ZoneFilter::YourLibrary) => SourceZoneFilter::YourLibrary,
                Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                None | Some(ZoneFilter::Battlefield) => SourceZoneFilter::Battlefield,
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            let obj = lower_filter_any(filter)?;
            if obj.has_target_player() && player != ResolvingPlayer::TargetPlayer {
                return Err("oracle:unsupported-pay-cost");
            }
            Ok(vec![Instruction::SelectZoneCardsToZone {
                player,
                source_zone,
                filter: Box::new(obj),
                min: ValueExpr::literal(if *min_one { 1 } else { 0 }),
                max: ValueExpr::literal(999),
                destination: SubjectDestination::Exile {
                    delayed_return: None,
                },
                reveal: false,
            }])
        }
        CostAtomAst::BounceToHand(subject) => match subject.as_ref() {
            SubjectRefAst::SelfObject => Ok(vec![Instruction::ReturnSourceToHand]),
            SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let ptf = lower_permanent_type_filter(f, vocabulary)?;
                Ok(vec![Instruction::ReturnControlledPermanents {
                    players: AffectedPlayers::One(player),
                    amount: ValueExpr::literal(1),
                    filter: ptf,
                }])
            }
            _ => Err("oracle:unsupported-pay-cost"),
        },
        CostAtomAst::TapPermanent {
            count,
            filter,
            any_number: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::TapOrUntapControlled {
                players: AffectedPlayers::One(player),
                tap: true,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::UntapPermanent { count, filter } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::TapOrUntapControlled {
                players: AffectedPlayers::One(player),
                tap: false,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::Mill(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            Ok(vec![Instruction::Mill {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::CollectEvidence(_) | CostAtomAst::Blight(_) => {
            Err("oracle:unsupported-pay-cost")
        }
        _ => Err("oracle:unsupported-pay-cost"),
    }
}

pub(super) fn lower_ward_cost_body(
    atoms: &[CostAtomAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ExecutableBody, &'static str> {
    let payer = ResolvingPlayer::UniqueRelatedPlayer;
    if let [CostAtomAst::PayMana(raw)] = atoms {
        if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
            && x_cnt > 0
        {
            let extra_generic = scale_value_expr(ValueExpr::chosen_x(), i64::from(x_cnt));
            return Ok(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::OfferDynamicManaPayment {
                    payer,
                    fixed,
                    extra_generic,
                    paid_destination: 2,
                    declined_destination: 1,
                }],
            ));
        }
        return Err("oracle:ward-non-mana");
    }
    let mut mana_cost: Option<ManaCost> = None;
    let mut non_mana_ops: Vec<Instruction> = Vec::new();
    for atom in atoms {
        if let CostAtomAst::PayMana(raw) = atom {
            let mc = parse_fixed_mana_cost(raw).ok_or("oracle:ward-non-mana")?;
            let entry = mana_cost.get_or_insert_with(ManaCost::free);
            entry.generic = entry.generic.plus(&mc.generic);
            for (k, v) in mc.required {
                let slot = entry.required.entry(k).or_insert_with(|| 0.into());
                *slot = slot.plus(&v);
            }
        } else {
            let ops = lower_resolution_pay_cost_ops(payer, atom, vocabulary, None)
                .map_err(|_| "oracle:ward-non-mana")?;
            non_mana_ops.extend(ops);
        }
    }
    let mut instructions = Vec::new();
    if let Some(mc) = mana_cost {
        if non_mana_ops.is_empty() {
            instructions.push(Instruction::OfferManaPayment {
                payer,
                cost: mc,
                paid_destination: 2,
                declined_destination: 1,
            });
            return Ok(ExecutableBody::new(BodyTarget::None, instructions));
        }
        // Offer mana first; if declined jump directly to CounterWardSource (the instruction after this body).
        let counter_idx = 1 + 1 + non_mana_ops.len() + 1;
        instructions.push(Instruction::OfferManaPayment {
            payer,
            cost: mc,
            paid_destination: 1,
            declined_destination: counter_idx,
        });
    }
    if non_mana_ops.is_empty() {
        return Err("oracle:ward-non-mana");
    }
    if non_mana_ops.len() == 1 {
        instructions.push(Instruction::OptionalEffect { chooser: payer });
    } else {
        instructions.push(Instruction::OptionalSequence {
            chooser: payer,
            instructions: non_mana_ops.len(),
        });
    }
    instructions.extend(non_mana_ops);
    let branch_idx = instructions.len();
    instructions.push(Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
            comparison: Comparison::Less,
            right: ValueExpr::literal(1),
        },
        // Skip CounterWardSource (which caller appends at branch_idx + 1) when paid.
        destination: branch_idx + 2,
    });
    Ok(ExecutableBody::new(BodyTarget::None, instructions))
}

pub(super) fn lower_permanent_type_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentTypeFilter, &'static str> {
    if f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.stat_bounds.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.counter_kind.is_none()
        && f.flags.0 == 0
        && f.predicates.is_empty()
        && f.named.is_none()
        && f.owner.is_none()
        && f.extra_zones.is_empty()
        && !f.any_spell
        && !f.is_ability
    {
        return Ok(PermanentTypeFilter {
            any_types: card_type_mask_to_set(f.any_types)?,
            required_types: BTreeSet::new(),
            forbidden_types: card_type_mask_to_set(f.forbidden_types)?,
            object_filter: None,
        });
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(PermanentTypeFilter {
        any_types: BTreeSet::new(),
        required_types: BTreeSet::new(),
        forbidden_types: BTreeSet::new(),
        object_filter: Some(obj),
    })
}

// Shared search-target binder for SearchLibrary lowering sites in body.rs and instructions.rs.
#[allow(dead_code)]
pub(super) fn bind_search_target(
    builder: &mut BodyBuilder,
    search: &LibrarySearch,
) -> Result<(), &'static str> {
    if search.source == SearchSource::TargetOpponentLibrary {
        builder.bind_target(BodyTarget::SingleOpponent)?;
    } else if search.source == SearchSource::TargetPlayerLibrary
        || search.searcher == ResolvingPlayer::TargetPlayer
        || search.curse_host() == Some(spellwake_engine::program::AuraSearchHost::TargetPlayer)
    {
        builder.bind_target(BodyTarget::SinglePlayer)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SearchLibraryExtra<'a> {
    pub(super) extra_zones: &'a [ZoneFilter],
    pub(super) secondary_filter: Option<&'a ObjectFilterAst>,
    pub(super) secondary_is_or: bool,
    pub(super) where_x: Option<&'a ValueExprAst>,
    pub(super) rest_to_zone: Option<&'a str>,
    pub(super) modifier: Option<&'a spellwake_oracle_compiler::ast::SearchModifierAst>,
}

fn filter_references_chosen_x(f: &ObjectFilterAst) -> bool {
    f.stat_bounds
        .iter()
        .any(|sb| value_expr_references_chosen_x(&sb.value))
}

fn substitute_chosen_x_in_value_expr(
    expr: &ValueExprAst,
    replacement: &ValueExprAst,
) -> (ValueExprAst, bool) {
    match expr {
        ValueExprAst::ChosenX => (replacement.clone(), true),
        ValueExprAst::Add(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Add(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Sub(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Sub(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Mul(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Mul(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::DivConst {
            expr: inner,
            denom,
            rounding,
        } => {
            let (ni, ci) = substitute_chosen_x_in_value_expr(inner, replacement);
            (
                ValueExprAst::DivConst {
                    expr: Box::new(ni),
                    denom: *denom,
                    rounding: *rounding,
                },
                ci,
            )
        }
        ValueExprAst::Pow { base, exp } => {
            let (nb, cb) = substitute_chosen_x_in_value_expr(base, replacement);
            let (ne, ce) = substitute_chosen_x_in_value_expr(exp, replacement);
            (
                ValueExprAst::Pow {
                    base: Box::new(nb),
                    exp: Box::new(ne),
                },
                cb || ce,
            )
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            let (nt, ct) = substitute_chosen_x_in_value_expr(threshold, replacement);
            let (na, ca) = substitute_chosen_x_in_value_expr(actual, replacement);
            (
                ValueExprAst::Shortfall {
                    threshold: Box::new(nt),
                    actual: Box::new(na),
                },
                ct || ca,
            )
        }
        ValueExprAst::AbsDiff(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::AbsDiff(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Max(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Max(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Min(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Min(Box::new(na), Box::new(nb)), ca || cb)
        }
        _ => (expr.clone(), false),
    }
}

fn substitute_chosen_x_in_filter(
    filter: &ObjectFilterAst,
    replacement: &ValueExprAst,
) -> Option<ObjectFilterAst> {
    let mut out = filter.clone();
    let mut changed = false;
    for sb in &mut out.stat_bounds {
        let (nv, c) = substitute_chosen_x_in_value_expr(&sb.value, replacement);
        if c {
            *sb.value = nv;
            changed = true;
        }
    }
    changed.then_some(out)
}

fn extract_single_basic_land_type(f: &ObjectFilterAst) -> Option<LandType> {
    if f.subtypes.len() != 1
        || !(f.any_types.is_empty() || f.any_types == AstCardTypeMask::LAND)
        || !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.controller.is_some()
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.named.is_some()
        || !f.flags.is_empty()
        || !f.predicates.is_empty()
        || f.any_spell
        || f.is_ability
    {
        return None;
    }
    match parse_basic_land_subtype(&f.subtypes[0])? {
        BasicLandType::Plains => Some(LandType::Plains),
        BasicLandType::Island => Some(LandType::Island),
        BasicLandType::Swamp => Some(LandType::Swamp),
        BasicLandType::Mountain => Some(LandType::Mountain),
        BasicLandType::Forest => Some(LandType::Forest),
    }
}

fn search_count_role_supported(role: ResolvingPlayer) -> bool {
    matches!(
        role,
        ResolvingPlayer::Controller
            | ResolvingPlayer::ActivePlayer
            | ResolvingPlayer::DefendingPlayer
    )
}

fn search_count_expr_supported(expr: &ValueExpr) -> bool {
    if expr.uses_target() {
        return false;
    }
    match expr {
        ValueExpr::Literal(v) => *v >= 0.into(),
        ValueExpr::Count {
            zone,
            filter,
            controller_scope,
        } => {
            if filter.has_target_player() || filter.has_related_player() {
                return false;
            }
            if controller_scope.is_some_and(|r| !search_count_role_supported(r)) {
                return false;
            }
            match zone {
                ValueZone::Battlefield | ValueZone::AllGraveyards | ValueZone::Exile(None) => true,
                ValueZone::Hand(r)
                | ValueZone::Graveyard(r)
                | ValueZone::Library(r)
                | ValueZone::Exile(Some(r)) => search_count_role_supported(*r),
            }
        }
        ValueExpr::OpponentMaxCount { filter, .. }
        | ValueExpr::OpponentMinCount { filter, .. }
        | ValueExpr::Aggregate { filter, .. }
        | ValueExpr::DistinctColors { filter } => {
            !filter.has_target_player() && !filter.has_related_player()
        }
        ValueExpr::DistinctManaValuesInGraveyard(r) => search_count_role_supported(*r),
        ValueExpr::PartyCount(_)
        | ValueExpr::CountAttackingController
        | ValueExpr::DomainCount
        | ValueExpr::OpponentsCount
        | ValueExpr::Devotion(_)
        | ValueExpr::SourceState(_) => true,
        ValueExpr::ObjectStat { subject, .. } => matches!(
            subject,
            ValueObjectRef::AbilitySource
                | ValueObjectRef::AttachedHost
                | ValueObjectRef::LastAmassedArmy
                | ValueObjectRef::RevealedTop
        ),
        ValueExpr::CounterCount { subject, .. } => match subject {
            spellwake_engine::program::CounterSubjectRef::AbilitySource
            | spellwake_engine::program::CounterSubjectRef::AttachedHost
            | spellwake_engine::program::CounterSubjectRef::ControllerExperience
            | spellwake_engine::program::CounterSubjectRef::ControllerEnergy
            | spellwake_engine::program::CounterSubjectRef::ControllerPoison => true,
            spellwake_engine::program::CounterSubjectRef::PlayerCounter(r, _) => {
                search_count_role_supported(*r)
            }
            _ => false,
        },
        ValueExpr::PlayerStat(ps) => match ps {
            PlayerStatExpr::PlayerIsActivePlayer(r)
            | PlayerStatExpr::PlayerLife(r)
            | PlayerStatExpr::PlayerHandSize(r)
            | PlayerStatExpr::PlayerLibrarySize(r)
            | PlayerStatExpr::PlayerGraveyardSize(r)
            | PlayerStatExpr::LandsEnteredThisTurn(r)
            | PlayerStatExpr::ActivatedGraveyardAbilityThisTurn(r)
            | PlayerStatExpr::PlayerLifeLostLastTurn(r) => search_count_role_supported(*r),
            PlayerStatExpr::SpellsCastMatchingThisTurn {
                players, filter, ..
            }
            | PlayerStatExpr::SacrificedMatchingThisTurn { players, filter } => {
                let players_ok = match players {
                    AffectedPlayers::One(r) => search_count_role_supported(*r),
                    AffectedPlayers::EachPlayer
                    | AffectedPlayers::EachOtherPlayer
                    | AffectedPlayers::EachOpponent => true,
                };
                players_ok && !filter.has_target_player() && !filter.has_related_player()
            }
            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                filter, controller, ..
            } => {
                controller.is_none_or(search_count_role_supported)
                    && !filter.has_target_player()
                    && !filter.has_related_player()
            }
            PlayerStatExpr::ControlsGreatestStat { player, filter, .. } => {
                search_count_role_supported(*player)
                    && !filter.has_target_player()
                    && !filter.has_related_player()
            }
            PlayerStatExpr::DamageDealtThisTurn { subject, .. }
            | PlayerStatExpr::DamageReceivedThisTurn { subject, .. } => matches!(
                subject,
                ValueObjectRef::AbilitySource
                    | ValueObjectRef::AttachedHost
                    | ValueObjectRef::LastAmassedArmy
                    | ValueObjectRef::RevealedTop
            ),
            _ => true,
        },
        ValueExpr::EventValue(kind) => matches!(
            kind,
            EventValueKind::ChosenX
                | EventValueKind::KickerCount(_)
                | EventValueKind::EnteredKickerX
                | EventValueKind::EnteredKickerCount(_)
                | EventValueKind::ManaSpentToCast
                | EventValueKind::ColorsSpentToCast
                | EventValueKind::ManaTypeSpentToCast(_)
                | EventValueKind::SnowManaSpentToCast
                | EventValueKind::TreasureManaSpentToCast
                | EventValueKind::CreatureManaSpentToCast
                | EventValueKind::LastDieRoll
                | EventValueKind::ChosenNumber
                | EventValueKind::AdditionalCostPaid
                | EventValueKind::DamagePreventedThisWay
        ),
        ValueExpr::Scale {
            expr: inner,
            factor,
        } => factor.is_positive() && search_count_expr_supported(inner),
        ValueExpr::DivConst { expr: inner, .. } => search_count_expr_supported(inner),
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            search_count_expr_supported(a) && search_count_expr_supported(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            search_count_expr_supported(left)
                && search_count_expr_supported(right)
                && search_count_expr_supported(then_value)
                && search_count_expr_supported(else_value)
        }
        ValueExpr::CountOtherSharingSourceSubtype
        | ValueExpr::AttachedCount(_)
        | ValueExpr::ObjectMatches { .. } => false,
    }
}

fn search_filter_is_static_evaluable(filter: ObjectFilter) -> bool {
    let clause_ok = |c: ObjectClause| {
        !matches!(
            c.power,
            PowerFilter::AtMostX
                | PowerFilter::EqualX
                | PowerFilter::AtLeastX
                | PowerFilter::AtMostControlledCreatureCount
                | PowerFilter::CompareAnchor { .. }
        ) && !matches!(
            c.toughness,
            ToughnessFilter::AtMostX
                | ToughnessFilter::EqualX
                | ToughnessFilter::AtLeastX
                | ToughnessFilter::CompareAnchor { .. }
        ) && !matches!(
            c.mana_value,
            ManaValueFilter::AtMostX
                | ManaValueFilter::EqualX
                | ManaValueFilter::AtLeastX
                | ManaValueFilter::CompareAnchor { .. }
        )
    };
    let (a, b) = filter.clauses();
    clause_ok(a) && b.is_none_or(clause_ok)
}

fn extract_search_mana_value(
    filter: &ObjectFilterAst,
) -> Result<(Option<SearchManaValue>, Option<ObjectFilterAst>), &'static str> {
    let mv_indices: Vec<usize> = filter
        .stat_bounds
        .iter()
        .enumerate()
        .filter_map(|(idx, sb)| (sb.stat == StatKind::ManaValue).then_some(idx))
        .collect();
    if mv_indices.is_empty() {
        let exact_indices: Vec<usize> = filter
            .predicates
            .iter()
            .enumerate()
            .filter_map(|(idx, pred)| {
                matches!(pred, FilterPredicateAst::ExactManaCostOneOf(_)).then_some(idx)
            })
            .collect();
        if exact_indices.len() != 1 {
            return Ok((None, None));
        }
        let idx = exact_indices[0];
        let FilterPredicateAst::ExactManaCostOneOf(raw_costs) = &filter.predicates[idx] else {
            return Ok((None, None));
        };
        if raw_costs.is_empty() {
            return Ok((None, None));
        }
        let mut costs = Vec::with_capacity(raw_costs.len());
        for raw in raw_costs {
            if raw.trim().is_empty() {
                return Ok((None, None));
            }
            let Some(cost) = parse_fixed_mana_cost(raw) else {
                return Ok((None, None));
            };
            costs.push(cost);
        }
        let mut stripped = filter.clone();
        stripped.predicates.remove(idx);
        return Ok((
            Some(SearchManaValue::ExactManaCostOneOf(costs)),
            Some(stripped),
        ));
    }
    if mv_indices.len() != 1 {
        return Ok((None, None));
    }
    let idx = mv_indices[0];
    let sb = &filter.stat_bounds[idx];
    let mv = match (sb.op, sb.value.as_ref()) {
        (CompareOp::Le, ValueExprAst::Const(v)) if *v >= 0 => {
            SearchManaValue::AtMost(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?)
        }
        (CompareOp::Lt, ValueExprAst::Const(v)) if *v > 0 => SearchManaValue::AtMost(
            v.checked_sub(1)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("oracle:mv-ovf")?,
        ),
        (CompareOp::Ge, ValueExprAst::Const(v)) if *v >= 0 => {
            SearchManaValue::AtLeast(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?)
        }
        (CompareOp::Gt, ValueExprAst::Const(v)) if *v >= 0 => SearchManaValue::AtLeast(
            v.checked_add(1)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("oracle:mv-ovf")?,
        ),
        (CompareOp::Eq, ValueExprAst::Const(v)) if *v >= 0 => {
            SearchManaValue::Exact(i64::from(*v).into())
        }
        (CompareOp::Le, ValueExprAst::ChosenX) => SearchManaValue::AtMostAnnouncedX,
        (_, ValueExprAst::ChosenX) => return Err("oracle:unsupported-search-mv"),
        (
            CompareOp::Eq,
            ValueExprAst::CounterCount {
                subject,
                counter_kind,
            },
        ) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let ck = parse_counter_kind(counter_kind)
                .filter(|c| c.is_permanent())
                .ok_or("oracle:unsupported-search-mv")?;
            SearchManaValue::EqualToSourceCounters(ck)
        }
        _ => return Ok((None, None)),
    };
    let mut stripped = filter.clone();
    stripped.stat_bounds.remove(idx);
    Ok((Some(mv), Some(stripped)))
}

// Internal lowering helper taking the orthogonal fields of `InstructionAst::SearchLibrary` plus lowering flags.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_library_search_ext(
    library: &str,
    count: &ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    vocabulary: &CreatureTypeVocabulary,
    optional_search: bool,
    up_to: bool,
    any_number: bool,
    extra: &SearchLibraryExtra<'_>,
) -> Result<LibrarySearch, &'static str> {
    match extra.modifier {
        None | Some(spellwake_oracle_compiler::ast::SearchModifierAst::Additional) => {}
        Some(spellwake_oracle_compiler::ast::SearchModifierAst::InsteadOfOne) => {
            return Err("oracle:unsupported-search-modifier");
        }
    }

    let lib_low = library.trim().to_ascii_lowercase();
    let (searcher, mut source) = match lib_low.as_str() {
        "your" | "own" | "" => (ResolvingPlayer::Controller, SearchSource::OwnLibrary),
        "your library and/or graveyard"
        | "your graveyard and/or library"
        | "your library and graveyard"
        | "your library, hand, and/or graveyard"
        | "your graveyard and library" => (
            ResolvingPlayer::Controller,
            SearchSource::OwnLibraryOrGraveyard,
        ),
        "target opponent" | "target opponent's" | "target opponent's library" => (
            ResolvingPlayer::Controller,
            SearchSource::TargetOpponentLibrary,
        ),
        // Callers (body.rs and instructions.rs) rewrite this sentinel via
        // `lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)`
        // so non-trigger spells and activations resolve to a bound target or fail closed
        // instead of leaking `UnboundResolvingPlayer`.
        "that player" | "that player's" | "that player's library" | "their" | "their library" => (
            ResolvingPlayer::UniqueRelatedPlayer,
            SearchSource::OwnLibrary,
        ),
        "target player" | "target player's" | "target player's library" => (
            ResolvingPlayer::Controller,
            SearchSource::TargetPlayerLibrary,
        ),
        _ => return Err("oracle:unsupported-search-source"),
    };
    if !extra.extra_zones.is_empty() {
        if searcher == ResolvingPlayer::Controller
            && source == SearchSource::OwnLibrary
            && extra.extra_zones == [ZoneFilter::YourGraveyard]
        {
            source = SearchSource::OwnLibraryOrGraveyard;
        } else {
            return Err("oracle:unsupported-search-source");
        }
    }

    let mut substituted_filter_owned: Option<ObjectFilterAst> = None;
    let mut substituted_secondary_owned: Option<ObjectFilterAst> = None;
    let mut where_x_count: Option<SearchCount> = None;
    let mut dynamic_count_ast: Option<&ValueExprAst> = None;
    if let Some(where_expr) = extra.where_x {
        if any_number || value_expr_references_chosen_x(where_expr) {
            return Err("oracle:unsupported-search-where-x");
        }
        if matches!(count, ValueExprAst::ChosenX) {
            if filter_references_chosen_x(filter)
                || extra
                    .secondary_filter
                    .is_some_and(filter_references_chosen_x)
            {
                return Err("oracle:unsupported-search-where-x");
            }
            let is_more_lands = matches!(
                where_expr,
                ValueExprAst::CountPlayersMatching { predicate }
                    if matches!(
                        predicate.trim().to_ascii_lowercase().as_str(),
                        "the number of players who control more lands than you"
                            | "players who control more lands than you"
                            | "the number of opponents who control more lands than you"
                            | "opponents who control more lands than you"
                    )
            );
            if is_more_lands {
                where_x_count = Some(SearchCount::UpToPlayersWithMoreLands);
            } else {
                dynamic_count_ast = Some(where_expr);
            }
        } else {
            let mut substituted = false;
            if let Some(sub_f) = substitute_chosen_x_in_filter(filter, where_expr) {
                substituted_filter_owned = Some(sub_f);
                substituted = true;
            }
            if let Some(sec) = extra.secondary_filter
                && let Some(sub_s) = substitute_chosen_x_in_filter(sec, where_expr)
            {
                substituted_secondary_owned = Some(sub_s);
                substituted = true;
            }
            if !substituted {
                return Err("oracle:unsupported-search-where-x");
            }
        }
    }
    let effective_filter = substituted_filter_owned.as_ref().unwrap_or(filter);
    let effective_secondary = substituted_secondary_owned
        .as_ref()
        .or(extra.secondary_filter);

    let (extracted_mana_value, stripped_filter_owned) = if effective_secondary.is_none() {
        extract_search_mana_value(effective_filter)?
    } else {
        (None, None)
    };
    let matcher_filter = stripped_filter_owned.as_ref().unwrap_or(effective_filter);

    let (matcher, pair_constraint, pair_count_override) = if let Some(sec) = effective_secondary {
        if extra.secondary_is_or {
            let first = lower_object_filter(effective_filter, vocabulary)?;
            let second = lower_object_filter(sec, vocabulary)?;
            let (ObjectFilter::Clause(c1), ObjectFilter::Clause(c2)) = (first, second) else {
                return Err("oracle:unsupported-secondary-search-filter");
            };
            let combined = ObjectFilter::Either(c1, c2);
            if !search_filter_is_static_evaluable(combined) {
                return Err("oracle:unsupported-search-mv");
            }
            (SearchCardMatcher::Filtered(Box::new(combined)), None, None)
        } else {
            if any_number
                || where_x_count.is_some()
                || dynamic_count_ast.is_some()
                || !matches!(count, ValueExprAst::Const(1 | 2))
            {
                return Err("oracle:unsupported-secondary-search-filter");
            }
            let (Some(sub1), Some(sub2)) = (
                extract_single_basic_land_type(effective_filter),
                extract_single_basic_land_type(sec),
            ) else {
                return Err("oracle:unsupported-secondary-search-filter");
            };
            if sub1 == sub2 {
                return Err("oracle:unsupported-secondary-search-filter");
            }
            (
                SearchCardMatcher::LandSubtypes {
                    subtypes: BTreeSet::from([sub1, sub2]),
                    require_land_type: true,
                },
                Some(
                    spellwake_engine::program::SearchPairConstraint::DistinctLandTypes(sub1, sub2),
                ),
                Some(SearchCount::UpTo(2)),
            )
        }
    } else {
        let m = lower_search_matcher(matcher_filter, vocabulary)?;
        if let SearchCardMatcher::Filtered(ref obj) = m
            && !search_filter_is_static_evaluable(**obj)
        {
            return Err("oracle:unsupported-search-mv");
        }
        (m, None, None)
    };

    let is_unrestricted = matcher == SearchCardMatcher::Any && extracted_mana_value.is_none();
    let effective_count = dynamic_count_ast.unwrap_or(count);
    let search_count = if let Some(pair_cnt) = pair_count_override {
        pair_cnt
    } else if let Some(wx_cnt) = where_x_count {
        wx_cnt
    } else if any_number {
        SearchCount::UpTo(u8::MAX)
    } else if let ValueExprAst::Const(n) = effective_count {
        let n_u8 = u8::try_from(*n).map_err(|_| "oracle:search-count-ovf")?;
        if n_u8 == 0 {
            return Err("oracle:search-zero");
        }
        if is_unrestricted && !optional_search && !up_to {
            SearchCount::Exact(NonZeroU8::new(n_u8).ok_or("oracle:search-zero")?)
        } else {
            SearchCount::UpTo(n_u8)
        }
    } else if up_to || optional_search || !is_unrestricted {
        let expr = lower_value_expr(effective_count, vocabulary)
            .map_err(|_| "oracle:dynamic-search-count")?;
        if !search_count_expr_supported(&expr) {
            return Err("oracle:dynamic-search-count");
        }
        SearchCount::UpToExpr(Box::new(expr))
    } else {
        return Err("oracle:dynamic-search-count");
    };

    let dest_low = to_zone.to_ascii_lowercase();
    if dest_low.contains("under your control") && searcher != ResolvingPlayer::Controller {
        return Err("oracle:unsupported-search-controller");
    }
    if (dest_low.contains("under their control")
        || dest_low.contains("under that player's control"))
        && searcher == ResolvingPlayer::Controller
    {
        return Err("oracle:unsupported-search-controller");
    }
    let mut destination = if dest_low.contains("hand") {
        SearchDestination::Hand
    } else if dest_low.contains("graveyard") {
        SearchDestination::Graveyard
    } else if dest_low.contains("battlefield") {
        SearchDestination::Battlefield {
            tapped: dest_low.contains("tapped"),
            post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
        }
    } else if dest_low.contains("top") {
        SearchDestination::LibraryTop
    } else if dest_low.contains("exile") {
        SearchDestination::Exile {
            link: SearchExileLink::None,
        }
    } else {
        return Err("oracle:unsupported-search-dest");
    };
    if let Some(rest) = extra.rest_to_zone {
        let is_count_two = matches!(search_count, SearchCount::UpTo(2))
            || matches!(search_count, SearchCount::Exact(n) if n.get() == 2);
        if !is_count_two {
            return Err("oracle:unsupported-search-dest");
        }
        let rest_norm = rest
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_lowercase();
        let rest_is_hand = if searcher == ResolvingPlayer::Controller {
            matches!(
                rest_norm.as_str(),
                "hand" | "your hand" | "into your hand" | "in your hand"
            )
        } else {
            matches!(
                rest_norm.as_str(),
                "hand"
                    | "their hand"
                    | "into their hand"
                    | "that player's hand"
                    | "into that player's hand"
            )
        };
        let rest_is_graveyard = if searcher == ResolvingPlayer::Controller {
            matches!(
                rest_norm.as_str(),
                "graveyard" | "your graveyard" | "into your graveyard"
            )
        } else {
            matches!(
                rest_norm.as_str(),
                "graveyard"
                    | "their graveyard"
                    | "into their graveyard"
                    | "that player's graveyard"
                    | "into that player's graveyard"
            )
        };
        destination = match destination {
            SearchDestination::Battlefield {
                tapped: true,
                post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
            } if rest_is_hand => SearchDestination::SplitBattlefieldTappedAndHand,
            SearchDestination::Hand if rest_is_graveyard => {
                SearchDestination::SplitHandAndGraveyard
            }
            _ => return Err("oracle:unsupported-search-dest"),
        };
    }
    if matches!(search_count, SearchCount::UpToExpr(_))
        && !matches!(
            destination,
            SearchDestination::Hand
                | SearchDestination::Graveyard
                | SearchDestination::Exile { .. }
                | SearchDestination::Battlefield {
                    post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
                    ..
                }
        )
    {
        return Err("oracle:dynamic-search-count");
    }

    let ls = LibrarySearch {
        searcher,
        source,
        matcher,
        mana_value: extracted_mana_value,
        count: search_count,
        pair_constraint,
        destination,
        reveal,
    };
    Ok(ls)
}

pub(super) fn lower_search_matcher(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SearchCardMatcher, &'static str> {
    if f.owner.is_some() {
        return Err("oracle:unsupported-search-filter");
    }
    if let Some(named) = &f.named {
        let build_extra =
            |base: &ObjectFilterAst| -> Result<Option<Box<ObjectFilter>>, &'static str> {
                let mut stripped = base.clone();
                stripped.named = None;
                stripped.zone = None;
                if is_unrestricted_filter(&stripped) {
                    Ok(None)
                } else {
                    Ok(Some(Box::new(lower_object_filter(&stripped, vocabulary)?)))
                }
            };
        return match named {
            NameFilterAst::Exact(n) => {
                let card_name = EnglishCardName::new(n.clone())
                    .map_err(|_| "oracle:unsupported-search-named")?;
                Ok(SearchCardMatcher::AnyOfNames(vec![card_name]))
            }
            NameFilterAst::SelfName => Ok(SearchCardMatcher::SameNameAs {
                reference: MoveSubjectRef::AbilitySource,
                extra: build_extra(f)?,
            }),
            NameFilterAst::SameAsReferenced => Ok(SearchCardMatcher::SameNameAs {
                reference: MoveSubjectRef::TriggeringObject,
                extra: build_extra(f)?,
            }),
            NameFilterAst::SameAs(ref_subj) => {
                let reference = match ref_subj.as_ref() {
                    SubjectRefAst::SelfObject => MoveSubjectRef::AbilitySource,
                    SubjectRefAst::TriggerSubject => MoveSubjectRef::TriggeringObject,
                    SubjectRefAst::AttachedHost(_) => MoveSubjectRef::AttachedHost,
                    SubjectRefAst::Anaphora(_) => MoveSubjectRef::TriggeringObject,
                    _ => return Err("oracle:unsupported-search-named"),
                };
                Ok(SearchCardMatcher::SameNameAs {
                    reference,
                    extra: build_extra(f)?,
                })
            }
        };
    }
    if (f.flags.0 & AstFilterFlags::MATCHES_NAMED_AGENDA.0) != 0 {
        let mut stripped = f.clone();
        stripped.flags = AstFilterFlags(stripped.flags.0 & !AstFilterFlags::MATCHES_NAMED_AGENDA.0);
        stripped.zone = None;
        let extra = if is_unrestricted_filter(&stripped) {
            None
        } else {
            Some(Box::new(lower_object_filter(&stripped, vocabulary)?))
        };
        return Ok(SearchCardMatcher::SameNameAs {
            reference: MoveSubjectRef::TriggeringObject,
            extra,
        });
    }
    let has_complex_predicates = match f.predicates.as_slice() {
        [] => false,
        [FilterPredicateAst::SubtypesMatchAny] => {
            f.subtypes.is_empty() || !f.required_supertypes.is_empty()
        }
        _ => true,
    };
    let has_complex = !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !f.forbidden_supertypes.is_empty()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.stat_bounds.is_empty()
        || has_complex_predicates
        || f.controller.is_some()
        || f.owner.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || f.combat.is_some()
        || f.counter_kind.is_some()
        || !f.flags.is_empty();
    if !has_complex {
        if f.any_card
            && f.any_types.is_empty()
            && f.forbidden_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Any);
        }
        if f.forbidden_types == AstCardTypeMask::LAND
            && f.any_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Nonland);
        }
        if f.required_supertypes == AstSupertypeMask::BASIC
            && f.any_types == AstCardTypeMask::LAND
            && f.subtypes.is_empty()
            && f.forbidden_types.is_empty()
        {
            return Ok(SearchCardMatcher::BasicLand);
        }
        if f.required_supertypes.is_empty() && f.forbidden_types.is_empty() {
            if !f.subtypes.is_empty() {
                if f.subtypes.len() == 1 {
                    let st = &f.subtypes[0];
                    if st.eq_ignore_ascii_case("equipment") {
                        return Ok(SearchCardMatcher::Equipment);
                    }
                    if let Some(ct) = vocabulary.resolve_label(st) {
                        return Ok(SearchCardMatcher::CreatureSubtype(ct));
                    }
                }
                let mut lands = BTreeSet::new();
                let mut all_lands = true;
                for st in &f.subtypes {
                    if let Some(lt) = parse_land_subtype(st) {
                        lands.insert(lt);
                    } else {
                        all_lands = false;
                        break;
                    }
                }
                if all_lands
                    && !lands.is_empty()
                    && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::LAND)
                {
                    return Ok(SearchCardMatcher::LandSubtypes {
                        subtypes: lands,
                        require_land_type: f.any_types.0 & AstCardTypeMask::LAND.0 != 0,
                    });
                }
            } else {
                let types = card_type_mask_to_set(f.any_types)?;
                if !types.is_empty() {
                    return Ok(SearchCardMatcher::CardTypes(types));
                }
                return Ok(SearchCardMatcher::Any);
            }
        }
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(SearchCardMatcher::Filtered(Box::new(obj)))
}

fn uppercase_mana_braces(raw: &str) -> String {
    let mut brace_norm = String::with_capacity(raw.len());
    let mut in_brace = false;
    for ch in raw.chars() {
        if ch == '{' {
            in_brace = true;
            brace_norm.push(ch);
        } else if ch == '}' {
            in_brace = false;
            brace_norm.push(ch);
        } else if in_brace {
            brace_norm.push(ch.to_ascii_uppercase());
        } else {
            brace_norm.push(ch);
        }
    }
    brace_norm
}

pub(super) fn normalize_token_ability_text(raw: &str, tok_name: &str) -> String {
    let mut s = raw
        .trim()
        .trim_end_matches([',', ';'])
        .trim()
        .replace("this token's", "~'s")
        .replace("This token's", "~'s")
        .replace("this token", "~")
        .replace("This token", "~")
        .replace("this creature's", "~'s")
        .replace("This creature's", "~'s")
        .replace("this creature", "~")
        .replace("This creature", "~")
        .replace("this permanent's", "~'s")
        .replace("This permanent's", "~'s")
        .replace("this permanent", "~")
        .replace("This permanent", "~")
        .replace("this artifact's", "~'s")
        .replace("This artifact's", "~'s")
        .replace("this artifact", "~")
        .replace("This artifact", "~")
        .replace("this enchantment's", "~'s")
        .replace("This enchantment's", "~'s")
        .replace("this enchantment", "~")
        .replace("This enchantment", "~")
        .replace("this land's", "~'s")
        .replace("This land's", "~'s")
        .replace("this land", "~")
        .replace("This land", "~")
        .replace("this equipment's", "~'s")
        .replace("This equipment's", "~'s")
        .replace("this equipment", "~")
        .replace("This equipment", "~")
        .replace("this aura's", "~'s")
        .replace("This aura's", "~'s")
        .replace("this aura", "~")
        .replace("This aura", "~")
        .replace("this vehicle's", "~'s")
        .replace("This vehicle's", "~'s")
        .replace("this vehicle", "~")
        .replace("This vehicle", "~")
        .replace("this planeswalker's", "~'s")
        .replace("This planeswalker's", "~'s")
        .replace("this planeswalker", "~")
        .replace("This planeswalker", "~")
        .replace("this saga's", "~'s")
        .replace("This saga's", "~'s")
        .replace("this saga", "~")
        .replace("This saga", "~")
        .replace("this battle's", "~'s")
        .replace("This battle's", "~'s")
        .replace("this battle", "~")
        .replace("This battle", "~");
    let tok_low = tok_name.trim().to_ascii_lowercase();
    if !tok_low.is_empty() && tok_low != "token" {
        // Replace case-insensitive token name occurrences with "~"
        let s_low = s.to_ascii_lowercase();
        let mut out = String::with_capacity(s.len());
        let mut cursor = 0usize;
        while let Some(rel) = s_low[cursor..].find(&tok_low) {
            let start = cursor + rel;
            let end = start + tok_low.len();
            let before_ok = start == 0 || !s.as_bytes()[start - 1].is_ascii_alphanumeric();
            let after_ok = end == s.len() || !s.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                out.push_str(&s[cursor..start]);
                out.push('~');
            } else {
                out.push_str(&s[cursor..end]);
            }
            cursor = end;
        }
        out.push_str(&s[cursor..]);
        s = out;
    }
    // Uppercase mana/tap/untap symbols inside braces (`{t}` -> `{T}`, `{w}` -> `{W}`, `{w/u}` -> `{W/U}`, etc.)
    let brace_norm = uppercase_mana_braces(&s);
    // Sentence-case the start and after `. ` or `: ` so `oracle_compiler`'s capitalized clause headers match.
    let mut result = String::with_capacity(brace_norm.len());
    let mut cap_next = true;
    let mut prev_punct = false;
    for ch in brace_norm.chars() {
        if cap_next && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            cap_next = false;
            prev_punct = false;
        } else {
            result.push(ch);
            if ch == '.' || ch == ':' || ch == '\n' {
                prev_punct = true;
                if ch == '\n' {
                    cap_next = true;
                }
            } else if prev_punct && ch == ' ' {
                cap_next = true;
                prev_punct = false;
            } else if !ch.is_ascii_whitespace() {
                prev_punct = false;
                if ch == '~' || ch == '{' {
                    cap_next = false;
                }
            }
        }
    }
    result
}

fn token_protection_quality_starts_with(right_low: &str) -> bool {
    let first = right_low.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "from"
            | "white"
            | "blue"
            | "black"
            | "red"
            | "green"
            | "colorless"
            | "multicolored"
            | "monocolored"
            | "everything"
            | "all"
            | "each"
            | "artifact"
            | "artifacts"
            | "creature"
            | "creatures"
            | "enchantment"
            | "enchantments"
            | "instant"
            | "instants"
            | "sorcery"
            | "sorceries"
            | "land"
            | "lands"
            | "planeswalker"
            | "planeswalkers"
            | "battle"
            | "battles"
            | "triggered"
            | "activated"
    )
}

/// Split a token keyword list on commas, semicolons, and conjunctions without tearing apart
/// `protection from X and from Y`, `hexproof from X and from Y`, `protection from X and Y`,
/// or `activated and triggered abilities`.
fn split_token_keyword_list(raw: &str) -> Vec<String> {
    fn strip_dangling_conj(input: &str) -> &str {
        let mut cur = input.trim().trim_matches(['.', ',', ';']).trim();
        loop {
            let prev = cur;
            if let Some(rest) = cur
                .strip_prefix("and ")
                .or_else(|| cur.strip_prefix("And "))
                .or_else(|| cur.strip_prefix("with "))
                .or_else(|| cur.strip_prefix("With "))
            {
                cur = rest.trim();
            }
            if let Some(rest) = cur
                .strip_suffix(" and")
                .or_else(|| cur.strip_suffix(" And"))
            {
                cur = rest.trim();
            }
            if cur.eq_ignore_ascii_case("and") || cur.eq_ignore_ascii_case("with") {
                return "";
            }
            if cur == prev {
                break;
            }
        }
        cur
    }

    let mut coarse: Vec<String> = Vec::new();
    for seg in raw.split([',', ';']) {
        let s = strip_dangling_conj(seg);
        if s.is_empty() {
            continue;
        }
        let s_low = s.to_ascii_lowercase();
        if s_low.starts_with("from ")
            && let Some(prev) = coarse.last_mut()
        {
            let prev_low = prev.to_ascii_lowercase();
            if prev_low.contains("protection from ") || prev_low.contains("hexproof from ") {
                prev.push_str(" and ");
                prev.push_str(s);
                continue;
            }
        }
        coarse.push(s.to_string());
    }

    let mut out = Vec::new();
    for chunk in coarse {
        let mut remaining = chunk.as_str();
        let mut search_from = 0usize;
        while let Some(rel) = remaining[search_from..].to_ascii_lowercase().find(" and ") {
            let idx = search_from + rel;
            let left = strip_dangling_conj(&remaining[..idx]);
            let right = strip_dangling_conj(&remaining[idx + 5..]);
            let left_low = left.to_ascii_lowercase();
            let right_low = right.to_ascii_lowercase();
            let keep_joined = right_low.starts_with("from ")
                || (left_low.ends_with("activated") && right_low.starts_with("triggered"))
                || ((left_low.contains("protection from ") || left_low.contains("hexproof from "))
                    && token_protection_quality_starts_with(&right_low));
            if keep_joined {
                search_from = idx + 5;
            } else {
                if !left.is_empty() {
                    out.push(left.to_string());
                }
                remaining = right;
                search_from = 0;
            }
        }
        let tail = strip_dangling_conj(remaining);
        if !tail.is_empty() {
            out.push(tail.to_string());
        }
    }
    out
}

fn parse_direct_token_keyword_ast(trimmed: &str) -> Option<KeywordAst> {
    let clean = trimmed
        .trim()
        .trim_start_matches("with ")
        .trim_start_matches("With ")
        .trim()
        .trim_matches(['.', ',', ';'])
        .trim();
    if clean.is_empty() {
        return None;
    }
    let low = clean.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "decayed"
            | "prowess"
            | "exalted"
            | "extort"
            | "flanking"
            | "battle cry"
            | "melee"
            | "dethrone"
            | "training"
            | "ingest"
            | "evolve"
            | "mentor"
            | "undying"
            | "persist"
            | "myriad"
            | "soulbond"
            | "exploit"
            | "cascade"
            | "sunburst"
            | "ravenous"
            | "unleash"
            | "riot"
            | "provoke"
            | "ascend"
            | "increment"
            | "living weapon"
            | "for mirrodin!"
            | "job select"
            | "station"
            | "vanishing"
    ) {
        return Some(KeywordAst {
            name: low,
            parameter: None,
            desugared_rule: None,
        });
    }
    if low.starts_with("protection from ") {
        let param = clean["protection ".len()..].trim().to_string();
        return Some(KeywordAst {
            name: "protection".to_string(),
            parameter: Some(param),
            desugared_rule: None,
        });
    }
    if low.starts_with("hexproof from ") {
        let param = clean["hexproof ".len()..].trim().to_string();
        return Some(KeywordAst {
            name: "hexproof".to_string(),
            parameter: Some(param),
            desugared_rule: None,
        });
    }
    if let Some(rest) = low
        .strip_prefix("ward ")
        .or_else(|| low.strip_prefix("ward\u{2014}"))
        .or_else(|| low.strip_prefix("ward\u{2013}"))
        .or_else(|| low.strip_prefix("ward-"))
    {
        let offset = clean.len().saturating_sub(rest.len());
        let param_raw = clean[offset..]
            .trim()
            .trim_start_matches([':', '-', '\u{2014}', '\u{2013}'])
            .trim();
        if !param_raw.is_empty() {
            return Some(KeywordAst {
                name: "ward".to_string(),
                parameter: Some(uppercase_mana_braces(param_raw)),
                desugared_rule: None,
            });
        }
    }
    for kw_name in [
        "cumulative upkeep",
        "level up",
        "annihilator",
        "bloodthirst",
        "firebending",
        "reconfigure",
        "afterlife",
        "fabricate",
        "poisonous",
        "soulshift",
        "vanishing",
        "affinity",
        "landwalk",
        "mobilize",
        "afflict",
        "bushido",
        "fortify",
        "modular",
        "outlast",
        "rampage",
        "devour",
        "fading",
        "frenzy",
        "renown",
        "saddle",
        "equip",
        "graft",
        "toxic",
        "crew",
        "echo",
    ] {
        if let Some(after) = low.strip_prefix(kw_name) {
            let sep_ok = after.starts_with(' ')
                || after.starts_with('\u{2014}')
                || after.starts_with('\u{2013}')
                || after.starts_with('-');
            if sep_ok {
                let offset = clean.len().saturating_sub(after.len());
                let param_raw = clean[offset..]
                    .trim()
                    .trim_start_matches([':', '-', '\u{2014}', '\u{2013}'])
                    .trim();
                if !param_raw.is_empty() {
                    return Some(KeywordAst {
                        name: kw_name.to_string(),
                        parameter: Some(uppercase_mana_braces(param_raw)),
                        desugared_rule: None,
                    });
                }
            }
        }
    }
    None
}

fn is_direct_token_keyword(trimmed: &str) -> bool {
    let clean = trimmed
        .trim()
        .trim_start_matches("with ")
        .trim_start_matches("With ")
        .trim()
        .trim_matches(['.', ',', ';'])
        .trim();
    if clean.is_empty() {
        return false;
    }
    parse_direct_token_keyword_ast(clean).is_some() || lower_keyword_ability_name(clean).is_some()
}

fn try_lower_token_keyword_direct(
    trimmed: &str,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    tok_ids: &mut LoweringIds,
) -> bool {
    let clean = trimmed
        .trim()
        .trim_start_matches("with ")
        .trim_start_matches("With ")
        .trim()
        .trim_matches(['.', ',', ';'])
        .trim();
    if clean.is_empty() {
        return true;
    }
    if let Some(kw_ast) = parse_direct_token_keyword_ast(clean) {
        let empty_face = FaceAst::default();
        let mut probe = program.clone();
        if lower_keyword(
            &kw_ast,
            0,
            &empty_face,
            &mut probe,
            vocabulary,
            tok_ids,
            false,
        )
        .is_ok()
        {
            *program = probe;
            return true;
        }
        return false;
    }
    if let Some(kw) = lower_keyword_ability_name(clean) {
        program.printed_keywords.insert(kw);
        return true;
    }
    false
}

#[derive(Clone, Copy)]
struct TokenCompileTarget<'a> {
    name: &'a str,
    type_line: &'a str,
    is_aura: bool,
}

fn lower_token_text_via_compile_card(
    raw: &str,
    target: TokenCompileTarget<'_>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    tok_ids: &mut LoweringIds,
    compiler_vocab: &Vocabulary,
) -> Result<(), &'static str> {
    let norm_part = normalize_token_ability_text(raw, target.name);
    let parsed_kw = compile_card(
        &CardInput {
            oracle_id: "",
            name: target.name,
            layout: "normal",
            faces: vec![FaceInput {
                name: target.name,
                type_line: target.type_line,
                oracle_text: &norm_part,
            }],
        },
        compiler_vocab,
    );
    if !parsed_kw.strictly_compiled {
        return Err("oracle:unsupported-granted-keyword");
    }
    let Some(face_ast) = parsed_kw.faces.first() else {
        return Err("oracle:unsupported-granted-keyword");
    };
    if !face_ast.spell_effects.is_empty()
        || (face_ast.keywords.is_empty()
            && face_ast.statics.is_empty()
            && face_ast.replacements.is_empty()
            && face_ast.abilities.is_empty())
    {
        return Err("oracle:unsupported-granted-keyword");
    }
    lower_token_face_ast(face_ast, target.is_aura, program, vocabulary, tok_ids)
}

fn lower_token_face_ast(
    face_ast: &FaceAst,
    is_aura: bool,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    tok_ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if is_aura
        && program.permanent().and_then(|d| d.attachment).is_none()
        && face_ast
            .statics
            .iter()
            .any(|st| matches!(st, StaticAbilityAst::AttachmentRestriction { .. }))
    {
        let restriction = find_aura_restriction(&face_ast.statics, vocabulary)?;
        if let ProgramKind::Permanent(def) = &mut program.kind {
            def.attachment = Some(AttachmentKind::Aura(restriction));
        }
    }
    for (k_idx, kw_ast) in face_ast.keywords.iter().enumerate() {
        lower_keyword(kw_ast, k_idx, face_ast, program, vocabulary, tok_ids, false)?;
    }
    for st in &face_ast.statics {
        lower_static(st, program, vocabulary, tok_ids, false)?;
    }
    for rep in &face_ast.replacements {
        lower_replacement(rep, program, vocabulary, tok_ids)?;
    }
    for ab in &face_ast.abilities {
        lower_ability(ab, program, vocabulary, tok_ids)?;
    }
    Ok(())
}

pub(super) fn lower_token_spec(
    spec: &TokenSpecAst,
    recipients: TokenRecipients,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    bound_x: Option<&ValueExprAst>,
    value_ctx: Option<(&BodyBuilder, bool)>,
) -> Result<(TokenCreation, bool), &'static str> {
    let lower_amt = |expr: &ValueExprAst| match value_ctx {
        Some((builder, in_trigger)) => {
            lower_value_expr_in_context(resolve_x(expr, bound_x), builder, in_trigger, vocabulary)
        }
        None => lower_value_expr(resolve_x(expr, bound_x), vocabulary),
    };
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => {
            let amt = lower_amt(count)?;
            let def = predefined_token_def(name, vocabulary, ids)?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size: None,
                    recipients,
                },
                false,
            ))
        }
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => {
            let amt = lower_amt(count)?;

            let mut creature_subtypes = BTreeSet::new();
            let mut artifact_subtypes = BTreeSet::new();
            let mut enchantment_subtypes = BTreeSet::new();
            let mut land_subtypes = BTreeSet::new();
            let mut ordered_subtypes = Vec::new();
            let mut is_aura = false;
            for st in subtypes {
                if st.eq_ignore_ascii_case("aura") {
                    is_aura = true;
                } else if let Some(ct) = vocabulary.resolve_label(st) {
                    if creature_subtypes.insert(ct) {
                        ordered_subtypes.push(TokenSubtype::Creature(ct));
                    }
                } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
                    if artifact_subtypes.insert(at) {
                        ordered_subtypes.push(TokenSubtype::Artifact(at));
                    }
                } else if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
                    if enchantment_subtypes.insert(et) {
                        ordered_subtypes.push(TokenSubtype::Enchantment(et));
                    }
                } else if let Some(lt) =
                    LandType::from_wire_label(st).or_else(|| parse_land_subtype(st))
                {
                    if land_subtypes.insert(lt) {
                        ordered_subtypes.push(TokenSubtype::Land(lt));
                    }
                } else {
                    return Err("oracle:token-unknown-subtype");
                }
            }

            let is_noncreature = power.is_empty()
                && toughness.is_empty()
                && (card_types.0 & AstCardTypeMask::CREATURE.0) == 0;
            let (mut p, base_size) = if is_noncreature {
                if *attacking {
                    return Err("oracle:noncreature-attacking-token");
                }
                let mut perm_types = card_type_mask_to_set(*card_types)?;
                if perm_types.is_empty()
                    || perm_types
                        .iter()
                        .any(|t| matches!(t, CardType::Instant | CardType::Sorcery))
                    || !perm_types.iter().copied().any(CardType::is_permanent)
                {
                    return Err("oracle:token-nonpermanent-type");
                }
                if is_aura {
                    perm_types.insert(CardType::Enchantment);
                }
                if artifact_subtypes.contains(&ArtifactSubtype::Equipment)
                    || artifact_subtypes.contains(&ArtifactSubtype::Fortification)
                {
                    perm_types.insert(CardType::Artifact);
                }
                let attachment = if artifact_subtypes.contains(&ArtifactSubtype::Equipment) {
                    Some(AttachmentKind::Equipment)
                } else if artifact_subtypes.contains(&ArtifactSubtype::Fortification) {
                    Some(AttachmentKind::Fortification)
                } else {
                    None
                };
                let prog = Program::new(
                    perm_types,
                    creature_subtypes,
                    land_subtypes,
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment,
                        printed_pt: None,
                        casting_cost: None,
                    }),
                );
                (prog, None)
            } else {
                let (p_val, t_val, base_size) =
                    if power.eq_ignore_ascii_case("x") && toughness.eq_ignore_ascii_case("x") {
                        let x_expr = if let Some(bx) = bound_x {
                            match value_ctx {
                                Some((builder, in_trigger)) => lower_value_expr_in_context(
                                    bx, builder, in_trigger, vocabulary,
                                )?,
                                None => lower_value_expr(bx, vocabulary)?,
                            }
                        } else {
                            ValueExpr::chosen_x()
                        };
                        (0i64, 0i64, Some(TokenSize::AtCreation(x_expr)))
                    } else {
                        let p: i64 = power.parse().map_err(|_| "oracle:token-pt")?;
                        let t: i64 = toughness.parse().map_err(|_| "oracle:token-pt")?;
                        (p, t, None)
                    };
                let mut prog =
                    Program::creature(None, p_val.into(), t_val.into(), creature_subtypes);
                let extra_types = card_type_mask_to_set(*card_types)?;
                for t in extra_types {
                    if t.is_permanent() {
                        prog.card_types.insert(t);
                    }
                }
                prog.land_subtypes = land_subtypes;
                if artifact_subtypes.contains(&ArtifactSubtype::Equipment)
                    && let ProgramKind::Permanent(def) = &mut prog.kind
                {
                    def.attachment = Some(AttachmentKind::Equipment);
                }
                (prog, base_size)
            };
            p.supertypes = supertype_mask_to_set(*supertypes)?;
            p.artifact_subtypes = artifact_subtypes;
            p.enchantment_subtypes = enchantment_subtypes;
            p.effect_defined_colors = Some(color_mask_to_set(*colors));

            let mut type_words: Vec<&str> = Vec::new();
            if p.supertypes.contains(&Supertype::Legendary) {
                type_words.push("Legendary");
            }
            for ct in &p.card_types {
                match ct {
                    CardType::Artifact => type_words.push("Artifact"),
                    CardType::Battle => type_words.push("Battle"),
                    CardType::Creature => type_words.push("Creature"),
                    CardType::Enchantment => type_words.push("Enchantment"),
                    CardType::Kindred => type_words.push("Kindred"),
                    CardType::Land => type_words.push("Land"),
                    CardType::Planeswalker => type_words.push("Planeswalker"),
                    CardType::Instant | CardType::Sorcery => {}
                }
            }
            if is_aura {
                type_words.push("Aura");
            }
            if p.artifact_subtypes.contains(&ArtifactSubtype::Equipment) {
                type_words.push("Equipment");
            }
            let tok_type_line = if type_words.is_empty() {
                "Creature".to_string()
            } else {
                type_words.join(" ")
            };

            let mut tok_ids = LoweringIds::default();
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let tok_name = named.as_deref().unwrap_or("Token");
            let compile_target = TokenCompileTarget {
                name: tok_name,
                type_line: &tok_type_line,
                is_aura,
            };
            for kw_raw in keywords {
                if kw_raw.contains('"') {
                    let mut rest = kw_raw.as_str();
                    let mut quoted_texts: Vec<String> = Vec::new();
                    let mut leading_text = String::new();
                    while let Some(open) = rest.find('"') {
                        leading_text.push_str(&rest[..open]);
                        let after_open = &rest[open + 1..];
                        if let Some(close) = after_open.find('"') {
                            let cleaned = after_open[..close]
                                .trim()
                                .trim_end_matches([',', ';'])
                                .trim();
                            if !cleaned.is_empty() {
                                quoted_texts.push(cleaned.to_string());
                            }
                            rest = &after_open[close + 1..];
                        } else {
                            return Err("oracle:unsupported-granted-keyword");
                        }
                    }
                    leading_text.push_str(rest);
                    for part in split_token_keyword_list(&leading_text) {
                        let trimmed = part.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if !try_lower_token_keyword_direct(
                            trimmed,
                            &mut p,
                            vocabulary,
                            &mut tok_ids,
                        ) {
                            lower_token_text_via_compile_card(
                                trimmed,
                                compile_target,
                                &mut p,
                                vocabulary,
                                &mut tok_ids,
                                &compiler_vocab,
                            )?;
                        }
                    }
                    for q in quoted_texts {
                        if q.is_empty() {
                            continue;
                        }
                        lower_token_text_via_compile_card(
                            &q,
                            compile_target,
                            &mut p,
                            vocabulary,
                            &mut tok_ids,
                            &compiler_vocab,
                        )?;
                    }
                } else {
                    let kw_trimmed = kw_raw.trim().trim_end_matches(['.', ',', ';']).trim();
                    if kw_trimmed.is_empty() {
                        continue;
                    }
                    if !try_lower_token_keyword_direct(kw_trimmed, &mut p, vocabulary, &mut tok_ids)
                    {
                        let parts = split_token_keyword_list(kw_trimmed);
                        if parts.len() > 1 && parts.iter().all(|part| is_direct_token_keyword(part))
                        {
                            for part in &parts {
                                if !try_lower_token_keyword_direct(
                                    part,
                                    &mut p,
                                    vocabulary,
                                    &mut tok_ids,
                                ) {
                                    lower_token_text_via_compile_card(
                                        part,
                                        compile_target,
                                        &mut p,
                                        vocabulary,
                                        &mut tok_ids,
                                        &compiler_vocab,
                                    )?;
                                }
                            }
                        } else {
                            lower_token_text_via_compile_card(
                                kw_trimmed,
                                compile_target,
                                &mut p,
                                vocabulary,
                                &mut tok_ids,
                                &compiler_vocab,
                            )?;
                        }
                    }
                }
            }
            if is_aura
                && !matches!(
                    p.permanent().and_then(|d| d.attachment),
                    Some(AttachmentKind::Aura(_))
                )
            {
                return Err("oracle:aura-missing-enchant");
            }

            let token_name = if let Some(n) = named {
                TokenName::Specified(
                    EnglishCardName::new(n.clone()).map_err(|_| "oracle:token-name")?,
                )
            } else if !ordered_subtypes.is_empty() {
                TokenName::Default { ordered_subtypes }
            } else if is_aura {
                TokenName::Specified(
                    EnglishCardName::new("Aura Token".to_string())
                        .map_err(|_| "oracle:token-name")?,
                )
            } else {
                return Err("oracle:nameless-token");
            };

            let def = TokenDefinition::new(p, token_name, vocabulary)
                .map_err(|_| "oracle:token-def-invalid")?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size,
                    recipients,
                },
                *attacking,
            ))
        }
        _ => Err("oracle:unsupported-token-spec"),
    }
}

pub(super) fn predefined_token_def(
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<TokenDefinition, &'static str> {
    match name.to_ascii_lowercase().as_str() {
        "treasure" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Treasure);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Treasure)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "clue" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::Sacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Clue);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Clue)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "food" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Food);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Food)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "gold" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Gold);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: false },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Gold)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "blood" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::Discard {
                        mode: DiscardCostMode::SelectedOne,
                        tap_source: true,
                        sacrifice_source: true,
                    },
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Blood);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Blood)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "junk" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ExileTopForPlay {
                            until_next_from_source: false,
                        }],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Junk);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Junk)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "shard" => {
            let mut p = Program::enchantment(None);
            p.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::Scry {
                            player: ResolvingPlayer::Controller,
                            amount: ValueExpr::literal(1),
                        },
                        Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        },
                    ],
                ),
            });
            p.enchantment_subtypes.insert(EnchantmentSubtype::Shard);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Enchantment(EnchantmentSubtype::Shard)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "walker" => {
            let zombie = vocabulary
                .resolve_label("Zombie")
                .ok_or("oracle:predef-token-invalid")?;
            let mut p = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
            p.effect_defined_colors = Some(BTreeSet::from([Color::Black]));
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Walker".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "etherium cell" => {
            let mut p = Program::artifact(None, vec![]);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Etherium Cell".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "powerstone" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Powerstone);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())]))
                    .restricted(ManaRestriction::NotNonartifactSpell),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Powerstone)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "lander" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::SearchLibrary {
                            filter: Box::new(LibrarySearch {
                                searcher: ResolvingPlayer::Controller,
                                source: SearchSource::OwnLibrary,
                                matcher: SearchCardMatcher::BasicLand,
                                mana_value: None,
                                count: SearchCount::UpTo(1),
                                pair_constraint: None,
                                destination: SearchDestination::Battlefield {
                                    tapped: true,
                                    post_entry:
                                        spellwake_engine::program::SearchBattlefieldPostEntry::None,
                                },
                                reveal: false,
                            }),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Lander);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Lander)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "incubator" => {
            let phyrexian = vocabulary
                .resolve_label("Phyrexian")
                .ok_or("oracle:predef-token-invalid")?;
            let mut back = Program::creature(None, 0.into(), 0.into(), BTreeSet::from([phyrexian]));
            back.card_types.insert(CardType::Artifact);
            back.effect_defined_colors = Some(BTreeSet::new());
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(BodyTarget::None, vec![Instruction::TransformSource]),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Incubator);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.transform_back = Some(Box::new(back));
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Incubator)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "map" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()),
                        vec![Instruction::ExploreSubject(MoveSubjectRef::TargetObject)],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Map);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Map)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "mutagen" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::AffectPermanentTargets {
                            minimum: 1,
                            maximum: 1,
                            action: PermanentTargetAction::PutCounters {
                                kind: CounterKind::PlusOne,
                                amount: ValueExpr::literal(1),
                            },
                        }],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Mutagen);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Mutagen)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "cursed role" | "monster role" | "royal role" | "sorcerer role" | "virtuous role"
        | "wicked role" | "young hero role" => {
            let mut p = Program::aura(None, EnchantRestriction::Creature);
            p.enchantment_subtypes.insert(EnchantmentSubtype::Role);
            p.effect_defined_colors = Some(BTreeSet::new());
            let attached_sel = PermanentSelector {
                attached_to_source: true,
                ..PermanentSelector::default()
            };
            let plus_one_pair = NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            };
            let role_name = match name.to_ascii_lowercase().as_str() {
                "cursed role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::SetPT(plus_one_pair)]),
                        },
                    )?;
                    "Cursed"
                }
                "monster role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::Abilities(AbilityChange {
                                    granted_keywords: BTreeSet::from([KeywordAbility::Trample]),
                                    ..AbilityChange::default()
                                }),
                            ]),
                        },
                    )?;
                    "Monster"
                }
                "royal role" => {
                    let ward = TriggeredAbility::ward(ids.triggered(), ManaCost::generic(1.into()));
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::GrantTrigger(Box::new(ward)),
                            ]),
                        },
                    )?;
                    "Royal"
                }
                "sorcerer role" => {
                    let scry_trig = TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::self_attacks(),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::Scry {
                                player: ResolvingPlayer::Controller,
                                amount: ValueExpr::literal(1),
                            }],
                        ),
                    };
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::GrantTrigger(Box::new(scry_trig)),
                            ]),
                        },
                    )?;
                    "Sorcerer"
                }
                "virtuous role" => {
                    let ench_sel = PermanentSelector {
                        required_types: BTreeSet::from([CardType::Enchantment]),
                        controller: ControllerSelector::SourceController,
                        ..PermanentSelector::default()
                    };
                    let count_expr = NumericExpression::count(ench_sel);
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                                power: count_expr.clone(),
                                toughness: count_expr,
                            })]),
                        },
                    )?;
                    "Virtuous"
                }
                "wicked role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::ModifyPT(plus_one_pair)]),
                        },
                    )?;
                    p.triggered.push(TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::new(TriggerEvent::Dies {
                            subject: ObjectFilter::self_object(),
                            mode: DiesTriggerMode::Permanent,
                        }),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::AdjustEachOpponentLife {
                                adjustment: LifeAdjustment::Lose(1.into()),
                                controller_adjustment: None,
                            }],
                        ),
                    });
                    "Wicked"
                }
                "young hero role" => {
                    let attack_trig = TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::self_attacks().only_if(TriggerCondition::Eval(
                            Box::new(ResolutionCondition {
                                left: ValueExpr::ObjectStat {
                                    subject: ValueObjectRef::AbilitySource,
                                    stat: ObjectStatKind::Toughness,
                                },
                                comparison: Comparison::LessOrEqual,
                                right: ValueExpr::literal(3),
                            }),
                        )),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::PutCountersOnSource { amount: 1.into() }],
                        ),
                    };
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(
                                attack_trig,
                            ))]),
                        },
                    )?;
                    "Young Hero"
                }
                _ => unreachable!(),
            };
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new(role_name.to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        _ => Err("oracle:unsupported-predef-token"),
    }
}
