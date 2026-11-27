use super::*;

pub(super) fn subject_has_explicit_target(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::TargetObject { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::TargetOneOf { .. } => true,
        SubjectRefAst::Compound(items) => items.iter().any(subject_has_explicit_target),
        SubjectRefAst::InZone { subject, .. } => subject_has_explicit_target(subject),
        _ => false,
    }
}

pub(super) fn player_ref_is_explicit_target(player: PlayerRefAst) -> bool {
    matches!(
        player,
        PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
    )
}

pub(super) fn zone_filter_has_explicit_target(zone: ZoneFilter) -> bool {
    matches!(
        zone,
        ZoneFilter::TargetOpponentGraveyard
            | ZoneFilter::TargetPlayerGraveyard
            | ZoneFilter::TargetPlayerHand
            | ZoneFilter::TargetOpponentHand
            | ZoneFilter::TargetPlayerLibrary
            | ZoneFilter::TargetOpponentLibrary
    )
}

pub(super) fn condition_has_explicit_target(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_has_explicit_target(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_has_explicit_target(a) || condition_has_explicit_target(b)
        }
        ConditionAst::StatCompare { subject, .. }
        | ConditionAst::CounterCompare { subject, .. }
        | ConditionAst::HasStatus { subject, .. }
        | ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::MatchesFilter { subject, .. }
        | ConditionAst::BlockedByFilter { subject, .. }
        | ConditionAst::SubjectInZone { subject, .. }
        | ConditionAst::WasCastFromZone { subject, .. }
        | ConditionAst::PaidAltCost { subject, .. }
        | ConditionAst::AllColors(subject) => subject_has_explicit_target(subject),
        ConditionAst::TargetsSubject { subject, target } => {
            subject_has_explicit_target(subject) || subject_has_explicit_target(target)
        }
        ConditionAst::AttachedTo { subject, host } => {
            subject_has_explicit_target(subject) || subject_has_explicit_target(host)
        }
        ConditionAst::SameNameAs { subject, other }
        | ConditionAst::SharesPropertyWith { subject, other, .. } => {
            subject_has_explicit_target(subject) || subject_has_explicit_target(other)
        }
        ConditionAst::SubjectControlledBy { subject, player } => {
            subject_has_explicit_target(subject) || player_ref_is_explicit_target(*player)
        }
        ConditionAst::DamageDealtThisTurn { source, .. } => subject_has_explicit_target(source),
        ConditionAst::ControlsCount { player, .. }
        | ConditionAst::HandCount { player, .. }
        | ConditionAst::GraveyardCount { player, .. }
        | ConditionAst::LifeCompare { player, .. }
        | ConditionAst::PlayerCounterCompare { player, .. }
        | ConditionAst::IsPlayersTurn(player)
        | ConditionAst::PlayerControlsMost { player, .. }
        | ConditionAst::AttackedWithFilteredThisTurn { player, .. }
        | ConditionAst::PlayerCoinFlipOutcome { player, .. }
        | ConditionAst::DieRollCompare { player, .. }
        | ConditionAst::IsStartingPlayer(player)
        | ConditionAst::ControlsEachColor(player)
        | ConditionAst::GainedLifeAtLeastThisTurn { player, .. } => {
            player_ref_is_explicit_target(*player)
        }
        ConditionAst::ControlsMoreThan {
            player,
            than_player,
            ..
        } => player_ref_is_explicit_target(*player) || player_ref_is_explicit_target(*than_player),
        ConditionAst::LifeComparePlayers { player, other, .. } => {
            player_ref_is_explicit_target(*player) || player_ref_is_explicit_target(*other)
        }
        ConditionAst::IsPhaseOrStep { player, .. } => {
            player.is_some_and(player_ref_is_explicit_target)
        }
        ConditionAst::ZoneChangeThisTurn {
            from_zone,
            to_zone,
            controller,
            ..
        } => {
            controller.is_some_and(player_ref_is_explicit_target)
                || from_zone.is_some_and(zone_filter_has_explicit_target)
                || to_zone.is_some_and(zone_filter_has_explicit_target)
        }
        ConditionAst::TopCardMatches { zone, .. } => zone_filter_has_explicit_target(*zone),
        ConditionAst::CardWithNameInZone {
            name_filter, zone, ..
        } => {
            zone_filter_has_explicit_target(*zone)
                || matches!(
                    name_filter,
                    spellwake_oracle_compiler::ast::NameFilterAst::SameAs(subj)
                        if subject_has_explicit_target(subj)
                )
        }
        ConditionAst::NumberParity { .. }
        | ConditionAst::ColorsOfManaSpentCompare { .. }
        | ConditionAst::PriorActionOccurred { .. }
        | ConditionAst::ResolutionCountThisTurn { .. } => false,
        _ => false,
    }
}

pub(super) fn instruction_has_explicit_target(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { source, target, .. } => {
            subject_has_explicit_target(source) || subject_has_explicit_target(target)
        }
        InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Transform { subject }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::DoubleStatOrCounters { subject, .. }
        | InstructionAst::BecomeChosenCharacteristic { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::Copy { subject, .. } => subject_has_explicit_target(subject),
        InstructionAst::BecomeCopy {
            subject, source, ..
        } => subject_has_explicit_target(subject) || subject_has_explicit_target(source),
        InstructionAst::ForcedBlockByFilter {
            blockers, target, ..
        } => subject_has_explicit_target(blockers) || subject_has_explicit_target(target),
        InstructionAst::Counter { target, .. } | InstructionAst::PreventDamage { target, .. } => {
            subject_has_explicit_target(target)
        }
        InstructionAst::Attach {
            subject, target, ..
        } => subject_has_explicit_target(subject) || subject_has_explicit_target(target),
        InstructionAst::MoveCounters { from, to, .. } => {
            subject_has_explicit_target(from) || subject_has_explicit_target(to)
        }
        InstructionAst::Fight { a, b } => {
            subject_has_explicit_target(a) || subject_has_explicit_target(b)
        }
        InstructionAst::Exchange { left, right, .. } => {
            subject_has_explicit_target(left)
                || right.as_deref().is_some_and(subject_has_explicit_target)
        }
        InstructionAst::ChangeLife { player, .. }
        | InstructionAst::SetLifeTotal { player, .. }
        | InstructionAst::Draw { player, .. }
        | InstructionAst::Mill { player, .. }
        | InstructionAst::Scry { player, .. }
        | InstructionAst::Surveil { player, .. }
        | InstructionAst::Investigate { player, .. }
        | InstructionAst::Blight { player, .. }
        | InstructionAst::Discard { player, .. }
        | InstructionAst::DiscardHand { player }
        | InstructionAst::RevealHand { player }
        | InstructionAst::ShuffleLibrary { player }
        | InstructionAst::ExtraTurn { player }
        | InstructionAst::BecomeMonarch { player }
        | InstructionAst::TakeInitiative { player }
        | InstructionAst::Sacrifice { player, .. }
        | InstructionAst::GainPlayerCounters { player, .. }
        | InstructionAst::PlayerPayCost { player, .. } => {
            matches!(
                player,
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
            )
        }
        InstructionAst::CreateToken {
            controller, token, ..
        } => {
            matches!(
                controller,
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
            ) || match token {
                TokenSpecAst::Role { attached_to, .. } => subject_has_explicit_target(attached_to),
                TokenSpecAst::Copy { source, .. } => subject_has_explicit_target(source),
                _ => false,
            }
        }
        InstructionAst::Optional(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::ScopedDuration { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. } => {
            inner.iter().any(instruction_has_explicit_target)
        }
        InstructionAst::ReplaceEffectWith(rew) => {
            rew.body.iter().any(instruction_has_explicit_target)
        }
        InstructionAst::OptionalPlayer { player, body } => {
            matches!(
                player,
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent
            ) || body.iter().any(instruction_has_explicit_target)
        }
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => {
            condition_has_explicit_target(condition)
                || then_body.iter().any(instruction_has_explicit_target)
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_action,
            ..
        } => {
            matches!(
                unless_player,
                Some(PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent)
            ) || body.iter().any(instruction_has_explicit_target)
                || unless_action
                    .as_ref()
                    .is_some_and(|a| a.iter().any(instruction_has_explicit_target))
        }
        _ => false,
    }
}

pub(super) fn condition_checks_last_action(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_checks_last_action(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_checks_last_action(a) || condition_checks_last_action(b)
        }
        ConditionAst::WonContest => true,
        ConditionAst::EventOccurredThisTurn { event } => {
            let low = event.trim().to_ascii_lowercase();
            low.contains("this way")
                || low.contains("the coin")
                || low.contains("comes up ")
                || low.contains("rolled ")
                || low.contains("the result is ")
        }
        ConditionAst::PlayerActionThisTurn { action, .. } => {
            let low = action.trim().to_ascii_lowercase();
            low.contains("this way")
                || matches!(
                    low.as_str(),
                    "the player does"
                        | "a player does"
                        | "they do"
                        | "that player does"
                        | "the player doesn't"
                        | "no player does"
                        | "they don't"
                        | "that player doesn't"
                        | "they can't"
                        | "that player can't"
                )
        }
        ConditionAst::Marker(
            GameMarker::DidNotPutToZone { .. }
            | GameMarker::DamagePreventedThisWay
            | GameMarker::DealtDamageThisWay { .. }
            | GameMarker::ZoneActionCountThisWay { .. }
            | GameMarker::EnergyPaidThisWayAtLeast(_)
            | GameMarker::PlayerActionExtremeStat { .. },
        )
        | ConditionAst::PriorActionOccurred { .. }
        | ConditionAst::PlayerCoinFlipOutcome { .. }
        | ConditionAst::DieRollCompare { .. } => true,
        _ => false,
    }
}

pub(super) fn next_unit_has_anaphora_before_new_target(
    expanded: &[InstructionAst],
    start: usize,
    skip_empty_bind_x: bool,
) -> bool {
    let mut j = start;
    while j < expanded.len() {
        if skip_empty_bind_x
            && matches!(&expanded[j], InstructionAst::BindX { body, .. } if body.is_empty())
        {
            j += 1;
            continue;
        }
        let step = logical_unit_len(expanded, j);
        let unit = &expanded[j..j + step];
        if unit.iter().any(instruction_has_explicit_target) {
            return false;
        }
        if unit.iter().any(instruction_references_target_anaphora) {
            return true;
        }
        j += step;
    }
    false
}

pub(super) fn expand_top_level_compound_targets(ins: &[InstructionAst]) -> Vec<InstructionAst> {
    let mut out = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::TapPermanent {
                subject,
                any_number,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::TapPermanent {
                            subject: Box::new(p.clone()),
                            any_number: *any_number,
                        });
                    }
                    continue;
                }
            }
            InstructionAst::UntapPermanent {
                subject,
                count,
                up_to,
            } => {
                if count.is_none()
                    && !*up_to
                    && let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::UntapPermanent {
                            subject: Box::new(p.clone()),
                            count: None,
                            up_to: false,
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Destroy {
                subject,
                random,
                except,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::Destroy {
                            subject: Box::new(p.clone()),
                            random: *random,
                            except: except.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Exile {
                actor,
                subject,
                random,
                face_down,
                into_pile,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::Exile {
                            actor: *actor,
                            subject: Box::new(p.clone()),
                            random: *random,
                            face_down: *face_down,
                            into_pile: *into_pile,
                        });
                    }
                    continue;
                }
            }
            InstructionAst::ReturnZone {
                subject,
                from_zone,
                to,
                tapped,
                transformed,
                with_counter,
                attached_to,
                random,
                except,
                count,
                up_to,
                any_number,
                actor,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::ReturnZone {
                            subject: Box::new(p.clone()),
                            from_zone: from_zone.clone(),
                            to: to.clone(),
                            tapped: *tapped,
                            transformed: *transformed,
                            with_counter: with_counter.clone(),
                            attached_to: attached_to.clone(),
                            random: *random,
                            except: except.clone(),
                            count: count.clone(),
                            up_to: *up_to,
                            any_number: *any_number,
                            actor: *actor,
                        });
                    }
                    continue;
                }
            }
            InstructionAst::MoveZone { subject, to } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::MoveZone {
                            subject: Box::new(p.clone()),
                            to: to.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::ModifyPT {
                subject,
                dx,
                dy,
                duration,
                scale,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::ModifyPT {
                            subject: Box::new(p.clone()),
                            dx: dx.clone(),
                            dy: dy.clone(),
                            duration: duration.clone(),
                            scale: scale.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::GrantKeywords {
                subject,
                keywords,
                duration,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::GrantKeywords {
                            subject: Box::new(p.clone()),
                            keywords: keywords.clone(),
                            duration: duration.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::PutCounters {
                subject,
                counter_kind,
                count,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::PutCounters {
                            subject: Box::new(p.clone()),
                            counter_kind: counter_kind.clone(),
                            count: count.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::DealDamage {
                source,
                target,
                amount,
            } => {
                if let SubjectRefAst::Compound(parts) = target.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::DealDamage {
                            source: source.clone(),
                            target: Box::new(p.clone()),
                            amount: amount.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::TapOrUntapPermanent { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::TapOrUntapPermanent {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::SkipNextUntap { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::SkipNextUntap {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Regenerate { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::Regenerate {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::PhaseOut { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::PhaseOut {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::RemoveCounters {
                subject,
                counter_kind,
                all,
                count,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::RemoveCounters {
                            subject: Box::new(p.clone()),
                            counter_kind: counter_kind.clone(),
                            all: *all,
                            count: count.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::GainControl {
                controller,
                subject,
                duration,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::GainControl {
                            controller: *controller,
                            subject: Box::new(p.clone()),
                            duration: duration.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Optional(inner) => {
                let exp = expand_top_level_compound_targets(inner);
                if exp.len() > inner.len() && inner.len() == 1 {
                    for sub in exp {
                        out.push(InstructionAst::Optional(vec![sub]));
                    }
                    continue;
                }
            }
            InstructionAst::OptionalPlayer { player, body } => {
                let exp = expand_top_level_compound_targets(body);
                if exp.len() > body.len() && body.len() == 1 {
                    for sub in exp {
                        out.push(InstructionAst::OptionalPlayer {
                            player: *player,
                            body: vec![sub],
                        });
                    }
                    continue;
                }
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => {
                let exp = expand_top_level_compound_targets(then_body);
                let multi_target = exp.len() > then_body.len()
                    || (exp.len() >= 2
                        && exp
                            .iter()
                            .filter(|op| instruction_has_explicit_target(op))
                            .count()
                            >= 2);
                if multi_target && !condition_checks_last_action(condition) {
                    for sub in exp {
                        out.push(InstructionAst::IfCondition {
                            condition: condition.clone(),
                            then_body: vec![sub],
                        });
                    }
                    continue;
                }
            }
            InstructionAst::BindX { x_expr, body } if !body.is_empty() => {
                let exp = expand_top_level_compound_targets(body);
                let multi_target = exp.len() > body.len()
                    || (exp.len() >= 2
                        && exp
                            .iter()
                            .filter(|op| instruction_has_explicit_target(op))
                            .count()
                            >= 2);
                if multi_target {
                    for sub in exp {
                        out.push(InstructionAst::BindX {
                            x_expr: x_expr.clone(),
                            body: vec![sub],
                        });
                    }
                    continue;
                }
            }
            _ => {}
        }
        out.push(item.clone());
    }
    out
}

pub(super) fn is_still_a_land_restriction(ins: &InstructionAst) -> bool {
    matches!(
        ins,
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if is_still_type_restriction_str(restriction)
    )
}

pub(super) enum CounterSpellDestOverride {
    Exile,
    LibraryTop,
    LibraryBottom,
    ChoiceLibraryTopOrBottom,
}

pub(super) fn is_spell_countered_this_way_condition(cond: &ConditionAst) -> bool {
    if let ConditionAst::PriorActionOccurred {
        action,
        subject_filter: None,
        min_count: 1,
    } = cond
        && matches!(
            action.trim().to_ascii_lowercase().as_str(),
            "countered_this_way" | "countered" | "spell_countered_this_way"
        )
    {
        return true;
    }
    let txt = match cond {
        ConditionAst::EventOccurredThisTurn { event } => event.as_str(),
        ConditionAst::PlayerActionThisTurn { action, .. } => action.as_str(),
        _ => return false,
    };
    matches!(
        txt.trim()
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "that spell is countered this way"
            | "the spell is countered this way"
            | "a spell is countered this way"
            | "it is countered this way"
    )
}

pub(super) fn extract_counter_destination_replacement(
    base: &InstructionAst,
    next: &InstructionAst,
) -> Option<CounterSpellDestOverride> {
    if !matches!(base, InstructionAst::Counter { .. }) {
        return None;
    }
    let InstructionAst::IfCondition {
        condition,
        then_body,
    } = next
    else {
        return None;
    };
    if !is_spell_countered_this_way_condition(condition) {
        return None;
    }
    let alt_body: &[InstructionAst] = match then_body.as_slice() {
        [InstructionAst::ReplaceEffectWith(rew)] if !rew.only => {
            if let Some(instead_of) = &rew.instead_of {
                let low = instead_of.trim().trim_end_matches('.').to_ascii_lowercase();
                if !low.contains("graveyard") {
                    return None;
                }
            }
            rew.body.as_slice()
        }
        [InstructionAst::ReplaceEffect(inner)] => inner.as_slice(),
        _ => return None,
    };
    let is_spell_anaphora = |subj: &SubjectRefAst| {
        matches!(
            subj,
            SubjectRefAst::Anaphora(a)
                if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "that card" | "that spell" | "the spell"
                )
        )
    };
    match alt_body {
        [
            InstructionAst::Exile {
                actor: None,
                subject,
                random: false,
                face_down: false,
                into_pile: false,
            },
        ] if is_spell_anaphora(subject) => Some(CounterSpellDestOverride::Exile),
        [InstructionAst::MoveZone { subject, to }] if is_spell_anaphora(subject) => {
            match to
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str()
            {
                "top of its owner's library"
                | "the top of its owner's library"
                | "top of that player's library"
                | "the top of that player's library"
                | "top_of_library" => Some(CounterSpellDestOverride::LibraryTop),
                "bottom of its owner's library"
                | "the bottom of its owner's library"
                | "bottom of that player's library"
                | "the bottom of that player's library"
                | "bottom_of_library" => Some(CounterSpellDestOverride::LibraryBottom),
                "your choice of the top or bottom of its owner's library"
                | "your choice of the top or bottom of that player's library"
                | "the top or bottom of its owner's library" => {
                    Some(CounterSpellDestOverride::ChoiceLibraryTopOrBottom)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn contains_replace_effect(ops: &[InstructionAst]) -> bool {
    ops.iter().any(|op| match op {
        InstructionAst::ReplaceEffect(_) => true,
        InstructionAst::ReplaceEffectWith(rew) => !rew.only && rew.instead_of.is_none(),
        InstructionAst::IfCondition { then_body, .. } => contains_replace_effect(then_body),
        InstructionAst::BindX { body, .. } => contains_replace_effect(body),
        _ => false,
    })
}

pub(super) fn is_optional_replace_effect(ops: &[InstructionAst]) -> bool {
    matches!(
        ops,
        [InstructionAst::Optional(inner) | InstructionAst::OptionalPlayer { body: inner, .. }]
            if contains_replace_effect(inner)
    )
}

pub(super) fn is_replace_effect_carrier(op: &InstructionAst) -> bool {
    match op {
        InstructionAst::ReplaceEffect(_) => true,
        InstructionAst::ReplaceEffectWith(rew) => !rew.only && rew.instead_of.is_none(),
        InstructionAst::Optional(inner) | InstructionAst::OptionalPlayer { body: inner, .. } => {
            contains_replace_effect(inner)
        }
        InstructionAst::BindX { body, .. } => contains_replace_effect(body),
        InstructionAst::IfCondition { then_body, .. }
        | InstructionAst::IfPreviousSucceeded(then_body)
        | InstructionAst::IfPreviousFailed(then_body) => {
            contains_replace_effect(then_body) || is_optional_replace_effect(then_body)
        }
        _ => false,
    }
}

pub(super) fn strip_replace_effect(ops: &[InstructionAst]) -> Vec<InstructionAst> {
    let mut out = Vec::with_capacity(ops.len() + 2);
    for op in ops {
        match op {
            InstructionAst::ReplaceEffect(alt_inner) => {
                out.extend(strip_replace_effect(alt_inner));
            }
            InstructionAst::ReplaceEffectWith(rew) if !rew.only && rew.instead_of.is_none() => {
                out.extend(strip_replace_effect(&rew.body));
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } if contains_replace_effect(then_body) => {
                out.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: strip_replace_effect(then_body),
                });
            }
            InstructionAst::BindX { x_expr, body } if contains_replace_effect(body) => {
                out.push(InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: strip_replace_effect(body),
                });
            }
            _ => out.push(op.clone()),
        }
    }
    out
}

pub(super) fn find_first_token_spec(ops: &[InstructionAst]) -> Option<&TokenSpecAst> {
    for op in ops {
        match op {
            InstructionAst::CreateToken { token, .. } => {
                if !matches!(token, TokenSpecAst::Predefined { name, .. } if name == "those_tokens")
                {
                    return Some(token);
                }
            }
            InstructionAst::IfCondition { then_body, .. }
            | InstructionAst::Optional(then_body)
            | InstructionAst::OptionalPlayer {
                body: then_body, ..
            }
            | InstructionAst::ReplaceEffect(then_body)
            | InstructionAst::BindX {
                body: then_body, ..
            } => {
                if let Some(tok) = find_first_token_spec(then_body) {
                    return Some(tok);
                }
            }
            InstructionAst::ReplaceEffectWith(rew) => {
                if let Some(tok) = find_first_token_spec(&rew.body) {
                    return Some(tok);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn rewrite_those_tokens_in_slice(
    ops: &[InstructionAst],
    base_tok: &TokenSpecAst,
) -> Vec<InstructionAst> {
    ops.iter()
        .map(|op| match op {
            InstructionAst::CreateToken {
                controller,
                token:
                    TokenSpecAst::Predefined {
                        name,
                        count,
                        tapped,
                    },
            } if name == "those_tokens" => {
                let mut replaced = base_tok.clone();
                match &mut replaced {
                    TokenSpecAst::Creature {
                        count: c,
                        tapped: t,
                        ..
                    }
                    | TokenSpecAst::Predefined {
                        count: c,
                        tapped: t,
                        ..
                    }
                    | TokenSpecAst::Copy {
                        count: c,
                        tapped: t,
                        ..
                    } => {
                        *c = count.clone();
                        *t |= *tapped;
                    }
                    TokenSpecAst::Role { .. } => {}
                }
                InstructionAst::CreateToken {
                    controller: *controller,
                    token: replaced,
                }
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: rewrite_those_tokens_in_slice(then_body, base_tok),
            },
            InstructionAst::Optional(inner) => {
                InstructionAst::Optional(rewrite_those_tokens_in_slice(inner, base_tok))
            }
            InstructionAst::OptionalPlayer { player, body } => InstructionAst::OptionalPlayer {
                player: *player,
                body: rewrite_those_tokens_in_slice(body, base_tok),
            },
            InstructionAst::ReplaceEffect(inner) => {
                InstructionAst::ReplaceEffect(rewrite_those_tokens_in_slice(inner, base_tok))
            }
            InstructionAst::ReplaceEffectWith(rew) => InstructionAst::ReplaceEffectWith(Box::new(
                spellwake_oracle_compiler::ast::ReplaceEffectWithAst {
                    body: rewrite_those_tokens_in_slice(&rew.body, base_tok),
                    instead_of: rew.instead_of.clone(),
                    only: rew.only,
                },
            )),
            InstructionAst::BindX { x_expr, body } => InstructionAst::BindX {
                x_expr: x_expr.clone(),
                body: rewrite_those_tokens_in_slice(body, base_tok),
            },
            other => other.clone(),
        })
        .collect()
}

pub(super) fn substitute_those_tokens(
    ops: &[InstructionAst],
    base_slice: &[InstructionAst],
) -> Vec<InstructionAst> {
    if let Some(base_tok) = find_first_token_spec(base_slice) {
        rewrite_those_tokens_in_slice(ops, base_tok)
    } else {
        ops.to_vec()
    }
}

pub(super) fn is_choose_new_targets_for_copy(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::MayChooseNewTargetsForCopy => true,
        InstructionAst::Optional(inner) => matches!(
            inner.as_slice(),
            [InstructionAst::ChangeTargets {
                target,
                new_target: None,
                must_target: None,
                single_target_only: false,
            }]
                if matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
        ),
        InstructionAst::ChangeTargets {
            target,
            new_target: None,
            must_target: None,
            single_target_only: false,
        } => matches!(
            target.as_ref(),
            SubjectRefAst::Anaphora(a)
                if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "the copy" | "the copies" | "that copy" | "it" | "them"
                )
        ),
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } => matches!(
            restriction.trim().to_ascii_lowercase().as_str(),
            "you may choose new targets for the copy"
                | "you may choose new targets for it"
                | "you may choose new targets for the copies"
                | "choose new targets for the copy"
                | "you may choose a new target for the copy"
                | "choose a new target for the copy"
                | "you may choose new targets for that copy"
                | "you may choose a new target for that copy"
        ),
        _ => false,
    }
}

pub(super) fn patch_last_copy_choose_new_targets(ops: &mut [Instruction]) -> bool {
    for op in ops.iter_mut().rev() {
        match op {
            Instruction::StackOp(spec) => {
                if let spellwake_engine::toolbox::StackOpKind::Copy {
                    may_choose_new_targets,
                    ..
                } = &mut spec.op
                {
                    *may_choose_new_targets = true;
                    return true;
                }
            }
            Instruction::ScheduleDelayedEventTrigger { body, .. }
            | Instruction::ScheduleDelayedStepTrigger { body, .. }
            | Instruction::EmitReflexiveTrigger { body }
            | Instruction::RepeatLoop { body, .. } => {
                if patch_last_copy_choose_new_targets(&mut body.instructions) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(super) fn is_cast_spell_this_way_condition(cond: &ConditionAst) -> bool {
    let text = match cond {
        ConditionAst::EventOccurredThisTurn { event } => event.as_str(),
        ConditionAst::PlayerActionThisTurn { action, .. } => action.as_str(),
        _ => return false,
    };
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "you cast a spell this way"
            | "you cast it this way"
            | "you cast a card this way"
            | "you cast that spell this way"
            | "you cast that card this way"
    )
}

pub(super) fn is_any_color_spend_restriction(ins: &InstructionAst) -> bool {
    let raw = match ins {
        InstructionAst::ManaSpendRestriction(r) => Some(r.as_str()),
        InstructionAst::Optional(inner) => match inner.as_slice() {
            [InstructionAst::ManaSpendRestriction(r)] => Some(r.as_str()),
            _ => None,
        },
        InstructionAst::IfCondition {
            condition,
            then_body,
        } if is_cast_spell_this_way_condition(condition) => match then_body.as_slice() {
            [InstructionAst::ManaSpendRestriction(r)] => Some(r.as_str()),
            [InstructionAst::Optional(inner)] => match inner.as_slice() {
                [InstructionAst::ManaSpendRestriction(r)] => Some(r.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    raw.is_some_and(|r| {
        let rl = r.trim().to_ascii_lowercase();
        rl.contains("any color") || rl.contains("any type")
    })
}

pub(super) fn lower_look_top_intermediate_grant(
    ins: &InstructionAst,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Option<Instruction> {
    let is_moved_anaphora = |subj: &SubjectRefAst| {
        matches!(
            subj,
            SubjectRefAst::Anaphora(a)
                if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "that creature"
                        | "that permanent"
                        | "that card"
                        | "them"
                        | "those creatures"
                        | "those permanents"
                )
        )
    };
    match ins {
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } if is_moved_anaphora(subject) => {
            let ab = lower_ability_change(keywords, vocabulary).ok()?;
            let dur = lower_duration(duration).ok()?;
            Some(super::super::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                    EffectSubject::LastMoved,
                )),
                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]),
                dur,
            ))
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: None,
        } if is_moved_anaphora(subject) => {
            let px: i64 = dx.trim().replace('\u{2212}', "-").parse().ok()?;
            let py: i64 = dy.trim().replace('\u{2212}', "-").parse().ok()?;
            let dur = lower_duration(duration).ok()?;
            Some(super::super::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                    EffectSubject::LastMoved,
                )),
                ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPTExpr {
                    power: Box::new(ValueExpr::literal(px)),
                    toughness: Box::new(ValueExpr::literal(py)),
                }]),
                dur,
            ))
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } if is_moved_anaphora(subject) => {
            let kind = parse_counter_kind(counter_kind)?;
            let amount = lower_value_expr(resolve_x(count, bound_x), vocabulary).ok()?;
            Some(super::super::toolbox_emit::put_counters_on_subject(
                EffectSubject::LastMoved,
                kind,
                amount,
            ))
        }
        _ => None,
    }
}

pub(super) fn resolve_stack_copy_target(
    subject: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<spellwake_engine::program::StackCopyTarget, &'static str> {
    use spellwake_engine::program::StackCopyTarget;
    match subject {
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            if filter.is_ability && !filter.any_spell {
                let no_extra_predicates = filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.flags.is_empty()
                    && filter.named.is_none()
                    && filter.counter_kind.is_none()
                    && filter.combat.is_none()
                    && filter.tapped.is_none()
                    && filter.token.is_none();
                if no_extra_predicates {
                    match filter.controller {
                        Some(ControllerRelation::You | ControllerRelation::YourTeam)
                            if matches!(filter.color_arity, None | Some(ColorArity::Colorless)) =>
                        {
                            builder.bind_target(BodyTarget::ControlledAbility {
                                colorless_source: matches!(
                                    filter.color_arity,
                                    Some(ColorArity::Colorless)
                                ),
                            })?;
                            return Ok(StackCopyTarget::TargetAbility);
                        }
                        None if filter.color_arity.is_none() => {
                            builder.bind_target(BodyTarget::SingleStackEntry)?;
                            return Ok(StackCopyTarget::TargetAbility);
                        }
                        _ => {}
                    }
                }
                let bt = lower_target_subject(subject, vocabulary)?;
                if !matches!(
                    bt,
                    BodyTarget::ControlledAbility { .. }
                        | BodyTarget::SingleStackEntry
                        | BodyTarget::SingleStackObject(_)
                        | BodyTarget::StackEntryTargetingControlledPermanent
                ) {
                    return Err("oracle:unsupported-copy");
                }
                builder.bind_target(bt)?;
                Ok(StackCopyTarget::TargetAbility)
            } else if filter.is_ability && filter.any_spell {
                let mut inner = filter.as_ref().clone();
                inner.is_ability = false;
                inner.any_spell = false;
                inner.zone = None;
                let sf = lower_spell_filter(&inner, vocabulary)?;
                builder.bind_target(BodyTarget::SingleStackObject(sf))?;
                Ok(StackCopyTarget::TargetSpellOrAbility)
            } else if filter.any_spell || filter.zone == Some(ZoneFilter::Stack) {
                let bt = lower_target_subject(subject, vocabulary)?;
                if !matches!(
                    bt,
                    BodyTarget::SingleSpell(_)
                        | BodyTarget::SingleStackObject(_)
                        | BodyTarget::SingleStackEntry
                ) {
                    return Err("oracle:unsupported-copy");
                }
                builder.bind_target(bt)?;
                Ok(StackCopyTarget::TargetSpell)
            } else {
                Err("oracle:unsupported-copy")
            }
        }
        SubjectRefAst::Anaphora(a) => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(StackCopyTarget::TargetSpell),
            BodyTarget::ControlledAbility { .. } => Ok(StackCopyTarget::TargetAbility),
            BodyTarget::SingleStackEntry
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Ok(StackCopyTarget::TargetSpellOrAbility)
            }
            BodyTarget::None if in_trigger => {
                let alow = a.trim().to_ascii_lowercase();
                if alow.contains("ability") && !alow.contains("spell") {
                    Ok(StackCopyTarget::TriggeringAbility)
                } else {
                    Ok(StackCopyTarget::TriggeringSpell)
                }
            }
            BodyTarget::None
                if !self_pronoun_valid
                    && matches!(
                        a.trim().to_ascii_lowercase().as_str(),
                        "it" | "this spell" | "~"
                    ) =>
            {
                Ok(StackCopyTarget::ResolvingSpell)
            }
            _ => Err("oracle:unsupported-copy"),
        },
        SubjectRefAst::TriggerSubject if in_trigger => Ok(StackCopyTarget::TriggeringSpell),
        SubjectRefAst::SelfObject => {
            if !in_trigger && !self_pronoun_valid {
                Ok(StackCopyTarget::ResolvingSpell)
            } else if in_trigger {
                Ok(StackCopyTarget::TriggeringSpell)
            } else {
                Err("oracle:unsupported-copy")
            }
        }
        _ => Err("oracle:unsupported-copy"),
    }
}

pub(super) struct LoweredReplacePair {
    base_ops: Vec<Instruction>,
    alt_ops: Vec<Instruction>,
    merged_target: BodyTarget,
    merged_gy: Option<(usize, usize)>,
    x_target_count: bool,
    up_to_x_target_count: bool,
    last_group_filter: Option<ObjectFilter>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_replace_pair(
    base_slice: &[InstructionAst],
    raw_alt: &[InstructionAst],
    builder: &BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<LoweredReplacePair, &'static str> {
    let stripped = strip_replace_effect(raw_alt);
    let mut alt_slice = substitute_those_tokens(&stripped, base_slice);
    if let [
        InstructionAst::ModifyPT {
            duration: base_dur, ..
        },
    ] = base_slice
        && *base_dur != DurationAst::Instant
        && let [
            InstructionAst::ModifyPT {
                subject: alt_subj,
                duration: alt_dur,
                ..
            },
        ] = alt_slice.as_mut_slice()
        && matches!(alt_subj.as_ref(), SubjectRefAst::Anaphora(_))
        && *alt_dur == DurationAst::Instant
    {
        *alt_dur = base_dur.clone();
    }

    let mut base_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
    base_builder.has_source = builder.has_source;
    base_builder.target = builder.target;
    base_builder.graveyard_target_bounds = builder.graveyard_target_bounds;
    base_builder.x_target_count = builder.x_target_count;
    base_builder.last_group_filter = builder.last_group_filter;
    lower_instruction_seq(
        base_slice,
        &mut base_builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    )?;

    let mut alt_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
    alt_builder.has_source = builder.has_source;
    alt_builder.target = builder.target;
    alt_builder.graveyard_target_bounds = builder.graveyard_target_bounds;
    alt_builder.x_target_count = builder.x_target_count;
    alt_builder.up_to_x_target_count = builder.up_to_x_target_count;
    alt_builder.last_group_filter = builder.last_group_filter;
    let alt_first = lower_instruction_seq(
        &alt_slice,
        &mut alt_builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    );
    let need_relower = match alt_first {
        Err(_) => {
            base_builder.target != BodyTarget::None || base_builder.last_group_filter.is_some()
        }
        Ok(()) => {
            alt_builder.target == BodyTarget::None
                && base_builder.target != BodyTarget::None
                && alt_slice.iter().any(instruction_references_target_anaphora)
        }
    };
    if need_relower {
        alt_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
        alt_builder.has_source = builder.has_source;
        alt_builder.target = base_builder.target;
        alt_builder.graveyard_target_bounds = base_builder.graveyard_target_bounds;
        alt_builder.x_target_count = base_builder.x_target_count;
        alt_builder.up_to_x_target_count = base_builder.up_to_x_target_count;
        alt_builder.last_group_filter =
            base_builder.last_group_filter.or(builder.last_group_filter);
        lower_instruction_seq(
            &alt_slice,
            &mut alt_builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
    } else {
        alt_first?;
    }

    // Valid when both paths share the same target, or when the replacement path is untargeted
    // while the base path already binds the target on cast (CR 601.2c). Reject replacements
    // that introduce a new target absent from the base effect.
    let targets_compatible =
        alt_builder.target == base_builder.target || alt_builder.target == BodyTarget::None;
    let gy_compatible = match (
        base_builder.graveyard_target_bounds,
        alt_builder.graveyard_target_bounds,
    ) {
        (Some(a), Some(b)) => a == b,
        (Some(_), None) | (None, None) => true,
        (None, Some(_)) => false,
    };
    let alt_introduces_fixed_target = alt_builder
        .instructions
        .iter()
        .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
    let base_has_fixed_target = base_builder
        .instructions
        .iter()
        .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
    if !targets_compatible
        || !gy_compatible
        || (alt_builder.x_target_count && !base_builder.x_target_count)
        || (alt_builder.up_to_x_target_count && !base_builder.up_to_x_target_count)
        || (alt_introduces_fixed_target && !base_has_fixed_target)
    {
        return Err("oracle:replace-effect-target-mismatch");
    }
    let merged_target = base_builder.target;
    let merged_gy = base_builder.graveyard_target_bounds;
    let last_group_filter = base_builder
        .last_group_filter
        .or(alt_builder.last_group_filter);

    Ok(LoweredReplacePair {
        base_ops: base_builder.instructions,
        alt_ops: alt_builder.instructions,
        merged_target,
        merged_gy,
        x_target_count: base_builder.x_target_count || alt_builder.x_target_count,
        up_to_x_target_count: base_builder.up_to_x_target_count || alt_builder.up_to_x_target_count,
        last_group_filter,
    })
}

pub(super) fn bind_search_target_local(
    builder: &mut BodyBuilder,
    search: &LibrarySearch,
) -> Result<(), &'static str> {
    if search.source == SearchSource::TargetOpponentLibrary {
        builder.bind_target(BodyTarget::SingleOpponent)
    } else if search.source == SearchSource::TargetPlayerLibrary
        || search.searcher == ResolvingPlayer::TargetPlayer
    {
        builder.bind_target(BodyTarget::SinglePlayer)
    } else {
        Ok(())
    }
}

pub(super) fn is_mana_retention_instruction(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::RetainManaUntilEndOfTurn => true,
        InstructionAst::ManaSpendRestriction(r) => {
            let low = r.trim().to_ascii_lowercase();
            low.contains("don't lose this mana")
                || low.contains("doesn't empty")
                || low.contains("does not empty")
        }
        _ => false,
    }
}

pub(super) fn wrap_retained_mana_yield(yield_kind: EffectManaYield) -> EffectManaYield {
    match yield_kind {
        already @ EffectManaYield::RetainedUntilCleanup(_) => already,
        other => EffectManaYield::RetainedUntilCleanup(Box::new(other)),
    }
}

pub(super) fn retain_mana_in_instruction(op: &mut Instruction) -> bool {
    match op {
        Instruction::MutatePlayer(spec) => {
            if let spellwake_engine::toolbox::PlayerMutation::AddMana(yield_kind) = &mut spec.op {
                let current =
                    std::mem::replace(yield_kind, EffectManaYield::Fixed(BTreeMap::new()));
                *yield_kind = wrap_retained_mana_yield(current);
                true
            } else {
                false
            }
        }
        Instruction::RepeatLoop { body, .. } => {
            let mut patched = false;
            for inner in &mut body.instructions {
                patched |= retain_mana_in_instruction(inner);
            }
            patched
        }
        _ => false,
    }
}

pub(super) fn parse_look_top_pick_filter_ext(
    pick_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let mut cleaned = pick_raw.trim().trim_end_matches('.').trim();
    for suffix in [
        " revealed this way",
        " looked at this way",
        " that way",
        " from among those cards",
        " from among those",
        " from among them",
        " among those cards",
        " among them",
    ] {
        if let Some(rest) = cleaned.strip_suffix(suffix) {
            cleaned = rest.trim();
        }
    }
    let stripped = cleaned
        .strip_prefix("up to one ")
        .or_else(|| cleaned.strip_prefix("up to 1 "))
        .or_else(|| cleaned.strip_prefix("one "))
        .or_else(|| cleaned.strip_prefix("1 "))
        .or_else(|| cleaned.strip_prefix("any "))
        .or_else(|| cleaned.strip_prefix("an "))
        .or_else(|| cleaned.strip_prefix("a "))
        .unwrap_or(cleaned)
        .trim();
    if let Some(f) = parse_look_top_pick_filter(stripped, vocabulary) {
        return Some(f);
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let parsed =
        spellwake_oracle_compiler::filter::parse_object_filter(stripped, &compiler_vocab).ok()?;
    if parsed.zone.is_some() || parsed.controller.is_some() || parsed.named.is_some() {
        return None;
    }
    lower_object_filter(&parsed, vocabulary).ok()
}

pub(super) fn base_pair_len(ins: &[InstructionAst], i: usize) -> usize {
    if i + 1 >= ins.len() || is_replace_effect_carrier(&ins[i + 1]) {
        return 1;
    }
    // Pair consecutive same-source same-amount DealDamage clauses (e.g. Breath of Darigaaz)
    // when immediately followed by a ReplaceEffect carrier.
    if let InstructionAst::DealDamage {
        source: s0,
        amount: a0,
        ..
    } = &ins[i]
    {
        let mut k = 1usize;
        while i + k < ins.len()
            && matches!(
                &ins[i + k],
                InstructionAst::DealDamage { source, amount, .. }
                    if source == s0 && amount == a0
            )
        {
            k += 1;
        }
        if k >= 2 && i + k < ins.len() && is_replace_effect_carrier(&ins[i + k]) {
            return k;
        }
    }
    let is_two_op = match (&ins[i], &ins[i + 1]) {
        (
            InstructionAst::ModifyPT {
                subject: s1,
                duration: d1,
                ..
            },
            InstructionAst::GrantKeywords {
                subject: s2,
                duration: d2,
                ..
            },
        ) => s1 == s2 && d1 == d2,
        (InstructionAst::AnimateOrSetBasePT { .. }, InstructionAst::KeepExistingTypes(_)) => true,
        (InstructionAst::AnimateOrSetBasePT { .. }, next) => is_still_a_land_restriction(next),
        (
            InstructionAst::Copy {
                exception: None, ..
            },
            next,
        ) => is_choose_new_targets_for_copy(next),
        (InstructionAst::AddMana { .. }, next) if is_mana_retention_instruction(next) => true,
        (InstructionAst::AddMana { .. }, InstructionAst::ManaSpendRestriction(_))
        | (InstructionAst::LookTopLibrary { .. }, InstructionAst::ChooseAndKeep { .. })
        | (
            InstructionAst::ChangeLife { gain: false, .. },
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                gain: true,
                ..
            },
        )
        | (
            InstructionAst::SearchLibrary { shuffle: false, .. },
            InstructionAst::ShuffleLibrary { .. },
        )
        | (InstructionAst::Exile { .. }, InstructionAst::DelayedTrigger { .. })
        | (InstructionAst::ExileTopLibrary { .. }, InstructionAst::GrantCastPermission { .. }) => {
            true
        }
        _ => false,
    };
    if is_two_op { 2 } else { 1 }
}

pub(super) fn logical_unit_len(ins: &[InstructionAst], i: usize) -> usize {
    if i + 1 < ins.len() && extract_counter_destination_replacement(&ins[i], &ins[i + 1]).is_some()
    {
        return 2;
    }
    let base = base_pair_len(ins, i);
    if i + base < ins.len() && is_replace_effect_carrier(&ins[i + base]) {
        if matches!(&ins[i + base], InstructionAst::IfCondition { .. })
            && i + base + 1 < ins.len()
            && matches!(&ins[i + base + 1], InstructionAst::Otherwise(_))
        {
            return base + 2;
        }
        return base + 1;
    }
    if i + 1 < ins.len() {
        match (&ins[i], &ins[i + 1]) {
            (InstructionAst::IfCondition { .. }, InstructionAst::Otherwise(_)) => return 2,
            (InstructionAst::FlipCoin, InstructionAst::IfCondition { condition, .. })
                if matches!(condition.as_ref(), ConditionAst::WonContest) =>
            {
                if i + 2 < ins.len() && matches!(&ins[i + 2], InstructionAst::Otherwise(_)) {
                    return 3;
                }
                return 2;
            }
            (
                InstructionAst::Optional(_) | InstructionAst::OptionalPlayer { .. },
                InstructionAst::IfPreviousSucceeded(_)
                | InstructionAst::IfPreviousFailed(_)
                | InstructionAst::ReflexiveTrigger(_),
            ) => {
                if i + 2 < ins.len()
                    && matches!(
                        &ins[i + 2],
                        InstructionAst::Otherwise(_) | InstructionAst::IfPreviousFailed(_)
                    )
                {
                    return 3;
                }
                return 2;
            }
            (
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent,
                    ..
                },
                InstructionAst::Otherwise(_),
            ) => return 2,
            (
                InstructionAst::OptionalPlayer {
                    player: PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent,
                    ..
                },
                InstructionAst::IfCondition { condition, .. },
            ) if is_offer_any_player_paid_cond(condition)
                || is_offer_any_player_declined_cond(condition) =>
            {
                if i + 2 < ins.len()
                    && (matches!(
                        &ins[i + 2],
                        InstructionAst::Otherwise(_) | InstructionAst::IfPreviousFailed(_)
                    ) || matches!(
                        &ins[i + 2],
                        InstructionAst::IfCondition { condition: c2, .. }
                            if is_offer_any_player_declined_cond(c2)
                    ))
                {
                    return 3;
                }
                return 2;
            }
            (
                InstructionAst::CreateToken { .. },
                InstructionAst::Attach { .. } | InstructionAst::DelayedTrigger { .. },
            ) => return 2,
            (InstructionAst::RevealHand { .. }, InstructionAst::Discard { .. }) => return 2,
            _ => {}
        }
    }
    base
}

pub(super) fn is_permanent_body_target(bt: BodyTarget) -> bool {
    matches!(
        bt,
        BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
    )
}

pub(super) fn rewrite_body_target_related_to_first_target_controller(bt: &mut BodyTarget) {
    if let BodyTarget::FilteredPermanent(obj) = bt {
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

pub(super) fn subject_has_that_player_controller_target(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::TargetObject { filter, .. } => {
            filter.controller == Some(ControllerRelation::ThatPlayer)
        }
        SubjectRefAst::Compound(items) => {
            items.iter().any(subject_has_that_player_controller_target)
        }
        SubjectRefAst::InZone { subject, .. } => subject_has_that_player_controller_target(subject),
        _ => false,
    }
}

pub(super) fn instruction_has_that_player_controller_target(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { source, target, .. } => {
            subject_has_that_player_controller_target(source)
                || subject_has_that_player_controller_target(target)
        }
        InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Transform { subject }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::DoubleStatOrCounters { subject, .. }
        | InstructionAst::BecomeChosenCharacteristic { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::Copy { subject, .. } => {
            subject_has_that_player_controller_target(subject)
        }
        InstructionAst::Attach {
            subject, target, ..
        } => {
            subject_has_that_player_controller_target(subject)
                || subject_has_that_player_controller_target(target)
        }
        InstructionAst::MoveCounters { from, to, .. } => {
            subject_has_that_player_controller_target(from)
                || subject_has_that_player_controller_target(to)
        }
        InstructionAst::Fight { a, b } => {
            subject_has_that_player_controller_target(a)
                || subject_has_that_player_controller_target(b)
        }
        InstructionAst::Exchange { left, right, .. } => {
            subject_has_that_player_controller_target(left)
                || right
                    .as_deref()
                    .is_some_and(subject_has_that_player_controller_target)
        }
        InstructionAst::Optional(inner)
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::ScopedDuration { body: inner, .. } => inner
            .iter()
            .any(instruction_has_that_player_controller_target),
        _ => false,
    }
}

pub(super) fn lower_multi_target_fixed_clauses(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    trigger_hint: TriggerHint,
) -> Result<ExecutableBody, &'static str> {
    let expanded = expand_top_level_compound_targets(ins);
    let mut standalone_x: Option<&ValueExprAst> = None;
    let mut empty_bind_count = 0usize;
    for item in &expanded {
        if let InstructionAst::BindX { x_expr, body } = item
            && body.is_empty()
        {
            standalone_x = Some(x_expr.as_ref());
            empty_bind_count += 1;
        }
    }
    let bound_x = if empty_bind_count == 1 {
        standalone_x
    } else {
        None
    };

    let mut outer_ops: Vec<Instruction> = Vec::new();
    let mut active_clause: Option<BodyBuilder> = None;
    let mut clause_count = 0usize;
    let mut first_clause_is_permanent: Option<bool> = None;

    let flush_clause = |active: &mut Option<BodyBuilder>,
                        outer: &mut Vec<Instruction>,
                        count: &mut usize,
                        first_is_perm: &mut Option<bool>|
     -> Result<(), &'static str> {
        if let Some(mut clause) = active.take() {
            if clause.graveyard_target_bounds == Some((1, 1)) {
                clause.graveyard_target_bounds = None;
            }
            // Rewrite single-target (1, 1) AffectPermanentTargets / DealDamageToAnnouncedTargets
            // into equivalent single-target instructions so FixedTargetClause validation does not
            // reject exact-1 target clauses on permanent_target_bounds().
            let has_internal_jumps = clause.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::BranchIfFalse { .. }
                        | Instruction::BranchCondition { .. }
                        | Instruction::Jump { .. }
                        | Instruction::OptionalSequence { .. }
                        | Instruction::OfferDynamicManaPayment { .. }
                ) || matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::FlipCoins {
                                lose_destination: Some(_),
                                ..
                            } | spellwake_engine::toolbox::PlayerMutation::OfferSourceCounters { .. }
                        )
                )
            });
            let single_instruction = clause.instructions.len() == 1;
            let mut normalized_ops = Vec::with_capacity(clause.instructions.len());
            for op in clause.instructions {
                match op {
                    Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action,
                    } => match action {
                        spellwake_engine::program::PermanentTargetAction::Tap => {
                            normalized_ops.push(super::super::toolbox_emit::set_subject_tapped(
                                EffectSubject::TargetObject,
                                true,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::Untap => {
                            normalized_ops.push(super::super::toolbox_emit::set_subject_tapped(
                                EffectSubject::TargetObject,
                                false,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::SkipNextUntap => {
                            normalized_ops.push(super::super::toolbox_emit::skip_next_untap(
                                EffectSubject::TargetObject,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::TapAndSkipNextUntap
                            if !has_internal_jumps =>
                        {
                            normalized_ops.push(super::super::toolbox_emit::set_subject_tapped(
                                EffectSubject::TargetObject,
                                true,
                            ));
                            normalized_ops.push(super::super::toolbox_emit::skip_next_untap(
                                EffectSubject::TargetObject,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::Destroy => {
                            normalized_ops.push(super::super::toolbox_emit::zone_op(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                                        semantics:
                                            spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                                allow_regenerate: true,
                                            },
                                    },
                                ),
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::Exile => {
                            normalized_ops.push(super::super::toolbox_emit::exile_target());
                        }
                        spellwake_engine::program::PermanentTargetAction::ReturnToHand => {
                            normalized_ops
                                .push(super::super::toolbox_emit::return_to_owners_hand());
                        }
                        spellwake_engine::program::PermanentTargetAction::MoveTo(destination) => {
                            normalized_ops.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(MoveSubjectRef::TargetObject, destination));
                        }
                        spellwake_engine::program::PermanentTargetAction::PutPlusOneCounter => {
                            normalized_ops.push(
                                super::super::toolbox_emit::put_counters_on_subject(
                                    EffectSubject::TargetObject,
                                    spellwake_engine::effects::CounterKind::PlusOne,
                                    ValueExpr::literal(1),
                                ),
                            );
                        }
                        spellwake_engine::program::PermanentTargetAction::PutCounters {
                            kind,
                            amount,
                        } => {
                            normalized_ops.push(
                                super::super::toolbox_emit::put_counters_on_subject(
                                    EffectSubject::TargetObject,
                                    kind,
                                    amount,
                                ),
                            );
                        }
                        spellwake_engine::program::PermanentTargetAction::RemoveCounters {
                            kind,
                            amount,
                        } => {
                            normalized_ops.push(match amount {
                                Some(a) => {
                                    super::super::toolbox_emit::remove_counters_from_subject(
                                        EffectSubject::TargetObject,
                                        kind,
                                        a,
                                    )
                                }
                                None => super::super::toolbox_emit::mutate_subject(
                                    EffectSubject::TargetObject,
                                    spellwake_engine::toolbox::ObjectMutation::ClearCounters {
                                        kind,
                                    },
                                ),
                            });
                        }
                        spellwake_engine::program::PermanentTargetAction::Modify {
                            parts,
                            duration,
                        } => {
                            normalized_ops.push(
                                super::super::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(
                                        Box::new(EffectSubject::TargetObject),
                                    ),
                                    parts,
                                    duration,
                                ),
                            );
                        }
                        spellwake_engine::program::PermanentTargetAction::RequireCombatAction(
                            kind,
                        ) => {
                            normalized_ops.push(super::super::toolbox_emit::require_combat_action(
                                EffectSubject::TargetObject,
                                kind,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::Regenerate => {
                            normalized_ops.push(super::super::toolbox_emit::regenerate_subject(
                                EffectSubject::TargetObject,
                            ));
                        }
                        spellwake_engine::program::PermanentTargetAction::Damage(amount)
                            if single_instruction =>
                        {
                            normalized_ops.push(super::super::toolbox_emit::deal_damage(
                                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                amount,
                                false,
                            ));
                        }
                        other => {
                            normalized_ops.push(Instruction::AffectPermanentTargets {
                                minimum: 1,
                                maximum: 1,
                                action: other,
                            });
                        }
                    },
                    Instruction::DealDamage(mut spec)
                        if single_instruction
                            && matches!(
                                spec.recipients,
                                spellwake_engine::toolbox::DamageTargetQuery::AnnouncedTargets {
                                    min_targets: 1,
                                    max_targets: 1,
                                }
                            ) =>
                    {
                        spec.recipients = spellwake_engine::toolbox::DamageTargetQuery::Targets;
                        normalized_ops.push(Instruction::DealDamage(spec));
                    }
                    other => normalized_ops.push(other),
                }
            }
            clause.instructions = normalized_ops;
            if clause.target == BodyTarget::None
                || clause.instructions.is_empty()
                || clause.graveyard_target_bounds.is_some()
            {
                return Err("oracle:invalid-fixed-clause");
            }
            let body = clause.finish()?;
            if body.permanent_target_bounds().is_some()
                || !body.instructions.iter().any(Instruction::uses_mode_target)
            {
                return Err("oracle:invalid-fixed-clause");
            }
            if first_is_perm.is_none() {
                *first_is_perm = Some(is_permanent_body_target(body.target));
            }
            outer.push(Instruction::FixedTargetClause(Box::new(body)));
            *count += 1;
        }
        Ok(())
    };

    let mut i = 0usize;
    while i < expanded.len() {
        if bound_x.is_some()
            && matches!(&expanded[i], InstructionAst::BindX { body, .. } if body.is_empty())
        {
            i += 1;
            continue;
        }
        let step = logical_unit_len(&expanded, i);
        let unit = &expanded[i..i + step];
        let unit_has_anaphora = unit.iter().any(|op| {
            instruction_references_target_anaphora(op)
                || matches!(
                    op,
                    InstructionAst::IfCondition { condition, .. }
                        if condition_references_anaphora(condition)
                )
        });
        let unit_has_explicit_target = unit.iter().any(instruction_has_explicit_target);
        let unit_is_prev_followup = !unit_has_explicit_target
            && matches!(
                unit.first(),
                Some(
                    InstructionAst::IfPreviousSucceeded(_)
                        | InstructionAst::IfPreviousFailed(_)
                        | InstructionAst::Otherwise(_)
                        | InstructionAst::ReflexiveTrigger(_)
                )
            );
        let keep_untargeted_in_active = active_clause.is_some()
            && !unit_has_explicit_target
            && next_unit_has_anaphora_before_new_target(&expanded, i + step, bound_x.is_some());

        if let Some(cur) = active_clause.as_mut()
            && (unit_has_anaphora
                || unit_is_prev_followup
                || keep_untargeted_in_active
                || (cur.instructions.is_empty() && !unit_has_explicit_target))
        {
            let mut try_cur = cur.clone();
            if lower_instruction_seq(
                unit,
                &mut try_cur,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && try_cur.target == cur.target
            {
                *cur = try_cur;
                i += step;
                continue;
            }
        }

        let mut probe = BodyBuilder::with_trigger_hint(trigger_hint);
        probe.has_source = in_trigger || self_pronoun_valid;
        lower_instruction_seq(
            unit,
            &mut probe,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
        if probe.target != BodyTarget::None {
            flush_clause(
                &mut active_clause,
                &mut outer_ops,
                &mut clause_count,
                &mut first_clause_is_permanent,
            )?;
            if !in_trigger
                && first_clause_is_permanent == Some(true)
                && unit
                    .iter()
                    .any(instruction_has_that_player_controller_target)
            {
                rewrite_body_target_related_to_first_target_controller(&mut probe.target);
            }
            active_clause = Some(probe);
        } else {
            flush_clause(
                &mut active_clause,
                &mut outer_ops,
                &mut clause_count,
                &mut first_clause_is_permanent,
            )?;
            let shift = isize::try_from(outer_ops.len()).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in probe.instructions {
                if let Instruction::FixedTargetClause(ref inner_body) = op {
                    if first_clause_is_permanent.is_none() {
                        first_clause_is_permanent =
                            Some(is_permanent_body_target(inner_body.target));
                    }
                    clause_count += 1;
                }
                op.shift_jumps(shift, 0);
                outer_ops.push(op);
            }
        }
        i += step;
    }

    flush_clause(
        &mut active_clause,
        &mut outer_ops,
        &mut clause_count,
        &mut first_clause_is_permanent,
    )?;
    if clause_count < 2 || outer_ops.is_empty() {
        return Err("oracle:multiple-targets");
    }
    Ok(ExecutableBody::new(BodyTarget::None, outer_ops))
}

pub(super) fn rewrite_spell_self_has_status_to_anaphora(cond: &ConditionAst) -> ConditionAst {
    match cond {
        ConditionAst::Not(inner) => {
            ConditionAst::Not(Box::new(rewrite_spell_self_has_status_to_anaphora(inner)))
        }
        ConditionAst::And(a, b) => ConditionAst::And(
            Box::new(rewrite_spell_self_has_status_to_anaphora(a)),
            Box::new(rewrite_spell_self_has_status_to_anaphora(b)),
        ),
        ConditionAst::Or(a, b) => ConditionAst::Or(
            Box::new(rewrite_spell_self_has_status_to_anaphora(a)),
            Box::new(rewrite_spell_self_has_status_to_anaphora(b)),
        ),
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && !matches!(
                    status.trim().to_ascii_lowercase().as_str(),
                    "kicked" | "on_stack" | "on stack" | "on the stack"
                ) =>
        {
            ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: status.clone(),
            }
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: filter.clone(),
            }
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            ConditionAst::HasKeyword {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                keyword: keyword.clone(),
            }
        }
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => ConditionAst::StatCompare {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: *stat,
            op: *op,
            value: *value,
        },
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            ConditionAst::CounterCompare {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: counter_kind.clone(),
                op: *op,
                value: *value,
            }
        }
        ConditionAst::Marker(GameMarker::AttackedThisCombat { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            ConditionAst::Marker(GameMarker::AttackedThisCombat {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            })
        }
        other => other.clone(),
    }
}

pub(super) fn value_expr_has_chosen_x(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ChosenX => true,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b) => value_expr_has_chosen_x(a) || value_expr_has_chosen_x(b),
        ValueExprAst::DivConst { expr, .. } => value_expr_has_chosen_x(expr),
        ValueExprAst::Pow { base, exp } => {
            value_expr_has_chosen_x(base) || value_expr_has_chosen_x(exp)
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            value_expr_has_chosen_x(threshold) || value_expr_has_chosen_x(actual)
        }
        _ => false,
    }
}

pub(super) fn str_references_x(raw: &str) -> bool {
    let t = raw.trim().replace('\u{2212}', "-");
    matches!(
        t.to_ascii_lowercase().as_str(),
        "x" | "+x" | "-x" | "2x" | "+2x" | "-2x"
    )
}

pub(super) fn condition_has_chosen_x(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_has_chosen_x(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_has_chosen_x(a) || condition_has_chosen_x(b)
        }
        ConditionAst::Compare { lhs, rhs, .. } | ConditionAst::ExprCompare { lhs, rhs, .. } => {
            value_expr_has_chosen_x(lhs) || value_expr_has_chosen_x(rhs)
        }
        _ => false,
    }
}

pub(super) fn instruction_has_chosen_x(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { amount, .. }
        | InstructionAst::PreventDamage { amount, .. } => value_expr_has_chosen_x(amount),
        InstructionAst::ChangeLife { delta, .. } => value_expr_has_chosen_x(delta),
        InstructionAst::Draw { count, .. }
        | InstructionAst::Mill { count, .. }
        | InstructionAst::Scry { count, .. }
        | InstructionAst::Surveil { count, .. }
        | InstructionAst::PutCounters { count, .. }
        | InstructionAst::GainPlayerCounters { count, .. }
        | InstructionAst::SearchLibrary { count, .. }
        | InstructionAst::LookTopLibrary { count, .. }
        | InstructionAst::ExileTopLibrary { count, .. }
        | InstructionAst::Connive { count, .. }
        | InstructionAst::Incubate { count } => value_expr_has_chosen_x(count),
        InstructionAst::Discover { value } => value_expr_has_chosen_x(value),
        InstructionAst::Discard { count, .. }
        | InstructionAst::Sacrifice { count, .. }
        | InstructionAst::RemoveCounters { count, .. } => {
            count.as_deref().is_some_and(value_expr_has_chosen_x)
        }
        InstructionAst::ModifyPT { dx, dy, scale, .. } => {
            str_references_x(dx)
                || str_references_x(dy)
                || scale.as_deref().is_some_and(value_expr_has_chosen_x)
        }
        InstructionAst::AnimateOrSetBasePT {
            power, toughness, ..
        } => str_references_x(power) || str_references_x(toughness),
        InstructionAst::CreateToken { token, .. } => match token {
            TokenSpecAst::Creature {
                count,
                power,
                toughness,
                ..
            } => {
                value_expr_has_chosen_x(count)
                    || str_references_x(power)
                    || str_references_x(toughness)
            }
            TokenSpecAst::Predefined { count, .. } | TokenSpecAst::Copy { count, .. } => {
                value_expr_has_chosen_x(count)
            }
            TokenSpecAst::Role { .. } => false,
        },
        InstructionAst::AddMana {
            mana: ManaSpecAst::AnyColor { amount, .. } | ManaSpecAst::CombinationOf { amount, .. },
            ..
        } => value_expr_has_chosen_x(amount),
        InstructionAst::ForEachCount { count, body } => {
            value_expr_has_chosen_x(count) || body.iter().any(instruction_has_chosen_x)
        }
        InstructionAst::IfCondition {
            condition,
            then_body: inner,
        } => condition_has_chosen_x(condition) || inner.iter().any(instruction_has_chosen_x),
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachObject { body: inner, .. } => {
            inner.iter().any(instruction_has_chosen_x)
        }
        InstructionAst::ReplaceEffectWith(rew) => rew.body.iter().any(instruction_has_chosen_x),
        InstructionAst::Unless {
            body,
            unless_action,
            ..
        } => {
            body.iter().any(instruction_has_chosen_x)
                || unless_action
                    .as_ref()
                    .is_some_and(|a| a.iter().any(instruction_has_chosen_x))
        }
        InstructionAst::BindX { x_expr, .. } => value_expr_has_chosen_x(x_expr),
        _ => false,
    }
}

pub(super) fn lower_bound_x_for_resolution(
    x_expr: &ValueExprAst,
    builder: &BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Result<ValueExpr, &'static str> {
    if let ValueExprAst::ObjectStat { subject, stat } = x_expr
        && let SubjectRefAst::Anaphora(raw) = subject.as_ref()
        && raw.to_ascii_lowercase().contains("army")
        && builder.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Amass { .. })
            )
        })
    {
        let st = match stat {
            StatKind::Power => ObjectStatKind::Power,
            StatKind::Toughness => ObjectStatKind::Toughness,
            StatKind::ManaValue => ObjectStatKind::ManaValue,
            StatKind::Loyalty => ObjectStatKind::Loyalty,
        };
        return Ok(ValueExpr::object_stat(ValueObjectRef::LastAmassedArmy, st));
    }
    let mut val = lower_value_expr_in_context(x_expr, builder, in_trigger, vocabulary)?;
    if !is_object_body_target(builder.target, false) {
        let replacement = builder.instructions.iter().rev().find_map(|op| match op {
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::Amass { .. }
                ) =>
            {
                Some(ValueObjectRef::LastAmassedArmy)
            }
            Instruction::Sacrifice { .. } => Some(ValueObjectRef::LastSacrificedCreature),
            Instruction::ZoneOp(_) | Instruction::ForEachPlayerQuery { .. }
                if super::super::toolbox_emit::unwrap_zone_op(op).is_some_and(|spec| {
                    matches!(
                        spec.destination,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                        }
                    )
                }) =>
            {
                Some(ValueObjectRef::LastSacrificedCreature)
            }
            Instruction::LookTopChooseExt {
                reveal_only: true, ..
            } => Some(ValueObjectRef::RevealedTop),
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                ) =>
            {
                Some(ValueObjectRef::RevealedTop)
            }
            Instruction::ZoneOp(spec)
                if matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        ..
                    }
                ) && matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                        | spellwake_engine::toolbox::ZoneDest::RevealOnly
                ) =>
            {
                Some(ValueObjectRef::RevealedTop)
            }
            _ => None,
        });
        if let Some(repl) = replacement {
            rewrite_value_expr_anaphora(&mut val, repl);
        }
    }
    Ok(val)
}

pub(super) fn is_searched_library_guard(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::SearchedLibraryThisWay => true,
        ConditionAst::PlayerActionThisTurn { action, .. }
        | ConditionAst::EventOccurredThisTurn { event: action } => matches!(
            action.trim().to_ascii_lowercase().as_str(),
            "you search your library this way"
                | "you searched your library this way"
                | "that player searches their library this way"
                | "that player searched their library this way"
                | "they search their library this way"
                | "they searched their library this way"
                | "a player searches their library this way"
                | "a player searched their library this way"
                | "a player searches a library this way"
                | "a player searched a library this way"
        ),
        _ => false,
    }
}

pub(super) fn is_search_found_anaphora(subj: &SubjectRefAst) -> bool {
    matches!(
        subj,
        SubjectRefAst::Anaphora(a)
            if {
                let low = a.trim().to_ascii_lowercase();
                !low.contains("rest")
                    && !low.contains("other")
                    && !low.contains("unrelated")
                    && (matches!(
                        low.as_str(),
                        "it" | "them"
                            | "that card"
                            | "those cards"
                            | "the card"
                            | "the cards"
                            | "one of them"
                            | "the chosen card"
                            | "the chosen cards"
                            | "the revealed card"
                            | "the revealed cards"
                    ) || low.starts_with("that ")
                        || low.starts_with("those ")
                        || low.contains("found this way")
                        || low.contains("searched"))
            }
    )
}

pub(super) fn is_search_placeholder_dest(dest: &str) -> bool {
    matches!(
        dest.trim().to_ascii_lowercase().as_str(),
        "" | "found_set" | "found" | "revealed" | "follow_up"
    )
}

pub(super) fn apply_search_continuation_step(
    step: &InstructionAst,
    library: &str,
    eff_reveal: &mut bool,
    eff_dest: &mut String,
    saw_shuffle: &mut bool,
) -> bool {
    match step {
        InstructionAst::RevealCards { subject } if is_search_found_anaphora(subject) => {
            *eff_reveal = true;
            true
        }
        InstructionAst::LookTopLibrary {
            reveal, library, ..
        } if library.eq_ignore_ascii_case("ref")
            || library.to_ascii_lowercase().starts_with("ref:") =>
        {
            *eff_reveal |= *reveal;
            true
        }
        InstructionAst::MoveZone { subject, to }
            if is_search_found_anaphora(subject) && is_search_placeholder_dest(eff_dest) =>
        {
            *eff_dest = to.clone();
            if to.to_ascii_lowercase().contains("after_shuffle") {
                *saw_shuffle = true;
            }
            true
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone: None,
            to,
            tapped,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            ..
        } if is_search_found_anaphora(subject) && is_search_placeholder_dest(eff_dest) => {
            let to_low = to.to_ascii_lowercase();
            *eff_dest = if *tapped && !to_low.contains("tapped") {
                format!("{to} tapped")
            } else {
                to.clone()
            };
            if to_low.contains("after_shuffle") {
                *saw_shuffle = true;
            }
            true
        }
        InstructionAst::Exile {
            actor:
                None
                | Some(
                    PlayerRefAst::You
                    | PlayerRefAst::ThatPlayer
                    | PlayerRefAst::TargetPlayer
                    | PlayerRefAst::TargetOpponent,
                ),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } if is_search_found_anaphora(subject) && is_search_placeholder_dest(eff_dest) => {
            *eff_dest = "exile".to_string();
            true
        }
        InstructionAst::ShuffleLibrary { player } => {
            let lib_low = library.trim().to_ascii_lowercase();
            let owner_ok = match lib_low.as_str() {
                "" | "your" | "own" | "you" => *player == PlayerRefAst::You,
                "each player" | "each_player" => {
                    matches!(player, PlayerRefAst::EachPlayer | PlayerRefAst::ThatPlayer)
                }
                "each opponent" | "each_opponent" => {
                    matches!(
                        player,
                        PlayerRefAst::EachOpponent | PlayerRefAst::ThatPlayer
                    )
                }
                _ => matches!(
                    player,
                    PlayerRefAst::ThatPlayer
                        | PlayerRefAst::TargetPlayer
                        | PlayerRefAst::TargetOpponent
                ),
            };
            if !owner_ok {
                return false;
            }
            let dest_low = eff_dest.to_ascii_lowercase();
            if dest_low.contains("top") && !dest_low.contains("after_shuffle") {
                return false;
            }
            *saw_shuffle = true;
            true
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_fuse_search_library(
    ins: &[InstructionAst],
    i: usize,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Option<usize> {
    let (opt_chooser, search_ast, inner_rest, effective_bound_x) = match &ins[i] {
        s @ InstructionAst::SearchLibrary { .. } => (None, s, &[][..], bound_x),
        InstructionAst::BindX { x_expr, body } => match body.as_slice() {
            [s @ InstructionAst::SearchLibrary { .. }] => (None, s, &[][..], Some(x_expr.as_ref())),
            _ => return None,
        },
        InstructionAst::Optional(inner) => match inner.split_first() {
            Some((s @ InstructionAst::SearchLibrary { .. }, rest)) => {
                (Some(PlayerRefAst::You), s, rest, bound_x)
            }
            _ => return None,
        },
        InstructionAst::OptionalPlayer { player, body } => match body.split_first() {
            Some((s @ InstructionAst::SearchLibrary { .. }, rest)) => {
                (Some(*player), s, rest, bound_x)
            }
            _ => return None,
        },
        _ => return None,
    };
    let bound_x = effective_bound_x;
    let InstructionAst::SearchLibrary {
        library,
        count,
        filter,
        reveal,
        to_zone,
        shuffle,
        up_to,
        any_number,
        extra_zones,
        secondary_filter,
        secondary_is_or,
        where_x,
        rest_to_zone,
        modifier,
        follow_up,
        exact: false,
    } = search_ast
    else {
        return None;
    };
    let mut eff_reveal = *reveal;
    let mut eff_dest = to_zone.clone();
    let mut eff_rest_to_zone = rest_to_zone.clone();
    let mut saw_shuffle = *shuffle;
    let mut post_entry: Option<spellwake_engine::program::SearchBattlefieldPostEntry> = None;
    let mut trailing_follow_up: Vec<InstructionAst> = Vec::new();
    if eff_dest.to_ascii_lowercase().contains("after_shuffle") {
        saw_shuffle = true;
    }
    if !follow_up.is_empty() {
        let mut raw_trailing: Vec<&InstructionAst> = Vec::new();
        for step in follow_up {
            if let InstructionAst::ChooseAndKeep {
                pick,
                keep_to,
                rest_to,
            } = step
                && is_search_placeholder_dest(&eff_dest)
                && matches!(
                    pick.trim().to_ascii_lowercase().as_str(),
                    "one" | "1" | "one of them" | "up to one" | "up to one of them"
                )
            {
                eff_dest = keep_to.clone();
                if eff_rest_to_zone.is_none() && !rest_to.trim().is_empty() {
                    eff_rest_to_zone = Some(rest_to.clone());
                }
                continue;
            }
            if apply_search_continuation_step(
                step,
                library,
                &mut eff_reveal,
                &mut eff_dest,
                &mut saw_shuffle,
            ) {
                continue;
            }
            raw_trailing.push(step);
        }
        let is_anaphora_haste = |st: &InstructionAst| -> bool {
            matches!(
                st,
                InstructionAst::GrantKeywords {
                    subject,
                    keywords,
                    ..
                } if subject_is_anaphora(subject)
                    && keywords.len() == 1
                    && keywords[0].trim().eq_ignore_ascii_case("haste")
            )
        };
        let is_end_step_return_hand = |st: &InstructionAst| -> bool {
            let InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                body,
                ..
            } = st
            else {
                return false;
            };
            let is_hand_dest = |d: &str| {
                matches!(
                    d.trim().to_ascii_lowercase().as_str(),
                    "your hand" | "its owner's hand" | "their owner's hand" | "hand"
                )
            };
            match body.as_slice() {
                [InstructionAst::MoveZone { subject, to }] if subject_is_anaphora(subject) => {
                    is_hand_dest(to)
                }
                [
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped: false,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        random: false,
                        except: None,
                        count: None,
                        up_to: false,
                        any_number: false,
                        ..
                    },
                ] if subject_is_anaphora(subject) => is_hand_dest(to),
                _ => false,
            }
        };
        let dest_is_bf = eff_dest.to_ascii_lowercase().contains("battlefield");
        if dest_is_bf
            && raw_trailing.len() == 2
            && ((is_anaphora_haste(raw_trailing[0]) && is_end_step_return_hand(raw_trailing[1]))
                || (is_end_step_return_hand(raw_trailing[0]) && is_anaphora_haste(raw_trailing[1])))
        {
            if !(in_trigger || self_pronoun_valid) {
                return None;
            }
            post_entry = Some(
                spellwake_engine::program::SearchBattlefieldPostEntry::HasteAndReturnAtEndStep,
            );
        } else {
            // Do not allow anaphora in follow_up to bind to an unrelated earlier object target.
            if is_object_body_target(builder.target, true)
                && raw_trailing
                    .iter()
                    .any(|st| instruction_references_anaphora(st))
            {
                return None;
            }
            trailing_follow_up = raw_trailing.into_iter().cloned().collect();
        }
    }
    // Strictly consume every instruction inside Optional / OptionalPlayer (fail-closed if any is unhandled).
    for step in inner_rest {
        if !apply_search_continuation_step(
            step,
            library,
            &mut eff_reveal,
            &mut eff_dest,
            &mut saw_shuffle,
        ) {
            return None;
        }
    }
    let mut cursor = i + 1;
    while cursor < ins.len() {
        if let InstructionAst::ChooseAndKeep {
            pick,
            keep_to,
            rest_to,
        } = &ins[cursor]
            && is_search_placeholder_dest(&eff_dest)
            && matches!(
                pick.trim().to_ascii_lowercase().as_str(),
                "one" | "1" | "one of them" | "up to one" | "up to one of them"
            )
        {
            eff_dest = keep_to.clone();
            if eff_rest_to_zone.is_none() && !rest_to.trim().is_empty() {
                eff_rest_to_zone = Some(rest_to.clone());
            }
            cursor += 1;
            continue;
        }
        if apply_search_continuation_step(
            &ins[cursor],
            library,
            &mut eff_reveal,
            &mut eff_dest,
            &mut saw_shuffle,
        ) {
            cursor += 1;
            continue;
        }
        if let InstructionAst::IfCondition {
            condition,
            then_body,
        } = &ins[cursor]
            && is_searched_library_guard(condition)
            && !then_body.is_empty()
        {
            let mut r = eff_reveal;
            let mut d = eff_dest.clone();
            let mut s = saw_shuffle;
            if then_body
                .iter()
                .all(|st| apply_search_continuation_step(st, library, &mut r, &mut d, &mut s))
            {
                eff_reveal = r;
                eff_dest = d;
                saw_shuffle = s;
                cursor += 1;
                continue;
            }
            // Fail-closed: do not emit a search leaving an unhandled SearchedLibraryThisWay clause.
            return None;
        }
        if let InstructionAst::IfPreviousSucceeded(then_body) = &ins[cursor]
            && !then_body.is_empty()
            && (!saw_shuffle || is_search_placeholder_dest(&eff_dest))
        {
            let mut r = eff_reveal;
            let mut d = eff_dest.clone();
            let mut s = saw_shuffle;
            if then_body
                .iter()
                .all(|st| apply_search_continuation_step(st, library, &mut r, &mut d, &mut s))
            {
                eff_reveal = r;
                eff_dest = d;
                saw_shuffle = s;
                cursor += 1;
                continue;
            }
        }
        break;
    }
    if matches!(&ins[i], InstructionAst::BindX { .. })
        && ins[cursor..].iter().any(instruction_has_chosen_x)
    {
        return None;
    }
    let is_optional_player = opt_chooser.is_some_and(|p| p != PlayerRefAst::You);
    let fused_something =
        cursor > i + 1 || !inner_rest.is_empty() || is_optional_player || !follow_up.is_empty();
    if !fused_something || !saw_shuffle || is_search_placeholder_dest(&eff_dest) {
        return None;
    }
    let dest_arg = if eff_dest.to_ascii_lowercase().contains("top") {
        "top of your library"
    } else {
        eff_dest.as_str()
    };
    let count_arg = if where_x.is_none() {
        resolve_x(count, bound_x)
    } else {
        count.as_ref()
    };
    let extra = SearchLibraryExtra {
        extra_zones: extra_zones.as_slice(),
        secondary_filter: secondary_filter.as_deref(),
        secondary_is_or: *secondary_is_or,
        where_x: where_x.as_deref(),
        rest_to_zone: eff_rest_to_zone.as_deref(),
        modifier: modifier.as_ref(),
    };
    let is_optional = opt_chooser.is_some();
    let mut search = match lower_library_search_ext(
        library,
        count_arg,
        filter,
        eff_reveal,
        dest_arg,
        vocabulary,
        is_optional,
        *up_to,
        *any_number,
        &extra,
    ) {
        Ok(s) => s,
        Err("oracle:dynamic-search-count")
            if (*up_to || is_optional)
                && where_x.is_none()
                && !*any_number
                && secondary_filter.is_none()
                && eff_rest_to_zone.is_none() =>
        {
            let dyn_expr =
                lower_value_expr_in_context(count_arg, builder, in_trigger, vocabulary).ok()?;
            let mut s = lower_library_search_ext(
                library,
                &ValueExprAst::Const(1),
                filter,
                eff_reveal,
                dest_arg,
                vocabulary,
                is_optional,
                true,
                false,
                &extra,
            )
            .ok()?;
            s.count = SearchCount::UpToExpr(Box::new(dyn_expr));
            s
        }
        Err(_) => return None,
    };
    if let Some(pe) = post_entry {
        if let SearchDestination::Battlefield {
            post_entry: ref mut slot,
            ..
        } = search.destination
        {
            *slot = pe;
        } else {
            return None;
        }
    }
    let mut probe = builder.clone();
    let opt_resolved = if let Some(opt_player) = opt_chooser {
        let chooser = if opt_player == PlayerRefAst::You {
            ResolvingPlayer::Controller
        } else {
            lower_resolving_player_in_context(opt_player, &mut probe, in_trigger).ok()?
        };
        if search.searcher == ResolvingPlayer::UniqueRelatedPlayer {
            search.searcher = if opt_player != PlayerRefAst::You {
                chooser
            } else {
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, &mut probe, in_trigger)
                    .ok()?
            };
        }
        if !search.supports_optional_wrapper() {
            return None;
        }
        Some(chooser)
    } else {
        if search.searcher == ResolvingPlayer::UniqueRelatedPlayer {
            search.searcher =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, &mut probe, in_trigger)
                    .ok()?;
        }
        None
    };
    bind_search_target_local(&mut probe, &search).ok()?;
    let search_idx = probe.instructions.len();
    probe.instructions.push(Instruction::SearchLibrary {
        filter: Box::new(search),
    });
    if !trailing_follow_up.is_empty() {
        lower_instruction_seq(
            &trailing_follow_up,
            &mut probe,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )
        .ok()?;
    }
    if let Some(chooser) = opt_resolved {
        let span = probe.instructions.len().saturating_sub(search_idx);
        if span == 1 {
            probe
                .instructions
                .insert(search_idx, Instruction::OptionalEffect { chooser });
        } else if span >= 2 {
            probe.instructions.insert(
                search_idx,
                Instruction::OptionalSequence {
                    chooser,
                    instructions: span,
                },
            );
        } else {
            return None;
        }
    }
    *builder = probe;
    Some(cursor)
}

pub(super) fn is_top_card_anaphora_label(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "it" | "that card" | "the card" | "the revealed card" | "the exiled card"
    )
}

pub(super) fn is_revealed_card_move_op(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::MoveZone { subject, .. }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::GrantCastPermission { subject, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_top_card_anaphora_label(a))
        }
        InstructionAst::Optional(inner) => {
            matches!(inner.as_slice(), [single] if is_revealed_card_move_op(single))
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_revealed_top_branch(
    branch: &[InstructionAst],
    outer_builder: &BodyBuilder,
    library_owner: ResolvingPlayer,
    cond_tests_revealed_top: bool,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<BodyBuilder, &'static str> {
    let mut sub = BodyBuilder::with_trigger_hint(outer_builder.trigger_hint);
    sub.has_source = outer_builder.has_source;
    sub.target = outer_builder.target;
    let prior_shuffled_target = outer_builder.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::ZoneOp(spec)
                if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                    && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Library { .. })
        )
    });
    sub.instructions.push(Instruction::LookTopChooseExt {
        library_owner,
        chooser: ResolvingPlayer::Controller,
        count: ValueExpr::literal(1),
        filter: None,
        max_picks: 0,
        take_all_matching: false,
        keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
        rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
        reveal_kept: false,
        reveal_all: true,
        reveal_only: true,
        secondary: None,
    });
    for step in branch {
        if is_revealed_card_move_op(step) {
            let saved_target = sub.target;
            let saved_has_source = sub.has_source;
            let inner_step = match step {
                InstructionAst::Optional(v) if v.len() == 1 => &v[0],
                other => other,
            };
            let force_card_ref = saved_target == BodyTarget::None
                || cond_tests_revealed_top
                || prior_shuffled_target
                || matches!(
                    inner_step,
                    InstructionAst::MoveZone { subject, .. }
                        | InstructionAst::ReturnZone { subject, .. }
                        | InstructionAst::Exile { subject, .. }
                        | InstructionAst::GrantCastPermission { subject, .. }
                        if matches!(
                            subject.as_ref(),
                            SubjectRefAst::Anaphora(a)
                                if !a.trim().eq_ignore_ascii_case("it")
                        )
                );
            if force_card_ref {
                let dest_explicitly_under_your_control = match inner_step {
                    InstructionAst::MoveZone { to, .. } | InstructionAst::ReturnZone { to, .. } => {
                        to.to_ascii_lowercase().contains("under your control")
                    }
                    _ => false,
                };
                sub.target = BodyTarget::None;
                let before_len = sub.instructions.len();
                let res = lower_instruction_seq(
                    std::slice::from_ref(step),
                    &mut sub,
                    vocabulary,
                    ids,
                    false,
                    false,
                    bound_x,
                );
                sub.target = saved_target;
                sub.has_source = saved_has_source;
                res?;
                if library_owner != ResolvingPlayer::Controller
                    && !dest_explicitly_under_your_control
                {
                    for op in &mut sub.instructions[before_len..] {
                        if let Instruction::ZoneOp(spec) = op
                            && matches!(
                                spec.source,
                                spellwake_engine::toolbox::ObjectQuery::Revealed
                            )
                            && let spellwake_engine::toolbox::ZoneDest::Battlefield {
                                controller,
                                ..
                            } = &mut spec.destination
                        {
                            *controller = spellwake_engine::toolbox::ZoneController::Owner;
                        }
                    }
                }
                continue;
            }
        }
        lower_instruction_seq(
            std::slice::from_ref(step),
            &mut sub,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
    }
    sub.instructions.remove(0);
    if sub.instructions.is_empty() {
        return Err("oracle:empty-body");
    }
    for op in &mut sub.instructions {
        op.shift_jumps(-1, 1);
    }
    Ok(sub)
}

pub(super) fn condition_references_anaphora(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_references_anaphora(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_references_anaphora(a) || condition_references_anaphora(b)
        }
        ConditionAst::MatchesFilter { subject, .. }
        | ConditionAst::HasStatus { subject, .. }
        | ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::StatCompare { subject, .. }
        | ConditionAst::CounterCompare { subject, .. }
        | ConditionAst::SubjectControlledBy { subject, .. }
        | ConditionAst::SubjectInZone { subject, .. }
        | ConditionAst::AllColors(subject)
        | ConditionAst::DamageDealtThisTurn {
            source: subject, ..
        } => matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)),
        ConditionAst::SameNameAs { subject, other }
        | ConditionAst::SharesPropertyWith { subject, other, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                || matches!(other.as_ref(), SubjectRefAst::Anaphora(_))
        }
        _ => false,
    }
}

pub(super) fn value_expr_references_anaphora_stat(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        }
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b) => {
            value_expr_references_anaphora_stat(a) || value_expr_references_anaphora_stat(b)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_anaphora_stat(expr),
        _ => false,
    }
}

pub(super) fn is_card_anaphora_grant_cast(ins: &InstructionAst) -> Option<&InstructionAst> {
    let inner = match ins {
        g @ InstructionAst::GrantCastPermission { .. } => g,
        InstructionAst::Optional(v) => match v.as_slice() {
            [single] => single,
            _ => return None,
        },
        _ => return None,
    };
    if let InstructionAst::GrantCastPermission {
        subject,
        count_limit: None,
        alt_cost: None,
        ..
    } = inner
        && matches!(
            subject.as_ref(),
            SubjectRefAst::Anaphora(a)
                if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "them"
                        | "that card"
                        | "those cards"
                        | "the card"
                        | "the exiled card"
                        | "the exiled cards"
                        | "the revealed card"
                        | "the revealed cards"
                )
        )
    {
        Some(inner)
    } else {
        None
    }
}

pub(super) fn resolve_their_library_owner(
    builder: &BodyBuilder,
    in_trigger: bool,
) -> Option<ResolvingPlayer> {
    match builder.target {
        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
            Some(ResolvingPlayer::TargetPlayer)
        }
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
            if builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ZoneOp(spec)
                        if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                            && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Library { .. })
                )
            }) =>
        {
            Some(ResolvingPlayer::TargetOwner)
        }
        BodyTarget::SinglePermanent
        | BodyTarget::SingleCreature
        | BodyTarget::SinglePlaneswalker
        | BodyTarget::FilteredPermanent(_)
        | BodyTarget::SinglePlayerOrPlaneswalker
        | BodyTarget::SingleOpponentOrPlaneswalker
        | BodyTarget::SingleCreatureOrPlayer
        | BodyTarget::SingleAnyTarget => Some(ResolvingPlayer::TargetController),
        BodyTarget::SingleOwnGraveyardCard | BodyTarget::FilteredOwnGraveyardCard(_) => {
            Some(ResolvingPlayer::Controller)
        }
        BodyTarget::FilteredAnyGraveyardCard { .. }
        | BodyTarget::AnyGraveyardCard
        | BodyTarget::AnyGraveyardCreature
        | BodyTarget::AnyGraveyardCreatureOrPlaneswalker => Some(ResolvingPlayer::TargetOwner),
        BodyTarget::SingleSpell(_)
        | BodyTarget::SingleStackObject(_)
        | BodyTarget::SingleStackEntry
        | BodyTarget::ControlledAbility { .. }
        | BodyTarget::StackEntryTargetingControlledPermanent => {
            Some(ResolvingPlayer::TargetSpellController)
        }
        _ if builder.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ChooseValue {
                    domain: spellwake_engine::program::ResolutionChoiceDomain::Player { .. },
                    ..
                }
            ) || matches!(
                op,
                Instruction::MutatePlayer(spec)
                    if matches!(
                        spec.op,
                        spellwake_engine::toolbox::PlayerMutation::SelectExtremeLife { .. }
                    )
            )
        }) =>
        {
            Some(ResolvingPlayer::ChosenPlayer)
        }
        _ if in_trigger => Some(ResolvingPlayer::UniqueRelatedPlayer),
        _ => None,
    }
}

pub(super) fn parse_look_top_library_owner(
    library: &str,
    builder: &BodyBuilder,
    in_trigger: bool,
) -> Option<(Option<BodyTarget>, ResolvingPlayer)> {
    let lib_low = library.trim().to_ascii_lowercase();
    match lib_low.as_str() {
        "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" => {
            Some((None, ResolvingPlayer::Controller))
        }
        "target player" | "target player's" | "targetplayer" => Some((
            Some(BodyTarget::SinglePlayer),
            ResolvingPlayer::TargetPlayer,
        )),
        "target opponent" | "target opponent's" | "targetopponent" => Some((
            Some(BodyTarget::SingleOpponent),
            ResolvingPlayer::TargetPlayer,
        )),
        "target"
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) =>
        {
            Some((None, ResolvingPlayer::TargetPlayer))
        }
        "their" | "their library" | "that player" | "that player's" | "thatplayer" => {
            resolve_their_library_owner(builder, in_trigger).map(|rp| (None, rp))
        }
        "its controller" | "its controller's" | "itscontroller" => match builder.target {
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Some((None, ResolvingPlayer::TargetController)),
            BodyTarget::SingleSpell(_)
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::SingleStackEntry
            | BodyTarget::ControlledAbility { .. }
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Some((None, ResolvingPlayer::TargetSpellController))
            }
            _ if in_trigger => Some((None, ResolvingPlayer::TriggerSubjectController)),
            _ => None,
        },
        "its owner" | "its owner's" | "itsowner" => match builder.target {
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
            | BodyTarget::SingleSpell(_) => Some((None, ResolvingPlayer::TargetOwner)),
            _ if in_trigger => Some((None, ResolvingPlayer::TriggerSubjectOwner)),
            _ => None,
        },
        "defending player" | "defending player's" | "defendingplayer" => {
            Some((None, ResolvingPlayer::DefendingPlayer))
        }
        "enchanted player" | "enchanted player's" | "enchantedplayer" if builder.has_source => {
            Some((None, ResolvingPlayer::EnchantedPlayer))
        }
        "active player" | "active player's" | "activeplayer" => {
            Some((None, ResolvingPlayer::ActivePlayer))
        }
        _ => None,
    }
}

pub(super) fn parse_extreme_life_tie_mode(
    ins: &InstructionAst,
    highest: bool,
) -> Option<spellwake_engine::program::ExtremeLifeTieMode> {
    use spellwake_engine::program::ExtremeLifeTieMode;
    let InstructionAst::IfCondition {
        condition,
        then_body,
    } = ins
    else {
        return None;
    };
    let cond_matches = match condition.as_ref() {
        ConditionAst::PlayersTiedFor { metric, min_tied } if *min_tied == 2 => {
            let ml = metric.trim().to_ascii_lowercase();
            if highest {
                (ml.contains("highest") || ml.contains("most") || ml.contains("greatest"))
                    && !ml.contains("lowest")
                    && !ml.contains("least")
                    && !ml.contains("fewest")
            } else {
                (ml.contains("lowest") || ml.contains("least") || ml.contains("fewest"))
                    && !ml.contains("highest")
                    && !ml.contains("most")
                    && !ml.contains("greatest")
            }
        }
        ConditionAst::EventOccurredThisTurn { event } => {
            let el = event.trim().trim_end_matches('.').to_ascii_lowercase();
            matches!(el.as_str(), "there is a tie" | "there's a tie")
        }
        _ => false,
    };
    if !cond_matches {
        return None;
    }
    match then_body.as_slice() {
        [InstructionAst::ChooseOption(opt)]
            if matches!(
                opt.trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
                    .as_str(),
                "you choose one of them"
                    | "one of them"
                    | "choose one of them"
                    | "you choose one"
                    | "choose one"
            ) =>
        {
            Some(ExtremeLifeTieMode::ControllerChooses)
        }
        [
            InstructionAst::AbilityRestriction {
                restriction,
                condition: None,
            },
        ] if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("the game is a draw") =>
        {
            Some(ExtremeLifeTieMode::DrawGame)
        }
        _ => None,
    }
}

pub(super) fn is_look_top_rest_anaphora(subj: &SubjectRefAst) -> bool {
    matches!(
        subj,
        SubjectRefAst::Anaphora(a)
            if matches!(
                a.trim().to_ascii_lowercase().as_str(),
                "the_rest"
                    | "the rest"
                    | "rest"
                    | "the other"
                    | "the other one"
                    | "the other cards"
                    | "the other card"
                    | "the others"
                    | "any other cards"
                    | "them"
                    | "those cards"
                    | "the rest of them"
                    | "the rest of the cards"
                    | "the rest of those cards"
                    | "the remaining cards"
                    | "the remaining card"
                    | "the remaining one"
                    | "the unchosen cards"
                    | "any cards not chosen"
            )
    )
}

pub(super) struct LookTopRestDest {
    rest_gy: bool,
    rest_exile: bool,
    rest_top: bool,
    rest_hand: bool,
    random_rest: bool,
}

impl LookTopRestDest {
    fn to_engine_ext(&self) -> spellwake_engine::program::LookTopRestDest {
        if self.rest_gy {
            spellwake_engine::program::LookTopRestDest::Graveyard
        } else if self.rest_exile {
            spellwake_engine::program::LookTopRestDest::Exile
        } else if self.rest_hand {
            spellwake_engine::program::LookTopRestDest::Hand
        } else if self.rest_top {
            spellwake_engine::program::LookTopRestDest::TopOrder
        } else if self.random_rest {
            spellwake_engine::program::LookTopRestDest::BottomRandom
        } else {
            spellwake_engine::program::LookTopRestDest::BottomOrder
        }
    }
}

pub(super) fn parse_look_top_keep_dest_ext(
    keep_to: &str,
    tapped_flag: bool,
) -> Option<spellwake_engine::program::LookTopKeepDest> {
    let low = keep_to.trim().to_ascii_lowercase();
    if low.contains("battlefield") {
        return Some(spellwake_engine::program::LookTopKeepDest::Battlefield {
            tapped: tapped_flag || low.contains("tapped"),
            attacking: low.contains("attacking"),
        });
    }
    if low.contains("hand") {
        return Some(spellwake_engine::program::LookTopKeepDest::Hand);
    }
    if low.contains("graveyard") {
        return Some(spellwake_engine::program::LookTopKeepDest::Graveyard);
    }
    if low.contains("exile") {
        let playable = low.contains("play") || low.contains("cast");
        let duration = if !playable {
            ResolvedDuration::NoStatedDuration
        } else if low.contains("next turn") {
            ResolvedDuration::ThroughControllerNextTurn
        } else if low.contains("end of turn") || low.contains("this turn") {
            ResolvedDuration::UntilEndOfTurn
        } else {
            ResolvedDuration::NoStatedDuration
        };
        let free = low.contains("without paying");
        let cast_only = low.contains("cast") && !low.contains("play");
        let any_color = low.contains("mana of any");
        if low.contains("face down") {
            return Some(spellwake_engine::program::LookTopKeepDest::ExileLinked {
                face_down: true,
                playable,
                free,
                cast_only,
                any_color,
                duration,
            });
        }
        return Some(spellwake_engine::program::LookTopKeepDest::Exile {
            playable,
            free,
            cast_only,
            any_color,
            duration,
        });
    }
    if (!low.contains("bottom") && low.contains("top"))
        || low == "back"
        || low.contains("back on top")
    {
        return Some(spellwake_engine::program::LookTopKeepDest::LibraryTop);
    }
    None
}

pub(super) fn parse_look_top_rest_dest_ext(
    rest_to: &str,
) -> Option<spellwake_engine::program::LookTopRestDest> {
    let low = rest_to.trim().to_ascii_lowercase();
    if low.contains("graveyard") {
        Some(spellwake_engine::program::LookTopRestDest::Graveyard)
    } else if low.contains("exile") {
        Some(spellwake_engine::program::LookTopRestDest::Exile)
    } else if low.contains("hand") {
        Some(spellwake_engine::program::LookTopRestDest::Hand)
    } else if low.contains("bottom") {
        if low.contains("random") {
            Some(spellwake_engine::program::LookTopRestDest::BottomRandom)
        } else {
            Some(spellwake_engine::program::LookTopRestDest::BottomOrder)
        }
    } else if low.contains("top") && !low.contains("bottom") {
        Some(spellwake_engine::program::LookTopRestDest::TopOrder)
    } else {
        None
    }
}

pub(super) fn extract_look_top_rest_dest(op: &InstructionAst) -> Option<LookTopRestDest> {
    match op {
        InstructionAst::ReorderTopLibrary => Some(LookTopRestDest {
            rest_gy: false,
            rest_exile: false,
            rest_top: true,
            rest_hand: false,
            random_rest: false,
        }),
        InstructionAst::MoveZone { subject, to } if is_look_top_rest_anaphora(subject) => {
            let low = to.to_ascii_lowercase();
            let rest_bottom = low.contains("bottom");
            let rest_gy = low.contains("graveyard");
            let rest_exile = low.contains("exile");
            let rest_hand = low.contains("hand");
            let rest_top = !rest_bottom && low.contains("top") && low.contains("library");
            let random_rest = low.contains("random");
            (rest_bottom || rest_gy || rest_exile || rest_top || rest_hand).then_some(
                LookTopRestDest {
                    rest_gy,
                    rest_exile,
                    rest_top,
                    rest_hand,
                    random_rest,
                },
            )
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone: None,
            to,
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            ..
        } if is_look_top_rest_anaphora(subject) => {
            let low = to.to_ascii_lowercase();
            let random_rest = *random || low.contains("random");
            let rest_gy = low.contains("graveyard");
            let rest_exile = low.contains("exile");
            let rest_hand = low.contains("hand");
            let rest_top = !random_rest && !low.contains("bottom") && low.contains("top");
            let rest_bottom = low.contains("bottom")
                || (random_rest
                    && !rest_gy
                    && !rest_exile
                    && !rest_top
                    && !rest_hand
                    && low.contains("library"));
            (rest_bottom || rest_gy || rest_exile || rest_top || rest_hand).then_some(
                LookTopRestDest {
                    rest_gy,
                    rest_exile,
                    rest_top,
                    rest_hand,
                    random_rest,
                },
            )
        }
        _ => None,
    }
}

pub(super) struct LookTopKeepBucket {
    filter: Option<ObjectFilter>,
    keep_dest: spellwake_engine::program::LookTopKeepDest,
    to_battlefield: bool,
    enters_tapped: bool,
    take_all_matching: bool,
    max_picks: u8,
}

pub(super) fn parse_look_top_keep_arity(keep_low: &str, rev_tag: &str) -> (bool, u8) {
    if rev_tag == "revealed:all" {
        return (true, 1);
    }
    if rev_tag == "revealed:any_number" {
        return (false, u8::MAX);
    }
    if let Some(num_str) = rev_tag.strip_prefix("revealed:")
        && let Ok(n) = num_str.parse::<u8>()
        && n >= 2
    {
        return (false, n);
    }
    if keep_low.contains("all of them")
        || keep_low.contains("all cards")
        || keep_low.starts_with("all ")
        || keep_low.contains("each of them")
    {
        (true, 1)
    } else if keep_low.contains("any number") {
        (false, u8::MAX)
    } else if keep_low.contains("four")
        || keep_low.starts_with("up to 4 ")
        || keep_low.starts_with("4 ")
    {
        (false, 4)
    } else if keep_low.contains("three")
        || keep_low.starts_with("up to 3 ")
        || keep_low.starts_with("3 ")
    {
        (false, 3)
    } else if keep_low.contains("two")
        || keep_low.starts_with("up to 2 ")
        || keep_low.starts_with("2 ")
    {
        (false, 2)
    } else {
        (false, 1)
    }
}

pub(super) fn is_look_top_zone_ref_predicate(
    pred: &spellwake_oracle_compiler::ast::FilterPredicateAst,
) -> bool {
    match pred {
        spellwake_oracle_compiler::ast::FilterPredicateAst::PriorActionZoneRef { kind } => {
            let low = kind.trim().to_ascii_lowercase();
            matches!(
                low.as_str(),
                "from_among_those"
                    | "from_among_those_cards"
                    | "among_them"
                    | "among them"
                    | "from_among_them"
                    | "from among them"
                    | "the_looked_at_cards"
                    | "the looked-at cards"
                    | "looked_at_cards"
                    | "the_revealed_cards"
                    | "the revealed cards"
                    | "revealed_cards"
                    | "revealed_this_way"
                    | "in_referenced_zone"
            )
        }
        spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
            spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay,
        ) => true,
        _ => false,
    }
}

pub(super) fn strip_look_top_zone_ref(f: &ObjectFilterAst) -> ObjectFilterAst {
    let mut cleaned = f.clone();
    cleaned
        .predicates
        .retain(|p| !is_look_top_zone_ref_predicate(p));
    cleaned
}

pub(super) fn parse_grant_cast_duration_resolved(
    duration: &str,
) -> Option<(ResolvedDuration, bool)> {
    let dur_low = duration.trim().to_ascii_lowercase();
    let any_color = dur_low.contains("any color") || dur_low.contains("any type");
    let resolved = if dur_low == "until_end_of_next_turn"
        || dur_low.contains("end of your next turn")
        || dur_low.contains("untilendofnextturn")
        || dur_low.contains("untilyournextturn")
    {
        ResolvedDuration::ThroughControllerNextTurn
    } else if dur_low == "while_exiled"
        || dur_low.contains("remains exiled")
        || dur_low.contains("remain exiled")
        || dur_low == "permanent"
        || dur_low == "until_next_exile"
        || dur_low.contains("until you exile another")
    {
        ResolvedDuration::NoStatedDuration
    } else if dur_low.is_empty()
        || dur_low == "this_turn"
        || dur_low == "until_end_of_turn"
        || dur_low == "now"
        || dur_low.contains("this turn")
        || dur_low.contains("end of turn")
        || dur_low.contains("untilendofturn")
    {
        ResolvedDuration::UntilEndOfTurn
    } else {
        return None;
    };
    Some((resolved, any_color))
}

pub(super) fn extract_look_top_rest_or_shuffle(
    op: &InstructionAst,
) -> Option<(LookTopRestDest, bool)> {
    if let Some(rest) = extract_look_top_rest_dest(op) {
        return Some((rest, false));
    }
    let is_shuffle_rest = matches!(op, InstructionAst::ShuffleLibrary { .. })
        || matches!(
            op,
            InstructionAst::ShuffleIntoLibrary { subject } if is_look_top_rest_anaphora(subject)
        );
    is_shuffle_rest.then_some((
        LookTopRestDest {
            rest_gy: false,
            rest_exile: false,
            rest_top: true,
            rest_hand: false,
            random_rest: false,
        },
        true,
    ))
}

pub(super) fn parse_look_top_single_keep_op(
    op: &InstructionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<LookTopKeepBucket> {
    let parse_filter_and_arity =
        |subj: &SubjectRefAst| -> Option<(bool, u8, Option<ObjectFilter>)> {
            match subj {
                SubjectRefAst::Group(f) => {
                    let cleaned = strip_look_top_zone_ref(f);
                    if is_unrestricted_filter(&cleaned) {
                        Some((false, 1, None))
                    } else {
                        lower_object_filter(&cleaned, vocabulary)
                            .ok()
                            .map(|o| (false, 1, Some(o)))
                    }
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let cleaned = strip_look_top_zone_ref(f);
                    if is_unrestricted_filter(&cleaned) {
                        Some((true, 1, None))
                    } else {
                        lower_object_filter(&cleaned, vocabulary)
                            .ok()
                            .map(|o| (true, 1, Some(o)))
                    }
                }
                SubjectRefAst::Anaphora(a) => {
                    let al = a.trim().to_ascii_lowercase();
                    match al.as_str() {
                        "one of them"
                        | "one"
                        | "up to one of them"
                        | "up to one"
                        | "that card"
                        | "it"
                        | "a card from among them" => Some((false, 1, None)),
                        "two of them" | "two" | "up to two of them" | "up to two" => {
                            Some((false, 2, None))
                        }
                        "three of them" | "three" | "up to three of them" | "up to three" => {
                            Some((false, 3, None))
                        }
                        "four of them" | "four" | "up to four of them" | "up to four" => {
                            Some((false, 4, None))
                        }
                        "any number of them" | "any number" => Some((false, u8::MAX, None)),
                        _ => None,
                    }
                }
                _ => None,
            }
        };
    let build_from_move =
        |subj: &SubjectRefAst, to: &str, tapped_flag: bool, override_max: Option<u8>| {
            let keep_dest = parse_look_top_keep_dest_ext(to, tapped_flag)?;
            let (to_battlefield, enters_tapped) = match keep_dest {
                spellwake_engine::program::LookTopKeepDest::Battlefield { tapped, .. } => {
                    (true, tapped)
                }
                _ => (false, false),
            };
            let (take_all_matching, subj_max, filter) = parse_filter_and_arity(subj)?;
            Some(LookTopKeepBucket {
                filter,
                keep_dest,
                to_battlefield,
                enters_tapped,
                take_all_matching,
                max_picks: override_max.unwrap_or(subj_max),
            })
        };
    match op {
        InstructionAst::RevealFromLooked { filter, to }
            if to != "revealed" && !to.starts_with("revealed:") =>
        {
            let keep_dest = parse_look_top_keep_dest_ext(to, false)?;
            let keep_low = to.to_ascii_lowercase();
            let (to_battlefield, enters_tapped) = match keep_dest {
                spellwake_engine::program::LookTopKeepDest::Battlefield { tapped, .. } => {
                    (true, tapped)
                }
                _ => (false, false),
            };
            let (take_all_matching, max_picks) = parse_look_top_keep_arity(&keep_low, "");
            let cleaned = strip_look_top_zone_ref(filter);
            let obj = lower_object_filter(&cleaned, vocabulary).ok()?;
            Some(LookTopKeepBucket {
                filter: Some(obj),
                keep_dest,
                to_battlefield,
                enters_tapped,
                take_all_matching,
                max_picks,
            })
        }
        InstructionAst::MoveZone { subject, to } => build_from_move(subject, to, false, None),
        InstructionAst::Exile {
            actor: None | Some(PlayerRefAst::You),
            subject,
            random: false,
            face_down: false,
            into_pile: false,
        } => build_from_move(subject, "exile", false, None),
        InstructionAst::ReturnZone {
            subject,
            from_zone: None,
            to,
            tapped,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count,
            any_number,
            ..
        } => {
            let override_max = if *any_number {
                Some(u8::MAX)
            } else if let Some(cnt_ast) = count {
                match cnt_ast.as_ref() {
                    ValueExprAst::Const(n) if *n >= 1 => u8::try_from(*n).ok(),
                    _ => return None,
                }
            } else {
                None
            };
            build_from_move(subject, to, *tapped, override_max)
        }
        InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only,
            subject,
            free,
            duration,
            count_limit,
            alt_cost: None,
        } if count_limit.is_none()
            || matches!(
                count_limit,
                Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
            )
            || matches!(
                count_limit,
                Some(spellwake_oracle_compiler::ast::CountLimitAst::UpTo(e))
                    if matches!(e.as_ref(), ValueExprAst::Const(1))
            ) =>
        {
            let (resolved_dur, any_color) = parse_grant_cast_duration_resolved(duration)?;
            let is_any_number = matches!(
                count_limit,
                Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
            );
            let (take_all_matching, subj_max, filter) = match subject.as_ref() {
                SubjectRefAst::Group(f) => {
                    let cleaned = strip_look_top_zone_ref(f);
                    let f_opt = if is_unrestricted_filter(&cleaned) {
                        None
                    } else {
                        Some(lower_object_filter(&cleaned, vocabulary).ok()?)
                    };
                    (false, if is_any_number { u8::MAX } else { 1 }, f_opt)
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    let cleaned = strip_look_top_zone_ref(f);
                    let f_opt = if is_unrestricted_filter(&cleaned) {
                        None
                    } else {
                        Some(lower_object_filter(&cleaned, vocabulary).ok()?)
                    };
                    (true, 1, f_opt)
                }
                SubjectRefAst::Anaphora(a) => {
                    let al = a.trim().to_ascii_lowercase();
                    match al.as_str() {
                        "it"
                        | "that card"
                        | "that spell"
                        | "one of them"
                        | "one"
                        | "up to one of them"
                        | "up to one"
                        | "a card from among them"
                        | "a spell from among them" => (false, 1, None),
                        "them" | "those cards" | "all of them" => (true, 1, None),
                        "any number of them" | "any number" => (false, u8::MAX, None),
                        _ => return None,
                    }
                }
                _ => return None,
            };
            Some(LookTopKeepBucket {
                filter,
                keep_dest: spellwake_engine::program::LookTopKeepDest::Exile {
                    playable: true,
                    free: *free,
                    cast_only: *cast_only,
                    any_color,
                    duration: resolved_dur,
                },
                to_battlefield: false,
                enters_tapped: false,
                take_all_matching,
                max_picks: subj_max,
            })
        }
        InstructionAst::Optional(inner) => match inner.as_slice() {
            [single] => parse_look_top_single_keep_op(single, vocabulary),
            [
                grant @ InstructionAst::GrantCastPermission { .. },
                mana_rule,
            ] if is_any_color_spend_restriction(mana_rule) => {
                let mut bucket = parse_look_top_single_keep_op(grant, vocabulary)?;
                if let spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. } =
                    &mut bucket.keep_dest
                {
                    *any_color = true;
                }
                Some(bucket)
            }
            [InstructionAst::RevealFromLooked { filter, to: rev_to }, mv]
                if rev_to == "revealed" || rev_to.starts_with("revealed:") =>
            {
                let (keep_to, tapped_flag) = match mv {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                    {
                        (to.as_str(), false)
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        from_zone: None,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        (to.as_str(), *tapped)
                    }
                    _ => return None,
                };
                let keep_dest = parse_look_top_keep_dest_ext(keep_to, tapped_flag)?;
                let keep_low = keep_to.to_ascii_lowercase();
                let (to_battlefield, enters_tapped) = match keep_dest {
                    spellwake_engine::program::LookTopKeepDest::Battlefield { tapped, .. } => {
                        (true, tapped)
                    }
                    _ => (false, false),
                };
                let (take_all_matching, max_picks) = parse_look_top_keep_arity(&keep_low, rev_to);
                let cleaned = strip_look_top_zone_ref(filter);
                let obj = lower_object_filter(&cleaned, vocabulary).ok()?;
                Some(LookTopKeepBucket {
                    filter: Some(obj),
                    keep_dest,
                    to_battlefield,
                    enters_tapped,
                    take_all_matching,
                    max_picks,
                })
            }
            [
                InstructionAst::RevealCards { subject: rev_subj }
                | InstructionAst::RevealByPlayer {
                    player: PlayerRefAst::You,
                    subject: rev_subj,
                },
                mv,
            ] => {
                let (keep_to, tapped_flag) = match mv {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                    {
                        (to.as_str(), false)
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        from_zone: None,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        (to.as_str(), *tapped)
                    }
                    _ => return None,
                };
                build_from_move(rev_subj, keep_to, tapped_flag, None)
            }
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn try_fuse_look_top_extended(
    ins: &[InstructionAst],
    i: usize,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Option<usize> {
    let (reveal_val, library, count, eff_bound_x) =
        interactive::unwrap_look_top_with_bind_x(&ins[i], bound_x)?;
    let reveal = &reveal_val;
    let bound_x = eff_bound_x;
    let (req_bt, library_owner) = parse_look_top_library_owner(library, builder, in_trigger)?;
    let push_shuffle = |ops: &mut Vec<Instruction>, owner: ResolvingPlayer| {
        if owner == ResolvingPlayer::Controller {
            ops.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library(),
            );
        } else {
            ops.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(
                    owner,
                ),
            );
        }
    };
    // Case 1: 2-step LookTopLibrary(Const(1)) + [Optional?]MoveZone/ReturnZone(card-anaphora, bottom)
    if i + 1 < ins.len() && matches!(count, ValueExprAst::Const(1)) {
        let single_bottom: Option<bool> = match &ins[i + 1] {
            InstructionAst::MoveZone { subject, to }
                if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_top_card_anaphora_label(a))
                    && to.to_ascii_lowercase().contains("bottom") =>
            {
                Some(false)
            }
            InstructionAst::ReturnZone {
                subject,
                from_zone: None,
                to,
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                except: None,
                count: None,
                up_to: false,
                any_number: false,
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_top_card_anaphora_label(a))
                && (to.to_ascii_lowercase().contains("bottom")
                    || to.to_ascii_lowercase().contains("random order")) =>
            {
                Some(false)
            }
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [InstructionAst::MoveZone { subject, to }]
                    if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_top_card_anaphora_label(a))
                        && to.to_ascii_lowercase().contains("bottom") =>
                {
                    Some(true)
                }
                [
                    InstructionAst::ReturnZone {
                        subject,
                        from_zone: None,
                        to,
                        tapped: false,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        except: None,
                        count: None,
                        up_to: false,
                        any_number: false,
                        ..
                    },
                ] if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_top_card_anaphora_label(a))
                    && (to.to_ascii_lowercase().contains("bottom")
                        || to.to_ascii_lowercase().contains("random order")) =>
                {
                    Some(true)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(is_opt) = single_bottom {
            let mut probe = builder.clone();
            if let Some(bt) = req_bt {
                probe.bind_target(bt).ok()?;
            }
            *builder = probe;
            builder.instructions.push(Instruction::LookTopChooseExt {
                library_owner,
                chooser: ResolvingPlayer::Controller,
                count: ValueExpr::literal(1),
                filter: None,
                max_picks: 0,
                take_all_matching: false,
                keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                reveal_kept: false,
                reveal_all: *reveal,
                reveal_only: true,
                secondary: None,
            });
            if is_opt {
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
            }
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject(
                    MoveSubjectRef::RevealedCard,
                    SubjectDestination::Library(LibraryPlacement::Bottom),
                ),
            );
            return Some(i + 2);
        }
    }
    // Case 1b: 2-step LookTopLibrary(count) + ReorderTopLibrary / MoveZone/ReturnZone(rest, top of library)
    if i + 1 < ins.len()
        && let Some(rest) = extract_look_top_rest_dest(&ins[i + 1])
        && rest.rest_top
        && !rest.rest_gy
        && !rest.rest_exile
        && !rest.rest_hand
    {
        let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary).ok()?;
        let mut probe = builder.clone();
        if let Some(bt) = req_bt {
            probe.bind_target(bt).ok()?;
        }
        *builder = probe;
        builder.instructions.push(Instruction::LookTopChooseExt {
            library_owner,
            chooser: ResolvingPlayer::Controller,
            count: cnt,
            filter: None,
            max_picks: 0,
            take_all_matching: false,
            keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
            rest_dest: spellwake_engine::program::LookTopRestDest::TopOrder,
            reveal_kept: false,
            reveal_all: *reveal,
            reveal_only: false,
            secondary: None,
        });
        return Some(i + 2);
    }
    // Case 2: Two-bucket LookTopLibrary + keep1 + keep2 + rest-move -> LookTopChoose with secondary
    if i + 3 < ins.len()
        && let Some(b1) = parse_look_top_single_keep_op(&ins[i + 1], vocabulary)
        && matches!(
            b1.keep_dest,
            spellwake_engine::program::LookTopKeepDest::Hand
                | spellwake_engine::program::LookTopKeepDest::Battlefield {
                    attacking: false,
                    ..
                }
        )
        && let Some(b2) = parse_look_top_single_keep_op(&ins[i + 2], vocabulary)
        && matches!(
            b2.keep_dest,
            spellwake_engine::program::LookTopKeepDest::Hand
                | spellwake_engine::program::LookTopKeepDest::Battlefield {
                    attacking: false,
                    ..
                }
        )
        && let Some(sec_filter) = b2.filter
        && !b2.take_all_matching
        && let Some((rest, do_shuffle)) = extract_look_top_rest_or_shuffle(&ins[i + 3])
        && !rest.rest_hand
    {
        let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary).ok()?;
        let mut probe = builder.clone();
        if let Some(bt) = req_bt {
            probe.bind_target(bt).ok()?;
        }
        *builder = probe;
        builder.instructions.push(Instruction::LookTopChooseExt {
            library_owner,
            chooser: ResolvingPlayer::Controller,
            count: cnt,
            filter: b1.filter.map(Box::new),
            max_picks: b1.max_picks,
            take_all_matching: b1.take_all_matching,
            keep_dest: if b1.to_battlefield {
                spellwake_engine::program::LookTopKeepDest::Battlefield {
                    tapped: b1.enters_tapped,
                    attacking: false,
                }
            } else {
                spellwake_engine::program::LookTopKeepDest::Hand
            },
            rest_dest: if rest.rest_gy {
                spellwake_engine::program::LookTopRestDest::Graveyard
            } else if rest.rest_exile {
                spellwake_engine::program::LookTopRestDest::Exile
            } else if rest.rest_top {
                spellwake_engine::program::LookTopRestDest::TopOrder
            } else if rest.random_rest {
                spellwake_engine::program::LookTopRestDest::BottomRandom
            } else {
                spellwake_engine::program::LookTopRestDest::BottomOrder
            },
            reveal_kept: false,
            reveal_all: *reveal,
            reveal_only: false,
            secondary: Some(Box::new(spellwake_engine::program::LookTopSecondary {
                filter: Box::new(sec_filter),
                to_battlefield: b2.to_battlefield,
                enters_tapped: b2.enters_tapped,
                max_picks: b2.max_picks,
            })),
        });
        if do_shuffle {
            push_shuffle(&mut builder.instructions, library_owner);
        }
        return Some(i + 4);
    }
    // Case 3: 3-step (or 4-step with intervening any-color spend rule) LookTopLibrary + keep1
    // (including Optional([RevealCards/RevealFromLooked, ReturnZone/MoveZone]) and GrantCastPermission)
    // + rest-move / ShuffleLibrary, supporting non-Controller library_owner too.
    if i + 2 < ins.len()
        && let Some(mut b1) = parse_look_top_single_keep_op(&ins[i + 1], vocabulary)
    {
        let rest_match =
            if let Some((rest, do_shuffle)) = extract_look_top_rest_or_shuffle(&ins[i + 2]) {
                Some((rest, do_shuffle, false, 3usize))
            } else if i + 3 < ins.len()
                && is_any_color_spend_restriction(&ins[i + 2])
                && matches!(
                    b1.keep_dest,
                    spellwake_engine::program::LookTopKeepDest::Exile { playable: true, .. }
                )
                && let Some((rest, do_shuffle)) = extract_look_top_rest_or_shuffle(&ins[i + 3])
            {
                Some((rest, do_shuffle, true, 4usize))
            } else {
                None
            };
        if let Some((rest, do_shuffle, set_any_color, step)) = rest_match {
            if set_any_color
                && let spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. } =
                    &mut b1.keep_dest
            {
                *any_color = true;
            }
            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary).ok()?;
            let mut probe = builder.clone();
            if let Some(bt) = req_bt {
                probe.bind_target(bt).ok()?;
            }
            *builder = probe;
            let needs_ext = do_shuffle
                || rest.rest_hand
                || !matches!(
                    b1.keep_dest,
                    spellwake_engine::program::LookTopKeepDest::Hand
                        | spellwake_engine::program::LookTopKeepDest::Battlefield {
                            attacking: false,
                            ..
                        }
                );
            if needs_ext {
                let reveal_kept = *reveal || b1.filter.is_some();
                builder.instructions.push(Instruction::LookTopChooseExt {
                    library_owner,
                    chooser: ResolvingPlayer::Controller,
                    count: cnt,
                    filter: b1.filter.map(Box::new),
                    max_picks: b1.max_picks,
                    take_all_matching: b1.take_all_matching,
                    keep_dest: b1.keep_dest,
                    rest_dest: rest.to_engine_ext(),
                    reveal_kept,
                    reveal_all: false,
                    reveal_only: false,
                    secondary: None,
                });
            } else {
                builder.instructions.push(Instruction::LookTopChooseExt {
                    library_owner,
                    chooser: ResolvingPlayer::Controller,
                    count: cnt,
                    filter: b1.filter.map(Box::new),
                    max_picks: b1.max_picks,
                    take_all_matching: b1.take_all_matching,
                    keep_dest: if b1.to_battlefield {
                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                            tapped: b1.enters_tapped,
                            attacking: false,
                        }
                    } else {
                        spellwake_engine::program::LookTopKeepDest::Hand
                    },
                    rest_dest: if rest.rest_gy {
                        spellwake_engine::program::LookTopRestDest::Graveyard
                    } else if rest.rest_exile {
                        spellwake_engine::program::LookTopRestDest::Exile
                    } else if rest.rest_top {
                        spellwake_engine::program::LookTopRestDest::TopOrder
                    } else if rest.random_rest {
                        spellwake_engine::program::LookTopRestDest::BottomRandom
                    } else {
                        spellwake_engine::program::LookTopRestDest::BottomOrder
                    },
                    reveal_kept: false,
                    reveal_all: *reveal,
                    reveal_only: false,
                    secondary: None,
                });
            }
            if do_shuffle {
                push_shuffle(&mut builder.instructions, library_owner);
            }
            return Some(i + step);
        }
    }
    // Case 4: 4-step LookTopLibrary + [Optional?](RevealCards | RevealFromLooked("revealed"))
    // + [Optional?]MoveZone/ReturnZone(Anaphora, hand/bf) + rest-move / ShuffleLibrary
    if i + 3 < ins.len()
        && let Some((rest, do_shuffle)) = extract_look_top_rest_or_shuffle(&ins[i + 3])
    {
        enum LookTopRevealStep<'a> {
            Cards(&'a SubjectRefAst),
            FromLooked(&'a ObjectFilterAst, &'a str),
        }
        let rev_step = match &ins[i + 1] {
            InstructionAst::RevealCards { subject }
            | InstructionAst::RevealByPlayer {
                player: PlayerRefAst::You,
                subject,
            } => Some(LookTopRevealStep::Cards(subject.as_ref())),
            InstructionAst::RevealFromLooked { filter, to }
                if to == "revealed" || to.starts_with("revealed:") =>
            {
                Some(LookTopRevealStep::FromLooked(filter.as_ref(), to.as_str()))
            }
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [
                    InstructionAst::RevealCards { subject }
                    | InstructionAst::RevealByPlayer {
                        player: PlayerRefAst::You,
                        subject,
                    },
                ] => Some(LookTopRevealStep::Cards(subject.as_ref())),
                [InstructionAst::RevealFromLooked { filter, to }]
                    if to == "revealed" || to.starts_with("revealed:") =>
                {
                    Some(LookTopRevealStep::FromLooked(filter.as_ref(), to.as_str()))
                }
                _ => None,
            },
            _ => None,
        };
        let mv_info = match &ins[i + 2] {
            InstructionAst::MoveZone { subject, to }
                if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
            {
                Some((to.as_str(), false))
            }
            InstructionAst::ReturnZone {
                subject,
                from_zone: None,
                to,
                tapped,
                transformed: false,
                with_counter: None,
                attached_to: None,
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                Some((to.as_str(), *tapped))
            }
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [InstructionAst::MoveZone { subject, to }]
                    if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                {
                    Some((to.as_str(), false))
                }
                [
                    InstructionAst::ReturnZone {
                        subject,
                        from_zone: None,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    },
                ] if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                    Some((to.as_str(), *tapped))
                }
                _ => None,
            },
            _ => None,
        };
        if let (Some(rstep), Some((keep_to, tapped_flag))) = (rev_step, mv_info) {
            let bucket_opt = match rstep {
                LookTopRevealStep::Cards(rsubj) => {
                    let synth_mv = InstructionAst::MoveZone {
                        subject: Box::new(rsubj.clone()),
                        to: if tapped_flag && !keep_to.to_ascii_lowercase().contains("tapped") {
                            format!("{keep_to} tapped")
                        } else {
                            keep_to.to_string()
                        },
                    };
                    parse_look_top_single_keep_op(&synth_mv, vocabulary)
                }
                LookTopRevealStep::FromLooked(filter_ast, rev_tag) => {
                    let keep_low = keep_to.to_ascii_lowercase();
                    parse_look_top_keep_dest_ext(keep_to, tapped_flag).and_then(|keep_dest| {
                        let (to_battlefield, enters_tapped) = match keep_dest {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped,
                                ..
                            } => (true, tapped),
                            _ => (false, false),
                        };
                        let (take_all_matching, max_picks) =
                            parse_look_top_keep_arity(&keep_low, rev_tag);
                        let cleaned = strip_look_top_zone_ref(filter_ast);
                        lower_object_filter(&cleaned, vocabulary).ok().map(|obj| {
                            LookTopKeepBucket {
                                filter: Some(obj),
                                keep_dest,
                                to_battlefield,
                                enters_tapped,
                                take_all_matching,
                                max_picks,
                            }
                        })
                    })
                }
            };
            if let Some(b1) = bucket_opt {
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary).ok()?;
                let mut probe = builder.clone();
                if let Some(bt) = req_bt {
                    probe.bind_target(bt).ok()?;
                }
                *builder = probe;
                let needs_ext = do_shuffle
                    || rest.rest_hand
                    || !matches!(
                        b1.keep_dest,
                        spellwake_engine::program::LookTopKeepDest::Hand
                            | spellwake_engine::program::LookTopKeepDest::Battlefield {
                                attacking: false,
                                ..
                            }
                    );
                if needs_ext {
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner,
                        chooser: ResolvingPlayer::Controller,
                        count: cnt,
                        filter: b1.filter.map(Box::new),
                        max_picks: b1.max_picks,
                        take_all_matching: b1.take_all_matching,
                        keep_dest: b1.keep_dest,
                        rest_dest: rest.to_engine_ext(),
                        reveal_kept: true,
                        reveal_all: false,
                        reveal_only: false,
                        secondary: None,
                    });
                } else {
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner,
                        chooser: ResolvingPlayer::Controller,
                        count: cnt,
                        filter: b1.filter.map(Box::new),
                        max_picks: b1.max_picks,
                        take_all_matching: b1.take_all_matching,
                        keep_dest: if b1.to_battlefield {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped: b1.enters_tapped,
                                attacking: false,
                            }
                        } else {
                            spellwake_engine::program::LookTopKeepDest::Hand
                        },
                        rest_dest: if rest.rest_gy {
                            spellwake_engine::program::LookTopRestDest::Graveyard
                        } else if rest.rest_exile {
                            spellwake_engine::program::LookTopRestDest::Exile
                        } else if rest.rest_top {
                            spellwake_engine::program::LookTopRestDest::TopOrder
                        } else if rest.random_rest {
                            spellwake_engine::program::LookTopRestDest::BottomRandom
                        } else {
                            spellwake_engine::program::LookTopRestDest::BottomOrder
                        },
                        reveal_kept: false,
                        reveal_all: *reveal,
                        reveal_only: false,
                        secondary: None,
                    });
                }
                if do_shuffle {
                    push_shuffle(&mut builder.instructions, library_owner);
                }
                return Some(i + 4);
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_fuse_choose_keyword_grant(
    ins: &[InstructionAst],
    i: usize,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Option<usize> {
    if i + 1 >= ins.len() {
        return None;
    }
    let InstructionAst::ChooseOption(opts_raw) = &ins[i] else {
        return None;
    };
    let (subject, duration) = match &ins[i + 1] {
        InstructionAst::GrantAbilityText {
            subject,
            ability_text,
            compiled: None,
            duration,
        } if matches!(
            ability_text
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "that ability" | "the chosen ability" | "that keyword" | "the chosen keyword"
        ) =>
        {
            (subject, duration)
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } if keywords.len() == 1
            && matches!(
                keywords[0]
                    .trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
                    .as_str(),
                "that ability" | "the chosen ability" | "that keyword" | "the chosen keyword"
            ) =>
        {
            (subject, duration)
        }
        _ => return None,
    };
    let trimmed = opts_raw.trim().trim_end_matches('.');
    let (chooser_ref, list_raw) = if let Some((prefix, rest)) = trimmed.split_once(':') {
        let pr = match prefix.trim().to_ascii_lowercase().as_str() {
            "you" => PlayerRefAst::You,
            "targetplayer" | "target player" => PlayerRefAst::TargetPlayer,
            "targetopponent" | "target opponent" => PlayerRefAst::TargetOpponent,
            "thatplayer" | "that player" => PlayerRefAst::ThatPlayer,
            "activeplayer" | "active player" => PlayerRefAst::ActivePlayer,
            "defendingplayer" | "defending player" => PlayerRefAst::DefendingPlayer,
            "enchantedplayer" | "enchanted player" => PlayerRefAst::EnchantedPlayer,
            _ => return None,
        };
        (pr, rest.trim())
    } else {
        (PlayerRefAst::You, trimmed)
    };
    let list_clean = list_raw
        .strip_prefix("choose ")
        .or_else(|| list_raw.strip_prefix("Choose "))
        .unwrap_or(list_raw)
        .trim();
    let mut kw_options: Vec<String> = Vec::new();
    for seg in list_clean.split(" or ") {
        for sub in seg.split(',') {
            let k = sub
                .trim()
                .trim_start_matches("or ")
                .trim_start_matches("and ")
                .trim();
            if !k.is_empty() {
                kw_options.push(k.to_string());
            }
        }
    }
    if kw_options.len() < 2 || kw_options.len() > 16 {
        return None;
    }
    for kw in &kw_options {
        if lower_ability_change(std::slice::from_ref(kw), vocabulary).is_err() {
            return None;
        }
    }
    let mut probe = builder.clone();
    let chooser = if chooser_ref == PlayerRefAst::You {
        ResolvingPlayer::Controller
    } else {
        lower_resolving_player_in_context(chooser_ref, &mut probe, in_trigger).ok()?
    };
    let mut shared_target: Option<BodyTarget> = None;
    let mut branches = Vec::with_capacity(kw_options.len());
    for kw in kw_options {
        let mut br = BodyBuilder::with_trigger_hint(probe.trigger_hint);
        br.has_source = probe.has_source;
        br.target = probe.target;
        let synth = InstructionAst::GrantKeywords {
            subject: subject.clone(),
            keywords: vec![kw.clone()],
            duration: duration.clone(),
        };
        if lower_instruction(
            &synth,
            &mut br,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )
        .is_err()
            || br.instructions.is_empty()
            || br.graveyard_target_bounds.is_some()
            || br.x_target_count
            || br.up_to_x_target_count
            || br.up_to_one_target
        {
            return None;
        }
        let br_target = br.target;
        if let Some(prev_t) = shared_target {
            if prev_t != br_target {
                return None;
            }
        } else {
            shared_target = Some(br_target);
        }
        br.target = BodyTarget::None;
        let body = br.finish().ok()?;
        branches.push(ChooseOptionBranch { label: kw, body });
    }
    if let Some(st) = shared_target
        && st != probe.target
    {
        probe.bind_target(st).ok()?;
    }
    *builder = probe;
    builder.instructions.push(Instruction::ChooseOption {
        chooser,
        options: branches,
    });
    Some(i + 2)
}

fn emit_condition_branches_resolution(
    condition: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<usize>, &'static str> {
    let mut false_branches = Vec::new();
    match condition {
        ConditionAst::And(a, b) => {
            let rc_a = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let rc_b = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_a,
                destination: 0,
            });
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_b,
                destination: 0,
            });
        }
        ConditionAst::Or(a, b) => {
            let mut rc_a = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let rc_b = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            rc_a.comparison = negate_comparison(rc_a.comparison);
            let true_jump_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_a,
                destination: 0,
            });
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_b,
                destination: 0,
            });
            let then_start = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(true_jump_idx)
            {
                *destination = then_start;
            }
        }
        other => {
            let res_cond =
                lower_resolution_condition(other, builder, vocabulary, in_trigger, bound_x)?;
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: res_cond,
                destination: 0,
            });
        }
    }
    Ok(false_branches)
}

pub(super) fn emit_condition_branches(
    condition: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<usize>, &'static str> {
    let snap_len = builder.instructions.len();
    let snap_target = builder.target;
    let snap_gy = builder.graveyard_target_bounds;
    let snap_x = builder.x_target_count;
    let snap_up_to_x = builder.up_to_x_target_count;
    let snap_group = builder.last_group_filter;
    match emit_condition_branches_resolution(condition, builder, vocabulary, in_trigger, bound_x) {
        Ok(branches) => Ok(branches),
        Err("oracle:unsupported-if-condition") => {
            builder.instructions.truncate(snap_len);
            builder.target = snap_target;
            builder.graveyard_target_bounds = snap_gy;
            builder.x_target_count = snap_x;
            builder.up_to_x_target_count = snap_up_to_x;
            builder.last_group_filter = snap_group;
            let cond_expr =
                lower_condition_expr(condition, builder, vocabulary, in_trigger, bound_x)?;
            let idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchCondition {
                condition: Box::new(cond_expr),
                destination: 0,
            });
            Ok(vec![idx])
        }
        Err(err) => Err(err),
    }
}

fn filter_references_that_player(f: &ObjectFilterAst) -> bool {
    matches!(
        f.controller,
        Some(ControllerRelation::ThatPlayer | ControllerRelation::OwnerThatPlayer)
    ) || matches!(
        f.owner,
        Some(ControllerRelation::ThatPlayer | ControllerRelation::OwnerThatPlayer)
    ) || matches!(
        f.zone,
        Some(
            ZoneFilter::ThatPlayerGraveyard
                | ZoneFilter::ThatPlayerHand
                | ZoneFilter::ThatPlayerLibrary
        )
    ) || f.extra_zones.iter().any(|z| {
        matches!(
            z,
            ZoneFilter::ThatPlayerGraveyard
                | ZoneFilter::ThatPlayerHand
                | ZoneFilter::ThatPlayerLibrary
        )
    })
}

fn subject_references_that_player(s: &SubjectRefAst) -> bool {
    match s {
        SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => true,
        SubjectRefAst::Group(f)
        | SubjectRefAst::All(f)
        | SubjectRefAst::Each(f)
        | SubjectRefAst::TargetObject { filter: f, .. } => filter_references_that_player(f),
        SubjectRefAst::InZone { subject, zone } => {
            matches!(
                zone,
                ZoneFilter::ThatPlayerGraveyard
                    | ZoneFilter::ThatPlayerHand
                    | ZoneFilter::ThatPlayerLibrary
            ) || subject_references_that_player(subject)
        }
        SubjectRefAst::Compound(items) => items.iter().any(subject_references_that_player),
        SubjectRefAst::ControllerOf(inner) | SubjectRefAst::OwnerOf(inner) => {
            subject_references_that_player(inner)
        }
        _ => false,
    }
}

fn value_expr_references_that_player(e: &ValueExprAst) -> bool {
    match e {
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::ThatPlayer,
            ..
        }
        | ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::ThatPlayer,
            ..
        }
        | ValueExprAst::StartingLifeTotal(PlayerRefAst::ThatPlayer)
        | ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::ThatPlayer,
            ..
        } => true,
        ValueExprAst::Count(f)
        | ValueExprAst::AggregateStat { filter: f, .. }
        | ValueExprAst::ColorsAmong(f)
        | ValueExprAst::CardTypesAmong(f)
        | ValueExprAst::SubtypesAmong(f) => filter_references_that_player(f),
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            subject_references_that_player(subject)
        }
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            value_expr_references_that_player(a) || value_expr_references_that_player(b)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_that_player(expr),
        _ => false,
    }
}

fn token_references_that_player(t: &TokenSpecAst) -> bool {
    match t {
        TokenSpecAst::Predefined { count, .. } | TokenSpecAst::Creature { count, .. } => {
            value_expr_references_that_player(count)
        }
        TokenSpecAst::Copy { count, source, .. } => {
            value_expr_references_that_player(count) || subject_references_that_player(source)
        }
        TokenSpecAst::Role { attached_to, .. } => subject_references_that_player(attached_to),
    }
}

pub(in super::super) fn try_wrap_each_player_that_player(
    ins: &InstructionAst,
) -> Option<InstructionAst> {
    let is_each = |p: PlayerRefAst| {
        matches!(
            p,
            PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer
        )
    };
    match ins {
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            if let SubjectRefAst::Player(p) = target.as_ref()
                && is_each(*p)
                && value_expr_references_that_player(amount)
            {
                Some(InstructionAst::ForEachPlayer {
                    player: *p,
                    body: vec![InstructionAst::DealDamage {
                        source: source.clone(),
                        target: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                        amount: amount.clone(),
                    }],
                })
            } else {
                None
            }
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } if is_each(*player) && value_expr_references_that_player(delta) => {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    delta: delta.clone(),
                    gain: *gain,
                }],
            })
        }
        InstructionAst::SetLifeTotal { player, value }
            if is_each(*player) && value_expr_references_that_player(value) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::SetLifeTotal {
                    player: PlayerRefAst::ThatPlayer,
                    value: value.clone(),
                }],
            })
        }
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } if is_each(*player) && value_expr_references_that_player(count) => {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    up_to: *up_to,
                }],
            })
        }
        InstructionAst::Mill { player, count }
            if is_each(*player) && value_expr_references_that_player(count) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::Mill {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                }],
            })
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } if is_each(*player)
            && (count
                .as_deref()
                .is_some_and(value_expr_references_that_player)
                || filter.as_deref().is_some_and(filter_references_that_player)) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::Discard {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    filter: filter.clone(),
                    anaphora: anaphora.clone(),
                    random: *random,
                }],
            })
        }
        InstructionAst::DiscardUpTo {
            player,
            max_count,
            filter,
            any_number,
            all,
            random,
        } if is_each(*player)
            && (value_expr_references_that_player(max_count)
                || filter.as_deref().is_some_and(filter_references_that_player)) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::DiscardUpTo {
                    player: PlayerRefAst::ThatPlayer,
                    max_count: max_count.clone(),
                    filter: filter.clone(),
                    any_number: *any_number,
                    all: *all,
                    random: *random,
                }],
            })
        }
        InstructionAst::CreateToken { controller, token }
            if is_each(*controller) && token_references_that_player(token) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *controller,
                body: vec![InstructionAst::CreateToken {
                    controller: PlayerRefAst::ThatPlayer,
                    token: token.clone(),
                }],
            })
        }
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half,
            rounding,
            chooser,
        } if is_each(*player)
            && (count
                .as_deref()
                .is_some_and(value_expr_references_that_player)
                || subject_references_that_player(subject)) =>
        {
            Some(InstructionAst::ForEachPlayer {
                player: *player,
                body: vec![InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    subject: subject.clone(),
                    except: except.clone(),
                    half: *half,
                    rounding: *rounding,
                    chooser: *chooser,
                }],
            })
        }
        _ => None,
    }
}

fn rewrite_single_target_player_to_that_player(
    ins: &InstructionAst,
) -> Option<(BodyTarget, InstructionAst)> {
    let bt_for = |p: PlayerRefAst| match p {
        PlayerRefAst::TargetPlayer => Some(BodyTarget::SinglePlayer),
        PlayerRefAst::TargetOpponent => Some(BodyTarget::SingleOpponent),
        _ => None,
    };
    match ins {
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            if let SubjectRefAst::Player(p) = target.as_ref()
                && let Some(bt) = bt_for(*p)
                && !subject_has_explicit_target(source)
            {
                Some((
                    bt,
                    InstructionAst::DealDamage {
                        source: source.clone(),
                        target: Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                        amount: amount.clone(),
                    },
                ))
            } else {
                None
            }
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::ChangeLife {
                    player: PlayerRefAst::ThatPlayer,
                    delta: delta.clone(),
                    gain: *gain,
                },
            )
        }),
        InstructionAst::SetLifeTotal { player, value } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::SetLifeTotal {
                    player: PlayerRefAst::ThatPlayer,
                    value: value.clone(),
                },
            )
        }),
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::Draw {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    up_to: *up_to,
                },
            )
        }),
        InstructionAst::Mill { player, count } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::Mill {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                },
            )
        }),
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::Discard {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    filter: filter.clone(),
                    anaphora: anaphora.clone(),
                    random: *random,
                },
            )
        }),
        InstructionAst::DiscardUpTo {
            player,
            max_count,
            filter,
            any_number,
            all,
            random,
        } => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::DiscardUpTo {
                    player: PlayerRefAst::ThatPlayer,
                    max_count: max_count.clone(),
                    filter: filter.clone(),
                    any_number: *any_number,
                    all: *all,
                    random: *random,
                },
            )
        }),
        InstructionAst::CreateToken { controller, token } => bt_for(*controller).map(|bt| {
            (
                bt,
                InstructionAst::CreateToken {
                    controller: PlayerRefAst::ThatPlayer,
                    token: token.clone(),
                },
            )
        }),
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
            except,
            half,
            rounding,
            chooser,
        } if !subject_has_explicit_target(subject) => bt_for(*player).map(|bt| {
            (
                bt,
                InstructionAst::Sacrifice {
                    player: PlayerRefAst::ThatPlayer,
                    count: count.clone(),
                    subject: subject.clone(),
                    except: except.clone(),
                    half: *half,
                    rounding: *rounding,
                    chooser: *chooser,
                },
            )
        }),
        _ => None,
    }
}

fn instruction_uses_that_player_as_primary(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { target, .. } => {
            matches!(
                target.as_ref(),
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
            )
        }
        InstructionAst::ChangeLife { player, .. }
        | InstructionAst::SetLifeTotal { player, .. }
        | InstructionAst::Draw { player, .. }
        | InstructionAst::Mill { player, .. }
        | InstructionAst::Discard { player, .. }
        | InstructionAst::DiscardHand { player }
        | InstructionAst::DiscardUpTo { player, .. }
        | InstructionAst::Sacrifice { player, .. } => *player == PlayerRefAst::ThatPlayer,
        InstructionAst::CreateToken { controller, .. } => *controller == PlayerRefAst::ThatPlayer,
        _ => false,
    }
}

fn try_rewrite_target_player_with_that_player(
    ins: &InstructionAst,
) -> Option<(BodyTarget, InstructionAst)> {
    match ins {
        InstructionAst::BindX { x_expr, body }
            if value_expr_references_that_player(x_expr) && !body.is_empty() =>
        {
            let (bt, rewritten_first) = rewrite_single_target_player_to_that_player(&body[0])?;
            // Require every trailing instruction in BindX to avoid introducing a new target
            // and to act on ThatPlayer rather than silently targeting You.
            if body[1..].iter().any(instruction_has_explicit_target)
                || !body[1..]
                    .iter()
                    .all(instruction_uses_that_player_as_primary)
            {
                return None;
            }
            let mut new_body = Vec::with_capacity(body.len());
            new_body.push(rewritten_first);
            new_body.extend_from_slice(&body[1..]);
            Some((
                bt,
                InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: new_body,
                },
            ))
        }
        other => {
            let refs_tp = match other {
                InstructionAst::DealDamage { amount, .. } => {
                    value_expr_references_that_player(amount)
                }
                InstructionAst::ChangeLife { delta, .. } => {
                    value_expr_references_that_player(delta)
                }
                InstructionAst::SetLifeTotal { value, .. } => {
                    value_expr_references_that_player(value)
                }
                InstructionAst::Draw { count, .. } | InstructionAst::Mill { count, .. } => {
                    value_expr_references_that_player(count)
                }
                InstructionAst::Discard { count, filter, .. } => {
                    count
                        .as_deref()
                        .is_some_and(value_expr_references_that_player)
                        || filter.as_deref().is_some_and(filter_references_that_player)
                }
                InstructionAst::DiscardUpTo {
                    max_count, filter, ..
                } => {
                    value_expr_references_that_player(max_count)
                        || filter.as_deref().is_some_and(filter_references_that_player)
                }
                InstructionAst::CreateToken { token, .. } => token_references_that_player(token),
                InstructionAst::Sacrifice { count, subject, .. } => {
                    count
                        .as_deref()
                        .is_some_and(value_expr_references_that_player)
                        || subject_references_that_player(subject)
                }
                _ => false,
            };
            if refs_tp {
                rewrite_single_target_player_to_that_player(other)
            } else {
                None
            }
        }
    }
}

fn try_extract_group_player_scope(ins: &InstructionAst) -> Option<PlayerRefAst> {
    let p = match ins {
        InstructionAst::DealDamage { target, .. } => match target.as_ref() {
            SubjectRefAst::Player(p) => *p,
            _ => return None,
        },
        InstructionAst::ChangeLife { player, .. }
        | InstructionAst::Draw { player, .. }
        | InstructionAst::Mill { player, .. }
        | InstructionAst::Discard { player, .. }
        | InstructionAst::DiscardHand { player } => *player,
        _ => return None,
    };
    matches!(p, PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer).then_some(p)
}

fn try_rewrite_followup_that_player(
    ins: &InstructionAst,
    scope: PlayerRefAst,
) -> Option<InstructionAst> {
    if instruction_has_explicit_target(ins) {
        return None;
    }
    match ins {
        InstructionAst::Discard {
            player: PlayerRefAst::ThatPlayer,
            count,
            filter,
            anaphora: None,
            random,
        } if !count
            .as_deref()
            .is_some_and(value_expr_references_that_player)
            && !filter.as_deref().is_some_and(filter_references_that_player) =>
        {
            Some(InstructionAst::Discard {
                player: scope,
                count: count.clone(),
                filter: filter.clone(),
                anaphora: None,
                random: *random,
            })
        }
        InstructionAst::DiscardHand {
            player: PlayerRefAst::ThatPlayer,
        } => Some(InstructionAst::DiscardHand { player: scope }),
        InstructionAst::Draw {
            player: PlayerRefAst::ThatPlayer,
            count,
            up_to,
        } if !value_expr_references_that_player(count) => Some(InstructionAst::Draw {
            player: scope,
            count: count.clone(),
            up_to: *up_to,
        }),
        InstructionAst::Mill {
            player: PlayerRefAst::ThatPlayer,
            count,
        } if !value_expr_references_that_player(count) => Some(InstructionAst::Mill {
            player: scope,
            count: count.clone(),
        }),
        InstructionAst::ChangeLife {
            player: PlayerRefAst::ThatPlayer,
            delta,
            gain,
        } if !value_expr_references_that_player(delta) => Some(InstructionAst::ChangeLife {
            player: scope,
            delta: delta.clone(),
            gain: *gain,
        }),
        _ => None,
    }
}

fn is_created_token_grant_subject(subj: &SubjectRefAst) -> bool {
    match subj {
        SubjectRefAst::Anaphora(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "it" | "them"
                | "that token"
                | "the token"
                | "those tokens"
                | "the tokens"
                | "that creature"
                | "the creature"
                | "those creatures"
                | "the creatures"
                | "that permanent"
                | "those permanents"
        ),
        // "They have \"...\"" parses "they" as PlayerRefAst::ThatPlayer in filter.rs.
        SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => true,
        // "The tokens have \"...\"" parses as a bare token-only Group/All/Each filter.
        SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f)
            if f.token == Some(true) =>
        {
            let mut bare = f.as_ref().clone();
            bare.token = None;
            bare.is_empty()
        }
        _ => false,
    }
}

fn append_quoted_abilities_to_token(token: &mut TokenSpecAst, abilities: &[String]) -> bool {
    let TokenSpecAst::Creature { keywords, .. } = token else {
        return false;
    };
    for ab in abilities {
        let trimmed = ab.trim();
        if !trimmed.is_empty() {
            keywords.push(format!("\"{trimmed}\""));
        }
    }
    true
}

/// Fuse a `CreateToken` (or `BindX` ending in `CreateToken`) with consecutive permanent
/// `GrantAbilityText` instructions referring to the just-created token(s) so those abilities
/// become part of the token definition (CR 111.3).
fn fuse_token_grant_ability_text_seq(ins: &[InstructionAst]) -> Option<Vec<InstructionAst>> {
    let mut changed = false;
    let mut out: Vec<InstructionAst> = Vec::with_capacity(ins.len());
    let mut idx = 0usize;
    while idx < ins.len() {
        let can_attach_token = match &ins[idx] {
            InstructionAst::CreateToken {
                token: TokenSpecAst::Creature { .. },
                ..
            } => true,
            InstructionAst::BindX { body, .. } => matches!(
                body.last(),
                Some(InstructionAst::CreateToken {
                    token: TokenSpecAst::Creature { .. },
                    ..
                })
            ),
            _ => false,
        };
        if can_attach_token {
            let mut quoted: Vec<String> = Vec::new();
            let mut scan = idx + 1;
            while scan < ins.len() {
                if let InstructionAst::GrantAbilityText {
                    subject,
                    ability_text,
                    duration: DurationAst::Permanent,
                    ..
                } = &ins[scan]
                    && is_created_token_grant_subject(subject)
                    && !ability_text.trim().is_empty()
                    && !ability_text.trim().eq_ignore_ascii_case("that ability")
                {
                    quoted.push(ability_text.clone());
                    scan += 1;
                } else {
                    break;
                }
            }
            if !quoted.is_empty() {
                let mut fused = ins[idx].clone();
                let attached = match &mut fused {
                    InstructionAst::CreateToken { token, .. } => {
                        append_quoted_abilities_to_token(token, &quoted)
                    }
                    InstructionAst::BindX { body, .. } => match body.last_mut() {
                        Some(InstructionAst::CreateToken { token, .. }) => {
                            append_quoted_abilities_to_token(token, &quoted)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if attached {
                    out.push(fused);
                    changed = true;
                    idx = scan;
                    continue;
                }
            }
        }
        out.push(ins[idx].clone());
        idx += 1;
    }
    changed.then_some(out)
}

fn is_your_zone(z: ZoneFilter) -> bool {
    matches!(
        z,
        ZoneFilter::YourHand
            | ZoneFilter::YourGraveyard
            | ZoneFilter::YourLibrary
            | ZoneFilter::YourGraveyardOrExile
            | ZoneFilter::YourHandOrGraveyard
            | ZoneFilter::YourLibraryOrGraveyard
            | ZoneFilter::YourGraveyardHandOrLibrary
    )
}

fn filter_references_you(f: &ObjectFilterAst) -> bool {
    matches!(
        f.controller,
        Some(ControllerRelation::You | ControllerRelation::OwnerYou | ControllerRelation::YourTeam)
    ) || matches!(
        f.owner,
        Some(ControllerRelation::You | ControllerRelation::OwnerYou | ControllerRelation::YourTeam)
    ) || f.zone.is_some_and(is_your_zone)
        || f.extra_zones.iter().copied().any(is_your_zone)
}

fn value_expr_references_you(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::Count(f)
        | ValueExprAst::AggregateStat { filter: f, .. }
        | ValueExprAst::ColorsAmong(f)
        | ValueExprAst::CardTypesAmong(f)
        | ValueExprAst::SubtypesAmong(f) => filter_references_you(f),
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::You,
            ..
        }
        | ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            ..
        }
        | ValueExprAst::StartingLifeTotal(PlayerRefAst::You)
        | ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::You,
            ..
        }
        | ValueExprAst::DomainCount
        | ValueExprAst::Devotion { .. }
        | ValueExprAst::PartySize
        | ValueExprAst::CardTypesInGraveyard {
            all_graveyards: false,
        } => true,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b) => {
            value_expr_references_you(a) || value_expr_references_you(b)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_you(expr),
        ValueExprAst::Pow { base, exp } => {
            value_expr_references_you(base) || value_expr_references_you(exp)
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            value_expr_references_you(threshold) || value_expr_references_you(actual)
        }
        _ => false,
    }
}

fn token_spec_references_you(tok: &TokenSpecAst) -> bool {
    match tok {
        TokenSpecAst::Predefined { count, .. } | TokenSpecAst::Creature { count, .. } => {
            value_expr_references_you(count)
        }
        TokenSpecAst::Copy { .. } | TokenSpecAst::Role { .. } => true,
    }
}

/// Strictly rewrite top-level actor `PlayerRefAst::You` to `target` for "You and <player> each <act>"
/// pairs, failing closed when nested filters or expressions depend on controller-relative `You`.
fn try_rewrite_actor_you_to_player(
    ins: &InstructionAst,
    target: PlayerRefAst,
) -> Option<InstructionAst> {
    match ins {
        InstructionAst::Draw {
            player: PlayerRefAst::You,
            count,
            up_to,
        } if !value_expr_references_you(count) => Some(InstructionAst::Draw {
            player: target,
            count: count.clone(),
            up_to: *up_to,
        }),
        InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token,
        } if !token_spec_references_you(token) => Some(InstructionAst::CreateToken {
            controller: target,
            token: token.clone(),
        }),
        InstructionAst::CreateTokenWithAbilities {
            controller: PlayerRefAst::You,
            token,
            compiled_abilities,
        } if !token_spec_references_you(token) => Some(InstructionAst::CreateTokenWithAbilities {
            controller: target,
            token: token.clone(),
            compiled_abilities: compiled_abilities.clone(),
        }),
        InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            delta,
            gain,
        } if !value_expr_references_you(delta) => Some(InstructionAst::ChangeLife {
            player: target,
            delta: delta.clone(),
            gain: *gain,
        }),
        InstructionAst::Discard {
            player: PlayerRefAst::You,
            count,
            filter,
            anaphora,
            random,
        } if count
            .as_deref()
            .is_none_or(|c| !value_expr_references_you(c))
            && filter.as_deref().is_none_or(|f| !filter_references_you(f)) =>
        {
            Some(InstructionAst::Discard {
                player: target,
                count: count.clone(),
                filter: filter.clone(),
                anaphora: anaphora.clone(),
                random: *random,
            })
        }
        InstructionAst::Mill {
            player: PlayerRefAst::You,
            count,
        } if !value_expr_references_you(count) => Some(InstructionAst::Mill {
            player: target,
            count: count.clone(),
        }),
        InstructionAst::Investigate {
            player: PlayerRefAst::You,
            count,
        } if !value_expr_references_you(count) => Some(InstructionAst::Investigate {
            player: target,
            count: count.clone(),
        }),
        InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            counter_kind,
            count,
        } if !value_expr_references_you(count) => Some(InstructionAst::GainPlayerCounters {
            player: target,
            counter_kind: counter_kind.clone(),
            count: count.clone(),
        }),
        InstructionAst::Scry {
            player: PlayerRefAst::You,
            count,
        } if !value_expr_references_you(count) => Some(InstructionAst::Scry {
            player: target,
            count: count.clone(),
        }),
        InstructionAst::Surveil {
            player: PlayerRefAst::You,
            count,
        } if !value_expr_references_you(count) => Some(InstructionAst::Surveil {
            player: target,
            count: count.clone(),
        }),
        _ => None,
    }
}

fn instruction_can_precede_if_prev_succeeded(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::Optional(_)
        | InstructionAst::OptionalPlayer { .. }
        | InstructionAst::Sacrifice { .. }
        | InstructionAst::Discard { .. }
        | InstructionAst::DiscardHand { .. }
        | InstructionAst::DiscardUpTo { .. }
        | InstructionAst::Exile { .. }
        | InstructionAst::ExileTopLibrary { .. }
        | InstructionAst::ExileGraveyard { .. }
        | InstructionAst::MoveZone { .. }
        | InstructionAst::ReturnZone { .. }
        | InstructionAst::MoveCounted { .. }
        | InstructionAst::Destroy { .. }
        | InstructionAst::PayCost(_)
        | InstructionAst::PlayerPayCost { .. }
        | InstructionAst::RemoveCounters { .. }
        | InstructionAst::SearchLibrary { .. }
        | InstructionAst::Clash { .. }
        | InstructionAst::CollectEvidence { .. }
        | InstructionAst::Forage => true,
        InstructionAst::BindX { body, .. } => body
            .last()
            .is_some_and(instruction_can_precede_if_prev_succeeded),
        _ => false,
    }
}

pub(in super::super) fn lower_instruction_seq(
    ins: &[InstructionAst],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    builder.has_source = in_trigger || self_pronoun_valid;
    let fused_token_grants = fuse_token_grant_ability_text_seq(ins);
    let ins: &[InstructionAst] = fused_token_grants.as_deref().unwrap_or(ins);
    let mut standalone_x: Option<&ValueExprAst> = None;
    let mut empty_bind_count = 0usize;
    if bound_x.is_none() {
        for item in ins {
            if let InstructionAst::BindX { x_expr, body } = item
                && body.is_empty()
            {
                standalone_x = Some(x_expr.as_ref());
                empty_bind_count += 1;
            }
        }
    }
    let bound_x = if bound_x.is_none() && empty_bind_count == 1 {
        standalone_x
    } else {
        bound_x
    };

    // Turn-long prevention installs a shield; it does not produce an immediate
    // damage amount. An adjacent dependent life change needs a prevention-event followup.
    // Do not scan across independent actions: they can establish a new PriorResult.
    if ins.windows(2).any(|pair| {
        matches!(
            &pair[0],
            InstructionAst::PreventAllDamageFrom {
                duration: DurationAst::UntilEndOfTurn,
                ..
            } | InstructionAst::PreventAllDamage {
                duration: DurationAst::UntilEndOfTurn,
                ..
            }
        ) && (match &pair[1] {
            InstructionAst::ChangeLife { delta, .. } => {
                value_expr_contains_prior_result(resolve_x(delta, bound_x))
            }
            InstructionAst::CreateToken { token, .. }
            | InstructionAst::CreateTokenWithAbilities { token, .. } => match token {
                TokenSpecAst::Predefined { count, .. }
                | TokenSpecAst::Copy { count, .. }
                | TokenSpecAst::Creature { count, .. } => {
                    value_expr_contains_prior_result(resolve_x(count, bound_x))
                }
                TokenSpecAst::Role { .. } => false,
            },
            InstructionAst::PutCounters { count, .. } | InstructionAst::Draw { count, .. } => {
                value_expr_contains_prior_result(resolve_x(count, bound_x))
            }
            InstructionAst::DealDamage { amount, .. } => {
                value_expr_contains_prior_result(resolve_x(amount, bound_x))
            }
            InstructionAst::ForEachCount { count, .. } => {
                value_expr_contains_prior_result(resolve_x(count, bound_x))
            }
            InstructionAst::IfPreviousSucceeded(_) | InstructionAst::IfPreviousFailed(_) => true,
            InstructionAst::IfCondition { condition, .. } => matches!(
                condition.as_ref(),
                ConditionAst::Marker(GameMarker::DamagePreventedThisWay)
            ),
            _ => false,
        })
    }) {
        return Err("oracle:unsupported-prevention-life-followup");
    }
    let mut i = 0usize;
    while i < ins.len() {
        if empty_bind_count == 1
            && matches!(&ins[i], InstructionAst::BindX { body, .. } if body.is_empty())
        {
            i += 1;
            continue;
        }
        // Fuse "You and <player> each <action>" pairs emitted by oracle_compiler
        // (e.g. Bloodroot Apothecary, Thran Spider, Xyris, Loran, Your Temple Is Under Attack).
        if i + 1 < ins.len()
            && let InstructionAst::ForEachPlayer {
                player: PlayerRefAst::You,
                body: body_you,
            } = &ins[i]
            && let InstructionAst::ForEachPlayer {
                player: other_player,
                body: body_other,
            } = &ins[i + 1]
            && *other_player != PlayerRefAst::You
            && !body_you.is_empty()
            && body_you == body_other
            && let Some(rewritten_other) = body_other
                .iter()
                .map(|op| try_rewrite_actor_you_to_player(op, *other_player))
                .collect::<Option<Vec<InstructionAst>>>()
        {
            let mut probe = builder.clone();
            let mut probe_ids = LoweringIds {
                next_activated: ids.next_activated,
                next_triggered: ids.next_triggered,
                next_mana: ids.next_mana,
                next_static: ids.next_static,
                next_ct_slot: ids.next_ct_slot,
            };
            if lower_instruction_seq(
                body_you,
                &mut probe,
                vocabulary,
                &mut probe_ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && lower_instruction_seq(
                    &rewritten_other,
                    &mut probe,
                    vocabulary,
                    &mut probe_ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
            {
                *builder = probe;
                *ids = probe_ids;
                i += 2;
                continue;
            }
        }
        // Fuse Counter(unless_pay: Some, unless_action: None) + IfPreviousSucceeded(paid_body)
        // (e.g. Divert Disaster, Assimilate Essence, Don't Make a Sound).
        if i + 1 < ins.len()
            && matches!(
                &ins[i],
                InstructionAst::Counter {
                    unless_pay: Some(_),
                    unless_action: None,
                    ..
                }
            )
            && let InstructionAst::IfPreviousSucceeded(paid_body) = &ins[i + 1]
            && !paid_body.is_empty()
        {
            let mut probe = builder.clone();
            let mut probe_ids = LoweringIds {
                next_activated: ids.next_activated,
                next_triggered: ids.next_triggered,
                next_mana: ids.next_mana,
                next_static: ids.next_static,
                next_ct_slot: ids.next_ct_slot,
            };
            let start_len = probe.instructions.len();
            if lower_instruction(
                &ins[i],
                &mut probe,
                vocabulary,
                &mut probe_ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && probe.instructions.len() >= start_len + 2
                && matches!(
                    probe.instructions.last(),
                    Some(Instruction::StackOp(spec))
                        if **spec
                            == spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Graveyard,
                            )
                )
            {
                let counter_idx = probe.instructions.len() - 1;
                let offer_idx = counter_idx - 1;
                let has_matching_offer = matches!(
                    probe.instructions.get(offer_idx),
                    Some(Instruction::OfferDynamicManaPayment {
                        paid_destination,
                        declined_destination,
                        ..
                    }) if *paid_destination == counter_idx + 1
                        && *declined_destination == counter_idx
                );
                if has_matching_offer {
                    let jump_idx = probe.instructions.len();
                    probe
                        .instructions
                        .push(Instruction::Jump { destination: 0 });
                    let paid_start = probe.instructions.len();
                    if let Some(Instruction::OfferDynamicManaPayment {
                        paid_destination, ..
                    }) = probe.instructions.get_mut(offer_idx)
                    {
                        *paid_destination = paid_start;
                    }
                    if lower_instruction_seq(
                        paid_body,
                        &mut probe,
                        vocabulary,
                        &mut probe_ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                        && probe.instructions.len() > paid_start
                    {
                        let end_dest = probe.instructions.len();
                        if let Some(Instruction::Jump { destination }) =
                            probe.instructions.get_mut(jump_idx)
                        {
                            *destination = end_dest;
                        }
                        *builder = probe;
                        *ids = probe_ids;
                        i += 2;
                        continue;
                    }
                }
            }
        }
        // Fuse Optional([Draw/ModifyPT]) (or BindX wrapping it) + IfPreviousSucceeded(follow_up)
        // into a single OptionalSequence (e.g. Force Away, Sanctum of Calm Waters,
        // Uncover the Moon-Letters, Bebop, Junkyo Bell).
        if i + 1 < ins.len()
            && let InstructionAst::IfPreviousSucceeded(follow_up) = &ins[i + 1]
            && !(i + 2 < ins.len()
                && matches!(
                    &ins[i + 2],
                    InstructionAst::Otherwise(_) | InstructionAst::IfPreviousFailed(_)
                ))
        {
            let opt_uncond: Option<(
                Option<&ValueExprAst>,
                &[InstructionAst],
                Option<&ValueExprAst>,
            )> = match &ins[i] {
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::Draw {
                            player: PlayerRefAst::You,
                            count,
                            up_to: false,
                        },
                    ] => Some((bound_x, inner.as_slice(), Some(count.as_ref()))),
                    [InstructionAst::ModifyPT { .. }] => Some((bound_x, inner.as_slice(), None)),
                    _ => None,
                },
                InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                    [InstructionAst::Optional(inner)] => match inner.as_slice() {
                        [
                            InstructionAst::Draw {
                                player: PlayerRefAst::You,
                                count,
                                up_to: false,
                            },
                        ] => Some((
                            Some(x_expr.as_ref()),
                            inner.as_slice(),
                            Some(count.as_ref()),
                        )),
                        [InstructionAst::ModifyPT { .. }] => {
                            Some((Some(x_expr.as_ref()), inner.as_slice(), None))
                        }
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            };
            if let Some((eff_x, opt_inner, draw_count_opt)) = opt_uncond {
                let mut probe = builder.clone();
                let mut probe_ids = LoweringIds {
                    next_activated: ids.next_activated,
                    next_triggered: ids.next_triggered,
                    next_mana: ids.next_mana,
                    next_static: ids.next_static,
                    next_ct_slot: ids.next_ct_slot,
                };
                let start = probe.instructions.len();
                if lower_instruction_seq(
                    opt_inner,
                    &mut probe,
                    vocabulary,
                    &mut probe_ids,
                    in_trigger,
                    self_pronoun_valid,
                    eff_x,
                )
                .is_ok()
                    && probe.instructions.len() > start
                {
                    let mut guard_idx: Option<usize> = None;
                    let mut guard_ok = true;
                    if let Some(draw_cnt) = draw_count_opt {
                        let resolved_cnt = resolve_x(draw_cnt, eff_x);
                        if !matches!(resolved_cnt, ValueExprAst::Const(n) if *n > 0) {
                            match lower_value_expr_in_context(
                                resolved_cnt,
                                &probe,
                                in_trigger,
                                vocabulary,
                            ) {
                                Ok(cnt_val) => {
                                    guard_idx = Some(probe.instructions.len());
                                    probe.instructions.push(Instruction::BranchIfFalse {
                                        condition: ResolutionCondition {
                                            left: cnt_val,
                                            comparison: Comparison::GreaterOrEqual,
                                            right: ValueExpr::literal(1),
                                        },
                                        destination: 0,
                                    });
                                }
                                Err(_) => {
                                    guard_ok = false;
                                }
                            }
                        }
                    }
                    if guard_ok
                        && lower_instruction_seq(
                            follow_up,
                            &mut probe,
                            vocabulary,
                            &mut probe_ids,
                            in_trigger,
                            self_pronoun_valid,
                            eff_x,
                        )
                        .is_ok()
                    {
                        let end_dest = probe.instructions.len();
                        if let Some(g_idx) = guard_idx
                            && let Some(Instruction::BranchIfFalse { destination, .. }) =
                                probe.instructions.get_mut(g_idx)
                        {
                            *destination = end_dest;
                        }
                        let span = end_dest.saturating_sub(start);
                        if span >= 2 {
                            prepend_instruction_at(
                                &mut probe.instructions,
                                start,
                                Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::Controller,
                                    instructions: span,
                                },
                            );
                            *builder = probe;
                            *ids = probe_ids;
                            i += 2;
                            continue;
                        }
                    }
                }
            }
        }
        // Unwrap Optional([IfCondition { .. }]) and OptionalPlayer([IfCondition { .. }]) directly
        // inside lower_instruction_seq so IfCondition lowers via control_flow rather than failing
        // in lower_instruction (e.g. Deathbringer Liege, Sage-Eye Avengers).
        if let InstructionAst::Optional(inner) = &ins[i]
            && let [
                InstructionAst::IfCondition {
                    condition,
                    then_body,
                },
            ] = inner.as_slice()
        {
            let rewritten = [InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: vec![InstructionAst::Optional(then_body.clone())],
            }];
            lower_instruction_seq(
                &rewritten,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 1;
            continue;
        }
        if let InstructionAst::OptionalPlayer { player, body } = &ins[i]
            && let [
                InstructionAst::IfCondition {
                    condition,
                    then_body,
                },
            ] = body.as_slice()
        {
            let rewritten = [InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: vec![InstructionAst::OptionalPlayer {
                    player: *player,
                    body: then_body.clone(),
                }],
            }];
            lower_instruction_seq(
                &rewritten,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 1;
            continue;
        }
        if let Some(consumed) = try_lower_preserving_type_animation(
            &ins[i..],
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )? {
            i += consumed;
            continue;
        }
        // Fuse `Target player sacrifices a creature, then gains life equal to that creature's ...`
        // (e.g. Devour Flesh) so both clauses share the single player target rather than splitting.
        if i + 1 < ins.len()
            && let InstructionAst::Sacrifice {
                player: sac_player @ (PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent),
                ..
            } = &ins[i]
            && let InstructionAst::ChangeLife {
                player: life_player,
                delta,
                gain,
            } = &ins[i + 1]
            && life_player == sac_player
            && matches!(
                delta.as_ref(),
                ValueExprAst::ObjectStat {
                    subject,
                    ..
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            )
        {
            lower_instruction(
                &ins[i],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let rewritten = InstructionAst::ChangeLife {
                player: PlayerRefAst::ThatPlayer,
                delta: delta.clone(),
                gain: *gain,
            };
            lower_instruction(
                &rewritten,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 2;
            continue;
        }
        // Fuse soulbond partner boost: `Target creature gets +P/+T until end of turn.
        // If it's paired with a creature, that creature also gets +P/+T until end of turn.` (Joint Assault)
        if i + 1 < ins.len()
            && let InstructionAst::ModifyPT {
                subject: subj1,
                dx: dx1,
                dy: dy1,
                duration: dur1,
                scale: None,
            } = &ins[i]
            && let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subj1.as_ref()
            && filter.zone.is_none()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && matches!(
                condition.as_ref(),
                ConditionAst::Marker(GameMarker::PairedWithAnotherCreature)
            )
            && let [
                InstructionAst::ModifyPT {
                    subject: subj2,
                    dx: dx2,
                    dy: dy2,
                    duration: dur2,
                    scale: None,
                },
            ] = then_body.as_slice()
            && matches!(subj2.as_ref(), SubjectRefAst::Anaphora(_))
            && dx1 == dx2
            && dy1 == dy2
            && dur1 == dur2
            && let (Ok(p), Ok(t), Ok(dur)) = (
                dx1.trim().parse::<i64>(),
                dy1.trim().parse::<i64>(),
                lower_duration(dur1),
            )
        {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 1,
                    maximum: 1,
                    action: PermanentTargetAction::ModifyWithPartner {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(
                            PowerToughness {
                                power: p.into(),
                                toughness: t.into(),
                            },
                        )]),
                        duration: dur,
                    },
                });
            i += 2;
            continue;
        }
        // Fuse [Counter, IfCondition("that spell is countered this way", [ReplaceEffectWith(...)])]
        // into CounterSpellToExile / CounterSpellToLibrary / ChooseOption(top vs bottom)
        // (Dissipate, Spell Crumple, Hinder).
        if i + 1 < ins.len()
            && let Some(override_dest) =
                extract_counter_destination_replacement(&ins[i], &ins[i + 1])
        {
            let mut probe = builder.clone();
            lower_instruction(
                &ins[i],
                &mut probe,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if let Some(last) = probe.instructions.last_mut()
                && matches!(
                    &*last,
                    Instruction::StackOp(spec)
                        if **spec
                            == spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Graveyard,
                            )
                )
            {
                *last = match override_dest {
                    CounterSpellDestOverride::Exile => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Exile,
                        ),
                    )),
                    CounterSpellDestOverride::LibraryTop => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Library { top: true },
                        ),
                    )),
                    CounterSpellDestOverride::LibraryBottom => Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Library { top: false },
                        ),
                    )),
                    CounterSpellDestOverride::ChoiceLibraryTopOrBottom => {
                        Instruction::ChooseOption {
                            chooser: ResolvingPlayer::Controller,
                            options: vec![
                                ChooseOptionBranch {
                                    label: "Top of owner's library".to_owned(),
                                    body: ExecutableBody::new(
                                        BodyTarget::None,
                                        vec![Instruction::StackOp(Box::new(
                                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                                spellwake_engine::toolbox::CounterDestination::Library {
                                                    top: true,
                                                },
                                            ),
                                        ))],
                                    ),
                                },
                                ChooseOptionBranch {
                                    label: "Bottom of owner's library".to_owned(),
                                    body: ExecutableBody::new(
                                        BodyTarget::None,
                                        vec![Instruction::StackOp(Box::new(
                                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                                spellwake_engine::toolbox::CounterDestination::Library {
                                                    top: false,
                                                },
                                            ),
                                        ))],
                                    ),
                                },
                            ],
                        }
                    }
                };
                *builder = probe;
                i += 2;
                continue;
            }
        }
        // Desugar [base_slice, <replace_carrier>] (+ optional Otherwise)
        // into BranchIfFalse(cond -> base_start) + [OptionalSequence] + alt_ops + Jump(end) + base_ops (+ else_ops).
        // Handles IfPreviousSucceeded/IfPreviousFailed, IfCondition (including Optional/OptionalPlayer([ReplaceEffect(...)])),
        // standalone Optional/OptionalPlayer([ReplaceEffect(...)]), and raw ReplaceEffect(alt) at ins[i + base_len].
        let base_len = base_pair_len(ins, i);
        if i + base_len < ins.len()
            && let Some((prev_cond, opt_chooser, raw_alt)) = match &ins[i + base_len] {
                InstructionAst::IfPreviousSucceeded(tb) if contains_replace_effect(tb) => {
                    Some((true, None, tb.as_slice()))
                }
                InstructionAst::IfPreviousSucceeded(tb) if is_optional_replace_effect(tb) => {
                    match tb.as_slice() {
                        [InstructionAst::Optional(inner)] => {
                            Some((true, Some(PlayerRefAst::You), inner.as_slice()))
                        }
                        [
                            InstructionAst::OptionalPlayer {
                                player,
                                body: inner,
                            },
                        ] => Some((true, Some(*player), inner.as_slice())),
                        _ => None,
                    }
                }
                InstructionAst::IfPreviousFailed(tb) if contains_replace_effect(tb) => {
                    Some((false, None, tb.as_slice()))
                }
                InstructionAst::IfPreviousFailed(tb) if is_optional_replace_effect(tb) => {
                    match tb.as_slice() {
                        [InstructionAst::Optional(inner)] => {
                            Some((false, Some(PlayerRefAst::You), inner.as_slice()))
                        }
                        [
                            InstructionAst::OptionalPlayer {
                                player,
                                body: inner,
                            },
                        ] => Some((false, Some(*player), inner.as_slice())),
                        _ => None,
                    }
                }
                _ => None,
            }
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                raw_alt,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if opt_chooser.is_some() && pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            builder.up_to_x_target_count |= pair.up_to_x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let resolved_chooser = match opt_chooser {
                Some(PlayerRefAst::You) => Some(ResolvingPlayer::Controller),
                Some(p) => Some(lower_resolving_player_in_context(p, builder, in_trigger)?),
                None => None,
            };
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: if prev_cond {
                        Comparison::GreaterOrEqual
                    } else {
                        Comparison::Equal
                    },
                    right: ValueExpr::literal(if prev_cond { 1 } else { 0 }),
                },
                destination: 0,
            });
            if let Some(chooser) = resolved_chooser {
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser,
                    instructions: pair.alt_ops.len() + 1,
                });
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(branch_idx)
            {
                *destination = base_start;
            }
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += base_len + 1;
            continue;
        }
        if i + base_len < ins.len()
            && let Some((condition, then_body, cond_bound_x)) = match &ins[i + base_len] {
                InstructionAst::IfCondition {
                    condition,
                    then_body,
                } if contains_replace_effect(then_body)
                    || is_optional_replace_effect(then_body) =>
                {
                    Some((condition, then_body, bound_x))
                }
                InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                    [
                        InstructionAst::IfCondition {
                            condition,
                            then_body,
                        },
                    ] if contains_replace_effect(then_body)
                        || is_optional_replace_effect(then_body) =>
                    {
                        Some((condition, then_body, Some(x_expr.as_ref())))
                    }
                    _ => None,
                },
                _ => None,
            }
        {
            let base_slice = &ins[i..i + base_len];
            let (opt_chooser, raw_alt) = match then_body.as_slice() {
                [InstructionAst::Optional(opt_inner)] if contains_replace_effect(opt_inner) => {
                    (Some(PlayerRefAst::You), opt_inner.as_slice())
                }
                [
                    InstructionAst::OptionalPlayer {
                        player,
                        body: opt_inner,
                    },
                ] if contains_replace_effect(opt_inner) => (Some(*player), opt_inner.as_slice()),
                _ => (None, then_body.as_slice()),
            };
            let pair = lower_replace_pair(
                base_slice,
                raw_alt,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                cond_bound_x,
            )?;
            if opt_chooser.is_some() && pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }

            let mut cond_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
            cond_builder.has_source = builder.has_source;
            cond_builder.target = pair.merged_target;
            let effective_cond = (!in_trigger && !self_pronoun_valid)
                .then(|| rewrite_spell_self_has_status_to_anaphora(condition));
            let cond_ref = effective_cond.as_ref().unwrap_or(condition);
            let false_branches = emit_condition_branches(
                cond_ref,
                &mut cond_builder,
                vocabulary,
                in_trigger,
                cond_bound_x,
            )?;
            if cond_builder.target != pair.merged_target {
                return Err("oracle:replace-effect-cond-target-mismatch");
            }

            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            builder.up_to_x_target_count |= pair.up_to_x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let resolved_chooser = match opt_chooser {
                Some(PlayerRefAst::You) => Some(ResolvingPlayer::Controller),
                Some(p) => Some(lower_resolving_player_in_context(p, builder, in_trigger)?),
                None => None,
            };

            let cond_start = builder.instructions.len();
            let cond_shift = isize::try_from(cond_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in cond_builder.instructions {
                op.shift_jumps(cond_shift, 0);
                builder.instructions.push(op);
            }
            if let Some(chooser) = resolved_chooser {
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser,
                    instructions: pair.alt_ops.len() + 1,
                });
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            for rel_idx in false_branches {
                if let Some(
                    Instruction::BranchIfFalse { destination, .. }
                    | Instruction::BranchCondition { destination, .. },
                ) = builder.instructions.get_mut(cond_start + rel_idx)
                {
                    *destination = base_start;
                }
            }
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let mut step = base_len + 1;
            if i + step < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + step]
            {
                let unwrapped_else: Vec<InstructionAst>;
                let else_slice: &[InstructionAst] = if contains_replace_effect(else_body) {
                    unwrapped_else =
                        substitute_those_tokens(&strip_replace_effect(else_body), base_slice);
                    &unwrapped_else
                } else {
                    else_body
                };
                lower_instruction_seq(
                    else_slice,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                step += 1;
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += step;
            continue;
        }
        if i + base_len < ins.len()
            && let Some((opt_player, opt_inner)) = match &ins[i + base_len] {
                InstructionAst::Optional(inner) if contains_replace_effect(inner) => {
                    Some((PlayerRefAst::You, inner.as_slice()))
                }
                InstructionAst::OptionalPlayer { player, body }
                    if contains_replace_effect(body) =>
                {
                    Some((*player, body.as_slice()))
                }
                _ => None,
            }
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                opt_inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            builder.up_to_x_target_count |= pair.up_to_x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let chooser = if opt_player == PlayerRefAst::You {
                ResolvingPlayer::Controller
            } else {
                lower_resolving_player_in_context(opt_player, builder, in_trigger)?
            };
            builder.instructions.push(Instruction::OptionalSequence {
                chooser,
                instructions: pair.alt_ops.len() + 1,
            });
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += base_len + 1;
            continue;
        }
        if i + base_len < ins.len()
            && let Some(alt_inner) = match &ins[i + base_len] {
                InstructionAst::ReplaceEffect(inner) => Some(inner.as_slice()),
                InstructionAst::ReplaceEffectWith(rew) if !rew.only && rew.instead_of.is_none() => {
                    Some(rew.body.as_slice())
                }
                _ => None,
            }
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                alt_inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            builder.up_to_x_target_count |= pair.up_to_x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            i += base_len + 1;
            continue;
        }
        // Fuse OneShotReplacement peepholes:
        // 1. [Counter, OneShotReplacement(countered -> exile/top/bottom)] -> CounterSpellToExile / CounterSpellToLibrary (Deny the Divine, Memory Lapse, Lapse of Certainty)
        // 2. [DealDamage(self -> single/any target), OneShotReplacement(die this turn -> exile)] -> DamageAmountThenExileIfDies (Yamabushi's Flame)
        if i + 1 < ins.len()
            && let InstructionAst::OneShotReplacement {
                event,
                replace_with,
                instead_body,
                ..
            } = &ins[i + 1]
        {
            let ev_low = event.trim().to_ascii_lowercase();
            let rep_low = replace_with.trim().to_ascii_lowercase();
            let is_top_lib =
                rep_low == "top_of_library" || rep_low.contains("top of its owner's library");
            let is_bottom_lib =
                rep_low == "bottom_of_library" || rep_low.contains("bottom of its owner's library");
            if ev_low.contains("counter")
                && matches!(&ins[i], InstructionAst::Counter { .. })
                && !rep_low.contains(" and ")
                && instead_body.as_ref().is_none_or(|b| b.len() <= 1)
                && (rep_low.contains("exile") || is_top_lib || is_bottom_lib)
            {
                lower_instruction(
                    &ins[i],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if let Some(last) = builder.instructions.last_mut()
                    && matches!(
                        &*last,
                        Instruction::StackOp(spec)
                            if **spec
                                == spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                                )
                    )
                {
                    if rep_low.contains("exile") {
                        *last = Instruction::StackOp(Box::new(
                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Exile,
                            ),
                        ));
                    } else if is_top_lib {
                        *last = Instruction::StackOp(Box::new(
                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Library {
                                    top: true,
                                },
                            ),
                        ));
                    } else {
                        *last = Instruction::StackOp(Box::new(
                            spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                spellwake_engine::toolbox::CounterDestination::Library {
                                    top: false,
                                },
                            ),
                        ));
                    }
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if (ev_low.contains("die") || ev_low.contains("graveyard"))
                && rep_low.contains("exile")
                && !rep_low.contains(" and ")
                && instead_body
                    .as_ref()
                    .is_none_or(|b| matches!(b.as_slice(), [InstructionAst::Exile { .. }]))
                && let InstructionAst::DealDamage {
                    source,
                    target,
                    amount,
                } = &ins[i]
                && is_self_pronoun(source, builder, self_pronoun_valid)
            {
                let prev_len = builder.instructions.len();
                let mut probe = builder.clone();
                if lower_instruction(
                    &InstructionAst::DealDamage {
                        source: source.clone(),
                        target: target.clone(),
                        amount: amount.clone(),
                    },
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.instructions.len() == prev_len + 1
                {
                    let upgraded = match probe.instructions.last() {
                        Some(Instruction::DealDamage(spec))
                            if !spec.exile_if_dies
                                && matches!(
                                    spec.source,
                                    spellwake_engine::toolbox::DamageSourceQuery::EntrySource
                                )
                                && matches!(
                                    spec.recipients,
                                    spellwake_engine::toolbox::DamageTargetQuery::Targets
                                ) =>
                        {
                            let mut next = (**spec).clone();
                            next.exile_if_dies = true;
                            Some(Instruction::DealDamage(Box::new(next)))
                        }
                        _ => None,
                    };
                    if let Some(op) = upgraded {
                        *builder = probe;
                        if let Some(last) = builder.instructions.last_mut() {
                            *last = op;
                        }
                        i += 2;
                        continue;
                    }
                }
            }
        }
        // Fuse paired ModifyPT + GrantKeywords on the exact same subject & duration
        // (emitted by clauses.rs for "Target creature gets +1/+1 and gains flying until end of turn").
        if i + 1 < ins.len()
            && let InstructionAst::ModifyPT {
                subject: s1,
                duration: d1,
                ..
            } = &ins[i]
            && let InstructionAst::GrantKeywords {
                subject: s2,
                keywords,
                duration: d2,
            } = &ins[i + 1]
            && s1 == s2
            && d1 == d2
        {
            lower_instruction(
                &ins[i],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let ab_change = lower_ability_change(keywords, vocabulary)?;
            let pure_kws = lower_keyword_list(keywords).ok();
            match builder.instructions.last_mut() {
                Some(Instruction::ApplyContinuous(spec)) => match &mut spec.payload {
                    spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                        keywords: prev_kws,
                        ..
                    } if pure_kws.is_some() => {
                        if let Some(kws) = pure_kws {
                            prev_kws.extend(kws);
                        }
                        i += 2;
                        continue;
                    }
                    spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts)
                    | spellwake_engine::toolbox::ContinuousPayload::ModifyAnyNumber { parts }
                        if !parts
                            .0
                            .iter()
                            .any(|p| matches!(p, ResolvedPartSpec::Abilities(_))) =>
                    {
                        parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                        i += 2;
                        continue;
                    }
                    _ => {}
                },
                Some(Instruction::AffectPermanentTargets {
                    action: PermanentTargetAction::Modify { parts, .. },
                    ..
                }) if !parts
                    .0
                    .iter()
                    .any(|p| matches!(p, ResolvedPartSpec::Abilities(_))) =>
                {
                    parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                    i += 2;
                    continue;
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        // Unified SearchLibrary fusion: Optional / OptionalPlayer / standalone SearchLibrary +
        // reveal / MoveZone / ReturnZone / Exile / ShuffleLibrary / IfCondition(SearchedLibraryThisWay) / IfPreviousSucceeded.
        if let Some(next_i) = try_fuse_search_library(
            ins,
            i,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ) {
            i = next_i;
            continue;
        }
        // Fuse ChooseOption(<keyword list>) + GrantAbilityText("that ability") (e.g. Angelic Skirmisher).
        if let Some(next_i) = try_fuse_choose_keyword_grant(
            ins,
            i,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ) {
            i = next_i;
            continue;
        }
        // Fuse Exile(target) + DelayedTrigger(EndStep, ReturnZone(Anaphora, battlefield)) -> ExileUntilNextEndStep
        if i + 1 < ins.len()
            && let InstructionAst::Exile {
                actor: None | Some(PlayerRefAst::You),
                subject,
                random: false,
                face_down: false,
                into_pile: false,
            } = &ins[i]
            && let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
            && filter.zone.is_none()
            && let InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                scope: DelayedStepScope::Next,
                body: delayed_body,
            } = &ins[i + 1]
            && let [
                InstructionAst::ReturnZone {
                    subject: ret_subj,
                    to,
                    tapped: false,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    ..
                },
            ] = delayed_body.as_slice()
            && matches!(ret_subj.as_ref(), SubjectRefAst::Anaphora(_))
            && matches!(
                to.trim().to_ascii_lowercase().as_str(),
                "battlefield" | "the battlefield" | "the battlefield under its owner's control"
            )
        {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_until_next_end_step(
                ),
            );
            i += 2;
            continue;
        }
        // Fuse FlipCoin + IfCondition(WonContest) (+ optional Otherwise) -> FlipCoin { lose_destination }
        if i + 1 < ins.len()
            && matches!(&ins[i], InstructionAst::FlipCoin)
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && matches!(condition.as_ref(), ConditionAst::WonContest)
        {
            let flip_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::flip_coins(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::CoinFlipMode::Single,
                        None,
                        None,
                        Some(0),
                    ),
                )));
            lower_instruction_seq(
                then_body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if i + 2 < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
            {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::MutatePlayer(spec)) =
                    builder.instructions.get_mut(flip_idx)
                    && let spellwake_engine::toolbox::PlayerMutation::FlipCoins {
                        lose_destination,
                        ..
                    } = &mut spec.op
                {
                    *lose_destination = Some(lose_dest);
                }
                lower_instruction_seq(
                    else_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 3;
            } else {
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::MutatePlayer(spec)) =
                    builder.instructions.get_mut(flip_idx)
                    && let spellwake_engine::toolbox::PlayerMutation::FlipCoins {
                        lose_destination,
                        ..
                    } = &mut spec.op
                {
                    *lose_destination = Some(lose_dest);
                }
                i += 2;
            }
            continue;
        }
        // Pull a trailing extreme-life tie-breaker sentence into an enclosing IfCondition whose
        // then_body ends with ExtremeLifePlayerAction (e.g. Celestial Convergence).
        if i + 1 < ins.len()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i]
            && let Some(InstructionAst::ExtremeLifePlayerAction { highest, .. }) = then_body.last()
            && parse_extreme_life_tie_mode(&ins[i + 1], *highest).is_some()
        {
            let mut merged_then = Vec::with_capacity(then_body.len() + 1);
            merged_then.extend_from_slice(then_body);
            merged_then.push(ins[i + 1].clone());
            let synthetic = InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: merged_then,
            };
            lower_instruction_seq(
                std::slice::from_ref(&synthetic),
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 2;
            continue;
        }
        // Lower IfCondition(MatchesFilter(Anaphora, f), then_body) (+ optional Otherwise)
        // directly following an already-emitted LookTopChoose { reveal_only: true }.
        if let Some(&Instruction::LookTopChooseExt {
            reveal_only: true,
            library_owner,
            ..
        }) = builder.instructions.last()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i]
            && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        {
            let mut inner_f = filter.as_ref().clone();
            inner_f.zone = None;
            if let Ok(obj_filter) = lower_object_filter(&inner_f, vocabulary) {
                let else_opt = if i + 1 < ins.len()
                    && let InstructionAst::Otherwise(eb) = &ins[i + 1]
                {
                    Some(eb.as_slice())
                } else {
                    None
                };
                let then_b = lower_revealed_top_branch(
                    then_body,
                    builder,
                    library_owner,
                    true,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
                let else_b = match else_opt {
                    Some(eb) => lower_revealed_top_branch(
                        eb,
                        builder,
                        library_owner,
                        true,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .map(Some),
                    None => Ok(None),
                };
                if let (Ok(then_b), Ok(else_b_opt)) = (then_b, else_b)
                    && (then_b.target == BodyTarget::None || then_b.target == builder.target)
                    && else_b_opt.as_ref().is_none_or(|eb| {
                        eb.target == BodyTarget::None || eb.target == builder.target
                    })
                {
                    let branch_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::object_matches(
                                MatchObjectRef::RevealedTop,
                                obj_filter,
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        },
                        destination: 0,
                    });
                    let then_shift = isize::try_from(builder.instructions.len())
                        .map_err(|_| "oracle:jump-shift-ovf")?;
                    for mut op in then_b.instructions {
                        op.shift_jumps(then_shift, 0);
                        builder.instructions.push(op);
                    }
                    if let Some(else_b) = else_b_opt {
                        let jump_idx = builder.instructions.len();
                        builder
                            .instructions
                            .push(Instruction::Jump { destination: 0 });
                        let else_start = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = else_start;
                        }
                        let else_shift =
                            isize::try_from(else_start).map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in else_b.instructions {
                            op.shift_jumps(else_shift, 0);
                            builder.instructions.push(op);
                        }
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::Jump { destination }) =
                            builder.instructions.get_mut(jump_idx)
                        {
                            *destination = end_dest;
                        }
                    } else {
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = end_dest;
                        }
                    }
                    i += if else_opt.is_some() { 2 } else { 1 };
                    continue;
                }
            }
        }
        // Lower IfCondition (+ optional Otherwise) via BranchIfFalse + Jump (including And/Or).
        // When the condition references target anaphora (e.g. Fatal Push / Overload:
        // "Destroy target creature if it has mana value 2 or less") and no target is bound yet,
        // probe then_body first to bind the target before lowering the condition.
        // Hoist `IfCondition { cond, then_body }` + `IfPreviousSucceeded(follow_up)` when
        // `then_body` ends in an optional or tracked action (e.g. Force Away, Chandra's Defeat,
        // Take Out the Trash, Archfiend's Vessel) so `follow_up` runs only when both `cond`
        // and the action succeeded.
        if i + 1 < ins.len()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i]
            && matches!(&ins[i + 1], InstructionAst::IfPreviousSucceeded(_))
            && then_body
                .last()
                .is_some_and(instruction_can_precede_if_prev_succeeded)
        {
            let mut combined = Vec::with_capacity(then_body.len() + 1);
            combined.extend_from_slice(then_body);
            combined.push(ins[i + 1].clone());
            let hoisted = [InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: combined,
            }];
            lower_instruction_seq(
                &hoisted,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 2;
            continue;
        }
        if let InstructionAst::IfCondition {
            condition,
            then_body,
        } = &ins[i]
        {
            let effective_cond = (!in_trigger && !self_pronoun_valid)
                .then(|| rewrite_spell_self_has_status_to_anaphora(condition));
            let cond_ref = effective_cond.as_ref().unwrap_or(condition);
            let prev_len = builder.instructions.len();
            let prev_target = builder.target;
            let prev_has_fixed = builder
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
            let prev_gy = builder.graveyard_target_bounds;
            let prev_x = builder.x_target_count;
            let prev_up_to_x = builder.up_to_x_target_count;
            let prev_group = builder.last_group_filter;
            let (false_branches, lowered_then_already) =
                match emit_condition_branches(cond_ref, builder, vocabulary, in_trigger, bound_x) {
                    Ok(fb) => (fb, false),
                    Err(cond_err) if prev_target == BodyTarget::None && !prev_has_fixed => {
                        builder.instructions.truncate(prev_len);
                        builder.target = prev_target;
                        builder.graveyard_target_bounds = prev_gy;
                        builder.x_target_count = prev_x;
                        builder.up_to_x_target_count = prev_up_to_x;
                        builder.last_group_filter = prev_group;
                        let mut then_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                        then_builder.has_source = builder.has_source;
                        then_builder.last_group_filter = prev_group;
                        lower_instruction_seq(
                            then_body,
                            &mut then_builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        if then_builder.target == BodyTarget::None {
                            return Err(cond_err);
                        }
                        builder.bind_target(then_builder.target)?;
                        let fb = emit_condition_branches(
                            cond_ref, builder, vocabulary, in_trigger, bound_x,
                        )?;
                        if builder.target != then_builder.target {
                            return Err("oracle:conflicting-targets");
                        }
                        if builder.graveyard_target_bounds.is_none() {
                            builder.graveyard_target_bounds = then_builder.graveyard_target_bounds;
                        }
                        builder.x_target_count |= then_builder.x_target_count;
                        builder.up_to_x_target_count |= then_builder.up_to_x_target_count;
                        if then_builder.last_group_filter.is_some() {
                            builder.last_group_filter = then_builder.last_group_filter;
                        }
                        let then_start = builder.instructions.len();
                        let then_shift =
                            isize::try_from(then_start).map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in then_builder.instructions {
                            op.shift_jumps(then_shift, 0);
                            builder.instructions.push(op);
                        }
                        (fb, true)
                    }
                    Err(cond_err) => return Err(cond_err),
                };
            if !lowered_then_already {
                lower_instruction_seq(
                    then_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
            }
            let else_branch_opt: Option<&[InstructionAst]> = if i + 1 < ins.len() {
                match &ins[i + 1] {
                    InstructionAst::Otherwise(else_body) => Some(else_body.as_slice()),
                    InstructionAst::IfPreviousFailed(else_body)
                        if !then_body
                            .last()
                            .is_some_and(instruction_can_precede_if_prev_succeeded) =>
                    {
                        Some(else_body.as_slice())
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(else_body) = else_branch_opt {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let else_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(
                        Instruction::BranchIfFalse { destination, .. }
                        | Instruction::BranchCondition { destination, .. },
                    ) = builder.instructions.get_mut(b_idx)
                    {
                        *destination = else_dest;
                    }
                }
                let fused_then = fuse_token_grant_ability_text_seq(then_body);
                let then_for_tokens: &[InstructionAst] = fused_then.as_deref().unwrap_or(then_body);
                let unwrapped_else = if contains_replace_effect(else_body) {
                    substitute_those_tokens(&strip_replace_effect(else_body), then_for_tokens)
                } else {
                    substitute_those_tokens(else_body, then_for_tokens)
                };
                let else_slice: &[InstructionAst] = &unwrapped_else;
                lower_instruction_seq(
                    else_slice,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 2;
            } else {
                let end_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(
                        Instruction::BranchIfFalse { destination, .. }
                        | Instruction::BranchCondition { destination, .. },
                    ) = builder.instructions.get_mut(b_idx)
                    {
                        *destination = end_dest;
                    }
                }
                i += 1;
            }
            continue;
        }
        // Fuse OptionalPlayer { AnyPlayer | AnyOpponent, cost } + paid / declined follow-ups into
        // Instruction::OfferAnyPlayer (must precede opt_mana_head so AnyPlayer/AnyOpponent mana offers
        // are not rejected by single-player opt_mana_head).
        if let InstructionAst::OptionalPlayer {
            player: PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent,
            ..
        } = &ins[i]
            && i + 1 < ins.len()
        {
            type OfferFollow<'a> = (
                Option<&'a [InstructionAst]>,
                Option<&'a [InstructionAst]>,
                usize,
            );
            let offer_follow: Option<OfferFollow<'_>> =
                match &ins[i + 1] {
                    InstructionAst::IfPreviousSucceeded(paid) => {
                        let dec_opt = if i + 2 < ins.len() {
                            match &ins[i + 2] {
                                InstructionAst::Otherwise(d)
                                | InstructionAst::IfPreviousFailed(d) => Some(d.as_slice()),
                                InstructionAst::IfCondition {
                                    condition,
                                    then_body,
                                } if is_offer_any_player_declined_cond(condition) => {
                                    Some(then_body.as_slice())
                                }
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let adv = if dec_opt.is_some() { 3 } else { 2 };
                        Some((Some(paid.as_slice()), dec_opt, adv))
                    }
                    InstructionAst::IfCondition {
                        condition,
                        then_body,
                    } if is_offer_any_player_paid_cond(condition) => {
                        let dec_opt = if i + 2 < ins.len() {
                            match &ins[i + 2] {
                                InstructionAst::Otherwise(d)
                                | InstructionAst::IfPreviousFailed(d) => Some(d.as_slice()),
                                InstructionAst::IfCondition {
                                    condition: c2,
                                    then_body: d2,
                                } if is_offer_any_player_declined_cond(c2) => Some(d2.as_slice()),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let adv = if dec_opt.is_some() { 3 } else { 2 };
                        Some((Some(then_body.as_slice()), dec_opt, adv))
                    }
                    InstructionAst::Otherwise(dec) | InstructionAst::IfPreviousFailed(dec) => {
                        Some((None, Some(dec.as_slice()), 2))
                    }
                    InstructionAst::IfCondition {
                        condition,
                        then_body,
                    } if is_offer_any_player_declined_cond(condition) => {
                        Some((None, Some(then_body.as_slice()), 2))
                    }
                    _ => None,
                };
            if let Some((paid_slice, declined_slice, advance)) = offer_follow {
                let mut probe = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                probe.has_source = builder.has_source;
                probe.target = builder.target;
                probe.graveyard_target_bounds = builder.graveyard_target_bounds;
                probe.x_target_count = builder.x_target_count;
                probe.up_to_x_target_count = builder.up_to_x_target_count;
                probe.last_group_filter = builder.last_group_filter;
                if lower_instruction(
                    &ins[i],
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.instructions.len() == 1
                    && let Some(Instruction::MutatePlayer(offer_spec)) = probe.instructions.pop()
                    && let spellwake_engine::toolbox::MutatePlayerSpec {
                        players: spellwake_engine::toolbox::PlayerSelector::Group(players),
                        op:
                            spellwake_engine::toolbox::PlayerMutation::OfferAnyPlayer {
                                cost_body, ..
                            },
                    } = *offer_spec
                {
                    let paid_body = match paid_slice {
                        Some(p_ast) => lower_offer_loop_player_sub_body(
                            p_ast,
                            &mut probe,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?,
                        None => ExecutableBody::new(BodyTarget::None, Vec::new()),
                    };
                    let declined_body = match declined_slice {
                        Some(d_ast) => {
                            let mut dec_sub = BodyBuilder::with_trigger_hint(probe.trigger_hint);
                            dec_sub.has_source = probe.has_source;
                            dec_sub.last_group_filter = probe.last_group_filter;
                            lower_instruction_seq(
                                d_ast,
                                &mut dec_sub,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                            if dec_sub.instructions.is_empty() {
                                return Err("oracle:empty-body");
                            }
                            if dec_sub.target != BodyTarget::None {
                                if probe.target == BodyTarget::None {
                                    probe.bind_target(dec_sub.target)?;
                                } else if probe.target != dec_sub.target {
                                    return Err("oracle:multiple-targets");
                                }
                                dec_sub.target = BodyTarget::None;
                            }
                            if probe.graveyard_target_bounds.is_none() {
                                probe.graveyard_target_bounds = dec_sub.graveyard_target_bounds;
                            }
                            probe.x_target_count |= dec_sub.x_target_count;
                            probe.up_to_x_target_count |= dec_sub.up_to_x_target_count;
                            Some(ExecutableBody::new(BodyTarget::None, dec_sub.instructions))
                        }
                        None => None,
                    };
                    if probe.target != builder.target {
                        builder.bind_target(probe.target)?;
                    }
                    if builder.graveyard_target_bounds.is_none() {
                        builder.graveyard_target_bounds = probe.graveyard_target_bounds;
                    }
                    builder.x_target_count |= probe.x_target_count;
                    builder.up_to_x_target_count |= probe.up_to_x_target_count;
                    builder
                        .instructions
                        .push(Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::offer_any_player(
                                players,
                                *cost_body,
                                paid_body,
                                declined_body,
                            ),
                        )));
                    i += advance;
                    continue;
                }
            }
        }
        // Unified Optional / OptionalPlayer starting with PayCost/PlayerPayCost mana payment
        // (PayMana, PayGenericManaExpr, PayScaledMana, or BindX wrapping PayMana):
        // Never wrap OfferManaPayment / OfferDynamicManaPayment in OptionalEffect (program.rs rejects it).
        #[derive(Clone, Copy)]
        enum OptManaSpec<'a> {
            Raw(&'a str),
            GenericExpr(&'a ValueExprAst),
            ScaledTwiceX,
            BoundRaw(&'a ValueExprAst, &'a str),
            ManaChoice(&'a [String]),
            ManaOrLife(&'a str, &'a ValueExprAst),
        }
        fn expand_hybrid_mana_costs(raw: &str) -> Option<Vec<ManaCost>> {
            let (base, x_cnt, hybrids) = parse_mana_cost_full(raw)?;
            if x_cnt > 0 || hybrids.is_empty() {
                return None;
            }
            let mut prices: std::collections::BTreeSet<(
                RulesInteger,
                std::collections::BTreeMap<ManaType, RulesInteger>,
            )> = std::collections::BTreeSet::from([(base.generic, base.required)]);
            for sym in &hybrids {
                let mut next = prices.clone();
                for (generic, required) in &prices {
                    let mut req = required.clone();
                    let first = req.get_mut(&sym.first)?;
                    if !first.is_positive() {
                        return None;
                    }
                    *first = first.minus(&1.into());
                    if !first.is_positive() {
                        req.remove(&sym.first);
                    }
                    let mut generic_amt = generic.clone();
                    match sym.alternative {
                        HybridAlternative::Mana(alt_color) => {
                            let entry = req.entry(alt_color).or_insert_with(|| 0.into());
                            *entry = entry.plus(&1.into());
                        }
                        HybridAlternative::TwoGeneric => {
                            generic_amt = generic_amt.plus(&2.into());
                        }
                        HybridAlternative::TwoLife => return None,
                    }
                    next.insert((generic_amt, req));
                }
                prices = next;
                if prices.len() > 16 {
                    return None;
                }
            }
            let out: Vec<ManaCost> = prices
                .into_iter()
                .map(|(generic, required)| ManaCost { generic, required })
                .collect();
            (2..=16).contains(&out.len()).then_some(out)
        }
        fn expand_mana_choice_costs(choices: &[String]) -> Option<Vec<ManaCost>> {
            let mut prices: std::collections::BTreeSet<(
                RulesInteger,
                std::collections::BTreeMap<ManaType, RulesInteger>,
            )> = std::collections::BTreeSet::new();
            for raw in choices {
                if let Some(fixed) = parse_fixed_mana_cost(raw) {
                    prices.insert((fixed.generic, fixed.required));
                } else {
                    let expanded = expand_hybrid_mana_costs(raw)?;
                    for c in expanded {
                        prices.insert((c.generic, c.required));
                    }
                }
                if prices.len() > 16 {
                    return None;
                }
            }
            let out: Vec<ManaCost> = prices
                .into_iter()
                .map(|(generic, required)| ManaCost { generic, required })
                .collect();
            (2..=16).contains(&out.len()).then_some(out)
        }
        fn format_mana_cost_label(cost: &ManaCost) -> String {
            let mut out = String::new();
            if cost.generic.is_positive()
                && let Some(g) = cost.generic.to_usize()
            {
                out.push('{');
                out.push_str(&g.to_string());
                out.push('}');
            }
            for (mt, sym) in [
                (ManaType::White, "{W}"),
                (ManaType::Blue, "{U}"),
                (ManaType::Black, "{B}"),
                (ManaType::Red, "{R}"),
                (ManaType::Green, "{G}"),
                (ManaType::Colourless, "{C}"),
            ] {
                if let Some(n) = cost.required.get(&mt).and_then(|v| v.to_usize()) {
                    for _ in 0..n {
                        out.push_str(sym);
                    }
                }
            }
            if out.is_empty() {
                "{0}".to_owned()
            } else {
                out
            }
        }
        fn build_multi_option_offer_instruction(
            payer: ResolvingPlayer,
            mana_costs: Vec<ManaCost>,
            life_opt: Option<ValueExpr>,
            paid_ops: &[Instruction],
            else_ops: &[Instruction],
        ) -> Result<Instruction, &'static str> {
            let mut options =
                Vec::with_capacity(1 + mana_costs.len() + usize::from(life_opt.is_some()));
            let decline_body = if else_ops.is_empty() {
                vec![Instruction::Jump { destination: 1 }]
            } else {
                else_ops.to_vec()
            };
            options.push(ChooseOptionBranch {
                label: "Decline".to_owned(),
                body: ExecutableBody::new(BodyTarget::None, decline_body),
            });
            for cost in mana_costs {
                let label = format!("Pay {}", format_mana_cost_label(&cost));
                let mut branch_ops = Vec::new();
                if paid_ops.is_empty() && else_ops.is_empty() {
                    branch_ops.push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 1,
                        declined_destination: 1,
                    });
                } else if else_ops.is_empty() {
                    branch_ops.push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 1,
                        declined_destination: 1 + paid_ops.len(),
                    });
                    for op in paid_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(1, 0);
                        branch_ops.push(cloned);
                    }
                } else if paid_ops.is_empty() {
                    branch_ops.push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 1 + else_ops.len(),
                        declined_destination: 1,
                    });
                    for op in else_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(1, 0);
                        branch_ops.push(cloned);
                    }
                } else {
                    let else_start = 1 + paid_ops.len() + 1;
                    let end_dest = else_start + else_ops.len();
                    let else_shift =
                        isize::try_from(else_start).map_err(|_| "oracle:unsupported-pay-cost")?;
                    branch_ops.push(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: cost,
                        extra_generic: ValueExpr::Literal(0.into()),
                        paid_destination: 1,
                        declined_destination: else_start,
                    });
                    for op in paid_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(1, 0);
                        branch_ops.push(cloned);
                    }
                    branch_ops.push(Instruction::Jump {
                        destination: end_dest,
                    });
                    for op in else_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(else_shift, 0);
                        branch_ops.push(cloned);
                    }
                }
                options.push(ChooseOptionBranch {
                    label,
                    body: ExecutableBody::new(BodyTarget::None, branch_ops),
                });
            }
            if let Some(life_amt) = life_opt {
                let mut branch_ops = Vec::new();
                branch_ops.push(super::super::toolbox_emit::pay_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(payer),
                    life_amt,
                ));
                if paid_ops.is_empty() && else_ops.is_empty() {
                    // PayLife alone suffices when no follow-up branch exists.
                } else if else_ops.is_empty() {
                    branch_ops.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: 2 + paid_ops.len(),
                    });
                    for op in paid_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(2, 0);
                        branch_ops.push(cloned);
                    }
                } else if paid_ops.is_empty() {
                    branch_ops.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(0),
                        },
                        destination: 2 + else_ops.len(),
                    });
                    for op in else_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(2, 0);
                        branch_ops.push(cloned);
                    }
                } else {
                    let else_start = 2 + paid_ops.len() + 1;
                    let end_dest = else_start + else_ops.len();
                    let else_shift =
                        isize::try_from(else_start).map_err(|_| "oracle:unsupported-pay-cost")?;
                    branch_ops.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
                        destination: else_start,
                    });
                    for op in paid_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(2, 0);
                        branch_ops.push(cloned);
                    }
                    branch_ops.push(Instruction::Jump {
                        destination: end_dest,
                    });
                    for op in else_ops {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(else_shift, 0);
                        branch_ops.push(cloned);
                    }
                }
                options.push(ChooseOptionBranch {
                    label: "Pay life".to_owned(),
                    body: ExecutableBody::new(BodyTarget::None, branch_ops),
                });
            }
            Ok(Instruction::ChooseOption {
                chooser: payer,
                options,
            })
        }
        fn extract_opt_mana_spec<'a>(
            default_player: PlayerRefAst,
            opt_inner: &'a [InstructionAst],
        ) -> Option<(PlayerRefAst, OptManaSpec<'a>, &'a [InstructionAst])> {
            match opt_inner.first() {
                Some(InstructionAst::PayCost(CostAtomAst::PayMana(raw))) => Some((
                    default_player,
                    OptManaSpec::Raw(raw.as_str()),
                    &opt_inner[1..],
                )),
                Some(InstructionAst::PayCost(CostAtomAst::PayManaChoice(choices)))
                    if choices.len() >= 2 =>
                {
                    Some((
                        default_player,
                        OptManaSpec::ManaChoice(choices.as_slice()),
                        &opt_inner[1..],
                    ))
                }
                Some(InstructionAst::PayCost(CostAtomAst::Or(lhs, rhs))) => {
                    match (lhs.as_ref(), rhs.as_ref()) {
                        (CostAtomAst::PayMana(raw), CostAtomAst::PayLife(life))
                        | (CostAtomAst::PayLife(life), CostAtomAst::PayMana(raw)) => Some((
                            default_player,
                            OptManaSpec::ManaOrLife(raw.as_str(), life.as_ref()),
                            &opt_inner[1..],
                        )),
                        _ => None,
                    }
                }
                Some(InstructionAst::PayCost(CostAtomAst::PayGenericManaExpr(expr))) => Some((
                    default_player,
                    OptManaSpec::GenericExpr(expr.as_ref()),
                    &opt_inner[1..],
                )),
                Some(InstructionAst::PayCost(CostAtomAst::PayScaledMana { base, per }))
                    if base.eq_ignore_ascii_case("{x}") && per.eq_ignore_ascii_case("twice") =>
                {
                    Some((default_player, OptManaSpec::ScaledTwiceX, &opt_inner[1..]))
                }
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayMana(raw),
                }) => Some((*player, OptManaSpec::Raw(raw.as_str()), &opt_inner[1..])),
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayManaChoice(choices),
                }) if choices.len() >= 2 => Some((
                    *player,
                    OptManaSpec::ManaChoice(choices.as_slice()),
                    &opt_inner[1..],
                )),
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::Or(lhs, rhs),
                }) => match (lhs.as_ref(), rhs.as_ref()) {
                    (CostAtomAst::PayMana(raw), CostAtomAst::PayLife(life))
                    | (CostAtomAst::PayLife(life), CostAtomAst::PayMana(raw)) => Some((
                        *player,
                        OptManaSpec::ManaOrLife(raw.as_str(), life.as_ref()),
                        &opt_inner[1..],
                    )),
                    _ => None,
                },
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayGenericManaExpr(expr),
                }) => Some((
                    *player,
                    OptManaSpec::GenericExpr(expr.as_ref()),
                    &opt_inner[1..],
                )),
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayScaledMana { base, per },
                }) if base.eq_ignore_ascii_case("{x}") && per.eq_ignore_ascii_case("twice") => {
                    Some((*player, OptManaSpec::ScaledTwiceX, &opt_inner[1..]))
                }
                Some(InstructionAst::BindX { x_expr, body }) => match body.as_slice() {
                    [InstructionAst::PayCost(CostAtomAst::PayMana(raw))] => Some((
                        default_player,
                        OptManaSpec::BoundRaw(x_expr.as_ref(), raw.as_str()),
                        &opt_inner[1..],
                    )),
                    [
                        InstructionAst::PlayerPayCost {
                            player,
                            cost: CostAtomAst::PayMana(raw),
                        },
                    ] => Some((
                        *player,
                        OptManaSpec::BoundRaw(x_expr.as_ref(), raw.as_str()),
                        &opt_inner[1..],
                    )),
                    _ => None,
                },
                _ => None,
            }
        }
        let opt_mana_head: Option<(PlayerRefAst, OptManaSpec<'_>, &[InstructionAst])> =
            match &ins[i] {
                InstructionAst::Optional(opt_inner) => {
                    extract_opt_mana_spec(PlayerRefAst::You, opt_inner)
                }
                InstructionAst::OptionalPlayer {
                    player: opt_player,
                    body: opt_inner,
                } if !matches!(
                    opt_player,
                    PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent
                ) =>
                {
                    extract_opt_mana_spec(*opt_player, opt_inner)
                }
                _ => None,
            };
        if let Some((payer_ast, mana_spec, opt_rest)) = opt_mana_head {
            let group_players = match payer_ast {
                PlayerRefAst::EachPlayer => Some(AffectedPlayers::EachPlayer),
                PlayerRefAst::EachOpponent => Some(AffectedPlayers::EachOpponent),
                PlayerRefAst::EachOtherPlayer => Some(AffectedPlayers::EachOtherPlayer),
                _ => None,
            };
            let payer = if group_players.is_some() {
                ResolvingPlayer::LoopPlayer
            } else {
                lower_resolving_player_in_context(payer_ast, builder, in_trigger)?
            };
            let eff_bound_x = match mana_spec {
                OptManaSpec::BoundRaw(xe, _) => Some(xe),
                _ => bound_x,
            };
            let mut multi_offer: Option<(Vec<ManaCost>, Option<ValueExpr>)> = None;
            let offer_template: Option<Instruction> = match mana_spec {
                OptManaSpec::Raw(raw_mana) | OptManaSpec::BoundRaw(_, raw_mana) => {
                    if let Some(op) =
                        try_build_offer_mana(payer, raw_mana, 0, 0, eff_bound_x, vocabulary)
                    {
                        Some(op)
                    } else if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw_mana)
                        && x_cnt > 0
                    {
                        let x_val = match eff_bound_x {
                            Some(expr) => {
                                lower_value_expr_in_context(expr, builder, in_trigger, vocabulary)?
                            }
                            None => ValueExpr::chosen_x(),
                        };
                        Some(Instruction::OfferDynamicManaPayment {
                            payer,
                            fixed,
                            extra_generic: scale_value_expr(x_val, i64::from(x_cnt)),
                            paid_destination: 0,
                            declined_destination: 0,
                        })
                    } else if let Some(hybrid_prices) = expand_hybrid_mana_costs(raw_mana) {
                        multi_offer = Some((hybrid_prices, None));
                        None
                    } else {
                        return Err("oracle:unsupported-pay-cost");
                    }
                }
                OptManaSpec::ManaChoice(choices) => {
                    let Some(choice_prices) = expand_mana_choice_costs(choices) else {
                        return Err("oracle:unsupported-pay-cost");
                    };
                    multi_offer = Some((choice_prices, None));
                    None
                }
                OptManaSpec::ManaOrLife(raw_mana, life_expr) => {
                    let mana_costs = if let Some(fixed) = parse_fixed_mana_cost(raw_mana) {
                        vec![fixed]
                    } else if let Some(expanded) = expand_hybrid_mana_costs(raw_mana) {
                        expanded
                    } else {
                        return Err("oracle:unsupported-pay-cost");
                    };
                    let life_val =
                        lower_value_expr_in_context(life_expr, builder, in_trigger, vocabulary)?;
                    multi_offer = Some((mana_costs, Some(life_val)));
                    None
                }
                OptManaSpec::GenericExpr(expr) => {
                    let extra_generic = lower_value_expr_in_context(
                        resolve_x(expr, eff_bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )?;
                    Some(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: ManaCost::free(),
                        extra_generic,
                        paid_destination: 0,
                        declined_destination: 0,
                    })
                }
                OptManaSpec::ScaledTwiceX => {
                    let x_val = match eff_bound_x {
                        Some(expr) => {
                            lower_value_expr_in_context(expr, builder, in_trigger, vocabulary)?
                        }
                        None => ValueExpr::chosen_x(),
                    };
                    Some(Instruction::OfferDynamicManaPayment {
                        payer,
                        fixed: ManaCost::free(),
                        extra_generic: scale_value_expr(x_val, 2),
                        paid_destination: 0,
                        declined_destination: 0,
                    })
                }
            };
            if let Some((mana_costs, life_opt)) = multi_offer {
                let bound_x = eff_bound_x;
                if let Some(players) = group_players {
                    if opt_rest.is_empty() {
                        let sub_op = build_multi_option_offer_instruction(
                            payer,
                            mana_costs,
                            life_opt,
                            &[],
                            &[],
                        )?;
                        builder.instructions.push(Instruction::ForEachPlayerQuery {
                            selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                                players,
                            )),
                            bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                            body: Box::new(ExecutableBody::new(BodyTarget::None, vec![sub_op])),
                        });
                        i += 1;
                        continue;
                    }
                    return Err("oracle:unsupported-pay-cost");
                }
                let lower_sub_branch = |sub_ins: &[InstructionAst],
                                        outer: &mut BodyBuilder,
                                        ids_ref: &mut LoweringIds|
                 -> Result<Vec<Instruction>, &'static str> {
                    let mut sub = BodyBuilder::with_trigger_hint(outer.trigger_hint);
                    sub.target = outer.target;
                    sub.has_source = outer.has_source;
                    sub.last_group_filter = outer.last_group_filter;
                    lower_instruction_seq(
                        sub_ins,
                        &mut sub,
                        vocabulary,
                        ids_ref,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    if sub.target != BodyTarget::None {
                        outer.bind_target(sub.target)?;
                    }
                    if outer.graveyard_target_bounds.is_none() {
                        outer.graveyard_target_bounds = sub.graveyard_target_bounds;
                    }
                    outer.x_target_count |= sub.x_target_count;
                    outer.up_to_x_target_count |= sub.up_to_x_target_count;
                    outer.up_to_one_target |= sub.up_to_one_target;
                    Ok(sub.instructions)
                };
                if !opt_rest.is_empty() {
                    let paid_ops = lower_sub_branch(opt_rest, builder, ids)?;
                    let choice_op = build_multi_option_offer_instruction(
                        payer,
                        mana_costs,
                        life_opt,
                        &paid_ops,
                        &[],
                    )?;
                    builder.instructions.push(choice_op);
                    i += 1;
                    continue;
                }
                if i + 1 < ins.len() {
                    match &ins[i + 1] {
                        InstructionAst::IfPreviousSucceeded(then_body) => {
                            let tail_checks_top = matches!(
                                then_body.last(),
                                Some(
                                    InstructionAst::LookTopLibrary {
                                        reveal: true,
                                        count,
                                        ..
                                    } | InstructionAst::ExileTopLibrary { count, .. }
                                ) if matches!(count.as_ref(), ValueExprAst::Const(1))
                            );
                            if tail_checks_top
                                && i + 2 < ins.len()
                                && let InstructionAst::IfCondition { condition, .. } = &ins[i + 2]
                                && condition_references_anaphora(condition)
                            {
                                let has_else = i + 3 < ins.len()
                                    && matches!(&ins[i + 3], InstructionAst::Otherwise(_));
                                let mut combined = Vec::with_capacity(
                                    then_body.len() + if has_else { 2 } else { 1 },
                                );
                                combined.extend_from_slice(then_body);
                                combined.push(ins[i + 2].clone());
                                if has_else {
                                    combined.push(ins[i + 3].clone());
                                }
                                let paid_ops = lower_sub_branch(&combined, builder, ids)?;
                                let choice_op = build_multi_option_offer_instruction(
                                    payer,
                                    mana_costs,
                                    life_opt,
                                    &paid_ops,
                                    &[],
                                )?;
                                builder.instructions.push(choice_op);
                                i += if has_else { 4 } else { 3 };
                                continue;
                            }
                            let paid_ops = lower_sub_branch(then_body, builder, ids)?;
                            if i + 2 < ins.len()
                                && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
                            {
                                let else_ops = lower_sub_branch(else_body, builder, ids)?;
                                let choice_op = build_multi_option_offer_instruction(
                                    payer, mana_costs, life_opt, &paid_ops, &else_ops,
                                )?;
                                builder.instructions.push(choice_op);
                                i += 3;
                            } else {
                                let choice_op = build_multi_option_offer_instruction(
                                    payer,
                                    mana_costs,
                                    life_opt,
                                    &paid_ops,
                                    &[],
                                )?;
                                builder.instructions.push(choice_op);
                                i += 2;
                            }
                            continue;
                        }
                        InstructionAst::IfPreviousFailed(else_body) => {
                            let else_ops = lower_sub_branch(else_body, builder, ids)?;
                            let choice_op = build_multi_option_offer_instruction(
                                payer,
                                mana_costs,
                                life_opt,
                                &[],
                                &else_ops,
                            )?;
                            builder.instructions.push(choice_op);
                            i += 2;
                            continue;
                        }
                        InstructionAst::ReflexiveTrigger(ref_body) => {
                            let bind_val = if let Some(bx) = bound_x
                                && ref_body.iter().any(instruction_has_chosen_x)
                            {
                                Some(lower_bound_x_for_resolution(
                                    bx, builder, vocabulary, in_trigger,
                                )?)
                            } else {
                                None
                            };
                            let reflexive_body = lower_effect_body(
                                ref_body,
                                vocabulary,
                                ids,
                                true,
                                self_pronoun_valid,
                            )?;
                            let mut paid_ops = Vec::new();
                            if let Some(val) = bind_val {
                                paid_ops.push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(
                                        val,
                                    ),
                                )));
                            }
                            paid_ops.push(Instruction::EmitReflexiveTrigger {
                                body: Box::new(reflexive_body),
                            });
                            let choice_op = build_multi_option_offer_instruction(
                                payer,
                                mana_costs,
                                life_opt,
                                &paid_ops,
                                &[],
                            )?;
                            builder.instructions.push(choice_op);
                            i += 2;
                            continue;
                        }
                        InstructionAst::BindX { x_expr, body } => {
                            if let [InstructionAst::ReflexiveTrigger(ref_body)] = body.as_slice() {
                                let x_val = lower_bound_x_for_resolution(
                                    x_expr, builder, vocabulary, in_trigger,
                                )?;
                                let reflexive_body = lower_effect_body(
                                    ref_body,
                                    vocabulary,
                                    ids,
                                    true,
                                    self_pronoun_valid,
                                )?;
                                let paid_ops = vec![
                                    Instruction::MutatePlayer(Box::new(
                                        spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(x_val),
                                    )),
                                    Instruction::EmitReflexiveTrigger {
                                        body: Box::new(reflexive_body),
                                    },
                                ];
                                let choice_op = build_multi_option_offer_instruction(
                                    payer,
                                    mana_costs,
                                    life_opt,
                                    &paid_ops,
                                    &[],
                                )?;
                                builder.instructions.push(choice_op);
                                i += 2;
                                continue;
                            }
                            if let [InstructionAst::IfPreviousSucceeded(then_body)] =
                                body.as_slice()
                            {
                                let mut sub = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                                sub.target = builder.target;
                                sub.has_source = builder.has_source;
                                sub.last_group_filter = builder.last_group_filter;
                                lower_instruction_seq(
                                    then_body,
                                    &mut sub,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    Some(x_expr.as_ref()),
                                )?;
                                if sub.target != BodyTarget::None {
                                    builder.bind_target(sub.target)?;
                                }
                                if builder.graveyard_target_bounds.is_none() {
                                    builder.graveyard_target_bounds = sub.graveyard_target_bounds;
                                }
                                builder.x_target_count |= sub.x_target_count;
                                builder.up_to_x_target_count |= sub.up_to_x_target_count;
                                builder.up_to_one_target |= sub.up_to_one_target;
                                let paid_ops = sub.instructions;
                                if i + 2 < ins.len()
                                    && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
                                {
                                    let else_ops = lower_sub_branch(else_body, builder, ids)?;
                                    let choice_op = build_multi_option_offer_instruction(
                                        payer, mana_costs, life_opt, &paid_ops, &else_ops,
                                    )?;
                                    builder.instructions.push(choice_op);
                                    i += 3;
                                } else {
                                    let choice_op = build_multi_option_offer_instruction(
                                        payer,
                                        mana_costs,
                                        life_opt,
                                        &paid_ops,
                                        &[],
                                    )?;
                                    builder.instructions.push(choice_op);
                                    i += 2;
                                }
                                continue;
                            }
                            if let [InstructionAst::IfPreviousFailed(else_body)] = body.as_slice() {
                                let mut sub = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                                sub.target = builder.target;
                                sub.has_source = builder.has_source;
                                sub.last_group_filter = builder.last_group_filter;
                                lower_instruction_seq(
                                    else_body,
                                    &mut sub,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    Some(x_expr.as_ref()),
                                )?;
                                if sub.target != BodyTarget::None {
                                    builder.bind_target(sub.target)?;
                                }
                                if builder.graveyard_target_bounds.is_none() {
                                    builder.graveyard_target_bounds = sub.graveyard_target_bounds;
                                }
                                builder.x_target_count |= sub.x_target_count;
                                builder.up_to_x_target_count |= sub.up_to_x_target_count;
                                builder.up_to_one_target |= sub.up_to_one_target;
                                let choice_op = build_multi_option_offer_instruction(
                                    payer,
                                    mana_costs,
                                    life_opt,
                                    &[],
                                    &sub.instructions,
                                )?;
                                builder.instructions.push(choice_op);
                                i += 2;
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                let choice_op =
                    build_multi_option_offer_instruction(payer, mana_costs, life_opt, &[], &[])?;
                builder.instructions.push(choice_op);
                i += 1;
                continue;
            }
            let offer_template = offer_template.ok_or("oracle:unsupported-pay-cost")?;
            if let Some(players) = group_players {
                if opt_rest.is_empty() {
                    let sub_op = with_offer_dests(offer_template, 1, 1);
                    builder.instructions.push(Instruction::ForEachPlayerQuery {
                        selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(
                            players,
                        )),
                        bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                        body: Box::new(ExecutableBody::new(BodyTarget::None, vec![sub_op])),
                    });
                    i += 1;
                    continue;
                }
                return Err("oracle:unsupported-pay-cost");
            }
            let bound_x = eff_bound_x;
            if !opt_rest.is_empty() {
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(with_offer_dests(offer_template, offer_idx + 1, 0));
                lower_instruction_seq(
                    opt_rest,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                set_offer_declined(builder.instructions.get_mut(offer_idx), end_dest);
                i += 1;
                continue;
            }
            if i + 1 < ins.len() {
                match &ins[i + 1] {
                    InstructionAst::IfPreviousSucceeded(then_body) => {
                        let offer_idx = builder.instructions.len();
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            offer_idx + 1,
                            0,
                        ));
                        // If the paid branch ends with revealing/exiling the top card of a library
                        // and the next sibling sentence tests that revealed/exiled card (plus optional Otherwise),
                        // pull those dependent sentences inside the paid branch (e.g. Skirk Drill Sergeant).
                        let tail_checks_top = matches!(
                            then_body.last(),
                            Some(
                                InstructionAst::LookTopLibrary {
                                    reveal: true,
                                    count,
                                    ..
                                } | InstructionAst::ExileTopLibrary { count, .. }
                            ) if matches!(count.as_ref(), ValueExprAst::Const(1))
                        );
                        if tail_checks_top
                            && i + 2 < ins.len()
                            && let InstructionAst::IfCondition { condition, .. } = &ins[i + 2]
                            && condition_references_anaphora(condition)
                        {
                            let has_else = i + 3 < ins.len()
                                && matches!(&ins[i + 3], InstructionAst::Otherwise(_));
                            let mut combined =
                                Vec::with_capacity(then_body.len() + if has_else { 2 } else { 1 });
                            combined.extend_from_slice(then_body);
                            combined.push(ins[i + 2].clone());
                            if has_else {
                                combined.push(ins[i + 3].clone());
                            }
                            lower_instruction_seq(
                                &combined,
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                            let end_dest = builder.instructions.len();
                            set_offer_declined(builder.instructions.get_mut(offer_idx), end_dest);
                            i += if has_else { 4 } else { 3 };
                            continue;
                        }
                        lower_instruction_seq(
                            then_body,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        if i + 2 < ins.len()
                            && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
                        {
                            let jump_idx = builder.instructions.len();
                            builder
                                .instructions
                                .push(Instruction::Jump { destination: 0 });
                            let else_start = builder.instructions.len();
                            set_offer_declined(builder.instructions.get_mut(offer_idx), else_start);
                            lower_instruction_seq(
                                else_body,
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::Jump { destination }) =
                                builder.instructions.get_mut(jump_idx)
                            {
                                *destination = end_dest;
                            }
                            i += 3;
                        } else {
                            let end_dest = builder.instructions.len();
                            set_offer_declined(builder.instructions.get_mut(offer_idx), end_dest);
                            i += 2;
                        }
                        continue;
                    }
                    InstructionAst::IfPreviousFailed(else_body) => {
                        let offer_idx = builder.instructions.len();
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            0,
                            offer_idx + 1,
                        ));
                        lower_instruction_seq(
                            else_body,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        let end_dest = builder.instructions.len();
                        set_offer_paid(builder.instructions.get_mut(offer_idx), end_dest);
                        i += 2;
                        continue;
                    }
                    InstructionAst::ReflexiveTrigger(ref_body) => {
                        let bind_val = if let Some(bx) = bound_x
                            && ref_body.iter().any(instruction_has_chosen_x)
                        {
                            Some(lower_bound_x_for_resolution(
                                bx, builder, vocabulary, in_trigger,
                            )?)
                        } else {
                            None
                        };
                        let reflexive_body =
                            lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
                        let offer_idx = builder.instructions.len();
                        let paid_len = if bind_val.is_some() { 2 } else { 1 };
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            offer_idx + 1,
                            offer_idx + 1 + paid_len,
                        ));
                        if let Some(val) = bind_val {
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(
                                        val,
                                    ),
                                )));
                        }
                        builder
                            .instructions
                            .push(Instruction::EmitReflexiveTrigger {
                                body: Box::new(reflexive_body),
                            });
                        i += 2;
                        continue;
                    }
                    InstructionAst::BindX { x_expr, body } => {
                        if let [InstructionAst::ReflexiveTrigger(ref_body)] = body.as_slice() {
                            let x_val = lower_bound_x_for_resolution(
                                x_expr, builder, vocabulary, in_trigger,
                            )?;
                            let reflexive_body = lower_effect_body(
                                ref_body,
                                vocabulary,
                                ids,
                                true,
                                self_pronoun_valid,
                            )?;
                            let offer_idx = builder.instructions.len();
                            builder.instructions.push(with_offer_dests(
                                offer_template,
                                offer_idx + 1,
                                offer_idx + 3,
                            ));
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(
                                        x_val,
                                    ),
                                )));
                            builder
                                .instructions
                                .push(Instruction::EmitReflexiveTrigger {
                                    body: Box::new(reflexive_body),
                                });
                            i += 2;
                            continue;
                        }
                        if let [InstructionAst::IfPreviousSucceeded(then_body)] = body.as_slice() {
                            let offer_idx = builder.instructions.len();
                            builder.instructions.push(with_offer_dests(
                                offer_template,
                                offer_idx + 1,
                                0,
                            ));
                            lower_instruction_seq(
                                then_body,
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                Some(x_expr.as_ref()),
                            )?;
                            if i + 2 < ins.len()
                                && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
                            {
                                let jump_idx = builder.instructions.len();
                                builder
                                    .instructions
                                    .push(Instruction::Jump { destination: 0 });
                                let else_start = builder.instructions.len();
                                set_offer_declined(
                                    builder.instructions.get_mut(offer_idx),
                                    else_start,
                                );
                                lower_instruction_seq(
                                    else_body,
                                    builder,
                                    vocabulary,
                                    ids,
                                    in_trigger,
                                    self_pronoun_valid,
                                    bound_x,
                                )?;
                                let end_dest = builder.instructions.len();
                                if let Some(Instruction::Jump { destination }) =
                                    builder.instructions.get_mut(jump_idx)
                                {
                                    *destination = end_dest;
                                }
                                i += 3;
                            } else {
                                let end_dest = builder.instructions.len();
                                set_offer_declined(
                                    builder.instructions.get_mut(offer_idx),
                                    end_dest,
                                );
                                i += 2;
                            }
                            continue;
                        }
                        if let [InstructionAst::IfPreviousFailed(else_body)] = body.as_slice() {
                            let offer_idx = builder.instructions.len();
                            builder.instructions.push(with_offer_dests(
                                offer_template,
                                0,
                                offer_idx + 1,
                            ));
                            lower_instruction_seq(
                                else_body,
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                Some(x_expr.as_ref()),
                            )?;
                            let end_dest = builder.instructions.len();
                            set_offer_paid(builder.instructions.get_mut(offer_idx), end_dest);
                            i += 2;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            // Standalone optional mana payment: emit OfferManaPayment directly without OptionalEffect.
            let offer_idx = builder.instructions.len();
            builder.instructions.push(with_offer_dests(
                offer_template,
                offer_idx + 1,
                offer_idx + 1,
            ));
            i += 1;
            continue;
        }
        // Lower ReflexiveTrigger ("When you do, ...") or BindX([ReflexiveTrigger(...)])
        // with CR 603.12 conditional guard and BindResolutionX propagation.
        let reflexive_pair: Option<(Option<&ValueExprAst>, &[InstructionAst])> = match &ins[i] {
            InstructionAst::ReflexiveTrigger(ref_body) => {
                let outer_x = bound_x.filter(|_| ref_body.iter().any(instruction_has_chosen_x));
                Some((outer_x, ref_body.as_slice()))
            }
            InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                [InstructionAst::ReflexiveTrigger(ref_body)] => {
                    Some((Some(x_expr.as_ref()), ref_body.as_slice()))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some((x_expr_opt, ref_body)) = reflexive_pair {
            let prev_tracks_action = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::DiscardUpTo { .. }
                        | InstructionAst::RevealByPlayer { .. }
                        | InstructionAst::PayAnyAmountOfMana { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::RemoveCounters { .. }
                        | InstructionAst::SearchLibrary { .. }
                        | InstructionAst::Clash { .. }
                );
            let bind_val = x_expr_opt
                .map(|xe| lower_bound_x_for_resolution(xe, builder, vocabulary, in_trigger))
                .transpose()?;
            let reflexive_body =
                lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
            if prev_tracks_action {
                let branch_idx = builder.instructions.len();
                let body_len = if bind_val.is_some() { 2 } else { 1 };
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    destination: branch_idx + 1 + body_len,
                });
            }
            if let Some(val) = bind_val {
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(val),
                    )));
            }
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            i += 1;
            continue;
        }
        // A sentence-local X definition can wrap the search while its found-card
        // movement and shuffle remain sibling sentences (Celebrate the Harvest).
        // Keep the bridge exact and let the existing search continuation own movement.
        if let Some(
            [
                InstructionAst::BindX { x_expr, body },
                movement @ InstructionAst::MoveZone { subject, to },
                shuffle @ InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                },
            ],
        ) = ins.get(i..i + 3)
            && i + 3 == ins.len()
            && let [
                search @ InstructionAst::SearchLibrary {
                    library,
                    count,
                    to_zone,
                    shuffle: false,
                    up_to: true,
                    any_number: false,
                    extra_zones,
                    secondary_filter: None,
                    secondary_is_or: false,
                    where_x: None,
                    rest_to_zone: None,
                    modifier: None,
                    follow_up,
                    exact: false,
                    ..
                },
            ] = body.as_slice()
            && library.eq_ignore_ascii_case("your")
            && matches!(count.as_ref(), ValueExprAst::ChosenX)
            && to_zone == "found_set"
            && extra_zones.is_empty()
            && follow_up.is_empty()
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(name) if name == "those cards")
            && to == "the battlefield tapped"
        {
            let merged = [search.clone(), movement.clone(), shuffle.clone()];
            let mut probe = builder.clone();
            let mut probe_ids = LoweringIds {
                next_activated: ids.next_activated,
                next_triggered: ids.next_triggered,
                next_mana: ids.next_mana,
                next_static: ids.next_static,
                next_ct_slot: ids.next_ct_slot,
            };
            lower_instruction_seq(
                &merged,
                &mut probe,
                vocabulary,
                &mut probe_ids,
                in_trigger,
                self_pronoun_valid,
                Some(x_expr.as_ref()),
            )?;
            *builder = probe;
            *ids = probe_ids;
            i += 3;
            continue;
        }
        // Sentence-scoped BindX whose bound X is referenced by downstream instructions in this sequence
        // (e.g. Chainer's Torment chapter III): emit BindResolutionX so downstream ChosenX validates and reads X.
        if let InstructionAst::BindX { x_expr, body } = &ins[i]
            && !body.is_empty()
            && ins[i + 1..].iter().any(instruction_has_chosen_x)
        {
            let pre_x = lower_bound_x_for_resolution(x_expr, builder, vocabulary, in_trigger).ok();
            let emit_before = pre_x
                .as_ref()
                .is_some_and(|v| builder.target != BodyTarget::None || !v.uses_target());
            if emit_before && let Some(val) = pre_x {
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(val),
                    )));
                lower_instruction_seq(
                    body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    Some(x_expr.as_ref()),
                )?;
            } else {
                lower_instruction_seq(
                    body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    Some(x_expr.as_ref()),
                )?;
                if let Ok(val) =
                    lower_bound_x_for_resolution(x_expr, builder, vocabulary, in_trigger)
                {
                    builder
                        .instructions
                        .push(Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::bind_resolution_x(val),
                        )));
                }
            }
            i += 1;
            continue;
        }
        // Lower IfPreviousSucceeded / IfPreviousFailed (including BindX-wrapped forms, plus
        // optional Otherwise or paired IfPreviousFailed) right after an action that tracks
        // last_action_succeeded.
        let if_prev_entry: Option<(bool, &[InstructionAst], Option<&ValueExprAst>)> = match &ins[i]
        {
            InstructionAst::IfPreviousSucceeded(tb) => Some((true, tb.as_slice(), bound_x)),
            InstructionAst::IfPreviousFailed(tb) => Some((false, tb.as_slice(), bound_x)),
            InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                [InstructionAst::IfPreviousSucceeded(tb)] => {
                    Some((true, tb.as_slice(), Some(x_expr.as_ref())))
                }
                [InstructionAst::IfPreviousFailed(tb)] => {
                    Some((false, tb.as_slice(), Some(x_expr.as_ref())))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some((succeeded, then_body, then_bound_x)) = if_prev_entry {
            let prev_was_optional = i > 0
                && match &ins[i - 1] {
                    InstructionAst::Optional(_) | InstructionAst::OptionalPlayer { .. } => true,
                    InstructionAst::BindX { body, .. } => matches!(
                        body.last(),
                        Some(InstructionAst::Optional(_) | InstructionAst::OptionalPlayer { .. })
                    ),
                    _ => false,
                };
            let prev_emitted_tracks_action = builder.instructions.last().is_some_and(|op| {
                matches!(
                    op,
                    Instruction::ZoneOp(_)
                        | Instruction::ForEachPlayerQuery { .. }
                        | Instruction::Sacrifice { .. }
                        | Instruction::Discard { .. }
                        | Instruction::ExileTopPlay { .. }
                        | Instruction::LookTopChooseExt { .. }
                        | Instruction::RevealUntilMatchExt { .. }
                        | Instruction::SearchLibrary { .. }
                        | Instruction::MutateObject(_)
                ) || matches!(
                    op,
                    Instruction::StackOp(spec)
                        if **spec == spellwake_engine::toolbox::StackOpSpec::counter_triggering()
                ) || matches!(
                    op,
                    Instruction::AdjustLife(spec)
                        if matches!(spec.op, spellwake_engine::toolbox::LifeOp::Pay(_))
                ) || matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::PayCounter { .. }
                                | spellwake_engine::toolbox::PlayerMutation::CollectEvidence { .. }
                                | spellwake_engine::toolbox::PlayerMutation::Behold { .. }
                                | spellwake_engine::toolbox::PlayerMutation::Clash
                                | spellwake_engine::toolbox::PlayerMutation::RevealTopTwoPiles { .. }
                                | spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                                | spellwake_engine::toolbox::PlayerMutation::ChooseOneLastMoved
                                | spellwake_engine::toolbox::PlayerMutation::Mill(_)
                        )
                ) || matches!(
                    op,
                    Instruction::ApplyContinuous(spec)
                        if matches!(
                            spec.payload,
                            spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                        )
                ) || matches!(
                    op,
                    Instruction::MutateObject(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::ObjectMutation::RemoveCounters { .. }
                                | spellwake_engine::toolbox::ObjectMutation::SetTapped(_)
                                | spellwake_engine::toolbox::ObjectMutation::TurnFaceUp
                                | spellwake_engine::toolbox::ObjectMutation::TurnFaceDown
                        )
                )
            });
            let prev_tracks_action = i > 0
                && (matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::DiscardUpTo { .. }
                        | InstructionAst::RevealByPlayer { .. }
                        | InstructionAst::PayAnyAmountOfMana { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::PlayerPayCost { .. }
                        | InstructionAst::RemoveCounters { .. }
                        | InstructionAst::SearchLibrary { .. }
                        | InstructionAst::Clash { .. }
                        | InstructionAst::CollectEvidence { .. }
                        | InstructionAst::Forage
                ) || (prev_emitted_tracks_action
                    && matches!(
                        &ins[i - 1],
                        InstructionAst::Exile { .. }
                            | InstructionAst::ExileTopLibrary { .. }
                            | InstructionAst::ExileGraveyard { .. }
                            | InstructionAst::MoveZone { .. }
                            | InstructionAst::ReturnZone { .. }
                            | InstructionAst::MoveCounted { .. }
                            | InstructionAst::Destroy { .. }
                            | InstructionAst::Mill { .. }
                            | InstructionAst::Counter { .. }
                            | InstructionAst::TapPermanent { .. }
                            | InstructionAst::UntapPermanent { .. }
                            | InstructionAst::ChooseAndKeep { .. }
                            | InstructionAst::LookTopLibrary { .. }
                            | InstructionAst::RevealUntil { .. }
                            | InstructionAst::ExileOrRevealTopUntil { .. }
                            | InstructionAst::BindX { .. }
                    )));
            if prev_tracks_action {
                let is_single_sacrifice = !prev_was_optional
                    && matches!(
                        builder.instructions.last(),
                        Some(Instruction::Sacrifice {
                            players: AffectedPlayers::One(_),
                            amount: ValueExpr::Literal(v),
                            ..
                        }) if *v == 1.into()
                    );
                let kind = if succeeded && is_single_sacrifice {
                    EventValueKind::LastSacrificeSucceeded
                } else {
                    EventValueKind::LastActionSucceeded
                };
                let branch_idx = builder.instructions.len();
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(kind),
                        comparison: if succeeded {
                            Comparison::GreaterOrEqual
                        } else {
                            Comparison::Equal
                        },
                        right: ValueExpr::literal(if succeeded { 1 } else { 0 }),
                    },
                    destination: 0,
                });
                lower_instruction_seq(
                    then_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    then_bound_x,
                )?;
                let paired_else: Option<&[InstructionAst]> = if i + 1 < ins.len() {
                    match &ins[i + 1] {
                        InstructionAst::Otherwise(else_body) => Some(else_body.as_slice()),
                        InstructionAst::IfPreviousFailed(else_body) if succeeded => {
                            Some(else_body.as_slice())
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(else_body) = paired_else {
                    let jump_idx = builder.instructions.len();
                    builder
                        .instructions
                        .push(Instruction::Jump { destination: 0 });
                    let else_dest = builder.instructions.len();
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(branch_idx)
                    {
                        *destination = else_dest;
                    }
                    let unwrapped_else: Vec<InstructionAst>;
                    let else_slice: &[InstructionAst] = if contains_replace_effect(else_body) {
                        unwrapped_else =
                            substitute_those_tokens(&strip_replace_effect(else_body), then_body);
                        &unwrapped_else
                    } else {
                        else_body
                    };
                    lower_instruction_seq(
                        else_slice,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::Jump { destination }) =
                        builder.instructions.get_mut(jump_idx)
                    {
                        *destination = end_dest;
                    }
                    i += 2;
                } else {
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(branch_idx)
                    {
                        *destination = end_dest;
                    }
                    i += 1;
                }
                continue;
            }
        }
        // Lower DelayedTrigger right after CreateTokens / CreateAttackingTokens for sacrifice/exile tokens
        if let InstructionAst::DelayedTrigger {
            step,
            scope: DelayedStepScope::Next,
            body,
        } = &ins[i]
            && matches!(
                builder.instructions.last(),
                Some(Instruction::CreateTokens { .. })
            )
        {
            match (step, body.as_slice()) {
                (
                    TurnStepAst::EndStep,
                    [
                        InstructionAst::Sacrifice {
                            player: PlayerRefAst::You,
                            count: None,
                            subject,
                            except: None,
                            half: false,
                            rounding: None,
                            chooser: None,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    builder
                        .instructions
                        .push(Instruction::ScheduleDelayedStepTrigger {
                            step: Step::End,
                            own_step_only: false,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ZoneOp(Box::new(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                                            semantics:
                                                spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                                        },
                                    ),
                                ))],
                            )),
                        });
                    i += 1;
                    continue;
                }
                (
                    TurnStepAst::EndStep | TurnStepAst::EndOfCombat,
                    [
                        InstructionAst::Exile {
                            actor: None | Some(PlayerRefAst::You),
                            subject,
                            random: false,
                            face_down: false,
                            into_pile: false,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    let eng_step = if *step == TurnStepAst::EndOfCombat {
                        Step::EndCombat
                    } else {
                        Step::End
                    };
                    builder
                        .instructions
                        .push(Instruction::ScheduleDelayedStepTrigger {
                            step: eng_step,
                            own_step_only: false,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ZoneOp(Box::new(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                        spellwake_engine::toolbox::ZoneDest::exile(),
                                    ),
                                ))],
                            )),
                        });
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        // Wrap Optional([ExileTopLibrary]) + dependent card-anaphora follow-ups (IfCondition / stat damage / GrantCastPermission)
        // in a single OptionalSequence so facts.definitely_checked_top holds across the follow-ups (e.g. Daredevil).
        let opt_exile_top = match &ins[i] {
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [exile @ InstructionAst::ExileTopLibrary { .. }] => {
                    Some((PlayerRefAst::You, exile))
                }
                _ => None,
            },
            InstructionAst::OptionalPlayer { player, body } => match body.as_slice() {
                [exile @ InstructionAst::ExileTopLibrary { .. }] => Some((*player, exile)),
                _ => None,
            },
            _ => None,
        };
        if let Some((opt_player, exile_op)) = opt_exile_top
            && i + 1 < ins.len()
        {
            let mut end = i + 1;
            while end < ins.len() {
                match &ins[end] {
                    InstructionAst::IfCondition { condition, .. }
                        if condition_references_anaphora(condition) =>
                    {
                        end += 1;
                        if end < ins.len() && matches!(&ins[end], InstructionAst::Otherwise(_)) {
                            end += 1;
                        }
                    }
                    InstructionAst::DealDamage { amount, .. }
                        if value_expr_references_anaphora_stat(amount) =>
                    {
                        end += 1;
                    }
                    InstructionAst::ChangeLife { delta, .. }
                        if value_expr_references_anaphora_stat(delta) =>
                    {
                        end += 1;
                    }
                    other if is_card_anaphora_grant_cast(other).is_some() => {
                        end += 1;
                    }
                    _ => break,
                }
            }
            if end > i + 1 {
                let mut probe = builder.clone();
                let chooser_res = if opt_player == PlayerRefAst::You {
                    Ok(ResolvingPlayer::Controller)
                } else {
                    lower_resolving_player_in_context(opt_player, &mut probe, in_trigger)
                };
                if let Ok(chooser) = chooser_res {
                    let start = probe.instructions.len();
                    let mut combined = Vec::with_capacity(end - i);
                    combined.push(exile_op.clone());
                    combined.extend_from_slice(&ins[i + 1..end]);
                    if lower_instruction_seq(
                        &combined,
                        &mut probe,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                        && probe.instructions.len() > start
                        && probe.instructions.get(start).is_some_and(|op| {
                            matches!(op, Instruction::ExileTopPlay { .. })
                                || super::super::toolbox_emit::unwrap_zone_op(op).is_some_and(
                                    |spec| {
                                        matches!(
                                            &spec.source,
                                            spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                                zone: spellwake_engine::toolbox::QueryZone::Library(
                                                    _
                                                ),
                                                ..
                                            }
                                        ) && matches!(
                                            spec.destination,
                                            spellwake_engine::toolbox::ZoneDest::Exile { .. }
                                        )
                                    },
                                )
                        })
                    {
                        let span = probe.instructions.len() - start;
                        if span == 1 {
                            prepend_instruction_at(
                                &mut probe.instructions,
                                start,
                                Instruction::OptionalEffect { chooser },
                            );
                        } else {
                            prepend_instruction_at(
                                &mut probe.instructions,
                                start,
                                Instruction::OptionalSequence {
                                    chooser,
                                    instructions: span,
                                },
                            );
                        }
                        *builder = probe;
                        i = end;
                        continue;
                    }
                }
            }
        }
        // Route card-anaphora GrantCastPermission after library-top exile / LookTopChoose(reveal_only) / MayRevealLibraryTop
        // to MoveSubjectRef::RevealedCard even inside triggered abilities (e.g. Daredevil).
        if builder.target == BodyTarget::None
            && builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::LookTopChooseExt {
                        reveal_only: true,
                        ..
                    }
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
                    )
                })
            })
            && let Some(grant_op) = is_card_anaphora_grant_cast(&ins[i])
        {
            lower_instruction(grant_op, builder, vocabulary, ids, false, false, bound_x)?;
            i += 1;
            continue;
        }
        // Fuse paired impulse draw: ExileTopLibrary + GrantCastPermission (+ optional ManaSpendRestriction) -> ExileTopForPlay / ExileTopPlay
        if i + 1 < ins.len()
            && let InstructionAst::ExileTopLibrary {
                actor: None | Some(PlayerRefAst::You),
                whose,
                count,
                face_down,
            } = &ins[i]
            && let Some(InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                subject,
                free,
                duration,
                count_limit: None,
                alt_cost: None,
            }) = (match &ins[i + 1] {
                g @ InstructionAst::GrantCastPermission { .. } => Some(g),
                InstructionAst::Optional(inner) if inner.len() == 1 => {
                    matches!(&inner[0], InstructionAst::GrantCastPermission { .. })
                        .then_some(&inner[0])
                }
                _ => None,
            })
            && matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "them" | "that card" | "those cards" | "the exiled card" | "the exiled cards" | "cards exiled this way" | "a card exiled this way"
                )
            )
        {
            let dur_low = duration.trim().to_ascii_lowercase();
            let parsed_dur = if dur_low == "until_end_of_next_turn"
                || dur_low.contains("end of your next turn")
                || dur_low.contains("untilendofnextturn")
                || dur_low.contains("untilyournextturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn)
            } else if dur_low == "this_turn"
                || dur_low == "until_end_of_turn"
                || dur_low.contains("this turn")
                || dur_low.contains("end of turn")
                || dur_low.contains("untilendofturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::ThisTurn)
            } else if dur_low == "while_exiled"
                || dur_low.contains("remains exiled")
                || dur_low.contains("remain exiled")
                || dur_low == "permanent"
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::WhileExiled)
            } else if dur_low == "until_next_exile" || dur_low.contains("until you exile another") {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource)
            } else {
                None
            };
            let w_low = whose.trim().to_ascii_lowercase();
            let owner_spec: Option<(Option<BodyTarget>, AffectedPlayers)> = match w_low.as_str() {
                "your" | "own" | "" => {
                    Some((None, AffectedPlayers::One(ResolvingPlayer::Controller)))
                }
                "target player" | "target player's" => Some((
                    Some(BodyTarget::SinglePlayer),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "target opponent" | "target opponent's" => Some((
                    Some(BodyTarget::SingleOpponent),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "each opponent" | "each opponent's" => Some((None, AffectedPlayers::EachOpponent)),
                "each player" | "each player's" => Some((None, AffectedPlayers::EachPlayer)),
                "each other player" | "each other player's" => {
                    Some((None, AffectedPlayers::EachOtherPlayer))
                }
                "that player" | "that player's" | "defending player" | "defending player's"
                | "enchanted player" | "enchanted player's" | "its controller"
                | "its controller's" => Some((
                    None,
                    AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                )),
                _ => None,
            };
            if let (Some(impulse_dur), Some((req_target, library_owner))) = (parsed_dur, owner_spec)
            {
                if let Some(bt) = req_target {
                    builder.bind_target(bt)?;
                }
                let mut any_color = false;
                let mut step = 2usize;
                if i + 2 < ins.len() && is_any_color_spend_restriction(&ins[i + 2]) {
                    any_color = true;
                    step = 3;
                }
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                if cnt == ValueExpr::literal(1)
                    && library_owner == AffectedPlayers::One(ResolvingPlayer::Controller)
                    && !*cast_only
                    && !*free
                    && !any_color
                    && !*face_down
                    && impulse_dur == spellwake_engine::program::ImpulsePlayDuration::ThisTurn
                {
                    let mut exile_spec = spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                                ValueExpr::literal(1),
                            )),
                        },
                        spellwake_engine::toolbox::ZoneDest::exile(),
                    );
                    exile_spec.bind_moved = Some(spellwake_engine::toolbox::BindingId(911));
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            exile_spec,
                        ),
                    );
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                            spellwake_engine::toolbox::ObjectQuery::Bound(
                                spellwake_engine::toolbox::BindingId(911),
                            ),
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            ResolvedDuration::UntilEndOfTurn,
                            false,
                            false,
                            false,
                        ),
                    );
                } else {
                    builder.instructions.push(Instruction::ExileTopPlay {
                        count: cnt,
                        library_owner,
                        duration: impulse_dur,
                        cast_only: *cast_only,
                        free: *free,
                        any_color,
                        face_down: *face_down,
                        grant_play: true,
                    });
                }
                i += step;
                continue;
            }
        }
        // Fuse ExileTopLibrary + ChooseOption("one of them") + GrantCastPermission("it"|"that card")
        // e.g. Jaya, Fiery Negotiator / Magmatic Channeler / Feldon, Ronom Excavator / Riverwheel Sweep
        if i + 2 < ins.len()
            && let InstructionAst::ExileTopLibrary {
                actor: None | Some(PlayerRefAst::You),
                whose,
                count,
                face_down,
            } = &ins[i]
            && let Some(choose_one_player) = match &ins[i + 1] {
                InstructionAst::ChooseOption(opt)
                    if matches!(
                        opt.trim()
                            .trim_end_matches('.')
                            .to_ascii_lowercase()
                            .as_str(),
                        "one of them" | "choose one of them"
                    ) =>
                {
                    Some(PlayerRefAst::You)
                }
                InstructionAst::PlayerChooseOption { chooser, options }
                    if options.len() == 1
                        && matches!(
                            options[0]
                                .trim()
                                .trim_end_matches('.')
                                .to_ascii_lowercase()
                                .as_str(),
                            "one of them" | "choose one of them"
                        ) =>
                {
                    Some(*chooser)
                }
                _ => None,
            }
            && let Some(InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                subject,
                free,
                duration,
                count_limit: None,
                alt_cost: None,
            }) = (match &ins[i + 2] {
                g @ InstructionAst::GrantCastPermission { .. } => Some(g),
                InstructionAst::Optional(inner) if inner.len() == 1 => {
                    matches!(&inner[0], InstructionAst::GrantCastPermission { .. })
                        .then_some(&inner[0])
                }
                _ => None,
            })
            && matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "that card" | "the chosen card" | "the exiled card"
                )
            )
        {
            let dur_low = duration.trim().to_ascii_lowercase();
            let parsed_dur = if dur_low == "until_end_of_next_turn"
                || dur_low.contains("end of your next turn")
                || dur_low.contains("untilendofnextturn")
                || dur_low.contains("untilyournextturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn)
            } else if dur_low == "this_turn"
                || dur_low == "until_end_of_turn"
                || dur_low.contains("this turn")
                || dur_low.contains("end of turn")
                || dur_low.contains("untilendofturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::ThisTurn)
            } else if dur_low == "while_exiled"
                || dur_low.contains("remains exiled")
                || dur_low.contains("remain exiled")
                || dur_low == "permanent"
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::WhileExiled)
            } else {
                None
            };
            let w_low = whose.trim().to_ascii_lowercase();
            let owner_spec: Option<(Option<BodyTarget>, AffectedPlayers)> = match w_low.as_str() {
                "your" | "own" | "" => {
                    Some((None, AffectedPlayers::One(ResolvingPlayer::Controller)))
                }
                "target player" | "target player's" => Some((
                    Some(BodyTarget::SinglePlayer),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "target opponent" | "target opponent's" => Some((
                    Some(BodyTarget::SingleOpponent),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "each opponent" | "each opponent's" => Some((None, AffectedPlayers::EachOpponent)),
                "each player" | "each player's" => Some((None, AffectedPlayers::EachPlayer)),
                "each other player" | "each other player's" => {
                    Some((None, AffectedPlayers::EachOtherPlayer))
                }
                _ => None,
            };
            if let (Some(impulse_dur), Some((req_target, library_owner))) = (parsed_dur, owner_spec)
                && let Ok(chooser_rp) =
                    lower_resolving_player_in_context(choose_one_player, builder, in_trigger)
            {
                if let Some(bt) = req_target {
                    builder.bind_target(bt)?;
                }
                let mut any_color = false;
                let mut step = 3usize;
                if i + 3 < ins.len() && is_any_color_spend_restriction(&ins[i + 3]) {
                    any_color = true;
                    step = 4;
                }
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                builder.instructions.push(Instruction::ExileTopPlay {
                    count: cnt,
                    library_owner,
                    duration: impulse_dur,
                    cast_only: *cast_only,
                    free: *free,
                    any_color,
                    face_down: *face_down,
                    grant_play: false,
                });
                builder
                    .instructions
                    .push(Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::choose_one_last_moved(
                            chooser_rp,
                        ),
                    )));
                let play_dur = match impulse_dur {
                    spellwake_engine::program::ImpulsePlayDuration::ThisTurn => {
                        ResolvedDuration::UntilEndOfTurn
                    }
                    spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn => {
                        ResolvedDuration::ThroughControllerNextTurn
                    }
                    spellwake_engine::program::ImpulsePlayDuration::UntilNextEndStep
                    | spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextEndStep
                    | spellwake_engine::program::ImpulsePlayDuration::UntilNextUpkeep => {
                        ResolvedDuration::UntilControllerNextTurn
                    }
                    spellwake_engine::program::ImpulsePlayDuration::WhileExiled
                    | spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource => {
                        ResolvedDuration::NoStatedDuration
                    }
                };
                builder
                    .instructions
                    .push(super::super::toolbox_emit::grant_play_permission(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        play_dur,
                        *cast_only,
                        *free,
                        any_color,
                    ));
                i += step;
                continue;
            }
        }
        // Lower GrantCastPermission (direct, Optional, or ForAsLongAs("remains exiled")) after a
        // prior move into Exile/Graveyard/Hand via ApplyContinuous(PlayPermission).
        {
            type GrantCandidate<'a> = (
                PlayerRefAst,
                bool,
                &'a SubjectRefAst,
                bool,
                &'a str,
                &'a Option<spellwake_oracle_compiler::ast::CountLimitAst>,
            );
            let grant_candidate: Option<GrantCandidate<'_>> = match &ins[i] {
                InstructionAst::GrantCastPermission {
                    player,
                    cast_only,
                    subject,
                    free,
                    duration,
                    count_limit,
                    alt_cost: None,
                } => Some((
                    *player,
                    *cast_only,
                    subject.as_ref(),
                    *free,
                    duration.as_str(),
                    count_limit,
                )),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::GrantCastPermission {
                            player,
                            cast_only,
                            subject,
                            free,
                            duration,
                            count_limit,
                            alt_cost: None,
                        },
                    ] => Some((
                        *player,
                        *cast_only,
                        subject.as_ref(),
                        *free,
                        duration.as_str(),
                        count_limit,
                    )),
                    _ => None,
                },
                InstructionAst::ForAsLongAs { condition, body }
                    if matches!(
                        condition.as_ref(),
                        ConditionAst::EventOccurredThisTurn { event }
                            if matches!(
                                event.trim().trim_end_matches('.').to_ascii_lowercase().as_str(),
                                "it remains exiled"
                                    | "that card remains exiled"
                                    | "those cards remain exiled"
                                    | "they remain exiled"
                            )
                    ) =>
                {
                    match body.as_slice() {
                        [
                            InstructionAst::GrantCastPermission {
                                player,
                                cast_only,
                                subject,
                                free,
                                duration: _,
                                count_limit,
                                alt_cost: None,
                            },
                        ] => Some((
                            *player,
                            *cast_only,
                            subject.as_ref(),
                            *free,
                            "while_exiled",
                            count_limit,
                        )),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((grant_player, cast_only, grant_subj, free, grant_dur, count_limit)) =
                grant_candidate
                && (count_limit.is_none()
                    || matches!(
                        count_limit,
                        Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
                    ))
                && let Some((resolved_dur, dur_any_color)) =
                    parse_grant_cast_duration_resolved(grant_dur)
            {
                // Verify the most recent non-transparent instruction populated last_moved_objects
                // in a zone PlayPermission can grant from (Exile/Graveyard/Hand).
                let playable_producer: Option<bool /* single_card_only */> = {
                    let mut found = None;
                    for op in builder.instructions.iter().rev() {
                        match op {
                            Instruction::ZoneOp(spec) => {
                                match &spec.destination {
                                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                                    | spellwake_engine::toolbox::ZoneDest::Graveyard { .. }
                                    | spellwake_engine::toolbox::ZoneDest::Hand => {
                                        let single = match &spec.source {
                                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                            slice: spellwake_engine::toolbox::QuerySlice::Top(n), ..
                                        } => **n == ValueExpr::literal(1),
                                        spellwake_engine::toolbox::ObjectQuery::Targets
                                        | spellwake_engine::toolbox::ObjectQuery::Source
                                        | spellwake_engine::toolbox::ObjectQuery::TriggerSubject
                                        | spellwake_engine::toolbox::ObjectQuery::AttachedHost
                                        | spellwake_engine::toolbox::ObjectQuery::Revealed
                                        | spellwake_engine::toolbox::ObjectQuery::LinkedTarget { .. } => true,
                                        _ => false,
                                    };
                                        found = Some(single);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            Instruction::ExileTopPlay {
                                count,
                                library_owner,
                                ..
                            } => {
                                found = Some(
                                    *count == ValueExpr::literal(1)
                                        && matches!(library_owner, AffectedPlayers::One(_)),
                                );
                                break;
                            }
                            Instruction::ForEachPlayerQuery { selector, body, .. }
                                if matches!(
                                    body.instructions.as_slice(),
                                    [Instruction::ZoneOp(_)]
                                ) =>
                            {
                                let single = matches!(
                                    selector.as_ref(),
                                    spellwake_engine::toolbox::PlayerSelector::Group(
                                        AffectedPlayers::One(_)
                                    )
                                ) && matches!(
                                    &body.instructions[0],
                                    Instruction::ZoneOp(spec)
                                        if matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Exile { .. })
                                            && matches!(
                                                &spec.selection,
                                                spellwake_engine::toolbox::ZoneSelection::Prompt { min, max, .. }
                                                    if **min == ValueExpr::literal(1) && **max == ValueExpr::literal(1)
                                            )
                                );
                                found = Some(single);
                                break;
                            }
                            Instruction::Discard { .. } => {
                                found = Some(false);
                                break;
                            }
                            Instruction::LookTopChooseExt { keep_dest, .. }
                            | Instruction::RevealUntilMatchExt {
                                match_dest: keep_dest,
                                ..
                            } if matches!(
                                keep_dest,
                                spellwake_engine::program::LookTopKeepDest::Exile { .. }
                                    | spellwake_engine::program::LookTopKeepDest::Graveyard
                                    | spellwake_engine::program::LookTopKeepDest::Hand
                            ) =>
                            {
                                found = Some(false);
                                break;
                            }
                            Instruction::MutatePlayer(spec) => match &spec.op {
                                spellwake_engine::toolbox::PlayerMutation::Mill(_) => {
                                    found = Some(false);
                                    break;
                                }
                                spellwake_engine::toolbox::PlayerMutation::ChooseOneLastMoved => {
                                    found = Some(true);
                                    break;
                                }
                                spellwake_engine::toolbox::PlayerMutation::AddMana(_)
                                | spellwake_engine::toolbox::PlayerMutation::BindResolutionX(_) => {
                                    continue;
                                }
                                _ => break,
                            },
                            Instruction::OptionalEffect { .. }
                            | Instruction::OptionalSequence { .. }
                            | Instruction::BranchIfFalse { .. }
                            | Instruction::BranchCondition { .. }
                            | Instruction::Jump { .. }
                            | Instruction::ChooseValue { .. }
                            | Instruction::AdjustLife(_) => continue,
                            Instruction::ApplyContinuous(spec)
                                if matches!(
                                    spec.payload,
                                    spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(_)
                                        | spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                                ) =>
                            {
                                continue;
                            }
                            _ => break,
                        }
                    }
                    found
                };
                if let Some(single_card_only) = playable_producer {
                    let is_any_number = matches!(
                        count_limit,
                        Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
                    );
                    let parsed_filter_opt: Option<Option<ObjectFilter>> = match grant_subj {
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                            let mut inner = f.as_ref().clone();
                            let before_len = inner.predicates.len();
                            inner.predicates.retain(|p| {
                                !matches!(
                                    p,
                                    spellwake_oracle_compiler::ast::FilterPredicateAst::PriorActionZoneRef { kind }
                                        if matches!(
                                            kind.trim().to_ascii_lowercase().as_str(),
                                            "from_among_those"
                                                | "from_among_those_cards"
                                                | "among_them"
                                                | "among them"
                                                | "from_among_them"
                                                | "from among them"
                                                | "exiled_this_way"
                                                | "cards_exiled_this_way"
                                                | "among_the_cards_exiled_this_way"
                                                | "from_among_the_cards_exiled_this_way"
                                                | "milled_this_way"
                                                | "discarded_this_way"
                                                | "revealed_this_way"
                                        )
                                ) && !matches!(
                                    p,
                                    spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
                                        spellwake_oracle_compiler::ast::PriorActionRefKind::ExiledThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::DiscardedThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::PutIntoGraveyardThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay
                                    )
                                )
                            });
                            if inner.predicates.len() < before_len {
                                inner.zone = None;
                                if is_unrestricted_filter(&inner) {
                                    Some(None)
                                } else {
                                    lower_object_filter(&inner, vocabulary)
                                        .ok()
                                        .filter(|o| {
                                            !o.has_target_player() && !o.uses_target_permanent()
                                        })
                                        .map(Some)
                                }
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::Group(f) if is_any_number || single_card_only => {
                            let mut inner = f.as_ref().clone();
                            let before_len = inner.predicates.len();
                            inner.predicates.retain(|p| {
                                !matches!(
                                    p,
                                    spellwake_oracle_compiler::ast::FilterPredicateAst::PriorActionZoneRef { kind }
                                        if matches!(
                                            kind.trim().to_ascii_lowercase().as_str(),
                                            "from_among_those"
                                                | "from_among_those_cards"
                                                | "among_them"
                                                | "among them"
                                                | "from_among_them"
                                                | "from among them"
                                                | "exiled_this_way"
                                                | "cards_exiled_this_way"
                                                | "among_the_cards_exiled_this_way"
                                                | "from_among_the_cards_exiled_this_way"
                                                | "milled_this_way"
                                                | "discarded_this_way"
                                                | "revealed_this_way"
                                        )
                                ) && !matches!(
                                    p,
                                    spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
                                        spellwake_oracle_compiler::ast::PriorActionRefKind::ExiledThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::DiscardedThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::PutIntoGraveyardThisWay
                                            | spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay
                                    )
                                )
                            });
                            if inner.predicates.len() < before_len {
                                inner.zone = None;
                                if is_unrestricted_filter(&inner) {
                                    Some(None)
                                } else {
                                    lower_object_filter(&inner, vocabulary)
                                        .ok()
                                        .filter(|o| {
                                            !o.has_target_player() && !o.uses_target_permanent()
                                        })
                                        .map(Some)
                                }
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::Anaphora(a)
                            if (builder.target == BodyTarget::None
                                || matches!(
                                    builder.target,
                                    BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                                ))
                                && matches!(
                                    a.trim().to_ascii_lowercase().as_str(),
                                    "it" | "them"
                                        | "that card"
                                        | "those cards"
                                        | "the exiled card"
                                        | "the exiled cards"
                                        | "cards exiled this way"
                                        | "the cards exiled this way"
                                        | "the milled cards"
                                        | "the discarded card"
                                        | "the discarded cards"
                                        | "the chosen card"
                                ) =>
                        {
                            Some(None)
                        }
                        _ => None,
                    };
                    if let Some(opt_filter) = parsed_filter_opt {
                        let mut any_color = dur_any_color;
                        let mut step = 1usize;
                        if i + 1 < ins.len() && is_any_color_spend_restriction(&ins[i + 1]) {
                            any_color = true;
                            step = 2;
                        }
                        let group_players = match grant_player {
                            PlayerRefAst::EachPlayer => Some(AffectedPlayers::EachPlayer),
                            PlayerRefAst::EachOpponent => Some(AffectedPlayers::EachOpponent),
                            PlayerRefAst::EachOtherPlayer => Some(AffectedPlayers::EachOtherPlayer),
                            _ => None,
                        };
                        let play_selector = match opt_filter {
                            Some(f) => spellwake_engine::toolbox::ObjectQuery::filtered(
                                spellwake_engine::toolbox::ObjectQuery::LastMoved,
                                f,
                            ),
                            None => spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        };
                        if let Some(players) = group_players {
                            push_for_each_player(
                                players,
                                vec![super::super::toolbox_emit::grant_play_permission(
                                    play_selector,
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::LoopPlayer,
                                    ),
                                    resolved_dur,
                                    cast_only,
                                    free,
                                    any_color,
                                )],
                                builder,
                            );
                            i += step;
                            continue;
                        }
                        let mut p_probe = builder.clone();
                        if let Ok(rp) = lower_resolving_player_in_context(
                            grant_player,
                            &mut p_probe,
                            in_trigger,
                        ) {
                            *builder = p_probe;
                            builder.instructions.push(
                                super::super::toolbox_emit::grant_play_permission(
                                    play_selector,
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                    resolved_dur,
                                    cast_only,
                                    free,
                                    any_color,
                                ),
                            );
                            i += step;
                            continue;
                        }
                    }
                }
            }
        }
        // Fuse paired drain life:
        // "Each opponent loses N life and you gain N life" / "Target player loses N life and you gain N life"
        if i + 1 < ins.len()
            && let InstructionAst::ChangeLife {
                player: loser,
                delta: lose_amt,
                gain: false,
            } = &ins[i]
            && let InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: gain_amt,
                gain: true,
            } = &ins[i + 1]
            && (lose_amt == gain_amt
                || (matches!(gain_amt.as_ref(), ValueExprAst::PriorResult)
                    && !matches!(loser, PlayerRefAst::EachOpponent)))
        {
            let amt = lower_value_expr(resolve_x(lose_amt, bound_x), vocabulary)?;
            match loser {
                PlayerRefAst::EachOpponent => {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::drain_life(
                            spellwake_engine::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ),
                            amt,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            false,
                        ));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::drain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                            amt,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            true,
                        ));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::drain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                            amt,
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            true,
                        ));
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse Optional([RevealUntil(You, filter, n)]) + ChooseAndKeep -> OptionalEffect + RevealUntilMatch (e.g. Spinner of Souls)
        if i + 1 < ins.len()
            && let InstructionAst::Optional(opt_body) = &ins[i]
            && let [
                InstructionAst::RevealUntil {
                    player: PlayerRefAst::You,
                    filter: until_filter,
                    count,
                },
            ] = opt_body.as_slice()
            && let ValueExprAst::Const(n_cnt) = count.as_ref()
            && let Ok(count_u8) = u8::try_from(*n_cnt)
            && count_u8 >= 1
            && let InstructionAst::ChooseAndKeep {
                pick,
                keep_to,
                rest_to,
            } = &ins[i + 1]
            && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
        {
            let pick_low = pick.trim().to_ascii_lowercase();
            let keep_low = keep_to.to_ascii_lowercase();
            let rest_low = rest_to.to_ascii_lowercase();
            let to_hand = keep_low.contains("hand");
            let to_battlefield = keep_low.contains("battlefield");
            let enters_tapped = to_battlefield && keep_low.contains("tapped");
            let keep_all = pick_low.contains("all cards revealed")
                || pick_low.contains("all cards exiled")
                || pick_low == "all of them"
                || pick_low == "them";
            let rest_bottom = rest_low.contains("bottom");
            let rest_gy = rest_low.contains("graveyard");
            let rest_exile = rest_low.contains("exile");
            let random_rest = rest_low.contains("random order");
            if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile || keep_all) {
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
                builder.instructions.push(Instruction::RevealUntilMatchExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj_filter),
                    count: ValueExpr::Literal(i64::from(count_u8).into()),
                    match_dest: if to_battlefield {
                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                            tapped: enters_tapped,
                            attacking: false,
                        }
                    } else {
                        spellwake_engine::program::LookTopKeepDest::Hand
                    },
                    rest_dest: if rest_gy {
                        spellwake_engine::program::LookTopRestDest::Graveyard
                    } else if rest_exile {
                        spellwake_engine::program::LookTopRestDest::Exile
                    } else if rest_bottom && random_rest {
                        spellwake_engine::program::LookTopRestDest::BottomRandom
                    } else if rest_bottom {
                        spellwake_engine::program::LookTopRestDest::BottomOrder
                    } else {
                        spellwake_engine::program::LookTopRestDest::BottomRandom
                    },
                    keep_all_revealed: keep_all,
                    optional_keep: false,
                    exile_instead_of_reveal: false,
                });
                i += 2;
                continue;
            }
        }
        // Fuse Optional([RevealUntil(You, filter, n)]) + IfPreviousSucceeded([keep, rest]) -> OptionalEffect + RevealUntilMatch (e.g. Avenging Druid)
        if i + 1 < ins.len()
            && let InstructionAst::Optional(opt_body) = &ins[i]
            && let [
                InstructionAst::RevealUntil {
                    player: PlayerRefAst::You,
                    filter: until_filter,
                    count,
                },
            ] = opt_body.as_slice()
            && let ValueExprAst::Const(n_cnt) = count.as_ref()
            && let Ok(count_u8) = u8::try_from(*n_cnt)
            && count_u8 >= 1
            && let InstructionAst::IfPreviousSucceeded(then_body) = &ins[i + 1]
            && then_body.len() == 2
            && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
        {
            let keep_info = match &then_body[0] {
                InstructionAst::MoveZone { subject, to }
                    if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                {
                    let low = to.to_ascii_lowercase();
                    if low.contains("battlefield") {
                        Some((true, low.contains("tapped")))
                    } else if low.contains("hand") {
                        Some((false, false))
                    } else {
                        None
                    }
                }
                InstructionAst::ReturnZone {
                    subject,
                    to,
                    tapped,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    ..
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                    let low = to.to_ascii_lowercase();
                    if low.contains("battlefield") {
                        Some((true, *tapped || low.contains("tapped")))
                    } else if low.contains("hand") {
                        Some((false, false))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let rest_info = match &then_body[1] {
                InstructionAst::MoveZone { subject, to }
                    if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if {
                        let l = a.to_ascii_lowercase();
                        l.contains("rest") || l.contains("other")
                    }) =>
                {
                    let low = to.to_ascii_lowercase();
                    Some((
                        low.contains("bottom"),
                        low.contains("graveyard"),
                        low.contains("exile"),
                        low.contains("random order"),
                        false,
                    ))
                }
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                } => Some((false, false, false, false, true)),
                InstructionAst::ShuffleIntoLibrary { subject }
                    if matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if {
                        let l = a.to_ascii_lowercase();
                        l.contains("rest") || l.contains("other")
                    }) =>
                {
                    Some((false, false, false, false, true))
                }
                _ => None,
            };
            if let (
                Some((to_battlefield, enters_tapped)),
                Some((rest_bottom, rest_gy, rest_exile, random_rest, do_shuffle)),
            ) = (keep_info, rest_info)
                && (rest_bottom || rest_gy || rest_exile || do_shuffle)
            {
                if do_shuffle {
                    builder.instructions.push(Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 2,
                    });
                } else {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                }
                builder.instructions.push(Instruction::RevealUntilMatchExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    filter: Box::new(obj_filter),
                    count: ValueExpr::Literal(i64::from(count_u8).into()),
                    match_dest: if to_battlefield {
                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                            tapped: enters_tapped,
                            attacking: false,
                        }
                    } else {
                        spellwake_engine::program::LookTopKeepDest::Hand
                    },
                    rest_dest: if rest_gy {
                        spellwake_engine::program::LookTopRestDest::Graveyard
                    } else if rest_exile {
                        spellwake_engine::program::LookTopRestDest::Exile
                    } else if rest_bottom && random_rest {
                        spellwake_engine::program::LookTopRestDest::BottomRandom
                    } else if rest_bottom {
                        spellwake_engine::program::LookTopRestDest::BottomOrder
                    } else {
                        spellwake_engine::program::LookTopRestDest::BottomRandom
                    },
                    keep_all_revealed: false,
                    optional_keep: false,
                    exile_instead_of_reveal: false,
                });
                if do_shuffle {
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                }
                i += 2;
                continue;
            }
        }
        // Fuse RevealUntil / ExileOrRevealTopUntil(You, filter, n) + ChooseAndKeep / MoveZone (+ rest MoveZone or Shuffle) -> RevealUntilMatch
        let until_header: Option<(&ObjectFilterAst, u8, bool)> = match &ins[i] {
            InstructionAst::RevealUntil {
                player: PlayerRefAst::You,
                filter,
                count,
            } => match count.as_ref() {
                ValueExprAst::Const(n_cnt) if *n_cnt >= 1 => u8::try_from(*n_cnt)
                    .ok()
                    .map(|c| (filter.as_ref(), c, false)),
                _ => None,
            },
            InstructionAst::ExileOrRevealTopUntil {
                exile,
                library,
                filter,
            } if matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            ) =>
            {
                Some((filter.as_ref(), 1, *exile))
            }
            _ => None,
        };
        if let Some((until_filter, count_u8, exile_instead)) = until_header
            && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
        {
            // Pattern A: 2-step RevealUntil + ChooseAndKeep (e.g. Hermit Druid)
            if i + 1 < ins.len()
                && let InstructionAst::ChooseAndKeep {
                    pick,
                    keep_to,
                    rest_to,
                } = &ins[i + 1]
            {
                let pick_low = pick.trim().to_ascii_lowercase();
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let keep_all = pick_low.contains("all cards revealed")
                    || pick_low.contains("all cards exiled")
                    || pick_low == "all of them"
                    || pick_low == "them";
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_shuffle_inline = rest_low.contains("shuffle");
                let trailing_shuffle = !rest_bottom
                    && !rest_gy
                    && !rest_exile
                    && !rest_shuffle_inline
                    && (rest_low.trim().is_empty() || rest_low.contains("library"))
                    && i + 2 < ins.len()
                    && matches!(
                        &ins[i + 2],
                        InstructionAst::ShuffleLibrary {
                            player: PlayerRefAst::You
                        }
                    );
                let random_rest = rest_low.contains("random order");
                if (to_hand || to_battlefield)
                    && (rest_bottom
                        || rest_gy
                        || rest_exile
                        || rest_shuffle_inline
                        || trailing_shuffle
                        || keep_all)
                {
                    builder.instructions.push(Instruction::RevealUntilMatchExt {
                        library_owner: ResolvingPlayer::Controller,
                        chooser: ResolvingPlayer::Controller,
                        filter: Box::new(obj_filter),
                        count: ValueExpr::Literal(i64::from(count_u8).into()),
                        match_dest: if to_battlefield {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped: enters_tapped,
                                attacking: false,
                            }
                        } else {
                            spellwake_engine::program::LookTopKeepDest::Hand
                        },
                        rest_dest: if rest_gy {
                            spellwake_engine::program::LookTopRestDest::Graveyard
                        } else if rest_exile {
                            spellwake_engine::program::LookTopRestDest::Exile
                        } else if rest_bottom && random_rest {
                            spellwake_engine::program::LookTopRestDest::BottomRandom
                        } else if rest_bottom {
                            spellwake_engine::program::LookTopRestDest::BottomOrder
                        } else {
                            spellwake_engine::program::LookTopRestDest::BottomRandom
                        },
                        keep_all_revealed: keep_all,
                        optional_keep: false,
                        exile_instead_of_reveal: exile_instead,
                    });
                    if rest_shuffle_inline || trailing_shuffle {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                    }
                    i += if trailing_shuffle { 3 } else { 2 };
                    continue;
                }
            }
            // Pattern B: 2-, 3-, or 4-step RevealUntil + [Optional?]MoveZone/ReturnZone(that card / all revealed -> hand/bf)
            // (+ optional 1 intermediate non-targeted DealDamage/ChangeLife) + [MoveZone(rest -> bottom/gy/exile) | ShuffleLibrary],
            // or reversed order RevealUntil + MoveZone(rest) + [Optional?]MoveZone/ReturnZone(that card -> hand/bf).
            if i + 1 < ins.len() {
                fn extract_keep_move(
                    op: &InstructionAst,
                ) -> (Option<&SubjectRefAst>, Option<&str>, bool, bool) {
                    match op {
                        InstructionAst::MoveZone { subject, to } => {
                            (Some(subject.as_ref()), Some(to.as_str()), false, false)
                        }
                        InstructionAst::ReturnZone {
                            subject,
                            to,
                            tapped,
                            transformed: false,
                            with_counter: None,
                            attached_to: None,
                            ..
                        } => (Some(subject.as_ref()), Some(to.as_str()), false, *tapped),
                        InstructionAst::Optional(inner) => match inner.as_slice() {
                            [InstructionAst::MoveZone { subject, to }] => {
                                (Some(subject.as_ref()), Some(to.as_str()), true, false)
                            }
                            [
                                InstructionAst::ReturnZone {
                                    subject,
                                    to,
                                    tapped,
                                    transformed: false,
                                    with_counter: None,
                                    attached_to: None,
                                    ..
                                },
                            ] => (Some(subject.as_ref()), Some(to.as_str()), true, *tapped),
                            _ => (None, None, false, false),
                        },
                        _ => (None, None, false, false),
                    }
                }
                let is_rest_move = |op: &InstructionAst| -> Option<(bool, bool, bool, bool)> {
                    if let InstructionAst::MoveZone {
                        subject: rest_subj,
                        to: rest_to,
                    } = op
                        && (matches!(
                            rest_subj.as_ref(),
                            SubjectRefAst::Anaphora(r) if {
                                let rl = r.to_ascii_lowercase();
                                rl.contains("rest") || rl.contains("other") || rl == "them" || rl.contains("exiled")
                            }
                        ) || matches!(
                            rest_subj.as_ref(),
                            SubjectRefAst::All(f) | SubjectRefAst::Group(f)
                                if f.any_types.is_empty()
                                    && f.forbidden_types.is_empty()
                                    && f.predicates.iter().any(|p| matches!(
                                        p,
                                        spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
                                            spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay
                                                | spellwake_oracle_compiler::ast::PriorActionRefKind::ExiledThisWay
                                        )
                                    ))
                        ))
                    {
                        let rest_low = rest_to.to_ascii_lowercase();
                        let rest_bottom = rest_low.contains("bottom");
                        let rest_gy = rest_low.contains("graveyard");
                        let rest_exile = rest_low.contains("exile");
                        let random_rest = rest_low.contains("random order");
                        if rest_bottom || rest_gy || rest_exile {
                            return Some((rest_bottom, rest_gy, rest_exile, random_rest));
                        }
                    }
                    None
                };
                // Reversed 3-step: RevealUntil + MoveZone(rest) + [Optional?]MoveZone/ReturnZone(that card -> hand/bf)
                if i + 2 < ins.len()
                    && let Some((rest_bottom, rest_gy, rest_exile, random_rest)) =
                        is_rest_move(&ins[i + 1])
                    && let (
                        Some(SubjectRefAst::Anaphora(a)),
                        Some(keep_to),
                        optional_put,
                        tapped_flag,
                    ) = extract_keep_move(&ins[i + 2])
                {
                    let a_low = a.trim().to_ascii_lowercase();
                    let single_card = a_low.contains("that card")
                        || a_low == "it"
                        || a_low.contains("revealed card")
                        || a_low.contains("exiled card");
                    let keep_low = keep_to.to_ascii_lowercase();
                    let to_hand = keep_low.contains("hand");
                    let to_battlefield = keep_low.contains("battlefield");
                    let enters_tapped =
                        to_battlefield && (tapped_flag || keep_low.contains("tapped"));
                    if (to_hand || to_battlefield) && single_card {
                        builder.instructions.push(Instruction::RevealUntilMatchExt {
                            library_owner: ResolvingPlayer::Controller,
                            chooser: ResolvingPlayer::Controller,
                            filter: Box::new(obj_filter),
                            count: ValueExpr::Literal(i64::from(count_u8).into()),
                            match_dest: if to_battlefield {
                                spellwake_engine::program::LookTopKeepDest::Battlefield {
                                    tapped: enters_tapped,
                                    attacking: false,
                                }
                            } else {
                                spellwake_engine::program::LookTopKeepDest::Hand
                            },
                            rest_dest: if rest_gy {
                                spellwake_engine::program::LookTopRestDest::Graveyard
                            } else if rest_exile {
                                spellwake_engine::program::LookTopRestDest::Exile
                            } else if rest_bottom && random_rest {
                                spellwake_engine::program::LookTopRestDest::BottomRandom
                            } else if rest_bottom {
                                spellwake_engine::program::LookTopRestDest::BottomOrder
                            } else {
                                spellwake_engine::program::LookTopRestDest::BottomRandom
                            },
                            keep_all_revealed: false,
                            optional_keep: optional_put,
                            exile_instead_of_reveal: exile_instead,
                        });
                        i += 3;
                        continue;
                    }
                }
                let (move_subj, move_to, optional_put, tapped_flag) =
                    extract_keep_move(&ins[i + 1]);
                if let (Some(SubjectRefAst::Anaphora(a)), Some(keep_to)) = (move_subj, move_to) {
                    let a_low = a.trim().to_ascii_lowercase();
                    let keep_all = a_low.contains("all cards revealed")
                        || a_low.contains("all revealed")
                        || a_low.contains("all cards exiled")
                        || a_low.contains("all exiled")
                        || a_low == "them"
                        || a_low == "those cards";
                    let single_card = a_low.contains("that card")
                        || a_low == "it"
                        || a_low.contains("revealed card")
                        || a_low.contains("exiled card");
                    let keep_low = keep_to.to_ascii_lowercase();
                    let to_hand = keep_low.contains("hand");
                    let to_battlefield = keep_low.contains("battlefield");
                    let enters_tapped =
                        to_battlefield && (tapped_flag || keep_low.contains("tapped"));
                    if (to_hand || to_battlefield) && keep_all && !optional_put {
                        builder.instructions.push(Instruction::RevealUntilMatchExt {
                            library_owner: ResolvingPlayer::Controller,
                            chooser: ResolvingPlayer::Controller,
                            filter: Box::new(obj_filter),
                            count: ValueExpr::Literal(i64::from(count_u8).into()),
                            match_dest: if to_battlefield {
                                spellwake_engine::program::LookTopKeepDest::Battlefield {
                                    tapped: enters_tapped,
                                    attacking: false,
                                }
                            } else {
                                spellwake_engine::program::LookTopKeepDest::Hand
                            },
                            rest_dest: spellwake_engine::program::LookTopRestDest::BottomRandom,
                            keep_all_revealed: true,
                            optional_keep: false,
                            exile_instead_of_reveal: exile_instead,
                        });
                        i += 2;
                        continue;
                    }
                    if (to_hand || to_battlefield)
                        && (single_card || (keep_all && count_u8 > 1))
                        && i + 2 < ins.len()
                    {
                        if let Some((rest_bottom, rest_gy, rest_exile, random_rest)) =
                            is_rest_move(&ins[i + 2])
                        {
                            builder.instructions.push(Instruction::RevealUntilMatchExt {
                                library_owner: ResolvingPlayer::Controller,
                                chooser: ResolvingPlayer::Controller,
                                filter: Box::new(obj_filter),
                                count: ValueExpr::Literal(i64::from(count_u8).into()),
                                match_dest: if to_battlefield {
                                    spellwake_engine::program::LookTopKeepDest::Battlefield {
                                        tapped: enters_tapped,
                                        attacking: false,
                                    }
                                } else {
                                    spellwake_engine::program::LookTopKeepDest::Hand
                                },
                                rest_dest: if rest_gy {
                                    spellwake_engine::program::LookTopRestDest::Graveyard
                                } else if rest_exile {
                                    spellwake_engine::program::LookTopRestDest::Exile
                                } else if rest_bottom && random_rest {
                                    spellwake_engine::program::LookTopRestDest::BottomRandom
                                } else if rest_bottom {
                                    spellwake_engine::program::LookTopRestDest::BottomOrder
                                } else {
                                    spellwake_engine::program::LookTopRestDest::BottomRandom
                                },
                                keep_all_revealed: false,
                                optional_keep: optional_put,
                                exile_instead_of_reveal: exile_instead,
                            });
                            i += 3;
                            continue;
                        }
                        if matches!(
                            &ins[i + 2],
                            InstructionAst::ShuffleLibrary {
                                player: PlayerRefAst::You,
                            }
                        ) || matches!(
                            &ins[i + 2],
                            InstructionAst::ShuffleIntoLibrary { subject }
                                if matches!(
                                    subject.as_ref(),
                                    SubjectRefAst::Anaphora(r)
                                        if {
                                            let rl = r.to_ascii_lowercase();
                                            rl.contains("rest") || rl.contains("other")
                                        }
                                )
                        ) {
                            builder.instructions.push(Instruction::RevealUntilMatchExt {
                                library_owner: ResolvingPlayer::Controller,
                                chooser: ResolvingPlayer::Controller,
                                filter: Box::new(obj_filter),
                                count: ValueExpr::Literal(i64::from(count_u8).into()),
                                match_dest: if to_battlefield {
                                    spellwake_engine::program::LookTopKeepDest::Battlefield {
                                        tapped: enters_tapped,
                                        attacking: false,
                                    }
                                } else {
                                    spellwake_engine::program::LookTopKeepDest::Hand
                                },
                                rest_dest: spellwake_engine::program::LookTopRestDest::BottomRandom,
                                keep_all_revealed: false,
                                optional_keep: optional_put,
                                exile_instead_of_reveal: exile_instead,
                            });
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                            i += 3;
                            continue;
                        }
                        // 4-step with 1 intermediate DealDamage or ChangeLife before MoveZone(rest) / ShuffleLibrary
                        if i + 3 < ins.len()
                            && matches!(
                                &ins[i + 2],
                                InstructionAst::DealDamage { .. }
                                    | InstructionAst::ChangeLife { .. }
                            )
                            && (is_rest_move(&ins[i + 3]).is_some()
                                || matches!(
                                    &ins[i + 3],
                                    InstructionAst::ShuffleLibrary {
                                        player: PlayerRefAst::You,
                                    }
                                ))
                        {
                            let mut follow_probe = builder.clone();
                            let prev_len = follow_probe.instructions.len();
                            if lower_instruction(
                                &ins[i + 2],
                                &mut follow_probe,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )
                            .is_ok()
                                && follow_probe.target == builder.target
                            {
                                let extra_ops = follow_probe.instructions.split_off(prev_len);
                                if let Some((rest_bottom, rest_gy, rest_exile, random_rest)) =
                                    is_rest_move(&ins[i + 3])
                                {
                                    builder.instructions.push(Instruction::RevealUntilMatchExt {
                                        library_owner: ResolvingPlayer::Controller,
                                        chooser: ResolvingPlayer::Controller,
                                        filter: Box::new(obj_filter),
                                        count: ValueExpr::Literal(i64::from(count_u8).into()),
                                        match_dest: if to_battlefield {
                                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                                tapped: enters_tapped,
                                                attacking: false,
                                            }
                                        } else {
                                            spellwake_engine::program::LookTopKeepDest::Hand
                                        },
                                        rest_dest: if rest_gy {
                                            spellwake_engine::program::LookTopRestDest::Graveyard
                                        } else if rest_exile {
                                            spellwake_engine::program::LookTopRestDest::Exile
                                        } else if rest_bottom && random_rest {
                                            spellwake_engine::program::LookTopRestDest::BottomRandom
                                        } else if rest_bottom {
                                            spellwake_engine::program::LookTopRestDest::BottomOrder
                                        } else {
                                            spellwake_engine::program::LookTopRestDest::BottomRandom
                                        },
                                        keep_all_revealed: false,
                                        optional_keep: optional_put,
                                        exile_instead_of_reveal: exile_instead,
                                    });
                                    builder.instructions.extend(extra_ops);
                                    i += 4;
                                    continue;
                                }
                                builder.instructions.push(Instruction::RevealUntilMatchExt {
                                    library_owner: ResolvingPlayer::Controller,
                                    chooser: ResolvingPlayer::Controller,
                                    filter: Box::new(obj_filter),
                                    count: ValueExpr::Literal(i64::from(count_u8).into()),
                                    match_dest: if to_battlefield {
                                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                                            tapped: enters_tapped,
                                            attacking: false,
                                        }
                                    } else {
                                        spellwake_engine::program::LookTopKeepDest::Hand
                                    },
                                    rest_dest:
                                        spellwake_engine::program::LookTopRestDest::BottomRandom,
                                    keep_all_revealed: false,
                                    optional_keep: optional_put,
                                    exile_instead_of_reveal: exile_instead,
                                });
                                builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                                builder.instructions.extend(extra_ops);
                                i += 4;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        // Fuse RevealUntil(single or EachOpponent/EachPlayer, filter, count) + single MoveZone/Exile/ReturnZone(all revealed / those cards -> graveyard/hand/exile/bottom)
        // e.g. Balustrade Spy, Undercity Informer, Mind Grind, Consuming Aberration
        if i + 1 < ins.len()
            && let InstructionAst::RevealUntil {
                player: rev_player,
                filter: rev_filter,
                count: rev_count,
            } = &ins[i]
            && let Some((rev_subj, rev_dest_opt, bottom_random_opt)) = (match &ins[i + 1] {
                InstructionAst::MoveZone { subject, to }
                | InstructionAst::ReturnZone {
                    subject,
                    to,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    ..
                } => {
                    let to_low = to.trim().to_ascii_lowercase();
                    if to_low.contains("graveyard") {
                        Some((
                            subject.as_ref(),
                            Some((
                                spellwake_engine::program::LookTopKeepDest::Graveyard,
                                spellwake_engine::program::LookTopRestDest::Graveyard,
                            )),
                            None,
                        ))
                    } else if to_low.contains("hand") {
                        Some((
                            subject.as_ref(),
                            Some((
                                spellwake_engine::program::LookTopKeepDest::Hand,
                                spellwake_engine::program::LookTopRestDest::Hand,
                            )),
                            None,
                        ))
                    } else if to_low.contains("exile") {
                        Some((
                            subject.as_ref(),
                            Some((
                                spellwake_engine::program::LookTopKeepDest::Exile {
                                    playable: false,
                                    free: false,
                                    cast_only: false,
                                    any_color: false,
                                    duration: ResolvedDuration::NoStatedDuration,
                                },
                                spellwake_engine::program::LookTopRestDest::Exile,
                            )),
                            None,
                        ))
                    } else if to_low.contains("bottom") && to_low.contains("library") {
                        Some((subject.as_ref(), None, Some(to_low.contains("random"))))
                    } else {
                        None
                    }
                }
                InstructionAst::Exile {
                    actor: None | Some(PlayerRefAst::You | PlayerRefAst::ThatPlayer),
                    subject,
                    random: false,
                    face_down: false,
                    into_pile: false,
                } => Some((
                    subject.as_ref(),
                    Some((
                        spellwake_engine::program::LookTopKeepDest::Exile {
                            playable: false,
                            free: false,
                            cast_only: false,
                            any_color: false,
                            duration: ResolvedDuration::NoStatedDuration,
                        },
                        spellwake_engine::program::LookTopRestDest::Exile,
                    )),
                    None,
                )),
                _ => None,
            })
            && {
                match rev_subj {
                    SubjectRefAst::Anaphora(a) => matches!(
                        a.trim().to_ascii_lowercase().as_str(),
                        "those cards"
                            | "all cards revealed this way"
                            | "all of those cards"
                            | "all of them"
                            | "them"
                            | "the revealed cards"
                    ),
                    SubjectRefAst::All(f) | SubjectRefAst::Group(f) => f
                        .predicates
                        .iter()
                        .any(|p| matches!(p, spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay)))
                        && f.any_types.is_empty()
                        && f.forbidden_types.is_empty(),
                    _ => false,
                }
            }
            && let Ok(obj_filter) = lower_object_filter(rev_filter, vocabulary)
            && let Ok(cnt_expr) = lower_value_expr_in_context(
                resolve_x(rev_count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )
        {
            if let Some(is_random) = bottom_random_opt {
                let p_sel_opt = match *rev_player {
                    PlayerRefAst::EachOpponent => Some((
                        None,
                        spellwake_engine::toolbox::PlayerSelector::Group(
                            AffectedPlayers::EachOpponent,
                        ),
                    )),
                    PlayerRefAst::EachPlayer => Some((
                        None,
                        spellwake_engine::toolbox::PlayerSelector::Group(
                            AffectedPlayers::EachPlayer,
                        ),
                    )),
                    PlayerRefAst::EachOtherPlayer => Some((
                        None,
                        spellwake_engine::toolbox::PlayerSelector::Group(
                            AffectedPlayers::EachOtherPlayer,
                        ),
                    )),
                    other => {
                        let mut p_probe = builder.clone();
                        lower_resolving_player_in_context(other, &mut p_probe, in_trigger)
                            .ok()
                            .map(|rp| {
                                (
                                    (p_probe.target != builder.target).then_some(p_probe.target),
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                                )
                            })
                    }
                };
                if let Some((req_bt, p_sel)) = p_sel_opt {
                    if let Some(bt) = req_bt {
                        builder.bind_target(bt)?;
                    }
                    let pos = if is_random {
                        spellwake_engine::toolbox::LibraryOrderPos::BottomRandom
                    } else {
                        spellwake_engine::toolbox::LibraryOrderPos::Bottom
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                        p_sel,
                                    )),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::UntilMatch {
                                        filter: Box::new(obj_filter),
                                        count: Box::new(cnt_expr),
                                        include_non_matches: true,
                                    },
                                },
                                spellwake_engine::toolbox::ZoneDest::Library { position: pos },
                            ),
                        ),
                    );
                    i += 2;
                    continue;
                }
            }
            if let Some((match_dest, rest_dest)) = rev_dest_opt {
                let group_opt = match *rev_player {
                    PlayerRefAst::EachOpponent => Some(AffectedPlayers::EachOpponent),
                    PlayerRefAst::EachPlayer => Some(AffectedPlayers::EachPlayer),
                    PlayerRefAst::EachOtherPlayer => Some(AffectedPlayers::EachOtherPlayer),
                    _ => None,
                };
                if let Some(group_players) = group_opt {
                    push_for_each_player(
                        group_players,
                        vec![Instruction::RevealUntilMatchExt {
                            library_owner: ResolvingPlayer::LoopPlayer,
                            chooser: ResolvingPlayer::Controller,
                            filter: Box::new(obj_filter),
                            count: cnt_expr,
                            match_dest,
                            rest_dest,
                            keep_all_revealed: true,
                            optional_keep: false,
                            exile_instead_of_reveal: false,
                        }],
                        builder,
                    );
                    i += 2;
                    continue;
                }
                let mut p_probe = builder.clone();
                if let Ok(library_owner) =
                    lower_resolving_player_in_context(*rev_player, &mut p_probe, in_trigger)
                {
                    *builder = p_probe;
                    builder.instructions.push(Instruction::RevealUntilMatchExt {
                        library_owner,
                        chooser: ResolvingPlayer::Controller,
                        filter: Box::new(obj_filter),
                        count: cnt_expr,
                        match_dest,
                        rest_dest,
                        keep_all_revealed: true,
                        optional_keep: false,
                        exile_instead_of_reveal: false,
                    });
                    i += 2;
                    continue;
                }
            }
        }
        // Generalized RevealUntil / ExileOrRevealTopUntil -> RevealUntilMatchExt
        // (supports arbitrary library_owner, dynamic count, BindX wrapper, and extended match_dest / rest_dest).
        if i + 1 < ins.len() {
            let (until_head, eff_bound_x) = match &ins[i] {
                InstructionAst::BindX { x_expr, body } if body.len() == 1 => {
                    (&body[0], Some(x_expr.as_ref()))
                }
                other => (other, bound_x),
            };
            let ext_until_info: Option<(
                Option<BodyTarget>,
                ResolvingPlayer,
                &ObjectFilterAst,
                ValueExpr,
                bool,
            )> = match until_head {
                InstructionAst::RevealUntil {
                    player,
                    filter,
                    count,
                } => {
                    let mut p_probe = builder.clone();
                    let owner_opt =
                        lower_resolving_player_in_context(*player, &mut p_probe, in_trigger).ok();
                    let req_bt = (p_probe.target != builder.target).then_some(p_probe.target);
                    let cnt_opt = lower_value_expr_in_context(
                        resolve_x(count, eff_bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )
                    .ok();
                    match (owner_opt, cnt_opt) {
                        (Some(owner), Some(cnt)) => {
                            Some((req_bt, owner, filter.as_ref(), cnt, false))
                        }
                        _ => None,
                    }
                }
                InstructionAst::ExileOrRevealTopUntil {
                    exile,
                    library,
                    filter,
                } => parse_look_top_library_owner(library, builder, in_trigger).map(
                    |(req_bt, owner)| {
                        (
                            req_bt,
                            owner,
                            filter.as_ref(),
                            ValueExpr::literal(1),
                            *exile,
                        )
                    },
                ),
                _ => None,
            };
            if let Some((req_bt, library_owner, until_filter, cnt_expr, is_exile_until)) =
                ext_until_info
                && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
            {
                let push_shuffle_for_owner =
                    |ops: &mut Vec<Instruction>, owner: ResolvingPlayer| {
                        if owner == ResolvingPlayer::Controller {
                            ops.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                        } else {
                            ops.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(owner));
                        }
                    };
                let parse_grant_keep =
                    |op: &InstructionAst| -> Option<spellwake_engine::program::LookTopKeepDest> {
                        let (grant_op, extra_any_color) = match op {
                            g @ InstructionAst::GrantCastPermission { .. } => (g, false),
                            InstructionAst::Optional(inner) => match inner.as_slice() {
                                [g @ InstructionAst::GrantCastPermission { .. }] => (g, false),
                                [g @ InstructionAst::GrantCastPermission { .. }, mana_rule]
                                    if is_any_color_spend_restriction(mana_rule) =>
                                {
                                    (g, true)
                                }
                                _ => return None,
                            },
                            _ => return None,
                        };
                        let InstructionAst::GrantCastPermission {
                            player: PlayerRefAst::You,
                            cast_only,
                            subject,
                            free,
                            duration,
                            count_limit,
                            alt_cost: None,
                        } = grant_op
                        else {
                            return None;
                        };
                        if !(count_limit.is_none()
                            || matches!(
                                count_limit,
                                Some(spellwake_oracle_compiler::ast::CountLimitAst::AnyNumber)
                            )
                            || matches!(
                                count_limit,
                                Some(spellwake_oracle_compiler::ast::CountLimitAst::UpTo(e))
                                    if matches!(e.as_ref(), ValueExprAst::Const(1))
                            ))
                        {
                            return None;
                        }
                        let subj_ok = match subject.as_ref() {
                            SubjectRefAst::Anaphora(a) => matches!(
                                a.trim().to_ascii_lowercase().as_str(),
                                "it" | "that card"
                                    | "that spell"
                                    | "the exiled card"
                                    | "the revealed card"
                                    | "them"
                                    | "those cards"
                                    | "cards exiled this way"
                                    | "cards revealed this way"
                            ),
                            _ => false,
                        };
                        if !subj_ok {
                            return None;
                        }
                        let (dur, dur_any_color) = parse_grant_cast_duration_resolved(duration)?;
                        Some(spellwake_engine::program::LookTopKeepDest::Exile {
                            playable: true,
                            free: *free,
                            cast_only: *cast_only,
                            any_color: dur_any_color || extra_any_color,
                            duration: dur,
                        })
                    };
                let parse_ext_keep_op =
                    |op: &InstructionAst| -> Option<spellwake_engine::program::LookTopKeepDest> {
                        if let Some(kd) = parse_grant_keep(op) {
                            return Some(kd);
                        }
                        match op {
                            InstructionAst::MoveZone { subject, to }
                                if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                                    || matches!(
                                        subject.as_ref(),
                                        SubjectRefAst::All(f) | SubjectRefAst::Group(f)
                                            if f.predicates.iter().any(|p| matches!(
                                                p,
                                                spellwake_oracle_compiler::ast::FilterPredicateAst::AffectedByPriorAction(
                                                    spellwake_oracle_compiler::ast::PriorActionRefKind::RevealedThisWay
                                                        | spellwake_oracle_compiler::ast::PriorActionRefKind::ExiledThisWay
                                                )
                                            ))
                                    ) =>
                            {
                                parse_look_top_keep_dest_ext(to, false)
                            }
                            InstructionAst::ReturnZone {
                                subject,
                                to,
                                tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                ..
                            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                                parse_look_top_keep_dest_ext(to, *tapped)
                            }
                            InstructionAst::Exile {
                                actor: None | Some(PlayerRefAst::You),
                                subject,
                                random: false,
                                face_down: false,
                                into_pile: false,
                            } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                                Some(spellwake_engine::program::LookTopKeepDest::Exile {
                                    playable: false,
                                    free: false,
                                    cast_only: false,
                                    any_color: false,
                                    duration: ResolvedDuration::NoStatedDuration,
                                })
                            }
                            _ => None,
                        }
                    };
                // Pattern Ext-A: RevealUntil / ExileOrRevealTopUntil + ChooseAndKeep (+ optional ShuffleLibrary)
                if let InstructionAst::ChooseAndKeep {
                    keep_to, rest_to, ..
                } = &ins[i + 1]
                    && let Some(match_dest) = parse_look_top_keep_dest_ext(keep_to, false)
                {
                    let rest_low = rest_to.to_ascii_lowercase();
                    let rest_shuffle_inline = rest_low.contains("shuffle");
                    let parsed_rest = parse_look_top_rest_dest_ext(rest_to);
                    let trailing_shuffle = parsed_rest.is_none()
                        && !rest_shuffle_inline
                        && (rest_low.trim().is_empty() || rest_low.contains("library"))
                        && i + 2 < ins.len()
                        && matches!(&ins[i + 2], InstructionAst::ShuffleLibrary { .. });
                    if parsed_rest.is_some() || rest_shuffle_inline || trailing_shuffle {
                        let rest_dest = if rest_shuffle_inline || trailing_shuffle {
                            spellwake_engine::program::LookTopRestDest::TopOrder
                        } else {
                            parsed_rest
                                .unwrap_or(spellwake_engine::program::LookTopRestDest::BottomOrder)
                        };
                        let mut probe = builder.clone();
                        if req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok()) {
                            *builder = probe;
                            builder.instructions.push(Instruction::RevealUntilMatchExt {
                                library_owner,
                                chooser: ResolvingPlayer::Controller,
                                filter: Box::new(obj_filter),
                                count: cnt_expr,
                                match_dest,
                                rest_dest,
                                keep_all_revealed: false,
                                optional_keep: false,
                                exile_instead_of_reveal: false,
                            });
                            if rest_shuffle_inline || trailing_shuffle {
                                push_shuffle_for_owner(&mut builder.instructions, library_owner);
                            }
                            i += if trailing_shuffle { 3 } else { 2 };
                            continue;
                        }
                    }
                }
                // Pattern Ext-B (reversed): RevealUntil + rest-move + keep-move
                if i + 2 < ins.len()
                    && let Some(r) = extract_look_top_rest_dest(&ins[i + 1])
                    && let Some(match_dest) = parse_ext_keep_op(&ins[i + 2])
                {
                    let mut probe = builder.clone();
                    if req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok()) {
                        *builder = probe;
                        builder.instructions.push(Instruction::RevealUntilMatchExt {
                            library_owner,
                            chooser: ResolvingPlayer::Controller,
                            filter: Box::new(obj_filter),
                            count: cnt_expr,
                            match_dest,
                            rest_dest: r.to_engine_ext(),
                            keep_all_revealed: false,
                            optional_keep: false,
                            exile_instead_of_reveal: false,
                        });
                        i += 3;
                        continue;
                    }
                }
                // Pattern Ext-B (forward): RevealUntil / ExileOrRevealTopUntil + keep-move
                // (+ optional any-color spend rule) + rest-move / ShuffleLibrary
                if i + 2 < ins.len()
                    && let Some(mut match_dest) = parse_ext_keep_op(&ins[i + 1])
                {
                    let parse_ext_rest = |op: &InstructionAst| -> Option<(
                        spellwake_engine::program::LookTopRestDest,
                        bool,
                    )> {
                        if let Some(r) = extract_look_top_rest_dest(op) {
                            Some((r.to_engine_ext(), false))
                        } else if matches!(op, InstructionAst::ShuffleLibrary { .. })
                            || matches!(
                                op,
                                InstructionAst::ShuffleIntoLibrary { subject }
                                    if matches!(
                                        subject.as_ref(),
                                        SubjectRefAst::Anaphora(a)
                                            if {
                                                let l = a.to_ascii_lowercase();
                                                l.contains("rest") || l.contains("other")
                                            }
                                    )
                            )
                        {
                            Some((spellwake_engine::program::LookTopRestDest::TopOrder, true))
                        } else {
                            None
                        }
                    };
                    let rest_match = if let Some((rd, shuf)) = parse_ext_rest(&ins[i + 2]) {
                        Some((rd, shuf, false, 3usize))
                    } else if i + 3 < ins.len()
                        && is_any_color_spend_restriction(&ins[i + 2])
                        && matches!(
                            match_dest,
                            spellwake_engine::program::LookTopKeepDest::Exile {
                                playable: true,
                                ..
                            }
                        )
                        && let Some((rd, shuf)) = parse_ext_rest(&ins[i + 3])
                    {
                        Some((rd, shuf, true, 4usize))
                    } else {
                        None
                    };
                    if let Some((rest_dest, do_shuffle, set_any_color, step)) = rest_match {
                        if set_any_color
                            && let spellwake_engine::program::LookTopKeepDest::Exile {
                                any_color,
                                ..
                            } = &mut match_dest
                        {
                            *any_color = true;
                        }
                        let mut probe = builder.clone();
                        if req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok()) {
                            *builder = probe;
                            builder.instructions.push(Instruction::RevealUntilMatchExt {
                                library_owner,
                                chooser: ResolvingPlayer::Controller,
                                filter: Box::new(obj_filter),
                                count: cnt_expr,
                                match_dest,
                                rest_dest,
                                keep_all_revealed: false,
                                optional_keep: false,
                                exile_instead_of_reveal: false,
                            });
                            if do_shuffle {
                                push_shuffle_for_owner(&mut builder.instructions, library_owner);
                            }
                            i += step;
                            continue;
                        }
                    }
                }
                // Pattern Ext-C (2-step): ExileOrRevealTopUntil(exile: true) + [Optional?]GrantCastPermission
                // (+ optional any-color spend rule), leaving non-matching cards in exile.
                if is_exile_until && let Some(mut match_dest) = parse_grant_keep(&ins[i + 1]) {
                    let mut step = 2usize;
                    if i + 2 < ins.len()
                        && is_any_color_spend_restriction(&ins[i + 2])
                        && let spellwake_engine::program::LookTopKeepDest::Exile {
                            any_color, ..
                        } = &mut match_dest
                    {
                        *any_color = true;
                        step = 3;
                    }
                    let mut probe = builder.clone();
                    if req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok()) {
                        *builder = probe;
                        builder.instructions.push(Instruction::RevealUntilMatchExt {
                            library_owner,
                            chooser: ResolvingPlayer::Controller,
                            filter: Box::new(obj_filter),
                            count: cnt_expr,
                            match_dest,
                            rest_dest: spellwake_engine::program::LookTopRestDest::Exile,
                            keep_all_revealed: false,
                            optional_keep: false,
                            exile_instead_of_reveal: false,
                        });
                        i += step;
                        continue;
                    }
                }
            }
        }
        // Consume redundant "reveal it" (LookTopLibrary { reveal: true, library: "ref" }) immediately after a filtered search or revealed look
        if let InstructionAst::LookTopLibrary {
            reveal: true,
            library,
            ..
        } = &ins[i]
            && library == "ref"
            && builder.instructions.last().is_some_and(|prev| {
                matches!(prev, Instruction::SearchLibrary { filter } if filter.reveal)
                    || matches!(
                        prev,
                        Instruction::LookTopChooseExt {
                            reveal_all: true,
                            ..
                        }
                    )
            })
        {
            i += 1;
            continue;
        }
        // Fuse 2-step LookTopLibrary + ReorderTopLibrary (or MoveZone(them -> top in any order)) -> LookTopChoose(rest_to_top=true, max_picks=0)
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && (matches!(&ins[i + 1], InstructionAst::ReorderTopLibrary)
                || matches!(
                    &ins[i + 1],
                    InstructionAst::MoveZone { subject, to }
                        if matches!(
                            subject.as_ref(),
                            SubjectRefAst::Anaphora(a)
                                if matches!(
                                    a.trim().to_ascii_lowercase().as_str(),
                                    "them" | "those cards" | "the rest" | "the_rest" | "rest"
                                )
                        ) && to.to_ascii_lowercase().contains("top of")
                            && to.to_ascii_lowercase().contains("library")
                ))
        {
            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::LookTopChooseExt {
                library_owner: ResolvingPlayer::Controller,
                chooser: ResolvingPlayer::Controller,
                count: cnt,
                filter: None,
                max_picks: 0,
                take_all_matching: false,
                keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                rest_dest: spellwake_engine::program::LookTopRestDest::TopOrder,
                reveal_kept: false,
                reveal_all: *reveal,
                reveal_only: false,
                secondary: None,
            });
            i += 2;
            continue;
        }
        // Fuse mandatory LookTopLibrary/PlayerLookTopLibrary(1, reveal: true)
        // + IfCondition(MatchesFilter(Anaphora, f), then_body) (+ optional Otherwise)
        // -> LookTopChoose(reveal_only: true) + BranchIfFalse(ObjectMatches(RevealedTop, f))
        if i + 1 < ins.len()
            && let Some((library, count)) = match &ins[i] {
                InstructionAst::LookTopLibrary {
                    reveal: true,
                    library,
                    count,
                }
                | InstructionAst::PlayerLookTopLibrary {
                    reveal: true,
                    library,
                    count,
                    ..
                } => Some((library.as_str(), count.as_ref())),
                _ => None,
            }
            && matches!(count, ValueExprAst::Const(1))
            && let Some((req_bt, library_owner)) =
                parse_look_top_library_owner(library, builder, in_trigger)
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        {
            let mut inner_f = filter.as_ref().clone();
            inner_f.zone = None;
            let mut probe = builder.clone();
            let target_ok = req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok());
            if target_ok && let Ok(obj_filter) = lower_object_filter(&inner_f, vocabulary) {
                let else_opt = if i + 2 < ins.len()
                    && let InstructionAst::Otherwise(eb) = &ins[i + 2]
                {
                    Some(eb.as_slice())
                } else {
                    None
                };
                let then_b = lower_revealed_top_branch(
                    then_body,
                    &probe,
                    library_owner,
                    true,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
                let else_b = match else_opt {
                    Some(eb) => lower_revealed_top_branch(
                        eb,
                        &probe,
                        library_owner,
                        true,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .map(Some),
                    None => Ok(None),
                };
                if let (Ok(then_b), Ok(else_b_opt)) = (then_b, else_b)
                    && (then_b.target == BodyTarget::None || then_b.target == probe.target)
                    && else_b_opt
                        .as_ref()
                        .is_none_or(|eb| eb.target == BodyTarget::None || eb.target == probe.target)
                {
                    *builder = probe;
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner,
                        chooser: ResolvingPlayer::Controller,
                        count: ValueExpr::literal(1),
                        filter: None,
                        max_picks: 0,
                        take_all_matching: false,
                        keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                        rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                        reveal_kept: false,
                        reveal_all: true,
                        reveal_only: true,
                        secondary: None,
                    });
                    let branch_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::object_matches(
                                MatchObjectRef::RevealedTop,
                                obj_filter,
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        },
                        destination: 0,
                    });
                    let then_shift = isize::try_from(builder.instructions.len())
                        .map_err(|_| "oracle:jump-shift-ovf")?;
                    for mut op in then_b.instructions {
                        op.shift_jumps(then_shift, 0);
                        builder.instructions.push(op);
                    }
                    if let Some(else_b) = else_b_opt {
                        let jump_idx = builder.instructions.len();
                        builder
                            .instructions
                            .push(Instruction::Jump { destination: 0 });
                        let else_start = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = else_start;
                        }
                        let else_shift =
                            isize::try_from(else_start).map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in else_b.instructions {
                            op.shift_jumps(else_shift, 0);
                            builder.instructions.push(op);
                        }
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::Jump { destination }) =
                            builder.instructions.get_mut(jump_idx)
                        {
                            *destination = end_dest;
                        }
                    } else {
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = end_dest;
                        }
                    }
                    i += if else_opt.is_some() { 3 } else { 2 };
                    continue;
                }
            }
        }
        // Fuse Optional([LookTopLibrary(1, reveal: true), IfCondition(MatchesFilter(Anaphora, f), then_body)])
        // OR [Optional([LookTopLibrary(1, reveal: true)])] + IfCondition(MatchesFilter(Anaphora, f), then_body) (+ optional Otherwise)
        // On permanent abilities uses MayRevealLibraryTop; on spells uses OptionalEffect + LookTopChoose(reveal_only: true).
        if builder.target == BodyTarget::None {
            let use_may_reveal = in_trigger || self_pronoun_valid;
            type RevealTopMatch<'a> = (
                &'a ObjectFilterAst,
                &'a [InstructionAst],
                Option<&'a [InstructionAst]>,
                usize,
            );
            let reveal_top_match: Option<RevealTopMatch<'_>> =
                if let InstructionAst::Optional(inner) = &ins[i]
                    && let [
                        InstructionAst::LookTopLibrary {
                            reveal: true,
                            library,
                            count,
                        },
                        InstructionAst::IfCondition {
                            condition,
                            then_body,
                        },
                    ] = inner.as_slice()
                    && matches!(
                        library.trim().to_ascii_lowercase().as_str(),
                        "your" | "own" | "" | "you"
                    )
                    && matches!(count.as_ref(), ValueExprAst::Const(1))
                    && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
                    && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                {
                    let else_opt = if i + 1 < ins.len()
                        && let InstructionAst::Otherwise(eb) = &ins[i + 1]
                    {
                        Some((eb.as_slice(), 2usize))
                    } else {
                        None
                    };
                    Some((
                        filter.as_ref(),
                        then_body.as_slice(),
                        else_opt.map(|(eb, _)| eb),
                        else_opt.map_or(1, |(_, s)| s),
                    ))
                } else if i + 1 < ins.len() {
                    let is_opt_reveal_one = match &ins[i] {
                        InstructionAst::Optional(inner) => matches!(
                            inner.as_slice(),
                            [InstructionAst::LookTopLibrary {
                                reveal: true,
                                library,
                                count,
                            }] if matches!(
                                library.trim().to_ascii_lowercase().as_str(),
                                "your" | "own" | "" | "you"
                            ) && matches!(count.as_ref(), ValueExprAst::Const(1))
                        ),
                        _ => false,
                    };
                    if is_opt_reveal_one
                        && let InstructionAst::IfCondition {
                            condition,
                            then_body,
                        } = &ins[i + 1]
                        && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
                        && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    {
                        let else_opt = if i + 2 < ins.len()
                            && let InstructionAst::Otherwise(eb) = &ins[i + 2]
                        {
                            Some((eb.as_slice(), 3usize))
                        } else {
                            None
                        };
                        Some((
                            filter.as_ref(),
                            then_body.as_slice(),
                            else_opt.map(|(eb, _)| eb),
                            else_opt.map_or(2, |(_, s)| s),
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };
            if let Some((filter_ast, then_slice, else_opt, step)) = reveal_top_match {
                let mut inner_f = filter_ast.clone();
                inner_f.zone = None;
                if let Ok(obj_filter) = lower_object_filter(&inner_f, vocabulary) {
                    let then_b = lower_revealed_top_branch(
                        then_slice,
                        builder,
                        ResolvingPlayer::Controller,
                        true,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                    let else_b = match else_opt {
                        Some(eb) => lower_revealed_top_branch(
                            eb,
                            builder,
                            ResolvingPlayer::Controller,
                            true,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )
                        .map(Some),
                        None => Ok(None),
                    };
                    if let (Ok(then_b), Ok(else_b_opt)) = (then_b, else_b)
                        && then_b.target == BodyTarget::None
                        && else_b_opt
                            .as_ref()
                            .is_none_or(|eb| eb.target == BodyTarget::None)
                    {
                        if use_may_reveal {
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                spellwake_engine::toolbox::MutatePlayerSpec::may_reveal_library_top(
                                ),
                            )));
                        } else {
                            builder.instructions.push(Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::Controller,
                            });
                            builder.instructions.push(Instruction::LookTopChooseExt {
                                library_owner: ResolvingPlayer::Controller,
                                chooser: ResolvingPlayer::Controller,
                                count: ValueExpr::literal(1),
                                filter: None,
                                max_picks: 0,
                                take_all_matching: false,
                                keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                                rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                                reveal_kept: false,
                                reveal_all: true,
                                reveal_only: true,
                                secondary: None,
                            });
                        }
                        let branch_idx = builder.instructions.len();
                        builder.instructions.push(Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: ValueExpr::object_matches(
                                    MatchObjectRef::RevealedTop,
                                    obj_filter,
                                ),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(1),
                            },
                            destination: 0,
                        });
                        let then_shift = isize::try_from(builder.instructions.len())
                            .map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in then_b.instructions {
                            op.shift_jumps(then_shift, 0);
                            builder.instructions.push(op);
                        }
                        if let Some(else_b) = else_b_opt {
                            let jump_idx = builder.instructions.len();
                            builder
                                .instructions
                                .push(Instruction::Jump { destination: 0 });
                            let else_start = builder.instructions.len();
                            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                                builder.instructions.get_mut(branch_idx)
                            {
                                *destination = else_start;
                            }
                            let else_shift =
                                isize::try_from(else_start).map_err(|_| "oracle:jump-shift-ovf")?;
                            for mut op in else_b.instructions {
                                op.shift_jumps(else_shift, 0);
                                builder.instructions.push(op);
                            }
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::Jump { destination }) =
                                builder.instructions.get_mut(jump_idx)
                            {
                                *destination = end_dest;
                            }
                        } else {
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                                builder.instructions.get_mut(branch_idx)
                            {
                                *destination = end_dest;
                            }
                        }
                        i += step;
                        continue;
                    }
                }
            }
        }
        // Unwrap Optional([LookTopLibrary | RevealUntil | ExileOrRevealTopUntil, ...]) where the inner slice lowers cleanly
        // to LookTopChoose or RevealUntilMatch (+ optional ShuffleOwnLibrary).
        if let InstructionAst::Optional(inner) = &ins[i]
            && inner.len() >= 2
            && matches!(
                inner.first(),
                Some(
                    InstructionAst::LookTopLibrary { .. }
                        | InstructionAst::RevealUntil { .. }
                        | InstructionAst::ExileOrRevealTopUntil { .. }
                )
            )
        {
            let mut sub = BodyBuilder::new();
            sub.has_source = builder.has_source;
            if lower_instruction_seq(
                inner,
                &mut sub,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && sub.target == BodyTarget::None
            {
                let converted = sub.instructions;
                if matches!(
                    converted.as_slice(),
                    [Instruction::LookTopChooseExt { .. }
                        | Instruction::RevealUntilMatchExt { .. }]
                ) {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
                if matches!(
                    converted.as_slice(),
                    [Instruction::ZoneOp(_), Instruction::ZoneOp(_)]
                ) {
                    builder.instructions.push(Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 2,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
                if matches!(
                    converted.as_slice(),
                    [
                        Instruction::ZoneOp(_),
                        Instruction::ZoneOp(_),
                        Instruction::ZoneOp(spec)
                    ] if spec.shuffle_library.is_some()
                ) {
                    builder.instructions.push(Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 3,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
                if matches!(
                    converted.as_slice(),
                    [
                        Instruction::LookTopChooseExt { .. }
                            | Instruction::RevealUntilMatchExt { .. },
                        Instruction::ZoneOp(spec)
                    ] if spec.shuffle_library.is_some()
                ) {
                    builder.instructions.push(Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 2,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
            }
        }
        // Extended LookTop fusions: 2-step LookTop(1) + MoveZone/ReturnZone(bottom),
        // two-bucket LookTopSecondary, RevealCards + MoveZone/ReturnZone + rest-move (including ReturnZone bottom in random order),
        // and generalized library_owner.
        if let Some(next_i) =
            try_fuse_look_top_extended(ins, i, builder, vocabulary, in_trigger, bound_x)
        {
            i = next_i;
            continue;
        }
        // Fuse 4-step LookTopLibrary + [Optional?]RevealFromLooked(to="revealed") + [Optional?]MoveZone(it -> hand/bf) + MoveZone(rest -> bottom/gy/exile/top)
        // AND 3-step LookTopLibrary + Optional([RevealFromLooked(to="revealed"), MoveZone(it -> hand/bf)]) + MoveZone(rest -> bottom/gy/exile/top)
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
        {
            let parse_reveal_pick_arity = |phrase: &str, rev_tag: &str| -> (bool, u8) {
                if rev_tag == "revealed:all" {
                    return (true, 1);
                }
                if rev_tag == "revealed:any_number" {
                    return (false, u8::MAX);
                }
                if let Some(num_str) = rev_tag.strip_prefix("revealed:")
                    && let Ok(n) = num_str.parse::<u8>()
                    && n >= 2
                {
                    return (false, n);
                }
                let low = phrase.trim().to_ascii_lowercase();
                if low.contains("all of them")
                    || low.contains("all cards")
                    || low.starts_with("all ")
                    || low.contains("each of them")
                {
                    (true, 1)
                } else if low.contains("any number") {
                    (false, u8::MAX)
                } else if low.contains("four")
                    || low.starts_with("up to 4 ")
                    || low.starts_with("4 ")
                {
                    (false, 4)
                } else if low.contains("three")
                    || low.starts_with("up to 3 ")
                    || low.starts_with("3 ")
                {
                    (false, 3)
                } else if low.contains("two")
                    || low.starts_with("up to 2 ")
                    || low.starts_with("2 ")
                {
                    (false, 2)
                } else {
                    (false, 1)
                }
            };
            let four_step = if i + 3 < ins.len() {
                let rev = match &ins[i + 1] {
                    InstructionAst::RevealFromLooked { filter, to }
                        if to == "revealed" || to.starts_with("revealed:") =>
                    {
                        Some((filter, to.as_str()))
                    }
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::RevealFromLooked { filter, to }]
                            if to == "revealed" || to.starts_with("revealed:") =>
                        {
                            Some((filter, to.as_str()))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let mv = match &ins[i + 2] {
                    InstructionAst::MoveZone { subject, to } => {
                        Some((subject.as_ref(), to.as_str(), false))
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } => Some((subject.as_ref(), to.as_str(), *tapped)),
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::MoveZone { subject, to }] => {
                            Some((subject.as_ref(), to.as_str(), false))
                        }
                        [
                            InstructionAst::ReturnZone {
                                subject,
                                to,
                                tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                ..
                            },
                        ] => Some((subject.as_ref(), to.as_str(), *tapped)),
                        _ => None,
                    },
                    _ => None,
                };
                let rest = match &ins[i + 3] {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(
                            subject.as_ref(),
                            SubjectRefAst::Anaphora(a) if matches!(
                                a.to_ascii_lowercase().as_str(),
                                "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                            )
                        ) =>
                    {
                        Some(to.as_str())
                    }
                    InstructionAst::ReorderTopLibrary => Some("top of your library"),
                    _ => None,
                };
                match (rev, mv, rest) {
                    (
                        Some((filter_ast, rev_tag)),
                        Some((SubjectRefAst::Anaphora(a), keep_to, tapped_flag)),
                        Some(rest_to),
                    ) if matches!(
                        a.to_ascii_lowercase().as_str(),
                        "it" | "that card"
                            | "the card"
                            | "the revealed card"
                            | "the revealed cards"
                            | "them"
                            | "those cards"
                    ) =>
                    {
                        let (all_m, max_p) = parse_reveal_pick_arity(keep_to, rev_tag);
                        Some((
                            filter_ast,
                            keep_to,
                            rest_to,
                            all_m,
                            max_p,
                            tapped_flag,
                            4usize,
                        ))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let three_step_opt = if four_step.is_none() {
                if let InstructionAst::Optional(inner) = &ins[i + 1]
                    && let [
                        InstructionAst::RevealFromLooked { filter, to },
                        InstructionAst::MoveZone {
                            subject,
                            to: keep_to,
                        },
                    ] = inner.as_slice()
                    && (to == "revealed" || to.starts_with("revealed:"))
                    && matches!(
                        subject.as_ref(),
                        SubjectRefAst::Anaphora(a) if matches!(
                            a.to_ascii_lowercase().as_str(),
                            "it" | "that card"
                                | "the card"
                                | "the revealed card"
                                | "the revealed cards"
                                | "them"
                                | "those cards"
                        )
                    )
                {
                    let rest = match &ins[i + 2] {
                        InstructionAst::MoveZone { subject, to }
                            if matches!(
                                subject.as_ref(),
                                SubjectRefAst::Anaphora(a) if matches!(
                                    a.to_ascii_lowercase().as_str(),
                                    "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                                )
                            ) =>
                        {
                            Some(to.as_str())
                        }
                        InstructionAst::ReorderTopLibrary => Some("top of your library"),
                        _ => None,
                    };
                    let (all_m, max_p) = parse_reveal_pick_arity(keep_to, to);
                    rest.map(|r| (filter, keep_to.as_str(), r, all_m, max_p, false, 3usize))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((
                filter_ast,
                keep_to,
                rest_to,
                take_all_matching,
                max_picks,
                tapped_flag,
                step,
            )) = four_step.or(three_step_opt)
            {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && (tapped_flag || keep_low.contains("tapped"));
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_top = rest_low.contains("top") && rest_low.contains("library");
                let random_rest = rest_low.contains("random order");
                if (to_hand || to_battlefield)
                    && (rest_bottom || rest_gy || rest_exile || rest_top)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner: ResolvingPlayer::Controller,
                        chooser: ResolvingPlayer::Controller,
                        count: cnt,
                        filter: Some(Box::new(obj_filter)),
                        max_picks,
                        take_all_matching,
                        keep_dest: if to_battlefield {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped: enters_tapped,
                                attacking: false,
                            }
                        } else {
                            spellwake_engine::program::LookTopKeepDest::Hand
                        },
                        rest_dest: if rest_gy {
                            spellwake_engine::program::LookTopRestDest::Graveyard
                        } else if rest_exile {
                            spellwake_engine::program::LookTopRestDest::Exile
                        } else if rest_top {
                            spellwake_engine::program::LookTopRestDest::TopOrder
                        } else if random_rest {
                            spellwake_engine::program::LookTopRestDest::BottomRandom
                        } else {
                            spellwake_engine::program::LookTopRestDest::BottomOrder
                        },
                        reveal_kept: false,
                        reveal_all: *reveal,
                        reveal_only: false,
                        secondary: None,
                    });
                    i += step;
                    continue;
                }
            }
        }
        // Fuse 3-step LookTopLibrary + [Optional(RevealFromLooked) | RevealFromLooked] + MoveZone(rest -> bottom/graveyard/exile/top) -> LookTopChoose
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && let InstructionAst::MoveZone {
                subject: rest_subj,
                to: rest_to,
            } = &ins[i + 2]
            && matches!(
                rest_subj.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.to_ascii_lowercase().as_str(),
                    "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                )
            )
        {
            let rev_opt = match &ins[i + 1] {
                InstructionAst::RevealFromLooked { filter, to } => Some((filter, to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::RevealFromLooked { filter, to }] => Some((filter, to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((filter_ast, keep_to)) = rev_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_top = rest_low.contains("top") && rest_low.contains("library");
                let random_rest = rest_low.contains("random order");
                let (take_all_matching, max_picks) = if keep_low.contains("all of them")
                    || keep_low.contains("all cards")
                    || keep_low.starts_with("all ")
                {
                    (true, 1)
                } else if keep_low.contains("any number") {
                    (false, u8::MAX)
                } else if keep_low.contains("four")
                    || keep_low.starts_with("up to 4 ")
                    || keep_low.starts_with("4 ")
                {
                    (false, 4)
                } else if keep_low.contains("three")
                    || keep_low.starts_with("up to 3 ")
                    || keep_low.starts_with("3 ")
                {
                    (false, 3)
                } else if keep_low.contains("two")
                    || keep_low.starts_with("up to 2 ")
                    || keep_low.starts_with("2 ")
                {
                    (false, 2)
                } else {
                    (false, 1)
                };
                if (to_hand || to_battlefield)
                    && (rest_bottom || rest_gy || rest_exile || rest_top)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner: ResolvingPlayer::Controller,
                        chooser: ResolvingPlayer::Controller,
                        count: cnt,
                        filter: Some(Box::new(obj_filter)),
                        max_picks,
                        take_all_matching,
                        keep_dest: if to_battlefield {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped: enters_tapped,
                                attacking: false,
                            }
                        } else {
                            spellwake_engine::program::LookTopKeepDest::Hand
                        },
                        rest_dest: if rest_gy {
                            spellwake_engine::program::LookTopRestDest::Graveyard
                        } else if rest_exile {
                            spellwake_engine::program::LookTopRestDest::Exile
                        } else if rest_top {
                            spellwake_engine::program::LookTopRestDest::TopOrder
                        } else if random_rest {
                            spellwake_engine::program::LookTopRestDest::BottomRandom
                        } else {
                            spellwake_engine::program::LookTopRestDest::BottomOrder
                        },
                        reveal_kept: false,
                        reveal_all: *reveal,
                        reveal_only: false,
                        secondary: None,
                    });
                    i += 3;
                    continue;
                }
            }
        }
        // Fuse 2-step LookTopLibrary + [Optional?]RevealFromLooked (+ optional ShuffleLibrary) -> LookTopChoose
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
        {
            let rev_opt = match &ins[i + 1] {
                InstructionAst::RevealFromLooked { filter, to } => Some((filter, to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::RevealFromLooked { filter, to }] => Some((filter, to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((filter_ast, keep_to)) = rev_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let (take_all_matching, max_picks) = if keep_low.contains("all of them")
                    || keep_low.contains("all cards")
                    || keep_low.starts_with("all ")
                {
                    (true, 1)
                } else if keep_low.contains("any number") {
                    (false, u8::MAX)
                } else if keep_low.contains("four") {
                    (false, 4)
                } else if keep_low.contains("three") {
                    (false, 3)
                } else if keep_low.contains("two") {
                    (false, 2)
                } else {
                    (false, 1)
                };
                if (to_hand || to_battlefield)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    let consume_shuffle = i + 2 < ins.len()
                        && matches!(
                            &ins[i + 2],
                            InstructionAst::ShuffleLibrary {
                                player: PlayerRefAst::You
                            }
                        );
                    builder.instructions.push(Instruction::LookTopChooseExt {
                        library_owner: ResolvingPlayer::Controller,
                        chooser: ResolvingPlayer::Controller,
                        count: cnt,
                        filter: Some(Box::new(obj_filter)),
                        max_picks,
                        take_all_matching,
                        keep_dest: if to_battlefield {
                            spellwake_engine::program::LookTopKeepDest::Battlefield {
                                tapped: enters_tapped,
                                attacking: false,
                            }
                        } else {
                            spellwake_engine::program::LookTopKeepDest::Hand
                        },
                        rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                        reveal_kept: false,
                        reveal_all: *reveal,
                        reveal_only: false,
                        secondary: None,
                    });
                    if consume_shuffle {
                        builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                    }
                    i += if consume_shuffle { 3 } else { 2 };
                    continue;
                }
            }
        }
        // Fuse 3- or 4-step LookTopLibrary + [Optional?]MoveZone(subject -> hand/bf)
        // (+ optional 1 intermediate grant/modifier on the put permanent, e.g. Winota)
        // + MoveZone(rest -> bottom/gy/exile/top) -> LookTopChoose (+ optional grant on LastMoved)
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
        {
            fn is_rest_anaphora(op: &InstructionAst) -> Option<&str> {
                if let InstructionAst::MoveZone {
                    subject: rest_subj,
                    to: rest_to,
                } = op
                    && matches!(
                        rest_subj.as_ref(),
                        SubjectRefAst::Anaphora(a) if matches!(
                            a.to_ascii_lowercase().as_str(),
                            "the_rest"
                                | "the rest"
                                | "rest"
                                | "the other cards"
                                | "the others"
                                | "them"
                                | "those cards"
                                | "the rest of them"
                                | "the rest of the cards"
                                | "the remaining cards"
                        )
                    )
                {
                    Some(rest_to.as_str())
                } else {
                    None
                }
            }
            let rest_and_grant: Option<(&str, Option<&InstructionAst>, usize)> =
                if let Some(rest_to) = is_rest_anaphora(&ins[i + 2]) {
                    Some((rest_to, None, 3))
                } else if i + 3 < ins.len()
                    && let Some(rest_to) = is_rest_anaphora(&ins[i + 3])
                {
                    Some((rest_to, Some(&ins[i + 2]), 4))
                } else {
                    None
                };
            if let Some((rest_to, outer_grant, advance)) = rest_and_grant {
                let mv_opt = match &ins[i + 1] {
                    InstructionAst::MoveZone { subject, to } => {
                        Some((subject.as_ref(), to.as_str(), false, None))
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } => Some((subject.as_ref(), to.as_str(), *tapped, None)),
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::MoveZone { subject, to }] => {
                            Some((subject.as_ref(), to.as_str(), false, None))
                        }
                        [
                            InstructionAst::ReturnZone {
                                subject,
                                to,
                                tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                ..
                            },
                        ] => Some((subject.as_ref(), to.as_str(), *tapped, None)),
                        [InstructionAst::MoveZone { subject, to }, grant_ins]
                            if outer_grant.is_none() =>
                        {
                            Some((subject.as_ref(), to.as_str(), false, Some(grant_ins)))
                        }
                        [
                            InstructionAst::ReturnZone {
                                subject,
                                to,
                                tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                ..
                            },
                            grant_ins,
                        ] if outer_grant.is_none() => {
                            Some((subject.as_ref(), to.as_str(), *tapped, Some(grant_ins)))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((keep_subj, keep_to, tapped_flag, inner_grant)) = mv_opt {
                    let keep_low = keep_to.to_ascii_lowercase();
                    let rest_low = rest_to.to_ascii_lowercase();
                    let to_hand = keep_low.contains("hand");
                    let to_battlefield = keep_low.contains("battlefield");
                    let enters_tapped =
                        to_battlefield && (tapped_flag || keep_low.contains("tapped"));
                    let enters_attacking = to_battlefield && keep_low.contains("attacking");
                    let rest_bottom = rest_low.contains("bottom");
                    let rest_gy = rest_low.contains("graveyard");
                    let rest_exile = rest_low.contains("exile");
                    let rest_top = rest_low.contains("top") && rest_low.contains("library");
                    let grant_ast = outer_grant.or(inner_grant);
                    let lowered_grant = match grant_ast {
                        None => Some(None),
                        Some(g) if to_battlefield => {
                            lower_look_top_intermediate_grant(g, vocabulary, bound_x).map(Some)
                        }
                        Some(_) => None,
                    };
                    if let Some(grant_op) = lowered_grant
                        && (to_hand || to_battlefield)
                        && (rest_bottom || rest_gy || rest_exile || rest_top)
                    {
                        let parsed_keep: Option<(bool, Option<ObjectFilter>)> = match keep_subj {
                            SubjectRefAst::Group(f) => {
                                let cleaned = strip_look_top_zone_ref(f);
                                if is_unrestricted_filter(&cleaned) {
                                    Some((false, None))
                                } else {
                                    lower_object_filter(&cleaned, vocabulary)
                                        .ok()
                                        .map(|o| (false, Some(o)))
                                }
                            }
                            SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                                let cleaned = strip_look_top_zone_ref(f);
                                if is_unrestricted_filter(&cleaned) {
                                    Some((true, None))
                                } else {
                                    lower_object_filter(&cleaned, vocabulary)
                                        .ok()
                                        .map(|o| (true, Some(o)))
                                }
                            }
                            SubjectRefAst::Anaphora(a)
                                if matches!(
                                    a.to_ascii_lowercase().as_str(),
                                    "one of them" | "one" | "that card" | "it"
                                ) =>
                            {
                                Some((false, None))
                            }
                            _ => None,
                        };
                        if let Some((take_all_matching, maybe_filter)) = parsed_keep {
                            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                            let random_rest = rest_low.contains("random order");
                            if enters_attacking {
                                let rest_dest = parse_look_top_rest_dest_ext(rest_to).unwrap_or(
                                    if random_rest {
                                        spellwake_engine::program::LookTopRestDest::BottomRandom
                                    } else {
                                        spellwake_engine::program::LookTopRestDest::BottomOrder
                                    },
                                );
                                builder.instructions.push(Instruction::LookTopChooseExt {
                                    library_owner: ResolvingPlayer::Controller,
                                    chooser: ResolvingPlayer::Controller,
                                    count: cnt,
                                    filter: maybe_filter.map(Box::new),
                                    max_picks: 1,
                                    take_all_matching,
                                    keep_dest:
                                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                                            tapped: enters_tapped,
                                            attacking: true,
                                        },
                                    rest_dest,
                                    reveal_kept: *reveal || maybe_filter.is_some(),
                                    reveal_all: false,
                                    reveal_only: false,
                                    secondary: None,
                                });
                            } else {
                                builder.instructions.push(Instruction::LookTopChooseExt {
                                    library_owner: ResolvingPlayer::Controller,
                                    chooser: ResolvingPlayer::Controller,
                                    count: cnt,
                                    filter: maybe_filter.map(Box::new),
                                    max_picks: 1,
                                    take_all_matching,
                                    keep_dest: if to_battlefield {
                                        spellwake_engine::program::LookTopKeepDest::Battlefield {
                                            tapped: enters_tapped,
                                            attacking: false,
                                        }
                                    } else {
                                        spellwake_engine::program::LookTopKeepDest::Hand
                                    },
                                    rest_dest: if rest_gy {
                                        spellwake_engine::program::LookTopRestDest::Graveyard
                                    } else if rest_exile {
                                        spellwake_engine::program::LookTopRestDest::Exile
                                    } else if rest_top {
                                        spellwake_engine::program::LookTopRestDest::TopOrder
                                    } else if random_rest {
                                        spellwake_engine::program::LookTopRestDest::BottomRandom
                                    } else {
                                        spellwake_engine::program::LookTopRestDest::BottomOrder
                                    },
                                    reveal_kept: false,
                                    reveal_all: *reveal,
                                    reveal_only: false,
                                    secondary: None,
                                });
                            }
                            if let Some(gop) = grant_op {
                                builder.instructions.push(gop);
                            }
                            i += advance;
                            continue;
                        }
                    }
                }
            }
        }
        // Fuse LookTopLibrary (or BindX([LookTopLibrary])) + SeparateIntoPiles (+ optional ChoosePile) + ChooseAndKeep("one pile"/"that pile") -> RevealTopTwoPiles
        if let Some(consumed) =
            interactive::try_fuse_two_pile_split(ins, i, builder, vocabulary, in_trigger, bound_x)
        {
            i += consumed;
            continue;
        }
        // Fuse ChooseFromGroup / InteractiveChoice(ChooseFromGroup) + Destroy/Exile/Return/Sacrifice/Discard/Tap of unchosen rest -> ChooseObjects
        if let Some(consumed) = interactive::try_fuse_choose_objects_and_rest(
            ins, i, builder, vocabulary, in_trigger, bound_x,
        ) {
            i += consumed;
            continue;
        }
        // Fuse [Optional?]LookTopLibrary (or BindX-wrapped) + [IfPreviousSucceeded?]ChooseAndKeep
        // (e.g. Anticipate, Impulse, Worldly Counsel, Thassa's Oracle, Memory Deluge, Cream of the Crop)
        struct LookKeepPair<'a> {
            is_optional_look: bool,
            reveal_val: bool,
            library: &'a str,
            count: &'a ValueExprAst,
            eff_bound_x: Option<&'a ValueExprAst>,
            pick: &'a str,
            keep_to: &'a str,
            rest_to: &'a str,
            inner_shuffle: bool,
        }
        let look_keep_pair: Option<LookKeepPair<'_>> = if i + 1 < ins.len() {
            if let Some((reveal_val, library, count, eff_bound_x)) =
                interactive::unwrap_look_top_with_bind_x(&ins[i], bound_x)
                && let InstructionAst::ChooseAndKeep {
                    pick,
                    keep_to,
                    rest_to,
                } = &ins[i + 1]
            {
                Some(LookKeepPair {
                    is_optional_look: false,
                    reveal_val,
                    library,
                    count,
                    eff_bound_x,
                    pick: pick.as_str(),
                    keep_to: keep_to.as_str(),
                    rest_to: rest_to.as_str(),
                    inner_shuffle: false,
                })
            } else {
                let opt_look = match &ins[i] {
                    InstructionAst::Optional(inner) if inner.len() == 1 => {
                        interactive::unwrap_look_top_with_bind_x(&inner[0], bound_x)
                    }
                    InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                        [InstructionAst::Optional(inner)] if inner.len() == 1 => {
                            interactive::unwrap_look_top_with_bind_x(
                                &inner[0],
                                Some(x_expr.as_ref()),
                            )
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((reveal_val, library, count, eff_bound_x)) = opt_look
                    && let InstructionAst::IfPreviousSucceeded(then_body) = &ins[i + 1]
                {
                    match then_body.as_slice() {
                        [
                            InstructionAst::ChooseAndKeep {
                                pick,
                                keep_to,
                                rest_to,
                            },
                        ] => Some(LookKeepPair {
                            is_optional_look: true,
                            reveal_val,
                            library,
                            count,
                            eff_bound_x,
                            pick: pick.as_str(),
                            keep_to: keep_to.as_str(),
                            rest_to: rest_to.as_str(),
                            inner_shuffle: false,
                        }),
                        [
                            InstructionAst::ChooseAndKeep {
                                pick,
                                keep_to,
                                rest_to,
                            },
                            InstructionAst::ShuffleLibrary { .. },
                        ] => Some(LookKeepPair {
                            is_optional_look: true,
                            reveal_val,
                            library,
                            count,
                            eff_bound_x,
                            pick: pick.as_str(),
                            keep_to: keep_to.as_str(),
                            rest_to: rest_to.as_str(),
                            inner_shuffle: true,
                        }),
                        _ => None,
                    }
                } else {
                    None
                }
            }
        } else {
            None
        };
        if let Some(LookKeepPair {
            is_optional_look,
            reveal_val,
            library,
            count,
            eff_bound_x,
            pick,
            keep_to,
            rest_to,
            inner_shuffle,
        }) = look_keep_pair
            && let Some((req_bt, library_owner)) =
                parse_look_top_library_owner(library, builder, in_trigger)
        {
            let reveal = &reveal_val;
            let keep_low = keep_to.to_ascii_lowercase();
            let rest_low = rest_to.to_ascii_lowercase();
            let keep_dest_opt = parse_look_top_keep_dest_ext(keep_to, false);
            let to_hand = matches!(
                keep_dest_opt,
                Some(spellwake_engine::program::LookTopKeepDest::Hand)
            );
            let (to_battlefield, enters_tapped) = match keep_dest_opt {
                Some(spellwake_engine::program::LookTopKeepDest::Battlefield {
                    tapped, ..
                }) => (true, tapped),
                _ => (false, false),
            };
            let rest_bottom = rest_low.contains("bottom");
            let rest_gy = rest_low.contains("graveyard");
            let rest_exile = rest_low.contains("exile");
            let rest_hand = rest_low.contains("hand");
            let rest_top = !rest_bottom && rest_low.contains("top") && rest_low.contains("library");
            let rest_shuffle_inline = rest_low.contains("shuffle") || inner_shuffle;
            let trailing_shuffle = !is_optional_look
                && !rest_bottom
                && !rest_gy
                && !rest_exile
                && !rest_top
                && !rest_hand
                && !rest_shuffle_inline
                && (rest_low.trim().is_empty() || rest_low.contains("library"))
                && i + 2 < ins.len()
                && matches!(&ins[i + 2], InstructionAst::ShuffleLibrary { .. });
            let pick_low = pick.trim().to_ascii_lowercase();
            if let Some(keep_dest) = keep_dest_opt
                && (rest_bottom
                    || rest_gy
                    || rest_exile
                    || rest_top
                    || rest_hand
                    || rest_shuffle_inline
                    || trailing_shuffle)
            {
                let any_one = matches!(
                    pick_low.as_str(),
                    "one"
                        | "1"
                        | "one of them"
                        | "one of those cards"
                        | "a card from among them"
                        | "that card"
                        | "a card"
                        | "any one"
                        | "up to one"
                        | "up to 1"
                        | "up to one of them"
                        | "up to 1 of them"
                        | "up to one card"
                        | "up to one card from among them"
                        | "up to one of those cards"
                );
                let any_multi: Option<u8> = match pick_low.as_str() {
                    "two"
                    | "2"
                    | "two of them"
                    | "up to two"
                    | "up to two of them"
                    | "two of those cards"
                    | "up to two of those cards"
                    | "up to two cards from among them" => Some(2),
                    "three"
                    | "3"
                    | "three of them"
                    | "three of those cards"
                    | "up to three"
                    | "up to three of them"
                    | "up to three of those cards" => Some(3),
                    "four"
                    | "4"
                    | "four of them"
                    | "four of those cards"
                    | "up to four"
                    | "up to four of them" => Some(4),
                    _ => None,
                };
                let (take_all_matching, max_picks, filter_opt): (
                    bool,
                    u8,
                    Option<Option<ObjectFilter>>,
                ) = if any_one {
                    (false, 1, Some(None))
                } else if let Some(k) = any_multi {
                    (false, k, Some(None))
                } else if matches!(
                    pick_low.as_str(),
                    "all"
                        | "all of them"
                        | "all cards"
                        | "all of those cards"
                        | "each of them"
                        | "them"
                        | "those cards"
                ) {
                    (true, 1, Some(None))
                } else if let Some(rest) = pick_low
                    .strip_prefix("all ")
                    .or_else(|| pick_low.strip_prefix("each "))
                    .or_else(|| pick_low.strip_prefix("those "))
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" from among them")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        true,
                        1,
                        parse_look_top_pick_filter_ext(cleaned, vocabulary).map(Some),
                    )
                } else if let Some((k, rest)) = pick_low
                    .strip_prefix("up to two ")
                    .or_else(|| pick_low.strip_prefix("two "))
                    .map(|r| (2u8, r))
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to three ")
                            .or_else(|| pick_low.strip_prefix("three "))
                            .map(|r| (3u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to four ")
                            .or_else(|| pick_low.strip_prefix("four "))
                            .map(|r| (4u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("any number of ")
                            .map(|r| (u8::MAX, r))
                    })
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" from among them")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        false,
                        k,
                        parse_look_top_pick_filter_ext(cleaned, vocabulary).map(Some),
                    )
                } else {
                    (
                        false,
                        1,
                        parse_look_top_pick_filter_ext(&pick_low, vocabulary).map(Some),
                    )
                };
                if let Some(maybe_filter) = filter_opt {
                    let cnt = lower_value_expr(resolve_x(count, eff_bound_x), vocabulary)?;
                    let mut probe = builder.clone();
                    if req_bt.is_none_or(|bt| probe.bind_target(bt).is_ok()) {
                        *builder = probe;
                        let is_controller_lib = library_owner == ResolvingPlayer::Controller;
                        let needs_ext = !is_controller_lib
                            || rest_hand
                            || !matches!(
                                keep_dest,
                                spellwake_engine::program::LookTopKeepDest::Hand
                                    | spellwake_engine::program::LookTopKeepDest::Battlefield {
                                        attacking: false,
                                        ..
                                    }
                            );
                        if is_optional_look {
                            if rest_shuffle_inline || trailing_shuffle {
                                builder.instructions.push(Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::Controller,
                                    instructions: 2,
                                });
                            } else {
                                builder.instructions.push(Instruction::OptionalEffect {
                                    chooser: ResolvingPlayer::Controller,
                                });
                            }
                        }
                        if needs_ext {
                            let rest_dest = if rest_shuffle_inline || trailing_shuffle {
                                spellwake_engine::program::LookTopRestDest::TopOrder
                            } else {
                                parse_look_top_rest_dest_ext(rest_to).unwrap_or(
                                    spellwake_engine::program::LookTopRestDest::BottomOrder,
                                )
                            };
                            let reveal_kept =
                                *reveal || keep_low.contains("reveal") || maybe_filter.is_some();
                            builder.instructions.push(Instruction::LookTopChooseExt {
                                library_owner,
                                chooser: ResolvingPlayer::Controller,
                                count: cnt,
                                filter: maybe_filter.map(Box::new),
                                max_picks,
                                take_all_matching,
                                keep_dest,
                                rest_dest,
                                reveal_kept,
                                reveal_all: false,
                                reveal_only: false,
                                secondary: None,
                            });
                            if rest_shuffle_inline || trailing_shuffle {
                                if is_controller_lib {
                                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                                } else {
                                    builder
                                        .instructions
                                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_player_library(library_owner));
                                }
                            }
                            i += if trailing_shuffle { 3 } else { 2 };
                            continue;
                        }
                        if !is_optional_look
                            && !*reveal
                            && !take_all_matching
                            && max_picks == 1
                            && maybe_filter.is_none()
                            && to_hand
                            && rest_bottom
                            && !rest_low.contains("random order")
                        {
                            const UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                                spellwake_engine::toolbox::BindingId(910);
                            let mut pick_spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                                            ResolvingPlayer::Controller,
                                        ),
                                    )),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                                        cnt,
                                    )),
                                },
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: true,
                                },
                                spellwake_engine::toolbox::ZoneDest::Hand,
                            );
                            pick_spec.bind_unchosen = Some(UNCHOSEN_BIND);
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    pick_spec,
                                ),
                            );
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::Bound(
                                            UNCHOSEN_BIND,
                                        ),
                                        spellwake_engine::toolbox::ZoneDest::Library {
                                            position:
                                                spellwake_engine::toolbox::LibraryOrderPos::Bottom,
                                        },
                                    ),
                                ),
                            );
                            i += 2;
                            continue;
                        }
                        if !is_optional_look
                            && !*reveal
                            && !take_all_matching
                            && max_picks == 1
                            && maybe_filter.is_none()
                            && to_hand
                            && rest_gy
                            && let ValueExpr::Literal(n) = &cnt
                            && let Some(u) = n.to_usize().and_then(|v| u8::try_from(v).ok())
                            && u > 0
                        {
                            const UNCHOSEN_BIND: spellwake_engine::toolbox::BindingId =
                                spellwake_engine::toolbox::BindingId(910);
                            let mut pick_spec = spellwake_engine::toolbox::ZoneOpSpec::simple(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                                            ResolvingPlayer::Controller,
                                        ),
                                    )),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::Top(Box::new(
                                        ValueExpr::literal(i64::from(u)),
                                    )),
                                },
                                spellwake_engine::toolbox::ZoneSelection::Prompt {
                                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    min: Box::new(ValueExpr::literal(1)),
                                    max: Box::new(ValueExpr::literal(1)),
                                    constraint: None,
                                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                    hidden: true,
                                },
                                spellwake_engine::toolbox::ZoneDest::Hand,
                            );
                            pick_spec.bind_unchosen = Some(UNCHOSEN_BIND);
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    pick_spec,
                                ),
                            );
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                        spellwake_engine::toolbox::ObjectQuery::Bound(
                                            UNCHOSEN_BIND,
                                        ),
                                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                                            semantics:
                                                spellwake_engine::toolbox::GraveyardSemantics::Move,
                                        },
                                    ),
                                ),
                            );
                            i += 2;
                            continue;
                        }
                        let random_rest = rest_low.contains("random order");
                        builder.instructions.push(Instruction::LookTopChooseExt {
                            library_owner,
                            chooser: ResolvingPlayer::Controller,
                            count: cnt,
                            filter: maybe_filter.map(Box::new),
                            max_picks,
                            take_all_matching,
                            keep_dest: if to_battlefield {
                                spellwake_engine::program::LookTopKeepDest::Battlefield {
                                    tapped: enters_tapped,
                                    attacking: false,
                                }
                            } else {
                                spellwake_engine::program::LookTopKeepDest::Hand
                            },
                            rest_dest: if rest_gy {
                                spellwake_engine::program::LookTopRestDest::Graveyard
                            } else if rest_exile {
                                spellwake_engine::program::LookTopRestDest::Exile
                            } else if rest_top {
                                spellwake_engine::program::LookTopRestDest::TopOrder
                            } else if random_rest {
                                spellwake_engine::program::LookTopRestDest::BottomRandom
                            } else {
                                spellwake_engine::program::LookTopRestDest::BottomOrder
                            },
                            reveal_kept: false,
                            reveal_all: *reveal,
                            reveal_only: false,
                            secondary: None,
                        });
                        if rest_shuffle_inline || trailing_shuffle {
                            builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_own_library());
                        }
                        i += if trailing_shuffle { 3 } else { 2 };
                        continue;
                    }
                }
            }
        }
        // Fuse Mill(N) + [Optional?]MoveZone/ReturnZone(Group(f with milled PriorActionZoneRef), dest)
        // via Instruction::SelectZoneCardsToZone { source_zone: SourceZoneFilter::LastMoved, .. }
        // (e.g. Cache Grab, Sparring Dummy, Winding Way-style milled picks).
        if i + 1 < ins.len()
            && matches!(&ins[i], InstructionAst::Mill { .. })
            && let Some(mill_pick) = parse_milled_zone_ref_move(&ins[i + 1], vocabulary, bound_x)
        {
            let mut probe = builder.clone();
            if lower_instruction(
                &ins[i],
                &mut probe,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && matches!(
                    probe.instructions.last(),
                    Some(Instruction::MutatePlayer(spec))
                        if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Mill(_))
                )
            {
                let sel_player_res = match mill_pick.selector {
                    Some(p) => lower_resolving_player_in_context(p, &mut probe, in_trigger),
                    None => Ok(ResolvingPlayer::Controller),
                };
                if let Ok(sel_player) = sel_player_res {
                    *builder = probe;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(sel_player, SourceZoneFilter::LastMoved, mill_pick.filter, mill_pick.min, mill_pick.max, mill_pick.destination, mill_pick.reveal));
                    i += 2;
                    continue;
                }
            }
        }
        // Fold trailing "you may spend mana as though it were mana of any color/type"
        // (direct, Optional, or wrapped in IfCondition(CastSpellThisWay)) onto prior
        // PlayPermission / ExileTopPlay / ZoneCastPermission / ExileTopForPlay.
        if is_any_color_spend_restriction(&ins[i]) {
            let mut patched = false;
            for op in builder.instructions.iter_mut().rev() {
                match op {
                    Instruction::ApplyContinuous(spec) => match &mut spec.payload {
                        spellwake_engine::toolbox::ContinuousPayload::PlayPermission {
                            any_color,
                            ..
                        } => {
                            *any_color = true;
                            patched = true;
                            break;
                        }
                        spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(rule) => {
                            if let TemporaryRuleKind::ZoneCastPermission { any_color, .. } =
                                rule.as_mut()
                            {
                                *any_color = true;
                                patched = true;
                                break;
                            }
                        }
                        _ => {}
                    },
                    Instruction::ExileTopPlay { any_color, .. }
                    | Instruction::LookTopChooseExt {
                        keep_dest:
                            spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. },
                        ..
                    }
                    | Instruction::RevealUntilMatchExt {
                        match_dest:
                            spellwake_engine::program::LookTopKeepDest::Exile { any_color, .. },
                        ..
                    } => {
                        *any_color = true;
                        patched = true;
                        break;
                    }
                    _ => {}
                }
            }
            if patched {
                i += 1;
                continue;
            }
        }
        // Fold trailing "Sorcery" cast-timing restriction directly onto an immediately preceding
        // ApplyContinuous(TemporaryRule(ZoneCastPermission { sorcery_only, .. })) (e.g. Lilah).
        if let InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } = &ins[i]
            && matches!(
                restriction
                    .trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
                    .as_str(),
                "sorcery"
                    | "as a sorcery"
                    | "only as a sorcery"
                    | "cast only as a sorcery"
                    | "play only as a sorcery"
                    | "cast it only as a sorcery"
                    | "play it only as a sorcery"
            )
            && let Some(Instruction::ApplyContinuous(spec)) = builder.instructions.last_mut()
            && let spellwake_engine::toolbox::ContinuousPayload::TemporaryRule(rule) =
                &mut spec.payload
            && let TemporaryRuleKind::ZoneCastPermission { sorcery_only, .. } = rule.as_mut()
        {
            *sorcery_only = true;
            i += 1;
            continue;
        }
        // Fold LookTopLibrary("exiled" | "ref") after prior exile/draw/look/search
        if let InstructionAst::LookTopLibrary { library, .. } = &ins[i] {
            let ll = library.trim().to_ascii_lowercase();
            if (ll == "exiled"
                && builder.instructions.iter().rev().any(|op| {
                    matches!(op, Instruction::ExileTopPlay { .. })
                        || super::super::toolbox_emit::unwrap_zone_op(op).is_some_and(|spec| {
                            matches!(
                                spec.destination,
                                spellwake_engine::toolbox::ZoneDest::Exile { .. }
                            )
                        })
                }))
                || (ll == "ref"
                    && builder.instructions.iter().rev().any(|op| {
                        matches!(
                            op,
                            Instruction::LookTopChooseExt { .. }
                                | Instruction::SearchLibrary { .. }
                                | Instruction::DrawCards(_)
                        ) || super::super::toolbox_emit::unwrap_zone_op(op).is_some_and(|spec| {
                            matches!(
                                &spec.source,
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Library(_),
                                    ..
                                }
                            )
                        })
                    }))
            {
                i += 1;
                continue;
            }
        }
        // Standalone LookTopLibrary(N, reveal: true|false) -> LookTopChoose(reveal_only: true)
        if let InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } = &ins[i]
        {
            let lib_low = library.trim().to_ascii_lowercase();
            let owner_opt = parse_look_top_library_owner(library, builder, in_trigger);
            if owner_opt.is_none()
                && matches!(
                    lib_low.as_str(),
                    "their" | "their library" | "that player" | "that player's" | "thatplayer"
                )
            {
                return Err("oracle:unbound-that-player");
            }
            if let Some((req_bt, library_owner)) = owner_opt {
                if let Some(bt) = req_bt {
                    builder.bind_target(bt)?;
                }
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                builder.instructions.push(Instruction::LookTopChooseExt {
                    library_owner,
                    chooser: ResolvingPlayer::Controller,
                    count: cnt,
                    filter: None,
                    max_picks: 0,
                    take_all_matching: false,
                    keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                    rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                    reveal_kept: false,
                    reveal_all: *reveal,
                    reveal_only: true,
                    secondary: None,
                });
                i += 1;
                continue;
            }
        }
        // Fuse RevealHand / LookAtHand (target opponent/player) + optional ChooseOption("<filter> from it")
        // + Discard / Exile(chosen card) -> ChooseRevealedHandCard
        let hand_reveal_player = match &ins[i] {
            InstructionAst::RevealHand {
                player: p @ (PlayerRefAst::TargetOpponent | PlayerRefAst::TargetPlayer),
            }
            | InstructionAst::LookAtHand {
                player: p @ (PlayerRefAst::TargetOpponent | PlayerRefAst::TargetPlayer),
            } => Some(*p),
            _ => None,
        };
        if let Some(reveal_player) = hand_reveal_player {
            let mut fused_step = 0usize;
            let mut chosen_spec: Option<(u8, Option<ObjectFilterAst>, ChosenHandAction)> = None;
            let mut optional_choose = false;
            let mut trailing_target_draw: Option<ValueExpr> = None;
            // 3-step library move: LookAtHand/RevealHand + ChooseOption/InteractiveChoice + MoveZone(anaphora -> top/bottom of that player's library)
            // using SourceZoneFilter::TargetPlayerHand (e.g. Painful Memories, Agonizing Memories).
            if i + 2 < ins.len()
                && let Some(core_desc) = interactive::extract_revealed_hand_choice_desc(&ins[i + 1])
                && let InstructionAst::MoveZone { subject, to } = &ins[i + 2]
                && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            {
                let to_low = to.trim().to_ascii_lowercase();
                let placement = if to_low.contains("library") && to_low.contains("top") {
                    Some(LibraryPlacement::Top)
                } else if to_low.contains("library") && to_low.contains("bottom") {
                    Some(if to_low.contains("random") {
                        LibraryPlacement::BottomRandom
                    } else {
                        LibraryPlacement::Bottom
                    })
                } else {
                    None
                };
                if let Some(lib_place) = placement {
                    let core = core_desc.trim();
                    let (pick_n, flt_raw) = if let Some(r) = core.strip_prefix("two ") {
                        (2i64, format!("a {r}"))
                    } else if let Some(r) = core.strip_prefix("2 ") {
                        (2i64, format!("a {r}"))
                    } else if let Some(r) = core.strip_prefix("three ") {
                        (3i64, format!("a {r}"))
                    } else {
                        (1i64, core.to_string())
                    };
                    if let Ok(mut ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                        &flt_raw,
                        &cached_compiler_vocab(vocabulary),
                    ) && matches!(ast_f.zone, None | Some(ZoneFilter::ThatPlayerHand))
                        && ast_f.controller.is_none()
                        && ast_f.named.is_none()
                    {
                        ast_f.zone = None;
                        let obj_res = if is_unrestricted_filter(&ast_f) {
                            Ok(ObjectFilter::any())
                        } else {
                            lower_object_filter(&ast_f, vocabulary)
                        };
                        if let Ok(obj_filter) = obj_res {
                            builder.bind_target(
                                if reveal_player == PlayerRefAst::TargetOpponent {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                },
                            )?;
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::select_zone_cards_to_zone(ResolvingPlayer::Controller, SourceZoneFilter::TargetPlayerHand, obj_filter, ValueExpr::literal(pick_n), ValueExpr::literal(pick_n), SubjectDestination::Library(lib_place), false));
                            i += 3;
                            continue;
                        }
                    }
                }
            }
            // 3-step pattern: RevealHand/LookAtHand + [Optional?]ChooseOption/InteractiveChoice("<count?> <filter> from it/them")
            // + Discard/Exile(anaphora) or IfPreviousSucceeded([Discard(anaphora), Draw(that_player, N)])
            if i + 2 < ins.len() {
                let (opt_flag, core_desc_opt) = match &ins[i + 1] {
                    InstructionAst::Optional(inner) if inner.len() == 1 => (
                        true,
                        interactive::extract_revealed_hand_choice_desc(&inner[0]),
                    ),
                    other => (false, interactive::extract_revealed_hand_choice_desc(other)),
                };
                if let Some(core_desc) = core_desc_opt {
                    let match_single_discard = |op: &InstructionAst| -> bool {
                        matches!(
                            op,
                            InstructionAst::Discard {
                                player: PlayerRefAst::ThatPlayer
                                    | PlayerRefAst::TargetOpponent
                                    | PlayerRefAst::TargetPlayer,
                                count,
                                filter: None,
                                anaphora: _,
                                random: false,
                            } if count.is_none()
                                || matches!(count.as_deref(), Some(ValueExprAst::Const(1)))
                        )
                    };
                    let (action_opt, draw_opt) = match &ins[i + 2] {
                        op if match_single_discard(op) => (Some(ChosenHandAction::Discard), None),
                        InstructionAst::Exile {
                            actor: None | Some(PlayerRefAst::You),
                            subject,
                            random: false,
                            face_down: false,
                            into_pile: false,
                        } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                            (Some(ChosenHandAction::Exile), None)
                        }
                        InstructionAst::IfPreviousSucceeded(then_body) if opt_flag => {
                            match then_body.as_slice() {
                                [op] if match_single_discard(op) => {
                                    (Some(ChosenHandAction::Discard), None)
                                }
                                [
                                    op,
                                    InstructionAst::Draw {
                                        player:
                                            PlayerRefAst::ThatPlayer
                                            | PlayerRefAst::TargetOpponent
                                            | PlayerRefAst::TargetPlayer,
                                        count,
                                        up_to: false,
                                    },
                                ] if match_single_discard(op) => {
                                    if let Ok(draw_amt) = lower_value_expr_in_context(
                                        resolve_x(count, bound_x),
                                        builder,
                                        in_trigger,
                                        vocabulary,
                                    ) {
                                        (Some(ChosenHandAction::Discard), Some(draw_amt))
                                    } else {
                                        (None, None)
                                    }
                                }
                                _ => (None, None),
                            }
                        }
                        _ => (None, None),
                    };
                    if let Some(action) = action_opt {
                        let core = core_desc.trim();
                        let (pick_n, flt_raw) = if let Some(r) = core.strip_prefix("two ") {
                            (2u8, format!("a {r}"))
                        } else if let Some(r) = core.strip_prefix("2 ") {
                            (2u8, format!("a {r}"))
                        } else if let Some(r) = core.strip_prefix("three ") {
                            (3u8, format!("a {r}"))
                        } else {
                            (1u8, core.to_string())
                        };
                        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                            &flt_raw,
                            &cached_compiler_vocab(vocabulary),
                        ) {
                            chosen_spec = Some((pick_n, Some(ast_f), action));
                            optional_choose = opt_flag;
                            trailing_target_draw = draw_opt;
                            fused_step = 3;
                        }
                    }
                }
            }
            // 2-step pattern: RevealHand/LookAtHand + Discard/Exile(chosen card)
            if chosen_spec.is_none() && i + 1 < ins.len() {
                match &ins[i + 1] {
                    InstructionAst::Discard {
                        player:
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::TargetOpponent
                            | PlayerRefAst::TargetPlayer,
                        count,
                        filter,
                        anaphora: _,
                        random: false,
                    } => {
                        let cnt_u8: Option<u8> = match count.as_deref() {
                            None => Some(1),
                            Some(ValueExprAst::Const(n)) if *n > 0 => u8::try_from(*n).ok(),
                            _ => None,
                        };
                        if let Some(n) = cnt_u8 {
                            chosen_spec =
                                Some((n, filter.as_deref().cloned(), ChosenHandAction::Discard));
                            fused_step = 2;
                        }
                    }
                    InstructionAst::Exile {
                        actor: None | Some(PlayerRefAst::You),
                        subject,
                        random: false,
                        face_down: false,
                        into_pile: false,
                    } => match subject.as_ref() {
                        SubjectRefAst::Anaphora(_) => {
                            chosen_spec = Some((1, None, ChosenHandAction::Exile));
                            fused_step = 2;
                        }
                        SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::ThatPlayerHand)) =>
                        {
                            chosen_spec =
                                Some((1, Some(f.as_ref().clone()), ChosenHandAction::Exile));
                            fused_step = 2;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            if let Some((n, filter_opt, action)) = chosen_spec
                && let Some(nz) = NonZeroU8::new(n)
            {
                let lowered_filter: Result<(bool, u16, Option<Box<ObjectFilter>>), &'static str> =
                    match filter_opt {
                        None => Ok((false, 0, None)),
                        Some(mut f) => {
                            if !matches!(f.zone, None | Some(ZoneFilter::ThatPlayerHand))
                                || f.controller.is_some()
                                || f.named.is_some()
                            {
                                Err("oracle:unsupported-revealed-hand-filter")
                            } else {
                                f.zone = None;
                                if is_unrestricted_filter(&f) {
                                    Ok((false, 0, None))
                                } else {
                                    lower_object_filter(&f, vocabulary)
                                        .map(|obj| (false, 0, Some(Box::new(obj))))
                                }
                            }
                        }
                    };
                if let Ok((nonland_only, minimum_mana_value, filter)) = lowered_filter {
                    builder.bind_target(if reveal_player == PlayerRefAst::TargetOpponent {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    if optional_choose {
                        if trailing_target_draw.is_some() {
                            builder.instructions.push(Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::Controller,
                                instructions: 2,
                            });
                        } else {
                            builder.instructions.push(Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::Controller,
                            });
                        }
                    }
                    builder
                        .instructions
                        .push(Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                                minimum_mana_value,
                                nonland_only,
                                filter,
                                nz,
                                action,
                            ),
                        )));
                    if let Some(draw_amt) = trailing_target_draw {
                        builder
                            .instructions
                            .push(super::super::toolbox_emit::draw_target_player(draw_amt));
                    }
                    i += fused_step;
                    continue;
                }
            }
        }
        // Fuse AddMana (or BindX([AddMana])) + [optional 1 side-effect or retention] + ManaSpendRestriction (+ optional retention)
        let mana_fuse = match &ins[i] {
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana,
            } => Some((mana, bound_x)),
            InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                [
                    InstructionAst::AddMana {
                        player: PlayerRefAst::You,
                        mana,
                    },
                ] => Some((mana, Some(x_expr.as_ref()))),
                _ => None,
            },
            _ => None,
        };
        let mana_restr_lookahead = if mana_fuse.is_some() {
            if let Some(InstructionAst::ManaSpendRestriction(r)) = ins.get(i + 1) {
                parse_mana_restriction(r, vocabulary).map(|res| {
                    let retain = ins.get(i + 2).is_some_and(is_mana_retention_instruction);
                    (res, if retain { 3usize } else { 2usize }, None, retain)
                })
            } else if ins.get(i + 1).is_some_and(is_mana_retention_instruction)
                && let Some(InstructionAst::ManaSpendRestriction(r)) = ins.get(i + 2)
            {
                parse_mana_restriction(r, vocabulary).map(|res| (res, 3usize, None, true))
            } else if let Some(InstructionAst::ManaSpendRestriction(r)) = ins.get(i + 2)
                && !matches!(
                    ins.get(i + 1),
                    Some(
                        InstructionAst::AddMana { .. }
                            | InstructionAst::ManaSpendRestriction(_)
                            | InstructionAst::RetainManaUntilEndOfTurn
                    )
                )
            {
                parse_mana_restriction(r, vocabulary).map(|res| {
                    let retain = ins.get(i + 3).is_some_and(is_mana_retention_instruction);
                    (
                        res,
                        if retain { 4usize } else { 3usize },
                        ins.get(i + 1),
                        retain,
                    )
                })
            } else {
                None
            }
        } else {
            None
        };
        if let (Some((mana, eff_bound_x)), Some((restriction, step_len, intervening, retain))) =
            (mana_fuse, mana_restr_lookahead)
        {
            let mut fused_mana = false;
            match mana {
                ManaSpecAst::Fixed(raw) => {
                    if let Some(bundle) = parse_mana_bundle(raw) {
                        builder
                            .instructions
                            .push(Instruction::MutatePlayer(Box::new(
                                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                    ResolvingPlayer::Controller,
                                    EffectManaYield::RestrictedFixed {
                                        bundle,
                                        restriction,
                                    },
                                ),
                            )));
                        fused_mana = true;
                    }
                }
                ManaSpecAst::Choice(opts) if opts.len() >= 2 => {
                    let all_single: Option<BTreeSet<ManaType>> =
                        opts.iter().map(|opt| parse_single_mana_type(opt)).collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                        && !colors.contains(&ManaType::Colourless)
                    {
                        builder
                            .instructions
                            .push(Instruction::MutatePlayer(Box::new(
                                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                    ResolvingPlayer::Controller,
                                    EffectManaYield::RestrictedChoice {
                                        colors,
                                        amount: ValueExpr::literal(1),
                                        restriction,
                                    },
                                ),
                            )));
                        fused_mana = true;
                    } else if let Some(bundles) = opts
                        .iter()
                        .map(|opt| parse_mana_bundle(opt))
                        .collect::<Option<Vec<_>>>()
                        && let Some(first) = bundles.first()
                        && first.len() == 1
                        && let Some(qty) = first.values().next().cloned()
                    {
                        let mut colors = BTreeSet::new();
                        let mut ok = true;
                        for b in &bundles {
                            if b.len() != 1 || b.values().next() != Some(&qty) {
                                ok = false;
                                break;
                            }
                            if let Some(&mt) = b.keys().next() {
                                if mt == ManaType::Colourless {
                                    ok = false;
                                    break;
                                }
                                colors.insert(mt);
                            }
                        }
                        if ok && !colors.is_empty() {
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                        ResolvingPlayer::Controller,
                                        EffectManaYield::RestrictedChoice {
                                            colors,
                                            amount: ValueExpr::Literal(qty),
                                            restriction,
                                        },
                                    ),
                                )));
                            fused_mana = true;
                        }
                    }
                }
                ManaSpecAst::AnyColor {
                    amount,
                    single_color,
                } => {
                    if let Ok(amt) = lower_value_expr(resolve_x(amount, eff_bound_x), vocabulary) {
                        let all_colors = BTreeSet::from([
                            ManaType::White,
                            ManaType::Blue,
                            ManaType::Black,
                            ManaType::Red,
                            ManaType::Green,
                        ]);
                        if *single_color {
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                        ResolvingPlayer::Controller,
                                        EffectManaYield::RestrictedChoice {
                                            colors: all_colors,
                                            amount: amt,
                                            restriction,
                                        },
                                    ),
                                )));
                        } else {
                            builder.instructions.push(Instruction::RepeatLoop {
                                count: amt,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::MutatePlayer(Box::new(
                                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                            ResolvingPlayer::Controller,
                                            EffectManaYield::RestrictedChoice {
                                                colors: all_colors,
                                                amount: ValueExpr::literal(1),
                                                restriction,
                                            },
                                        ),
                                    ))],
                                )),
                            });
                        }
                        fused_mana = true;
                    }
                }
                ManaSpecAst::CombinationOf { amount, symbols } if symbols.len() >= 2 => {
                    let all_single: Option<BTreeSet<ManaType>> = symbols
                        .iter()
                        .map(|opt| parse_single_mana_type(opt))
                        .collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                        && !colors.contains(&ManaType::Colourless)
                        && let Ok(amt) =
                            lower_value_expr(resolve_x(amount, eff_bound_x), vocabulary)
                    {
                        if amt == ValueExpr::literal(1) {
                            builder
                                .instructions
                                .push(Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                        ResolvingPlayer::Controller,
                                        EffectManaYield::RestrictedChoice {
                                            colors,
                                            amount: ValueExpr::literal(1),
                                            restriction,
                                        },
                                    ),
                                )));
                        } else {
                            builder.instructions.push(Instruction::RepeatLoop {
                                count: amt,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::MutatePlayer(Box::new(
                                        spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                            ResolvingPlayer::Controller,
                                            EffectManaYield::RestrictedChoice {
                                                colors,
                                                amount: ValueExpr::literal(1),
                                                restriction,
                                            },
                                        ),
                                    ))],
                                )),
                            });
                        }
                        fused_mana = true;
                    }
                }
                _ => {}
            }
            if fused_mana {
                if retain && let Some(last_op) = builder.instructions.last_mut() {
                    retain_mana_in_instruction(last_op);
                }
                if let Some(mid) = intervening {
                    lower_instruction(
                        mid,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                i += step_len;
                continue;
            }
        }
        // Fold ManaSpendRestriction onto a preceding AddMana / AddManaForPlayer or impulse play permission.
        if let InstructionAst::ManaSpendRestriction(r) = &ins[i] {
            if let Some(restriction) = parse_mana_restriction(r, vocabulary) {
                let mut patched = false;
                for op in builder.instructions.iter_mut().rev() {
                    if interactive::restrict_mana_in_instruction(op, &restriction) {
                        patched = true;
                        break;
                    }
                    if matches!(
                        op,
                        Instruction::BranchIfFalse { .. }
                            | Instruction::BranchCondition { .. }
                            | Instruction::Jump { .. }
                    ) {
                        break;
                    }
                }
                if patched {
                    i += 1;
                    continue;
                }
            } else if is_any_color_spend_restriction(&ins[i]) {
                let mut patched = false;
                for op in builder.instructions.iter_mut().rev() {
                    if interactive::patch_any_color_cast_permission(op) {
                        patched = true;
                        break;
                    }
                    if matches!(
                        op,
                        Instruction::BranchIfFalse { .. }
                            | Instruction::BranchCondition { .. }
                            | Instruction::Jump { .. }
                    ) {
                        break;
                    }
                }
                if patched {
                    i += 1;
                    continue;
                }
            }
        }
        // Fold RetainManaUntilEndOfTurn (or retention ManaSpendRestriction) onto prior straight-line AddMana
        if is_mana_retention_instruction(&ins[i]) {
            let mut patched = false;
            for op in builder.instructions.iter_mut().rev() {
                if retain_mana_in_instruction(op) {
                    patched = true;
                    break;
                }
                if matches!(
                    op,
                    Instruction::BranchIfFalse { .. }
                        | Instruction::BranchCondition { .. }
                        | Instruction::Jump { .. }
                        | Instruction::OptionalEffect { .. }
                        | Instruction::OptionalSequence { .. }
                ) {
                    break;
                }
            }
            if patched {
                i += 1;
                continue;
            }
        }
        // Fuse CreateToken (1 untapped non-attacking controlled token) + Attach(self -> it) -> CreateTokenAndAttach
        if i + 1 < ins.len()
            && let InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token,
            } = &ins[i]
            && let InstructionAst::Attach {
                subject,
                target,
                any_number: false,
                all: false,
            } = &ins[i + 1]
            && is_self_pronoun(subject, builder, self_pronoun_valid)
            && matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
            && let Ok((creation, false)) = lower_token_spec(
                token,
                TokenRecipients::Controller,
                vocabulary,
                ids,
                bound_x,
                Some((builder, in_trigger)),
            )
            && !creation.enters_tapped
            && creation.amount == ValueExpr::literal(1)
            && creation.base_size.is_none()
        {
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::create_token_and_attach(Box::new(
                        creation.definition,
                    )),
                )));
            i += 2;
            continue;
        }
        // Fuse bite spells: "Target creature ... deals damage equal to its power to target creature/planeswalker/permanent."
        // (+ optional prefix instructions on target A like "+1/+1", and optional trample excess damage)
        if let InstructionAst::DealDamage {
            source,
            target,
            amount,
        } = &ins[i]
            && matches!(
                amount.as_ref(),
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            )
            && let SubjectRefAst::TargetObject {
                filter: dst_f,
                count: TargetCountAst::Exact(1),
            } = target.as_ref()
            && dst_f.zone.is_none()
            && let Ok(mut dst_bt) = lower_battlefield_body_target(dst_f, vocabulary)
            && matches!(
                dst_bt,
                BodyTarget::SingleCreature
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::SinglePermanent
                    | BodyTarget::FilteredPermanent(_)
            )
        {
            if !in_trigger && dst_f.controller == Some(ControllerRelation::ThatPlayer) {
                rewrite_body_target_related_to_first_target_controller(&mut dst_bt);
            }
            let trample_overflow = i + 1 < ins.len()
                && matches!(
                    &ins[i + 1],
                    InstructionAst::AbilityRestriction {
                        restriction,
                        condition: None,
                    } if restriction == "ExcessDamageToController"
                );
            if let SubjectRefAst::TargetObject {
                filter: src_f,
                count: TargetCountAst::Exact(1),
            } = source.as_ref()
                && src_f.zone.is_none()
                && let Ok(src_bt) = lower_battlefield_body_target(src_f, vocabulary)
                && src_bt.guarantees_creature()
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
            {
                let fixed = |bt: BodyTarget| {
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        bt,
                        vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                    )))
                };
                builder.instructions.push(fixed(src_bt));
                builder.instructions.push(fixed(dst_bt));
                builder.instructions.push(Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(
                        trample_overflow,
                    ),
                )));
                i += if trample_overflow { 2 } else { 1 };
                continue;
            }
            if matches!(source.as_ref(), SubjectRefAst::Anaphora(_))
                && builder.target.guarantees_creature()
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.up_to_x_target_count
                && !builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::FixedTargetClause(_)
                            | Instruction::AffectPermanentTargets { .. }
                    ) || matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                            )
                    )
                })
            {
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
                            dst_bt,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                builder.instructions.push(Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(
                        trample_overflow,
                    ),
                )));
                i += if trample_overflow { 2 } else { 1 };
                continue;
            }
        }
        // Fuse Copy (+ Optional(ChangeTargets) / MayChooseNewTargetsForCopy / AbilityRestriction)
        // directly into Instruction::StackOp(StackOpSpec::copy_stack(target, count, true)).
        if i + 1 < ins.len() && is_choose_new_targets_for_copy(&ins[i + 1]) {
            let copy_parts = match &ins[i] {
                InstructionAst::Copy {
                    subject,
                    exception: None,
                    count,
                } => Some((false, subject, count)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::Copy {
                            subject,
                            exception: None,
                            count,
                        },
                    ] => Some((true, subject, count)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((is_optional, subject, count)) = copy_parts {
                let mut probe = builder.clone();
                if let Ok(count_expr) = match count {
                    Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary),
                    None => Ok(ValueExpr::literal(1)),
                } && let Ok(stack_copy_target) = resolve_stack_copy_target(
                    subject,
                    &mut probe,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                ) {
                    *builder = probe;
                    if is_optional {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    builder.instructions.push(Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::copy_stack(
                            stack_copy_target,
                            count_expr,
                            true,
                        ),
                    )));
                    i += 2;
                    continue;
                }
            }
        }
        // Fold standalone ChangeTargets("the copy"|"the copies"|"that copy") onto a preceding
        // CopyStackObject (including inside a preceding BranchIfFalse or ScheduleDelayedEventTrigger).
        if is_choose_new_targets_for_copy(&ins[i])
            && patch_last_copy_choose_new_targets(&mut builder.instructions)
        {
            i += 1;
            continue;
        }
        // Fuse 2-target Exchange(Control) (direct or Optional) into FixedTargetClause pair + Exchange(Control).
        if builder.target == BodyTarget::None && builder.instructions.is_empty() {
            let exch_parts = match &ins[i] {
                InstructionAst::Exchange {
                    kind: spellwake_oracle_compiler::ast::ExchangeKind::Control,
                    left,
                    right,
                } => Some((false, left, right)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::Exchange {
                            kind: spellwake_oracle_compiler::ast::ExchangeKind::Control,
                            left,
                            right,
                        },
                    ] => Some((true, left, right)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((is_optional, left, right)) = exch_parts {
                let targets_pair = match (left.as_ref(), right.as_deref()) {
                    (
                        SubjectRefAst::TargetObject {
                            filter: fa,
                            count: TargetCountAst::Exact(1),
                        },
                        Some(SubjectRefAst::TargetObject {
                            filter: fb,
                            count: TargetCountAst::Exact(1),
                        }),
                    ) if fa.zone.is_none() && fb.zone.is_none() => {
                        match (
                            lower_battlefield_body_target(fa, vocabulary),
                            lower_battlefield_body_target(fb, vocabulary),
                        ) {
                            (Ok(bt_a), Ok(mut bt_b)) => {
                                if !in_trigger
                                    && fb.controller == Some(ControllerRelation::ThatPlayer)
                                {
                                    rewrite_body_target_related_to_first_target_controller(
                                        &mut bt_b,
                                    );
                                }
                                Some((bt_a, bt_b))
                            }
                            _ => None,
                        }
                    }
                    (
                        SubjectRefAst::TargetObject {
                            filter: fa,
                            count: TargetCountAst::Exact(2),
                        },
                        None,
                    ) if fa.zone.is_none() => lower_battlefield_body_target(fa, vocabulary)
                        .ok()
                        .map(|bt| (bt, bt)),
                    (SubjectRefAst::Compound(items), None) if items.len() == 2 => {
                        match (&items[0], &items[1]) {
                            (
                                SubjectRefAst::TargetObject {
                                    filter: fa,
                                    count: TargetCountAst::Exact(1),
                                },
                                SubjectRefAst::TargetObject {
                                    filter: fb,
                                    count: TargetCountAst::Exact(1),
                                },
                            ) if fa.zone.is_none() && fb.zone.is_none() => {
                                match (
                                    lower_battlefield_body_target(fa, vocabulary),
                                    lower_battlefield_body_target(fb, vocabulary),
                                ) {
                                    (Ok(bt_a), Ok(mut bt_b)) => {
                                        if !in_trigger
                                            && fb.controller == Some(ControllerRelation::ThatPlayer)
                                        {
                                            rewrite_body_target_related_to_first_target_controller(
                                                &mut bt_b,
                                            );
                                        }
                                        Some((bt_a, bt_b))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((bt_a, bt_b)) = targets_pair {
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_a, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))]),
                        )));
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_b, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))]),
                        )));
                    if is_optional {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    builder.instructions.push(Instruction::Exchange(
                        spellwake_engine::program::ExchangeSpec::Control {
                            first: Box::new(MoveSubjectRef::RememberedFirst),
                            second: Box::new(MoveSubjectRef::RememberedSecond),
                        },
                    ));
                    i += 1;
                    continue;
                }
            }
        }
        // Fuse 2-target Attach (direct, Optional, or prefix-on-target-A + Attach) into FixedTargetClause pair
        // + MutateObject(Attach) (e.g. Brass Squire, Kor Outfitter, Crown of the Ages).
        let attach_parts = match &ins[i] {
            InstructionAst::Attach {
                subject,
                target,
                any_number: false,
                all: false,
            } => Some((false, subject, target)),
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [
                    InstructionAst::Attach {
                        subject,
                        target,
                        any_number: false,
                        all: false,
                    },
                ] => Some((true, subject, target)),
                _ => None,
            },
            _ => None,
        };
        if let Some((is_optional_attach, att_subj, att_host)) = attach_parts {
            if let (
                SubjectRefAst::TargetObject {
                    filter: fa,
                    count: TargetCountAst::Exact(1),
                },
                SubjectRefAst::TargetObject {
                    filter: fb,
                    count: TargetCountAst::Exact(1),
                },
            ) = (att_subj.as_ref(), att_host.as_ref())
                && matches!(fa.zone, None | Some(ZoneFilter::Battlefield))
                && matches!(fb.zone, None | Some(ZoneFilter::Battlefield))
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
            {
                let mut inner_a = fa.as_ref().clone();
                inner_a.zone = None;
                let mut inner_b = fb.as_ref().clone();
                inner_b.zone = None;
                if let (Ok(bt_a), Ok(mut bt_b)) = (
                    lower_battlefield_body_target(&inner_a, vocabulary),
                    lower_battlefield_body_target(&inner_b, vocabulary),
                ) {
                    if !in_trigger && inner_b.controller == Some(ControllerRelation::ThatPlayer) {
                        rewrite_body_target_related_to_first_target_controller(&mut bt_b);
                    }
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_a, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))]),
                        )));
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_b, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))]),
                        )));
                    if is_optional_attach {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::attach_query(
                            super::super::toolbox_emit::move_subject_to_query(
                                MoveSubjectRef::RememberedFirst,
                            ),
                            super::super::toolbox_emit::move_subject_to_query(
                                MoveSubjectRef::RememberedSecond,
                            ),
                        ));
                    i += 1;
                    continue;
                }
            }
            let prefix_attach = match (att_subj.as_ref(), att_host.as_ref()) {
                (
                    SubjectRefAst::Anaphora(_),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                ) if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    Some((filter, false))
                }
                (
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                    SubjectRefAst::Anaphora(_),
                ) if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    Some((filter, true))
                }
                _ => None,
            };
            if let Some((fb, swapped)) = prefix_attach
                && matches!(
                    builder.target,
                    BodyTarget::SinglePermanent
                        | BodyTarget::SingleCreature
                        | BodyTarget::SinglePlaneswalker
                        | BodyTarget::FilteredPermanent(_)
                )
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.up_to_x_target_count
                && !builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::FixedTargetClause(_)
                            | Instruction::AffectPermanentTargets { .. }
                    ) || matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                            )
                    )
                })
            {
                let mut inner_b = fb.as_ref().clone();
                inner_b.zone = None;
                if let Ok(mut bt_b) = lower_battlefield_body_target(&inner_b, vocabulary) {
                    if !in_trigger && inner_b.controller == Some(ControllerRelation::ThatPlayer) {
                        rewrite_body_target_related_to_first_target_controller(&mut bt_b);
                    }
                    let mut first_ops = std::mem::take(&mut builder.instructions);
                    prepend_instruction_at(
                        &mut first_ops,
                        0,
                        Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
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
                            ExecutableBody::new(bt_b, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target()))]),
                        )));
                    if is_optional_attach {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    let (attachment, host) = if swapped {
                        (
                            MoveSubjectRef::RememberedSecond,
                            MoveSubjectRef::RememberedFirst,
                        )
                    } else {
                        (
                            MoveSubjectRef::RememberedFirst,
                            MoveSubjectRef::RememberedSecond,
                        )
                    };
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::attach_query(
                            super::super::toolbox_emit::move_subject_to_query(attachment),
                            super::super::toolbox_emit::move_subject_to_query(host),
                        ));
                    i += 1;
                    continue;
                }
            }
        }
        // Fuse 2-target Fight, prefix-on-target-A + Fight, Optional([Fight]), and hybrid trigger/attached Fight
        let fight_parts = match &ins[i] {
            InstructionAst::Fight { a, b } => Some((false, a, b)),
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [InstructionAst::Fight { a, b }] => Some((true, a, b)),
                _ => None,
            },
            _ => None,
        };
        if let Some((is_optional_fight, a, b)) = fight_parts {
            if let (
                SubjectRefAst::TargetObject {
                    filter: fa,
                    count: TargetCountAst::Exact(1),
                },
                SubjectRefAst::TargetObject {
                    filter: fb,
                    count: TargetCountAst::Exact(1),
                },
            ) = (a.as_ref(), b.as_ref())
                && fa.zone.is_none()
                && fb.zone.is_none()
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
                && let Ok(bt_a) = lower_battlefield_body_target(fa, vocabulary)
                && let Ok(mut bt_b) = lower_battlefield_body_target(fb, vocabulary)
                && bt_a.guarantees_creature()
                && bt_b.guarantees_creature()
            {
                if !in_trigger && fb.controller == Some(ControllerRelation::ThatPlayer) {
                    rewrite_body_target_related_to_first_target_controller(&mut bt_b);
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
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                } else {
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                }
                i += 1;
                continue;
            }
            let other_target = match (a.as_ref(), b.as_ref()) {
                (
                    SubjectRefAst::Anaphora(_),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                )
                | (
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                    SubjectRefAst::Anaphora(_),
                ) if filter.zone.is_none() => Some(filter),
                _ => None,
            };
            if let Some(fb) = other_target
                && builder.target.guarantees_creature()
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.up_to_x_target_count
                && !builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::FixedTargetClause(_)
                            | Instruction::AffectPermanentTargets { .. }
                    ) || matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                            )
                    )
                })
                && let Ok(mut bt_b) = lower_battlefield_body_target(fb, vocabulary)
                && bt_b.guarantees_creature()
            {
                if !in_trigger && fb.controller == Some(ControllerRelation::ThatPlayer) {
                    rewrite_body_target_related_to_first_target_controller(&mut bt_b);
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
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                } else {
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                }
                i += 1;
                continue;
            }
            // Hybrid 1-target Fight (triggering creature / attached host vs target creature,
            // or ability source vs already-targeted creature via anaphora).
            let hybrid_target_pair = match (a.as_ref(), b.as_ref()) {
                (
                    lhs,
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                )
                | (
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                    lhs,
                ) if filter.zone.is_none()
                    && !is_self_pronoun(lhs, builder, self_pronoun_valid) =>
                {
                    match lhs {
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some((MoveSubjectRef::TriggeringObject, filter))
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && in_trigger
                                && !self_pronoun_valid =>
                        {
                            Some((MoveSubjectRef::TriggeringObject, filter))
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            Some((MoveSubjectRef::AttachedHost, filter))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((first_ref, tgt_f)) = hybrid_target_pair
                && let Ok(bt) = lower_battlefield_body_target(tgt_f, vocabulary)
                && bt.guarantees_creature()
                && builder.target == BodyTarget::None
            {
                builder.bind_target(bt)?;
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                }
                builder.instructions.push(Instruction::Fight {
                    first: Box::new(first_ref),
                    second: Box::new(MoveSubjectRef::TargetObject),
                });
                i += 1;
                continue;
            }
            if (self_pronoun_valid || in_trigger)
                && builder.target.guarantees_creature()
                && matches!(
                    (a.as_ref(), b.as_ref()),
                    (SubjectRefAst::SelfObject, SubjectRefAst::Anaphora(_))
                        | (SubjectRefAst::Anaphora(_), SubjectRefAst::SelfObject)
                )
            {
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                }
                builder.instructions.push(Instruction::Fight {
                    first: Box::new(MoveSubjectRef::AbilitySource),
                    second: Box::new(MoveSubjectRef::TargetObject),
                });
                i += 1;
                continue;
            }
        }
        // Fuse AnimateOrSetBasePT (+ optional KeepExistingTypes or "it's still a land") -> ResolvedPartSpec::SetPT + Type + Color + Abilities
        if let InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            duration,
        } = &ins[i]
        {
            let keep_types = i + 1 < ins.len()
                && (matches!(&ins[i + 1], InstructionAst::KeepExistingTypes(_))
                    || is_still_a_land_restriction(&ins[i + 1]));
            lower_animate_or_set_pt(
                subject,
                power,
                toughness,
                description.as_deref(),
                keywords,
                duration,
                keep_types,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += if keep_types { 2 } else { 1 };
            continue;
        }
        // Fuse ExtremeLifePlayerAction + recognized tie-breaker clause (Loxodon Peacekeeper,
        // Celestial Convergence). Fail closed whenever the tie-breaker is absent or unrecognized.
        if let InstructionAst::ExtremeLifePlayerAction { highest, action } = &ins[i] {
            let Some(tie_mode) = ins
                .get(i + 1)
                .and_then(|next| parse_extreme_life_tie_mode(next, *highest))
            else {
                return Err("oracle:unsupported-extreme-life-tie");
            };
            if builder.target != BodyTarget::None {
                return Err("oracle:unsupported-extreme-life-tie");
            }
            let sentinel_idx = builder.instructions.len();
            builder.instructions.push(Instruction::ChooseValue {
                chooser: ResolvingPlayer::Controller,
                domain: spellwake_engine::program::ResolutionChoiceDomain::Player {
                    opponents_only: false,
                },
            });
            lower_instruction(
                action,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if let Some(slot) = builder.instructions.get_mut(sentinel_idx) {
                *slot = Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::select_extreme_life(
                        *highest, tie_mode,
                    ),
                ));
            }
            i += 2;
            continue;
        }
        // Unwrap Optional([DiscardUpTo { all: false, .. }]) (e.g. Fable of the Mirror-Breaker Ch. II)
        // directly into DiscardUpToExt (min = 0) without emitting a redundant Yes/No prompt.
        if let InstructionAst::Optional(inner) = &ins[i]
            && let [d @ InstructionAst::DiscardUpTo { all: false, .. }] = inner.as_slice()
        {
            lower_instruction(
                d,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += 1;
            continue;
        }
        // Auto-wrap EachPlayer / EachOpponent / EachOtherPlayer instructions whose nested
        // expression references ThatPlayer (e.g. Acidic Soil, Price of Progress, Stern Judge,
        // Biorhythm, Folio of Fancies, Nature's Resurgence) into ForEachPlayer.
        if let Some(InstructionAst::ForEachPlayer {
            player: loop_player,
            body: loop_body,
        }) = try_wrap_each_player_that_player(&ins[i])
        {
            let players = match loop_player {
                PlayerRefAst::EachPlayer => AffectedPlayers::EachPlayer,
                PlayerRefAst::EachOpponent => AffectedPlayers::EachOpponent,
                _ => AffectedPlayers::EachOtherPlayer,
            };
            let mut probe = builder.clone();
            if let Ok(sub) = super::super::instructions::lower_loop_player_sub_body(
                &loop_body,
                &mut probe,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            ) {
                probe.instructions.push(Instruction::ForEachPlayerQuery {
                    selector: Box::new(spellwake_engine::toolbox::PlayerSelector::Group(players)),
                    bind_player: spellwake_engine::toolbox::BindingId::LOOP_PLAYER,
                    body: Box::new(sub),
                });
                *builder = probe;
                i += 1;
                continue;
            }
        }
        // Pre-bind TargetPlayer / TargetOpponent when the same instruction or BindX expression
        // references ThatPlayer (e.g. Storm Seeker, Burden of Greed, Keening Stone, Jovial Evil).
        if builder.target == BodyTarget::None
            && let Some((bt, rewritten)) = try_rewrite_target_player_with_that_player(&ins[i])
        {
            let mut probe = builder.clone();
            if probe.bind_target(bt).is_ok()
                && lower_instruction(
                    &rewritten,
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
            {
                *builder = probe;
                i += 1;
                continue;
            }
        }
        // Resolve follow-up ThatPlayer after an EachOpponent / EachPlayer instruction
        // when no single target or trigger player is bound (e.g. Skull Rend).
        if !in_trigger
            && builder.target == BodyTarget::None
            && i + 1 < ins.len()
            && let Some(scope) = try_extract_group_player_scope(&ins[i])
            && let Some(rewritten_next) = try_rewrite_followup_that_player(&ins[i + 1], scope)
        {
            let mut probe = builder.clone();
            if lower_instruction(
                &ins[i],
                &mut probe,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && lower_instruction(
                    &rewritten_next,
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
            {
                *builder = probe;
                i += 2;
                continue;
            }
        }
        match lower_instruction(
            &ins[i],
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ) {
            Ok(()) => {}
            Err("oracle:unsupported-instruction")
                if matches!(&ins[i], InstructionAst::InteractiveChoice(_)) =>
            {
                let InstructionAst::InteractiveChoice(kind) = &ins[i] else {
                    return Err("oracle:unsupported-instruction");
                };
                try_lower_interactive_choice_prompt(kind, builder, vocabulary, in_trigger)?;
            }
            Err(err) => return Err(err),
        }
        i += 1;
    }
    Ok(())
}

fn emit_prompt_choice_for_player(
    chooser: PlayerRefAst,
    domain: spellwake_engine::toolbox::PromptDomain,
    category: spellwake_engine::toolbox::ChoiceCategory,
    auto_resolve_trivial: bool,
    builder: &mut BodyBuilder,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{BindingId, PlayerSelector, PromptChoiceSpec};
    match chooser {
        PlayerRefAst::EachPlayer | PlayerRefAst::EachOpponent | PlayerRefAst::EachOtherPlayer => {
            let players = lower_affected_players_in_context(chooser, builder, in_trigger)?;
            builder.instructions.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(PlayerSelector::Group(players)),
                bind_player: BindingId(0),
                body: Box::new(ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                        chooser: PlayerSelector::Bound(BindingId(0)),
                        domain,
                        category,
                        bind_to: BindingId(1),
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial,
                    }))],
                )),
            });
            Ok(())
        }
        PlayerRefAst::AnyPlayer | PlayerRefAst::AnyOpponent => {
            let sel = if chooser == PlayerRefAst::AnyOpponent {
                PlayerSelector::Group(AffectedPlayers::EachOpponent)
            } else {
                PlayerSelector::Group(AffectedPlayers::EachPlayer)
            };
            builder
                .instructions
                .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                    chooser: PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    domain: spellwake_engine::toolbox::PromptDomain::Players {
                        selector: sel,
                        min: 1,
                        max: 1,
                    },
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    bind_to: BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial: true,
                })));
            builder
                .instructions
                .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                    chooser: PlayerSelector::Bound(BindingId(0)),
                    domain,
                    category,
                    bind_to: BindingId(1),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial,
                })));
            Ok(())
        }
        other => {
            let rp = lower_resolving_player_in_context(other, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                    chooser: PlayerSelector::Resolving(rp),
                    domain,
                    category,
                    bind_to: BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial,
                })));
            Ok(())
        }
    }
}

fn parse_prompt_count_prefix(raw: &str) -> (i64, i64, &str) {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("up to ") {
        let rest = rest.trim();
        for (word, n) in [
            ("one ", 1_i64),
            ("two ", 2),
            ("three ", 3),
            ("four ", 4),
            ("five ", 5),
            ("six ", 6),
            ("seven ", 7),
            ("eight ", 8),
            ("nine ", 9),
            ("ten ", 10),
        ] {
            if let Some(after) = rest.strip_prefix(word) {
                return (0, n, after.trim());
            }
        }
        if let Some((num_s, after)) = rest.split_once(' ')
            && let Ok(n) = num_s.parse::<i64>()
            && n > 0
        {
            return (0, n, after.trim());
        }
    }
    for (word, n) in [
        ("a ", 1_i64),
        ("an ", 1),
        ("one ", 1),
        ("two ", 2),
        ("three ", 3),
        ("four ", 4),
        ("five ", 5),
        ("six ", 6),
        ("seven ", 7),
        ("eight ", 8),
        ("nine ", 9),
        ("ten ", 10),
    ] {
        if let Some(after) = trimmed.strip_prefix(word) {
            return (n, n, after.trim());
        }
    }
    if let Some((num_s, after)) = trimmed.split_once(' ')
        && let Ok(n) = num_s.parse::<i64>()
        && n > 0
    {
        return (n, n, after.trim());
    }
    (1, 1, trimmed)
}

fn try_lower_interactive_choice_prompt(
    kind: &InteractiveChoiceKind,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Result<(), &'static str> {
    use spellwake_engine::toolbox::{
        ChoiceCategory, ObjectQuery, PlayerSelector, PromptDomain, SubtypeChoiceKind,
    };
    match kind {
        InteractiveChoiceKind::SeparateIntoPiles {
            chooser,
            subject,
            pile_count,
        } if *pile_count >= 2 => {
            let query = lower_subject_to_object_query(subject, builder, in_trigger, vocabulary)
                .ok_or("oracle:unsupported-instruction")?;
            emit_prompt_choice_for_player(
                *chooser,
                PromptDomain::Partition {
                    query,
                    pile_count: *pile_count,
                    min_per_pile: 0,
                },
                ChoiceCategory::PileSplit,
                true,
                builder,
                in_trigger,
            )
        }
        InteractiveChoiceKind::BiddingAction { .. } => emit_prompt_choice_for_player(
            PlayerRefAst::EachPlayer,
            PromptDomain::Number {
                min: Box::new(ValueExpr::literal(0)),
                max: Box::new(ValueExpr::literal(20)),
            },
            ChoiceCategory::Generic,
            false,
            builder,
            in_trigger,
        ),
        InteractiveChoiceKind::ChooseFromGroup {
            chooser,
            description,
        } => {
            let low = description
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let cleaned = low.strip_prefix("choose ").unwrap_or(low.as_str()).trim();
            if matches!(cleaned, "a creature type" | "creature type") {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Subtype {
                        kind: SubtypeChoiceKind::Creature,
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(cleaned, "a basic land type" | "basic land type") {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Subtype {
                        kind: SubtypeChoiceKind::BasicLand,
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(cleaned, "a planeswalker type" | "planeswalker type") {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Subtype {
                        kind: SubtypeChoiceKind::Planeswalker,
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(cleaned, "a color" | "color" | "one of those colors") {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Color {
                        allowed: spellwake_engine::program::ColorMask::EMPTY,
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(cleaned, "a card type" | "card type") {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::CardType {
                        allowed: Vec::new(),
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(
                cleaned,
                "a card name" | "card name" | "a nonland card name" | "nonland card name"
            ) {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::CardName {
                        nonland_only: cleaned.contains("nonland"),
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(
                cleaned,
                "a player"
                    | "another player"
                    | "an opponent"
                    | "another opponent"
                    | "one of your opponents"
            ) {
                let selector = if cleaned.contains("opponent") {
                    PlayerSelector::Group(AffectedPlayers::EachOpponent)
                } else if cleaned.contains("another") {
                    PlayerSelector::Group(AffectedPlayers::EachOtherPlayer)
                } else {
                    PlayerSelector::Group(AffectedPlayers::EachPlayer)
                };
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Players {
                        selector,
                        min: 1,
                        max: 1,
                    },
                    ChoiceCategory::Generic,
                    true,
                    builder,
                    in_trigger,
                );
            }
            if let Some(rest) = cleaned.strip_prefix("a number between ")
                && let Some((lo_s, hi_s)) = rest.split_once(" and ")
                && let (Ok(lo), Ok(hi)) = (lo_s.trim().parse::<i64>(), hi_s.trim().parse::<i64>())
                && lo <= hi
            {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Number {
                        min: Box::new(ValueExpr::literal(lo)),
                        max: Box::new(ValueExpr::literal(hi)),
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(
                cleaned,
                "odd or even"
                    | "even or odd"
                    | "heads or tails"
                    | "khans or dragons"
                    | "friends or foe"
                    | "friend or foe"
                    | "left or right"
                    | "yes or no"
                    | "time or space"
                    | "attack or block"
            ) && let Some((a, b)) = cleaned.split_once(" or ")
            {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Options {
                        labels: vec![a.trim().to_owned(), b.trim().to_owned()],
                        min: 1,
                        max: 1,
                        allow_repeat: false,
                    },
                    ChoiceCategory::Generic,
                    false,
                    builder,
                    in_trigger,
                );
            }
            if matches!(
                cleaned,
                "one of those cards" | "a card from among them" | "one of them"
            ) && !builder.instructions.is_empty()
            {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Objects {
                        query: ObjectQuery::LastMoved,
                        min: Box::new(ValueExpr::literal(1)),
                        max: Box::new(ValueExpr::literal(1)),
                        constraint: None,
                        hidden: false,
                    },
                    ChoiceCategory::Generic,
                    true,
                    builder,
                    in_trigger,
                );
            }
            let (min_c, max_c, rest_filter) = parse_prompt_count_prefix(cleaned);
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            if !rest_filter.is_empty()
                && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                    rest_filter,
                    &compiler_vocab,
                )
                && let Some(query) =
                    lower_filter_to_zone_query(&ast_f, builder, in_trigger, vocabulary)
            {
                return emit_prompt_choice_for_player(
                    *chooser,
                    PromptDomain::Objects {
                        query,
                        min: Box::new(ValueExpr::literal(min_c)),
                        max: Box::new(ValueExpr::literal(max_c)),
                        constraint: None,
                        hidden: false,
                    },
                    ChoiceCategory::Generic,
                    true,
                    builder,
                    in_trigger,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

#[cfg(test)]
#[path = "wave12_prevention_followup_tests.rs"]
mod wave12_prevention_followup_tests;
