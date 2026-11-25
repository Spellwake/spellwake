//! Composable trigger-event and intervening-if matcher over transition occurrences.
use super::*;
use crate::program::{
    AttackTriggerMode, BlockTriggerKind, CastEntryKind, CounterTriggerKind, DamageTriggerRecipient,
    DiesTriggerMode, ObjectFilterFlags, PlayerTriggerAction, SpellCastZoneFilter,
    SpellSpecialFilter, SpellTargetShape, StateThresholdKind, StatusTriggerKind, SubtypeFilter,
    TriggerCondition, TriggerEvent, TriggerFromZone, TriggerKeywordEvent, TriggerPlayerScope,
};

pub(super) fn player_matches(
    relation: PlayerRelation,
    controller: SeatId,
    player: SeatId,
    view: &RulesView<'_>,
) -> bool {
    view.window.participants().contains(&player)
        && match relation {
            PlayerRelation::Controller => player == controller,
            PlayerRelation::Opponent => player != controller,
            PlayerRelation::AnyParticipant => true,
        }
}

fn player_scope_matches(
    scope: TriggerPlayerScope,
    source: &SupportedAppearance,
    controller: SeatId,
    player: SeatId,
    after: &RulesView<'_>,
) -> bool {
    match scope {
        TriggerPlayerScope::Relation(rel) => player_matches(rel, controller, player, after),
        TriggerPlayerScope::EnchantedPlayer => {
            after.characteristics.attachments.get(&source.object.id())
                == Some(&crate::targets::TargetRef::Player(player))
        }
        TriggerPlayerScope::AttachedHostController => matches!(
            after.characteristics.attachments.get(&source.object.id()),
            Some(crate::targets::TargetRef::Object(host))
                if after.evaluated.get(*host).and_then(|v| v.controller) == Some(player)
        ),
        TriggerPlayerScope::Monarch => after.characteristics.monarch == Some(player),
    }
}

fn check_intervening_if(
    condition: Option<&TriggerCondition>,
    ability: &TriggeredAbility,
    source: &SupportedAppearance,
    controller: SeatId,
    occurrence_value: &RulesOccurrence,
    after: &RulesView<'_>,
) -> bool {
    let Some(cond) = condition else {
        return true;
    };
    match cond {
        TriggerCondition::Morbid => after.morbid,
        TriggerCondition::DuringStep(step) => after.step == Some(*step),
        TriggerCondition::SourceHasLoyalty => after
            .characteristics
            .loyalty
            .get(&source.object.id())
            .is_some_and(RulesInteger::is_positive),
        TriggerCondition::SourceCounterAtLeast { kind, minimum } => {
            after
                .characteristics
                .permanent_counter_count(source.object.id(), *kind)
                >= i64::from(*minimum).into()
        }
        TriggerCondition::NoOtherControlledCreatures => !after
            .evaluated
            .has_other_controlled_creatures(controller, source.object.id()),
        TriggerCondition::DefendingPlayerHasMoreLands => true,
        TriggerCondition::UnrenownedSource => !source.renowned,
        TriggerCondition::SourceSaddled => source.saddled,
        TriggerCondition::SourceDidNotAttackThisTurn => true,
        TriggerCondition::ControllerHasCityBlessing => {
            after.characteristics.city_blessing.contains(&controller)
        }
        TriggerCondition::Monarch { opponent } => after.characteristics.monarch.is_some_and(|m| {
            if *opponent {
                m != controller
            } else {
                m == controller
            }
        }),
        TriggerCondition::ControlsOwnCommander => after.controls_own_commander(controller),
        TriggerCondition::OpponentHasMoreLands => after.opponent_has_more_lands(controller),
        TriggerCondition::ControlsGreatestArtifact => after.controls_greatest_artifact(controller),
        TriggerCondition::NoLoyaltyActivationThisTurn => !after
            .characteristics
            .loyalty_activators
            .contains(&controller),
        TriggerCondition::OpponentLostLifeAtLeast(min) => {
            after.opponent_lost_life(controller, *min)
        }
        TriggerCondition::PreviousTurnCasts(expected) => {
            after.previous_turn_casts == Some(*expected)
        }
        TriggerCondition::ControlledCreatureCount(count) => {
            after.evaluated.controlled_creature_count(controller) == usize::from(*count)
        }
        TriggerCondition::ControlledCreaturePowerAtLeast(min) => {
            after.evaluated.objects.values().any(|o| {
                o.zone == ObjectZone::Battlefield
                    && o.controller == Some(controller)
                    && o.is_creature()
                    && o.power_toughness
                        .as_ref()
                        .is_some_and(|pt| pt.derived.power >= i64::from(*min).into())
            })
        }
        TriggerCondition::TotalControlledPowerAtLeast(min) => {
            after.evaluated.total_creature_power(controller) >= i64::from(*min).into()
        }
        TriggerCondition::HandSizeBelow(limit) => {
            after
                .cards
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(controller))
                .count()
                < usize::from(*limit)
        }
        TriggerCondition::ControlsMeldPair => {
            let Some(mate) = ability.body.instructions.iter().find_map(|op| match op {
                crate::program::Instruction::ChooseMeldPartner(mate) => Some(mate),
                _ => None,
            }) else {
                return false;
            };
            source.object.owner() == controller
                && after.evaluated.objects.values().any(|v| {
                    v.zone == ObjectZone::Battlefield
                        && v.object != source.object.id()
                        && v.owner == controller
                        && v.controller == Some(controller)
                        && after.name_rules.has_name(&v.current_names, mate)
                })
        }
        TriggerCondition::ControllersTurn => after.window.active_player() == controller,
        TriggerCondition::OpponentsTurn => after.window.active_player() != controller,
        TriggerCondition::Delirium => {
            let mut types = BTreeSet::new();
            for card in after
                .evaluated
                .objects
                .values()
                .filter(|c| c.zone == ObjectZone::Graveyard(controller))
            {
                types.extend(card.current_types.iter().copied());
            }
            types.len() >= 4
        }
        TriggerCondition::Metalcraft => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && o.has_type(CardType::Artifact)
                })
                .count()
                >= 3
        }
        TriggerCondition::Threshold => {
            after
                .cards
                .iter()
                .filter(|o| o.zone() == ObjectZone::Graveyard(controller))
                .count()
                >= 7
        }
        TriggerCondition::HandSizeAtMost(limit) => {
            after
                .cards
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(controller))
                .count()
                <= usize::from(*limit)
        }
        TriggerCondition::HandSizeAtLeast(limit) => {
            after
                .cards
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(controller))
                .count()
                >= usize::from(*limit)
        }
        TriggerCondition::ControllerLifeAtLeast(limit) => after
            .life
            .get(controller)
            .is_some_and(|life| *life >= i64::from(*limit).into()),
        TriggerCondition::ControllerLifeAtMost(limit) => after
            .life
            .get(controller)
            .is_some_and(|life| *life <= i64::from(*limit).into()),
        TriggerCondition::ControlsCardType { kind, minimum } => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && o.has_type(*kind)
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::ControlsSubtype { kind, minimum } => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && o.current_creature_subtypes.contains(kind)
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::GraveyardCardTypeCount { kind, minimum } => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Graveyard(controller)
                        && kind.is_none_or(|t| o.has_type(t))
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::SourceIsTapped => source.object.tapped() == Some(true),
        TriggerCondition::SourceIsUntapped => source.object.tapped() == Some(false),
        TriggerCondition::SourceIsAttacking => after
            .combat
            .is_some_and(|c| c.attacker(source.object.id()).is_some()),
        TriggerCondition::SourceClassLevelAtLeast(min) => {
            after
                .characteristics
                .class_levels
                .get(&source.object.id())
                .copied()
                .unwrap_or(1)
                >= *min
        }
        TriggerCondition::SourceIsSolved => after
            .characteristics
            .solved_cases
            .contains(&source.object.id()),
        TriggerCondition::SourceIsUnsolved => !after
            .characteristics
            .solved_cases
            .contains(&source.object.id()),
        TriggerCondition::SourceIsPrepared => {
            after.characteristics.prepared.contains(&source.object.id())
        }
        TriggerCondition::SourceEntryOption(expected) => {
            after
                .characteristics
                .entry_option_choices
                .get(&source.object.id())
                == Some(expected)
        }
        TriggerCondition::ControllerGainedLifeThisTurn => after
            .characteristics
            .life_gained_this_turn
            .get(&controller)
            .is_some_and(RulesInteger::is_positive),
        TriggerCondition::ControllerLostLifeThisTurn => after
            .characteristics
            .life_lost_this_turn
            .get(&controller)
            .is_some_and(RulesInteger::is_positive),
        TriggerCondition::ControllerAttackedThisTurn => after
            .characteristics
            .attacked_this_turn_players
            .contains(&controller),
        TriggerCondition::ControllerCastSpellsThisTurn(min) => {
            after
                .characteristics
                .spells_cast_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= *min
        }
        TriggerCondition::ControllerDiscardedThisTurn => {
            after
                .characteristics
                .cards_discarded_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        TriggerCondition::ControllerSacrificedThisTurn => {
            after
                .characteristics
                .permanents_sacrificed_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        TriggerCondition::PermanentLeftBattlefieldThisTurn => {
            after
                .characteristics
                .permanents_left_battlefield_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        TriggerCondition::CreatureEnteredUnderControlThisTurn => {
            after
                .characteristics
                .creatures_entered_this_turn
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= 1
        }
        TriggerCondition::DescendedThisTurn => after
            .characteristics
            .descended_this_turn
            .contains(&controller),
        TriggerCondition::CommittedCrimeThisTurn => after
            .characteristics
            .committed_crime_this_turn
            .contains(&controller),
        TriggerCondition::ControllerDrewCardsThisTurn(min) => {
            after
                .characteristics
                .cards_drawn_this_turn_counts
                .get(&controller)
                .copied()
                .unwrap_or(0)
                >= *min
        }
        TriggerCondition::OpponentHasMoreLife => {
            let Some(own) = after.life.get(controller) else {
                return false;
            };
            after
                .window
                .participants()
                .iter()
                .any(|&p| p != controller && after.life.get(p).is_some_and(|l| l > own))
        }
        TriggerCondition::OpponentControlsMoreCreatures => {
            let count_creatures = |seat: SeatId| {
                after
                    .evaluated
                    .objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.controller == Some(seat)
                            && o.is_creature()
                    })
                    .count()
            };
            let own = count_creatures(controller);
            after
                .window
                .participants()
                .iter()
                .any(|&p| p != controller && count_creatures(p) > own)
        }
        TriggerCondition::OpponentHasMoreCardsInHand => {
            let count_hand = |seat: SeatId| {
                after
                    .cards
                    .iter()
                    .filter(|o| o.zone() == ObjectZone::Hand(seat))
                    .count()
            };
            let own = count_hand(controller);
            after
                .window
                .participants()
                .iter()
                .any(|&p| p != controller && count_hand(p) > own)
        }
        TriggerCondition::ControlsMatching { filter, minimum } => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && matches_evaluated_permanent(*filter, source, controller, o, after)
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::OpponentControlsMatching { filter, minimum } => {
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.controller.is_some_and(|seat| seat != controller)
                        && matches_evaluated_permanent(*filter, source, controller, o, after)
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::SourceMatches(filter) => after
            .evaluated
            .get(source.object.id())
            .is_some_and(|o| matches_evaluated_permanent(**filter, source, controller, o, after)),
        TriggerCondition::GraveyardMatching { filter, minimum } => {
            let excluded = BTreeSet::new();
            after
                .evaluated
                .objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Graveyard(controller)
                        && filter.matches_static_characteristics(o, &excluded, || {
                            Some(o.defined.mana_value.clone())
                        })
                })
                .count()
                >= usize::from(*minimum)
        }
        TriggerCondition::TriggeringAbilityNotMana => !matches!(
            occurrence_value,
            RulesOccurrence::ManaAbility { .. }
                | RulesOccurrence::ManaProduced { .. }
                | RulesOccurrence::ArtifactTappedForMana { .. }
                | RulesOccurrence::PermanentTappedForMana { .. }
        ),
        TriggerCondition::Eval(cond) => {
            let ctx = crate::characteristics::WorldEvalCtx {
                active_player: after.window.active_player(),
                participants: after.window.participants(),
                life: Some(after.life),
                combat: after.combat,
                cards: after.cards,
                programs: after.programs,
                state: after.characteristics,
                objects: &after.evaluated.objects,
                source: source.object.id(),
                controller,
                name_rules: after.name_rules,
                trigger_subject: None,
                related_player: None,
                defending_player: None,
            };
            crate::characteristics::eval_world_condition(&ctx, cond)
        }
        TriggerCondition::Not(inner) => !check_intervening_if(
            Some(inner),
            ability,
            source,
            controller,
            occurrence_value,
            after,
        ),
        TriggerCondition::And(a, b) => {
            check_intervening_if(
                Some(a),
                ability,
                source,
                controller,
                occurrence_value,
                after,
            ) && check_intervening_if(
                Some(b),
                ability,
                source,
                controller,
                occurrence_value,
                after,
            )
        }
        TriggerCondition::Or(a, b) => {
            check_intervening_if(
                Some(a),
                ability,
                source,
                controller,
                occurrence_value,
                after,
            ) || check_intervening_if(
                Some(b),
                ability,
                source,
                controller,
                occurrence_value,
                after,
            )
        }
    }
}

