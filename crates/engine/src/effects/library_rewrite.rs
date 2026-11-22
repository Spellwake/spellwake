//! Multiplayer library movement retains the owners, former controllers and exact exiled cards.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::ZoneActionKind,
    stack_window::StackEntry,
};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryRewrite {
    pub(super) owners: VecDeque<SeatId>,
    pub(super) controllers: VecDeque<SeatId>,
    pub(super) spells: Vec<(SeatId, ObjectId)>,
    pub(super) lands: Vec<ObjectId>,
    pub(super) finished_exiling: bool,
}
impl LibraryRewrite {
    pub(super) fn finish_player(&mut self) -> Result<(), EffectError> {
        self.controllers
            .pop_front()
            .ok_or(EffectError::InconsistentProgram)?;
        self.lands.clear();
        self.finished_exiling = false;
        Ok(())
    }
}
impl EffectWindow {
    pub(super) fn capture_library_rewrite(
        &mut self,
        targets: &BTreeSet<ObjectId>,
    ) -> Result<(), EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let mut owners = BTreeSet::new();
        let mut controllers = BTreeSet::new();
        for id in targets {
            owners.insert(
                self.window
                    .cards()
                    .get(*id)
                    .ok_or(EffectError::UnknownObject)?
                    .owner(),
            );
            controllers.insert(
                evaluated
                    .get(*id)
                    .and_then(|view| view.controller)
                    .ok_or(EffectError::InconsistentProgram)?,
            );
        }
        let rewrite = LibraryRewrite {
            owners: self
                .window
                .apnap_players()?
                .filter(|seat| owners.contains(seat))
                .collect(),
            controllers: self
                .window
                .apnap_players()?
                .filter(|seat| controllers.contains(seat))
                .collect(),
            spells: vec![],
            lands: vec![],
            finished_exiling: false,
        };
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .impulse
            .rewrite = Some(Box::new(rewrite));
        Ok(())
    }
    pub(super) fn resolving_cast_actor(
        &self,
        instruction: &Instruction,
    ) -> Result<SeatId, EffectError> {
        if let Instruction::CastFoundSpell(id) = instruction
            && let Some(rewrite) = self
                .resolution_cursor()
                .and_then(|c| c.impulse.rewrite.as_ref())
        {
            return rewrite
                .spells
                .iter()
                .find(|(_, spell)| spell == id)
                .map(|(seat, _)| *seat)
                .ok_or(EffectError::InconsistentProgram);
        }
        self.resolving_player(crate::program::ResolvingPlayer::Controller)
    }
    pub(super) fn progress_library_rewrite(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let rewrite = cursor
            .impulse
            .rewrite
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        if let Some(&owner) = rewrite.owners.front() {
            return self.begin_shuffle(owner, true);
        }
        let Some(&player) = rewrite.controllers.front() else {
            let spells = rewrite
                .spells
                .iter()
                .map(|(_, id)| Instruction::CastFoundSpell(*id))
                .collect::<Vec<_>>();
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.expand_instruction(index, true, spells);
            return self.advance_instruction();
        };
        if !rewrite.finished_exiling && !self.window.cards().library(player).is_empty() {
            self.begin_player_impulse(entry, index, player, false)?;
            return Ok(());
        }
        if rewrite.lands.len() <= 1 {
            return self.begin_rewrite_bottom(rewrite.lands.clone());
        }
        let cursor = cursor.clone();
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
            cursor,
            chooser: player,
            options: BTreeSet::new(),
            rebound_or_buyback: false,
            cast: None,
        })));
        Ok(())
    }
    pub(super) fn rewrite_bottom_batch(
        &self,
        cursor: &ResolutionCursor,
        objects: &[ObjectId],
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action: ZoneActionKind::EffectPutIntoLibrary,
            intents: objects
                .iter()
                .map(|before| zone_replacement::ZoneMoveIntent {
                    before: *before,
                    reason: ZoneReason::ExiledCardsToLibraryBottom,
                })
                .collect(),
        })
    }
    pub(super) fn begin_rewrite_bottom(
        &mut self,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let rewrite = cursor
            .impulse
            .rewrite
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        if objects.len() != rewrite.lands.len()
            || objects.iter().collect::<BTreeSet<_>>() != rewrite.lands.iter().collect()
        {
            return Err(EffectError::InconsistentProgram);
        }
        if objects.is_empty() {
            return self
                .resolution_cursor_mut()
                .and_then(|c| c.impulse.rewrite.as_mut())
                .ok_or(EffectError::InconsistentProgram)?
                .finish_player();
        }
        let batch = self.rewrite_bottom_batch(cursor, &objects)?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ImpulseReplacement(Box::new(
            ImpulseReplacementWork {
                cursor,
                objects,
                batch,
                play_permission: None,
                face_down_viewer: None,
                return_to_hand: false,
                bottom_from_exile: true,
                graveyard_orders: BTreeMap::new(),
            },
        )));
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KinReveal {
    pub(super) rest: Vec<ObjectId>,
    found: Option<ObjectId>,
}
impl EffectWindow {
    pub(super) fn combat_contributors(&self) -> Result<BTreeSet<ObjectId>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let current = self.evaluated_characteristics()?;
        let mut eligible = BTreeSet::new();
        for cause in binding.causes.members() {
            let crate::occurrence::TriggerRelated::CombatDamageToPlayer { source, .. } =
                cause.related
            else {
                return Err(EffectError::InconsistentProgram);
            };
            if !current.sacrifice_prohibited(source)
                && current.get(source).is_some_and(|card| {
                    card.zone == ObjectZone::Battlefield
                        && card.controller == Some(entry.controller())
                })
            {
                eligible.insert(source);
            }
        }
        Ok(eligible)
    }
    pub(super) fn begin_kin_reveal(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(types) = &cursor.impulse.sacrificed_types else {
            return self.advance_instruction();
        };
        let current = self.evaluated_characteristics()?;
        let mut reveals = Vec::new();
        let mut rest = Vec::new();
        let mut found = None;
        for &id in self.window.cards().library(entry.controller()) {
            let card = current.get(id).ok_or(EffectError::UnknownObject)?;
            let object = self
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?
                .clone();
            reveals.push(crate::occurrence::RulesOccurrence::LibraryCardRevealed { object });
            if card.is_creature() && !card.current_creature_subtypes.is_disjoint(types) {
                found = Some(id);
                break;
            }
            rest.push(id);
        }
        let before = self.rules_snapshot(None)?;
        if !reveals.is_empty() {
            self.emit_from(
                EventCause::Instruction {
                    entry: entry.id(),
                    instruction,
                },
                &before,
                None,
                append,
                |_, _, _| Ok(reveals),
            )?;
        }
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        cursor.impulse.kin_reveal = Some(Box::new(KinReveal { found, rest }));
        let kin = cursor
            .impulse
            .kin_reveal
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?;
        if kin.rest.len() <= 1 {
            let order = kin.rest.clone();
            return self.finish_kin_reveal(order, append);
        }
        let work = OptionalEffectWork {
            cursor: cursor.clone(),
            chooser: entry.controller(),
            options: BTreeSet::new(),
            rebound_or_buyback: false,
            cast: None,
        };
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(work)));
        Ok(())
    }
    pub(super) fn finish_kin_reveal(
        &mut self,
        order: Vec<ObjectId>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let player = self.resolving_player(crate::program::ResolvingPlayer::Controller)?;
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = cursor.entry;
        let instruction = cursor.instruction;
        let kin = cursor
            .impulse
            .kin_reveal
            .take()
            .ok_or(EffectError::InconsistentProgram)?;
        if order.len() != kin.rest.len()
            || order.iter().collect::<BTreeSet<_>>() != kin.rest.iter().collect()
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        if let Some(object) = kin.found {
            if self.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram);
            }
            self.linked_return_work = Some(LinkedReturnWork {
                origin: LinkedReturnOrigin::LibraryTop {
                    player,
                    entry,
                    instruction,
                    tapped: false,
                },
                objects: vec![object],
                library_bottom: Some(order),
                wake_entry: None,
                orders: BTreeMap::new(),
                graveyard_orders: BTreeMap::new(),
                choices: BTreeMap::new(),
                auras: BTreeMap::new(),
                replacements: None,
            });
            self.finish_linked_return_if_ready(append)?;
        } else {
            self.bottom_remaining_looked_at(player, entry, &order)?;
        }
        self.advance_instruction()
    }
}

