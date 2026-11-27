//! Effect-time choices retain the one resolution cursor and the actual quote.
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence},
    program::ResolvingPlayer,
};
mod chooser_replacement;
pub use chooser_replacement::ReplacementChooserFrame;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CounterPlacement {
    Ordinary,
    Renown,
    Evolve,
    Mentor(ObjectId),
}
#[cfg(test)]
mod tests;

/// The selected outcome of one optional primitive, not trigger placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptionalEffectChoice {
    Take,
    Decline,
    PermanentTypeOrder([CardType; 3]),
    PermanentType(CardType),
}

/// Resolution payment supplies only an allocation, never its payer or cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPaymentReply {
    Pay(ManaPayment),
    Decline,
}

/// Explicit cost truth. A zero payment has a record even without a debit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPaymentOutcome {
    Paid(ManaPaymentRecord),
    Declined,
    /// CR 800.4f prevents the departed player from paying the quote.
    UnpaidDeparted,
}

/// Immutable executor-issued view of an optional primitive's actual domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalEffectFrame {
    rebound_or_buyback: bool,
    cast: Option<ExileCastOffer>,
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    options: BTreeSet<OptionalEffectChoice>,
    token_offer: Option<super::token_offers::TemptingTokenOffer>,
}
impl OptionalEffectFrame {
    /// Optional event-replacement prompt (CR 614.1, e.g. `StaticReplacementAction::ReplaceWithBody`).
    pub(super) fn event_replacement(revision: u64, chooser: SeatId) -> Self {
        Self {
            rebound_or_buyback: false,
            cast: None,
            revision,
            entry: StackEntryId::entry_replacement_creator(),
            instruction: 0,
            chooser,
            options: BTreeSet::from([OptionalEffectChoice::Take, OptionalEffectChoice::Decline]),
            token_offer: None,
        }
    }

    /// The card owner is choosing between two replacements of the same spell exit.
    pub fn rebound_or_buyback(&self) -> bool {
        self.rebound_or_buyback
    }

    /// Spell offered by a resolving suspend trigger, including its legal target choices.
    pub fn suspended_cast(&self) -> Option<&SuspendedCastOffer> {
        self.cast
            .as_ref()
            .filter(|offer| offer.ability == ExileCastAbility::Suspend)
    }
    /// The spell and legal targets offered by a suspend or rebound trigger.
    pub fn exile_cast(&self) -> Option<&ExileCastOffer> {
        self.cast.as_ref()
    }
    /// Executor revision at which this domain was issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving stack entry.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute index of the optional wrapper in that entry's body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Player derived by the owned instruction context.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Offered choices after checking the next primitive's current availability.
    pub fn options(&self) -> &BTreeSet<OptionalEffectChoice> {
        &self.options
    }
    /// Quantity offered to this opponent and the earlier public responses in order.
    pub fn token_offer(&self) -> Option<(&RulesInteger, &[(SeatId, bool)])> {
        self.token_offer
            .as_ref()
            .map(|offer| (&offer.amount, offer.replies.as_slice()))
    }
}

/// Immutable current-revision quote. The owned work retains its continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPaymentFrame {
    variable: bool,
    restricted_colors: Option<[ManaType; 2]>,
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    payer: SeatId,
    cost: ManaCost,
}
impl ResolutionPaymentFrame {
    /// Latest executor revision, including accepted mana-ability prefixes.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Resolving offer entry, independently of its targeted spell or source card.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute index of the payment instruction in its owned body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// The payer is also the only player authorized to answer this quote.
    pub fn chooser(&self) -> SeatId {
        self.payer
    }
    /// Player derived from the resolving instruction's actual context.
    pub fn payer(&self) -> SeatId {
        self.payer
    }
    /// The payer can choose any affordable generic amount including zero.
    pub fn variable(&self) -> bool {
        self.variable
    }
    /// The permissible mana types for a single hybrid payment, when restricted.
    pub fn restricted_colors(&self) -> Option<[ManaType; 2]> {
        self.restricted_colors
    }
    /// Fixed requirements and total quantity retained by the owned payment work.
    pub fn cost(&self) -> &ManaCost {
        &self.cost
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OptionalEffectWork {
    pub(super) rebound_or_buyback: bool,
    pub(super) cast: Option<ExileCastOffer>,
    pub(super) cursor: ResolutionCursor,
    pub(super) chooser: SeatId,
    pub(super) options: BTreeSet<OptionalEffectChoice>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolutionPaymentWork {
    variable: bool,
    defines_x: bool,
    x_factor: RulesInteger,
    record_last_action: bool,
    restricted_colors: Option<[ManaType; 2]>,
    pub(super) cursor: ResolutionCursor,
    pub(super) payer: SeatId,
    cost: ManaCost,
    paid_destination: usize,
    declined_destination: usize,
}
impl ResolutionPaymentWork {
    fn frame(&self, revision: u64) -> ResolutionPaymentFrame {
        ResolutionPaymentFrame {
            variable: self.variable,
            restricted_colors: self.restricted_colors,
            revision,
            entry: self.cursor.entry,
            instruction: self.cursor.instruction,
            payer: self.payer,
            cost: self.cost.clone(),
        }
    }
}

/// Private parent ownership never travels in the public ManaColorChoice token.
// Ephemeral mana-parent continuation stored only while a single color choice is pending.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManaParent {
    CombatPayment(Box<crate::combat::CombatCursor>),
    Casting(StackEntryId),
    Recovery(CastRecoveryFrame, StackEntryId),
    Priority,
    Payment(ResolutionPaymentWork),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManaColorWork {
    pub(super) frame: ManaColorChoice,
    pub(super) parent: ManaParent,
    pub(super) controller_damage: Option<RulesInteger>,
    pub(super) triggered: bool,
    pub(super) activation_controller: Option<SeatId>,
    pub(super) restriction: Option<crate::mana::ManaRestriction>,
    /// Whether a sacrificed-cost ability tapped a land; None uses its live source.
    pub(super) sacrificed_source: Option<SacrificedManaSource>,
}

/// Privileged borrowed continuation evidence; this cannot resume an instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionChoiceState<'a> {
    Optional {
        cursor: ResolutionState<'a>,
        chooser: SeatId,
        options: &'a BTreeSet<OptionalEffectChoice>,
    },
    Payment {
        cursor: ResolutionState<'a>,
        payer: SeatId,
        cost: &'a ManaCost,
        restricted_colors: Option<[ManaType; 2]>,
        paid_destination: usize,
        declined_destination: usize,
        suspended_for_mana_color: bool,
    },
}

/// Discriminant of the sole owned process; payload inspection stays borrowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingWorkKind {
    PermanentEntry,
    CastRecovery,
    BattlefieldEntry,
    SearchEntry,
    /// A targeted return is waiting for replacement choices.
    ZoneMove,
    LandPlay,
    ActivationCost,
    ImpulseReplacement,
    LibraryTop,
    LookTop,
    /// One cursor owns frozen choices, owner orders and a resolving batch.
    ResolutionSelection,
    Resolution,
    OptionalEffect,
    ResolutionPayment,
    ManaColor,
    TokenEntry,
    Damage,
    Checks,
    Combat,
}

impl EffectWindow {
    /// Observe which mutually exclusive process actually owns pending work.
    pub fn pending_work_kind(&self) -> Option<PendingWorkKind> {
        self.pending_work.as_ref().map(|work| match work {
            PendingWork::PermanentEntry(_) => PendingWorkKind::PermanentEntry,
            PendingWork::CastRecovery(_) => PendingWorkKind::CastRecovery,
            PendingWork::BattlefieldEntry(work) => {
                if work.action == crate::program::ZoneActionKind::EffectEnterBattlefield {
                    PendingWorkKind::BattlefieldEntry
                } else {
                    PendingWorkKind::ZoneMove
                }
            }
            PendingWork::SearchEntry(_) => PendingWorkKind::SearchEntry,
            PendingWork::LandPlay(_) => PendingWorkKind::LandPlay,
            PendingWork::ManaSacrifice(_)
            | PendingWork::ActivationCost(_)
            | PendingWork::SelectedActivationCost(_)
            | PendingWork::GraveyardExile(_)
            | PendingWork::GraveyardShuffle(_) => PendingWorkKind::ActivationCost,
            PendingWork::ImpulseReplacement(_) => PendingWorkKind::ImpulseReplacement,
            PendingWork::LibraryTop(_) => PendingWorkKind::LibraryTop,
            PendingWork::LookTop(_) => PendingWorkKind::LookTop,
            PendingWork::ResolutionSelection(_) => PendingWorkKind::ResolutionSelection,
            PendingWork::Resolution(_) => PendingWorkKind::Resolution,
            PendingWork::OptionalEffect(_) => PendingWorkKind::OptionalEffect,
            PendingWork::ResolutionPayment(_) => PendingWorkKind::ResolutionPayment,
            PendingWork::ManaColor(_) => PendingWorkKind::ManaColor,
            PendingWork::TokenEntry(_) => PendingWorkKind::TokenEntry,
            PendingWork::Damage(_) => PendingWorkKind::Damage,
            PendingWork::Checks(_) => PendingWorkKind::Checks,
            PendingWork::Combat(_) => PendingWorkKind::Combat,
        })
    }
    /// Current actual effect-time choice, derived at the latest executor revision.
    /// A color child exposes its own decision until its mana ability completes.
    pub fn pending_resolution_decision(&self) -> Option<EffectDecision> {
        if let Some(frame) = self.pending_life_replacement_decision() {
            return Some(frame);
        }
        match self.pending_work.as_ref() {
            Some(PendingWork::Combat(cursor)) if cursor.attack_payment.is_some() => {
                match &cursor.frame {
                    CombatDecision::Attackers(frame) => Some(EffectDecision::Combat(
                        CombatDecision::Attackers(frame.at_revision(self.revision)),
                    )),
                    _ => None,
                }
            }
            Some(PendingWork::ManaSacrifice(work)) => work
                .batch
                .as_ref()
                .and_then(|batch| batch.frame(self.revision))
                .map(EffectDecision::ZoneReplacement),
            Some(PendingWork::GraveyardExile(work)) => work
                .batch
                .frame(self.revision)
                .map(EffectDecision::ZoneReplacement),
            Some(PendingWork::GraveyardShuffle(work)) => self.graveyard_shuffle_frame(work),
            Some(PendingWork::ActivationCost(work)) => work
                .batch
                .frame(self.revision)
                .map(EffectDecision::ZoneReplacement),
            Some(PendingWork::SelectedActivationCost(work)) => {
                self.selected_activation_cost_frame(work).ok().flatten()
            }
            Some(PendingWork::CastRecovery(frame)) => {
                let mut frame = frame.as_ref().clone();
                frame.revision = self.revision;
                Some(EffectDecision::CastRecovery(frame))
            }
            Some(PendingWork::LibraryTop(_)) => self.library_top_frame(),
            Some(PendingWork::LookTop(_)) => self.look_top_frame(),
            Some(PendingWork::ResolutionSelection(_)) => self.selection_frame(),
            Some(PendingWork::OptionalEffect(work)) => {
                if let Some(frame) = self.replacement_chooser_frame(work) {
                    return Some(EffectDecision::ReplacementChooser(frame));
                }
                Some(EffectDecision::OptionalEffect(OptionalEffectFrame {
                    rebound_or_buyback: work.rebound_or_buyback,
                    cast: work.cast.clone(),
                    revision: self.revision,
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                    chooser: work.chooser,
                    options: work.options.clone(),
                    token_offer: work.cursor.tempting_token_offer.clone(),
                }))
            }
            Some(PendingWork::ResolutionPayment(work)) => {
                Some(EffectDecision::ResolutionPayment(work.frame(self.revision)))
            }
            None
            | Some(
                PendingWork::Resolution(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::PermanentEntry(_)
                | PendingWork::BattlefieldEntry(_)
                | PendingWork::SearchEntry(_)
                | PendingWork::LandPlay(_)
                | PendingWork::ImpulseReplacement(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_)
                | PendingWork::Damage(_)
                | PendingWork::ManaColor(_),
            ) => None,
        }
    }

    /// Inspect the complete sole choice owner, including a suspended color parent.
    pub fn resolution_choice_state(&self) -> Option<ResolutionChoiceState<'_>> {
        let (work, suspended_for_mana_color) = match self.pending_work.as_ref() {
            Some(PendingWork::OptionalEffect(work)) => {
                return Some(ResolutionChoiceState::Optional {
                    cursor: (&work.cursor).into(),
                    chooser: work.chooser,
                    options: &work.options,
                });
            }
            Some(PendingWork::ResolutionPayment(work)) => (work.as_ref(), false),
            Some(PendingWork::ManaColor(work)) => match &work.parent {
                ManaParent::Payment(parent) => (parent, true),
                ManaParent::Priority
                | ManaParent::CombatPayment(_)
                | ManaParent::Casting(_)
                | ManaParent::Recovery(_, _) => return None,
            },
            None
            | Some(
                PendingWork::LibraryTop(_)
                | PendingWork::ManaSacrifice(_)
                | PendingWork::LookTop(_)
                | PendingWork::PermanentEntry(_)
                | PendingWork::CastRecovery(_)
                | PendingWork::BattlefieldEntry(_)
                | PendingWork::SearchEntry(_)
                | PendingWork::LandPlay(_)
                | PendingWork::ActivationCost(_)
                | PendingWork::SelectedActivationCost(_)
                | PendingWork::GraveyardExile(_)
                | PendingWork::GraveyardShuffle(_)
                | PendingWork::ImpulseReplacement(_)
                | PendingWork::ResolutionSelection(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_)
                | PendingWork::Damage(_)
                | PendingWork::Resolution(_),
            ) => return None,
        };
        Some(ResolutionChoiceState::Payment {
            cursor: (&work.cursor).into(),
            payer: work.payer,
            cost: &work.cost,
            restricted_colors: work.restricted_colors,
            paid_destination: work.paid_destination,
            declined_destination: work.declined_destination,
            suspended_for_mana_color,
        })
    }

