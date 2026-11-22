//! Private top-library choice with host-owned random bottom order.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::{LookTopFilter, ZoneActionKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LookTopWork {
    pub(super) cursor: ResolutionCursor,
    player: SeatId,
    library_owner: SeatId,
    private_exile: bool,
    mana_value_life: Option<(RulesInteger, bool)>,
    exploring: Option<GameObject>,
    explore_nonland: bool,
    domain: Vec<ObjectId>,
    eligible: Vec<ObjectId>,
    reveal: bool,
    reveal_only: bool,
    required: bool,
    battlefield_tapped: bool,
    battlefield_else_hand: bool,
    planeswalkers: bool,
    choose_land_kind: bool,
    keep_remaining: bool,
    hand_limit: Option<usize>,
    selected: Vec<ObjectId>,
    graveyard_order: Option<Vec<ObjectId>>,
    bottom: Option<Vec<ObjectId>>,
    pub(super) replacement: Option<ZoneReplacementBatch>,
}
impl LookTopWork {
    fn graveyard_order_frame(&self, revision: u64) -> Option<LinkedReturnOrderFrame> {
        if self.graveyard_order.is_some() {
            return None;
        }
        let batch = self.replacement.as_ref()?;
        let destination = crate::program::PublicZoneDestination::Graveyard;
        let objects: Vec<_> = self
            .selected
            .iter()
            .copied()
            .filter(|&id| batch.destination(id).ok() == Some(destination))
            .collect();
        (objects.len() > 1).then_some(LinkedReturnOrderFrame {
            revision,
            chooser: self.library_owner,
            objects,
            destination,
        })
    }
    fn choice(&self, revision: u64) -> Option<EffectDecision> {
        if self.bottom.is_some() {
            return self
                .replacement
                .as_ref()
                .and_then(|batch| batch.frame(revision))
                .map(EffectDecision::ZoneReplacement)
                .or_else(|| {
                    self.graveyard_order_frame(revision)
                        .map(EffectDecision::LinkedReturnOrder)
                });
        }
        Some(EffectDecision::LibraryTop(
            LibraryTopFrame::looked_top(
                revision,
                self.cursor.entry,
                self.cursor.instruction,
                self.player,
                self.domain.clone(),
                self.eligible.clone(),
                self.required,
                self.reveal,
                self.planeswalkers,
                self.choose_land_kind,
                self.hand_limit,
                self.explore_nonland,
            )
            .with_reveal_only(self.reveal_only),
        ))
    }
}
impl EffectWindow {
    pub(super) fn currently_revealed_look_top(&self, id: ObjectId) -> bool {
        matches!(&self.pending_work, Some(PendingWork::LookTop(work)) if (work.exploring.is_some() || work.mana_value_life.is_some() || work.battlefield_else_hand) && work.domain.first()==Some(&id))
    }
    fn explore_counter(
        &mut self,
        source: &GameObject,
        cursor: &ResolutionCursor,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self
            .window
            .cards()
            .get(source.id())
            .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
        {
            self.add_source_counters(
                source.id(),
                cursor.entry,
                cursor.instruction,
                1.into(),
                resolution_choices::CounterPlacement::Ordinary,
                journal,
            )?;
        }
        Ok(())
    }
    fn explored(
        &mut self,
        source: GameObject,
        controller: SeatId,
        cursor: &ResolutionCursor,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            journal,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::Explored {
                    source,
                    controller,
                }])
            },
        )
    }
    pub(super) fn look_top_frame(&self) -> Option<EffectDecision> {
        match &self.pending_work {
            Some(PendingWork::LookTop(work)) => work.choice(self.revision),
            _ => None,
        }
    }
    pub(super) fn begin_look_top(
        &mut self,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let (count, filter, required, reveal, battlefield_tapped, battlefield_else_hand) =
            match instruction {
                Instruction::LookTopReveal { count, filter } => {
                    (*count, Some(*filter), false, true, false, false)
                }
                Instruction::LookOpponentExileForCast { count }
                | Instruction::LookTopTakeOne { count } => {
                    (*count, None, true, false, false, false)
                }
                Instruction::LookTopLandBattlefieldTapped => {
                    (1, Some(LookTopFilter::Land), false, false, true, false)
                }
                Instruction::MayRevealLibraryTop => (1, None, false, true, false, false),
                Instruction::ExploreSource | Instruction::RevealTopHandLoseManaValue { .. } => {
                    (1, None, false, true, false, false)
                }
                Instruction::RevealUntilLandKind => (0, None, false, false, false, false),
                Instruction::LookTopPlaneswalkers => (7, None, false, false, false, false),
                Instruction::RevealTopPermanentOrHand => (1, None, false, false, false, true),
                _ => return Ok(false),
            };
        self.resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if *instruction == Instruction::MayRevealLibraryTop {
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results
                .revealed_top_types
                .clear();
        }
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let exploring = if instruction == &Instruction::ExploreSource {
            let (StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source }) =
                entry.kind()
            else {
                return Err(EffectError::InconsistentProgram);
            };
            Some(source.clone())
        } else {
            None
        };
        let player = exploring
            .as_ref()
            .and_then(|source| {
                self.window
                    .cards()
                    .get(source.id())
                    .and_then(GameObject::controller)
            })
            .unwrap_or(entry.controller());
        let private_exile = matches!(instruction, Instruction::LookOpponentExileForCast { .. });
        let library_owner = if private_exile {
            let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
            let target = self.instruction_target(entry)?;
            let TargetRef::Player(seat) = target.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            seat
        } else {
            player
        };
        let domain = self
            .window
            .cards()
            .library(library_owner)
            .iter()
            .take(usize::from(count))
            .copied()
            .collect::<Vec<_>>();
        let choose_land_kind = matches!(instruction, Instruction::RevealUntilLandKind);
        if domain.is_empty()
            && let Some(source) = &exploring
        {
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            self.explore_counter(source, &cursor, append)?;
            self.explored(source.clone(), player, &cursor, append)?;
        }
        if domain.is_empty() && !choose_land_kind {
            self.advance_instruction()?;
            return Ok(true);
        }
        let evaluated = self.evaluated_characteristics()?;
        let planeswalkers = matches!(instruction, Instruction::LookTopPlaneswalkers);
        let explore_nonland = exploring.is_some()
            && evaluated
                .get(domain[0])
                .is_some_and(|card| !card.has_type(CardType::Land));
        let keep_remaining =
            explore_nonland || filter == Some(LookTopFilter::SourceChosenCreatureTypeKeepRest);
        let chosen_type = match filter {
            Some(LookTopFilter::AllOfChosenType(slot)) => Some(
                *self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.values.get(&slot))
                    .ok_or(EffectError::InconsistentProgram)?,
            ),
            Some(
                LookTopFilter::SourceChosenCreatureType
                | LookTopFilter::SourceChosenCreatureTypeKeepRest,
            ) => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => {
                        self.entry_creature_type_choices.get(&source.id()).copied()
                    }
                    _ => return Err(EffectError::InconsistentProgram),
                }
            }
            _ => None,
        };
        let eligible = domain
            .iter()
            .copied()
            .filter(|id| {
                evaluated.get(*id).is_some_and(|value| {
                    if planeswalkers {
                        return value.has_type(CardType::Planeswalker);
                    }
                    if battlefield_else_hand {
                        return value
                            .current_types
                            .iter()
                            .copied()
                            .any(CardType::is_permanent)
                            && self
                                .copiable_program(*id)
                                .is_some_and(|program| program.mana_value() <= 3.into())
                            && self
                                .linked_aura_domain(*id, player, &evaluated)
                                .is_none_or(|domain| !domain.is_empty());
                    }
                    match filter {
                        Some(LookTopFilter::NoncreatureNonland) => {
                            !value.has_type(CardType::Creature) && !value.has_type(CardType::Land)
                        }
                        Some(LookTopFilter::Enchantment) => value.has_type(CardType::Enchantment),
                        Some(
                            LookTopFilter::SourceChosenCreatureType
                            | LookTopFilter::SourceChosenCreatureTypeKeepRest,
                        ) => {
                            value.is_creature()
                                && chosen_type.is_some_and(|kind| {
                                    value.current_creature_subtypes.contains(&kind)
                                })
                        }
                        Some(LookTopFilter::AllOfChosenType(_)) => chosen_type
                            .is_some_and(|kind| value.current_creature_subtypes.contains(&kind)),
                        Some(LookTopFilter::Land) => value.has_type(CardType::Land),
                        None => true,
                    }
                })
            })
            .collect::<Vec<_>>();
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let mana_value_life =
            if let Instruction::RevealTopHandLoseManaValue { gain_life } = instruction {
                Some((
                    self.copiable_program(domain[0])
                        .ok_or(EffectError::InconsistentProgram)?
                        .mana_value(),
                    *gain_life,
                ))
            } else {
                None
            };
        if exploring.is_some() || battlefield_else_hand || mana_value_life.is_some() {
            let object = self
                .window
                .cards()
                .get(domain[0])
                .ok_or(EffectError::UnknownObject)?
                .clone();
            let before = self.rules_snapshot(None)?;
            self.emit_from(
                EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                &before,
                None,
                append,
                |_, _, _| {
                    Ok(vec![
                        crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
                    ])
                },
            )?;
        }
        let hand_limit =
            matches!(filter, Some(LookTopFilter::AllOfChosenType(_))).then_some(eligible.len());
        if let Some(source) = &exploring
            && explore_nonland
        {
            self.explore_counter(source, &cursor, append)?;
        }
        let mut work = LookTopWork {
            reveal_only: *instruction == Instruction::MayRevealLibraryTop,
            exploring,
            explore_nonland,
            mana_value_life,
            library_owner,
            private_exile,
            cursor,
            player,
            domain,
            eligible,
            reveal,
            required,
            battlefield_tapped,
            battlefield_else_hand,
            planeswalkers,
            choose_land_kind,
            keep_remaining,
            hand_limit,
            selected: vec![],
            graveyard_order: None,
            bottom: None,
            replacement: None,
        };
        if work.exploring.is_some() && !work.explore_nonland
            || work.mana_value_life.is_some()
            || battlefield_else_hand && work.eligible.is_empty()
        {
            let id = work.domain[0];
            work.selected.push(id);
            work.bottom = Some(vec![]);
            work.replacement = Some(self.look_top_batch(&work)?);
        }
        self.pending_work = Some(PendingWork::LookTop(Box::new(work)));
        Ok(true)
    }
    pub(super) fn bottom_remaining_looked_at(
        &mut self,
        player: SeatId,
        entry: StackEntryId,
        bottom: &[ObjectId],
    ) -> Result<(), EffectError> {
        if bottom.is_empty() {
            return Ok(());
        }
        let top = self
            .window
            .cards()
            .library(player)
            .get(..bottom.len())
            .ok_or(EffectError::InvalidLibraryPartition)?;
        let (window, _) = self.window.record_library_top(
            self.window.revision(),
            entry,
            crate::stack_window::LibraryTopCommit {
                player,
                frozen: top,
                top: &[],
                bottom,
                moves_application: &[],
            },
        )?;
        self.window = window;
        Ok(())
    }
    /// Resolve one controller's reveal and entry before shuffling and processing the next creature.
    pub(super) fn progress_creature_reveals(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(&player) = cursor.impulse.transformed_controllers.front() else {
            return self.advance_instruction();
        };
        if cursor.impulse.shuffle_after_reveal {
            return self.begin_shuffle(player, true);
        }
        let entry = cursor.entry;
        let instruction = cursor.instruction;
        let evaluated = self.evaluated_characteristics()?;
        let mut reveals = Vec::new();
        let mut found = None;
        for &id in self.window.cards().library(player) {
            let card = self
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?;
            reveals.push(crate::occurrence::RulesOccurrence::LibraryCardRevealed {
                object: card.clone(),
            });
            if evaluated.get(id).is_some_and(|card| card.is_creature()) {
                found = Some(id);
                break;
            }
        }
        if !reveals.is_empty() {
            let before = self.rules_snapshot(None)?;
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &before,
                None,
                append,
                |_, _, _| Ok(reveals),
            )?;
        }
        self.resolution_cursor_mut()
            .ok_or(EffectError::InconsistentProgram)?
            .impulse
            .shuffle_after_reveal = true;
        if let Some(id) = found {
            self.linked_return_work = Some(LinkedReturnWork {
                origin: LinkedReturnOrigin::LibraryTop {
                    player,
                    entry,
                    instruction,
                    tapped: false,
                },
                objects: vec![id],
                library_bottom: None,
                wake_entry: None,
                orders: BTreeMap::new(),
                graveyard_orders: BTreeMap::new(),
                choices: BTreeMap::new(),
                auras: BTreeMap::new(),
                replacements: None,
            });
            self.finish_linked_return_if_ready(append)?;
        }
        Ok(())
    }
    pub(super) fn hand_entry_selection(
        &self,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let (chooser, types, maximum) = match self.current_instruction()? {
            Instruction::PutHandPermanentsOntoBattlefield {
                player,
                types,
                maximum,
                ..
            } => (
                self.resolving_player(*player)?,
                Some(types),
                usize::from(*maximum),
            ),
            Instruction::ReturnOddGraveyardPermanentIfPaid { .. }
            | Instruction::PutGraveyardCreatureAtMostX
            | Instruction::PutLandFromExiledCohort => (
                self.resolving_player(crate::program::ResolvingPlayer::Controller)?,
                None,
                1,
            ),
            Instruction::PutTargetOwnedExiledLandsOntoBattlefield => (
                self.resolving_player(crate::program::ResolvingPlayer::Controller)?,
                None,
                usize::MAX,
            ),
            Instruction::PutEquipmentFromHandOrGraveyard => (
                self.resolving_player(crate::program::ResolvingPlayer::Controller)?,
                None,
                1,
            ),
            _ => return Err(EffectError::InconsistentProgram),
        };
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let evaluated = self.evaluated_characteristics()?;
        let exile_owner = if self.current_instruction()?
            == &Instruction::PutTargetOwnedExiledLandsOntoBattlefield
        {
            Some(self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?)
        } else {
            None
        };
        let from_cohort = matches!(
            self.current_instruction()?,
            Instruction::PutLandFromExiledCohort
        );
        let odd_permanent = matches!(
            self.current_instruction()?,
            Instruction::ReturnOddGraveyardPermanentIfPaid { .. }
        );
        if odd_permanent
            && !matches!(self.payloads.get(&cursor.entry), Some(StackPayload::Instructions(payload)) if payload.met_mana_threshold)
        {
            return Ok(None);
        }
        let from_graveyard = odd_permanent
            || self.current_instruction()? == &Instruction::PutGraveyardCreatureAtMostX;
        let maximum_mana = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .chosen_x()
            .cloned()
            .unwrap_or_else(|| 0.into());
        let cohort = self.typed_exile_cohorts.get(&cursor.entry);
        let domain: Vec<_> = evaluated
            .objects
            .values()
            .filter(|card| {
                (match types {
                    None if odd_permanent => {
                        card.zone == ObjectZone::Graveyard(chooser)
                            && self.copiable_program(card.object).is_some_and(|program| {
                                let value = program.mana_value();
                                program.permanent().is_some()
                                    && value.nonnegative_half_rounded_up()
                                        != value.nonnegative_half_rounded_down()
                            })
                    }
                    None if from_graveyard => {
                        card.zone == ObjectZone::Graveyard(chooser)
                            && card.is_creature()
                            && self
                                .copiable_program(card.object)
                                .is_some_and(|p| p.mana_value() <= maximum_mana)
                    }
                    None if from_cohort => {
                        card.zone == ObjectZone::Exile
                            && card.has_type(CardType::Land)
                            && cohort.is_some_and(|c| c.cards.contains(&card.object))
                    }
                    None if exile_owner.is_some() => {
                        card.zone == ObjectZone::Exile
                            && card.has_type(CardType::Land)
                            && self
                                .window
                                .cards()
                                .get(card.object)
                                .is_some_and(|object| Some(object.owner()) == exile_owner)
                            && !self.face_down_exile.contains_key(&card.object)
                            && !self.characteristics.foretold.contains_key(&card.object)
                    }
                    Some(types) => {
                        card.zone == ObjectZone::Hand(chooser)
                            && !types.is_disjoint(&card.current_types)
                    }
                    None => {
                        (card.zone == ObjectZone::Hand(chooser)
                            || card.zone == ObjectZone::Graveyard(chooser))
                            && card
                                .current_artifact_subtypes
                                .contains(&crate::program::ArtifactSubtype::Equipment)
                    }
                }) && self
                    .linked_aura_domain(card.object, chooser, &evaluated)
                    .is_none_or(|candidates| !candidates.is_empty())
            })
            .map(|card| card.object)
            .collect();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                cursor.entry,
                cursor.instruction,
                chooser,
                domain,
                maximum,
                if from_graveyard {
                    ResolutionSelectionKind::GraveyardToBattlefield
                } else if exile_owner.is_some() || from_cohort {
                    ResolutionSelectionKind::ExileToBattlefield
                } else {
                    ResolutionSelectionKind::HandToBattlefield
                },
            )
        }))
    }
    pub(super) fn resume_hand_entry(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.hand_entry_selection()?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(objects) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if objects.len() < frame.minimum()
            || objects.len() > frame.required()
            || objects.iter().collect::<BTreeSet<_>>().len() != objects.len()
            || objects.iter().any(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let mut journal = RulesJournal::default();
        if !objects.is_empty() {
            if next.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram);
            }
            next.linked_return_work = Some(LinkedReturnWork {
                origin: if frame.operation() == ResolutionSelectionKind::ExileToBattlefield {
                    LinkedReturnOrigin::ChosenExile {
                        controller: Some(actor),
                        entry: frame.entry(),
                        instruction: frame.instruction(),
                    }
                } else if next
                    .window
                    .cards()
                    .get(objects[0])
                    .is_some_and(|card| card.zone() == ObjectZone::Graveyard(actor))
                {
                    LinkedReturnOrigin::Graveyard {
                        controller: None,
                        subtype_counters: None,
                        entry_additions: crate::program::EntryAdditions::default(),
                        forest: false,
                        corpse: false,
                        tapped: false,
                        player: Some(actor),
                        entry: frame.entry(),
                        instruction: frame.instruction(),
                    }
                } else {
                    LinkedReturnOrigin::Hand {
                        tapped: matches!(
                            self.current_instruction()?,
                            Instruction::PutHandPermanentsOntoBattlefield { tapped: true, .. }
                        ),
                        player: actor,
                        entry: frame.entry(),
                        instruction: frame.instruction(),
                    }
                },
                objects,
                library_bottom: None,
                wake_entry: None,
                orders: BTreeMap::new(),
                graveyard_orders: BTreeMap::new(),
                choices: BTreeMap::new(),
                auras: BTreeMap::new(),
                replacements: None,
            });
            next.finish_linked_return_if_ready(&mut journal)?;
        }
        next.advance_instruction()?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            journal,
        )
    }
    /// Whether these previously revealed library cards need an owner-provided graveyard order.
    pub fn is_looked_graveyard_order(&self, frame: &LinkedReturnOrderFrame) -> bool {
        self.look_top_frame() == Some(EffectDecision::LinkedReturnOrder(frame.clone()))
    }
    pub(super) fn resume_looked_order(
        &self,
        expected: u64,
        bound: &LinkedReturnOrderFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if !self.is_looked_graveyard_order(bound) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if bound.chooser != actor {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::LinkedReturnOrder(order) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if order.len() != bound.objects.len()
            || order.iter().copied().collect::<BTreeSet<_>>()
                != bound.objects.iter().copied().collect()
        {
            return Err(EffectError::IllegalTimestampOrder);
        }
        let mut next = self.clone();
        next.revision = revision;
        let Some(PendingWork::LookTop(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongResolutionFrame);
        };
        work.graveyard_order = Some(order);
        next.progress_look_top(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    fn look_top_batch(&self, work: &LookTopWork) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            action: if work.explore_nonland {
                ZoneActionKind::EffectRevealedToGraveyard
            } else if work.private_exile {
                ZoneActionKind::EffectExile
            } else {
                ZoneActionKind::EffectLookToHand
            },
            intents: work
                .selected
                .iter()
                .map(|&before| zone_replacement::ZoneMoveIntent {
                    before,
                    reason: if work.explore_nonland {
                        ZoneReason::RevealedToGraveyard
                    } else if work.private_exile {
                        ZoneReason::ExiledByEffect
                    } else {
                        ZoneReason::LookTopToHand
                    },
                })
                .collect(),
        })
    }
    pub(crate) fn resume_reveal_land_kind(
        &self,
        expected: u64,
        frame: &LibraryTopFrame,
        actor: SeatId,
        land: bool,
        tape: &EntropyTape,
    ) -> Result<(RuleProgress, EntropyTape), crate::game::GameError> {
        let revision = self.next_revision(expected)?;
        if !frame.chooses_land_kind()
            || self.look_top_frame().as_ref() != Some(&EffectDecision::LibraryTop(frame.clone()))
        {
            return Err(EffectError::WrongLibraryTopFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser.into());
        }
        let mut next = self.clone();
        let Some(PendingWork::LookTop(mut work)) = next.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame.into());
        };
        let evaluated = next.evaluated_characteristics()?;
        let mut shown = Vec::new();
        for &id in next.window.cards().library(work.player) {
            let object = next
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?
                .clone();
            work.domain.push(id);
            shown.push(object);
            if evaluated
                .get(id)
                .ok_or(EffectError::InconsistentProgram)?
                .has_type(CardType::Land)
                == land
            {
                work.selected.push(id);
                break;
            }
        }
        let (bottom, entropy) = tape.shuffled(
            work.domain
                .iter()
                .copied()
                .filter(|id| !work.selected.contains(id))
                .collect(),
        )?;
        work.bottom = Some(bottom);
        let mut append = RulesJournal::default();
        for object in shown {
            let before = next.rules_snapshot(None)?;
            next.emit_from(
                EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                &before,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![
                        crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
                    ])
                },
            )?;
        }
        if !work.selected.is_empty() {
            work.replacement = Some(next.look_top_batch(&work)?);
        }
        next.revision = revision;
        next.pending_work = Some(PendingWork::LookTop(work));
        Ok((
            next.progress_look_top(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                append,
            )?,
            entropy,
        ))
    }
    pub(crate) fn resume_look_top(
        &self,
        expected: u64,
        frame: &LibraryTopFrame,
        actor: SeatId,
        selected: Vec<ObjectId>,
        tape: &crate::shuffle::EntropyTape,
    ) -> Result<(RuleProgress, crate::shuffle::EntropyTape), crate::game::GameError> {
        let revision = self.next_revision(expected)?;
        if self.look_top_frame().as_ref() != Some(&EffectDecision::LibraryTop(frame.clone())) {
            return Err(EffectError::WrongLibraryTopFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser.into());
        }
        let mut next = self.clone();
        let Some(PendingWork::LookTop(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame.into());
        };
        let mut work = *work;
        if work.planeswalkers {
            if selected.len() > 2
                || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
                || selected.iter().any(|id| !work.eligible.contains(id))
            {
                return Err(EffectError::InvalidLibraryPartition.into());
            }
            let (bottom, entropy) = tape.shuffled(
                work.domain
                    .iter()
                    .copied()
                    .filter(|id| !selected.contains(id))
                    .collect(),
            )?;
            let mut append = RulesJournal::default();
            let player = work.player;
            let entry = work.cursor.entry;
            let instruction = work.cursor.instruction;
            next.pending_work = Some(PendingWork::Resolution(work.cursor));
            if selected.is_empty() {
                next.bottom_remaining_looked_at(player, entry, &bottom)?;
            } else {
                if next.linked_return_work.is_some() {
                    return Err(EffectError::InconsistentProgram.into());
                }
                next.linked_return_work = Some(LinkedReturnWork {
                    origin: LinkedReturnOrigin::LibraryTop {
                        player,
                        entry,
                        instruction,
                        tapped: false,
                    },
                    library_bottom: Some(bottom),
                    objects: selected,
                    wake_entry: None,
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                next.finish_linked_return_if_ready(&mut append)?;
            }
            next.revision = revision;
            next.advance_instruction()?;
            return Ok((
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                entropy,
            ));
        }
        if selected.len() > work.hand_limit.unwrap_or(1)
            || selected.iter().copied().collect::<BTreeSet<_>>().len() != selected.len()
        {
            return Err(EffectError::InvalidLibraryPartition.into());
        }
        if work.required && selected.is_empty()
            || selected.iter().any(|id| !work.eligible.contains(id))
        {
            return Err(EffectError::IllegalTarget.into());
        }
        if work.reveal_only {
            let mut append = RulesJournal::default();
            work.cursor.results.revealed_top_types.clear();
            if let Some(&id) = selected.first() {
                let before = next.rules_snapshot(None)?;
                let object = next
                    .window
                    .cards()
                    .get(id)
                    .cloned()
                    .ok_or(EffectError::UnknownObject)?;
                work.cursor.results.revealed_top_types = before
                    .view()
                    .evaluated
                    .get(id)
                    .ok_or(EffectError::InconsistentProgram)?
                    .current_types
                    .clone();
                next.emit_from(
                    EventCause::Instruction {
                        entry: work.cursor.entry,
                        instruction: work.cursor.instruction,
                    },
                    &before,
                    None,
                    &mut append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
                        ])
                    },
                )?;
            }
            next.pending_work = Some(PendingWork::Resolution(work.cursor));
            next.revision = revision;
            next.advance_instruction()?;
            return Ok((
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                tape.clone(),
            ));
        }
        if work.battlefield_tapped || work.battlefield_else_hand && !selected.is_empty() {
            if next.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram.into());
            }
            let cursor = work.cursor;
            next.pending_work = Some(PendingWork::Resolution(cursor.clone()));
            let mut append = RulesJournal::default();
            if let Some(&id) = selected.first() {
                next.linked_return_work = Some(LinkedReturnWork {
                    library_bottom: None,
                    origin: LinkedReturnOrigin::LibraryTop {
                        player: work.player,
                        entry: cursor.entry,
                        instruction: cursor.instruction,
                        tapped: work.battlefield_tapped,
                    },
                    wake_entry: None,
                    objects: vec![id],
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                next.finish_linked_return_if_ready(&mut append)?;
            }
            next.revision = revision;
            next.advance_instruction()?;
            let (state, result, more) = next
                .progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?
                .into_parts();
            return Ok((RuleProgress::new(state, result, more), tape.clone()));
        }
        let selected = if work.battlefield_else_hand {
            vec![work.domain[0]]
        } else {
            selected
        };
        let remaining = work
            .domain
            .iter()
            .copied()
            .filter(|id| !selected.contains(id))
            .collect::<Vec<_>>();
        let (bottom, entropy) = if work.keep_remaining {
            (Vec::new(), tape.clone())
        } else {
            tape.shuffled(remaining)?
        };
        let mut append = RulesJournal::default();
        if work.reveal && work.exploring.is_none() && !selected.is_empty() {
            let shown = selected
                .iter()
                .map(|&id| {
                    next.window
                        .cards()
                        .get(id)
                        .cloned()
                        .map(
                            |object| crate::occurrence::RulesOccurrence::LibraryCardRevealed {
                                object,
                            },
                        )
                        .ok_or(EffectError::UnknownObject)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let before = next.rules_snapshot(None)?;
            next.emit_from(
                EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                &before,
                None,
                &mut append,
                |_, _, _| Ok(shown),
            )?;
        }
        work.selected = selected;
        work.bottom = Some(bottom);
        if !work.selected.is_empty() {
            work.replacement = Some(next.look_top_batch(&work)?);
        }
        next.revision = revision;
        next.pending_work = Some(PendingWork::LookTop(Box::new(work)));
        let progress = next.progress_look_top(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )?;
        Ok((progress, entropy))
    }
    pub(super) fn progress_look_top(
        mut self,
        journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        draws: DrawJournal,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::LookTop(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        if let Some(batch) = &mut work.replacement
            && let Some(frame) = batch.progress(self.revision)?
        {
            self.pending_work = Some(PendingWork::LookTop(Box::new(work)));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal,
                    declarations,
                    assignments,
                    draws,
                },
                append,
            ));
        }
        if let Some(frame) = work.graveyard_order_frame(self.revision) {
            self.pending_work = Some(PendingWork::LookTop(Box::new(work)));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame: EffectDecision::LinkedReturnOrder(frame),
                    journal,
                    declarations,
                    assignments,
                    draws,
                },
                append,
            ));
        }
        let mut moves = Vec::new();
        if let Some(batch) = &work.replacement {
            batch.verify_initial(self.look_top_batch(&work)?)?;
            for &id in &work.selected {
                moves.push(crate::stack_window::LibraryTopMove {
                    before: id,
                    destination: batch.destination(id)?,
                });
            }
            if let Some(order) = &work.graveyard_order {
                moves.sort_by_key(|movement| {
                    order
                        .iter()
                        .rev()
                        .position(|id| *id == movement.before)
                        .unwrap_or(order.len())
                });
            }
        }
        let before = self.rules_snapshot(None)?;
        let top = if work.keep_remaining {
            work.domain
                .iter()
                .copied()
                .filter(|id| !work.selected.contains(id))
                .collect()
        } else {
            Vec::new()
        };
        let (window, movements) = self.window.record_library_top(
            self.window.revision(),
            work.cursor.entry,
            crate::stack_window::LibraryTopCommit {
                player: work.library_owner,
                frozen: &work.domain,
                top: &top,
                bottom: work
                    .bottom
                    .as_deref()
                    .ok_or(EffectError::InconsistentProgram)?,
                moves_application: &moves,
            },
        )?;
        self.window = window;
        let reason = if work.explore_nonland {
            ZoneReason::RevealedToGraveyard
        } else if work.private_exile {
            ZoneReason::ExiledByEffect
        } else {
            ZoneReason::LookTopToHand
        };
        if work.private_exile {
            for movement in &movements {
                if movement.after.zone() == ObjectZone::Exile {
                    self.face_down_exile
                        .insert(movement.after.id(), work.player);
                    self.any_type_exile_cast
                        .insert((movement.after.id(), work.player));
                }
            }
        }

        if !movements.is_empty() {
            self.emit_from(
                EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                &before,
                None,
                &mut append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(previous, current, movement, reason, controllers)
                        })
                        .collect()
                },
            )?;
        }
        if let Some(source) = work.exploring {
            self.explored(source, work.player, &work.cursor, &mut append)?;
        }
        if let Some((amount, gain)) = &work.mana_value_life
            && amount.is_positive()
        {
            let adjustment = crate::life::LifeAdjustment::Lose(amount.clone());
            let own = gain.then(|| crate::life::LifeAdjustment::Gain(amount.clone()));
            self.adjust_each_opponent_life_at_cut(
                work.cursor.entry,
                work.cursor.instruction,
                work.player,
                &adjustment,
                own.as_ref(),
                &mut append,
            )?;
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        self.advance_instruction()?;
        self.progress_rules(journal, declarations, assignments, draws, append)
    }
}