impl EffectWindow {
    pub(super) fn reveal_top_against_attacker(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let player = entry.controller();
        let Some(&top) = self.window.cards().library(player).first() else {
            return Ok(());
        };
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|binding| binding.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = trigger.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let crate::occurrence::TriggerRelated::Combatant(attacker) = member.related else {
            return Err(EffectError::InconsistentProgram);
        };
        let before = self.rules_snapshot(None)?;
        let object = self
            .window
            .cards()
            .get(top)
            .ok_or(EffectError::UnknownObject)?
            .clone();
        let is_forest = before.view().evaluated.get(top).is_some_and(|card| {
            card.current_land_subtypes
                .contains(&crate::land::LandType::Forest)
        });
        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction,
        };
        self.emit_from(cause, &before, None, append, |_, _, _| {
            Ok(vec![
                crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
            ])
        })?;
        if is_forest && let Some(combat) = &mut self.combat {
            combat.reconcile(|combatant, _| combatant.object() != attacker);
        }
        let (window, _) = self.window.record_library_top(
            self.window.revision(),
            entry.id(),
            crate::stack_window::LibraryTopCommit {
                player,
                frozen: &[top],
                top: &[],
                bottom: &[top],
                moves_application: &[],
            },
        )?;
        self.window = window;
        Ok(())
    }
}