    pub(super) fn highest_entrant_controller(&self) -> Result<Option<SeatId>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|binding| binding.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [cause] = trigger.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let (crate::occurrence::TriggerRelated::ZoneChange { after, .. }
        | crate::occurrence::TriggerRelated::TokenCreated { after, .. }) = &cause.related
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let entrant = after.object.id();
        let view = self.evaluated_characteristics()?;
        let (power, controller) = if let Some(object) = view.get(entrant) {
            (
                object.power_toughness.as_ref().map(|pt| &pt.derived.power),
                object.controller,
            )
        } else if let Some(last) = self.departed_damage_sources.get(&entrant) {
            (last.power.as_ref(), last.object.controller())
        } else {
            return Ok(None);
        };
        let Some(power) = power else {
            return Ok(None);
        };
        let highest = view
            .objects
            .values()
            .filter(|other| {
                other.zone == ObjectZone::Battlefield
                    && other.is_creature()
                    && other.object != entrant
            })
            .all(|other| {
                other
                    .power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.derived.power < *power)
            });
        Ok(controller.filter(|seat| highest && self.window.participants().contains(seat)))
    }

    pub(super) fn combat_damage_source_controller(&self) -> Result<Option<SeatId>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let members = self
            .execution_bindings(entry.id())
            .and_then(|binding| binding.trigger.as_ref())
            .ok_or(EffectError::InvalidResolvingPlayer)?
            .causes
            .members();
        let [member] = members else {
            return Err(EffectError::InvalidResolvingPlayer);
        };
        let crate::occurrence::TriggerRelated::SubtypeCombatDamage { creature, .. } =
            member.related
        else {
            return Err(EffectError::InvalidResolvingPlayer);
        };
        let controller = self
            .current_characteristics(creature)?
            .and_then(|view| view.controller)
            .or_else(|| {
                self.departed_damage_sources
                    .get(&creature)
                    .and_then(|source| source.object.controller())
            });
        Ok(controller.filter(|seat| self.window.participants().contains(seat)))
    }

    pub(super) fn captured_related_player(&self) -> Result<SeatId, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        if !matches!(entry.kind(), StackKind::TriggeredAbility { source } if source == &binding.source.object)
        {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        let (first, rest) = binding
            .causes
            .members()
            .split_first()
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        let player = first
            .related_player
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        // Setup gives every admitted seat one life total. An invented seat cannot
        // become a legitimate former participant when checking a later payment.
        if self.life_total(player).is_none()
            || rest
                .iter()
                .any(|cause| cause.related_player != Some(player))
            || matches!(binding.causes.key(), crate::occurrence::TriggerCauseKey::Player(seat) if seat != player)
        {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        Ok(player)
    }

    fn triggering_attacker_controller(&self) -> Result<SeatId, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let crate::occurrence::TriggerRelated::Combatant(attacker) = member.related else {
            return Err(EffectError::InconsistentProgram);
        };
        self.current_characteristics(attacker)?
            .filter(|p| p.zone == ObjectZone::Battlefield)
            .and_then(|p| p.controller)
            .or_else(|| {
                self.departed_damage_sources
                    .get(&attacker)
                    .and_then(|last| last.object.controller())
            })
            .ok_or(EffectError::InvalidResolvingPlayer)
    }

    pub(super) fn resolving_player(&self, role: ResolvingPlayer) -> Result<SeatId, EffectError> {
        self.resolving_player_at(role, None)
    }

    pub(super) fn resolving_player_at(
        &self,
        role: ResolvingPlayer,
        selection_cursor: Option<&ResolutionCursor>,
    ) -> Result<SeatId, EffectError> {
        let player = self.resolving_player_identity_at(role, selection_cursor)?;
        if !self.window.participants().contains(&player) {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        Ok(player)
    }

    /// Resolve the original subject independently of whether they can act now.
    pub(super) fn resolving_player_identity_at(
        &self,
        role: ResolvingPlayer,
        selection_cursor: Option<&ResolutionCursor>,
    ) -> Result<SeatId, EffectError> {
        let index = selection_cursor
            .map(|cursor| cursor.instruction)
            .or_else(|| self.instruction_index());
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        let player = match role {
            ResolvingPlayer::Controller => entry.controller(),
            ResolvingPlayer::DestructionTargetController => selection_cursor
                .or_else(|| self.resolution_cursor())
                .and_then(|cursor| cursor.results.destruction_target_controller)
                .ok_or(EffectError::InvalidResolvingPlayer)?,
            ResolvingPlayer::ExiledTargetController => {
                selection_cursor
                    .or_else(|| self.resolution_cursor())
                    .and_then(|cursor| cursor.exiled_target.as_ref())
                    .ok_or(EffectError::InvalidResolvingPlayer)?
                    .controller
            }
            ResolvingPlayer::TargetPlayer => {
                let bound = self.instruction_target_at(entry, index)?;
                let TargetRef::Player(player) = bound.choice().target else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                player
            }
            ResolvingPlayer::UniqueRelatedPlayer => self.captured_related_player()?,
            ResolvingPlayer::TriggeringAttackerController => {
                self.triggering_attacker_controller()?
            }
            ResolvingPlayer::HighestEntrantController => self
                .highest_entrant_controller()?
                .ok_or(EffectError::InvalidResolvingPlayer)?,
            ResolvingPlayer::CombatDamageSourceController => self
                .combat_damage_source_controller()?
                .ok_or(EffectError::InvalidResolvingPlayer)?,
            ResolvingPlayer::TargetSpellController => {
                let bound = self.instruction_target_at(entry, index)?;
                let TargetRef::Stack(target) = bound.choice().target else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                self.window
                    .entries()
                    .iter()
                    .find(|candidate| candidate.id() == target)
                    .map(crate::stack_window::StackEntry::controller)
                    .or_else(|| {
                        selection_cursor
                            .or_else(|| self.resolution_cursor())
                            .and_then(|cursor| cursor.results.countered_spell_controller)
                            .filter(|(id, _)| *id == target)
                            .map(|(_, controller)| controller)
                    })
                    .ok_or(EffectError::InvalidResolvingPlayer)?
            }
            ResolvingPlayer::TargetController => {
                let bound = self.instruction_target_at(entry, index)?;
                match bound.choice().target {
                    TargetRef::Player(player) => player,
                    TargetRef::Object(id) => {
                        self.evaluated_characteristics()?
                            .get(id)
                            .and_then(|view| view.controller)
                            .or_else(|| {
                                selection_cursor
                                    .or_else(|| self.resolution_cursor())
                                    .and_then(|cursor| {
                                        cursor
                                            .results
                                            .destruction_target_controller
                                            .or_else(|| {
                                                cursor.exiled_target.as_ref().map(|t| t.controller)
                                            })
                                            .or_else(|| {
                                                cursor.results.target_lki.get(&id).and_then(|lki| {
                                                    lki.controller.or(Some(lki.owner))
                                                })
                                            })
                                    })
                            })
                            .or_else(|| self.window.cards().get(id).map(GameObject::owner))
                            .ok_or(EffectError::InvalidResolvingPlayer)?
                    }
                    TargetRef::Stack(target) => self
                        .window
                        .entries()
                        .iter()
                        .find(|candidate| candidate.id() == target)
                        .map(crate::stack_window::StackEntry::controller)
                        .or_else(|| {
                            selection_cursor
                                .or_else(|| self.resolution_cursor())
                                .and_then(|cursor| cursor.results.countered_spell_controller)
                                .filter(|(id, _)| *id == target)
                                .map(|(_, controller)| controller)
                        })
                        .ok_or(EffectError::InvalidResolvingPlayer)?,
                }
            }
            ResolvingPlayer::TargetOwner => {
                let bound = self.instruction_target_at(entry, index)?;
                match bound.choice().target {
                    TargetRef::Object(id) => self
                        .window
                        .cards()
                        .get(id)
                        .map(GameObject::owner)
                        .or_else(|| {
                            selection_cursor
                                .or_else(|| self.resolution_cursor())
                                .and_then(|cursor| cursor.results.target_lki.get(&id))
                                .map(|lki| lki.owner)
                        })
                        .ok_or(EffectError::InvalidResolvingPlayer)?,
                    TargetRef::Stack(target) => self
                        .window
                        .entries()
                        .iter()
                        .find(|candidate| candidate.id() == target)
                        .map(crate::stack_window::StackEntry::controller)
                        .or_else(|| {
                            selection_cursor
                                .or_else(|| self.resolution_cursor())
                                .and_then(|cursor| cursor.results.countered_spell_controller)
                                .filter(|(id, _)| *id == target)
                                .map(|(_, controller)| controller)
                        })
                        .ok_or(EffectError::InvalidResolvingPlayer)?,
                    TargetRef::Player(player) => player,
                }
            }
            ResolvingPlayer::ActivePlayer => self.window.active_player(),
            ResolvingPlayer::EnchantedPlayer => {
                let src = match entry.kind() {
                    StackKind::ActivatedAbility { source, .. }
                    | StackKind::TriggeredAbility { source, .. } => source.id(),
                    _ => return Err(EffectError::InvalidResolvingPlayer),
                };
                match self.attached_to(src) {
                    Some(TargetRef::Player(seat)) => seat,
                    _ => return Err(EffectError::InvalidResolvingPlayer),
                }
            }
            ResolvingPlayer::AttachedHostController => {
                let src = match entry.kind() {
                    StackKind::ActivatedAbility { source, .. }
                    | StackKind::TriggeredAbility { source, .. } => source.id(),
                    _ => return Err(EffectError::InvalidResolvingPlayer),
                };
                match self.attached_to(src) {
                    Some(TargetRef::Player(seat)) => seat,
                    Some(TargetRef::Object(host)) => self
                        .evaluated_characteristics()?
                        .get(host)
                        .and_then(|view| view.controller)
                        .or_else(|| self.window.cards().get(host).map(GameObject::owner))
                        .ok_or(EffectError::InvalidResolvingPlayer)?,
                    _ => {
                        // Fall back to captured trigger attachment LKI when source left battlefield.
                        let lki_host = self
                            .execution_bindings(entry.id())
                            .and_then(|b| b.trigger.as_ref())
                            .and_then(|t| t.source.attached_to);
                        match lki_host {
                            Some(TargetRef::Player(seat)) => seat,
                            Some(TargetRef::Object(host)) => self
                                .evaluated_characteristics()?
                                .get(host)
                                .and_then(|view| view.controller)
                                .or_else(|| self.window.cards().get(host).map(GameObject::owner))
                                .ok_or(EffectError::InvalidResolvingPlayer)?,
                            _ => return Err(EffectError::InvalidResolvingPlayer),
                        }
                    }
                }
            }
            ResolvingPlayer::TriggerSubjectController => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                let member = binding
                    .causes
                    .members()
                    .first()
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                let from_related = match &member.related {
                    crate::occurrence::TriggerRelated::ZoneChange { before, after } => {
                        if before.object.zone() == ObjectZone::Battlefield
                            && after.object.zone() != ObjectZone::Battlefield
                        {
                            before.object.controller().or(Some(before.object.owner()))
                        } else {
                            after
                                .object
                                .controller()
                                .or_else(|| before.object.controller())
                                .or(Some(after.object.owner()))
                        }
                    }
                    crate::occurrence::TriggerRelated::StatusChange(sc) => {
                        sc.after.controller().or_else(|| sc.before.controller())
                    }
                    crate::occurrence::TriggerRelated::TokenCreated { creator, after } => {
                        after.object.controller().or(Some(*creator))
                    }
                    crate::occurrence::TriggerRelated::Discard { player, .. } => Some(*player),
                    crate::occurrence::TriggerRelated::SpellCast(cast) => Some(cast.controller()),
                    crate::occurrence::TriggerRelated::CardDrawn { player }
                    | crate::occurrence::TriggerRelated::PlayerLost(player) => Some(*player),
                    _ => None,
                };
                if let Some(seat) = from_related {
                    seat
                } else if let Some(subject_id) = member.subject_object_id() {
                    self.evaluated_characteristics()?
                        .get(subject_id)
                        .and_then(|view| view.controller)
                        .or_else(|| self.window.cards().get(subject_id).map(GameObject::owner))
                        .or(member.related_player)
                        .ok_or(EffectError::InvalidResolvingPlayer)?
                } else {
                    member
                        .related_player
                        .ok_or(EffectError::InvalidResolvingPlayer)?
                }
            }
            ResolvingPlayer::TriggerSubjectOwner => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                let member = binding
                    .causes
                    .members()
                    .first()
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                let from_related = match &member.related {
                    crate::occurrence::TriggerRelated::ZoneChange { before, .. } => {
                        Some(before.object.owner())
                    }
                    crate::occurrence::TriggerRelated::StatusChange(sc) => Some(sc.after.owner()),
                    crate::occurrence::TriggerRelated::TokenCreated { after, .. } => {
                        Some(after.object.owner())
                    }
                    crate::occurrence::TriggerRelated::Discard { before, .. } => {
                        Some(before.object.owner())
                    }
                    crate::occurrence::TriggerRelated::SpellCast(cast) => {
                        Some(cast.spell().object.owner())
                    }
                    _ => None,
                };
                if let Some(seat) = from_related {
                    seat
                } else if let Some(subject_id) = member.subject_object_id() {
                    self.window
                        .cards()
                        .get(subject_id)
                        .map(GameObject::owner)
                        .or(member.related_player)
                        .ok_or(EffectError::InvalidResolvingPlayer)?
                } else {
                    member
                        .related_player
                        .ok_or(EffectError::InvalidResolvingPlayer)?
                }
            }
            ResolvingPlayer::SourceChosenPlayer => {
                let src = match entry.kind() {
                    StackKind::ActivatedAbility { source, .. }
                    | StackKind::TriggeredAbility { source, .. } => source.id(),
                    _ => return Err(EffectError::InvalidResolvingPlayer),
                };
                self.characteristics
                    .entry_player_choices
                    .get(&src)
                    .copied()
                    .or_else(|| match self.entry_choices.get(&src) {
                        Some(crate::land::EntryChoice::Opponent(seat)) => Some(*seat),
                        _ => None,
                    })
                    .ok_or(EffectError::InvalidResolvingPlayer)?
            }
            ResolvingPlayer::DefendingPlayer => {
                let combat = self
                    .combat
                    .as_ref()
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                let src_opt = match entry.kind() {
                    StackKind::ActivatedAbility { source, .. }
                    | StackKind::TriggeredAbility { source, .. } => Some(source.id()),
                    _ => None,
                };
                src_opt
                    .and_then(|src| combat.attacker(src).map(|a| a.defender()))
                    .or_else(|| combat.attackers().next().map(|a| a.defender()))
                    .ok_or(EffectError::InvalidResolvingPlayer)?
            }
            ResolvingPlayer::ChosenPlayer => selection_cursor
                .or_else(|| self.resolution_cursor())
                .and_then(|cursor| cursor.chosen_opponent)
                .ok_or(EffectError::InvalidResolvingPlayer)?,
            ResolvingPlayer::LoopPlayer => selection_cursor
                .or_else(|| self.resolution_cursor())
                .and_then(|cursor| cursor.results.loop_player)
                .ok_or(EffectError::InvalidResolvingPlayer)?,
            ResolvingPlayer::RandomOpponent => {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), entry.controller())?;
                if opponents.is_empty() {
                    return Err(EffectError::InvalidResolvingPlayer);
                }
                let pick = if opponents.len() == 1 {
                    0
                } else {
                    let mut h = self.revision
                        ^ u64::from(entry.controller().0).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ u64::try_from(self.window.entries().len())
                            .unwrap_or(0)
                            .wrapping_mul(0xBF58_476D_1CE4_E5B9)
                        ^ u64::try_from(index.unwrap_or(0))
                            .unwrap_or(0)
                            .wrapping_mul(0x94D0_49BB_1331_11EB);
                    h ^= h >> 30;
                    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                    h ^= h >> 27;
                    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
                    h ^= h >> 31;
                    let denom = u64::try_from(opponents.len()).unwrap_or(1);
                    usize::try_from(h % denom).unwrap_or(0)
                };
                opponents[pick]
            }
            ResolvingPlayer::SourceOwner => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    source.owner()
                }
                StackKind::CardSpell(id) => self
                    .window
                    .cards()
                    .get(*id)
                    .map(GameObject::owner)
                    .unwrap_or_else(|| entry.controller()),
                _ => self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .map(|t| t.source.object.owner())
                    .unwrap_or_else(|| entry.controller()),
            },
        };
        if !self.window.original_participants().contains(&player) {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        Ok(player)
    }

    fn optional_instruction(&self, cursor: &ResolutionCursor) -> Result<&Instruction, EffectError> {
        self.payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(cursor.instruction))
            .ok_or(EffectError::InconsistentProgram)
    }

    fn optional_following_instruction(
        &self,
        cursor: &ResolutionCursor,
    ) -> Result<&Instruction, EffectError> {
        let following = cursor
            .instruction
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        self.payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(following))
            .ok_or(EffectError::InconsistentProgram)
    }

    fn skip_unpayable_optional_after_departure(&mut self) -> Result<(), EffectError> {
        let Some(PendingWork::OptionalEffect(work)) = &self.pending_work else {
            return Ok(());
        };
        if self.window.participants().contains(&work.chooser) {
            return Ok(());
        }
        let count = match self.optional_instruction(&work.cursor)? {
            Instruction::OptionalEffect { .. } => 1,
            Instruction::OptionalSequence { instructions, .. } => *instructions,
            _ => return Err(EffectError::WrongPhase),
        };
        if work.options.contains(&OptionalEffectChoice::Take) {
            let controller = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            let next_instr = self.optional_following_instruction(&work.cursor)?;
            let is_payment = matches!(
                next_instr,
                Instruction::AdjustLife(spec) if matches!(spec.op, crate::toolbox::LifeOp::Pay(_))
            ) || matches!(
                next_instr,
                Instruction::MutatePlayer(spec)
                    if matches!(spec.op, crate::toolbox::PlayerMutation::PayCounter { .. })
            );
            let unpaid_cost = is_payment && !self.window.participants().contains(&controller);
            if !unpaid_cost {
                return Ok(());
            }
        }
        let Some(PendingWork::OptionalEffect(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongOptionalEffectFrame);
        };
        work.cursor.results.last_action_succeeded = false;
        work.cursor.results.last_action_count = Some(0.into());
        let destination = work
            .cursor
            .instruction
            .checked_add(1)
            .and_then(|next| next.checked_add(count))
            .ok_or(EffectError::InconsistentProgram)?;
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        self.set_instruction(destination)?;
        Ok(())
    }

    fn start_resolution_payment(
        &mut self,
        work: ResolutionPaymentWork,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self.window.participants().contains(&work.payer) {
            self.pending_work = Some(PendingWork::ResolutionPayment(Box::new(work)));
            Ok(())
        } else {
            self.finish_resolution_payment(work, ResolutionPaymentOutcome::UnpaidDeparted, append)
        }
    }

    pub(super) fn awaiting_departed_payer(&self) -> bool {
        matches!(&self.pending_work, Some(PendingWork::ResolutionPayment(work))
            if !self.window.participants().contains(&work.payer))
    }

    pub(super) fn settle_departed_payer(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        if !self.awaiting_departed_payer() {
            return Ok(false);
        }
        let Some(PendingWork::ResolutionPayment(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionPaymentFrame);
        };
        self.finish_resolution_payment(*work, ResolutionPaymentOutcome::UnpaidDeparted, append)?;
        Ok(true)
    }

    /// Keep the original cursor and accepted prefix while rebinding a pending choice.
    pub(super) fn refresh_resolution_choices_after_departure(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if let Some(PendingWork::OptionalEffect(work)) = &self.pending_work {
            if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
                self.pending_work = None;
                return Ok(());
            }
            let general = matches!(
                self.optional_instruction(&work.cursor)?,
                Instruction::OptionalEffect { .. } | Instruction::OptionalSequence { .. }
            );
            if !general && self.window.participants().contains(&work.chooser) {
                return Ok(());
            }
            if !general {
                return Err(EffectError::WrongPhase);
            }
            let is_sequence = matches!(
                self.optional_instruction(&work.cursor)?,
                Instruction::OptionalSequence { .. }
            );
            let available = self.optional_effect_available(
                self.optional_following_instruction(&work.cursor)?,
                is_sequence,
            )?;
            let Some(PendingWork::OptionalEffect(work)) = &mut self.pending_work else {
                return Err(EffectError::WrongOptionalEffectFrame);
            };
            work.options = [OptionalEffectChoice::Decline].into();
            if available {
                work.options.insert(OptionalEffectChoice::Take);
            }
            self.skip_unpayable_optional_after_departure()?;
            return Ok(());
        }
        if let Some(PendingWork::ManaColor(work)) = &mut self.pending_work {
            work.frame.revision = self.revision;
            let chooser_alive = self.window.participants().contains(&work.frame.chooser);
            if let ManaParent::Payment(parent) = &work.parent {
                let payment_survives =
                    self.window.resolving().map(|entry| entry.id()) == Some(parent.cursor.entry);
                if !chooser_alive {
                    self.pending_work = payment_survives
                        .then(|| PendingWork::ResolutionPayment(Box::new(parent.clone())));
                } else if !payment_survives {
                    work.parent = ManaParent::Priority;
                }
            } else if !chooser_alive && matches!(work.parent, ManaParent::Priority) {
                self.pending_work = None;
            } else if !chooser_alive {
                return Err(EffectError::WrongPhase);
            }
        }
        if matches!(&self.pending_work, Some(PendingWork::ResolutionPayment(work))
            if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry))
        {
            self.pending_work = None;
        }
        self.settle_departed_payer(append)?;
        Ok(())
    }

    /// Availability is distinct from the driver's targeted-operation dispatch.
    fn optional_effect_available(
        &self,
        instruction: &Instruction,
        is_sequence: bool,
    ) -> Result<bool, EffectError> {
        let allowed_sequence_head = matches!(
            instruction,
            Instruction::OfferManaPayment { .. } | Instruction::OfferDynamicManaPayment { .. }
        ) || (is_sequence
            && matches!(
                instruction,
                Instruction::BranchIfFalse { .. } | Instruction::Jump { .. }
            ));
        if !instruction.is_optional_primitive() && !allowed_sequence_head {
            return Err(EffectError::InconsistentProgram);
        }
        match instruction {
            Instruction::AffectPermanentTargets { .. }
            | Instruction::ModifyAnyNumberOfPermanents { .. } => {
                Ok(self.window.target_assessment().is_some()
                    && !self.legal_permanent_targets()?.is_empty())
            }
            Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource => Ok(true),
            Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::DamageFromGrantingPermanent { .. } => {
                Err(EffectError::InconsistentProgram)
            }
            Instruction::ReturnDelayedGraveyardCardTransformed(object) => {
                Ok(self.delayed_transformed_return_player(*object).is_some())
            }
            Instruction::ReturnDelayedGraveyardCard(object) => {
                Ok(self.delayed_graveyard_return_player(*object).is_some())
            }
            Instruction::ScheduleCreatedTokenExile(_)
            | Instruction::ScheduleCreatedTokenSacrifice
            | Instruction::ExileDelayedObjects(_)
            | Instruction::GrantFlashback(_) => Err(EffectError::InconsistentProgram),
            Instruction::AdjustLife(spec) => {
                if let crate::toolbox::LifeOp::Pay(amount) = &spec.op {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let controller = entry.controller();
                    let evaluated = self.evaluated_characteristics()?;
                    let cost = self.resolve_value_expr(controller, amount)?.max(0.into());
                    if !cost.is_positive() {
                        return Ok(true);
                    }
                    if evaluated.life_and_creature_costs_prohibited() {
                        return Ok(false);
                    }
                    let seats = self.eval_player_selector_at(
                        controller,
                        Some(entry),
                        &spec.players,
                        self.resolution_cursor(),
                    )?;
                    Ok(!seats.is_empty()
                        && seats
                            .iter()
                            .all(|s| self.life.get(*s).is_some_and(|total| *total >= cost)))
                } else if spec.players.uses_target() {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let controller = entry.controller();
                    let seats = self.eval_player_selector_at(
                        controller,
                        Some(entry),
                        &spec.players,
                        self.resolution_cursor(),
                    )?;
                    Ok(seats.iter().any(|s| self.window.participants().contains(s)))
                } else if matches!(
                    spec.players,
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::UniqueRelatedPlayer
                    ) | crate::toolbox::PlayerSelector::Group(
                        crate::program::AffectedPlayers::One(
                            crate::program::ResolvingPlayer::UniqueRelatedPlayer
                        )
                    )
                ) {
                    Ok(self
                        .window
                        .participants()
                        .contains(&self.captured_related_player()?))
                } else {
                    Ok(true)
                }
            }
            Instruction::MutatePlayer(spec) => {
                if let crate::toolbox::PlayerMutation::PayCounter { kind, amount } = &spec.op {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let controller = entry.controller();
                    let cost = self.resolve_value_expr(controller, amount)?.max(0.into());
                    if !cost.is_positive() {
                        return Ok(true);
                    }
                    let seats = self.eval_player_selector_at(
                        controller,
                        Some(entry),
                        &spec.players,
                        self.resolution_cursor(),
                    )?;
                    Ok(!seats.is_empty()
                        && seats
                            .iter()
                            .all(|s| self.player_counters_of_kind(*s, *kind) >= cost))
                } else {
                    Ok(true)
                }
            }
            Instruction::CollectEvidence { player, minimum } => {
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                let actor = self.resolving_player_identity_at(*player, None)?;
                let needed = self.resolve_value_expr(controller, minimum)?.max(0.into());
                let total_mv = self
                    .window
                    .cards()
                    .iter()
                    .filter(|c| c.zone() == ObjectZone::Graveyard(actor) && c.card_id().is_some())
                    .fold(RulesInteger::from(0), |acc, c| {
                        acc.plus(
                            &self
                                .permanent_mana_value(c.id())
                                .unwrap_or_else(|| 0.into()),
                        )
                    });
                Ok(total_mv >= needed)
            }
            Instruction::Blight { player, .. } => {
                let actor = self.resolving_player_identity_at(*player, None)?;
                let evaluated = self.evaluated_characteristics()?;
                Ok(evaluated.objects.values().any(|v| {
                    v.zone == ObjectZone::Battlefield
                        && v.controller == Some(actor)
                        && v.is_creature()
                        && self.window.cards().phased_out_under(v.object).is_none()
                }))
            }
            Instruction::Discard {
                players: crate::program::AffectedPlayers::One(role),
                amount,
            } => {
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                let player = self.resolving_player_identity_at(*role, None)?;
                let requested = self.resolve_value_expr(controller, amount)?;
                let hand_count = self
                    .window
                    .cards()
                    .iter()
                    .filter(|card| {
                        card.zone() == ObjectZone::Hand(player) && card.card_id().is_some()
                    })
                    .count();
                Ok(!requested.is_positive() || RulesInteger::from_usize(hand_count) >= requested)
            }
            Instruction::ChooseNewTargets
            | Instruction::OfferManaPayment { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::FixedTargetClause(_) => Ok(true),
            Instruction::Sacrifice {
                players: crate::program::AffectedPlayers::One(role),
                amount,
                filter,
            } => {
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                let player = self.resolving_player_identity_at(*role, None)?;
                let requested = self.resolve_value_expr(controller, amount)?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let matching_count = evaluated
                    .objects
                    .values()
                    .filter(|view| {
                        view.zone == ObjectZone::Battlefield
                            && view.controller == Some(player)
                            && self.window.cards().phased_out_under(view.object).is_none()
                            && filter.matches_view(view, &excluded, || {
                                self.permanent_mana_value(view.object)
                            })
                    })
                    .count();
                Ok(!requested.is_positive()
                    || RulesInteger::from_usize(matching_count) >= requested)
            }
            Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Proliferate
            | Instruction::AddMana(_)
            | Instruction::ExileTopForPlay { .. }
            | Instruction::Bolster(_)
            | Instruction::Amass { .. }
            | Instruction::ManifestTop { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::LookTopChoose { .. }
            | Instruction::ChooseObjects { .. }
            | Instruction::ChooseOption { .. }
            | Instruction::DivideDamageAmongTargets { .. }
            | Instruction::DivideDamageExprAmongTargets { .. }
            | Instruction::BecomeCopy { .. }
            | Instruction::BecomeCopyFixedTargets { .. }
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::RollDie { .. }
            | Instruction::FlipCoins { .. }
            | Instruction::ChooseValue { .. }
            | Instruction::ChooseOptionForPlayers { .. }
            | Instruction::Vote { .. }
            | Instruction::Incubate { .. }
            | Instruction::Connive { .. }
            | Instruction::ConniveFinalize { .. }
            | Instruction::Discover { .. }
            | Instruction::Clash { .. }
            | Instruction::RingTemptsYou
            | Instruction::VentureIntoDungeon { .. }
            | Instruction::TakeInitiative { .. }
            | Instruction::UnlockDoor { .. }
            | Instruction::Behold { .. }
            | Instruction::ManifestDread
            | Instruction::Monstrosity { .. }
            | Instruction::Fight { .. }
            | Instruction::Exchange(_)
            | Instruction::CopyStackObject { .. }
            | Instruction::CastLinkedFaceDownFree
            | Instruction::Ripple(_)
            | Instruction::Demonstrate
            | Instruction::Enlist
            | Instruction::ChooseOneLastMoved { .. }
            | Instruction::DrawCards(_)
            | Instruction::ApplyContinuous(_)
            | Instruction::PromptChoice(_)
            | Instruction::BranchCondition { .. }
            | Instruction::ForEach { .. }
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::BindEnv { .. }
            | Instruction::SetObjectTag { .. }
            | Instruction::UpdateStateTrack { .. }
            | Instruction::SideDeckOp(_)
            | Instruction::CreateTokenCopy { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::CreateAuraTokenAttached { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::RepeatLoop { .. }
            | Instruction::RevealUntilMatch { .. } => Ok(true),
            Instruction::ZoneOp(spec) => {
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Graveyard {
                        semantics: crate::toolbox::GraveyardSemantics::Sacrifice
                    }
                ) && let crate::toolbox::ObjectQuery::ZoneScan {
                    zone: crate::toolbox::QueryZone::BattlefieldControlledBy(player_sel),
                    filter,
                    ..
                } = &spec.source
                    && let crate::toolbox::ZoneSelection::Prompt { min, .. } = &spec.selection
                {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let controller = entry.controller();
                    let requested = self.resolve_value_expr(controller, min)?;
                    if !requested.is_positive() {
                        return Ok(true);
                    }
                    let seats = self.eval_player_selector_at(
                        controller,
                        Some(entry),
                        player_sel,
                        self.resolution_cursor(),
                    )?;
                    let source = match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => Some(source.id()),
                        _ => None,
                    };
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    Ok(!seats.is_empty()
                        && seats.iter().all(|player| {
                            let matching_count = evaluated
                                .objects
                                .values()
                                .filter(|permanent| {
                                    permanent.zone == ObjectZone::Battlefield
                                        && permanent.controller == Some(*player)
                                        && self
                                            .window
                                            .cards()
                                            .phased_out_under(permanent.object)
                                            .is_none()
                                        && self.matches_object_filter(
                                            **filter, permanent, controller, source, &excluded,
                                            &evaluated,
                                        )
                                        && !evaluated.sacrifice_prohibited(permanent.object)
                                })
                                .count();
                            RulesInteger::from_usize(matching_count) >= requested
                        }))
                } else {
                    Ok(true)
                }
            }
            Instruction::DealDamage(spec) => match &spec.recipients {
                crate::toolbox::DamageTargetQuery::Targets => {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    let evaluated = self.evaluated_characteristics()?;
                    Ok(match self.instruction_target(entry)?.choice().target {
                        TargetRef::Player(player) => self.window.participants().contains(&player),
                        TargetRef::Object(card) => evaluated
                            .get(card)
                            .and_then(ObjectCharacteristics::damage_recipient)
                            .is_some(),
                        TargetRef::Stack(_) => false,
                    })
                }
                crate::toolbox::DamageTargetQuery::Relative(rel) => match rel.as_ref() {
                    crate::program::DamageRecipientsRef::Group(group) => {
                        let controller = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .controller();
                        Ok(!self.group_damage_recipients(*group, controller)?.is_empty())
                    }
                    _ => Ok(true),
                },
                _ => Ok(true),
            },
            Instruction::MutateObject(spec) => {
                if matches!(spec.source, crate::toolbox::ObjectQuery::Targets) {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    Ok(match self.instruction_target(entry)?.choice().target {
                        TargetRef::Object(card) => {
                            if matches!(spec.op, crate::toolbox::ObjectMutation::RemoveFromCombat) {
                                self.combat.as_ref().is_some_and(|combat| {
                                    combat.attacker(card).is_some()
                                        || combat.blocker(card).is_some()
                                })
                            } else {
                                self.window
                                    .cards()
                                    .get(card)
                                    .is_some_and(|v| v.zone() == ObjectZone::Battlefield)
                            }
                        }
                        _ => true,
                    })
                } else if matches!(spec.source, crate::toolbox::ObjectQuery::Source) {
                    let entry = self
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongResolutionFrame)?;
                    Ok(matches!(
                        entry.kind(),
                        StackKind::TriggeredAbility { source }
                            | StackKind::ActivatedAbility { source }
                            if self
                                .window
                                .cards()
                                .get(source.id())
                                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    ))
                } else {
                    Ok(true)
                }
            }
            Instruction::Mill { player, amount } => {
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                let requested = self.resolve_value_expr(controller, amount)?;
                let player = self.resolving_player_identity_at(*player, None)?;
                Ok(self.window.participants().contains(&player)
                    && (!requested.is_positive()
                        || !self.window.cards().library(player).is_empty()))
            }
            Instruction::AddManaForPlayer { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::PutHandPermanentsOntoBattlefield { .. }
            | Instruction::RequireCondition(_)
            | Instruction::ReselectAttackDefender
            | Instruction::SetBlockDeclarations(_)
            | Instruction::AdditionalLoyaltyActivation
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::Cascade
            | Instruction::CascadeBottom
            | Instruction::Myriad
            | Instruction::WatchNextSpellOrLoyaltyTwice
            | Instruction::WatchNextLoyaltyCopy
            | Instruction::CopyCapturedAbility
            | Instruction::CopyOnCast(_)
            | Instruction::MadnessCast
            | Instruction::MiracleCast(_)
            | Instruction::CastDefeatedSiege
            | Instruction::ReboundCast(_)
            | Instruction::EndStepCast { .. }
            | Instruction::SacrificeDelayedObjects(_)
            | Instruction::ScheduleArtifactReturn(_)
            | Instruction::ReturnDelayedPermanentToHand(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::SacrificeStateTriggerSource
            | Instruction::CounterWardSource
            | Instruction::CounterTriggeringStackObject => Ok(true),
            Instruction::AdditionalCombat { .. } => Ok(true),
            Instruction::ExileOnePerPlayerUntilSourceLeaves
            | Instruction::Populate
            | Instruction::Sacrifice { .. }
            | Instruction::SacrificeRandomUntilLand
            | Instruction::Discard { .. } => Err(EffectError::InconsistentProgram),
            Instruction::SearchLibrary { filter } => {
                if let Some(host) = filter.curse_host() {
                    Ok(self.searched_curse_host(host)?.is_some())
                } else {
                    Ok(self.resolving_player(filter.searcher).is_ok())
                }
            }
            Instruction::ReturnGraveyardCreatureWithCorpse(_) => {
                Err(EffectError::InconsistentProgram)
            }
            Instruction::ResolveTransfer(_) => Ok(true),
            Instruction::ResolvePowerAbility(_)
            | Instruction::CreateTokenAndAttach { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::CreateAttackingTokens { .. }
            | Instruction::ForEachPlayer { .. }
            | Instruction::OfferAnyPlayer { .. }
            | Instruction::DealDamageToAnnouncedTargets { .. }
            | Instruction::GrantChosenProtection { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::RevealTopTwoPiles { .. }
            | Instruction::RevealUntilMatchExt { .. }
            | Instruction::CreateTokenCopyExt { .. }
            | Instruction::MoveCounters { .. }
            | Instruction::CounterMatchingStackObjects { .. }
            | Instruction::TransferAllCountersFromTriggeringLki { .. } => Ok(true),
            Instruction::ChooseCreatureType { excluding, .. } => {
                Ok(excluding.len() < self.creature_vocabulary.len())
            }
            Instruction::CopyTargetToSource { .. } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?;
                let StackKind::ActivatedAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                if !self
                    .window
                    .cards()
                    .get(source.id())
                    .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
                {
                    return Ok(false);
                }
                let bound = self.instruction_target(entry)?;
                let TargetRef::Object(donor) = bound.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                Ok(
                    self.operation_available(instruction, bound.choice().target)?
                        && self.copiable_program(donor).is_some_and(|program| {
                            matches!(program.kind, ProgramKind::Permanent(_))
                        }),
                )
            }
            Instruction::DamageTargetControllersCreatures { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ExileDefeatedSiege
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::FightFixedTargets
            | Instruction::BindOrderedPermanents { .. }
            | Instruction::MayRevealLibraryTop
            | Instruction::ChooseFoundSpell
            | Instruction::Encore
            | Instruction::PairWithCreature { .. }
            | Instruction::FlipCoin { .. }
            | Instruction::CastFoundSpell(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::Attach(_)
            | Instruction::ChooseRevealedHandCard { .. }
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. }
            | Instruction::CounterSpell
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::Earthbend { .. }
            | Instruction::GrantProtectionChosenColor(_) => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?;
                let bound = self.instruction_target(entry)?;
                self.operation_available(instruction, bound.choice().target)
            }
            Instruction::ReturnCardsDestroyedThisWay
            | Instruction::ReturnEarthbentCard
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferSourceCounters { .. } => Err(EffectError::InconsistentProgram),
            Instruction::BranchIfFalse { .. } | Instruction::Jump { .. } => {
                if is_sequence {
                    Ok(true)
                } else {
                    Err(EffectError::InconsistentProgram)
                }
            }
        }
    }

    pub(super) fn begin_rebound_buyback_choice(
        &mut self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let StackKind::CardSpell(object) = entry.kind() else {
            return Ok(None);
        };
        if !entry.bought_back()
            || (!entry.exiles_if_put_into_graveyard() && !entry.cast_from_controllers_hand())
            || !matches!(self.payloads.get(&entry.id()),Some(StackPayload::Instructions(payload)) if payload.rebound_over_buyback.is_none() && !payload.exile_on_resolve)
            || (!entry.exiles_if_put_into_graveyard() && {
                let eval = self.evaluated_characteristics()?;
                !eval.get(*object).is_some_and(|c| {
                    c.has_keyword(KeywordAbility::Rebound)
                        || self.spell_has_granted_keyword(
                            &eval,
                            c,
                            entry.controller(),
                            KeywordAbility::Rebound,
                        )
                })
            })
        {
            return Ok(None);
        }
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
            rebound_or_buyback: true,
            cast: None,
            cursor,
            chooser: entry.controller(),
            options: BTreeSet::from([OptionalEffectChoice::Take, OptionalEffectChoice::Decline]),
        })));
        Ok(self.pending_resolution_decision())
    }
    pub(super) fn begin_resolution_choice(
        &mut self,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        match instruction {
            Instruction::ResolveCounterKeyword(
                crate::program::CounterKeyword::SuspendLastCounter,
            )
            | Instruction::FlipCoin { .. }
            | Instruction::Cascade
            | Instruction::CascadeBottom
            | Instruction::MadnessCast
            | Instruction::MiracleCast(_)
            | Instruction::CastFoundSpell(_)
            | Instruction::CastDefeatedSiege
            | Instruction::ReboundCast(_)
            | Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferSourceCounters { .. } => {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let (chooser, available, cast) = match instruction {
                    Instruction::OptionalEffect { chooser }
                    | Instruction::OptionalSequence { chooser, .. } => {
                        if (*chooser == ResolvingPlayer::CombatDamageSourceController
                            && self.combat_damage_source_controller()?.is_none())
                            || (*chooser == ResolvingPlayer::HighestEntrantController
                                && self.highest_entrant_controller()?.is_none())
                        {
                            let count = match instruction {
                                Instruction::OptionalSequence { instructions, .. } => *instructions,
                                _ => 1,
                            };
                            let next = cursor
                                .instruction
                                .checked_add(1)
                                .and_then(|n| n.checked_add(count))
                                .ok_or(EffectError::InconsistentProgram)?;
                            self.resolution_cursor_mut()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .instruction = next;
                            return Ok(true);
                        }

                        let player = self.resolving_player_identity_at(*chooser, Some(&cursor))?;
                        let is_sequence =
                            matches!(instruction, Instruction::OptionalSequence { .. });
                        let operation = self.optional_following_instruction(&cursor)?;
                        let available = self.optional_effect_available(operation, is_sequence)?;
                        (player, available, None)
                    }
                    Instruction::OfferSourceCounters { .. } => {
                        let source = self.trigger_source_for_counters()?;
                        let available = self
                            .window
                            .cards()
                            .get(source)
                            .is_some_and(|o| o.zone() == ObjectZone::Battlefield);
                        (
                            self.resolving_player(ResolvingPlayer::Controller)?,
                            available,
                            None,
                        )
                    }
                    _ => {
                        if matches!(instruction, Instruction::FlipCoin { .. }) {
                            let chooser = self.resolving_player(ResolvingPlayer::Controller)?;
                            self.pending_work =
                                Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                                    rebound_or_buyback: false,
                                    cast: None,
                                    cursor,
                                    chooser,
                                    options: BTreeSet::new(),
                                })));
                            return Ok(true);
                        }
                        if matches!(instruction, Instruction::CascadeBottom) {
                            let ids = self.cascade_remaining(cursor.entry)?;
                            if ids.len() <= 1 {
                                self.return_cascade_cards(&cursor, &ids, append)?;
                            } else {
                                let chooser = self.resolving_player(ResolvingPlayer::Controller)?;
                                self.pending_work = Some(PendingWork::OptionalEffect(Box::new(
                                    OptionalEffectWork {
                                        rebound_or_buyback: false,
                                        cast: None,
                                        cursor,
                                        chooser,
                                        options: BTreeSet::new(),
                                    },
                                )));
                            }
                            return Ok(true);
                        }
                        if matches!(instruction, Instruction::Cascade) {
                            self.exile_for_cascade(&cursor, append)?;
                        }
                        let Some(cast) = self.exiled_cast_offer(instruction)? else {
                            if matches!(instruction, Instruction::MadnessCast) {
                                self.finish_declined_madness(&cursor, append)?;
                            }
                            self.advance_instruction()?;
                            return Ok(true);
                        };
                        (self.resolving_cast_actor(instruction)?, true, Some(cast))
                    }
                };
                let mut options = BTreeSet::from([OptionalEffectChoice::Decline]);
                if available {
                    options.insert(OptionalEffectChoice::Take);
                }
                self.pending_work =
                    Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                        rebound_or_buyback: false,
                        cast,
                        cursor,
                        chooser,
                        options,
                    })));
                self.skip_unpayable_optional_after_departure()?;
            }
            Instruction::OfferDynamicManaPayment {
                payer,
                fixed,
                extra_generic,
                paid_destination,
                declined_destination,
            } => {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .controller();
                let (defines_x, x_factor) = if *extra_generic == ValueExpr::chosen_x() {
                    (true, RulesInteger::from(1))
                } else if let ValueExpr::Scale { expr, factor } = extra_generic
                    && **expr == ValueExpr::chosen_x()
                    && factor.is_positive()
                {
                    (true, factor.clone())
                } else {
                    (false, RulesInteger::from(1))
                };
                let mut cost = fixed.clone();
                if !defines_x {
                    let extra = self
                        .resolve_value_expr(controller, extra_generic)?
                        .max(0.into());
                    cost.generic = cost.generic.plus(&extra);
                }
                let payer = self.resolving_player_identity_at(*payer, Some(&cursor))?;
                self.start_resolution_payment(
                    ResolutionPaymentWork {
                        variable: defines_x,
                        defines_x,
                        x_factor,
                        record_last_action: true,
                        restricted_colors: None,
                        cursor,
                        payer,
                        cost,
                        paid_destination: *paid_destination,
                        declined_destination: *declined_destination,
                    },
                    append,
                )?;
            }
            Instruction::OfferManaPayment {
                payer,
                cost,
                paid_destination,
                declined_destination,
            } => {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let payer = self.resolving_player_identity_at(*payer, Some(&cursor))?;
                let restricted_colors = self
                    .payloads
                    .get(&cursor.entry)
                    .and_then(StackPayload::instructions)
                    .and_then(|ops| ops.get(cursor.instruction + 1))
                    .is_some_and(|op| matches!(op, Instruction::ExtortDrain))
                    .then_some([ManaType::White, ManaType::Black]);
                self.start_resolution_payment(
                    ResolutionPaymentWork {
                        variable: false,
                        defines_x: false,
                        x_factor: 1.into(),
                        record_last_action: false,
                        restricted_colors,
                        cursor,
                        payer,
                        cost: cost.clone(),
                        paid_destination: *paid_destination,
                        declined_destination: *declined_destination,
                    },
                    append,
                )?;
            }
            Instruction::Myriad
            | Instruction::WatchNextSpellOrLoyaltyTwice
            | Instruction::WatchNextLoyaltyCopy
            | Instruction::CopyCapturedAbility
            | Instruction::CopyOnCast(_)
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::ModifyAnyNumberOfPermanents { .. }
            | Instruction::AffectPermanentTargets { .. }
            | Instruction::ExileOnePerPlayerUntilSourceLeaves
            | Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::DamageFromGrantingPermanent { .. }
            | Instruction::Populate
            | Instruction::Bolster(_)
            | Instruction::Amass { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::SacrificeRandomUntilLand
            | Instruction::Discard { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::LookTopChoose { .. }
            | Instruction::ChooseObjects { .. }
            | Instruction::ChooseOption { .. }
            | Instruction::DivideDamageAmongTargets { .. }
            | Instruction::DivideDamageExprAmongTargets { .. }
            | Instruction::BecomeCopy { .. }
            | Instruction::BecomeCopyFixedTargets { .. }
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::RollDie { .. }
            | Instruction::FlipCoins { .. }
            | Instruction::ChooseValue { .. }
            | Instruction::ChooseOptionForPlayers { .. }
            | Instruction::Vote { .. }
            | Instruction::Incubate { .. }
            | Instruction::Connive { .. }
            | Instruction::ConniveFinalize { .. }
            | Instruction::Discover { .. }
            | Instruction::Clash { .. }
            | Instruction::RingTemptsYou
            | Instruction::VentureIntoDungeon { .. }
            | Instruction::TakeInitiative { .. }
            | Instruction::UnlockDoor { .. }
            | Instruction::Behold { .. }
            | Instruction::ManifestDread
            | Instruction::Monstrosity { .. }
            | Instruction::Fight { .. }
            | Instruction::Exchange(_)
            | Instruction::CopyStackObject { .. }
            | Instruction::CastLinkedFaceDownFree
            | Instruction::Ripple(_)
            | Instruction::Demonstrate
            | Instruction::Enlist
            | Instruction::ChooseOneLastMoved { .. }
            | Instruction::ZoneOp(_)
            | Instruction::DrawCards(_)
            | Instruction::MutateObject(_)
            | Instruction::AdjustLife(_)
            | Instruction::DealDamage(_)
            | Instruction::MutatePlayer(_)
            | Instruction::ApplyContinuous(_)
            | Instruction::PromptChoice(_)
            | Instruction::BranchCondition { .. }
            | Instruction::ForEach { .. }
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::BindEnv { .. }
            | Instruction::SetObjectTag { .. }
            | Instruction::UpdateStateTrack { .. }
            | Instruction::SideDeckOp(_)
            | Instruction::CreateTokenCopy { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::CreateAuraTokenAttached { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::RepeatLoop { .. }
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
            | Instruction::RevealUntilMatch { .. }
            | Instruction::ResolvePowerAbility(_)
            | Instruction::CreateTokenAndAttach { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::CreateAttackingTokens { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::AdditionalCombat { .. }
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ExileDefeatedSiege
            | Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource
            | Instruction::Proliferate
            | Instruction::DamageTargetControllersCreatures { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::FixedTargetClause(_)
            | Instruction::FightFixedTargets
            | Instruction::BindOrderedPermanents { .. }
            | Instruction::PutHandPermanentsOntoBattlefield { .. }
            | Instruction::MayRevealLibraryTop
            | Instruction::ManifestTop { .. }
            | Instruction::ChooseFoundSpell
            | Instruction::RequireCondition(_)
            | Instruction::Encore
            | Instruction::ReselectAttackDefender
            | Instruction::SetBlockDeclarations(_)
            | Instruction::AdditionalLoyaltyActivation
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::ReturnCardsDestroyedThisWay
            | Instruction::ExileTopForPlay { .. }
            | Instruction::ScheduleCreatedTokenExile(_)
            | Instruction::ScheduleCreatedTokenSacrifice
            | Instruction::ExileDelayedObjects(_)
            | Instruction::GrantFlashback(_)
            | Instruction::ReturnEarthbentCard
            | Instruction::AddMana(_)
            | Instruction::PairWithCreature { .. }
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::EndStepCast { .. }
            | Instruction::SacrificeDelayedObjects(_)
            | Instruction::ScheduleArtifactReturn(_)
            | Instruction::ReturnDelayedGraveyardCard(_)
            | Instruction::ReturnDelayedGraveyardCardTransformed(_)
            | Instruction::ReturnDelayedPermanentToHand(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::ResolveTransfer(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::CounterWardSource
            | Instruction::CounterTriggeringStackObject
            | Instruction::Attach(_)
            | Instruction::ChooseRevealedHandCard { .. }
            | Instruction::ReturnGraveyardCreatureWithCorpse(_)
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. }
            | Instruction::CounterSpell
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::ChooseNewTargets
            | Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::Earthbend { .. }
            | Instruction::GrantProtectionChosenColor(_)
            | Instruction::SearchLibrary { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::ForEachPlayer { .. }
            | Instruction::OfferAnyPlayer { .. }
            | Instruction::DealDamageToAnnouncedTargets { .. }
            | Instruction::GrantChosenProtection { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::RevealTopTwoPiles { .. }
            | Instruction::RevealUntilMatchExt { .. }
            | Instruction::CreateTokenCopyExt { .. }
            | Instruction::MoveCounters { .. }
            | Instruction::CounterMatchingStackObjects { .. }
            | Instruction::TransferAllCountersFromTriggeringLki { .. }
            | Instruction::AddManaForPlayer { .. }
            | Instruction::SelectExtremeLifePlayer { .. } => return Ok(false),
        }
        Ok(true)
    }

    pub(super) fn mana_activation_parent(&self, actor: SeatId) -> Result<ManaParent, EffectError> {
        if let Some(PendingWork::Combat(cursor)) = &self.pending_work
            && cursor.attack_payment.is_some()
        {
            if cursor.frame.chooser() != actor {
                return Err(EffectError::WrongChooser);
            }
            return Ok(ManaParent::CombatPayment(Box::new(cursor.clone())));
        }

        if let Some(PendingWork::CastRecovery(frame)) = &self.pending_work {
            if frame.chooser != actor {
                return Err(EffectError::WrongChooser);
            }
            let parent = self
                .deferred_mana_cast
                .as_ref()
                .ok_or(EffectError::InconsistentProgram)?
                .parent_entry;
            return Ok(ManaParent::Recovery(frame.as_ref().clone(), parent));
        }
        if let Some(PendingWork::ResolutionPayment(work)) = &self.pending_work {
            if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
                return Err(EffectError::WrongResolutionPaymentFrame);
            }
            if work.payer != actor {
                return Err(EffectError::WrongChooser);
            }
            return Ok(ManaParent::Payment(work.as_ref().clone()));
        }
        self.ensure_resolution_or_idle()?;
        self.window.validate_action_actor(actor)?;
        Ok(ManaParent::Priority)
    }

    fn trigger_source_for_counters(&self) -> Result<ObjectId, EffectError> {
        match self.window.resolving().map(|entry| entry.kind()) {
            Some(StackKind::TriggeredAbility { source }) => Ok(source.id()),
            _ => Err(EffectError::InconsistentProgram),
        }
    }
    pub(super) fn add_source_counters(
        &mut self,
        source: ObjectId,
        entry: StackEntryId,
        instruction: usize,
        amount: RulesInteger,
        placement: CounterPlacement,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let added = PowerToughnessCounters {
            plus_one: amount,
            minus_one: 0.into(),
        };
        let added = self.adjusted_pt_counters(&before.view().evaluated, source, added, None)?;
        if added.plus_one.is_positive() {
            self.characteristics.place_counters(source, &added)?;
        }
        if placement == CounterPlacement::Renown {
            self.characteristics.renowned.insert(source);
        }
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |old, new, _| {
                let mut changes = Vec::new();
                match placement {
                    CounterPlacement::Renown => {
                        changes.push(RulesOccurrence::BecameRenowned { object: source })
                    }
                    CounterPlacement::Evolve if added.plus_one.is_positive() => {
                        changes.push(RulesOccurrence::CreatureEvolved { object: source })
                    }
                    CounterPlacement::Mentor(mentor) => {
                        changes.push(RulesOccurrence::CreatureMentored {
                            source: mentor,
                            target: source,
                        })
                    }
                    _ => {}
                }
                if added.plus_one.is_positive() {
                    changes.push(RulesOccurrence::CountersPlaced {
                        object: source,
                        before: old.characteristics.counters(source),
                        after: new.characteristics.counters(source),
                        added,
                        before_appearance: Box::new(
                            old.appearance(source)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            new.appearance(source)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(changes)
            },
        )
    }
    pub(super) fn resume_optional_effect(
        &self,
        expected: u64,
        frame: &OptionalEffectFrame,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_resolution_decision() != Some(EffectDecision::OptionalEffect(frame.clone()))
        {
            return Err(EffectError::WrongOptionalEffectFrame);
        }
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let mut mana_abilities = vec![];
        let (choice, cast_payment) = match reply {
            DecisionReply::ImpulseCast {
                target,
                payment,
                mana_abilities: planned,
            } if frame.cast.as_ref().is_some_and(|offer| {
                matches!(
                    offer.ability,
                    ExileCastAbility::Miracle
                        | ExileCastAbility::Impulse
                        | ExileCastAbility::RevealedLibrary
                        | ExileCastAbility::Commander
                        | ExileCastAbility::FoundSpell
                        | ExileCastAbility::MatchingHand
                        | ExileCastAbility::Siege
                        | ExileCastAbility::TargetedGraveyard
                )
            }) =>
            {
                mana_abilities = planned;
                (OptionalEffectChoice::Take, Some((target, payment)))
            }
            DecisionReply::CascadeCast { target, payment }
                if frame
                    .cast
                    .as_ref()
                    .is_some_and(|offer| offer.ability == ExileCastAbility::Cascade) =>
            {
                (OptionalEffectChoice::Take, Some((target, payment)))
            }
            DecisionReply::MadnessCast {
                target,
                payment,
                mana_abilities: planned,
            } if frame
                .cast
                .as_ref()
                .is_some_and(|offer| offer.ability == ExileCastAbility::Madness) =>
            {
                mana_abilities = planned;
                (OptionalEffectChoice::Take, Some((target, payment)))
            }
            DecisionReply::OptionalEffect(value) => (value, None),
            DecisionReply::ReboundCast { target, payment }
                if frame
                    .cast
                    .as_ref()
                    .is_some_and(|offer| offer.ability == ExileCastAbility::Rebound) =>
            {
                (OptionalEffectChoice::Take, Some((target, payment)))
            }
            DecisionReply::SuspendCast { target, payment } if frame.suspended_cast().is_some() => {
                (OptionalEffectChoice::Take, Some((target, payment)))
            }
            _ => return Err(EffectError::WrongDecisionKind),
        };
        if !frame.options.contains(&choice) {
            return Err(EffectError::IllegalOptionalEffect);
        }
        let mut next = self.clone();
        let Some(PendingWork::OptionalEffect(mut work)) = next.pending_work.take() else {
            return Err(EffectError::WrongOptionalEffectFrame);
        };
        let mut append = RulesJournal::default();
        let mut continue_current_instruction = false;
        let destination = if frame.rebound_or_buyback {
            let Some(StackPayload::Instructions(payload)) = next.payloads.get_mut(&frame.entry)
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.rebound_over_buyback = Some(choice == OptionalEffectChoice::Take);
            frame.instruction
        } else {
            let instruction = next
                .payloads
                .get(&frame.entry)
                .and_then(StackPayload::instructions)
                .and_then(|program| program.get(frame.instruction))
                .ok_or(EffectError::InconsistentProgram)?;
            let is_madness = matches!(instruction, Instruction::MadnessCast);
            match instruction {
                Instruction::Cascade
                | Instruction::MadnessCast
                | Instruction::MiracleCast(_)
                | Instruction::CastFoundSpell(_)
                | Instruction::CastDefeatedSiege
                | Instruction::ReboundCast(_)
                | Instruction::ResolveCounterKeyword(
                    crate::program::CounterKeyword::SuspendLastCounter,
                ) => {
                    if choice == OptionalEffectChoice::Take {
                        let offer = frame
                            .cast
                            .as_ref()
                            .ok_or(EffectError::IllegalOptionalEffect)?;
                        let (target, payment) = cast_payment.unwrap_or_default();
                        let offer = if payment.split_half == Some(crate::program::SplitHalf::Right)
                        {
                            offer.split_right.as_deref()
                        } else if payment.adventure {
                            offer.adventure.as_deref()
                        } else {
                            offer.normal_available.then_some(offer)
                        }
                        .ok_or(EffectError::IllegalOptionalEffect)?;
                        if !(if payment.use_alternative && offer.alternative_targetless {
                            target.is_none()
                                && payment.modes.is_empty()
                                && payment.permanent_targets.is_empty()
                                && payment.fixed_targets.is_empty()
                        } else {
                            match (&offer.targets, target) {
                                (None, None) => true,
                                (Some(choices), Some(target)) => choices.contains(&target),
                                _ => false,
                            }
                        }) {
                            return Err(EffectError::IllegalTarget);
                        }
                        let (cast, _, journal) = next
                            .cast_in_context(
                                next.revision,
                                actor,
                                offer.spell,
                                target,
                                payment,
                                turn,
                                Some(ResolvingCast {
                                    parent: frame.entry,
                                    kind: offer.ability,
                                    mana_abilities: &mana_abilities,
                                }),
                            )?
                            .into_parts();
                        next = cast;
                        append.append(journal);
                    } else if is_madness {
                        next.finish_declined_madness(&work.cursor, &mut append)?;
                    }
                    frame
                        .instruction
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?
                }
                Instruction::OfferSourceCounters {
                    amount,
                    placed_destination,
                    declined_destination,
                } => {
                    let destination = if choice == OptionalEffectChoice::Take {
                        *placed_destination
                    } else {
                        *declined_destination
                    };
                    if choice == OptionalEffectChoice::Take && amount.is_positive() {
                        next.add_source_counters(
                            next.trigger_source_for_counters()?,
                            frame.entry,
                            frame.instruction,
                            amount.clone(),
                            CounterPlacement::Ordinary,
                            &mut append,
                        )?;
                    }
                    destination
                }
                Instruction::CopyOnCast(
                    crate::program::CastCopyKeyword::OfferCopyToOtherPlayers,
                ) => {
                    next.accept_copy_offer(
                        frame.entry,
                        actor,
                        choice == OptionalEffectChoice::Take,
                    )?;
                    continue_current_instruction = true;
                    frame.instruction
                }
                Instruction::OptionalSequence { instructions, .. } => {
                    let next = work
                        .cursor
                        .instruction
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                    if choice == OptionalEffectChoice::Take {
                        next
                    } else {
                        work.cursor.results.last_action_succeeded = false;
                        work.cursor.results.last_action_count = Some(0.into());
                        next.checked_add(*instructions)
                            .ok_or(EffectError::InconsistentProgram)?
                    }
                }
                Instruction::OptionalEffect { .. } => {
                    let increment = if choice == OptionalEffectChoice::Take {
                        1
                    } else {
                        work.cursor.results.last_action_succeeded = false;
                        work.cursor.results.last_action_count = Some(0.into());
                        2
                    };
                    work.cursor
                        .instruction
                        .checked_add(increment)
                        .ok_or(EffectError::InconsistentProgram)?
                }
                _ => return Err(EffectError::InconsistentProgram),
            }
        };
        next.revision = revision;
        if next.deferred_mana_cast.is_some() {
            if !frame.rebound_or_buyback {
                work.cursor.instruction = destination;
            }
            let deferred = next
                .deferred_mana_cast
                .as_mut()
                .ok_or(EffectError::InconsistentProgram)?;
            if deferred.kind == ExileCastAbility::Impulse {
                work.cursor.impulse.decided = false;
                work.cursor.impulse.cast = false;
            }
            deferred.parent_cursor = Some(work.cursor);
            let damage = std::mem::take(&mut deferred.damage_journal);
            append.records.insert(
                0,
                crate::occurrence::RulesRecord::OptionalEffectOutcome {
                    entry: frame.entry,
                    instruction: frame.instruction,
                    chooser: actor,
                    outcome: choice,
                    next: destination,
                },
            );
            return next.progress_rules(damage, vec![], vec![], DrawJournal::default(), append);
        }
        next.pending_work = Some(PendingWork::Resolution(work.cursor));
        if !frame.rebound_or_buyback && !continue_current_instruction {
            next.set_instruction(destination)?;
        }
        append.records.insert(
            0,
            crate::occurrence::RulesRecord::OptionalEffectOutcome {
                entry: frame.entry,
                instruction: frame.instruction,
                chooser: actor,
                outcome: choice,
                next: destination,
            },
        );
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }

    pub(super) fn resume_resolution_payment(
        &self,
        expected: u64,
        frame: &ResolutionPaymentFrame,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_resolution_decision()
            != Some(EffectDecision::ResolutionPayment(frame.clone()))
        {
            return Err(EffectError::WrongResolutionPaymentFrame);
        }
        if actor != frame.payer {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ResolutionPayment(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let Some(PendingWork::ResolutionPayment(mut work)) = next.pending_work.take() else {
            return Err(EffectError::WrongResolutionPaymentFrame);
        };
        let mut append = RulesJournal::default();
        let outcome = match reply {
            ResolutionPaymentReply::Pay(allocation) => {
                if work.restricted_colors.is_some_and(|colors| {
                    allocation
                        .spend
                        .iter()
                        .any(|spend| !colors.contains(&spend.mana_type))
                }) {
                    return Err(EffectError::InvalidManaPayment(
                        crate::mana::ManaPaymentError::MissingRequired,
                    ));
                }
                let mut paid_x: Option<RulesInteger> = None;
                if work.defines_x {
                    let amount = allocation
                        .spend
                        .iter()
                        .fold(RulesInteger::from(0), |total, row| total.plus(&row.amount));
                    let fixed_total = work.cost.total();
                    if amount < fixed_total {
                        return Err(EffectError::InvalidManaPayment(
                            crate::mana::ManaPaymentError::WrongTotal,
                        ));
                    }
                    let extra = amount.minus(&fixed_total);
                    let x = if work.x_factor > 1.into() {
                        let factor =
                            work.x_factor
                                .to_usize()
                                .ok_or(EffectError::InvalidManaPayment(
                                    crate::mana::ManaPaymentError::WrongTotal,
                                ))?;
                        let extra_u = extra.to_usize().ok_or(EffectError::InvalidManaPayment(
                            crate::mana::ManaPaymentError::WrongTotal,
                        ))?;
                        if factor == 0 || extra_u % factor != 0 {
                            return Err(EffectError::InvalidManaPayment(
                                crate::mana::ManaPaymentError::WrongTotal,
                            ));
                        }
                        RulesInteger::from_usize(extra_u / factor)
                    } else {
                        extra.clone()
                    };
                    work.cost.generic = work.cost.generic.plus(&extra);
                    if work.record_last_action {
                        work.cursor.results.last_action_count = Some(x.clone());
                        work.cursor.results.last_action_succeeded = x.is_positive();
                    }
                    paid_x = Some(x);
                } else if work.variable {
                    let amount = allocation
                        .spend
                        .iter()
                        .fold(RulesInteger::from(0), |total, row| total.plus(&row.amount));
                    work.cost = ManaCost::generic(amount.clone());
                    work.cursor.impulse.contributions = Some(
                        work.cursor
                            .impulse
                            .contributions
                            .as_ref()
                            .map_or(amount.clone(), |total| total.plus(&amount)),
                    );
                } else if work.record_last_action {
                    work.cursor.results.last_action_count = Some(work.cost.total());
                    work.cursor.results.last_action_succeeded = true;
                }
                let spend = prepare_payment(&work.cost, Some(&allocation), |kind| {
                    self.unrestricted_mana(work.payer, kind)
                })
                .map_err(EffectError::InvalidManaPayment)?;
                let payment = ManaPaymentRecord {
                    from_lots: allocation.from_lots,
                    source: work.cursor.entry,
                    payer: work.payer,
                    cost: work.cost.clone(),
                    spend,
                    restricted_mana: Vec::new(),
                };
                next.debit_mana_at_cut(&payment, false, turn, &mut append)?;
                if let Some(x) = paid_x {
                    next.window.set_chosen_x(work.cursor.entry, x)?;
                }
                ResolutionPaymentOutcome::Paid(payment)
            }
            ResolutionPaymentReply::Decline => {
                if work.record_last_action {
                    work.cursor.results.last_action_count = Some(0.into());
                    work.cursor.results.last_action_succeeded = false;
                }
                ResolutionPaymentOutcome::Declined
            }
        };
        next.revision = revision;
        next.finish_resolution_payment(*work, outcome, &mut append)?;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }

    fn finish_resolution_payment(
        &mut self,
        mut work: ResolutionPaymentWork,
        outcome: ResolutionPaymentOutcome,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let frame = work.frame(self.revision);
        if work.record_last_action && matches!(outcome, ResolutionPaymentOutcome::UnpaidDeparted) {
            work.cursor.results.last_action_count = Some(0.into());
            work.cursor.results.last_action_succeeded = false;
        }
        let mut destination = match outcome {
            ResolutionPaymentOutcome::Paid(_) => work.paid_destination,
            ResolutionPaymentOutcome::Declined | ResolutionPaymentOutcome::UnpaidDeparted => {
                work.declined_destination
            }
        };
        if work.variable && !work.defines_x {
            let seats = work
                .cursor
                .impulse
                .contribution_seats
                .as_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if seats.pop_front() != Some(work.payer) {
                return Err(EffectError::WrongResolutionFrame);
            }
            if !seats.is_empty() {
                destination = frame.instruction;
            }
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        if destination != frame.instruction {
            self.set_instruction(destination)?;
        }
        append
            .records
            .push(crate::occurrence::RulesRecord::ResolutionPaymentOutcome {
                entry: frame.entry,
                instruction: frame.instruction,
                payer: frame.payer,
                cost: work.cost,
                restricted_colors: frame.restricted_colors,
                outcome,
                next: destination,
            });
        Ok(())
    }
}
