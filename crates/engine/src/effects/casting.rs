//! Spell casting validation, cost payment, and stack placement.
use super::*;

impl EffectWindow {
    #[allow(clippy::too_many_arguments)] // One complete cast announcement plus an optional resolving parent.
    #[inline(never)]
    pub(super) fn cast_in_context(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
        turn: Option<&TurnScheduler>,
        resolving_cast: Option<ResolvingCast<'_>>,
    ) -> Result<CastTransition, EffectError> {
        let resolving_parent = resolving_cast.map(|cast| cast.parent);
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let source = self
            .window
            .cards()
            .get(spell)
            .ok_or(EffectError::UnknownObject)?;
        let printed = source.card_id().and_then(|card| self.programs.get(&card));
        let siege = resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Siege);
        let split = printed.and_then(|card| card.split.as_deref());
        let selected = match (split, payment.split_half, payment.adventure) {
            (Some(split), half, false) => {
                Some((split, half.unwrap_or(crate::program::SplitHalf::Left)))
            }
            (None, None, _) => None,
            _ => return Err(EffectError::InvalidCastResource),
        };
        let aftermath = selected.is_some_and(|(split, half)| {
            split.aftermath && half == crate::program::SplitHalf::Right
        });
        if aftermath && !matches!(source.zone(), ObjectZone::Graveyard(_)) {
            return Err(EffectError::InvalidCastResource);
        }
        let own_aftermath = aftermath && source.zone() == ObjectZone::Graveyard(actor);
        let disturb = if payment.disturb {
            if source.zone() != ObjectZone::Graveyard(actor)
                || payment.adventure
                || payment.modal_back
                || payment.prototype
                || payment.prepare
                || payment.split_half.is_some()
                || payment.use_alternative
                || payment.alternative_cost_source.is_some()
                || resolving_cast.is_some()
            {
                return Err(EffectError::InvalidCastResource);
            }
            Some(
                printed
                    .and_then(|front| front.disturb.as_deref())
                    .ok_or(EffectError::InvalidCastResource)?,
            )
        } else {
            None
        };
        if (payment.modal_back || payment.prototype || payment.prepare)
            && (siege
                || payment.adventure
                || payment.split_half.is_some()
                || (usize::from(payment.modal_back)
                    + usize::from(payment.prototype)
                    + usize::from(payment.prepare)
                    > 1))
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payment.prepare
            && (source.zone() != ObjectZone::Battlefield
                || source.controller() != Some(actor)
                || !self.characteristics.prepared.contains(&spell)
                || resolving_cast.is_some()
                || payment.use_alternative
                || payment.alternative_cost_source.is_some())
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payment.prototype
            && (payment.use_alternative || payment.alternative_cost_source.is_some())
        {
            return Err(EffectError::InvalidCastResource);
        }
        let definition = if let Some(disturb) = disturb {
            Some(&disturb.back)
        } else if siege {
            Some(
                printed
                    .and_then(|front| front.siege_back.as_deref())
                    .ok_or(EffectError::InvalidCastResource)?,
            )
        } else if payment.modal_back {
            let back = printed
                .and_then(|front| front.modal_back.as_deref())
                .ok_or(EffectError::InvalidCastResource)?;
            if back.has_type(CardType::Land) {
                return Err(EffectError::NotCastableSpell);
            }
            Some(back)
        } else if payment.prototype {
            Some(
                printed
                    .and_then(|front| front.prototype.as_deref())
                    .ok_or(EffectError::InvalidCastResource)?,
            )
        } else if payment.prepare {
            Some(
                printed
                    .and_then(|front| front.prepare_spell.as_deref())
                    .ok_or(EffectError::InvalidCastResource)?,
            )
        } else if let Some((split, half)) = selected {
            Some(split.half(half))
        } else if payment.adventure {
            Some(
                printed
                    .and_then(|front| front.adventure.as_deref())
                    .ok_or(EffectError::InvalidCastResource)?,
            )
        } else {
            printed
        };
        let proposed_cards = if siege || disturb.is_some() || payment.modal_back {
            let mut cards = self.window.cards().clone();
            cards
                .select_reverse_spell(spell)
                .map_err(StackWindowError::from)?;
            Some(cards)
        } else if payment.prototype {
            let mut cards = self.window.cards().clone();
            cards
                .select_prototype(spell)
                .map_err(StackWindowError::from)?;
            Some(cards)
        } else if payment.prepare {
            let spell_def = definition.ok_or(EffectError::InvalidCastResource)?;
            let mut cards = self.window.cards().clone();
            cards
                .preview_spell_copy(spell, spell_def.clone())
                .map_err(StackWindowError::from)?;
            Some(cards)
        } else if let Some((_, half)) = selected {
            let mut cards = self.window.cards().clone();
            cards
                .select_split_half(spell, half)
                .map_err(StackWindowError::from)?;
            Some(cards)
        } else if payment.adventure {
            let mut cards = self.window.cards().clone();
            cards
                .select_adventure(spell)
                .map_err(StackWindowError::from)?;
            Some(cards)
        } else {
            None
        };
        use crate::mana::CastResource;
        let foretold_turn = self.foretold_on(spell).filter(|_| source.owner() == actor);
        let from_foretell = foretold_turn.is_some();
        if let Some(placed) = foretold_turn
            && (placed >= turn.ok_or(TimingError::TurnRequired)?.position().number
                || payment.use_alternative)
        {
            return Err(EffectError::InvalidCastResource);
        }
        let plotted_turn = self
            .plotted_on(spell)
            .filter(|_| source.zone() == ObjectZone::Exile && source.owner() == actor);
        let warped = self
            .warped
            .get(&spell)
            .filter(|_| source.zone() == ObjectZone::Exile && source.owner() == actor);
        if let Some(warped) = warped
            && *warped >= turn.ok_or(TimingError::TurnRequired)?.position().number
        {
            return Err(EffectError::InvalidCastResource);
        }
        let from_plot = plotted_turn.is_some();
        if let Some(plotted_turn) = plotted_turn {
            let schedule = turn.ok_or(TimingError::TurnRequired)?;
            validate_main_phase(&self.window, Some(schedule), actor)?;
            if plotted_turn >= schedule.position().number || payment.use_alternative {
                return Err(EffectError::InvalidCastResource);
            }
        }
        let commander = self.commander_in_command(source, actor);
        let cast_from_command = resolving_cast
            .is_some_and(|cast| cast.kind == ExileCastAbility::Commander)
            && commander.is_some();
        let cast_graveyard_target = resolving_cast
            .as_ref()
            .is_some_and(|cast| cast.kind == ExileCastAbility::TargetedGraveyard)
            && source.zone() == ObjectZone::Graveyard(actor);
        let cast_revealed = matches!(
            resolving_cast.as_ref().map(|value| value.kind),
            Some(ExileCastAbility::RevealedLibrary)
        ) && source.zone() == ObjectZone::Library(actor)
            && self.window.cards().library(actor).first() == Some(&spell);
        let from_hand = source.zone() == ObjectZone::Hand(actor);
        let cast_for_miracle =
            from_hand && resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Miracle);
        let cast_from_hand = cast_for_miracle
            || (from_hand
                && resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::MatchingHand));
        if resolving_parent.is_some()
            && (!cast_from_command
                && !cast_graveyard_target
                && !cast_revealed
                && !cast_from_hand
                && source.zone() != ObjectZone::Exile
                || (source.owner() != actor
                    && !resolving_cast.is_some_and(|cast| {
                        matches!(
                            cast.kind,
                            ExileCastAbility::FoundSpell | ExileCastAbility::Siege
                        )
                    }))
                || (payment.use_alternative
                    && !resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Impulse)))
        {
            return Err(EffectError::InvalidCastResource);
        }
        let from_graveyard = source.zone() == ObjectZone::Graveyard(actor);
        let mayhem = from_graveyard
            && payment.use_alternative
            && definition.is_some_and(|p| {
                matches!(
                    p.conditional_cost,
                    Some(crate::program::ConditionalCastCost::Mayhem(_))
                )
            })
            && self
                .cast_history
                .as_ref()
                .and_then(|h| h.discarded.get(&spell))
                == Some(&actor);
        let via_escape = from_graveyard
            && payment.use_alternative
            && definition.is_some_and(|p| {
                matches!(
                    p.conditional_cost,
                    Some(crate::program::ConditionalCastCost::Escape { .. })
                )
            });
        let escape_cards = payment
            .objects
            .iter()
            .filter(|r| matches!(r, CastResource::Escape(_)))
            .count();
        if escape_cards > 0 && !via_escape {
            return Err(EffectError::InvalidCastResource);
        }
        let mut discards = payment
            .objects
            .iter()
            .filter(|r| matches!(r, CastResource::Retrace(_) | CastResource::JumpStart(_)));
        let graveyard_discard = discards.next().copied();
        if discards.next().is_some()
            || (graveyard_discard.is_some() && (!from_graveyard || via_escape || mayhem))
        {
            return Err(EffectError::InvalidCastResource);
        }
        let may_pay_flashback = from_graveyard
            && !cast_graveyard_target
            && !mayhem
            && !via_escape
            && disturb.is_none()
            && graveyard_discard.is_none();
        let printed_flashback =
            may_pay_flashback && definition.is_some_and(|p| p.flashback.is_some());
        let granted_flashback = may_pay_flashback
            && self.has_granted_flashback(spell, actor)
            && definition
                .is_some_and(|p| p.has_type(CardType::Instant) || p.has_type(CardType::Sorcery));
        if payment.use_granted_flashback
            && (!granted_flashback
                || payment.use_alternative
                || payment.graveyard_permission.is_some()
                || payment.alternative_cost_source.is_some()
                || resolving_parent.is_some())
        {
            return Err(EffectError::InvalidCastResource);
        }
        let via_granted_flashback = granted_flashback
            && (payment.use_granted_flashback
                || !printed_flashback && payment.graveyard_permission.is_none());
        let via_printed_flashback = printed_flashback && !via_granted_flashback;
        let via_flashback = via_granted_flashback || via_printed_flashback;
        if payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::RevealColorlessCreature(_)))
            .count()
            != usize::from(definition.is_some_and(|p| p.reveal_colorless_creature_cast_cost))
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::SacrificeForSpell(_)))
            .count()
            != usize::from(definition.is_some_and(|p| p.sacrifice_creature_cast_cost))
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payment
            .objects
            .iter()
            .filter(|r| matches!(r, CastResource::DiscardForSpell(_)))
            .count()
            != usize::from(definition.is_some_and(|p| p.discard_cast_cost))
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::ExileGraveyardCreatureForSpell(_)))
            .count()
            != definition.map_or(0, |program| {
                usize::from(program.graveyard_creature_exile_cost)
            })
        {
            return Err(EffectError::InvalidCastResource);
        }
        let flashback_taps = payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::TapForFlashback(_)))
            .count();
        let needed_taps = if via_printed_flashback {
            definition.map_or(0, |p| usize::from(p.flashback_tap_white_creatures))
        } else {
            0
        };
        if flashback_taps != needed_taps {
            return Err(EffectError::InvalidCastResource);
        }
        let flashback_sacrifices = payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::FlashbackSacrifice(_)))
            .count();
        let required_flashback_sacrifices = if via_printed_flashback {
            definition.map_or(0, |program| program.flashback_sacrifice_creatures as usize)
        } else {
            0
        };
        if flashback_sacrifices != required_flashback_sacrifices {
            return Err(EffectError::InvalidCastResource);
        }
        let face_down_price = ManaCost::generic(3.into());
        let triggered_base_price = ManaCost::free();
        let program = source
            .face_down()
            .map(|kind| kind.definition())
            .or(definition)
            .ok_or(EffectError::NotCastableSpell)?;
        let cast_mutating = payment.use_alternative
            && matches!(
                program.conditional_cost,
                Some(crate::program::ConditionalCastCost::Mutate(_))
            );
        let cast_bestowed = payment.use_alternative
            && matches!(
                program.conditional_cost,
                Some(crate::program::ConditionalCastCost::Bestow(_))
            );
        let current = if from_foretell
            || cast_bestowed
            || payment.adventure
            || payment.modal_back
            || payment.prototype
            || payment.prepare
            || siege
            || disturb.is_some()
            || selected.is_some()
        {
            let mut face_up = self.characteristics.clone();
            face_up.foretold.remove(&spell);
            if cast_bestowed {
                face_up.bestowed_spells.insert(spell);
            }
            evaluate(CharacteristicWorld {
                active_player: self.window.active_player(),
                participants: self.window.participants(),
                life: Some(&self.life),
                entry_creature_types: &self.entry_creature_type_choices,
                mana: &self.mana,
                combat: self.combat.as_ref(),
                cards: proposed_cards
                    .as_ref()
                    .unwrap_or_else(|| self.window.cards()),
                programs: &self.programs,
                state: &face_up,
                vocabulary: &self.creature_vocabulary,
                name_rules: &self.name_rules,
                step: turn.map(|t| t.position().step).or(self.current_step),
            })?
        } else {
            self.evaluated_characteristics()?
        };
        let self_graveyard_permission = from_graveyard
            && (current
                .get(spell)
                .and_then(|card| card.printed_text.graveyard_self_cast.as_ref())
                .is_some_and(|permission| match permission {
                    crate::program::GraveyardSelfCast::Unconditional
                    | crate::program::GraveyardSelfCast::WithExtraCost(_)
                    | crate::program::GraveyardSelfCast::WithAlternativeCost { .. }
                    | crate::program::GraveyardSelfCast::GraveyardOrExile => true,
                    crate::program::GraveyardSelfCast::WhileControllingSubtype(kind) => {
                        current.objects.values().any(|permanent| {
                            permanent.zone == ObjectZone::Battlefield
                                && permanent.controller == Some(actor)
                                && permanent.current_creature_subtypes.contains(kind)
                        })
                    }
                })
                || (payment.use_alternative
                    && matches!(
                        current
                            .get(spell)
                            .and_then(|c| c.printed_text.conditional_cost.as_ref()),
                        Some(crate::program::ConditionalCastCost::Harmonize(_))
                    ))
                || current.get(spell).is_some_and(|card| {
                    card.printed_text
                        .continuous
                        .iter()
                        .chain(&card.printed_text.additional_battlefield_abilities)
                        .any(|ab| {
                            let crate::characteristics::StaticContinuousAbility::CastPermission {
                                permission:
                                    crate::characteristics::CastPermissionRule::GraveyardSelfConditional {
                                        condition,
                                    },
                                ..
                            } = ab
                            else {
                                return false;
                            };
                            let ctx = crate::characteristics::WorldEvalCtx {
                                active_player: self.window.active_player(),
                                step: turn.map(|t| t.position().step).or(self.current_step),
                                participants: self.window.participants(),
                                life: Some(&self.life),
                                combat: self.combat.as_ref(),
                                cards: self.window.cards(),
                                programs: &self.programs,
                                state: &self.characteristics,
                                objects: &current.objects,
                                source: spell,
                                controller: actor,
                                name_rules: &self.name_rules,
                                trigger_subject: None,
                                related_player: None,
                                defending_player: None,
                            };
                            crate::characteristics::eval_world_condition(&ctx, condition)
                        })
                }));
        let self_exile_permission = source.zone() == ObjectZone::Exile
            && source.owner() == actor
            && source.face_down().is_none()
            && matches!(
                current
                    .get(spell)
                    .and_then(|card| card.printed_text.graveyard_self_cast.as_ref()),
                Some(crate::program::GraveyardSelfCast::GraveyardOrExile)
            );
        let static_exile_grant = if turn.is_some() {
            self.static_exile_cast_grant(spell, actor, &current)
        } else {
            library_access::StaticExileCastGrant::default()
        };
        let graveyard_permission_candidates = if from_graveyard
            && !cast_graveyard_target
            && !via_escape
            && !mayhem
            && !via_flashback
            && graveyard_discard.is_none()
            && turn.is_some()
        {
            let spell_view = current.get(spell);
            let is_creature = spell_view.is_some_and(|card| card.is_creature());
            let mut cands = Vec::new();
            for permanent in current.objects.values() {
                if permanent.zone != ObjectZone::Battlefield || permanent.controller != Some(actor)
                {
                    continue;
                }
                for ab in permanent
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&permanent.printed_text.additional_battlefield_abilities)
                {
                    let active = permanent.abilities.continuous == Some(ab.id())
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ab.id());
                    if !active {
                        continue;
                    }
                    match ab {
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardCreatureCast,
                        } if is_creature
                            && self.window.active_player() == actor
                            && !self.source_cast_uses.contains(&(permanent.object, *id)) =>
                        {
                            cands.push((permanent.object, *id, true));
                        }
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardMatching {
                                    filter,
                                    once_each_turn,
                                    ..
                                },
                        } if (!*once_each_turn
                            || !self.source_cast_uses.contains(&(permanent.object, *id)))
                            && spell_view.is_some_and(|sv| {
                                self.matches_object_filter(
                                    **filter,
                                    sv,
                                    actor,
                                    Some(permanent.object),
                                    &BTreeSet::new(),
                                    &current,
                                )
                            }) =>
                        {
                            cands.push((permanent.object, *id, *once_each_turn));
                        }
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardMatchingOwnTurn {
                                    filter,
                                    once_each_turn,
                                    ..
                                },
                        } if self.window.active_player() == actor
                            && (!*once_each_turn
                                || !self.source_cast_uses.contains(&(permanent.object, *id)))
                            && spell_view.is_some_and(|sv| {
                                self.matches_object_filter(
                                    **filter,
                                    sv,
                                    actor,
                                    Some(permanent.object),
                                    &BTreeSet::new(),
                                    &current,
                                )
                            }) =>
                        {
                            cands.push((permanent.object, *id, *once_each_turn));
                        }
                        _ => {}
                    }
                }
            }
            cands
        } else {
            vec![]
        };
        let graveyard_permission = match (
            payment.graveyard_permission,
            graveyard_permission_candidates.as_slice(),
        ) {
            (Some(selected), candidates) => Some(
                *candidates
                    .iter()
                    .find(|(source, _, _)| *source == selected)
                    .ok_or(EffectError::InvalidCastResource)?,
            ),
            (None, []) => None,
            (None, [only]) => Some(*only),
            (None, _) => return Err(EffectError::InvalidCastResource),
        };
        let available_types = self.free_exile_types_for(spell, actor, Some(program));
        let type_payment = match (payment.exile_cast_type, available_types.as_slice()) {
            (None, []) => None,
            (None, [only]) => Some(*only),
            (Some(chosen), available) if available.contains(&chosen) => Some(chosen),
            _ => return Err(EffectError::InvalidCastResource),
        };
        let (temp_zone_allowed, temp_zone_free, temp_zone_any_color, temp_zone_consumed_idx) =
            if turn.is_some() {
                self.temporary_zone_cast_grant(spell, actor, &current, Some(program), turn)
            } else {
                (false, false, false, None)
            };
        let from_free_exile = turn.is_some()
            && (self.may_cast_free_face(spell, actor, Some(program))
                || temp_zone_free
                || static_exile_grant.free);
        if from_free_exile && (payment.use_alternative || payment.alternative_cost_source.is_some())
        {
            return Err(EffectError::InvalidCastResource);
        }
        let from_exile_permission = turn.is_some()
            && (self.play_exile_permission(spell, actor, !payment.adventure)
                || self.may_cast_from_granted_exile(spell, actor)
                || static_exile_grant.allowed);
        let used_other_exile_permission = from_plot
            || from_foretell
            || warped.is_some()
            || self_exile_permission
            || resolving_parent.is_some()
            || (turn.is_some()
                && (self.play_exile_permission(spell, actor, !payment.adventure)
                    || self.may_cast_from_granted_exile(spell, actor)
                    || self.may_cast_free_face(spell, actor, Some(program))
                    || temp_zone_allowed));
        let static_exile_once_permission = if source.zone() == ObjectZone::Exile
            && (!used_other_exile_permission
                || static_exile_grant.free
                || static_exile_grant.any_color)
        {
            static_exile_grant.consumed_once
        } else {
            None
        };
        let library_top_once_permission = if turn.is_some()
            && resolving_parent.is_none()
            && !temp_zone_allowed
            && source.zone() == ObjectZone::Library(actor)
            && self.window.cards().library(actor).first() == Some(&spell)
        {
            self.library_top_once_permission(&current, spell, actor)
        } else {
            None
        };
        if source.zone() != ObjectZone::Hand(actor)
            && !payment.prepare
            && resolving_parent.is_none()
            && !from_plot
            && !from_free_exile
            && !from_exile_permission
            && !self_exile_permission
            && !temp_zone_allowed
            && !(turn.is_some()
                && self.window.cards().library(actor).first() == Some(&spell)
                && self.library_top_access(&current, actor, actor).playable)
            && !from_foretell
            && warped.is_none()
            && commander.is_none()
            && !via_escape
            && !mayhem
            && !via_flashback
            && disturb.is_none()
            && !own_aftermath
            && graveyard_permission.is_none()
            && !self_graveyard_permission
            && !(turn.is_some() && self.may_cast_from_graveyard_by_permission(spell, actor))
            && graveyard_discard.is_none()
        {
            return Err(EffectError::NotOwnHand);
        }
        if matches!(
            source.zone(),
            ObjectZone::Graveyard(_) | ObjectZone::Library(_)
        ) && Self::graveyard_and_library_restricted(&current)
        {
            return Err(EffectError::CastingFromZoneProhibited);
        }
        self.validate_legendary_spell_cast(spell, actor, &current)?;
        self.check_split_second(&current)?;
        self.check_opponents_turn_restriction(actor, &current, None)?;
        if self.spells_cast_by_player.contains_key(&actor)
            && (current.objects.values().any(|object| {
                object.zone == ObjectZone::Battlefield
                    && (Self::permanent_has_restriction(object, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::OneSpellPerTurn
                        )
                    }) || (object.controller == Some(actor)
                        && Self::permanent_has_restriction(object, |rule| {
                            matches!(
                                rule,
                                crate::characteristics::StaticRestriction::ControllerOneSpellPerTurn
                            )
                        }))
                        || (object.attached_to == Some(TargetRef::Player(actor))
                            && Self::permanent_has_restriction(object, |rule| {
                                matches!(
                                    rule,
                                    crate::characteristics::StaticRestriction::EnchantedPlayerCastLimit
                                )
                            })))
            }) || self.temporary_rules.iter().any(|active| {
                self.active_temporary_static(active, &current)
                    .is_some_and(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::OneSpellPerTurn,
                                ..
                            }
                        ) || (active.controller == actor
                            && matches!(
                                ab,
                                crate::characteristics::StaticContinuousAbility::Restriction {
                                    rule: crate::characteristics::StaticRestriction::ControllerOneSpellPerTurn,
                                    ..
                                }
                            ))
                    })
            }))
        {
            return Err(EffectError::SpellCastLimitReached);
        }
        if turn.is_some_and(|t| t.position().step.phase() == crate::turn::Phase::Combat)
            && (current.objects.values().any(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.controller.is_some_and(|ctrl| ctrl != actor)
                    && Self::permanent_has_restriction(object, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::OpponentsCantCastDuringCombat
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                active.controller != actor
                    && self
                        .active_temporary_static(active, &current)
                        .is_some_and(|ab| {
                            matches!(
                                ab,
                                crate::characteristics::StaticContinuousAbility::Restriction {
                                    rule: crate::characteristics::StaticRestriction::OpponentsCantCastDuringCombat,
                                    ..
                                }
                            )
                        })
            }))
        {
            return Err(EffectError::OpponentActionProhibited);
        }
        let step_now = turn.map(|t| t.position().step).or(self.current_step);
        let actor_has_sorcery_timing = self.window.active_player() == actor
            && self.window.entries().is_empty()
            && matches!(
                step_now,
                Some(crate::turn::Step::PrecombatMain | crate::turn::Step::PostcombatMain)
            );
        if !actor_has_sorcery_timing
            && (current.objects.values().any(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.controller.is_some_and(|ctrl| ctrl != actor)
                    && Self::permanent_has_restriction(object, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::OpponentsSorceryOnly
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                active.controller != actor
                    && self
                        .active_temporary_static(active, &current)
                        .is_some_and(|ab| {
                            matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule:
                                    crate::characteristics::StaticRestriction::OpponentsSorceryOnly,
                                ..
                            }
                        )
                        })
            }))
        {
            return Err(EffectError::OpponentActionProhibited);
        }
        let current_spell = current.get(spell).ok_or(EffectError::InconsistentProgram)?;
        let aura_or_equipment = ((cast_bestowed
            || matches!(
                program.permanent().and_then(|p| p.attachment),
                Some(crate::attachment::AttachmentKind::Aura(_))
            ))
            && current_spell.has_type(CardType::Enchantment))
            || (current_spell.has_type(CardType::Artifact)
                && program.permanent().and_then(|p| p.attachment)
                    == Some(crate::attachment::AttachmentKind::Equipment));
        self.check_chosen_name_cast_restriction(current_spell, &current)?;
        if self.temporary_rules.iter().any(|active| {
            if let crate::program::TemporaryRuleKind::CantCastSpells { filter, .. } = &active.rule {
                if !active.resolved_players.contains(&actor) {
                    return false;
                }
                return match filter {
                    None => true,
                    Some(f) => self.matches_object_filter(
                        *f,
                        current_spell,
                        active.controller,
                        active.source_object,
                        &BTreeSet::new(),
                        &current,
                    ),
                };
            }
            let Some(crate::characteristics::StaticContinuousAbility::Restriction {
                rule: crate::characteristics::StaticRestriction::ProhibitCasting { player, filter },
                ..
            }) = self.active_temporary_static(active, &current)
            else {
                return false;
            };
            let player_matches = match player {
                crate::program::PlayerRelation::Controller => actor == active.controller,
                crate::program::PlayerRelation::Opponent => actor != active.controller,
                crate::program::PlayerRelation::AnyParticipant => true,
            };
            let entry_type = active
                .source_object
                .and_then(|src| self.entry_creature_type_choices.get(&src).copied());
            player_matches
                && self.prohibit_casting_matches(
                    **filter,
                    current_spell,
                    source,
                    actor,
                    aura_or_equipment,
                    active.source_object,
                    entry_type,
                )
        }) {
            return Err(EffectError::SpellCastLimitReached);
        }
        if current.objects.values().any(|perm| {
            if perm.zone != ObjectZone::Battlefield {
                return false;
            }
            let Some(ctrl) = perm.controller else {
                return false;
            };
            Self::permanent_has_restriction(perm, |rule| {
                let crate::characteristics::StaticRestriction::ProhibitCasting { player, filter } =
                    rule
                else {
                    return false;
                };
                let player_matches = match player {
                    crate::program::PlayerRelation::Controller => actor == ctrl,
                    crate::program::PlayerRelation::Opponent => actor != ctrl,
                    crate::program::PlayerRelation::AnyParticipant => true,
                };
                player_matches
                    && self.prohibit_casting_matches(
                        **filter,
                        current_spell,
                        source,
                        actor,
                        aura_or_equipment,
                        Some(perm.object),
                        perm.entry_creature_type,
                    )
            })
        }) {
            return Err(EffectError::SpellCastLimitReached);
        }
        let mut alternative_life = false;
        let external_alternative: Option<(
            ObjectId,
            crate::characteristics::StaticAbilityId,
            bool,
            ManaCost,
        )> = if let Some(source_id) = payment.alternative_cost_source {
            if turn.is_none()
                || source.face_down().is_some()
                || payment.use_alternative
                || via_flashback
                || via_escape
                || mayhem
                || from_foretell
                || from_plot
                || resolving_cast.is_some_and(|cast| cast.kind != ExileCastAbility::Impulse)
            {
                return Err(EffectError::InvalidCastResource);
            }
            let permanent = current
                .get(source_id)
                .filter(|p| p.zone == ObjectZone::Battlefield)
                .ok_or(EffectError::InvalidCastResource)?;
            let controller_matches = permanent.controller == Some(actor);
            let mut matched: Option<(
                crate::characteristics::StaticAbilityId,
                bool,
                bool,
                ManaCost,
            )> = None;
            for ability in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active = permanent.abilities.continuous == Some(ability.id())
                    || permanent
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id());
                if !active {
                    continue;
                }
                let spec = match ability {
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::FreeColorlessHandSpellOnce,
                        ..
                    } if controller_matches
                        && source.zone() == ObjectZone::Hand(actor)
                        && current_spell.current_colors.is_empty() =>
                    {
                        Some((true, false, ManaCost::free()))
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce,
                        ..
                    } if controller_matches
                        && actor == self.window.active_player()
                        && current_spell.has_type(CardType::Enchantment) =>
                    {
                        Some((true, true, ManaCost::free()))
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::FreeSubtypeCreatureSpells(
                                subtype,
                            ),
                        ..
                    } if controller_matches
                        && current_spell.is_creature()
                        && current_spell.current_creature_subtypes.contains(subtype) =>
                    {
                        Some((false, false, ManaCost::free()))
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::FirstSpellRule {
                                filter,
                                own_turn_only,
                                each_player,
                                free,
                                pay_life_mv,
                                ..
                            },
                        ..
                    } if (*each_player || controller_matches)
                        && (*free || *pay_life_mv)
                        && (!*own_turn_only || actor == self.window.active_player())
                        && self.spell_filter_matches_cast(
                            *filter,
                            current_spell,
                            source,
                            actor,
                            aura_or_equipment,
                            permanent.entry_creature_type,
                        )
                        && (filter.ordinal_this_turn.is_some()
                            || self.prior_matching_spells_cast_this_turn(actor, *filter) == 0) =>
                    {
                        Some((!*each_player, *pay_life_mv, ManaCost::free()))
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::AlternativeForMatching {
                                filter,
                                mana,
                                once_each_turn,
                                own_turn_only,
                            },
                        ..
                    } if controller_matches
                        && (!*own_turn_only || actor == self.window.active_player())
                        && self.spell_filter_matches_cast(
                            *filter,
                            current_spell,
                            source,
                            actor,
                            aura_or_equipment,
                            permanent.entry_creature_type,
                        ) =>
                    {
                        Some((*once_each_turn, false, mana.clone()))
                    }
                    _ => None,
                };
                if let Some((once, pay_life, alt_mana)) = spec
                    && (!once || !self.source_cast_uses.contains(&(source_id, ability.id())))
                {
                    matched = Some((ability.id(), once, pay_life, alt_mana));
                    break;
                }
            }
            let (ability_id, once, pay_life, alt_mana) =
                matched.ok_or(EffectError::InvalidCastResource)?;
            alternative_life = pay_life;
            Some((source_id, ability_id, once, alt_mana))
        } else {
            None
        };
        let mut returns = payment.objects.iter().filter_map(|r| {
            if let CastResource::Sneak(id) = r {
                Some(*id)
            } else {
                None
            }
        });
        let sneak_defender = if payment.use_alternative
            && matches!(
                current_spell.printed_text.conditional_cost,
                Some(crate::program::ConditionalCastCost::Sneak(_))
            ) {
            if self.window.active_player() != actor
                || turn.is_none_or(|t| t.position().step != crate::turn::Step::DeclareBlockers)
            {
                return Err(EffectError::InvalidCastResource);
            }
            Some(self.unblocked_defender(
                returns.next().ok_or(EffectError::InvalidCastResource)?,
                actor,
                &current,
            )?)
        } else {
            None
        };
        if returns.next().is_some() {
            return Err(EffectError::InvalidCastResource);
        }
        let mut consumed_temp_rules = Vec::new();
        let mut temporary_flash = false;
        // Every matching "next spell" effect is used by the cast, even when another
        // permission or ordinary timing already allows it. Commit consumption only
        // on the successful candidate below, after all cast checks have passed.
        for (rule_idx, active) in self.temporary_rules.iter().enumerate() {
            if self
                .next_spell_keyword_for_cast(active, &current, current_spell, actor)
                .is_some()
            {
                consumed_temp_rules.push(rule_idx);
            }
            if !active.consumed
                && active.resolved_players.contains(&actor)
                && let crate::program::TemporaryRuleKind::FilteredFlash {
                    filter, next_only, ..
                } = &active.rule
                && self.matches_object_filter(
                    *filter,
                    current_spell,
                    active.controller,
                    None,
                    &BTreeSet::new(),
                    &current,
                )
            {
                temporary_flash = true;
                if *next_only {
                    consumed_temp_rules.push(rule_idx);
                }
            }
        }
        let mut flash_extra_mana: Option<ManaCost> = None;
        let mut flash = sneak_defender.is_some()
            || current_spell.has_keyword(KeywordAbility::Flash)
            || self.spell_has_granted_keyword_in(
                &current,
                current_spell,
                actor,
                KeywordAbility::Flash,
                Some(aura_or_equipment),
            )
            || self.flash_through_cleanup.contains(&actor)
            || temporary_flash
            || current.objects.values().any(|source| {
                source.zone == ObjectZone::Battlefield
                && source
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&source.printed_text.additional_battlefield_abilities)
                    .any(|ab| {
                        let active = source.abilities.continuous == Some(ab.id())
                            || source
                                .abilities
                                .additional_battlefield_abilities
                                .contains(&ab.id());
                        active
                            && matches!(
                                ab,
                                crate::characteristics::StaticContinuousAbility::CastPermission {
                                    permission: crate::characteristics::CastPermissionRule::Flash {
                                        any_player,
                                        matching,
                                    },
                                    ..
                                } if (*any_player || source.controller == Some(actor))
                                    && matching.matches_characteristics(
                                        current_spell,
                                        aura_or_equipment,
                                        source.entry_creature_type,
                                    )
                            )
                    })
            })
            || self.temporary_rules.iter().any(|active| {
                let Some(ab) = self.active_temporary_static(active, &current) else {
                    return false;
                };
                matches!(
                    ab,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission: crate::characteristics::CastPermissionRule::Flash {
                            any_player,
                            matching,
                        },
                        ..
                    } if (*any_player || active.controller == actor)
                        && matching.matches_characteristics(
                            current_spell,
                            aura_or_equipment,
                            None,
                        )
                )
            })
            || current_spell
                .printed_text
                .continuous
                .iter()
                .chain(&current_spell.printed_text.additional_battlefield_abilities)
                .any(|ab| {
                    matches!(
                        ab,
                        crate::characteristics::StaticContinuousAbility::Restriction {
                            rule:
                                crate::characteristics::StaticRestriction::FlashSacrificeAtCleanup,
                            ..
                        }
                    )
                });
        if !flash && let Some(self_flash) = &program.conditional_flash {
            match self_flash {
                crate::program::SelfFlashPermission::Condition(cond) => {
                    let lhs = self.resolve_value_expr_at(actor, &cond.left, None)?;
                    let rhs = self.resolve_value_expr_at(actor, &cond.right, None)?;
                    if cond.comparison.evaluate(&lhs, &rhs) {
                        flash = true;
                    }
                }
                crate::program::SelfFlashPermission::PayExtraMana(extra) => {
                    if resolving_parent.is_none()
                        && !current_spell.has_type(CardType::Instant)
                        && validate_main_phase(&self.window, turn, actor).is_err()
                    {
                        flash = true;
                        flash_extra_mana = Some(extra.clone());
                    }
                }
            }
        }
        let spliced = self.prepare_splice(&payment.splice, actor, current_spell, &current)?;
        let mut mode_targets = Vec::new();
        let mut mode_clauses = Vec::new();
        let via_gy_alt_cost = if from_graveyard
            && !cast_graveyard_target
            && !via_flashback
            && !via_escape
            && !mayhem
            && disturb.is_none()
            && !own_aftermath
            && graveyard_discard.is_none()
            && graveyard_permission.is_none()
            && !temp_zone_allowed
            && resolving_parent.is_none()
            && external_alternative.is_none()
            && !from_free_exile
            && !payment.use_alternative
        {
            match current_spell.printed_text.graveyard_self_cast.as_ref() {
                Some(crate::program::GraveyardSelfCast::WithAlternativeCost { mana, .. }) => {
                    Some(mana)
                }
                _ => None,
            }
        } else {
            None
        };
        let (cost, mut payload) = match &program.kind {
            ProgramKind::Spell(spell) => {
                if resolving_parent.is_none()
                    && !current_spell.has_type(CardType::Instant)
                    && !flash
                {
                    validate_main_phase(&self.window, turn, actor)?;
                }
                (
                    if via_printed_flashback {
                        current_spell
                            .printed_text
                            .flashback
                            .as_ref()
                            .ok_or(EffectError::NotCastableSpell)?
                    } else if spell.uncastable_mana_cost {
                        if from_free_exile
                            || resolving_parent.is_some()
                            || external_alternative.is_some()
                            || via_gy_alt_cost.is_some()
                            || from_foretell
                            || from_plot
                            || cast_for_miracle
                            || (payment.use_alternative
                                && current_spell.printed_text.conditional_cost.is_some())
                        {
                            &triggered_base_price
                        } else {
                            return Err(EffectError::NotCastableSpell);
                        }
                    } else {
                        &spell.cost
                    },
                    if spell.modes.is_some() {
                        if !payment.fixed_targets.is_empty() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        if target.is_some() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        let entwined = current_spell
                            .printed_text
                            .additional_cast_costs
                            .iter()
                            .enumerate()
                            .any(|(index, cost)| {
                                matches!(cost, crate::program::AdditionalCastCost::Entwine(_))
                                    && payment.additional.contains_key(&index)
                            });
                        let (body, modes, targets) = self.select_modes(
                            spell,
                            &payment.modes,
                            entwined,
                            &current,
                            current_spell,
                            actor,
                        )?;
                        mode_clauses = modes
                            .iter()
                            .filter_map(|mode| mode.clause.clone())
                            .collect();
                        mode_targets = targets;
                        let mut payload = StackPayload::from_body(
                            body,
                            Some(CopiableSpellTraits::from_program(program)),
                        );
                        if let StackPayload::Instructions(p) = &mut payload {
                            p.modes = modes;
                        }
                        payload
                    } else {
                        if !payment.modes.is_empty() {
                            return Err(EffectError::InvalidCastResource);
                        }
                        let body = match &current_spell.printed_text.conditional_cost {
                            Some(
                                crate::program::ConditionalCastCost::Overload {
                                    each: unbracketed,
                                    ..
                                }
                                | crate::program::ConditionalCastCost::Cleave { unbracketed, .. },
                            ) if payment.use_alternative => unbracketed.as_ref().clone(),
                            _ => spell.body.clone(),
                        };
                        let has_fixed = body
                            .instructions
                            .iter()
                            .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
                        if has_fixed {
                            if target.is_some() {
                                return Err(EffectError::UnexpectedTarget);
                            }
                            let (body, segments, targets) = self.expand_fixed_clauses(
                                &body,
                                &payment.fixed_targets,
                                actor,
                                &current,
                                current_spell,
                            )?;
                            mode_targets = targets;
                            let mut payload = StackPayload::from_body(
                                body,
                                Some(CopiableSpellTraits::from_program(program)),
                            );
                            if let StackPayload::Instructions(p) = &mut payload {
                                p.modes = segments;
                            }
                            payload
                        } else {
                            if !payment.fixed_targets.is_empty() {
                                return Err(EffectError::UnexpectedTarget);
                            }
                            StackPayload::from_body(
                                body,
                                Some(CopiableSpellTraits::from_program(program)),
                            )
                        }
                    },
                )
            }
            ProgramKind::Permanent(permanent) if !program.has_type(CardType::Land) => {
                if !payment.modes.is_empty() || !payment.fixed_targets.is_empty() {
                    return Err(EffectError::InvalidCastResource);
                }
                let cost = if let Some(disturb) = disturb {
                    &disturb.cost
                } else if source.face_down().is_some() {
                    &face_down_price
                } else if (from_free_exile
                    || resolving_parent.is_some()
                    || external_alternative.is_some()
                    || via_gy_alt_cost.is_some())
                    && permanent.casting_cost.is_none()
                {
                    &triggered_base_price
                } else {
                    permanent
                        .casting_cost
                        .as_ref()
                        .ok_or(EffectError::NotCastableSpell)?
                };
                if resolving_parent.is_none() && !flash {
                    validate_main_phase(&self.window, turn, actor)?;
                }
                (cost, StackPayload::Permanent)
            }
            _ => return Err(EffectError::NotCastableSpell),
        };
        let enchant = if cast_bestowed {
            Some(crate::attachment::EnchantRestriction::Creature)
        } else {
            program
                .permanent()
                .and_then(|permanent| match permanent.attachment {
                    Some(crate::attachment::AttachmentKind::Aura(enchant)) => Some(enchant),
                    _ => None,
                })
        };
        let target_contract = if cast_mutating {
            BodyTarget::SingleCreature
        } else {
            payload
                .body()
                .map(|body| body.target)
                .unwrap_or_else(|| enchant.map_or(BodyTarget::None, |value| value.target()))
        };
        if program.cast_during_own_combat
            && (actor != self.window.active_player()
                || turn
                    .map(|s| s.position().step)
                    .or(self.current_step)
                    .is_none_or(|s| s.phase() != crate::turn::Phase::Combat))
        {
            return Err(EffectError::InvalidCastResource);
        }
        if let Some(timing) = &program.cast_timing
            && !self.cast_timing_satisfied_with_x(
                timing,
                source.id(),
                actor,
                turn,
                payment.chosen_x.as_ref(),
            )?
        {
            return Err(EffectError::InvalidCastResource);
        }
        if payload
            .body()
            .is_some_and(|body| body.instructions == [Instruction::ReturnChosenXGraveyardCreatures])
        {
            let schedule = turn.ok_or(TimingError::TurnRequired)?;
            if !self
                .combat_rules
                .opponents(self.window.participants(), actor)?
                .contains(&self.window.active_player())
                || schedule.position().step.phase() != crate::turn::Phase::Combat
            {
                return Err(EffectError::InvalidCastResource);
            }
            if payment.chosen_x.clone().unwrap_or_else(|| 0.into())
                != RulesInteger::from_usize(payment.permanent_targets.len())
            {
                return Err(EffectError::InvalidCastResource);
            }
        }
        if payload
            .body()
            .is_some_and(ExecutableBody::has_x_target_count)
            && payment.chosen_x.clone().unwrap_or_else(|| 0.into())
                != RulesInteger::from_usize(payment.permanent_targets.len())
        {
            return Err(EffectError::InvalidCastResource);
        }
        let one_per_player = payload.body().is_some_and(|body| {
            body.instructions.first() == Some(&Instruction::ShuffleOnePermanentPerPlayer)
        });
        let one_per_opponent = matches!(target_contract, BodyTarget::FilteredPermanent(filter) if filter.is_opposing_even_mana_value());
        let target_bounds = payload
            .body()
            .and_then(ExecutableBody::permanent_target_bounds)
            .map(|bounds| {
                if one_per_player {
                    let seats = self.window.participants().len();
                    return (seats, seats);
                }
                if payload.body().is_some_and(|body| {
                    body.instructions == [Instruction::ReturnChosenXGraveyardCreatures]
                        || body.has_x_target_count()
                }) {
                    (
                        payment.permanent_targets.len(),
                        payment.permanent_targets.len(),
                    )
                } else {
                    bounds
                }
            });
        if let Some(dependent) = payload.body().and_then(ExecutableBody::dependent_targets) {
            let primary = target.ok_or(EffectError::TargetRequired)?;
            if payment.permanent_targets.len() > dependent.maximum() {
                return Err(EffectError::InvalidCastResource);
            }
            for (index, &chosen) in payment.permanent_targets.iter().enumerate() {
                if Self::protected_target(
                    chosen,
                    &protection::ProtectionSource::from(current_spell).with_controller(actor),
                    &current,
                ) || !self.legal_dependent_target(dependent, primary, chosen, &current)
                {
                    return Err(EffectError::IllegalTarget);
                }
                mode_targets.push(TargetChoice {
                    id: TargetId(
                        u32::try_from(index + 1).map_err(|_| EffectError::InvalidCastResource)?,
                    ),
                    clause: TargetClauseId(1),
                    target: chosen,
                });
            }
            mode_clauses = vec![
                TargetClause {
                    id: TargetClauseId(0),
                    minimum: 1,
                    maximum: 1,
                },
                TargetClause {
                    id: TargetClauseId(1),
                    minimum: 0,
                    maximum: dependent.maximum(),
                },
            ];
        } else if let Some((minimum, maximum)) = target_bounds {
            if target.is_some() || !(minimum..=maximum).contains(&payment.permanent_targets.len()) {
                return Err(EffectError::InvalidCastResource);
            }
            for (ordinal, &chosen) in payment.permanent_targets.iter().enumerate() {
                if Self::protected_target(
                    chosen,
                    &protection::ProtectionSource::from(current_spell).with_controller(actor),
                    &current,
                ) || !self.legal_target_kind_in(target_contract, chosen, actor, &current)
                {
                    return Err(EffectError::IllegalTarget);
                }
                mode_targets.push(TargetChoice {
                    id: TargetId(
                        u32::try_from(ordinal).map_err(|_| EffectError::InvalidCastResource)?,
                    ),
                    clause: TargetClauseId(0),
                    target: chosen,
                });
            }
            if let Some(total) = payload
                .body()
                .and_then(ExecutableBody::target_division_total)
            {
                let shares = &payment.target_division;
                if shares.len() != mode_targets.len()
                    || shares.contains(&0)
                    || shares.iter().map(|&share| u32::from(share)).sum::<u32>() != u32::from(total)
                    || payment
                        .permanent_targets
                        .iter()
                        .copied()
                        .collect::<BTreeSet<_>>()
                        .len()
                        != shares.len()
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let StackPayload::Instructions(bound) = &mut payload else {
                    return Err(EffectError::InconsistentProgram);
                };
                bound.target_division.extend(
                    mode_targets
                        .iter()
                        .zip(shares)
                        .map(|(choice, &share)| (choice.id, share)),
                );
            }
            if one_per_player || one_per_opponent {
                let StackPayload::Instructions(bound) = &mut payload else {
                    return Err(EffectError::InconsistentProgram);
                };
                let mut represented = BTreeSet::new();
                for choice in &mode_targets {
                    let TargetRef::Object(id) = choice.target else {
                        return Err(EffectError::IllegalTarget);
                    };
                    let seat = current
                        .get(id)
                        .and_then(|view| view.controller)
                        .ok_or(EffectError::IllegalTarget)?;
                    if !represented.insert(seat) {
                        return Err(EffectError::IllegalTarget);
                    }
                    bound.target_controllers.insert(choice.id, seat);
                }
            }
        } else if !payment.permanent_targets.is_empty() {
            return Err(EffectError::UnexpectedTarget);
        }
        if !payment.target_division.is_empty()
            && payload
                .body()
                .and_then(ExecutableBody::target_division_total)
                .is_none()
        {
            return Err(EffectError::InvalidCastResource);
        }
        match (target_contract, target) {
            (_, None) if target_bounds.is_some() => {}
            (BodyTarget::None, Some(_)) => return Err(EffectError::UnexpectedTarget),
            (BodyTarget::None, None) => {}
            (_, None) => return Err(EffectError::TargetRequired),
            (_, Some(target)) => {
                if Self::protected_target(
                    target,
                    &protection::ProtectionSource::from(current_spell).with_controller(actor),
                    &current,
                ) {
                    return Err(EffectError::IllegalTarget);
                }
                let legal = if cast_mutating {
                    self.legal_mutation_target(source.owner(), target, &current)
                } else if let Some(enchant) = enchant {
                    self.aura_target_legal(enchant, target, actor, &current)
                } else {
                    self.legal_body_target_in(
                        payload.body().ok_or(EffectError::InconsistentProgram)?,
                        target,
                        actor,
                        &current,
                    )
                };
                if !legal {
                    return Err(EffectError::IllegalTarget);
                }
            }
        }
        let mut emerge = payment.objects.iter().filter_map(|res| {
            if let CastResource::Emerge(id) = res {
                Some(*id)
            } else {
                None
            }
        });
        let sacrifice = emerge.next();
        let emerge_type = match &current_spell.printed_text.conditional_cost {
            Some(crate::program::ConditionalCastCost::Emerge { from, .. })
                if payment.use_alternative =>
            {
                Some(*from)
            }
            _ => None,
        };
        let emerge_discount = match (emerge_type, sacrifice) {
            (None, None) => None,
            (Some(from), Some(id))
                if emerge.next().is_none()
                    && id != spell
                    && current.get(id).is_some_and(|s| {
                        s.zone == ObjectZone::Battlefield
                            && s.controller == Some(actor)
                            && s.has_type(from)
                    }) =>
            {
                let value = self
                    .permanent_mana_value(id)
                    .ok_or(EffectError::InvalidCastResource)?;
                Some(value)
            }
            _ => return Err(EffectError::InvalidCastResource),
        };
        let required_hand_exiles = match &current_spell.printed_text.conditional_cost {
            Some(crate::program::ConditionalCastCost::ExileHandCards { count, .. })
                if payment.use_alternative =>
            {
                usize::from(*count)
            }
            _ => 0,
        };
        if payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::ExileHandForAlternative(_)))
            .count()
            != required_hand_exiles
        {
            return Err(EffectError::InvalidCastResource);
        }
        let required_taps = match &current_spell.printed_text.conditional_cost {
            Some(crate::program::ConditionalCastCost::TapPermanents { count, .. })
                if payment.use_alternative =>
            {
                usize::from(*count)
            }
            _ => 0,
        };
        if payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::TapForAlternative(_)))
            .count()
            != required_taps
        {
            return Err(EffectError::InvalidCastResource);
        }
        let mut web = payment.objects.iter().filter_map(|resource| {
            if let CastResource::WebSlinging(id) = resource {
                Some(*id)
            } else {
                None
            }
        });
        let web_paid = payment.use_alternative
            && matches!(
                current_spell.printed_text.conditional_cost,
                Some(crate::program::ConditionalCastCost::WebSlinging(_))
            );
        if web_paid {
            let id = web.next().ok_or(EffectError::InvalidCastResource)?;
            if !self
                .window
                .cards()
                .get(id)
                .is_some_and(|o| o.tapped() == Some(true))
                || !current.get(id).is_some_and(|o| {
                    o.zone == ObjectZone::Battlefield
                        && o.is_creature()
                        && o.controller == Some(actor)
                })
            {
                return Err(EffectError::InvalidCastResource);
            }
        }
        if web.next().is_some() {
            return Err(EffectError::InvalidCastResource);
        }
        let mut cost = if let Some((_, _, _, alt_mana)) = &external_alternative {
            alt_mana.clone()
        } else if from_foretell {
            let Some(crate::program::ExileAction::Foretell(price)) =
                &current_spell.printed_text.exile_action
            else {
                return Err(EffectError::InvalidCastResource);
            };
            price.clone()
        } else if resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Madness) {
            current_spell
                .printed_text
                .madness
                .clone()
                .ok_or(EffectError::InvalidCastResource)?
        } else if cast_for_miracle {
            program
                .miracle
                .as_ref()
                .ok_or(EffectError::InvalidCastResource)?
                .mana
                .clone()
        } else if from_free_exile
            || from_plot
            || resolving_cast.is_some_and(|cast| cast.kind != ExileCastAbility::Impulse)
        {
            ManaCost::free()
        } else if payment.use_alternative {
            use crate::program::ConditionalCastCost;
            let alternative = current_spell
                .printed_text
                .conditional_cost
                .as_ref()
                .ok_or(EffectError::InvalidCastResource)?;
            let eligible = match alternative {
                ConditionalCastCost::EndStep { kind, .. } => {
                    *kind != crate::program::EndStepCast::Warp
                        || source.zone() == ObjectZone::Hand(actor)
                }
                ConditionalCastCost::Emerge { .. } => emerge_discount.is_some(),
                ConditionalCastCost::Overload { .. }
                | ConditionalCastCost::Cleave { .. }
                | ConditionalCastCost::Evoke(_) => true,
                ConditionalCastCost::Sneak(_) => sneak_defender.is_some(),
                ConditionalCastCost::Mutate(_) => cast_mutating,
                ConditionalCastCost::Bestow(_) => cast_bestowed,
                ConditionalCastCost::Escape { exile, .. } => {
                    via_escape && escape_cards == usize::from(*exile)
                }
                ConditionalCastCost::Prowl(_) => self
                    .cast_history
                    .as_ref()
                    .and_then(|h| h.combat_types.get(&actor))
                    .is_some_and(|types| {
                        !types.is_disjoint(&current_spell.current_creature_subtypes)
                    }),
                ConditionalCastCost::Freerunning(_) => self
                    .cast_history
                    .as_ref()
                    .is_some_and(|h| h.freerunning.contains(&actor)),
                ConditionalCastCost::Mayhem(_) => mayhem,
                ConditionalCastCost::ExileHandCards { .. } => required_hand_exiles > 0,
                ConditionalCastCost::TapPermanents { .. } => required_taps > 0,
                ConditionalCastCost::WebSlinging(_) => web_paid,
                ConditionalCastCost::ControlsCommander(_) => {
                    current.objects.values().any(|permanent| {
                        permanent.zone == ObjectZone::Battlefield
                            && permanent.controller == Some(actor)
                            && self
                                .window
                                .cards()
                                .get(permanent.object)
                                .and_then(|card| card.card_id())
                                .is_some_and(|id| self.commanders.contains_key(&id))
                    })
                }
                ConditionalCastCost::Condition { condition, .. } => {
                    let ctx = crate::characteristics::WorldEvalCtx {
                        active_player: self.window.active_player(),
                        step: turn.map(|t| t.position().step).or(self.current_step),
                        participants: self.window.participants(),
                        life: Some(&self.life),
                        combat: self.combat.as_ref(),
                        cards: self.window.cards(),
                        programs: &self.programs,
                        state: &self.characteristics,
                        objects: &current.objects,
                        source: spell,
                        controller: actor,
                        name_rules: &self.name_rules,
                        trigger_subject: None,
                        related_player: None,
                        defending_player: None,
                    };
                    crate::characteristics::eval_world_condition(&ctx, condition)
                }
                ConditionalCastCost::Surge(_) => {
                    turn.is_some()
                        && self
                            .cast_history
                            .as_ref()
                            .is_some_and(|history| history.casters.contains(&actor))
                }
                ConditionalCastCost::Spectacle(_) => {
                    turn.is_some()
                        && self
                            .cast_history
                            .as_ref()
                            .is_some_and(|history| history.lost_life.iter().any(|p| *p != actor))
                }
                ConditionalCastCost::Awaken { .. }
                | ConditionalCastCost::Impending { .. }
                | ConditionalCastCost::Offering { .. } => true,
                ConditionalCastCost::Harmonize(_) => {
                    from_graveyard || source.zone() == ObjectZone::Hand(actor)
                }
                ConditionalCastCost::Alternative { condition, .. } => {
                    condition.as_deref().is_none_or(|cond| {
                        let ctx = crate::characteristics::WorldEvalCtx {
                            active_player: self.window.active_player(),
                            step: turn.map(|t| t.position().step).or(self.current_step),
                            participants: self.window.participants(),
                            life: Some(&self.life),
                            combat: self.combat.as_ref(),
                            cards: self.window.cards(),
                            programs: &self.programs,
                            state: &self.characteristics,
                            objects: &current.objects,
                            source: spell,
                            controller: actor,
                            name_rules: &self.name_rules,
                            trigger_subject: None,
                            related_player: None,
                            defending_player: None,
                        };
                        crate::characteristics::eval_world_condition(&ctx, cond)
                    })
                }
            };
            if via_flashback || !eligible {
                return Err(EffectError::InvalidCastResource);
            }
            alternative.mana().clone()
        } else if let Some(gy_mana) = via_gy_alt_cost {
            gy_mana.clone()
        } else {
            cost.clone()
        };
        let mut next_spell_free_used = false;
        for (rule_idx, active) in self.temporary_rules.iter().enumerate() {
            if active.consumed || active.controller != actor {
                continue;
            }
            if let crate::program::TemporaryRuleKind::NextSpellFree { filter } = &active.rule
                && filter.is_none_or(|f| {
                    self.matches_object_filter(
                        f,
                        current_spell,
                        actor,
                        None,
                        &BTreeSet::new(),
                        &current,
                    )
                })
            {
                cost = ManaCost::free();
                consumed_temp_rules.push(rule_idx);
                next_spell_free_used = true;
                break;
            }
        }
        if let Some(extra) = &flash_extra_mana {
            cost.generic = cost.generic.plus(&extra.generic);
            for (&color, amount) in &extra.required {
                let slot = cost.required.entry(color).or_insert_with(|| 0.into());
                *slot = slot.plus(amount);
            }
        }
        let pays_printed_cost = disturb.is_none()
            && !next_spell_free_used
            && !from_free_exile
            && external_alternative.is_none()
            && via_gy_alt_cost.is_none()
            && source.face_down().is_none()
            && !via_printed_flashback
            && !from_foretell
            && !from_plot
            && !payment.use_alternative
            && resolving_cast.is_none_or(|cast| cast.kind == ExileCastAbility::Impulse);
        let symbols = if pays_printed_cost {
            program.hybrid_symbols.as_slice()
        } else {
            &[]
        };
        let phyrexian_life = cost
            .choose_hybrid(symbols, &payment.hybrid_choices)
            .ok_or(EffectError::InvalidCastResource)?;
        let life_price = source
            .face_down()
            .is_none()
            .then_some(program.additional_life_cost.as_ref())
            .flatten();
        let variable_kicker = current_spell
            .printed_text
            .additional_cast_costs
            .iter()
            .enumerate()
            .any(|(i, cost)| {
                matches!(cost, crate::program::AdditionalCastCost::KickerX(_))
                    && payment.additional.contains_key(&i)
            });
        let priced_x_symbols = if cast_for_miracle {
            program
                .miracle
                .as_ref()
                .ok_or(EffectError::InvalidCastResource)?
                .x_symbols
        } else if via_printed_flashback {
            program.flashback_x_mana_symbols
        } else {
            program.x_mana_symbols
        };
        let active_kicker_extra_uses_x = current_spell
            .printed_text
            .additional_cast_costs
            .iter()
            .enumerate()
            .any(|(i, c)| {
                payment.additional.contains_key(&i)
                    && matches!(
                        c,
                        crate::program::AdditionalCastCost::KickerExtra { extra, .. }
                            if extra
                                .iter()
                                .any(crate::activated::ExtraActivationCost::uses_chosen_x)
                    )
            });
        let active_alt_extra_uses_x = payment.use_alternative
            && matches!(
                &current_spell.printed_text.conditional_cost,
                Some(crate::program::ConditionalCastCost::Alternative { extra, .. })
                    if extra
                        .iter()
                        .any(crate::activated::ExtraActivationCost::uses_chosen_x)
            );
        let active_gy_self_extra_uses_x = from_graveyard
            && matches!(
                current_spell.printed_text.graveyard_self_cast.as_ref(),
                Some(
                    crate::program::GraveyardSelfCast::WithExtraCost(extra)
                        | crate::program::GraveyardSelfCast::WithAlternativeCost { extra, .. }
                ) if extra
                    .iter()
                    .any(crate::activated::ExtraActivationCost::uses_chosen_x)
            );
        let extra_cast_uses_x = program
            .extra_cast_costs
            .iter()
            .any(crate::activated::ExtraActivationCost::uses_chosen_x)
            || active_kicker_extra_uses_x
            || active_alt_extra_uses_x
            || active_gy_self_extra_uses_x;
        let chosen_x = if priced_x_symbols == 0
            && (!cast_for_miracle || program.x_mana_symbols == 0)
            && !program.graveyard_exile_x_cost
            && !variable_kicker
            && !life_price.is_some_and(ValueExpr::is_chosen_x)
            && !extra_cast_uses_x
            || source.face_down().is_some()
        {
            if payment.chosen_x.is_some() {
                return Err(EffectError::InvalidCastResource);
            }
            None
        } else {
            let x = payment.chosen_x.clone().unwrap_or_else(|| 0.into());
            // An alternative price containing no X requires a choice of zero (CR 107.3b).
            let fixed_alternative = from_free_exile
                || external_alternative.is_some()
                || via_gy_alt_cost.is_some()
                || (via_printed_flashback && priced_x_symbols == 0)
                || from_foretell
                || from_plot
                || resolving_cast.is_some_and(|cast| cast.kind != ExileCastAbility::Impulse)
                || payment.use_alternative;
            if x < 0.into()
                || (priced_x_symbols == 0 && cast_for_miracle
                    || program.x_mana_symbols != 0 && fixed_alternative && !cast_for_miracle)
                    && x.is_positive()
            {
                return Err(EffectError::InvalidCastResource);
            }
            cost.generic = cost
                .generic
                .plus(&x.times(&i64::from(priced_x_symbols).into()));
            Some(x)
        };
        let exile_count = payment
            .objects
            .iter()
            .filter(|resource| matches!(resource, CastResource::ExileGraveyardCardForSpell(_)))
            .count();
        let expected_exiles = if program.graveyard_exile_x_cost {
            chosen_x.clone().unwrap_or_else(|| 0.into())
        } else {
            0.into()
        };
        if RulesInteger::from_usize(exile_count) != expected_exiles {
            return Err(EffectError::InvalidCastResource);
        }
        if let Some(record) = commander.and_then(|card| self.commander_record(card)) {
            cost.generic = cost
                .generic
                .plus(&record.command_zone_casts().times(&2.into()));
        }
        let mut bought_back = false;
        let mut kicked = BTreeMap::new();
        let mut replicated = BTreeMap::new();
        let mut granted_replicate = BTreeMap::new();
        let printed_costs = &current_spell.printed_text.additional_cast_costs;
        let grants = current.replicate_grants(actor, current_spell);
        for (&index, times) in &payment.additional {
            use crate::program::AdditionalCastCost;
            let derived;
            let ability = if let Some(printed) = printed_costs.get(index) {
                printed
            } else {
                let grant = grants
                    .get(index - printed_costs.len())
                    .ok_or(EffectError::InvalidCastResource)?;
                let mut price = program
                    .printed_mana_cost()
                    .cloned()
                    .ok_or(EffectError::InvalidCastResource)?;
                if let Some(x) = chosen_x.as_ref() {
                    price.generic = price
                        .generic
                        .plus(&x.times(&i64::from(program.x_mana_symbols).into()));
                }
                derived = AdditionalCastCost::Replicate(price);
                granted_replicate.insert(*grant, index);
                &derived
            };
            if !times.is_positive()
                || (!matches!(
                    ability,
                    AdditionalCastCost::Multikicker(_) | AdditionalCastCost::Replicate(_)
                ) && times != &1.into())
            {
                return Err(EffectError::InvalidCastResource);
            }
            if matches!(ability, AdditionalCastCost::Buyback(_)) {
                bought_back = true;
            } else if matches!(ability, AdditionalCastCost::Replicate(_)) {
                replicated.insert(index, times.clone());
            } else if !matches!(ability, AdditionalCastCost::Entwine(_)) {
                kicked.insert(index, times.clone());
            }
            if matches!(ability, AdditionalCastCost::KickerX(_)) {
                cost.generic = cost
                    .generic
                    .plus(chosen_x.as_ref().ok_or(EffectError::InvalidCastResource)?);
            }
            let price = ability.mana();
            cost.generic = cost.generic.plus(&price.generic.times(times));
            for (&color, price) in &price.required {
                let current = cost.required.entry(color).or_insert_with(|| 0.into());
                *current = current.plus(&price.times(times));
            }
        }
        for addition in &spliced {
            cost.generic = cost.generic.plus(&addition.cost.generic);
            for (&color, amount) in &addition.cost.required {
                let existing = cost.required.entry(color).or_insert_with(|| 0.into());
                *existing = existing.plus(amount);
            }
        }
        if let ProgramKind::Spell(spell) = &program.kind
            && let Some(spec) = &spell.modes
        {
            if !spec.mode_costs.is_empty() {
                for choice in &payment.modes {
                    let mode_price = spec
                        .mode_costs
                        .get(usize::from(choice.mode))
                        .ok_or(EffectError::InvalidCastResource)?;
                    cost.generic = cost.generic.plus(&mode_price.generic);
                    for (&color, amount) in &mode_price.required {
                        let existing = cost.required.entry(color).or_insert_with(|| 0.into());
                        *existing = existing.plus(amount);
                    }
                }
            }
            if let Some(esc) = &spec.escalate_mana {
                let extra = payment.modes.len().saturating_sub(1);
                if extra > 0 {
                    let times = RulesInteger::from_usize(extra);
                    cost.generic = cost.generic.plus(&esc.generic.times(&times));
                    for (&color, amount) in &esc.required {
                        let existing = cost.required.entry(color).or_insert_with(|| 0.into());
                        *existing = existing.plus(&amount.times(&times));
                    }
                }
            }
        }
        let keyword_cast_matches = |kind: crate::characteristics::CastKeywordCostKind| -> bool {
            use crate::characteristics::CastKeywordCostKind as K;
            use crate::program::{ConditionalCastCost, EndStepCast};
            match kind {
                K::Dash => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::EndStep {
                                kind: EndStepCast::Dash,
                                ..
                            })
                        )
                }
                K::Blitz => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::EndStep {
                                kind: EndStepCast::Blitz,
                                ..
                            })
                        )
                }
                K::Warp => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::EndStep {
                                kind: EndStepCast::Warp,
                                ..
                            })
                        )
                }
                K::Flashback => via_flashback,
                K::Buyback => bought_back,
                K::Bestow => cast_bestowed,
                K::Evoke => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::Evoke(_))
                        )
                }
                K::Mutate => cast_mutating,
                K::Escape => via_escape,
                K::Foretell => from_foretell,
                K::Madness => {
                    resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Madness)
                }
                K::Kicker => !kicked.is_empty(),
                K::Spectacle => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::Spectacle(_))
                        )
                }
                K::Prowl => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::Prowl(_))
                        )
                }
                K::Surge => {
                    payment.use_alternative
                        && matches!(
                            current_spell.printed_text.conditional_cost,
                            Some(ConditionalCastCost::Surge(_))
                        )
                }
            }
        };
        let keyword_cost_controller_matches =
            |relation: crate::program::PlayerRelation, rule_ctrl: SeatId| -> bool {
                match relation {
                    crate::program::PlayerRelation::Controller => actor == rule_ctrl,
                    crate::program::PlayerRelation::Opponent => self
                        .combat_rules
                        .opponents(self.window.participants(), rule_ctrl)
                        .is_ok_and(|ops| ops.contains(&actor)),
                    crate::program::PlayerRelation::AnyParticipant => true,
                }
            };
        for permanent in current
            .objects
            .values()
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        {
            for rule in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active_rule = |id: &crate::characteristics::StaticAbilityId| {
                    permanent.abilities.continuous == Some(*id)
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(id)
                };
                match rule {
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        id,
                        modifier:
                            crate::characteristics::SpellCostModifier::IncreaseExceptType {
                                exempt,
                                amount,
                            },
                    } if active_rule(id) && !current_spell.has_type(*exempt) => {
                        cost.generic = cost.generic.plus(&i64::from(amount.get()).into());
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        id,
                        modifier:
                            crate::characteristics::SpellCostModifier::Increase {
                                any_player,
                                opponents_only,
                                matching,
                                amount,
                            },
                    } if active_rule(id)
                        && (*any_player
                            || (*opponents_only
                                && permanent.controller.is_some_and(|c| c != actor))
                            || (!*opponents_only && permanent.controller == Some(actor)))
                        && matching.matches_characteristics(
                            current_spell,
                            aura_or_equipment,
                            permanent.entry_creature_type,
                        ) =>
                    {
                        cost.generic = cost.generic.plus(&i64::from(amount.get()).into());
                    }
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        id,
                        modifier:
                            crate::characteristics::SpellCostModifier::KeywordCastCost {
                                keyword,
                                controller,
                                delta,
                                less: false,
                            },
                    } if active_rule(id)
                        && permanent.controller.is_some_and(|ctrl| {
                            keyword_cost_controller_matches(*controller, ctrl)
                        })
                        && keyword_cast_matches(*keyword) =>
                    {
                        cost.generic = cost.generic.plus(&i64::from(*delta).into());
                    }
                    _ => {}
                }
            }
        }
        for (seat, rule) in &current.emblem_rules {
            if let crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::KeywordCastCost {
                        keyword,
                        controller,
                        delta,
                        less: false,
                    },
                ..
            } = rule
                && keyword_cost_controller_matches(*controller, *seat)
                && keyword_cast_matches(*keyword)
            {
                cost.generic = cost.generic.plus(&i64::from(*delta).into());
            }
        }
        for active in &self.temporary_rules {
            if let Some(crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::KeywordCastCost {
                        keyword,
                        controller,
                        delta,
                        less: false,
                    },
                ..
            }) = self.active_temporary_static(active, &current)
                && keyword_cost_controller_matches(*controller, active.controller)
                && keyword_cast_matches(*keyword)
            {
                cost.generic = cost.generic.plus(&i64::from(*delta).into());
            }
        }
        if let Some(discount) = emerge_discount {
            cost.generic = cost.generic.minus(&discount).max(0.into());
        }
        for (rule_idx, active) in self.temporary_rules.iter().enumerate() {
            if active.consumed {
                continue;
            }
            match &active.rule {
                crate::program::TemporaryRuleKind::SpellCostReduction {
                    filter,
                    generic,
                    next_only,
                } if active.controller == actor
                    && self.matches_object_filter(
                        *filter,
                        current_spell,
                        actor,
                        None,
                        &BTreeSet::new(),
                        &current,
                    ) =>
                {
                    cost.generic = cost
                        .generic
                        .minus(&i64::from(*generic).into())
                        .max(0.into());
                    if *next_only {
                        consumed_temp_rules.push(rule_idx);
                    }
                }
                crate::program::TemporaryRuleKind::SpellCostIncrease {
                    filter, generic, ..
                } if active.resolved_players.contains(&actor)
                    && self.matches_object_filter(
                        *filter,
                        current_spell,
                        active.controller,
                        None,
                        &BTreeSet::new(),
                        &current,
                    ) =>
                {
                    cost.generic = cost.generic.plus(&i64::from(*generic).into());
                }
                _ => {}
            }
        }
        for reduction in &current_spell.printed_text.cast_reductions {
            let discount = match reduction {
                crate::program::CastReduction::BattlefieldValue(value) => {
                    self.battlefield_cost_value(*value, actor, &current)?
                }
                crate::program::CastReduction::ExileGraveyardCreatures { per_card } => {
                    RulesInteger::from_usize(
                        payment
                            .objects
                            .iter()
                            .filter(|r| matches!(r, CastResource::ExileCreatureForReduction(_)))
                            .count(),
                    )
                    .times(&i64::from(*per_card).into())
                }
                crate::program::CastReduction::PerObject(count) => {
                    self.counted_cast_discount(*count, spell, actor, &current)
                }
                crate::program::CastReduction::Affinity(selector) => {
                    crate::characteristics::count_permanents(
                        self.window.cards(),
                        &current,
                        spell,
                        actor,
                        selector,
                        &self.name_rules,
                    )
                }
                crate::program::CastReduction::StackTargetThreatensCreature {
                    minimum_power,
                    discount,
                } => {
                    if target.is_some_and(|target| {
                        self.stack_entry_targets_controlled_permanent(
                            target,
                            actor,
                            &current,
                            Some(*minimum_power),
                        )
                    }) {
                        i64::from(*discount).into()
                    } else {
                        0.into()
                    }
                }
                crate::program::CastReduction::CountZone {
                    zone,
                    filter,
                    per_card,
                } => self
                    .resolve_value_expr_at(
                        actor,
                        &crate::program::ValueExpr::Count {
                            zone: *zone,
                            filter: filter.clone(),
                            controller_scope: None,
                        },
                        None,
                    )?
                    .times(&i64::from(*per_card).into()),
                crate::program::CastReduction::Conditional {
                    condition,
                    discount,
                } => {
                    let lhs = self.resolve_value_expr_at(actor, &condition.left, None)?;
                    let rhs = self.resolve_value_expr_at(actor, &condition.right, None)?;
                    if condition.comparison.evaluate(&lhs, &rhs) {
                        i64::from(*discount).into()
                    } else {
                        0.into()
                    }
                }
                crate::program::CastReduction::IfTargets { filter, discount } => {
                    let matched = target
                        .into_iter()
                        .chain(payment.permanent_targets.iter().copied())
                        .any(|t| {
                            matches!(t, TargetRef::Object(id) if current.get(id).is_some_and(|obj| {
                                self.matches_object_filter(
                                    **filter,
                                    obj,
                                    actor,
                                    None,
                                    &BTreeSet::new(),
                                    &current,
                                )
                            }))
                        });
                    if matched {
                        i64::from(*discount).into()
                    } else {
                        0.into()
                    }
                }
                crate::program::CastReduction::Undaunted => RulesInteger::from_usize(
                    self.window
                        .participants()
                        .iter()
                        .filter(|&&seat| seat != actor)
                        .count(),
                ),
                crate::program::CastReduction::Dynamic(expr) => {
                    self.resolve_value_expr_at(actor, expr, None)?
                }
            };
            cost.generic = cost.generic.minus(&discount).max(0.into());
        }
        let printed_value = if source.face_down().is_some() {
            0.into()
        } else {
            let mut value = program.mana_value();
            if let Some(x) = &chosen_x {
                value = value.plus(&x.times(&i64::from(program.x_mana_symbols).into()));
            }
            value
        };
        for source in current.objects.values().filter(|p| {
            p.object != spell
                && match p.zone {
                    ObjectZone::Command => self
                        .window
                        .cards()
                        .get(p.object)
                        .is_some_and(|c| c.owner() == actor),
                    ObjectZone::Battlefield => p.controller == Some(actor),
                    _ => false,
                }
        }) {
            for rule in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
            {
                if let crate::characteristics::StaticContinuousAbility::CostModifier { id, modifier: crate::characteristics::SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, amount } }=rule
                    && (source.abilities.continuous==Some(*id) || source.abilities.additional_battlefield_abilities.contains(id))
                    && current_spell.current_creature_subtypes.contains(subtype) {
                    cost.generic=cost.generic.minus(&i64::from(amount.get()).into()).max(0.into());
                }
            }
        }
        for permanent in current.objects.values() {
            use crate::characteristics::{
                SpellCostModifier, SpellDiscount, StaticContinuousAbility,
            };
            if permanent.zone != ObjectZone::Battlefield {
                continue;
            }
            for rule in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active_rule = |id: &crate::characteristics::StaticAbilityId| {
                    permanent.abilities.continuous == Some(*id)
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(id)
                };
                if let StaticContinuousAbility::CostModifier {
                    id,
                    modifier:
                        SpellCostModifier::Discount {
                            any_player: true,
                            matching,
                            amount: SpellDiscount::Fixed(amount),
                        },
                } = rule
                    && active_rule(id)
                    && matching.matches_characteristics(
                        current_spell,
                        aura_or_equipment,
                        permanent.entry_creature_type,
                    )
                {
                    cost.generic = cost
                        .generic
                        .minus(&i64::from(amount.get()).into())
                        .max(0.into());
                }
                if let StaticContinuousAbility::CostModifier {
                    id,
                    modifier:
                        SpellCostModifier::FirstSpellRule {
                            filter,
                            own_turn_only,
                            each_player: true,
                            discount,
                            ..
                        },
                } = rule
                    && active_rule(id)
                    && *discount > 0
                    && (!*own_turn_only || actor == self.window.active_player())
                    && self.spell_filter_matches_cast(
                        *filter,
                        current_spell,
                        source,
                        actor,
                        aura_or_equipment,
                        permanent.entry_creature_type,
                    )
                    && (filter.ordinal_this_turn.is_some()
                        || self.prior_matching_spells_cast_this_turn(actor, *filter) == 0)
                {
                    cost.generic = cost
                        .generic
                        .minus(&i64::from(*discount).into())
                        .max(0.into());
                }
                if let StaticContinuousAbility::CostModifier {
                    id,
                    modifier:
                        SpellCostModifier::KeywordCastCost {
                            keyword,
                            controller,
                            delta,
                            less: true,
                        },
                } = rule
                    && active_rule(id)
                    && permanent
                        .controller
                        .is_some_and(|ctrl| keyword_cost_controller_matches(*controller, ctrl))
                    && keyword_cast_matches(*keyword)
                {
                    cost.generic = cost.generic.minus(&i64::from(*delta).into()).max(0.into());
                }
            }
            if permanent.controller != Some(actor) {
                continue;
            }
            if permanent.printed_text.continuous.iter().chain(&permanent.printed_text.additional_battlefield_abilities).any(|ability|
                matches!(ability, StaticContinuousAbility::CostModifier { id, modifier: SpellCostModifier::ChosenTypeOneLessOfEachColor } if permanent.abilities.continuous==Some(*id) || permanent.abilities.additional_battlefield_abilities.contains(id)))
                && permanent.entry_creature_type.is_some_and(|kind|current_spell.current_creature_subtypes.contains(&kind)) {
                cost.required.retain(|color,amount| {
                    if *color!=ManaType::Colourless {*amount=amount.minus(&1.into());}
                    amount.is_positive()
                });
            }
            for rule in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active_rule = |id: &crate::characteristics::StaticAbilityId| {
                    permanent.abilities.continuous == Some(*id)
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(id)
                };
                let discount = match rule {
                    StaticContinuousAbility::CostModifier {
                        id,
                        modifier:
                            SpellCostModifier::Discount {
                                any_player: false,
                                matching,
                                amount,
                            },
                    } if active_rule(id)
                        && matching.matches_characteristics(
                            current_spell,
                            aura_or_equipment,
                            permanent.entry_creature_type,
                        ) =>
                    {
                        match amount {
                            SpellDiscount::SourcePlusOneCounters => {
                                self.counters_of_kind(permanent.object, CounterKind::PlusOne)
                            }
                            SpellDiscount::PerExperienceCounter(value) => self
                                .experience_counters(actor)
                                .times(&i64::from(value.get()).into()),
                            SpellDiscount::Fixed(value) => {
                                RulesInteger::from(i64::from(value.get()))
                            }
                            SpellDiscount::ControlledCreaturesWithPlusOneCounters => self
                                .counted_cast_discount(
                                crate::program::CastCostCount::ControlledCreaturesWithPlusOneCounters,
                                spell,
                                actor,
                                &current,
                            ),
                        }
                    }
                    StaticContinuousAbility::CostModifier {
                        id,
                        modifier: SpellCostModifier::HighManaValueProtected { minimum, reduction },
                    } if active_rule(id)
                        && printed_value >= i64::from(minimum.get()).into() =>
                    {
                        i64::from(reduction.get()).into()
                    }
                    StaticContinuousAbility::CostModifier {
                        id,
                        modifier:
                            SpellCostModifier::FirstSpellRule {
                                filter,
                                own_turn_only,
                                each_player: false,
                                discount,
                                ..
                            },
                    } if active_rule(id)
                        && *discount > 0
                        && (!*own_turn_only || actor == self.window.active_player())
                        && self.spell_filter_matches_cast(
                            *filter,
                            current_spell,
                            source,
                            actor,
                            aura_or_equipment,
                            permanent.entry_creature_type,
                        )
                        && (filter.ordinal_this_turn.is_some()
                            || self.prior_matching_spells_cast_this_turn(actor, *filter) == 0) =>
                    {
                        RulesInteger::from(i64::from(*discount))
                    }
                    _ => continue,
                };
                cost.generic = cost.generic.minus(&discount).max(0.into());
            }
        }
        for (seat, rule) in &current.emblem_rules {
            if let crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::FirstSpellRule {
                        filter,
                        own_turn_only,
                        each_player,
                        discount,
                        ..
                    },
                ..
            } = rule
                && (*each_player || *seat == actor)
                && *discount > 0
                && (!*own_turn_only || actor == self.window.active_player())
                && self.spell_filter_matches_cast(
                    *filter,
                    current_spell,
                    source,
                    actor,
                    aura_or_equipment,
                    None,
                )
                && (filter.ordinal_this_turn.is_some()
                    || self.prior_matching_spells_cast_this_turn(actor, *filter) == 0)
            {
                cost.generic = cost
                    .generic
                    .minus(&i64::from(*discount).into())
                    .max(0.into());
            }
            if let crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::KeywordCastCost {
                        keyword,
                        controller,
                        delta,
                        less: true,
                    },
                ..
            } = rule
                && keyword_cost_controller_matches(*controller, *seat)
                && keyword_cast_matches(*keyword)
            {
                cost.generic = cost.generic.minus(&i64::from(*delta).into()).max(0.into());
            }
        }
        for active in &self.temporary_rules {
            let Some(rule) = self.active_temporary_static(active, &current) else {
                continue;
            };
            if let crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::KeywordCastCost {
                        keyword,
                        controller,
                        delta,
                        less: true,
                    },
                ..
            } = rule
                && keyword_cost_controller_matches(*controller, active.controller)
                && keyword_cast_matches(*keyword)
            {
                cost.generic = cost.generic.minus(&i64::from(*delta).into()).max(0.into());
            }
            if let crate::characteristics::StaticContinuousAbility::CostModifier {
                modifier:
                    crate::characteristics::SpellCostModifier::FirstSpellRule {
                        filter,
                        own_turn_only,
                        each_player,
                        discount,
                        ..
                    },
                ..
            } = rule
                && (*each_player || active.controller == actor)
                && *discount > 0
                && (!*own_turn_only || actor == self.window.active_player())
                && self.spell_filter_matches_cast(
                    *filter,
                    current_spell,
                    source,
                    actor,
                    aura_or_equipment,
                    None,
                )
                && (filter.ordinal_this_turn.is_some()
                    || self.prior_matching_spells_cast_this_turn(actor, *filter) == 0)
            {
                cost.generic = cost
                    .generic
                    .minus(&i64::from(*discount).into())
                    .max(0.into());
            }
        }
        let mut unpaid_mana = cost.clone();
        self.allocate_cast_resources(
            &mut unpaid_mana,
            &payment.objects,
            &current,
            current_spell,
            actor,
        )?;
        if let Some(any_type) = self
            .flexible_exile_mana(spell, actor)
            .or_else(|| (temp_zone_any_color || static_exile_grant.any_color).then_some(false))
        {
            // Only mana, after object-based payments and discount calculation, is flexible.
            // An explicit colorless requirement is still payable only with colorless mana.
            let colored: Vec<_> = unpaid_mana
                .required
                .keys()
                .copied()
                .filter(|kind| any_type || *kind != ManaType::Colourless)
                .collect();
            for color in colored {
                if let Some(amount) = unpaid_mana.required.remove(&color) {
                    unpaid_mana.generic = unpaid_mana.generic.plus(&amount);
                }
            }
        }
        let mut window = if payment.prepare {
            self.window
                .admit_prepared_spell_copy(self.window.revision(), actor, program.clone())?
        } else {
            self.window
                .admit_card_spell(self.window.revision(), actor, spell, resolving_parent)?
        };
        let entry = window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .id();
        if let Some((_, half)) = selected {
            let Some(StackKind::CardSpell(stacked)) =
                window.entries().last().map(|entry| entry.kind())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            window.select_split_half(*stacked, half)?;
        }
        if siege || disturb.is_some() || payment.modal_back {
            let Some(StackKind::CardSpell(stacked)) =
                window.entries().last().map(|entry| entry.kind())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            window.select_siege_back(*stacked)?;
        }
        if payment.prototype {
            let Some(StackKind::CardSpell(stacked)) =
                window.entries().last().map(|entry| entry.kind())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            window.select_prototype_spell(*stacked)?;
        }
        if payment.adventure {
            window.select_adventure_spell(entry)?;
        }
        let life_payment = match life_price {
            Some(expr) if expr.is_chosen_x() => chosen_x.clone(),
            Some(ValueExpr::Literal(n)) => Some(n.clone()),
            None => None,
            _ => return Err(EffectError::InconsistentProgram),
        };
        if actor == self.window.active_player()
            && turn.is_some_and(|turn| {
                matches!(
                    turn.position().step,
                    crate::turn::Step::PrecombatMain | crate::turn::Step::PostcombatMain
                )
            })
        {
            window.mark_main_phase_cast(entry)?;
        }
        let life_payment = if alternative_life {
            Some(
                life_payment
                    .unwrap_or_else(|| 0.into())
                    .plus(&program.mana_value()),
            )
        } else {
            life_payment
        };
        let life_payment = if phyrexian_life.is_positive() {
            Some(phyrexian_life.plus(&life_payment.unwrap_or_else(|| 0.into())))
        } else {
            life_payment
        };
        let life_payment = if via_printed_flashback && program.flashback_life > 0 {
            Some(
                life_payment
                    .unwrap_or_else(|| 0.into())
                    .plus(&i64::from(program.flashback_life).into()),
            )
        } else {
            life_payment
        };
        if let Some(x) = chosen_x {
            window.set_chosen_x(entry, x)?;
        }
        if let Some(target) = target {
            mode_targets.insert(
                0,
                TargetChoice {
                    id: TargetId(0),
                    clause: TargetClauseId(0),
                    target,
                },
            );
        }
        if let Some((minimum, maximum)) = target_bounds {
            mode_clauses = vec![TargetClause {
                id: TargetClauseId(0),
                minimum,
                maximum,
            }];
        } else if mode_clauses.is_empty() {
            mode_clauses = mode_targets
                .iter()
                .map(|choice| TargetClause {
                    id: choice.clause,
                    minimum: 1,
                    maximum: 1,
                })
                .collect();
        }
        splice::append_splice_text(&mut payload, &spliced, &mut mode_clauses, &mut mode_targets)?;
        if !mode_clauses.is_empty() {
            window.bind_targets(entry, mode_clauses, mode_targets)?;
        }
        if payment.use_alternative {
            window.mark_conditional_cost(entry)?;
            if let Some(defender) = sneak_defender {
                window.note_attacking_defender(entry, defender)?;
            }
        }
        let bargained = payment
            .objects
            .iter()
            .any(|r| matches!(r, CastResource::Bargain(_)));
        if bought_back || bargained || !kicked.is_empty() {
            window.set_additional_costs(entry, bought_back, bargained, kicked)?;
        }
        if !payment.additional.is_empty()
            || !replicated.is_empty()
            || payment.objects.iter().any(|r| {
                matches!(
                    r,
                    CastResource::SacrificeForSpell(_)
                        | CastResource::DiscardForSpell(_)
                        | CastResource::TapCreatureForSpell(_)
                        | CastResource::ExileGraveyardCreatureForSpell(_)
                        | CastResource::ExileGraveyardCardForSpell(_)
                        | CastResource::ExtraCost(_)
                )
            })
        {
            window.mark_additional_cost_paid(entry)?;
        }
        let paid_granted_replicate = !granted_replicate.is_empty();
        if !replicated.is_empty() {
            window.set_replicate(entry, replicated, granted_replicate)?;
        }
        if aftermath {
            window.mark_exile_cast(entry, crate::stack_window::ExileCast::Aftermath)?;
        } else if disturb.is_some() || cast_graveyard_target {
            window.mark_exile_cast(entry, crate::stack_window::ExileCast::IfGraveyard)?;
        } else if via_flashback {
            window.mark_exile_cast(entry, crate::stack_window::ExileCast::Flashback)?;
        } else if matches!(graveyard_discard, Some(CastResource::JumpStart(_))) {
            window.mark_exile_cast(entry, crate::stack_window::ExileCast::JumpStart)?;
        }
        let mut next = self.clone();
        if payment.prepare {
            next.characteristics.prepared.remove(&spell);
        }
        if let Some(idx) = temp_zone_consumed_idx {
            consumed_temp_rules.push(idx);
        }
        let mut mark_entry_uncounterable = false;
        let mut uncounterable_colors = crate::program::ColorMask::EMPTY;
        for (rule_idx, active) in self.temporary_rules.iter().enumerate() {
            if active.consumed {
                continue;
            }
            match &active.rule {
                crate::program::TemporaryRuleKind::CantBeCountered {
                    filter,
                    controller_only,
                    next_only: true,
                } => {
                    if *controller_only && active.controller != actor {
                        continue;
                    }
                    let matches_spell = filter.is_none_or(|f| {
                        self.matches_object_filter(
                            f,
                            current_spell,
                            active.controller,
                            None,
                            &BTreeSet::new(),
                            &current,
                        )
                    });
                    if matches_spell {
                        consumed_temp_rules.push(rule_idx);
                        mark_entry_uncounterable = true;
                    }
                }
                crate::program::TemporaryRuleKind::CantBeCounteredByColors {
                    colors,
                    filter,
                    controller_only,
                    next_only: true,
                } => {
                    if *controller_only && active.controller != actor {
                        continue;
                    }
                    let matches_spell = filter.is_none_or(|f| {
                        self.matches_object_filter(
                            f,
                            current_spell,
                            active.controller,
                            None,
                            &BTreeSet::new(),
                            &current,
                        )
                    });
                    if matches_spell {
                        consumed_temp_rules.push(rule_idx);
                        uncounterable_colors = uncounterable_colors.union(*colors);
                    }
                }
                _ => {}
            }
        }
        for rule_idx in consumed_temp_rules {
            if let Some(active) = next.temporary_rules.get_mut(rule_idx) {
                active.consumed = true;
            }
        }
        if mark_entry_uncounterable {
            next.uncounterable_stack_entries.insert(entry);
        }
        if !uncounterable_colors.is_empty() {
            let slot = next
                .uncounterable_by_colors
                .entry(entry)
                .or_insert(crate::program::ColorMask::EMPTY);
            *slot = slot.union(uncounterable_colors);
        }
        next.revision = revision;
        next.payloads.insert(entry, payload);
        if let Some(kind) = type_payment {
            let (&grant, _) = next
                .typed_exile_cohorts
                .iter()
                .find(|(_, cohort)| cohort.player == actor && cohort.cards.contains(&spell))
                .ok_or(EffectError::InconsistentProgram)?;
            next.typed_exile_pending.insert(entry, (grant, kind));
        }
        let permissions = graveyard_permission
            .and_then(|(source, ability, once)| once.then_some((source, ability)))
            .into_iter()
            .chain(static_exile_once_permission)
            .chain(library_top_once_permission)
            .chain(
                external_alternative
                    .and_then(|(source, ability, once, _)| once.then_some((source, ability))),
            )
            .collect::<Vec<_>>();
        if !permissions.is_empty() {
            next.source_cast_pending.insert(entry, permissions);
        }
        next.characteristics.foretold.remove(&spell);
        next.window = window;
        let mut append = RulesJournal::default();
        let announcement = self.rules_snapshot(turn)?;
        let StackKind::CardSpell(admitted_object) = next
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .kind()
        else {
            return Err(EffectError::InconsistentProgram);
        };
        if cast_bestowed {
            next.characteristics
                .bestowed_spells
                .insert(*admitted_object);
        }
        if resolving_cast.is_some_and(|cast| cast.kind == ExileCastAbility::Suspend)
            && current_spell.is_creature()
        {
            next.characteristics
                .suspend_haste
                .insert(*admitted_object, (actor, None));
        }
        let resulting_mana_value = next.stack_spell_mana_value(*admitted_object)?;
        if let Some(cast) = resolving_cast
            && cast.kind == ExileCastAbility::Cascade
        {
            let cascade = next
                .cascade_batch(cast.parent)
                .ok_or(EffectError::InconsistentProgram)?;
            if !cascade.allows_result(spell, &resulting_mana_value) {
                return Err(EffectError::InvalidCastResource);
            }
        }
        let movement = crate::object_identity::ObjectMovement {
            before: source.clone(),
            after: next
                .window
                .cards()
                .get(*admitted_object)
                .ok_or(EffectError::InconsistentProgram)?
                .clone(),
        };
        let announcement_record_start = append.records.len();
        next.emit_from(
            crate::occurrence::EventCause::CastAnnouncement { entry },
            &announcement,
            turn,
            &mut append,
            |previous, current, controllers| {
                let mut events = if payment.prepare {
                    vec![crate::occurrence::RulesOccurrence::PreparedSpellCast {
                        source: spell,
                        entry,
                    }]
                } else {
                    vec![zone_occurrence_at(
                        previous,
                        current,
                        &movement,
                        crate::occurrence::ZoneReason::CastAnnouncement,
                        controllers,
                    )?]
                };
                events.extend(spliced.iter().map(|addition| {
                    crate::occurrence::RulesOccurrence::HandCardRevealed {
                        object: addition.card.clone(),
                    }
                }));
                Ok(events)
            },
        )?;
        let announcement_event = append.records[announcement_record_start..].iter().find_map(|record| match record {
            crate::occurrence::RulesRecord::Event(event) if matches!(&event.cause, crate::occurrence::EventCause::CastAnnouncement { entry: actual } if *actual == entry) => Some(event.id),
            _ => None,
        }).ok_or(EffectError::InconsistentProgram)?;
        let has_attached_spell_copy_trigger = current.objects.values().any(|permanent| {
            permanent.zone == ObjectZone::Battlefield
                && self.characteristics.attachments.get(&permanent.object)
                    == Some(&TargetRef::Player(actor))
                && permanent.printed_text.triggered.iter().any(|trigger| {
                    permanent.abilities.triggered.contains(&trigger.id)
                        && matches!(
                            trigger.condition.event,
                            crate::program::TriggerEvent::SpellCast {
                                caster: crate::program::TriggerPlayerScope::EnchantedPlayer,
                                ..
                            }
                        )
                        && trigger.body.instructions.contains(&Instruction::CopyOnCast(
                            crate::program::CastCopyKeyword::OfferCopyToOtherPlayers,
                        ))
                })
        });
        let prepared = PreparedCastPayment {
            library_tops: self
                .window
                .participants()
                .iter()
                .filter_map(|&seat| {
                    self.window
                        .cards()
                        .library(seat)
                        .first()
                        .map(|&id| (seat, id))
                })
                .collect(),
            entry,
            actor,
            cost,
            unpaid_mana,
            payment,
            life_payment,
            mana_value: resulting_mana_value,
            sunburst: current_spell
                .printed_text
                .entry_counter_abilities
                .contains(&crate::program::EntryCounterAbility::Sunburst),
            copy_source_needed: paid_granted_replicate
                || current_spell
                    .printed_text
                    .triggered
                    .iter()
                    .any(|t| t.cast_copy_keyword().is_some())
                || (source.zone() == ObjectZone::Library(actor)
                    || self.triggers.watches_next_spell(actor)
                    || has_attached_spell_copy_trigger)
                    && (current_spell.has_type(CardType::Instant)
                        || current_spell.has_type(CardType::Sorcery)),
            original_spell: source.clone(),
            announcement_event,
            graveyard_below: if cast_graveyard_target {
                self.window
                    .cards()
                    .graveyard(actor)
                    .iter()
                    .skip_while(|&&id| id != spell)
                    .skip(1)
                    .copied()
                    .collect()
            } else {
                Vec::new()
            },
            foretold_at: self.characteristics.foretold.get(&spell).copied(),
            plotted_at: self.plotted.get(&spell).copied(),
            warped_at: self.warped.get(&spell).copied(),
            time_counters: self.characteristics.time_counters.get(&spell).cloned(),
        };
        if let Some(cast) = resolving_cast {
            if cast.mana_abilities.iter().any(|act| {
                !matches!(
                    current
                        .get(act.source)
                        .and_then(|p| p.mana_output(act.ability))
                        .as_deref(),
                    Some(ManaOutput::Modified {
                        modifier: crate::mana::ManaOutputModifier::TapControlledArtifact,
                        ..
                    })
                ) && prepared.payment.objects.iter().any(|resource| {
                    matches!(
                        resource,
                        CastResource::TapForFlashback(_)
                            | CastResource::TapForAlternative(_)
                            | CastResource::Convoke { .. }
                            | CastResource::Improvise(_)
                    ) && resource.object() == act.source
                })
            }) {
                return Err(EffectError::InvalidCastResource);
            }
            if cast.mana_abilities.iter().any(|act| {
                act.cost_objects.iter().any(|cost| {
                    prepared
                        .payment
                        .objects
                        .iter()
                        .any(|resource| resource.object() == *cost)
                })
            }) {
                return Err(EffectError::InvalidCastResource);
            }
            if let Some((consumed, damage_journal)) =
                next.activate_cast_mana(cast, actor, &mut append)?
            {
                let receipt = ManaPaymentRecord {
                    from_lots: Default::default(),
                    source: entry,
                    payer: actor,
                    cost: prepared.cost.clone(),
                    spend: Vec::new(),
                    restricted_mana: Vec::new(),
                };
                next.deferred_mana_cast = Some(DeferredManaCast {
                    prepared,
                    remaining: cast.mana_abilities[consumed..].to_vec(),
                    parent_entry: cast.parent,
                    kind: cast.kind,
                    turn: turn.cloned(),
                    parent_cursor: None,
                    damage_journal,
                });
                return Ok(CastTransition::new(next, receipt, append));
            }
        }
        next.finish_cast_payment(prepared, turn, append)
    }

    pub(super) fn cast_timing_satisfied(
        &self,
        restriction: &crate::program::CastTimingRestriction,
        source: ObjectId,
        caster: SeatId,
        turn: Option<&TurnScheduler>,
    ) -> Result<bool, EffectError> {
        self.cast_timing_satisfied_with_x(restriction, source, caster, turn, None)
    }

    fn cast_timing_satisfied_with_x(
        &self,
        restriction: &crate::program::CastTimingRestriction,
        source: ObjectId,
        caster: SeatId,
        turn: Option<&TurnScheduler>,
        chosen_x: Option<&RulesInteger>,
    ) -> Result<bool, EffectError> {
        use crate::program::CastTimingRestriction as R;
        use crate::turn::{Phase, Step};
        let step = turn.map(|s| s.position().step).or(self.current_step);
        let active = self.window.active_player();
        match restriction {
            R::Sorcery => Ok(active == caster
                && self.window.entries().is_empty()
                && matches!(step, Some(Step::PrecombatMain | Step::PostcombatMain))),
            R::DuringCombat => Ok(step.is_some_and(|s| s.phase() == Phase::Combat)),
            R::DuringCombatBeforeBlockers => Ok(matches!(
                step,
                Some(Step::BeginningCombat | Step::DeclareAttackers)
            )),
            R::BeforeCombatDamage => Ok(matches!(
                step,
                Some(
                    Step::Untap
                        | Step::Upkeep
                        | Step::Draw
                        | Step::PrecombatMain
                        | Step::BeginningCombat
                        | Step::DeclareAttackers
                        | Step::DeclareBlockers
                )
            )),
            R::DuringCombatAfterBlockers => Ok(matches!(
                step,
                Some(Step::DeclareBlockers | Step::CombatDamage | Step::EndCombat)
            )),
            R::DuringDeclareAttackers => Ok(step == Some(Step::DeclareAttackers)),
            R::DuringDeclareBlockers => Ok(step == Some(Step::DeclareBlockers)),
            R::DuringDeclareAttackersIfAttacked => Ok(step == Some(Step::DeclareAttackers)
                && !self.characteristics.attacked_this_turn_players.is_empty()),
            R::DuringCombatOnOpponentsTurn => {
                Ok(active != caster && step.is_some_and(|s| s.phase() == Phase::Combat))
            }
            R::DuringYourTurn => Ok(active == caster),
            R::DuringOpponentsTurn => Ok(active != caster),
            R::DuringStep(expected) => Ok(step == Some(*expected)),
            R::DuringOwnStep(expected) => Ok(active == caster && step == Some(*expected)),
            R::DuringOpponentsStep(expected) => Ok(active != caster && step == Some(*expected)),
            R::XNonZero => Ok(chosen_x.is_some_and(RulesInteger::is_positive)),
            R::AfterCombat => Ok(matches!(
                step,
                Some(Step::PostcombatMain | Step::End | Step::Cleanup)
            ) || (self.characteristics.combat_phases_this_turn > 0
                && step.is_some_and(|s| s.phase() != Phase::Combat))),
            R::Condition(cond) => {
                let evaluated = self.evaluated_characteristics()?;
                let ctx = crate::characteristics::WorldEvalCtx {
                    active_player: active,
                    step,
                    participants: self.window.participants(),
                    life: Some(&self.life),
                    combat: self.combat.as_ref(),
                    cards: self.window.cards(),
                    programs: &self.programs,
                    state: &self.characteristics,
                    objects: &evaluated.objects,
                    source,
                    controller: caster,
                    name_rules: &self.name_rules,
                    trigger_subject: None,
                    related_player: None,
                    defending_player: None,
                };
                Ok(crate::characteristics::eval_world_condition(&ctx, cond))
            }
            R::All(items) => {
                for item in items {
                    if !self.cast_timing_satisfied_with_x(item, source, caster, turn, chosen_x)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    fn next_spell_keyword_for_cast(
        &self,
        active: &ActiveTemporaryRule,
        evaluated: &EvaluatedCharacteristics,
        spell: &crate::characteristics::ObjectCharacteristics,
        actor: SeatId,
    ) -> Option<KeywordAbility> {
        let crate::program::TemporaryRuleKind::NextSpellKeyword { filter, keyword } = &active.rule
        else {
            return None;
        };
        (!active.consumed
            && active.controller == actor
            && self.matches_object_filter(
                *filter,
                spell,
                active.controller,
                None,
                &BTreeSet::new(),
                evaluated,
            ))
        .then_some(*keyword)
    }
    pub(super) fn spell_has_granted_keyword(
        &self,
        evaluated: &EvaluatedCharacteristics,
        spell: &crate::characteristics::ObjectCharacteristics,
        actor: SeatId,
        keyword: KeywordAbility,
    ) -> bool {
        self.spell_has_granted_keyword_in(evaluated, spell, actor, keyword, None)
    }

    fn spell_has_granted_keyword_in(
        &self,
        evaluated: &EvaluatedCharacteristics,
        spell: &crate::characteristics::ObjectCharacteristics,
        actor: SeatId,
        keyword: KeywordAbility,
        aura_or_equipment_override: Option<bool>,
    ) -> bool {
        let Some(physical) = self.window.cards().get(spell.object) else {
            return false;
        };
        let aura_or_equipment = aura_or_equipment_override.unwrap_or_else(|| {
            self.copiable_program(spell.object)
                .and_then(Program::permanent)
                .is_some_and(|p| {
                    (spell.has_type(CardType::Enchantment)
                        && matches!(
                            p.attachment,
                            Some(crate::attachment::AttachmentKind::Aura(_))
                        ))
                        || (spell.has_type(CardType::Artifact)
                            && matches!(
                                p.attachment,
                                Some(crate::attachment::AttachmentKind::Equipment)
                            ))
                })
        });
        let rule_grants =
            |ability: &crate::characteristics::StaticContinuousAbility,
             owner_matches: bool,
             entry_type: Option<spellwake_contracts::creature_types::CreatureTypeId>| {
                matches!(
                    ability,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission: crate::characteristics::CastPermissionRule::GrantSpellKeyword {
                            any_player,
                            matching,
                            keyword: granted,
                        },
                        ..
                    } if (*any_player || owner_matches)
                        && *granted == keyword
                        && self.spell_filter_matches_cast(
                            *matching,
                            spell,
                            physical,
                            actor,
                            aura_or_equipment,
                            entry_type,
                        )
                )
            };
        if evaluated.objects.values().any(|permanent| {
            permanent.zone == ObjectZone::Battlefield
                && permanent.has_battlefield_rule(|ability| {
                    rule_grants(
                        ability,
                        permanent.controller == Some(actor),
                        permanent.entry_creature_type,
                    )
                })
        }) {
            return true;
        }
        if evaluated
            .emblem_rules
            .iter()
            .any(|(seat, ability)| rule_grants(ability, *seat == actor, None))
        {
            return true;
        }
        self.temporary_rules.iter().any(|active| {
            self.next_spell_keyword_for_cast(active, evaluated, spell, actor) == Some(keyword)
                || self
                    .active_temporary_static(active, evaluated)
                    .is_some_and(|ability| rule_grants(ability, active.controller == actor, None))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn prohibit_casting_matches(
        &self,
        filter: crate::program::SpellFilter,
        spell: &crate::characteristics::ObjectCharacteristics,
        physical: &GameObject,
        actor: SeatId,
        aura_or_equipment: bool,
        source_object: Option<ObjectId>,
        source_entry_type: Option<spellwake_contracts::creature_types::CreatureTypeId>,
    ) -> bool {
        let zone_ok = match filter.cast_zone {
            crate::program::SpellCastZoneFilter::Any => true,
            crate::program::SpellCastZoneFilter::OwnLibrary => {
                physical.zone() == ObjectZone::Library(actor)
            }
            crate::program::SpellCastZoneFilter::Graveyard => {
                matches!(physical.zone(), ObjectZone::Graveyard(_))
            }
            crate::program::SpellCastZoneFilter::Exile => physical.zone() == ObjectZone::Exile,
            crate::program::SpellCastZoneFilter::Hand => {
                matches!(physical.zone(), ObjectZone::Hand(_))
            }
            crate::program::SpellCastZoneFilter::NotOwnersHand => {
                physical.zone() != ObjectZone::Hand(physical.owner())
            }
        };
        if !zone_ok {
            return false;
        }
        let is_commander = spell.commander_owner.is_some()
            || self
                .window
                .cards()
                .get(spell.object)
                .and_then(GameObject::card_id)
                .is_some_and(|card| self.commanders.contains_key(&card));
        let clause_ok = |c: crate::program::ObjectClause| -> bool {
            let single = crate::program::SpellFilter {
                object: crate::program::ObjectFilter::Clause(c),
                ..filter
            };
            if !single.matches_characteristics(spell, aura_or_equipment, source_entry_type) {
                return false;
            }
            if !c.required_supertypes.matches_all(&spell.current_supertypes)
                || !c
                    .forbidden_supertypes
                    .matches_none(&spell.current_supertypes)
            {
                return false;
            }
            let mv_ok = match c.mana_value {
                crate::program::ManaValueFilter::Any => true,
                crate::program::ManaValueFilter::AtMost(max) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() <= i64::from(max).into()),
                crate::program::ManaValueFilter::AtLeast(min) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() >= i64::from(min).into()),
                crate::program::ManaValueFilter::Equal(exact) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() == i64::from(exact).into()),
                crate::program::ManaValueFilter::Even => {
                    self.copiable_program(spell.object).is_some_and(|p| {
                        let v = p.mana_value();
                        v.nonnegative_half_rounded_up() == v.nonnegative_half_rounded_down()
                    })
                }
                crate::program::ManaValueFilter::Odd => {
                    self.copiable_program(spell.object).is_some_and(|p| {
                        let v = p.mana_value();
                        v.nonnegative_half_rounded_up() != v.nonnegative_half_rounded_down()
                    })
                }
                _ => true,
            };
            if !mv_ok {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::COMMANDER_ONLY)
                && !is_commander
            {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::NOT_COMMANDER)
                && is_commander
            {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR)
                && !source_object.is_some_and(|src| {
                    self.characteristics
                        .entry_color_choices
                        .get(&src)
                        .copied()
                        .or_else(|| {
                            self.entry_color_choices.get(&src).copied().and_then(
                                |mana| match mana {
                                    ManaType::White => Some(crate::characteristics::Color::White),
                                    ManaType::Blue => Some(crate::characteristics::Color::Blue),
                                    ManaType::Black => Some(crate::characteristics::Color::Black),
                                    ManaType::Red => Some(crate::characteristics::Color::Red),
                                    ManaType::Green => Some(crate::characteristics::Color::Green),
                                    ManaType::Colourless => None,
                                },
                            )
                        })
                        .is_some_and(|color| spell.current_colors.contains(&color))
                })
            {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::MATCHES_CHOSEN_NAME)
                && !self.object_matches_chosen_name(&spell.current_names, source_object)
            {
                return false;
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::MATCHES_CHOSEN_CARD_TYPE)
                && !self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.results.chosen_card_type)
                    .or_else(|| source_object.and_then(|src| self.entry_chosen_card_type(src)))
                    .is_some_and(|kind| spell.has_type(kind))
            {
                return false;
            }
            true
        };
        match filter.object {
            crate::program::ObjectFilter::Clause(c) => clause_ok(c),
            crate::program::ObjectFilter::Either(a, b) => clause_ok(a) || clause_ok(b),
        }
    }

    fn spell_filter_matches_cast(
        &self,
        filter: crate::program::SpellFilter,
        spell: &crate::characteristics::ObjectCharacteristics,
        physical: &GameObject,
        actor: SeatId,
        aura_or_equipment: bool,
        source_entry_type: Option<spellwake_contracts::creature_types::CreatureTypeId>,
    ) -> bool {
        let zone_ok = match filter.cast_zone {
            crate::program::SpellCastZoneFilter::Any => true,
            crate::program::SpellCastZoneFilter::OwnLibrary => {
                physical.zone() == ObjectZone::Library(actor)
            }
            crate::program::SpellCastZoneFilter::Graveyard => {
                matches!(physical.zone(), ObjectZone::Graveyard(_))
            }
            crate::program::SpellCastZoneFilter::Exile => physical.zone() == ObjectZone::Exile,
            crate::program::SpellCastZoneFilter::Hand => {
                matches!(physical.zone(), ObjectZone::Hand(_))
            }
            crate::program::SpellCastZoneFilter::NotOwnersHand => {
                physical.zone() != ObjectZone::Hand(physical.owner())
            }
        };
        if !zone_ok {
            return false;
        }
        if filter.special == crate::program::SpellSpecialFilter::AboveExperience
            && !self
                .copiable_program(spell.object)
                .is_some_and(|p| p.mana_value() > self.experience_counters(actor))
        {
            return false;
        }
        let extra_clause_ok = |c: crate::program::ObjectClause| -> bool {
            if !c.required_supertypes.matches_all(&spell.current_supertypes)
                || !c
                    .forbidden_supertypes
                    .matches_none(&spell.current_supertypes)
            {
                return false;
            }
            match c.mana_value {
                crate::program::ManaValueFilter::Any => true,
                crate::program::ManaValueFilter::AtMost(max) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() <= i64::from(max).into()),
                crate::program::ManaValueFilter::AtLeast(min) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() >= i64::from(min).into()),
                crate::program::ManaValueFilter::Equal(exact) => self
                    .copiable_program(spell.object)
                    .is_some_and(|p| p.mana_value() == i64::from(exact).into()),
                crate::program::ManaValueFilter::Even => {
                    self.copiable_program(spell.object).is_some_and(|p| {
                        let v = p.mana_value();
                        v.nonnegative_half_rounded_up() == v.nonnegative_half_rounded_down()
                    })
                }
                crate::program::ManaValueFilter::Odd => {
                    self.copiable_program(spell.object).is_some_and(|p| {
                        let v = p.mana_value();
                        v.nonnegative_half_rounded_up() != v.nonnegative_half_rounded_down()
                    })
                }
                _ => true,
            }
        };
        let clause_ok = |c: crate::program::ObjectClause| -> bool {
            let single = crate::program::SpellFilter {
                object: crate::program::ObjectFilter::Clause(c),
                ..filter
            };
            single.matches_characteristics(spell, aura_or_equipment, source_entry_type)
                && extra_clause_ok(c)
        };
        let object_ok = match filter.object {
            crate::program::ObjectFilter::Clause(c) => clause_ok(c),
            crate::program::ObjectFilter::Either(a, b) => clause_ok(a) || clause_ok(b),
        };
        if !object_ok {
            return false;
        }
        if let Some(ordinal) = filter.ordinal_this_turn
            && self
                .prior_matching_spells_cast_this_turn(actor, filter)
                .saturating_add(1)
                != ordinal.get()
        {
            return false;
        }
        true
    }

    fn prior_matching_spells_cast_this_turn(
        &self,
        actor: SeatId,
        filter: crate::program::SpellFilter,
    ) -> u32 {
        let total = u32::from(
            self.characteristics
                .spells_cast_this_turn
                .get(&actor)
                .copied()
                .unwrap_or(0),
        );
        if total == 0 {
            return 0;
        }
        if let Some(records) = self.characteristics.cast_spells_this_turn.get(&actor)
            && !records.is_empty()
        {
            let count = records
                .iter()
                .filter(|rec| Self::cast_turn_record_matches(filter, rec))
                .count();
            return u32::try_from(count).unwrap_or(u32::MAX);
        }
        let stripped = crate::program::SpellFilter {
            ordinal_this_turn: None,
            ..filter
        };
        if stripped == crate::program::SpellFilter::any() {
            return total;
        }
        if stripped == crate::program::SpellFilter::noncreature() {
            return u32::from(
                self.characteristics
                    .noncreature_spells_cast_this_turn
                    .get(&actor)
                    .copied()
                    .unwrap_or(0),
            );
        }
        if stripped == crate::program::SpellFilter::any().from_hand() {
            return u32::from(
                self.characteristics
                    .spells_cast_from_hand_this_turn
                    .get(&actor)
                    .copied()
                    .unwrap_or(0),
            );
        }
        if stripped.cast_zone == crate::program::SpellCastZoneFilter::Any
            && stripped.special == crate::program::SpellSpecialFilter::None
            && let crate::program::ObjectFilter::Clause(c) = stripped.object
            && !c.any_types.is_empty()
            && c.required_types.is_empty()
            && c.forbidden_types.is_empty()
            && c.required_supertypes.is_empty()
            && c.forbidden_supertypes.is_empty()
            && c.subtypes == crate::program::SubtypeFilter::Any
            && c.color == crate::program::ObjectColorFilter::Any
            && c.owner == crate::program::ControllerFilter::Any
            && c.mana_value == crate::program::ManaValueFilter::Any
            && c.power == crate::program::PowerFilter::Any
            && c.flags == crate::program::ObjectFilterFlags::EMPTY
        {
            let by_type = self.characteristics.cast_spell_types_this_turn.get(&actor);
            let mut sum: u32 = 0;
            for kind in [
                CardType::Battle,
                CardType::Planeswalker,
                CardType::Land,
                CardType::Artifact,
                CardType::Creature,
                CardType::Enchantment,
                CardType::Kindred,
                CardType::Instant,
                CardType::Sorcery,
            ] {
                if c.any_types.contains(kind)
                    && let Some(n) = by_type.and_then(|map| map.get(&kind))
                {
                    sum = sum.saturating_add(u32::from(*n));
                }
            }
            return sum.min(total);
        }
        0
    }

    fn cast_turn_record_matches(
        filter: crate::program::SpellFilter,
        rec: &crate::characteristics::CastSpellTurnRecord,
    ) -> bool {
        let zone_ok = match filter.cast_zone {
            crate::program::SpellCastZoneFilter::Any => true,
            crate::program::SpellCastZoneFilter::Hand => rec.from_hand,
            crate::program::SpellCastZoneFilter::Graveyard => rec.from_graveyard,
            crate::program::SpellCastZoneFilter::Exile => rec.from_exile,
            crate::program::SpellCastZoneFilter::NotOwnersHand => !rec.from_hand,
            crate::program::SpellCastZoneFilter::OwnLibrary => {
                !rec.from_hand && !rec.from_graveyard && !rec.from_exile
            }
        };
        if !zone_ok {
            return false;
        }
        let mut record_types = BTreeSet::new();
        for kind in [
            CardType::Battle,
            CardType::Planeswalker,
            CardType::Land,
            CardType::Artifact,
            CardType::Creature,
            CardType::Enchantment,
            CardType::Kindred,
            CardType::Instant,
            CardType::Sorcery,
        ] {
            if rec.types.contains(kind) {
                record_types.insert(kind);
            }
        }
        let mut record_colors = BTreeSet::new();
        for color in [
            crate::characteristics::Color::White,
            crate::characteristics::Color::Blue,
            crate::characteristics::Color::Black,
            crate::characteristics::Color::Red,
            crate::characteristics::Color::Green,
        ] {
            if rec.colors.contains(color) {
                record_colors.insert(color);
            }
        }
        let mut record_supertypes = BTreeSet::new();
        for st in [
            crate::program::Supertype::Basic,
            crate::program::Supertype::Legendary,
            crate::program::Supertype::Ongoing,
            crate::program::Supertype::Snow,
            crate::program::Supertype::World,
        ] {
            let bit = crate::program::SupertypeMask::from_supertype(st);
            if rec.supertypes.union(bit) == rec.supertypes {
                record_supertypes.insert(st);
            }
        }
        let match_clause = |c: crate::program::ObjectClause| -> bool {
            if c.exact_name.is_some()
                || c.required_counter_kind.is_some()
                || c.forbidden_counter_kind.is_some()
            {
                return false;
            }
            if !c.any_types.matches_any(&record_types)
                || !c.required_types.matches_all(&record_types)
                || !c.forbidden_types.matches_none(&record_types)
                || !c.required_supertypes.matches_all(&record_supertypes)
                || !c.forbidden_supertypes.matches_none(&record_supertypes)
                || !c.color.matches(&record_colors)
            {
                return false;
            }
            match c.subtypes {
                crate::program::SubtypeFilter::Any => {}
                crate::program::SubtypeFilter::Has(kind) => {
                    if !rec.subtypes.contains(&kind) {
                        return false;
                    }
                }
                crate::program::SubtypeFilter::Lacks(kind) => {
                    if rec.subtypes.contains(&kind) {
                        return false;
                    }
                }
                crate::program::SubtypeFilter::AnyOf(kinds) => {
                    if !kinds.iter().any(|k| rec.subtypes.contains(k)) {
                        return false;
                    }
                }
                crate::program::SubtypeFilter::NoneOf(kinds)
                    if kinds.iter().any(|k| rec.subtypes.contains(k)) =>
                {
                    return false;
                }
                crate::program::SubtypeFilter::AnyOfList(list) => {
                    if !list.iter().any(|item| match item {
                        crate::program::SingleSubtype::Creature(k) => rec.subtypes.contains(k),
                        _ => false,
                    }) {
                        return false;
                    }
                }
                crate::program::SubtypeFilter::AllOfList(list) => {
                    if !list.iter().all(|item| match item {
                        crate::program::SingleSubtype::Creature(k) => rec.subtypes.contains(k),
                        _ => false,
                    }) {
                        return false;
                    }
                }
                crate::program::SubtypeFilter::NoneOfList(list)
                    if !list.iter().all(|item| {
                        matches!(item, crate::program::SingleSubtype::Creature(k) if !rec.subtypes.contains(k))
                    }) =>
                {
                    return false;
                }
                _ => {}
            }
            if c.flags
                .contains(crate::program::ObjectFilterFlags::HISTORIC)
                && !(rec.types.contains(CardType::Artifact)
                    || record_supertypes.contains(&crate::program::Supertype::Legendary))
            {
                return false;
            }
            true
        };
        match filter.object {
            crate::program::ObjectFilter::Clause(c) => match_clause(c),
            crate::program::ObjectFilter::Either(a, b) => match_clause(a) || match_clause(b),
        }
    }
}