fn trigger_condition_needs_event_context(cond: &TriggerCondition) -> bool {
    match cond {
        TriggerCondition::Eval(rc) => !rc.is_world_evaluable(),
        TriggerCondition::Not(inner) => trigger_condition_needs_event_context(inner),
        TriggerCondition::And(a, b) | TriggerCondition::Or(a, b) => {
            trigger_condition_needs_event_context(a) || trigger_condition_needs_event_context(b)
        }
        _ => false,
    }
}

fn member_event_context(
    member: &TriggerCauseMember,
    after: &RulesView<'_>,
) -> (Option<ObjectId>, Option<SeatId>, Option<SeatId>) {
    let trigger_subject = match &member.related {
        TriggerRelated::ZoneChange { after: app, .. }
        | TriggerRelated::Discard { after: app, .. }
        | TriggerRelated::TokenCreated { after: app, .. } => Some(app.object.id()),
        TriggerRelated::Combatant(id)
        | TriggerRelated::AttackedAlone(id)
        | TriggerRelated::TargetedPermanent(id)
        | TriggerRelated::CounterSource(id)
        | TriggerRelated::StateTriggerSource(id)
        | TriggerRelated::Phased(id)
        | TriggerRelated::Mutated(id)
        | TriggerRelated::ManaSource(id)
        | TriggerRelated::MiracleRevealed(id)
        | TriggerRelated::LeftGame(id)
        | TriggerRelated::FinalSagaChapter { saga: id, .. }
        | TriggerRelated::SubtypeCombatDamage { creature: id, .. }
        | TriggerRelated::AttachedCreatureDamaged { creature: id, .. }
        | TriggerRelated::DamageDealtToPermanent { recipient: id, .. }
        | TriggerRelated::CombatDamageToPlaneswalker {
            planeswalker: id, ..
        }
        | TriggerRelated::CombatDamageToPlayer { source: id, .. } => Some(*id),
        TriggerRelated::SpellCast(cast) => Some(cast.spell().object.id()),
        TriggerRelated::StatusChange(ch) => Some(ch.after.id()),
        _ => None,
    };
    let defending_player = match &member.related {
        TriggerRelated::AttackGroup { attacked, .. } => match attacked {
            crate::targets::TargetRef::Player(seat) => Some(*seat),
            crate::targets::TargetRef::Object(obj) => {
                after.evaluated.get(*obj).and_then(|p| p.controller)
            }
            crate::targets::TargetRef::Stack(_) => None,
        },
        TriggerRelated::Combatant(id) | TriggerRelated::AttackedAlone(id) => after
            .combat
            .and_then(|c| c.attacker(*id).map(|a| a.defender())),
        _ => None,
    };
    (trigger_subject, member.related_player, defending_player)
}

fn check_intervening_if_with_ctx(
    condition: Option<&TriggerCondition>,
    ability: &TriggeredAbility,
    source: &SupportedAppearance,
    controller: SeatId,
    occurrence_value: &RulesOccurrence,
    after: &RulesView<'_>,
    event_ctx: (Option<ObjectId>, Option<SeatId>, Option<SeatId>),
) -> bool {
    let Some(cond) = condition else {
        return true;
    };
    match cond {
        TriggerCondition::Eval(rc) => {
            let (trigger_subject, related_player, defending_player) = event_ctx;
            let ctx = crate::characteristics::WorldEvalCtx {
                active_player: after.window.active_player(),
                participants: after.window.participants(),
                life: Some(after.life),
                combat: after.combat,
                cards: after.cards,
                programs: after.programs,
                state: after.characteristics,
                objects: &after.evaluated.objects,
                source: source.object.id(),
                controller,
                name_rules: after.name_rules,
                trigger_subject,
                related_player,
                defending_player,
            };
            crate::characteristics::eval_world_condition(&ctx, rc)
        }
        TriggerCondition::Not(inner) => !check_intervening_if_with_ctx(
            Some(inner),
            ability,
            source,
            controller,
            occurrence_value,
            after,
            event_ctx,
        ),
        TriggerCondition::And(a, b) => {
            check_intervening_if_with_ctx(
                Some(a),
                ability,
                source,
                controller,
                occurrence_value,
                after,
                event_ctx,
            ) && check_intervening_if_with_ctx(
                Some(b),
                ability,
                source,
                controller,
                occurrence_value,
                after,
                event_ctx,
            )
        }
        TriggerCondition::Or(a, b) => {
            check_intervening_if_with_ctx(
                Some(a),
                ability,
                source,
                controller,
                occurrence_value,
                after,
                event_ctx,
            ) || check_intervening_if_with_ctx(
                Some(b),
                ability,
                source,
                controller,
                occurrence_value,
                after,
                event_ctx,
            )
        }
        _ => check_intervening_if(
            Some(cond),
            ability,
            source,
            controller,
            occurrence_value,
            after,
        ),
    }
}

