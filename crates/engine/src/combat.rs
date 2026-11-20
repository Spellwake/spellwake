//! Ordinary creature declarations, damage assignments and current relationships.
//! Granted haste/vigilance affect declarations; other keywords, attack costs,
//! teams and limited range remain unimplemented.
#[cfg(test)]
mod tests;
use crate::{
    damage::DamageRecipient,
    object_identity::{ControlEpoch, GameObject, ObjectId},
    priority::SeatId,
    turn::TurnPosition,
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

/// Explicit native table policy. Unimplemented policies cannot be silently chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatRules {
    UnrestrictedFreeForAll,
}
impl CombatRules {
    /// Query the canonical validated participants in order after this player.
    /// This helper does not authorize a caller-supplied participant list.
    pub fn opponents(
        &self,
        participants: &[SeatId],
        player: SeatId,
    ) -> Result<Vec<SeatId>, CombatError> {
        let start = participants
            .iter()
            .position(|&seat| seat == player)
            .ok_or(CombatError::UnknownPlayer)?;
        match self {
            Self::UnrestrictedFreeForAll => Ok(participants
                .iter()
                .cycle()
                .skip(start + 1)
                .take(participants.len() - 1)
                .copied()
                .collect()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackDeclaration {
    pub attacker: ObjectId,
    pub defender: SeatId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockDeclaration {
    pub blocker: ObjectId,
    pub attacker: ObjectId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationOrigin {
    PlayerReply,
    ForcedEmpty,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackTap {
    pub before: GameObject,
    pub after: GameObject,
}
/// Current-operation native facts, not a public projection or a full event journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatDeclarationRecord {
    Attackers {
        player: SeatId,
        origin: DeclarationOrigin,
        attackers: Vec<AttackDeclaration>,
        taps: Vec<AttackTap>,
    },
    Blockers {
        player: SeatId,
        origin: DeclarationOrigin,
        blockers: Vec<BlockDeclaration>,
    },
}

/// One positive contribution selected by the source's actual controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatDamageAssignment {
    pub source: ObjectId,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}

/// A forced assignment is kernel work, not a fabricated player submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOrigin {
    PlayerReply,
    Forced,
}

/// Assignment evidence emitted only by the current successful operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatAssignmentRecord {
    pub player: SeatId,
    pub origin: AssignmentOrigin,
    pub assignments: Vec<CombatDamageAssignment>,
}

/// Exact current power and recipient domain for one positive assignable source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAssignmentDomain {
    source: ObjectId,
    power: RulesInteger,
    recipients: Vec<DamageRecipient>,
}
impl DamageAssignmentDomain {
    pub(crate) fn new(
        source: ObjectId,
        power: RulesInteger,
        recipients: Vec<DamageRecipient>,
    ) -> Self {
        Self {
            source,
            power,
            recipients,
        }
    }
    pub fn source(&self) -> ObjectId {
        self.source
    }
    pub fn power(&self) -> &RulesInteger {
        &self.power
    }
    pub fn recipients(&self) -> &[DamageRecipient] {
        &self.recipients
    }
}

/// Whole-actor assignment domain reconstructed from the current owned world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAssignmentsDecision {
    pub(crate) revision: u64,
    chooser: SeatId,
    position: TurnPosition,
    domains: Vec<DamageAssignmentDomain>,
}
impl DamageAssignmentsDecision {
    pub(crate) fn new(
        revision: u64,
        chooser: SeatId,
        position: TurnPosition,
        domains: Vec<DamageAssignmentDomain>,
    ) -> Self {
        Self {
            revision,
            chooser,
            position,
            domains,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn position(&self) -> TurnPosition {
        self.position
    }
    pub fn domains(&self) -> &[DamageAssignmentDomain] {
        &self.domains
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatError {
    UnknownPlayer,
    WrongFrame,
    InvalidContext,
    DuplicateAttacker,
    IllegalAttacker,
    IllegalDefender,
    DuplicateBlocker,
    IllegalBlocker,
    IllegalBlockTarget,
    AlreadyDeclared,
    DuplicateAssignment,
    IllegalDamageSource,
    IllegalDamageRecipient,
    InvalidDamageAmount,
    IncorrectDamageTotal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackersDecision {
    pub(crate) revision: u64,
    chooser: SeatId,
    attackers: Vec<ObjectId>,
    defenders: Vec<SeatId>,
}
impl AttackersDecision {
    pub(crate) fn new(
        revision: u64,
        chooser: SeatId,
        attackers: Vec<ObjectId>,
        defenders: Vec<SeatId>,
    ) -> Self {
        Self {
            revision,
            chooser,
            attackers,
            defenders,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn eligible_attackers(&self) -> &[ObjectId] {
        &self.attackers
    }
    pub fn defenders(&self) -> &[SeatId] {
        &self.defenders
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockersDecision {
    pub(crate) revision: u64,
    chooser: SeatId,
    blockers: Vec<ObjectId>,
    attackers: Vec<ObjectId>,
}
impl BlockersDecision {
    pub(crate) fn new(
        revision: u64,
        chooser: SeatId,
        blockers: Vec<ObjectId>,
        attackers: Vec<ObjectId>,
    ) -> Self {
        Self {
            revision,
            chooser,
            blockers,
            attackers,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn eligible_blockers(&self) -> &[ObjectId] {
        &self.blockers
    }
    pub fn attackers(&self) -> &[ObjectId] {
        &self.attackers
    }
}
/// Privileged readonly domains derived by the executor; replies contain choices only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatDecision {
    Attackers(AttackersDecision),
    Blockers(BlockersDecision),
    DamageAssignments(DamageAssignmentsDecision),
}
impl CombatDecision {
    pub fn revision(&self) -> u64 {
        match self {
            Self::Attackers(f) => f.revision(),
            Self::Blockers(f) => f.revision(),
            Self::DamageAssignments(f) => f.revision(),
        }
    }
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::Attackers(f) => f.chooser(),
            Self::Blockers(f) => f.chooser(),
            Self::DamageAssignments(f) => f.chooser(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CombatCursor {
    pub frame: CombatDecision,
    pub remaining_actors: Vec<SeatId>,
    pub assignments: Vec<CombatDamageAssignment>,
}

/// Current incarnation and interval of control, not another mutable card snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combatant {
    object: ObjectId,
    controller: SeatId,
    control_epoch: ControlEpoch,
}
impl Combatant {
    pub(crate) fn from_object(object: &GameObject) -> Option<Self> {
        Some(Self {
            object: object.id(),
            controller: object.controller()?,
            control_epoch: object.control_epoch()?,
        })
    }
    pub fn object(&self) -> ObjectId {
        self.object
    }
    pub fn controller(&self) -> SeatId {
        self.controller
    }
    pub fn control_epoch(&self) -> ControlEpoch {
        self.control_epoch
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    NotDeclared,
    Unblocked,
    Blocked,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackingCreature {
    combatant: Combatant,
    defender: SeatId,
    block_status: BlockStatus,
}
impl AttackingCreature {
    pub fn combatant(&self) -> Combatant {
        self.combatant
    }
    pub fn defender(&self) -> SeatId {
        self.defender
    }
    pub fn block_status(&self) -> BlockStatus {
        self.block_status
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingCreature {
    combatant: Combatant,
    attacker: Option<ObjectId>,
}
impl BlockingCreature {
    pub fn combatant(&self) -> Combatant {
        self.combatant
    }
    pub fn attacker(&self) -> Option<ObjectId> {
        self.attacker
    }
}

/// One executor-owned current combat. Historical attack presence and established
/// blocked status intentionally survive removal of their former counterparts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatState {
    attackers: BTreeMap<ObjectId, AttackingCreature>,
    blockers: BTreeMap<ObjectId, BlockingCreature>,
    had_attackers: bool,
}
impl CombatState {
    pub(crate) fn new(attackers: Vec<(Combatant, SeatId)>) -> Result<Self, CombatError> {
        let mut current = BTreeMap::new();
        for (combatant, defender) in attackers {
            if current
                .insert(
                    combatant.object(),
                    AttackingCreature {
                        combatant,
                        defender,
                        block_status: BlockStatus::NotDeclared,
                    },
                )
                .is_some()
            {
                return Err(CombatError::DuplicateAttacker);
            }
        }
        Ok(Self {
            had_attackers: !current.is_empty(),
            attackers: current,
            blockers: BTreeMap::new(),
        })
    }
    pub fn attackers(&self) -> impl Iterator<Item = &AttackingCreature> {
        self.attackers.values()
    }
    pub fn blockers(&self) -> impl Iterator<Item = &BlockingCreature> {
        self.blockers.values()
    }
    pub fn attacker(&self, object: ObjectId) -> Option<&AttackingCreature> {
        self.attackers.get(&object)
    }
    pub fn blocker(&self, object: ObjectId) -> Option<&BlockingCreature> {
        self.blockers.get(&object)
    }
    pub fn had_attackers(&self) -> bool {
        self.had_attackers
    }

    /// At the end of declarations, an attack without blockers becomes unblocked
    /// even if its defender departed before choosing (509.1h, 800.4e).
    pub(crate) fn finish_absent_defenders(&mut self, participants: &[SeatId]) {
        for attacker in self.attackers.values_mut() {
            if attacker.block_status == BlockStatus::NotDeclared
                && !participants.contains(&attacker.defender)
            {
                attacker.block_status = BlockStatus::Unblocked;
            }
        }
    }

    pub(crate) fn declare_blocks(
        &mut self,
        defender: SeatId,
        blockers: Vec<(Combatant, ObjectId)>,
    ) -> Result<(), CombatError> {
        if self
            .attackers
            .values()
            .any(|a| a.defender == defender && a.block_status != BlockStatus::NotDeclared)
        {
            return Err(CombatError::AlreadyDeclared);
        }
        let mut selected = BTreeSet::new();
        for &(blocker, attacker) in &blockers {
            if blocker.controller() != defender {
                return Err(CombatError::IllegalBlocker);
            }
            if !selected.insert(blocker.object()) || self.blockers.contains_key(&blocker.object()) {
                return Err(CombatError::DuplicateBlocker);
            }
            if !self
                .attackers
                .get(&attacker)
                .is_some_and(|a| a.defender == defender)
            {
                return Err(CombatError::IllegalBlockTarget);
            }
        }
        for attacker in self
            .attackers
            .values_mut()
            .filter(|a| a.defender == defender)
        {
            attacker.block_status = BlockStatus::Unblocked;
        }
        for (combatant, attacker) in blockers {
            if let Some(target) = self.attackers.get_mut(&attacker) {
                target.block_status = BlockStatus::Blocked;
            }
            self.blockers.insert(
                combatant.object(),
                BlockingCreature {
                    combatant,
                    attacker: Some(attacker),
                },
            );
        }
        Ok(())
    }
    /// The executor supplies its canonical current creature/controller/epoch check.
    /// Opposite membership and historical status are not recomputed from link count.
    pub(crate) fn reconcile(&mut self, mut current: impl FnMut(Combatant) -> bool) {
        self.attackers.retain(|_, a| current(a.combatant));
        self.blockers.retain(|_, b| current(b.combatant));
        for blocker in self.blockers.values_mut() {
            if blocker
                .attacker
                .is_some_and(|id| !self.attackers.contains_key(&id))
            {
                blocker.attacker = None;
            }
        }
    }
}