impl EffectWindow {
    pub(super) fn library_cohort(
        &self,
        entry: &StackEntry,
        instruction: usize,
        cohort: crate::program::LibraryCohort,
    ) -> Result<(SeatId, Vec<ObjectId>), EffectError> {
        use crate::program::LibraryCohort;
        match cohort {
            LibraryCohort::BottomCardOfTargetPlayer => {
                let TargetRef::Player(player) = self
                    .instruction_target_at(entry, Some(instruction))?
                    .choice()
                    .target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                Ok((
                    player,
                    self.window
                        .cards()
                        .library(player)
                        .last()
                        .copied()
                        .into_iter()
                        .collect(),
                ))
            }
            LibraryCohort::DefendingPlayerRevealsThroughLand => {
                let player = self.captured_related_player()?;
                let current = self.evaluated_characteristics()?;
                let mut found = Vec::new();
                for &card in self.window.cards().library(player) {
                    found.push(card);
                    if current
                        .get(card)
                        .is_some_and(|view| view.has_type(CardType::Land))
                    {
                        break;
                    }
                }
                Ok((player, found))
            }
        }
    }

    pub(super) fn reveal_library_cohort_and_boost_attacker(
        &mut self,
        entry: &StackEntry,
        cards: &[ObjectId],
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if cards.is_empty() {
            return Ok(());
        }
        let instruction = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .instruction;
        let before = self.rules_snapshot(None)?;
        let revealed = cards
            .iter()
            .map(|&id| {
                let object = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::UnknownObject)?
                    .clone();
                Ok(crate::occurrence::RulesOccurrence::LibraryCardRevealed { object })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| Ok(revealed),
        )?;
        self.modify_group(
            entry,
            instruction,
            crate::program::EffectSubject::TriggeringAttacker,
            Some(crate::characteristics::PowerToughness {
                power: RulesInteger::from_usize(cards.len()),
                toughness: 0.into(),
            }),
            BTreeSet::new(),
            append,
        )
    }
}
