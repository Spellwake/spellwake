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
        let definition = if let Some(disturb) = disturb {
            Some(&disturb.back)
        } else if siege {
            Some(
                printed
                    .and_then(|front| front.siege_back.as_deref())
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
        let proposed_cards = if siege || disturb.is_some() {
            let mut cards = self.window.cards().clone();
            cards
                .select_reverse_spell(spell)
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
            })?
        } else {
            self.evaluated_characteristics()?
        };
        let self_graveyard_permission = from_graveyard
            && current
                .get(spell)
                .and_then(|card| card.printed_text.graveyard_self_cast)
                .is_some_and(|permission| match permission {
                    crate::program::GraveyardSelfCast::Unconditional => true,
                    crate::program::GraveyardSelfCast::WhileControllingSubtype(kind) => {
                        current.objects.values().any(|permanent| {
                            permanent.zone == ObjectZone::Battlefield
                                && permanent.controller == Some(actor)
                                && permanent.current_creature_subtypes.contains(&kind)
                        })
                    }
                });
        let graveyard_permission_candidates = if from_graveyard
            && !cast_graveyard_target
            && !via_escape
            && !mayhem
            && !via_flashback
            && graveyard_discard.is_none()
            && turn.is_some()
            && self.window.active_player() == actor
        {
            if current.get(spell).is_some_and(|card| card.is_creature()) {
                current
                    .objects
                    .values()
                    .filter_map(|permanent| match &permanent.printed_text.continuous {
                        Some(crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardCreatureCast,
                        }) if permanent.zone == ObjectZone::Battlefield
                            && permanent.controller == Some(actor)
                            && permanent.abilities.continuous == Some(*id)
                            && !self.source_cast_uses.contains(&(permanent.object, *id)) =>
                        {
                            Some((permanent.object, *id))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            }
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
                    .find(|(source, _)| *source == selected)
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
        let from_free_exile =
            turn.is_some() && self.may_cast_free_face(spell, actor, Some(program));
        if from_free_exile && (payment.use_alternative || payment.alternative_cost_source.is_some())
        {
            return Err(EffectError::InvalidCastResource);
        }
        let from_exile_permission = turn.is_some()
            && (self.play_exile_permission(spell, actor, !payment.adventure)
                || self.may_cast_from_granted_exile(spell, actor));
        if source.zone() != ObjectZone::Hand(actor)
            && resolving_parent.is_none()
            && !from_plot
            && !from_free_exile
            && !from_exile_permission
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
            && current.objects.values().any(|object| {
                object.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticContinuousAbility::Restriction {
                            rule: crate::characteristics::StaticRestriction::OneSpellPerTurn,
                            ..
                        }
                    )
                }) || (object.attached_to == Some(TargetRef::Player(actor))
                    && object.has_battlefield_rule(|rule| {
                        matches!(rule, crate::characteristics::StaticContinuousAbility::Restriction { rule: crate::characteristics::StaticRestriction::EnchantedPlayerCastLimit, .. })
                    }))
            })
        {
            return Err(EffectError::SpellCastLimitReached);
        }
        let current_spell = current.get(spell).ok_or(EffectError::InconsistentProgram)?;
        self.check_chosen_name_cast_restriction(current_spell, &current)?;
        let mut alternative_life = false;
        let external_alternative = if let Some(source_id) = payment.alternative_cost_source {
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
                .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(actor))
                .ok_or(EffectError::InvalidCastResource)?;
            let ability=permanent.printed_text.continuous.iter().chain(&permanent.printed_text.additional_battlefield_abilities)
                .find(|ability| {
                    let eligible = match ability {
                        crate::characteristics::StaticContinuousAbility::CostModifier { modifier: crate::characteristics::SpellCostModifier::FreeColorlessHandSpellOnce, .. } => source.zone()==ObjectZone::Hand(actor) && current_spell.current_colors.is_empty(),
                        crate::characteristics::StaticContinuousAbility::CostModifier { modifier: crate::characteristics::SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce, .. } => actor==self.window.active_player() && current_spell.has_type(CardType::Enchantment),
                        crate::characteristics::StaticContinuousAbility::CostModifier { modifier: crate::characteristics::SpellCostModifier::FreeSubtypeCreatureSpells(subtype), .. } => {
                            current_spell.is_creature() && current_spell.current_creature_subtypes.contains(subtype)
                        }
                        _=>false,
                    };
                    let unlimited = matches!(ability, crate::characteristics::StaticContinuousAbility::CostModifier { modifier: crate::characteristics::SpellCostModifier::FreeSubtypeCreatureSpells(_), .. });
                    eligible && (permanent.abilities.continuous==Some(ability.id()) || permanent.abilities.additional_battlefield_abilities.contains(&ability.id()))
                        && (unlimited || !self.source_cast_uses.contains(&(source_id,ability.id())))
                }).ok_or(EffectError::InvalidCastResource)?;
            alternative_life = matches!(
                ability,
                crate::characteristics::StaticContinuousAbility::CostModifier {
                    modifier:
                        crate::characteristics::SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce,
                    ..
                }
            );
            Some((
                source_id,
                ability.id(),
                !matches!(
                    ability,
                    crate::characteristics::StaticContinuousAbility::CostModifier {
                        modifier:
                            crate::characteristics::SpellCostModifier::FreeSubtypeCreatureSpells(_),
                        ..
                    }
                ),
            ))
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
        let flash = sneak_defender.is_some() || current_spell.has_keyword(KeywordAbility::Flash)
            || self.flash_through_cleanup.contains(&actor)
            || current.objects.values().any(|source| {
                source.zone == ObjectZone::Battlefield && matches!(&source.printed_text.continuous,
                    Some(crate::characteristics::StaticContinuousAbility::CastPermission {
                        id,
                        permission: crate::characteristics::CastPermissionRule::Flash { any_player, matching },
                    })
                        if source.abilities.continuous == Some(*id)
                            && (*any_player || source.controller == Some(actor))
                            && matching.matches_characteristics(current_spell, false, source.entry_creature_type))
            });
        let spliced = self.prepare_splice(&payment.splice, actor, current_spell, &current)?;
        let mut mode_targets = Vec::new();
        let mut mode_clauses = Vec::new();
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
                    || external_alternative.is_some())
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
                    &protection::ProtectionSource::from(current_spell),
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
                    &protection::ProtectionSource::from(current_spell),
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
                    &protection::ProtectionSource::from(current_spell),
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
        let mut cost = if external_alternative.is_some() {
            ManaCost::free()
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
            };
            if via_flashback || !eligible {
                return Err(EffectError::InvalidCastResource);
            }
            alternative.mana().clone()
        } else {
            cost.clone()
        };
        let pays_printed_cost = disturb.is_none()
            && !from_free_exile
            && external_alternative.is_none()
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
        let chosen_x = if priced_x_symbols == 0
            && (!cast_for_miracle || program.x_mana_symbols == 0)
            && !program.graveyard_exile_x_cost
            && !variable_kicker
            && !life_price.is_some_and(ValueExpr::is_chosen_x)
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
        let aura_or_equipment = (enchant.is_some()
            && current_spell.has_type(CardType::Enchantment))
            || (current_spell.has_type(CardType::Artifact)
                && program.permanent().and_then(|p| p.attachment)
                    == Some(crate::attachment::AttachmentKind::Equipment));
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
                    _ => {}
                }
            }
        }
        if let Some(discount) = emerge_discount {
            cost.generic = cost.generic.minus(&discount).max(0.into());
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
                crate::program::CastReduction::Undaunted => RulesInteger::from_usize(
                    self.window
                        .participants()
                        .iter()
                        .filter(|&&seat| seat != actor)
                        .count(),
                ),
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
                    _ => continue,
                };
                cost.generic = cost.generic.minus(&discount).max(0.into());
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
        if let Some(any_type) = self.flexible_exile_mana(spell, actor) {
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
        let mut window =
            self.window
                .admit_card_spell(self.window.revision(), actor, spell, resolving_parent)?;
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
        if siege || disturb.is_some() {
            let Some(StackKind::CardSpell(stacked)) =
                window.entries().last().map(|entry| entry.kind())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            window.select_siege_back(*stacked)?;
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
            .into_iter()
            .chain(
                external_alternative
                    .and_then(|(source, ability, once)| once.then_some((source, ability))),
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
                let mut events = vec![zone_occurrence_at(
                    previous,
                    current,
                    &movement,
                    crate::occurrence::ZoneReason::CastAnnouncement,
                    controllers,
                )?];
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
}
