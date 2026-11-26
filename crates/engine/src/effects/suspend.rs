//! Optional casts during suspend, rebound and madness trigger resolution.
use super::*;

/// Rule granting an optional cast during a trigger's resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExileCastAbility {
    /// The controller of a defeated Siege may cast its reverse face for free.
    Siege,
    Suspend,
    Rebound,
    Madness,
    /// An exact first draw that was revealed and remained in its owner’s hand.
    Miracle,
    Cascade,
    /// A normal-price cast offered by a resolving exile-top effect.
    Impulse,
    /// Cast a card found in any player’s library without paying its mana cost.
    FoundSpell,
    /// Cast a spell from hand matching the counter target's card types.
    MatchingHand,
    /// A designated commander cast from its owner's command zone during resolution.
    Commander,
    /// Cast the revealed top card directly from the library.
    RevealedLibrary,
    /// Resolving permission to cast the targeted instant from its graveyard.
    TargetedGraveyard,
}
/// One printed mode and its legal current target domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExileCastMode {
    pub multiple: Option<(usize, usize)>,
    pub index: u8,
    /// None means no target; Some(empty) means no legal target at present.
    pub targets: Option<Vec<TargetRef>>,
}
/// Mode-selection limits retained with a resolving cast offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExileCastModes {
    pub entwine_index: Option<u16>,
    pub minimum: u8,
    pub maximum: u8,
    pub allow_repeat: bool,
    pub modes: Vec<ExileCastMode>,
}
/// A nonmodal multiple-permanent target clause offered during an impulse cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExilePermanentTargets {
    pub minimum: usize,
    pub maximum: usize,
    pub targets: Vec<TargetRef>,
    pub division_total: Option<u16>,
}
/// An exact exiled card that its controller may cast while a trigger resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExileCastOffer {
    /// Whether the normal spell meets this permission and has legal targets.
    pub normal_available: bool,
    /// The independently evaluated Adventure spell, when castable.
    pub adventure: Option<Box<ExileCastOffer>>,
    /// Independently evaluated right half of a split card, if legal from this zone.
    pub split_right: Option<Box<ExileCastOffer>>,
    /// The ability granting this permission.
    pub ability: ExileCastAbility,
    /// The current incarnation in exile.
    pub spell: ObjectId,
    /// Printed madness mana cost when the card is offered for that alternative.
    pub madness_price: Option<ManaCost>,
    /// Printed base cost for an impulse cast, before normal adjustments.
    pub normal_price: Option<ManaCost>,
    /// Ordered alternative symbols included in the normal price.
    pub hybrid_symbols: Vec<crate::mana::HybridSymbol>,
    /// A multiple-permanent target clause, if present.
    pub permanent_targets: Option<ExilePermanentTargets>,
    /// Ordered legal target domains for independently targeted printed clauses.
    pub fixed_targets: Option<Vec<Vec<TargetRef>>>,
    /// Mode choices, when this is a modal impulse cast.
    pub modes: Option<ExileCastModes>,
    /// Whether the printed mana cost requires an announced X.
    pub allows_x: bool,
    /// The current represented face offers an alternative cost.
    pub allows_alternative: bool,
    /// The alternative cost replaces the normal targeted body with a targetless body.
    pub alternative_targetless: bool,
    /// Printed alternative mana price, before reductions, when targetless.
    pub alternative_price: Option<ManaCost>,
    /// None for a spell without targets. A targeted spell offers only legal current choices.
    pub targets: Option<Vec<TargetRef>>,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct ResolvingCast<'a> {
    pub(super) parent: StackEntryId,
    pub(super) kind: ExileCastAbility,
    pub(super) mana_abilities: &'a [crate::mana::CastManaActivation],
}
impl EffectWindow {
    pub(super) fn activate_cast_mana(
        &mut self,
        cast: ResolvingCast<'_>,
        actor: SeatId,
        append: &mut RulesJournal,
    ) -> Result<Option<(usize, DamageJournal)>, EffectError> {
        for (index, activation) in cast.mana_abilities.iter().enumerate() {
            let (mut next, result, journal) = self
                .begin_mana_with_parent(
                    self.revision,
                    actor,
                    activation.source,
                    activation.ability,
                    Some(ManaParent::Casting(cast.parent)),
                    ManaActivationChoices {
                        payment: activation.payment.as_ref(),
                        bundle: activation.bundle.as_ref(),
                        cost_objects: Some(&activation.cost_objects),
                        recipient: activation.recipient,
                        restricted: Some(&activation.restricted_mana),
                    },
                )?
                .into_parts();
            append.append(journal);
            match (result, activation.color) {
                (ManaActivationResult::PendingRule { .. }, _) => {
                    *self = next;
                    return Ok(Some((index + 1, DamageJournal::default())));
                }
                (ManaActivationResult::Complete { .. }, None) => {}
                (ManaActivationResult::Pending { frame }, None)
                    if frame.chooser() != actor
                        || frame.source() != activation.source
                        || frame.ability() != activation.ability =>
                {
                    *self = next;
                    return Ok(Some((index + 1, DamageJournal::default())));
                }
                (ManaActivationResult::Pending { frame }, Some(color)) => {
                    if frame.source() != activation.source || frame.ability() != activation.ability
                    {
                        return Err(EffectError::WrongManaColorFrame);
                    }
                    let has_damage = matches!(&next.pending_work, Some(PendingWork::ManaColor(work)) if work.controller_damage.is_some());
                    if has_damage {
                        let (chosen, _, journal, produced) =
                            next.stage_mana_color_damage(next.revision, &frame, actor, color)?;
                        next = chosen;
                        append.append(produced);
                        *self = next;
                        return Ok(Some((index + 1, journal)));
                    }
                    let (chosen, _, journal) = next
                        .resume_mana_color(next.revision, &frame, actor, color)?
                        .into_parts();
                    next = chosen;
                    append.append(journal);
                    if next.pending_mana_color().is_some() {
                        *self = next;
                        return Ok(Some((index + 1, DamageJournal::default())));
                    }
                }
                _ => return Err(EffectError::WrongManaColorFrame),
            }
            *self = next;
        }
        Ok(None)
    }