fn matches_evaluated_permanent(
    filter: ObjectFilter,
    source: &SupportedAppearance,
    controller: SeatId,
    candidate: &crate::characteristics::ObjectCharacteristics,
    after: &RulesView<'_>,
) -> bool {
    let clause_ok = |c: crate::program::ObjectClause| -> bool {
        let same = candidate.object == source.object.id();
        let source_ok = match c.source_relation {
            crate::program::SourceRelation::Any => true,
            crate::program::SourceRelation::SelfObject => same,
            crate::program::SourceRelation::AttachedToSelf => {
                source.attached_to == Some(crate::targets::TargetRef::Object(candidate.object))
            }
            crate::program::SourceRelation::OtherThanAbilitySource
            | crate::program::SourceRelation::OtherThanTriggerSource => !same,
            crate::program::SourceRelation::OtherThanBound(id) => candidate.object != id,
            crate::program::SourceRelation::SharingNameWithAbilitySource
            | crate::program::SourceRelation::MatchesNamedAgendaSlot => false,
        };
        if !source_ok {
            return false;
        }
        let controller_ok = match c.controller {
            crate::program::ControllerFilter::Any => true,
            crate::program::ControllerFilter::You => candidate.controller == Some(controller),
            crate::program::ControllerFilter::Opponent
            | crate::program::ControllerFilter::NotYou => {
                candidate.controller.is_some_and(|seat| seat != controller)
            }
            crate::program::ControllerFilter::OwnerYou => candidate.owner == controller,
            crate::program::ControllerFilter::OwnerOpponent => candidate.owner != controller,
            crate::program::ControllerFilter::RelatedPlayer
            | crate::program::ControllerFilter::TargetPlayer
            | crate::program::ControllerFilter::TargetOpponent
            | crate::program::ControllerFilter::ActivePlayer
            | crate::program::ControllerFilter::DefendingPlayer => false,
            crate::program::ControllerFilter::Player(seat) => candidate.controller == Some(seat),
        };
        if !controller_ok {
            return false;
        }
        if let Some(expected_token) = c.token
            && candidate.is_token != expected_token
        {
            return false;
        }
        if c.flags.contains(ObjectFilterFlags::COMMANDER_ONLY)
            && candidate.commander_owner.is_none()
        {
            return false;
        }
        if c.flags.contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            && !after
                .characteristics
                .counters(candidate.object)
                .plus_one
                .is_positive()
        {
            return false;
        }
        c.matches_static_characteristics(candidate, &BTreeSet::new(), || {
            Some(candidate.defined.mana_value.clone())
        })
    };
    match filter {
        ObjectFilter::Clause(a) => clause_ok(a),
        ObjectFilter::Either(a, b) => clause_ok(a) || clause_ok(b),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn match_trigger_event(
    event: &TriggerEvent,
    intervening_if: Option<&TriggerCondition>,
    ability: &TriggeredAbility,
    source: &SupportedAppearance,
    controller: SeatId,
    occurrence: &EventOccurrence,
    event_cause: &EventCause,
    object: ObjectId,
    ability_id: TriggeredAbilityId,
    granting_effect: Option<crate::characteristics::EffectInstanceKey>,
    after: &RulesView<'_>,
    members: &mut Vec<TriggerCauseMember>,
) -> Result<(), TriggerError> {
    let value = &occurrence.value;
    if let TriggerEvent::AnyOf(sub_events) = event {
        for sub in sub_events {
            match_trigger_event(
                sub,
                intervening_if,
                ability,
                source,
                controller,
                occurrence,
                event_cause,
                object,
                ability_id,
                granting_effect,
                after,
                members,
            )?;
        }
        return Ok(());
    }
    if let Some(crate::program::CastCopyKeyword::Replicate(i)) = ability.cast_copy_keyword()
        && let RulesOccurrence::CastCompleted(cast) = value
        && !cast
            .copy_source
            .as_ref()
            .is_some_and(|s| s.replicated(i).is_positive())
    {
        return Ok(());
    }
    let deferred_if = intervening_if.is_some_and(trigger_condition_needs_event_context);
    if !deferred_if
        && !check_intervening_if(intervening_if, ability, source, controller, value, after)
    {
        return Ok(());
    }
    let start_len = members.len();

    match event {
        TriggerEvent::StateThreshold(threshold) => {
            if matches!(
                value,
                RulesOccurrence::CounterThresholdMet {
                    object: found,
                    definition,
                    grant,
                    ..
                } | RulesOccurrence::SubtypeThresholdMet {
                    object: found,
                    definition,
                    grant,
                } | RulesOccurrence::StateConditionMet {
                    object: found,
                    definition,
                    grant,
                } if *found == object && *definition == ability_id && *grant == granting_effect
            ) {
                let related = match threshold {
                    StateThresholdKind::SourceCounters { .. } => {
                        TriggerRelated::CounterSource(object)
                    }
                    StateThresholdKind::ControlledSubtype { .. } | StateThresholdKind::Eval(_) => {
                        TriggerRelated::StateTriggerSource(object)
                    }
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(controller),
                    related,
                });
            }
        }
        TriggerEvent::CountersChanged { subject, kind } => match kind {
            CounterTriggerKind::SagaChapter(number) => {
                let chapter = RulesInteger::from(i64::from(*number));
                if let RulesOccurrence::LoreCountersChanged {
                    object: target,
                    before,
                    after: count,
                    exact_chapter_only,
                } = value
                    && *target == source.object.id()
                    && *before < chapter
                    && *count >= chapter
                    && (!exact_chapter_only || *count == chapter)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::SagaChapter { number: *number },
                    });
                }
            }
            CounterTriggerKind::LastDefenseRemoved => {
                if let RulesOccurrence::PermanentCounterChanged {
                    object: target,
                    kind: crate::effects::CounterKind::Defense,
                    before,
                    after: count,
                } = value
                    && *target == source.object.id()
                    && before.is_positive()
                    && !count.is_positive()
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::CounterSource(*target),
                    });
                }
            }
            CounterTriggerKind::CounterChanged {
                kind: req_kind,
                added: req_added,
                by_controller_only,
            } => {
                let actor_matches = |before_zone: ObjectZone, after_ctrl: Option<SeatId>| -> bool {
                    if !*by_controller_only {
                        return true;
                    }
                    let actor = if before_zone != ObjectZone::Battlefield {
                        after_ctrl
                    } else {
                        match event_cause {
                            EventCause::TurnFaceUp { actor, .. } => Some(*actor),
                            EventCause::Instruction { entry, .. }
                            | EventCause::ActivatedAbilityCost { entry, .. } => after
                                .window
                                .entries()
                                .iter()
                                .chain(after.window.resolving())
                                .find(|item| item.id() == *entry)
                                .map(|item| item.controller()),
                            _ => None,
                        }
                    };
                    actor == Some(controller)
                };
                match value {
                    RulesOccurrence::CountersPlaced {
                        object: recipient,
                        before_appearance,
                        after_appearance,
                        added,
                        ..
                    } if *req_added
                        && match req_kind {
                            None => added.plus_one.is_positive() || added.minus_one.is_positive(),
                            Some(crate::effects::CounterKind::PlusOne) => {
                                added.plus_one.is_positive()
                            }
                            Some(crate::effects::CounterKind::MinusOne) => {
                                added.minus_one.is_positive()
                            }
                            Some(_) => false,
                        }
                        && subject.matches_appearance(source, controller, after_appearance)
                        && actor_matches(
                            before_appearance.object.zone(),
                            after_appearance.object.controller(),
                        ) =>
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CounterSource(*recipient),
                        });
                    }
                    RulesOccurrence::PermanentCounterChanged {
                        object: recipient,
                        kind,
                        before: old_cnt,
                        after: new_cnt,
                    } if req_kind.is_none_or(|k| k == *kind)
                        && (if *req_added {
                            new_cnt > old_cnt
                        } else {
                            new_cnt < old_cnt
                        })
                        && after.appearance(*recipient).is_some_and(|app| {
                            subject.matches_appearance(source, controller, &app)
                                && actor_matches(app.object.zone(), app.object.controller())
                        }) =>
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CounterSource(*recipient),
                        });
                    }
                    RulesOccurrence::ChargeCountersChanged {
                        object: recipient,
                        before: old_cnt,
                        after: new_cnt,
                    } if req_kind.is_none_or(|k| k == crate::effects::CounterKind::Charge)
                        && (if *req_added {
                            new_cnt > old_cnt
                        } else {
                            new_cnt < old_cnt
                        })
                        && after.appearance(*recipient).is_some_and(|app| {
                            subject.matches_appearance(source, controller, &app)
                                && actor_matches(app.object.zone(), app.object.controller())
                        }) =>
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CounterSource(*recipient),
                        });
                    }
                    RulesOccurrence::LoyaltyCountersChanged {
                        object: recipient,
                        before: old_cnt,
                        after: new_cnt,
                    } if req_kind.is_none_or(|k| k == crate::effects::CounterKind::Loyalty)
                        && (if *req_added {
                            new_cnt > old_cnt
                        } else {
                            new_cnt < old_cnt
                        })
                        && after.appearance(*recipient).is_some_and(|app| {
                            subject.matches_appearance(source, controller, &app)
                                && actor_matches(app.object.zone(), app.object.controller())
                        }) =>
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CounterSource(*recipient),
                        });
                    }
                    _ => {}
                }
            }
            CounterTriggerKind::PlusOnePlaced { by_controller_only } => match value {
                RulesOccurrence::CountersPlaced {
                    object: recipient,
                    before_appearance,
                    after_appearance,
                    added,
                    ..
                } if added.plus_one.is_positive()
                    && subject.matches_appearance(source, controller, after_appearance) =>
                {
                    let actor_ok = if *by_controller_only {
                        let actor = if before_appearance.object.zone() != ObjectZone::Battlefield {
                            after_appearance.object.controller()
                        } else {
                            match event_cause {
                                EventCause::TurnFaceUp { actor, .. } => Some(*actor),
                                EventCause::Instruction { entry, .. }
                                | EventCause::ActivatedAbilityCost { entry, .. } => after
                                    .window
                                    .entries()
                                    .iter()
                                    .chain(after.window.resolving())
                                    .find(|item| item.id() == *entry)
                                    .map(|item| item.controller()),
                                _ => None,
                            }
                        };
                        actor == Some(controller)
                    } else {
                        true
                    };
                    if actor_ok {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CounterSource(*recipient),
                        });
                    }
                }
                RulesOccurrence::TokenCreated { after: created, .. }
                    if !*by_controller_only
                        && *subject == ObjectFilter::self_object()
                        && created.object.id() == source.object.id()
                        && created
                            .battlefield_counters
                            .as_ref()
                            .is_some_and(|counters| counters.plus_one.is_positive()) =>
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::CounterSource(created.object.id()),
                    });
                }
                _ => {}
            },
        },
        TriggerEvent::Attacks { subject, mode } => {
            let RulesOccurrence::AttackersDeclared {
                selected,
                membership,
                ..
            } = value
            else {
                return Ok(());
            };
            match *mode {
                AttackTriggerMode::GroupSubtype(subtype) => {
                    let group = selected
                        .iter()
                        .map(|attack| attack.attacker)
                        .filter(|&attacker| {
                            after.evaluated.get(attacker).is_some_and(|p| {
                                p.controller == Some(controller)
                                    && p.current_creature_subtypes.contains(&subtype)
                            })
                        })
                        .collect::<Vec<_>>();
                    if !group.is_empty() {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::DeclaredSubtypeAttackers(group),
                        });
                    }
                }
                AttackTriggerMode::GroupTotalPowerAtLeast(minimum) => {
                    if after.window.active_player() == controller && !selected.is_empty() {
                        let total = selected
                            .iter()
                            .filter_map(|attack| {
                                after
                                    .evaluated
                                    .get(attack.attacker)?
                                    .power_toughness
                                    .as_ref()
                            })
                            .fold(RulesInteger::from(0), |total, pt| {
                                total.plus(&pt.derived.power)
                            });
                        if total >= i64::from(minimum).into() {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(controller),
                                related: TriggerRelated::AttackedWithTotalPower(total),
                            });
                        }
                    }
                }
                AttackTriggerMode::OpponentsAtController { minimum } => {
                    let min = usize::from(minimum);
                    if player_matches(
                        PlayerRelation::Opponent,
                        controller,
                        after.window.active_player(),
                        after,
                    ) && selected
                        .iter()
                        .filter(|attack| attack.defender == controller)
                        .take(min)
                        .count()
                        == min
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(after.window.active_player()),
                            related: TriggerRelated::AttackGroup {
                                attacked: crate::targets::TargetRef::Player(controller),
                                creatures: selected.iter().map(|row| row.attacker).collect(),
                            },
                        });
                    }
                }
                AttackTriggerMode::EquippedAtPlayer => {
                    if after.window.active_player() == controller {
                        let mut groups = BTreeMap::<SeatId, Vec<ObjectId>>::new();
                        for attack in selected
                            .iter()
                            .filter(|attack| attack.defending_planeswalker.is_none())
                        {
                            if after
                                .evaluated
                                .equipment_attached_to(attack.attacker)
                                .is_positive()
                            {
                                groups
                                    .entry(attack.defender)
                                    .or_default()
                                    .push(attack.attacker);
                            }
                        }
                        members.extend(groups.into_iter().map(|(player, creatures)| {
                            TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(player),
                                related: TriggerRelated::AttackGroup {
                                    attacked: crate::targets::TargetRef::Player(player),
                                    creatures,
                                },
                            }
                        }));
                    }
                }
                AttackTriggerMode::AtOpponentOrPlaneswalker => {
                    use crate::targets::TargetRef;
                    let mut cohorts: BTreeMap<TargetRef, (SeatId, Vec<ObjectId>)> = BTreeMap::new();
                    for attack in selected.iter().filter(|attack| {
                        attack.defender != controller
                            && attack.defending_planeswalker.is_none_or(|id| {
                                after
                                    .evaluated
                                    .get(id)
                                    .is_some_and(|v| v.has_type(CardType::Planeswalker))
                            })
                    }) {
                        let attacked = attack
                            .defending_planeswalker
                            .map_or(TargetRef::Player(attack.defender), TargetRef::Object);
                        let (_, group) = cohorts
                            .entry(attacked)
                            .or_insert_with(|| (attack.defender, Vec::new()));
                        group.push(attack.attacker);
                    }
                    members.extend(
                        cohorts
                            .into_iter()
                            .map(|(attacked, (defender, creatures))| TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(defender),
                                related: TriggerRelated::AttackGroup {
                                    attacked,
                                    creatures,
                                },
                            }),
                    );
                }
                AttackTriggerMode::AtControllerOrPlaneswalker => {
                    for attack in selected {
                        let attacker = after
                            .evaluated
                            .get(attack.attacker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        if attack.defender == controller
                            && attacker.is_creature()
                            && attack.defending_planeswalker.is_none_or(|id| {
                                after
                                    .evaluated
                                    .get(id)
                                    .is_some_and(|p| p.has_type(CardType::Planeswalker))
                            })
                        {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(attack.defender),
                                related: TriggerRelated::Combatant(attacker.object),
                            });
                        }
                    }
                }
                AttackTriggerMode::Alone => {
                    if let ([attack], [member]) = (selected.as_slice(), membership.as_slice())
                        && member.combatant().controller() == controller
                        && member.combatant().object() == attack.attacker
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::AttackedAlone(attack.attacker),
                        });
                    }
                }
                AttackTriggerMode::WithAtLeastOthers(minimum) => {
                    if selected.len() > usize::from(minimum)
                        && selected
                            .iter()
                            .any(|attack| attack.attacker == source.object.id())
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(source.object.id()),
                        });
                    }
                }
                AttackTriggerMode::FirstThisTurn { highest_life } => {
                    let Some(attack) = selected
                        .iter()
                        .find(|row| row.attacker == source.object.id())
                    else {
                        return Ok(());
                    };
                    let qualifies = if highest_life {
                        if attack.defending_planeswalker.is_some() {
                            false
                        } else {
                            let defending = after
                                .life
                                .get(attack.defender)
                                .ok_or(TriggerError::InvalidWorld)?;
                            after.window.participants().iter().all(|&seat| {
                                after.life.get(seat).is_some_and(|life| life <= defending)
                            })
                        }
                    } else {
                        true
                    };
                    if qualifies {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(attack.attacker),
                        });
                    }
                }
                AttackTriggerMode::HighestLifeDefender => {
                    let Some(attack) = selected
                        .iter()
                        .find(|row| row.attacker == source.object.id())
                    else {
                        return Ok(());
                    };
                    if attack.defending_planeswalker.is_some() {
                        return Ok(());
                    }
                    let defending = after
                        .life
                        .get(attack.defender)
                        .ok_or(TriggerError::InvalidWorld)?;
                    if after
                        .window
                        .participants()
                        .iter()
                        .all(|&seat| after.life.get(seat).is_some_and(|life| life <= defending))
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(attack.attacker),
                        });
                    }
                }
                AttackTriggerMode::WithStrongerCreature => {
                    let Some(attack) = selected
                        .iter()
                        .find(|row| row.attacker == source.object.id())
                    else {
                        return Ok(());
                    };
                    let qualifies = source.current_power_toughness.as_ref().is_some_and(|pt| {
                        selected
                            .iter()
                            .filter(|other| other.attacker != attack.attacker)
                            .any(|other| {
                                after.evaluated.get(other.attacker).is_some_and(|view| {
                                    view.controller == Some(controller)
                                        && view
                                            .power_toughness
                                            .as_ref()
                                            .is_some_and(|stats| stats.derived.power > pt.power)
                                })
                            })
                    });
                    if qualifies {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(attack.attacker),
                        });
                    }
                }
                AttackTriggerMode::Individual { capture_defender } => {
                    if *subject == ObjectFilter::self_object() {
                        if let Some(attack) = selected
                            .iter()
                            .find(|row| row.attacker == source.object.id())
                        {
                            let player = if capture_defender {
                                attack.defender
                            } else {
                                controller
                            };
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(player),
                                related: TriggerRelated::Combatant(attack.attacker),
                            });
                        }
                    } else if *subject == ObjectFilter::any().attached_to_self() {
                        if let Some(attack) = selected.iter().find(|attack| {
                            source.attached_to
                                == Some(crate::targets::TargetRef::Object(attack.attacker))
                        }) {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(attack.defender),
                                related: TriggerRelated::Combatant(attack.attacker),
                            });
                        }
                    } else {
                        for attack in selected {
                            let attacker = after
                                .evaluated
                                .get(attack.attacker)
                                .ok_or(TriggerError::InvalidWorld)?;
                            if matches_evaluated_permanent(
                                *subject, source, controller, attacker, after,
                            ) {
                                let related = if capture_defender {
                                    let attacked = attack.defending_planeswalker.map_or(
                                        crate::targets::TargetRef::Player(attack.defender),
                                        crate::targets::TargetRef::Object,
                                    );
                                    TriggerRelated::AttackGroup {
                                        attacked,
                                        creatures: vec![attack.attacker],
                                    }
                                } else {
                                    TriggerRelated::Combatant(attacker.object)
                                };
                                members.push(TriggerCauseMember {
                                    occurrence: occurrence.id,
                                    related_player: Some(attack.defender),
                                    related,
                                });
                            }
                        }
                    }
                }
                AttackTriggerMode::AtController
                | AttackTriggerMode::AtOpponent
                | AttackTriggerMode::AtPlayer
                | AttackTriggerMode::AtPlaneswalker
                | AttackTriggerMode::AtEnchantedPlayer => {
                    let enchanted_player = match source.attached_to {
                        Some(crate::targets::TargetRef::Player(seat)) => Some(seat),
                        _ => None,
                    };
                    for attack in selected {
                        let defender_ok = match *mode {
                            AttackTriggerMode::AtController => {
                                attack.defender == controller
                                    && attack.defending_planeswalker.is_none()
                            }
                            AttackTriggerMode::AtOpponent => {
                                attack.defender != controller
                                    && attack.defending_planeswalker.is_none()
                            }
                            AttackTriggerMode::AtPlayer => attack.defending_planeswalker.is_none(),
                            AttackTriggerMode::AtPlaneswalker => {
                                attack.defending_planeswalker.is_some()
                            }
                            AttackTriggerMode::AtEnchantedPlayer => {
                                enchanted_player == Some(attack.defender)
                                    && attack.defending_planeswalker.is_none()
                            }
                            _ => false,
                        };
                        if !defender_ok {
                            continue;
                        }
                        let attacker = after
                            .evaluated
                            .get(attack.attacker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        let subj_ok = if *subject == ObjectFilter::self_object() {
                            attack.attacker == source.object.id()
                        } else if *subject == ObjectFilter::any().attached_to_self() {
                            source.attached_to
                                == Some(crate::targets::TargetRef::Object(attack.attacker))
                        } else {
                            matches_evaluated_permanent(
                                *subject, source, controller, attacker, after,
                            )
                        };
                        if subj_ok {
                            let related_player = match mode {
                                AttackTriggerMode::AtEnchantedPlayer => attacker.controller,
                                _ => Some(attack.defender),
                            };
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player,
                                related: TriggerRelated::Combatant(attacker.object),
                            });
                        }
                    }
                }
                AttackTriggerMode::GroupAtLeast(minimum) => {
                    let mut matching = Vec::new();
                    for attack in selected {
                        let attacker = after
                            .evaluated
                            .get(attack.attacker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        if matches_evaluated_permanent(
                            *subject, source, controller, attacker, after,
                        ) {
                            matching.push(attack.attacker);
                        }
                    }
                    if matching.len() >= usize::from(minimum) {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::AttackGroup {
                                attacked: crate::targets::TargetRef::Player(controller),
                                creatures: matching,
                            },
                        });
                    }
                }
            }
        }
        TriggerEvent::BlocksOrBlocked { subject, kind } => {
            let RulesOccurrence::BlockersDeclared {
                selected, statuses, ..
            } = value
            else {
                return Ok(());
            };
            let target_id = if *subject == ObjectFilter::self_object() {
                Some(source.object.id())
            } else if *subject == ObjectFilter::any().attached_to_self() {
                match source.attached_to {
                    Some(crate::targets::TargetRef::Object(host)) => Some(host),
                    _ => None,
                }
            } else {
                Some(source.object.id())
            };
            let Some(watched_id) = target_id else {
                return Ok(());
            };
            match kind {
                BlockTriggerKind::BlocksOrBlockedByEach { required_subtype } => {
                    for pair in selected {
                        let other = if pair.blocker == watched_id {
                            pair.attacker
                        } else if pair.attacker == watched_id {
                            pair.blocker
                        } else {
                            continue;
                        };
                        let combatant = after
                            .evaluated
                            .get(other)
                            .ok_or(TriggerError::InvalidWorld)?;
                        if required_subtype
                            .is_some_and(|k| !combatant.current_creature_subtypes.contains(&k))
                        {
                            continue;
                        }
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: combatant.controller,
                            related: TriggerRelated::Combatant(other),
                        });
                    }
                }
                BlockTriggerKind::BlockedByEach {
                    without_flanking_only,
                } => {
                    for pair in selected.iter().filter(|pair| pair.attacker == watched_id) {
                        let blocker = after
                            .evaluated
                            .get(pair.blocker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        let has_flanking = blocker.combat_trigger_keywords().any(|keyword| {
                            matches!(keyword, crate::program::CombatTriggerKeyword::Flanking)
                        });
                        if !*without_flanking_only || !has_flanking {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: blocker.controller,
                                related: TriggerRelated::Combatant(pair.blocker),
                            });
                        }
                    }
                }
                BlockTriggerKind::BecomesBlocked { capture_defender } => {
                    if statuses.iter().any(|&(id, before, after_status)| {
                        id == watched_id
                            && before == BlockStatus::NotDeclared
                            && after_status == BlockStatus::Blocked
                    }) {
                        let player = if *capture_defender {
                            after
                                .combat
                                .and_then(|combat| combat.attacker(watched_id))
                                .ok_or(TriggerError::InvalidWorld)?
                                .defender()
                        } else {
                            controller
                        };
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::Combatant(watched_id),
                        });
                    }
                }
                BlockTriggerKind::Unblocked => {
                    if statuses.iter().any(|&(id, before, after_status)| {
                        id == watched_id
                            && before == BlockStatus::NotDeclared
                            && after_status == BlockStatus::Unblocked
                    }) {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(watched_id),
                        });
                    }
                }
                BlockTriggerKind::Blocks => {
                    if selected.iter().any(|row| row.blocker == watched_id) {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(watched_id),
                        });
                    }
                }
                BlockTriggerKind::BlocksOrBecomesBlocked => {
                    if selected.iter().any(|row| row.blocker == watched_id)
                        || statuses.iter().any(|&(id, before, after_status)| {
                            id == watched_id
                                && before != BlockStatus::Blocked
                                && after_status == BlockStatus::Blocked
                        })
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::Combatant(watched_id),
                        });
                    }
                }
                BlockTriggerKind::BlockedByMatching(blocker_filter) => {
                    for pair in selected.iter().filter(|pair| pair.attacker == watched_id) {
                        let blocker = after
                            .evaluated
                            .get(pair.blocker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        if matches_evaluated_permanent(
                            **blocker_filter,
                            source,
                            controller,
                            blocker,
                            after,
                        ) {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: blocker.controller,
                                related: TriggerRelated::Combatant(pair.blocker),
                            });
                        }
                    }
                }
                BlockTriggerKind::BlocksMatching(attacker_filter) => {
                    for pair in selected.iter().filter(|pair| pair.blocker == watched_id) {
                        let attacker = after
                            .evaluated
                            .get(pair.attacker)
                            .ok_or(TriggerError::InvalidWorld)?;
                        if matches_evaluated_permanent(
                            **attacker_filter,
                            source,
                            controller,
                            attacker,
                            after,
                        ) {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: attacker.controller,
                                related: TriggerRelated::Combatant(pair.attacker),
                            });
                        }
                    }
                }
            }
        }
        TriggerEvent::Keyword(kw) => match *kw {
            TriggerKeywordEvent::Soulbond { entrant_only } => {
                let unpaired = |id| {
                    after.evaluated.get(id).is_some_and(|view| {
                        view.zone == ObjectZone::Battlefield
                            && view.is_creature()
                            && view.controller == Some(controller)
                            && !after.characteristics.paired_creatures.contains_key(&id)
                    })
                };
                let src = source.object.id();
                if !unpaired(src) {
                    return Ok(());
                }
                let (entrant, related) = match value {
                    RulesOccurrence::ZoneChange(change)
                        if change.before.object.zone() != ObjectZone::Battlefield
                            && change.after.object.zone() == ObjectZone::Battlefield =>
                    {
                        (
                            change.after.object.id(),
                            TriggerRelated::ZoneChange {
                                before: change.before.clone(),
                                after: change.after.clone(),
                            },
                        )
                    }
                    RulesOccurrence::TokenCreated { creator, after } => (
                        after.object.id(),
                        TriggerRelated::TokenCreated {
                            creator: *creator,
                            after: after.as_ref().clone(),
                        },
                    ),
                    _ => return Ok(()),
                };
                let eligible = if entrant_only {
                    entrant != src && unpaired(entrant)
                } else {
                    entrant == src
                        && after
                            .evaluated
                            .objects
                            .keys()
                            .copied()
                            .any(|id| id != src && unpaired(id))
                };
                if eligible {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related,
                    });
                }
            }
            TriggerKeywordEvent::Counter(keyword) => {
                use crate::program::CounterKeyword as K;
                let relation = match (keyword, value) {
                    (K::SuspendUpkeep, RulesOccurrence::StepBegan { position, active })
                        if position.step == crate::turn::Step::Upkeep
                            && *active == controller
                            && source.time_counters.is_some() =>
                    {
                        Some(TriggerRelated::Step {
                            position: *position,
                            active: *active,
                        })
                    }
                    (
                        K::SuspendLastCounter,
                        RulesOccurrence::TimeCounterRemoved {
                            object: target,
                            after: remaining,
                            ..
                        },
                    ) if *target == source.object.id() && !remaining.is_positive() => {
                        Some(TriggerRelated::CounterSource(*target))
                    }
                    (
                        K::Fading(_) | K::Vanishing(_),
                        RulesOccurrence::StepBegan { position, active },
                    ) if position.step == crate::turn::Step::Upkeep
                        && *active == controller
                        && (!matches!(keyword, K::Vanishing(_))
                            || source.time_counters.is_some()) =>
                    {
                        Some(TriggerRelated::Step {
                            position: *position,
                            active: *active,
                        })
                    }
                    (
                        K::VanishingLastCounter,
                        RulesOccurrence::TimeCounterRemoved {
                            object: target,
                            after: remaining,
                            ..
                        },
                    ) if *target == source.object.id() && !remaining.is_positive() => {
                        Some(TriggerRelated::CounterSource(*target))
                    }
                    (K::Persist | K::Undying, RulesOccurrence::ZoneChange(change))
                        if change.before.object.id() == source.object.id()
                            && change.before.object.zone() == ObjectZone::Battlefield
                            && matches!(change.after.object.zone(), ObjectZone::Graveyard(_))
                            && keyword.eligible_at_death(&change.before) =>
                    {
                        Some(TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        })
                    }
                    _ => None,
                };
                if let Some(related) = relation {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related,
                    });
                }
            }
            TriggerKeywordEvent::Upkeep(keyword) => {
                if let RulesOccurrence::StepBegan { position, active } = value
                    && position.step == crate::turn::Step::Upkeep
                    && *active == controller
                    && (keyword != crate::program::UpkeepKeyword::Echo || source.echo_condition())
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*active),
                        related: TriggerRelated::Step {
                            position: *position,
                            active: *active,
                        },
                    });
                }
            }
            TriggerKeywordEvent::Increment => {
                if let RulesOccurrence::CastCompleted(cast) = value {
                    if !after.window.participants().contains(&cast.controller()) {
                        return Err(TriggerError::InvalidWorld);
                    }
                    if controller == cast.controller()
                        && source.is_creature()
                        && cast
                            .spent_for_increment()
                            .zip(source.current_power_toughness.as_ref())
                            .is_some_and(|(paid, stats)| {
                                paid > &stats.power || paid > &stats.toughness
                            })
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(cast.controller()),
                            related: TriggerRelated::SpellCast(cast.clone()),
                        });
                    }
                }
            }
            TriggerKeywordEvent::Evolve | TriggerKeywordEvent::Graft => {
                let relation = if kw == &TriggerKeywordEvent::Graft {
                    if !source
                        .battlefield_counters
                        .as_ref()
                        .is_some_and(|c| c.plus_one.is_positive())
                    {
                        return Ok(());
                    }
                    ObjectFilter::creature().other_than_ability_source()
                } else {
                    ObjectFilter::creature().controlled()
                };
                let entering = match value {
                    RulesOccurrence::ZoneChange(change)
                        if change.after.object.zone() == ObjectZone::Battlefield
                            && change.before.object.zone() != ObjectZone::Battlefield =>
                    {
                        &change.after
                    }
                    RulesOccurrence::TokenCreated { after: created, .. } => created.as_ref(),
                    _ => return Ok(()),
                };
                if kw == &TriggerKeywordEvent::Evolve
                    && (!source.is_creature()
                        || !entering
                            .current_power_toughness
                            .as_ref()
                            .zip(source.current_power_toughness.as_ref())
                            .is_some_and(|(e, cur)| {
                                e.power > cur.power || e.toughness > cur.toughness
                            }))
                {
                    return Ok(());
                }
                if !relation.matches_appearance(source, controller, entering) {
                    return Ok(());
                }
                let player = entering
                    .object
                    .controller()
                    .ok_or(TriggerError::InvalidWorld)?;
                let related = match value {
                    RulesOccurrence::ZoneChange(change) => TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                    RulesOccurrence::TokenCreated { creator, after } => {
                        TriggerRelated::TokenCreated {
                            creator: *creator,
                            after: after.as_ref().clone(),
                        }
                    }
                    _ => return Ok(()),
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(player),
                    related,
                });
            }
        },
        TriggerEvent::Targeted { .. } | TriggerEvent::TargetedByController { .. } => {
            let (subject, by_opponent, by_controller, spell_only, instant_or_sorcery_only) =
                if let TriggerEvent::Targeted {
                    subject,
                    by_opponent,
                    spell_only,
                    instant_or_sorcery_only,
                } = event
                {
                    (
                        *subject,
                        *by_opponent,
                        false,
                        *spell_only,
                        *instant_or_sorcery_only,
                    )
                } else if let TriggerEvent::TargetedByController {
                    subject,
                    spell_only,
                    instant_or_sorcery_only,
                } = event
                {
                    (*subject, false, true, *spell_only, *instant_or_sorcery_only)
                } else {
                    return Ok(());
                };
            let RulesOccurrence::PermanentTargeted {
                object: target,
                entry,
                controller: targeter,
                instant_or_sorcery,
            } = value
            else {
                return Ok(());
            };
            if by_opponent && *targeter == controller {
                return Ok(());
            }
            if by_controller && *targeter != controller {
                return Ok(());
            }
            if instant_or_sorcery_only && !*instant_or_sorcery {
                return Ok(());
            }
            if spell_only
                && !after.window.entries().iter().any(|item| {
                    item.id() == *entry
                        && matches!(
                            item.kind(),
                            crate::stack_window::StackKind::CardSpell(_)
                                | crate::stack_window::StackKind::SpellCopy { .. }
                        )
                })
            {
                return Ok(());
            }
            let watched = if subject == ObjectFilter::self_object() {
                *target == source.object.id()
            } else if subject == ObjectFilter::any().attached_to_self() {
                after.evaluated.get(source.object.id()).is_some_and(|aura| {
                    aura.attached_to == Some(crate::targets::TargetRef::Object(*target))
                })
            } else {
                after.evaluated.get(*target).is_some_and(|candidate| {
                    matches_evaluated_permanent(subject, source, controller, candidate, after)
                })
            };
            if watched {
                let (related_player, related) = if by_controller || instant_or_sorcery_only {
                    (controller, TriggerRelated::TargetedPermanent(*target))
                } else {
                    (*targeter, TriggerRelated::TargetingEntry(*entry))
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(related_player),
                    related,
                });
            }
        }
        TriggerEvent::SpellCast {
            caster,
            filter,
            self_cast,
            include_copy,
            min_mana_value,
        } => match value {
            RulesOccurrence::CopyCreated {
                entry,
                controller: copying_player,
                copiable_card_types,
                ..
            } if *include_copy
                && player_scope_matches(*caster, source, controller, *copying_player, after)
                && filter.matches_card_types(copiable_card_types) =>
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(*copying_player),
                    related: TriggerRelated::SpellCopied(*entry),
                });
            }
            RulesOccurrence::CastCompleted(cast) => {
                if !after.window.participants().contains(&cast.controller()) {
                    return Err(TriggerError::InvalidWorld);
                }
                let matched = if *self_cast {
                    let same = source.object.id() == cast.spell().object.id();
                    if same && controller != cast.controller() {
                        return Err(TriggerError::InvalidWorld);
                    }
                    same
                } else {
                    let caster_ok =
                        player_scope_matches(*caster, source, controller, cast.controller(), after);
                    if !caster_ok {
                        false
                    } else if let Some(min_mv) = min_mana_value
                        && *cast.mana_value() < i64::from(*min_mv).into()
                    {
                        false
                    } else {
                        matches_cast_spell_filter(*filter, source, controller, cast, after)
                    }
                };
                if matched {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(cast.controller()),
                        related: TriggerRelated::SpellCast(cast.clone()),
                    });
                }
            }
            _ => {}
        },
        TriggerEvent::Enters {
            subject,
            from_zone,
            cast_origin,
            attached_to_creature,
        } => {
            let creature_enters_suppressed = after.evaluated.objects.values().any(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && permanent.has_battlefield_rule(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::SuppressEntersTriggers,
                                ..
                            }
                        )
                    })
            });
            if creature_enters_suppressed {
                let entering_is_creature = match value {
                    RulesOccurrence::ZoneChange(change)
                        if change.after.object.zone() == ObjectZone::Battlefield =>
                    {
                        change.after.types.contains(&CardType::Creature)
                    }
                    RulesOccurrence::TokenCreated { after: created, .. } => {
                        created.types.contains(&CardType::Creature)
                    }
                    _ => false,
                };
                if entering_is_creature {
                    return Ok(());
                }
            }
            if let Some(origin) = cast_origin {
                match (origin, value) {
                    (
                        CastEntryKind::NotCast,
                        RulesOccurrence::TokenCreated {
                            creator,
                            after: created,
                            ..
                        },
                    ) => {
                        if subject.matches_appearance(source, controller, created) {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: created.object.controller().or(Some(controller)),
                                related: TriggerRelated::TokenCreated {
                                    creator: *creator,
                                    after: (**created).clone(),
                                },
                            });
                        }
                    }
                    (_, RulesOccurrence::ZoneChange(change))
                        if change.after.object.zone() == ObjectZone::Battlefield
                            && subject.matches_appearance(source, controller, &change.after) =>
                    {
                        let is_self = *subject == ObjectFilter::self_object();
                        let ok = match *origin {
                            CastEntryKind::Cast { from_hand } => {
                                change.before.cast_origin.is_some_and(|(caster, hand)| {
                                    (!is_self || caster == controller) && (!from_hand || hand)
                                })
                            }
                            CastEntryKind::NotCast => change.before.cast_origin.is_none(),
                            CastEntryKind::Kicked => {
                                change.before.object.zone() == ObjectZone::Stack
                                    && after
                                        .characteristics
                                        .kicked
                                        .contains(&change.after.object.id())
                            }
                            CastEntryKind::Evoked => {
                                change.before.object.zone() == ObjectZone::Stack
                                    && after
                                        .characteristics
                                        .evoked
                                        .contains(&change.after.object.id())
                            }
                        };
                        if ok {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: change
                                    .after
                                    .object
                                    .controller()
                                    .or(Some(controller)),
                                related: TriggerRelated::ZoneChange {
                                    before: change.before.clone(),
                                    after: change.after.clone(),
                                },
                            });
                        }
                    }
                    _ => {}
                }
                return Ok(());
            }
            if *from_zone != TriggerFromZone::Any {
                if let RulesOccurrence::ZoneChange(change) = value
                    && change.after.object.zone() == ObjectZone::Battlefield
                    && match from_zone {
                        TriggerFromZone::Graveyard => {
                            matches!(change.before.object.zone(), ObjectZone::Graveyard(_))
                        }
                        TriggerFromZone::ControllerGraveyard => {
                            change.before.object.zone() == ObjectZone::Graveyard(controller)
                        }
                        TriggerFromZone::OpponentGraveyard => {
                            matches!(change.before.object.zone(), ObjectZone::Graveyard(owner) if owner != controller)
                        }
                        TriggerFromZone::Hand => {
                            matches!(change.before.object.zone(), ObjectZone::Hand(_))
                        }
                        TriggerFromZone::Exile => change.before.object.zone() == ObjectZone::Exile,
                        TriggerFromZone::Library => {
                            matches!(change.before.object.zone(), ObjectZone::Library(_))
                        }
                        TriggerFromZone::Battlefield => {
                            change.before.object.zone() == ObjectZone::Battlefield
                        }
                        TriggerFromZone::NotGraveyardOrExile => !matches!(
                            change.before.object.zone(),
                            ObjectZone::Graveyard(_) | ObjectZone::Exile | ObjectZone::Battlefield
                        ),
                        TriggerFromZone::Any => true,
                    }
                    && subject.matches_appearance(source, controller, &change.after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    });
                }
                return Ok(());
            }
            let entering = match value {
                RulesOccurrence::ZoneChange(change)
                    if change.after.object.zone() == ObjectZone::Battlefield
                        && change.before.object.zone() != ObjectZone::Battlefield =>
                {
                    &change.after
                }
                RulesOccurrence::TokenCreated { after: created, .. } => created.as_ref(),
                _ => return Ok(()),
            };
            if *attached_to_creature
                && !matches!(
                    entering.attached_to,
                    Some(crate::targets::TargetRef::Object(host))
                        if after.evaluated.get(host).is_some_and(|view| view.is_creature())
                )
            {
                return Ok(());
            }
            if !subject.matches_appearance(source, controller, entering) {
                return Ok(());
            }
            let player = entering
                .object
                .controller()
                .ok_or(TriggerError::InvalidWorld)?;
            let related = match value {
                RulesOccurrence::ZoneChange(change) => TriggerRelated::ZoneChange {
                    before: change.before.clone(),
                    after: change.after.clone(),
                },
                RulesOccurrence::TokenCreated { creator, after } => TriggerRelated::TokenCreated {
                    creator: *creator,
                    after: after.as_ref().clone(),
                },
                _ => return Ok(()),
            };
            members.push(TriggerCauseMember {
                occurrence: occurrence.id,
                related_player: Some(player),
                related,
            });
        }
        TriggerEvent::Exiled { subject, from_zone } => {
            if let RulesOccurrence::ZoneChange(change) = value
                && change.after.object.zone() == ObjectZone::Exile
                && match from_zone {
                    TriggerFromZone::Any => true,
                    TriggerFromZone::Graveyard => {
                        matches!(change.before.object.zone(), ObjectZone::Graveyard(_))
                    }
                    TriggerFromZone::ControllerGraveyard => {
                        change.before.object.zone() == ObjectZone::Graveyard(controller)
                    }
                    TriggerFromZone::OpponentGraveyard => {
                        matches!(change.before.object.zone(), ObjectZone::Graveyard(owner) if owner != controller)
                    }
                    TriggerFromZone::Hand => {
                        matches!(change.before.object.zone(), ObjectZone::Hand(_))
                    }
                    TriggerFromZone::Exile => change.before.object.zone() == ObjectZone::Exile,
                    TriggerFromZone::Library => {
                        matches!(change.before.object.zone(), ObjectZone::Library(_))
                    }
                    TriggerFromZone::Battlefield => {
                        change.before.object.zone() == ObjectZone::Battlefield
                    }
                    TriggerFromZone::NotGraveyardOrExile => !matches!(
                        change.before.object.zone(),
                        ObjectZone::Graveyard(_) | ObjectZone::Exile
                    ),
                }
                && subject.matches_appearance(source, controller, &change.before)
            {
                let related_player = change
                    .before
                    .object
                    .controller()
                    .unwrap_or_else(|| change.before.object.owner());
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(related_player),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::ReturnedToHand { subject } => {
            if let RulesOccurrence::ZoneChange(change) = value
                && change.before.object.zone() == ObjectZone::Battlefield
                && matches!(change.after.object.zone(), ObjectZone::Hand(_))
                && subject.matches_appearance(source, controller, &change.before)
            {
                let related_player = change
                    .before
                    .object
                    .controller()
                    .unwrap_or_else(|| change.before.object.owner());
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(related_player),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::LeavesGraveyard {
            subject,
            player: relation,
        } => {
            if let RulesOccurrence::ZoneChange(change) = value
                && let ObjectZone::Graveyard(owner) = change.before.object.zone()
                && change.after.object.zone() != ObjectZone::Graveyard(owner)
                && player_matches(*relation, controller, owner, after)
                && subject.matches_appearance(source, controller, &change.before)
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(owner),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::LeavesBattlefield { subject }
        | TriggerEvent::LeavesBattlefieldWithoutDying { subject } => {
            let without_dying = matches!(event, TriggerEvent::LeavesBattlefieldWithoutDying { .. });
            match value {
                RulesOccurrence::PermanentLeftGame { before }
                    if subject.matches_appearance(source, controller, before) =>
                {
                    let departing_controller = before
                        .object
                        .controller()
                        .ok_or(TriggerError::InvalidWorld)?;
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(departing_controller),
                        related: TriggerRelated::LeftGame(before.object.id()),
                    });
                }
                RulesOccurrence::ZoneChange(change)
                    if change.before.object.zone() == ObjectZone::Battlefield
                        && change.after.object.zone() != ObjectZone::Battlefield
                        && (!without_dying
                            || !matches!(change.after.object.zone(), ObjectZone::Graveyard(_)))
                        && subject.matches_appearance(source, controller, &change.before) =>
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(
                            change
                                .before
                                .object
                                .controller()
                                .ok_or(TriggerError::InvalidWorld)?,
                        ),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    });
                }
                _ => {}
            }
        }
        TriggerEvent::Dies { subject, mode } => {
            let RulesOccurrence::ZoneChange(change) = value else {
                return Ok(());
            };
            if change.before.object.zone() != ObjectZone::Battlefield
                || !(matches!(change.after.object.zone(), ObjectZone::Graveyard(_))
                    || (*mode == DiesTriggerMode::OrCommandZone
                        && change.after.object.zone() == ObjectZone::Command))
            {
                return Ok(());
            }
            let (matched, related_player) = match mode {
                DiesTriggerMode::ToControllerGraveyard => {
                    let ok = change.after.object.zone() == ObjectZone::Graveyard(controller)
                        && change.before.is_creature()
                        && subject.matches_appearance(source, controller, &change.before);
                    (ok, Some(controller))
                }
                DiesTriggerMode::DamagedBySource => {
                    let ok = change.before.is_creature()
                        && after
                            .characteristics
                            .creature_damage_pairs
                            .as_ref()
                            .is_some_and(|pairs| {
                                pairs.contains(&(source.object.id(), change.before.object.id()))
                            });
                    (ok, None)
                }
                DiesTriggerMode::DamagedByAttachedHost => {
                    let ok = change.before.is_creature()
                        && subject.matches_appearance(source, controller, &change.before)
                        && matches!(
                            source.attached_to,
                            Some(crate::targets::TargetRef::Object(host))
                                if after
                                    .characteristics
                                    .creature_damage_pairs
                                    .as_ref()
                                    .is_some_and(|pairs| {
                                        pairs.contains(&(host, change.before.object.id()))
                                    })
                        );
                    (ok, None)
                }
                DiesTriggerMode::Blitz => {
                    let ok = source.object.id() == change.before.object.id();
                    (ok, None)
                }
                DiesTriggerMode::Permanent | DiesTriggerMode::OrCommandZone => {
                    let ok = subject.matches_appearance(source, controller, &change.before);
                    (ok, None)
                }
                DiesTriggerMode::Creature => {
                    let ok = (change.before.is_creature()
                        || (*subject
                            == ObjectFilter::creature().controlled().nontoken().or_self()
                            && source.object.id() == change.before.object.id()))
                        && subject.matches_appearance(source, controller, &change.before);
                    (ok, None)
                }
            };
            if matched {
                let player = match related_player {
                    Some(p) => p,
                    None => change
                        .before
                        .object
                        .controller()
                        .ok_or(TriggerError::InvalidWorld)?,
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(player),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::PutIntoGraveyard {
            subject,
            player: relation,
            from_library,
            own_turn_only,
        } => {
            let RulesOccurrence::ZoneChange(change) = value else {
                return Ok(());
            };
            if *own_turn_only && after.window.active_player() != controller {
                return Ok(());
            }
            let ObjectZone::Graveyard(owner) = change.after.object.zone() else {
                return Ok(());
            };
            if *from_library && change.before.object.zone() != ObjectZone::Library(owner) {
                return Ok(());
            }
            if change.after.object.card_id().is_some()
                && player_matches(*relation, controller, owner, after)
                && subject.matches_appearance(source, controller, &change.after)
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(owner),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::DealsDamage {
            source: source_filter,
            recipient,
            combat_only,
        } => {
            let RulesOccurrence::DamageDealt {
                damage,
                kind,
                source_subtypes,
                ..
            } = value
            else {
                return Ok(());
            };
            if *combat_only && *kind != DamageKind::Combat {
                return Ok(());
            }
            if *recipient == DamageTriggerRecipient::Controller {
                if damage.target == DamageRecipient::Player(controller) {
                    let subtype_ok = match source_filter {
                        ObjectFilter::Clause(c) => match c.subtypes {
                            SubtypeFilter::Has(k) => source_subtypes.contains(&k),
                            SubtypeFilter::Any => true,
                            _ => false,
                        },
                        ObjectFilter::Either(_, _) => false,
                    };
                    if subtype_ok {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::DamageSource(damage.source.id()),
                        });
                    }
                }
                return Ok(());
            }
            let crate::damage::DamageSourceId::Object(dealer) = damage.source.id() else {
                return Ok(());
            };
            let source_ok = if *source_filter == ObjectFilter::self_object() {
                dealer == source.object.id()
            } else if *source_filter == ObjectFilter::any().attached_to_self() {
                source.attached_to == Some(crate::targets::TargetRef::Object(dealer))
            } else {
                after.evaluated.get(dealer).is_some_and(|view| {
                    matches_evaluated_permanent(*source_filter, source, controller, view, after)
                })
            };
            if !source_ok {
                return Ok(());
            }
            let is_subtype_filtered = matches!(
                source_filter,
                ObjectFilter::Clause(c) if matches!(c.subtypes, SubtypeFilter::Has(_))
            );
            match *recipient {
                DamageTriggerRecipient::Any => match damage.target {
                    DamageRecipient::Player(player) => {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::CombatDamageToPlayer {
                                source: dealer,
                                amount: damage.amount.clone(),
                                player,
                                attempt: damage.attempt,
                            },
                        });
                    }
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => {
                        let seat = after
                            .evaluated
                            .get(id)
                            .and_then(|p| p.controller)
                            .unwrap_or(controller);
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(seat),
                            related: TriggerRelated::DamageDealtToPermanent {
                                recipient: id,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                },
                DamageTriggerRecipient::Controller => {}
                DamageTriggerRecipient::Opponent => {
                    if let DamageRecipient::Player(player) = damage.target
                        && player != controller
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::DamageToOpponent {
                                player,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                }
                DamageTriggerRecipient::EnchantedPlayer => {
                    if let DamageRecipient::Player(player) = damage.target
                        && source.attached_to == Some(crate::targets::TargetRef::Player(player))
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::SubtypeCombatDamage {
                                creature: dealer,
                                player,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                }
                DamageTriggerRecipient::Creature => {
                    if let DamageRecipient::Creature(creature) = damage.target
                        && let Some(player) =
                            after.evaluated.get(creature).and_then(|r| r.controller)
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::Combatant(creature),
                        });
                    }
                }
                DamageTriggerRecipient::AnyPlayer => {
                    if let DamageRecipient::Player(player) = damage.target
                        && (intervening_if != Some(&TriggerCondition::DefendingPlayerHasMoreLands)
                            || after.land_count(player) > after.land_count(controller))
                    {
                        let related = if is_subtype_filtered {
                            TriggerRelated::SubtypeCombatDamage {
                                creature: dealer,
                                player,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            }
                        } else {
                            TriggerRelated::CombatDamageToPlayer {
                                source: dealer,
                                amount: damage.amount.clone(),
                                player,
                                attempt: damage.attempt,
                            }
                        };
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related,
                        });
                    }
                }
                DamageTriggerRecipient::PlayerOrPlaneswalker => match damage.target {
                    DamageRecipient::Player(player) => {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::CombatDamageToPlayer {
                                source: dealer,
                                amount: damage.amount.clone(),
                                player,
                                attempt: damage.attempt,
                            },
                        });
                    }
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => {
                        if let Some(seat) = after
                            .evaluated
                            .get(id)
                            .filter(|p| p.has_type(CardType::Planeswalker))
                            .and_then(|p| p.controller)
                        {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(seat),
                                related: TriggerRelated::CombatDamageToPlaneswalker {
                                    planeswalker: id,
                                    attempt: damage.attempt,
                                    amount: damage.amount.clone(),
                                },
                            });
                        }
                    }
                },
                DamageTriggerRecipient::OpponentOrPlaneswalker => match damage.target {
                    DamageRecipient::Player(player) if player != controller => {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(player),
                            related: TriggerRelated::DamageToOpponent {
                                player,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                    DamageRecipient::Planeswalker(id) => {
                        if let Some(seat) = after
                            .evaluated
                            .get(id)
                            .and_then(|p| p.controller)
                            .filter(|&s| s != controller)
                        {
                            members.push(TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: Some(seat),
                                related: TriggerRelated::CombatDamageToPlaneswalker {
                                    planeswalker: id,
                                    attempt: damage.attempt,
                                    amount: damage.amount.clone(),
                                },
                            });
                        }
                    }
                    _ => {}
                },
                DamageTriggerRecipient::ControllerOrPlaneswalker => match damage.target {
                    DamageRecipient::Player(player) if player == controller => {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::DamageSource(damage.source.id()),
                        });
                    }
                    DamageRecipient::Planeswalker(id)
                        if after.evaluated.get(id).and_then(|p| p.controller)
                            == Some(controller) =>
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::CombatDamageToPlaneswalker {
                                planeswalker: id,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                    _ => {}
                },
                DamageTriggerRecipient::Battle => {
                    if let DamageRecipient::Battle(id) = damage.target {
                        let seat = after
                            .evaluated
                            .get(id)
                            .and_then(|p| p.controller)
                            .unwrap_or(controller);
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(seat),
                            related: TriggerRelated::DamageDealtToPermanent {
                                recipient: id,
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                }
                DamageTriggerRecipient::SelfObject => {
                    let hit_self = matches!(
                        damage.target,
                        DamageRecipient::Creature(id)
                            | DamageRecipient::Planeswalker(id)
                            | DamageRecipient::Battle(id)
                            if id == source.object.id()
                    );
                    if hit_self {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(controller),
                            related: TriggerRelated::DamageDealtToPermanent {
                                recipient: source.object.id(),
                                attempt: damage.attempt,
                                amount: damage.amount.clone(),
                            },
                        });
                    }
                }
            }
        }
        TriggerEvent::DealsNoncombatDamage {
            source: source_filter,
            recipient,
        } => {
            let RulesOccurrence::DamageDealt { damage, kind, .. } = value else {
                return Ok(());
            };
            if *kind != DamageKind::Noncombat {
                return Ok(());
            }
            let crate::damage::DamageSourceId::Object(dealer) = damage.source.id() else {
                return Ok(());
            };
            let source_ok = if *source_filter == ObjectFilter::self_object() {
                dealer == source.object.id()
            } else if *source_filter == ObjectFilter::any().attached_to_self() {
                source.attached_to == Some(crate::targets::TargetRef::Object(dealer))
            } else {
                after.evaluated.get(dealer).is_some_and(|view| {
                    matches_evaluated_permanent(*source_filter, source, controller, view, after)
                })
            };
            if !source_ok {
                return Ok(());
            }
            let rec_ok = match *recipient {
                DamageTriggerRecipient::Any => true,
                DamageTriggerRecipient::AnyPlayer => {
                    matches!(damage.target, DamageRecipient::Player(_))
                }
                DamageTriggerRecipient::Opponent => {
                    matches!(damage.target, DamageRecipient::Player(p) if p != controller)
                }
                DamageTriggerRecipient::Controller => {
                    damage.target == DamageRecipient::Player(controller)
                }
                DamageTriggerRecipient::EnchantedPlayer => matches!(
                    damage.target,
                    DamageRecipient::Player(p)
                        if source.attached_to == Some(crate::targets::TargetRef::Player(p))
                ),
                DamageTriggerRecipient::Creature => {
                    matches!(damage.target, DamageRecipient::Creature(_))
                }
                DamageTriggerRecipient::Battle => {
                    matches!(damage.target, DamageRecipient::Battle(_))
                }
                DamageTriggerRecipient::PlayerOrPlaneswalker => matches!(
                    damage.target,
                    DamageRecipient::Player(_) | DamageRecipient::Planeswalker(_)
                ),
                DamageTriggerRecipient::OpponentOrPlaneswalker => match damage.target {
                    DamageRecipient::Player(p) => p != controller,
                    DamageRecipient::Planeswalker(id) => after
                        .evaluated
                        .get(id)
                        .and_then(|p| p.controller)
                        .is_some_and(|c| c != controller),
                    _ => false,
                },
                DamageTriggerRecipient::ControllerOrPlaneswalker => match damage.target {
                    DamageRecipient::Player(p) => p == controller,
                    DamageRecipient::Planeswalker(id) => {
                        after.evaluated.get(id).and_then(|p| p.controller) == Some(controller)
                    }
                    _ => false,
                },
                DamageTriggerRecipient::SelfObject => matches!(
                    damage.target,
                    DamageRecipient::Creature(id)
                        | DamageRecipient::Planeswalker(id)
                        | DamageRecipient::Battle(id)
                        if id == source.object.id()
                ),
            };
            if rec_ok {
                let (related_player, related) = match damage.target {
                    DamageRecipient::Player(p) => (
                        Some(p),
                        TriggerRelated::CombatDamageToPlayer {
                            source: dealer,
                            amount: damage.amount.clone(),
                            player: p,
                            attempt: damage.attempt,
                        },
                    ),
                    DamageRecipient::Creature(id)
                    | DamageRecipient::Planeswalker(id)
                    | DamageRecipient::Battle(id) => (
                        after.evaluated.get(id).and_then(|p| p.controller),
                        TriggerRelated::DamageDealtToPermanent {
                            recipient: id,
                            attempt: damage.attempt,
                            amount: damage.amount.clone(),
                        },
                    ),
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player,
                    related,
                });
            }
        }
        TriggerEvent::AbilityActivated {
            player: relation,
            source_filter,
        } => {
            if let RulesOccurrence::ActivatedAbility {
                source: act_src, ..
            } = value
                && let Some(actor) = act_src.controller()
                && player_matches(*relation, controller, actor, after)
                && after.evaluated.get(act_src.id()).is_some_and(|view| {
                    matches_evaluated_permanent(*source_filter, source, controller, view, after)
                })
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(actor),
                    related: TriggerRelated::ManaSource(act_src.id()),
                });
            }
        }
        TriggerEvent::BecomesAttached { subject } => {
            if let RulesOccurrence::AttachmentChanged {
                source: att_id,
                before,
                after: Some(host_ref),
            } = value
                && before.as_ref() != Some(host_ref)
                && after.evaluated.get(*att_id).is_some_and(|view| {
                    matches_evaluated_permanent(*subject, source, controller, view, after)
                })
            {
                let host_player = match host_ref {
                    crate::targets::TargetRef::Player(p) => Some(*p),
                    crate::targets::TargetRef::Object(hid) => {
                        after.evaluated.get(*hid).and_then(|v| v.controller)
                    }
                    crate::targets::TargetRef::Stack(_) => None,
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: host_player.or(Some(controller)),
                    related: TriggerRelated::ManaSource(*att_id),
                });
            }
        }
        TriggerEvent::PutIntoLibrary { subject } => {
            if let RulesOccurrence::ZoneChange(change) = value
                && matches!(change.after.object.zone(), ObjectZone::Library(_))
                && subject.matches_appearance(source, controller, &change.before)
            {
                let owner = match change.after.object.zone() {
                    ObjectZone::Library(p) => p,
                    _ => controller,
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(owner),
                    related: TriggerRelated::ZoneChange {
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                });
            }
        }
        TriggerEvent::Exploits { subject } => {
            if let RulesOccurrence::KeywordActionPerformed {
                player: actor,
                action: PlayerTriggerAction::Sacrificed(_),
            } = value
                && *subject == ObjectFilter::self_object()
                && *actor == controller
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(*actor),
                    related: TriggerRelated::StateTriggerSource(source.object.id()),
                });
            }
        }
        TriggerEvent::DayNightChanged => {
            if let RulesOccurrence::DayNightChanged { before, after: nxt } = value
                && before != nxt
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(controller),
                    related: TriggerRelated::StateTriggerSource(source.object.id()),
                });
            }
        }
        TriggerEvent::DealtDamage {
            subject,
            linked_player,
        } => {
            let RulesOccurrence::DamageDealt { damage, .. } = value else {
                return Ok(());
            };
            if *linked_player {
                let DamageRecipient::Player(recipient) = damage.target else {
                    return Ok(());
                };
                let Some(&[first, second]) = after
                    .characteristics
                    .chosen_entry_players
                    .get(&source.object.id())
                else {
                    return Ok(());
                };
                let player = if recipient == first {
                    second
                } else if recipient == second {
                    first
                } else {
                    return Ok(());
                };
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(player),
                    related: TriggerRelated::DamageReceived {
                        attempt: damage.attempt,
                        amount: damage.amount.clone(),
                    },
                });
                return Ok(());
            }
            if let DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id) = damage.target
            {
                if *subject == ObjectFilter::self_object() && id == source.object.id() {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::DamageReceived {
                            attempt: damage.attempt,
                            amount: damage.amount.clone(),
                        },
                    });
                } else if *subject == ObjectFilter::any().attached_to_self()
                    && source.attached_to == Some(crate::targets::TargetRef::Object(id))
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::AttachedCreatureDamaged {
                            creature: id,
                            attempt: damage.attempt,
                            amount: damage.amount.clone(),
                        },
                    });
                }
            }
        }
        TriggerEvent::StatusChanged { subject, kind } => match *kind {
            StatusTriggerKind::BecomesTapped => {
                if let RulesOccurrence::StatusChange { change, .. } = value
                    && change.before.zone() == ObjectZone::Battlefield
                    && change.after.zone() == ObjectZone::Battlefield
                    && change.before.id() == change.after.id()
                    && change.before.tapped() == Some(false)
                    && change.after.tapped() == Some(true)
                {
                    let obj = after
                        .appearance(change.after.id())
                        .ok_or(TriggerError::InvalidWorld)?;
                    if subject.matches_appearance(source, controller, &obj) {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(
                                obj.object.controller().ok_or(TriggerError::InvalidWorld)?,
                            ),
                            related: TriggerRelated::StatusChange(change.clone()),
                        });
                    }
                }
            }
            StatusTriggerKind::BecomesUntapped => {
                if let RulesOccurrence::StatusChange { change, .. } = value
                    && change.before.zone() == ObjectZone::Battlefield
                    && change.after.zone() == ObjectZone::Battlefield
                    && change.before.id() == change.after.id()
                    && change.before.tapped() == Some(true)
                    && change.after.tapped() == Some(false)
                {
                    let obj = after
                        .appearance(change.after.id())
                        .ok_or(TriggerError::InvalidWorld)?;
                    if subject.matches_appearance(source, controller, &obj) {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(
                                obj.object.controller().ok_or(TriggerError::InvalidWorld)?,
                            ),
                            related: TriggerRelated::StatusChange(change.clone()),
                        });
                    }
                }
            }
            StatusTriggerKind::TurnsFaceUp => {
                if let RulesOccurrence::TurnedFaceUp {
                    before,
                    after: face_up,
                    ..
                }
                | RulesOccurrence::TurnedFaceUpByEffect {
                    before,
                    after: face_up,
                } = value
                    && face_up.object.id() == source.object.id()
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::StatusChange(ObjectStatusChange {
                            before: before.object.clone(),
                            after: face_up.object.clone(),
                        }),
                    });
                }
            }
            StatusTriggerKind::Transforms => {
                if let RulesOccurrence::Transformed {
                    after: transformed, ..
                } = value
                    && transformed.object.id() == source.object.id()
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            StatusTriggerKind::Phased { out: expected } => {
                if let RulesOccurrence::Phased { objects, out } = value
                    && expected == *out
                    && objects.contains(&source.object.id())
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::Phased(source.object.id()),
                    });
                }
            }
            StatusTriggerKind::Mutated => {
                if let RulesOccurrence::Mutated { permanent } = value
                    && *permanent == source.object.id()
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::Mutated(*permanent),
                    });
                }
            }
            StatusTriggerKind::ClassLevelReached(expected) => {
                if let RulesOccurrence::ClassLevelReached { object, level } = value
                    && *object == source.object.id()
                    && *level == expected
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            StatusTriggerKind::CaseSolved => {
                if let RulesOccurrence::CaseSolved { object } = value
                    && *object == source.object.id()
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            StatusTriggerKind::BecomesMonstrous => {
                if let RulesOccurrence::BecameMonstrous { object, .. } = value
                    && after.evaluated.get(*object).is_some_and(|v| {
                        matches_evaluated_permanent(*subject, source, controller, v, after)
                    })
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: after.evaluated.get(*object).and_then(|v| v.controller),
                        related: TriggerRelated::StateTriggerSource(*object),
                    });
                }
            }
            StatusTriggerKind::Crewed => {
                if let RulesOccurrence::PowerAbilityResolved {
                    source: vehicle,
                    kind: crate::activated::PowerAbility::Crew,
                    contributors,
                    ..
                } = value
                {
                    let matched_obj = if after.evaluated.get(*vehicle).is_some_and(|v| {
                        matches_evaluated_permanent(*subject, source, controller, v, after)
                    }) {
                        Some(*vehicle)
                    } else {
                        contributors.iter().copied().find(|cid| {
                            after.evaluated.get(*cid).is_some_and(|v| {
                                matches_evaluated_permanent(*subject, source, controller, v, after)
                            })
                        })
                    };
                    if let Some(id) = matched_obj {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: after.evaluated.get(id).and_then(|v| v.controller),
                            related: TriggerRelated::ManaSource(id),
                        });
                    }
                }
            }
            StatusTriggerKind::Explored => {
                if let RulesOccurrence::Explored {
                    source: exp_src,
                    controller: exp_ctrl,
                } = value
                    && after.evaluated.get(exp_src.id()).is_some_and(|v| {
                        matches_evaluated_permanent(*subject, source, controller, v, after)
                    })
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*exp_ctrl),
                        related: TriggerRelated::ManaSource(exp_src.id()),
                    });
                }
            }
            StatusTriggerKind::Regenerated => {
                if let RulesOccurrence::PermanentRegenerated { object: reg_id, .. } = value
                    && after.evaluated.get(*reg_id).is_some_and(|v| {
                        matches_evaluated_permanent(*subject, source, controller, v, after)
                    })
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: after.evaluated.get(*reg_id).and_then(|v| v.controller),
                        related: TriggerRelated::ManaSource(*reg_id),
                    });
                }
            }
            StatusTriggerKind::Exerted => {
                if let RulesOccurrence::KeywordActionPerformed {
                    player: actor,
                    action: PlayerTriggerAction::Exerted,
                } = value
                    && *subject == ObjectFilter::self_object()
                    && *actor == controller
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
        },
        TriggerEvent::PlayerAction {
            player: relation,
            action,
        } => match *action {
            PlayerTriggerAction::Sacrificed(filter) => {
                if let RulesOccurrence::ZoneChange(change) = value
                    && change.before.object.zone() == ObjectZone::Battlefield
                    && filter.matches_appearance(source, controller, &change.before)
                {
                    let actor = match change.reason {
                        ZoneReason::Sacrifice { player } => Some(player),
                        ZoneReason::SacrificeCost => change.before.object.controller(),
                        ZoneReason::StateBased(reasons)
                            if reasons.contains(StateBasedMoveReason::CompletedSaga) =>
                        {
                            change.before.object.controller()
                        }
                        _ => None,
                    };
                    if let Some(seat) =
                        actor.filter(|seat| player_matches(*relation, controller, *seat, after))
                    {
                        members.push(TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(seat),
                            related: TriggerRelated::ZoneChange {
                                before: change.before.clone(),
                                after: change.after.clone(),
                            },
                        });
                    }
                }
            }
            PlayerTriggerAction::Discarded
            | PlayerTriggerAction::DiscardedMatching(_)
            | PlayerTriggerAction::DiscardedByOpponent(_)
            | PlayerTriggerAction::Cycled { .. } => {
                let RulesOccurrence::ZoneChange(change) = value else {
                    return Ok(());
                };
                let (player, cycled) = match change.reason {
                    ZoneReason::Discard { player } | ZoneReason::TransmuteDiscard { player } => {
                        (player, false)
                    }
                    ZoneReason::CycleDiscard { player } => (player, true),
                    _ => return Ok(()),
                };
                let matched = match *action {
                    PlayerTriggerAction::Cycled { self_only: true } => {
                        cycled && change.after.object.id() == source.object.id()
                    }
                    PlayerTriggerAction::Cycled { self_only: false } => {
                        cycled && player_matches(*relation, controller, player, after)
                    }
                    PlayerTriggerAction::Discarded => {
                        player_matches(*relation, controller, player, after)
                    }
                    PlayerTriggerAction::DiscardedMatching(filter) => {
                        player_matches(*relation, controller, player, after)
                            && filter.matches_appearance(source, controller, &change.after)
                    }
                    PlayerTriggerAction::DiscardedByOpponent(opt_filter) => {
                        let cause_controller = match event_cause {
                            EventCause::Instruction { entry, .. } => after
                                .window
                                .entries()
                                .iter()
                                .chain(after.window.resolving())
                                .find(|item| item.id() == *entry)
                                .map(crate::stack_window::StackEntry::controller),
                            _ => None,
                        };
                        player_matches(*relation, controller, player, after)
                            && cause_controller.is_some_and(|actor| actor != player)
                            && opt_filter.is_none_or(|filter| {
                                filter.matches_appearance(source, controller, &change.after)
                            })
                    }
                    _ => false,
                };
                if matched {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(player),
                        related: TriggerRelated::Discard {
                            player,
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    });
                }
            }
            PlayerTriggerAction::TokenCreated => {
                if let RulesOccurrence::TokenCreated {
                    creator,
                    after: appearance,
                } = value
                    && player_matches(*relation, controller, *creator, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*creator),
                        related: TriggerRelated::TokenCreated {
                            creator: *creator,
                            after: appearance.as_ref().clone(),
                        },
                    });
                }
            }
            PlayerTriggerAction::DrewCard { nth_each_turn } => match (nth_each_turn, value) {
                (Some(ordinal), RulesOccurrence::CardDrawOrdinal { player, number })
                    if *player == controller && *number == i64::from(ordinal.get()).into() =>
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::CardDrawn { player: controller },
                    });
                }
                (None, RulesOccurrence::ZoneChange(change))
                    if let ZoneReason::Draw { player } = change.reason
                        && player_matches(*relation, controller, player, after) =>
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(player),
                        related: TriggerRelated::CardDrawn { player },
                    });
                }
                _ => {}
            },
            PlayerTriggerAction::Scried | PlayerTriggerAction::Surveilled => {
                let expected = if matches!(action, PlayerTriggerAction::Scried) {
                    crate::effects::LibraryActionKind::Scry
                } else {
                    crate::effects::LibraryActionKind::Surveil
                };
                if let RulesOccurrence::LibraryAction(lib) = value
                    && lib.requested.is_positive()
                    && lib.kind == expected
                    && player_matches(*relation, controller, lib.player, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(lib.player),
                        related: TriggerRelated::LibraryAction(lib.clone()),
                    });
                }
            }
            PlayerTriggerAction::ArtifactTappedForMana => {
                if let RulesOccurrence::ArtifactTappedForMana {
                    source: artifact,
                    actor,
                } = value
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::ManaSource(*artifact),
                    });
                }
            }
            PlayerTriggerAction::PermanentTappedForMana(filter) => {
                if let RulesOccurrence::PermanentTappedForMana {
                    source: perm,
                    actor,
                    appearance,
                } = value
                    && player_matches(*relation, controller, *actor, after)
                    && filter.matches_appearance(source, controller, appearance)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::ManaSource(*perm),
                    });
                }
            }
            PlayerTriggerAction::SagaFinalChapterResolved => {
                if let RulesOccurrence::FinalSagaChapterCompleted {
                    saga,
                    controller: owner,
                    mana_value,
                } = value
                    && player_matches(*relation, controller, *owner, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::FinalSagaChapter {
                            saga: *saga,
                            mana_value: mana_value.clone(),
                        },
                    });
                }
            }
            PlayerTriggerAction::CommittedCrime => {
                if let RulesOccurrence::PermanentTargeted {
                    object,
                    controller: actor,
                    ..
                } = value
                    && player_matches(*relation, controller, *actor, after)
                    && after
                        .evaluated
                        .get(*object)
                        .and_then(|o| o.controller)
                        .is_some_and(|owner| owner != *actor)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::ManaSource(*object),
                    });
                }
            }
            PlayerTriggerAction::LandPlayed => {
                if let RulesOccurrence::ZoneChange(change) = value
                    && matches!(change.reason, ZoneReason::LandPlay)
                    && let Some(actor) = change.after.object.controller()
                    && player_matches(*relation, controller, actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(actor),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    });
                }
            }
            PlayerTriggerAction::Proliferated => {
                if let RulesOccurrence::Proliferated { player: actor, .. } = value
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::Explored => {
                if let RulesOccurrence::Explored {
                    source: exp_src,
                    controller: actor,
                } = value
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::ManaSource(exp_src.id()),
                    });
                }
            }
            PlayerTriggerAction::ShuffledLibrary => {
                if let RulesOccurrence::Shuffled(rec) = value
                    && player_matches(*relation, controller, rec.player, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(rec.player),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::SearchedLibrary => {
                if let RulesOccurrence::ZoneChange(change) = value
                    && matches!(
                        change.reason,
                        ZoneReason::SearchToHand
                            | ZoneReason::SearchToGraveyard
                            | ZoneReason::SearchToExile
                            | ZoneReason::SearchToBattlefield
                    )
                    && let ObjectZone::Library(actor) = change.before.object.zone()
                    && player_matches(*relation, controller, actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::FlippedCoin { won: expected } => {
                if let RulesOccurrence::CoinFlipped { player: actor, won } = value
                    && expected.is_none_or(|w| w == *won)
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::BecameMonarch => {
                if let RulesOccurrence::MonarchChanged {
                    after: Some(actor), ..
                } = value
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::SpellCountered => {
                if let RulesOccurrence::ZoneChange(change) = value
                    && matches!(change.reason, ZoneReason::Countered)
                    && let Some(actor) = change.before.object.controller()
                    && player_matches(*relation, controller, actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(actor),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    });
                }
            }
            PlayerTriggerAction::GainedEnergy => {
                if let RulesOccurrence::PlayerCounterChanged {
                    player: actor,
                    kind: crate::effects::CounterKind::Energy,
                    before,
                    after: after_cnt,
                } = value
                    && after_cnt > before
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            PlayerTriggerAction::Saddled => {
                if let RulesOccurrence::PowerAbilityResolved {
                    source: mount,
                    kind: crate::activated::PowerAbility::Saddle,
                    ..
                } = value
                    && let Some(actor) = after.evaluated.get(*mount).and_then(|v| v.controller)
                    && player_matches(*relation, controller, actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(actor),
                        related: TriggerRelated::ManaSource(*mount),
                    });
                }
            }
            PlayerTriggerAction::RolledDie { exact } => {
                if let RulesOccurrence::DieRolled {
                    player: actor,
                    result,
                    ..
                } = value
                    && exact.is_none_or(|n| n == *result)
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                } else if let RulesOccurrence::KeywordActionPerformed {
                    player: actor,
                    action: PlayerTriggerAction::RolledDie { exact: seen },
                } = value
                    && (exact.is_none() || exact == *seen)
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
            other @ (PlayerTriggerAction::Investigated
            | PlayerTriggerAction::Connived
            | PlayerTriggerAction::Foraged
            | PlayerTriggerAction::CollectedEvidence
            | PlayerTriggerAction::Foretold
            | PlayerTriggerAction::Exerted
            | PlayerTriggerAction::Plotted
            | PlayerTriggerAction::Clashed
            | PlayerTriggerAction::Exhausted
            | PlayerTriggerAction::Evolved
            | PlayerTriggerAction::RingTemptsYou
            | PlayerTriggerAction::Expend(_)
            | PlayerTriggerAction::Populated
            | PlayerTriggerAction::Discovered
            | PlayerTriggerAction::Ventured
            | PlayerTriggerAction::CompletedDungeon
            | PlayerTriggerAction::ManifestedDread
            | PlayerTriggerAction::PaidLife
            | PlayerTriggerAction::GaveGift
            | PlayerTriggerAction::WonClash
            | PlayerTriggerAction::CopiedSpellOrAbility
            | PlayerTriggerAction::Enlisted
            | PlayerTriggerAction::Trained
            | PlayerTriggerAction::KickedSpell
            | PlayerTriggerAction::ScryToBottom
            | PlayerTriggerAction::Waterbended) => {
                if let RulesOccurrence::KeywordActionPerformed {
                    player: actor,
                    action: seen,
                } = value
                    && *seen == other
                    && player_matches(*relation, controller, *actor, after)
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*actor),
                        related: TriggerRelated::StateTriggerSource(source.object.id()),
                    });
                }
            }
        },
        TriggerEvent::LifeChanged { kind, player } => {
            if let RulesOccurrence::LifeChanged(change) = value
                && change.kind == *kind
                && player_matches(*player, controller, change.player, after)
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(change.player),
                    related: TriggerRelated::LifeChanged(change.clone()),
                });
            }
        }
        TriggerEvent::PlayerLosesGame(relation) => {
            if let RulesOccurrence::PlayerLost { player, .. } = value
                && player_matches(*relation, controller, *player, after)
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(*player),
                    related: TriggerRelated::PlayerLost(*player),
                });
            }
        }
        TriggerEvent::StepBegan { step, player } => {
            if let RulesOccurrence::StepBegan { position, active } = value
                && position.step == *step
            {
                let player_ok = match *player {
                    TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant) => true,
                    other => player_scope_matches(other, source, controller, *active, after),
                };
                if player_ok {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*active),
                        related: TriggerRelated::Step {
                            position: *position,
                            active: *active,
                        },
                    });
                }
            }
        }
        // Supplemental paper format & tabletop honor-system events (wired when those subsystems emit occurrences).
        TriggerEvent::Planar(_)
        | TriggerEvent::SchemeSetInMotion
        | TriggerEvent::ManualTabletopClaim { .. }
        | TriggerEvent::AnyOf(_) => {}
        TriggerEvent::DoorUnlocked {
            subject,
            fully_unlocked_only,
        } => {
            if let RulesOccurrence::DoorUnlocked {
                object,
                player,
                fully_unlocked,
                ..
            } = value
                && (!*fully_unlocked_only || *fully_unlocked)
                && after.evaluated.get(*object).is_some_and(|view| {
                    subject.matches_static_characteristics(view, &BTreeSet::new(), || {
                        Some(view.defined.mana_value.clone())
                    })
                })
            {
                members.push(TriggerCauseMember {
                    occurrence: occurrence.id,
                    related_player: Some(*player),
                    related: TriggerRelated::ManaSource(*object),
                });
            }
        }
    }
    if deferred_if && members.len() > start_len {
        let mut kept = Vec::with_capacity(members.len() - start_len);
        for m in members.drain(start_len..) {
            let ctx = member_event_context(&m, after);
            if check_intervening_if_with_ctx(
                intervening_if,
                ability,
                source,
                controller,
                value,
                after,
                ctx,
            ) {
                kept.push(m);
            }
        }
        members.extend(kept);
    }
    Ok(())
}

