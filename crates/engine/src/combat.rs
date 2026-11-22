//! Ordinary creature declarations, damage assignments and current relationships.
//! Block decisions expose current blocker-to-attacker restrictions and group minima.
mod attack_requirements;
mod banding;
mod block_requirements;
pub use banding::BandQuality;
pub(crate) use banding::controls_opposing_assignment;
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
    /// Defending player, including the controller of an attacked permanent.
    pub defender: SeatId,
    /// The attacked permanent (planeswalker or battle), or None for a direct attack.
    pub defending_planeswalker: Option<ObjectId>,
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
    trample_lethal: BTreeMap<ObjectId, RulesInteger>,
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
            trample_lethal: BTreeMap::new(),
        }
    }
    pub fn source(&self) -> ObjectId {
        self.source
    }
    pub fn power(&self) -> &RulesInteger {
        &self.power
    }
    pub(crate) fn with_trample_lethal(mut self, lethal: BTreeMap<ObjectId, RulesInteger>) -> Self {
        self.trample_lethal = lethal;
        self
    }
    /// Damage required on each blocker before assigning trample damage to the defender.
    pub fn trample_lethal(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.trample_lethal
    }
    pub fn recipients(&self) -> &[DamageRecipient] {
        &self.recipients
    }
}

/// Whole-actor assignment domain reconstructed from the current owned world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAssignmentsDecision {
    revision: u64,
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
    InsufficientTrampleAssignment,
}