    pub(super) fn finish_declined_madness(
        &mut self,
        cursor: &ResolutionCursor,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let StackKind::TriggeredAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let object = source.id();
        if !self
            .window
            .cards()
            .get(object)
            .is_some_and(|card| card.zone() == ObjectZone::Exile)
        {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let (window, exit) = self.window.resolution_move_with_exit(
            self.window.revision(),
            object,
            ObjectDestination::Graveyard,
        )?;
        self.window = window;
        self.emit_from(
            crate::occurrence::EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                Ok(vec![occurrence_flow::exit_occurrence_at(
                    previous,
                    current,
                    exit,
                    crate::occurrence::ZoneReason::MadnessDeclined,
                    controllers,
                )?])
            },
        )
    }

    pub(super) fn found_spell_selection(
        &self,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mut domain = Vec::new();
        for &spell in &cursor.found_spells {
            if self
                .exiled_cast_offer(&Instruction::CastFoundSpell(spell))?
                .is_some()
            {
                domain.push(spell);
            }
        }
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::found_spell(
                self.revision,
                entry.id(),
                cursor.instruction,
                entry.controller(),
                domain,
            )
        }))
    }
    pub(super) fn resume_found_spell(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.current_instruction()? != &Instruction::ChooseFoundSpell
            || self.found_spell_selection()?.as_ref() != Some(frame)
        {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(selected) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if selected.len() > 1
            || selected
                .first()
                .is_some_and(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let cursor = next
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if let Some(&spell) = selected.first() {
            cursor.found_spells.retain(|id| *id != spell);
            let Some(StackPayload::Instructions(payload)) = next.payloads.get_mut(&frame.entry())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[frame.instruction()] = Instruction::CastFoundSpell(spell);
        } else {
            cursor.found_spells.clear();
            next.advance_instruction()?;
        }
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn exiled_cast_offer(
        &self,
        instruction: &Instruction,
    ) -> Result<Option<ExileCastOffer>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let caster = self.resolving_cast_actor(instruction)?;
        let source = match entry.kind() {
            StackKind::TriggeredAbility { source } => Some(source),
            _ => None,
        };
        let (ability, spell) = match instruction {
            Instruction::CastDefeatedSiege => {
                let Some(id) = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.impulse.exiled)
                else {
                    return Ok(None);
                };
                (ExileCastAbility::Siege, id)
            }
            Instruction::CastTargetGraveyardInstant => {
                let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                (ExileCastAbility::TargetedGraveyard, object)
            }
            Instruction::RevealSharedTypeCreatureCastOrBottom => {
                let Some(&id) = self.window.cards().library(caster).first() else {
                    return Ok(None);
                };
                let evaluated = self.evaluated_characteristics()?;
                let card = evaluated.get(id).ok_or(EffectError::InconsistentProgram)?;
                if !card.is_creature()
                    || !evaluated.objects.values().any(|other| {
                        other.zone == ObjectZone::Battlefield
                            && other.controller == Some(caster)
                            && other.is_creature()
                            && !other
                                .current_creature_subtypes
                                .is_disjoint(&card.current_creature_subtypes)
                    })
                {
                    return Ok(None);
                }
                (ExileCastAbility::RevealedLibrary, id)
            }
            Instruction::RevealOddCastOrDraw => {
                let Some(&id) = self.window.cards().library(caster).first() else {
                    return Ok(None);
                };
                let value = self
                    .copiable_program(id)
                    .ok_or(EffectError::InconsistentProgram)?
                    .mana_value();
                if value.nonnegative_half_rounded_down() == value.nonnegative_half_rounded_up() {
                    return Ok(None);
                }
                (ExileCastAbility::RevealedLibrary, id)
            }
            Instruction::ImpulseCastOrDamage(_) => {
                let Some(id) = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.impulse.exiled)
                else {
                    return Ok(None);
                };
                (ExileCastAbility::Impulse, id)
            }
            Instruction::Cascade => {
                let Some(id) = self
                    .cascade_batch(entry.id())
                    .and_then(|batch| batch.candidate)
                else {
                    return Ok(None);
                };
                (ExileCastAbility::Cascade, id)
            }
            Instruction::CastFoundSpell(object) => {
                let card = self.window.cards().get(*object);
                let ability =
                    if card.is_some_and(|card| self.commander_in_command(card, caster).is_some()) {
                        ExileCastAbility::Commander
                    } else if card.is_some_and(|card| card.zone() == ObjectZone::Hand(caster)) {
                        ExileCastAbility::MatchingHand
                    } else {
                        ExileCastAbility::FoundSpell
                    };
                (ability, *object)
            }
            Instruction::MiracleCast(object) => (ExileCastAbility::Miracle, *object),
            Instruction::ReboundCast(object) => (ExileCastAbility::Rebound, *object),
            Instruction::MadnessCast => (
                ExileCastAbility::Madness,
                source.ok_or(EffectError::InconsistentProgram)?.id(),
            ),
            Instruction::ResolveCounterKeyword(
                crate::program::CounterKeyword::SuspendLastCounter,
            ) => (
                ExileCastAbility::Suspend,
                source.ok_or(EffectError::InconsistentProgram)?.id(),
            ),
            _ => return Err(EffectError::InconsistentProgram),
        };
        let Some(card) = self.window.cards().get(spell).filter(|c| {
            if ability == ExileCastAbility::Commander {
                self.commander_in_command(c, caster).is_some()
            } else if matches!(
                ability,
                ExileCastAbility::Miracle | ExileCastAbility::MatchingHand
            ) {
                c.zone() == ObjectZone::Hand(caster)
            } else if ability == ExileCastAbility::TargetedGraveyard {
                c.zone() == ObjectZone::Graveyard(caster)
            } else if ability == ExileCastAbility::RevealedLibrary {
                c.zone() == ObjectZone::Library(caster)
                    && self.window.cards().library(caster).first() == Some(&spell)
            } else {
                c.zone() == ObjectZone::Exile
            }
        }) else {
            return Ok(None);
        };
        if ability == ExileCastAbility::Siege {
            if self
                .copiable_program(spell)
                .is_none_or(|front| front.siege_back.is_none())
            {
                return Ok(None);
            }
            let mut prospective = self.clone();
            prospective.window.select_siege_back(spell)?;
            let face = prospective
                .window
                .cards()
                .get(spell)
                .ok_or(EffectError::InconsistentProgram)?;
            return prospective.cast_offer_for_face(ability, caster, face, entry.id());
        }
        if let Some(split) = self
            .copiable_program(spell)
            .and_then(|program| program.split.as_ref())
        {
            use crate::program::SplitHalf;
            let for_half = |half| -> Result<_, EffectError> {
                let mut prospective = self.clone();
                prospective.window.select_split_half(spell, half)?;
                let face = prospective
                    .window
                    .cards()
                    .get(spell)
                    .ok_or(EffectError::InconsistentProgram)?;
                prospective.cast_offer_for_face(ability, caster, face, entry.id())
            };
            let left = for_half(SplitHalf::Left)?;
            let right = if split.aftermath && !matches!(card.zone(), ObjectZone::Graveyard(_)) {
                None
            } else {
                for_half(SplitHalf::Right)?
            };
            return Ok(match (left, right) {
                (left, None) => left,
                (left, Some(right)) => {
                    let mut combined = left.unwrap_or_else(|| ExileCastOffer {
                        normal_available: false,
                        ..right.clone()
                    });
                    combined.split_right = Some(Box::new(right));
                    Some(combined)
                }
            });
        }
        let normal = self.cast_offer_for_face(ability, caster, card, entry.id())?;
        if self
            .copiable_program(spell)
            .is_none_or(|program| program.adventure.is_none())
        {
            return Ok(normal);
        }
        // A proposed Adventurer spell has only the characteristics of its chosen face.
        let mut prospective = self.clone();
        prospective.window.preview_adventure(spell)?;
        let alternate = prospective
            .window
            .cards()
            .get(spell)
            .ok_or(EffectError::InconsistentProgram)?;
        let adventure = prospective.cast_offer_for_face(ability, caster, alternate, entry.id())?;
        match (normal, adventure) {
            (normal, None) => Ok(normal),
            (normal, Some(adventure)) => {
                let mut combined = normal.unwrap_or_else(|| ExileCastOffer {
                    normal_available: false,
                    ..adventure.clone()
                });
                combined.adventure = Some(Box::new(adventure));
                Ok(Some(combined))
            }
        }
    }

    fn cast_offer_for_face(
        &self,
        ability: ExileCastAbility,
        caster: SeatId,
        card: &GameObject,
        entry: StackEntryId,
    ) -> Result<Option<ExileCastOffer>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let prospective = evaluated
            .get(card.id())
            .ok_or(EffectError::InconsistentProgram)?;
        match self
            .check_split_second(&evaluated)
            .and_then(|()| self.validate_legendary_spell_cast(card.id(), caster, &evaluated))
            .and_then(|()| self.check_chosen_name_cast_restriction(prospective, &evaluated))
        {
            Ok(()) => {}
            Err(
                EffectError::SplitSecond
                | EffectError::LegendarySpellPrerequisite
                | EffectError::ChosenNameProhibitsCasting,
            ) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let program = self
            .copiable_program(card.id())
            .ok_or(EffectError::InconsistentProgram)?;
        if ability == ExileCastAbility::MatchingHand
            && self.resolution_cursor().is_none_or(|cursor| {
                cursor
                    .results
                    .counter_target_types
                    .is_disjoint(&program.card_types)
            })
        {
            return Ok(None);
        }
        if ability == ExileCastAbility::Cascade
            && self
                .cascade_batch(entry)
                .is_none_or(|batch| !batch.allows_result(card.id(), &program.mana_value()))
        {
            return Ok(None);
        }
        if program.cast_during_own_combat
            && (caster != self.window.active_player()
                || self
                    .current_step
                    .is_none_or(|s| s.phase() != crate::turn::Phase::Combat))
        {
            return Ok(None);
        }
        if let Some(timing) = &program.cast_timing
            && !self.cast_timing_satisfied(timing, card.id(), caster, None)?
        {
            return Ok(None);
        }
        if matches!(&program.kind, ProgramKind::Spell(spell) if spell.body.instructions == [Instruction::ReturnChosenXGraveyardCreatures])
            && (!self
                .combat_rules
                .opponents(self.window.participants(), caster)?
                .contains(&self.window.active_player())
                || self
                    .current_step
                    .is_none_or(|step| step.phase() != crate::turn::Phase::Combat))
        {
            return Ok(None);
        }
        let full_announcement = matches!(
            ability,
            ExileCastAbility::Miracle
                | ExileCastAbility::Impulse
                | ExileCastAbility::RevealedLibrary
                | ExileCastAbility::Commander
                | ExileCastAbility::FoundSpell
                | ExileCastAbility::MatchingHand
                | ExileCastAbility::Siege
                | ExileCastAbility::TargetedGraveyard
        );
        if full_announcement && program.has_type(CardType::Land) {
            return Ok(None);
        }
        let normal_price = if ability == ExileCastAbility::Commander {
            let tax = card
                .card_id()
                .and_then(|id| self.commander_record(id))
                .ok_or(EffectError::InconsistentProgram)?
                .command_zone_casts()
                .times(&2.into());
            Some(ManaCost::generic(tax))
        } else if ability == ExileCastAbility::Miracle {
            Some(
                program
                    .miracle
                    .as_ref()
                    .ok_or(EffectError::InconsistentProgram)?
                    .mana
                    .clone(),
            )
        } else if matches!(
            ability,
            ExileCastAbility::Siege
                | ExileCastAbility::TargetedGraveyard
                | ExileCastAbility::FoundSpell
                | ExileCastAbility::MatchingHand
                | ExileCastAbility::RevealedLibrary
        ) {
            Some(ManaCost::free())
        } else if ability == ExileCastAbility::Impulse {
            match &program.kind {
                ProgramKind::Spell(spell) => Some(spell.cost.clone()),
                ProgramKind::Permanent(permanent) => permanent.casting_cost.clone(),
            }
        } else {
            None
        };
        if ability == ExileCastAbility::Impulse && normal_price.is_none() {
            return Ok(None);
        }
        let modes = if full_announcement {
            if let ProgramKind::Spell(spell) = &program.kind
                && let Some(spec) = &spell.modes
            {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(card.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                let mut modes = Vec::new();
                for (index, body) in std::iter::once(&spell.body)
                    .chain(spec.additional.iter())
                    .enumerate()
                {
                    let targets = if body.target == BodyTarget::None {
                        None
                    } else {
                        Some(self.trigger_target_domain(body, caster, Some(&source))?)
                    };
                    let multiple = body
                        .permanent_target_bounds()
                        .map(|(min, max)| (min, max.min(targets.as_ref().map_or(0, Vec::len))));
                    modes.push(ExileCastMode {
                        multiple,
                        index: u8::try_from(index).map_err(|_| EffectError::InconsistentProgram)?,
                        targets,
                    });
                }
                let entwine_index = program
                    .additional_cast_costs
                    .iter()
                    .position(|cost| matches!(cost, crate::program::AdditionalCastCost::Entwine(_)))
                    .map(u16::try_from)
                    .transpose()
                    .map_err(|_| EffectError::InconsistentProgram)?;
                Some(ExileCastModes {
                    entwine_index,
                    minimum: spec.minimum,
                    maximum: spec.maximum,
                    allow_repeat: spec.allow_repeat,
                    modes,
                })
            } else {
                None
            }
        } else {
            None
        };
        let permanent_targets = if ability != ExileCastAbility::Impulse
            && matches!(&program.kind, ProgramKind::Spell(spell) if spell.body.instructions == [Instruction::ReturnChosenXGraveyardCreatures] || spell.body.has_x_target_count() || spell.body.has_up_to_x_target_count())
        {
            // Without paying its mana cost X must be zero, so the spell has no targets.
            Some(ExilePermanentTargets {
                minimum: 0,
                maximum: 0,
                targets: vec![],
                division_total: None,
            })
        } else if full_announcement {
            if let ProgramKind::Spell(spell) = &program.kind
                && spell.modes.is_none()
                && let Some((minimum, maximum)) = spell.body.permanent_target_bounds()
            {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(card.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                let targets = self.trigger_target_domain(&spell.body, caster, Some(&source))?;
                let (minimum, maximum) = if spell.body.instructions.first()
                    == Some(&Instruction::ShuffleOnePermanentPerPlayer)
                {
                    let represented: BTreeSet<_> = targets
                        .iter()
                        .filter_map(|target| match target {
                            TargetRef::Object(id) => evaluated.get(*id)?.controller,
                            _ => None,
                        })
                        .collect();
                    (self.window.participants().len(), represented.len())
                } else {
                    (minimum, maximum.min(targets.len()))
                };
                Some(ExilePermanentTargets {
                    minimum,
                    maximum,
                    targets,
                    division_total: spell.body.target_division_total(),
                })
            } else {
                None
            }
        } else {
            None
        };
        let fixed_targets = if full_announcement {
            if let ProgramKind::Spell(spell) = &program.kind {
                let clauses = spell
                    .body
                    .instructions
                    .iter()
                    .filter_map(|op| match op {
                        Instruction::FixedTargetClause(body) => Some(body.as_ref()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if clauses.is_empty() {
                    None
                } else {
                    let source = self
                        .rules_snapshot(None)?
                        .view()
                        .appearance(card.id())
                        .ok_or(EffectError::InconsistentProgram)?;
                    Some(
                        clauses
                            .into_iter()
                            .map(|body| self.trigger_target_domain(body, caster, Some(&source)))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                }
            } else {
                None
            }
        } else {
            None
        };
        if matches!(
            ability,
            ExileCastAbility::Miracle
                | ExileCastAbility::Commander
                | ExileCastAbility::Siege
                | ExileCastAbility::TargetedGraveyard
                | ExileCastAbility::FoundSpell
                | ExileCastAbility::MatchingHand
                | ExileCastAbility::RevealedLibrary
        ) {
            let legal_modes = modes.as_ref().is_none_or(|spec| {
                let available = spec
                    .modes
                    .iter()
                    .filter(|mode| match mode.multiple {
                        Some((minimum, maximum)) => minimum <= maximum,
                        None => mode
                            .targets
                            .as_ref()
                            .is_none_or(|targets| !targets.is_empty()),
                    })
                    .count();
                if spec.allow_repeat && available > 0 {
                    true
                } else {
                    available >= usize::from(spec.minimum)
                }
            });
            if !legal_modes
                || permanent_targets
                    .as_ref()
                    .is_some_and(|spec| spec.minimum > spec.maximum)
                || fixed_targets
                    .as_ref()
                    .is_some_and(|clauses| clauses.iter().any(Vec::is_empty))
            {
                return Ok(None);
            }
        }
        let alternative_targetless = ability == ExileCastAbility::Impulse
            && matches!(
                program.conditional_cost,
                Some(crate::program::ConditionalCastCost::Overload { .. })
            );
        let target = match &program.kind {
            ProgramKind::Spell(spell) => spell.body.target,
            ProgramKind::Permanent(p) => match p.attachment {
                Some(crate::attachment::AttachmentKind::Aura(quality)) => quality.target(),
                _ => BodyTarget::None,
            },
        };
        let targets =
            if modes.is_some() || permanent_targets.is_some() || target == BodyTarget::None {
                None
            } else {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(card.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                let body = match &program.kind {
                    ProgramKind::Spell(spell) => spell.body.clone(),
                    _ => ExecutableBody::new(target, vec![]),
                };
                let mut options = self.trigger_target_domain(&body, caster, Some(&source))?;
                if let Some(crate::attachment::AttachmentKind::Aura(quality)) =
                    program.permanent().and_then(|p| p.attachment)
                {
                    options.retain(|&t| self.aura_target_legal(quality, t, caster, &evaluated));
                }
                if options.is_empty() && !alternative_targetless {
                    return Ok(None);
                }
                Some(options)
            };
        Ok(Some(ExileCastOffer {
            normal_available: true,
            adventure: None,
            split_right: None,
            ability,
            spell: card.id(),
            madness_price: if ability == ExileCastAbility::Madness {
                Some(
                    program
                        .madness
                        .clone()
                        .ok_or(EffectError::InconsistentProgram)?,
                )
            } else {
                None
            },
            hybrid_symbols: if ability == ExileCastAbility::Impulse {
                program.hybrid_symbols.clone()
            } else {
                Vec::new()
            },
            normal_price,
            allows_x: (ability == ExileCastAbility::Miracle
                && program
                    .miracle
                    .as_ref()
                    .is_some_and(|price| price.x_symbols != 0))
                || (ability == ExileCastAbility::Impulse && program.x_mana_symbols != 0)
                || program.x_mana_symbols == 0
                    && (program
                        .additional_life_cost
                        .as_ref()
                        .is_some_and(crate::program::ValueExpr::is_chosen_x)
                        || program.additional_cast_costs.iter().any(|cost| {
                            matches!(cost, crate::program::AdditionalCastCost::KickerX(_))
                        })),
            allows_alternative: ability == ExileCastAbility::Impulse
                && program.conditional_cost.is_some(),
            alternative_targetless,
            alternative_price: if alternative_targetless {
                program
                    .conditional_cost
                    .as_ref()
                    .map(|cost| cost.mana().clone())
            } else {
                None
            },
            modes,
            permanent_targets,
            fixed_targets,
            targets,
        }))
    }
}
