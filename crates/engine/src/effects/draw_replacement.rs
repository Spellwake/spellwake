//! Draw-event replacements apply to a multi-card instruction before its individual draws.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::ZoneActionKind;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawReplacementDomain {
    /// Several opponents could replace the same multiple draw.
    Collector(Vec<SeatId>),
    /// Draw normally, or reveal until finding a land or a nonland.
    LandKind,
    /// An exact top card known only to the drawing player until their reply.
    Miracle { card: ObjectId },
    /// All of these publicly revealed cards go to the bottom in the player's order.
    Bottom(Vec<ObjectId>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawReplacementFrame {
    pub revision: u64,
    pub chooser: SeatId,
    pub domain: DrawReplacementDomain,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawReplacementChoice {
    Collector(SeatId),
    Draw,
    Miracle(bool),
    Land,
    Nonland,
    Bottom(Vec<ObjectId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    player: SeatId,
    count: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Revealed {
    frozen: Vec<ObjectId>,
    found: Option<ObjectId>,
    bottom: Vec<ObjectId>,
    ordered: bool,
    replacement: Option<ZoneReplacementBatch>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DrawReplacement {
    origin: DrawOrigin,
    finished: BTreeMap<SeatId, RulesInteger>,
    original: Option<Chunk>,
    queue: VecDeque<Chunk>,
    journal: DrawJournal,
    decision: Option<DrawReplacementFrame>,
    reply: Option<DrawReplacementChoice>,
    revealed: Option<Revealed>,
    miracle: Option<ObjectId>,
    body_decision: Option<OptionalEffectFrame>,
    pending_body: Option<event_replacement::EventBodyReplacement>,
    body_reply: Option<OptionalEffectChoice>,
    declined_body: BTreeSet<(Option<ObjectId>, crate::characteristics::StaticAbilityId)>,
}
impl DrawReplacement {
    fn new(origin: DrawOrigin) -> Self {
        Self {
            origin,
            finished: BTreeMap::new(),
            original: None,
            queue: VecDeque::new(),
            journal: DrawJournal::default(),
            decision: None,
            reply: None,
            revealed: None,
            miracle: None,
            body_decision: None,
            pending_body: None,
            body_reply: None,
            declined_body: BTreeSet::new(),
        }
    }
    fn spend_one(&mut self) -> Result<(), EffectError> {
        let chunk = self
            .queue
            .front_mut()
            .ok_or(EffectError::InconsistentProgram)?;
        chunk.count = chunk.count.minus(&1.into());
        if !chunk.count.is_positive() {
            self.queue.pop_front();
        }
        self.declined_body.clear();
        Ok(())
    }
    fn prompt(
        &mut self,
        revision: u64,
        chooser: SeatId,
        domain: DrawReplacementDomain,
    ) -> EffectDecision {
        let frame = DrawReplacementFrame {
            revision,
            chooser,
            domain,
        };
        self.decision = Some(frame.clone());
        EffectDecision::DrawReplacement(frame)
    }
}
impl EffectWindow {
    pub(super) fn requested_draw_count(&self, player: SeatId) -> Option<RulesInteger> {
        let work = self.draw_replacement.as_ref()?;
        work.original
            .as_ref()
            .filter(|chunk| chunk.player == player)
            .map(|chunk| chunk.count.clone())
            .or_else(|| work.finished.get(&player).cloned())
    }
    /// Library cards currently revealed to every viewer while ordering a replaced draw.
    pub fn draw_reveals(&self, owner: SeatId) -> &[ObjectId] {
        self.draw_replacement
            .as_ref()
            .filter(|w| w.queue.front().is_some_and(|chunk| chunk.player == owner))
            .and_then(|w| w.revealed.as_ref())
            .map_or(&[], |r| r.frozen.as_slice())
    }
    pub(crate) fn take_draw_receipts(&mut self) -> DrawJournal {
        self.draw_replacement
            .as_mut()
            .map_or_else(DrawJournal::default, |work| {
                std::mem::take(&mut work.journal)
            })
    }
    pub(crate) fn draw_decision(&self) -> Option<EffectDecision> {
        let work = self.draw_replacement.as_ref()?;
        if let Some(frame) = &work.body_decision {
            return Some(EffectDecision::OptionalEffect(frame.clone()));
        }
        if let Some(frame) = &work.decision {
            return Some(EffectDecision::DrawReplacement(frame.clone()));
        }
        work.revealed
            .as_ref()?
            .replacement
            .as_ref()?
            .frame(self.revision)
            .map(EffectDecision::ZoneReplacement)
    }
    pub(crate) fn accept_draw_decision(
        &mut self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<(), EffectError> {
        let revision = self.next_revision(expected)?;
        if self.draw_decision().as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        let work = self
            .draw_replacement
            .as_mut()
            .ok_or(EffectError::InconsistentProgram)?;
        match (frame, reply) {
            (EffectDecision::DrawReplacement(frame), DecisionReply::DrawReplacement(reply)) => {
                let valid = match (&frame.domain, &reply) {
                    (DrawReplacementDomain::Miracle { .. }, DrawReplacementChoice::Miracle(_)) => {
                        true
                    }
                    (
                        DrawReplacementDomain::Collector(seats),
                        DrawReplacementChoice::Collector(seat),
                    ) => seats.contains(seat),
                    (
                        DrawReplacementDomain::LandKind,
                        DrawReplacementChoice::Draw
                        | DrawReplacementChoice::Land
                        | DrawReplacementChoice::Nonland,
                    ) => true,
                    (
                        DrawReplacementDomain::Bottom(domain),
                        DrawReplacementChoice::Bottom(order),
                    ) => {
                        domain.len() == order.len()
                            && order.iter().copied().collect::<BTreeSet<_>>()
                                == domain.iter().copied().collect()
                    }
                    _ => false,
                };
                if !valid {
                    return Err(EffectError::InvalidLibraryPartition);
                }
                work.reply = Some(reply);
                work.decision = None;
            }
            (EffectDecision::OptionalEffect(_), DecisionReply::OptionalEffect(choice)) => {
                if !matches!(
                    choice,
                    OptionalEffectChoice::Take | OptionalEffectChoice::Decline
                ) {
                    return Err(EffectError::IllegalOptionalEffect);
                }
                work.body_reply = Some(choice);
                work.body_decision = None;
            }
            (EffectDecision::ZoneReplacement(frame), DecisionReply::ZoneReplacement(reply)) => {
                work.revealed
                    .as_mut()
                    .and_then(|r| r.replacement.as_mut())
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .accept(expected, frame, actor, reply)?;
            }
            _ => return Err(EffectError::WrongDecisionKind),
        }
        self.revision = revision;
        Ok(())
    }
    fn draw_replacers(
        &self,
        player: SeatId,
        count: &RulesInteger,
    ) -> Result<(Vec<SeatId>, bool, bool), EffectError> {
        let current = self.evaluated_characteristics()?;
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let collectors = if *count > 1.into() {
            opponents
                .iter()
                .copied()
                .filter(|&seat| {
                    current.controlled_rules(seat, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::DrawRule { rule: crate::characteristics::StaticDrawRule::SplitOpponentsMultipleDraw, .. }
                        )
                    }) > 0
                })
                .collect()
        } else {
            vec![]
        };
        let abundance = current.controlled_rules(player, |rule| {
            matches!(
                rule,
                crate::characteristics::StaticContinuousAbility::DrawRule {
                    rule: crate::characteristics::StaticDrawRule::ReplaceWithLandKind,
                    ..
                }
            )
        }) > 0;
        let prohibited = current.objects.values().any(|perm| {
            perm.zone == ObjectZone::Battlefield
                && perm.has_battlefield_rule(|rule| match rule {
                    crate::characteristics::StaticContinuousAbility::DrawRule {
                        rule: crate::characteristics::StaticDrawRule::PlayersCannotDraw,
                        ..
                    } => true,
                    crate::characteristics::StaticContinuousAbility::DrawRule {
                        rule: crate::characteristics::StaticDrawRule::OpponentsCannotDraw,
                        ..
                    } => perm.controller.is_some_and(|c| opponents.contains(&c)),
                    _ => false,
                })
        }) || (self.cards_drawn_this_turn.contains_key(&player)
            && opponents.iter().any(|&seat| {
                current.controlled_rules(seat, |rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticContinuousAbility::DrawRule {
                            rule: crate::characteristics::StaticDrawRule::OpponentsAtMostOne,
                            ..
                        }
                    )
                }) > 0
            }));
        Ok((collectors, abundance, prohibited))
    }
    pub(crate) fn draw_with_replacements(
        &mut self,
        player: SeatId,
        count: RulesInteger,
        origin: DrawOrigin,
        append: &mut RulesJournal,
    ) -> Result<Result<DrawJournal, EffectDecision>, EffectError> {
        if let DrawOrigin::Instruction { entry, .. } = origin
            && self
                .window
                .resolving()
                .is_some_and(|resolving| resolving.id() == entry)
            && count >= 0.into()
            && self.window.original_participants().contains(&player)
            && !self.window.participants().contains(&player)
        {
            // The instruction continues without a draw event or an empty-library loss.
            return Ok(Ok(DrawJournal::default()));
        }
        if self
            .draw_replacement
            .as_ref()
            .is_some_and(|work| work.origin != origin && work.original.is_none())
        {
            self.draw_replacement = None;
        }
        if self.draw_replacement.is_none()
            && !self.has_draw_body_replacement_source()
            && !self.programs.values().any(|program| {
                program.miracle.is_some() || program
                    .continuous
                    .iter()
                    .chain(&program.additional_battlefield_abilities)
                    .any(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::DrawRule { rule: crate::characteristics::StaticDrawRule::SplitOpponentsMultipleDraw, .. }
                                | crate::characteristics::StaticContinuousAbility::DrawRule { rule: crate::characteristics::StaticDrawRule::ReplaceWithLandKind, .. }
                        )
                    })
            })
        {
            return Ok(Ok(self.draw_at_cuts(player, count, origin, append)?));
        }
        self.window
            .validate_draw(self.window.revision(), player, &count)?;
        let mut work = self
            .draw_replacement
            .take()
            .unwrap_or_else(|| Box::new(DrawReplacement::new(origin)));
        if work.origin != origin {
            return Err(EffectError::InconsistentProgram);
        }
        if work.finished.contains_key(&player) {
            self.draw_replacement = Some(work);
            return Ok(Ok(DrawJournal::default()));
        }
        if work.original.is_none() {
            let chunk = Chunk { player, count };
            work.original = Some(chunk.clone());
            if chunk.count.is_positive() {
                work.queue.push_back(chunk);
            }
        }
        let pending = self.continue_replaced_draws(&mut work, append)?;
        let result = if let Some(frame) = pending {
            Err(frame)
        } else {
            let original = work
                .original
                .take()
                .ok_or(EffectError::InconsistentProgram)?;
            work.finished.insert(original.player, original.count);
            Ok(std::mem::take(&mut work.journal))
        };
        self.draw_replacement = Some(work);
        Ok(result)
    }
    fn continue_replaced_draws(
        &mut self,
        work: &mut DrawReplacement,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        while self.won_by_effect.is_none()
            && let Some(chunk) = work.queue.front().cloned()
        {
            let player = chunk.player;
            let cause = EventCause::Draw {
                player,
                origin: work.origin,
            };
            if let Some(rep) = work.pending_body.take() {
                let Some(choice) = work.body_reply.take() else {
                    return Err(EffectError::WrongDecisionKind);
                };
                match choice {
                    OptionalEffectChoice::Take => {
                        work.spend_one()?;
                        self.execute_inline_replacement_body(&rep, cause, append)?;
                    }
                    OptionalEffectChoice::Decline => {
                        work.declined_body.insert((rep.source, rep.ability));
                    }
                    _ => return Err(EffectError::IllegalOptionalEffect),
                }
                continue;
            }
            if let Some(card) = work.miracle.take() {
                let Some(DrawReplacementChoice::Miracle(reveal)) = work.reply.take() else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if self.window.cards().library(player).first() != Some(&card) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let physical = self
                    .window
                    .cards()
                    .get(card)
                    .and_then(GameObject::card_id)
                    .ok_or(EffectError::UnknownObject)?;
                let drawn = self.draw_at_cuts(player, 1.into(), work.origin, append)?;
                if reveal {
                    let drawn_id = self
                        .window
                        .cards()
                        .current_identity(physical)
                        .ok_or(EffectError::UnknownObject)?;
                    let before = self.rules_snapshot(None)?;
                    let object = before
                        .view()
                        .appearance(drawn_id)
                        .ok_or(EffectError::UnknownObject)?;
                    if object.object.zone() != ObjectZone::Hand(player) {
                        return Err(EffectError::InconsistentProgram);
                    }
                    self.emit_from(cause, &before, None, append, |_, _, _| {
                        Ok(vec![RulesOccurrence::MiracleRevealed {
                            object: Box::new(object),
                        }])
                    })?;
                }
                work.journal.append(drawn);
                work.spend_one()?;
                continue;
            }
            if let Some(mut shown) = work.revealed.take() {
                if !shown.ordered {
                    if let Some(DrawReplacementChoice::Bottom(order)) = work.reply.take() {
                        shown.bottom = order;
                        shown.ordered = true;
                    } else {
                        let frame = work.prompt(
                            self.revision,
                            player,
                            DrawReplacementDomain::Bottom(shown.bottom.clone()),
                        );
                        work.revealed = Some(shown);
                        return Ok(Some(frame));
                    }
                }
                if let Some(batch) = &mut shown.replacement
                    && let Some(frame) = batch.progress(self.revision)?
                {
                    work.revealed = Some(shown);
                    return Ok(Some(frame));
                }
                let mut moves = Vec::new();
                if let Some(id) = shown.found {
                    let batch = shown
                        .replacement
                        .as_ref()
                        .ok_or(EffectError::InconsistentProgram)?;
                    batch.verify_initial(self.revealed_draw_batch(cause.clone(), id)?)?;
                    moves.push(crate::stack_window::LibraryTopMove {
                        before: id,
                        destination: batch.destination(id)?,
                    });
                }
                let before = self.rules_snapshot(None)?;
                let (window, movements) = self.window.record_revealed_top(
                    self.window.revision(),
                    crate::stack_window::LibraryTopCommit {
                        player,
                        frozen: &shown.frozen,
                        top: &[],
                        bottom: &shown.bottom,
                        moves_application: &moves,
                    },
                )?;
                self.window = window;
                if !movements.is_empty() {
                    self.emit_from(
                        cause.clone(),
                        &before,
                        None,
                        append,
                        |old, new, controllers| {
                            movements
                                .iter()
                                .map(|m| {
                                    zone_occurrence_at(
                                        old,
                                        new,
                                        m,
                                        ZoneReason::LookTopToHand,
                                        controllers,
                                    )
                                })
                                .collect()
                        },
                    )?;
                }
                work.spend_one()?;
                continue;
            }
            let (collectors, abundance, prohibited) = self.draw_replacers(player, &chunk.count)?;
            if !collectors.is_empty() {
                let collector = if collectors.len() == 1 {
                    collectors[0]
                } else if let Some(DrawReplacementChoice::Collector(seat)) = work.reply.take() {
                    seat
                } else {
                    return Ok(Some(work.prompt(
                        self.revision,
                        player,
                        DrawReplacementDomain::Collector(collectors),
                    )));
                };
                work.queue.pop_front();
                let seats = self
                    .window
                    .apnap_players()?
                    .filter(|&seat| seat == player || seat == collector)
                    .collect::<Vec<_>>();
                for seat in seats.into_iter().rev() {
                    work.queue.push_front(Chunk {
                        player: seat,
                        count: 1.into(),
                    });
                }
                continue;
            }
            if prohibited {
                work.queue.pop_front();
                continue;
            }
            if !matches!(work.origin, DrawOrigin::Opening) {
                let current = self.evaluated_characteristics()?;
                if let Some(rep) = self.find_event_body_replacement(
                    &current,
                    event_replacement::ReplacementEventKind::Draw,
                    player,
                    &work.declined_body,
                )? {
                    if rep.optional {
                        let frame = OptionalEffectFrame::event_replacement(self.revision, player);
                        work.pending_body = Some(rep);
                        work.body_decision = Some(frame.clone());
                        return Ok(Some(EffectDecision::OptionalEffect(frame)));
                    }
                    work.spend_one()?;
                    self.execute_inline_replacement_body(&rep, cause, append)?;
                    continue;
                }
            }
            if !abundance {
                if let Some(frame) = self.offer_miracle(player, work) {
                    return Ok(Some(frame));
                }
                if !work.declined_body.is_empty() && chunk.count > 1.into() {
                    work.journal.append(self.draw_at_cuts(
                        player,
                        1.into(),
                        work.origin,
                        append,
                    )?);
                    work.spend_one()?;
                } else {
                    work.queue.pop_front();
                    work.declined_body.clear();
                    work.journal.append(self.draw_at_cuts(
                        player,
                        chunk.count,
                        work.origin,
                        append,
                    )?);
                }
                continue;
            }
            let land = match work.reply.take() {
                Some(DrawReplacementChoice::Draw) => {
                    if let Some(frame) = self.offer_miracle(player, work) {
                        return Ok(Some(frame));
                    }
                    work.journal.append(self.draw_at_cuts(
                        player,
                        1.into(),
                        work.origin,
                        append,
                    )?);
                    work.spend_one()?;
                    continue;
                }
                Some(DrawReplacementChoice::Land) => true,
                Some(DrawReplacementChoice::Nonland) => false,
                None => {
                    return Ok(Some(work.prompt(
                        self.revision,
                        player,
                        DrawReplacementDomain::LandKind,
                    )));
                }
                _ => return Err(EffectError::WrongDecisionKind),
            };
            let current = self.evaluated_characteristics()?;
            let mut frozen = Vec::new();
            let mut found = None;
            let mut reveals = Vec::new();
            for &id in self.window.cards().library(player) {
                let card = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::UnknownObject)?;
                frozen.push(id);
                reveals.push(RulesOccurrence::LibraryCardRevealed {
                    object: card.clone(),
                });
                if current
                    .get(id)
                    .ok_or(EffectError::UnknownObject)?
                    .has_type(CardType::Land)
                    == land
                {
                    found = Some(id);
                    break;
                }
            }
            if !reveals.is_empty() {
                let before = self.rules_snapshot(None)?;
                self.emit_from(cause.clone(), &before, None, append, |_, _, _| Ok(reveals))?;
            }
            if frozen.is_empty() {
                work.spend_one()?;
                continue;
            }
            let bottom = frozen
                .iter()
                .copied()
                .filter(|id| Some(*id) != found)
                .collect::<Vec<_>>();
            let replacement = found
                .map(|id| self.revealed_draw_batch(cause.clone(), id))
                .transpose()?;
            work.revealed = Some(Revealed {
                frozen,
                found,
                ordered: bottom.len() < 2,
                bottom,
                replacement,
            });
        }
        Ok(None)
    }
    fn offer_miracle(&self, player: SeatId, work: &mut DrawReplacement) -> Option<EffectDecision> {
        if matches!(work.origin, DrawOrigin::Opening)
            || self.cards_drawn_this_turn.contains_key(&player)
        {
            return None;
        }
        let &card = self.window.cards().library(player).first()?;
        self.copiable_program(card)?.miracle.as_ref()?;
        work.miracle = Some(card);
        Some(work.prompt(
            self.revision,
            player,
            DrawReplacementDomain::Miracle { card },
        ))
    }

    fn revealed_draw_batch(
        &self,
        cause: EventCause,
        object: ObjectId,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause,
            action: ZoneActionKind::EffectLookToHand,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: object,
                reason: ZoneReason::LookTopToHand,
            }],
        })
    }
}