/// Accumulated costs for creatures assigned to a defender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackCost {
    pub generic: RulesInteger,
    pub white_phyrexian: RulesInteger,
}
impl Default for AttackCost {
    fn default() -> Self {
        Self {
            generic: 0.into(),
            white_phyrexian: 0.into(),
        }
    }
}
impl AttackCost {
    pub(crate) fn add(&mut self, other: &Self) {
        self.generic = self.generic.plus(&other.generic);
        self.white_phyrexian = self.white_phyrexian.plus(&other.white_phyrexian);
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackersDecision {
    pub(crate) banding_attackers: BTreeSet<ObjectId>,
    band_qualities: BTreeMap<BandQuality, (BTreeSet<ObjectId>, BTreeSet<ObjectId>)>,
    pub(crate) capped_defenders: BTreeSet<SeatId>,
    pub(crate) scores_with_caps: BTreeMap<ObjectId, Vec<(crate::targets::TargetRef, usize)>>,
    pub(crate) defender_costs: BTreeMap<crate::targets::TargetRef, AttackCost>,
    pub(crate) payment: Option<AttackCost>,
    single_attacker: bool,
    forbidden: BTreeMap<ObjectId, BTreeSet<crate::targets::TargetRef>>,
    required: BTreeMap<ObjectId, BTreeSet<crate::targets::TargetRef>>,
    revision: u64,
    chooser: SeatId,
    attackers: Vec<ObjectId>,
    defenders: Vec<SeatId>,
    planeswalkers: Vec<ObjectId>,
    battles: Vec<ObjectId>,
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
            capped_defenders: BTreeSet::new(),
            banding_attackers: BTreeSet::new(),
            band_qualities: BTreeMap::new(),
            scores_with_caps: BTreeMap::new(),
            defender_costs: BTreeMap::new(),
            payment: None,
            single_attacker: false,
            forbidden: BTreeMap::new(),
            required: BTreeMap::new(),
            planeswalkers: Vec::new(),
            battles: Vec::new(),
        }
    }
    pub(crate) fn with_planeswalkers(mut self, planeswalkers: Vec<ObjectId>) -> Self {
        self.planeswalkers = planeswalkers;
        self
    }
    pub(crate) fn with_battles(mut self, battles: Vec<ObjectId>) -> Self {
        self.battles = battles;
        self
    }
    /// Battles protected by defending opponents.
    pub fn battles(&self) -> &[ObjectId] {
        &self.battles
    }
    /// Players that at most one creature may attack directly.
    pub fn capped_defenders(&self) -> &BTreeSet<SeatId> {
        &self.capped_defenders
    }
    /// Unit cost to assign one creature to a particular defender.
    pub fn defender_costs(&self) -> &BTreeMap<crate::targets::TargetRef, AttackCost> {
        &self.defender_costs
    }
    /// Locked total while the active player may activate mana abilities and pay.
    pub fn payment(&self) -> Option<&AttackCost> {
        self.payment.as_ref()
    }
    pub(crate) fn at_revision(&self, revision: u64) -> Self {
        let mut frame = self.clone();
        frame.revision = revision;
        frame
    }
    /// Opponent-controlled planeswalker incarnations eligible to be attacked.
    pub fn planeswalkers(&self) -> &[ObjectId] {
        &self.planeswalkers
    }
    /// Whether one creature at most may be declared.
    pub fn single_attacker(&self) -> bool {
        self.single_attacker
    }
    pub(crate) fn limit_to_one_attacker(&mut self, strongest: &BTreeSet<ObjectId>) {
        self.single_attacker = true;
        if !strongest.is_empty() {
            self.attackers.retain(|id| strongest.contains(id));
            self.required.retain(|id, _| strongest.contains(id));
        }
    }
    /// Defenders this particular creature is prohibited from attacking.
    pub fn forbidden_defenders(&self) -> &BTreeMap<ObjectId, BTreeSet<crate::targets::TargetRef>> {
        &self.forbidden
    }
    pub(crate) fn forbid_defender(
        &mut self,
        attacker: ObjectId,
        target: crate::targets::TargetRef,
    ) {
        self.forbidden.entry(attacker).or_default().insert(target);
    }
    pub(crate) fn require_attack(
        &mut self,
        attacker: ObjectId,
        targets: BTreeSet<crate::targets::TargetRef>,
    ) {
        self.required.insert(attacker, targets);
    }
    pub(crate) fn exclude_attacker(&mut self, attacker: ObjectId) {
        self.attackers.retain(|id| *id != attacker);
    }
    /// Each required attacker and the destinations maximizing its applicable attack requirements.
    pub fn required_attacks(&self) -> &BTreeMap<ObjectId, BTreeSet<crate::targets::TargetRef>> {
        &self.required
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
    defending_player: SeatId,
    single_blocker: bool,
    revision: u64,
    chooser: SeatId,
    blockers: Vec<ObjectId>,
    attackers: Vec<ObjectId>,
    restricted_targets: BTreeMap<ObjectId, Vec<ObjectId>>,
    block_minimums: BTreeMap<ObjectId, usize>,
    additional_blockers: BTreeMap<ObjectId, usize>,
    block_requirements: BTreeMap<ObjectId, usize>,
    blocked_attacker_requirements: BTreeMap<ObjectId, usize>,
    require_at_least_one_block: BTreeSet<ObjectId>,
    block_requirement_score: usize,
}
impl BlockersDecision {
    pub(crate) fn new(
        revision: u64,
        chooser: SeatId,
        attackers: Vec<ObjectId>,
        choices: BTreeMap<ObjectId, Vec<ObjectId>>,
        block_minimums: BTreeMap<ObjectId, usize>,
    ) -> Self {
        let blockers = choices.keys().copied().collect();
        let restricted_targets = choices
            .into_iter()
            .filter(|(_, targets)| targets != &attackers)
            .collect();
        Self {
            defending_player: chooser,
            revision,
            chooser,
            blockers,
            attackers,
            restricted_targets,
            block_minimums,
            additional_blockers: BTreeMap::new(),
            block_requirements: BTreeMap::new(),
            blocked_attacker_requirements: BTreeMap::new(),
            require_at_least_one_block: BTreeSet::new(),
            block_requirement_score: 0,
            single_blocker: false,
        }
    }
    /// Player whose creatures are declaring blocks. An effect can give this decision to another player.
    pub fn defending_player(&self) -> SeatId {
        self.defending_player
    }
    pub(crate) fn chosen_by(mut self, player: SeatId) -> Self {
        self.chooser = player;
        self
    }
    /// Whether this defending player may declare only one distinct blocker.
    pub fn single_blocker(&self) -> bool {
        self.single_blocker
    }
    pub(crate) fn with_single_blocker(mut self, single: bool) -> Self {
        self.single_blocker = single;
        self
    }
    pub(crate) fn with_additional_blockers(mut self, blockers: BTreeMap<ObjectId, usize>) -> Self {
        self.additional_blockers = blockers;
        self
    }
    pub(crate) fn with_block_requirements(
        mut self,
        requirements: &BTreeMap<ObjectId, usize>,
        blocked_attackers: &BTreeMap<ObjectId, usize>,
        require_at_least_one_block: BTreeSet<ObjectId>,
    ) -> Self {
        self.block_requirements = self
            .blockers
            .iter()
            .filter_map(|blocker| requirements.get(blocker).map(|score| (*blocker, *score)))
            .collect();
        self.blocked_attacker_requirements = self
            .attackers
            .iter()
            .filter_map(|attacker| {
                blocked_attackers
                    .get(attacker)
                    .map(|score| (*attacker, *score))
            })
            .collect();
        self.require_at_least_one_block = require_at_least_one_block;
        self.block_requirement_score = self.maximum_block_requirement_score();
        self
    }
    /// Maximum number of distinct attackers this blocker may be assigned to.
    pub fn maximum_blocks(&self, blocker: ObjectId) -> usize {
        1 + self
            .additional_blockers
            .get(&blocker)
            .copied()
            .unwrap_or_default()
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

    /// Attacking creatures this blocker can be assigned to in a legal declaration.
    pub fn legal_attackers(&self, blocker: ObjectId) -> &[ObjectId] {
        if self.blockers.binary_search(&blocker).is_err() {
            return &[];
        }
        self.restricted_targets
            .get(&blocker)
            .map_or(&self.attackers, Vec::as_slice)
    }

    /// Blocker rows that differ from the full attack list.
    pub fn restricted_blockers(&self) -> impl Iterator<Item = (ObjectId, &[ObjectId])> {
        self.restricted_targets
            .iter()
            .map(|(id, targets)| (*id, targets.as_slice()))
    }

    /// Offered attackers whose assignments require more than one blocker when blocked.
    pub fn group_minimums(&self) -> impl Iterator<Item = (ObjectId, usize)> + '_ {
        self.block_minimums
            .iter()
            .map(|(id, minimum)| (*id, *minimum))
    }

