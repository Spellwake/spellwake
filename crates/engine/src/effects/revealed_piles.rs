//! Ordered library selections and two-pile choices. The objects stay in place while players choose.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{PublicZoneDestination as Zone, ZoneActionKind};

/// A choice among identified objects. Dividing a pile can leave either pile empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PileChoice {
    Divide(Vec<ObjectId>),
    DividePermanents(Vec<ObjectId>),
    Sacrifice {
        first: Vec<ObjectId>,
        second: Vec<ObjectId>,
    },
    Creature(Vec<ObjectId>),
    /// The controller privately chooses one of the looked-at cards to keep.
    OneToHand(Vec<ObjectId>),
    OneToHandRestBottom(Vec<ObjectId>),
    /// Privately select up to one eligible card to enter, then order all other looked-at cards.
    OptionalCreatureToBattlefield {
        looked: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
    },
    /// Select up to `max` distinct eligible cards from the looked-at or revealed cards.
    OptionalUpTo {
        looked: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
        max: usize,
    },
    Take {
        first: Vec<ObjectId>,
        second: Vec<ObjectId>,
    },
    GraveyardOrder(Vec<ObjectId>),
    BottomOrder(Vec<ObjectId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PileChoiceFrame {
    revision: u64,
    pub(super) chooser: SeatId,
    choice: PileChoice,
}

impl PileChoiceFrame {
    pub(super) fn graveyard_order(revision: u64, chooser: SeatId, domain: Vec<ObjectId>) -> Self {
        Self {
            revision,
            chooser,
            choice: PileChoice::GraveyardOrder(domain),
        }
    }

    pub(super) fn validate_graveyard_order(&self, order: &[ObjectId]) -> Result<(), EffectError> {
        if matches!(&self.choice, PileChoice::GraveyardOrder(domain)
            if order.len() == domain.len() && order.iter().collect::<BTreeSet<_>>() == domain.iter().collect())
        {
            Ok(())
        } else {
            Err(EffectError::InvalidLibraryPartition)
        }
    }

    pub fn chooser(&self) -> SeatId {
        self.chooser
    }

    pub fn choice(&self) -> &PileChoice {
        &self.choice
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RevealedPiles {
    pub(super) cards: Vec<ObjectId>,
    entry_offer: Option<EntryChoiceFrame>,
    pub(super) aura_offer: Option<LinkedAuraChoiceFrame>,
    entry_choice: Option<crate::land::EntryChoice>,
    first: Option<Vec<ObjectId>>,
    pub(super) hand: Option<Vec<ObjectId>>,
    pub(super) pending_random_bottom: Option<Vec<ObjectId>>,
    batches: Vec<(Zone, Vec<ObjectId>, ZoneReplacementBatch)>,
    graveyard_order: Option<Vec<ObjectId>>,
    pub(super) bottom_order: Option<Vec<ObjectId>>,
    offered: Option<PileChoiceFrame>,
}

impl RevealedPiles {
    pub(super) fn new(cards: Vec<ObjectId>) -> Self {
        Self {
            cards,
            entry_offer: None,
            aura_offer: None,
            entry_choice: None,
            first: None,
            hand: None,
            pending_random_bottom: None,
            batches: Vec::new(),
            graveyard_order: None,
            bottom_order: None,
            offered: None,
        }
    }
}

impl EffectWindow {
    fn revealed_batch(
        &self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        zone: Zone,
        cards: &[ObjectId],
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let (action, reason) = match zone {
            Zone::Battlefield => (
                ZoneActionKind::EffectEnterBattlefield,
                ZoneReason::LookTopToBattlefield,
            ),
            Zone::Hand => (ZoneActionKind::EffectLookToHand, ZoneReason::LookTopToHand),
            Zone::Graveyard => (
                ZoneActionKind::EffectRevealedToGraveyard,
                ZoneReason::RevealedToGraveyard,
            ),
            Zone::Exile => (ZoneActionKind::EffectExile, ZoneReason::ExiledByEffect),
            _ => return Err(EffectError::InconsistentProgram),
        };
        let input = zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction { entry, instruction },
            action,
            intents: cards
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        };
        if zone == Zone::Battlefield {
            self.begin_zone_replacements_with_entry(
                input,
                Some(&cards.iter().map(|&id| (id, player)).collect()),
            )
        } else {
            self.begin_zone_replacements(input)
        }
    }

    pub(super) fn progress_revealed_piles(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        journal: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Instruction::MutatePlayer(spec) = self.current_instruction()? else {
            return Err(EffectError::InconsistentProgram);
        };
        let crate::toolbox::PlayerMutation::RevealTopTwoPiles {
            count: count_expr,
            controller_divides,
            target_opponent,
            chosen_dest,
            other_dest,
        } = &spec.op
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let (controller_divides, target_opponent, chosen_dest, other_dest) = (
            *controller_divides,
            *target_opponent,
            *chosen_dest,
            *other_dest,
        );
        let count_expr = count_expr.clone();
        let count = self
            .resolve_value_expr(
                self.window.resolving().map_or(player, |e| e.controller()),
                &count_expr,
            )?
            .max(0.into())
            .min(RulesInteger::from_usize(
                self.window.cards().library(player).len(),
            ))
            .to_usize()
            .ok_or(EffectError::InconsistentProgram)?;
        let other_pile_to_bottom = matches!(
            other_dest,
            crate::program::LookTopRestDest::BottomRandom
                | crate::program::LookTopRestDest::BottomOrder
                | crate::program::LookTopRestDest::TopOrder
        );

        let mut work = if let Some(work) = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .revealed_piles
            .take()
        {
            work
        } else {
            let cards: Vec<_> = self
                .window
                .cards()
                .library(player)
                .iter()
                .take(count)
                .copied()
                .collect();
            if cards.is_empty() {
                return Ok(None);
            }
            let objects = cards
                .iter()
                .map(|id| {
                    self.window
                        .cards()
                        .get(*id)
                        .cloned()
                        .ok_or(EffectError::UnknownObject)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let before = self.rules_snapshot(None)?;
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &before,
                None,
                journal,
                |_, _, _| {
                    Ok(objects
                        .into_iter()
                        .map(|object| RulesOccurrence::LibraryCardRevealed { object })
                        .collect())
                },
            )?;
            Box::new(RevealedPiles::new(cards))
        };

        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let opponent = if target_opponent {
            let Some(resolving) = self.window.resolving() else {
                return Err(EffectError::WrongResolutionFrame);
            };
            let TargetRef::Player(opp) = self.instruction_target(resolving)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            opp
        } else {
            match opponents.as_slice() {
                [] => return Ok(None),
                [only] => *only,
                _ => match self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_opponent)
                {
                    Some(opponent) => opponent,
                    None => {
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .revealed_piles = Some(work);
                        return Ok(Some(EffectDecision::OpponentChoice(OpponentChoiceFrame {
                            revision: self.revision,
                            entry,
                            instruction,
                            chooser: player,
                            opponents,
                        })));
                    }
                },
            }
        };
        let chooser = self.window.resolving().map_or(player, |e| e.controller());

        let choice = if let Some(first) = &work.first {
            if work.hand.is_none() {
                let second = work
                    .cards
                    .iter()
                    .copied()
                    .filter(|id| !first.contains(id))
                    .collect();
                let taker = if controller_divides { opponent } else { player };
                Some((
                    taker,
                    PileChoice::Take {
                        first: first.clone(),
                        second,
                    },
                ))
            } else {
                None
            }
        } else {
            let divider = if controller_divides { player } else { opponent };
            Some((divider, PileChoice::Divide(work.cards.clone())))
        };
        if let Some((actor, choice)) = choice {
            let frame = PileChoiceFrame {
                revision: self.revision,
                chooser: actor,
                choice,
            };
            work.offered = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::RevealedPiles(frame)));
        }

        let keep_stays_on_top = matches!(chosen_dest, crate::program::LookTopKeepDest::LibraryTop);
        if work.batches.is_empty() {
            let chosen_zone = match chosen_dest {
                crate::program::LookTopKeepDest::Hand
                | crate::program::LookTopKeepDest::LibraryTop => Zone::Hand,
                crate::program::LookTopKeepDest::Battlefield { .. } => Zone::Battlefield,
                crate::program::LookTopKeepDest::Graveyard => Zone::Graveyard,
                crate::program::LookTopKeepDest::Exile { .. }
                | crate::program::LookTopKeepDest::ExileLinked { .. } => Zone::Exile,
            };
            let chosen = work.hand.clone().ok_or(EffectError::InconsistentProgram)?;
            let remainder: Vec<_> = work
                .cards
                .iter()
                .copied()
                .filter(|id| !chosen.contains(id))
                .collect();
            if other_pile_to_bottom && work.bottom_order.is_none() {
                if remainder.len() > 1 {
                    if matches!(other_dest, crate::program::LookTopRestDest::BottomRandom) {
                        work.pending_random_bottom = Some(remainder);
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .revealed_piles = Some(work);
                        let cursor = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        self.pending_work =
                            Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                                cursor,
                                chooser,
                                options: BTreeSet::new(),
                                rebound_or_buyback: false,
                                cast: None,
                            })));
                        return Ok(None);
                    }
                    let frame = PileChoiceFrame {
                        revision: self.revision,
                        chooser,
                        choice: PileChoice::BottomOrder(remainder),
                    };
                    work.offered = Some(frame.clone());
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .revealed_piles = Some(work);
                    return Ok(Some(EffectDecision::RevealedPiles(frame)));
                }
                work.bottom_order = Some(remainder.clone());
            }
            let rest_zone = match other_dest {
                crate::program::LookTopRestDest::Exile => Zone::Exile,
                crate::program::LookTopRestDest::Hand => Zone::Hand,
                crate::program::LookTopRestDest::Graveyard
                | crate::program::LookTopRestDest::BottomRandom
                | crate::program::LookTopRestDest::BottomOrder
                | crate::program::LookTopRestDest::TopOrder => Zone::Graveyard,
            };
            let rest_cards = if other_pile_to_bottom {
                Vec::new()
            } else {
                remainder
            };
            let kept_for_move = if keep_stays_on_top {
                Vec::new()
            } else {
                chosen
            };
            for (zone, cards) in [(chosen_zone, kept_for_move), (rest_zone, rest_cards)] {
                if !cards.is_empty() {
                    let batch = self.revealed_batch(entry, instruction, chooser, zone, &cards)?;
                    work.batches.push((zone, cards, batch));
                }
            }
        }

        let mut moves = Vec::new();
        let mut graveyard = Vec::new();
        let mut reasons = BTreeMap::new();
        for (zone, cards, batch) in &mut work.batches {
            if let Some(frame) = batch.progress(self.revision)? {
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .revealed_piles = Some(work);
                return Ok(Some(frame));
            }
            batch.verify_initial(self.revealed_batch(
                entry,
                instruction,
                chooser,
                *zone,
                cards,
            )?)?;
            self.prospective_entry_copies.extend(batch.entry_copies());
            self.prospective_entry_controllers
                .extend(batch.entry_controllers());
            for &id in cards.iter() {
                if batch.entry_prohibited(id) {
                    continue;
                }
                let destination = batch.destination(id)?;
                if destination == Zone::Battlefield {
                    let controller = batch.entry_controller(id).unwrap_or(player);
                    if let Some(domain) = self.linked_aura_domain(id, controller, batch.evaluated())
                    {
                        if domain.is_empty() {
                            self.prospective_entry_copies.remove(&id);
                            self.prospective_entry_controllers.remove(&id);
                            continue;
                        }
                        if let Some(host) = self.resolution_aura_choices.get(&id) {
                            if !domain.contains(host) {
                                return Err(EffectError::IllegalTarget);
                            }
                        } else {
                            let frame = LinkedAuraChoiceFrame {
                                revision: self.revision,
                                chooser: controller,
                                object: id,
                                domain,
                            };
                            work.aura_offer = Some(frame.clone());
                            self.resolution_cursor_mut()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .revealed_piles = Some(work);
                            return Ok(Some(EffectDecision::LinkedAuraChoice(frame)));
                        }
                    }
                }
                reasons.insert(
                    id,
                    match zone {
                        Zone::Hand => ZoneReason::LookTopToHand,
                        Zone::Battlefield => ZoneReason::LookTopToBattlefield,
                        Zone::Exile => ZoneReason::ExiledByEffect,
                        _ => ZoneReason::RevealedToGraveyard,
                    },
                );
                if destination == Zone::Graveyard {
                    graveyard.push(id);
                } else {
                    moves.push((
                        id,
                        destination
                            .physical(Some(chooser))
                            .ok_or(EffectError::InconsistentProgram)?,
                    ));
                }
            }
        }
        if graveyard.len() > 1 && work.graveyard_order.is_none() {
            let frame = PileChoiceFrame::graveyard_order(self.revision, player, graveyard);
            work.offered = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::RevealedPiles(frame)));
        }
        if let Some((&(id, _), frame)) = moves
            .iter()
            .find(|(_, destination)| matches!(destination, ObjectDestination::Battlefield { .. }))
            .map(|movement| {
                Ok::<_, EffectError>((
                    movement,
                    self.object_entry_choice_frame(
                        match movement.1 {
                            ObjectDestination::Battlefield { controller } => controller,
                            _ => chooser,
                        },
                        movement.0,
                    )?,
                ))
            })
            .transpose()?
            && let Some(frame) = frame
            && work.entry_choice.is_none()
        {
            debug_assert_eq!(frame.object(), id);
            work.entry_offer = Some(frame.clone());
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .revealed_piles = Some(work);
            return Ok(Some(EffectDecision::EntryChoice(frame)));
        }
        for (_, _, batch) in &work.batches {
            batch.commit_devour(self, journal)?;
        }
        let order = work.graveyard_order.unwrap_or(graveyard);
        moves.extend(
            order
                .into_iter()
                .rev()
                .map(|id| (id, ObjectDestination::Graveyard)),
        );

        let before = self.rules_snapshot(None)?;
        if !self.window.cards().library(player).starts_with(&work.cards) {
            return Err(EffectError::InvalidLibraryPartition);
        }
        let mut movements = if moves.is_empty() {
            Vec::new()
        } else {
            let (window, movements) = self.window.record_library_search_moves(
                self.window.revision(),
                entry,
                player,
                chooser,
                &moves,
            )?;
            self.window = window;
            movements
        };
        let rest_is_top = matches!(other_dest, crate::program::LookTopRestDest::TopOrder);
        if keep_stays_on_top {
            let kept_top = work.hand.clone().unwrap_or_default();
            if let Some(rest_order) = work.bottom_order {
                if rest_is_top {
                    let mut combined = kept_top;
                    combined.extend(rest_order);
                    self.top_remaining_looked_at(player, entry, &combined)?;
                } else {
                    let total = kept_top.len() + rest_order.len();
                    if total > 0 {
                        let top_slice = self
                            .window
                            .cards()
                            .library(player)
                            .get(..total)
                            .ok_or(EffectError::InvalidLibraryPartition)?
                            .to_vec();
                        let (window, _) = self.window.record_library_top(
                            self.window.revision(),
                            entry,
                            crate::stack_window::LibraryTopCommit {
                                player,
                                frozen: &top_slice,
                                top: &kept_top,
                                bottom: &rest_order,
                                moves_application: &[],
                            },
                        )?;
                        self.window = window;
                    }
                }
            } else if !kept_top.is_empty() {
                self.top_remaining_looked_at(player, entry, &kept_top)?;
            }
        } else if let Some(bottom) = work.bottom_order {
            if rest_is_top {
                self.top_remaining_looked_at(player, entry, &bottom)?;
            } else {
                self.bottom_remaining_looked_at(player, entry, &bottom)?;
            }
        }

        let mut counters = Vec::new();
        for movement in &mut movements {
            if movement.after.zone() != ObjectZone::Battlefield {
                continue;
            }
            let id = movement.after.id();
            if let Some(choice) = work.entry_choice {
                self.resolution_entry_choices
                    .insert(movement.before.id(), choice);
            }
            self.stage_resolution_entry_choice(movement.before.id(), id)?;
            self.characteristics.register_battlefield_entry(id)?;
            if matches!(
                chosen_dest,
                crate::program::LookTopKeepDest::Battlefield { tapped: true, .. }
            ) {
                self.window.enter_tapped(id)?;
            }
            if matches!(
                chosen_dest,
                crate::program::LookTopKeepDest::Battlefield {
                    attacking: true,
                    ..
                }
            ) && self.combat.is_some()
                && self.window.cards().get(id).and_then(GameObject::controller)
                    == Some(self.window.active_player())
                && let Ok(current_eval) = self.evaluated_characteristics()
                && let Ok(defenders) =
                    self.attack_destinations(self.window.active_player(), &current_eval)
                && let Some((&target, &seat)) = defenders.first_key_value()
            {
                let _ = self.put_attacking_at(id, seat, target);
            }
            if let Some(host) = self.resolution_aura_choices.remove(&movement.before.id()) {
                self.characteristics.attach(id, host, true)?;
            }
            let added =
                self.apply_entry_counters(&before, id, PowerToughnessCounters::default(), 0)?;
            movement.after = self
                .window
                .cards()
                .get(id)
                .cloned()
                .ok_or(EffectError::InconsistentProgram)?;
            if added.valid_placement() {
                counters.push((movement.before.id(), id, added));
            }
        }

        if let crate::program::LookTopKeepDest::ExileLinked {
            face_down: _,
            playable,
            free,
            cast_only,
            any_color,
            duration,
        }
        | crate::program::LookTopKeepDest::Exile {
            playable,
            free,
            cast_only,
            any_color,
            duration,
        } = chosen_dest
        {
            let link_to_source = matches!(
                chosen_dest,
                crate::program::LookTopKeepDest::ExileLinked { .. }
            );
            let face_down = matches!(
                chosen_dest,
                crate::program::LookTopKeepDest::ExileLinked {
                    face_down: true,
                    ..
                }
            );
            if link_to_source || playable {
                let kept_before: BTreeSet<ObjectId> = work
                    .hand
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .copied()
                    .collect();
                let source_obj = link_to_source
                    .then(|| {
                        self.window.resolving().and_then(|e| match e.kind() {
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => Some(source.id()),
                            StackKind::CardSpell(id) => Some(*id),
                            _ => None,
                        })
                    })
                    .flatten();
                let play_duration = match duration {
                    crate::characteristics::ResolvedDuration::UntilEndOfTurn
                    | crate::characteristics::ResolvedDuration::UntilEndOfCombat => {
                        CardPlayDuration::ThroughTurn
                    }
                    crate::characteristics::ResolvedDuration::ThroughControllerNextTurn
                    | crate::characteristics::ResolvedDuration::UntilControllerNextTurn
                    | crate::characteristics::ResolvedDuration::ThroughNextTurn { .. }
                    | crate::characteristics::ResolvedDuration::UntilPlayerNextTurn { .. } => {
                        if self.window.active_player() == chooser {
                            CardPlayDuration::ThroughNextOwnTurnSkipCurrent(chooser)
                        } else {
                            CardPlayDuration::ThroughNextOwnTurn(chooser)
                        }
                    }
                    _ => CardPlayDuration::WhileInZone,
                };
                for movement in &movements {
                    if kept_before.contains(&movement.before.id())
                        && movement.after.zone() == ObjectZone::Exile
                        && movement.after.card_id().is_some()
                    {
                        if face_down {
                            self.face_down_exile.insert(movement.after.id(), chooser);
                        }
                        if let Some(src_id) = source_obj {
                            self.characteristics
                                .linked_exiles
                                .entry(src_id)
                                .or_default()
                                .push(movement.after.id());
                        }
                        if playable {
                            self.card_play_permissions.insert(CardPlayPermission {
                                zone: CardPlayZone::Exile,
                                object: movement.after.id(),
                                player: chooser,
                                duration: play_duration,
                                any_color,
                                free,
                                cast_only,
                            });
                        }
                    }
                }
            }
        }

        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .chosen_opponent = None;
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            journal,
            |previous, current, controllers| {
                let mut rows = movements
                    .iter()
                    .map(|movement| {
                        zone_occurrence_at(
                            previous,
                            current,
                            movement,
                            *reasons
                                .get(&movement.before.id())
                                .ok_or(EffectError::InconsistentProgram)?,
                            controllers,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (old, id, added) in counters {
                    rows.push(RulesOccurrence::CountersPlaced {
                        object: id,
                        before: previous.characteristics.counters(id),
                        added,
                        after: current.characteristics.counters(id),
                        before_appearance: Box::new(
                            previous
                                .appearance(old)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(id)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )?;
        Ok(None)
    }

    pub(super) fn resume_pile_choice(
        &self,
        expected: u64,
        frame: &PileChoiceFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        if work.offered.take().as_ref() != Some(frame) || frame.revision != self.revision {
            return Err(EffectError::WrongResolutionFrame);
        }
        match (&frame.choice, reply) {
            (
                PileChoice::Divide(domain) | PileChoice::DividePermanents(domain),
                DecisionReply::SelectResolution(selected),
            ) if selected.iter().all(|id| domain.contains(id))
                && selected.iter().collect::<BTreeSet<_>>().len() == selected.len() =>
            {
                work.first = Some(selected)
            }
            (
                PileChoice::Take { first, second } | PileChoice::Sacrifice { first, second },
                DecisionReply::ChoosePile(index @ (1 | 2)),
            ) => work.hand = Some(if index == 1 { first } else { second }.clone()),
            (PileChoice::BottomOrder(domain), DecisionReply::OrderResolutionGraveyard(order))
                if order.len() == domain.len()
                    && order.iter().collect::<BTreeSet<_>>() == domain.iter().collect() =>
            {
                work.bottom_order = Some(order);
            }
            (PileChoice::GraveyardOrder(_), DecisionReply::OrderResolutionGraveyard(order)) => {
                frame.validate_graveyard_order(&order)?;
                work.graveyard_order = Some(order)
            }
            _ => return Err(EffectError::InvalidLibraryPartition),
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

    pub(super) fn resume_pile_entry(
        &self,
        expected: u64,
        frame: &EntryChoiceFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::EntryChoice(choice) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if frame.chooser() != actor || !self.valid_entry_choice(frame, choice) {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        if work.entry_offer.take().as_ref() != Some(frame) || frame.revision() != expected {
            return Err(EffectError::WrongResolutionFrame);
        }
        work.entry_choice = Some(choice);
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    pub(super) fn resume_pile_replacement(
        &self,
        expected: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let DecisionReply::ZoneReplacement(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let work = next
            .resolution_cursor_mut()
            .and_then(|cursor| cursor.revealed_piles.as_mut())
            .ok_or(EffectError::WrongResolutionFrame)?;
        let (_, _, batch) = work
            .batches
            .iter_mut()
            .find(|(_, _, batch)| batch.frame(expected).as_ref() == Some(frame))
            .ok_or(EffectError::WrongResolutionFrame)?;
        batch.accept(expected, frame, actor, reply)?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
}