fn matches_cast_spell_filter(
    filter: SpellFilter,
    source: &SupportedAppearance,
    controller: SeatId,
    cast: &CastOccurrence,
    after: &RulesView<'_>,
) -> bool {
    let zone_ok = match filter.cast_zone {
        SpellCastZoneFilter::Any => true,
        SpellCastZoneFilter::OwnLibrary => cast.cast_from_library == Some(controller),
        SpellCastZoneFilter::Graveyard => cast.cast_from_graveyard,
        SpellCastZoneFilter::Exile => cast.cast_from_exile,
        SpellCastZoneFilter::Hand => cast.cast_from_hand,
        SpellCastZoneFilter::NotOwnersHand => !cast
            .spell()
            .cast_origin
            .is_some_and(|(_, from_hand)| from_hand),
    };
    if !zone_ok {
        return false;
    }
    if filter.target_shape == SpellTargetShape::TargetingOnlySource {
        let targeting_only = after
            .window
            .entries()
            .iter()
            .find(|entry| entry.id() == cast.entry())
            .is_some_and(|entry| {
                let targets = entry.targets().targets();
                !targets.is_empty()
                    && targets.iter().all(|target| {
                        target.choice().target
                            == crate::targets::TargetRef::Object(source.object.id())
                    })
            });
        if !targeting_only {
            return false;
        }
    }
    if let Some(ordinal) = filter.ordinal_this_turn
        && !cast
            .ordinal_this_turn
            .as_ref()
            .is_some_and(|actual| *actual == i64::from(ordinal.get()).into())
    {
        return false;
    }
    if !filter.matches_spell_appearance(cast.spell()) {
        return false;
    }
    match filter.special {
        SpellSpecialFilter::None => true,
        SpellSpecialFilter::ChosenEntryCreatureType => after
            .evaluated
            .get(source.object.id())
            .and_then(|object| object.entry_creature_type)
            .is_some_and(|kind| cast.spell().creature_subtypes.contains(&kind)),
        SpellSpecialFilter::AboveExperience => after
            .characteristics
            .experience
            .get(&controller)
            .map_or(cast.mana_value().is_positive(), |count| {
                cast.mana_value() > count
            }),
        SpellSpecialFilter::CopyableInstruction | SpellSpecialFilter::AuraOrEquipment => false,
    }
}