    pub(crate) fn validate(&self, selected: &[BlockDeclaration]) -> Result<(), CombatError> {
        let mut seen = BTreeSet::new();
        let mut block_counts = BTreeMap::new();
        let mut counts = BTreeMap::new();
        for row in selected {
            if !seen.insert((row.blocker, row.attacker)) {
                return Err(CombatError::DuplicateBlocker);
            }
            let count = block_counts.entry(row.blocker).or_insert(0usize);
            *count += 1;
            if *count > self.maximum_blocks(row.blocker) {
                return Err(CombatError::DuplicateBlocker);
            }
            if self.blockers.binary_search(&row.blocker).is_err() {
                return Err(CombatError::IllegalBlocker);
            }
            if !self.legal_attackers(row.blocker).contains(&row.attacker) {
                return Err(CombatError::IllegalBlockTarget);
            }
            *counts.entry(row.attacker).or_insert(0usize) += 1;
        }
        if self.single_blocker && block_counts.len() > 1 {
            return Err(CombatError::IllegalBlocker);
        }
        if self
            .block_minimums
            .iter()
            .any(|(target, min)| counts.get(target).is_some_and(|count| count < min))
        {
            return Err(CombatError::IllegalBlockTarget);
        }
        let satisfied = block_counts
            .keys()
            .map(|blocker| self.block_requirement(*blocker))
            .sum::<usize>()
            + selected
                .iter()
                .map(|row| self.blocked_attacker_requirement(row.attacker))
                .sum::<usize>()
            + counts
                .keys()
                .filter(|attacker| self.require_at_least_one_block.contains(attacker))
                .count();
        if satisfied < self.block_requirement_score {
            return Err(CombatError::IllegalBlocker);
        }
        Ok(())
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
    pub attack_payment: Option<Box<AttackPaymentWork>>,
    pub frame: CombatDecision,
    pub remaining_actors: Vec<SeatId>,
    pub assignments: Vec<CombatDamageAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttackPaymentWork {
    pub bands: Vec<Vec<ObjectId>>,
    pub selected: Vec<AttackDeclaration>,
    pub taps: Vec<AttackTap>,
    pub before: std::sync::Arc<crate::effects::EffectWindow>,
    pub tap_triggers: BTreeSet<crate::occurrence::TriggerInstanceId>,
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
    defending_planeswalker: Option<ObjectId>,
    block_status: BlockStatus,
}
impl AttackingCreature {
    pub fn combatant(&self) -> Combatant {
        self.combatant
    }
    pub fn defender(&self) -> SeatId {
        self.defender
    }
    /// The attacked planeswalker, if any; defender() remains its controller.
    pub fn defending_planeswalker(&self) -> Option<ObjectId> {
        self.defending_planeswalker
    }
    pub fn block_status(&self) -> BlockStatus {
        self.block_status
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingCreature {
    combatant: Combatant,
    attackers: BTreeSet<ObjectId>,
}
impl BlockingCreature {
    pub fn combatant(&self) -> Combatant {
        self.combatant
    }
    pub fn attacker(&self) -> Option<ObjectId> {
        self.attackers.iter().next().copied()
    }
    pub fn attackers(&self) -> &BTreeSet<ObjectId> {
        &self.attackers
    }
}

/// One executor-owned current combat. Historical attack presence and established
/// blocked status intentionally survive removal of their former counterparts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatState {
    attacking_bands: Vec<BTreeSet<ObjectId>>,
    pub(crate) one_blocker_for: BTreeSet<SeatId>,
    pub(crate) blocking_chooser: Option<SeatId>,
    attackers: BTreeMap<ObjectId, AttackingCreature>,
    blockers: BTreeMap<ObjectId, BlockingCreature>,
    attacked_player_count: usize,
    had_attackers: bool,
    blockers_started: bool,
    first_strike_participants: BTreeMap<ObjectId, Combatant>,
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
                        defending_planeswalker: None,
                        block_status: BlockStatus::NotDeclared,
                    },
                )
                .is_some()
            {
                return Err(CombatError::DuplicateAttacker);
            }
        }
        Ok(Self {
            one_blocker_for: BTreeSet::new(),
            blocking_chooser: None,
            had_attackers: !current.is_empty(),
            attacked_player_count: current
                .values()
                .map(AttackingCreature::defender)
                .collect::<BTreeSet<_>>()
                .len(),
            blockers_started: false,
            attacking_bands: Vec::new(),
            attackers: current,
            blockers: BTreeMap::new(),
            first_strike_participants: BTreeMap::new(),
        })
    }
    pub(crate) fn set_planeswalker_targets(
        &mut self,
        declarations: &[AttackDeclaration],
    ) -> Result<(), CombatError> {
        for declaration in declarations {
            self.attackers
                .get_mut(&declaration.attacker)
                .ok_or(CombatError::IllegalAttacker)?
                .defending_planeswalker = declaration.defending_planeswalker;
        }
        self.attacked_player_count = self
            .attackers
            .values()
            .filter(|attacker| attacker.defending_planeswalker.is_none())
            .map(AttackingCreature::defender)
            .collect::<BTreeSet<_>>()
            .len();
        Ok(())
    }
    /// Change only current combat membership. This never declares another attack.
    pub(crate) fn reselect_defender(
        &mut self,
        attacker: ObjectId,
        player: SeatId,
        walker: Option<ObjectId>,
    ) -> Result<(), CombatError> {
        let membership = self
            .attackers
            .get_mut(&attacker)
            .ok_or(CombatError::IllegalAttacker)?;
        membership.defender = player;
        membership.defending_planeswalker = walker;
        Ok(())
    }
    /// After blockers have been declared, a creature put into combat starts unblocked.
    pub(crate) fn enter_attacking(&mut self, combatant: Combatant, defender: SeatId) {
        self.attackers.insert(
            combatant.object(),
            AttackingCreature {
                combatant,
                defender,
                defending_planeswalker: None,
                block_status: if self.blockers_started {
                    BlockStatus::Unblocked
                } else {
                    BlockStatus::NotDeclared
                },
            },
        );
    }
    pub(crate) fn begin_blockers(&mut self) {
        self.blockers_started = true;
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
    /// Combatants with first or double strike when the first combat damage step began.
    pub fn first_strike_participants(&self) -> &BTreeMap<ObjectId, Combatant> {
        &self.first_strike_participants
    }
    pub(crate) fn begin_first_strike(&mut self, participants: BTreeMap<ObjectId, Combatant>) {
        self.first_strike_participants = participants;
    }

    /// Distinct players attacked during declarations, including those whose attackers subsequently left.
    pub fn attacked_player_count(&self) -> usize {
        self.attacked_player_count
    }
    pub fn had_attackers(&self) -> bool {
        self.had_attackers
    }

    pub(crate) fn declare_blocks(
        &mut self,
        defender: SeatId,
        blockers: Vec<(Combatant, ObjectId)>,
    ) -> Result<Vec<BlockDeclaration>, CombatError> {
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
            if !selected.insert((blocker.object(), attacker))
                || self.blockers.contains_key(&blocker.object())
            {
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
        let mut relationships = Vec::with_capacity(blockers.len());
        for (combatant, attacker) in self.expand_band_blocks(blockers) {
            relationships.push(BlockDeclaration {
                blocker: combatant.object(),
                attacker,
            });
            if let Some(target) = self.attackers.get_mut(&attacker) {
                target.block_status = BlockStatus::Blocked;
            }
            self.blockers
                .entry(combatant.object())
                .or_insert_with(|| BlockingCreature {
                    combatant,
                    attackers: BTreeSet::new(),
                })
                .attackers
                .insert(attacker);
        }
        Ok(relationships)
    }
    /// The executor supplies its canonical current creature/controller/epoch check.
    /// Opposite membership and historical status are not recomputed from link count.
    pub(crate) fn reconcile(
        &mut self,
        mut current: impl FnMut(Combatant, Option<(ObjectId, SeatId)>) -> bool,
    ) {
        self.attackers.retain(|_, a| {
            current(
                a.combatant,
                a.defending_planeswalker.map(|id| (id, a.defender)),
            )
        });
        self.attacking_bands.retain_mut(|band| {
            band.retain(|id| self.attackers.contains_key(id));
            !band.is_empty()
        });
        self.blockers.retain(|_, b| current(b.combatant, None));
        for blocker in self.blockers.values_mut() {
            blocker
                .attackers
                .retain(|id| self.attackers.contains_key(id));
        }
    }
}
